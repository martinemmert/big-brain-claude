//! Working with a whole conversation: finding text in it, the user's prompts, Claude's last
//! answer, and the conversation as one Markdown document.

use std::ops::Range;

use crate::transcript::{Message, Role};

/// Indices of the messages that contain `query`, ignoring case. An empty query finds nothing.
pub fn find(messages: &[Message], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.text.to_lowercase().contains(&query))
        .map(|(i, _)| i)
        .collect()
}

/// A one-line excerpt of `text` around the first match of `query`, at most about `width`
/// characters, and the byte range of the match inside the excerpt.
pub fn snippet(text: &str, query: &str, width: usize) -> (String, Option<Range<usize>>) {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let query = query.trim();
    let Some(start) = find_ignoring_case(&line, query) else {
        return (line.chars().take(width).collect(), None);
    };
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let at = chars.iter().position(|(b, _)| *b == start).unwrap_or(0);
    let query_chars = query.chars().count();
    let before = width.saturating_sub(query_chars) / 3;
    let from = at.saturating_sub(before);
    let to = (from + width).min(chars.len());
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    let offset = out.len();
    let first_byte = chars[from].0;
    let last_byte = chars.get(to).map_or(line.len(), |(b, _)| *b);
    out.push_str(&line[first_byte..last_byte]);
    if to < chars.len() {
        out.push('…');
    }
    let match_start = offset + (start - first_byte);
    let match_end = (match_start + matched_len(&line[start..], query)).min(offset + (last_byte - first_byte));
    (out, Some(match_start..match_end))
}

/// The byte offset of the first case-insensitive match of `query` in `text`.
fn find_ignoring_case(text: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    text.char_indices().map(|(i, _)| i).find(|&i| {
        let mut rest = text[i..].chars().flat_map(char::to_lowercase);
        query.iter().all(|q| rest.next() == Some(*q))
    })
}

/// How many bytes of `text` (which starts with a match) the match of `query` covers.
fn matched_len(text: &str, query: &str) -> usize {
    text.char_indices().nth(query.chars().count()).map_or(text.len(), |(i, _)| i)
}

/// The user's own prompts, oldest first, with their index in `messages`.
pub fn prompts(messages: &[Message]) -> Vec<(usize, &Message)> {
    messages.iter().enumerate().filter(|(_, m)| m.role == Role::User).collect()
}

/// Claude's most recent answer.
pub fn last_answer(messages: &[Message]) -> Option<&Message> {
    messages.iter().rev().find(|m| m.role == Role::Assistant && !m.text.trim().is_empty())
}

/// The conversation as Markdown: prompts as quotes, answers as they are, tool calls as a list.
pub fn export_markdown(title: &str, messages: &[Message]) -> String {
    let mut out = format!("# {title}\n");
    let mut in_tools = false;
    for message in messages {
        let is_tool = message.role == Role::Tool;
        match message.role {
            Role::User => {
                out.push_str("\n---\n\n");
                if let Some(ts) = message.ts {
                    out.push_str(&format!("**{}**\n\n", ts.format("%Y-%m-%d %H:%M")));
                }
                for line in message.text.lines() {
                    out.push_str(&format!("> {line}\n"));
                }
            }
            Role::Assistant => out.push_str(&format!("\n{}\n", message.text.trim_end())),
            Role::Tool => {
                if !in_tools {
                    out.push('\n');
                }
                let tool = message.tool.as_deref().unwrap_or("Tool");
                out.push_str(&format!("- `{tool}` {}\n", message.text.lines().next().unwrap_or_default()));
            }
            Role::System => out.push_str(&format!("\n_{}_\n", message.text)),
        }
        in_tools = is_tool;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: Role, text: &str) -> Message {
        Message { role, tool: (role == Role::Tool).then(|| "Bash".to_string()), text: text.to_string(), ts: None }
    }

    #[test]
    fn find_ignores_case_and_empty_queries() {
        let messages = [message(Role::User, "Fix the Login"), message(Role::Assistant, "Done: login works"), message(Role::Tool, "cargo test")];
        assert_eq!(find(&messages, "LOGIN"), [0, 1]);
        assert!(find(&messages, "  ").is_empty());
    }

    #[test]
    fn snippets_cut_around_the_match_and_point_at_it() {
        let text = format!("{} needle {}", "a".repeat(100), "b".repeat(100));
        let (line, range) = snippet(&text, "NEEDLE", 40);
        let range = range.unwrap();
        assert_eq!(&line[range], "needle");
        assert!(line.starts_with('…') && line.ends_with('…'));
        assert!(line.chars().count() <= 42);
    }

    #[test]
    fn snippets_handle_umlauts_and_no_match() {
        let (line, range) = snippet("Größe ändern\nund mehr", "ÄNDERN", 80);
        assert_eq!(&line[range.unwrap()], "ändern");
        assert_eq!(line, "Größe ändern und mehr");
        assert_eq!(snippet("abc", "x", 2), ("ab".to_string(), None));
    }

    #[test]
    fn prompts_and_the_last_answer() {
        let messages = [message(Role::User, "one"), message(Role::Assistant, "first"), message(Role::User, "two"), message(Role::Assistant, "second"), message(Role::Tool, "ls")];
        assert_eq!(prompts(&messages).iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(last_answer(&messages).map(|m| m.text.as_str()), Some("second"));
    }

    #[test]
    fn export_quotes_prompts_and_lists_tool_calls() {
        let messages = [message(Role::User, "Build it\nplease"), message(Role::Tool, "cargo build"), message(Role::Tool, "cargo test"), message(Role::Assistant, "Built.")];
        assert_eq!(
            export_markdown("Session", &messages),
            "# Session\n\n---\n\n> Build it\n> please\n\n- `Bash` cargo build\n- `Bash` cargo test\n\nBuilt.\n"
        );
    }
}
