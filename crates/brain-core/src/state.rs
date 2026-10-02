use std::collections::HashMap;

use crate::event::{Event, Kind, Source};
use crate::sessions::SessionFile;

/// What the user sees for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// Claude asked a question or needs a permission.
    NeedsYou,
    /// The turn finished; the user is up.
    YourTurn,
    Working,
    Ended,
}

/// Turn state as observed by a hook or by Claude Code's own session file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    Busy,
    Waiting,
    Idle,
    Ended,
}

impl Signal {
    fn from_hook(kind: Kind) -> Option<Self> {
        match kind {
            Kind::Prompt => Some(Signal::Busy),
            Kind::Permission => Some(Signal::Waiting),
            Kind::Stop | Kind::SessionStart => Some(Signal::Idle),
            Kind::SessionEnd => Some(Signal::Ended),
            Kind::Doing | Kind::Waiting | Kind::Done => None,
        }
    }

    fn from_file_status(status: &str) -> Option<Self> {
        match status {
            "busy" | "shell" => Some(Signal::Busy),
            "waiting" => Some(Signal::Waiting),
            "idle" => Some(Signal::Idle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey {
    pub account: String,
    pub pid: u32,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub key: SessionKey,
    pub session_id: Option<String>,
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub alive: bool,
    pub started_ms: Option<i64>,
    pub last_activity_ms: i64,
    /// Events of this session, oldest first.
    pub timeline: Vec<Event>,

    // Texts of the current turn; cleared when a new prompt arrives.
    prompt: Option<String>,
    doing: Option<String>,
    question: Option<String>,
    permission: Option<String>,
    done: Option<String>,
    reply: Option<String>,

    hook_signal: Option<(Signal, i64)>,
    file_signal: Option<(Signal, i64)>,
}

const TIMELINE_LIMIT: usize = 200;

impl Session {
    fn new(key: SessionKey) -> Self {
        Self {
            key,
            session_id: None,
            name: None,
            cwd: None,
            alive: true,
            started_ms: None,
            last_activity_ms: 0,
            timeline: Vec::new(),
            prompt: None,
            doing: None,
            question: None,
            permission: None,
            done: None,
            reply: None,
            hook_signal: None,
            file_signal: None,
        }
    }

    pub fn display_name(&self) -> String {
        if let Some(name) = self.name.as_deref().filter(|n| !n.is_empty()) {
            return name.to_string();
        }
        self.cwd
            .as_deref()
            .and_then(|c| c.rsplit('/').find(|s| !s.is_empty()))
            .map(str::to_string)
            .unwrap_or_else(|| format!("pid {}", self.key.pid))
    }

    fn current_signal(&self) -> Option<(Signal, i64)> {
        match (self.hook_signal, self.file_signal) {
            (Some(h), Some(f)) => Some(if h.1 >= f.1 { h } else { f }),
            (h, f) => h.or(f),
        }
    }

    pub fn phase(&self) -> Phase {
        if !self.alive || matches!(self.hook_signal, Some((Signal::Ended, _))) {
            return Phase::Ended;
        }
        match self.current_signal().map(|(signal, _)| signal) {
            Some(Signal::Busy) => Phase::Working,
            Some(Signal::Waiting) => Phase::NeedsYou,
            Some(Signal::Idle) if self.question.is_some() => Phase::NeedsYou,
            Some(Signal::Idle) => Phase::YourTurn,
            Some(Signal::Ended) => Phase::Ended,
            None if self.question.is_some() => Phase::NeedsYou,
            None => Phase::Working,
        }
    }

    /// When the current phase began (epoch ms).
    pub fn phase_since_ms(&self) -> i64 {
        self.current_signal()
            .map(|(_, ts)| ts)
            .unwrap_or(self.last_activity_ms)
    }

    /// The one line that best describes the session right now.
    pub fn headline(&self) -> Option<String> {
        let pick = |options: &[&Option<String>]| options.iter().find_map(|o| (*o).clone());
        match self.phase() {
            Phase::NeedsYou => match self.current_signal() {
                Some((Signal::Waiting, _)) => pick(&[&self.permission, &self.question])
                    .or_else(|| Some("Wartet auf Freigabe".into())),
                _ => pick(&[&self.question, &self.permission]),
            },
            Phase::YourTurn | Phase::Ended => pick(&[&self.done, &self.reply, &self.doing, &self.prompt]),
            Phase::Working => pick(&[&self.doing, &self.prompt]),
        }
    }

    /// True when the headline comes from Claude's own `brain report`.
    pub fn headline_is_reported(&self) -> bool {
        match self.phase() {
            Phase::NeedsYou => self.question.is_some() && self.permission.is_none(),
            Phase::YourTurn | Phase::Ended => self.done.is_some(),
            Phase::Working => self.doing.is_some(),
        }
    }

    fn apply(&mut self, event: &Event) {
        let ts = event.ts.timestamp_millis();
        self.last_activity_ms = self.last_activity_ms.max(ts);
        if event.session_id.is_some() {
            self.session_id = event.session_id.clone();
        }
        if self.cwd.is_none() {
            self.cwd = event.cwd.clone();
        }

        // Claude Code also sends a `Notification` when a finished session has been
        // idle for a while. Permission prompts only happen mid-turn, so a
        // notification on an idle session is kept in the timeline only.
        let idle_reminder = event.kind == Kind::Permission
            && matches!(self.current_signal(), Some((Signal::Idle, _)));
        if idle_reminder {
            self.timeline.push(event.clone());
            return;
        }

        let text = event.text.clone();
        match event.kind {
            Kind::Prompt => {
                self.prompt = text;
                self.doing = None;
                self.question = None;
                self.permission = None;
                self.done = None;
                self.reply = None;
            }
            Kind::Permission => self.permission = text,
            Kind::Stop => {
                self.reply = text;
                self.permission = None;
            }
            Kind::Doing => self.doing = text,
            Kind::Waiting => self.question = text,
            Kind::Done => {
                self.done = text;
                self.question = None;
            }
            Kind::SessionStart | Kind::SessionEnd => {}
        }

        if event.source == Source::Hook {
            if let Some(signal) = Signal::from_hook(event.kind) {
                if self.hook_signal.is_none_or(|(_, prev)| ts >= prev) {
                    self.hook_signal = Some((signal, ts));
                }
            }
        }

        self.timeline.push(event.clone());
        if self.timeline.len() > TIMELINE_LIMIT {
            self.timeline.remove(0);
        }
    }
}

/// All sessions across all accounts, built from events and session files.
#[derive(Debug, Default)]
pub struct Board {
    sessions: HashMap<SessionKey, Session>,
}

impl Board {
    pub fn apply_event(&mut self, event: &Event) {
        let key = SessionKey { account: event.account.clone(), pid: event.pid };
        self.sessions
            .entry(key.clone())
            .or_insert_with(|| Session::new(key))
            .apply(event);
    }

    pub fn apply_session_file(&mut self, account: &str, file: &SessionFile, alive: bool) {
        let key = SessionKey { account: account.to_string(), pid: file.pid };
        let session = self
            .sessions
            .entry(key.clone())
            .or_insert_with(|| Session::new(key));
        session.alive = alive;
        session.name = file.name.clone().or(session.name.take());
        session.cwd = file.cwd.clone().or(session.cwd.take());
        session.session_id = file.session_id.clone().or(session.session_id.take());
        session.started_ms = file.started_at.or(session.started_ms);
        if let Some(updated) = file.updated_at {
            session.last_activity_ms = session.last_activity_ms.max(updated);
        }
        let status_ts = file.status_updated_at.or(file.updated_at).unwrap_or(0);
        session.file_signal = file
            .status
            .as_deref()
            .and_then(Signal::from_file_status)
            .map(|signal| (signal, status_ts));
    }

    pub fn set_alive(&mut self, key: &SessionKey, alive: bool) {
        if let Some(session) = self.sessions.get_mut(key) {
            session.alive = alive;
        }
    }

    pub fn get(&self, key: &SessionKey) -> Option<&Session> {
        self.sessions.get(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &SessionKey> {
        self.sessions.keys()
    }

    /// Sessions in triage order: needs-you and your-turn first (longest waiting
    /// first), then working (by name), then ended (most recent first).
    pub fn sorted(&self) -> Vec<&Session> {
        let mut list: Vec<&Session> = self.sessions.values().collect();
        list.sort_by(|a, b| {
            let (pa, pb) = (a.phase(), b.phase());
            pa.cmp(&pb).then_with(|| match pa {
                Phase::NeedsYou | Phase::YourTurn => a.phase_since_ms().cmp(&b.phase_since_ms()),
                Phase::Working => a.display_name().cmp(&b.display_name()),
                Phase::Ended => b.last_activity_ms.cmp(&a.last_activity_ms),
            })
        });
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn ev(secs: i64, source: Source, kind: Kind, text: Option<&str>) -> Event {
        Event {
            v: 1,
            ts: Utc.timestamp_opt(1_790_000_000 + secs, 0).unwrap(),
            account: "second".into(),
            pid: 40461,
            session_id: Some("s1".into()),
            cwd: Some("/w/checkout-app".into()),
            source,
            kind,
            text: text.map(str::to_string),
        }
    }

    fn key() -> SessionKey {
        SessionKey { account: "second".into(), pid: 40461 }
    }

    fn file(status: &str, secs: i64) -> SessionFile {
        SessionFile {
            pid: 40461,
            session_id: Some("s1".into()),
            cwd: Some("/w/checkout-app".into()),
            name: Some("checkout-app".into()),
            status: Some(status.into()),
            started_at: None,
            updated_at: Some((1_790_000_000 + secs) * 1000),
            status_updated_at: Some((1_790_000_000 + secs) * 1000),
        }
    }

    #[test]
    fn a_waiting_report_survives_the_stop_hook_until_the_next_prompt() {
        let mut board = Board::default();
        board.apply_event(&ev(0, Source::Hook, Kind::Prompt, Some("Baue Event-Store")));
        board.apply_event(&ev(5, Source::Report, Kind::Doing, Some("Migrationen")));
        assert_eq!(board.get(&key()).unwrap().phase(), Phase::Working);
        assert_eq!(board.get(&key()).unwrap().headline().as_deref(), Some("Migrationen"));

        board.apply_event(&ev(10, Source::Report, Kind::Waiting, Some("Postgres oder SQLite?")));
        board.apply_event(&ev(11, Source::Hook, Kind::Stop, Some("Ich brauche eine Entscheidung …")));
        let s = board.get(&key()).unwrap();
        assert_eq!(s.phase(), Phase::NeedsYou);
        assert_eq!(s.headline().as_deref(), Some("Postgres oder SQLite?"));
        assert!(s.headline_is_reported());

        board.apply_event(&ev(20, Source::Hook, Kind::Prompt, Some("Postgres")));
        assert_eq!(board.get(&key()).unwrap().phase(), Phase::Working);
    }

    #[test]
    fn stop_without_question_is_your_turn_with_done_text_preferred() {
        let mut board = Board::default();
        board.apply_event(&ev(0, Source::Hook, Kind::Prompt, Some("Export bauen")));
        board.apply_event(&ev(5, Source::Hook, Kind::Stop, Some("Langer Abschlusstext")));
        assert_eq!(board.get(&key()).unwrap().headline().as_deref(), Some("Langer Abschlusstext"));

        board.apply_event(&ev(6, Source::Report, Kind::Done, Some("SVG-Export fertig")));
        let s = board.get(&key()).unwrap();
        assert_eq!(s.phase(), Phase::YourTurn);
        assert_eq!(s.headline().as_deref(), Some("SVG-Export fertig"));
    }

    #[test]
    fn the_newer_of_hook_and_session_file_decides_the_phase() {
        let mut board = Board::default();
        board.apply_event(&ev(0, Source::Hook, Kind::Stop, None));
        board.apply_session_file("second", &file("busy", 3), true);
        assert_eq!(board.get(&key()).unwrap().phase(), Phase::Working);

        board.apply_event(&ev(7, Source::Hook, Kind::Permission, Some("Claude needs your permission to use Bash")));
        let s = board.get(&key()).unwrap();
        assert_eq!(s.phase(), Phase::NeedsYou);
        assert_eq!(s.headline().as_deref(), Some("Claude needs your permission to use Bash"));
        assert_eq!(s.phase_since_ms(), (1_790_000_000 + 7) * 1000);
    }

    #[test]
    fn idle_reminder_notifications_do_not_turn_a_finished_session_red() {
        let mut board = Board::default();
        board.apply_event(&ev(0, Source::Hook, Kind::Prompt, None));
        board.apply_event(&ev(5, Source::Hook, Kind::Stop, Some("Fertig")));
        board.apply_event(&ev(65, Source::Hook, Kind::Permission, Some("Claude is waiting for your input")));

        let s = board.get(&key()).unwrap();
        assert_eq!(s.phase(), Phase::YourTurn);
        assert_eq!(s.headline().as_deref(), Some("Fertig"));
        assert_eq!(s.timeline.len(), 3);
    }

    #[test]
    fn sessions_without_hooks_get_their_phase_from_the_file() {
        let mut board = Board::default();
        board.apply_session_file("second", &file("waiting", 0), true);
        assert_eq!(board.get(&key()).unwrap().phase(), Phase::NeedsYou);
        assert_eq!(board.get(&key()).unwrap().headline().as_deref(), Some("Wartet auf Freigabe"));

        board.apply_session_file("second", &file("idle", 1), false);
        assert_eq!(board.get(&key()).unwrap().phase(), Phase::Ended);
    }

    #[test]
    fn triage_order_puts_longest_waiting_first() {
        let mut board = Board::default();
        let mk = |pid: u32, status: &str, secs: i64| SessionFile {
            pid,
            name: Some(format!("s{pid}")),
            ..file(status, secs)
        };
        board.apply_session_file("main", &mk(1, "busy", 0), true);
        board.apply_session_file("main", &mk(2, "idle", 50), true);
        board.apply_session_file("main", &mk(3, "idle", 10), true);
        board.apply_session_file("main", &mk(4, "waiting", 60), true);
        board.apply_session_file("main", &mk(5, "idle", 0), false);

        let pids: Vec<u32> = board.sorted().iter().map(|s| s.key.pid).collect();

        assert_eq!(pids, vec![4, 3, 2, 1, 5]);
    }
}
