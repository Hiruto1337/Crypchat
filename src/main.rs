use std::{
    io::{BufRead, BufReader, Write, stdout},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, RwLock},
    thread,
};

use crossterm::{
    cursor::{MoveLeft, MoveTo},
    event::{Event, KeyCode, MouseEventKind},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};

struct Terminal {
    height: u16,
    width: u16,
    messages: Vec<Message>,
    input_buffer: Vec<char>,
    msg_offset: usize,
}

impl Terminal {
    fn new() -> Self {
        // Get terminal dimensions
        let Ok((width, height)) = crossterm::terminal::size() else {
            panic!("Couldn't read width and height of terminal!");
        };

        Terminal {
            height,
            width,
            messages: vec![],
            input_buffer: vec![],
            msg_offset: 0,
        }
    }

    fn get_input_position(&self) -> (u16, u16) {
        (
            (" Message: ".len() + self.input_buffer.len()) as u16,
            self.height - 1,
        )
    }

    fn draw(&self) {
        // Use [scroll_pos] to get relevant messages
        let input_height = 3;
        let output_height = self.height - input_height;

        let total_messages = self.messages.len();

        let messages = if output_height < total_messages as u16 {
            let lower_bound = total_messages - output_height as usize;
            let upper_bound = total_messages;
            let offset = self.msg_offset;
            &self.messages[lower_bound - offset..upper_bound - offset]
        } else {
            &self.messages.as_slice()
        };

        // Clear entire screen
        execute!(stdout(), Clear(ClearType::All)).unwrap();

        // Move cursor to (0,0)
        execute!(stdout(), MoveTo(0, 0)).unwrap();

        // Draw messages
        for (i, message) in messages.iter().enumerate() {
            execute!(stdout(), MoveTo(0, i as u16), Print(message)).unwrap();
        }

        // Draw "separation bar" between messages and input space
        let (_, y) = self.get_input_position();
        execute!(stdout(), MoveTo(0, y - 2)).unwrap();
        execute!(
            stdout(),
            Print((0..self.width).map(|_| '_').collect::<String>())
        )
        .unwrap();
        // Draw input area
        execute!(stdout(), MoveTo(0, y)).unwrap();
        execute!(
            stdout(),
            Print(format!(
                " Message: {}",
                self.input_buffer.iter().collect::<String>()
            ))
        )
        .unwrap();
    }
}

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

        write!(f, " {color}<{sender}>\x1b[0m{msg}")
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
    // Enter raw mode and take full control of scrolling behavior
    crossterm::terminal::enable_raw_mode().unwrap();
    execute!(stdout(), crossterm::terminal::EnterAlternateScreen).unwrap();

    // Connect to the server
    let stream = Arc::new(TcpStream::connect(addr).unwrap());
    let stream_read = stream.clone();
    let stream_write = stream.clone();
    let name_clone = name.clone();

    // Create the terminal representative
    let terminal = Arc::new(Mutex::new(Terminal::new()));
    let terminal_clone = terminal.clone();

    // Draw initial UI
    terminal.lock().unwrap().draw();

    // Create thread that prints incoming lines
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

                    terminal_clone.lock().unwrap().messages.push(message);

                    terminal_clone.lock().unwrap().draw();
                }
                Err(_) => println!("Error!"),
            }
        }
    });

    // Listen for events...
    loop {
        match crossterm::event::read() {
            Ok(Event::Key(key_event)) => match key_event.code {
                KeyCode::Char(c) => {
                    terminal.lock().unwrap().input_buffer.push(c);
                    execute!(stdout(), Print(c)).unwrap();
                }
                KeyCode::Backspace => {
                    if let Some(_) = terminal.lock().unwrap().input_buffer.pop() {
                        execute!(stdout(), MoveLeft(1), Print(" "), MoveLeft(1)).unwrap();
                    }
                }
                KeyCode::Enter => {
                    // Add a newline
                    terminal.lock().unwrap().input_buffer.push('\n');

                    // Convert input to string
                    let input_string: String =
                        terminal.lock().unwrap().input_buffer.iter().collect();

                    // Quit app if the input is "/quit"
                    if input_string == "/quit\n".to_string() {
                        crossterm::terminal::disable_raw_mode().unwrap();
                        execute!(stdout(), crossterm::terminal::LeaveAlternateScreen).unwrap();
                        break;
                    }

                    let message = format!("{name}: {input_string}");

                    // Write message to stream
                    write!(stream_write.as_ref(), "{message}",).unwrap();

                    // Clear input_buffer
                    terminal.lock().unwrap().input_buffer.clear();

                    terminal.lock().unwrap().draw();
                }
                KeyCode::Up => match terminal.lock() {
                    Ok(mut lock) => {
                        let output_height = lock.height as usize - 3;
                        if lock.msg_offset + output_height < lock.messages.len() {
                            lock.msg_offset += 1;
                            lock.draw();
                        }
                    }
                    _ => {}
                },
                KeyCode::Down => match terminal.lock() {
                    Ok(mut lock) => {
                        if lock.msg_offset != 0 {
                            lock.msg_offset -= 1;
                            lock.draw();
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
            Ok(Event::Resize(new_width, new_height)) => match terminal.lock() {
                Ok(mut lock) => {
                    lock.width = new_width;
                    lock.height = new_height;
                    lock.draw();
                }
                Err(_) => todo!(),
            },
            Ok(Event::Mouse(mouse_event)) => match mouse_event.kind {
                MouseEventKind::ScrollDown => match terminal.lock() {
                    Ok(mut lock) => {
                        if lock.msg_offset != 0 {
                            lock.msg_offset -= 1;
                            lock.draw();
                        }
                    }
                    _ => {}
                },
                MouseEventKind::ScrollUp => match terminal.lock() {
                    Ok(mut lock) => {
                        let output_height = lock.height as usize - 3;
                        if lock.msg_offset + output_height < lock.messages.len() {
                            lock.msg_offset += 1;
                            lock.draw();
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
    }
}

fn main() {
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
