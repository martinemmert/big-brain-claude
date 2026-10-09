//! Questions and answers of a session: what Claude asked (with the AskUserQuestion tool, or as a
//! question at the end of a reply) and what the user answered.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// With the AskUserQuestion tool: the options were offered, the answer was picked or typed.
    Tool,
    /// In the text of a reply that ended the turn; the answer is the next prompt.
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub asked: Asked,
    pub question: String,
    /// The offered options (AskUserQuestion only).
    pub options: Vec<String>,
    /// `None` while unanswered.
    pub answer: Option<String>,
    pub ts: Option<DateTime<Utc>>,
}

/// Every question of the transcript, oldest first. Reads line by line.
pub fn exchanges(transcript: &Path) -> Vec<Exchange> {
    let Ok(file) = std::fs::File::open(transcript) else { return Vec::new() };
    let mut out: Vec<Exchange> = Vec::new();
    // AskUserQuestion call id → indices of its questions in `out`.
    let mut pending_tool: HashMap<String, Vec<usize>> = HashMap::new();
    // The last reply's text since the previous prompt, and when.
    let mut last_reply: Option<(String, Option<DateTime<Utc>>)> = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
        if entry.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let ts = entry.get("timestamp").and_then(Value::as_str).and_then(|t| t.parse().ok());
        let Some(content) = entry.get("message").and_then(|m| m.get("content")) else { continue };
        match entry.get("type").and_then(Value::as_str).unwrap_or_default() {
            "assistant" => {
                for block in content.as_array().into_iter().flatten() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let text = block.get("text").and_then(Value::as_str).unwrap_or_default();
                            if !text.trim().is_empty() {
                                last_reply = Some((text.to_string(), ts));
                            }
                        }
                        Some("tool_use") if block.get("name").and_then(Value::as_str) == Some("AskUserQuestion") => {
                            let id = block.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                            let questions = block.get("input").and_then(|i| i.get("questions")).and_then(Value::as_array);
                            let mut indices = Vec::new();
                            for q in questions.into_iter().flatten() {
                                let question = q.get("question").and_then(Value::as_str).unwrap_or_default().to_string();
                                let options = q
                                    .get("options")
                                    .and_then(Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|o| o.get("label").and_then(Value::as_str).map(str::to_string))
                                    .collect();
                                indices.push(out.len());
                                out.push(Exchange { asked: Asked::Tool, question, options, answer: None, ts });
                            }
                            pending_tool.insert(id, indices);
                            // A question asked with the tool isn't asked again in the text.
                            last_reply = None;
                        }
                        Some("tool_use") => last_reply = None,
                        _ => {}
                    }
                }
            }
            "user" if !entry.get("isMeta").and_then(Value::as_bool).unwrap_or(false) => match content {
                Value::Array(blocks) if blocks.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) => {
                    for block in blocks {
                        let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or_default();
                        let Some(indices) = pending_tool.remove(id) else { continue };
                        let answers = entry.get("toolUseResult").and_then(|r| r.get("answers"));
                        for i in indices {
                            let answer = answers.and_then(|a| a.get(&out[i].question)).and_then(Value::as_str);
                            out[i].answer = answer.map(str::to_string);
                        }
                    }
                }
                prompt => {
                    let text = match prompt {
                        Value::String(text) => text.clone(),
                        Value::Array(blocks) => blocks
                            .iter()
                            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                            .filter_map(|b| b.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        _ => String::new(),
                    };
                    // Harness input (command output, reminders) is no answer.
                    if text.trim().is_empty() || text.trim_start().starts_with('<') {
                        continue;
                    }
                    if let Some((reply, asked_ts)) = last_reply.take() {
                        if let Some(question) = closing_question(&reply) {
                            out.push(Exchange { asked: Asked::Text, question, options: Vec::new(), answer: Some(text.trim().to_string()), ts: asked_ts });
                        }
                    }
                }
            },
            _ => {}
        }
    }
    // The last reply may still wait for its answer.
    if let Some((reply, ts)) = last_reply {
        if let Some(question) = closing_question(&reply) {
            out.push(Exchange { asked: Asked::Text, question, options: Vec::new(), answer: None, ts });
        }
    }
    out
}

/// The question a reply ends with: its last paragraph, when that ends with a question mark.
fn closing_question(reply: &str) -> Option<String> {
    let last = reply.trim().rsplit("\n\n").next()?.trim();
    last.ends_with('?').then(|| last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(lines: &[serde_json::Value]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        std::fs::write(file.path(), text.join("\n")).unwrap();
        file
    }

    #[test]
    fn tool_questions_get_their_picked_answers() {
        let file = transcript(&[
            serde_json::json!({"type":"assistant","timestamp":"2026-10-09T10:00:00Z","message":{"content":[
                {"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"questions":[
                    {"question":"Which font?","header":"Font","multiSelect":false,"options":[{"label":"Mono","description":""},{"label":"Sans","description":""}]}
                ]}}
            ]}}),
            serde_json::json!({"type":"user","timestamp":"2026-10-09T10:01:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"answered"}]},
                "toolUseResult":{"questions":[],"answers":{"Which font?":"Mono"}}}),
        ]);
        let found = exchanges(file.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].asked, Asked::Tool);
        assert_eq!(found[0].options, ["Mono", "Sans"]);
        assert_eq!(found[0].answer.as_deref(), Some("Mono"));
    }

    #[test]
    fn a_reply_ending_in_a_question_is_answered_by_the_next_prompt() {
        let file = transcript(&[
            serde_json::json!({"type":"assistant","timestamp":"2026-10-09T10:00:00Z","message":{"content":[{"type":"text","text":"Done.\n\nShall I install it?"}]}}),
            serde_json::json!({"type":"user","timestamp":"2026-10-09T10:01:00Z","message":{"content":"yes"}}),
            serde_json::json!({"type":"assistant","timestamp":"2026-10-09T10:02:00Z","message":{"content":[{"type":"text","text":"Installed."}]}}),
            serde_json::json!({"type":"user","timestamp":"2026-10-09T10:03:00Z","message":{"content":"thanks"}}),
            serde_json::json!({"type":"assistant","timestamp":"2026-10-09T10:04:00Z","message":{"content":[{"type":"text","text":"Push it too?"}]}}),
        ]);
        let found = exchanges(file.path());
        let pairs: Vec<(&str, Option<&str>)> = found.iter().map(|e| (e.question.as_str(), e.answer.as_deref())).collect();
        assert_eq!(pairs, [("Shall I install it?", Some("yes")), ("Push it too?", None)]);
    }

    #[test]
    fn harness_input_is_no_answer() {
        let file = transcript(&[
            serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"Ready?"}]}}),
            serde_json::json!({"type":"user","message":{"content":"<command-name>/clear</command-name>"}}),
            serde_json::json!({"type":"user","message":{"content":"go"}}),
        ]);
        let found = exchanges(file.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].answer.as_deref(), Some("go"));
    }
}
