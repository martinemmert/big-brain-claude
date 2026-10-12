//! The quick-terminal hotkey. Linux has no global hotkey an app can claim on every desktop
//! (Wayland leaves it to the compositor), so it is not offered there yet.

/// Always false: the hotkey can't be registered.
pub fn register(_spec: &str) -> bool {
    false
}

/// Presses of the hotkey: none.
pub fn presses() -> impl iced::futures::Stream<Item = ()> {
    iced::futures::stream::pending()
}
