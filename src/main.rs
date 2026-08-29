pub mod crypto;
pub mod misc;

use std::{
    io::stdout,
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering::Relaxed}}, thread::{JoinHandle, sleep}, time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{cursor::{EnableBlinking, Hide, MoveLeft, MoveToColumn, Show}, event::Event, execute, style::Print};
use iroh::{Endpoint, EndpointId, endpoint::presets};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::misc::terminal::Terminal;

const ALPN: &[u8] = b"crypchat";

#[tokio::main]
async fn main() -> Result<()> {
    // Get display name
    let name = get_input("Display name: ");
    
    // Go online and display peer ID
    execute!(stdout(), MoveToColumn(0), Print("My peer ID: "), Hide).unwrap();

    //// Connect to the p2p network
    let ep: Endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;

    //// Wait until user is connected
    let (animation, kill) = loading_animation();
    ep.online().await;

    //// Kill animation
    kill.store(true, Relaxed);
    animation.join().unwrap();

    //// Show peer ID
    execute!(stdout(), Print(ep.id().to_string() + "\n")).unwrap();

    // Get other peer ID
    let peer_id: EndpointId = get_input("Other peer ID: ").parse().unwrap();

    // Establish connection
    execute!(stdout(), Print("Connecting")).unwrap();

    let (animation, kill) = loading_animation();

    // Decide caller and receiver based on peer ID
    let smallest_id = ep.id().to_string() < peer_id.to_string();

    let (conn, mut write_stream, read_stream) = loop {
        let conn_ = if smallest_id {
            ep.accept().await.context("Accept failed")?.await?
        } else {
            ep.connect(peer_id, ALPN).await?
        };

        // If the connection isn't from the expected peer, abort
        if conn_.remote_id() != peer_id {
            conn_.close(1u8.into(), b"Unauthorized");
            continue;
        }

        let (write_stream_, read_stream_) = if smallest_id {
            conn_.accept_bi().await?
        } else {
            conn_.open_bi().await?
        };

        break (conn_, write_stream_, read_stream_);
    };

    // Kill loading animation
    kill.store(true, Relaxed);
    animation.join().unwrap();

    // Enter raw mode and take full control of scrolling behavior
    crossterm::terminal::enable_raw_mode().unwrap();
    execute!(
        stdout(),
        crossterm::terminal::EnterAlternateScreen
    )
    .unwrap();

    // Create the terminal representative
    let terminal = Arc::new(Mutex::new(Terminal::from(name)));

    execute!(stdout(), Show, EnableBlinking).unwrap();

    // Draw initial UI
    terminal.lock().unwrap().draw();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1024);
    terminal.lock().unwrap().tx = Some(tx.clone());

    // Use a single centralized thread to send messages
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if message == "/status" {
                for path in &conn.paths() {
                    if path.is_selected() == false {
                        continue;
                    }

                    if path.is_ip() {
                        execute!(stdout(), Print("👤⇄👤")).unwrap();
                        break;
                    } else if path.is_relay() {
                        execute!(stdout(), Print("👤⇄📻⇄👤")).unwrap();
                        break;
                    }
                }
                continue;
            }
            write_stream.write_all(message.as_bytes()).await.unwrap();
        }
    });

    // Create thread that reacts to incoming data
    let terminal_clone = terminal.clone();
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        let reader = BufReader::new(read_stream);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let response = {
                let mut lock = terminal_clone.lock().unwrap();

                if lock.cipher.is_some() {
                    // Save message
                    lock.save_message(line);
                    None
                } else if line != lock.ec_point.to_string() {
                    // If incoming EC point is not my own
                    // Create cipher and reciprocate my own EC point
                    lock.create_cipher(line);
                    Some(lock.ec_point.to_string() + "\n")
                } else {
                    None
                }
            };

            if let Some(ec_point) = response {
                tx_clone.send(ec_point).await.unwrap();
            }
        }
    });

    // Announce elliptic curve point to peer
    let ec_point = terminal.lock().unwrap().ec_point.to_string() + "\n";
    tx.send(ec_point).await.unwrap();

    // Listen for events...
    loop {
        let event = crossterm::event::read();
        let mut lock = terminal.lock().unwrap();
        match event {
            Ok(Event::Key(key_event)) => lock.handle_key_event(key_event).await?,
            Ok(Event::Resize(new_width, new_height)) => lock.handle_resize(new_width, new_height),
            Ok(Event::Mouse(mouse_event)) => lock.handle_mouse_event(mouse_event),
            _ => {}
        }
    }
}

fn get_input(prompt: &str) -> String {
    execute!(stdout(), Print(format!("{}", prompt))).unwrap();

    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer).unwrap();
    buffer.pop().unwrap();
    buffer
}

fn loading_animation() -> (JoinHandle<()>, Arc<AtomicBool>){
    let kill = Arc::new(AtomicBool::new(false));
    let killer = kill.clone();

    let animation = std::thread::spawn(move || {
        let mut animation = "⠋⠙⠸⢰⣠⣄⡆⠇".chars().cycle();

        while kill.load(Relaxed) == false {
            execute!(stdout(), Print(animation.next().unwrap()), MoveLeft(1)).unwrap();
            sleep(Duration::from_millis(125));
        }
    });

    (animation, killer)
}
