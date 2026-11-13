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
    // `Published <file> at <url> (Version 1, …)`: the URL is what follows the last " at " before
    // the version note (the file's path may hold " at " itself).
    let head = answer.lines().next()?;
    let head = head.split_once(" (Version").map_or(head, |(before, _)| before);
    let url = head.rsplit_once(" at ")?.1.trim().trim_end_matches(['.', ',']);
    is_artifact_url(url).then(|| url.to_string())
}

/// `https://claude.ai/artifact/<id>` or `https://claude.ai/code/artifact/<id>`, nothing else:
/// Brain opens this URL in the browser. The host must be exactly `claude.ai`.
fn is_artifact_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let (host, path) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    if host != "claude.ai" {
        return false;
    }
    let Some(id) = path.strip_prefix("/artifact/").or_else(|| path.strip_prefix("/code/artifact/")) else { return false };
    let end = id.find(['/', '?', '#']).unwrap_or(id.len());
    let (id, tail) = id.split_at(end);
    let tail_ok = tail.is_empty() || tail == "/" || tail.starts_with(['?', '#']) || tail.starts_with('/') && tail[1..].starts_with(['?', '#']);
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && tail_ok
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
    fn only_claude_ai_artifact_urls_are_taken() {
        for good in ["https://claude.ai/artifact/4xSG-vPf2", "https://claude.ai/code/artifact/0b1c-22", "https://claude.ai/artifact/abc/", "https://claude.ai/artifact/abc?x=1", "https://claude.ai/artifact/abc#top"] {
            assert!(is_artifact_url(good), "{good}");
        }
        for bad in [
            "https://evil.example/?x=claude.ai/artifact/1",
            "https://claude.ai.evil.com/artifact/1",
            "http://claude.ai/artifact/1",
            "https://claude.ai/chat/1",
            "https://evil.com/claude.ai/artifact/x",
            "https://claude.ai/artifact/",
            "https://claude.ai/artifact/a_b",
            "https://claude.ai@evil.com/artifact/1",
        ] {
            assert!(!is_artifact_url(bad), "{bad}");
        }
        assert_eq!(published_url("Published /t/x at y.html at https://claude.ai/artifact/AAA (Version 1, at https://evil.com)"), Some("https://claude.ai/artifact/AAA".into()));
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
