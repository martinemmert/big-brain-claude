//! The window under the status item. Iced draws it (a second, borderless, transparent window); this
//! module does what Iced has no call for: put it under the item, show it above other apps without
//! bringing Brain forward, and notice a click outside.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventMask, NSPopUpMenuWindowLevel, NSWindow, NSWindowCollectionBehavior,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::ptr::NonNull;

/// Width of the popup in points.
pub const WIDTH: f32 = 360.0;

/// What AppKit calls the popup's window; the way to find it among Brain's windows.
pub const TITLE: &str = "Brain Status";

/// The id the monitors send through the menu channel when a click lands elsewhere.
pub const CLICK_OUTSIDE: &str = "status-popup:outside";

/// Settings of the popup's window: borderless, transparent, never shown by Iced itself.
pub fn window_settings() -> iced::window::Settings {
    iced::window::Settings {
        size: iced::Size::new(WIDTH, 120.0),
        visible: false,
        decorations: false,
        transparent: true,
        resizable: false,
        closeable: false,
        minimizable: false,
        level: iced::window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..iced::window::Settings::default()
    }
}

pub struct StatusPopup {
    mtm: MainThreadMarker,
    open: bool,
    /// Clicks in other apps, and clicks in Brain's other windows: both close the popup.
    monitors: Vec<Retained<AnyObject>>,
}

impl StatusPopup {
    /// `None` off the main thread.
    pub fn new() -> Option<Self> {
        Some(Self { mtm: MainThreadMarker::new()?, open: false, monitors: Vec::new() })
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn window(&self) -> Option<Retained<NSWindow>> {
        let title = NSString::from_str(TITLE);
        NSApplication::sharedApplication(self.mtm).windows().iter().find(|w| w.title().isEqualToString(&title))
    }

    /// Shows the popup `height` points high with its top edge under `anchor` (the status item's
    /// frame on screen), centred on it and kept inside the screen. Brain stays where it is: the
    /// window comes to the front of the other apps' windows, but the app isn't activated. Returns
    /// false while the window doesn't exist yet.
    pub fn show(&mut self, anchor: NSRect, height: f32, status_item_window: Option<Retained<NSWindow>>) -> bool {
        let Some(window) = self.window() else { return false };
        self.place(&window, anchor, height);
        window.setLevel(NSPopUpMenuWindowLevel);
        window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
        window.setHasShadow(true);
        window.setOpaque(false);
        if NSApplication::sharedApplication(self.mtm).isActive() {
            // Brain is in front anyway: the popup takes the keys, so esc closes it.
            window.makeKeyAndOrderFront(None);
        } else {
            window.orderFrontRegardless();
        }
        window.invalidateShadow();
        self.watch_clicks(status_item_window);
        self.open = true;
        true
    }

    /// Follows the content: same top edge, new height.
    pub fn resize(&mut self, height: f32) {
        let Some(window) = self.window() else { return };
        let frame = window.frame();
        let top = frame.origin.y + frame.size.height;
        let rect = NSRect::new(NSPoint::new(frame.origin.x, top - height as f64), NSSize::new(WIDTH as f64, height as f64));
        if (rect.size.height - frame.size.height).abs() > 0.5 {
            window.setFrame_display(rect, true);
            window.invalidateShadow();
        }
    }

    pub fn hide(&mut self) {
        if let Some(window) = self.window() {
            window.orderOut(None);
        }
        for monitor in self.monitors.drain(..) {
            // SAFETY: the object came from `addGlobalMonitor…` / `addLocalMonitor…`.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
        self.open = false;
    }

    fn place(&self, window: &NSWindow, anchor: NSRect, height: f32) {
        const GAP: f64 = 4.0;
        const MARGIN: f64 = 8.0;
        let width = WIDTH as f64;
        let mut x = anchor.origin.x + anchor.size.width / 2.0 - width / 2.0;
        if let Some(screen) = window.screen().or_else(|| objc2_app_kit::NSScreen::mainScreen(self.mtm)) {
            let visible = screen.visibleFrame();
            x = x.min(visible.origin.x + visible.size.width - width - MARGIN).max(visible.origin.x + MARGIN);
        }
        let top = anchor.origin.y - GAP;
        let rect = NSRect::new(NSPoint::new(x, top - height as f64), NSSize::new(width, height as f64));
        window.setFrame_display(rect, true);
    }

    /// A click anywhere but in the popup (and on the status item, whose own action toggles)
    /// reaches Brain as `CLICK_OUTSIDE`.
    fn watch_clicks(&mut self, status_item_window: Option<Retained<NSWindow>>) {
        if !self.monitors.is_empty() {
            return;
        }
        let mask = NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
        let outside = RcBlock::new(|_: NonNull<NSEvent>| crate::appmenu::send(CLICK_OUTSIDE));
        if let Some(monitor) = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &outside) {
            self.monitors.push(monitor);
        }
        let title = NSString::from_str(TITLE);
        let local = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands the monitor a live event.
            let event_ref = unsafe { event.as_ref() };
            let window = event_ref.window(MainThreadMarker::new().expect("event monitors run on the main thread"));
            let in_popup = window.as_ref().is_some_and(|w| w.title().isEqualToString(&title));
            let on_item = window.as_ref().zip(status_item_window.as_ref()).is_some_and(|(w, item)| std::ptr::eq(&**w, &**item));
            if !in_popup && !on_item {
                crate::appmenu::send(CLICK_OUTSIDE);
            }
            event.as_ptr()
        });
        // SAFETY: the block returns the event it was given, a valid pointer.
        if let Some(monitor) = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) } {
            self.monitors.push(monitor);
        }
    }
}
