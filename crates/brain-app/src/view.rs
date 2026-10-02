use std::time::{Duration, Instant};

use brain_core::event::{Event, Kind, Source};
use brain_core::state::{Phase, Session, SessionKey};
use chrono::{Local, TimeZone, Utc};
use gpui::{
    div, prelude::*, pulsating_between, px, relative, AnyElement, Animation, AnimationExt as _,
    ClickEvent, Context, ElementId, FocusHandle, FontWeight, KeyDownEvent, Rgba, ScrollHandle,
    SharedString, Task, Window,
};

use crate::model::Model;
use crate::system::{self, JumpResult};
use crate::theme;

/// Sessions on "your turn" for longer than this move to the "Ruhend" section.
const RESTING_AFTER_MS: i64 = 2 * 60 * 60 * 1000;
/// Ended sessions stay visible this long.
const ENDED_VISIBLE_MS: i64 = 12 * 60 * 60 * 1000;

pub struct BrainView {
    model: Model,
    focus: FocusHandle,
    /// `None` shows every account.
    filter: Option<String>,
    selected: Option<SessionKey>,
    show_ended: bool,
    status: Option<(String, Instant)>,
    list_scroll: ScrollHandle,
    _poll: Task<()>,
}

/// The left column, already filtered and sorted.
struct Groups<'a> {
    attention: Vec<&'a Session>,
    working: Vec<&'a Session>,
    resting: Vec<&'a Session>,
    ended: Vec<&'a Session>,
}

impl<'a> Groups<'a> {
    fn navigable(&self, include_ended: bool) -> Vec<&'a Session> {
        let mut all: Vec<&Session> = Vec::new();
        all.extend(&self.attention);
        all.extend(&self.working);
        all.extend(&self.resting);
        if include_ended {
            all.extend(&self.ended);
        }
        all
    }
}

impl BrainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut model = Model::load();
        model.refresh();

        let focus = cx.focus_handle();
        focus.focus(window);

        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(800)).await;
            if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                break;
            }
        });

        let mut view = Self {
            model,
            focus,
            filter: None,
            selected: None,
            show_ended: false,
            status: None,
            list_scroll: ScrollHandle::new(),
            _poll: poll,
        };
        view.selected = view.groups(now_ms()).navigable(false).first().map(|s| s.key.clone());
        view
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        for item in self.model.refresh() {
            let subtitle = match item.phase {
                Phase::NeedsYou => format!("{} · braucht dich", item.account),
                _ => format!("{} · fertig, du bist dran", item.account),
            };
            system::notify(&item.name, &subtitle, item.headline.as_deref().unwrap_or(""));
        }
        if self.status.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(6)) {
            self.status = None;
        }
        cx.notify();
    }

    fn groups(&self, now: i64) -> Groups<'_> {
        let mut groups = Groups { attention: vec![], working: vec![], resting: vec![], ended: vec![] };
        let visible = self
            .model
            .board
            .sorted()
            .into_iter()
            .filter(|s| self.filter.as_ref().is_none_or(|f| *f == s.key.account));
        for session in visible {
            match session.phase() {
                Phase::NeedsYou => groups.attention.push(session),
                Phase::YourTurn if now - session.phase_since_ms() > RESTING_AFTER_MS => {
                    groups.resting.push(session)
                }
                Phase::YourTurn => groups.attention.push(session),
                Phase::Working => groups.working.push(session),
                Phase::Ended if now - session.last_activity_ms < ENDED_VISIBLE_MS => {
                    groups.ended.push(session)
                }
                Phase::Ended => {}
            }
        }
        // Resting: most recently finished first.
        groups.resting.reverse();
        groups
    }

    fn account_index(&self, account: &str) -> usize {
        self.model.accounts.iter().position(|a| a.id == account).unwrap_or(0)
    }

    // ---- input -------------------------------------------------------------

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.platform || keystroke.modifiers.control {
            return;
        }
        let list: Vec<SessionKey> = self
            .groups(now_ms())
            .navigable(self.show_ended)
            .iter()
            .map(|s| s.key.clone())
            .collect();
        let current = self.selected.as_ref().and_then(|k| list.iter().position(|l| l == k));

        match keystroke.key.as_str() {
            "down" | "j" => self.select_index(&list, current.map_or(0, |i| (i + 1).min(list.len().saturating_sub(1)))),
            "up" | "k" => self.select_index(&list, current.map_or(0, |i| i.saturating_sub(1))),
            "enter" => self.jump_selected(),
            "tab" => self.cycle_filter(keystroke.modifiers.shift),
            "e" => self.show_ended = !self.show_ended,
            key if key.len() == 1 && ('1'..='9').contains(&key.chars().next().unwrap()) => {
                let index = key.parse::<usize>().unwrap() - 1;
                if index < list.len() {
                    self.select_index(&list, index);
                    self.jump_selected();
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn select_index(&mut self, list: &[SessionKey], index: usize) {
        if let Some(key) = list.get(index) {
            self.selected = Some(key.clone());
            self.list_scroll.scroll_to_item(index);
        }
    }

    fn cycle_filter(&mut self, backwards: bool) {
        let mut options: Vec<Option<String>> = vec![None];
        options.extend(self.model.accounts.iter().map(|a| Some(a.id.clone())));
        let at = options.iter().position(|o| *o == self.filter).unwrap_or(0);
        let next = if backwards { (at + options.len() - 1) % options.len() } else { (at + 1) % options.len() };
        self.filter = options[next].clone();
    }

    fn jump_selected(&mut self) {
        let Some(key) = self.selected.clone() else { return };
        let message = match system::jump_to_iterm(key.pid) {
            JumpResult::Focused => return,
            JumpResult::NoTerminal => format!("pid {} hat kein Terminal (beendet oder SDK-Session)", key.pid),
            JumpResult::NotFound(reason) => format!("Sprung fehlgeschlagen: {reason}"),
        };
        self.status = Some((message, Instant::now()));
    }

    // ---- rendering -------------------------------------------------------------

    fn render_titlebar(&self, groups: &Groups, cx: &mut Context<Self>) -> impl IntoElement {
        let total = groups.attention.len() + groups.working.len() + groups.resting.len();
        let waiting = groups.attention.len();

        let mut segments = vec![(None, SharedString::from("Alle"))];
        segments.extend(self.model.accounts.iter().map(|a| (Some(a.id.clone()), SharedString::from(a.id.clone()))));

        div()
            .id("titlebar")
            .flex()
            .flex_none()
            .items_center()
            .h(px(46.))
            .pl(px(88.))
            .pr(px(14.))
            .gap_3()
            .bg(theme::chrome_bg())
            .border_b_1()
            .border_color(theme::border())
            .on_click(|event: &ClickEvent, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(dot(if waiting > 0 { theme::red() } else { theme::green() }, waiting > 0, "brand-dot"))
                    .child(div().font_weight(FontWeight::BOLD).text_color(theme::text_strong()).child("Brain")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("{total} Sessions · {waiting} brauchen dich")),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .p(px(2.))
                    .rounded(px(7.))
                    .bg(gpui::rgb(0x1a1e27))
                    .children(segments.into_iter().enumerate().map(|(ix, (value, label))| {
                        let active = value == self.filter;
                        div()
                            .id(("segment", ix))
                            .px(px(11.))
                            .py(px(3.))
                            .rounded(px(5.))
                            .text_xs()
                            .cursor_pointer()
                            .text_color(if active { theme::text_strong() } else { theme::text_muted() })
                            .when(active, |d| d.bg(gpui::rgb(0x2a3040)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.filter = value.clone();
                                cx.notify();
                            }))
                            .child(label)
                    })),
            )
    }

    fn render_list(&self, groups: &Groups, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let mut children: Vec<AnyElement> = Vec::new();
        let mut nav_index = 0usize;

        children.push(section_header("Braucht dich", groups.attention.len(), true));
        if groups.attention.is_empty() {
            children.push(
                div()
                    .px_3()
                    .py_4()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .child("Niemand wartet auf dich. ✨")
                    .into_any_element(),
            );
        }
        for session in &groups.attention {
            children.push(self.render_card(session, nav_index, now, cx).into_any_element());
            nav_index += 1;
        }

        let compact_sections = [
            ("Arbeitet", &groups.working),
            ("Ruhend", &groups.resting),
        ];
        for (title, sessions) in compact_sections {
            if sessions.is_empty() {
                continue;
            }
            children.push(section_header(title, sessions.len(), false));
            for session in sessions.iter() {
                children.push(self.render_row(session, nav_index, now, cx).into_any_element());
                nav_index += 1;
            }
        }

        if !groups.ended.is_empty() {
            let arrow = if self.show_ended { "▾" } else { "▸" };
            children.push(
                div()
                    .id("ended-toggle")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.show_ended = !this.show_ended;
                        cx.notify();
                    }))
                    .child(section_header(&format!("Beendet {arrow}"), groups.ended.len(), false))
                    .into_any_element(),
            );
            if self.show_ended {
                for session in &groups.ended {
                    children.push(self.render_row(session, nav_index, now, cx).into_any_element());
                    nav_index += 1;
                }
            }
        }

        div()
            .id("session-list")
            .flex()
            .flex_col()
            .w(relative(0.42))
            .min_w(px(340.))
            .h_full()
            .p_3()
            .gap(px(6.))
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .border_r_1()
            .border_color(theme::border())
            .children(children)
    }

    fn render_card(&self, s: &Session, nav_index: usize, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.as_ref() == Some(&s.key);
        let phase = s.phase();
        let (accent, edge) = match phase {
            Phase::NeedsYou => (theme::red(), theme::red_edge()),
            _ => (theme::amber(), theme::amber_edge()),
        };
        let headline = s.headline().unwrap_or_else(|| match phase {
            Phase::NeedsYou => "Wartet auf Eingabe".into(),
            _ => "Fertig – du bist dran".into(),
        });

        div()
            .id(("card", nav_index))
            .flex()
            .flex_col()
            .gap(px(6.))
            .px_3()
            .py(px(10.))
            .rounded(px(9.))
            .border_1()
            .cursor_pointer()
            .bg(if selected { theme::card_selected_bg() } else { theme::card_bg() })
            .border_color(if selected { edge } else { theme::card_border() })
            .when(selected, |d| d.shadow_md())
            .on_click({
                let key = s.key.clone();
                cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.selected = Some(key.clone());
                    if event.click_count() >= 2 {
                        this.jump_selected();
                    }
                    cx.notify();
                })
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(dot(accent, phase == Phase::NeedsYou, ("card-dot", nav_index)))
                    .child(name_label(s))
                    .child(self.account_badge(&s.key.account))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(format!("· {}", theme::ago(s.phase_since_ms(), now))),
                    )
                    .child(div().flex_1())
                    .when(nav_index < 9, |d| d.child(kbd(format!("{}", nav_index + 1)))),
            )
            .child(
                div()
                    .pl(px(16.))
                    .text_sm()
                    .line_clamp(2)
                    .text_color(if phase == Phase::NeedsYou { theme::text_strong() } else { theme::text() })
                    .child(if s.headline_is_reported() && phase == Phase::NeedsYou {
                        format!("„{headline}“")
                    } else {
                        headline
                    }),
            )
    }

    fn render_row(&self, s: &Session, nav_index: usize, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.as_ref() == Some(&s.key);
        let phase = s.phase();
        let color = match phase {
            Phase::Working => theme::blue(),
            Phase::YourTurn => theme::amber(),
            Phase::NeedsYou => theme::red(),
            Phase::Ended => theme::grey(),
        };
        let detail = match phase {
            Phase::Working => s.headline().unwrap_or_else(|| "arbeitet…".into()),
            Phase::YourTurn => format!("seit {}", theme::ago(s.phase_since_ms(), now)),
            _ => format!("vor {}", theme::ago(s.last_activity_ms, now)),
        };

        div()
            .id(("row", nav_index))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py(px(7.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme::card_selected_bg()))
            .when(!selected, |d| d.hover(|d| d.bg(theme::row_hover_bg())))
            .when(phase == Phase::Ended, |d| d.opacity(0.55))
            .on_click({
                let key = s.key.clone();
                cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.selected = Some(key.clone());
                    if event.click_count() >= 2 {
                        this.jump_selected();
                    }
                    cx.notify();
                })
            })
            .child(dot(color, false, ("row-dot", nav_index)))
            .child(name_label(s))
            .child(self.account_badge(&s.key.account))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .truncate()
                    .child(detail),
            )
    }

    fn account_badge(&self, account: &str) -> impl IntoElement {
        let (bg, fg) = theme::account_colors(self.account_index(account));
        div()
            .flex_none()
            .px(px(7.))
            .rounded(px(6.))
            .bg(bg)
            .text_color(fg)
            .text_size(px(10.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(account.to_string())
    }

    fn render_detail(&self, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let session = self.selected.as_ref().and_then(|k| self.model.board.get(k));
        let Some(s) = session else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::panel_bg())
                .text_color(theme::text_muted())
                .child("Wähle links eine Session aus.")
                .into_any_element();
        };
        let phase = s.phase();

        let (label, accent, tint, edge) = match phase {
            Phase::NeedsYou => ("Wartet auf dich", theme::red_soft(), theme::red_tint(), theme::red_edge()),
            Phase::YourTurn => ("Fertig · du bist dran", theme::amber(), gpui::rgba(0xf5b94214), theme::amber_edge()),
            Phase::Working => ("Arbeitet gerade", theme::blue(), gpui::rgba(0x4da3ff14), gpui::rgba(0x4da3ff44)),
            Phase::Ended => ("Beendet", theme::text_muted(), gpui::rgba(0xffffff08), theme::card_border()),
        };
        let headline = s.headline().unwrap_or_else(|| "Keine Meldung – Zustand kommt direkt aus Claude Code.".into());

        let mut meta = vec![s.cwd.as_deref().map(theme::tilde).unwrap_or_default(), format!("pid {}", s.key.pid)];
        if let Some(started) = s.started_ms {
            meta.push(format!("seit {}", clock(started)));
        }

        div()
            .id("detail")
            .flex_1()
            .flex()
            .flex_col()
            .h_full()
            .px(px(22.))
            .py(px(18.))
            .bg(theme::panel_bg())
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(dot(accent_color(phase), phase == Phase::NeedsYou, "detail-dot"))
                    .child(
                        div()
                            .text_size(px(19.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_strong())
                            .child(s.display_name()),
                    )
                    .child(self.account_badge(&s.key.account))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("jump")
                            .flex()
                            .items_center()
                            .gap_2()
                            .px(px(12.))
                            .py(px(6.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_strong())
                            .bg(if phase == Phase::NeedsYou { theme::red() } else { gpui::rgb(0x2a3040) })
                            .hover(|d| d.opacity(0.85))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.jump_selected();
                                cx.notify();
                            }))
                            .child("↗ Zu iTerm springen")
                            .child(div().text_xs().opacity(0.7).child("⏎")),
                    ),
            )
            .child(
                div()
                    .mt(px(4.))
                    .mb(px(18.))
                    .font_family("Menlo")
                    .text_size(px(11.5))
                    .text_color(theme::text_faint())
                    .child(meta.join(" · ")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .mb(px(22.))
                    .px(px(14.))
                    .py(px(12.))
                    .rounded(px(9.))
                    .bg(tint)
                    .border_1()
                    .border_color(edge)
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(accent)
                            .child(format!("{} · seit {}", label.to_uppercase(), theme::ago(s.phase_since_ms(), now))),
                    )
                    .child(div().text_color(theme::text_strong()).line_height(relative(1.4)).child(headline)),
            )
            .child(section_header("Verlauf · neueste zuerst", s.timeline.len(), false))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .ml(px(4.))
                    .pl(px(16.))
                    .border_l_2()
                    .border_color(gpui::rgb(0x232836))
                    .when(s.timeline.is_empty(), |d| {
                        d.child(
                            div()
                                .py_2()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Noch keine Ereignisse. Sie erscheinen, sobald die Hooks aktiv sind (brain install)."),
                        )
                    })
                    .children(s.timeline.iter().rev().take(80).map(timeline_entry)),
            )
            .into_any_element()
    }

    fn render_footer(&self) -> impl IntoElement {
        let hints = ["↑↓ wählen", "⏎ springen", "1–9 direkt", "⇥ Konto", "E beendete"];
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_4()
            .h(px(30.))
            .px(px(14.))
            .bg(theme::chrome_bg())
            .border_t_1()
            .border_color(theme::border())
            .text_xs()
            .text_color(theme::text_faint())
            .children(hints.map(|h| div().child(h)))
            .child(div().flex_1())
            .when_some(self.status.as_ref(), |d, (message, _)| {
                d.child(div().text_color(theme::red_soft()).child(message.clone()))
            })
    }
}

impl Render for BrainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = now_ms();

        // Keep the selection on a visible session.
        let visible: Vec<SessionKey> = self.groups(now).navigable(self.show_ended).iter().map(|s| s.key.clone()).collect();
        if self.selected.as_ref().is_none_or(|k| !visible.contains(k)) {
            self.selected = visible.first().cloned();
        }

        let groups = self.groups(now);
        let waiting = groups.attention.len();
        window.set_window_title(&if waiting > 0 { format!("Brain — {waiting} brauchen dich") } else { "Brain".into() });

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::window_bg())
            .font_family(".SystemUIFont")
            .text_color(theme::text())
            .text_size(px(13.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(self.render_titlebar(&groups, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_list(&groups, now, cx))
                    .child(self.render_detail(now, cx)),
            )
            .child(self.render_footer())
    }
}

// ---- small building blocks ---------------------------------------------------------

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn clock(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

fn accent_color(phase: Phase) -> Rgba {
    match phase {
        Phase::NeedsYou => theme::red(),
        Phase::YourTurn => theme::amber(),
        Phase::Working => theme::blue(),
        Phase::Ended => theme::grey(),
    }
}

fn dot(color: Rgba, pulse: bool, id: impl Into<ElementId>) -> AnyElement {
    let dot = div().flex_none().size(px(8.)).rounded_full().bg(color);
    if pulse {
        dot.with_animation(
            id,
            Animation::new(Duration::from_millis(1800)).repeat().with_easing(pulsating_between(0.35, 1.0)),
            |dot, delta| dot.opacity(delta),
        )
        .into_any_element()
    } else {
        dot.into_any_element()
    }
}

fn name_label(s: &Session) -> impl IntoElement {
    div()
        .flex_none()
        .max_w(px(220.))
        .truncate()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_strong())
        .child(s.display_name())
}

fn kbd(label: String) -> impl IntoElement {
    div()
        .px(px(5.))
        .rounded(px(4.))
        .border_1()
        .border_color(gpui::rgb(0x2a3040))
        .text_size(px(10.))
        .text_color(theme::text_faint())
        .child(label)
}

fn section_header(title: &str, count: usize, alert: bool) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .mt(px(10.))
        .mb(px(2.))
        .px_1()
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(title.to_uppercase())
        .child(if alert && count > 0 {
            div()
                .px(px(6.))
                .rounded(px(8.))
                .bg(theme::red())
                .text_color(theme::text_strong())
                .font_weight(FontWeight::BOLD)
                .child(count.to_string())
        } else {
            div().text_color(theme::text_faint()).child(count.to_string())
        })
        .into_any_element()
}

fn timeline_entry(event: &Event) -> impl IntoElement {
    let text = event.text.clone().unwrap_or_default();
    let (color, label): (Rgba, String) = match event.kind {
        Kind::SessionStart => (theme::grey(), "Session gestartet".into()),
        Kind::SessionEnd => (theme::grey(), "Session beendet".into()),
        Kind::Prompt => (gpui::rgb(0x8a93a6), if text.is_empty() { "Neuer Prompt".into() } else { format!("Du: {text}") }),
        Kind::Permission => (theme::red(), if text.is_empty() { "Braucht Freigabe".into() } else { text }),
        Kind::Stop => (theme::amber(), if text.is_empty() { "Turn beendet".into() } else { format!("Antwort: {text}") }),
        Kind::Doing => (theme::blue(), text),
        Kind::Waiting => (theme::red(), format!("Wartet: {text}")),
        Kind::Done => (theme::green(), format!("Fertig: {text}")),
    };
    let tag = match event.source {
        Source::Hook => "hook",
        Source::Report => "report",
    };
    let time = event.ts.with_timezone(&Local).format("%H:%M").to_string();

    div()
        .relative()
        .flex()
        .items_start()
        .gap_2()
        .py(px(5.))
        .child(
            div()
                .absolute()
                .left(px(-21.))
                .top(px(10.))
                .size(px(8.))
                .rounded_full()
                .bg(color),
        )
        .child(
            div()
                .flex_none()
                .w(px(40.))
                .font_family("Menlo")
                .text_size(px(11.))
                .text_color(theme::text_faint())
                .pt(px(1.))
                .child(time),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .line_clamp(3)
                .text_color(if event.source == Source::Report { theme::text_strong() } else { theme::text() })
                .when(event.kind == Kind::Waiting, |d| d.text_color(theme::red_soft()))
                .child(label),
        )
        .child(
            div()
                .flex_none()
                .px(px(5.))
                .rounded(px(4.))
                .border_1()
                .border_color(gpui::rgb(0x2a3040))
                .text_size(px(9.5))
                .text_color(if event.source == Source::Report { theme::blue() } else { theme::text_faint() })
                .child(tag),
        )
}
