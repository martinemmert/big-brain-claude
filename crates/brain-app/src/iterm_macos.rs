//! iTerm2's default profile: its font and colours, so Brain's chat and terminal look like the
//! terminal you already use.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, AnyThread};
use objc2_foundation::{NSString, NSUserDefaults};

/// The default profile's settings dictionary, or the first profile's.
fn profile() -> Option<Retained<AnyObject>> {
    let ns = NSString::from_str;
    let defaults = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&ns("com.googlecode.iterm2")))?;
    let default_guid = defaults.stringForKey(&ns("Default Bookmark Guid")).map(|g| g.to_string());
    let profiles = defaults.arrayForKey(&ns("New Bookmarks"))?;
    let guid_of = |profile: &AnyObject| -> Option<String> {
        // SAFETY: iTerm2 stores each profile as a dictionary of strings, numbers and dictionaries.
        let guid: Option<Retained<NSString>> = unsafe { msg_send![profile, objectForKey: &*ns("Guid")] };
        guid.map(|g| g.to_string())
    };
    let chosen = profiles.iter().find(|p| default_guid.is_some() && guid_of(p) == default_guid).or_else(|| profiles.iter().next())?;
    Some(chosen)
}

fn value(dict: &AnyObject, key: &str) -> Option<Retained<AnyObject>> {
    // SAFETY: `dict` is an NSDictionary (a profile or a colour).
    unsafe { msg_send![dict, objectForKey: &*NSString::from_str(key)] }
}

/// `Normal Font`, e.g. "JetBrainsMono-Regular 13": the PostScript name and the size.
pub fn font() -> Option<(String, f32)> {
    let profile = profile()?;
    let spec = value(&profile, "Normal Font")?;
    // SAFETY: the font is stored as a string.
    let spec: Retained<NSString> = unsafe { Retained::cast_unchecked(spec) };
    let spec = spec.to_string();
    let (postscript, size) = spec.rsplit_once(' ')?;
    Some((postscript.to_string(), size.parse().ok()?))
}

/// A profile colour (`Ansi 1 Color`, `Foreground Color` …) as 0–1 RGB.
pub fn color(key: &str) -> Option<(f32, f32, f32)> {
    let profile = profile()?;
    let color = value(&profile, key)?;
    let component = |name: &str| -> Option<f32> {
        let number = value(&color, name)?;
        // SAFETY: components are NSNumbers or numeric NSStrings; both answer `doubleValue`.
        let v: f64 = unsafe { msg_send![&*number, doubleValue] };
        Some(v as f32)
    };
    Some((component("Red Component")?, component("Green Component")?, component("Blue Component")?))
}
