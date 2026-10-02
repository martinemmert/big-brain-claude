//! Keeps the [`Board`] in sync with the event log and the session files.

use std::collections::HashMap;

use brain_core::account::{discover_accounts, home_dir, Account};
use brain_core::process::pid_alive;
use brain_core::sessions::read_session_files;
use brain_core::state::{Board, Phase, SessionKey};
use brain_core::store::{Store, Tail};
use chrono::{Local, NaiveDate};

/// A session that just started waiting for the user.
pub struct Attention {
    pub name: String,
    pub account: String,
    pub phase: Phase,
    pub headline: Option<String>,
}

pub struct Model {
    pub accounts: Vec<Account>,
    pub board: Board,
    store: Store,
    tail: Tail,
    day: NaiveDate,
    last_phases: HashMap<SessionKey, Phase>,
    primed: bool,
}

impl Model {
    pub fn load() -> Self {
        let home = home_dir();
        let store = Store::new(Store::default_root(&home));
        let day = Local::now().date_naive();
        Self {
            accounts: discover_accounts(&home),
            board: Board::default(),
            tail: Tail::new(store.file_for(day)),
            store,
            day,
            last_phases: HashMap::new(),
            primed: false,
        }
    }

    /// Reads new events and session files. Returns sessions that moved into
    /// NeedsYou/YourTurn since the last refresh (none on the very first one).
    pub fn refresh(&mut self) -> Vec<Attention> {
        let today = Local::now().date_naive();
        if today != self.day {
            // Finish yesterday's file, then follow today's.
            for event in self.tail.read_new() {
                self.board.apply_event(&event);
            }
            self.day = today;
            self.tail = Tail::new(self.store.file_for(today));
        }
        for event in self.tail.read_new() {
            self.board.apply_event(&event);
        }

        let mut seen = Vec::new();
        for account in &self.accounts {
            for file in read_session_files(account) {
                let alive = pid_alive(file.pid);
                self.board.apply_session_file(&account.id, &file, alive);
                seen.push(SessionKey { account: account.id.clone(), pid: file.pid });
            }
        }
        // Sessions known only from events: a missing session file means Claude cleaned up.
        let unseen: Vec<SessionKey> = self.board.keys().filter(|k| !seen.contains(k)).cloned().collect();
        for key in unseen {
            self.board.set_alive(&key, pid_alive(key.pid));
        }

        let mut attention = Vec::new();
        for session in self.board.sorted() {
            let phase = session.phase();
            let previous = self.last_phases.insert(session.key.clone(), phase);
            let entered = matches!(phase, Phase::NeedsYou | Phase::YourTurn) && previous != Some(phase);
            if self.primed && entered && previous.is_some() {
                attention.push(Attention {
                    name: session.display_name(),
                    account: session.key.account.clone(),
                    phase,
                    headline: session.headline(),
                });
            }
        }
        self.primed = true;
        attention
    }
}
