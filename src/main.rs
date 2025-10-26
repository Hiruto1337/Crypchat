use std::{
    io::{stdout, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, RwLock},
    thread, time::Duration,
};

struct Message {
    sender: String,
    msg: String,
    from_self: bool,
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Message {
            sender,
            msg,
            from_self,
        } = self;

        let color = if *from_self {
            "\x1b[1;32m" // Set color to green
        } else {
            "\x1b[1;31m" // Set color to red
        };

        write!(f, "{color}<{sender}>\x1b[0m{msg}")
    }
}

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
    let neutralize_cursor = || {
        print!("\x1b[43;1H");
        print!("Message: ");
        stdout().flush().unwrap();
    };

    neutralize_cursor();

    let mut messages: Vec<Message> = vec![];
    // Create thread that prints incoming lines
    let stream = Arc::new(TcpStream::connect(addr).unwrap());
    let stream_read = stream.clone();
    let stream_write = stream.clone();
    let name_clone = name.clone();
    let mut message_position = 1;

    thread::spawn(move || {
        let reader = BufReader::new(stream_read.as_ref());

        for line in reader.lines() {
            match line {
                Ok(msg) => {
                    // Save incoming message
                    let (sender, msg) = msg.split_once(':').unwrap();

                    let message = Message {
                        sender: sender.to_string(),
                        msg: msg.to_string(),
                        from_self: sender == name_clone.as_str(),
                    };

                    messages.push(message);

                    // Save current cursor position
                    print!("\x1b[s");
                    // Move cursor up to line position
                    print!("\x1b[{message_position};1H");
                    // Print the message
                    print!(" {}", messages.last().unwrap());
                    // Return to saved cursor position
                    print!("\x1b[u");
                    stdout().flush().unwrap();
                    message_position += 1;
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
                neutralize_cursor();
                print!("\x1b[0J"); // Delete from cursor to end of screen
                std::io::stdout().flush().unwrap();
                if msg.len() != 0 {
                    let _ = writeln!(stream_write.as_ref(), "{name}: {msg}");
                }
            }
            Err(_) => println!("Error!"),
        }
    }
}

fn main() {
    // Clear terminal
    std::process::Command::new("clear").spawn().unwrap();
    std::thread::sleep(Duration::from_millis(50));

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
