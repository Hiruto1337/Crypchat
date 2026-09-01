use std::{io::stdout, sync::{Arc, Mutex, atomic::AtomicBool}, thread::{JoinHandle, sleep}, time::Duration};

use crossterm::{cursor::{Hide, MoveLeft, Show}, execute, style::Print};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use tokio::{io::{AsyncBufReadExt, BufReader}, sync::mpsc::{Receiver, Sender}};

use crate::misc::terminal::Terminal;

pub struct LoadingAnimation {
    thread_handle: JoinHandle<()>,
    kill: Arc<AtomicBool>
}

impl LoadingAnimation {
    pub fn new() -> Self {
    let kill = Arc::new(AtomicBool::new(false));
    let killer = kill.clone();

    // Spawn animation thread
    let animation = std::thread::spawn(move || {
        let mut animation = "⠋⠙⠸⢰⣠⣄⡆⠇".chars().cycle();

        execute!(stdout(), Hide).unwrap();
        while kill.load(std::sync::atomic::Ordering::Relaxed) == false {
            execute!(stdout(), Print(animation.next().unwrap()), MoveLeft(1)).unwrap();
            sleep(Duration::from_millis(125));
        }
        execute!(stdout(), Show).unwrap();
    });

    LoadingAnimation { thread_handle: animation, kill: killer }
    }

    pub fn kill(self) {
        self.kill.store(true, std::sync::atomic::Ordering::Relaxed);
        self.thread_handle.join().unwrap();
    }
}

pub fn spawn_writer(conn: Connection, mut write_stream: SendStream, mut rx: Receiver<String>) {
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if message == "/status" {
                for path in &conn.paths() {
                    if path.is_selected() == false {
                        continue;
                    }

                    if path.is_ip() {
                        execute!(stdout(), Print("👤⇄👤")).unwrap();
                        break;
                    } else if path.is_relay() {
                        execute!(stdout(), Print("👤⇄📻⇄👤")).unwrap();
                        break;
                    }
                }
                continue;
            }
            write_stream.write_all(message.as_bytes()).await.unwrap();
        }
    });
}

pub fn spawn_reader(read_stream: RecvStream, terminal: Arc<Mutex<Terminal>>, tx: Sender<String>) {
    tokio::spawn(async move {
        let reader = BufReader::new(read_stream);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let response = {
                let mut lock = terminal.lock().unwrap();

                if lock.cipher.is_some() {
                    // Save message
                    lock.save_message(line);
                    None
                } else if line != lock.ec_point.to_string() {
                    // If incoming EC point is not my own
                    // Create cipher and reciprocate my own EC point
                    lock.create_cipher(line);
                    Some(lock.ec_point.to_string() + "\n")
                } else {
                    None
                }
            };

            if let Some(ec_point) = response {
                tx.send(ec_point).await.unwrap();
            }
        }
    });
}
