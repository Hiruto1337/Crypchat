pub struct Message {
    sender: String,
    msg: String,
    from_self: bool,
}

impl From<(String, String, bool)> for Message {
    fn from(value: (String, String, bool)) -> Self {
        Message {
            sender: value.0,
            msg: value.1,
            from_self: value.2,
        }
    }
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
    pub fn get_len(&self) -> u16 {
        " <> ".len() as u16 + self.sender.chars().count() as u16 + self.msg.chars().count() as u16
    }
}