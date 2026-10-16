//! What the user chose in Brain and keeps across restarts: pinned and muted sessions, what the
//! user has seen and the like, in `~/.claude-brain/state.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use brain_core::state::SessionKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pinned: BTreeSet<String>,
    #[serde(default)]
    muted: BTreeSet<String>,
    /// Show Claude Code's background sessions (`claude --bg`); off by default.
    #[serde(default)]
    pub show_background: bool,
    /// Show sessions programs started (SDK reviews, `claude -p`); off by default.
    #[serde(default)]
    pub show_automated: bool,
    /// Width of the session list in points, as dragged.
    #[serde(default)]
    pub sidebar_width: Option<f32>,
    /// Session id → snoozed until (epoch ms).
    #[serde(default)]
    snoozed: BTreeMap<String, i64>,
    /// Session id → when it was hidden (epoch ms). New activity brings it back.
    #[serde(default)]
    hidden: BTreeMap<String, i64>,
    /// Session id → when the user last looked at it (epoch ms).
    #[serde(default)]
    seen: BTreeMap<String, i64>,
    /// The sessions that were finished when the list with a reading zone first started were
    /// marked as seen, once.
    #[serde(default)]
    seen_started: bool,
    /// Session id → when the user chose "only a report" for it (epoch ms).
    #[serde(default)]
    reports: BTreeMap<String, i64>,
}

/// Sessions are remembered by account and session id, so a pin outlives the process.
fn id(key: &SessionKey) -> String {
    format!("{}:{}", key.account, key.id)
}

impl Prefs {
    fn path() -> PathBuf {
        brain_core::account::home_dir().join(".claude-brain/state.json")
    }

    pub fn load() -> Self {
        if crate::demo::enabled() {
            return Self::default();
        }
        std::fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes the choices back; the demo never touches the user's real file.
    pub fn save(&self) {
        if crate::demo::enabled() {
            return;
        }
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Every pinned or saved session (`account:session id`).
    pub fn pinned_keys(&self) -> Vec<SessionKey> {
        self.pinned
            .iter()
            .filter_map(|entry| entry.split_once(':'))
            .map(|(account, id)| SessionKey { account: account.to_string(), id: id.to_string() })
            .collect()
    }

    pub fn is_pinned(&self, key: &SessionKey) -> bool {
        self.pinned.contains(&id(key))
    }

    pub fn is_muted(&self, key: &SessionKey) -> bool {
        self.muted.contains(&id(key))
    }

    /// Returns the new state.
    pub fn toggle_pin(&mut self, key: &SessionKey) -> bool {
        toggle(&mut self.pinned, id(key))
    }

    pub fn toggle_mute(&mut self, key: &SessionKey) -> bool {
        toggle(&mut self.muted, id(key))
    }

    /// Until when the session is snoozed, if that is still in the future.
    pub fn snoozed_until(&self, key: &SessionKey, now_ms: i64) -> Option<i64> {
        self.snoozed.get(&id(key)).copied().filter(|until| *until > now_ms)
    }

    /// Brain 0.5 remembered sessions by account and pid. Entries of running sessions move to
    /// their session id; the rest no longer point anywhere and are dropped. Returns whether
    /// anything changed.
    pub fn migrate_pid_entries(&mut self, live: impl Fn(&str, u32) -> Option<SessionKey>) -> bool {
        let old = |entry: &str| entry.split_once(':').and_then(|(account, pid)| Some((account.to_string(), pid.parse::<u32>().ok()?)));
        let renamed = |entry: &str| old(entry).map(|(account, pid)| live(&account, pid).map(|key| id(&key)));
        let mut changed = false;
        for set in [&mut self.pinned, &mut self.muted] {
            for entry in set.iter().filter(|e| old(e).is_some()).cloned().collect::<Vec<_>>() {
                set.remove(&entry);
                set.extend(renamed(&entry).flatten());
                changed = true;
            }
        }
        for entry in self.snoozed.keys().filter(|e| old(e).is_some()).cloned().collect::<Vec<_>>() {
            let until = self.snoozed.remove(&entry).unwrap_or_default();
            if let Some(Some(new)) = renamed(&entry) {
                self.snoozed.entry(new).or_insert(until);
            }
            changed = true;
        }
        changed
    }

    /// Hidden, and nothing happened in the session since.
    pub fn is_hidden(&self, key: &SessionKey, last_activity_ms: i64) -> bool {
        self.hidden.get(&id(key)).is_some_and(|at| *at >= last_activity_ms)
    }

    pub fn hide(&mut self, key: &SessionKey, now_ms: i64) {
        self.hidden.insert(id(key), now_ms);
    }

    pub fn unhide(&mut self, key: &SessionKey) {
        self.hidden.remove(&id(key));
    }

    /// Whether the user looked at the session since `since_ms` (when its turn ended).
    pub fn is_seen(&self, key: &SessionKey, since_ms: i64) -> bool {
        self.seen.get(&id(key)).is_some_and(|at| *at >= since_ms)
    }

    /// Notes that the user looks at the session now. Returns whether anything changed, so not
    /// every refresh writes the file.
    pub fn see(&mut self, key: &SessionKey, now_ms: i64, since_ms: i64) -> bool {
        if self.is_seen(key, since_ms) {
            return false;
        }
        self.seen.insert(id(key), now_ms);
        true
    }

    /// Whether the user chose "only a report" since `since_ms`.
    /// The first start of the list with a reading zone: the finished sessions there count as
    /// seen, so "Zum Lesen" starts empty. Only once. Returns whether anything changed.
    pub fn start_seen(&mut self, finished: impl IntoIterator<Item = (SessionKey, i64)>, now_ms: i64) -> bool {
        if self.seen_started {
            return false;
        }
        self.seen_started = true;
        for (key, since_ms) in finished {
            self.see(&key, now_ms, since_ms);
        }
        true
    }

    pub fn is_report(&self, key: &SessionKey, since_ms: i64) -> bool {
        self.reports.get(&id(key)).is_some_and(|at| *at >= since_ms)
    }

    pub fn mark_report(&mut self, key: &SessionKey, now_ms: i64) {
        self.reports.insert(id(key), now_ms);
    }

    /// Drops everything remembered about a session that no longer exists.
    pub fn forget(&mut self, key: &SessionKey) {
        let id = id(key);
        self.pinned.remove(&id);
        self.muted.remove(&id);
        self.snoozed.remove(&id);
        self.hidden.remove(&id);
        self.seen.remove(&id);
        self.reports.remove(&id);
    }

    pub fn snooze(&mut self, key: &SessionKey, until: Option<i64>) {
        match until {
            Some(until) => self.snoozed.insert(id(key), until),
            None => self.snoozed.remove(&id(key)),
        };
    }
}

fn toggle(set: &mut BTreeSet<String>, id: String) -> bool {
    if set.remove(&id) {
        false
    } else {
        set.insert(id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_session_comes_back_with_new_activity() {
        let mut prefs = Prefs::default();
        let key = SessionKey { account: "main".into(), id: "abc".into() };
        prefs.hide(&key, 1_000);
        assert!(prefs.is_hidden(&key, 900));
        assert!(!prefs.is_hidden(&key, 1_500));
    }

    #[test]
    fn a_session_is_read_only_when_seen_after_its_turn_ended() {
        let mut prefs = Prefs::default();
        let key = SessionKey { account: "main".into(), id: "abc".into() };
        assert!(!prefs.is_seen(&key, 1_000));
        assert!(prefs.see(&key, 1_200, 1_000));
        assert!(prefs.is_seen(&key, 1_000));
        assert!(!prefs.see(&key, 1_300, 1_000), "nothing to write twice");
        assert!(!prefs.is_seen(&key, 2_000), "the next turn end makes it unread again");

        assert!(!prefs.is_report(&key, 1_000));
        prefs.mark_report(&key, 1_100);
        assert!(prefs.is_report(&key, 1_000));
        assert!(!prefs.is_report(&key, 2_000));
        prefs.forget(&key);
        assert!(!prefs.is_seen(&key, 0) && !prefs.is_report(&key, 0));
    }

    #[test]
    fn the_first_start_marks_the_finished_sessions_as_seen_once() {
        let mut prefs: Prefs = serde_json::from_str(r#"{"pinned":["main:abc"]}"#).unwrap();
        let key = SessionKey { account: "main".into(), id: "abc".into() };
        assert!(prefs.start_seen([(key.clone(), 1_000)], 2_000));
        assert!(prefs.is_seen(&key, 1_000));
        let later = SessionKey { account: "main".into(), id: "new".into() };
        assert!(!prefs.start_seen([(later.clone(), 3_000)], 4_000));
        assert!(!prefs.is_seen(&later, 3_000));
    }

    #[test]
    fn an_old_state_file_with_a_layout_still_loads_with_its_pins() {
        let prefs: Prefs = serde_json::from_str(r#"{"pinned":["main:abc"],"layout":"projects","show_background":true}"#).unwrap();
        assert!(prefs.is_pinned(&SessionKey { account: "main".into(), id: "abc".into() }));
        assert!(prefs.show_background);
    }

    #[test]
    fn pid_entries_move_to_the_running_session_or_are_dropped() {
        let mut prefs = Prefs::default();
        prefs.pinned.extend(["main:4711".to_string(), "main:9".to_string(), "main:abc-1".to_string()]);
        prefs.snoozed.insert("main:4711".into(), 5);
        let live = |account: &str, pid: u32| (pid == 4711).then(|| SessionKey { account: account.into(), id: "abc-2".into() });

        assert!(prefs.migrate_pid_entries(live));
        assert_eq!(prefs.pinned.iter().map(String::as_str).collect::<Vec<_>>(), vec!["main:abc-1", "main:abc-2"]);
        assert_eq!(prefs.snoozed.get("main:abc-2"), Some(&5));
        assert!(!prefs.migrate_pid_entries(live));
    }
}
