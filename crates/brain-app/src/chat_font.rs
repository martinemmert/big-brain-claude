//! The font of the terminals and the chat history: the one your terminal uses, so Brain's
//! conversation reads like the session itself. Family: `chat_font` in config.json, else the
//! font of iTerm2's default profile, else Menlo; size: `chat_font_size`, else iTerm2's, else 13.
//! The settings sheet changes it while Brain runs ([`apply`]).

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use iced::{font, Font};
use objc2_app_kit::{NSFont, NSFontManager, NSFontTraitMask};
use objc2_foundation::NSString;

#[derive(Debug, Clone, Copy)]
pub struct ChatFont {
    pub regular: Font,
    pub bold: Font,
    /// Code spans and blocks: the same font in the chat, Menlo in prose drawn in the system font.
    pub code: Font,
    pub size: f32,
}

static CHAT_FONT: RwLock<Option<ChatFont>> = RwLock::new(None);
/// Iced names fonts by `&'static str`: each family is kept once per run.
static NAMES: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);

const FALLBACK: (&str, f32) = ("Menlo", 13.0);

/// Resolves the font, at start.
pub fn init() {
    apply(crate::config::chat_font(), crate::config::chat_font_size());
}

/// Takes the configured family and size (`None`: the default for each) as the font from now on.
pub fn apply(family: Option<String>, size: Option<f32>) {
    let default = default_font();
    let family = family.unwrap_or(default.0);
    let size = size.unwrap_or(default.1);
    let regular = Font::with_name(keep(family));
    let chosen = ChatFont { regular, bold: Font { weight: font::Weight::Bold, ..regular }, code: regular, size };
    if let Ok(mut slot) = CHAT_FONT.write() {
        *slot = Some(chosen);
    }
}

fn keep(family: String) -> &'static str {
    let Ok(mut names) = NAMES.lock() else { return "Menlo" };
    let names = names.get_or_insert_with(HashMap::new);
    if let Some(name) = names.get(&family) {
        return name;
    }
    let name: &'static str = Box::leak(family.clone().into_boxed_str());
    names.insert(family, name);
    name
}

pub fn get() -> ChatFont {
    CHAT_FONT.read().ok().and_then(|f| *f).unwrap_or_else(|| {
        let regular = Font::with_name(FALLBACK.0);
        ChatFont { regular, bold: Font { weight: font::Weight::Bold, ..regular }, code: regular, size: FALLBACK.1 }
    })
}

/// The font without settings: iTerm2's default profile, else Menlo 13.
pub fn default_font() -> (String, f32) {
    iterm().unwrap_or_else(|| (FALLBACK.0.to_string(), FALLBACK.1))
}

/// The family and size of iTerm2's default profile font, e.g. `JetBrainsMono-Regular 13` →
/// ("JetBrains Mono", 13).
fn iterm() -> Option<(String, f32)> {
    let (postscript, size) = crate::iterm::font()?;
    let family = NSFont::fontWithName_size(&NSString::from_str(&postscript), size as f64)?.familyName()?.to_string();
    Some((family, size))
}

/// The installed font families with fixed-width letters, by name (on the main thread; empty off
/// it).
pub fn monospace_families() -> Vec<String> {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return Vec::new() };
    let manager = NSFontManager::sharedFontManager(mtm);
    let mut families: Vec<String> = manager
        .availableFontFamilies()
        .iter()
        .filter(|family| manager.fontWithFamily_traits_weight_size(family, NSFontTraitMask(0), 5, 13.0).is_some_and(|f| f.isFixedPitch()))
        .map(|family| family.to_string())
        .filter(|family| !family.starts_with('.'))
        .collect();
    families.sort_by_key(|f| f.to_lowercase());
    families.dedup();
    families
}
