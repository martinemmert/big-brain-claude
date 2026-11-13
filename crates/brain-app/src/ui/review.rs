//! What review comments add to the Quick Look sheet: the "+" in front of a diff line, the field
//! and the cards under it, the counts in the list of files, and the review bar at the bottom.

use brain_core::review::{Comment, Place, Review, State};
use brain_core::state::SessionKey;
use iced::widget::{button, column, container, row, scrollable, text, text_editor, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Padding};

use super::scrollbar;
use crate::app::{Brain, Message, ReviewEditor, REVIEW_EDITOR};
use crate::format::{clip, clock};
use crate::i18n::t;
use crate::style::{self, alpha, boxed, action, BACKGROUND, CHROME, DONE, INK, LINE, LINE_STRONG, MONO, PRIMARY, RAISED, SURFACE, TEXT, TEXT_FAINT, TEXT_MUTED, TEXT_STRONG, UI, WORKING};
use crate::tr;

/// The cards and the field are this wide at most.
const CARD_W: f32 = 620.0;

/// The width of the column in front of the diff's numbers, where the "+" appears.
pub const PLUS_W: f32 = 22.0;

/// The "+" in front of a diff row: blue while the pointer is over the row, else a dot when the row
/// has comments, else nothing.
pub fn plus<'a>(row: usize, hovered: bool, commented: Option<bool>) -> Element<'a, Message> {
    if hovered {
        let plus = button(container(style::plus(10.0, Color::WHITE)).center(Fill))
            .width(18)
            .height(18)
            .padding(0)
            .on_press(Message::ReviewCommentLine(row))
            .style(|_, status| button::Style {
                background: Some(Background::Color(if matches!(status, button::Status::Hovered | button::Status::Pressed) { alpha(PRIMARY, 0xdd) } else { PRIMARY })),
                border: iced::border::rounded(4),
                ..button::Style::default()
            });
        return container(plus).width(PLUS_W).height(Fill).align_y(iced::Center).padding(Padding { left: 3.0, ..Padding::ZERO }).into();
    }
    let mark: Element<'a, Message> = match commented {
        Some(queued) => style::dot(if queued { BACKGROUND } else { TEXT_MUTED }, 5.0),
        None => Space::new().width(5).height(5).into(),
    };
    container(mark).width(PLUS_W).height(Fill).align_y(iced::Center).padding(Padding { left: 8.0, ..Padding::ZERO }).into()
}

fn chip<'a>(label: String, color: Color, fill: Color, edge: Color) -> Element<'a, Message> {
    container(text(label).size(11).color(color).font(style::semibold()))
        .padding([1, 6])
        .style(move |_| boxed(fill, edge, 5.0))
        .into()
}

/// A comment as a card under its line. Drafts can be changed and deleted. `gone`: its line is
/// not in the diff any more, so it sits at the head of the file and says so.
pub fn card<'a>(comment: &'a Comment, number: Option<usize>, gone: bool) -> Element<'a, Message> {
    let state = match comment.state {
        State::Draft => chip(t("Entwurf", "Draft").into(), TEXT, LINE_STRONG, Color::TRANSPARENT),
        State::Queued => chip(t("wartet", "waiting").into(), BACKGROUND, alpha(BACKGROUND, 0x24), alpha(BACKGROUND, 0x66)),
        State::Sent => {
            let at = comment.sent_ms.map(clock).unwrap_or_default();
            chip(tr!("gesendet {at}", "sent {at}"), TEXT_FAINT, Color::TRANSPARENT, LINE_STRONG)
        }
        State::Changed => chip(t("von Claude geändert", "changed by Claude").into(), DONE, alpha(DONE, 0x1f), Color::TRANSPARENT),
    };
    let place = match (comment.place, gone) {
        (Place::File, _) => t("ganze Datei", "whole file").to_string(),
        (Place::Line { new: Some(n), .. }, false) => tr!("Zeile {n}", "line {n}"),
        (Place::Line { new: None, old: Some(o) }, false) => tr!("entfernte Zeile {o}", "removed line {o}"),
        (_, true) => t("Zeile gibt es nicht mehr", "the line is gone").to_string(),
        _ => String::new(),
    };
    let mut head = row![].spacing(8).align_y(iced::Center).height(22);
    if let Some(n) = number {
        head = head.push(text(format!("{n}.")).size(11.5).color(TEXT).font(style::semibold()));
    }
    head = head.push(state).push(text(place).size(11).color(TEXT_FAINT).font(MONO)).push(Space::new().width(Fill));
    if comment.state == State::Draft {
        head = head
            .push(style::text_button(t("Bearbeiten", "Edit"), false, Message::ReviewEditComment(comment.id)))
            .push(style::text_button(t("Löschen", "Delete"), false, Message::ReviewDelete(comment.id)));
    }
    let sent = matches!(comment.state, State::Sent | State::Changed);
    let mut body = column![head, text(comment.text.clone()).size(13).color(if sent { TEXT_MUTED } else { TEXT_STRONG }).font(UI)].spacing(5);
    let old_line = || -> Element<'a, Message> {
        let line = comment.anchor.trim().to_string();
        iced::widget::rich_text(vec![iced::widget::span::<(), _>(clip(&line, 90)).strikethrough(true).color(TEXT_MUTED)]).size(11).font(MONO).into()
    };
    if comment.state == State::Changed {
        body = body.push(row![text(t("vorher", "before")).size(11.5).color(TEXT_FAINT).font(UI), old_line()].spacing(6).align_y(iced::Center));
    } else if gone && comment.place != Place::File {
        body = body.push(row![text(t("war", "was")).size(11.5).color(TEXT_FAINT).font(UI), old_line()].spacing(6).align_y(iced::Center));
    }
    let (fill, edge) = match comment.state {
        State::Sent => (SURFACE, LINE),
        State::Changed => (SURFACE, alpha(DONE, 0x66)),
        _ => (RAISED, LINE_STRONG),
    };
    container(body)
        .padding(Padding { top: 8.0, right: 10.0, bottom: 10.0, left: 12.0 })
        .max_width(CARD_W)
        .width(Fill)
        .style(move |_| boxed(fill, edge, 8.0))
        .into()
}

/// The field for a comment: what it is on, the text, save and cancel.
pub fn editor<'a>(editor: &'a ReviewEditor) -> Element<'a, Message> {
    let what: Element<'a, Message> = match editor.place {
        Place::File => {
            let name = editor.relative.rsplit('/').next().unwrap_or(&editor.relative);
            text(tr!("Kommentar zur ganzen Datei {name}", "Comment on the whole file {name}")).size(11.5).color(TEXT_MUTED).font(UI).into()
        }
        Place::Line { new, old } => {
            let lead = match (new, old) {
                (Some(n), _) => tr!("Zeile {n}", "Line {n}"),
                (None, Some(o)) => tr!("Entfernte Zeile {o}", "Removed line {o}"),
                _ => String::new(),
            };
            row![
                text(lead).size(11.5).color(TEXT_MUTED).font(UI),
                text(clip(&editor.anchor, 80)).size(11).color(TEXT_FAINT).font(MONO).wrapping(text::Wrapping::None),
            ]
            .spacing(8)
            .into()
        }
    };
    let field = text_editor(&editor.content)
        .id(REVIEW_EDITOR)
        .on_action(Message::ReviewEdit)
        .placeholder(t("Kommentar", "Comment"))
        .height(62)
        .padding([7, 9])
        .size(13)
        .font(UI)
        .style(|_, _| text_editor::Style {
            background: Background::Color(INK),
            border: Border { color: LINE_STRONG, width: 1.0, radius: 6.0.into() },
            placeholder: TEXT_FAINT,
            value: TEXT_STRONG,
            selection: alpha(WORKING, 0x55),
        });
    let hint = if editor.edit_id.is_some() { t("Änderung am Entwurf", "Changing the draft") } else { t("Kommt ins Review, geht noch nicht raus", "Joins the review, doesn't go out yet") };
    let buttons = row![
        text(hint).size(11.5).color(TEXT_FAINT).font(UI),
        Space::new().width(Fill),
        action(t("Abbrechen", "Cancel"), "esc", false, Some(Message::ReviewCancel)),
        action(t("Speichern", "Save"), "⌘⏎", false, Some(Message::ReviewSave)),
    ]
    .spacing(6)
    .align_y(iced::Center);
    container(column![container(what).padding([0, 4]), field, buttons].spacing(8))
        .padding(8)
        .max_width(CARD_W)
        .width(Fill)
        .style(|_| boxed(SURFACE, PRIMARY, 8.0))
        .into()
}

/// What stacks under a line or at a file's head: the cards, in a column that starts where the
/// code does.
pub fn slot<'a>(items: Vec<Element<'a, Message>>, left: f32, width: Length) -> Element<'a, Message> {
    container(column(items).spacing(6)).padding(Padding { top: 6.0, right: 24.0, bottom: 8.0, left }).width(width).into()
}

/// The cards of comments (with `true`: the line is gone); a draft being changed is its field.
pub fn cards<'a>(list: &[(&'a Comment, bool)], editing: Option<&'a ReviewEditor>, number: &dyn Fn(&Comment) -> Option<usize>) -> Vec<Element<'a, Message>> {
    list.iter()
        .map(|(comment, gone)| match editing.filter(|e| e.edit_id == Some(comment.id)) {
            Some(field) => editor(field),
            None => card(comment, number(comment), *gone),
        })
        .collect()
}

/// A file's entry in the list: how many comments it has (waiting ones in violet), or that its
/// review went out.
pub fn file_count<'a>(review: Option<&Review>, path: &str) -> Option<Element<'a, Message>> {
    let mine: Vec<&Comment> = review?.comments.iter().filter(|c| c.path == path).collect();
    let open = mine.iter().filter(|c| c.is_open()).count();
    if open > 0 {
        let queued = mine.iter().any(|c| c.state == State::Queued);
        let (color, fill, edge) = if queued { (BACKGROUND, alpha(BACKGROUND, 0x24), alpha(BACKGROUND, 0x66)) } else { (TEXT, RAISED, LINE_STRONG) };
        return Some(chip(open.to_string(), color, fill, edge));
    }
    (!mine.is_empty()).then(|| text(format!("✓ {}", mine.len())).size(11).color(TEXT_FAINT).font(UI).into())
}

/// The button that opens the field for the whole file, in the list on hover and in the strip.
pub fn file_button<'a>(label: Option<String>, index: usize) -> Element<'a, Message> {
    let plain = label.is_none();
    let content: Element<'a, Message> = match label {
        Some(label) => text(label).size(12).color(TEXT).font(UI).into(),
        None => container(style::plus(10.0, TEXT_MUTED)).center(Fill).into(),
    };
    button(if plain { content } else { style::centered(content) })
        .padding([0, if plain { 0 } else { 7 }])
        .width(if plain { Length::Fixed(22.0) } else { Length::Shrink })
        .height(22)
        .on_press(Message::ReviewCommentFile(index))
        .style(|_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if hovered { LINE_STRONG } else { RAISED })),
                text_color: TEXT,
                border: Border { color: LINE_STRONG, width: 1.0, radius: 5.0.into() },
                ..button::Style::default()
            }
        })
        .into()
}

/// A violet dot: a review waits for its session.
pub fn waiting_sign<'a>() -> Element<'a, Message> {
    style::dot(BACKGROUND, 7.0)
}

fn primary<'a>(label: String, keys: &'a str, on_press: Option<Message>) -> Element<'a, Message> {
    let enabled = on_press.is_some();
    let content = row![text(label).size(12.5).color(Color::WHITE).font(style::semibold()), text(keys).size(10.5).color(alpha(Color::WHITE, 0xcc)).font(UI)]
        .spacing(8)
        .align_y(iced::Center);
    button(style::centered(content))
        .padding([0, 12])
        .height(28)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hovered = enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
            let fade = |c: Color| if enabled { c } else { Color { a: c.a * 0.4, ..c } };
            button::Style {
                background: Some(Background::Color(fade(if hovered { alpha(PRIMARY, 0xdd) } else { PRIMARY }))),
                text_color: fade(Color::WHITE),
                border: iced::border::rounded(7),
                ..button::Style::default()
            }
        })
        .into()
}

fn plain<'a>(label: String, on_press: Message) -> Element<'a, Message> {
    style::text_button(label, false, on_press)
}

fn comments_word(n: usize) -> String {
    if n == 1 { t("1 Kommentar", "1 comment").to_string() } else { tr!("{n} Kommentare", "{n} comments") }
}

/// The message as Claude gets it, and under it the review bar: how many comments, a look at the
/// message, "Review an Claude schicken" and "Verwerfen"; a waiting review says so and can be
/// taken back.
pub fn bar<'a>(brain: &'a Brain, key: &SessionKey) -> Element<'a, Message> {
    let review = brain.review_of(key);
    let order = brain.review_file_order();
    let drafts = review.map_or(0, |r| r.count(State::Draft));
    let queued = review.map_or(0, |r| r.count(State::Queued));
    let sent = review.map_or(0, |r| r.count(State::Sent) + r.count(State::Changed));
    let changed = review.map_or(0, |r| r.count(State::Changed));

    // What the preview shows: the message that waits or would go, else the last one sent.
    let shown: Option<(String, String)> = review.and_then(|r| {
        if let Some(text) = r.pending_message(&order) {
            let label = if queued > 0 { t("Diese Nachricht wartet", "This message waits") } else { t("So bekommt Claude das Review", "What Claude gets") };
            Some((label.to_string(), text))
        } else {
            let at = r.last_sent_ms.map(clock).unwrap_or_default();
            r.last_message.clone().map(|text| (tr!("Gesendet um {at}", "Sent at {at}"), text))
        }
    });
    let toggle: Option<Element<'a, Message>> = shown.as_ref().map(|_| {
        let label = if brain.review_preview { t("Nachricht verbergen", "Hide message") } else { t("Nachricht ansehen", "Show message") };
        style::text_button(format!("{label} {}", if brain.review_preview { "▾" } else { "▴" }), false, Message::ReviewTogglePreview)
    });

    let last: Option<Element<'a, Message>> = (sent > 0).then(|| {
        let at = review.and_then(|r| r.last_sent_ms).map(clock).unwrap_or_default();
        let n = review.map_or(sent, |r| r.last_count);
        let word = comments_word(n);
        let mut line = tr!("✓ Letztes Review {at} · {word}", "✓ Last review {at} · {word}");
        if changed > 0 {
            line.push_str(&tr!(" · {changed} von Claude geändert", " · {changed} changed by Claude"));
        }
        text(line).size(12).color(if changed > 0 { DONE } else { TEXT_MUTED }).font(UI).into()
    });

    let mut line = row![].spacing(12).align_y(iced::Center).height(52).padding([0, 14]);
    if queued > 0 {
        let word = comments_word(queued);
        line = line.push(text(tr!("Review wartet – geht raus, sobald Claude fertig ist · {word}", "Review waiting – goes out once Claude is done · {word}")).size(12.5).color(BACKGROUND).font(style::semibold()));
        if let Some(toggle) = toggle {
            line = line.push(toggle);
        }
        line = line.push(Space::new().width(Fill)).push(text(brain.review_wait_reason(key)).size(12).color(TEXT_MUTED).font(UI));
        line = line.push(action(t("Zurücknehmen", "Take back"), "", false, Some(Message::ReviewUnqueue)));
    } else if drafts > 0 {
        let word = comments_word(drafts);
        line = line.push(text(tr!("{word} im Review", "{word} in the review")).size(12.5).color(TEXT_STRONG).font(style::semibold()));
        if let Some(toggle) = toggle {
            line = line.push(toggle);
        }
        if let Some(last) = last {
            line = line.push(last);
        }
        line = line.push(Space::new().width(Fill));
        if !brain.review_ready(key) {
            let why = brain.review_wait_reason(key);
            line = line.push(text(tr!("{why} Das Review wartet dann.", "{why} The review waits then.")).size(12).color(TEXT_MUTED).font(UI));
        }
        line = line.push(plain(t("Verwerfen", "Discard").into(), Message::ReviewDiscard));
        line = line.push(primary(tr!("Review an Claude schicken ({drafts})", "Send review to Claude ({drafts})"), "⇧⌘⏎", Some(Message::ReviewSend)));
    } else if let Some(last) = last {
        line = line.push(last);
        if let Some(toggle) = toggle {
            line = line.push(toggle);
        }
        line = line.push(Space::new().width(Fill)).push(text(t("Neue Kommentare starten ein neues Review.", "New comments start a new review.")).size(12).color(TEXT_MUTED).font(UI));
        line = line.push(primary(t("Review an Claude schicken", "Send review to Claude").into(), "", None));
    } else {
        line = line.push(text(t("Noch keine Kommentare. Über eine Zeile fahren und auf + klicken.", "No comments yet. Hover over a line and click +.")).size(12).color(TEXT_MUTED).font(UI));
        line = line.push(Space::new().width(Fill));
        line = line.push(primary(t("Review an Claude schicken", "Send review to Claude").into(), "", None));
    }
    let bar = container(line).width(Fill).style(|_| style::fill(RAISED));
    let edge = || container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE_STRONG));

    let mut out = column![edge()];
    if let (true, Some((label, message))) = (brain.review_preview, shown) {
        let head = row![
            text(label).size(12).color(TEXT).font(style::semibold()),
            text(t("Brain tippt sie als eine Nachricht in die Session.", "Brain hands it to the session as one message.")).size(12).color(TEXT_MUTED).font(UI),
            Space::new().width(Fill),
            style::text_button(t("Kopieren", "Copy"), false, Message::ReviewCopyMessage),
        ]
        .spacing(10)
        .align_y(iced::Center);
        let body = container(scrollable(text(message).size(12).color(TEXT).font(MONO)).style(scrollbar).width(Fill))
            .padding([10, 12])
            .width(Fill)
            .max_height(170)
            .style(|_| boxed(INK, LINE, 8.0));
        out = out.push(container(column![head, body].spacing(8)).padding(Padding { top: 10.0, right: 16.0, bottom: 12.0, left: 16.0 }).width(Fill).style(|_| style::fill(CHROME)));
    }
    out.push(bar).into()
}
