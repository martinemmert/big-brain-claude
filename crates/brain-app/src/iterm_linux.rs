//! iTerm2 exists only on macOS; Brain falls back to its own font and colours.

pub fn font() -> Option<(String, f32)> {
    None
}

pub fn color(_key: &str) -> Option<(f32, f32, f32)> {
    None
}
