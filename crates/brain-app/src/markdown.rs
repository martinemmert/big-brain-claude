//! The detail pane's content: conversation messages (with Markdown) and the event timeline.

use brain_core::event::{Event, Kind, Source};
use brain_core::markdown::{self, Block, Inline, Span};
use brain_core::state::Session;
use brain_core::transcript::{classify_prompt, Message, Prompt, Role};
use chrono::Local;
use iced::widget::{column, container, rich_text, row, span, text, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Padding};

use crate::format::{clip, note, plain, tilde};
use crate::i18n::t;
use crate::style::{self, alpha, boxed, LINE, LINE_STRONG, MONO, SURFACE, TEXT, TEXT_FAINT, TEXT_MUTED, TEXT_STRONG, TURN, UI, WORKING};
use crate::tr;

/// Consecutive tool calls are shown as one block; long runs are cut to this many rows.
const TOOL_ROWS_SHOWN: usize = 6;

pub fn conversation<'a, M: 'a>(messages: &[Message]) -> Vec<Element<'a, M>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let message = &messages[i];
        if message.role == Role::Tool {
            let run = messages[i..].iter().take_while(|m| m.role == Role::Tool).count();
            out.push(tool_block(&messages[i..i + run]));
            i += run;
            continue;
        }
        out.push(match message.role {
            Role::User => user_bubble(message),
            Role::Assistant => assistant_reply(message),
            Role::System => system_note(&message.text),
            Role::Tool => unreachable!(),
        });
        i += 1;
    }
    out
}

fn time_of(message: &Message) -> String {
    message.ts.map(|t| t.with_timezone(&Local).format("%H:%M").to_string()).unwrap_or_default()
}

fn user_bubble<'a, M: 'a>(message: &Message) -> Element<'a, M> {
    // Very long prompts (pasted logs) are cut like the GPUI version's 14-line clamp.
    let body: String = message.text.lines().take(14).collect::<Vec<_>>().join("\n");
    column![
        text(format!("{} · {}", t("Du", "You"), time_of(message))).size(11).color(TEXT_FAINT).font(UI),
        container(text(body).size(13).color(TEXT_STRONG).font(UI).line_height(1.5))
            .padding([9, 13])
            .max_width(620)
            .style(|_| container::Style {
                background: Some(Background::Color(alpha(TURN, 0x1c))),
                border: Border {
                    color: alpha(TURN, 0x40),
                    width: 1.0,
                    radius: iced::border::Radius { top_left: 12.0, top_right: 4.0, bottom_right: 12.0, bottom_left: 12.0 },
                },
                ..container::Style::default()
            }),
    ]
    .spacing(4)
    .align_x(iced::Right)
    .width(Fill)
    .into()
}

fn assistant_reply<'a, M: 'a>(message: &Message) -> Element<'a, M> {
    column![
        row![
            style::dot(WORKING, 6.0),
            text("Claude").size(11).color(TEXT_MUTED).font(style::semibold()),
            text(time_of(message)).size(11).color(TEXT_FAINT).font(UI),
        ]
        .spacing(6)
        .align_y(iced::Center),
        body(&message.text),
    ]
    .spacing(6)
    .into()
}

fn system_note<'a, M: 'a>(label: &str) -> Element<'a, M> {
    row![
        container(Space::new().width(14).height(1)).style(|_| style::fill(LINE_STRONG)),
        text(clip(&note(label), 140)).size(11.5).color(TEXT_FAINT).font(UI),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .padding([0, 4])
    .into()
}

fn tool_block<'a, M: 'a>(calls: &[Message]) -> Element<'a, M> {
    let hidden = calls.len().saturating_sub(TOOL_ROWS_SHOWN);
    let mut rows: Vec<Element<'a, M>> = Vec::new();
    if hidden > 0 {
        rows.push(text(tr!("{hidden} frühere Tool-Aufrufe", "{hidden} earlier tool calls")).size(11).color(TEXT_FAINT).font(UI).into());
    }
    rows.extend(calls[hidden..].iter().map(tool_row));
    row![
        container(Space::new().width(2).height(Length::Fill)).style(|_| style::fill(LINE_STRONG)),
        column(rows).spacing(1).padding([2, 0]),
    ]
    .spacing(12)
    .height(Length::Shrink)
    .padding(Padding { left: 2.0, ..Padding::ZERO })
    .into()
}

fn tool_row<'a, M: 'a>(call: &Message) -> Element<'a, M> {
    let tool = call.tool.clone().unwrap_or_default();
    row![
        container(text(tool_icon(&tool)).size(12).color(TEXT_FAINT).font(UI)).width(14),
        text(short_tool_name(&tool)).size(12).color(TEXT_MUTED).font(style::medium()),
        text(clip(&tilde(&note(&call.text)), 110)).size(11.5).color(TEXT).font(MONO).wrapping(text::Wrapping::None),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .padding([2, 0])
    .into()
}

fn tool_icon(tool: &str) -> &'static str {
    match tool {
        "Bash" => "❯",
        "Read" => "◱",
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => "✎",
        "Grep" | "Glob" => "⌕",
        "WebSearch" | "WebFetch" => "◍",
        "Agent" | "Task" => "◆",
        "TodoWrite" => "☑",
        _ => "⚙",
    }
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

pub fn body<'a, M: 'a>(source: &str) -> Element<'a, M> {
    column(markdown::parse(source).into_iter().map(block)).spacing(8).into()
}

fn block<'a, M: 'a>(block: Block) -> Element<'a, M> {
    match block {
        Block::Heading { level, text: heading } => {
            container(styled(&heading, if level <= 2 { 15.0 } else { 13.5 }, TEXT_STRONG, true)).padding(Padding { top: 4.0, ..Padding::ZERO }).into()
        }
        Block::Paragraph(paragraph) => styled(&paragraph, 13.0, TEXT, false),
        Block::Item { indent, marker, text: item } => row![
            container(text(marker.clone()).size(13).color(if marker == "•" { TEXT_FAINT } else { TEXT_MUTED }).font(UI)).width(Length::Fixed(14.0)),
            container(styled(&item, 13.0, TEXT, false)).width(Fill),
        ]
        .spacing(8)
        .padding(Padding { left: indent as f32 * 18.0, ..Padding::ZERO })
        .into(),
        Block::Code { lang, text: code } => {
            let label: Element<'a, M> = match lang {
                Some(lang) => container(text(lang).size(10).color(TEXT_FAINT).font(UI)).align_right(Fill).into(),
                None => Space::new().into(),
            };
            container(column![label, text(code).size(12).color(TEXT).font(MONO).line_height(1.5)].spacing(2))
                .padding([10, 12])
                .width(Fill)
                .style(|_| boxed(style::INK, LINE, 8.0))
                .into()
        }
        Block::Quote(quote) => row![
            container(Space::new().width(2).height(Length::Fill)).style(|_| style::fill(LINE_STRONG)),
            styled(&quote, 13.0, TEXT_MUTED, false),
        ]
        .spacing(12)
        .height(Length::Shrink)
        .into(),
        Block::Table { header, rows } => table(header, rows),
        Block::Rule => container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)).padding([4, 0]).into(),
    }
}

fn table<'a, M: 'a>(header: Vec<Inline>, rows: Vec<Vec<Inline>>) -> Element<'a, M> {
    let line = |cells: Vec<Inline>, head: bool| -> Element<'a, M> {
        let cells = cells.into_iter().map(|cell| container(styled(&cell, 12.5, if head { TEXT_STRONG } else { TEXT }, head)).padding([5, 10]).width(Fill).into());
        container(row(cells)).width(Fill).style(move |_| if head { style::fill(SURFACE) } else { container::Style::default() }).into()
    };
    let mut lines = vec![line(header, true)];
    for cells in rows {
        lines.push(container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)).into());
        lines.push(line(cells, false));
    }
    container(column(lines)).style(|_| boxed(Color::TRANSPARENT, LINE, 8.0)).clip(true).into()
}

/// Text with bold, code and link ranges drawn as spans.
fn styled<'a, M: 'a>(inline: &Inline, size: f32, color: Color, strong: bool) -> Element<'a, M> {
    let base = if strong { style::semibold() } else { UI };
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
            Span::Bold => span(piece).font(style::semibold()).color(TEXT_STRONG),
            Span::Code => span(piece).font(MONO).color(style::CODE).background(alpha(WORKING, 0x22)),
            Span::Link => span(piece).font(base).color(WORKING),
        });
        at = range.end;
    }
    if at < inline.text.len() {
        spans.push(span(inline.text[at..].to_string()).font(base).color(color));
    }
    rich_text(spans).size(size).line_height(1.55).into()
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
