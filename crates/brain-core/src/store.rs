use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate, Utc};

use crate::event::Event;

/// Append-only JSONL event log, one file per local day.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// `~/.claude-brain`
    pub fn default_root(home: &Path) -> PathBuf {
        home.join(".claude-brain")
    }

    pub fn events_dir(&self) -> PathBuf {
        self.root.join("events")
    }

    pub fn file_for(&self, day: NaiveDate) -> PathBuf {
        self.events_dir().join(format!("{}.jsonl", day.format("%Y-%m-%d")))
    }

    pub fn file_for_ts(&self, ts: DateTime<Utc>) -> PathBuf {
        self.file_for(ts.with_timezone(&Local).date_naive())
    }

    /// Writes the event as a single line with one `write` call on an
    /// `O_APPEND` handle, so concurrent writers from many sessions don't interleave.
    pub fn append(&self, event: &Event) -> std::io::Result<()> {
        std::fs::create_dir_all(self.events_dir())?;
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.file_for_ts(event.ts))?;
        file.write_all(&line)
    }
}

/// Incrementally reads complete lines from a growing JSONL file.
/// Malformed lines are skipped; a trailing line without `\n` is left for the next read.
#[derive(Debug)]
pub struct Tail {
    path: PathBuf,
    offset: u64,
}

impl Tail {
    pub fn new(path: PathBuf) -> Self {
        Self { path, offset: 0 }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read_new(&mut self) -> Vec<Event> {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        let Some(last_newline) = buf.iter().rposition(|b| *b == b'\n') else {
            return Vec::new();
        };
        self.offset += last_newline as u64 + 1;
        buf[..last_newline]
            .split(|b| *b == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Kind, Source};

    fn event(text: &str) -> Event {
        Event {
            v: 1,
            ts: Utc::now(),
            account: "main".into(),
            pid: 42,
            session_id: None,
            cwd: None,
            source: Source::Report,
            kind: Kind::Doing,
            text: Some(text.into()),
            tasks: None,
            name: None,
        }
    }

    #[test]
    fn tail_reads_only_new_complete_lines_and_skips_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let first = event("one");
        store.append(&first).unwrap();
        let path = store.file_for_ts(first.ts);
        let mut tail = Tail::new(path.clone());

        assert_eq!(tail.read_new().len(), 1);
        assert!(tail.read_new().is_empty());

        let mut raw = OpenOptions::new().append(true).open(&path).unwrap();
        raw.write_all(b"{not json}\n").unwrap();
        store.append(&event("two")).unwrap();
        raw.write_all(b"{\"partial\":").unwrap();

        let new = tail.read_new();
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].text.as_deref(), Some("two"));
    }
}
