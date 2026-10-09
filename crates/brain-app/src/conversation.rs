//! The latest transcript messages of the selected session, reloaded when the file grows.

use std::path::PathBuf;

use brain_core::account::Account;
use brain_core::state::SessionKey;
use brain_core::transcript::{find_transcript, read_recent_messages, Message};

const MESSAGE_LIMIT: usize = 40;

pub struct Conversation {
    pub key: SessionKey,
    session_id: Option<String>,
    path: Option<PathBuf>,
    len: u64,
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn empty(key: SessionKey) -> Self {
        Self { key, session_id: None, path: None, len: 0, messages: Vec::new() }
    }

    /// A conversation that never reloads (demo mode).
    pub fn fixed(key: SessionKey, messages: Vec<Message>) -> Self {
        Self { messages, ..Self::empty(key) }
    }

    /// Re-reads the transcript if the session, its id or the file size changed.
    /// Returns true when `messages` changed.
    pub fn sync(&mut self, key: &SessionKey, session_id: Option<&str>, accounts: &[Account]) -> bool {
        if &self.key != key || self.session_id.as_deref() != session_id {
            *self = Self::empty(key.clone());
            self.session_id = session_id.map(str::to_string);
            self.path = session_id.and_then(|id| {
                let account = accounts.iter().find(|a| a.id == key.account)?;
                find_transcript(account, id)
            });
            self.load();
            return true;
        }
        if self.path.is_none() {
            // The transcript file appears with the first message of a new session.
            self.path = session_id.and_then(|id| {
                let account = accounts.iter().find(|a| a.id == key.account)?;
                find_transcript(account, id)
            });
        }
        let len = self.current_len();
        if len != self.len {
            self.load();
            return true;
        }
        false
    }

    /// The transcript file, once it exists, and its size when last read.
    pub fn transcript(&self) -> Option<(&std::path::Path, u64)> {
        self.path.as_deref().map(|p| (p, self.len))
    }

    fn current_len(&self) -> u64 {
        self.path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map_or(0, |m| m.len())
    }

    fn load(&mut self) {
        self.len = self.current_len();
        self.messages = self
            .path
            .as_deref()
            .map(|p| read_recent_messages(p, MESSAGE_LIMIT))
            .unwrap_or_default();
    }
}
