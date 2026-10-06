//! `brain://session/<account>/<pid>` links (Alfred's ⌥⏎, `brain show`) and the update check.

use std::sync::Mutex;


static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Called by the platform when macOS hands Brain a `brain://` URL.
pub fn received(urls: Vec<String>) {
    OPENED.lock().unwrap().extend(urls);
}

/// Sessions asked for since the last call, as account and pid of their process.
pub fn take_sessions() -> Vec<(String, u32)> {
    std::mem::take(&mut *OPENED.lock().unwrap()).iter().filter_map(|u| parse(u)).collect()
}

fn parse(url: &str) -> Option<(String, u32)> {
    let rest = url.strip_prefix("brain://session/")?;
    let (account, pid) = rest.trim_end_matches('/').split_once('/')?;
    Some((account.to_string(), pid.parse().ok()?))
}

/// A newer release on GitHub: its version and page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    pub url: String,
}

const LATEST: &str = "https://api.github.com/repos/martinemmert/big-brain-claude/releases/latest";

/// Asks GitHub for the latest release (blocking; run off the UI thread).
pub fn check_for_update() -> Option<Update> {
    let out = std::process::Command::new("/usr/bin/curl")
        .args(["-fsSL", "-m", "10", "-H", "Accept: application/vnd.github+json", LATEST])
        .output()
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    let url = json.get("html_url")?.as_str()?.to_string();
    newer(tag, env!("CARGO_PKG_VERSION")).then(|| Update { version: tag.trim_start_matches('v').to_string(), url })
}

fn version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
}

fn newer(candidate: &str, current: &str) -> bool {
    matches!((version(candidate), version(current)), (Some(a), Some(b)) if a > b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_links_and_versions_are_parsed() {
        assert_eq!(parse("brain://session/second/4711"), Some(("second".to_string(), 4711)));
        assert_eq!(parse("brain://other/x"), None);
        assert!(newer("v0.3.0", "0.2.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(!newer("v0.2.0", "0.2.0"));
        assert!(!newer("nightly", "0.2.0"));
    }
}
