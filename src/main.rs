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
    event::{Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};

use crate::crypto::{
    aes_cbc,
    diffie_hellman::{Point, U576, get_elliptic_curve, get_generator_point, get_random_uint},
};

struct Terminal {
    name: String,
    stream: Arc<TcpStream>,
    height: u16,
    width: u16,
    messages: Vec<Message>,
    input_buffer: String,
    msg_offset: usize,
    cipher: Option<Aes128>,
    secret_number: U576,
    ec_point: Point,
}

impl Terminal {
    fn new(name: String, stream: Arc<TcpStream>) -> Self {
        // Get terminal dimensions
        let Ok((width, height)) = crossterm::terminal::size() else {
            panic!("Couldn't read width and height of terminal!");
        };

        let generator = get_generator_point();
        let secret_number = get_random_uint();
        let ec_point = get_elliptic_curve().get_point_from(generator, secret_number);

        Terminal {
            name,
            stream,
            height,
            width,
            messages: vec![],
            input_buffer: String::new(),
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
        let mut offset = 0;

        for message in messages.iter() {
            if self.height - input_height <= offset {
                break;
            }

            let msg_height = message.get_len() / (self.width + 1) + 1;

            offset += msg_height;

            execute!(stdout(), Print(message), MoveTo(0, offset)).unwrap();
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
                &self.input_buffer
            ))
        )
        .unwrap();
    }

    fn send_ec_point(&mut self) {
        let ec_point = self.ec_point.to_string();
        write!(self.stream.as_ref(), "{ec_point}\n").unwrap();
    }

    fn create_cipher(&mut self, ec_point_string: String) {
        let (x, y) = ec_point_string.split_once(';').unwrap();

        let received_ec_point = Point::from((x, y));
        let secret_shared_point = get_elliptic_curve()
            .get_point_from(received_ec_point, self.secret_number);
        let x = secret_shared_point.get_x();
        
        let mut key: [u8; 16] = [0; 16];
        key.copy_from_slice(&sha256::digest(x.to_string()).as_bytes()[0..16]);

        let array = aes::cipher::Array::from(key);
        self.cipher = Some(Aes128::new(&array));
    }

    fn send_message(&mut self) {
        let trimmed = self.input_buffer.trim().to_string();

        if trimmed == "" {
            return;
        }

        if let Some(cipher) = &self.cipher {
            let bytes: Vec<u8> = trimmed.into_bytes();

            let encrypted = aes_cbc::encrypt(&bytes, cipher);

            let encoded = base64::engine::general_purpose::STANDARD.encode(encrypted);

            let message = format!("{}:{}\n", &self.name, encoded);

            // Write message to stream
            write!(self.stream.as_ref(), "{message}",).unwrap();

            // Clear input_buffer
            self.input_buffer.clear();

            self.draw();
        }
    }

    fn save_message(&mut self, message: String) {
        let Some((sender, msg)) = message.split_once(':') else {
            return;
        };

        let decoded: Vec<u8> = base64::engine::general_purpose::STANDARD
            .decode(msg)
            .unwrap();

        let cipher = self.cipher.as_ref().unwrap();

        let clean_decrypted_vec = aes_cbc::decrypt(&decoded, cipher)
            .into_iter()
            .filter(|v| *v != b'\0')
            .collect();

        let decrypted = String::from_utf8(clean_decrypted_vec).unwrap();

        let message = Message {
            sender: sender.to_string(),
            msg: decrypted,
            from_self: sender == &self.name,
        };

        self.messages.push(message);
        self.draw();
    }

    fn scroll_up(&mut self) {
        let output_height = self.height as usize - 3;
        if self.msg_offset + output_height < self.messages.len() {
            self.msg_offset += 1;
            self.draw();
        }
    }

    fn scroll_down(&mut self) {
        if self.msg_offset != 0 {
            self.msg_offset -= 1;
            self.draw();
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        // Ignore key releases
        if key_event.is_release() {
            return;
        }

        match key_event.code {
            KeyCode::Esc => {
                crossterm::terminal::disable_raw_mode().unwrap();
                execute!(stdout(), crossterm::terminal::LeaveAlternateScreen).unwrap();
                return;
            }
            KeyCode::Char(c) => {
                // Add char to input buffer
                self.input_buffer.push(c);
                execute!(stdout(), Print(c)).unwrap();
            }
            KeyCode::Backspace => {
                // Remove char from input buffer + Clear char in input
                if let Some(_) = self.input_buffer.pop() {
                    execute!(stdout(), MoveLeft(1), Print(" "), MoveLeft(1)).unwrap();
                }
            }
            KeyCode::Enter => {
                // Send message to server
                self.send_message();
            }
            KeyCode::Up => self.scroll_up(),
            KeyCode::Down => self.scroll_down(),
            _ => {}
        }
    }

    fn handle_resize(&mut self, new_width: u16, new_height: u16) {
        self.width = new_width;
        self.height = new_height;
        self.draw();
    }

    fn handle_mouse_event(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::ScrollDown => self.scroll_down(),
            MouseEventKind::ScrollUp => self.scroll_up(),
            _ => {}
        }
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

impl Message {
    fn get_len(&self) -> u16 {
        // 4 comes from ' ', '<', '>' and ' '
        4 + self.sender.chars().count() as u16 + self.msg.chars().count() as u16
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

    // Create the terminal representative
    let terminal = Arc::new(Mutex::new(Terminal::new(name, stream)));

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

            // Ignore my own EC point
            if incoming == lock.ec_point.to_string() {
                continue;
            }

            // Create cipher and reciprocate my own EC point
            lock.create_cipher(incoming);
            lock.send_ec_point();
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
            start_client(name, addr);
        }
        _ => {
            println!("Error: Arguments must be \"[address] [name?]\"");
        }
    }
}
