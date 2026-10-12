//! A session in a few lines, to know what it was about before resuming it: its title, how it
//! began and ended, a question left open and the files it changed.

use std::path::Path;

use chrono::{DateTime, Utc};

use crate::files::Touch;
use crate::transcript::Role;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Brief {
    pub title: Option<String>,
    pub first_prompt: Option<String>,
    pub last_prompt: Option<String>,
    pub last_reply: Option<String>,
    /// The last question nobody answered yet.
    pub open_question: Option<String>,
    pub prompts: usize,
    /// Files written or edited, most recent first.
    pub changed: Vec<String>,
    pub started: Option<DateTime<Utc>>,
    pub last: Option<DateTime<Utc>>,
}

pub fn brief(transcript: &Path) -> Brief {
    let messages = crate::transcript::read_all_messages(transcript);
    let prompts: Vec<_> = messages.iter().filter(|m| m.role == Role::User).collect();
    let changed = crate::files::session_files(transcript)
        .into_iter()
        .filter(|f| f.has(Touch::Written) || f.has(Touch::Edited))
        .map(|f| f.path)
        .collect();
    Brief {
        title: crate::transcript::title_of(transcript),
        first_prompt: prompts.first().map(|m| m.text.clone()),
        last_prompt: prompts.last().map(|m| m.text.clone()),
        last_reply: crate::history::last_answer(&messages).map(|m| m.text.clone()),
        open_question: crate::qa::exchanges(transcript).into_iter().rev().find(|e| e.answer.is_none()).map(|e| e.question),
        prompts: prompts.len(),
        changed,
        started: messages.iter().find_map(|m| m.ts),
        last: messages.iter().rev().find_map(|m| m.ts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_up_a_session() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let lines = [
            serde_json::json!({"type":"user","timestamp":"2026-10-09T10:00:00Z","message":{"role":"user","content":"Build the export"}}),
            serde_json::json!({"type":"assistant","timestamp":"2026-10-09T10:05:00Z","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"w","name":"Write","input":{"file_path":"/repo/export.rs","content":"x"}},
                {"type":"text","text":"Export written.\n\nShall I add tests?"}
            ]}}),
        ];
        std::fs::write(file.path(), lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();

        let brief = brief(file.path());

        assert_eq!(brief.first_prompt.as_deref(), Some("Build the export"));
        assert_eq!(brief.last_prompt.as_deref(), Some("Build the export"));
        assert_eq!(brief.prompts, 1);
        assert_eq!(brief.open_question.as_deref(), Some("Shall I add tests?"));
        assert_eq!(brief.changed, ["/repo/export.rs"]);
        assert!(brief.last_reply.unwrap().starts_with("Export written."));
        assert!(brief.started < brief.last);
    }
}
