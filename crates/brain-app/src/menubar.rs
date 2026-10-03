//! A menu bar item with the number of sessions waiting for you; a click brings Brain forward.
//! GPUI has no status item API, so this talks to AppKit directly.

use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

static CLICKED: AtomicBool = AtomicBool::new(false);

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BrainStatusItemTarget"]
    #[ivars = ()]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(clicked:))]
        fn clicked(&self, _sender: Option<&AnyObject>) {
            CLICKED.store(true, Ordering::SeqCst);
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

pub struct MenuBar {
    item: Retained<NSStatusItem>,
    _target: Retained<Target>,
    mtm: MainThreadMarker,
    title: String,
}

impl MenuBar {
    /// `None` off the main thread.
    pub fn new() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let target = Target::new(mtm);
        if let Some(button) = item.button(mtm) {
            let target_object: &AnyObject = &target;
            // SAFETY: `target` lives as long as the item (kept in `_target`) and answers `clicked:`.
            unsafe {
                button.setTarget(Some(target_object));
                button.setAction(Some(sel!(clicked:)));
            }
        }
        let mut bar = Self { item, _target: target, mtm, title: String::new() };
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

    /// Whether the item was clicked since the last call.
    pub fn take_click() -> bool {
        CLICKED.swap(false, Ordering::SeqCst)
    }
}
