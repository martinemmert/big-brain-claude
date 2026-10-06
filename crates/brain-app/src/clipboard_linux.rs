//! The clipboard on Linux. GPUI sets it on Wayland with the serial of the last key press, so a
//! copy triggered by a click is dropped by the compositor; the system tools don't have that
//! problem. GPUI still gets the text, for pasting inside Brain.

use std::io::Write;
use std::process::{Command, Stdio};

use gpui::{App, ClipboardItem};

pub fn copy(text: &str, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let _ = (wayland && pipe("wl-copy", &[], text)) || klipper(text) || pipe("xclip", &["-selection", "clipboard"], text)
        || pipe("xsel", &["--clipboard", "--input"], text);
}

/// Writes `text` to the stdin of a clipboard tool; false if the tool isn't installed.
fn pipe(program: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    else {
        return false;
    };
    let written = child.stdin.take().is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    // The tools keep running in the background to serve the clipboard; reap them off the UI thread.
    std::thread::spawn(move || child.wait());
    written
}

/// KDE's clipboard manager.
fn klipper(text: &str) -> bool {
    Command::new("dbus-send")
        .args(["--session", "--print-reply", "--dest=org.kde.klipper", "/klipper", "org.kde.klipper.klipper.setClipboardContents"])
        .arg(format!("string:{text}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
