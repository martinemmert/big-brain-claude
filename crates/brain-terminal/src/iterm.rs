//! iTerm2 via AppleScript; sessions are found by their tty.

use std::path::Path;

use super::script::{applescript_string, run_osascript};
use super::{Key, Outcome};

pub fn is_installed() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    Path::new("/Applications/iTerm.app").exists() || Path::new(&home).join("Applications/iTerm.app").exists()
}

/// Selects tab → session → window, in that order, then activates iTerm2.
/// The tab comes first: selecting a background tab restores the split pane
/// that was active in it, undoing a session selected before.
pub fn focus(tty: &str) -> Outcome {
    run_on_session(tty, "select theTab\n          select theSession\n          select theWindow\n          activate")
}

/// Types `text` and presses Return without bringing iTerm2 to the front.
/// Text and Return are sent separately so Claude Code does not take the
/// Return as part of a paste.
pub fn type_text(tty: &str, text: &str) -> Outcome {
    let action = format!(
        "tell theSession to write text {} newline no\n          delay 0.15\n          tell theSession to write text (ASCII character 13) newline no",
        applescript_string(text)
    );
    run_on_session(tty, &action)
}

pub fn send_key(tty: &str, key: Key) -> Outcome {
    let code = match key {
        Key::Return => 13,
        Key::Escape => 27,
    };
    run_on_session(tty, &format!("tell theSession to write text (ASCII character {code}) newline no"))
}

/// A new tab in the frontmost window (or a new window) whose shell runs `shell_command`.
pub fn open_new(shell_command: &str) -> Outcome {
    let script = format!(
        r#"tell application "iTerm2"
  activate
  if current window is missing value then
    set theWindow to (create window with default profile)
    set theSession to current session of theWindow
  else
    tell current window to set theTab to (create tab with default profile)
    set theSession to current session of theTab
  end if
  tell theSession to write text {}
  return "ok"
end tell"#,
        applescript_string(shell_command)
    );
    run_osascript(&script, || "iTerm2 did not open a tab".into())
}

fn run_on_session(tty: &str, action: &str) -> Outcome {
    run_osascript(&session_script(tty, action), || format!("no iTerm2 session with {tty}"))
}

/// Finds the session by tty and runs `action` on it (`theSession`, `theTab`, `theWindow`).
///
/// Windows are addressed by their stable `id`: selecting a window reorders
/// `windows`, so index-based references taken before would point elsewhere.
fn session_script(tty: &str, action: &str) -> String {
    let tty = applescript_string(tty);
    format!(
        r#"tell application "iTerm2"
  set windowIds to id of windows
  repeat with wid in windowIds
    set theWindow to window id wid
    repeat with ti from 1 to count of tabs of theWindow
      set theTab to tab ti of theWindow
      repeat with si from 1 to count of sessions of theTab
        set theSession to session si of theTab
        if tty of theSession is {tty} then
          {action}
          return "ok"
        end if
      end repeat
    end repeat
  end repeat
  return "missing"
end tell"#
    )
}
