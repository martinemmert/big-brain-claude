//! No iTerm2 on Linux.

pub fn font() -> Option<(String, f32)> {
    None
}

pub fn color(_key: &str) -> Option<(f32, f32, f32)> {
    None
}
