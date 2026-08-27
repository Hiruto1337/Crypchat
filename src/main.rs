pub mod crypto;
pub mod misc;

use std::{
    io::stdout,
    sync::{Arc, Mutex}, thread::sleep, time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{cursor::{EnableBlinking, MoveLeft, MoveRight, MoveToColumn}, event::Event, execute, style::Print};
use iroh::{Endpoint, EndpointId, endpoint::presets};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::misc::terminal::Terminal;

const ALPN: &[u8] = b"crypchat";

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    match (args.next(), args.next()) {
        (Some(name), None) => start_client(name, None).await?,
        (Some(name), Some(peer_id)) => {
            let peer_endpoint: EndpointId = peer_id.parse()?;
            start_client(name, Some(peer_endpoint)).await?
        },
        _ => {
            println!("Error: Must provide arguments \"[name] [peer_id?]\"");
            return Ok(());
        }
    }

    Ok(())
}

async fn start_client(name: String, peer_id: Option<EndpointId>) -> Result<()> {
    // Enter raw mode and take full control of scrolling behavior
    crossterm::terminal::enable_raw_mode().unwrap();
    execute!(
        stdout(),
        crossterm::terminal::EnterAlternateScreen,
        EnableBlinking
    )
    .unwrap();

    // Create the terminal representative
    let terminal = Arc::new(Mutex::new(Terminal::from(name)));

    // Draw initial UI⠤
    terminal.lock().unwrap().draw();

    execute!(stdout(), MoveToColumn(0), Print("Connection: ")).unwrap();

    // Connect to the p2p network
    let ep: Endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;

    // Wait until user is connected
    {
        let animation_read = Arc::new(Mutex::new(true));
        let animation_write = animation_read.clone();

        let loading_animation = std::thread::spawn(move || {
            let mut animation = "⠋⠙⠸⢰⣠⣄⡆⠇".chars().cycle();
            execute!(stdout(), MoveRight(1)).unwrap();

            loop {
                if *animation_read.lock().unwrap() == false {
                    break;
                }
                execute!(stdout(), MoveLeft(1), Print(animation.next().unwrap())).unwrap();
                sleep(Duration::from_millis(125));
            }
        });

        ep.online().await;
        *animation_write.lock().unwrap() = false;
        loading_animation.join().unwrap();
    }

    execute!(stdout(), MoveLeft(1), Print(ep.id().to_string())).unwrap();

    let (conn, mut write_stream, read_stream) = if let Some(peer_id) = peer_id {
        let conn = ep.connect(peer_id, ALPN).await?;
        let (write_stream, read_stream) = conn.open_bi().await?;
        (conn, write_stream, read_stream)
    } else {
        let conn = ep.accept().await.context("Accept failed")?.await?;
        let (write_stream, read_stream) = conn.accept_bi().await?;
        (conn, write_stream, read_stream)
    };

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

    // TODO: Only accept peer with appropriate ID

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
