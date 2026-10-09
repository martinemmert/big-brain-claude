//! The detail pane's content: the conversation, drawn like Claude Code in the terminal (in the
//! terminal's font), and the event timeline.

use brain_core::event::{Event, Kind, Source};
use brain_core::markdown::{self, Block, Inline, Span};
use brain_core::state::Session;
use brain_core::transcript::{classify_prompt, Message, Prompt, Role};
use chrono::Local;
use iced::widget::{column, container, rich_text, row, span, text, Space};
use iced::{Border, Color, Element, Fill, Length, Padding};

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
            Role::Assistant => gutter(font, DOT, TEXT, body_in(font, &message.text)),
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

/// A Markdown file for the reader, in the chat font.
pub fn document<'a, M: 'a>(source: &str) -> Element<'a, M> {
    body_in(chat_font::get(), source)
}

fn body_in<'a, M: 'a>(font: &ChatFont, source: &str) -> Element<'a, M> {
    column(markdown::parse(source).into_iter().map(|b| block(font, b))).spacing(font.size * 0.6).into()
}

fn block<'a, M: 'a>(font: &ChatFont, block: Block) -> Element<'a, M> {
    match block {
        // The terminal has one size: headings are bold, the first levels also brighter.
        Block::Heading { level, text: heading } => styled(font, &heading, if level <= 2 { TEXT_STRONG } else { TEXT }, true),
        Block::Paragraph(paragraph) => styled(font, &paragraph, TEXT, false),
        Block::Item { indent, marker, text: item } => {
            let marker = if marker == "•" { "-".to_string() } else { marker };
            let width = font.size * 0.62 * (marker.chars().count() as f32 + 1.0);
            row![
                container(text(marker).size(font.size).color(TEXT_MUTED).font(font.regular)).width(Length::Fixed(width)),
                container(styled(font, &item, TEXT, false)).width(Fill),
            ]
            .padding(Padding { left: indent as f32 * font.size * 0.62 * 2.0, ..Padding::ZERO })
            .into()
        }
        Block::Code { text: code, .. } => container(text(code).size(font.size).color(style::CODE).font(font.regular).line_height(1.45))
            .padding(Padding { left: font.size * 0.62 * 2.0, ..Padding::ZERO })
            .into(),
        Block::Quote(quote) => row![
            text("│ ").size(font.size).color(TEXT_FAINT).font(font.regular),
            styled(font, &quote, TEXT_MUTED, false),
        ]
        .into(),
        Block::Table { header, rows } => table(font, header, rows),
        Block::Rule => text("─".repeat(40)).size(font.size).color(TEXT_FAINT).font(font.regular).into(),
    }
}

fn table<'a, M: 'a>(font: &ChatFont, header: Vec<Inline>, rows: Vec<Vec<Inline>>) -> Element<'a, M> {
    let line = |cells: Vec<Inline>, head: bool| -> Element<'a, M> {
        let cells = cells.into_iter().map(|cell| container(styled(font, &cell, if head { TEXT_STRONG } else { TEXT }, head)).padding([3, 8]).width(Fill).into());
        container(row(cells)).width(Fill).style(move |_| if head { style::fill(SURFACE) } else { container::Style::default() }).into()
    };
    let mut lines = vec![line(header, true)];
    for cells in rows {
        lines.push(container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)).into());
        lines.push(line(cells, false));
    }
    container(column(lines))
        .style(|_| container::Style { border: Border { color: LINE, width: 1.0, radius: 4.0.into() }, ..container::Style::default() })
        .clip(true)
        .into()
}

/// Text with bold, code and link ranges drawn as spans, all in the chat font.
fn styled<'a, M: 'a>(font: &ChatFont, inline: &Inline, color: Color, strong: bool) -> Element<'a, M> {
    let base = if strong { font.bold } else { font.regular };
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
            Span::Bold => span(piece).font(font.bold).color(TEXT_STRONG),
            Span::Code => span(piece).font(font.regular).color(style::CODE).background(alpha(WORKING, 0x1a)),
            Span::Link => span(piece).font(base).color(WORKING).underline(true),
        });
        at = range.end;
    }
    if at < inline.text.len() {
        spans.push(span(inline.text[at..].to_string()).font(base).color(color));
    }
    rich_text(spans).size(font.size).line_height(1.45).into()
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
