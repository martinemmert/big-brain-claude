//! The artifacts a session published, from its transcript (and its subagents', which publish on
//! its behalf). Claude Code's `Artifact` tool answers a publication with `Published <file> at
//! <url> (Version n, …)` (a new version: `Updated …`). Opening one (`Opened the Artifact at …`),
//! reading one, or a link that is only mentioned is no publication.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Artifact {
    pub url: String,
    /// The title of the published page, when the transcript tells it.
    pub title: Option<String>,
    /// When it was last published (Unix ms); 0 when the transcript has no time.
    pub last_ms: i64,
}

/// The session's artifacts, the most recently published first. The same URL counts once; its
/// newest time wins. `transcript` is the session's `<id>.jsonl`; the transcripts of its
/// subagents lie in `<id>/subagents/`.
pub fn session_artifacts(transcript: &Path) -> Vec<Artifact> {
    let mut found: HashMap<String, Artifact> = HashMap::new();
    let mut files = vec![transcript.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(transcript.with_extension("").join("subagents")) {
        files.extend(entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jsonl")));
    }
    for file in files {
        read_into(&file, &mut found);
    }
    let mut out: Vec<Artifact> = found.into_values().collect();
    out.sort_by(|a, b| b.last_ms.cmp(&a.last_ms).then_with(|| a.url.cmp(&b.url)));
    out
}

/// What a publishing call knew about the page before its answer came.
struct Call {
    title: Option<String>,
}

fn read_into(transcript: &Path, found: &mut HashMap<String, Artifact>) {
    let Ok(file) = std::fs::File::open(transcript) else { return };
    // The `<title>` of the last `Write` to each file: the call that publishes it names only the path.
    let mut written: HashMap<String, String> = HashMap::new();
    let mut calls: HashMap<String, Call> = HashMap::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        // Most lines have nothing to do with artifacts: parse only those that can.
        if !(line.contains("\"Artifact\"") || line.contains("<title>") || line.contains("artifact/")) {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
        let ms = entry
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|t| t.parse::<chrono::DateTime<chrono::Utc>>().ok())
            .map_or(0, |t| t.timestamp_millis());
        let Some(blocks) = entry.get("message").and_then(|m| m.get("content")).and_then(Value::as_array) else { continue };
        for block in blocks {
            let text = |value: Option<&Value>, key: &str| value.and_then(|v| v.get(key)).and_then(Value::as_str).map(str::to_string);
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let input = block.get("input");
                    match block.get("name").and_then(Value::as_str) {
                        Some("Write") => {
                            if let (Some(path), Some(title)) = (text(input, "file_path"), text(input, "content").as_deref().and_then(page_title)) {
                                written.insert(path, title);
                            }
                        }
                        Some("Artifact") => {
                            let title = text(input, "title")
                                .or_else(|| text(input, "file_path").and_then(|p| written.get(&p).cloned()))
                                .or_else(|| text(input, "description"))
                                .filter(|t| !t.trim().is_empty());
                            if let Some(id) = text(Some(block), "id") {
                                calls.insert(id, Call { title });
                            }
                        }
                        _ => {}
                    }
                }
                Some("tool_result") => {
                    let Some(call) = block.get("tool_use_id").and_then(Value::as_str).and_then(|id| calls.remove(id)) else { continue };
                    let Some(url) = published_url(&result_text(block.get("content"))) else { continue };
                    let artifact = found.entry(url.clone()).or_insert_with(|| Artifact { url, title: None, last_ms: 0 });
                    if call.title.is_some() {
                        artifact.title = call.title;
                    }
                    artifact.last_ms = artifact.last_ms.max(ms);
                }
                _ => {}
            }
        }
    }
}

/// A tool result's text: a string, or the text blocks of a list.
fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// The URL of the answer to a publication (`Published <file> at <url> (Version 1 …`); none for
/// anything else the tool answers.
fn published_url(answer: &str) -> Option<String> {
    let answer = answer.trim_start();
    if !(answer.starts_with("Published ") || answer.starts_with("Updated ")) {
        return None;
    }
    let after = answer.split_once(" at ")?.1;
    let url = after.split_whitespace().next()?.trim_end_matches(['.', ',', ')']);
    let ok = url.starts_with("https://") && url.contains("claude.ai/") && url.contains("artifact/");
    ok.then(|| url.to_string())
}

/// The text of the first `<title>` in a page.
fn page_title(html: &str) -> Option<String> {
    let start = html.find("<title>")? + "<title>".len();
    let title = html[start..].split_once("</title>")?.0.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// `claude.ai/artifact/4xSG…` for a list: no scheme.
pub fn short_url(url: &str) -> &str {
    url.strip_prefix("https://").unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(lines: &[&str]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), lines.join("\n")).unwrap();
        file
    }

    fn call(id: &str, input: &str, ts: &str) -> String {
        format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"Artifact","input":{input}}}]}}}}"#)
    }

    fn answer(id: &str, text: &str, ts: &str) -> String {
        format!(r#"{{"type":"user","timestamp":"{ts}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{text}"}}]}}}}"#)
    }

    fn write(path: &str, content: &str) -> String {
        format!(r#"{{"type":"assistant","timestamp":"2026-11-11T09:00:00Z","message":{{"content":[{{"type":"tool_use","id":"w","name":"Write","input":{{"file_path":"{path}","content":"{content}"}}}}]}}}}"#)
    }

    #[test]
    fn a_publication_is_found_with_the_title_of_its_page_and_the_time() {
        let lines = [
            write("/t/a.html", "<title>Brain-Seitenleiste</title><style></style>"),
            call("c1", r#"{"file_path":"/t/a.html","icon":"sidebar","description":"Seitenleiste"}"#, "2026-11-11T09:10:00Z"),
            answer("c1", "Published /t/a.html at https://claude.ai/artifact/AAA (Version 1, version id 1-ab).", "2026-11-11T09:10:05Z"),
        ];
        let file = transcript(&lines.each_ref().map(String::as_str));
        let found = session_artifacts(file.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].url, "https://claude.ai/artifact/AAA");
        assert_eq!(found[0].title.as_deref(), Some("Brain-Seitenleiste"));
        assert_eq!(found[0].last_ms, 1_794_388_205_000);
    }

    #[test]
    fn the_same_url_counts_once_and_the_newest_time_wins() {
        let lines = [
            call("c1", r#"{"file_path":"/t/a.html","description":"erste"}"#, "2026-11-11T09:00:00Z"),
            answer("c1", "Published /t/a.html at https://claude.ai/artifact/AAA (Version 1)", "2026-11-11T09:00:01Z"),
            call("c2", r#"{"file_path":"/t/b.html","description":"andere"}"#, "2026-11-12T09:00:00Z"),
            answer("c2", "Published /t/b.html at https://claude.ai/artifact/BBB (Version 1)", "2026-11-12T09:00:01Z"),
            call("c3", r#"{"file_path":"/t/a.html","description":"zweite"}"#, "2026-11-13T09:00:00Z"),
            answer("c3", "Updated /t/a.html at https://claude.ai/artifact/AAA (Version 2)", "2026-11-13T09:00:01Z"),
        ];
        let file = transcript(&lines.each_ref().map(String::as_str));
        let found = session_artifacts(file.path());
        let urls: Vec<&str> = found.iter().map(|a| a.url.as_str()).collect();
        assert_eq!(urls, ["https://claude.ai/artifact/AAA", "https://claude.ai/artifact/BBB"]);
        assert_eq!(found[0].title.as_deref(), Some("zweite"));
        assert!(found[0].last_ms > found[1].last_ms);
    }

    #[test]
    fn opening_and_mentioning_an_artifact_is_no_publication() {
        let lines = [
            call("c1", r#"{"action":"open","url":"https://claude.ai/artifact/OPEN"}"#, "2026-11-11T09:00:00Z"),
            answer("c1", "Opened the Artifact at https://claude.ai/artifact/OPEN for the user. Nothing was published or changed.", "2026-11-11T09:00:01Z"),
            r#"{"type":"assistant","timestamp":"2026-11-11T09:01:00Z","message":{"content":[{"type":"text","text":"Siehe https://claude.ai/artifact/TALK"}]}}"#.to_string(),
            r#"{"type":"user","timestamp":"2026-11-11T09:02:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"x","content":"Published at https://claude.ai/artifact/STRAY"}]}}"#.to_string(),
        ];
        let file = transcript(&lines.each_ref().map(String::as_str));
        assert!(session_artifacts(file.path()).is_empty());
    }

    #[test]
    fn what_a_subagent_published_belongs_to_the_session() {
        let folder = tempfile::tempdir().unwrap();
        let main = folder.path().join("abc.jsonl");
        std::fs::write(&main, "").unwrap();
        std::fs::create_dir_all(folder.path().join("abc/subagents")).unwrap();
        let lines = [
            call("c1", r#"{"file_path":"/t/a.html","description":"Entwurf"}"#, "2026-11-11T09:00:00Z"),
            answer("c1", "Published /t/a.html at https://claude.ai/artifact/SUB (Version 1)", "2026-11-11T09:00:01Z"),
        ];
        std::fs::write(folder.path().join("abc/subagents/agent-1.jsonl"), lines.join("\n")).unwrap();
        let found = session_artifacts(&main);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title.as_deref(), Some("Entwurf"));
    }
}
