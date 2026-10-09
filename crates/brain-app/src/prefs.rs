//! What the user chose in Brain and keeps across restarts: pinned and muted sessions and the
//! list layout, in `~/.claude-brain/state.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use brain_core::state::SessionKey;
use serde::{Deserialize, Serialize};

/// How the list is arranged: by state, by project, or the day's digest.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Status,
    Projects,
    Today,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pinned: BTreeSet<String>,
    #[serde(default)]
    muted: BTreeSet<String>,
    #[serde(default)]
    pub layout: Layout,
    /// Session id → snoozed until (epoch ms).
    #[serde(default)]
    snoozed: BTreeMap<String, i64>,
    /// Session id → when it was hidden (epoch ms). New activity brings it back.
    #[serde(default)]
    hidden: BTreeMap<String, i64>,
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
        std::fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, json);
        }
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

    /// Drops everything remembered about a session that no longer exists.
    pub fn forget(&mut self, key: &SessionKey) {
        let id = id(key);
        self.pinned.remove(&id);
        self.muted.remove(&id);
        self.snoozed.remove(&id);
        self.hidden.remove(&id);
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
