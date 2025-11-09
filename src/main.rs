use std::{
    io::{BufRead, BufReader, LineWriter, Write, stdout},
    net::{TcpListener, TcpStream},
    sync::{Arc, RwLock, mpsc},
    thread,
};

use crossterm::{
    cursor::{MoveLeft, MoveTo},
    event::{Event as TerminalEvent, KeyCode, MouseEventKind},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};

struct Terminal {
    height: u16,
    width: u16,
    messages: Vec<Message>,
    input_buffer: String,
    msg_offset: usize,
}

enum Event {
    TerminalEvent(TerminalEvent),
    MessageEvent(Message),
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
            input_buffer: String::new(),
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
        execute!(stdout(), Print(format!(" Message: {}", &self.input_buffer)))
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
    let clients: Arc<RwLock<Vec<(String, Arc<TcpStream>)>>> =
        Arc::new(RwLock::new(vec![]));
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
                            clients.write().unwrap().iter().for_each(
                                |client| {
                                    let _ =
                                        writeln!(client.1.as_ref(), "{msg}");
                                    // if client.0 != soc_addr.to_string() {
                                    //     let _ = writeln!(client.1.as_ref(), "{msg}");
                                    // }
                                },
                            );
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
    // crossterm::terminal::enable_raw_mode().unwrap();
    // execute!(stdout(), crossterm::terminal::EnterAlternateScreen).unwrap();

    // Connect to the server
    let s = TcpStream::connect(addr).unwrap();
    let t = s.try_clone().unwrap();

    let stream_read = BufReader::new(s);
    let mut stream_write = LineWriter::new(t);

    let name_clone = name.clone();

    // Create the terminal representative
    let mut terminal = Terminal::new();

    let (event_tx, event_rx) = mpsc::channel::<Event>();

    // Draw initial UI
    terminal.draw();
    //Consumer of messages
    let event_consumer = thread::spawn(move || {
        loop {
            match event_rx.recv() {
                Ok(event) => match event {
                    Event::TerminalEvent(ev) => match ev {
                        TerminalEvent::Key(key_event) => match key_event.code {
                            KeyCode::Char(c) => {
                                terminal.input_buffer.push(c);
                                execute!(stdout(), Print(c)).unwrap();
                            }
                            KeyCode::Backspace => {
                                if let Some(_) = terminal.input_buffer.pop() {
                                    execute!(
                                        stdout(),
                                        MoveLeft(1),
                                        Print(" "),
                                        MoveLeft(1)
                                    )
                                    .unwrap();
                                }
                            }
                            KeyCode::Enter => {
                                // Add a newline
                                terminal.input_buffer.push('\n');

                                // Convert input to string
                                let input_string: &str = &terminal.input_buffer;

                                // Quit app if the input is "/quit"
                                if input_string == "/quit\n" {
                                    crossterm::terminal::disable_raw_mode()
                                        .unwrap();
                                    execute!(
                                    stdout(),
                                    crossterm::terminal::LeaveAlternateScreen
                                )
                                    .unwrap();
                                    break;
                                }

                                let message = format!("{name}: {input_string}");

                                // Write message to stream
                                write!(stream_write, "{message}",).unwrap();

                                // Clear input_buffer
                                terminal.input_buffer.clear();

                                terminal.draw();
                            }
                            KeyCode::Up => {
                                let output_height =
                                    terminal.height as usize - 3;
                                if terminal.msg_offset + output_height
                                    < terminal.messages.len()
                                {
                                    terminal.msg_offset += 1;
                                    terminal.draw();
                                }
                            }
                            KeyCode::Down => {
                                if terminal.msg_offset != 0 {
                                    terminal.msg_offset -= 1;
                                    terminal.draw();
                                }
                            }
                            _ => {}
                        },
                        TerminalEvent::Resize(new_width, new_height) => {
                            terminal.width = new_width;
                            terminal.height = new_height;
                            terminal.draw();
                        }
                        TerminalEvent::Mouse(mouse_event) => {
                            match mouse_event.kind {
                                MouseEventKind::ScrollDown => {
                                    if terminal.msg_offset != 0 {
                                        terminal.msg_offset -= 1;
                                        terminal.draw();
                                    }
                                }
                                MouseEventKind::ScrollUp => {
                                    let output_height =
                                        terminal.height as usize - 3;
                                    if terminal.msg_offset + output_height
                                        < terminal.messages.len()
                                    {
                                        terminal.msg_offset += 1;
                                        terminal.draw();
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    },
                    Event::MessageEvent(message) => {
                        terminal.messages.push(message);
                        terminal.draw();
                    }
                },
                Err(err) => {
                    println!("err: {}", err);
                }
            }
        }
    });
    // Producer for MessageEvents
    let message_producer = {
        let event_tx = event_tx.clone();

        thread::spawn(move || {
            for line in stream_read.lines() {
                match line {
                    Ok(msg) => {
                        execute!(stdout(), Print("haha")).unwrap();
                        // Save incoming message
                        let (sender, msg) = msg.split_once(':').unwrap();

                        let message = Message {
                            sender: sender.to_string(),
                            msg: msg.to_string(),
                            from_self: sender == name_clone.as_str(),
                        };
                        let _ = event_tx.send(Event::MessageEvent(message));
                    }
                    Err(_) => println!("Error!"),
                }
            }
        })
    };
    //Producer for key board events
    let terminal_producer = {
        let event_tx = event_tx.clone();
        thread::spawn(move || {
            loop {
                match crossterm::event::read() {
                    Ok(ev) => {
                        let _ = event_tx.send(Event::TerminalEvent(ev));
                    }
                    Err(_) => {}
                }
            }
        })
    };
    event_consumer.join().unwrap();
    message_producer.join().unwrap();

    terminal_producer.join().unwrap();
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
