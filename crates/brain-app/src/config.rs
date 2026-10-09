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

/// The chat history's font family (`chat_font`), e.g. "JetBrains Mono"; by default the
/// terminal's.
pub fn chat_font() -> Option<String> {
    read().and_then(|v| v.get("chat_font")?.as_str().map(str::to_string)).filter(|f| !f.trim().is_empty())
}

/// The chat history's font size (`chat_font_size`) in points.
pub fn chat_font_size() -> Option<f32> {
    read().and_then(|v| v.get("chat_font_size")?.as_f64()).map(|s| s.clamp(9.0, 24.0) as f32)
}

/// Canned replies for `T` then `1`–`9`: `quick_replies` in config.json, or a default set in the
/// UI language.
pub fn quick_replies() -> Vec<String> {
    let configured: Option<Vec<String>> = read()
        .and_then(|v| v.get("quick_replies")?.as_array().cloned())
        .map(|list| list.iter().filter_map(|r| r.as_str().map(str::to_string)).filter(|r| !r.trim().is_empty()).collect());
    if let Some(list) = configured.filter(|l| !l.is_empty()) {
        return list.into_iter().take(9).collect();
    }
    let defaults: [(&str, &str); 5] = [
        ("Weiter.", "Go on."),
        ("Ja, mach das.", "Yes, do that."),
        ("Lass die Tests laufen und melde das Ergebnis.", "Run the tests and report the result."),
        ("Committe die Änderungen.", "Commit the changes."),
        ("Fass kurz zusammen, wo du stehst.", "Summarise briefly where you are."),
    ];
    defaults.iter().map(|(de, en)| crate::i18n::t(de, en).to_string()).collect()
}
