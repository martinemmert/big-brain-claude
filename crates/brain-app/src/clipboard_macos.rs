//! The clipboard through GPUI (NSPasteboard).

use gpui::{App, ClipboardItem};

pub fn copy(text: &str, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
}
