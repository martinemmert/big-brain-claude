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
}

/// Sessions are remembered by account and pid: stable for the life of a session.
fn id(key: &SessionKey) -> String {
    format!("{}:{}", key.account, key.pid)
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
