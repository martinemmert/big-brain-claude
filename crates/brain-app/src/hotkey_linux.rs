//! No global hotkey on Linux yet (Wayland leaves it to the compositor).

pub fn register(_spec: &str) -> bool {
    false
}

pub fn presses() -> impl iced::futures::Stream<Item = ()> {
    iced::futures::stream::pending()
}
