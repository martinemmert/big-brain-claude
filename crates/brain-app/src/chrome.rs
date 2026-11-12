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

/// The main window: a hidden title with transparent bar, content up to the top edge.
pub fn main_window_settings() -> iced::window::Settings {
    iced::window::Settings {
        size: iced::Size::new(1180.0, 760.0),
        min_size: Some(iced::Size::new(900.0, 520.0)),
        position: iced::window::Position::Centered,
        platform_specific: iced::window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        },
        ..iced::window::Settings::default()
    }
}
