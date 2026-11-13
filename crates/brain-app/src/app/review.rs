//! Review comments on the Quick Look sheet: the field that opens under a line, saving and
//! deleting drafts, sending the review (at once, or when the session waits for a prompt) and
//! keeping it on disk. `ui` draws it.

use std::collections::HashMap;

use brain_core::review::{self, AnchorRow, Comment, Found, Place, Review, State};
use brain_core::state::SessionKey;
use iced::widget::{operation, text_editor};
use iced::Task;

use super::quick_look::{DiffRow, QuickDiff};
use super::{Brain, Message};
use crate::format::now_ms;
use crate::i18n::t;
use crate::tr;

pub const REVIEW_EDITOR: &str = "review-editor";

/// The field for a comment that is being written or changed.
pub struct ReviewEditor {
    pub path: String,
    pub relative: String,
    pub place: Place,
    /// The code line the comment is on (empty for the whole file).
    pub anchor: String,
    /// The draft this edits; none for a new comment.
    pub edit_id: Option<u64>,
    /// The diff row the field sits under; none: at the head of the file.
    pub row: Option<usize>,
    pub content: text_editor::Content,
}

/// Where a file's comments sit in its diff.
#[derive(Default)]
pub struct Placed<'a> {
    /// Under which diff row.
    pub by_row: HashMap<usize, Vec<&'a Comment>>,
    /// At the head of the file: the whole-file comments, then those whose line is gone (`true`).
    pub head: Vec<(&'a Comment, bool)>,
}

/// The diff's lines as `locate` reads them, and the row each belongs to.
pub fn anchor_rows(diff: &QuickDiff) -> (Vec<AnchorRow<'_>>, Vec<usize>) {
    let mut rows = Vec::new();
    let mut at = Vec::new();
    for (i, row) in diff.rows.iter().enumerate() {
        if let DiffRow::Line { old, new, text, .. } = row {
            rows.push(AnchorRow { old: *old, new: *new, text });
            at.push(i);
        }
    }
    (rows, at)
}

/// Puts the comments on `path` under their lines. Without the diff only the whole-file comments
/// show; a line that isn't in it any more sends its comment to the head.
pub fn place<'a>(review: &'a Review, path: &str, diff: Option<&QuickDiff>, file_order: &[String]) -> Placed<'a> {
    let mine: Vec<&Comment> = review.comments.iter().filter(|c| c.path == path).collect();
    let mut placed = Placed::default();
    let (rows, at) = diff.map(anchor_rows).unwrap_or_default();
    for comment in review::ordered(&mine, file_order) {
        if comment.place == Place::File {
            placed.head.push((comment, false));
            continue;
        }
        if diff.is_none() {
            continue;
        }
        match review::locate(comment, &rows) {
            Found::At(i) | Found::Moved(i) => placed.by_row.entry(at[i]).or_default().push(comment),
            Found::Gone => placed.head.push((comment, true)),
        }
    }
    placed
}

impl Brain {
    pub fn review_of(&self, key: &SessionKey) -> Option<&Review> {
        self.reviews.get(key)
    }

    /// Whether a review of the session waits for it to want a prompt.
    pub fn review_waits(&self, key: &SessionKey) -> bool {
        self.reviews.get(key).is_some_and(Review::has_queued)
    }

    /// The paths of the selected session's files in the sheet's order.
    pub fn review_file_order(&self) -> Vec<String> {
        self.changes_in_order().iter().map(|c| c.path.clone()).collect()
    }

    fn touched(&mut self, key: &SessionKey) {
        self.review_dirty.insert(key.clone());
    }

    /// Whether the user has begun a prompt in the session's Brain terminal that isn't submitted
    /// yet. Sessions in other terminals show nothing of it.
    pub fn input_begun(&self, key: &SessionKey) -> bool {
        self.terminals.contains_key(key) && self.typed_input.get(key).is_some_and(review::TypedInput::begun)
    }

    /// Text pasted into the terminal (a clipboard, a dropped path), as the terminal got it: one
    /// bracketed paste without ESC and Ctrl-C.
    pub(super) fn note_pasted(&mut self, key: &SessionKey, text: &str) {
        let mut bytes = b"\x1b[200~".to_vec();
        bytes.extend(text.bytes().filter(|&b| b != 0x1b && b != 0x03));
        bytes.extend_from_slice(b"\x1b[201~");
        self.typed_input.entry(key.clone()).or_default().feed(&bytes);
    }

    /// Whether the session wants a new prompt now, the user hasn't begun one in Brain's terminal
    /// and Brain can hand one to its mod. A question or a permission is no "done".
    pub fn review_ready(&self, key: &SessionKey) -> bool {
        let Some(session) = self.model.board.get(key) else { return false };
        let waits = review::waits_for_prompt(session.phase(), self.real_ask(key));
        review::may_deliver(waits, self.input_begun(key)) && self.request_taker(key).is_some()
    }

    /// Why a review isn't going out yet.
    pub fn review_wait_reason(&self, key: &SessionKey) -> &'static str {
        use brain_core::state::Phase;
        match (self.model.board.get(key).map(|s| s.phase()), self.real_ask(key)) {
            (_, Some(_)) => t("Claude fragt etwas.", "Claude is asking something."),
            (Some(Phase::YourTurn), None) if self.input_begun(key) => {
                t("Du schreibst gerade – geht raus, sobald deine Eingabe abgeschickt oder leer ist.", "You are typing – goes out once your input is sent or empty.")
            }
            (Some(Phase::Working), _) => t("Claude arbeitet gerade.", "Claude is working."),
            (Some(Phase::Background), _) => t("Hintergrundaufgaben laufen noch.", "Background tasks are still running."),
            _ => t("Die Session wartet auf keinen neuen Prompt.", "The session isn't waiting for a new prompt."),
        }
    }

    /// The "+" in front of a diff row: a field for a comment opens under it.
    pub(super) fn review_comment_line(&mut self, row: usize) -> Task<Message> {
        let Some(preview) = self.preview.as_ref() else { return Task::none() };
        let Some(DiffRow::Line { old, new, text, .. }) = preview.diffs.get(&preview.change.path).and_then(|d| d.rows.get(row)) else { return Task::none() };
        self.review_editor = Some(ReviewEditor {
            path: preview.change.path.clone(),
            relative: preview.change.relative.clone(),
            place: Place::Line { new: *new, old: *old },
            anchor: text.clone(),
            edit_id: None,
            row: Some(row),
            content: text_editor::Content::new(),
        });
        operation::focus(REVIEW_EDITOR)
    }

    /// A comment on a whole file: the file at `index` of the sheet's list opens first.
    pub(super) fn review_comment_file(&mut self, index: usize) -> Task<Message> {
        let Some(path) = self.changes_in_order().get(index).map(|c| c.path.clone()) else { return Task::none() };
        let open = if self.preview.as_ref().is_some_and(|p| p.change.path == path) { Task::none() } else { self.open_preview(&path) };
        let Some(preview) = self.preview.as_ref() else { return open };
        self.review_editor = Some(ReviewEditor {
            path: preview.change.path.clone(),
            relative: preview.change.relative.clone(),
            place: Place::File,
            anchor: String::new(),
            edit_id: None,
            row: None,
            content: text_editor::Content::new(),
        });
        Task::batch([open, operation::focus(REVIEW_EDITOR)])
    }

    /// The pencil on a draft: its text in the field, where the card was.
    pub(super) fn review_edit_comment(&mut self, id: u64) -> Task<Message> {
        let Some(key) = self.preview.as_ref().map(|p| p.key.clone()) else { return Task::none() };
        let Some(comment) = self.reviews.get(&key).and_then(|r| r.comments.iter().find(|c| c.id == id && c.state == State::Draft)).cloned() else {
            return Task::none();
        };
        let order = self.review_file_order();
        let row = self.preview.as_ref().and_then(|p| {
            let diff = p.diffs.get(&comment.path)?;
            let review = self.reviews.get(&key)?;
            place(review, &comment.path, Some(diff), &order).by_row.into_iter().find(|(_, list)| list.iter().any(|c| c.id == id)).map(|(row, _)| row)
        });
        self.review_editor = Some(ReviewEditor {
            path: comment.path,
            relative: comment.relative,
            place: comment.place,
            anchor: comment.anchor,
            edit_id: Some(id),
            row,
            content: text_editor::Content::with_text(&comment.text),
        });
        operation::focus(REVIEW_EDITOR)
    }

    pub(super) fn review_edit(&mut self, action: text_editor::Action) {
        if let Some(editor) = self.review_editor.as_mut() {
            editor.content.perform(action);
        }
    }

    pub(super) fn review_cancel(&mut self) {
        self.review_editor = None;
    }

    /// ⌘⏎ in the field: the comment joins the review. An empty one is not kept.
    pub(super) fn review_save(&mut self) {
        let Some(key) = self.preview.as_ref().map(|p| p.key.clone()) else { return };
        let Some(editor) = self.review_editor.take() else { return };
        let text = editor.content.text();
        if text.trim().is_empty() {
            self.review_editor = Some(editor);
            return;
        }
        let review = self.reviews.entry(key.clone()).or_insert_with(|| Review::new(&key));
        match editor.edit_id {
            Some(id) => {
                review.edit(id, &text);
            }
            None => {
                // Added while the review waits: it waits with it.
                let waiting = review.has_queued();
                review.add(&editor.path, &editor.relative, editor.place, &editor.anchor, &text);
                if waiting {
                    review.queue_drafts();
                }
            }
        }
        self.touched(&key);
    }

    pub(super) fn review_delete(&mut self, id: u64) {
        let Some(key) = self.preview.as_ref().map(|p| p.key.clone()) else { return };
        if self.review_editor.as_ref().is_some_and(|e| e.edit_id == Some(id)) {
            self.review_editor = None;
        }
        if self.reviews.get_mut(&key).is_some_and(|r| r.delete(id)) {
            self.touched(&key);
        }
    }

    /// "Verwerfen": the drafts are dropped.
    pub(super) fn review_discard(&mut self) {
        let Some(key) = self.preview.as_ref().map(|p| p.key.clone()) else { return };
        self.review_editor = None;
        let dropped = self.reviews.get_mut(&key).map_or(0, Review::discard_drafts);
        if dropped > 0 {
            self.touched(&key);
            self.set_status(tr!("{dropped} Entwürfe verworfen.", "{dropped} drafts discarded."));
        }
    }

    /// "Zurücknehmen": the waiting review is drafts again.
    pub(super) fn review_unqueue(&mut self) {
        let Some(key) = self.preview.as_ref().map(|p| p.key.clone()) else { return };
        if let Some(review) = self.reviews.get_mut(&key) {
            review.unqueue();
            self.touched(&key);
            self.set_status(t("Review zurückgenommen. Die Kommentare sind wieder Entwürfe.", "Review taken back. The comments are drafts again."));
        }
    }

    /// "Review an Claude schicken": at once when the session waits for a prompt, else it waits.
    pub(super) fn review_send(&mut self) {
        let Some(key) = self.preview.as_ref().map(|p| p.key.clone()) else { return };
        if self.reviews.get(&key).map_or(true, |r| r.open_count() == 0) {
            return;
        }
        self.review_editor = None;
        if !self.can_send(&key) {
            self.refuse(self.why_brain_cant_send());
            return;
        }
        if self.review_ready(&key) {
            self.deliver_review(&key);
            return;
        }
        if let Some(review) = self.reviews.get_mut(&key) {
            review.queue_drafts();
        }
        self.touched(&key);
        let why = self.review_wait_reason(&key);
        self.set_status(tr!("{why} Das Review geht raus, sobald die Session auf einen neuen Prompt wartet.", "{why} The review goes out once the session waits for a new prompt."));
    }

    /// Hands the review to the session's mod as one prompt. In the demo nothing goes out, the
    /// state changes only.
    fn deliver_review(&mut self, key: &SessionKey) -> bool {
        let order = if self.selected.as_ref() == Some(key) { self.review_file_order() } else { Vec::new() };
        let Some(text) = self.reviews.get(key).and_then(|r| r.pending_message(&order)) else { return false };
        if !self.model.is_demo() && !self.send_text(key, &text) {
            return false;
        }
        let Some(review) = self.reviews.get_mut(key) else { return false };
        review.send(now_ms(), &order);
        let count = review.last_count;
        self.touched(key);
        let name = self.model.board.get(key).map(|s| s.display_name()).unwrap_or_default();
        self.set_status(if self.model.is_demo() {
            tr!("Demo: Review mit {count} Kommentaren an „{name}“ gilt als gesendet, es geht nichts raus.", "Demo: the review with {count} comments counts as sent to “{name}”, nothing goes out.")
        } else {
            tr!("Review mit {count} Kommentaren an „{name}“ geschickt.", "Review with {count} comments sent to “{name}”.")
        });
        true
    }

    /// On every tick: a waiting review goes out once its session wants a prompt; the changed
    /// reviews are written to disk.
    pub(super) fn tick_reviews(&mut self) {
        let waiting: Vec<SessionKey> = self.reviews.iter().filter(|(_, r)| r.has_queued()).map(|(k, _)| k.clone()).collect();
        for key in waiting {
            if self.review_ready(&key) && !self.deliver_review(&key) {
                // It didn't go: back to drafts, instead of trying again every tick.
                if let Some(review) = self.reviews.get_mut(&key) {
                    review.unqueue();
                }
                self.touched(&key);
            }
        }
        self.flush_reviews();
    }

    /// Writes the reviews that changed since the last tick, off the UI thread; not once per key
    /// pressed, the field's text isn't part of a review until it is saved. The demo writes nothing.
    fn flush_reviews(&mut self) {
        if self.review_dirty.is_empty() {
            return;
        }
        let keys: Vec<SessionKey> = self.review_dirty.drain().collect();
        if self.model.is_demo() {
            return;
        }
        let reviews: Vec<Review> = keys.iter().map(|k| self.reviews.get(k).cloned().unwrap_or_else(|| Review::new(k))).collect();
        std::thread::spawn(move || {
            let dir = Review::dir(&brain_core::account::home_dir());
            for review in reviews {
                let _ = review.save(&dir);
            }
        });
    }

    /// A diff was read: sent comments whose line Claude changed since are marked.
    pub(super) fn review_diff_read(&mut self, key: &SessionKey, path: &str, diff: &QuickDiff) {
        // A long diff is cut, and a failed read is empty: neither says the line is gone.
        if diff.cut > 0 || diff.rows.is_empty() {
            return;
        }
        let (rows, _) = anchor_rows(diff);
        if self.reviews.get_mut(key).is_some_and(|r| r.mark_changed(path, &rows) > 0) {
            self.touched(key);
        }
    }

    /// The open file was written again (Claude works on): its diff is read anew.
    pub(super) fn refresh_open_diff(&mut self) -> Task<Message> {
        let Some(preview) = self.preview.as_ref() else { return Task::none() };
        let Some(current) = self.changes_in_order().into_iter().find(|c| c.path == preview.change.path).cloned() else { return Task::none() };
        if current.last_ms == preview.change.last_ms && current.added == preview.change.added && current.removed == preview.change.removed {
            return Task::none();
        }
        let key = preview.key.clone();
        self.show_file(key, current, true)
    }
}
