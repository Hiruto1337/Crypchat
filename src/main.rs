use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, RwLock},
    thread,
};

fn start_server_tunnel(addr: String) {
    let clients: Arc<RwLock<Vec<(String, Arc<TcpStream>)>>> = Arc::new(RwLock::new(vec![]));
    let clients = clients.clone();

    // Start listening for connections
    let listener = TcpListener::bind(addr).unwrap();

    // Create a thread that handles new connections
    let connection_handler = thread::spawn(move || {
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
                                let _ = writeln!(client.1.as_ref(), "{msg}");
                                // if client.0 != soc_addr.to_string() {
                                //     let _ = writeln!(client.1.as_ref(), "{msg}");
                                // }
                            });
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    });

    let _ = connection_handler.join();
}

fn start_client(name: String, addr: String) {
    // Create thread that prints incoming lines
    let stream = Arc::new(TcpStream::connect(addr).unwrap());
    let stream_read = stream.clone();
    let stream_write = stream.clone();
    let name_clone = name.clone();

    thread::spawn(move || {
        let reader = BufReader::new(stream_read.as_ref());

        for line in reader.lines() {
            match line {
                Ok(msg) => {
                    let (sender, msg) = msg.split_once(':').unwrap();

                    if sender == name_clone {
                        print!("\x1b[1;32m");
                    } else {
                        print!("\x1b[1;31m");
                    };
                    print!("{sender}");
                    println!("\x1b[0m{msg}");
                }
                Err(_) => println!("Error!"),
            }
        }
    });

    // Create stdin reader that sends user input to server
    let stdin = std::io::stdin();

    for line in stdin.lock().lines() {
        match line {
            Ok(msg) => {
                print!("\x1b[1A"); // Move cursor up by 1
                print!("\x1b[1K"); // Delete from beginning of line to position
                print!("\x1b[1G"); // Place cursor at column 1
                let _ = writeln!(stream_write.as_ref(), "{name}: {msg}");
            }
            Err(_) => println!("Error!"),
        }
    }
}

fn main() {
    print!("\x1b[2J"); // Clear terminal
    print!("\x1b[H"); // Move cursor to start
    std::io::stdout().flush().unwrap();
    let args: Vec<String> = std::env::args().collect();

    match (
        args.get(1).map(|string| string.as_str()),
        args.get(2),
        args.get(3),
    ) {
        (Some("server"), Some(addr), None) => {
            start_server_tunnel(addr.clone());
        }
        (Some("client"), Some(name), Some(addr)) => {
            start_client(name.clone(), addr.clone());
        }
        _ => {
            println!(
                "Error: Arguments must be either \"server [address]\" or \"client [name] [address]\""
            );
            return;
        }
    }
}
