//! A menu bar item with the number of sessions waiting for you; a click opens a menu with who
//! needs you, who finished, the usage of each account and a new session.
//! GPUI has no status item API, so this talks to AppKit directly.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::NSString;

use crate::appmenu::{AppMenu, Entry};

pub struct MenuBar {
    item: objc2::rc::Retained<NSStatusItem>,
    mtm: MainThreadMarker,
    title: String,
    /// What the item's menu lists now.
    entries: Vec<Entry>,
}

impl MenuBar {
    /// `None` off the main thread.
    pub fn new() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let mut bar = Self { item, mtm, title: String::new(), entries: Vec::new() };
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

    /// Makes a click open this menu; builds it again only when its entries changed.
    pub fn show_menu(&mut self, entries: &[Entry], menus: &AppMenu) {
        if self.entries == entries {
            return;
        }
        self.item.setMenu(Some(&menus.menu(entries)));
        self.entries = entries.to_vec();
    }
}
