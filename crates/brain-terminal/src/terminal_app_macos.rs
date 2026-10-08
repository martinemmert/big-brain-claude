//! Terminal.app via AppleScript; tabs carry a `tty` property.

use super::applescript::{applescript_string, run_osascript};
use super::Outcome;

/// Activates Terminal.app, selects the tab and raises its window. Raising only
/// takes effect while Terminal.app is active, hence `activate` first.
/// Windows are addressed by `id` because raising one reorders `windows`.
pub fn focus(tty: &str) -> Outcome {
    let script = format!(
        r#"tell application "Terminal"
  set windowIds to id of windows
  repeat with wid in windowIds
    set theWindow to window id wid
    repeat with ti from 1 to count of tabs of theWindow
      set theTab to tab ti of theWindow
      if tty of theTab is {} then
        activate
        set miniaturized of theWindow to false
        set selected of theTab to true
        set index of theWindow to 1
        return "ok"
      end if
    end repeat
  end repeat
  return "missing"
end tell"#,
        applescript_string(tty)
    );
    run_osascript(&script, || format!("no Terminal tab with {tty}"))
}

/// `do script` without a target opens a new window running `shell_command`.
pub fn open_new(shell_command: &str) -> Outcome {
    let script = format!(
        "tell application \"Terminal\"\n  activate\n  do script {}\n  return \"ok\"\nend tell",
        applescript_string(shell_command)
    );
    run_osascript(&script, || "Terminal did not open a window".into())
}
