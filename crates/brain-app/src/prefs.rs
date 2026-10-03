//! What the user chose in Brain and keeps across restarts: pinned and muted sessions and the
//! list layout, in `~/.claude-brain/state.json`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use brain_core::state::SessionKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pinned: BTreeSet<String>,
    #[serde(default)]
    muted: BTreeSet<String>,
    #[serde(default)]
    pub group_by_project: bool,
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
}

fn toggle(set: &mut BTreeSet<String>, id: String) -> bool {
    if set.remove(&id) {
        false
    } else {
        set.insert(id);
        true
    }
}
