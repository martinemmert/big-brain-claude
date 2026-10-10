//! Councils ("Konsil") of a session: the konsil skill lets two subagents, the lazy senior and
//! his pragmatic buddy, debate a topic while the session narrates. The skill marks the council
//! with `brain konsil start|stand|end`; everything else is read from the transcript: the
//! statements from the subagents' replies, the user's prompts, the narrator's `*italic*` lines.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::transcript::{classify_prompt, Prompt};

/// What a council's Bash call starts with.
const MARKER: &str = "brain konsil";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Figure {
    /// The lazy senior: argues against building anything that isn't needed.
    Lazy,
    /// The pragmatic buddy: optimistic, wants to ship.
    Buddy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub figure: Figure,
    pub text: String,
}

/// Where the two stand after a round (`brain konsil stand --einig … --strittig …`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stand {
    pub agreed: Vec<String>,
    pub disputed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Round {
    /// What the user said to the table during this round.
    pub prompts: Vec<String>,
    pub statements: Vec<Statement>,
    /// Set when the round was closed with `brain konsil stand`.
    pub stand: Option<Stand>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Council {
    pub topic: String,
    pub started: Option<DateTime<Utc>>,
    pub rounds: Vec<Round>,
    /// The narrator's latest scene line.
    pub scene: Option<String>,
    /// `brain konsil end "<decision>"`, with when.
    pub decision: Option<(String, Option<DateTime<Utc>>)>,
    /// Figures whose answer is still out.
    pub writing: Vec<Figure>,
}

impl Council {
    /// The latest stand of any round.
    pub fn stand(&self) -> Option<&Stand> {
        self.rounds.iter().rev().find_map(|r| r.stand.as_ref())
    }

    /// The latest statement of a figure.
    pub fn last_word(&self, figure: Figure) -> Option<&str> {
        self.rounds.iter().rev().flat_map(|r| r.statements.iter().rev()).find(|s| s.figure == figure).map(|s| s.text.as_str())
    }
}

/// Every council in the transcript, oldest first. Transcripts without a council are only
/// searched for the marker, not parsed.
pub fn councils(transcript: &Path) -> Vec<Council> {
    let Some(offset) = first_marker(transcript) else { return Vec::new() };
    let Ok(mut file) = std::fs::File::open(transcript) else { return Vec::new() };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return Vec::new();
    }
    let mut reader = Reader::default();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if let Ok(entry) = serde_json::from_str::<Value>(&line) {
            reader.entry(&entry);
        }
    }
    reader.finish()
}

/// The start of the transcript line that first mentions the marker.
fn first_marker(transcript: &Path) -> Option<u64> {
    let mut bytes = Vec::new();
    std::fs::File::open(transcript).ok()?.read_to_end(&mut bytes).ok()?;
    let at = bytes.windows(MARKER.len()).position(|w| w == MARKER.as_bytes())?;
    let line_start = bytes[..at].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    Some(line_start as u64)
}

#[derive(Default)]
struct Reader {
    done: Vec<Council>,
    open: Option<Council>,
    /// Agent tool call id → figure, until its result names the agent.
    calls: HashMap<String, Figure>,
    /// Agent id → figure.
    agents: HashMap<String, Figure>,
}

impl Reader {
    fn entry(&mut self, entry: &Value) {
        if entry.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            return;
        }
        let ts = entry.get("timestamp").and_then(Value::as_str).and_then(|t| t.parse().ok());
        // A background subagent's report, handed back to the session (a meta entry).
        if let Some(origin) = entry.get("origin").filter(|o| o.get("handback").and_then(Value::as_bool) == Some(true)) {
            let agent = origin.get("from").and_then(Value::as_str).unwrap_or_default();
            let body = origin.get("body").and_then(Value::as_str).unwrap_or_default();
            if let Some(&figure) = self.agents.get(agent) {
                self.statement(figure, &report_of(body));
            }
            return;
        }
        let Some(content) = entry.get("message").and_then(|m| m.get("content")) else { return };
        match entry.get("type").and_then(Value::as_str).unwrap_or_default() {
            "assistant" => {
                for block in content.as_array().into_iter().flatten() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => self.narration(block.get("text").and_then(Value::as_str).unwrap_or_default()),
                        Some("tool_use") => self.tool_use(block, ts),
                        _ => {}
                    }
                }
            }
            "user" if !entry.get("isMeta").and_then(Value::as_bool).unwrap_or(false) => match content {
                Value::Array(blocks) if blocks.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) => {
                    for block in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
                        self.tool_result(block, entry.get("toolUseResult"));
                    }
                }
                Value::String(text) => self.user_text(text),
                Value::Array(blocks) => {
                    let text: Vec<&str> = blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("text")).filter_map(|b| b.get("text").and_then(Value::as_str)).collect();
                    self.user_text(&text.join("\n"));
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn tool_use(&mut self, block: &Value, ts: Option<DateTime<Utc>>) {
        let input = block.get("input").unwrap_or(&Value::Null);
        let field = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or_default();
        match block.get("name").and_then(Value::as_str).unwrap_or_default() {
            "Bash" => {
                if let Some(args) = marker_args(field("command")) {
                    self.marker(&args, ts);
                }
            }
            "Agent" | "Task" => {
                if let Some(figure) = figure_of(field("description"), field("prompt")) {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                    self.calls.insert(id.to_string(), figure);
                    self.writing(figure, true);
                }
            }
            "SendMessage" => {
                if let Some(&figure) = self.agents.get(field("to")) {
                    self.writing(figure, true);
                }
            }
            _ => {}
        }
    }

    fn tool_result(&mut self, block: &Value, result: Option<&Value>) {
        let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or_default();
        let Some(figure) = self.calls.remove(id) else { return };
        let agent = result.and_then(|r| r.get("agentId")).and_then(Value::as_str);
        if let Some(agent) = agent {
            self.agents.insert(agent.to_string(), figure);
        }
        // A subagent in the foreground answers in its tool result.
        let launched = result.and_then(|r| r.get("status")).and_then(Value::as_str) == Some("async_launched");
        if !launched {
            let text = match block.get("content") {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .filter(|t| !t.trim_start().starts_with("agentId:"))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                _ => String::new(),
            };
            self.statement(figure, &text);
        }
    }

    fn user_text(&mut self, text: &str) {
        // A background subagent hands its report back as a message to the session.
        if let Some((agent, report)) = agent_message(text) {
            if let Some(&figure) = self.agents.get(agent) {
                self.statement(figure, &report);
            }
            return;
        }
        if let Some((agent, report)) = task_result(text) {
            if let Some(&figure) = self.agents.get(agent) {
                self.statement(figure, &report);
            }
            return;
        }
        if text.contains("<cross-session-message") {
            return;
        }
        if let (Some(council), Some(Prompt::User(prompt))) = (self.open.as_mut(), classify_prompt(text)) {
            // The user's words open the next round once the table has spoken.
            current_round(council, |r| !r.statements.is_empty()).prompts.push(prompt);
        }
    }

    fn narration(&mut self, text: &str) {
        let Some(council) = self.open.as_mut() else { return };
        for line in text.lines().map(str::trim) {
            if let Some(scene) = line.strip_prefix('*').and_then(|l| l.strip_suffix('*')) {
                if !scene.starts_with('*') && !scene.trim().is_empty() {
                    council.scene = Some(scene.trim().to_string());
                }
            }
        }
    }

    fn marker(&mut self, args: &[String], ts: Option<DateTime<Utc>>) {
        let Some((command, rest)) = args.split_first() else { return };
        match command.as_str() {
            "start" => {
                self.close();
                self.open = Some(Council { topic: rest.join(" "), started: ts, rounds: Vec::new(), scene: None, decision: None, writing: Vec::new() });
            }
            "stand" => {
                let Some(council) = self.open.as_mut() else { return };
                let mut stand = Stand::default();
                let mut options = rest.iter();
                while let Some(option) = options.next() {
                    let (name, value) = match option.split_once('=') {
                        Some((name, value)) => (name, value.to_string()),
                        None => (option.as_str(), options.next().cloned().unwrap_or_default()),
                    };
                    let points = value.split(';').map(str::trim).filter(|p| !p.is_empty()).map(str::to_string);
                    match name {
                        "--einig" | "--agreed" => stand.agreed.extend(points),
                        "--strittig" | "--disputed" => stand.disputed.extend(points),
                        _ => {}
                    }
                }
                // Anything said since the last stand opened a new round, so the last round is
                // this one; a stand right after another one corrects it.
                match council.rounds.last_mut() {
                    Some(round) => round.stand = Some(stand),
                    None => council.rounds.push(Round { stand: Some(stand), ..Round::default() }),
                }
            }
            "end" => {
                if let Some(mut council) = self.open.take() {
                    council.decision = Some((rest.join(" "), ts));
                    council.writing.clear();
                    self.done.push(council);
                }
            }
            _ => {}
        }
    }

    fn statement(&mut self, figure: Figure, text: &str) {
        let Some(council) = self.open.as_mut() else { return };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        council.writing.retain(|f| *f != figure);
        // Each figure speaks once per round: a second word opens the next one.
        let round = current_round(council, |r| r.statements.iter().any(|s| s.figure == figure));
        round.statements.push(Statement { figure, text: text.to_string() });
    }

    fn writing(&mut self, figure: Figure, writing: bool) {
        let Some(council) = self.open.as_mut() else { return };
        council.writing.retain(|f| *f != figure);
        if writing {
            council.writing.push(figure);
        }
    }

    fn close(&mut self) {
        if let Some(council) = self.open.take() {
            self.done.push(council);
        }
    }

    fn finish(mut self) -> Vec<Council> {
        self.close();
        self.done
    }
}

/// The round to add to: the last one, or a new one once the last has its stand or is `done`.
fn current_round(council: &mut Council, done: impl Fn(&Round) -> bool) -> &mut Round {
    if council.rounds.last().is_none_or(|r| r.stand.is_some() || done(r)) {
        council.rounds.push(Round::default());
    }
    council.rounds.last_mut().expect("just pushed")
}

/// Which figure an Agent call plays, by the skill's description (`Konsil: lazy senior …`) or
/// the persona file its prompt names.
fn figure_of(description: &str, prompt: &str) -> Option<Figure> {
    let description = description.to_lowercase();
    if description.starts_with("konsil") {
        if description.contains("lazy") {
            return Some(Figure::Lazy);
        }
        if description.contains("buddy") {
            return Some(Figure::Buddy);
        }
    }
    if prompt.contains("lazy-senior/reviewer.md") {
        return Some(Figure::Lazy);
    }
    if prompt.contains("pragmatic-buddy.md") {
        return Some(Figure::Buddy);
    }
    None
}

/// `brain konsil stand --einig "a; b"` in a shell command → `["stand", "--einig", "a; b"]`.
/// Quotes and backslashes are undone; the words end at the first `;`, `&`, `|`, `<`, `>` or line
/// break outside quotes.
fn marker_args(command: &str) -> Option<Vec<String>> {
    let at = command.find(MARKER)?;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = command[at + MARKER.len()..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                word.extend(chars.by_ref().take_while(|&c| c != '\''));
            }
            '"' => {
                in_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(next @ ('"' | '\\' | '$' | '`')) => word.push(next),
                            Some(next) => {
                                word.push('\\');
                                word.push(next);
                            }
                            None => {}
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.extend(chars.next());
            }
            ';' | '&' | '|' | '<' | '>' | '\n' => {
                // `2>/dev/null`: the 2 is the redirection's, not a word.
                if matches!(c, '<' | '>') && in_word && word.chars().all(|c| c.is_ascii_digit()) {
                    in_word = false;
                }
                break;
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Some(words)
}

/// `<agent-message from="a1">…The report follows:\n  report…</agent-message>` → `("a1", "report…")`.
fn agent_message(text: &str) -> Option<(&str, String)> {
    let start = text.find("<agent-message from=\"")? + "<agent-message from=\"".len();
    let agent = &text[start..start + text[start..].find('"')?];
    let body_start = start + text[start..].find('>')? + 1;
    let body = &text[body_start..body_start + text[body_start..].find("</agent-message>")?];
    Some((agent, report_of(body)))
}

/// The report in a hand-back, after the harness's frame.
fn report_of(body: &str) -> String {
    let report = match body.find("The report follows:") {
        Some(at) => &body[at + "The report follows:".len()..],
        None => body,
    };
    dedent(report)
}

/// A task notification that carries the subagent's report itself.
fn task_result(text: &str) -> Option<(&str, String)> {
    if !text.contains("<task-notification>") {
        return None;
    }
    let between = |open: &str, close: &str| -> Option<&str> {
        let start = text.find(open)? + open.len();
        Some(&text[start..start + text[start..].find(close)?])
    };
    let agent = between("<task-id>", "</task-id>")?.trim();
    let result = between("<result>", "</result>")?.trim();
    if between("<status>", "</status>")?.trim() != "completed" || result.starts_with("This agent's report was delivered") {
        return None;
    }
    Some((agent, result.to_string()))
}

/// The harness indents every line of a hand-back by two spaces.
fn dedent(report: &str) -> String {
    report.lines().map(|l| l.strip_prefix("  ").unwrap_or(l)).collect::<Vec<_>>().join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn transcript(lines: &[Value]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        std::fs::write(file.path(), text.join("\n")).unwrap();
        file
    }

    fn bash(command: &str) -> Value {
        json!({"type":"assistant","timestamp":"2026-10-10T18:00:00Z","message":{"content":[{"type":"tool_use","id":"b","name":"Bash","input":{"command":command}}]}})
    }

    fn launch(id: &str, description: &str) -> Value {
        json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":id,"name":"Agent","input":{"description":description,"prompt":"…"}}]}})
    }

    fn launched(id: &str, agent: &str) -> Value {
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":id,"content":[{"type":"text","text":"Async agent launched successfully."}]}]},
            "toolUseResult":{"isAsync":true,"status":"async_launched","agentId":agent}})
    }

    fn hand_back(agent: &str, report: &str) -> Value {
        let indented: Vec<String> = report.lines().map(|l| format!("  {l}")).collect();
        let text = format!("Another Claude session sent a message:\n<agent-message from=\"{agent}\">\n[Subagent hand-back] … The report follows:\n{}\n</agent-message>", indented.join("\n"));
        json!({"type":"user","message":{"content":text}})
    }

    fn say(text: &str) -> Value {
        json!({"type":"assistant","message":{"content":[{"type":"text","text":text}]}})
    }

    fn prompt(text: &str) -> Value {
        json!({"type":"user","message":{"content":text}})
    }

    #[test]
    fn no_marker_no_council() {
        let file = transcript(&[say("hello"), prompt("hi")]);
        assert!(councils(file.path()).is_empty());
    }

    #[test]
    fn a_council_with_two_rounds_and_a_decision() {
        let file = transcript(&[
            prompt("frag das konsil"),
            bash(r#"brain konsil start "Eigene API-App?" >/dev/null 2>&1 || true"#),
            launch("t1", "Konsil: lazy senior opening"),
            launch("t2", "Konsil: pragmatic buddy opening"),
            launched("t1", "a1"),
            launched("t2", "a2"),
            say("*Beide beugen sich über den Code.*\n\nDie Eröffnungen kommen gleich."),
            hand_back("a1", "Weder noch.\n\n- **Streichen**"),
            hand_back("a2", "Weder noch, zumindest heute nicht."),
            bash(r#"brain konsil stand --einig "Dieselbe App; kein Token" --strittig "Nachschicken? (faul: ja, kumpel: nein)""#),
            prompt("Was ist mit Apple Pay?"),
            json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"s1","name":"SendMessage","input":{"to":"a1","message":"…"}}]}}),
            hand_back("a1", "Später."),
            bash("brain konsil stand --einig 'Dieselbe App' --strittig ''"),
            say("*Die beiden nicken sich zu.*\n\nWas tust du?"),
            bash(r#"brain konsil end "Gleiche App, ohne Token""#),
            say("*Der Tisch löst sich auf.*"),
        ]);
        let found = councils(file.path());
        assert_eq!(found.len(), 1);
        let council = &found[0];
        assert_eq!(council.topic, "Eigene API-App?");
        assert_eq!(council.rounds.len(), 2);
        assert_eq!(council.rounds[0].statements, [
            Statement { figure: Figure::Lazy, text: "Weder noch.\n\n- **Streichen**".into() },
            Statement { figure: Figure::Buddy, text: "Weder noch, zumindest heute nicht.".into() },
        ]);
        assert_eq!(council.rounds[0].stand.as_ref().unwrap().agreed, ["Dieselbe App", "kein Token"]);
        assert_eq!(council.rounds[0].stand.as_ref().unwrap().disputed, ["Nachschicken? (faul: ja, kumpel: nein)"]);
        assert_eq!(council.rounds[1].prompts, ["Was ist mit Apple Pay?"]);
        assert_eq!(council.rounds[1].statements.len(), 1);
        assert!(council.rounds[1].stand.as_ref().unwrap().disputed.is_empty());
        assert_eq!(council.scene.as_deref(), Some("Die beiden nicken sich zu."));
        assert_eq!(council.decision.as_ref().unwrap().0, "Gleiche App, ohne Token");
        assert_eq!(council.last_word(Figure::Lazy), Some("Später."));
        assert!(council.writing.is_empty());
    }

    #[test]
    fn a_figure_speaking_again_opens_the_next_round() {
        let file = transcript(&[
            bash("brain konsil start X"),
            launch("t1", "Konsil: lazy senior opening"),
            launch("t2", "Konsil: pragmatic buddy opening"),
            launched("t1", "a1"),
            launched("t2", "a2"),
            hand_back("a1", "Eröffnung Faul"),
            hand_back("a2", "Eröffnung Kumpel"),
            hand_back("a1", "Antwort Faul"),
            hand_back("a2", "Antwort Kumpel"),
            bash("brain konsil stand --einig A"),
        ]);
        let council = &councils(file.path())[0];
        let texts: Vec<Vec<&str>> = council.rounds.iter().map(|r| r.statements.iter().map(|s| s.text.as_str()).collect()).collect();
        assert_eq!(texts, [vec!["Eröffnung Faul", "Eröffnung Kumpel"], vec!["Antwort Faul", "Antwort Kumpel"]]);
        assert!(council.rounds[0].stand.is_none());
        assert_eq!(council.rounds[1].stand.as_ref().unwrap().agreed, ["A"]);
    }

    #[test]
    fn a_hand_back_is_read_from_its_origin() {
        let file = transcript(&[
            bash("brain konsil start X"),
            launch("t1", "Konsil: lazy senior opening"),
            launched("t1", "a1"),
            json!({"type":"user","isMeta":true,"message":{"content":"Another Claude session sent a message: …"},
                "origin":{"kind":"peer","from":"a1","handback":true,"body":"[Subagent hand-back] … The report follows:\n  Nein.\n  \n  - Punkt"}}),
        ]);
        let council = &councils(file.path())[0];
        assert_eq!(council.last_word(Figure::Lazy), Some("Nein.\n\n- Punkt"));
        assert!(council.rounds[0].prompts.is_empty());
    }

    #[test]
    fn a_running_council_knows_who_still_writes() {
        let file = transcript(&[
            bash("brain konsil start Monorepo"),
            launch("t1", "Konsil: lazy senior opening"),
            launch("t2", "Konsil: pragmatic buddy opening"),
            launched("t1", "a1"),
            launched("t2", "a2"),
            hand_back("a2", "Klar, machen."),
        ]);
        let council = &councils(file.path())[0];
        assert_eq!(council.topic, "Monorepo");
        assert_eq!(council.writing, [Figure::Lazy]);
        assert!(council.decision.is_none());
        assert!(council.stand().is_none());
    }

    #[test]
    fn a_foreground_subagent_answers_in_its_tool_result() {
        let file = transcript(&[
            bash("brain konsil start X"),
            launch("t1", "Konsil: lazy senior opening"),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"Nein."},{"type":"text","text":"agentId: a1 (use SendMessage …)"}]}]},
                "toolUseResult":{"status":"completed","agentId":"a1"}}),
        ]);
        assert_eq!(councils(file.path())[0].last_word(Figure::Lazy), Some("Nein."));
    }

    #[test]
    fn a_new_start_closes_the_open_council() {
        let file = transcript(&[bash("brain konsil start Eins"), bash("brain konsil start Zwei")]);
        let found = councils(file.path());
        assert_eq!(found.iter().map(|c| c.topic.as_str()).collect::<Vec<_>>(), ["Eins", "Zwei"]);
        assert!(found[0].decision.is_none());
    }

    #[test]
    fn shell_words_are_unquoted() {
        assert_eq!(marker_args(r#"brain konsil stand --einig "a \"b\"; c" --strittig 'd' 2>/dev/null"#).unwrap(), ["stand", "--einig", "a \"b\"; c", "--strittig", "d"]);
        assert_eq!(marker_args("cd x && brain konsil end Gleiche\\ App; true").unwrap(), ["end", "Gleiche App"]);
        assert!(marker_args("brain report --done x").is_none());
    }
}
