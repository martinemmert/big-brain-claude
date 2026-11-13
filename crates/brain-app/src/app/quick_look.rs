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
/// The air between the title bar and the sheet's top edge when the sheet is as tall as it gets.
const SHEET_GAP: f32 = 12.0;

/// The sheet's size in a window: the one dragged (kept inside the bounds the window allows now),
/// else 80 % each way. `top` is what the title bar takes from the top: the sheet lives in the
/// area below it and is centred there, so it is never taller than that area less a gap above and
/// below, and its head stays under the title bar and the traffic lights. `window` is in the
/// layout's points (what the zoom leaves), as is `top`.
pub fn sheet_size(chosen: Option<(f32, f32)>, window: iced::Size, top: f32) -> iced::Size {
    let area = (window.height - top).max(0.0);
    let (w, h) = chosen.unwrap_or((window.width * 0.8, area * 0.8));
    let most_width = (window.width - SHEET_MARGIN).max(SHEET_MIN.width);
    let most_height = (area - 2.0 * SHEET_GAP).max(SHEET_MIN.height);
    iced::Size::new(w.clamp(SHEET_MIN.width, most_width), h.clamp(SHEET_MIN.height, most_height))
}

/// The window's size in the layout's points after the zoom went from `from` to `to`: iced tells
/// the size only when the window is resized, so a zoom has to move what was told.
pub fn rescaled(window: iced::Size, from: f32, to: f32) -> iced::Size {
    iced::Size::new(window.width * from / to, window.height * from / to)
}

/// The sheet's size while an edge (`horizontal`: left or right, `vertical`: top or bottom) follows
/// the pointer. The sheet stays centred in the area below the title bar, so the edge is as far
/// from that area's middle as the pointer is, and the opposite one moves with it.
pub fn dragged_size(pointer: iced::Point, horizontal: bool, vertical: bool, window: iced::Size, top: f32, current: iced::Size) -> iced::Size {
    let middle = top + (window.height - top) / 2.0;
    let width = if horizontal { 2.0 * (pointer.x - window.width / 2.0).abs() } else { current.width };
    let height = if vertical { 2.0 * (pointer.y - middle).abs() } else { current.height };
    sheet_size(Some((width, height)), window, top)
}

/// The folder of a path relative to the repository: "" for a file at the top.
pub fn folder_of(relative: &str) -> &str {
    relative.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Puts the files in the order of the sheet's list, whose islands are folders: a merge request
/// by folder, then path; else the folders in the order their newest file comes, each folder's
/// files keeping their order (`files` comes newest first).
pub fn order_files(files: &mut [&SessionChange], by_path: bool) {
    if by_path {
        files.sort_by(|a, b| (folder_of(&a.relative), &a.relative).cmp(&(folder_of(&b.relative), &b.relative)));
        return;
    }
    let mut seen: Vec<&str> = Vec::new();
    for file in files.iter() {
        if !seen.contains(&folder_of(&file.relative)) {
            seen.push(folder_of(&file.relative));
        }
    }
    files.sort_by_key(|f| seen.iter().position(|d| *d == folder_of(&f.relative)));
}

/// A run of files in one folder: the island of the sheet's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub dir: String,
    /// Index of its first file in the ordered list, and how many it has.
    pub first: usize,
    pub len: usize,
}

/// The ordered files cut into their folders.
pub fn folders(files: &[&SessionChange]) -> Vec<Folder> {
    let mut out: Vec<Folder> = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let dir = folder_of(&file.relative);
        match out.last_mut() {
            Some(last) if last.dir == dir => last.len += 1,
            _ => out.push(Folder { dir: dir.to_string(), first: i, len: 1 }),
        }
    }
    out
}

/// A folder of the tree: `dir` is its whole path (the deepest folder, when folders with one
/// subfolder and no file are drawn as one line), `label` what its line says, `/` between them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DirNode {
    dir: String,
    label: String,
    /// Indices of its own files in the list the tree was built from.
    files: Vec<usize>,
    children: Vec<DirNode>,
}

impl DirNode {
    fn new(dir: &str, label: &str) -> Self {
        DirNode { dir: dir.to_string(), label: label.to_string(), files: Vec::new(), children: Vec::new() }
    }

    /// The folders of `files`, children in the order they first appear, folders without files and
    /// with one subfolder drawn together with it. The root stands for the top level.
    fn build(files: &[&SessionChange]) -> Self {
        let mut root = DirNode::new("", "");
        for (i, file) in files.iter().enumerate() {
            let mut node = &mut root;
            let mut dir = String::new();
            for part in folder_of(&file.relative).split('/').filter(|p| !p.is_empty()) {
                if !dir.is_empty() {
                    dir.push('/');
                }
                dir.push_str(part);
                let at = match node.children.iter().position(|c| c.dir == dir) {
                    Some(at) => at,
                    None => {
                        node.children.push(DirNode::new(&dir, part));
                        node.children.len() - 1
                    }
                };
                node = &mut node.children[at];
            }
            node.files.push(i);
        }
        root.pull_together();
        root
    }

    fn pull_together(&mut self) {
        for child in &mut self.children {
            while child.files.is_empty() && child.children.len() == 1 {
                if let Some(only) = child.children.pop() {
                    child.label = format!("{}/{}", child.label, only.label);
                    child.dir = only.dir;
                    child.files = only.files;
                    child.children = only.children;
                }
            }
            child.pull_together();
        }
    }

    /// Own files first, then the folders: the order paging goes in.
    fn walk(&self, out: &mut Vec<usize>) {
        out.extend(&self.files);
        for child in &self.children {
            child.walk(out);
        }
    }

    /// How many files lie in it and below, and their added and removed lines.
    fn totals(&self, files: &[&SessionChange]) -> (usize, u64, u64) {
        let own = self.files.iter().fold((self.files.len(), 0, 0), |(n, a, r), i| (n, a + files[*i].added.unwrap_or(0), r + files[*i].removed.unwrap_or(0)));
        self.children.iter().fold(own, |(n, a, r), child| {
            let (cn, ca, cr) = child.totals(files);
            (n + cn, a + ca, r + cr)
        })
    }

    fn lines(&self, files: &[&SessionChange], folded: &impl Fn(&str) -> bool, depth: usize, out: &mut Vec<TreeLine>) {
        out.extend(self.files.iter().map(|index| TreeLine::File { index: *index, depth }));
        for child in &self.children {
            let (count, added, removed) = child.totals(files);
            let is_folded = folded(&child.dir);
            out.push(TreeLine::Dir { dir: child.dir.clone(), label: child.label.clone(), depth, folded: is_folded, count, added, removed });
            if !is_folded {
                child.lines(files, folded, depth + 1, out);
            }
        }
    }
}

/// Puts the files in the order of the tree: a folder's own files, then its folders.
pub fn tree_order(files: &mut Vec<&SessionChange>) {
    let mut order = Vec::with_capacity(files.len());
    DirNode::build(files).walk(&mut order);
    *files = order.into_iter().map(|i| files[i]).collect();
}

/// One line of the sheet's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeLine {
    Dir { dir: String, label: String, depth: usize, folded: bool, count: usize, added: u64, removed: u64 },
    File { index: usize, depth: usize },
}

/// The lines of the tree for files already in tree order; what lies in a folded folder is left out.
pub fn tree_lines(files: &[&SessionChange], folded: impl Fn(&str) -> bool) -> Vec<TreeLine> {
    let mut out = Vec::new();
    DirNode::build(files).lines(files, &folded, 0, &mut out);
    out
}

/// The top and bottom of file `index`'s row in the tree, which is one island of rows of equal
/// height; `None` when a folded folder hides it.
pub fn tree_file_span(lines: &[TreeLine], index: usize) -> Option<(f32, f32)> {
    use crate::tree::{GAP, ISLAND_PAD_Y, ROW_H};
    let at = lines.iter().position(|l| matches!(l, TreeLine::File { index: i, .. } if *i == index))?;
    let top = LIST_TOP + ISLAND_PAD_Y + at as f32 * (ROW_H + GAP);
    Some((top, top + ROW_H))
}

/// Where the sheet's list starts below its top edge, before the first island.
pub const LIST_TOP: f32 = 8.0;

/// The top and bottom of file `index`'s row in the sheet's scrolled list, by the heights the list
/// is drawn with; `None` when its folder is folded.
pub fn file_span(folders: &[Folder], folded: impl Fn(&str) -> bool, index: usize) -> Option<(f32, f32)> {
    use crate::tree::{BLOCK_GAP, GAP, ISLAND_PAD_Y, PROJECT_H, ROW_H};
    let mut top = LIST_TOP;
    for folder in folders {
        top += ISLAND_PAD_Y + PROJECT_H + GAP;
        if !folded(&folder.dir) {
            if (folder.first..folder.first + folder.len).contains(&index) {
                top += (index - folder.first) as f32 * (ROW_H + GAP);
                return Some((top, top + ROW_H));
            }
            top += folder.len as f32 * (ROW_H + GAP);
        }
        top += ISLAND_PAD_Y - GAP + BLOCK_GAP;
    }
    None
}

/// Where to scroll a list so the row between `top` and `bottom` is in view: `None` when it is
/// (`viewport`: the offset and the height seen), else the offset that brings it in with a little air.
pub fn scroll_target(top: f32, bottom: f32, viewport: Option<(f32, f32)>) -> Option<f32> {
    match viewport {
        Some((offset, _)) if top < offset => Some((top - 8.0).max(0.0)),
        Some((offset, seen)) if bottom > offset + seen => Some(bottom - seen + 8.0),
        Some(_) => None,
        None => Some((top - 8.0).max(0.0)),
    }
}

/// The file `delta` places from `index` in a list of `len`; `None` past either end.
pub fn stepped(index: usize, len: usize, delta: isize) -> Option<usize> {
    index.checked_add_signed(delta).filter(|i| *i < len)
}

impl Brain {
    /// What the title bar takes from the sheet's window at the current zoom.
    pub fn sheet_top(&self) -> f32 {
        crate::chrome::TITLEBAR_HEIGHT / self.zoom()
    }

    /// Scrolls the sheet's list so the open file's row is in view; its folder is opened first.
    fn scroll_sheet_to_file(&mut self) -> Task<Message> {
        let Some((index, _)) = self.preview_position() else { return Task::none() };
        let dir = folder_of(&self.changes_in_order()[index].relative).to_string();
        if self.prefs.sheet_tree {
            // Every folder on the way to the file is opened.
            self.folded_dirs.retain(|d| dir != *d && !dir.starts_with(&format!("{d}/")));
        } else {
            self.folded_dirs.remove(&dir);
        }
        let files = self.changes_in_order();
        let span = if self.prefs.sheet_tree {
            tree_file_span(&tree_lines(&files, |d| self.folded_dirs.contains(d)), index)
        } else {
            file_span(&folders(&files), |d| self.folded_dirs.contains(d), index)
        };
        let Some((top, bottom)) = span else { return Task::none() };
        match scroll_target(top, bottom, self.sheet_viewport) {
            Some(y) => iced::widget::operation::scroll_to(super::SHEET_FILES, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y }),
            None => Task::none(),
        }
    }

    /// The selected session's changed files: the order of the list and of paging, folder by folder.
    /// A merge request lists its folders by path; the files a transcript names, without git, by
    /// the time of each folder's newest file.
    pub fn changes_in_order(&self) -> Vec<&SessionChange> {
        let mut files: Vec<&SessionChange> = self.selected_changes().into_iter().flatten().collect();
        let by_path = self.selected_cache().is_some_and(|c| c.merge.is_some());
        if !by_path {
            files.sort_by_key(|c| std::cmp::Reverse(c.last_ms));
        }
        order_files(&mut files, by_path);
        if self.prefs.sheet_tree {
            tree_order(&mut files);
        }
        files
    }

    /// Shows or hides the sheet's list of files (⌃⌘S while the sheet is open).
    pub(super) fn toggle_sheet_files(&mut self) -> Task<Message> {
        self.prefs.hide_sheet_files = !self.prefs.hide_sheet_files;
        self.prefs.save();
        self.sheet_viewport = None;
        self.scroll_sheet_to_file()
    }

    /// Switches the list between flat islands per folder and the tree; the open file stays open.
    pub(super) fn set_sheet_tree(&mut self, tree: bool) -> Task<Message> {
        self.prefs.sheet_tree = tree;
        self.prefs.save();
        self.sheet_viewport = None;
        self.scroll_sheet_to_file()
    }

    /// Where the open sheet's file stands in the list: its place and how many there are.
    pub fn preview_position(&self) -> Option<(usize, usize)> {
        let open = &self.preview.as_ref()?.change.path;
        let files = self.changes_in_order();
        Some((files.iter().position(|c| c.path == *open)?, files.len()))
    }

    /// Opens the sheet on a file of the selected session's changes. The terminal gives up the
    /// keyboard: esc, space and the arrows are the sheet's.
    pub(super) fn open_preview(&mut self, path: &str) -> Task<Message> {
        self.changes_open = false;
        let Some(key) = self.selected.clone() else { return Task::none() };
        let Some(change) = self.changes_in_order().into_iter().find(|c| c.path == path).cloned() else { return Task::none() };
        self.focused_terminal = None;
        self.focused_shell = None;
        self.review_editor = None;
        let task = self.show_file(key, change, false);
        Task::batch([super::unfocus(), task])
    }

    /// The sheet's next (`delta` 1) or previous (-1) file; stays where it is at either end.
    pub(super) fn step_preview(&mut self, delta: isize) -> Task<Message> {
        let Some((index, len)) = self.preview_position() else { return Task::none() };
        let Some(next) = stepped(index, len, delta) else { return Task::none() };
        let (Some(key), Some(change)) = (self.selected.clone(), self.changes_in_order().get(next).map(|c| (*c).clone())) else {
            return Task::none();
        };
        self.review_editor = None;
        self.show_file(key, change, false)
    }

    /// Puts a file on the sheet and reads its diff unless that has been read (`reload`: read it
    /// anew, the old one stays on show until the new one is there).
    pub(super) fn show_file(&mut self, key: SessionKey, change: SessionChange, reload: bool) -> Task<Message> {
        let exists = !self.model.is_demo() && std::path::Path::new(&change.path).is_file();
        let diffs = self.preview.take().filter(|p| p.key == key).map(|p| p.diffs).unwrap_or_default();
        let known = diffs.contains_key(&change.path) && !reload;
        let demo_diff = self.model.is_demo().then(|| self.model.demo_diffs.get(&change.path).cloned());
        let path = change.path.clone();
        let reading = change.clone();
        self.preview = Some(Preview { key: key.clone(), change, exists, diffs });
        let scroll = self.scroll_sheet_to_file();
        if known {
            return scroll;
        }
        let load = match demo_diff {
            Some(diff) => self.update(Message::PreviewLoaded(key, path, diff)),
            None => Task::perform(super::off_thread(move || brain_core::changes::session_diff(&reading)), move |diff| {
                Message::PreviewLoaded(key.clone(), path.clone(), diff)
            }),
        };
        Task::batch([scroll, load])
    }

    /// A diff came in: kept for its file, shown when that is the open one.
    pub(super) fn preview_loaded(&mut self, key: SessionKey, path: String, diff: Option<String>) {
        if self.preview.as_ref().is_some_and(|p| p.key == key) {
            let read = quick_diff(diff.as_deref().unwrap_or_default(), MOST_ROWS);
            if diff.is_some() {
                self.review_diff_read(&key, &path, &read);
            }
            if let Some(preview) = self.preview.as_mut().filter(|p| p.key == key) {
                preview.diffs.insert(path, read);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the title bar takes at 100 % zoom.
    const TOP: f32 = 52.0;

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
    fn dragging_an_edge_keeps_the_sheet_centred_below_the_title_bar_and_inside_its_bounds() {
        let window = iced::Size::new(1200.0, 800.0);
        let current = sheet_size(None, window, TOP);
        assert_eq!(current, iced::Size::new(960.0, 598.4));

        // The right edge at x = 900 is 300 from the middle: 600 wide, height untouched.
        assert_eq!(dragged_size(iced::Point::new(900.0, 10.0), true, false, window, TOP, current), iced::Size::new(600.0, 598.4));
        // The bottom edge at y = 700 is 326 from the middle of the area below the title bar (426).
        assert_eq!(dragged_size(iced::Point::new(600.0, 700.0), false, true, window, TOP, current), iced::Size::new(960.0, 548.0));
        // Past the window, and across the middle, are held to the bounds: the height leaves a gap
        // above and below the area under the title bar.
        assert_eq!(dragged_size(iced::Point::new(5000.0, 5000.0), true, true, window, TOP, current), iced::Size::new(1176.0, 724.0));
        assert_eq!(dragged_size(iced::Point::new(610.0, 426.0), true, true, window, TOP, current), SHEET_MIN);
    }

    #[test]
    fn a_kept_size_is_pulled_in_when_the_window_got_smaller() {
        let small = iced::Size::new(900.0, 520.0);
        assert_eq!(sheet_size(Some((1100.0, 700.0)), small, TOP), iced::Size::new(876.0, 444.0));
        assert_eq!(sheet_size(Some((600.0, 400.0)), small, TOP), iced::Size::new(600.0, 400.0));
        assert_eq!(sheet_size(Some((600.0, 300.0)), small, TOP), iced::Size::new(600.0, 320.0));
    }

    #[test]
    fn the_sheet_never_reaches_above_the_title_bar() {
        // At any zoom, however large it is dragged or kept, the sheet centred in the area below
        // the title bar has its top edge below the title bar, with its gap.
        for (window, top) in [(iced::Size::new(1400.0, 900.0), 52.0), (iced::Size::new(1400.0, 900.0), 52.0 / 0.8), (iced::Size::new(1000.0, 600.0), 52.0 / 1.5)] {
            let size = sheet_size(Some((5000.0, 5000.0)), window, top);
            let edge = top + ((window.height - top) - size.height) / 2.0;
            assert!(edge >= top + SHEET_GAP, "{window:?} {top}: {edge}");
        }
        // The size the window had before the zoom went from 100 % to 125 % is the old one
        // divided; used unscaled, the sheet of a 1800 x 1125 window would be taller than the 900
        // points the layout has left.
        let layout = rescaled(iced::Size::new(1800.0, 1125.0), 1.0, 1.25);
        assert_eq!(layout, iced::Size::new(1440.0, 900.0));
        let size = sheet_size(Some((5000.0, 5000.0)), layout, 52.0 / 1.25);
        assert!(52.0 / 1.25 + SHEET_GAP + size.height + SHEET_GAP <= layout.height);
    }

    fn change(relative: &str, last_ms: i64) -> SessionChange {
        SessionChange {
            path: format!("/repo/{relative}"),
            relative: relative.to_string(),
            last_ms,
            added: None,
            removed: None,
            said: None,
            root: "/repo".into(),
            git: None,
            from_session: true,
            base: None,
            original: None,
        }
    }

    #[test]
    fn files_are_grouped_by_folder_and_a_merge_request_orders_them_by_path() {
        let all = [change("src/b.rs", 1), change("README.md", 2), change("src/a/x.rs", 3), change("src/a.rs", 4), change("src/a/y.rs", 5)];
        let mut files: Vec<&SessionChange> = all.iter().collect();

        order_files(&mut files, true);

        let order: Vec<&str> = files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(order, ["README.md", "src/a.rs", "src/b.rs", "src/a/x.rs", "src/a/y.rs"]);
        assert_eq!(
            folders(&files),
            [
                Folder { dir: String::new(), first: 0, len: 1 },
                Folder { dir: "src".into(), first: 1, len: 2 },
                Folder { dir: "src/a".into(), first: 3, len: 2 },
            ]
        );
    }

    #[test]
    fn without_a_merge_request_the_folders_follow_their_newest_file() {
        let all = [change("src/new.rs", 9), change("docs/a.md", 8), change("src/old.rs", 1)];
        let mut files: Vec<&SessionChange> = all.iter().collect();

        order_files(&mut files, false);

        let order: Vec<&str> = files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(order, ["src/new.rs", "src/old.rs", "docs/a.md"]);
    }

    #[test]
    fn a_files_row_is_found_by_the_heights_of_the_list_and_skips_folded_folders() {
        use crate::tree::{BLOCK_GAP, GAP, ISLAND_PAD_Y, PROJECT_H, ROW_H};
        let groups = [Folder { dir: "a".into(), first: 0, len: 2 }, Folder { dir: "b".into(), first: 2, len: 3 }];

        let (top, bottom) = file_span(&groups, |_| false, 3).unwrap();
        let second_island = LIST_TOP + ISLAND_PAD_Y + PROJECT_H + GAP + 2.0 * (ROW_H + GAP) + ISLAND_PAD_Y - GAP + BLOCK_GAP;
        assert_eq!(top, second_island + ISLAND_PAD_Y + PROJECT_H + GAP + (ROW_H + GAP));
        assert_eq!(bottom - top, ROW_H);

        // Folded, the first folder takes only its head; a file inside a folded folder has no row.
        let (folded_top, _) = file_span(&groups, |d| d == "a", 3).unwrap();
        assert_eq!(top - folded_top, 2.0 * (ROW_H + GAP));
        assert_eq!(file_span(&groups, |d| d == "a", 1), None);
    }

    #[test]
    fn the_list_scrolls_only_when_the_selected_row_is_out_of_view() {
        assert_eq!(scroll_target(300.0, 330.0, Some((100.0, 400.0))), None);
        assert_eq!(scroll_target(50.0, 80.0, Some((100.0, 400.0))), Some(42.0));
        assert_eq!(scroll_target(600.0, 630.0, Some((100.0, 400.0))), Some(238.0));
        assert_eq!(scroll_target(4.0, 34.0, None), Some(0.0));
    }

    fn counted(relative: &str, added: u64, removed: u64) -> SessionChange {
        SessionChange { added: Some(added), removed: Some(removed), ..change(relative, 1) }
    }

    #[test]
    fn the_tree_nests_folders_and_pages_through_a_folders_files_before_its_folders() {
        let all = [change("src/a/x.rs", 1), change("README.md", 2), change("src/b.rs", 3), change("src/a/y.rs", 4), change("docs/z.md", 5)];
        let mut files: Vec<&SessionChange> = all.iter().collect();

        tree_order(&mut files);

        let order: Vec<&str> = files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(order, ["README.md", "src/b.rs", "src/a/x.rs", "src/a/y.rs", "docs/z.md"]);
        let shape: Vec<(String, usize)> = tree_lines(&files, |_| false)
            .iter()
            .map(|l| match l {
                TreeLine::Dir { dir, depth, .. } => (format!("{dir}/"), *depth),
                TreeLine::File { index, depth } => (files[*index].relative.clone(), *depth),
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("README.md".to_string(), 0),
                ("src/".into(), 0),
                ("src/b.rs".into(), 1),
                ("src/a/".into(), 1),
                ("src/a/x.rs".into(), 2),
                ("src/a/y.rs".into(), 2),
                ("docs/".into(), 0),
                ("docs/z.md".into(), 1),
            ]
        );
    }

    #[test]
    fn a_folder_with_only_one_subfolder_is_drawn_together_with_it() {
        let all = [change("src/Phoenix/Applets/a.php", 1), change("src/Phoenix/Applets/b.php", 2), change("src/Phoenix/Core/c.php", 3), change("lib/x/y/z.rs", 4)];
        let mut files: Vec<&SessionChange> = all.iter().collect();
        tree_order(&mut files);

        let dirs: Vec<(String, String, usize)> = tree_lines(&files, |_| false)
            .into_iter()
            .filter_map(|l| match l {
                TreeLine::Dir { dir, label, depth, .. } => Some((dir, label, depth)),
                _ => None,
            })
            .collect();

        // "src" holds nothing but "Phoenix": one line "src/Phoenix", which has two folders below
        // it and stays; lib/x/y is one line.
        assert_eq!(
            dirs,
            [
                ("src/Phoenix".to_string(), "src/Phoenix".to_string(), 0),
                ("src/Phoenix/Applets".into(), "Applets".into(), 1),
                ("src/Phoenix/Core".into(), "Core".into(), 1),
                ("lib/x/y".into(), "lib/x/y".into(), 0),
            ]
        );
    }

    #[test]
    fn a_folder_sums_the_files_and_lines_below_it_and_a_folded_one_hides_them() {
        let all = [counted("src/a.rs", 3, 1), counted("src/deep/b.rs", 10, 0), counted("src/deep/c.rs", 2, 5)];
        let mut files: Vec<&SessionChange> = all.iter().collect();
        tree_order(&mut files);

        let open = tree_lines(&files, |_| false);
        let TreeLine::Dir { count, added, removed, .. } = &open[0] else { panic!("{open:?}") };
        assert_eq!((*count, *added, *removed), (3, 15, 6));

        let closed = tree_lines(&files, |d| d == "src");
        assert_eq!(closed.len(), 1);
        assert_eq!(tree_file_span(&closed, 1), None);
        // Open: src, a.rs, deep, b.rs, c.rs: the file `c.rs` (index 2) is the fifth line.
        let (top, bottom) = tree_file_span(&open, 2).unwrap();
        let line_h = crate::tree::ROW_H + crate::tree::GAP;
        assert_eq!(top, LIST_TOP + crate::tree::ISLAND_PAD_Y + 4.0 * line_h);
        assert_eq!(bottom - top, crate::tree::ROW_H);
    }

    #[test]
    fn paging_stops_at_the_first_and_the_last_file() {
        assert_eq!(stepped(1, 4, 1), Some(2));
        assert_eq!(stepped(1, 4, -1), Some(0));
        assert_eq!(stepped(0, 4, -1), None);
        assert_eq!(stepped(3, 4, 1), None);
    }
}
