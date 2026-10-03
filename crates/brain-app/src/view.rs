use std::collections::HashMap;
use std::time::{Duration, Instant};

use brain_core::state::{Phase, Session, SessionKey};
use brain_core::transcript::Role;
use gpui::{
    div, prelude::*, px, relative, AnyElement, BoxShadow, ClickEvent, Context, FocusHandle,
    FontWeight, KeyDownEvent, ScrollHandle, SharedString, Task, Window,
};

use crate::conversation::Conversation;
use crate::i18n::t;
use crate::input::{InputAction, LineInput};
use crate::menubar::MenuBar;
use crate::model::{Model, HISTORY_DAYS};
use crate::notify::{self, Notifier};
use crate::prefs::Prefs;
use crate::terminal::{self, Capabilities, Key, Outcome};
use crate::widgets::{
    account_badge, caret, chip, clock, dot, kbd, now_ms, phase_color, phase_label, plain, section_title,
};
use crate::{messages, theme, tr};

/// Sessions on "your turn" for longer than this move to the "resting" section.
const RESTING_AFTER_MS: i64 = 2 * 60 * 60 * 1000;
/// Ended sessions stay listed this long (they are read from the event history).
const ENDED_VISIBLE_MS: i64 = HISTORY_DAYS as i64 * 24 * 60 * 60 * 1000;
const SNOOZE_MS: i64 = 15 * 60 * 1000;

pub struct BrainView {
    model: Model,
    prefs: Prefs,
    notifier: Notifier,
    menubar: Option<MenuBar>,
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
    reply: LineInput,
    /// Session to scroll into view on the next render of the list.
    pending_scroll: Option<SessionKey>,
    /// Notifications for these sessions are held back until the given time (epoch ms).
    snoozed: HashMap<SessionKey, i64>,
    /// The terminal hosting the selected session and what Brain can do with it.
    host: Option<(SessionKey, Capabilities)>,
    /// Project name and worktree per session, refreshed on render.
    projects: HashMap<SessionKey, (String, Option<String>)>,
    _poll: Task<()>,
}

/// Where typed keys go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Search,
    Rename,
    Reply,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Messages,
    Timeline,
}

/// The left column, already filtered and sorted.
struct Groups<'a> {
    pinned: Vec<&'a Session>,
    attention: Vec<&'a Session>,
    working: Vec<&'a Session>,
    resting: Vec<&'a Session>,
    ended: Vec<&'a Session>,
}

impl<'a> Groups<'a> {
    fn navigable(&self, include_ended: bool) -> Vec<&'a Session> {
        let mut all: Vec<&Session> = Vec::new();
        all.extend(&self.pinned);
        all.extend(&self.attention);
        all.extend(&self.working);
        all.extend(&self.resting);
        if include_ended {
            all.extend(&self.ended);
        }
        all
    }

    fn live_count(&self) -> usize {
        self.pinned.len() + self.attention.len() + self.working.len() + self.resting.len()
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
            prefs: Prefs::load(),
            notifier: Notifier::new(),
            menubar: MenuBar::new(),
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
            reply: LineInput::default(),
            pending_scroll: None,
            snoozed: HashMap::new(),
            host: None,
            projects: HashMap::new(),
            _poll: poll,
        };
        view.selected = view.groups(now_ms()).navigable(false).first().map(|s| s.key.clone());
        view
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        for item in self.model.refresh() {
            let snoozed = self.snoozed.get(&item.key).is_some_and(|until| *until > now);
            if snoozed || self.prefs.is_muted(&item.key) {
                continue;
            }
            let account = item.key.account.clone();
            let subtitle = match item.phase {
                Phase::NeedsYou => tr!("{account} · braucht dich", "{account} · needs you"),
                _ => tr!("{account} · fertig, du bist dran", "{account} · done, your turn"),
            };
            let body = plain(item.headline.as_deref().unwrap_or(""));
            self.notifier.post(&item.key, &item.name, &subtitle, &body, item.phase == Phase::NeedsYou);
        }

        for response in notify::take_responses() {
            match response {
                notify::Response::Open(key) => {
                    self.selected = Some(key.clone());
                    self.pending_scroll = Some(key);
                    cx.activate(true);
                }
                notify::Response::Snooze(key) => {
                    self.snoozed.insert(key, now + SNOOZE_MS);
                }
            }
        }
        if MenuBar::take_click() {
            cx.activate(true);
        }

        if self.status.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(6)) {
            self.status = None;
        }
        cx.notify();
    }

    fn groups(&self, now: i64) -> Groups<'_> {
        let mut groups = Groups { pinned: vec![], attention: vec![], working: vec![], resting: vec![], ended: vec![] };
        let searching = !self.search.text.is_empty();
        let visible = self
            .model
            .board
            .sorted()
            .into_iter()
            .filter(|s| self.filter.as_ref().is_none_or(|f| *f == s.key.account))
            .filter(|s| s.matches(&self.search.text));
        for session in visible {
            if self.prefs.is_pinned(&session.key) && session.phase() != Phase::Ended {
                groups.pinned.push(session);
                continue;
            }
            match session.phase() {
                Phase::NeedsYou => groups.attention.push(session),
                Phase::YourTurn if now - session.phase_since_ms() > RESTING_AFTER_MS => groups.resting.push(session),
                Phase::YourTurn => groups.attention.push(session),
                Phase::Working => groups.working.push(session),
                Phase::Ended if searching || now - session.last_activity_ms < ENDED_VISIBLE_MS => groups.ended.push(session),
                Phase::Ended => {}
            }
        }
        // Resting: most recently finished first.
        groups.resting.reverse();
        groups
    }

    fn include_ended(&self) -> bool {
        self.show_ended || !self.search.text.is_empty()
    }

    fn account_index(&self, account: &str) -> usize {
        self.model.accounts.iter().position(|a| a.id == account).unwrap_or(0)
    }

    fn selected_session(&self) -> Option<&Session> {
        self.selected.as_ref().and_then(|k| self.model.board.get(k))
    }

    /// What Brain may do with the selected session's terminal. Demo sessions get everything so
    /// screenshots show the controls; their actions are blocked elsewhere.
    fn capabilities(&self) -> Capabilities {
        if self.model.is_demo() {
            return Capabilities { focus: true, type_text: true, keys: true };
        }
        match &self.host {
            Some((key, caps)) if Some(key) == self.selected.as_ref() => *caps,
            _ => Capabilities { focus: false, type_text: false, keys: false },
        }
    }

    // ---- input -------------------------------------------------------------

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let clipboard = || cx.read_from_clipboard().and_then(|item| item.text());

        match self.mode {
            Mode::Rename | Mode::Reply => {
                let renaming = self.mode == Mode::Rename;
                let field = if renaming { &mut self.rename } else { &mut self.reply };
                match field.handle(keystroke, clipboard) {
                    InputAction::Submit if renaming => self.submit_rename(),
                    InputAction::Submit => self.submit_reply(),
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
        let list: Vec<SessionKey> =
            self.groups(now_ms()).navigable(self.include_ended()).iter().map(|s| s.key.clone()).collect();
        let current = self.selected.as_ref().and_then(|k| list.iter().position(|l| l == k));
        let next = current.map_or(0, |i| (i + 1).min(list.len().saturating_sub(1)));
        let previous = current.map_or(0, |i| i.saturating_sub(1));

        match keystroke.key.as_str() {
            "down" => self.select_index(&list, next),
            "up" => self.select_index(&list, previous),
            "enter" => self.open_selected(),
            "escape" if !self.search.text.is_empty() => self.search.text.clear(),
            _ if self.mode == Mode::Search => return,
            "j" => self.select_index(&list, next),
            "k" => self.select_index(&list, previous),
            "tab" => self.cycle_filter(keystroke.modifiers.shift),
            "e" => self.show_ended = !self.show_ended,
            "r" => self.start_rename(),
            "t" => self.start_reply(),
            "y" => self.answer_permission(true),
            "n" => self.answer_permission(false),
            "p" => self.toggle_pin(),
            "m" => self.toggle_mute(),
            "g" => {
                self.prefs.group_by_project = !self.prefs.group_by_project;
                self.prefs.save();
            }
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
                    self.open_selected();
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn select_first_visible(&mut self) {
        let first = self.groups(now_ms()).navigable(self.include_ended()).first().map(|s| s.key.clone());
        if first.is_some() {
            self.selected = first.clone();
            self.pending_scroll = first;
        }
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

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), Instant::now()));
    }

    /// Demo pids are made up and may belong to unrelated processes: nothing is ever sent.
    fn blocked_in_demo(&mut self) -> bool {
        if self.model.is_demo() {
            self.set_status(t("Im Demo-Modus schickt Brain nichts an Terminals.", "In demo mode Brain sends nothing to terminals."));
        }
        self.model.is_demo()
    }

    fn report(&mut self, outcome: Outcome, done: Option<String>) {
        match outcome {
            Outcome::Done => {
                if let Some(message) = done {
                    self.set_status(message);
                }
            }
            Outcome::NoTerminal => self.set_status(t(
                "Kein Terminal zu dieser Session gefunden (beendet oder ohne Terminal gestartet).",
                "No terminal found for this session (it ended or runs without one).",
            )),
            Outcome::Unsupported(reason) => self.set_status(reason),
            Outcome::Failed(reason) => self.set_status(tr!("Fehlgeschlagen: {reason}", "Failed: {reason}")),
        }
    }

    /// ⏎: a live session's terminal comes forward; an ended one is resumed in a new tab.
    fn open_selected(&mut self) {
        let Some(session) = self.selected_session() else { return };
        if session.phase() == Phase::Ended {
            self.resume_selected();
            return;
        }
        let (pid, cwd) = (session.key.pid, session.cwd.clone());
        if self.blocked_in_demo() {
            return;
        }
        let outcome = terminal::focus(pid, cwd.as_deref());
        self.report(outcome, None);
    }

    fn resume_selected(&mut self) {
        let Some(session) = self.selected_session() else { return };
        let (Some(session_id), Some(cwd)) = (session.session_id.clone(), session.cwd.clone()) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder to resume."));
            return;
        };
        let config_dir = self
            .model
            .account(&session.key.account)
            .filter(|a| a.id != "main")
            .map(|a| a.config_dir.display().to_string());
        if self.blocked_in_demo() {
            return;
        }
        let outcome = terminal::open_new(&cwd, config_dir.as_deref(), &format!("claude --resume {session_id}"));
        self.report(outcome, Some(t("Session in einem neuen Tab fortgesetzt.", "Resumed the session in a new tab.").into()));
    }

    fn start_rename(&mut self) {
        let Some(session) = self.selected_session() else { return };
        if !session.accepts_input() {
            self.set_status(t(
                "Umbenennen geht nur, wenn die Session fertig ist und auf dich wartet.",
                "Renaming works only while the session has finished and waits for you.",
            ));
            return;
        }
        if !self.capabilities().type_text {
            self.set_status(t("In dieses Terminal kann Brain nicht tippen.", "Brain can't type into this terminal."));
            return;
        }
        self.rename = LineInput::with_text(&session.display_name());
        self.mode = Mode::Rename;
    }

    fn submit_rename(&mut self) {
        self.mode = Mode::Normal;
        let name = self.rename.text.trim().to_string();
        if name.is_empty() || self.blocked_in_demo() {
            return;
        }
        self.type_into_selected(&format!("/rename {name}"), tr!("„{name}“ an die Session geschickt.", "Sent “{name}” to the session."));
    }

    fn start_reply(&mut self) {
        let Some(session) = self.selected_session() else { return };
        if !session.accepts_input() {
            self.set_status(t(
                "Antworten geht, sobald die Session fertig ist und auf dich wartet.",
                "You can reply once the session has finished and waits for you.",
            ));
            return;
        }
        if !self.capabilities().type_text {
            self.set_status(t("In dieses Terminal kann Brain nicht tippen.", "Brain can't type into this terminal."));
            return;
        }
        self.reply = LineInput::default();
        self.mode = Mode::Reply;
    }

    fn submit_reply(&mut self) {
        self.mode = Mode::Normal;
        let text = self.reply.text.trim().to_string();
        if text.is_empty() || self.blocked_in_demo() {
            return;
        }
        self.type_into_selected(&text, t("Antwort geschickt.", "Reply sent.").into());
    }

    /// Types text plus Return, but only if the session still accepts input right now.
    fn type_into_selected(&mut self, text: &str, done: String) {
        let Some(key) = self.selected.clone() else { return };
        if !self.model.board.get(&key).is_some_and(|s| s.accepts_input()) {
            self.set_status(t("Die Session arbeitet inzwischen wieder – nichts geschickt.", "The session is working again – nothing sent."));
            return;
        }
        let outcome = terminal::type_text(key.pid, text);
        self.report(outcome, Some(done));
    }

    /// Y / N on an open permission dialog: Return picks its default "Yes", Esc declines.
    fn answer_permission(&mut self, allow: bool) {
        let Some(session) = self.selected_session() else { return };
        if !session.awaiting_permission() {
            self.set_status(t("Gerade ist keine Freigabe offen.", "No permission request is open right now."));
            return;
        }
        if !self.capabilities().keys {
            self.set_status(t("In diesem Terminal kann Brain keine Freigaben beantworten.", "Brain can't answer permissions in this terminal."));
            return;
        }
        let pid = session.key.pid;
        if self.blocked_in_demo() {
            return;
        }
        let (key, done) = if allow {
            (Key::Return, t("Freigabe erteilt.", "Allowed."))
        } else {
            (Key::Escape, t("Freigabe abgelehnt.", "Denied."))
        };
        let outcome = terminal::send_key(pid, key);
        self.report(outcome, Some(done.into()));
    }

    fn toggle_pin(&mut self) {
        let Some(key) = self.selected.clone() else { return };
        let pinned = self.prefs.toggle_pin(&key);
        self.prefs.save();
        self.pending_scroll = Some(key);
        self.set_status(if pinned { t("Angeheftet.", "Pinned.") } else { t("Nicht mehr angeheftet.", "Unpinned.") });
    }

    fn toggle_mute(&mut self) {
        let Some(key) = self.selected.clone() else { return };
        let muted = self.prefs.toggle_mute(&key);
        self.prefs.save();
        self.set_status(if muted {
            t("Keine Benachrichtigungen mehr für diese Session.", "Notifications muted for this session.")
        } else {
            t("Benachrichtigungen wieder an.", "Notifications on again.")
        });
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

    /// Looks up the terminal of a newly selected session (one `ps` call per selection).
    fn sync_host(&mut self) {
        let Some(key) = self.selected.clone() else {
            self.host = None;
            return;
        };
        if self.model.is_demo() || self.host.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let caps = terminal::host_of(key.pid)
            .map(|host| terminal::capabilities(&host))
            .unwrap_or(Capabilities { focus: false, type_text: false, keys: false });
        self.host = Some((key, caps));
    }

    /// Project names for the sessions (git is asked once per directory).
    fn sync_projects(&mut self) {
        let sessions: Vec<(SessionKey, String)> = self
            .model
            .board
            .keys()
            .filter(|k| !self.projects.contains_key(*k))
            .filter_map(|k| Some((k.clone(), self.model.board.get(k)?.cwd.clone()?)))
            .collect();
        for (key, cwd) in sessions {
            let project = self.model.project(&cwd);
            let entry = (project.name.clone(), project.worktree.clone());
            self.projects.insert(key, entry);
        }
    }

    // ---- rendering -------------------------------------------------------------

    fn render_titlebar(&self, groups: &Groups, cx: &mut Context<Self>) -> impl IntoElement {
        let total = groups.live_count();
        let waiting = groups
            .attention
            .iter()
            .chain(&groups.pinned)
            .filter(|s| matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn))
            .count();
        let calling = groups.attention.iter().chain(&groups.pinned).any(|s| s.phase() == Phase::NeedsYou);
        let sessions = if total == 1 { t("1 Session", "1 session").to_string() } else { tr!("{total} Sessions", "{total} sessions") };
        let summary = match waiting {
            _ if total == 0 => t("Keine laufenden Sessions", "No running sessions").to_string(),
            0 => tr!("{sessions}, niemand wartet", "{sessions}, nobody waiting"),
            1 => tr!("{sessions}, 1 wartet auf dich", "{sessions}, 1 waiting for you"),
            w => tr!("{sessions}, {w} warten auf dich", "{sessions}, {w} waiting for you"),
        };

        let mut accounts = vec![(None, SharedString::from(t("Alle", "All")))];
        accounts.extend(self.model.accounts.iter().map(|a| (Some(a.id.clone()), SharedString::from(a.id.clone()))));

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
            .child(brand_mark(calling))
            .child(div().text_size(px(14.)).font_weight(FontWeight::BOLD).text_color(theme::text_strong()).child("Brain"))
            .child(div().text_size(px(12.5)).text_color(theme::text_muted()).child(summary))
            .child(div().flex_1())
            .child(segmented(
                "layout",
                vec![(false, SharedString::from(t("Status", "Status"))), (true, SharedString::from(t("Projekte", "Projects")))],
                self.prefs.group_by_project,
                cx.listener(|this, value: &bool, _, cx| {
                    this.prefs.group_by_project = *value;
                    this.prefs.save();
                    cx.notify();
                }),
            ))
            .child(segmented(
                "accounts",
                accounts,
                self.filter.clone(),
                cx.listener(|this, value: &Option<String>, _, cx| {
                    this.filter = value.clone();
                    cx.notify();
                }),
            ))
    }

    fn render_list(&self, groups: &Groups, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let mut children: Vec<AnyElement> = vec![self.render_search(cx)];
        let searching = !self.search.text.is_empty();

        if searching && groups.navigable(true).is_empty() {
            let query = self.search.text.clone();
            children.push(hint(tr!("Keine Session passt zu „{query}“. Esc leert die Suche.", "No session matches “{query}”. Esc clears the search.")));
        }

        let mut nav = 0usize;
        if self.prefs.group_by_project {
            self.render_by_project(groups, now, cx, &mut nav, &mut children);
        } else {
            self.render_by_status(groups, now, cx, &mut nav, &mut children);
        }
        self.push_ended(groups, &mut nav, now, cx, &mut children);

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

    fn render_by_status(&self, groups: &Groups, now: i64, cx: &mut Context<Self>, nav: &mut usize, children: &mut Vec<AnyElement>) {
        let searching = !self.search.text.is_empty();
        if !groups.pinned.is_empty() {
            children.push(section_title(t("Angeheftet", "Pinned"), groups.pinned.len(), false));
            for session in &groups.pinned {
                self.push_session(session, nav, now, cx, children);
            }
        }
        children.push(section_title(t("Braucht dich", "Needs you"), groups.attention.len(), true));
        if groups.attention.is_empty() && !searching {
            children.push(hint(t("Gerade wartet keine Session auf dich.", "No session is waiting for you right now.").into()));
        }
        for session in &groups.attention {
            self.push_session(session, nav, now, cx, children);
        }
        for (title, sessions) in [
            (t("Arbeitet", "Working"), &groups.working),
            (t("Ruht seit über 2 h", "Resting for over 2 h"), &groups.resting),
        ] {
            if sessions.is_empty() {
                continue;
            }
            children.push(section_title(title, sessions.len(), false));
            for session in sessions.iter() {
                self.push_session(session, nav, now, cx, children);
            }
        }
    }

    /// Projects ordered by their most urgent session; worktrees count as their main repository.
    fn render_by_project(&self, groups: &Groups, now: i64, cx: &mut Context<Self>, nav: &mut usize, children: &mut Vec<AnyElement>) {
        let mut live = groups.navigable(false);
        live.sort_by_key(|s| s.phase());
        let mut projects: Vec<(String, Vec<&Session>)> = Vec::new();
        for session in live {
            let name = self.projects.get(&session.key).map(|(n, _)| n.clone()).unwrap_or_else(|| session.display_name());
            match projects.iter_mut().find(|(n, _)| *n == name) {
                Some((_, list)) => list.push(session),
                None => projects.push((name, vec![session])),
            }
        }
        if projects.is_empty() && self.search.text.is_empty() {
            children.push(hint(t("Keine laufenden Sessions.", "No running sessions.").into()));
        }
        for (name, sessions) in projects {
            let alert = sessions.iter().any(|s| s.phase() == Phase::NeedsYou);
            children.push(section_title(&name, sessions.len(), alert));
            for session in sessions {
                self.push_session(session, nav, now, cx, children);
            }
        }
    }

    fn push_ended(&self, groups: &Groups, nav: &mut usize, now: i64, cx: &mut Context<Self>, children: &mut Vec<AnyElement>) {
        if groups.ended.is_empty() {
            return;
        }
        let open = self.include_ended();
        let title = format!("{} {}", t("Beendet", "Ended"), if open { "▾" } else { "▸" });
        children.push(
            div()
                .id("ended-toggle")
                .cursor_pointer()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.show_ended = !this.show_ended;
                    cx.notify();
                }))
                .child(section_title(&title, groups.ended.len(), false))
                .into_any_element(),
        );
        if open {
            for session in &groups.ended {
                self.push_session(session, nav, now, cx, children);
            }
        }
    }

    /// Recently waiting sessions as cards, the others as rows.
    fn push_session(&self, s: &Session, nav: &mut usize, now: i64, cx: &mut Context<Self>, children: &mut Vec<AnyElement>) {
        self.scroll_if_pending(&s.key, children.len(), *nav);
        let waiting = matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn);
        let recent = now - s.phase_since_ms() <= RESTING_AFTER_MS || s.phase() == Phase::NeedsYou;
        let element = if waiting && recent { self.render_card(s, *nav, now, cx) } else { self.render_row(s, *nav, now, cx) };
        children.push(element);
        *nav += 1;
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
            div().text_color(theme::text_faint()).child(t("Sessions durchsuchen", "Search sessions")).into_any_element()
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

    /// Small markers after a session's name: worktree, pinned, muted.
    fn markers(&self, s: &Session) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if let Some((_, Some(worktree))) = self.projects.get(&s.key) {
            out.push(marker(format!("⑂ {worktree}"), theme::text_faint()));
        }
        if self.prefs.is_pinned(&s.key) {
            out.push(marker("★".into(), theme::turn()));
        }
        if self.prefs.is_muted(&s.key) {
            out.push(marker(t("stumm", "muted").into(), theme::text_faint()));
        }
        out
    }

    /// A session that waits for the user: colour rail, name, waiting time, headline.
    fn render_card(&self, s: &Session, nav_index: usize, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.as_ref() == Some(&s.key);
        let phase = s.phase();
        let color = phase_color(phase);
        let headline = plain(&s.headline().unwrap_or_else(|| match phase {
            Phase::NeedsYou => t("Wartet auf deine Eingabe", "Waiting for your input").into(),
            _ => t("Fertig, du bist dran", "Done, your turn").into(),
        }));
        let headline = if s.headline_is_reported() && phase == Phase::NeedsYou { format!("“{headline}”") } else { headline };

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
                    .children(self.markers(s))
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
            Phase::Working => plain(&s.headline().unwrap_or_else(|| t("arbeitet …", "working …").into())),
            Phase::YourTurn => {
                let ago = theme::ago(s.phase_since_ms(), now);
                tr!("fertig seit {ago}", "done for {ago}")
            }
            _ => {
                let ago = theme::ago(s.last_activity_ms, now);
                tr!("vor {ago}", "{ago} ago")
            }
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
            .when(phase == Phase::Ended, |d| d.opacity(0.6))
            .on_click(self.select_on_click(s.key.clone(), cx))
            .child(dot(phase_color(phase), 7., false, ("row-dot", nav_index)))
            .child(name_label(s, 13.))
            .child(account_badge(&s.key.account, self.account_index(&s.key.account)))
            .children(self.markers(s))
            .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(theme::text_muted()).truncate().child(detail))
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
                this.open_selected();
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

    /// Path, project, model, context, cost, permission mode, pid, start.
    fn meta_chips(&self, s: &Session) -> Vec<AnyElement> {
        if self.mode == Mode::Rename {
            return vec![div()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child(t("⏎ schickt /rename an die Session, esc bricht ab", "⏎ sends /rename to the session, esc cancels"))
                .into_any_element()];
        }
        let mut meta = vec![chip(s.cwd.as_deref().map(theme::tilde).unwrap_or_default(), true).into_any_element()];
        let info = &s.insight;
        if let Some(model) = &info.model {
            meta.push(chip(short_model(model), false).into_any_element());
        }
        if let Some(tokens) = info.context_tokens {
            let tokens = format_tokens(tokens);
            meta.push(chip(tr!("{tokens} Kontext", "{tokens} context"), false).into_any_element());
        }
        if let Some(cost) = info.cost_usd {
            meta.push(chip(format!("${cost:.2}"), false).into_any_element());
        }
        if let Some(mode) = info.permission_mode.as_deref().filter(|m| *m != "default") {
            meta.push(chip(mode.to_string(), false).into_any_element());
        }
        meta.push(chip(format!("pid {}", s.key.pid), false).into_any_element());
        if let Some(started) = s.started_ms {
            let at = clock(started);
            meta.push(chip(tr!("seit {at}", "since {at}"), false).into_any_element());
        }
        meta
    }

    fn render_detail(&self, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let Some(s) = self.selected_session() else {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .bg(theme::ink())
                .child(div().text_color(theme::text()).child(t("Keine Session ausgewählt", "No session selected")))
                .child(div().text_size(px(12.5)).text_color(theme::text_muted()).child(t(
                    "Wähle links eine Session aus oder starte eine neue im Terminal.",
                    "Pick a session on the left, or start a new one in your terminal.",
                )))
                .into_any_element();
        };
        let phase = s.phase();
        let caps = self.capabilities();
        let primary = if phase == Phase::Ended {
            button("open", t("Fortsetzen", "Resume"), "⏎", false, true, cx.listener(|this, _: &ClickEvent, _, cx| {
                this.resume_selected();
                cx.notify();
            }))
        } else {
            button("open", t("Zum Terminal", "Open terminal"), "⏎", phase == Phase::NeedsYou, caps.focus, cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open_selected();
                cx.notify();
            }))
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
                    .children(self.markers(s))
                    .child(div().flex_1())
                    .child(primary),
            )
            .child(div().flex().flex_wrap().gap(px(6.)).mt(px(10.)).mb(px(18.)).children(self.meta_chips(s)))
            .child(self.render_callout(s, now, caps, cx))
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

    /// The state box: what the session waits for, plus the actions that fit.
    fn render_callout(&self, s: &Session, now: i64, caps: Capabilities, cx: &mut Context<Self>) -> AnyElement {
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
                .unwrap_or_else(|| t("Noch keine Meldung von dieser Session.", "No message from this session yet.").into()),
        );
        let since = theme::ago(s.phase_since_ms(), now);

        let mut actions: Vec<AnyElement> = Vec::new();
        if s.awaiting_permission() {
            actions.push(button("allow", t("Erlauben", "Allow"), "Y", true, caps.keys, cx.listener(|this, _: &ClickEvent, _, cx| {
                this.answer_permission(true);
                cx.notify();
            })));
            actions.push(button("deny", t("Ablehnen", "Deny"), "N", false, caps.keys, cx.listener(|this, _: &ClickEvent, _, cx| {
                this.answer_permission(false);
                cx.notify();
            })));
        } else if s.accepts_input() && self.mode != Mode::Reply {
            actions.push(button("reply", t("Antworten", "Reply"), "T", false, caps.type_text, cx.listener(|this, _: &ClickEvent, _, cx| {
                this.start_reply();
                cx.notify();
            })));
        }

        div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(8.))
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
                    .child(div().text_color(theme::text_muted()).child(tr!("seit {since}", "for {since}"))),
            )
            .child(div().text_color(theme::text_strong()).line_height(relative(1.5)).child(headline))
            .when(self.mode == Mode::Reply, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mt(px(2.))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(8.))
                        .bg(theme::ink())
                        .border_1()
                        .border_color(theme::alpha(theme::working(), 0x99))
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                .text_color(theme::text_strong())
                                .when(self.reply.text.is_empty(), |d| {
                                    d.child(div().text_color(theme::text_faint()).child(t("Antwort an die Session …", "Reply to the session …")))
                                })
                                .child(self.reply.text.clone())
                                .child(caret("reply-caret")),
                        )
                        .child(div().flex_none().text_size(px(11.)).text_color(theme::text_muted()).child(t("⏎ senden · esc", "⏎ send · esc"))),
                )
            })
            .when(!actions.is_empty(), |d| d.child(div().flex().gap(px(8.)).mt(px(2.)).children(actions)))
            .into_any_element()
    }

    fn render_tabs(&self, s: &Session, cx: &mut Context<Self>) -> impl IntoElement {
        let message_count = self.conversation.as_ref().map_or(0, |c| c.messages.len());
        let tabs = [
            (DetailTab::Messages, "messages-tab", t("Nachrichten", "Messages"), message_count),
            (DetailTab::Timeline, "timeline-tab", t("Verlauf", "Timeline"), s.timeline.len()),
        ];
        div()
            .flex()
            .flex_none()
            .gap(px(18.))
            .border_b_1()
            .border_color(theme::line())
            .children(tabs.into_iter().map(|(tab, id, label, count)| {
                let active = self.tab == tab;
                div()
                    .id(id)
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
            vec![hint(t("Diese Session hat noch keine Nachrichten.", "This session has no messages yet.").into())]
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
        let hints: [(&str, &str); 9] = [
            ("↑↓", t("wählen", "select")),
            ("⏎", t("öffnen", "open")),
            ("/", t("suchen", "search")),
            ("T", t("antworten", "reply")),
            ("Y N", t("Freigabe", "permission")),
            ("R", t("umbenennen", "rename")),
            ("P M", t("anheften · stumm", "pin · mute")),
            ("G", t("Projekte", "projects")),
            ("⇥", t("Konto", "account")),
        ];
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .h(px(32.))
            .px(px(14.))
            .bg(theme::chrome())
            .border_t_1()
            .border_color(theme::line())
            .text_size(px(11.5))
            .text_color(theme::text_faint())
            .children(hints.map(|(key, label)| div().flex().flex_none().items_center().gap(px(5.)).child(kbd(key)).child(label)))
            .child(div().flex_1())
            .when_some(self.status.as_ref(), |d, (message, _)| {
                d.child(div().min_w_0().text_color(theme::calls_soft()).truncate().child(message.clone()))
            })
    }
}

impl Render for BrainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = now_ms();

        // Keep the selection on a visible session.
        let visible: Vec<SessionKey> =
            self.groups(now).navigable(self.include_ended()).iter().map(|s| s.key.clone()).collect();
        if self.selected.as_ref().is_none_or(|k| !visible.contains(k)) {
            self.selected = visible.first().cloned();
        }
        self.sync_conversation();
        self.sync_host();
        self.sync_projects();

        let groups = self.groups(now);
        let waiting_sessions: Vec<&Session> = groups
            .attention
            .iter()
            .chain(&groups.pinned)
            .copied()
            .filter(|s| matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn))
            .collect();
        let calling = waiting_sessions.iter().filter(|s| s.phase() == Phase::NeedsYou).count();
        let waiting = waiting_sessions.len();
        window.set_window_title(&if waiting > 0 { tr!("Brain – {waiting} warten", "Brain – {waiting} waiting") } else { "Brain".into() });
        let titlebar = self.render_titlebar(&groups, cx).into_any_element();
        let list = self.render_list(&groups, now, cx).into_any_element();
        let detail = self.render_detail(now, cx);
        drop(waiting_sessions);
        drop(groups);
        self.pending_scroll = None;
        if let Some(menubar) = self.menubar.as_mut() {
            menubar.show(waiting, calling);
        }

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

/// A segmented control; `on_pick` gets the picked option's value.
fn segmented<T: Clone + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, SharedString)>,
    current: T,
    on_pick: impl Fn(&T, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let on_pick = std::rc::Rc::new(on_pick);
    div()
        .flex()
        .flex_none()
        .p(px(3.))
        .gap(px(2.))
        .rounded(px(8.))
        .bg(theme::ink())
        .border_1()
        .border_color(theme::line())
        .children(options.into_iter().enumerate().map(move |(ix, (value, label))| {
            let active = value == current;
            let on_pick = on_pick.clone();
            div()
                .id((id, ix))
                .px(px(11.))
                .py(px(3.))
                .rounded(px(5.))
                .text_size(px(12.))
                .cursor_pointer()
                .text_color(if active { theme::text_strong() } else { theme::text_muted() })
                .when(active, |d| d.bg(theme::raised()).font_weight(FontWeight::MEDIUM))
                .when(!active, |d| d.hover(|d| d.text_color(theme::text())))
                .on_click(move |_: &ClickEvent, window, cx| on_pick(&value, window, cx))
                .child(label)
        }))
}

/// A labelled button with its key; `strong` fills it with the alert colour.
fn button(
    id: &'static str,
    label: &str,
    key: &str,
    strong: bool,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(30.))
        .px(px(12.))
        .rounded(px(8.))
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_strong())
        .when(strong, |d| d.bg(theme::calls()).hover(|d| d.bg(theme::calls_soft())))
        .when(!strong, |d| d.bg(theme::raised()).border_1().border_color(theme::line_strong()).hover(|d| d.bg(theme::line_strong())))
        .when(enabled, |d| d.cursor_pointer().on_click(on_click))
        .when(!enabled, |d| d.opacity(0.4))
        .child(label.to_string())
        .child(kbd(key.to_string()))
        .into_any_element()
}

fn marker(text: String, color: gpui::Rgba) -> AnyElement {
    div().flex_none().text_size(px(11.)).text_color(color).child(text).into_any_element()
}

/// The phase colour as a bar along the left edge of a rounded container.
fn rail(color: gpui::Rgba, radius: f32) -> impl IntoElement {
    div().absolute().left(px(-1.)).top(px(-1.)).bottom(px(-1.)).w(px(4.)).rounded_l(px(radius)).bg(color)
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

/// `claude-opus-5-5` → `opus-5-5`, `claude-haiku-4-5-20251001` → `haiku-4-5`.
fn short_model(model: &str) -> String {
    let name = model.strip_prefix("claude-").unwrap_or(model);
    name.split('-').filter(|p| !(p.len() == 8 && p.chars().all(|c| c.is_ascii_digit()))).collect::<Vec<_>>().join("-")
}

/// 23012 → `23k`, 1_340_000 → `1.3M`.
fn format_tokens(tokens: u64) -> String {
    match tokens {
        0..=999 => tokens.to_string(),
        1_000..=999_999 => format!("{}k", (tokens + 500) / 1000),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_and_token_counts_are_shortened() {
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "haiku-4-5");
        assert_eq!(short_model("claude-opus-5-5"), "opus-5-5");
        assert_eq!(format_tokens(23_012), "23k");
        assert_eq!(format_tokens(1_340_000), "1.3M");
        assert_eq!(format_tokens(812), "812");
    }
}
