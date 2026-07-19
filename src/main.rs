pub mod crypto;

use std::{
    io::{BufRead, BufReader, Write, stdout},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, RwLock},
    thread,
};

use aes::{Aes128, cipher::KeyInit};
use base64::Engine;
use crossterm::{
    cursor::{MoveLeft, MoveTo},
    event::{Event, KeyCode, MouseEventKind},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};

use crate::crypto::{
    aes_cbc,
    diffie_hellman::{Point, U576, get_elliptic_curve, get_generator_point, get_random_uint},
};

struct Terminal {
    height: u16,
    width: u16,
    messages: Vec<Message>,
    input_buffer: Vec<char>,
    msg_offset: usize,
    cipher: Option<Aes128>,
    secret_number: U576,
    ec_point: Point,
}

impl Terminal {
    fn new() -> Self {
        // Get terminal dimensions
        let Ok((width, height)) = crossterm::terminal::size() else {
            panic!("Couldn't read width and height of terminal!");
        };

        let generator = get_generator_point();
        let secret_number = get_random_uint();
        let ec_point = get_elliptic_curve().get_point_from(generator, secret_number);

        Terminal {
            height,
            width,
            messages: vec![],
            input_buffer: vec![],
            msg_offset: 0,
            cipher: None,
            secret_number,
            ec_point,
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

        write!(f, " {color}<{sender}> \x1b[0m{msg}")
    }
}

fn start_server_tunnel(addr: String) {
    let clients: Arc<RwLock<Vec<(String, Arc<TcpStream>)>>> = Arc::new(RwLock::new(vec![]));
    let clients = clients.clone();

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

fn start_client(name: String, addr: String) {
    // Enter raw mode and take full control of scrolling behavior
    crossterm::terminal::enable_raw_mode().unwrap();
    execute!(stdout(), crossterm::terminal::EnterAlternateScreen).unwrap();

    // Connect to the server
    let stream = Arc::new(TcpStream::connect(addr).unwrap());
    let stream_read = stream.clone();
    let stream_write1 = stream.clone();
    let stream_write2 = stream.clone();
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
                Ok(incoming) => {
                    let mut lock = terminal_clone.lock().unwrap();

                    if let Some(cipher) = &lock.cipher {
                        // Save incoming message
                        if let Some((sender, msg)) = incoming.split_once(':') {
                            let decoded: Vec<u8> = base64::engine::general_purpose::STANDARD
                                .decode(msg)
                                .unwrap();

                            let decrypted = String::from_utf8(aes_cbc::decrypt(&decoded, cipher)).unwrap();

                            let message = Message {
                                sender: sender.to_string(),
                                msg: decrypted,
                                from_self: sender == name_clone.as_str(),
                            };

                            lock.messages.push(message);
                            lock.draw();
                        }
                    } else if incoming != lock.ec_point.to_string() {
                        let (x, y) = incoming.split_once(';').unwrap();

                        let received_ec_point = Point::from((x, y));
                        let secret_shared_point = get_elliptic_curve()
                            .get_point_from(received_ec_point, lock.secret_number);
                        let x = secret_shared_point.get_x();
                        
                        let mut key: [u8; 16] = [0; 16];
                        key.copy_from_slice(&sha256::digest(x.to_string()).as_bytes()[0..16]);

                        let array = aes::cipher::Array::from(key);
                        lock.cipher = Some(Aes128::new(&array));

                        let ec_point_string = lock.ec_point.to_string();

                        write!(stream_write1.as_ref(), "{ec_point_string}\n").unwrap();
                    }
                }
                Err(_) => println!("Error!"),
            }
        }
    });

    // Announce elliptic curve point to server
    let ec_point = terminal.lock().unwrap().ec_point.to_string();
    write!(stream_write2.as_ref(), "{ec_point}\n").unwrap();

    // Listen for events...
    loop {
        match crossterm::event::read() {
            Ok(Event::Key(key_event)) => {
                if key_event.is_release() {
                    continue;
                }

                match key_event.code {
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
                        // Get mutable mutex lock on terminal
                        let mut lock = terminal.lock().unwrap();

                        // Convert input to string
                        let input_string: String = lock.input_buffer.iter().collect();

                        if input_string == "" {
                            continue;
                        }

                        // Quit app if the input is "/quit"
                        if input_string == "/quit" {
                            crossterm::terminal::disable_raw_mode().unwrap();
                            execute!(stdout(), crossterm::terminal::LeaveAlternateScreen).unwrap();
                            break;
                        }

                        if let Some(cipher) = &lock.cipher {
                            let bytes: Vec<u8> =
                                (input_string + "\n").into_bytes();

                            let encrypted = aes_cbc::encrypt(&bytes, cipher);

                            let encoded = base64::engine::general_purpose::STANDARD.encode(encrypted);

                            let message = format!("{name}:{encoded}\n");

                            // Write message to stream
                            write!(stream_write2.as_ref(), "{message}",).unwrap();

                            // Clear input_buffer
                            lock.input_buffer.clear();

                            lock.draw();
                        }
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
                }
            }
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
    let mut args = std::env::args().skip(1);

    match (args.next(), args.next()) {
        (Some(addr), None) => {
            start_server_tunnel(addr.clone());
        }
        (Some(addr), Some(name)) => {
            start_client(name.clone(), addr.clone());
        }
        _ => {
            println!("Error: Arguments must be \"[address] [name?]\"");
        }
    }
}
