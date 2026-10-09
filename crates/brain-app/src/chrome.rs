//! The window's title bar. Iced draws the whole window; an empty unified toolbar gives it the
//! standard tall macOS title bar, with the traffic lights centred in Brain's own title row.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSToolbar, NSWindowStyleMask, NSWindowToolbarStyle};

/// Height of the unified title bar the toolbar creates, in points.
pub const TITLEBAR_HEIGHT: f32 = 52.0;

/// Gives Brain's window the unified title bar. Returns false while the window doesn't exist yet.
pub fn unified_titlebar() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    // The menu bar item has a window of its own; Brain's is the one with a title bar.
    let windows = NSApplication::sharedApplication(mtm).windows();
    let Some(window) = windows.iter().find(|w| w.styleMask().contains(NSWindowStyleMask::Titled)) else { return false };
    if window.toolbar().is_none() {
        let toolbar = NSToolbar::new(mtm);
        window.setToolbar(Some(&toolbar));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
    }
    true
}
