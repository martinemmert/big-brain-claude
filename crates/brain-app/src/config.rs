//! Settings from `~/.claude-brain/config.json` (all optional).

use serde_json::Value;

fn read() -> Option<Value> {
    let path = brain_core::account::home_dir().join(".claude-brain/config.json");
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// How long a session may keep waiting before Brain reminds you again: `remind_after_minutes`,
/// 10 by default, 0 turns reminders off. Read on every call, so edits apply without a restart.
pub fn remind_after_ms() -> Option<i64> {
    let minutes = read().and_then(|v| v.get("remind_after_minutes")?.as_i64()).unwrap_or(10);
    (minutes > 0).then_some(minutes * 60 * 1000)
}
