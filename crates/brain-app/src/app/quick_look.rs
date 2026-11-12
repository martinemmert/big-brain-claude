//! The Quick Look sheet for a changed file: the diff read as numbered lines, paging through the
//! session's files, and opening and closing it. It floats over the whole window; `ui` draws it.

use std::collections::HashMap;

use brain_core::changes::{DiffKind, SessionChange};
use brain_core::state::SessionKey;
use iced::Task;

use super::{Brain, Message};

/// The most diff lines the sheet draws; the rest is a line "… n more lines".
const MOST_ROWS: usize = 1200;

/// What the sheet shows of a file.
pub struct Preview {
    pub key: SessionKey,
    pub change: SessionChange,
    /// The file is still there to open in the editor.
    pub exists: bool,
    /// The diffs read so far, by path: paging back and forth reads each once.
    pub diffs: HashMap<String, QuickDiff>,
}

/// One row of the sheet's diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    /// Where the next lines are: from line `from` of the new file, `len` lines long. `context` is
    /// what git prints after the second `@@` (usually the enclosing function or type).
    Hunk { from: u32, len: u32, context: String, raw: String },
    Line { kind: DiffKind, old: Option<u32>, new: Option<u32>, text: String },
}

/// A diff as numbered rows, without git's file header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuickDiff {
    pub rows: Vec<DiffRow>,
    /// Rows left out because the diff is longer than the sheet draws.
    pub cut: usize,
    /// The file has no old version: only new lines, and no column for old numbers.
    pub new_file: bool,
    /// The diff as git printed it, for the clipboard.
    pub raw: String,
}

/// `@@ -12,7 +14,9 @@ context` as (old start, new start, new length, context).
fn hunk_head(line: &str) -> Option<(u32, u32, u32, String)> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, context) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let start = |range: &str| range.split(',').next().and_then(|s| s.parse::<u32>().ok());
    let len = |range: &str| range.split(',').nth(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
    Some((start(old)?, start(new)?, len(new), context.trim().to_string()))
}

/// Reads a unified diff into rows with the line numbers the hunk heads give, at most `limit` rows.
pub fn quick_diff(diff: &str, limit: usize) -> QuickDiff {
    let mut rows = Vec::new();
    let (mut old, mut new) = (0, 0);
    let mut new_file = false;
    for (kind, line) in brain_core::changes::diff_lines(diff) {
        match kind {
            DiffKind::Meta => {}
            DiffKind::Hunk => {
                let Some((old_start, new_start, len, context)) = hunk_head(&line) else { continue };
                new_file |= rows.is_empty() && old_start == 0 && line.starts_with("@@ -0,0 ");
                (old, new) = (old_start, new_start);
                let raw = line.split_once(" @@").map_or(line.clone(), |(head, _)| format!("{head} @@"));
                rows.push(DiffRow::Hunk { from: new_start, len, context, raw });
            }
            // "\ No newline at end of file" belongs to no line.
            _ if line.starts_with('\\') => {}
            DiffKind::Added => {
                rows.push(DiffRow::Line { kind, old: None, new: Some(new), text: line[1..].to_string() });
                new += 1;
            }
            DiffKind::Removed => {
                rows.push(DiffRow::Line { kind, old: Some(old), new: None, text: line[1..].to_string() });
                old += 1;
            }
            DiffKind::Context => {
                let text = line.strip_prefix(' ').unwrap_or(&line).to_string();
                rows.push(DiffRow::Line { kind, old: Some(old), new: Some(new), text });
                old += 1;
                new += 1;
            }
        }
    }
    let cut = rows.len().saturating_sub(limit);
    rows.truncate(limit);
    QuickDiff { rows, cut, new_file, raw: diff.to_string() }
}

/// The smallest the sheet gets when dragged, and the margin it leaves around itself at most.
pub const SHEET_MIN: iced::Size = iced::Size::new(520.0, 320.0);
const SHEET_MARGIN: f32 = 24.0;

/// The sheet's size in a window: the one dragged (kept inside the bounds the window allows now),
/// else 80 % each way.
pub fn sheet_size(chosen: Option<(f32, f32)>, window: iced::Size) -> iced::Size {
    let most = |window: f32, least: f32| (window - SHEET_MARGIN).max(least);
    let (w, h) = chosen.unwrap_or((window.width * 0.8, window.height * 0.8));
    iced::Size::new(w.clamp(SHEET_MIN.width, most(window.width, SHEET_MIN.width)), h.clamp(SHEET_MIN.height, most(window.height, SHEET_MIN.height)))
}

/// The sheet's size while an edge (`horizontal`: left or right, `vertical`: top or bottom) follows
/// the pointer. The sheet stays centred, so the edge is as far from the window's middle as the
/// pointer is, and the opposite one moves with it.
pub fn dragged_size(pointer: iced::Point, horizontal: bool, vertical: bool, window: iced::Size, current: iced::Size) -> iced::Size {
    let width = if horizontal { 2.0 * (pointer.x - window.width / 2.0).abs() } else { current.width };
    let height = if vertical { 2.0 * (pointer.y - window.height / 2.0).abs() } else { current.height };
    sheet_size(Some((width, height)), window)
}

/// The file `delta` places from `index` in a list of `len`; `None` past either end.
pub fn stepped(index: usize, len: usize, delta: isize) -> Option<usize> {
    index.checked_add_signed(delta).filter(|i| *i < len)
}

impl Brain {
    /// The selected session's changed files, newest first: the order of the list and of paging.
    pub fn changes_newest_first(&self) -> Vec<&SessionChange> {
        let mut files: Vec<&SessionChange> = self.selected_changes().into_iter().flatten().collect();
        files.sort_by_key(|c| std::cmp::Reverse(c.last_ms));
        files
    }

    /// Where the open sheet's file stands in the list: its place and how many there are.
    pub fn preview_position(&self) -> Option<(usize, usize)> {
        let open = &self.preview.as_ref()?.change.path;
        let files = self.changes_newest_first();
        Some((files.iter().position(|c| c.path == *open)?, files.len()))
    }

    /// Opens the sheet on a file of the selected session's changes. The terminal gives up the
    /// keyboard: esc, space and the arrows are the sheet's.
    pub(super) fn open_preview(&mut self, path: &str) -> Task<Message> {
        self.changes_open = false;
        let Some(key) = self.selected.clone() else { return Task::none() };
        let Some(change) = self.changes_newest_first().into_iter().find(|c| c.path == path).cloned() else { return Task::none() };
        self.focused_terminal = None;
        self.focused_shell = None;
        let task = self.show_file(key, change);
        Task::batch([super::unfocus(), task])
    }

    /// The sheet's next (`delta` 1) or previous (-1) file; stays where it is at either end.
    pub(super) fn step_preview(&mut self, delta: isize) -> Task<Message> {
        let Some((index, len)) = self.preview_position() else { return Task::none() };
        let Some(next) = stepped(index, len, delta) else { return Task::none() };
        let (Some(key), Some(change)) = (self.selected.clone(), self.changes_newest_first().get(next).map(|c| (*c).clone())) else {
            return Task::none();
        };
        self.show_file(key, change)
    }

    /// Puts a file on the sheet and reads its diff unless that has been read.
    fn show_file(&mut self, key: SessionKey, change: SessionChange) -> Task<Message> {
        let exists = !self.model.is_demo() && std::path::Path::new(&change.path).is_file();
        let diffs = self.preview.take().filter(|p| p.key == key).map(|p| p.diffs).unwrap_or_default();
        let known = diffs.contains_key(&change.path);
        let demo_diff = self.model.is_demo().then(|| self.model.demo_diffs.get(&change.path).cloned());
        let path = change.path.clone();
        let reading = change.clone();
        self.preview = Some(Preview { key: key.clone(), change, exists, diffs });
        if known {
            return Task::none();
        }
        match demo_diff {
            Some(diff) => self.update(Message::PreviewLoaded(key, path, diff)),
            None => Task::perform(super::off_thread(move || brain_core::changes::session_diff(&reading)), move |diff| {
                Message::PreviewLoaded(key.clone(), path.clone(), diff)
            }),
        }
    }

    /// A diff came in: kept for its file, shown when that is the open one.
    pub(super) fn preview_loaded(&mut self, key: SessionKey, path: String, diff: Option<String>) {
        if let Some(preview) = self.preview.as_mut().filter(|p| p.key == key) {
            preview.diffs.insert(path, quick_diff(diff.as_deref().unwrap_or_default(), MOST_ROWS));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_carry_old_and_new_line_numbers_from_the_hunk_heads() {
        let diff = "diff --git a/a.rs b/a.rs\nindex 1..2 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -12,3 +14,4 @@ pub struct Config {\n keep\n-gone\n+new one\n+new two\n tail\n@@ -40 +50 @@\n-x\n+y\n";

        let rows = quick_diff(diff, 100).rows;

        let line = |kind, old, new, text: &str| DiffRow::Line { kind, old, new, text: text.into() };
        assert_eq!(
            rows[..6],
            [
                DiffRow::Hunk { from: 14, len: 4, context: "pub struct Config {".into(), raw: "@@ -12,3 +14,4 @@".into() },
                line(DiffKind::Context, Some(12), Some(14), "keep"),
                line(DiffKind::Removed, Some(13), None, "gone"),
                line(DiffKind::Added, None, Some(15), "new one"),
                line(DiffKind::Added, None, Some(16), "new two"),
                line(DiffKind::Context, Some(14), Some(17), "tail"),
            ]
        );
        // The second hunk starts its own count and has no context.
        assert_eq!(rows[6], DiffRow::Hunk { from: 50, len: 1, context: String::new(), raw: "@@ -40 +50 @@".into() });
        assert_eq!(rows[7], line(DiffKind::Removed, Some(40), None, "x"));
        assert!(!quick_diff(diff, 100).new_file);
    }

    #[test]
    fn a_new_file_has_no_old_numbers_and_a_long_diff_is_cut() {
        let diff = "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,3 @@\n+a\n+b\n+c\n\\ No newline at end of file\n";

        let all = quick_diff(diff, 100);
        let short = quick_diff(diff, 3);

        assert!(all.new_file);
        assert_eq!(all.rows.len(), 4);
        assert!(matches!(&all.rows[3], DiffRow::Line { old: None, new: Some(3), .. }));
        assert_eq!((short.rows.len(), short.cut), (3, 1));
    }

    #[test]
    fn dragging_an_edge_keeps_the_sheet_centred_and_inside_its_bounds() {
        let window = iced::Size::new(1200.0, 800.0);
        let current = sheet_size(None, window);
        assert_eq!(current, iced::Size::new(960.0, 640.0));

        // The right edge at x = 900 is 300 from the middle: 600 wide, height untouched.
        assert_eq!(dragged_size(iced::Point::new(900.0, 10.0), true, false, window, current), iced::Size::new(600.0, 640.0));
        // Past the window, and across the middle, are held to the bounds.
        assert_eq!(dragged_size(iced::Point::new(5000.0, 400.0), true, true, window, current), iced::Size::new(1176.0, 776.0));
        assert_eq!(dragged_size(iced::Point::new(610.0, 395.0), true, true, window, current), SHEET_MIN);
    }

    #[test]
    fn a_kept_size_is_pulled_in_when_the_window_got_smaller() {
        let small = iced::Size::new(900.0, 520.0);
        assert_eq!(sheet_size(Some((1100.0, 700.0)), small), iced::Size::new(876.0, 496.0));
        assert_eq!(sheet_size(Some((600.0, 400.0)), small), iced::Size::new(600.0, 400.0));
    }

    #[test]
    fn paging_stops_at_the_first_and_the_last_file() {
        assert_eq!(stepped(1, 4, 1), Some(2));
        assert_eq!(stepped(1, 4, -1), Some(0));
        assert_eq!(stepped(0, 4, -1), None);
        assert_eq!(stepped(3, 4, 1), None);
    }
}
