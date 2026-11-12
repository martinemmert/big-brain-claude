//! A menu bar item with the number of sessions waiting for you; a click opens the popup under it
//! (see `popup`). GPUI has no status item API, so this talks to AppKit directly.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSStatusBar, NSStatusItem, NSVariableStatusItemLength, NSWindow};
use objc2_foundation::{NSRect, NSString};

use crate::appmenu::AppMenu;

pub struct MenuBar {
    item: Retained<NSStatusItem>,
    mtm: MainThreadMarker,
    title: String,
    connected: bool,
}

impl MenuBar {
    /// `None` off the main thread.
    pub fn new() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let mut bar = Self { item, mtm, title: String::new(), connected: false };
        bar.show(0, 0);
        Some(bar)
    }

    /// ● n when someone calls you, ◐ n when sessions only finished, ○ when nobody waits.
    pub fn show(&mut self, waiting: usize, calling: usize) {
        let title = match (waiting, calling) {
            (0, _) => "○".to_string(),
            (n, 0) => format!("◐ {n}"),
            (n, _) => format!("● {n}"),
        };
        if title == self.title {
            return;
        }
        if let Some(button) = self.item.button(self.mtm) {
            button.setTitle(&NSString::from_str(&title));
        }
        self.title = title;
    }

    /// Makes a click on the item reach Brain (once).
    pub fn connect(&mut self, menus: &AppMenu) {
        if self.connected {
            return;
        }
        if let Some(button) = self.item.button(self.mtm) {
            menus.connect_status_button(&button);
            self.connected = true;
        }
    }

    /// The item's frame on screen, and its window: what the popup hangs under.
    pub fn anchor(&self) -> Option<(NSRect, Retained<NSWindow>)> {
        let window = self.item.button(self.mtm)?.window()?;
        Some((window.frame(), window))
    }
}
