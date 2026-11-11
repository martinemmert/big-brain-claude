//! The macOS menu bar and the menu of the status item, built from plain descriptions.
//!
//! A description (`Menu`, `Entry`) knows nothing of AppKit and compares with `==`, so Brain
//! rebuilds a menu only when what it shows changed. An item either runs a standard AppKit action
//! (hide, quit, close the window: the responder chain answers) or carries an id; choosing it sends
//! that id through a channel, which a subscription turns into a message.

use std::ffi::CStr;
use std::sync::Mutex;

use iced::futures::channel::mpsc;
use iced::futures::SinkExt;
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

/// Modifier keys of a shortcut; ⌘ is `CMD`, ⌥ `OPT`, ⌃ `CTRL`, ⇧ `SHIFT`.
pub const CMD: u8 = 1;
pub const SHIFT: u8 = 2;
pub const OPT: u8 = 4;
pub const CTRL: u8 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// The character the key types, lowercase (`"n"`, `"+"`, `"\u{8}"` for ⌫, `"\t"` for ⇥).
    pub key: &'static str,
    pub modifiers: u8,
}

/// What choosing an item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Sends this id to Brain.
    Brain(String),
    /// An AppKit action looked up along the responder chain (`hide:`, `performClose:` …).
    Standard(&'static CStr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub shortcut: Option<Shortcut>,
    pub action: Action,
    pub enabled: bool,
    pub checked: bool,
}

impl Item {
    /// An item that tells Brain `id`.
    pub fn new(title: impl Into<String>, id: impl Into<String>) -> Self {
        Self { title: title.into(), shortcut: None, action: Action::Brain(id.into()), enabled: true, checked: false }
    }

    /// An item that runs a standard AppKit action.
    pub fn standard(title: impl Into<String>, selector: &'static CStr) -> Self {
        Self { title: title.into(), shortcut: None, action: Action::Standard(selector), enabled: true, checked: false }
    }

    pub fn keys(mut self, key: &'static str, modifiers: u8) -> Self {
        self.shortcut = Some(Shortcut { key, modifiers });
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Separator,
    Item(Item),
    Submenu { title: String, enabled: bool, entries: Vec<Entry> },
}

impl From<Item> for Entry {
    fn from(item: Item) -> Self {
        Entry::Item(item)
    }
}

/// What macOS does with a menu of the bar besides showing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Plain,
    /// Lists the open windows at its end.
    Windows,
    /// Gets the search field of the Help menu.
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub title: String,
    pub role: Role,
    pub entries: Vec<Entry>,
}

/// Ids of the chosen items, from the open subscription's channel.
static PICKED: Mutex<Option<mpsc::UnboundedSender<String>>> = Mutex::new(None);

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BrainMenuTarget"]
    #[ivars = ()]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(pick:))]
        fn pick(&self, sender: &NSMenuItem) {
            let id = sender.representedObject().and_then(|object| object.downcast::<NSString>().ok());
            if let (Some(id), Some(sender)) = (id, PICKED.lock().ok().and_then(|s| s.clone())) {
                let _ = sender.unbounded_send(id.to_string());
            }
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// The ids of chosen items, as a subscription stream.
pub fn picks() -> impl iced::futures::Stream<Item = String> {
    iced::stream::channel(16, async |mut output| {
        let (sender, mut receiver) = mpsc::unbounded();
        if let Ok(mut slot) = PICKED.lock() {
            *slot = Some(sender);
        }
        use iced::futures::StreamExt;
        while let Some(id) = receiver.next().await {
            let _ = output.send(id).await;
        }
    })
}

/// The menu bar of the app as AppKit has it, and what it was last built from.
pub struct AppMenu {
    mtm: MainThreadMarker,
    target: Retained<Target>,
    /// One per menu of the bar, with the description it shows.
    built: Vec<(Retained<NSMenu>, Menu)>,
    bar: Option<Retained<NSMenu>>,
}

impl AppMenu {
    /// `None` off the main thread.
    pub fn new() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        Some(Self { mtm, target: Target::new(mtm), built: Vec::new(), bar: None })
    }

    /// Makes the bar show `menus`: replaces the bar when the menus themselves changed (or macOS
    /// put another one in its place), otherwise only the menus whose items differ.
    pub fn show(&mut self, menus: &[Menu]) {
        let app = NSApplication::sharedApplication(self.mtm);
        let ours = self.bar.as_ref().is_some_and(|bar| app.mainMenu().is_some_and(|main| std::ptr::eq(&*main, &**bar)));
        let same_menus = self.built.len() == menus.len() && self.built.iter().zip(menus).all(|((_, a), b)| a.title == b.title && a.role == b.role);
        if !ours || !same_menus {
            self.rebuild(&app, menus);
            return;
        }
        for ((menu, shown), wanted) in self.built.iter_mut().zip(menus) {
            if shown != wanted {
                menu.removeAllItems();
                fill(menu, &wanted.entries, &self.target, self.mtm);
                *shown = wanted.clone();
            }
        }
    }

    fn rebuild(&mut self, app: &NSApplication, menus: &[Menu]) {
        let bar = NSMenu::new(self.mtm);
        bar.setAutoenablesItems(false);
        self.built.clear();
        for menu in menus {
            let submenu = new_menu(&menu.title, self.mtm);
            fill(&submenu, &menu.entries, &self.target, self.mtm);
            let item = NSMenuItem::new(self.mtm);
            item.setTitle(&NSString::from_str(&menu.title));
            item.setSubmenu(Some(&submenu));
            bar.addItem(&item);
            match menu.role {
                Role::Windows => app.setWindowsMenu(Some(&submenu)),
                Role::Help => app.setHelpMenu(Some(&submenu)),
                Role::Plain => {}
            }
            self.built.push((submenu, menu.clone()));
        }
        app.setMainMenu(Some(&bar));
        self.bar = Some(bar);
    }
}

fn new_menu(title: &str, mtm: MainThreadMarker) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(title));
    menu.setAutoenablesItems(false);
    menu
}

fn fill(menu: &NSMenu, entries: &[Entry], target: &Target, mtm: MainThreadMarker) {
    for entry in entries {
        match entry {
            Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            Entry::Item(item) => menu.addItem(&menu_item(item, target, mtm)),
            Entry::Submenu { title, enabled, entries } => {
                let item = NSMenuItem::new(mtm);
                item.setTitle(&NSString::from_str(title));
                item.setEnabled(*enabled);
                let submenu = new_menu(title, mtm);
                fill(&submenu, entries, target, mtm);
                item.setSubmenu(Some(&submenu));
                menu.addItem(&item);
            }
        }
    }
}

fn menu_item(item: &Item, target: &Target, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let key = item.shortcut.as_ref().map_or("", |s| s.key);
    let (selector, id): (Sel, Option<&str>) = match &item.action {
        Action::Brain(id) => (sel!(pick:), Some(id)),
        Action::Standard(name) => (Sel::register(name), None),
    };
    // SAFETY: the selector is `pick:` on our target, or a standard action sent along the responder chain.
    let ns_item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str(&item.title), Some(selector), &NSString::from_str(key)) };
    if let Some(shortcut) = &item.shortcut {
        ns_item.setKeyEquivalentModifierMask(flags(shortcut.modifiers));
    }
    if let Some(id) = id {
        let object = NSString::from_str(id);
        // SAFETY: `target` is kept by the MenuBar for as long as its items exist; the object is an NSString.
        unsafe {
            ns_item.setTarget(Some(target));
            ns_item.setRepresentedObject(Some(&object));
        }
    }
    ns_item.setEnabled(item.enabled);
    ns_item.setState(if item.checked { NSControlStateValueOn } else { NSControlStateValueOff });
    ns_item
}

fn flags(modifiers: u8) -> NSEventModifierFlags {
    let mut flags = NSEventModifierFlags::empty();
    for (bit, flag) in [
        (CMD, NSEventModifierFlags::Command),
        (SHIFT, NSEventModifierFlags::Shift),
        (OPT, NSEventModifierFlags::Option),
        (CTRL, NSEventModifierFlags::Control),
    ] {
        if modifiers & bit != 0 {
            flags |= flag;
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_bits_become_appkit_flags() {
        let all = flags(CMD | SHIFT | OPT | CTRL);
        assert!(all.contains(NSEventModifierFlags::Command | NSEventModifierFlags::Shift | NSEventModifierFlags::Option | NSEventModifierFlags::Control));
        assert_eq!(flags(CMD | OPT), NSEventModifierFlags::Command | NSEventModifierFlags::Option);
        assert_eq!(flags(0), NSEventModifierFlags::empty());
    }
}
