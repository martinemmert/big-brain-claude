//! macOS integrations: iTerm2 control for a session's pid, and notifications.

use std::process::Command;

use brain_core::process::tty_of;

pub enum ItermResult {
    Done,
    NoTerminal,
    Failed(String),
}

/// Brings the iTerm2 window, tab and split pane whose tty belongs to `pid` to the front.
pub fn jump_to_iterm(pid: u32) -> ItermResult {
    run_on_iterm_session(pid, true, "")
}

/// Types `text` into the session's iTerm2 pane and presses Return, without
/// bringing iTerm2 to the front.
pub fn type_into_session(pid: u32, text: &str) -> ItermResult {
    // Text and Return are sent separately so Claude Code does not take the
    // Return as part of a paste.
    let action = format!(
        "tell theSession to write text {} newline no\n          delay 0.15\n          tell theSession to write text (ASCII character 13) newline no",
        applescript_string(text)
    );
    run_on_iterm_session(pid, false, &action)
}

/// Finds the pane by tty and runs `action` on it (`theSession`). With `focus`,
/// it first selects session → tab → window, in that order.
///
/// Windows are addressed by their stable `id`: selecting a window reorders
/// `windows`, so index-based references taken before would point elsewhere.
fn run_on_iterm_session(pid: u32, focus: bool, action: &str) -> ItermResult {
    let Some(tty) = tty_of(pid) else {
        return ItermResult::NoTerminal;
    };
    let focus = if focus {
        "select theSession\n          select theTab\n          select theWindow\n          activate"
    } else {
        ""
    };
    let script = format!(
        r#"tell application "iTerm2"
  set windowIds to id of windows
  repeat with wid in windowIds
    set theWindow to window id wid
    repeat with ti from 1 to count of tabs of theWindow
      set theTab to tab ti of theWindow
      repeat with si from 1 to count of sessions of theTab
        set theSession to session si of theTab
        if tty of theSession is "{tty}" then
          {focus}
          {action}
          return "ok"
        end if
      end repeat
    end repeat
  end repeat
  return "missing"
end tell"#
    );
    match Command::new("osascript").args(["-e", &script]).output() {
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "ok" => ItermResult::Done,
        Ok(out) if out.status.success() => ItermResult::Failed(format!("Kein iTerm-Tab mit {tty}")),
        Ok(out) => ItermResult::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(err) => ItermResult::Failed(err.to_string()),
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
