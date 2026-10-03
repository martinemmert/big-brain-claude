use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;

use crate::event::{BackgroundTask, Kind};

/// The subset of the Claude Code hook stdin payload Brain cares about.
#[derive(Debug, Deserialize)]
pub struct HookPayload {
    pub hook_event_name: String,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub transcript_path: Option<String>,
    /// `UserPromptSubmit`
    pub prompt: Option<String>,
    /// `Notification`
    pub message: Option<String>,
    /// `Stop`
    pub last_assistant_message: Option<String>,
    /// `Stop`: background shells, subagents, monitors … still running.
    pub background_tasks: Option<Vec<BackgroundTask>>,
}

impl HookPayload {
    pub fn kind(&self) -> Option<Kind> {
        match self.hook_event_name.as_str() {
            "SessionStart" => Some(Kind::SessionStart),
            "UserPromptSubmit" => Some(Kind::Prompt),
            "Notification" => Some(Kind::Permission),
            "Stop" => Some(Kind::Stop),
            "SessionEnd" => Some(Kind::SessionEnd),
            _ => None,
        }
    }

    /// Short human text for the timeline: the prompt, the notification message,
    /// or for `Stop` the start of Claude's last reply.
    pub fn text(&self) -> Option<String> {
        let raw = match self.kind()? {
            Kind::Prompt => self
                .prompt
                .as_deref()
                .and_then(crate::transcript::classify_prompt)
                .map(|p| match p {
                    crate::transcript::Prompt::User(t) => t,
                    crate::transcript::Prompt::System(t) => format!("[{t}]"),
                }),
            Kind::Permission => self.message.clone(),
            Kind::Stop => self.last_assistant_message.clone().filter(|m| !m.trim().is_empty()).or_else(|| {
                self.transcript_path
                    .as_deref()
                    .and_then(|p| last_assistant_text(Path::new(p)))
            }),
            _ => None,
        }?;
        Some(one_line(&raw, 240))
    }
}

/// Collapses whitespace and cuts at `max` characters with an ellipsis.
pub fn one_line(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        return collapsed;
    }
    let cut: String = collapsed.chars().take(max - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Reads the last ~256 KiB of a transcript JSONL and returns the newest
/// assistant text block.
pub fn last_assistant_text(transcript: &Path) -> Option<String> {
    const TAIL_BYTES: u64 = 256 * 1024;
    let mut file = std::fs::File::open(transcript).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES))).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;

    buf.lines().rev().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if value.get("type")?.as_str()? != "assistant" {
            return None;
        }
        let blocks = value.get("message")?.get("content")?.as_array()?;
        blocks.iter().rev().find_map(|block| {
            (block.get("type")?.as_str()? == "text")
                .then(|| block.get("text")?.as_str().map(str::to_string))
                .flatten()
                .filter(|t| !t.trim().is_empty())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_text_is_the_last_assistant_text_block() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("t.jsonl");
        let lines = [
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"old"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Fertig:\n  SVG-Export   steht."},{"type":"tool_use"}]}}"#,
            r#"{"type":"attachment"}"#,
            "{broken",
        ];
        std::fs::write(&transcript, lines.join("\n")).unwrap();
        let payload = HookPayload {
            hook_event_name: "Stop".into(),
            session_id: Some("s1".into()),
            cwd: None,
            transcript_path: Some(transcript.to_string_lossy().into()),
            prompt: None,
            message: None,
            last_assistant_message: None,
            background_tasks: None,
        };

        assert_eq!(payload.kind(), Some(Kind::Stop));
        assert_eq!(payload.text().as_deref(), Some("Fertig: SVG-Export steht."));
    }

    #[test]
    fn one_line_truncates_on_char_boundaries() {
        assert_eq!(one_line("äöü äöü", 5), "äöü…");
        assert_eq!(one_line("kurz", 5), "kurz");
    }
}
