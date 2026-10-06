//! Konsole only runs on Linux.

use super::{Key, Outcome};

pub struct Tab;

pub fn locate(_konsole_pid: u32, _shell_pid: u32) -> Option<Tab> {
    None
}

impl Tab {
    pub fn accepts_typing(&self) -> bool {
        false
    }

    pub fn focus(&self) -> Outcome {
        Outcome::NoTerminal
    }

    pub fn type_text(&self, _text: &str) -> Outcome {
        Outcome::NoTerminal
    }

    pub fn send_key(&self, _key: Key) -> Outcome {
        Outcome::NoTerminal
    }
}
