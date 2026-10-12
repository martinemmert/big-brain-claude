//! The files a session touched, from its transcript: what Claude wrote, edited and read, what
//! was attached to it (an `@file`, or a file Claude Code attached again after compacting) and
//! what the user dropped or pasted into a prompt.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

/// How a session touched a file. A file can be touched several ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Touch {
    /// Written as a whole (`Write`): usually a new file.
    Written,
    /// Changed in place (`Edit`, `MultiEdit`, `NotebookEdit`).
    Edited,
    /// Read by Claude (`Read`).
    Read,
    /// Attached to a prompt: an `@file` of the user's, or one Claude Code attached itself.
    Attached,
    /// A path the user dropped or pasted into a prompt.
    Shared,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionFile {
    /// Absolute path.
    pub path: String,
    /// Each way the file was touched, in the order of [`Touch`].
    pub touches: Vec<Touch>,
    /// When it was last touched (Unix ms); 0 when the transcript has no time.
    pub last_ms: i64,
}

impl SessionFile {
    pub fn has(&self, touch: Touch) -> bool {
        self.touches.contains(&touch)
    }
}

/// Every file the transcript mentions, most recently touched first. Reads the file line by
/// line, so long transcripts don't need to fit in memory at once.
pub fn session_files(transcript: &Path) -> Vec<SessionFile> {
    let Ok(file) = std::fs::File::open(transcript) else {
        return Vec::new();
    };
    let mut files: HashMap<String, SessionFile> = HashMap::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
        let ms = entry
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|t| t.parse::<chrono::DateTime<chrono::Utc>>().ok())
            .map_or(0, |t| t.timestamp_millis());
        for (path, touch) in touches_of(&entry) {
            let file = files.entry(path.clone()).or_insert_with(|| SessionFile { path, touches: Vec::new(), last_ms: 0 });
            if !file.touches.contains(&touch) {
                file.touches.push(touch);
                file.touches.sort();
            }
            file.last_ms = file.last_ms.max(ms);
        }
    }
    let mut out: Vec<SessionFile> = files.into_values().collect();
    out.sort_by(|a, b| b.last_ms.cmp(&a.last_ms).then_with(|| a.path.cmp(&b.path)));
    out
}

fn touches_of(entry: &Value) -> Vec<(String, Touch)> {
    let mut out = Vec::new();
    if let Some(attachment) = entry.get("attachment") {
        if attachment.get("type").and_then(Value::as_str) == Some("file") {
            if let Some(path) = attachment.get("filename").and_then(Value::as_str) {
                out.push((path.to_string(), Touch::Attached));
            }
        }
        return out;
    }
    let kind = entry.get("type").and_then(Value::as_str).unwrap_or_default();
    let Some(content) = entry.get("message").and_then(|m| m.get("content")) else { return out };
    match kind {
        "assistant" => {
            for block in content.as_array().into_iter().flatten() {
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let input = block.get("input");
                let field = |key: &str| input.and_then(|i| i.get(key)).and_then(Value::as_str).map(str::to_string);
                let touch = match block.get("name").and_then(Value::as_str).unwrap_or_default() {
                    "Write" => field("file_path").map(|p| (p, Touch::Written)),
                    "Edit" | "MultiEdit" => field("file_path").map(|p| (p, Touch::Edited)),
                    "NotebookEdit" => field("notebook_path").map(|p| (p, Touch::Edited)),
                    "Read" => field("file_path").map(|p| (p, Touch::Read)),
                    _ => None,
                };
                out.extend(touch);
            }
        }
        "user" if !entry.get("isMeta").and_then(Value::as_bool).unwrap_or(false) => {
            // Only what the user typed: tool results are user entries too.
            let texts: Vec<&str> = match content {
                Value::String(text) => vec![text.as_str()],
                Value::Array(blocks) => blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect(),
                _ => Vec::new(),
            };
            for text in texts {
                out.extend(prompt_paths(text).into_iter().map(|p| (p, Touch::Shared)));
            }
        }
        _ => {}
    }
    out
}

/// Absolute or `~/` paths in a prompt that name existing files: what a drop or a paste puts
/// there, with spaces and specials backslash-escaped like iTerm does.
fn prompt_paths(text: &str) -> Vec<String> {
    let home = crate::account::home_dir();
    let mut out = Vec::new();
    for word in shell_words(text) {
        let path = match word.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None if word.starts_with('/') => word.clone().into(),
            None => continue,
        };
        if path.is_file() {
            out.push(path.display().to_string());
        }
    }
    out
}

/// Splits on unescaped whitespace and removes the escaping backslashes.
fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            c if c.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(lines: &[String]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), lines.join("\n")).unwrap();
        file
    }

    fn tool(name: &str, input: &str, ts: &str) -> String {
        format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"tool_use","name":"{name}","input":{input}}}]}}}}"#)
    }

    #[test]
    fn tool_calls_say_how_each_file_was_touched() {
        let file = transcript(&[
            tool("Read", r#"{"file_path":"/repo/src/app.rs"}"#, "2026-10-09T10:00:00Z"),
            tool("Write", r#"{"file_path":"/repo/docs/plan.md","content":"x"}"#, "2026-10-09T10:01:00Z"),
            tool("Edit", r#"{"file_path":"/repo/src/app.rs","old_string":"a","new_string":"b"}"#, "2026-10-09T10:02:00Z"),
            tool("NotebookEdit", r#"{"notebook_path":"/repo/n.ipynb"}"#, "2026-10-09T10:03:00Z"),
            tool("Bash", r#"{"command":"ls"}"#, "2026-10-09T10:04:00Z"),
        ]);
        let files = session_files(file.path());
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        // Most recently touched first.
        assert_eq!(paths, ["/repo/n.ipynb", "/repo/src/app.rs", "/repo/docs/plan.md"]);
        assert_eq!(files[1].touches, [Touch::Edited, Touch::Read]);
        assert_eq!(files[2].touches, [Touch::Written]);
    }

    #[test]
    fn attachments_and_paths_in_prompts_are_given_files() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("Bildschirmfoto 1.png");
        std::fs::write(&shot, b"png").unwrap();
        let escaped = shot.display().to_string().replace(' ', "\\ ");
        let file = transcript(&[
            r#"{"type":"attachment","timestamp":"2026-10-09T09:00:00Z","attachment":{"type":"file","filename":"/repo/README.md","displayPath":"README.md"}}"#.to_string(),
            serde_json::json!({
                "type": "user",
                "timestamp": "2026-10-09T09:01:00Z",
                "message": {"role": "user", "content": format!("Look at {escaped} and /no/such/file.txt")},
            })
            .to_string(),
            // A tool result is not something the user shared.
            format!(r#"{{"type":"user","timestamp":"2026-10-09T09:02:00Z","message":{{"role":"user","content":[{{"type":"tool_result","content":"{}"}}]}}}}"#, shot.display()),
        ]);
        let files = session_files(file.path());
        let shared: Vec<(&str, &[Touch])> = files.iter().map(|f| (f.path.as_str(), f.touches.as_slice())).collect();
        assert_eq!(shared, [(shot.to_str().unwrap(), &[Touch::Shared][..]), ("/repo/README.md", &[Touch::Attached][..])]);
    }

    #[test]
    fn escaped_spaces_stay_inside_a_word() {
        assert_eq!(shell_words(r"a /x/my\ file.png  b"), ["a", "/x/my file.png", "b"]);
    }

    #[test]
    fn a_missing_transcript_has_no_files() {
        assert!(session_files(Path::new("/no/such/transcript.jsonl")).is_empty());
    }
}
