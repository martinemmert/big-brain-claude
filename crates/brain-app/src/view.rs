use std::time::{Duration, Instant};

use brain_core::state::{Phase, Session, SessionKey};
use brain_core::transcript::Role;
use gpui::{
    div, prelude::*, px, relative, AnyElement, BoxShadow, ClickEvent, Context, FocusHandle,
    FontWeight, KeyDownEvent, ScrollHandle, SharedString, Task, Window,
};

use crate::conversation::Conversation;
use crate::input::{InputAction, LineInput};
use crate::model::Model;
use crate::system::{self, ItermResult};
use crate::widgets::{
    account_badge, caret, chip, clock, dot, kbd, now_ms, phase_color, phase_label, plain, section_title,
};
use crate::{messages, theme};

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
    tab: DetailTab,
    conversation: Option<Conversation>,
    messages_scroll: ScrollHandle,
    mode: Mode,
    search: LineInput,
    rename: LineInput,
    /// Session to scroll into view on the next render of the list.
    pending_scroll: Option<SessionKey>,
    _poll: Task<()>,
}

/// Where typed keys go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Search,
    Rename,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Messages,
    Timeline,
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
            tab: DetailTab::Messages,
            conversation: None,
            messages_scroll: ScrollHandle::new(),
            mode: Mode::Normal,
            search: LineInput::default(),
            rename: LineInput::default(),
            pending_scroll: None,
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
            .filter(|s| self.filter.as_ref().is_none_or(|f| *f == s.key.account))
            .filter(|s| s.matches(&self.search.text));
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
        let clipboard = || cx.read_from_clipboard().and_then(|item| item.text());

        match self.mode {
            Mode::Rename => {
                match self.rename.handle(keystroke, clipboard) {
                    InputAction::Submit => self.submit_rename(),
                    InputAction::Cancel => self.mode = Mode::Normal,
                    InputAction::Changed | InputAction::Ignored => {}
                }
                cx.stop_propagation();
                cx.notify();
                return;
            }
            Mode::Search if !matches!(keystroke.key.as_str(), "up" | "down" | "enter") => {
                match self.search.handle(keystroke, clipboard) {
                    InputAction::Cancel => {
                        self.search.text.clear();
                        self.mode = Mode::Normal;
                    }
                    InputAction::Changed => self.select_first_visible(),
                    InputAction::Submit | InputAction::Ignored => {}
                }
                cx.stop_propagation();
                cx.notify();
                return;
            }
            _ => {}
        }

        if keystroke.modifiers.platform && keystroke.key == "f" {
            self.mode = Mode::Search;
            cx.stop_propagation();
            cx.notify();
            return;
        }
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
            "down" => self.select_index(&list, current.map_or(0, |i| (i + 1).min(list.len().saturating_sub(1)))),
            "up" => self.select_index(&list, current.map_or(0, |i| i.saturating_sub(1))),
            "enter" => self.jump_selected(),
            "escape" if !self.search.text.is_empty() => self.search.text.clear(),
            _ if self.mode == Mode::Search => return,
            "j" => self.select_index(&list, current.map_or(0, |i| (i + 1).min(list.len().saturating_sub(1)))),
            "k" => self.select_index(&list, current.map_or(0, |i| i.saturating_sub(1))),
            "tab" => self.cycle_filter(keystroke.modifiers.shift),
            "e" => self.show_ended = !self.show_ended,
            "r" => self.start_rename(),
            "left" | "right" => {
                self.tab = match self.tab {
                    DetailTab::Messages => DetailTab::Timeline,
                    DetailTab::Timeline => DetailTab::Messages,
                }
            }
            _ if keystroke.key_char.as_deref() == Some("/") => self.mode = Mode::Search,
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

    fn select_first_visible(&mut self) {
        let first = self.groups(now_ms()).navigable(self.show_ended).first().map(|s| s.key.clone());
        if first.is_some() {
            self.selected = first.clone();
            self.pending_scroll = first;
        }
    }

    fn start_rename(&mut self) {
        let Some(session) = self.selected.as_ref().and_then(|k| self.model.board.get(k)) else {
            return;
        };
        if !session.accepts_input() {
            self.set_status("Umbenennen geht nur, wenn die Session fertig ist und auf dich wartet – nicht während sie arbeitet oder eine Freigabe offen ist.");
            return;
        }
        self.rename = LineInput::with_text(&session.display_name());
        self.mode = Mode::Rename;
    }

    fn submit_rename(&mut self) {
        self.mode = Mode::Normal;
        if self.model.demo_messages.is_some() {
            // Demo pids are made up; the field can be shown but nothing is sent.
            self.set_status("Im Demo-Modus wird nichts umbenannt.");
            return;
        }
        let name = self.rename.text.trim().to_string();
        let Some(key) = self.selected.clone() else { return };
        let still_idle = self.model.board.get(&key).is_some_and(|s| s.accepts_input());
        if name.is_empty() || !still_idle {
            if !still_idle {
                self.set_status("Die Session arbeitet inzwischen wieder – nicht umbenannt.");
            }
            return;
        }
        match system::type_into_session(key.pid, &format!("/rename {name}")) {
            ItermResult::Done => self.set_status(&format!("„{name}“ an die Session geschickt (/rename).")),
            ItermResult::NoTerminal => self.set_status("Kein Terminal zu dieser Session gefunden."),
            ItermResult::Failed(reason) => self.set_status(&format!("Umbenennen fehlgeschlagen: {reason}")),
        }
    }

    fn set_status(&mut self, message: &str) {
        self.status = Some((message.to_string(), Instant::now()));
    }

    fn select_index(&mut self, list: &[SessionKey], index: usize) {
        if let Some(key) = list.get(index) {
            self.selected = Some(key.clone());
            self.pending_scroll = Some(key.clone());
        }
    }

    fn cycle_filter(&mut self, backwards: bool) {
        let mut options: Vec<Option<String>> = vec![None];
        options.extend(self.model.accounts.iter().map(|a| Some(a.id.clone())));
        let at = options.iter().position(|o| *o == self.filter).unwrap_or(0);
        let next = if backwards { (at + options.len() - 1) % options.len() } else { (at + 1) % options.len() };
        self.filter = options[next].clone();
    }

    /// Keeps the message list in step with the selected session. New messages
    /// scroll into view unless the user scrolled up to read older ones.
    fn sync_conversation(&mut self) {
        let Some(key) = self.selected.clone() else {
            self.conversation = None;
            return;
        };
        let switched = self.conversation.as_ref().is_none_or(|c| c.key != key);
        if let Some(demo) = &self.model.demo_messages {
            if switched {
                self.conversation = Some(Conversation::fixed(key.clone(), demo.get(&key).cloned().unwrap_or_default()));
                self.messages_scroll.scroll_to_bottom();
            }
            return;
        }
        let session_id = self.model.board.get(&key).and_then(|s| s.session_id.clone());
        let at_bottom = {
            let offset = self.messages_scroll.offset().y;
            let max = self.messages_scroll.max_offset().height;
            -offset >= max - px(24.)
        };
        let conversation = self.conversation.get_or_insert_with(|| Conversation::empty(key.clone()));
        let changed = conversation.sync(&key, session_id.as_deref(), &self.model.accounts);
        if switched || (changed && at_bottom) {
            self.messages_scroll.scroll_to_bottom();
        }
    }

    fn jump_selected(&mut self) {
        let Some(key) = self.selected.clone() else { return };
        if self.model.demo_messages.is_some() {
            // Demo pids are made up and may belong to unrelated processes.
            self.set_status("Im Demo-Modus öffnet Brain kein Terminal.");
            return;
        }
        let message = match system::jump_to_iterm(key.pid) {
            ItermResult::Done => return,
            ItermResult::NoTerminal => format!("pid {} hat kein Terminal (beendet oder SDK-Session)", key.pid),
            ItermResult::Failed(reason) => format!("Sprung fehlgeschlagen: {reason}"),
        };
        self.set_status(&message);
    }

    // ---- rendering -------------------------------------------------------------

    fn render_titlebar(&self, groups: &Groups, cx: &mut Context<Self>) -> impl IntoElement {
        let total = groups.attention.len() + groups.working.len() + groups.resting.len();
        let calling = groups.attention.iter().filter(|s| s.phase() == Phase::NeedsYou).count();
        let waiting = groups.attention.len();
        let sessions = if total == 1 { "1 Session".to_string() } else { format!("{total} Sessions") };
        let summary = match (waiting, total) {
            (_, 0) => "Keine laufenden Sessions".to_string(),
            (0, _) => format!("{sessions}, niemand wartet"),
            (1, _) => format!("{sessions}, 1 wartet auf dich"),
            (w, _) => format!("{sessions}, {w} warten auf dich"),
        };

        let mut segments = vec![(None, SharedString::from("Alle"))];
        segments.extend(self.model.accounts.iter().map(|a| (Some(a.id.clone()), SharedString::from(a.id.clone()))));

        div()
            .id("titlebar")
            .flex()
            .flex_none()
            .items_center()
            .h(px(48.))
            .pl(px(86.))
            .pr(px(14.))
            .gap(px(12.))
            .bg(theme::chrome())
            .border_b_1()
            .border_color(theme::line())
            .on_click(|event: &ClickEvent, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .child(brand_mark(calling > 0))
            .child(div().text_size(px(14.)).font_weight(FontWeight::BOLD).text_color(theme::text_strong()).child("Brain"))
            .child(div().text_size(px(12.5)).text_color(theme::text_muted()).child(summary))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .p(px(3.))
                    .gap(px(2.))
                    .rounded(px(8.))
                    .bg(theme::ink())
                    .border_1()
                    .border_color(theme::line())
                    .children(segments.into_iter().enumerate().map(|(ix, (value, label))| {
                        let active = value == self.filter;
                        div()
                            .id(("segment", ix))
                            .px(px(11.))
                            .py(px(3.))
                            .rounded(px(5.))
                            .text_size(px(12.))
                            .cursor_pointer()
                            .text_color(if active { theme::text_strong() } else { theme::text_muted() })
                            .when(active, |d| d.bg(theme::raised()).font_weight(FontWeight::MEDIUM))
                            .when(!active, |d| d.hover(|d| d.text_color(theme::text())))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.filter = value.clone();
                                cx.notify();
                            }))
                            .child(label)
                    })),
            )
    }

    fn render_list(&self, groups: &Groups, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let mut children: Vec<AnyElement> = vec![self.render_search(cx)];
        let mut nav_index = 0usize;
        let searching = !self.search.text.is_empty();

        if searching && groups.navigable(true).is_empty() {
            children.push(hint(format!("Keine Session passt zu „{}“. Esc leert die Suche.", self.search.text)));
        }
        children.push(section_title("Braucht dich", groups.attention.len(), true));
        if groups.attention.is_empty() && !searching {
            children.push(hint("Gerade wartet keine Session auf dich.".into()));
        }
        for session in &groups.attention {
            self.scroll_if_pending(&session.key, children.len(), nav_index);
            children.push(self.render_card(session, nav_index, now, cx));
            nav_index += 1;
        }

        for (title, sessions) in [("Arbeitet", &groups.working), ("Ruht seit über 2 h", &groups.resting)] {
            if sessions.is_empty() {
                continue;
            }
            children.push(section_title(title, sessions.len(), false));
            for session in sessions.iter() {
                self.scroll_if_pending(&session.key, children.len(), nav_index);
                children.push(self.render_row(session, nav_index, now, cx));
                nav_index += 1;
            }
        }

        if !groups.ended.is_empty() {
            let title = if self.show_ended { "Beendet ▾" } else { "Beendet ▸" };
            children.push(
                div()
                    .id("ended-toggle")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.show_ended = !this.show_ended;
                        cx.notify();
                    }))
                    .child(section_title(title, groups.ended.len(), false))
                    .into_any_element(),
            );
            if self.show_ended {
                for session in &groups.ended {
                    self.scroll_if_pending(&session.key, children.len(), nav_index);
                    children.push(self.render_row(session, nav_index, now, cx));
                    nav_index += 1;
                }
            }
        }

        div()
            .id("session-list")
            .flex()
            .flex_col()
            .flex_none()
            .w(relative(0.4))
            .min_w(px(360.))
            .max_w(px(480.))
            .h_full()
            .px(px(12.))
            .pt(px(12.))
            .pb(px(16.))
            .gap(px(6.))
            .bg(theme::chrome())
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .border_r_1()
            .border_color(theme::line())
            .children(children)
    }

    fn scroll_if_pending(&self, key: &SessionKey, child_index: usize, nav_index: usize) {
        if self.pending_scroll.as_ref() == Some(key) {
            // The first session scrolls the list to the very top so the search
            // field and section title stay visible.
            self.list_scroll.scroll_to_item(if nav_index == 0 { 0 } else { child_index });
        }
    }

    fn render_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.mode == Mode::Search;
        let query = self.search.text.clone();
        let content: AnyElement = if query.is_empty() && !editing {
            div().text_color(theme::text_faint()).child("Sessions durchsuchen").into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .min_w_0()
                .text_color(theme::text_strong())
                .child(query)
                .when(editing, |d| d.child(caret("search-caret")))
                .into_any_element()
        };
        div()
            .id("search")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(34.))
            .px(px(10.))
            .rounded(px(8.))
            .bg(theme::ink())
            .border_1()
            .border_color(if editing { theme::alpha(theme::working(), 0x99) } else { theme::line() })
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.mode = Mode::Search;
                cx.notify();
            }))
            .child(div().text_color(theme::text_faint()).child("⌕"))
            .child(content)
            .child(div().flex_1())
            .child(kbd(if editing { "esc" } else { "/" }))
            .into_any_element()
    }

    /// A session that waits for the user: colour rail, name, waiting time, headline.
    fn render_card(&self, s: &Session, nav_index: usize, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.as_ref() == Some(&s.key);
        let phase = s.phase();
        let color = phase_color(phase);
        let headline = plain(&s.headline().unwrap_or_else(|| match phase {
            Phase::NeedsYou => "Wartet auf deine Eingabe".into(),
            _ => "Fertig, du bist dran".into(),
        }));
        let headline = if s.headline_is_reported() && phase == Phase::NeedsYou { format!("„{headline}“") } else { headline };

        div()
            .id(("card", nav_index))
            .relative()
            .flex()
            .flex_col()
            .gap(px(5.))
            .pl(px(16.))
            .pr(px(12.))
            .py(px(11.))
            .rounded(px(10.))
            .border_1()
            .cursor_pointer()
            .bg(if selected { theme::raised() } else { theme::surface() })
            .border_color(if selected { theme::alpha(color, 0x77) } else { theme::line() })
            .when(selected, |d| {
                d.shadow(vec![BoxShadow {
                    color: theme::alpha(color, 0x30).into(),
                    offset: gpui::point(px(0.), px(0.)),
                    blur_radius: px(18.),
                    spread_radius: px(0.),
                }])
            })
            .when(!selected, |d| d.hover(|d| d.bg(theme::hover())))
            .on_click(self.select_on_click(s.key.clone(), cx))
            .child(rail(color, 10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(name_label(s, 13.5))
                    .child(account_badge(&s.key.account, self.account_index(&s.key.account)))
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(theme::ago(s.phase_since_ms(), now)),
                    )
                    .when(nav_index < 9, |d| d.child(kbd(format!("{}", nav_index + 1)))),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .line_height(relative(1.45))
                    .line_clamp(2)
                    .text_color(if phase == Phase::NeedsYou { theme::text_strong() } else { theme::text() })
                    .child(headline),
            )
            .into_any_element()
    }

    fn render_row(&self, s: &Session, nav_index: usize, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.as_ref() == Some(&s.key);
        let phase = s.phase();
        let detail = match phase {
            Phase::Working => plain(&s.headline().unwrap_or_else(|| "arbeitet …".into())),
            Phase::YourTurn => format!("fertig seit {}", theme::ago(s.phase_since_ms(), now)),
            _ => format!("vor {}", theme::ago(s.last_activity_ms, now)),
        };

        div()
            .id(("row", nav_index))
            .relative()
            .flex()
            .items_center()
            .gap(px(9.))
            .pl(px(12.))
            .pr(px(8.))
            .py(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme::raised()).child(rail(phase_color(phase), 8.)))
            .when(!selected, |d| d.hover(|d| d.bg(theme::hover())))
            .when(phase == Phase::Ended, |d| d.opacity(0.55))
            .on_click(self.select_on_click(s.key.clone(), cx))
            .child(dot(phase_color(phase), 7., false, ("row-dot", nav_index)))
            .child(name_label(s, 13.))
            .child(account_badge(&s.key.account, self.account_index(&s.key.account)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .truncate()
                    .child(detail),
            )
            .into_any_element()
    }

    fn select_on_click(
        &self,
        key: SessionKey,
        cx: &mut Context<Self>,
    ) -> Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static> {
        Box::new(cx.listener(move |this, event: &ClickEvent, _, cx| {
            this.selected = Some(key.clone());
            if event.click_count() >= 2 {
                this.jump_selected();
            }
            cx.notify();
        }))
    }

    fn render_name(&self, s: &Session, cx: &mut Context<Self>) -> AnyElement {
        if self.mode == Mode::Rename {
            return div()
                .flex()
                .items_center()
                .min_w_0()
                .px(px(8.))
                .py(px(1.))
                .rounded(px(7.))
                .border_1()
                .border_color(theme::alpha(theme::working(), 0x99))
                .bg(theme::ink())
                .text_size(px(20.))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_strong())
                .child(self.rename.text.clone())
                .child(caret("rename-caret"))
                .into_any_element();
        }
        div()
            .id("session-name")
            .group("session-name")
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.start_rename();
                cx.notify();
            }))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(20.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::text_strong())
                    .child(s.display_name()),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text_faint())
                    .group_hover("session-name", |d| d.text_color(theme::text()))
                    .child("✎"),
            )
            .into_any_element()
    }

    fn render_detail(&self, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let session = self.selected.as_ref().and_then(|k| self.model.board.get(k));
        let Some(s) = session else {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .bg(theme::ink())
                .child(div().text_color(theme::text()).child("Keine Session ausgewählt"))
                .child(div().text_size(px(12.5)).text_color(theme::text_muted()).child("Wähle links eine Session aus oder starte eine neue im Terminal."))
                .into_any_element();
        };
        let phase = s.phase();
        let color = phase_color(phase);

        let last_reply = self
            .conversation
            .as_ref()
            .filter(|c| c.key == s.key)
            .and_then(|c| c.messages.iter().rev().find(|m| m.role == Role::Assistant))
            .map(|m| brain_core::hook::one_line(&m.text, 320));
        let headline = plain(
            &s.headline()
                .or(last_reply)
                .unwrap_or_else(|| "Noch keine Meldung von dieser Session.".into()),
        );

        let meta: Vec<AnyElement> = if self.mode == Mode::Rename {
            vec![div()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child("⏎ schickt /rename an die Session, esc bricht ab")
                .into_any_element()]
        } else {
            let mut meta = vec![
                chip(s.cwd.as_deref().map(theme::tilde).unwrap_or_default(), true).into_any_element(),
                chip(format!("pid {}", s.key.pid), false).into_any_element(),
            ];
            if let Some(started) = s.started_ms {
                meta.push(chip(format!("seit {}", clock(started)), false).into_any_element());
            }
            meta
        };

        div()
            .id("detail")
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .h_full()
            .px(px(28.))
            .pt(px(20.))
            .bg(theme::ink())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(self.render_name(s, cx))
                    .child(account_badge(&s.key.account, self.account_index(&s.key.account)))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("jump")
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_strong())
                            .when(phase == Phase::NeedsYou, |d| d.bg(theme::calls()).hover(|d| d.bg(theme::calls_soft())))
                            .when(phase != Phase::NeedsYou, |d| {
                                d.bg(theme::raised()).border_1().border_color(theme::line_strong()).hover(|d| d.bg(theme::line_strong()))
                            })
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.jump_selected();
                                cx.notify();
                            }))
                            .child("In iTerm öffnen")
                            .child(kbd("⏎")),
                    ),
            )
            .child(div().flex().flex_wrap().gap(px(6.)).mt(px(10.)).mb(px(18.)).children(meta))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .mb(px(18.))
                    .pl(px(16.))
                    .pr(px(14.))
                    .py(px(12.))
                    .rounded(px(10.))
                    .bg(theme::alpha(color, 0x14))
                    .border_1()
                    .border_color(theme::alpha(color, 0x3a))
                    .child(rail(color, 10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .child(dot(color, 7., phase == Phase::NeedsYou, "detail-dot"))
                            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(color).child(phase_label(phase)))
                            .child(div().text_color(theme::text_muted()).child(format!("seit {}", theme::ago(s.phase_since_ms(), now)))),
                    )
                    .child(div().text_color(theme::text_strong()).line_height(relative(1.5)).child(headline)),
            )
            .child(self.render_tabs(s, cx))
            .child(match self.tab {
                DetailTab::Messages => self.render_messages(),
                DetailTab::Timeline => div()
                    .id("timeline")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .pt(px(12.))
                    .pb(px(20.))
                    .overflow_y_scroll()
                    .children(messages::timeline(s))
                    .into_any_element(),
            })
            .into_any_element()
    }

    fn render_tabs(&self, s: &Session, cx: &mut Context<Self>) -> impl IntoElement {
        let message_count = self.conversation.as_ref().map_or(0, |c| c.messages.len());
        let tabs = [
            (DetailTab::Messages, "Nachrichten", message_count),
            (DetailTab::Timeline, "Verlauf", s.timeline.len()),
        ];
        div()
            .flex()
            .flex_none()
            .gap(px(18.))
            .border_b_1()
            .border_color(theme::line())
            .children(tabs.into_iter().map(|(tab, label, count)| {
                let active = self.tab == tab;
                div()
                    .id(label)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pb(px(9.))
                    .mb(px(-1.))
                    .cursor_pointer()
                    .border_b_2()
                    .border_color(if active { theme::working() } else { theme::alpha(theme::working(), 0) })
                    .text_size(px(13.))
                    .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                    .text_color(if active { theme::text_strong() } else { theme::text_muted() })
                    .when(!active, |d| d.hover(|d| d.text_color(theme::text())))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
                    .child(label)
                    .child(div().text_size(px(11.5)).text_color(theme::text_faint()).child(count.to_string()))
            }))
    }

    fn render_messages(&self) -> AnyElement {
        let messages = self.conversation.as_ref().map(|c| c.messages.as_slice()).unwrap_or_default();
        let children = if messages.is_empty() {
            vec![hint("Diese Session hat noch keine Nachrichten.".into())]
        } else {
            messages::conversation(messages)
        };
        div()
            .id("messages")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(16.))
            .pt(px(16.))
            .pb(px(24.))
            .pr(px(4.))
            .overflow_y_scroll()
            .track_scroll(&self.messages_scroll)
            .children(children)
            .into_any_element()
    }

    fn render_footer(&self) -> impl IntoElement {
        let hints: [(&str, &str); 8] = [
            ("↑↓", "wählen"),
            ("⏎", "öffnen"),
            ("/", "suchen"),
            ("R", "umbenennen"),
            ("←→", "Ansicht"),
            ("1–9", "direkt"),
            ("⇥", "Konto"),
            ("E", "beendete"),
        ];
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(14.))
            .h(px(32.))
            .px(px(14.))
            .bg(theme::chrome())
            .border_t_1()
            .border_color(theme::line())
            .text_size(px(11.5))
            .text_color(theme::text_faint())
            .children(hints.map(|(key, label)| div().flex().items_center().gap(px(5.)).child(kbd(key)).child(label)))
            .child(div().flex_1())
            .when_some(self.status.as_ref(), |d, (message, _)| {
                d.child(div().text_color(theme::calls_soft()).truncate().child(message.clone()))
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
        self.sync_conversation();

        let groups = self.groups(now);
        let waiting = groups.attention.len();
        window.set_window_title(&if waiting > 0 { format!("Brain – {waiting} warten") } else { "Brain".into() });
        let titlebar = self.render_titlebar(&groups, cx).into_any_element();
        let list = self.render_list(&groups, now, cx).into_any_element();
        let detail = self.render_detail(now, cx);
        drop(groups);
        self.pending_scroll = None;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::ink())
            .font_family(".SystemUIFont")
            .text_color(theme::text())
            .text_size(px(13.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(titlebar)
            .child(div().flex().flex_1().min_h_0().child(list).child(detail))
            .child(self.render_footer())
    }
}

/// Brain's mark in miniature: a grid of sessions, one of them calling.
fn brand_mark(calling: bool) -> impl IntoElement {
    let cell = |color: gpui::Rgba| div().size(px(4.)).rounded_full().bg(color);
    let quiet = theme::line_strong();
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(4.))
        .rounded(px(6.))
        .bg(theme::raised())
        .child(div().flex().gap(px(2.)).child(cell(quiet)).child(cell(theme::working())).child(cell(if calling { theme::calls() } else { quiet })))
        .child(div().flex().gap(px(2.)).child(cell(theme::working())).child(cell(quiet)).child(cell(quiet)))
        .child(div().flex().gap(px(2.)).child(cell(quiet)).child(cell(quiet)).child(cell(theme::working())))
}

/// The phase colour as a bar along the left edge of a rounded container.
fn rail(color: gpui::Rgba, radius: f32) -> impl IntoElement {
    div()
        .absolute()
        .left(px(-1.))
        .top(px(-1.))
        .bottom(px(-1.))
        .w(px(4.))
        .rounded_l(px(radius))
        .bg(color)
}

fn name_label(s: &Session, size: f32) -> impl IntoElement {
    div()
        .flex_none()
        .max_w(px(210.))
        .truncate()
        .text_size(px(size))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_strong())
        .child(s.display_name())
}

fn hint(text: String) -> AnyElement {
    div().px(px(6.)).py(px(10.)).text_size(px(12.5)).text_color(theme::text_muted()).child(text).into_any_element()
}
