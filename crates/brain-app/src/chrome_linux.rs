//! The window's title bar on Linux: the desktop draws its own frame, Brain's title row sits below.

/// Height of Brain's title row, in points.
pub const TITLEBAR_HEIGHT: f32 = 40.0;

/// Nothing to set up; the window is ready as is.
pub fn unified_titlebar() -> bool {
    true
}
