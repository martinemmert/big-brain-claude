//! Reads the latest conversation messages from a Claude Code transcript
//! (`<config>/projects/<cwd-slug>/<session-id>.jsonl`).

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::account::Account;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    /// A tool call by Claude, summarised in one line.
    Tool,
    /// Input that came from the harness, not the user: subagent hand-backs,
    /// background task notifications.
    System,
}

/// What a user-turn text really is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    User(String),
    System(String),
}

impl Prompt {
    pub fn text(&self) -> &str {
        match self {
            Prompt::User(t) | Prompt::System(t) => t,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    /// For `Tool`: the tool name.
    pub tool: Option<String>,
    pub text: String,
    pub ts: Option<DateTime<Utc>>,
}

/// Locates the transcript of a session in any project folder of the account.
pub fn find_transcript(account: &Account, session_id: &str) -> Option<PathBuf> {
    let file_name = format!("{session_id}.jsonl");
    std::fs::read_dir(account.config_dir.join("projects"))
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(&file_name))
        .find(|path| path.is_file())
}

/// The last `limit` messages, oldest first. Reads from the end of the file and
/// reaches further back only while too few messages were found, so large
/// transcripts stay cheap.
pub fn read_recent_messages(transcript: &Path, limit: usize) -> Vec<Message> {
    const FIRST_READ: u64 = 512 * 1024;
    const MAX_READ: u64 = 6 * 1024 * 1024;
    let len = std::fs::metadata(transcript).map(|m| m.len()).unwrap_or(0);
    let mut window = FIRST_READ;
    loop {
        let mut messages = read_tail(transcript, len, window);
        let reached_start = window >= len;
        if messages.len() >= limit || reached_start || window >= MAX_READ {
            let skip = messages.len().saturating_sub(limit);
            messages.drain(..skip);
            return messages;
        }
        window *= 4;
    }
}

fn read_tail(transcript: &Path, len: u64, window: u64) -> Vec<Message> {
    let Ok(mut file) = std::fs::File::open(transcript) else {
        return Vec::new();
    };
    let start = len.saturating_sub(window);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if file.take(len - start).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // probably cut in the middle
    }
    lines
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .flat_map(|entry| messages_of(&entry))
        .collect()
}

fn messages_of(entry: &Value) -> Vec<Message> {
    let kind = entry.get("type").and_then(Value::as_str).unwrap_or_default();
    let flagged = |key: &str| entry.get(key).and_then(Value::as_bool).unwrap_or(false);
    if !matches!(kind, "user" | "assistant") || flagged("isMeta") || flagged("isSidechain") {
        return Vec::new();
    }
    let ts = entry
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|t| t.parse().ok());
    let Some(content) = entry.get("message").and_then(|m| m.get("content")) else {
        return Vec::new();
    };
    let message = |role, tool: Option<String>, text: String| Message { role, tool, text, ts };

    match (kind, content) {
        ("user", Value::String(text)) => classify_prompt(text)
            .map(|p| vec![prompt_message(p, ts)])
            .unwrap_or_default(),
        ("user", Value::Array(blocks)) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str).and_then(classify_prompt))
            .map(|p| prompt_message(p, ts))
            .collect(),
        ("assistant", Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| match block.get("type").and_then(Value::as_str)? {
                "text" => {
                    let text = block.get("text")?.as_str()?.trim();
                    (!text.is_empty()).then(|| message(Role::Assistant, None, text.to_string()))
                }
                "tool_use" => {
                    let name = block.get("name")?.as_str()?.to_string();
                    let summary = tool_summary(&name, block.get("input").unwrap_or(&Value::Null));
                    Some(message(Role::Tool, Some(name), summary))
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn prompt_message(prompt: Prompt, ts: Option<DateTime<Utc>>) -> Message {
    let role = match prompt {
        Prompt::User(_) => Role::User,
        Prompt::System(_) => Role::System,
    };
    Message { role, tool: None, text: prompt.text().to_string(), ts }
}

/// Tells what the user actually typed from harness-generated turn input.
/// Slash commands, `!` shell input and pastes are unwrapped; subagent hand-backs
/// and task notifications become short system notes; command output is dropped.
pub fn classify_prompt(raw: &str) -> Option<Prompt> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    if !text.starts_with('<') {
        return Some(Prompt::User(text.to_string()));
    }
    if let Some(name) = between(text, "<command-name>", "</command-name>") {
        let args = between(text, "<command-args>", "</command-args>").unwrap_or_default();
        return Some(Prompt::User(format!("{} {}", name.trim(), args.trim()).trim().to_string()));
    }
    if let Some(input) = between(text, "<bash-input>", "</bash-input>") {
        return Some(Prompt::User(format!("! {}", input.trim())));
    }
    if text.starts_with("<pasted_content") {
        let inner: Vec<&str> = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("<pasted_content") && !l.trim_start().starts_with("</pasted_content"))
            .collect();
        let inner = inner.join("\n").trim().to_string();
        return (!inner.is_empty()).then_some(Prompt::User(inner));
    }
    if text.starts_with("<agent-message") {
        return Some(Prompt::System("Rückmeldung eines Agents".into()));
    }
    if text.starts_with("<task-notification") {
        let summary = between(text, "<summary>", "</summary>")
            .map(|s| crate::hook::one_line(s, 160))
            .unwrap_or_else(|| "Hintergrund-Aufgabe meldet sich".into());
        return Some(Prompt::System(summary));
    }
    None
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

/// One line describing a tool call, e.g. `cargo test` for Bash or the path for Edit.
fn tool_summary(name: &str, input: &Value) -> String {
    let field = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_string);
    let summary = match name {
        "Bash" => field("command"),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => field("file_path").or_else(|| field("notebook_path")),
        "Grep" | "Glob" => field("pattern"),
        "WebSearch" => field("query"),
        "WebFetch" => field("url"),
        "Agent" | "Task" => field("description"),
        "Skill" => field("skill"),
        "TodoWrite" => Some("Todo-Liste aktualisiert".into()),
        _ => None,
    }
    .or_else(|| {
        // Unknown tools: the first string argument says the most.
        input.as_object()?.values().find_map(|v| v.as_str().map(str::to_string))
    })
    .unwrap_or_default();
    crate::hook::one_line(&summary, 160)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &[&str] = &[
        r#"{"type":"user","message":{"role":"user","content":"Baue den Export"},"timestamp":"2026-10-02T14:00:00Z"}"#,
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Base directory for this skill"}}"#,
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>x</task-id>\n</task-notification>"}}"#,
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/review</command-name>\n<command-args>42</command-args>"}}"#,
        r#"{"type":"user","message":{"role":"user","content":"<agent-message from=\"a1\">done</agent-message>"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"Ich schaue mir das an.\n\nZuerst die Tests."},{"type":"tool_use","name":"Bash","input":{"command":"cargo test\n  -p core","description":"Run tests"}}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
        r#"{"type":"assistant","isSidechain":true,"message":{"role":"assistant","content":[{"type":"text","text":"subagent"}]}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/w/src/view.rs","old_string":"a"}},{"type":"tool_use","name":"mcp__x__thing","input":{"limit":3,"query":"needle"}}]}}"#,
        r#"{"type":"attachment"}"#,
    ];

    fn transcript(lines: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();
        (dir, path)
    }

    #[test]
    fn extracts_prompts_replies_and_tool_calls_but_not_harness_noise() {
        let (_dir, path) = transcript(LINES);

        let got: Vec<(Role, String)> = read_recent_messages(&path, 50)
            .into_iter()
            .map(|m| (m.role, m.text))
            .collect();

        assert_eq!(
            got,
            vec![
                (Role::User, "Baue den Export".into()),
                (Role::System, "Hintergrund-Aufgabe meldet sich".into()),
                (Role::User, "/review 42".into()),
                (Role::System, "Rückmeldung eines Agents".into()),
                (Role::Assistant, "Ich schaue mir das an.\n\nZuerst die Tests.".into()),
                (Role::Tool, "cargo test -p core".into()),
                (Role::Tool, "/w/src/view.rs".into()),
                (Role::Tool, "needle".into()),
            ]
        );
    }

    #[test]
    fn keeps_only_the_last_messages() {
        let (_dir, path) = transcript(LINES);

        let got = read_recent_messages(&path, 2);

        assert_eq!(got.len(), 2);
        assert_eq!(got[0].tool.as_deref(), Some("Edit"));
        assert_eq!(got[1].tool.as_deref(), Some("mcp__x__thing"));
    }

    #[test]
    fn reaches_back_past_large_tool_output_at_the_end() {
        let filler = format!(r#"{{"type":"attachment","data":"{}"}}"#, "x".repeat(300 * 1024));
        let mut lines: Vec<&str> = LINES.to_vec();
        lines.extend([filler.as_str(), filler.as_str(), filler.as_str()]);
        let (_dir, path) = transcript(&lines);

        let got = read_recent_messages(&path, 50);

        assert_eq!(got.first().map(|m| m.text.as_str()), Some("Baue den Export"));
    }

    #[test]
    fn finds_the_transcript_in_any_project_folder() {
        let home = tempfile::tempdir().unwrap();
        let account = Account::from_config_dir(home.path().join(".claude"));
        let project = account.config_dir.join("projects/-Users-me-app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("abc.jsonl"), "").unwrap();

        assert_eq!(find_transcript(&account, "abc"), Some(project.join("abc.jsonl")));
        assert_eq!(find_transcript(&account, "nope"), None);
    }
}
