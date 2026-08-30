use std::io::stdout;

use aes::{Aes128, cipher::KeyInit};
use anyhow::Result;
use base64::Engine;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};
use tokio::sync::mpsc::Sender;

use crate::crypto::{aes_cbc, diffie_hellman::*};

pub struct Terminal {
    pub name: String,
    pub tx: Option<Sender<String>>,
    pub current_writer: Option<String>,
    pub height: u16,
    pub width: u16,
    pub input_height: u16,
    pub messages: Vec<String>,
    pub top_reached: bool,
    pub input_buffer: String,
    pub scroll: usize,
    pub cipher: Option<Aes128>,
    pub secret_number: Option<U576>,
    pub ec_point: Point,
}

impl From<String> for Terminal {
    fn from(value: String) -> Self {
        let name = value;
        let (width, height) = crossterm::terminal::size().unwrap();

        // Elliptic curve data
        let generator = get_generator_point();
        let secret_number = get_random_uint();
        let ec_point = get_elliptic_curve().get_point_from(generator, secret_number);

        Terminal {
            name,
            tx: None,
            current_writer: None,
            height,
            width,
            input_height: 2,
            messages: vec![],
            top_reached: true,
            input_buffer: String::new(),
            scroll: 0,
            cipher: None,
            secret_number: Some(secret_number),
            ec_point,
        }
    }
}

impl Terminal {
    fn get_string_height(&self, message: &String) -> usize {
        message.len() / self.width as usize + 1
    }

    fn update_input_height(&mut self) {
        let input_height =
            self.get_string_height(&(self.input_buffer.clone() + "Message: ")) as u16;

        // Add 1 to accomodate the separator line
        self.input_height = 1 + input_height;
    }

    fn get_input_coordinates(&self) -> (u16, u16) {
        let input_x = ("Message: ".len() + self.input_buffer.len()) % self.width as usize;

        let input_height = self.get_string_height(&(self.input_buffer.clone() + "Message: "));
        let input_y = self.height - self.input_height + input_height as u16;

        (input_x as u16, input_y)
    }

    pub fn draw(&mut self) {
        self.update_input_height();
        execute!(stdout(), Hide).unwrap();
        self.draw_messages();
        self.draw_input_area();
        execute!(stdout(), Show).unwrap();
    }

    pub fn draw_messages(&mut self) {
        // Track remaining screen estate
        let output_height = self.height - self.input_height;
        let mut rem_height = output_height + 1;
        let mut rem_scroll = self.scroll;

        // Collect the relevant messages
        let mut lines: Vec<String> = vec![];

        'msg_loop: for message in self.messages.iter().rev() {
            // Skip unnecessary messages
            let msg_height = self.get_string_height(message);
            if msg_height <= rem_scroll {
                rem_scroll -= msg_height;
                continue;
            }

            let chars: Vec<char> = message.chars().collect();
            let msg_lines = chars.chunks(self.width as usize);

            // Add them to [lines] in reverse
            for line in msg_lines.rev() {
                // Skip unnecessary lines
                if rem_scroll != 0 {
                    rem_scroll -= 1;
                    continue;
                }

                if rem_height == 0 {
                    break 'msg_loop;
                }

                lines.push(line.iter().collect());
                rem_height -= 1;
            }
        }

        if rem_height == 0 {
            self.top_reached = false;
            let _ = lines.pop();
        } else {
            self.top_reached = true;
        }

        lines.reverse();

        let output = lines
            .into_iter()
            .fold(String::new(), |acc, line| acc + line.as_str() + "\r\n");

        // Print result
        let (input_x, input_y) = self.get_input_coordinates();
        execute!(
            stdout(),
            MoveTo(self.width, output_height - 1),
            Clear(ClearType::FromCursorUp),
            MoveTo(0, 0),
            Print(output),
            MoveTo(input_x, input_y)
        )
        .unwrap();
    }

    pub fn draw_input_area(&mut self) {
        execute!(
            stdout(),
            // Draw separator line
            MoveTo(0, self.height - self.input_height),
            Clear(ClearType::FromCursorDown),
            Print((0..self.width).map(|_| '_').collect::<String>()),
            // Draw input area
            Print(format!("Message: {}", &self.input_buffer))
        )
        .unwrap();
    }

    pub fn create_cipher(&mut self, ec_point_string: String) {
        let (x, y) = ec_point_string.split_once(';').unwrap();

        let received_ec_point = Point::from((x, y));
        let secret_shared_point =
            get_elliptic_curve().get_point_from(received_ec_point, self.secret_number.take().unwrap());
        let x = secret_shared_point.get_x();

        let mut key: [u8; 16] = [0; 16];
        key.copy_from_slice(&sha256::digest(x.to_string()).as_bytes()[0..16]);

        let array = aes::cipher::Array::from(key);
        self.cipher = Some(Aes128::new(&array));
    }

    async fn send_message(&mut self) -> Result<()> {
        let trimmed = self.input_buffer.trim().to_string();

        if trimmed == "" {
            return Ok(());
        }

        if trimmed == "/status" {
            self.input_buffer.clear();
            self.draw();
            self.tx.as_ref().unwrap().send("/status".to_string()).await?;
            return Ok(());
        }

        let Some(cipher) = &self.cipher else {
            return Ok(());
        };

        let bytes: Vec<u8> = trimmed.into_bytes();
        let encrypted = aes_cbc::encrypt(&bytes, cipher);
        let encoded = base64::engine::general_purpose::STANDARD.encode(encrypted);

        let mut message = format!("{}:{}", &self.name, encoded);

        // Save message to our own terminal
        self.save_message(message.clone());

        // Add newline
        message.push('\n');

        // Write message to stream
        let Some(tx) = self.tx.as_ref() else {
            return Ok(());
        };

        tx.send(message).await?;

        // Clear input_buffer
        self.input_buffer.clear();

        self.draw();

        Ok(())
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

        if let Some(msg_sender) = &self.current_writer
            && &sender == msg_sender
        {
            // Do nothing
        } else {
            self.current_writer = Some(sender.to_string());
            let color = if &sender == &self.name {
                "\x1b[1;32m" // Set color to green
            } else {
                "\x1b[1;31m" // Set color to red
            };

            self.messages.push(format!("{color}<{sender}>\x1b[0m"));
        }

        self.messages.push(decrypted);
        self.draw();
    }

    fn scroll_up(&mut self) {
        if self.top_reached == false {
            self.scroll += 1;
            self.draw();
        }
    }

    fn scroll_down(&mut self) {
        if self.scroll != 0 {
            self.scroll -= 1;
            self.draw();
        }
    }

    pub async fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        // Ignore key releases
        if key_event.is_release() {
            return Ok(());
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
                self.draw();
            }
            KeyCode::Backspace => {
                // Remove char from input buffer + Clear char in input
                if self.input_buffer.pop().is_some() {
                    self.draw();
                }
            }
            KeyCode::Enter => {
                // Send message
                self.send_message().await?;
            }
            KeyCode::Up => self.scroll_up(),
            KeyCode::Down => self.scroll_down(),
            _ => {}
        }

        Ok(())
    }

    pub fn handle_resize(&mut self, new_width: u16, new_height: u16) {
        if self.height < new_height {
            self.scroll = 0;
        }

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
