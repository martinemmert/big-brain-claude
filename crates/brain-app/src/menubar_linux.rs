//! No menu bar item on Linux (yet); the window title bar shows the count.

pub struct MenuBar;

impl MenuBar {
    pub fn new() -> Option<Self> {
        None
    }

    pub fn show(&mut self, _waiting: usize, _calling: usize) {}

    pub fn take_click() -> bool {
        false
    }
}
