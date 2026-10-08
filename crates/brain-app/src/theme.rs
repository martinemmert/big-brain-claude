//! Brain's palette: ink-blue surfaces, four signal colours that mean exactly one
//! thing each (calls you, your turn, working, done), and per-account hues.

use gpui::{rgb, rgba, Rgba};

pub const MONO: &str = if cfg!(target_os = "macos") { "Menlo" } else { "DejaVu Sans Mono" };

// Surfaces, darkest to lightest.
pub fn ink() -> Rgba { rgb(0x0f1322) }
pub fn chrome() -> Rgba { rgb(0x121728) }
pub fn surface() -> Rgba { rgb(0x161b2c) }
pub fn raised() -> Rgba { rgb(0x1d2338) }
pub fn hover() -> Rgba { rgb(0x1a2033) }
pub fn line() -> Rgba { rgb(0x232a40) }
pub fn line_strong() -> Rgba { rgb(0x2e3654) }

// Text.
pub fn text() -> Rgba { rgb(0xd5d9e6) }
pub fn text_strong() -> Rgba { rgb(0xf3f4f9) }
pub fn text_muted() -> Rgba { rgb(0x8c93aa) }
pub fn text_faint() -> Rgba { rgb(0x5f6782) }

// Signals.
pub fn calls() -> Rgba { rgb(0xff5c6c) }
pub fn calls_soft() -> Rgba { rgb(0xff8f9a) }
pub fn turn() -> Rgba { rgb(0xf2b84b) }
pub fn working() -> Rgba { rgb(0x5aa9ff) }
/// Working, but only in the background (subagents, shells): a quieter blue.
pub fn background() -> Rgba { rgb(0x8f9cf5) }
pub fn done() -> Rgba { rgb(0x45d19a) }
pub fn ended() -> Rgba { rgb(0x4a5272) }

/// `color` at the given alpha (0–255).
pub fn alpha(color: Rgba, a: u8) -> Rgba {
    Rgba { a: a as f32 / 255.0, ..color }
}

/// Background and text colour of an account badge: main is blue, then violet, teal, rose.
pub fn account_colors(index: usize) -> (Rgba, Rgba) {
    const PALETTE: [(u32, u32); 4] = [
        (0x5aa9ff24, 0x9ccaff),
        (0xa78bfa26, 0xc9b8ff),
        (0x2dd4bf24, 0x86eadb),
        (0xfb718524, 0xffa8b8),
    ];
    let (bg, fg) = PALETTE[index % PALETTE.len()];
    (rgba(bg), rgb(fg))
}

/// "now", "4 min", "2 h", "3 d"
pub fn ago(ms: i64, now_ms: i64) -> String {
    let secs = ((now_ms - ms) / 1000).max(0);
    match secs {
        0..=59 => crate::i18n::t("gerade", "now").into(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

/// "5 min ago", or "just now" in the first minute.
pub fn ago_phrase(ms: i64, now_ms: i64) -> String {
    if now_ms - ms < 60_000 {
        return crate::i18n::t("gerade eben", "just now").into();
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
