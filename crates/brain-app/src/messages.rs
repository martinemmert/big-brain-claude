//! The detail pane's content: conversation messages (with Markdown) and the event timeline.

use brain_core::event::{Event, Kind, Source};
use brain_core::markdown::{self, Block, Inline, Span};
use brain_core::state::Session;
use brain_core::transcript::{classify_prompt, Message, Prompt, Role};
use chrono::Local;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gpui::{
    div, point, prelude::*, px, relative, size, AnyElement, App, AvailableSpace, Bounds, ClickEvent, Element, ElementId,
    FontWeight, GlobalElementId, HighlightStyle, InspectorElementId, LayoutId, Pixels, SharedString, Window,
};

use crate::i18n::t;
use crate::selection;
use crate::widgets::{note, plain};
use crate::{theme, tr};

/// Consecutive tool calls are shown as one block; long runs are cut to this many rows.
const TOOL_ROWS_SHOWN: usize = 6;

/// The code block copied last and when, for the "Copied" confirmation.
static COPIED: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

/// Height of a copyable block's header (language and copy button).
const CODE_HEADER: f32 = 28.;

pub fn conversation(messages: &[Message]) -> Vec<AnyElement> {
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

fn user_bubble(message: &Message) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(4.))
        .child(div().text_size(px(11.)).text_color(theme::text_faint()).child(format!("{} · {}", t("Du", "You"), time_of(message))))
        .child(
            div()
                .max_w(relative(0.82))
                .px(px(13.))
                .py(px(9.))
                .rounded(px(12.))
                .rounded_tr(px(4.))
                .bg(theme::alpha(theme::turn(), 0x1c))
                .border_1()
                .border_color(theme::alpha(theme::turn(), 0x40))
                .text_color(theme::text_strong())
                .line_height(relative(1.5))
                .line_clamp(14)
                .child(selection::plain(message.text.clone())),
        )
        .into_any_element()
}

fn assistant_reply(message: &Message) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(11.))
                .child(div().size(px(6.)).rounded_full().bg(theme::working()))
                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme::text_muted()).child("Claude"))
                .child(div().text_color(theme::text_faint()).child(time_of(message))),
        )
        .child(markdown_body(&message.text))
        .into_any_element()
}

fn system_note(text: &str) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(4.))
        .text_size(px(11.5))
        .text_color(theme::text_faint())
        .child(div().h(px(1.)).w(px(14.)).bg(theme::line_strong()))
        .child(div().flex_1().min_w_0().truncate().child(selection::plain(note(text))))
        .into_any_element()
}

fn tool_block(calls: &[Message]) -> AnyElement {
    let hidden = calls.len().saturating_sub(TOOL_ROWS_SHOWN);
    div()
        .flex()
        .flex_col()
        .gap(px(1.))
        .ml(px(2.))
        .pl(px(12.))
        .py(px(2.))
        .border_l_2()
        .border_color(theme::line_strong())
        .when(hidden > 0, |d| {
            d.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_faint())
                    .pb(px(2.))
                    .child(tr!("{hidden} frühere Tool-Aufrufe", "{hidden} earlier tool calls")),
            )
        })
        .children(calls[hidden..].iter().map(tool_row))
        .into_any_element()
}

fn tool_row(call: &Message) -> AnyElement {
    let tool = call.tool.clone().unwrap_or_default();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .py(px(2.))
        .text_size(px(12.))
        .child(div().flex_none().w(px(14.)).text_color(theme::text_faint()).child(tool_icon(&tool)))
        .child(
            div()
                .flex_none()
                .text_color(theme::text_muted())
                .font_weight(FontWeight::MEDIUM)
                .child(selection::plain(short_tool_name(&tool))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme::MONO)
                .text_size(px(11.5))
                .text_color(theme::text())
                .child(selection::plain(theme::tilde(&note(&call.text)))),
        )
        .into_any_element()
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

pub fn markdown_body(source: &str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .text_color(theme::text())
        .line_height(relative(1.55))
        .children(markdown::parse(source).into_iter().map(block))
        .into_any_element()
}

fn block(block: Block) -> AnyElement {
    match block {
        Block::Heading { level, text } => div()
            .mt(px(4.))
            .text_size(px(if level <= 2 { 15. } else { 13.5 }))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text_strong())
            .child(styled(&text))
            .into_any_element(),
        Block::Paragraph(text) => div().child(styled(&text)).into_any_element(),
        Block::Item { indent, marker, text } => div()
            .flex()
            .items_start()
            .gap(px(8.))
            .pl(px(indent as f32 * 18.))
            .mt(px(-4.))
            .child(
                div()
                    .flex_none()
                    .min_w(px(14.))
                    .text_color(if marker == "•" { theme::text_faint() } else { theme::text_muted() })
                    .child(marker),
            )
            .child(div().flex_1().min_w_0().child(styled(&text)))
            .into_any_element(),
        Block::Code { lang, text } if copyable(lang.as_deref()) => copyable_code(lang, text),
        Block::Code { lang, text } => div()
            .relative()
            .px(px(12.))
            .py(px(10.))
            .rounded(px(8.))
            .bg(theme::ink())
            .border_1()
            .border_color(theme::line())
            .font_family(theme::MONO)
            .text_size(px(12.))
            .line_height(relative(1.5))
            .text_color(theme::text())
            .child(selection::plain(text))
            .when_some(lang, |d, lang| {
                d.child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .right(px(10.))
                        .text_size(px(10.))
                        .text_color(theme::text_faint())
                        .child(lang),
                )
            })
            .into_any_element(),
        Block::Quote(text) => div()
            .pl(px(12.))
            .border_l_2()
            .border_color(theme::line_strong())
            .text_color(theme::text_muted())
            .child(styled(&text))
            .into_any_element(),
        Block::Table { header, rows } => table(header, rows),
        Block::Rule => div().h(px(1.)).my(px(4.)).bg(theme::line()).into_any_element(),
    }
}

/// Code blocks that get a copy button; add languages here (or return `true` for all).
fn copyable(lang: Option<&str>) -> bool {
    matches!(lang, Some("markdown" | "md"))
}

/// A code block with a sticky header holding its language and a copy button.
fn copyable_code(lang: Option<String>, text: String) -> AnyElement {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let id = hasher.finish();
    let copied = COPIED.lock().unwrap().is_some_and(|(last, at)| last == id && at.elapsed() < Duration::from_secs(2));
    let button = div()
        .id(SharedString::from(format!("copy-{id:x}")))
        .flex_none()
        .px(px(7.))
        .py(px(2.))
        .rounded(px(5.))
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::ink())
        .font_family(".SystemUIFont")
        .text_size(px(10.5))
        .text_color(if copied { theme::done() } else { theme::text_muted() })
        .cursor_pointer()
        .hover(|d| d.bg(theme::hover()).text_color(theme::text_strong()))
        .child(if copied { t("✓ Kopiert", "✓ Copied") } else { t("Kopieren", "Copy") })
        .on_click({
            let text = text.clone();
            move |_: &ClickEvent, _, cx| {
                crate::clipboard::copy(&text, cx);
                *COPIED.lock().unwrap() = Some((id, Instant::now()));
                cx.refresh_windows();
            }
        });
    let block = div()
        .px(px(12.))
        .pt(px(CODE_HEADER + 4.))
        .pb(px(10.))
        .rounded(px(8.))
        .bg(theme::ink())
        .border_1()
        .border_color(theme::line())
        .child(div().font_family(theme::MONO).text_size(px(12.)).line_height(relative(1.5)).text_color(theme::text()).child(selection::plain(text)));
    let header = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(6.))
        .rounded(px(5.))
        .bg(theme::ink())
        .text_size(px(10.))
        .text_color(theme::text_faint())
        .children(lang)
        .child(button);
    StickyHeader { block: block.into_any_element(), header: header.into_any_element() }.into_any_element()
}

/// Keeps `header` at the block's top right, or at the top of the visible area while the block
/// is scrolled past (GPUI has no `position: sticky`).
struct StickyHeader {
    block: AnyElement,
    header: AnyElement,
}

impl IntoElement for StickyHeader {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for StickyHeader {
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
        (self.block.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        const INSET: f32 = 6.;
        self.block.prepaint(window, cx);
        let header = self.header.layout_as_root(size(AvailableSpace::MinContent, AvailableSpace::MinContent), window, cx);
        let visible_top = f32::from(window.content_mask().bounds.top());
        let top = f32::from(bounds.top());
        let room = (f32::from(bounds.size.height) - f32::from(header.height) - 2. * INSET).max(0.);
        let offset = (visible_top - top).clamp(0., room);
        let origin = point(bounds.right() - header.width - px(10.), px(top + INSET + offset));
        self.header.prepaint_at(origin, window, cx);
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
        self.block.paint(window, cx);
        self.header.paint(window, cx);
    }
}

fn table(header: Vec<Inline>, rows: Vec<Vec<Inline>>) -> AnyElement {
    let row = |cells: Vec<Inline>, head: bool| {
        div()
            .flex()
            .when(head, |d| d.bg(theme::surface()).font_weight(FontWeight::SEMIBOLD).text_color(theme::text_strong()))
            .when(!head, |d| d.border_t_1().border_color(theme::line()))
            .children(cells.into_iter().map(|cell| {
                div().flex_1().min_w_0().px(px(10.)).py(px(5.)).text_size(px(12.5)).child(styled(&cell))
            }))
    };
    div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(theme::line())
        .overflow_hidden()
        .child(row(header, true))
        .children(rows.into_iter().map(|cells| row(cells, false)))
        .into_any_element()
}

fn styled(inline: &Inline) -> AnyElement {
    let highlights: Vec<_> = inline
        .spans
        .iter()
        .map(|(range, span)| {
            let style = match span {
                Span::Bold => HighlightStyle {
                    font_weight: Some(FontWeight::SEMIBOLD),
                    color: Some(theme::text_strong().into()),
                    ..Default::default()
                },
                Span::Code => HighlightStyle {
                    color: Some(gpui::rgb(0xc8d4ff).into()),
                    background_color: Some(theme::alpha(theme::working(), 0x22).into()),
                    ..Default::default()
                },
                Span::Link => HighlightStyle { color: Some(theme::working().into()), ..Default::default() },
            };
            (range.clone(), style)
        })
        .collect();
    selection::text(inline.text.clone(), highlights)
}

// ---- Timeline ------------------------------------------------------------------------

pub fn timeline(s: &Session) -> Vec<AnyElement> {
    if s.timeline.is_empty() {
        return vec![div()
            .py_2()
            .text_sm()
            .text_color(theme::text_muted())
            .child(t(
                "Noch keine Ereignisse. Sie erscheinen, sobald die Hooks aktiv sind (brain install).",
                "No events yet. They appear once the hooks are installed (brain install).",
            ))
            .into_any_element()];
    }
    s.timeline[s.timeline.len().saturating_sub(80)..].iter().map(timeline_entry).collect()
}

fn timeline_entry(event: &Event) -> AnyElement {
    let text = event.text.clone().unwrap_or_default();
    let (color, label): (gpui::Rgba, String) = match event.kind {
        Kind::SessionStart => (theme::ended(), t("Session gestartet", "Session started").into()),
        Kind::SessionEnd => (theme::ended(), t("Session beendet", "Session ended").into()),
        Kind::Prompt => match classify_prompt(&text) {
            Some(Prompt::User(prompt)) => (theme::turn(), format!("{}: {prompt}", t("Du", "You"))),
            Some(Prompt::System(note_text)) => (theme::ended(), note(note_text.trim_matches(['[', ']']))),
            None => (theme::turn(), t("Neuer Prompt", "New prompt").into()),
        },
        Kind::Permission => (theme::calls(), if text.is_empty() { t("Braucht Freigabe", "Needs permission").into() } else { plain(&text) }),
        Kind::Stop => (theme::text_faint(), if text.is_empty() { t("Turn beendet", "Turn ended").into() } else { plain(&text) }),
        Kind::Doing => (theme::working(), text),
        Kind::Waiting => (theme::calls(), format!("{}: {text}", t("Frage", "Question"))),
        Kind::Done => (theme::done(), format!("{}: {text}", t("Erledigt", "Done"))),
    };
    let reported = event.source == Source::Report;
    let time = event.ts.with_timezone(&Local).format("%H:%M").to_string();

    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .py(px(6.))
        .child(div().flex_none().w(px(36.)).text_size(px(11.5)).text_color(theme::text_faint()).pt(px(1.)).child(selection::plain(time)))
        .child(div().flex_none().mt(px(6.)).size(px(7.)).rounded_full().bg(color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .line_clamp(3)
                .line_height(relative(1.45))
                .text_color(if reported { theme::text_strong() } else { theme::text() })
                .child(selection::plain(label)),
        )
        .when(reported, |d| {
            d.child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .rounded(px(4.))
                    .bg(theme::alpha(theme::working(), 0x1f))
                    .text_size(px(10.5))
                    .text_color(theme::working())
                    .child(t("gemeldet", "reported")),
            )
        })
        .into_any_element()
}
