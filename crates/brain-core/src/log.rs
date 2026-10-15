//! Brain's log: what Brain said in its status line, what failed, and what a session's terminal
//! showed when it closed. One file per day in `~/.claude-brain/logs/`, kept two weeks, so a
//! message that flashed by can be read again (⌘K → "Open the log").

use std::io::Write;
use std::path::{Path, PathBuf};

/// Days a log file is kept.
const KEEP_DAYS: u64 = 14;

pub fn dir(home: &Path) -> PathBuf {
    home.join(".claude-brain/logs")
}

/// Today's log file.
pub fn today(home: &Path) -> PathBuf {
    dir(home).join(format!("{}.log", chrono::Local::now().format("%Y-%m-%d")))
}

/// Appends `HH:MM:SS  kind  text`; further lines of `text` are indented under it. Never fails
/// loudly: a log that can't be written must not stop what it logs.
pub fn write(home: &Path, kind: &str, text: &str) {
    let dir = dir(home);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut lines = text.trim_end().lines();
    let first = lines.next().unwrap_or_default();
    let mut entry = format!("{}  {kind:<8} {first}\n", chrono::Local::now().format("%H:%M:%S"));
    for line in lines {
        entry.push_str(&format!("                   {line}\n"));
    }
    let path = today(home);
    let fresh = !path.exists();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(entry.as_bytes());
    }
    if fresh {
        prune(&dir);
    }
}

fn prune(dir: &Path) {
    let limit = std::time::Duration::from_secs(KEEP_DAYS * 24 * 3600);
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit);
        if old && entry.path().extension().is_some_and(|e| e == "log") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_one_entry_per_call_with_its_lines_indented() {
        let home = tempfile::tempdir().unwrap();
        write(home.path(), "status", "Stopping failed: no such agent");
        write(home.path(), "terminal", "FUX Session closed after 2 s\nError: pty gone\n");
        let text = std::fs::read_to_string(today(home.path())).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with("status   Stopping failed: no such agent"), "{}", lines[0]);
        assert!(lines[1].contains("terminal FUX Session closed after 2 s"));
        assert_eq!(lines[2].trim(), "Error: pty gone");
    }
}
