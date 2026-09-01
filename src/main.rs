pub mod crypto;
pub mod misc;

use std::{
    io::stdout,
    sync::{Arc, Mutex}
};

use anyhow::{Context, Result};
use crossterm::{cursor::EnableBlinking, event::Event, execute, style::Print};
use iroh::{Endpoint, EndpointId, endpoint::presets};
use tokio::sync::mpsc::channel;

use crate::misc::{terminal::Terminal, threads::{spawn_reader, spawn_writer, LoadingAnimation}};

macro_rules! load {
    ($($code:tt)*) => {
        let load_animation = LoadingAnimation::new();
        $($code)*
        load_animation.kill();
    };
}

const ALPN: &[u8] = b"crypchat";

fn get_input(prompt: &str) -> String {
    execute!(stdout(), Print(format!("{}", prompt))).unwrap();

    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer).unwrap();
    buffer.pop().unwrap();
    buffer
}

#[tokio::main]
async fn main() -> Result<()> {
    // Get display name
    let name = get_input("Display name: ");
    
    // Go online and display peer ID
    execute!(stdout(), Print("My peer ID: ")).unwrap();

    load!{
        let ep: Endpoint = Endpoint::builder(presets::N0)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await?;

        ep.online().await;
    };

    execute!(stdout(), Print(ep.id().to_string() + "\n")).unwrap();

    // Get other peer ID
    let peer_id: EndpointId = get_input("Other peer ID: ").parse().unwrap();

    // Establish connection
    execute!(stdout(), Print("Connecting ")).unwrap();

    load!{
        // Decide caller and receiver based on peer ID
        let smallest_id = ep.id().to_string() < peer_id.to_string();

        let (conn, write_stream, read_stream) = loop {
            let conn = if smallest_id {
                ep.accept().await.context("Accept failed")?.await?
            } else {
                ep.connect(peer_id, ALPN).await?
            };

            // If the connection isn't from the expected peer, abort
            if conn.remote_id() != peer_id {
                conn.close(1u8.into(), b"Unauthorized");
                continue;
            }

            let (write_stream, read_stream) = if smallest_id {
                conn.accept_bi().await?
            } else {
                conn.open_bi().await?
            };

            break (conn, write_stream, read_stream);
        };
    };

    // Enter raw mode and take full control of scrolling behavior
    crossterm::terminal::enable_raw_mode().unwrap();
    execute!(
        stdout(),
        crossterm::terminal::EnterAlternateScreen,
        EnableBlinking
    )
    .unwrap();

    // Create the terminal representative
    let (tx, rx) = channel::<String>(1024);
    let terminal = Arc::new(Mutex::new(Terminal::from((name, tx.clone()))));

    // Draw initial UI
    terminal.lock().unwrap().draw();

    // Use a single centralized thread to send messages
    spawn_writer(conn, write_stream, rx);

    // Create thread that reacts to incoming data
    spawn_reader(read_stream, terminal.clone(), tx.clone());

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
