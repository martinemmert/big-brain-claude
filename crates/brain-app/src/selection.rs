//! Mouse text selection across the texts of a scroll area (GPUI has none built in).
//!
//! Each selectable text gets the next index while the area renders and registers its
//! layout once laid out; a selection is a range between two (index, byte offset) points.

use std::cell::RefCell;
use std::ops::Range;

use gpui::{
    px, AnyElement, App, Bounds, Element, ElementId, GlobalElementId, HighlightStyle, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, SharedString, StyledText, TextLayout, Window,
};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Pos {
    text: usize,
    offset: usize,
}

#[derive(Default)]
struct State {
    /// What the area shows (session and tab); a change drops the selection.
    scope: String,
    anchor: Option<Pos>,
    head: Option<Pos>,
    selecting: bool,
    /// The double-clicked word while dragging extends the selection word by word.
    word: Option<(Pos, Pos)>,
    next: usize,
    laid_out: Vec<(usize, SharedString, TextLayout)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Call before rendering the area's texts.
pub fn begin(scope: String) {
    STATE.with_borrow_mut(|s| {
        if s.scope != scope {
            *s = State { scope, ..State::default() };
        }
        s.next = 0;
        s.laid_out.clear();
    });
}

/// A selectable text with `highlights` (sorted, not overlapping).
pub fn text(text: impl Into<SharedString>, highlights: Vec<(Range<usize>, HighlightStyle)>) -> AnyElement {
    let text = text.into();
    let (index, selected) = STATE.with_borrow_mut(|s| {
        let index = s.next;
        s.next += 1;
        (index, selected_range(s, index, text.len()))
    });
    let highlights = match selected {
        Some(range) if !range.is_empty() => with_selection(&text, highlights, range),
        _ => highlights,
    };
    let styled = StyledText::new(text.clone()).with_highlights(highlights);
    let layout = styled.layout().clone();
    Selectable { inner: styled.into_any_element(), index, text, layout }.into_any_element()
}

pub fn plain(text: impl Into<SharedString>) -> AnyElement {
    self::text(text, Vec::new())
}

/// Starts a selection at `position`; a double click selects the word there.
pub fn press(position: Point<Pixels>, clicks: usize) {
    STATE.with_borrow_mut(|s| {
        let pos = locate(&s.laid_out, position);
        s.anchor = pos;
        s.head = pos;
        s.selecting = pos.is_some();
        s.word = None;
        let Some(pos) = pos.filter(|_| clicks == 2) else { return };
        let word = word_around(&s.laid_out, pos);
        s.anchor = Some(word.0);
        s.head = Some(word.1);
        s.word = Some(word);
    })
}

/// The word (letters, digits, `_`) at or just before `offset`; empty between words.
fn word_at(text: &str, offset: usize) -> Range<usize> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let after = text[offset..].chars().next().filter(|c| is_word(*c));
    let before = text[..offset].chars().next_back().filter(|c| is_word(*c));
    if after.is_none() && before.is_none() {
        return offset..offset;
    }
    let start = text[..offset].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(offset, |(i, _)| i);
    let end = text[offset..].char_indices().find(|(_, c)| !is_word(*c)).map_or(text.len(), |(i, _)| offset + i);
    start..end
}

/// Extends the selection while the button is held; returns whether it changed.
pub fn drag(position: Point<Pixels>) -> bool {
    STATE.with_borrow_mut(|s| {
        if !s.selecting {
            return false;
        }
        let Some(pos) = locate(&s.laid_out, position) else { return false };
        let (anchor, head) = match s.word {
            // Keep the double-clicked word and grow to whole words at the pointer.
            Some((start, end)) if pos < start => (end, word_around(&s.laid_out, pos).0),
            Some((start, end)) => (start, word_around(&s.laid_out, pos).1.max(end)),
            None => (s.anchor.unwrap_or(pos), pos),
        };
        let changed = s.anchor != Some(anchor) || s.head != Some(head);
        s.anchor = Some(anchor);
        s.head = Some(head);
        changed
    })
}

/// Start and end of the word at `pos`.
fn word_around(laid_out: &[(usize, SharedString, TextLayout)], pos: Pos) -> (Pos, Pos) {
    let word = laid_out.iter().find(|(index, _, _)| *index == pos.text).map_or(pos.offset..pos.offset, |(_, text, _)| word_at(text, pos.offset));
    (Pos { text: pos.text, offset: word.start }, Pos { text: pos.text, offset: word.end })
}

pub fn release() {
    STATE.with_borrow_mut(|s| s.selecting = false);
}

/// The selected text; pieces on one row are joined by spaces, rows by newlines.
pub fn selected_text() -> Option<String> {
    STATE.with_borrow(|s| {
        let mut out = String::new();
        let mut last_top: Option<Pixels> = None;
        for (index, text, layout) in &s.laid_out {
            let Some(range) = selected_range(s, *index, text.len()).filter(|r| !r.is_empty()) else { continue };
            let top = layout.bounds().top();
            if let Some(last) = last_top {
                out.push(if (top - last).abs() < px(2.) { ' ' } else { '\n' });
            }
            out.push_str(&text[range]);
            last_top = Some(top);
        }
        (!out.is_empty()).then_some(out)
    })
}

fn selected_range(s: &State, index: usize, len: usize) -> Option<Range<usize>> {
    let (a, b) = (s.anchor?, s.head?);
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    if index < start.text || index > end.text {
        return None;
    }
    let from = if index == start.text { start.offset.min(len) } else { 0 };
    let to = if index == end.text { end.offset.min(len) } else { len };
    Some(from..to.max(from))
}

/// The text position under `position`: the text whose row it is on, else the next one below.
fn locate(laid_out: &[(usize, SharedString, TextLayout)], position: Point<Pixels>) -> Option<Pos> {
    let on_row: Vec<_> = laid_out
        .iter()
        .filter(|(_, _, l)| {
            let b = l.bounds();
            position.y >= b.top() && position.y <= b.bottom()
        })
        .collect();
    let at = |(index, text, layout): &(usize, SharedString, TextLayout)| {
        let offset = layout.index_for_position(position).unwrap_or_else(|i| i);
        Pos { text: *index, offset: floor_char_boundary(text, offset) }
    };
    if !on_row.is_empty() {
        let hit = on_row.iter().find(|(_, _, l)| position.x <= l.bounds().right()).unwrap_or(on_row.last().unwrap());
        return Some(at(hit));
    }
    match laid_out.iter().find(|(_, _, l)| l.bounds().top() > position.y) {
        Some((index, _, _)) => Some(Pos { text: *index, offset: 0 }),
        None => laid_out.last().map(|(index, text, _)| Pos { text: *index, offset: text.len() }),
    }
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Lays the selection background over the existing highlights.
fn with_selection(
    text: &str,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    selection: Range<usize>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let selected = HighlightStyle { background_color: Some(theme::alpha(theme::working(), 0x55).into()), ..Default::default() };
    let mut cuts = vec![0, text.len(), selection.start, selection.end];
    for (range, _) in &highlights {
        cuts.extend([range.start, range.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter_map(|w| {
            let range = w[0]..w[1];
            let mut style = highlights.iter().find(|(r, _)| r.start <= range.start && range.end <= r.end).map(|(_, s)| *s);
            if selection.start <= range.start && range.end <= selection.end {
                style = Some(style.map_or(selected, |s| s.highlight(selected)));
            }
            style.map(|s| (range, s))
        })
        .collect()
}

/// A text that registers its layout for hit testing once it is laid out.
struct Selectable {
    inner: AnyElement,
    index: usize,
    text: SharedString,
    layout: TextLayout,
}

impl IntoElement for Selectable {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Selectable {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.inner.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.prepaint(window, cx);
        let entry = (self.index, self.text.clone(), self.layout.clone());
        STATE.with_borrow_mut(|s| {
            s.laid_out.retain(|(index, _, _)| *index != entry.0);
            s.laid_out.push(entry);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_splits_existing_highlights() {
        let bold = HighlightStyle { font_weight: Some(gpui::FontWeight::BOLD), ..Default::default() };

        let got = with_selection("abcdefgh", vec![(2..6, bold)], 4..8);

        let ranges: Vec<_> = got.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(ranges, vec![2..4, 4..6, 6..8]);
        assert_eq!(got[0].1, bold);
        assert!(got[1].1.font_weight.is_some() && got[1].1.background_color.is_some());
        assert!(got[2].1.font_weight.is_none() && got[2].1.background_color.is_some());
    }

    #[test]
    fn double_click_finds_the_whole_word() {
        let text = "Größe von brain_core::markdown!";

        assert_eq!(&text[word_at(text, 2)], "Größe");
        assert_eq!(&text[word_at(text, text.find(" von").unwrap())], "Größe");
        assert_eq!(&text[word_at(text, text.find("core").unwrap())], "brain_core");
        assert_eq!(&text[word_at(text, text.len() - 1)], "markdown");
        assert!(word_at("a  b", 2).is_empty());
    }

    #[test]
    fn range_covers_whole_texts_between_the_ends() {
        let s = State { anchor: Some(Pos { text: 3, offset: 2 }), head: Some(Pos { text: 1, offset: 4 }), ..State::default() };

        assert_eq!(selected_range(&s, 0, 10), None);
        assert_eq!(selected_range(&s, 1, 10), Some(4..10));
        assert_eq!(selected_range(&s, 2, 10), Some(0..10));
        assert_eq!(selected_range(&s, 3, 10), Some(0..2));
    }
}
