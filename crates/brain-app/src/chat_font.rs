//! The chat history's font: the one your terminal uses, so Brain's conversation reads like the
//! session itself. Order: `chat_font` / `chat_font_size` in config.json, then the font of
//! iTerm2's default profile, then Menlo 13.

use std::sync::OnceLock;

use iced::{font, Font};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, AnyThread};
use objc2_app_kit::NSFont;
use objc2_foundation::{NSString, NSUserDefaults};

pub struct ChatFont {
    pub regular: Font,
    pub bold: Font,
    pub size: f32,
}

static CHAT_FONT: OnceLock<ChatFont> = OnceLock::new();

/// Resolves the font once, at start.
pub fn init() {
    let (family, size) = configured().or_else(iterm).unwrap_or_else(|| ("Menlo".to_string(), 13.0));
    // Iced names fonts by `&'static str`; the family is chosen once per run.
    let family: &'static str = Box::leak(family.into_boxed_str());
    let regular = Font::with_name(family);
    let _ = CHAT_FONT.set(ChatFont { regular, bold: Font { weight: font::Weight::Bold, ..regular }, size });
}

pub fn get() -> &'static ChatFont {
    CHAT_FONT.get_or_init(|| ChatFont { regular: Font::with_name("Menlo"), bold: Font { weight: font::Weight::Bold, ..Font::with_name("Menlo") }, size: 13.0 })
}

fn configured() -> Option<(String, f32)> {
    let family = crate::config::chat_font()?;
    Some((family, crate::config::chat_font_size().unwrap_or(13.0)))
}

/// The family and size of iTerm2's default profile font, e.g. `JetBrainsMono-Regular 13` →
/// ("JetBrains Mono", 13).
fn iterm() -> Option<(String, f32)> {
    let ns = NSString::from_str;
    let defaults = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&ns("com.googlecode.iterm2")))?;
    let default_guid = defaults.stringForKey(&ns("Default Bookmark Guid"));
    let profiles = defaults.arrayForKey(&ns("New Bookmarks"))?;
    let font_of = |profile: &AnyObject| -> Option<String> {
        // SAFETY: iTerm2 stores each profile as a dictionary of strings and numbers.
        let font: Option<Retained<NSString>> = unsafe { msg_send![profile, objectForKey: &*ns("Normal Font")] };
        font.map(|f| f.to_string())
    };
    let guid_of = |profile: &AnyObject| -> Option<String> {
        // SAFETY: as above.
        let guid: Option<Retained<NSString>> = unsafe { msg_send![profile, objectForKey: &*ns("Guid")] };
        guid.map(|g| g.to_string())
    };
    let chosen = profiles
        .iter()
        .find(|p| default_guid.as_ref().is_some_and(|g| guid_of(p).as_deref() == Some(&g.to_string())))
        .or_else(|| profiles.iter().next())?;
    let spec = font_of(&chosen)?;
    let (postscript, size) = spec.rsplit_once(' ')?;
    let size: f32 = size.parse().ok()?;
    let family = NSFont::fontWithName_size(&ns(postscript), size as f64)?.familyName()?.to_string();
    Some((family, size))
}
