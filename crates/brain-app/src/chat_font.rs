//! The chat history's font: the one your terminal uses, so Brain's conversation reads like the
//! session itself. Order: `chat_font` / `chat_font_size` in config.json, then the font of
//! iTerm2's default profile, then Menlo 13.

use std::sync::OnceLock;

use iced::{font, Font};
use objc2_app_kit::NSFont;
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
    let (family, size) = configured().or_else(iterm).unwrap_or_else(|| ("Menlo".to_string(), 13.0));
    // Iced names fonts by `&'static str`; the family is chosen once per run.
    let family: &'static str = Box::leak(family.into_boxed_str());
    let regular = Font::with_name(family);
    let _ = CHAT_FONT.set(ChatFont { regular, bold: Font { weight: font::Weight::Bold, ..regular }, code: regular, size });
}

pub fn get() -> &'static ChatFont {
    CHAT_FONT.get_or_init(|| ChatFont { regular: Font::with_name("Menlo"), bold: Font { weight: font::Weight::Bold, ..Font::with_name("Menlo") }, code: Font::with_name("Menlo"), size: 13.0 })
}

fn configured() -> Option<(String, f32)> {
    let family = crate::config::chat_font()?;
    Some((family, crate::config::chat_font_size().unwrap_or(13.0)))
}

/// The family and size of iTerm2's default profile font, e.g. `JetBrainsMono-Regular 13` →
/// ("JetBrains Mono", 13).
fn iterm() -> Option<(String, f32)> {
    let (postscript, size) = crate::iterm::font()?;
    let family = NSFont::fontWithName_size(&NSString::from_str(&postscript), size as f64)?.familyName()?.to_string();
    Some((family, size))
}
