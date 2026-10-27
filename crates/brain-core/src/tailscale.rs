//! Tailscale on the Mac, only while it's needed: on when the phone asks before going out, off
//! (disconnected, and the app quit, so nothing of it talks to the outside) when the phone is back
//! in the home Wi-Fi or Brain quits. `tailscale down` alone keeps the app's connections to
//! Tailscale's servers; quitting the app ends them and the VPN.

use std::process::Command;
use std::time::{Duration, Instant};

/// The CLI the Mac app ships.
const CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

pub fn installed() -> bool {
    std::path::Path::new(CLI).exists()
}

/// Opens the app and connects; returns once it is connected, or why not.
pub fn up() -> Result<(), String> {
    if !installed() {
        return Err("Tailscale is not installed".into());
    }
    let opened = Command::new("open").args(["-g", "-a", "Tailscale"]).status().map_err(|e| e.to_string())?;
    if !opened.success() {
        return Err("couldn't open Tailscale".into());
    }
    // The app answers its CLI a moment after it started.
    wait(Duration::from_secs(30), || state().is_some()).ok_or("Tailscale didn't start")?;
    if state().as_deref() == Some("NeedsLogin") {
        return Err("Tailscale needs a login on the Mac".into());
    }
    let out = Command::new(CLI).arg("up").output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    wait(Duration::from_secs(20), || state().as_deref() == Some("Running")).ok_or("Tailscale didn't connect")?;
    Ok(())
}

/// Disconnects and quits the app.
pub fn down() -> Result<(), String> {
    if state().is_some() {
        let _ = Command::new(CLI).arg("down").output();
    }
    let quit = Command::new("osascript").args(["-e", QUIT]).output().map_err(|e| e.to_string())?;
    if quit.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&quit.stderr).trim().to_string())
    }
}

/// Quits the app only if it runs: `tell application … to quit` would start it first.
const QUIT: &str = r#"if application "Tailscale" is running then tell application "Tailscale" to quit"#;

/// Whether it's connected.
pub fn running() -> bool {
    state().as_deref() == Some("Running")
}

/// Once `pid` (Brain) has ended, however it ended: disconnect and quit the app. A process of its
/// own, since a quitting or crashing Brain gets no chance to.
pub fn down_after(pid: u32) -> std::io::Result<()> {
    let script = format!("while kill -0 {pid} 2>/dev/null; do sleep 5; done; '{CLI}' down >/dev/null 2>&1; osascript -e '{QUIT}' >/dev/null 2>&1");
    Command::new("/bin/sh").args(["-c", &script]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map(|_| ())
}

/// The Mac's name in the tailnet (`macbook.tail1234.ts.net`), while the app runs.
pub fn name() -> Option<String> {
    name_in(&status()?)
}

/// The backend's state (`Running`, `Stopped`, `NeedsLogin`, …); none while the app doesn't run.
fn state() -> Option<String> {
    let status: serde_json::Value = serde_json::from_slice(&status()?).ok()?;
    status.get("BackendState")?.as_str().filter(|s| !s.is_empty() && *s != "NoState").map(str::to_string)
}

fn status() -> Option<Vec<u8>> {
    let out = Command::new(CLI).args(["status", "--json"]).output().ok()?;
    // Stopped, it still answers (exit status 1 with the JSON); without the app it doesn't.
    (!out.stdout.is_empty()).then_some(out.stdout)
}

fn name_in(status: &[u8]) -> Option<String> {
    let status: serde_json::Value = serde_json::from_slice(status).ok()?;
    let name = status.get("Self")?.get("DNSName")?.as_str()?.trim_end_matches('.');
    (!name.is_empty()).then(|| name.to_string())
}

fn wait(limit: Duration, done: impl Fn() -> bool) -> Option<()> {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        if done() {
            return Some(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_the_macs_own_without_the_final_dot() {
        let status = br#"{"BackendState":"Running","Self":{"DNSName":"macbook-pro-3.tail1b3106.ts.net.","TailscaleIPs":["100.117.51.56"]}}"#;
        assert_eq!(name_in(status), Some("macbook-pro-3.tail1b3106.ts.net".into()));
        assert_eq!(name_in(br#"{"Self":{"DNSName":""}}"#), None);
        assert_eq!(name_in(b"not json"), None);
    }
}
