//! A review: the comments a person leaves on the lines Claude changed, collected per session and
//! sent as one message. Kept in `~/.claude-brain/reviews/<account>-<session id>.json`, so drafts
//! survive a restart.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::state::{Ask, Phase, SessionKey};

/// The longest code line the message quotes.
const QUOTE_CHARS: usize = 90;

/// What a comment is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    /// The whole file.
    File,
    /// One line of the diff: its number in the new file and in the old one (a removed line has
    /// only the old number, an added one only the new).
    Line { new: Option<u32>, old: Option<u32> },
}

/// Where a comment stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Written, not sent.
    Draft,
    /// Waits for the session to want a new prompt.
    Queued,
    /// Handed to the session.
    Sent,
    /// Sent, and Claude changed the line since.
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    /// Absolute path of the file.
    pub path: String,
    /// Path relative to the session's folder: what the message says.
    pub relative: String,
    pub place: Place,
    /// The code line when the comment was written: how it is found again.
    pub anchor: String,
    pub text: String,
    pub state: State,
    /// When it was sent (Unix ms).
    #[serde(default)]
    pub sent_ms: Option<i64>,
}

impl Comment {
    /// Whether it is still to send or on its way (not yet handed over).
    pub fn is_open(&self) -> bool {
        matches!(self.state, State::Draft | State::Queued)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub account: String,
    pub session_id: String,
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// The message of the last review that went out, and when.
    #[serde(default)]
    pub last_message: Option<String>,
    #[serde(default)]
    pub last_sent_ms: Option<i64>,
    #[serde(default)]
    pub last_count: usize,
}

impl Review {
    pub fn new(key: &SessionKey) -> Self {
        Self { account: key.account.clone(), session_id: key.id.clone(), next_id: 1, ..Self::default() }
    }

    pub fn key(&self) -> SessionKey {
        SessionKey { account: self.account.clone(), id: self.session_id.clone() }
    }

    pub fn add(&mut self, path: &str, relative: &str, place: Place, anchor: &str, text: &str) -> u64 {
        self.next_id = self.next_id.max(self.comments.iter().map(|c| c.id).max().unwrap_or(0)) + 1;
        let id = self.next_id;
        self.comments.push(Comment {
            id,
            path: path.to_string(),
            relative: relative.to_string(),
            place,
            anchor: anchor.to_string(),
            text: text.trim().to_string(),
            state: State::Draft,
            sent_ms: None,
        });
        id
    }

    /// Changes the text of a draft; false when there is none such.
    pub fn edit(&mut self, id: u64, text: &str) -> bool {
        match self.comments.iter_mut().find(|c| c.id == id && c.state == State::Draft) {
            Some(comment) => {
                comment.text = text.trim().to_string();
                true
            }
            None => false,
        }
    }

    /// Removes a draft; false when there is none such.
    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.comments.len();
        self.comments.retain(|c| !(c.id == id && c.state == State::Draft));
        self.comments.len() != before
    }

    pub fn count(&self, state: State) -> usize {
        self.comments.iter().filter(|c| c.state == state).count()
    }

    /// Drafts and queued comments: what the next send takes.
    pub fn open_count(&self) -> usize {
        self.comments.iter().filter(|c| c.is_open()).count()
    }

    pub fn has_queued(&self) -> bool {
        self.count(State::Queued) > 0
    }

    /// Drops the drafts. Returns how many.
    pub fn discard_drafts(&mut self) -> usize {
        let before = self.comments.len();
        self.comments.retain(|c| c.state != State::Draft);
        before - self.comments.len()
    }

    /// Drafts wait for their turn.
    pub fn queue_drafts(&mut self) {
        for comment in self.comments.iter_mut().filter(|c| c.state == State::Draft) {
            comment.state = State::Queued;
        }
    }

    /// Takes the queued comments back to drafts.
    pub fn unqueue(&mut self) {
        for comment in self.comments.iter_mut().filter(|c| c.state == State::Queued) {
            comment.state = State::Draft;
        }
    }

    /// Hands every draft and queued comment over: they become sent, what was sent before is
    /// dropped, and the message that goes to the session is returned. `file_order` is the order
    /// of the sheet's list of files.
    pub fn send(&mut self, now_ms: i64, file_order: &[String]) -> Option<String> {
        let message = self.pending_message(file_order)?;
        self.comments.retain(Comment::is_open);
        for comment in &mut self.comments {
            comment.state = State::Sent;
            comment.sent_ms = Some(now_ms);
        }
        self.last_count = self.comments.len();
        self.last_message = Some(message.clone());
        self.last_sent_ms = Some(now_ms);
        Some(message)
    }

    /// The message the drafts and queued comments would make: the preview of what goes out.
    pub fn pending_message(&self, file_order: &[String]) -> Option<String> {
        let open: Vec<&Comment> = self.comments.iter().filter(|c| c.is_open()).collect();
        (!open.is_empty()).then(|| message(&open, file_order))
    }

    /// The file's diff was read again: sent comments whose line is no longer there are marked as
    /// changed by Claude. Returns how many.
    pub fn mark_changed(&mut self, path: &str, rows: &[AnchorRow]) -> usize {
        let mut marked = 0;
        for comment in self.comments.iter_mut().filter(|c| c.path == path && c.state == State::Sent) {
            if matches!(comment.place, Place::Line { .. }) && locate(comment, rows) == Found::Gone {
                comment.state = State::Changed;
                marked += 1;
            }
        }
        marked
    }

    /// `~/.claude-brain/reviews`.
    pub fn dir(home: &Path) -> PathBuf {
        home.join(".claude-brain/reviews")
    }

    fn file(dir: &Path, key: &SessionKey) -> PathBuf {
        let safe = |s: &str| s.replace(['/', '\\'], "_");
        dir.join(format!("{}-{}.json", safe(&key.account), safe(&key.id)))
    }

    /// Removes a session's review file (none is fine).
    pub fn delete_file(dir: &Path, key: &SessionKey) -> std::io::Result<()> {
        match std::fs::remove_file(Self::file(dir, key)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Writes the review; one without comments and nothing sent removes its file.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let file = Self::file(dir, &self.key());
        if self.comments.is_empty() && self.last_message.is_none() {
            return match std::fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        std::fs::create_dir_all(dir)?;
        let temp = file.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, file)
    }

    /// Every review on disk. A queued comment stays queued: it goes out once Brain sees its
    /// session wait for a prompt.
    pub fn load_all(dir: &Path) -> Vec<Review> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        entries
            .filter_map(|e| std::fs::read(e.ok()?.path()).ok())
            .filter_map(|bytes| serde_json::from_slice::<Review>(&bytes).ok())
            .collect()
    }
}

/// Whether a session wants a new prompt: its turn ended, it asks nothing and wants no permission.
/// A question or a permission is not "done"; a review would be the wrong answer to it.
pub fn waits_for_prompt(phase: Phase, ask: Option<Ask>) -> bool {
    phase == Phase::YourTurn && ask.is_none()
}

/// Whether a waiting review may go out now: the session wants a prompt and the user hasn't
/// started one in Brain's terminal.
pub fn may_deliver(waits_for_prompt: bool, typing: bool) -> bool {
    waits_for_prompt && !typing
}

/// What the user typed into a session's terminal since the last submit, as far as Brain can tell
/// from the bytes it passes on. A rough count (a character in, a backspace out), so a prompt
/// counts as begun unless it is surely empty again.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TypedInput {
    len: usize,
    in_paste: bool,
    lone_escape: bool,
}

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

impl TypedInput {
    /// Whether something typed is waiting to be submitted.
    pub fn begun(&self) -> bool {
        self.len > 0
    }

    /// Takes bytes written to the terminal by the user: keys, a paste, a dropped path, a commit
    /// of the input method.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            if self.in_paste {
                match find(&bytes[i..], PASTE_END) {
                    Some(end) => {
                        self.len += chars(&bytes[i..i + end]);
                        self.in_paste = false;
                        i += end + PASTE_END.len();
                    }
                    None => {
                        self.len += chars(&bytes[i..]);
                        i = bytes.len();
                    }
                }
                continue;
            }
            let byte = bytes[i];
            if byte == 0x1b {
                i += self.escape(&bytes[i..]);
                continue;
            }
            self.lone_escape = false;
            match byte {
                // Return submits; Ctrl+C empties the line.
                b'\r' | 0x03 => self.len = 0,
                // Backspace.
                0x7f | 0x08 => self.len = self.len.saturating_sub(1),
                // Ctrl+J is a new line in the prompt.
                b'\n' => self.len += 1,
                0x00..=0x1f => {}
                _ => self.len += usize::from(byte & 0xc0 != 0x80),
            }
            i += 1;
        }
    }

    /// An escape at the start of `rest`; returns how many bytes it takes.
    fn escape(&mut self, rest: &[u8]) -> usize {
        match rest.get(1) {
            // A lone ESC; two in a row empty Claude Code's prompt.
            None => {
                if self.lone_escape {
                    self.len = 0;
                }
                self.lone_escape = !self.lone_escape;
                1
            }
            Some(b'[') => {
                self.lone_escape = false;
                if rest.starts_with(PASTE_START) {
                    self.in_paste = true;
                    return PASTE_START.len();
                }
                // Cursor keys and the like: up to the final byte.
                rest.iter().skip(2).position(|b| (0x40..=0x7e).contains(b)).map_or(rest.len(), |p| p + 3)
            }
            Some(b'O') => {
                self.lone_escape = false;
                rest.len().min(3)
            }
            // ESC CR is a new line (⇧⏎, ⌥⏎).
            Some(b'\r') => {
                self.lone_escape = false;
                self.len += 1;
                2
            }
            Some(_) => {
                self.lone_escape = false;
                2
            }
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn chars(bytes: &[u8]) -> usize {
    bytes.iter().filter(|b| **b & 0xc0 != 0x80).count()
}

/// One line of a diff, as far as finding a comment's line again needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorRow<'a> {
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: &'a str,
}

/// Where a comment's line is in a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// At its number, with its text.
    At(usize),
    /// Elsewhere: the number moved, the text is the same.
    Moved(usize),
    /// Nowhere.
    Gone,
}

/// Finds the line a comment was written on: at its number when the text still matches, else the
/// line with the same text nearest to that number. A comment on a removed line looks among the
/// removed lines, one on the whole file has no line.
pub fn locate(comment: &Comment, rows: &[AnchorRow]) -> Found {
    let (number, of): (u32, fn(&AnchorRow) -> Option<u32>) = match comment.place {
        Place::File => return Found::Gone,
        Place::Line { new: Some(n), .. } => (n, |r| r.new),
        Place::Line { new: None, old: Some(o) } => (o, |r| if r.new.is_none() { r.old } else { None }),
        Place::Line { new: None, old: None } => return Found::Gone,
    };
    let anchor = comment.anchor.trim();
    let same = |r: &AnchorRow| r.text.trim() == anchor;
    if let Some(at) = rows.iter().position(|r| of(r) == Some(number) && same(r)) {
        return Found::At(at);
    }
    rows.iter()
        .enumerate()
        .filter(|(_, r)| same(r))
        .filter_map(|(i, r)| Some((i, of(r)?.abs_diff(number))))
        .min_by_key(|(_, distance)| *distance)
        .map_or(Found::Gone, |(i, _)| Found::Moved(i))
}

/// The code line as the message quotes it: trimmed, one line, shortened.
fn quote(code: &str) -> String {
    let code = code.trim();
    if code.chars().count() > QUOTE_CHARS {
        format!("{}…", code.chars().take(QUOTE_CHARS - 1).collect::<String>())
    } else {
        code.to_string()
    }
}

/// Where a comment is, as the message says it: `src/a.rs:22`, `src/a.rs (ganze Datei)`,
/// `src/a.rs, entfernte Zeile 14`.
pub fn where_is(comment: &Comment) -> String {
    match comment.place {
        Place::File => format!("{} (ganze Datei)", comment.relative),
        Place::Line { new: Some(n), .. } => format!("{}:{n}", comment.relative),
        Place::Line { new: None, old: Some(o) } => format!("{}, entfernte Zeile {o}", comment.relative),
        Place::Line { new: None, old: None } => comment.relative.clone(),
    }
}

/// The comments in the sheet's order: file by file (as `file_order` lists them), the whole file
/// first, then the removed lines, then the new ones by number.
pub fn ordered<'a>(comments: &[&'a Comment], file_order: &[String]) -> Vec<&'a Comment> {
    let mut list = comments.to_vec();
    // A file the order doesn't list (the comments of a session that isn't open) goes by its path.
    let rank = |c: &'a Comment| -> (usize, &'a str, u8, u32) {
        let file = file_order.iter().position(|p| *p == c.path).unwrap_or(usize::MAX);
        let by_path = if file == usize::MAX { c.relative.as_str() } else { "" };
        match c.place {
            Place::File => (file, by_path, 0, 0),
            Place::Line { new: None, old } => (file, by_path, 1, old.unwrap_or(0)),
            Place::Line { new: Some(n), .. } => (file, by_path, 2, n),
        }
    };
    list.sort_by_key(|c| (rank(c), c.id));
    list
}

/// The message Claude gets:
///
/// ```text
/// Review zu deinen Änderungen (2 Kommentare):
///
/// 1. src/a.rs:22 → `pub burst: u32,`
///    Bitte als Option.
/// ```
pub fn message(comments: &[&Comment], file_order: &[String]) -> String {
    let list = ordered(comments, file_order);
    let n = list.len();
    let head = format!("Review zu deinen Änderungen ({n} {}):", if n == 1 { "Kommentar" } else { "Kommentare" });
    let items = list.iter().enumerate().map(|(i, c)| {
        let mut lead = format!("{}. {}", i + 1, where_is(c));
        if c.place != Place::File {
            lead.push_str(&format!(" → `{}`", quote(&c.anchor)));
        }
        let body = c.text.trim().lines().map(|l| format!("   {l}")).collect::<Vec<_>>().join("\n");
        format!("{lead}\n{body}")
    });
    std::iter::once(head).chain(items).collect::<Vec<_>>().join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SessionKey {
        SessionKey { account: "main".into(), id: "ab-12".into() }
    }

    fn line(review: &mut Review, path: &str, new: Option<u32>, old: Option<u32>, code: &str, text: &str) -> u64 {
        review.add(&format!("/p/{path}"), path, Place::Line { new, old }, code, text)
    }

    fn row(old: Option<u32>, new: Option<u32>, text: &str) -> AnchorRow<'_> {
        AnchorRow { old, new, text }
    }

    #[test]
    fn the_message_lists_the_comments_file_by_file_with_path_line_and_code() {
        let mut review = Review::new(&key());
        line(&mut review, "src/mod.rs", Some(3), Some(3), "pub mod rate_limit;", "Mit Doku.");
        line(&mut review, "src/config.rs", Some(22), None, "    pub burst: u32,", "Bitte als Option,\nDefault 100.");
        review.add("/p/src/config.rs", "src/config.rs", Place::File, "", "Insgesamt zu groß.");
        line(&mut review, "src/config.rs", None, Some(14), "let old = 1;", "Warum weg?");
        let order = vec!["/p/src/config.rs".to_string(), "/p/src/mod.rs".to_string()];
        let all: Vec<&Comment> = review.comments.iter().collect();

        assert_eq!(
            message(&all, &order),
            "Review zu deinen Änderungen (4 Kommentare):\n\n\
             1. src/config.rs (ganze Datei)\n   Insgesamt zu groß.\n\n\
             2. src/config.rs, entfernte Zeile 14 → `let old = 1;`\n   Warum weg?\n\n\
             3. src/config.rs:22 → `pub burst: u32,`\n   Bitte als Option,\n   Default 100.\n\n\
             4. src/mod.rs:3 → `pub mod rate_limit;`\n   Mit Doku."
        );
        assert!(message(&all[..1], &order).starts_with("Review zu deinen Änderungen (1 Kommentar):"));
    }

    #[test]
    fn a_long_code_line_is_cut_in_the_message() {
        let mut review = Review::new(&key());
        line(&mut review, "a.rs", Some(1), None, &"x".repeat(200), "kurz");
        let all: Vec<&Comment> = review.comments.iter().collect();

        let quoted = message(&all, &[]).lines().nth(2).unwrap().to_string();

        assert_eq!(quoted.chars().filter(|c| *c == 'x').count(), 89);
        assert!(quoted.ends_with("…`"));
    }

    #[test]
    fn a_line_that_moved_is_found_by_its_text() {
        let mut review = Review::new(&key());
        let id = line(&mut review, "a.rs", Some(10), None, "    let now = Instant::now();", "x");
        let comment = review.comments.iter().find(|c| c.id == id).unwrap();

        // Still at 10.
        let same = [row(None, Some(9), "a"), row(None, Some(10), "let now = Instant::now();")];
        assert_eq!(locate(comment, &same), Found::At(1));
        // Two lines were added above: it is at 12, and a second copy far away doesn't count.
        let moved = [row(None, Some(12), "let now = Instant::now();"), row(None, Some(40), "let now = Instant::now();")];
        assert_eq!(locate(comment, &moved), Found::Moved(0));
        // The text is nowhere.
        let gone = [row(None, Some(10), "let now = clock.now();")];
        assert_eq!(locate(comment, &gone), Found::Gone);
    }

    #[test]
    fn a_comment_on_a_removed_line_looks_only_among_removed_lines() {
        let mut review = Review::new(&key());
        let id = line(&mut review, "a.rs", None, Some(5), "old();", "x");
        let comment = review.comments.iter().find(|c| c.id == id).unwrap();

        // The same text on an added line is not the removed one.
        assert_eq!(locate(comment, &[row(None, Some(5), "old();")]), Found::Gone);
        assert_eq!(locate(comment, &[row(Some(5), None, "old();")]), Found::At(0));
        assert_eq!(locate(comment, &[row(Some(8), None, "old();")]), Found::Moved(0));
    }

    #[test]
    fn a_sent_comment_whose_line_claude_changed_is_marked_changed() {
        let mut review = Review::new(&key());
        line(&mut review, "a.rs", Some(22), None, "pub burst: u32,", "Option bitte");
        line(&mut review, "a.rs", Some(30), None, "keep();", "ok so");
        line(&mut review, "b.rs", Some(1), None, "other();", "anderes File");
        review.send(1000, &[]).unwrap();

        // Claude rewrote line 22; line 30 only moved; b.rs wasn't read again.
        let rows = [row(None, Some(22), "pub burst: Option<u32>,"), row(None, Some(31), "keep();")];
        let marked = review.mark_changed("/p/a.rs", &rows);

        assert_eq!(marked, 1);
        let states: Vec<State> = review.comments.iter().map(|c| c.state).collect();
        assert_eq!(states, [State::Changed, State::Sent, State::Sent]);
        // The old line stays as the anchor, to show struck through.
        assert_eq!(review.comments[0].anchor, "pub burst: u32,");
    }

    #[test]
    fn a_draft_is_not_marked_changed_and_a_changed_one_is_not_marked_twice() {
        let mut review = Review::new(&key());
        line(&mut review, "a.rs", Some(1), None, "gone();", "x");

        assert_eq!(review.mark_changed("/p/a.rs", &[]), 0);
        review.send(1, &[]);
        assert_eq!(review.mark_changed("/p/a.rs", &[]), 1);
        assert_eq!(review.mark_changed("/p/a.rs", &[]), 0);
    }

    fn typed(parts: &[&[u8]]) -> TypedInput {
        let mut t = TypedInput::default();
        for p in parts {
            t.feed(p);
        }
        t
    }

    #[test]
    fn typing_begins_an_input() {
        assert!(!TypedInput::default().begun());
        assert!(typed(&[b"h", b"i"]).begun());
        assert!(typed(&["ü".as_bytes()]).begun());
    }

    #[test]
    fn return_empties_the_input() {
        assert!(!typed(&[b"hi", b"\r"]).begun());
        assert!(typed(&[b"hi", b"\r", b"x"]).begun());
    }

    #[test]
    fn backspace_down_to_nothing_is_empty() {
        assert!(!typed(&[b"h", b"i", b"\x7f", b"\x7f"]).begun());
        assert!(typed(&[b"h", b"i", b"\x7f"]).begun());
        assert!(!typed(&[b"\x7f"]).begun());
        // A character of several bytes is one.
        assert!(!typed(&["ü".as_bytes(), b"\x7f"]).begun());
    }

    #[test]
    fn a_paste_begins_an_input() {
        assert!(typed(&[b"\x1b[200~/tmp/a.png \x1b[201~"]).begun());
        assert!(typed(&[b"/tmp/a.png"]).begun());
        // A line break inside a paste doesn't submit.
        assert!(typed(&[b"\x1b[200~a\rb\x1b[201~"]).begun());
        // A paste in pieces.
        assert!(typed(&[b"\x1b[200~ab", b"c\x1b[201~"]).begun());
        assert!(!typed(&[b"\x1b[200~ab", b"c\x1b[201~", b"\r"]).begun());
    }

    #[test]
    fn ctrl_c_empties_the_input() {
        assert!(!typed(&[b"hi", b"\x03"]).begun());
    }

    #[test]
    fn escape_twice_empties_the_input() {
        assert!(!typed(&[b"hi", b"\x1b", b"\x1b"]).begun());
        assert!(typed(&[b"hi", b"\x1b"]).begun());
        assert!(typed(&[b"hi", b"\x1b", b"x", b"\x1b"]).begun());
    }

    #[test]
    fn cursor_keys_and_new_lines() {
        assert!(!typed(&[b"\x1b[A", b"\x1b[1;2C", b"\x1bOP"]).begun());
        assert!(typed(&[b"\x1b\r"]).begun());
        assert!(typed(&[b"a", b"\x1b[A", b"\x7f", b"\x1b[B", b"b"]).begun());
    }

    #[test]
    fn a_review_goes_out_when_waiting_and_not_typing() {
        assert!(may_deliver(true, false));
        assert!(!may_deliver(true, true));
        assert!(!may_deliver(false, false));
        assert!(!may_deliver(false, true));
    }

    #[test]
    fn a_review_is_delivered_only_when_the_session_waits_for_a_new_prompt() {
        assert!(waits_for_prompt(Phase::YourTurn, None));
        // Working, a turn that ended with a question or a permission, background work: not yet.
        assert!(!waits_for_prompt(Phase::Working, None));
        assert!(!waits_for_prompt(Phase::NeedsYou, Some(Ask::Question)));
        assert!(!waits_for_prompt(Phase::YourTurn, Some(Ask::Question)));
        assert!(!waits_for_prompt(Phase::YourTurn, Some(Ask::Permission)));
        assert!(!waits_for_prompt(Phase::YourTurn, Some(Ask::Options)));
        assert!(!waits_for_prompt(Phase::Background, None));
        assert!(!waits_for_prompt(Phase::Ended, None));
    }

    #[test]
    fn sending_drops_the_previous_review_and_unqueueing_returns_drafts() {
        let mut review = Review::new(&key());
        line(&mut review, "a.rs", Some(1), None, "a", "eins");
        review.queue_drafts();
        assert!(review.has_queued());
        review.unqueue();
        assert_eq!((review.count(State::Draft), review.count(State::Queued)), (1, 0));

        review.send(10, &[]).unwrap();
        line(&mut review, "a.rs", Some(2), None, "b", "zwei");
        review.send(20, &[]).unwrap();

        assert_eq!(review.comments.len(), 1);
        assert_eq!(review.comments[0].text, "zwei");
        assert_eq!((review.last_count, review.last_sent_ms), (1, Some(20)));
        assert_eq!(review.send(30, &[]), None);
    }

    #[test]
    fn drafts_survive_a_restart_and_an_empty_review_leaves_no_file() {
        let dir = std::env::temp_dir().join(format!("brain-review-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut review = Review::new(&key());
        let id = line(&mut review, "a.rs", Some(7), Some(6), "code();", "Bitte prüfen");
        review.save(&dir).unwrap();

        let loaded = Review::load_all(&dir);
        assert_eq!(loaded, [review.clone()]);
        assert!(dir.join("main-ab-12.json").is_file());

        review.delete(id);
        review.save(&dir).unwrap();
        assert!(Review::load_all(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
