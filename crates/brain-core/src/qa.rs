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

/// An AskUserQuestion call still waiting for its answer.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OpenQuestion {
    /// The tool call's id: an answer names it, so it can only ever reach this question.
    pub call: String,
    pub items: Vec<QuestionItem>,
    /// Some question takes several picks (answered in the terminal, not with one key).
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuestionItem {
    pub question: String,
    pub options: Vec<String>,
}

/// The transcript's last megabyte: an open question and its answer land at the end.
fn tail(transcript: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    const TAIL: u64 = 1024 * 1024;
    let mut file = std::fs::File::open(transcript).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    Some(text)
}

/// Whether the transcript holds the result of tool call `call`. Claude Code writes an
/// AskUserQuestion call to the transcript only together with its answer, so a question Brain's
/// mod reported stays open until then.
pub fn answered(transcript: &Path, call: &str) -> bool {
    let Some(text) = tail(transcript) else { return false };
    text.lines().filter(|line| line.contains(call)).any(|line| {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { return false };
        let blocks = entry.get("message").and_then(|m| m.get("content")).and_then(Value::as_array);
        blocks.into_iter().flatten().any(|b| {
            b.get("type").and_then(Value::as_str) == Some("tool_result") && b.get("tool_use_id").and_then(Value::as_str) == Some(call)
        })
    })
}

/// The AskUserQuestion call the session waits on, if any: the last one in the transcript that
/// has no result yet. Reads only the transcript's end (the question is the last thing in it).
pub fn open_question(transcript: &Path) -> Option<OpenQuestion> {
    let text = tail(transcript)?;
    let mut open: Option<OpenQuestion> = None;
    // The first line may be cut off by the seek; it doesn't parse and is skipped.
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        if entry.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(blocks) = entry.get("message").and_then(|m| m.get("content")).and_then(Value::as_array) else { continue };
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") if block.get("name").and_then(Value::as_str) == Some("AskUserQuestion") => {
                    let questions = block.get("input").and_then(|i| i.get("questions")).and_then(Value::as_array);
                    let mut multi_select = false;
                    let items = questions
                        .into_iter()
                        .flatten()
                        .map(|q| {
                            multi_select |= q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false);
                            QuestionItem {
                                question: q.get("question").and_then(Value::as_str).unwrap_or_default().to_string(),
                                options: q
                                    .get("options")
                                    .and_then(Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|o| o.get("label").and_then(Value::as_str).map(str::to_string))
                                    .collect(),
                            }
                        })
                        .collect();
                    let call = block.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                    open = Some(OpenQuestion { call, items, multi_select });
                }
                Some("tool_result") => {
                    let id = block.get("tool_use_id").and_then(Value::as_str);
                    if open.as_ref().is_some_and(|o| Some(o.call.as_str()) == id) {
                        open = None;
                    }
                }
                _ => {}
            }
        }
    }
    open.filter(|o| !o.call.is_empty() && !o.items.is_empty())
}

/// The keys that answer an open question in Claude Code's dialog, as tested by hand: each
/// question's option number in turn, and with several questions `1` on the review page that
/// follows ("Submit answers"). `None` when a pick doesn't fit the question.
pub fn answer_keys(question: &OpenQuestion, picks: &[usize]) -> Option<String> {
    if question.multi_select || picks.len() != question.items.len() {
        return None;
    }
    let mut keys = String::new();
    for (item, &pick) in question.items.iter().zip(picks) {
        if pick >= item.options.len() || pick >= 9 {
            return None;
        }
        keys.push(char::from(b'1' + pick as u8));
    }
    if question.items.len() > 1 {
        keys.push('1');
    }
    Some(keys)
}

/// The question a reply ends with: its last paragraph, when that ends with a question mark.
fn closing_question(reply: &str) -> Option<String> {
    let last = reply.trim().rsplit("\n\n").next()?.trim();
    last.ends_with('?').then(|| last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_open_question_and_the_keys_that_answer_it() {
        let ask = serde_json::json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "toolu_1", "name": "AskUserQuestion", "input": { "questions": [
            { "question": "Color?", "options": [{ "label": "Red" }, { "label": "Blue" }] },
            { "question": "Size?", "options": [{ "label": "Small" }, { "label": "Large" }] }
        ] } }] } });
        let file = transcript(&[ask.clone()]);
        let open = open_question(file.path()).unwrap();
        assert_eq!((open.call.as_str(), open.items.len(), open.items[1].options[1].as_str()), ("toolu_1", 2, "Large"));
        // Blue, Small, then "Submit answers" — what the dialog took in the test by hand.
        assert_eq!(answer_keys(&open, &[1, 0]).as_deref(), Some("211"));
        assert_eq!(answer_keys(&open, &[2, 0]), None);
        assert_eq!(answer_keys(&open, &[1]), None);
        let single = OpenQuestion { items: vec![open.items[0].clone()], ..open.clone() };
        assert_eq!(answer_keys(&single, &[1]).as_deref(), Some("2"));
        let multi = OpenQuestion { multi_select: true, ..single };
        assert_eq!(answer_keys(&multi, &[1]), None);

        let answered = serde_json::json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "ok" }] } });
        let file = transcript(&[ask, answered]);
        assert_eq!(open_question(file.path()), None);
    }

    #[test]
    fn a_reported_question_is_answered_once_its_result_is_in_the_transcript() {
        let result = |id: &str| serde_json::json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": "ok" }] } });
        let file = transcript(&[result("toolu_0")]);
        assert!(!answered(file.path(), "toolu_1"));
        let file = transcript(&[result("toolu_0"), result("toolu_1")]);
        assert!(answered(file.path(), "toolu_1"));
    }

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
