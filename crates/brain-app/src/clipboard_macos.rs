//! Pictures on the clipboard (a screenshot copied with ⌃⇧⌘4) saved as files, so they can be
//! pasted into a terminal as a path.

use objc2_app_kit::NSPasteboard;
use objc2_foundation::NSString;

/// Saves a PNG on the clipboard to `~/.claude-brain/attachments/` and returns its path; `None`
/// when the clipboard holds no picture (or also text, which the terminal pastes itself).
pub fn save_image() -> Option<std::path::PathBuf> {
    let pasteboard = NSPasteboard::generalPasteboard();
    if pasteboard.stringForType(&NSString::from_str("public.utf8-plain-text")).is_some() {
        return None;
    }
    let data = pasteboard.dataForType(&NSString::from_str("public.png"))?;
    let dir = brain_core::account::home_dir().join(".claude-brain/attachments");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("paste-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::write(&path, data.to_vec()).ok()?;
    Some(path)
}
