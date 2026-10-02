//! macOS integrations: jumping to the iTerm2 session of a pid and notifications.

use std::process::Command;

use brain_core::process::tty_of;

pub enum JumpResult {
    Focused,
    NoTerminal,
    NotFound(String),
}

/// Brings the iTerm2 window, tab and split pane whose tty belongs to `pid` to the front.
pub fn jump_to_iterm(pid: u32) -> JumpResult {
    let Some(tty) = tty_of(pid) else {
        return JumpResult::NoTerminal;
    };
    let script = format!(
        r#"tell application "iTerm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "{tty}" then
          select w
          tell t to select
          tell s to select
          activate
          return "ok"
        end if
      end repeat
    end repeat
  end repeat
  return "missing"
end tell"#
    );
    match Command::new("osascript").args(["-e", &script]).output() {
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "ok" => JumpResult::Focused,
        Ok(out) if out.status.success() => JumpResult::NotFound(format!("Kein iTerm-Tab mit {tty}")),
        Ok(out) => JumpResult::NotFound(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(err) => JumpResult::NotFound(err.to_string()),
    }
}

pub fn notify(title: &str, subtitle: &str, body: &str) {
    let script = format!(
        "display notification {} with title {} subtitle {} sound name \"Glass\"",
        applescript_string(body),
        applescript_string(title),
        applescript_string(subtitle),
    );
    let _ = Command::new("osascript").args(["-e", &script]).spawn();
}

fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}
