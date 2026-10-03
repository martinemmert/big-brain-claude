//! Small building blocks shared by the views.

use std::time::Duration;

use brain_core::state::Phase;
use chrono::{Local, TimeZone, Utc};
use gpui::{
    div, prelude::*, pulsating_between, px, AnyElement, Animation, AnimationExt as _, ElementId,
    FontWeight, Rgba,
};

use crate::i18n::t;
use crate::theme;

pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn clock(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// Drops Markdown emphasis and code markers for one-line previews, and words Claude Code's
/// notification texts (and Brain's own notes) in the UI's language.
pub fn plain(text: &str) -> String {
    let text = flatten_tables(&text.replace("**", "").replace('`', ""));
    if let Some(rest) = text.strip_prefix("Claude needs your permission") {
        let tool = rest.trim().strip_prefix("to use ").map(str::trim).filter(|t| !t.is_empty());
        return match tool {
            Some(tool) => crate::tr!("Braucht deine Freigabe für {tool}", "Needs your permission for {tool}"),
            None => t("Braucht deine Freigabe", "Needs your permission").into(),
        };
    }
    if text.starts_with("Claude is waiting for your input") {
        return t("Wartet auf deine Eingabe", "Waiting for your input").into();
    }
    // Harness prompts (subagent hand-backs, task notifications) are stored as `[note]`.
    if let Some(inner) = text.strip_prefix('[') {
        return note(inner.strip_suffix(']').unwrap_or(inner));
    }
    note(&text)
}

/// A Markdown table squashed into one line reads `| a | b | |---|---| | 1 | 2 |`: keep the cells,
/// drop the separator row, join with middle dots.
fn flatten_tables(text: &str) -> String {
    if !text.contains("|-") && !text.contains("| -") {
        return text.to_string();
    }
    text.split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty() && !cell.chars().all(|c| matches!(c, '-' | ':' | ' ')))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Brain's harness notes from `brain_core::transcript` in the UI language.
pub fn note(text: &str) -> String {
    use brain_core::transcript::{NOTE_AGENT_REPLY, NOTE_TASK, NOTE_TODOS};
    match text {
        NOTE_AGENT_REPLY => t("Rückmeldung eines Subagents", NOTE_AGENT_REPLY).into(),
        NOTE_TASK => t("Eine Hintergrund-Aufgabe meldet sich", NOTE_TASK).into(),
        NOTE_TODOS => t("Todo-Liste aktualisiert", NOTE_TODOS).into(),
        other => other.to_string(),
    }
}

pub fn phase_color(phase: Phase) -> Rgba {
    match phase {
        Phase::NeedsYou => theme::calls(),
        Phase::YourTurn => theme::turn(),
        Phase::Working => theme::working(),
        Phase::Background => theme::background(),
        Phase::Ended => theme::ended(),
    }
}

/// "2 subagents, 1 shell running" from a stop's background tasks.
pub fn background_summary(tasks: &[brain_core::event::BackgroundTask]) -> String {
    let count = |kind: &str| tasks.iter().filter(|t| t.kind == kind).count();
    let (agents, shells) = (count("subagent"), count("shell"));
    let other = tasks.len() - agents - shells;
    let mut parts = Vec::new();
    if agents > 0 {
        parts.push(if agents == 1 { t("1 Subagent", "1 subagent").to_string() } else { crate::tr!("{agents} Subagents", "{agents} subagents") });
    }
    if shells > 0 {
        parts.push(if shells == 1 { t("1 Shell", "1 shell").to_string() } else { crate::tr!("{shells} Shells", "{shells} shells") });
    }
    if other > 0 {
        parts.push(if other == 1 { t("1 Task", "1 task").to_string() } else { crate::tr!("{other} Tasks", "{other} tasks") });
    }
    let list = parts.join(", ");
    if tasks.len() == 1 { crate::tr!("{list} läuft", "{list} running") } else { crate::tr!("{list} laufen", "{list} running") }
}

pub fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::NeedsYou => t("Wartet auf dich", "Waiting for you"),
        Phase::YourTurn => t("Fertig, du bist dran", "Done, your turn"),
        Phase::Working => t("Arbeitet", "Working"),
        Phase::Background => t("Arbeitet im Hintergrund", "Working in the background"),
        Phase::Ended => t("Beendet", "Ended"),
    }
}

/// A status dot; `pulse` breathes it to draw the eye (used only for "calls you").
pub fn dot(color: Rgba, size: f32, pulse: bool, id: impl Into<ElementId>) -> AnyElement {
    let dot = div().flex_none().size(px(size)).rounded_full().bg(color);
    if !pulse {
        return dot.into_any_element();
    }
    div()
        .flex_none()
        .relative()
        .size(px(size))
        .child(
            div()
                .absolute()
                .top(px(-3.))
                .left(px(-3.))
                .size(px(size + 6.))
                .rounded_full()
                .bg(theme::alpha(color, 70))
                .with_animation(
                    id,
                    Animation::new(Duration::from_millis(1800)).repeat().with_easing(pulsating_between(0.0, 1.0)),
                    |halo, delta| halo.opacity(delta),
                ),
        )
        .child(dot.absolute().top_0().left_0())
        .into_any_element()
}

/// Blinking text cursor for Brain's line inputs.
pub fn caret(id: &'static str) -> AnyElement {
    div()
        .flex_none()
        .ml(px(1.))
        .w(px(1.5))
        .h(px(16.))
        .bg(theme::working())
        .with_animation(
            id,
            Animation::new(Duration::from_millis(1000)).repeat().with_easing(pulsating_between(0.0, 1.0)),
            |caret, delta| caret.opacity(delta),
        )
        .into_any_element()
}

/// A key cap like `⏎` or `/`.
pub fn kbd(label: impl Into<String>) -> impl IntoElement {
    div()
        .flex_none()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(theme::raised())
        .border_1()
        .border_color(theme::line_strong())
        .text_size(px(10.5))
        .text_color(theme::text_muted())
        .child(label.into())
}

/// The account name as a tinted badge.
pub fn account_badge(account: &str, index: usize) -> impl IntoElement {
    let (bg, fg) = theme::account_colors(index);
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(5.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .child(account.to_string())
}

/// Section title with its count, e.g. "Braucht dich 3". `alert` turns the count red.
pub fn section_title(title: &str, count: usize, alert: bool) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(7.))
        .mt(px(14.))
        .mb(px(4.))
        .px(px(4.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(title.to_string())
        .child(if alert && count > 0 {
            div()
                .px(px(6.))
                .rounded_full()
                .bg(theme::calls())
                .text_color(theme::text_strong())
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .child(count.to_string())
        } else {
            div().text_color(theme::text_faint()).font_weight(FontWeight::MEDIUM).child(count.to_string())
        })
        .into_any_element()
}

/// A small rounded chip for metadata (path, pid, start time).
pub fn chip(content: impl Into<String>, mono: bool) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(7.))
        .py(px(2.))
        .rounded(px(5.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::line())
        .text_size(px(11.5))
        .text_color(theme::text_muted())
        .when(mono, |d| d.font_family("Menlo").text_size(px(11.)))
        .child(content.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_in_one_line_previews_keep_their_cells() {
        assert_eq!(flatten_tables("Plan: | a | b | |---|---| | 1 | 2 |"), "Plan: · a · b · 1 · 2");
        assert_eq!(flatten_tables("no | table here"), "no | table here");
    }
}
