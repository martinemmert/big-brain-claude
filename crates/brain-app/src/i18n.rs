//! UI language: German when the first preferred macOS language is German, English otherwise.
//! Overridable with `BRAIN_LANG=de|en` or `{"language": "de"|"en"}` in `~/.claude-brain/config.json`.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

static LANG: OnceLock<Lang> = OnceLock::new();

pub fn lang() -> Lang {
    *LANG.get_or_init(detect)
}

pub fn is_german() -> bool {
    lang() == Lang::De
}

/// Picks the German or English text.
pub fn t(de: &'static str, en: &'static str) -> &'static str {
    if is_german() { de } else { en }
}

/// `tr!("{n} warten", "{n} waiting")`: like `format!`, in the UI language.
#[macro_export]
macro_rules! tr {
    ($de:literal, $en:literal) => {
        if $crate::i18n::is_german() { format!($de) } else { format!($en) }
    };
}

fn detect() -> Lang {
    if let Some(lang) = std::env::var("BRAIN_LANG").ok().and_then(|v| parse(&v)) {
        return lang;
    }
    let config = brain_core::account::home_dir().join(".claude-brain/config.json");
    let configured = std::fs::read(config)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|v| v.get("language")?.as_str().and_then(parse));
    if let Some(lang) = configured {
        return lang;
    }
    system_language().unwrap_or(Lang::En)
}

fn parse(value: &str) -> Option<Lang> {
    match value.trim().to_lowercase().get(..2)? {
        "de" => Some(Lang::De),
        "en" => Some(Lang::En),
        _ => None,
    }
}

/// The first entry of macOS's preferred languages (`defaults read -g AppleLanguages`).
fn system_language() -> Option<Lang> {
    let out = std::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().map(|l| l.trim().trim_matches(|c| c == '"' || c == ',')).find(|l| l.len() >= 2 && l.chars().next().is_some_and(char::is_alphabetic))?;
    Some(parse(first).unwrap_or(Lang::En))
}
