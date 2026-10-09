//! Apps started from Finder or the Dock get launchd's minimal `PATH` (`/usr/bin:/bin:…`), so
//! `claude`, `gh` and `git` from Homebrew or `~/.local/bin` would not be found. Brain takes the
//! `PATH` the user's login shell sets up instead, once, at start.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MARKER: &str = "__BRAIN_PATH__";

pub fn adopt_login_path() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let Ok(mut child) = Command::new(shell)
        .args(["-ilc", &format!("printf '{MARKER}%s' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    // A shell profile that hangs must not keep Brain from starting.
    let started = Instant::now();
    while child.try_wait().ok().flatten().is_none() {
        if started.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let Ok(out) = child.wait_with_output() else { return };
    let text = String::from_utf8_lossy(&out.stdout);
    if let Some(path) = text.rsplit(MARKER).next().filter(|p| !p.is_empty() && text.contains(MARKER)) {
        std::env::set_var("PATH", path.trim());
    }
}
