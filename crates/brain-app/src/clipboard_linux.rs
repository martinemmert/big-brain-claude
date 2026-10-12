//! A picture on the clipboard saved as a file, via `wl-paste` or `xclip`.

use std::process::{Command, Stdio};

/// `None` without a picture or with text, which the terminal pastes itself.
pub fn save_image() -> Option<std::path::PathBuf> {
    let types = read(&["wl-paste", "--list-types"]).or_else(|| read(&["xclip", "-selection", "clipboard", "-t", "TARGETS", "-o"]))?;
    let types = String::from_utf8_lossy(&types);
    if types.lines().any(|t| t.starts_with("text/plain") || t == "UTF8_STRING") || !types.lines().any(|t| t == "image/png") {
        return None;
    }
    let data = read(&["wl-paste", "--type", "image/png"]).or_else(|| read(&["xclip", "-selection", "clipboard", "-t", "image/png", "-o"]))?;
    let dir = brain_core::account::home_dir().join(".claude-brain/attachments");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("paste-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::write(&path, data).ok()?;
    Some(path)
}

fn read(command: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(command[0]).args(&command[1..]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}
