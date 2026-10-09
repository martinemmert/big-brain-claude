//! Keeps the [`Board`] in sync with the event log, the session files and the transcripts.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use brain_core::agents::{self, BackgroundAgent};

use brain_core::account::{discover_accounts, home_dir, Account};
use brain_core::process::{pid_alive, process_started_ms};
use brain_core::project::{project_of, Project};
use brain_core::sessions::read_session_files;
use brain_core::state::{Board, Phase, Session, SessionKey};
use brain_core::store::{Store, Tail};
use brain_core::transcript::{entrypoint_of, find_transcript, insight, kept_transcripts, Message};
use brain_core::usage::{self, Snapshot};
use chrono::{Days, Local, NaiveDate};

use crate::demo;

/// How many days of events are read at start, so sessions running past midnight keep their
/// timeline and recently ended ones stay searchable.
/// Days of events read at start: as long as Claude Code keeps transcripts in any account, since
/// that is how long an ended session can be resumed.
fn history_days(accounts: &[Account]) -> u64 {
    accounts.iter().map(|a| a.transcript_days() as u64).max().unwrap_or(30)
}

/// How often (in refreshes) the kept transcripts are listed again.
const KEPT_EVERY: u32 = 60;
/// How often `claude agents` is asked for background sessions.
const AGENTS_EVERY: Duration = Duration::from_secs(5);

/// Per account, its background sessions, or `None` when `claude agents` failed there.
type AgentLists = Vec<(String, Option<Vec<BackgroundAgent>>)>;

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
    /// Per account: ids of the sessions whose transcripts still exist (resumable).
    kept: HashMap<String, HashMap<String, PathBuf>>,
    /// Sessions whose transcript start was read for how they were started.
    entrypoints_read: HashSet<SessionKey>,
    kept_age: u32,
    /// The `claude agents` run in flight, and when the last one started.
    agents_pending: Option<Receiver<AgentLists>>,
    agents_asked: Option<Instant>,
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
            kept: HashMap::new(),
            entrypoints_read: HashSet::new(),
            kept_age: 0,
            agents_pending: None,
            agents_asked: None,
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
        for back in (1..history_days(&model.accounts)).rev() {
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
        let session = self.board.get(key)?;
        self.usage.iter().find(|s| s.account == key.account && s.pid == session.pid)
    }

    /// Whether an ended session can still be resumed: its transcript still exists. Unknown until
    /// the first refresh listed the transcripts (and always in the demo): assumed yes.
    pub fn resumable(&self, session: &Session) -> bool {
        // Sessions younger than the last listing: the transcript Brain found while they ran.
        if self.transcripts.get(&session.key).and_then(|c| c.path.as_ref()).is_some_and(|p| p.is_file()) {
            return true;
        }
        match (self.kept.get(&session.key.account), &session.session_id) {
            (Some(ids), Some(id)) => ids.contains_key(id),
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    /// Days until Claude Code deletes an ended session's transcript.
    pub fn days_left(&self, session: &Session, now_ms: i64) -> Option<i64> {
        let days = self.account(&session.key.account)?.transcript_days() as i64;
        let left_ms = session.last_activity_ms + days * 86_400_000 - now_ms;
        Some((left_ms as f64 / 86_400_000.0).ceil().max(0.0) as i64)
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
        let mut claimed: Vec<(String, u32)> = Vec::new();
        for account in &self.accounts {
            for file in read_session_files(account) {
                let alive = pid_alive(file.pid);
                seen.push(self.board.apply_session_file(&account.id, &file, alive));
                claimed.push((account.id.clone(), file.pid));
            }
        }
        // Sessions known only from events (no status file, e.g. `claude -p`): still running only
        // if their process is, is not another session's, and started before the session did —
        // otherwise macOS has given the pid to a new process.
        let unseen: Vec<SessionKey> = self.board.keys().filter(|k| !seen.contains(k)).cloned().collect();
        for key in unseen {
            let Some(session) = self.board.get(&key) else { continue };
            let pid = session.pid;
            let first_ms = session.timeline.first().map(|e| e.ts.timestamp_millis()).unwrap_or(session.last_activity_ms);
            let alive = !claimed.contains(&(key.account.clone(), pid))
                && pid_alive(pid)
                && process_started_ms(pid).is_some_and(|started| started <= first_ms);
            self.board.set_alive(&key, alive);
        }
        if self.kept_age == 0 {
            self.kept = self.accounts.iter().map(|a| (a.id.clone(), kept_transcripts(a))).collect();
        }
        self.read_entrypoints();
        self.kept_age = (self.kept_age + 1) % KEPT_EVERY;
        self.refresh_agents();
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

    /// Lists the transcripts again on the next refresh (after Brain deleted one).
    pub fn relist_transcripts(&mut self) {
        self.kept_age = 0;
        self.agents_asked = None;
    }

    /// Applies the last `claude agents` answer and asks again every 20 s, off the UI thread.
    /// A background session counts for the account whose folder holds its transcript.
    fn refresh_agents(&mut self) {
        if let Some(lists) = self.agents_pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.agents_pending = None;
            let mut owned: HashMap<String, Vec<BackgroundAgent>> = HashMap::new();
            let mut seen: HashSet<String> = HashSet::new();
            for (listed_by, agents) in &lists {
                for agent in agents.iter().flatten() {
                    if !seen.insert(agent.session_id.clone()) {
                        continue;
                    }
                    let owner = self
                        .accounts
                        .iter()
                        .find(|a| self.kept.get(&a.id).is_some_and(|ids| ids.contains_key(&agent.session_id)))
                        .map_or(listed_by.clone(), |a| a.id.clone());
                    owned.entry(owner).or_default().push(agent.clone());
                }
            }
            for account in &self.accounts {
                // An account whose listing failed keeps what it had.
                if lists.iter().any(|(id, agents)| *id == account.id && agents.is_none()) {
                    continue;
                }
                let agents = owned.remove(&account.id).unwrap_or_default();
                self.board.set_agents(&account.id, &agents);
                for agent in &agents {
                    let key = SessionKey { account: account.id.clone(), id: agent.session_id.clone() };
                    let modified = find_transcript(account, &agent.session_id)
                        .and_then(|path| std::fs::metadata(path).ok()?.modified().ok())
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|since| since.as_millis() as i64);
                    if let Some(at) = modified {
                        self.board.touch(&key, at);
                    }
                }
            }
        }
        if self.agents_pending.is_some() || self.agents_asked.is_some_and(|at| at.elapsed() < AGENTS_EVERY) {
            return;
        }
        let accounts = self.accounts.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let lists: AgentLists = accounts.iter().map(|a| (a.id.clone(), agents::list(a))).collect();
            let _ = tx.send(lists);
        });
        self.agents_pending = Some(rx);
        self.agents_asked = Some(Instant::now());
    }

    /// Learns how sessions without a transcript reading were started (a program, or the user):
    /// once per session, from the start of its transcript.
    fn read_entrypoints(&mut self) {
        let unread: Vec<(SessionKey, PathBuf)> = self
            .board
            .keys()
            .filter(|k| !self.entrypoints_read.contains(*k))
            .filter_map(|k| {
                let session = self.board.get(k)?;
                if session.insight.entrypoint.is_some() {
                    return None;
                }
                let path = self.kept.get(&k.account)?.get(session.session_id.as_ref()?)?;
                Some((k.clone(), path.clone()))
            })
            .collect();
        for (key, path) in unread {
            self.board.set_entrypoint(&key, entrypoint_of(&path));
            self.entrypoints_read.insert(key);
        }
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
