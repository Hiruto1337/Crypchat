use std::{io::{Write, stdout}, net::TcpStream, sync::Arc};

use aes::{Aes128, cipher::KeyInit};
use base64::Engine;
use crossterm::{cursor::MoveTo, event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind}, execute, style::Print, terminal::{Clear, ClearType}};

use crate::{crypto::{aes_cbc, diffie_hellman::*}, misc::message::Message};

pub struct Terminal {
    pub name: String,
    pub stream: Arc<TcpStream>,
    pub height: u16,
    pub width: u16,
    pub messages: Vec<Message>,
    pub input_buffer: String,
    pub msg_offset: usize,
    pub cipher: Option<Aes128>,
    pub secret_number: U576,
    pub ec_point: Point,
}

impl From<(String, Arc<TcpStream>)> for Terminal {
    fn from(value: (String, Arc<TcpStream>)) -> Self {
        let (name, stream) = value;
        let (width, height) = crossterm::terminal::size().unwrap();
        
        // Elliptic curve data
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
}

impl Terminal {
    pub fn draw(&mut self) {
        self.draw_messages();
        self.draw_input_area();
    }

    pub fn draw_messages(&mut self) {
        // Use [scroll_pos] to get relevant messages
        let input_height = 3;
        let output_height = self.height - input_height;

        let total_messages = self.messages.len();

        // NOTE: Not all messages are only 1 line long
        let messages = if output_height < total_messages as u16 {
            let lower_bound = total_messages - output_height as usize;
            let upper_bound = total_messages;
            let offset = self.msg_offset;
            &self.messages[lower_bound - offset..upper_bound - offset]
        } else {
            &self.messages.as_slice()
        };

        // Clear message area
        execute!(stdout(), MoveTo(0, self.height - 3), Clear(ClearType::FromCursorUp), MoveTo(0, 0)).unwrap();

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
    }

    pub fn draw_input_area(&mut self) {
        // NOTE: Needs fixing (breaks on multi-line input)
        execute!(
            stdout(),
            // Draw separator line
            MoveTo(0, self.height - 3),
            Clear(ClearType::FromCursorDown),
            Print((0..self.width).map(|_| '_').collect::<String>()),
            // Draw input area
            MoveTo(0, self.height - 1),
            Clear(ClearType::CurrentLine),
            Print(format!(
                " Message: {}",
                &self.input_buffer
            ))
        )
        .unwrap();
    }

    pub fn send_ec_point(&mut self) {
        let ec_point = self.ec_point.to_string();
        write!(self.stream.as_ref(), "{ec_point}\n").unwrap();
    }

    pub fn create_cipher(&mut self, ec_point_string: String) {
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

    pub fn save_message(&mut self, message: String) {
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

        let message = Message::from((sender.to_string(), decrypted, sender == self.name));

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

    pub fn handle_key_event(&mut self, key_event: KeyEvent) {
        // Ignore key releases
        if key_event.is_release() {
            return;
        }

        match key_event.code {
            KeyCode::Esc => {
                crossterm::terminal::disable_raw_mode().unwrap();
                execute!(stdout(), crossterm::terminal::LeaveAlternateScreen).unwrap();
                std::process::exit(0);
            }
            KeyCode::Char(c) => {
                // Add char to input buffer
                self.input_buffer.push(c);
                self.draw_input_area();
            }
            KeyCode::Backspace => {
                // Remove char from input buffer + Clear char in input
                if let Some(_) = self.input_buffer.pop() {
                    self.draw_input_area();
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

    pub fn handle_resize(&mut self, new_width: u16, new_height: u16) {
        self.width = new_width;
        self.height = new_height;
        self.draw();
    }

    pub fn handle_mouse_event(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::ScrollDown => self.scroll_down(),
            MouseEventKind::ScrollUp => self.scroll_up(),
            _ => {}
        }
    }
}