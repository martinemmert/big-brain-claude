//! The Konsil tab: a council of the konsil skill as a stage. The lazy senior faces his buddy,
//! the topic between them; below, where they stand, then the rounds with what each one said.

use std::f32::consts::FRAC_PI_2;

use brain_core::konsil::{Council, Figure, Round, Statement};
use iced::widget::{column, container, mouse_area, responsive, row, text, Space};
use iced::{font, gradient, Background, Border, Color, Element, Fill, Font, Length, Padding, Radians};

use crate::app::{Brain, Message};
use crate::format::{clip, clock};
use crate::i18n::t;
use crate::markdown;
use crate::style::{self, alpha, boxed, CALLS, CALLS_SOFT, DONE, LINE, SURFACE, TEXT, TEXT_FAINT, TEXT_MUTED, TEXT_STRONG, TURN, UI, WORKING};
use crate::tr;

/// The narrator's voice: a serif in italics, like stage directions.
const SCENE: Font = Font { style: font::Style::Italic, ..Font::with_name("Georgia") };
/// Below this width the two statements of a round stand below each other.
const SIDE_BY_SIDE: f32 = 640.0;

fn color(figure: Figure) -> Color {
    match figure {
        Figure::Lazy => TURN,
        Figure::Buddy => WORKING,
    }
}

fn emoji(figure: Figure) -> &'static str {
    match figure {
        Figure::Lazy => "🦥",
        Figure::Buddy => "🚀",
    }
}

fn name(figure: Figure) -> &'static str {
    match figure {
        Figure::Lazy => t("Der Faule", "The lazy senior"),
        Figure::Buddy => t("Der Kumpel", "The buddy"),
    }
}

/// The open council of the selected session, or why there is none.
pub fn view<'a>(brain: &'a Brain, session_key: &brain_core::state::SessionKey) -> Element<'a, Message> {
    let Some(cache) = brain.konsil.as_ref().filter(|k| &k.key == session_key) else {
        return style::hint(t("Lade die Beratungen …", "Loading the councils …"));
    };
    if cache.councils.is_empty() {
        return style::hint(t(
            "In dieser Session tagte noch kein Konsil.",
            "No council sat in this session yet.",
        ));
    }
    let picked = brain.konsil_pick.filter(|&i| i < cache.councils.len()).unwrap_or(cache.councils.len() - 1);
    let council = &cache.councils[picked];
    let mut page = column![].spacing(14).padding(Padding { top: 6.0, right: 12.0, bottom: 28.0, left: 0.0 });
    if cache.councils.len() > 1 {
        page = page.push(picker(&cache.councils, picked));
    }
    page = page.push(stage(council)).push(stand(council)).push(rounds(brain, council)).push(ending(council));
    page.into()
}

/// One small card per council, newest first.
fn picker<'a>(councils: &[Council], picked: usize) -> Element<'a, Message> {
    let cards = councils.iter().enumerate().rev().map(|(i, council)| {
        let (dot, _) = status(council);
        let when = council.started.map(|ts| clock(ts.timestamp_millis())).unwrap_or_default();
        let active = i == picked;
        let card = container(
            row![style::dot::<Message>(dot, 7.0), text(clip(&council.topic, 42)).size(12).color(if active { TEXT_STRONG } else { TEXT_MUTED }).font(UI), text(when).size(11).color(TEXT_FAINT).font(UI)]
                .spacing(7)
                .align_y(iced::Center),
        )
        .padding([5, 10])
        .style(move |_| boxed(SURFACE, if active { TURN } else { LINE }, 6.0));
        mouse_area(card).on_press(Message::KonsilPick(i)).interaction(iced::mouse::Interaction::Pointer).into()
    });
    row(cards).spacing(8).wrap().into()
}

/// The council's state as a colour and a word.
fn status(council: &Council) -> (Color, &'static str) {
    if council.decision.is_some() {
        (DONE, t("entschieden", "decided"))
    } else if !council.writing.is_empty() {
        (TURN, t("tagt", "in session"))
    } else {
        (CALLS, t("wartet auf dich", "waits for you"))
    }
}

/// The head: the two figures facing each other, the topic and the latest scene between them.
fn stage<'a>(council: &Council) -> Element<'a, Message> {
    let figure = |figure: Figure| -> Element<'a, Message> {
        let line: Element<'a, Message> = if council.writing.contains(&figure) {
            text(t("schreibt …", "is writing …")).size(11.5).color(color(figure)).font(SCENE).into()
        } else {
            let said = council.last_word(figure).map(first_sentence).unwrap_or_default();
            text(if said.is_empty() { String::new() } else { format!("„{}“", clip(&said, 90)) }).size(11.5).color(TEXT_MUTED).font(UI).align_x(iced::Center).into()
        };
        column![
            text(emoji(figure)).size(30),
            text(name(figure).to_uppercase()).size(11).color(color(figure)).font(style::semibold()),
            line,
        ]
        .spacing(4)
        .align_x(iced::Center)
        .width(Length::FillPortion(2))
        .into()
    };
    let rounds = council.rounds.iter().filter(|r| !r.statements.is_empty()).count();
    let (state_color, state) = status(council);
    let mut middle = column![
        row![
            text(tr!("KONSIL · {rounds} RUNDEN", "COUNCIL · {rounds} ROUNDS")).size(10.5).color(TEXT_FAINT).font(style::medium()),
            text("·").size(10.5).color(TEXT_FAINT),
            text(state.to_uppercase()).size(10.5).color(state_color).font(style::medium()),
        ]
        .spacing(6),
        text(council.topic.clone()).size(16).color(TEXT_STRONG).font(style::semibold()).align_x(iced::Center),
    ]
    .spacing(6)
    .align_x(iced::Center)
    .width(Length::FillPortion(3));
    if let Some(scene) = &council.scene {
        middle = middle.push(text(scene.clone()).size(12.5).color(TEXT_MUTED).font(SCENE).align_x(iced::Center));
    }
    let background = gradient::Linear::new(Radians(FRAC_PI_2))
        .add_stop(0.0, alpha(TURN, 0x2e))
        .add_stop(0.35, SURFACE)
        .add_stop(0.65, SURFACE)
        .add_stop(1.0, alpha(WORKING, 0x2e));
    container(row![figure(Figure::Lazy), middle, figure(Figure::Buddy)].spacing(12).align_y(iced::Center))
        .padding([18, 16])
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(Background::Gradient(background.into())),
            border: Border { color: LINE, width: 1.0, radius: 12.0.into() },
            ..container::Style::default()
        })
        .into()
}

/// Where the two stand: what they agree on, what they still fight about.
fn stand<'a>(council: &Council) -> Element<'a, Message> {
    let Some(stand) = council.stand() else {
        let waiting = t("Die beiden sitzen noch über dem Code …", "The two are still bent over the code …");
        return container(text(waiting).size(12.5).color(TEXT_MUTED).font(SCENE)).padding([10, 12]).width(Fill).center_x(Fill).style(|_| boxed(SURFACE, LINE, 8.0)).into();
    };
    let block = |title: &'static str, tint: Color, title_color: Color, points: &[String], empty: &'static str| -> Element<'a, Message> {
        let body: Element<'a, Message> = if points.is_empty() {
            text(empty).size(12.5).color(TEXT_FAINT).font(UI).into()
        } else {
            column(points.iter().map(|p| text(format!("• {}", with_figures(p))).size(12.5).color(TEXT).font(UI).line_height(1.4).into())).spacing(3).into()
        };
        container(column![text(title).size(10.5).color(title_color).font(style::semibold()), body].spacing(5))
            .padding([10, 12])
            .width(Fill)
            .height(Length::Shrink)
            .style(move |_| boxed(alpha(tint, 0x16), alpha(tint, 0x55), 8.0))
            .into()
    };
    row![
        block(t("EINIG", "AGREED"), DONE, DONE, &stand.agreed, t("noch nichts", "nothing yet")),
        block(t("STRITTIG", "DISPUTED"), CALLS, CALLS_SOFT, &stand.disputed, t("nichts mehr", "nothing left")),
    ]
    .spacing(10)
    .into()
}

/// `(faul: ja, kumpel: nein)` → `(🦥 ja, 🚀 nein)`.
fn with_figures(point: &str) -> String {
    let mut out = point.to_string();
    for (word, figure) in [("faul:", Figure::Lazy), ("lazy:", Figure::Lazy), ("kumpel:", Figure::Buddy), ("buddy:", Figure::Buddy)] {
        while let Some(at) = out.char_indices().map(|(i, _)| i).find(|&i| out.get(i..i + word.len()).is_some_and(|w| w.eq_ignore_ascii_case(word))) {
            out.replace_range(at..at + word.len(), emoji(figure));
        }
    }
    out
}

/// The rounds, the newest unfolded; a click on a round's title folds or unfolds it.
fn rounds<'a>(brain: &'a Brain, council: &Council) -> Element<'a, Message> {
    let last = council.rounds.len().saturating_sub(1);
    let mut list = column![row![
        container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)),
        text(t("RUNDEN", "ROUNDS")).size(10.5).color(TEXT_FAINT).font(style::medium()),
        container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)),
    ]
    .spacing(10)
    .align_y(iced::Center)]
    .spacing(8);
    let undecided = council.decision.is_none();
    let writing_in = |i: usize| -> &[Figure] { if i == last && undecided { &council.writing } else { &[] } };
    // Words to the table that nobody answered: after a decision, they were the decision.
    let shown = |i: usize, round: &Round| !round.statements.is_empty() || !writing_in(i).is_empty() || (!round.prompts.is_empty() && undecided);
    let newest = council.rounds.iter().enumerate().rev().find(|(i, r)| shown(*i, r)).map(|(i, _)| i);
    for (i, round) in council.rounds.iter().enumerate() {
        if !shown(i, round) {
            continue;
        }
        let writing = writing_in(i);
        let open = (Some(i) == newest) != brain.konsil_toggled.contains(&i);
        list = list.push(round_title(i, round, open));
        if open {
            list = list.push(container(round_body(round, writing)).padding(Padding { left: 14.0, ..Padding::ZERO }));
        }
    }
    if council.rounds.is_empty() && !council.writing.is_empty() {
        list = list.push(round_body(&Round::default(), &council.writing));
    }
    list.into()
}

fn round_title<'a>(index: usize, round: &Round, open: bool) -> Element<'a, Message> {
    let number = index + 1;
    let title = if index == 0 { t("Eröffnung", "Opening").to_string() } else { tr!("Runde {number}", "Round {number}") };
    let mut speakers: Vec<Figure> = Vec::new();
    for statement in &round.statements {
        if !speakers.contains(&statement.figure) {
            speakers.push(statement.figure);
        }
    }
    let gist = round
        .prompts
        .last()
        .map(|prompt| {
            let prompt = clip(prompt, 60);
            tr!("deine Frage: {prompt}", "your question: {prompt}")
        })
        .or_else(|| round.statements.first().map(|s| clip(&first_sentence(&s.text), 70))).unwrap_or_default();
    let line = row![
        text(if open { "▾" } else { "▸" }).size(12).color(TEXT_MUTED),
        text(title).size(13).color(TEXT_STRONG).font(style::semibold()),
        text(gist).size(12).color(TEXT_FAINT).font(UI).wrapping(text::Wrapping::None).width(Fill),
        text(speakers.iter().map(|f| emoji(*f)).collect::<Vec<_>>().join(" ")).size(12),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let card = container(line).padding([8, 12]).width(Fill).clip(true).style(|_| boxed(SURFACE, LINE, 8.0));
    mouse_area(card).on_press(Message::KonsilRound(index)).interaction(iced::mouse::Interaction::Pointer).into()
}

/// The user's words first, then the statements: a pair side by side, anything else stacked.
fn round_body<'a>(round: &Round, writing: &[Figure]) -> Element<'a, Message> {
    let mut body = column![].spacing(8);
    for prompt in &round.prompts {
        body = body.push(
            container(column![text(t("DU", "YOU")).size(10.5).color(DONE).font(style::semibold()), text(prompt.clone()).size(12.5).color(TEXT).font(UI).line_height(1.4)].spacing(4))
                .padding([8, 12])
                .width(Fill)
                .style(|_| boxed(alpha(DONE, 0x12), alpha(DONE, 0x55), 8.0)),
        );
    }
    let mut cards: Vec<(Figure, Option<String>)> = round.statements.iter().map(|Statement { figure, text }| (*figure, Some(text.clone()))).collect();
    cards.extend(writing.iter().map(|f| (*f, None)));
    let pair = cards.len() == 2 && cards[0].0 != cards[1].0;
    if pair {
        let cards = cards.clone();
        body = body.push(responsive(move |size| {
            let left = card(cards[0].0, cards[0].1.as_deref());
            let right = card(cards[1].0, cards[1].1.as_deref());
            if size.width >= SIDE_BY_SIDE {
                row![left, right].spacing(10).into()
            } else {
                column![left, right].spacing(8).into()
            }
        }).height(Length::Shrink));
    } else {
        for (figure, said) in &cards {
            body = body.push(card(*figure, said.as_deref()));
        }
    }
    body.into()
}

/// A statement under its speaker, with a frame in the speaker's colour; `None` while
/// the figure is still writing.
fn card<'a>(figure: Figure, said: Option<&str>) -> Element<'a, Message> {
    let head = row![
        container(text(emoji(figure)).size(13)).padding([2, 5]).style(move |_| boxed(alpha(color(figure), 0x26), Color::TRANSPARENT, 11.0)),
        text(name(figure).to_uppercase()).size(11).color(color(figure)).font(style::semibold()),
    ]
    .spacing(7)
    .align_y(iced::Center);
    let body: Element<'a, Message> = match said {
        Some(said) => markdown::prose(said, 13.0),
        None => text(t("schreibt …", "is writing …")).size(12.5).color(TEXT_FAINT).font(SCENE).into(),
    };
    container(column![head, body].spacing(8))
        .padding([10, 14])
        .width(Fill)
        .style(move |_| boxed(SURFACE, alpha(color(figure), 0x70), 8.0))
        .into()
}

/// The decision, or the narrator's question to the user.
fn ending<'a>(council: &Council) -> Element<'a, Message> {
    if let Some((decision, ts)) = &council.decision {
        let when = ts.map(|ts| clock(ts.timestamp_millis())).unwrap_or_default();
        return container(
            row![
                text(t("Entschieden:", "Decided:")).size(13).color(DONE).font(style::semibold()),
                text(decision.clone()).size(13).color(TEXT_STRONG).font(UI).width(Fill),
                text(when).size(11).color(TEXT_FAINT).font(UI),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding([10, 14])
        .width(Fill)
        .style(|_| boxed(alpha(DONE, 0x1a), alpha(DONE, 0x66), 8.0))
        .into();
    }
    if !council.writing.is_empty() {
        return Space::new().height(0).into();
    }
    container(text(t("Was tust du?", "What do you do?")).size(19).color(TEXT_STRONG).font(SCENE)).width(Fill).center_x(Fill).padding([8, 0]).into()
}

/// The first sentence of a statement, without Markdown markers.
fn first_sentence(said: &str) -> String {
    let first = said.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let plain: String = first.trim_start_matches(['-', '*', '#', '>', ' ']).replace("**", "").replace('`', "");
    let end = plain.char_indices().find(|&(i, c)| matches!(c, '.' | '!' | '?') && plain[i + c.len_utf8()..].starts_with(' ')).map(|(i, c)| i + c.len_utf8());
    match end {
        Some(end) => plain[..end].to_string(),
        None => plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figures_replace_their_names_in_a_disputed_point() {
        assert_eq!(with_figures("Nachschicken? (faul: ja, Kumpel: nein)"), "Nachschicken? (🦥 ja, 🚀 nein)");
        assert_eq!(with_figures("Größe: egal"), "Größe: egal");
    }

    #[test]
    fn the_first_sentence_without_markers() {
        assert_eq!(first_sentence("Kurz: Weder noch. Ihr wollt"), "Kurz: Weder noch.");
        assert_eq!(first_sentence("Weder noch, zumindest heute nicht. Zweiter Kernel?"), "Weder noch, zumindest heute nicht.");
        assert_eq!(first_sentence("- **Streichen** das"), "Streichen das");
    }
}
