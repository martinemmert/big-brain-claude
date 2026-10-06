//! On Wayland GPUI's copy needs a recent key press, so a click copies via the system tools.
//! GPUI keeps the text too, for pasting inside Brain.

use std::io::Write;
use std::process::{Command, Stdio};

use gpui::{App, ClipboardItem};

pub fn copy(text: &str, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let _ = (wayland && pipe("wl-copy", &[], text)) || klipper(text) || pipe("xclip", &["-selection", "clipboard"], text)
        || pipe("xsel", &["--clipboard", "--input"], text);
}

/// False if `program` isn't installed.
fn pipe(program: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    else {
        return false;
    };
    let written = child.stdin.take().is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    // The tool keeps serving the clipboard; reap it off the UI thread.
    std::thread::spawn(move || child.wait());
    written
}

fn klipper(text: &str) -> bool {
    Command::new("dbus-send")
        .args(["--session", "--print-reply", "--dest=org.kde.klipper", "/klipper", "org.kde.klipper.klipper.setClipboardContents"])
        .arg(format!("string:{text}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
