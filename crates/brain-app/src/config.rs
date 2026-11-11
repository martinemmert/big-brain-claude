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

/// The quick-terminal hotkey (`quick_terminal`), e.g. "cmd+shift+b". Off unless set: a global
/// key overrides the same combination everywhere, including macOS' own shortcuts (⌃⌥Space
/// switches the input source), so only a key the user picked is taken.
pub fn quick_terminal() -> Option<String> {
    let spec = read().and_then(|v| v.get("quick_terminal")?.as_str().map(str::to_string))?;
    (!spec.trim().is_empty() && spec.trim() != "off").then_some(spec)
}

/// The chat history's font family (`chat_font`), e.g. "JetBrains Mono"; by default the
/// terminal's.
pub fn chat_font() -> Option<String> {
    read().and_then(|v| v.get("chat_font")?.as_str().map(str::to_string)).filter(|f| !f.trim().is_empty())
}

/// The chat history's font size (`chat_font_size`) in points.
pub fn chat_font_size() -> Option<f32> {
    read().and_then(|v| v.get("chat_font_size")?.as_f64()).map(|s| s.clamp(9.0, 24.0) as f32)
}

