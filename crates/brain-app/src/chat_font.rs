//! The chat history's font: the one your terminal uses, so Brain's conversation reads like the
//! session itself. Order: `chat_font` / `chat_font_size` in config.json, then the font of
//! iTerm2's default profile, then Menlo 13 (Linux: system monospace).

use std::sync::OnceLock;

use iced::{font, Font};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSFont;
#[cfg(target_os = "macos")]
use objc2_foundation::NSString;

pub struct ChatFont {
    pub regular: Font,
    pub bold: Font,
    /// Code spans and blocks: the same font in the chat, Menlo in prose drawn in the system font.
    pub code: Font,
    pub size: f32,
}

static CHAT_FONT: OnceLock<ChatFont> = OnceLock::new();

/// Resolves the font once, at start.
pub fn init() {
    let (regular, size) = match configured().or_else(iterm) {
        // Iced names fonts by `&'static str`; the family is chosen once per run.
        Some((family, size)) => (Font::with_name(Box::leak(family.into_boxed_str())), size),
        None => (crate::style::MONO, 13.0),
    };
    let _ = CHAT_FONT.set(ChatFont { regular, bold: Font { weight: font::Weight::Bold, ..regular }, code: regular, size });
}

pub fn get() -> &'static ChatFont {
    let mono = crate::style::MONO;
    CHAT_FONT.get_or_init(|| ChatFont { regular: mono, bold: Font { weight: font::Weight::Bold, ..mono }, code: mono, size: 13.0 })
}

fn configured() -> Option<(String, f32)> {
    let family = crate::config::chat_font()?;
    Some((family, crate::config::chat_font_size().unwrap_or(13.0)))
}

/// The family and size of iTerm2's default profile font, e.g. `JetBrainsMono-Regular 13` →
/// ("JetBrains Mono", 13).
#[cfg(target_os = "macos")]
fn iterm() -> Option<(String, f32)> {
    let (postscript, size) = crate::iterm::font()?;
    let family = NSFont::fontWithName_size(&NSString::from_str(&postscript), size as f64)?.familyName()?.to_string();
    Some((family, size))
}

#[cfg(not(target_os = "macos"))]
fn iterm() -> Option<(String, f32)> {
    crate::iterm::font()
}
