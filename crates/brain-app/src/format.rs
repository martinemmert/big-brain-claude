//! Text for the UI: times, previews, labels and short forms. Nothing here draws.

use brain_core::state::Phase;
use chrono::{Local, TimeZone, Utc};

use crate::i18n::t;

pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn clock(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// "now", "4 min", "2 h", "3 d"
pub fn ago(ms: i64, now_ms: i64) -> String {
    let secs = ((now_ms - ms) / 1000).max(0);
    match secs {
        0..=59 => t("gerade", "now").into(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

/// "5 min ago", or "just now" in the first minute.
pub fn ago_phrase(ms: i64, now_ms: i64) -> String {
    if now_ms - ms < 60_000 {
        return t("gerade eben", "just now").into();
    }
    let ago = ago(ms, now_ms);
    crate::tr!("vor {ago}", "{ago} ago")
}

/// Replaces the home directory with `~`.
pub fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

/// Drops Markdown emphasis and code markers for one-line previews, and words Claude Code's
/// notification texts (and Brain's own notes) in the UI's language.
pub fn plain(text: &str) -> String {
    let text = flatten_tables(&text.replace("**", "").replace('`', ""));
    if let Some(rest) = text.strip_prefix("Claude needs your permission") {
        let tool = rest.trim().strip_prefix("to use ").map(str::trim).filter(|t| !t.is_empty());
        return match tool {
            Some(tool) => crate::tr!("Braucht deine Freigabe für {tool}", "Needs your permission for {tool}"),
            None => t("Braucht deine Freigabe", "Needs your permission").into(),
        };
    }
    if text.starts_with("Claude is waiting for your input") {
        return t("Wartet auf deine Eingabe", "Waiting for your input").into();
    }
    // Harness prompts (subagent hand-backs, task notifications) are stored as `[note]`.
    if let Some(inner) = text.strip_prefix('[') {
        return note(inner.strip_suffix(']').unwrap_or(inner));
    }
    note(&text)
}

/// A Markdown table squashed into one line reads `| a | b | |---|---| | 1 | 2 |`: keep the cells,
/// drop the separator row, join with middle dots.
fn flatten_tables(text: &str) -> String {
    if !text.contains("|-") && !text.contains("| -") {
        return text.to_string();
    }
    text.split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty() && !cell.chars().all(|c| matches!(c, '-' | ':' | ' ')))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Brain's harness notes from `brain_core::transcript` in the UI language.
pub fn note(text: &str) -> String {
    use brain_core::transcript::{NOTE_AGENT_REPLY, NOTE_TASK, NOTE_TODOS};
    match text {
        NOTE_AGENT_REPLY => t("Rückmeldung eines Subagents", NOTE_AGENT_REPLY).into(),
        NOTE_TASK => t("Eine Hintergrund-Aufgabe meldet sich", NOTE_TASK).into(),
        NOTE_TODOS => t("Todo-Liste aktualisiert", NOTE_TODOS).into(),
        other => other.to_string(),
    }
}

/// The first `max` characters of a single line, with `…` when cut. Iced has no ellipsis of its
/// own, so long one-line texts are shortened before they are drawn.
pub fn clip(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let cut: String = line.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// "2 subagents, 1 shell running" from a stop's background tasks.
pub fn background_summary(tasks: &[brain_core::event::BackgroundTask]) -> String {
    let count = |kind: &str| tasks.iter().filter(|t| t.kind == kind).count();
    let (agents, shells) = (count("subagent"), count("shell"));
    let other = tasks.len() - agents - shells;
    let mut parts = Vec::new();
    if agents > 0 {
        parts.push(if agents == 1 { t("1 Subagent", "1 subagent").to_string() } else { crate::tr!("{agents} Subagents", "{agents} subagents") });
    }
    if shells > 0 {
        parts.push(if shells == 1 { t("1 Shell", "1 shell").to_string() } else { crate::tr!("{shells} Shells", "{shells} shells") });
    }
    if other > 0 {
        parts.push(if other == 1 { t("1 Task", "1 task").to_string() } else { crate::tr!("{other} Tasks", "{other} tasks") });
    }
    let list = parts.join(", ");
    if tasks.len() == 1 { crate::tr!("{list} läuft", "{list} running") } else { crate::tr!("{list} laufen", "{list} running") }
}

pub fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::NeedsYou => t("Wartet auf dich", "Waiting for you"),
        Phase::YourTurn => t("Fertig, du bist dran", "Done, your turn"),
        Phase::Working => t("Arbeitet", "Working"),
        Phase::Background => t("Arbeitet im Hintergrund", "Working in the background"),
        Phase::Ended => t("Beendet", "Ended"),
    }
}

/// `claude-opus-5-5` → `opus-5-5`, `claude-haiku-4-5-20251001` → `haiku-4-5`.
pub fn short_model(model: &str) -> String {
    let name = model.strip_prefix("claude-").unwrap_or(model);
    name.split('-').filter(|p| !(p.len() == 8 && p.chars().all(|c| c.is_ascii_digit()))).collect::<Vec<_>>().join("-")
}

/// 23012 → `23k`, 1_340_000 → `1.3M`.
pub fn format_tokens(tokens: u64) -> String {
    match tokens {
        0..=999 => tokens.to_string(),
        1_000..=999_999 => format!("{}k", (tokens + 500) / 1000),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

/// Single-quotes a word for `sh`.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_in_one_line_previews_keep_their_cells() {
        assert_eq!(flatten_tables("Plan: | a | b | |---|---| | 1 | 2 |"), "Plan: · a · b · 1 · 2");
        assert_eq!(flatten_tables("no | table here"), "no | table here");
    }

    #[test]
    fn model_names_and_token_counts_are_shortened() {
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "haiku-4-5");
        assert_eq!(short_model("claude-opus-5-5"), "opus-5-5");
        assert_eq!(format_tokens(23_012), "23k");
        assert_eq!(format_tokens(1_340_000), "1.3M");
        assert_eq!(format_tokens(812), "812");
    }

    #[test]
    fn long_lines_are_clipped_with_an_ellipsis() {
        assert_eq!(clip("short   text", 20), "short text");
        assert_eq!(clip("Ändere die Datei überall", 10), "Ändere di…");
    }
}
