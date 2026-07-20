pub mod crypto;
pub mod misc;

use std::{
    io::{BufRead, BufReader, Write, stdout},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, RwLock},
    thread,
};


use crossterm::{
    event::Event,
    execute,
};

use crate::{misc::terminal::Terminal};

fn start_server_tunnel(addr: String) {
    let clients: Arc<RwLock<Vec<(String, Arc<TcpStream>)>>> = Arc::new(RwLock::new(vec![]));

    // Start listening for connections
    let listener = TcpListener::bind(addr).unwrap();

    // Create a loop that handles new connections
    loop {
        let Ok((tcp_stream, soc_addr)) = listener.accept() else {
            println!("Connection attempt failed...");
            continue;
        };

        println!("{soc_addr} connected!");

        // Wrap the stream in an Arc<>
        let stream = Arc::new(tcp_stream);

        // Add stream to server clients
        clients
            .write()
            .unwrap()
            .push((soc_addr.to_string(), stream.clone()));

        let clients = clients.clone();

        // Create a thread that listens for input from stream and broadcasts it to all clients
        thread::spawn(move || {
            let reader = BufReader::new(stream.as_ref());

            for line in reader.lines() {
                match line {
                    Ok(msg) => {
                        println!("{msg}");
                        clients.write().unwrap().iter().for_each(|client| {
                            writeln!(client.1.as_ref(), "{msg}").unwrap();
                        });
                    }
                    Err(_) => break,
                }
            }
        });
    }
}

fn start_client(addr: String, name: String) {
    // Enter raw mode and take full control of scrolling behavior
    crossterm::terminal::enable_raw_mode().unwrap();
    execute!(stdout(), crossterm::terminal::EnterAlternateScreen).unwrap();

    // Connect to the server
    let stream = Arc::new(TcpStream::connect(addr).unwrap());

    // Create the terminal representative
    let terminal = Arc::new(Mutex::new(Terminal::from((name, stream))));

    // Draw initial UI
    terminal.lock().unwrap().draw();

    // Create thread that reacts to incoming data
    let terminal_clone = terminal.clone();
    thread::spawn(move || {
        let read_stream = terminal_clone.lock().unwrap().stream.clone();
        let reader = BufReader::new(read_stream.as_ref());

        for line in reader.lines() {
            let Ok(incoming) = line else {
                continue;
            };

            let mut lock = terminal_clone.lock().unwrap();

            // Save message (if cipher exists)
            if lock.cipher.is_some() {
                lock.save_message(incoming);
                continue;
            }

            // If incoming EC point is not my own
            if incoming != lock.ec_point.to_string() {
                // Create cipher and reciprocate my own EC point
                lock.create_cipher(incoming);
                lock.send_ec_point();
            }
        }
    });

    // Announce elliptic curve point to server
    terminal.lock().unwrap().send_ec_point();

    // Listen for events...
    loop {
        let event = crossterm::event::read();
        let mut lock = terminal.lock().unwrap();
        match event {
            Ok(Event::Key(key_event)) => lock.handle_key_event(key_event),
            Ok(Event::Resize(new_width, new_height)) => lock.handle_resize(new_width, new_height),
            Ok(Event::Mouse(mouse_event)) => lock.handle_mouse_event(mouse_event),
            _ => {}
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);

    match (args.next(), args.next()) {
        (Some(addr), None) => {
            start_server_tunnel(addr);
        }
        (Some(addr), Some(name)) => {
            start_client(addr, name);
        }
        _ => {
            println!("Error: Arguments must be \"[address] [name?]\"");
        }
    }
}
