//! The quick-terminal hotkey: a system-wide key (⌃⌥Space by default) that brings Brain forward
//! with the waiting session's terminal. Carbon's `RegisterEventHotKey` is the macOS way to own a
//! global key without the accessibility permission; the declarations below follow the SDK's
//! HIToolbox headers (CarbonEvents.h, CarbonEventsCore.h, Events.h).

use std::ffi::c_void;
use std::sync::Mutex;

use iced::futures::channel::mpsc;
use iced::futures::SinkExt;

type OSStatus = i32;
type OSType = u32;
type EventTargetRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerUPP = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus;

#[repr(C)]
struct EventTypeSpec {
    event_class: OSType,
    event_kind: u32,
}

#[repr(C)]
struct EventHotKeyID {
    signature: OSType,
    id: u32,
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: EventHandlerUPP,
        num_types: usize,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut *mut c_void,
    ) -> OSStatus;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyID,
        target: EventTargetRef,
        options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> OSStatus;
}

const fn four_cc(code: &[u8; 4]) -> OSType {
    u32::from_be_bytes(*code)
}

const K_EVENT_CLASS_KEYBOARD: OSType = four_cc(b"keyb");
const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
// Modifier bits (Events.h): cmdKeyBit 8, shiftKeyBit 9, optionKeyBit 11, controlKeyBit 12.
const CMD: u32 = 1 << 8;
const SHIFT: u32 = 1 << 9;
const OPTION: u32 = 1 << 11;
const CONTROL: u32 = 1 << 12;
// Virtual key codes (Events.h, kVK_*).
const SPACE: u32 = 0x31;
const LETTERS: [(&str, u32); 26] = [
    ("a", 0x00), ("s", 0x01), ("d", 0x02), ("f", 0x03), ("h", 0x04), ("g", 0x05), ("z", 0x06),
    ("x", 0x07), ("c", 0x08), ("v", 0x09), ("b", 0x0b), ("q", 0x0c), ("w", 0x0d), ("e", 0x0e),
    ("r", 0x0f), ("y", 0x10), ("t", 0x11), ("o", 0x1f), ("u", 0x20), ("i", 0x22), ("p", 0x23),
    ("l", 0x25), ("j", 0x26), ("k", 0x28), ("n", 0x2d), ("m", 0x2e),
];

/// Where presses go: the open subscription's channel.
static PRESSES: Mutex<Option<mpsc::UnboundedSender<()>>> = Mutex::new(None);

extern "C" fn on_hot_key(_call: EventHandlerCallRef, _event: EventRef, _data: *mut c_void) -> OSStatus {
    if let Some(sender) = PRESSES.lock().ok().and_then(|s| s.clone()) {
        let _ = sender.unbounded_send(());
    }
    0
}

/// `"ctrl+option+space"`, `"cmd+shift+b"` …: Carbon modifiers and key code; `None` when the
/// text names no key or no modifier (a bare key can't be global).
pub fn parse(spec: &str) -> Option<(u32, u32)> {
    let mut modifiers = 0;
    let mut key = None;
    for part in spec.to_lowercase().split('+').map(str::trim) {
        match part {
            "ctrl" | "control" | "⌃" => modifiers |= CONTROL,
            "option" | "alt" | "opt" | "⌥" => modifiers |= OPTION,
            "cmd" | "command" | "⌘" => modifiers |= CMD,
            "shift" | "⇧" => modifiers |= SHIFT,
            "space" => key = Some(SPACE),
            letter => key = Some(LETTERS.iter().find(|(name, _)| *name == letter)?.1),
        }
    }
    (modifiers != 0).then_some((key?, modifiers))
}

/// Registers the hotkey once (on the main thread, at start). Returns false when the text names
/// no valid key or macOS refused it (another app owns the combination).
pub fn register(spec: &str) -> bool {
    let Some((key, modifiers)) = parse(spec) else { return false };
    let spec = EventTypeSpec { event_class: K_EVENT_CLASS_KEYBOARD, event_kind: K_EVENT_HOT_KEY_PRESSED };
    let mut hot_key: EventHotKeyRef = std::ptr::null_mut();
    // SAFETY: plain Carbon calls with valid pointers; the handler is a static function and the
    // hotkey stays registered for the life of the process.
    unsafe {
        let target = GetApplicationEventTarget();
        if InstallEventHandler(target, on_hot_key, 1, &spec, std::ptr::null_mut(), std::ptr::null_mut()) != 0 {
            return false;
        }
        RegisterEventHotKey(key, modifiers, EventHotKeyID { signature: four_cc(b"BRNq"), id: 1 }, target, 0, &mut hot_key) == 0
    }
}

/// Presses of the hotkey, as a subscription stream.
pub fn presses() -> impl iced::futures::Stream<Item = ()> {
    iced::stream::channel(8, async |mut output| {
        let (sender, mut receiver) = mpsc::unbounded();
        if let Ok(mut slot) = PRESSES.lock() {
            *slot = Some(sender);
        }
        use iced::futures::StreamExt;
        while receiver.next().await.is_some() {
            let _ = output.send(()).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_name_modifiers_and_a_key() {
        assert_eq!(parse("ctrl+option+space"), Some((SPACE, CONTROL | OPTION)));
        assert_eq!(parse("Cmd + Shift + B"), Some((0x0b, CMD | SHIFT)));
        assert_eq!(parse("space"), None, "a bare key would swallow typing everywhere");
        assert_eq!(parse("ctrl+ä"), None);
    }
}
