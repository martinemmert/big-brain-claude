//! The detail pane's content: the conversation, drawn like Claude Code in the terminal (in the
//! terminal's font), and the event timeline.

use brain_core::event::{Event, Kind, Source};
use brain_core::markdown::{self, Block, Inline, Span};
use brain_core::state::Session;
use brain_core::transcript::{classify_prompt, Message, Prompt, Role};
use chrono::Local;
use iced::widget::{column, container, rich_text, row, scrollable, span, text, Space};
use iced::{Color, Element, Fill, Font, Length, Padding};

use crate::chat_font::{self, ChatFont};
use crate::format::{clip, note, plain, tilde};
use crate::i18n::t;
use crate::style::{self, alpha, boxed, DONE, LINE, SURFACE, TEXT, TEXT_FAINT, TEXT_MUTED, TEXT_STRONG, TURN, UI, WORKING};
use crate::tr;

/// Consecutive tool calls are shown as one block; long runs are cut to this many rows.
const TOOL_ROWS_SHOWN: usize = 6;
/// Width of the `● ` / `> ` gutter in characters, so wrapped lines stay aligned like in the terminal.
const GUTTER: f32 = 2.0;
/// Claude Code's reply marker. (`⏺`, which it uses on macOS, comes out as a colour emoji here:
/// the terminal fonts don't have it and the fallback is Apple Color Emoji.)
const DOT: &str = "●";

/// The width of `chars` characters of the chat font (monospaced: about 0.6 em each).
fn cells(font: &ChatFont, chars: f32) -> Length {
    Length::Fixed((font.size * 0.62 * chars).ceil() + 1.0)
}

pub fn conversation<'a, M: 'a>(messages: &[Message]) -> Vec<Element<'a, M>> {
    let font = chat_font::get();
    let mut out = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let message = &messages[i];
        if message.role == Role::Tool {
            let run = messages[i..].iter().take_while(|m| m.role == Role::Tool).count();
            out.push(tool_calls(font, &messages[i..i + run]));
            i += run;
            continue;
        }
        out.push(match message.role {
            Role::User => prompt(font, message),
            Role::Assistant => gutter(font, DOT, TEXT, body_in(Look::chat(font), &message.text)),
            Role::System => note_line(font, &message.text),
            Role::Tool => unreachable!(),
        });
        i += 1;
    }
    out
}

/// A marker in a fixed-width column, the content beside it: `⏺ text`, `> prompt`.
fn gutter<'a, M: 'a>(font: &ChatFont, marker: &str, color: Color, content: Element<'a, M>) -> Element<'a, M> {
    row![
        container(text(marker.to_string()).size(font.size).color(color).font(font.regular)).width(cells(font, GUTTER)),
        container(content).width(Fill),
    ]
    .into()
}

/// Your prompt: `> text` on a faint band, like Claude Code's echo of what you typed.
fn prompt<'a, M: 'a>(font: &ChatFont, message: &Message) -> Element<'a, M> {
    // Very long prompts (pasted logs) are cut to 14 lines.
    let body: String = message.text.lines().take(14).collect::<Vec<_>>().join("\n");
    container(gutter(font, ">", TEXT_FAINT, text(body).size(font.size).color(TEXT_MUTED).font(font.regular).line_height(1.45).into()))
        .padding([6, 8])
        .width(Fill)
        .style(|_| boxed(SURFACE, Color::TRANSPARENT, 4.0))
        .into()
}

/// A harness note (subagent hand-back, task notification): `⎿ note`, dim.
fn note_line<'a, M: 'a>(font: &ChatFont, label: &str) -> Element<'a, M> {
    row![
        Space::new().width(cells(font, GUTTER)),
        text(format!("⎿  {}", clip(&note(label), 140))).size(font.size).color(TEXT_FAINT).font(font.regular),
    ]
    .into()
}

/// `● Bash(cargo test -p gateway)` per call, as one tight block; long runs start with how many
/// came before.
fn tool_calls<'a, M: 'a>(font: &ChatFont, calls: &[Message]) -> Element<'a, M> {
    let hidden = calls.len().saturating_sub(TOOL_ROWS_SHOWN);
    let mut out = Vec::new();
    if hidden > 0 {
        out.push(gutter(font, DOT, TEXT_FAINT, text(tr!("… {hidden} frühere Tool-Aufrufe", "… {hidden} earlier tool calls")).size(font.size).color(TEXT_FAINT).font(font.regular).into()));
    }
    for call in &calls[hidden..] {
        let tool = short_tool_name(call.tool.as_deref().unwrap_or_default());
        let argument = clip(&tilde(&note(&call.text)), 110);
        // Two texts, not two spans: Iced shapes a word in the font its first letter has, so
        // "Bash(pnpm" as spans would draw "pnpm" bold too.
        let line = row![
            text(tool).size(font.size).font(font.bold).color(TEXT_STRONG).wrapping(text::Wrapping::None),
            text(format!("({argument})")).size(font.size).font(font.regular).color(TEXT_MUTED).wrapping(text::Wrapping::None),
        ];
        out.push(gutter(font, DOT, DONE, line.into()));
    }
    column(out).spacing(font.size * 0.35).into()
}

/// `mcp__claude-in-chrome__navigate` → `chrome · navigate`
fn short_tool_name(tool: &str) -> String {
    match tool.strip_prefix("mcp__") {
        Some(rest) => {
            let mut parts = rest.splitn(2, "__");
            let server = parts.next().unwrap_or_default();
            let action = parts.next().unwrap_or_default();
            let server = server.rsplit(['_', '-']).next().unwrap_or(server);
            format!("{server} · {action}")
        }
        None => tool.to_string(),
    }
}

// ---- Markdown ------------------------------------------------------------------------

/// How Markdown is set. The chat looks like Claude Code in the terminal: one font, one size.
/// The reader sets a document: proportional text, a heading scale, code in the terminal's font.
#[derive(Clone, Copy)]
struct Look {
    text: Font,
    bold: Font,
    code: Font,
    size: f32,
    line_height: f32,
    document: bool,
}

impl Look {
    fn chat(font: &ChatFont) -> Self {
        Self { text: font.regular, bold: font.bold, code: font.regular, size: font.size, line_height: 1.45, document: false }
    }

    fn document() -> Self {
        Self { text: UI, bold: style::semibold(), code: chat_font::get().regular, size: 14.0, line_height: 1.55, document: true }
    }

    /// Width of one character of the code font (monospaced: about 0.6 em).
    fn cell(&self) -> f32 {
        self.size * 0.62
    }
}

/// A Markdown file for the reader.
pub fn document<'a, M: 'a>(source: &str) -> Element<'a, M> {
    body_in(Look::document(), source)
}

fn body_in<'a, M: 'a>(look: Look, source: &str) -> Element<'a, M> {
    let blocks = markdown::parse(source);
    let first = blocks.first().map(|b| matches!(b, Block::Heading { .. })).unwrap_or(false);
    let spacing = if look.document { look.size * 0.8 } else { look.size * 0.6 };
    column(blocks.into_iter().enumerate().map(|(i, b)| block(look, b, i == 0 && first)))
        .spacing(spacing)
        .into()
}

fn block<'a, M: 'a>(look: Look, block: Block, at_top: bool) -> Element<'a, M> {
    match block {
        Block::Heading { level, text: heading } if look.document => document_heading(look, level, &heading, at_top),
        // The terminal has one size: headings are bold, the first levels also brighter.
        Block::Heading { level, text: heading } => styled(look, &heading, if level <= 2 { TEXT_STRONG } else { TEXT }, true, look.size),
        Block::Paragraph(paragraph) => styled(look, &paragraph, TEXT, false, look.size),
        Block::Item { indent, marker, text: item } => {
            let bullet = marker == "•";
            let marker = match (bullet, look.document) {
                (true, true) => "•".to_string(),
                (true, false) => "-".to_string(),
                (false, _) => marker,
            };
            // Numbers line up on their dot; a bullet gets a fixed narrow column.
            let width = if bullet && look.document { look.size * 1.3 } else { look.cell() * (marker.chars().count() as f32 + 1.0) };
            let step = if look.document { look.size * 1.3 } else { look.cell() * 2.0 };
            row![
                container(text(marker).size(look.size).color(TEXT_MUTED).font(if bullet { look.text } else { look.code }).line_height(look.line_height)).width(Length::Fixed(width)),
                container(styled(look, &item, TEXT, false, look.size)).width(Fill),
            ]
            .padding(Padding { left: indent as f32 * step, ..Padding::ZERO })
            .into()
        }
        Block::Code { text: code, .. } if look.document => {
            // Code keeps its lines: long ones scroll sideways instead of wrapping.
            let lines = text(code).size(look.size - 1.5).color(style::CODE).font(look.code).line_height(1.5).wrapping(text::Wrapping::None);
            container(scrollable(container(lines).padding(Padding { bottom: 6.0, ..Padding::ZERO })).direction(sideways()))
                .padding(Padding { top: 10.0, right: 12.0, bottom: 4.0, left: 12.0 })
                .width(Fill)
                .style(|_| boxed(SURFACE, LINE, 6.0))
                .into()
        }
        Block::Code { text: code, .. } => container(text(code).size(look.size).color(style::CODE).font(look.code).line_height(look.line_height))
            .padding(Padding { left: look.cell() * 2.0, ..Padding::ZERO })
            .into(),
        Block::Quote(quote) if look.document => container(styled(look, &quote, TEXT_MUTED, false, look.size))
            .padding([8, 14])
            .width(Fill)
            .style(|_| boxed(alpha(SURFACE, 0xaa), LINE, 6.0))
            .into(),
        Block::Quote(quote) => row![
            text("│ ").size(look.size).color(TEXT_FAINT).font(look.code),
            styled(look, &quote, TEXT_MUTED, false, look.size),
        ]
        .into(),
        Block::Table { header, rows } => table(look, header, rows),
        Block::Rule if look.document => container(container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE))).padding([6, 0]).into(),
        Block::Rule => text("─".repeat(40)).size(look.size).color(TEXT_FAINT).font(look.code).into(),
    }
}

/// A scale of sizes with room above; the top two levels get a line under them.
fn document_heading<'a, M: 'a>(look: Look, level: u8, heading: &Inline, at_top: bool) -> Element<'a, M> {
    let (scale, color, above) = match level {
        1 => (1.6, TEXT_STRONG, 1.4),
        2 => (1.3, TEXT_STRONG, 1.2),
        3 => (1.1, TEXT_STRONG, 0.8),
        _ => (1.0, TEXT_MUTED, 0.5),
    };
    let title = styled(look, heading, color, true, look.size * scale);
    let content: Element<'a, M> = if level <= 2 {
        column![title, container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE))].spacing(look.size * 0.4).into()
    } else {
        title
    };
    let top = if at_top { 0.0 } else { look.size * above };
    container(content).padding(Padding { top, ..Padding::ZERO }).into()
}

fn sideways() -> scrollable::Direction {
    scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4).spacing(2))
}

/// Columns of short values keep their width; columns of prose share the rest of the line in
/// proportion to their longest cell, so wide tables wrap inside the pane instead of overflowing.
fn table<'a, M: 'a>(look: Look, header: Vec<Inline>, rows: Vec<Vec<Inline>>) -> Element<'a, M> {
    /// Up to this many characters a column is sized to its content.
    const SHORT: usize = 22;
    let size = if look.document { look.size - 1.0 } else { look.size };
    let longest = |column: usize| {
        std::iter::once(&header)
            .chain(rows.iter())
            .filter_map(|cells| cells.get(column))
            .map(|cell| cell.text.chars().count())
            .max()
            .unwrap_or(0)
    };
    let widths: Vec<Length> = (0..header.len())
        .map(|column| match longest(column) {
            chars if chars <= SHORT => Length::Shrink,
            chars => Length::FillPortion(chars.min(120) as u16),
        })
        .collect();
    let columns = header.into_iter().zip(widths).enumerate().map(move |(index, (head, width))| {
        iced::widget::table::column(styled(look, &head, TEXT_STRONG, true, size), move |cells: Vec<Inline>| {
            let cell = cells.get(index).cloned().unwrap_or_default();
            styled(look, &cell, TEXT, false, size)
        })
        .width(width)
    });
    iced::widget::table(columns, rows).width(Fill).padding_x(10).padding_y(6).into()
}

/// Text with bold, code and link ranges drawn as spans.
fn styled<'a, M: 'a>(look: Look, inline: &Inline, color: Color, strong: bool, size: f32) -> Element<'a, M> {
    let base = if strong { look.bold } else { look.text };
    // In a document, code sits in prose set in another font: a touch smaller looks the same size.
    let code_size = if look.document { size * 0.9 } else { size };
    let mut spans: Vec<text::Span<'a, ()>> = Vec::new();
    let mut at = 0;
    let mut ranges: Vec<_> = inline.spans.iter().filter(|(r, _)| r.end <= inline.text.len()).cloned().collect();
    ranges.sort_by_key(|(r, _)| r.start);
    for (range, kind) in ranges {
        if range.start < at || !inline.text.is_char_boundary(range.start) || !inline.text.is_char_boundary(range.end) {
            continue;
        }
        if range.start > at {
            spans.push(span(inline.text[at..range.start].to_string()).font(base).color(color));
        }
        let piece = inline.text[range.clone()].to_string();
        spans.push(match kind {
            Span::Bold => span(piece).font(look.bold).color(TEXT_STRONG),
            Span::Code => span(piece).font(look.code).size(code_size).color(style::CODE).background(alpha(WORKING, 0x1a)),
            Span::Link => span(piece).font(base).color(WORKING).underline(true),
        });
        at = range.end;
    }
    if at < inline.text.len() {
        spans.push(span(inline.text[at..].to_string()).font(base).color(color));
    }
    rich_text(spans).size(size).line_height(look.line_height).into()
}

// ---- Timeline ------------------------------------------------------------------------

pub fn timeline<'a, M: 'a>(s: &Session) -> Vec<Element<'a, M>> {
    if s.timeline.is_empty() {
        return vec![style::hint(t(
            "Noch keine Ereignisse. Sie erscheinen, sobald die Hooks aktiv sind (brain install).",
            "No events yet. They appear once the hooks are installed (brain install).",
        ))];
    }
    s.timeline.iter().rev().take(80).map(timeline_entry).collect()
}

fn timeline_entry<'a, M: 'a>(event: &Event) -> Element<'a, M> {
    let raw = event.text.clone().unwrap_or_default();
    let (color, label): (Color, String) = match event.kind {
        Kind::SessionStart => (style::ENDED, t("Session gestartet", "Session started").into()),
        Kind::SessionEnd => (style::ENDED, t("Session beendet", "Session ended").into()),
        Kind::Prompt => match classify_prompt(&raw) {
            Some(Prompt::User(prompt)) => (TURN, format!("{}: {prompt}", t("Du", "You"))),
            Some(Prompt::System(note_text)) => (style::ENDED, note(note_text.trim_matches(['[', ']']))),
            None => (TURN, t("Neuer Prompt", "New prompt").into()),
        },
        Kind::Permission => (style::CALLS, if raw.is_empty() { t("Braucht Freigabe", "Needs permission").into() } else { plain(&raw) }),
        Kind::Stop => (TEXT_FAINT, if raw.is_empty() { t("Turn beendet", "Turn ended").into() } else { plain(&raw) }),
        Kind::Doing => (WORKING, raw),
        Kind::Waiting => (style::CALLS, format!("{}: {raw}", t("Frage", "Question"))),
        Kind::Done => (style::DONE, format!("{}: {raw}", t("Erledigt", "Done"))),
    };
    let reported = event.source == Source::Report;
    let time = event.ts.with_timezone(&Local).format("%H:%M").to_string();
    // Three lines at most, like the GPUI version's line clamp.
    let label = clip(&label, 300);
    let mut line = row![
        container(text(time).size(11.5).color(TEXT_FAINT).font(UI)).width(36).padding(Padding { top: 1.0, ..Padding::ZERO }),
        container(style::dot::<M>(color, 7.0)).padding(Padding { top: 6.0, ..Padding::ZERO }),
        container(text(label).size(13).color(if reported { TEXT_STRONG } else { TEXT }).font(UI).line_height(1.45)).width(Fill),
    ]
    .spacing(12)
    .padding([6, 0]);
    if reported {
        line = line.push(
            container(text(t("gemeldet", "reported")).size(10.5).color(WORKING).font(UI))
                .padding([0, 6])
                .style(|_| boxed(alpha(WORKING, 0x1f), Color::TRANSPARENT, 4.0)),
        );
    }
    line.into()
}
