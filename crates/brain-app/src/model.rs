//! Keeps the [`Board`] in sync with the event log, the session files and the transcripts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use brain_core::account::{discover_accounts, home_dir, Account};
use brain_core::process::pid_alive;
use brain_core::project::{project_of, Project};
use brain_core::sessions::read_session_files;
use brain_core::state::{Board, Phase, SessionKey};
use brain_core::store::{Store, Tail};
use brain_core::transcript::{find_transcript, insight, Message};
use brain_core::usage::{self, Snapshot};
use chrono::{Days, Local, NaiveDate};

use crate::demo;

/// How many days of events are read at start, so sessions running past midnight keep their
/// timeline and recently ended ones stay searchable.
pub const HISTORY_DAYS: u64 = 7;

/// A session that just started waiting for the user.
pub struct Attention {
    pub key: SessionKey,
    pub name: String,
    pub phase: Phase,
    pub headline: Option<String>,
}

/// Where a session's transcript is and how long it was when last read.
struct TranscriptCache {
    session_id: String,
    path: Option<PathBuf>,
    len: u64,
}

pub struct Model {
    pub accounts: Vec<Account>,
    pub board: Board,
    store: Store,
    tail: Tail,
    day: NaiveDate,
    last_phases: HashMap<SessionKey, Phase>,
    primed: bool,
    transcripts: HashMap<SessionKey, TranscriptCache>,
    projects: HashMap<String, Project>,
    /// Status line snapshots (plan limits, context, cost) of all sessions.
    pub usage: Vec<Snapshot>,
    /// Fixed conversations in demo mode; `None` reads real transcripts.
    pub demo_messages: Option<HashMap<SessionKey, Vec<Message>>>,
}

impl Model {
    pub fn load() -> Self {
        let home = home_dir();
        let store = Store::new(Store::default_root(&home));
        let day = Local::now().date_naive();
        let mut model = Self {
            accounts: discover_accounts(&home),
            board: Board::default(),
            tail: Tail::new(store.file_for(day)),
            store,
            day,
            last_phases: HashMap::new(),
            primed: false,
            transcripts: HashMap::new(),
            projects: HashMap::new(),
            usage: Vec::new(),
            demo_messages: None,
        };
        if demo::enabled() {
            let demo = demo::build();
            model.accounts = demo.accounts;
            model.board = demo.board;
            model.usage = demo.usage;
            model.demo_messages = Some(demo.messages);
            return model;
        }
        for back in (1..HISTORY_DAYS).rev() {
            let Some(day) = day.checked_sub_days(Days::new(back)) else { continue };
            for event in Tail::new(model.store.file_for(day)).read_new() {
                model.board.apply_event(&event);
            }
        }
        model
    }

    pub fn is_demo(&self) -> bool {
        self.demo_messages.is_some()
    }

    /// The status line snapshot of a session, if its status line reported to Brain.
    pub fn snapshot(&self, key: &SessionKey) -> Option<&Snapshot> {
        self.usage.iter().find(|s| s.account == key.account && s.pid == key.pid)
    }

    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    /// The project of a directory, computed once per directory.
    pub fn project(&mut self, cwd: &str) -> &Project {
        self.projects
            .entry(cwd.to_string())
            .or_insert_with(|| project_of(Path::new(cwd)))
    }

    /// Reads new events, session files and grown transcripts. Returns sessions that moved into
    /// NeedsYou/YourTurn since the last refresh (none on the very first one).
    pub fn refresh(&mut self) -> Vec<Attention> {
        if self.is_demo() {
            return Vec::new();
        }
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
        self.refresh_transcripts(&seen);
        self.usage = usage::read_all(&usage::status_dir(&home_dir()));

        let mut attention = Vec::new();
        for session in self.board.sorted() {
            let phase = session.phase();
            let previous = self.last_phases.insert(session.key.clone(), phase);
            let entered = matches!(phase, Phase::NeedsYou | Phase::YourTurn) && previous != Some(phase);
            if self.primed && entered && previous.is_some() {
                attention.push(Attention {
                    key: session.key.clone(),
                    name: session.display_name(),
                    phase,
                    headline: session.headline(),
                });
            }
        }
        self.primed = true;
        attention
    }

    /// Re-reads a live session's transcript tail when the file grew.
    fn refresh_transcripts(&mut self, live: &[SessionKey]) {
        for key in live {
            let Some(session) = self.board.get(key) else { continue };
            if !session.alive {
                continue;
            }
            let Some(session_id) = session.session_id.clone() else { continue };
            let Some(account) = self.accounts.iter().find(|a| a.id == key.account) else { continue };

            let cache = self.transcripts.entry(key.clone()).or_insert_with(|| TranscriptCache {
                session_id: session_id.clone(),
                path: None,
                len: 0,
            });
            if cache.session_id != session_id {
                *cache = TranscriptCache { session_id: session_id.clone(), path: None, len: 0 };
            }
            if cache.path.is_none() {
                cache.path = find_transcript(account, &session_id);
            }
            let Some(path) = cache.path.clone() else { continue };
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len == cache.len {
                continue;
            }
            cache.len = len;
            let info = insight(&path);
            self.board.apply_insight(key, info);
        }
    }
}
