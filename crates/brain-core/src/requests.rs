//! What Brain asks of a session through its mod: a reply, a slash command, an interrupt. Nothing
//! is typed into a terminal: the mod in the session submits the prompt or runs the command itself
//! (`$.prompt.submit`, `$.command.run`, `$.turn.abort`).
//!
//! Brain writes `~/.claude-brain/requests/<session id>.json`, `{ items: [{ seq, kind, text? }] }`;
//! the mod takes each `seq` once and writes the last one it took to `<session id>.done`. A session
//! whose mod wrote that file takes requests.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Request {
    /// Text as the person's own words; a known `/command` runs as that command.
    Prompt { text: String },
    /// Ends the running turn.
    Interrupt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Item {
    seq: u64,
    #[serde(flatten)]
    request: Request,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Requests {
    items: Vec<Item>,
}

pub fn dir(home: &Path) -> PathBuf {
    home.join(".claude-brain/requests")
}

fn done_file(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.done"))
}

fn requests_file(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.json"))
}

/// The sessions whose mod takes requests, by session id.
pub fn takers(dir: &Path) -> std::collections::HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Default::default() };
    entries.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".done").map(str::to_string)).collect()
}

/// Whether the session's mod started taking requests at or after `since`: a session woken then
/// is ready for what waited for it.
pub fn taken_since(dir: &Path, session_id: &str, since: std::time::SystemTime) -> bool {
    std::fs::metadata(done_file(dir, session_id)).and_then(|m| m.modified()).is_ok_and(|at| at >= since)
}

fn last_done(dir: &Path, session_id: &str) -> u64 {
    std::fs::read_to_string(done_file(dir, session_id)).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0)
}

/// Hands `request` to the session's mod. What it took already is dropped from the file; the file
/// is replaced whole, so the mod never reads half of it.
pub fn send(dir: &Path, session_id: &str, request: Request) -> std::io::Result<()> {
    let file = requests_file(dir, session_id);
    let done = last_done(dir, session_id);
    let mut requests: Requests = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let seq = requests.items.iter().map(|i| i.seq).max().unwrap_or(0).max(done) + 1;
    requests.items.retain(|i| i.seq > done);
    requests.items.push(Item { seq, request });
    let tmp = dir.join(format!(".{session_id}.json.tmp"));
    std::fs::write(&tmp, serde_json::to_vec(&requests)?)?;
    std::fs::rename(&tmp, &file)
}

/// Removes the files of sessions not seen for two days: a session writes `.done` again when it
/// starts or resumes.
pub fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 60 * 60);
    for entry in entries.flatten() {
        if entry.metadata().and_then(|m| m.modified()).is_ok_and(|at| at < old) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_count_on_from_what_the_mod_took_and_keep_only_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        assert!(takers(dir.path()).is_empty());
        std::fs::write(done_file(dir.path(), "s1"), "0").unwrap();
        assert!(takers(dir.path()).contains("s1"));

        send(dir.path(), "s1", Request::Prompt { text: "go on".into() }).unwrap();
        send(dir.path(), "s1", Request::Interrupt).unwrap();
        let written = std::fs::read_to_string(requests_file(dir.path(), "s1")).unwrap();
        assert_eq!(written, r#"{"items":[{"seq":1,"kind":"prompt","text":"go on"},{"seq":2,"kind":"interrupt"}]}"#);

        // The mod took both; the next request follows them and stands alone.
        std::fs::write(done_file(dir.path(), "s1"), "2").unwrap();
        send(dir.path(), "s1", Request::Prompt { text: "/compact".into() }).unwrap();
        let written = std::fs::read_to_string(requests_file(dir.path(), "s1")).unwrap();
        assert_eq!(written, r#"{"items":[{"seq":3,"kind":"prompt","text":"/compact"}]}"#);
    }
}
