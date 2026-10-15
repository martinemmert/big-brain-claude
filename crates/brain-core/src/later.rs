//! Tasks for later: noted from a session (`/later …` in Claude Code, `brain later …`), listed in
//! Brain, started there as a new session in their folder or checked off.
//!
//! `~/.claude-brain/later.jsonl`, append-only like the event log, so sessions noting at the same
//! moment don't overwrite each other: one line notes a task, another checks it off.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One task noted for later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub ts: DateTime<Utc>,
    pub text: String,
    /// Where it was noted: the session's folder, account, id and name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Line {
    Add(Item),
    Done { id: String, ts: DateTime<Utc> },
}

/// `~/.claude-brain/later.jsonl`
pub fn file(home: &Path) -> PathBuf {
    home.join(".claude-brain").join("later.jsonl")
}

/// Notes a task; its id is when it was noted, to the nanosecond.
pub fn add(file: &Path, mut task: Item) -> std::io::Result<Item> {
    task.text = task.text.trim().to_string();
    if task.text.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty task"));
    }
    if task.id.is_empty() {
        task.id = task.ts.timestamp_nanos_opt().unwrap_or_default().to_string();
    }
    append(file, &Line::Add(task.clone()))?;
    Ok(task)
}

/// Checks a task off.
pub fn done(file: &Path, id: &str) -> std::io::Result<()> {
    append(file, &Line::Done { id: id.to_string(), ts: Utc::now() })
}

/// The tasks not checked off yet, oldest first.
pub fn open(file: &Path) -> Vec<Item> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let mut tasks: Vec<Item> = Vec::new();
    for line in text.lines() {
        match serde_json::from_str::<Line>(line) {
            Ok(Line::Add(task)) => tasks.push(task),
            Ok(Line::Done { id, .. }) => tasks.retain(|t| t.id != id),
            Err(_) => {}
        }
    }
    tasks
}

/// One line with one `write` on an `O_APPEND` handle; the file only the user can read.
fn append(file: &Path, line: &Line) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut bytes = serde_json::to_vec(line)?;
    bytes.push(b'\n');
    std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(file)?.write_all(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(text: &str, secs: i64) -> Item {
        Item {
            id: String::new(),
            ts: DateTime::from_timestamp(1_790_000_000 + secs, 0).unwrap(),
            text: text.into(),
            cwd: Some("/w/app".into()),
            account: Some("second".into()),
            session_id: None,
            session_name: None,
        }
    }

    #[test]
    fn noted_tasks_stay_until_checked_off() {
        let dir = tempfile::tempdir().unwrap();
        let file = file(dir.path());
        assert!(open(&file).is_empty());
        let first = add(&file, task("  Tests für den Export  ", 0)).unwrap();
        add(&file, task("Changelog", 1)).unwrap();
        assert!(add(&file, task("   ", 2)).is_err());
        let texts = |tasks: Vec<Item>| tasks.into_iter().map(|t| t.text).collect::<Vec<_>>();
        assert_eq!(texts(open(&file)), ["Tests für den Export", "Changelog"]);
        done(&file, &first.id).unwrap();
        assert_eq!(texts(open(&file)), ["Changelog"]);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
