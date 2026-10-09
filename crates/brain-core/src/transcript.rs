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

/// Notes for turn input that came from the harness; the UI may translate them.
pub const NOTE_AGENT_REPLY: &str = "A subagent reported back";
pub const NOTE_TASK: &str = "A background task reported back";
pub const NOTE_TODOS: &str = "Updated the todo list";

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

/// A session's files: its transcript and, if there is one, the folder of its subagents' transcripts
/// and tool output.
pub fn session_files(account: &Account, session_id: &str) -> Vec<PathBuf> {
    let Some(transcript) = find_transcript(account, session_id) else { return Vec::new() };
    let folder = transcript.with_extension("");
    let mut files = vec![transcript];
    if folder.is_dir() {
        files.push(folder);
    }
    files
}

/// The ids of every session whose transcript Claude Code still keeps for this account, i.e. the
/// sessions that can still be resumed.
pub fn kept_session_ids(account: &Account) -> std::collections::HashSet<String> {
    std::fs::read_dir(account.config_dir.join("projects"))
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|project| std::fs::read_dir(project.path()).into_iter().flatten().flatten())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".jsonl").map(str::to_string)
        })
        .collect()
}

/// Copies a session's transcript (and its folder of subagent transcripts and tool output, if any)
/// into the same project folder of another account, so that account can `claude --resume` it.
/// Returns the copied transcript's path.
pub fn copy_to_account(transcript: &Path, target: &Account) -> std::io::Result<PathBuf> {
    let not_found = || std::io::Error::new(std::io::ErrorKind::NotFound, "transcript has no project folder");
    let project = transcript.parent().and_then(Path::file_name).ok_or_else(not_found)?;
    let file = transcript.file_name().ok_or_else(not_found)?;
    let dest_dir = target.config_dir.join("projects").join(project);
    std::fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(file);
    std::fs::copy(transcript, &dest)?;
    let side = transcript.with_extension("");
    if side.is_dir() {
        copy_dir(&side, &dest.with_extension(""))?;
    }
    Ok(dest)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
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
    tail_entries(transcript, len, window).iter().flat_map(messages_of).collect()
}

/// The JSON entries in the last `window` bytes of a transcript.
fn tail_entries(transcript: &Path, len: u64, window: u64) -> Vec<Value> {
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
    lines.filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect()
}

/// Whether the latest turn is still running or has ended, according to the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Working,
    Ended,
}

/// What the end of a transcript says about its session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Insight {
    /// The latest turn state and when it was written (epoch ms).
    pub turn: Option<(Turn, i64)>,
    pub model: Option<String>,
    /// Tokens the last request sent: input plus cache reads and writes.
    pub context_tokens: Option<u64>,
    pub permission_mode: Option<String>,
    /// Claude Code's own running total (`cost-state` entries).
    pub cost_usd: Option<f64>,
    /// Claude Code's generated title (`ai-title`).
    pub title: Option<String>,
    /// Files this session wrote or edited (Write, Edit, MultiEdit, NotebookEdit), most recent last.
    pub edited: Vec<String>,
}

/// Reads the last 512 KiB of a transcript.
///
/// A turn ends with a `system`/`turn_duration` entry; a denied permission or Esc writes
/// `[Request interrupted by user…]` and no hook fires, so this catches what hooks miss.
pub fn insight(transcript: &Path) -> Insight {
    let len = std::fs::metadata(transcript).map(|m| m.len()).unwrap_or(0);
    let mut out = Insight::default();
    for entry in tail_entries(transcript, len, 512 * 1024) {
        if entry.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let ts = entry
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|t| t.parse::<DateTime<Utc>>().ok())
            .map(|t| t.timestamp_millis());
        let field = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_string);
        match entry.get("type").and_then(Value::as_str).unwrap_or_default() {
            "system" if field("subtype").as_deref() == Some("turn_duration") => {
                if let Some(ts) = ts {
                    out.turn = Some((Turn::Ended, ts));
                }
            }
            "user" if !entry.get("isMeta").and_then(Value::as_bool).unwrap_or(false) => {
                let Some(ts) = ts else { continue };
                let state = if user_texts(&entry).any(|t| t.starts_with("[Request interrupted by user")) {
                    Turn::Ended
                } else {
                    Turn::Working
                };
                out.turn = Some((state, ts));
            }
            "assistant" => {
                if let Some(ts) = ts {
                    out.turn = Some((Turn::Working, ts));
                }
                let message = entry.get("message");
                if let Some(model) = message.and_then(|m| m.get("model")).and_then(Value::as_str) {
                    out.model = Some(model.to_string());
                }
                for path in edited_paths(message) {
                    out.edited.retain(|p| *p != path);
                    out.edited.push(path);
                }
                if let Some(usage) = message.and_then(|m| m.get("usage")) {
                    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
                    out.context_tokens = Some(n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens"));
                }
            }
            "permission-mode" => out.permission_mode = field("permissionMode").or(out.permission_mode.take()),
            "cost-state" => out.cost_usd = entry.get("totalCostUSD").and_then(Value::as_f64).or(out.cost_usd),
            "ai-title" => out.title = field("aiTitle").or(out.title.take()),
            _ => {}
        }
    }
    out
}

/// File paths of the editing tool calls in an assistant message.
fn edited_paths(message: Option<&Value>) -> Vec<String> {
    let blocks = message.and_then(|m| m.get("content")).and_then(Value::as_array);
    blocks
        .into_iter()
        .flatten()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
        .filter(|b| matches!(b.get("name").and_then(Value::as_str), Some("Write" | "Edit" | "MultiEdit" | "NotebookEdit")))
        .filter_map(|b| {
            let input = b.get("input")?;
            input.get("file_path").or_else(|| input.get("notebook_path"))?.as_str().map(str::to_string)
        })
        .collect()
}

/// The text parts of a user entry (a plain string or text blocks).
fn user_texts(entry: &Value) -> impl Iterator<Item = &str> {
    let content = entry.get("message").and_then(|m| m.get("content"));
    let single = content.and_then(Value::as_str);
    let blocks = content.and_then(Value::as_array).into_iter().flatten().filter_map(|b| {
        (b.get("type").and_then(Value::as_str) == Some("text")).then(|| b.get("text").and_then(Value::as_str)).flatten()
    });
    single.into_iter().chain(blocks)
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
        return Some(Prompt::System(NOTE_AGENT_REPLY.into()));
    }
    if text.starts_with("<task-notification") {
        let summary = between(text, "<summary>", "</summary>")
            .map(|s| crate::hook::one_line(s, 160))
            .unwrap_or_else(|| NOTE_TASK.into());
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
        "TodoWrite" => Some(NOTE_TODOS.into()),
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
                (Role::System, NOTE_TASK.into()),
                (Role::User, "/review 42".into()),
                (Role::System, NOTE_AGENT_REPLY.into()),
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

    /// Shapes taken from a real transcript where a Bash permission was denied with Esc.
    #[test]
    fn a_denied_permission_ends_the_turn_and_session_details_are_read() {
        let lines = [
            r#"{"type":"permission-mode","permissionMode":"default"}"#,
            r#"{"type":"user","message":{"role":"user","content":"Run date"},"timestamp":"2026-10-03T08:42:46.000Z"}"#,
            r#"{"type":"ai-title","aiTitle":"Date output file test"}"#,
            r#"{"type":"assistant","message":{"model":"claude-haiku-4-5-20251001","content":[{"type":"tool_use","name":"Bash","input":{"command":"date"}}],"usage":{"input_tokens":12,"cache_read_input_tokens":20000,"cache_creation_input_tokens":3000,"output_tokens":40}},"timestamp":"2026-10-03T08:42:49.000Z"}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"content":"The user doesn't want to proceed with this tool use."}]},"timestamp":"2026-10-03T08:43:07.000Z"}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]},"timestamp":"2026-10-03T08:43:07.100Z"}"#,
            r#"{"type":"cost-state","totalCostUSD":0.0474824}"#,
        ];
        let (_dir, path) = transcript(&lines);

        let got = insight(&path);

        let ended_at = "2026-10-03T08:43:07.100Z".parse::<DateTime<Utc>>().unwrap().timestamp_millis();
        assert_eq!(got.turn, Some((Turn::Ended, ended_at)));
        assert_eq!(got.model.as_deref(), Some("claude-haiku-4-5-20251001"));
        assert_eq!(got.context_tokens, Some(23012));
        assert_eq!(got.permission_mode.as_deref(), Some("default"));
        assert_eq!(got.cost_usd, Some(0.0474824));
        assert_eq!(got.title.as_deref(), Some("Date output file test"));
    }

    #[test]
    fn collects_the_files_a_session_edited() {
        let lines = [
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/w/a.rs"}},{"type":"tool_use","name":"Read","input":{"file_path":"/w/b.rs"}}]},"timestamp":"2026-10-04T09:00:00Z"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"/w/c.rs"}},{"type":"tool_use","name":"Edit","input":{"file_path":"/w/a.rs"}}]},"timestamp":"2026-10-04T09:01:00Z"}"#,
        ];
        let (_dir, path) = transcript(&lines);

        assert_eq!(insight(&path).edited, vec!["/w/c.rs", "/w/a.rs"]);
    }

    #[test]
    fn a_new_prompt_after_the_turn_ended_means_working_again() {
        let lines = [
            r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-10-03T09:00:00.000Z"}"#,
            r#"{"type":"user","message":{"role":"user","content":"weiter"},"timestamp":"2026-10-03T09:01:00.000Z"}"#,
        ];
        let (_dir, path) = transcript(&lines);

        assert_eq!(insight(&path).turn.map(|(t, _)| t), Some(Turn::Working));
    }

    #[test]
    fn copies_a_transcript_with_its_subagent_folder_into_another_account() {
        let home = tempfile::tempdir().unwrap();
        let main = Account::from_config_dir(home.path().join(".claude"));
        let second = Account::from_config_dir(home.path().join(".claude-second"));
        let project = main.config_dir.join("projects/-Users-me-app");
        std::fs::create_dir_all(project.join("abc/subagents")).unwrap();
        std::fs::write(project.join("abc.jsonl"), "{}\n").unwrap();
        std::fs::write(project.join("abc/subagents/agent-1.jsonl"), "{}\n").unwrap();

        let copied = copy_to_account(&project.join("abc.jsonl"), &second).unwrap();

        assert_eq!(copied, second.config_dir.join("projects/-Users-me-app/abc.jsonl"));
        assert!(second.config_dir.join("projects/-Users-me-app/abc/subagents/agent-1.jsonl").is_file());
        assert_eq!(find_transcript(&second, "abc"), Some(copied));
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
