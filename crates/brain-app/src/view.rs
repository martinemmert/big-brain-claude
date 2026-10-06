use std::collections::HashMap;
use std::time::{Duration, Instant};

use brain_core::state::{Phase, Session, SessionKey};
use brain_core::transcript::Role;
use gpui::{
    div, prelude::*, px, relative, AnyElement, BoxShadow, ClickEvent, Context, Decorations, FocusHandle,
    FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollHandle, SharedString,
    Task, Window, WindowControls,
};

use crate::config;
use crate::conversation::Conversation;
use crate::i18n::t;
use crate::input::{primary, primary_label, shift_label, InputAction, LineInput};
use crate::menubar::MenuBar;
use crate::model::Model;
use crate::notify::{self, Notifier};
use crate::prefs::{Layout, Prefs};
use brain_terminal::{self as terminal, Capabilities, Key, Limit, Outcome};
use crate::window_frame;
use crate::widgets::{
    account_badge, background_summary, caret, chip, clock, dot, kbd, now_ms, phase_color, phase_label, plain, section_title,
};
use crate::{messages, theme, tr};

/// Sessions on "your turn" for longer than this move to the "resting" section.
const RESTING_AFTER_MS: i64 = 2 * 60 * 60 * 1000;
/// Ended sessions stay listed this long (they are read from the event history).
const SNOOZE_MS: i64 = 15 * 60 * 1000;
/// How long a failed terminal action stays on screen unless clicked away.
const ERROR_SECS: u64 = 30;

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
    /// A failed terminal action; stays until clicked away or `ERROR_SECS` pass.
    error: Option<(String, Instant)>,
    /// Start of a press in the title bar; dragging moves the window (own frame only).
    titlebar_press: Option<Point<Pixels>>,
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
    /// Reminders sent while a session keeps waiting: since when it waits, and how many.
    reminders: HashMap<SessionKey, (i64, u32)>,
    /// The terminal hosting the selected session and what Brain can do with it.
    host: Option<(SessionKey, Capabilities, Option<Limit>)>,
    /// Project name and worktree per session, refreshed on render.
    projects: HashMap<SessionKey, (String, Option<String>)>,
    /// The checked-out working tree per session (for conflicts and PR lookups).
    checkouts: HashMap<SessionKey, std::path::PathBuf>,
    /// The PR of each session's branch, refreshed every two minutes in the background.
    prs: HashMap<SessionKey, brain_core::github::PullRequest>,
    _pr_check: Option<Task<()>>,
    /// Sessions editing the same files or checkout, computed on render.
    conflicts: HashMap<SessionKey, brain_core::conflicts::Conflict>,
    /// Pairs already notified about shared files.
    conflicts_notified: std::collections::HashSet<(SessionKey, SessionKey)>,
    changes: Option<ChangesCache>,
    changes_loading: bool,
    new_session: NewSession,
    /// `A` or `X` was pressed once on this session: a second press within 5 s acts.
    armed: Option<(char, SessionKey, Instant)>,
    /// A newer release on GitHub, if the daily check found one.
    update: Option<crate::links::Update>,
    _update_check: Option<Task<()>>,
    _poll: Task<()>,
}

/// Where typed keys go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Search,
    Rename,
    Reply,
    NewSession,
}

/// The "new session" dialog: a folder (picked from recent ones or typed) and an account.
#[derive(Default)]
struct NewSession {
    /// The search field, or the value of the placeholder being asked for.
    folder: LineInput,
    account: usize,
    pick: usize,
    templates: Vec<brain_core::templates::Template>,
    /// The picked template and its placeholder values so far.
    chosen: Option<Chosen>,
}

struct Chosen {
    template: brain_core::templates::Template,
    values: Vec<(String, String)>,
}

/// One row of the new-session list.
#[derive(Clone)]
enum Choice {
    Template(usize),
    Folder(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Messages,
    Timeline,
    Changes,
}

/// The selected session's git changes, loaded off the UI thread.
struct ChangesCache {
    key: SessionKey,
    loaded: Instant,
    changes: Option<brain_core::changes::Changes>,
}

/// The left column, already filtered and sorted.
struct Groups<'a> {
    /// Ended sessions the user marked with `P` to resume later.
    saved: Vec<&'a Session>,
    pinned: Vec<&'a Session>,
    attention: Vec<&'a Session>,
    working: Vec<&'a Session>,
    resting: Vec<&'a Session>,
    snoozed: Vec<&'a Session>,
    ended: Vec<&'a Session>,
}

impl<'a> Groups<'a> {
    fn navigable(&self, include_ended: bool) -> Vec<&'a Session> {
        let mut all: Vec<&Session> = self.saved.clone();
        all.extend(self.live());
        if include_ended {
            all.extend(&self.ended);
        }
        all
    }

    fn live(&self) -> Vec<&'a Session> {
        let mut all: Vec<&Session> = Vec::new();
        all.extend(&self.pinned);
        all.extend(&self.attention);
        all.extend(&self.working);
        all.extend(&self.resting);
        all.extend(&self.snoozed);
        all
    }

    fn live_count(&self) -> usize {
        self.pinned.len() + self.attention.len() + self.working.len() + self.resting.len() + self.snoozed.len()
    }
}

impl BrainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut model = Model::load();
        model.refresh();
        let mut prefs = Prefs::load();
        if prefs.migrate_pid_entries(|account, pid| Some(model.board.live_by_pid(account, pid)?.key.clone())) {
            prefs.save();
        }

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
            prefs,
            notifier: Notifier::new(),
            menubar: MenuBar::new(),
            focus,
            filter: None,
            selected: None,
            show_ended: false,
            status: None,
            error: None,
            titlebar_press: None,
            list_scroll: ScrollHandle::new(),
            tab: DetailTab::Messages,
            conversation: None,
            messages_scroll: ScrollHandle::new(),
            mode: Mode::Normal,
            search: LineInput::default(),
            rename: LineInput::default(),
            reply: LineInput::default(),
            pending_scroll: None,
            reminders: HashMap::new(),
            host: None,
            projects: HashMap::new(),
            checkouts: HashMap::new(),
            prs: HashMap::new(),
            _pr_check: None,
            conflicts: HashMap::new(),
            conflicts_notified: std::collections::HashSet::new(),
            changes: None,
            changes_loading: false,
            new_session: NewSession::default(),
            armed: None,
            update: None,
            _update_check: None,
            _poll: poll,
        };
        view.selected = view.groups(now_ms()).navigable(false).first().map(|s| s.key.clone());
        if !view.model.is_demo() {
            view._pr_check = Some(cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let Ok(work) = this.update(cx, |this, _| this.pr_lookups()) else { break };
                let found = cx.background_executor().spawn(async move { lookup_prs(work) }).await;
                if this.update(cx, |this, cx| {
                    this.prs = found;
                    cx.notify();
                })
                .is_err()
                {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(117)).await;
            }));
            view._update_check = Some(cx.spawn(async move |this, cx| loop {
                let found = cx.background_executor().spawn(async { crate::links::check_for_update() }).await;
                if this.update(cx, |this, cx| {
                    this.update = found;
                    cx.notify();
                })
                .is_err()
                {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(24 * 60 * 60)).await;
            }));
        }
        view
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        for item in self.model.refresh() {
            if self.prefs.snoozed_until(&item.key, now).is_some() || self.prefs.is_muted(&item.key) {
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
                    self.prefs.snooze(&key, Some(now + SNOOZE_MS));
                    self.prefs.save();
                }
            }
        }
        self.remind(now);
        self.notify_conflicts();
        self.load_changes(cx);
        let linked: Vec<SessionKey> = crate::links::take_sessions()
            .into_iter()
            .filter_map(|(account, pid)| Some(self.model.board.live_by_pid(&account, pid)?.key.clone()))
            .collect();
        for key in linked {
            self.selected = Some(key.clone());
            self.pending_scroll = Some(key);
            self.mode = Mode::Normal;
            cx.activate(true);
        }
        if MenuBar::take_click() {
            cx.activate(true);
        }

        if self.status.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(6)) {
            self.status = None;
        }
        if self.error.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(ERROR_SECS)) {
            self.error = None;
        }
        cx.notify();
    }

    /// Reminds again about sessions that keep waiting: every `remind_after_minutes`, at most
    /// three times per waiting period, never for muted or snoozed ones.
    fn remind(&mut self, now: i64) {
        const MAX_REMINDERS: u32 = 3;
        let Some(interval) = config::remind_after_ms() else { return };
        let waiting: Vec<(SessionKey, i64, Phase, String, Option<String>)> = self
            .model
            .board
            .sorted()
            .into_iter()
            .filter(|s| matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn))
            .map(|s| (s.key.clone(), s.phase_since_ms(), s.phase(), s.display_name(), s.headline()))
            .collect();
        self.reminders.retain(|key, _| waiting.iter().any(|(k, ..)| k == key));
        for (key, since, phase, name, headline) in waiting {
            if self.prefs.is_muted(&key) || self.prefs.snoozed_until(&key, now).is_some() {
                continue;
            }
            let entry = self.reminders.entry(key.clone()).or_insert((since, 0));
            if entry.0 != since {
                *entry = (since, 0);
            }
            let due = since + interval * (entry.1 as i64 + 1);
            if entry.1 >= MAX_REMINDERS || now < due {
                continue;
            }
            entry.1 += 1;
            let waited = theme::ago(since, now);
            let account = key.account.clone();
            let subtitle = tr!("{account} · wartet seit {waited}", "{account} · waiting for {waited}");
            let body = plain(headline.as_deref().unwrap_or(""));
            self.notifier.post(&key, &name, &subtitle, &body, phase == Phase::NeedsYou);
        }
    }

    /// Live sessions with a folder, for the background PR lookup.
    fn pr_lookups(&self) -> Vec<(SessionKey, String)> {
        self.model
            .board
            .keys()
            .filter_map(|k| {
                let s = self.model.board.get(k)?;
                (s.phase() != Phase::Ended).then(|| Some((k.clone(), s.cwd.clone()?))).flatten()
            })
            .collect()
    }

    /// Edited files per live session, checked against each other.
    fn compute_conflicts(&self) -> HashMap<SessionKey, brain_core::conflicts::Conflict> {
        let work: Vec<brain_core::conflicts::Work> = self
            .model
            .board
            .keys()
            .filter_map(|k| self.model.board.get(k))
            .filter(|s| s.phase() != Phase::Ended)
            .map(|s| brain_core::conflicts::Work {
                key: s.key.clone(),
                checkout: self.checkouts.get(&s.key).cloned(),
                edited: &s.insight.edited,
            })
            .collect();
        brain_core::conflicts::conflicts(&work)
    }

    /// "Edits the same files as X: a.rs, b.rs" and "Works in the same checkout as Y".
    fn conflict_lines(&self, s: &Session) -> Vec<AnyElement> {
        let Some(conflict) = self.conflicts.get(&s.key) else { return Vec::new() };
        let name = |key: &SessionKey| self.model.board.get(key).map(|o| o.display_name()).unwrap_or_default();
        let mut out = Vec::new();
        for (other, files) in &conflict.shared_files {
            let other = name(other);
            let list = files.iter().map(|f| f.rsplit('/').next().unwrap_or(f)).collect::<Vec<_>>().join(", ");
            out.push(
                div()
                    .text_size(px(12.))
                    .line_height(relative(1.45))
                    .text_color(theme::calls_soft())
                    .child(tr!("⚠ Bearbeitet dieselben Dateien wie {other}: {list}", "⚠ Edits the same files as {other}: {list}"))
                    .into_any_element(),
            );
        }
        if !conflict.same_checkout.is_empty() {
            let others = conflict.same_checkout.iter().map(name).collect::<Vec<_>>().join(", ");
            out.push(
                div()
                    .text_size(px(12.))
                    .text_color(theme::turn())
                    .child(tr!("Ändert Dateien im selben Checkout wie {others}", "Changes files in the same checkout as {others}"))
                    .into_any_element(),
            );
        }
        out
    }

    /// One notification per pair of sessions that start editing the same files.
    fn notify_conflicts(&mut self) {
        let found = self.compute_conflicts();
        let mut posts = Vec::new();
        for (key, conflict) in &found {
            for (other, files) in &conflict.shared_files {
                let pair = if key < other { (key.clone(), other.clone()) } else { (other.clone(), key.clone()) };
                if self.conflicts_notified.insert(pair) {
                    posts.push((key.clone(), other.clone(), files.clone()));
                }
            }
        }
        for (key, other, files) in posts {
            let name = self.model.board.get(&key).map(|s| s.display_name()).unwrap_or_default();
            let other_name = self.model.board.get(&other).map(|s| s.display_name()).unwrap_or_default();
            let list = files.iter().map(|f| f.rsplit('/').next().unwrap_or(f)).collect::<Vec<_>>().join(", ");
            let subtitle = tr!("bearbeitet dieselben Dateien wie {other_name}", "edits the same files as {other_name}");
            self.notifier.post(&key, &name, &subtitle, &list, false);
        }
    }

    /// While the Changes tab is open: reloads the selected session's git changes every 5 s.
    fn load_changes(&mut self, cx: &mut Context<Self>) {
        if self.tab != DetailTab::Changes || self.changes_loading || self.model.is_demo() {
            return;
        }
        let Some(session) = self.selected_session() else { return };
        let fresh = self.changes.as_ref().is_some_and(|c| c.key == session.key && c.loaded.elapsed() < Duration::from_secs(5));
        let Some(cwd) = session.cwd.clone().filter(|_| !fresh) else { return };
        let key = session.key.clone();
        self.changes_loading = true;
        let task = cx.background_executor().spawn(async move { brain_core::changes::changes_of(std::path::Path::new(&cwd)) });
        cx.spawn(async move |this, cx| {
            let changes = task.await;
            let _ = this.update(cx, |this, cx| {
                this.changes = Some(ChangesCache { key, loaded: Instant::now(), changes });
                this.changes_loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn open_new_session_dialog(&mut self) {
        let templates = brain_core::templates::load_all(&brain_core::templates::templates_dir(&brain_core::account::home_dir()));
        self.new_session = NewSession { templates, ..NewSession::default() };
        self.mode = Mode::NewSession;
    }

    /// Folders to start a session in: the typed path first (if it is one), then the folders of
    /// known sessions, most recently active first, filtered by the typed words.
    fn folder_suggestions(&self, query: &str) -> Vec<String> {
        let mut sessions: Vec<&Session> = self.model.board.keys().filter_map(|k| self.model.board.get(k)).collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity_ms));
        let mut folders: Vec<String> = Vec::new();
        if query.starts_with('/') || query.starts_with('~') {
            let home = brain_core::account::home_dir().display().to_string();
            folders.push(query.replacen('~', &home, 1));
        }
        let terms: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
        for cwd in sessions.iter().filter_map(|s| s.cwd.clone()) {
            let lower = cwd.to_lowercase();
            if !folders.contains(&cwd) && terms.iter().all(|t| lower.contains(t)) {
                folders.push(cwd);
            }
        }
        folders.truncate(12);
        folders
    }

    /// What the dialog lists right now: templates and folders while picking, only folders once a
    /// template without a folder is chosen, nothing while a placeholder is asked for.
    fn new_session_choices(&self) -> Vec<Choice> {
        let query = self.new_session.folder.text.trim().to_string();
        match &self.new_session.chosen {
            Some(chosen) if self.pending_placeholder().is_some() || chosen.template.folder.is_some() => Vec::new(),
            Some(_) => self.folder_suggestions(&query).into_iter().map(Choice::Folder).collect(),
            None => {
                let lower = query.to_lowercase();
                let templates = self
                    .new_session
                    .templates
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| lower.split_whitespace().all(|w| t.name.to_lowercase().contains(w)))
                    .map(|(i, _)| Choice::Template(i));
                templates.chain(self.folder_suggestions(&query).into_iter().map(Choice::Folder)).collect()
            }
        }
    }

    /// The next placeholder of the chosen template that has no value yet.
    fn pending_placeholder(&self) -> Option<String> {
        let chosen = self.new_session.chosen.as_ref()?;
        chosen.template.placeholders().into_iter().find(|name| !chosen.values.iter().any(|(n, _)| n == name))
    }

    fn on_new_session_key(&mut self, keystroke: &gpui::Keystroke, cx: &mut Context<Self>) {
        let m = &keystroke.modifiers;
        if primary(m) && keystroke.key == "e" {
            if let Some(Choice::Template(i)) = self.new_session_choices().get(self.new_session.pick).cloned() {
                open_in_editor(&self.new_session.templates[i].path);
            }
            return;
        }
        if primary(m) && m.shift && keystroke.key == "n" {
            let dir = brain_core::templates::templates_dir(&brain_core::account::home_dir());
            let prompt = t(
                "Beschreibe hier die Aufgabe. Platzhalter wie {ticket} fragt Brain beim Start ab.",
                "Describe the task here. Brain asks for placeholders like {ticket} when it starts.",
            );
            match brain_core::templates::create(&dir, t("Neue Vorlage", "New template"), prompt) {
                Ok(path) => {
                    open_in_editor(&path);
                    let cmd = primary_label();
                    self.set_status(tr!("Vorlage angelegt – nach dem Speichern {cmd}N erneut öffnen.", "Template created – reopen {cmd}N after saving it."));
                    self.mode = Mode::Normal;
                }
                Err(err) => self.set_status(err.to_string()),
            }
            return;
        }
        let count = self.new_session_choices().len();
        match keystroke.key.as_str() {
            "down" => self.new_session.pick = (self.new_session.pick + 1).min(count.saturating_sub(1)),
            "up" => self.new_session.pick = self.new_session.pick.saturating_sub(1),
            "tab" => self.new_session.account = (self.new_session.account + 1) % self.model.accounts.len().max(1),
            "enter" => self.confirm_new_session(),
            "escape" => self.mode = Mode::Normal,
            _ => {
                let clipboard = || cx.read_from_clipboard().and_then(|item| item.text());
                if self.new_session.folder.handle(keystroke, clipboard) == InputAction::Changed {
                    self.new_session.pick = 0;
                }
            }
        }
    }

    /// ⏎ in the dialog: pick a template or folder, take a placeholder value, or start.
    fn confirm_new_session(&mut self) {
        if let Some(name) = self.pending_placeholder() {
            let value = self.new_session.folder.text.trim().to_string();
            if let Some(chosen) = self.new_session.chosen.as_mut() {
                chosen.values.push((name, value));
            }
            self.new_session.folder = LineInput::default();
            self.new_session.pick = 0;
            let has_folder = self.new_session.chosen.as_ref().is_some_and(|c| c.template.folder.is_some());
            if self.pending_placeholder().is_none() && has_folder {
                self.start_new_session(None);
            }
            return;
        }
        match self.new_session_choices().get(self.new_session.pick).cloned() {
            Some(Choice::Template(i)) => {
                let template = self.new_session.templates[i].clone();
                if let Some(index) = template.account.as_ref().and_then(|id| self.model.accounts.iter().position(|a| a.id == *id)) {
                    self.new_session.account = index;
                }
                let ready = template.folder.is_some() && template.placeholders().is_empty();
                self.new_session.chosen = Some(Chosen { template, values: Vec::new() });
                self.new_session.folder = LineInput::default();
                self.new_session.pick = 0;
                if ready {
                    self.start_new_session(None);
                }
            }
            Some(Choice::Folder(folder)) => self.start_new_session(Some(folder)),
            None => self.set_status(t("Wähle einen Ordner oder tippe einen Pfad.", "Pick a folder or type a path.")),
        }
    }

    /// Opens a new tab running `claude` (with the template's model and filled-in prompt, if any).
    fn start_new_session(&mut self, folder: Option<String>) {
        let chosen = self.new_session.chosen.as_ref();
        let folder = folder.or_else(|| chosen.and_then(|c| c.template.folder.clone())).map(|f| {
            let home = brain_core::account::home_dir().display().to_string();
            match f.strip_prefix('~') {
                Some(rest) => format!("{home}{rest}"),
                None => f,
            }
        });
        let Some(folder) = folder else { return };
        if !std::path::Path::new(&folder).is_dir() {
            self.set_status(tr!("Ordner nicht gefunden: {folder}", "Folder not found: {folder}"));
            return;
        }
        let mut command = String::from("claude");
        if let Some(chosen) = chosen {
            if let Some(model) = &chosen.template.model {
                command.push_str(&format!(" --model {}", shell_quote(model)));
            }
            let prompt = chosen.template.fill(&chosen.values);
            if !prompt.is_empty() {
                command.push_str(&format!(" {}", shell_quote(&prompt)));
            }
        }
        self.mode = Mode::Normal;
        let config_dir = self
            .model
            .accounts
            .get(self.new_session.account)
            .filter(|a| a.id != "main")
            .map(|a| a.config_dir.display().to_string());
        if self.blocked_in_demo() {
            return;
        }
        let outcome = brain_terminal::open_new(&folder, config_dir.as_deref(), &command);
        self.report(outcome, Some(t("Neue Session gestartet.", "Started a new session.").into()));
    }

    fn render_new_session(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.new_session.folder.text.clone();
        let choices = self.new_session_choices();
        let accounts: Vec<(usize, SharedString)> =
            self.model.accounts.iter().enumerate().map(|(i, a)| (i, SharedString::from(a.id.clone()))).collect();
        let chosen = self.new_session.chosen.as_ref();
        let asking = self.pending_placeholder();
        let placeholder: String = match (&asking, chosen) {
            (Some(name), _) => tr!("Wert für {{{name}}} …", "Value for {{{name}}} …"),
            (None, Some(_)) => t("Ordner für die Vorlage suchen oder Pfad tippen …", "Search a folder for the template or type a path …").into(),
            (None, None) => t("Vorlage oder Ordner suchen, oder Pfad tippen …", "Search templates or folders, or type a path …").into(),
        };
        let title = match chosen {
            Some(c) => format!("{} · {}", t("Neue Session", "New session"), c.template.name),
            None => t("Neue Session", "New session").to_string(),
        };
        let preview = chosen.filter(|c| !c.template.prompt.is_empty()).map(|c| c.template.fill(&c.values));
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(14.))
            .h_full()
            .px(px(28.))
            .pt(px(24.))
            .bg(theme::ink())
            .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(theme::text_strong()).child(title))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(px(12.5)).text_color(theme::text_muted()).child(t("Konto", "Account")))
                    .child(segmented("new-account", accounts, self.new_session.account, cx.listener(|this, value: &usize, _, cx| {
                        this.new_session.account = *value;
                        cx.notify();
                    }))),
            )
            .when_some(preview, |d, prompt| {
                d.child(
                    div()
                        .px(px(12.))
                        .py(px(9.))
                        .rounded(px(8.))
                        .bg(theme::surface())
                        .border_1()
                        .border_color(theme::line())
                        .text_size(px(12.5))
                        .line_height(relative(1.5))
                        .line_clamp(6)
                        .text_color(theme::text())
                        .child(prompt),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(36.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::alpha(theme::working(), 0x99))
                    .when(query.is_empty(), |d| d.child(div().text_color(theme::text_faint()).child(placeholder)))
                    .child(div().text_color(theme::text_strong()).child(query))
                    .child(caret("new-caret")),
            )
            .child(div().flex().flex_col().gap(px(2.)).children(choices.into_iter().enumerate().map(|(i, choice)| {
                let active = i == self.new_session.pick;
                let row = div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .text_size(px(12.5))
                    .text_color(if active { theme::text_strong() } else { theme::text() })
                    .when(active, |d| d.bg(theme::raised()));
                match choice {
                    Choice::Template(index) => {
                        let template = &self.new_session.templates[index];
                        row.child(div().flex_none().text_color(theme::turn()).child("▸"))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(template.name.clone()))
                            .when_some(template.folder.clone(), |d, f| {
                                d.child(div().text_color(theme::text_faint()).font_family(theme::MONO).text_size(px(11.5)).child(f))
                            })
                    }
                    Choice::Folder(folder) => row.font_family(theme::MONO).text_size(px(12.)).child(theme::tilde(&folder)),
                }
            })))
            .child(div().text_size(px(11.5)).text_color(theme::text_faint()).child({
                let (cmd, shift) = (primary_label(), shift_label());
                tr!(
                    "⏎ wählen/starten · ↑↓ · ⇥ Konto · {cmd}E Vorlage bearbeiten · {cmd}{shift}N neue Vorlage · esc",
                    "⏎ pick/start · ↑↓ · ⇥ account · {cmd}E edit template · {cmd}{shift}N new template · esc"
                )
            }))
            .into_any_element()
    }

    /// The first press of a two-press action arms it; returns whether this press is the second,
    /// within 5 s, on the same session.
    fn arm(&mut self, action: char, key: &SessionKey) -> bool {
        let confirmed = self.armed.as_ref().is_some_and(|(a, k, at)| *a == action && k == key && at.elapsed() < Duration::from_secs(5));
        self.armed = if confirmed { None } else { Some((action, key.clone(), Instant::now())) };
        confirmed
    }

    /// `X` twice: ends a waiting session with `/exit`. It stays in the list to resume later.
    fn end_selected(&mut self) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        if session.phase() == Phase::Ended {
            self.set_status(t("Die Session ist schon beendet.", "The session has already ended."));
            return;
        }
        if !session.accepts_input() {
            self.set_status(t(
                "Beenden geht, sobald die Session fertig ist und auf dich wartet.",
                "Ending works once the session has finished and waits for you.",
            ));
            return;
        }
        if !self.arm('x', &key) {
            self.set_status(t("Nochmal X beendet die Session (fortsetzen mit ⏎).", "Press X again to end the session (⏎ resumes it)."));
            return;
        }
        if self.blocked_in_demo() {
            return;
        }
        self.type_into_selected("/exit", t("Session beendet – ⏎ setzt sie fort, P merkt sie dir.", "Session ended – ⏎ resumes it, P saves it for later.").into());
    }

    /// `A` twice: continues a session in the next account. The transcript is copied and resumed
    /// there in a new tab (`--fork-session`); a running original gets `/exit`.
    fn move_to_other_account(&mut self) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        let Some(index) = self.model.accounts.iter().position(|a| a.id == key.account) else { return };
        let next = self.model.accounts.get((index + 1) % self.model.accounts.len()).cloned();
        let Some(target) = next.filter(|t| t.id != key.account) else {
            self.set_status(t("Es gibt kein zweites Konto.", "There is no other account."));
            return;
        };
        let ended = session.phase() == Phase::Ended;
        if !ended && !session.accepts_input() {
            self.set_status(t(
                "Umziehen geht, sobald die Session fertig ist und auf dich wartet.",
                "Moving works once the session has finished and waits for you.",
            ));
            return;
        }
        let (session_id, cwd, pid) = (session.session_id.clone(), session.cwd.clone(), session.pid);
        if !self.arm('a', &key) {
            let target = target.id.clone();
            self.set_status(if ended {
                tr!("Nochmal A setzt die Session in {target} fort.", "Press A again to resume the session in {target}.")
            } else {
                tr!("Nochmal A zieht die Session nach {target} um.", "Press A again to move the session to {target}.")
            });
            return;
        }
        let (Some(session_id), Some(cwd)) = (session_id, cwd) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder."));
            return;
        };
        if self.blocked_in_demo() {
            return;
        }
        let transcript = self.model.account(&key.account).and_then(|a| brain_core::transcript::find_transcript(a, &session_id));
        let Some(transcript) = transcript else {
            self.set_status(t("Transkript nicht gefunden.", "Transcript not found."));
            return;
        };
        if let Err(err) = brain_core::transcript::copy_to_account(&transcript, &target) {
            self.set_status(tr!("Kopieren fehlgeschlagen: {err}", "Copy failed: {err}"));
            return;
        }
        let config_dir = (target.id != "main").then(|| target.config_dir.display().to_string());
        let outcome = brain_terminal::open_new(&cwd, config_dir.as_deref(), &format!("claude --resume {session_id} --fork-session"));
        if !matches!(outcome, Outcome::Done) {
            self.report(outcome, None);
            return;
        }
        if self.prefs.is_pinned(&key) && ended {
            self.prefs.toggle_pin(&key);
            self.prefs.save();
        }
        let target = target.id;
        if ended {
            self.set_status(tr!("In {target} fortgesetzt.", "Resumed in {target}."));
            return;
        }
        let closed = matches!(brain_terminal::type_text(pid, "/exit"), Outcome::Done);
        self.set_status(if closed {
            tr!("Nach {target} umgezogen, das Original ist geschlossen.", "Moved to {target}; the original is closed.")
        } else {
            tr!("Nach {target} umgezogen. Das Original konnte Brain nicht schließen.", "Moved to {target}. Brain could not close the original.")
        });
    }

    /// `S`: not snoozed → 15 min → 1 h → until tomorrow 9:00 → not snoozed.
    fn cycle_snooze(&mut self) {
        let Some(key) = self.selected.clone() else { return };
        let now = now_ms();
        let tomorrow = (chrono::Local::now() + chrono::Duration::days(1))
            .date_naive()
            .and_hms_opt(9, 0, 0)
            .and_then(|t| t.and_local_timezone(chrono::Local).single())
            .map(|t| t.timestamp_millis());
        let current = self.prefs.snoozed_until(&key, now);
        let quarter = now + 15 * 60 * 1000;
        let hour = now + 60 * 60 * 1000;
        let next = match current {
            None => Some(quarter),
            Some(until) if until <= quarter + 60_000 => Some(hour),
            Some(until) if until <= hour + 60_000 => tomorrow,
            Some(_) => None,
        };
        self.prefs.snooze(&key, next);
        self.prefs.save();
        self.set_status(match next {
            Some(until) => {
                let at = clock(until);
                tr!("Pausiert bis {at}.", "Snoozed until {at}.")
            }
            None => t("Nicht mehr pausiert.", "No longer snoozed.").to_string(),
        });
    }

    fn groups(&self, now: i64) -> Groups<'_> {
        let mut groups = Groups { saved: vec![], pinned: vec![], attention: vec![], working: vec![], resting: vec![], snoozed: vec![], ended: vec![] };
        let visible = self
            .model
            .board
            .sorted()
            .into_iter()
            .filter(|s| self.filter.as_ref().is_none_or(|f| *f == s.key.account))
            .filter(|s| s.matches(&self.search.text));
        for session in visible {
            if session.phase() == Phase::Ended && !self.model.resumable(session) {
                continue;
            }
            if self.prefs.is_pinned(&session.key) {
                if session.phase() == Phase::Ended {
                    groups.saved.push(session);
                } else {
                    groups.pinned.push(session);
                }
                continue;
            }
            let waiting = matches!(session.phase(), Phase::NeedsYou | Phase::YourTurn);
            if waiting && self.prefs.snoozed_until(&session.key, now).is_some() {
                groups.snoozed.push(session);
                continue;
            }
            match session.phase() {
                Phase::NeedsYou => groups.attention.push(session),
                Phase::YourTurn if now - session.phase_since_ms() > RESTING_AFTER_MS => groups.resting.push(session),
                Phase::YourTurn => groups.attention.push(session),
                Phase::Working | Phase::Background => groups.working.push(session),
                Phase::Ended => groups.ended.push(session),
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
            Some((key, caps, _)) if Some(key) == self.selected.as_ref() => *caps,
            _ => Capabilities { focus: false, type_text: false, keys: false },
        }
    }

    /// What to change so Brain can type into the selected session's terminal.
    fn typing_help(&self) -> Option<String> {
        match &self.host {
            Some((key, _, Some(limit))) if Some(key) == self.selected.as_ref() => Some(limit_help(limit)),
            _ => None,
        }
    }

    // ---- input -------------------------------------------------------------

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let clipboard = || cx.read_from_clipboard().and_then(|item| item.text());

        match self.mode {
            Mode::NewSession => {
                self.on_new_session_key(keystroke, cx);
                cx.stop_propagation();
                cx.notify();
                return;
            }
            Mode::Reply if self.reply.text.is_empty() && keystroke.key.len() == 1 && ('1'..='9').contains(&keystroke.key.chars().next().unwrap()) && !primary(&keystroke.modifiers) => {
                let index = keystroke.key.parse::<usize>().unwrap() - 1;
                if let Some(reply) = config::quick_replies().get(index).cloned() {
                    self.reply = LineInput::with_text(&reply);
                    self.submit_reply();
                }
                cx.stop_propagation();
                cx.notify();
                return;
            }
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

        if primary(&keystroke.modifiers) && keystroke.key == "f" {
            self.mode = Mode::Search;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if primary(&keystroke.modifiers) && keystroke.key == "n" {
            self.open_new_session_dialog();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if primary(&keystroke.modifiers) && keystroke.key == "c" && self.prefs.layout == Layout::Today {
            let date = chrono::Local::now().format("%d.%m.%Y").to_string();
            let markdown = brain_core::digest::markdown(&tr!("Heute, {date}", "Today, {date}"), &self.today_digest());
            crate::clipboard::copy(&markdown, cx);
            self.set_status(t("Tagesübersicht kopiert.", "Copied the day's digest."));
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
            "e" => {
                self.show_ended = !self.show_ended;
                if !self.show_ended && self.selected_session().is_some_and(|s| s.phase() == Phase::Ended) {
                    self.selected = None;
                }
            }
            "r" => self.start_rename(),
            "t" => self.start_reply(),
            "y" => self.answer_permission(true),
            "n" => self.answer_permission(false),
            "p" => self.toggle_pin(),
            "m" => self.toggle_mute(),
            "s" => self.cycle_snooze(),
            "a" => self.move_to_other_account(),
            "x" => self.end_selected(),
            "g" => {
                self.prefs.layout = if self.prefs.layout == Layout::Projects { Layout::Status } else { Layout::Projects };
                self.prefs.save();
            }
            "d" => {
                self.prefs.layout = if self.prefs.layout == Layout::Today { Layout::Status } else { Layout::Today };
                self.prefs.save();
            }
            "right" => {
                self.tab = match self.tab {
                    DetailTab::Messages => DetailTab::Timeline,
                    DetailTab::Timeline => DetailTab::Changes,
                    DetailTab::Changes => DetailTab::Messages,
                }
            }
            "left" => {
                self.tab = match self.tab {
                    DetailTab::Messages => DetailTab::Changes,
                    DetailTab::Timeline => DetailTab::Messages,
                    DetailTab::Changes => DetailTab::Timeline,
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

    fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some((message.into(), Instant::now()));
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
            Outcome::NoTerminal => self.set_error(t(
                "Kein Terminal zu dieser Session gefunden (beendet oder ohne Terminal gestartet).",
                "No terminal found for this session (it ended or runs without one).",
            )),
            Outcome::Unsupported(reason) => self.set_error(reason),
            // macOS asks once whether Brain may control iTerm2; a "no" is error -1743.
            Outcome::Failed(reason) if reason.contains("-1743") => self.set_error(t(
                "macOS erlaubt Brain nicht, iTerm2 zu steuern. Systemeinstellungen → Datenschutz & Sicherheit → Automation → Brain → „iTerm“ einschalten.",
                "macOS doesn't let Brain control iTerm2. System Settings → Privacy & Security → Automation → Brain → turn on “iTerm”.",
            )),
            Outcome::Failed(reason) => self.set_error(tr!("Fehlgeschlagen: {reason}", "Failed: {reason}")),
        }
    }

    /// ⏎: a live session's terminal comes forward; an ended one is resumed in a new tab.
    fn open_selected(&mut self) {
        let Some(session) = self.selected_session() else { return };
        if session.phase() == Phase::Ended {
            self.resume_selected();
            return;
        }
        let (pid, cwd) = (session.pid, session.cwd.clone());
        if self.blocked_in_demo() {
            return;
        }
        let outcome = terminal::focus(pid, cwd.as_deref());
        self.report(outcome, None);
    }

    fn resume_selected(&mut self) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
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
        if matches!(outcome, Outcome::Done) && self.prefs.is_pinned(&key) {
            self.prefs.toggle_pin(&key);
            self.prefs.save();
        }
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
            match self.typing_help() {
                Some(help) => self.set_error(help),
                None => self.set_status(t("In dieses Terminal kann Brain nicht tippen.", "Brain can't type into this terminal.")),
            }
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
            match self.typing_help() {
                Some(help) => self.set_error(help),
                None => self.set_status(t("In dieses Terminal kann Brain nicht tippen.", "Brain can't type into this terminal.")),
            }
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
        let Some(pid) = self.model.board.get(&key).map(|s| s.pid) else { return };
        let outcome = terminal::type_text(pid, text);
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
            match self.typing_help() {
                Some(help) => self.set_error(help),
                None => self.set_status(t("In diesem Terminal kann Brain keine Freigaben beantworten.", "Brain can't answer permissions in this terminal.")),
            }
            return;
        }
        let pid = session.pid;
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
        let ended = self.model.board.get(&key).is_some_and(|s| s.phase() == Phase::Ended);
        let pinned = self.prefs.toggle_pin(&key);
        self.prefs.save();
        self.pending_scroll = Some(key);
        self.set_status(match (pinned, ended) {
            (true, true) => t("Zum Fortsetzen gemerkt.", "Saved to resume."),
            (false, true) => t("Nicht mehr gemerkt.", "No longer saved."),
            (true, false) => t("Angeheftet.", "Pinned."),
            (false, false) => t("Nicht mehr angeheftet.", "Unpinned."),
        });
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
        if self.model.is_demo() || self.host.as_ref().is_some_and(|(k, _, _)| *k == key) {
            return;
        }
        let Some(pid) = self.model.board.get(&key).map(|s| s.pid) else { return };
        let host = terminal::host_of(pid);
        let caps = host
            .as_ref()
            .map(terminal::capabilities)
            .unwrap_or(Capabilities { focus: false, type_text: false, keys: false });
        let limit = host.as_ref().and_then(terminal::limit);
        self.host = Some((key, caps, limit));
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
            let checkout = project.checkout.clone();
            self.projects.insert(key.clone(), entry);
            self.checkouts.insert(key, checkout);
        }
    }

    // ---- rendering -------------------------------------------------------------

    /// `own_frame`: Brain draws the window frame, so the title bar moves the window.
    fn render_titlebar(&self, groups: &Groups, own_frame: Option<WindowControls>, cx: &mut Context<Self>) -> impl IntoElement {
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
            // Room for the traffic lights on macOS.
            .pl(px(if cfg!(target_os = "macos") { 86. } else { 14. }))
            .pr(px(14.))
            .gap(px(12.))
            .bg(theme::chrome())
            .border_b_1()
            .border_color(theme::line())
            .on_click(move |event: &ClickEvent, window, _| match own_frame {
                Some(_) if event.is_right_click() => window.show_window_menu(event.position()),
                Some(_) if event.click_count() == 2 => window.zoom_window(),
                None if event.click_count() == 2 => window.titlebar_double_click(),
                _ => {}
            })
            .when(own_frame.is_some(), |d| {
                // Move only once the pointer travels, so clicks on title bar controls still work.
                d.on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, _, _| {
                    this.titlebar_press = Some(event.position);
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.titlebar_press = None))
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, _| {
                    let Some(start) = this.titlebar_press else { return };
                    if !event.dragging() {
                        this.titlebar_press = None;
                    } else if (event.position.x - start.x).abs() + (event.position.y - start.y).abs() > px(4.) {
                        this.titlebar_press = None;
                        window.start_window_move();
                    }
                }))
            })
            .child(brand_mark(calling))
            .child(div().text_size(px(14.)).font_weight(FontWeight::BOLD).text_color(theme::text_strong()).child("Brain"))
            .child(div().text_size(px(12.5)).text_color(theme::text_muted()).child(summary))
            .child(div().flex_1())
            .when_some(self.update.clone(), |d, update| {
                let version = update.version.clone();
                d.child(
                    div()
                        .id("update")
                        .flex_none()
                        .px(px(9.))
                        .py(px(3.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .bg(theme::alpha(theme::done(), 0x22))
                        .text_size(px(12.))
                        .text_color(theme::done())
                        .on_click(move |_: &ClickEvent, _, cx| cx.open_url(&update.url))
                        .child(tr!("Update {version}", "Update {version}")),
                )
            })
            .child(segmented(
                "layout",
                vec![
                    (Layout::Status, SharedString::from(t("Status", "Status"))),
                    (Layout::Projects, SharedString::from(t("Projekte", "Projects"))),
                    (Layout::Today, SharedString::from(t("Heute", "Today"))),
                ],
                self.prefs.layout,
                cx.listener(|this, value: &Layout, _, cx| {
                    this.prefs.layout = *value;
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
            .when_some(own_frame, |d, controls| d.child(window_frame::window_buttons(controls)))
    }

    fn render_list(&self, groups: &Groups, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let mut children: Vec<AnyElement> = vec![self.render_search(cx)];
        let searching = !self.search.text.is_empty();

        if searching && groups.navigable(true).is_empty() {
            let query = self.search.text.clone();
            children.push(hint(tr!("Keine Session passt zu „{query}“. Esc leert die Suche.", "No session matches “{query}”. Esc clears the search.")));
        }

        let mut nav = 0usize;
        if !groups.saved.is_empty() {
            children.push(section_title(t("Zum Fortsetzen gemerkt", "Saved to resume"), groups.saved.len(), false));
            for session in &groups.saved {
                self.push_session(session, &mut nav, now, cx, &mut children);
            }
        }
        if self.prefs.layout != Layout::Status {
            self.render_by_project(groups, now, cx, &mut nav, &mut children);
        } else {
            self.render_by_status(groups, now, cx, &mut nav, &mut children);
        }
        self.push_ended(groups, &mut nav, now, cx, &mut children);

        div()
            .id("session-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px(px(12.))
            .pt(px(12.))
            .pb(px(16.))
            .gap(px(6.))
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .children(children)
    }

    /// Plan limits per account (from the status line) and today's cost, under the list.
    fn render_usage(&self, now: i64) -> Option<AnyElement> {
        let today = chrono::Local::now().date_naive();
        let rows: Vec<AnyElement> = self
            .model
            .accounts
            .iter()
            .filter_map(|account| {
                let limits = brain_core::usage::account_limits(&self.model.usage, &account.id);
                let cost: f64 = self
                    .model
                    .usage
                    .iter()
                    .filter(|s| s.account == account.id)
                    .filter(|s| chrono::DateTime::from_timestamp_millis(s.ts).is_some_and(|t| t.with_timezone(&chrono::Local).date_naive() == today))
                    .filter_map(|s| s.cost_usd)
                    .sum();
                if limits.is_none() && cost == 0.0 {
                    return None;
                }
                let mut row = div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(account_badge(&account.id, self.account_index(&account.id)));
                if let Some(snapshot) = limits {
                    for (label, limit) in [("5h", snapshot.five_hour), ("7d", snapshot.seven_day)] {
                        if let Some(limit) = limit {
                            row = row.child(limit_bar(label, limit, now));
                        }
                    }
                }
                if cost > 0.0 {
                    row = row.child(div().flex_1()).child(
                        // Claude Code's per-session totals at API prices: a measure of use, not a bill.
                        div().flex_none().text_size(px(11.5)).text_color(theme::text_muted()).child(tr!("≈ ${cost:.0} API-Wert", "≈ ${cost:.0} API value")),
                    );
                }
                Some(row.into_any_element())
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .gap(px(8.))
                .px(px(16.))
                .py(px(10.))
                .border_t_1()
                .border_color(theme::line())
                .child(div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::text_faint()).child(t("Nutzung", "Usage")))
                .children(rows)
                .into_any_element(),
        )
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
            (t("Pausiert", "Snoozed"), &groups.snoozed),
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
        let mut live = groups.live();
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
        let snoozed = self.prefs.snoozed_until(&s.key, now).is_some();
        let waiting = matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn) && !snoozed;
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
        if let Some(pr) = self.prs.get(&s.key) {
            out.push(pr_marker(pr));
        }
        if self.conflicts.get(&s.key).is_some_and(|c| !c.shared_files.is_empty()) {
            out.push(marker(t("⚠ Konflikt", "⚠ conflict").into(), theme::calls()));
        }
        if self.prefs.is_pinned(&s.key) {
            out.push(marker("★".into(), theme::turn()));
        }
        if self.prefs.is_muted(&s.key) {
            out.push(marker(t("stumm", "muted").into(), theme::text_faint()));
        }
        if let Some(until) = self.prefs.snoozed_until(&s.key, now_ms()) {
            let at = clock(until);
            out.push(marker(tr!("pausiert bis {at}", "snoozed until {at}"), theme::text_faint()));
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
            Phase::Background => background_summary(s.background_tasks()),
            Phase::YourTurn => {
                let ago = theme::ago(s.phase_since_ms(), now);
                tr!("fertig seit {ago}", "done for {ago}")
            }
            Phase::Ended => {
                let ago = theme::ago_phrase(s.last_activity_ms, now);
                match self.model.days_left(s, now) {
                    Some(days) => tr!("{ago} · noch {days} T fortsetzbar", "{ago} · resumable for {days} more days"),
                    None => ago,
                }
            }
            _ => theme::ago_phrase(s.last_activity_ms, now),
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
        let status = self.model.snapshot(&s.key);
        if let Some(model) = &info.model {
            meta.push(chip(short_model(model), false).into_any_element());
        }
        // The status line knows the real fill level; the transcript only the token count.
        if let Some(percent) = status.and_then(|s| s.context_percent) {
            meta.push(chip(tr!("{percent:.0}% Kontext", "{percent:.0}% context"), false).into_any_element());
        } else if let Some(tokens) = info.context_tokens {
            let tokens = format_tokens(tokens);
            meta.push(chip(tr!("{tokens} Kontext", "{tokens} context"), false).into_any_element());
        }
        if let Some(cost) = status.and_then(|s| s.cost_usd).or(info.cost_usd) {
            meta.push(chip(format!("${cost:.2}"), false).into_any_element());
        }
        if let Some(mode) = info.permission_mode.as_deref().filter(|m| *m != "default") {
            meta.push(chip(mode.to_string(), false).into_any_element());
        }
        if s.alive {
            meta.push(chip(format!("pid {}", s.pid), false).into_any_element());
        } else if let Some(days) = self.model.days_left(s, now_ms()) {
            meta.push(chip(tr!("noch {days} Tage fortsetzbar", "resumable for {days} more days"), false).into_any_element());
        }
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
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .mt(px(10.))
                    .mb(px(18.))
                    .children(self.meta_chips(s))
                    .when_some(self.prs.get(&s.key).cloned(), |d, pr| d.child(pr_chip(pr))),
            )
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
                DetailTab::Changes => self.render_changes(s),
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
        let tasks: Vec<AnyElement> = if phase == Phase::Background {
            s.background_tasks()
                .iter()
                .map(|task| {
                    let what = task.description.clone().unwrap_or_else(|| task.id.clone());
                    let kind = task.agent_type.clone().unwrap_or_else(|| task.kind.clone());
                    div()
                        .flex()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .child(div().flex_none().text_color(theme::background()).child(kind))
                        .child(div().min_w_0().truncate().text_color(theme::text()).child(what))
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };

        let mut actions: Vec<AnyElement> = Vec::new();
        // Shown under greyed-out buttons: what to change in the terminal.
        let mut help = None;
        if s.awaiting_permission() {
            help = (!caps.keys).then(|| self.typing_help()).flatten();
            actions.push(button("allow", t("Erlauben", "Allow"), "Y", true, caps.keys, cx.listener(|this, _: &ClickEvent, _, cx| {
                this.answer_permission(true);
                cx.notify();
            })));
            actions.push(button("deny", t("Ablehnen", "Deny"), "N", false, caps.keys, cx.listener(|this, _: &ClickEvent, _, cx| {
                this.answer_permission(false);
                cx.notify();
            })));
        } else if s.accepts_input() && self.mode != Mode::Reply {
            help = (!caps.type_text).then(|| self.typing_help()).flatten();
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
            .children(self.conflict_lines(s))
            .when(!tasks.is_empty(), |d| {
                d.child(div().text_size(px(12.)).text_color(theme::text_muted()).child(background_summary(s.background_tasks())))
                    .children(tasks)
            })
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
                .when(self.reply.text.is_empty(), |d| {
                    d.child(div().flex().flex_col().items_start().gap(px(4.)).children(config::quick_replies().into_iter().enumerate().map(|(i, reply)| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .bg(theme::surface())
                            .border_1()
                            .border_color(theme::line())
                            .text_size(px(12.))
                            .child(kbd(format!("{}", i + 1)))
                            .child(div().text_color(theme::text()).child(reply))
                    })))
                })
            })
            .when(!actions.is_empty(), |d| d.child(div().flex().gap(px(8.)).mt(px(2.)).children(actions)))
            .when_some(help, |d, help| {
                d.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .text_size(px(11.5))
                        .line_height(relative(1.5))
                        .text_color(theme::text_muted())
                        .child(div().flex_none().text_color(theme::text_faint()).child("ⓘ"))
                        .child(div().flex_1().min_w_0().child(help)),
                )
            })
            .into_any_element()
    }

    fn render_tabs(&self, s: &Session, cx: &mut Context<Self>) -> impl IntoElement {
        let message_count = self.conversation.as_ref().map_or(0, |c| c.messages.len());
        let change_count = self
            .changes
            .as_ref()
            .filter(|c| c.key == s.key)
            .and_then(|c| c.changes.as_ref())
            .map_or(0, |c| c.files.len());
        let tabs = [
            (DetailTab::Messages, "messages-tab", t("Nachrichten", "Messages"), message_count),
            (DetailTab::Timeline, "timeline-tab", t("Verlauf", "Timeline"), s.timeline.len()),
            (DetailTab::Changes, "changes-tab", t("Änderungen", "Changes"), change_count),
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

    fn render_changes(&self, s: &Session) -> AnyElement {
        let cache = self.changes.as_ref().filter(|c| c.key == s.key);
        let body: Vec<AnyElement> = match cache.map(|c| c.changes.as_ref()) {
            None => vec![hint(t("Lade Änderungen …", "Loading changes …").into())],
            Some(None) => vec![hint(t("Der Ordner dieser Session ist kein Git-Repository.", "This session's folder is not a git repository.").into())],
            Some(Some(changes)) => {
                let (added, removed) = changes.totals();
                let mut head = div().flex().flex_wrap().items_center().gap(px(6.)).pb(px(6.));
                if let Some(branch) = &changes.branch {
                    head = head.child(chip(format!("⑂ {branch}"), true));
                }
                if let Some((ahead, behind)) = changes.ahead_behind {
                    head = head.child(chip(format!("↑{ahead} ↓{behind}"), false));
                }
                let files = changes.files.len();
                head = head.child(chip(tr!("{files} Dateien · +{added} −{removed}", "{files} files · +{added} −{removed}"), false));
                let mut out = vec![head.into_any_element()];
                if changes.files.is_empty() {
                    out.push(hint(t("Keine offenen Änderungen.", "No uncommitted changes.").into()));
                }
                out.extend(changes.files.iter().map(change_row));
                out
            }
        };
        div()
            .id("changes")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            .pt(px(14.))
            .pb(px(20.))
            .overflow_y_scroll()
            .children(body)
            .into_any_element()
    }

    /// The day's digest: what each session reported as done, per project.
    fn today_digest(&self) -> Vec<brain_core::digest::ProjectDay> {
        let sessions = self
            .model
            .board
            .keys()
            .filter(|k| self.filter.as_ref().is_none_or(|f| *f == k.account))
            .filter_map(|k| {
                let session = self.model.board.get(k)?;
                let project = self.projects.get(k).map(|(name, _)| name.clone()).unwrap_or_else(|| session.display_name());
                Some((project, session))
            })
            .collect::<Vec<_>>();
        brain_core::digest::day(sessions, chrono::Local::now().date_naive())
    }

    fn render_today(&self, cx: &mut Context<Self>) -> AnyElement {
        let projects = self.today_digest();
        let date = chrono::Local::now().format("%d.%m.%Y").to_string();
        let title = tr!("Heute, {date}", "Today, {date}");
        let mut body: Vec<AnyElement> = Vec::new();
        if projects.is_empty() {
            body.push(hint(t(
                "Heute hat noch keine Session etwas als erledigt gemeldet.",
                "No session has reported anything as done today.",
            ).into()));
        }
        for project in &projects {
            body.push(
                div().mt(px(14.)).text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::text_strong()).child(project.project.clone()).into_any_element(),
            );
            for session in &project.sessions {
                body.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mt(px(6.))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::text()).child(session.name.clone()))
                        .child(account_badge(&session.account, self.account_index(&session.account)))
                        .into_any_element(),
                );
                for entry in &session.entries {
                    body.push(
                        div()
                            .flex()
                            .gap(px(10.))
                            .pl(px(2.))
                            .text_size(px(12.5))
                            .child(div().flex_none().w(px(40.)).text_color(theme::text_faint()).child(entry.at.format("%H:%M").to_string()))
                            .child(div().flex_1().min_w_0().line_height(relative(1.45)).text_color(theme::text()).child(plain(&entry.text)))
                            .into_any_element(),
                    );
                }
            }
        }
        let markdown = brain_core::digest::markdown(&title, &projects);
        div()
            .id("today")
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
                    .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(theme::text_strong()).child(title))
                    .child(div().flex_1())
                    .child(button("copy-digest", t("Als Markdown kopieren", "Copy as Markdown"), &format!("{}C", primary_label()), false, !projects.is_empty(), cx.listener(move |this, _: &ClickEvent, _, cx| {
                        crate::clipboard::copy(&markdown, cx);
                        this.set_status(t("Tagesübersicht kopiert.", "Copied the day's digest."));
                        cx.notify();
                    }))),
            )
            .child(div().id("today-body").flex_1().min_h_0().flex().flex_col().gap(px(2.)).pb(px(24.)).overflow_y_scroll().children(body))
            .into_any_element()
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
            ("P M S", t("anheften · stumm · pausieren", "pin · mute · snooze")),
            ("G", t("Projekte", "projects")),
            ("X A", t("beenden · Konto wechseln", "end · move account")),
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

impl BrainView {
    /// The failed action, above the footer at the bottom right; a click dismisses it.
    fn render_error(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (message, _) = self.error.as_ref()?;
        let toast = div()
            .id("error-toast")
            .absolute()
            .bottom(px(44.))
            .right(px(16.))
            .max_w(px(460.))
            .flex()
            .items_start()
            .gap(px(10.))
            .px(px(14.))
            .py(px(10.))
            .rounded(px(8.))
            .bg(theme::raised())
            .border_1()
            .border_color(theme::calls())
            .shadow(vec![BoxShadow {
                color: theme::alpha(theme::calls(), 0x40).into(),
                offset: gpui::point(px(0.), px(4.)),
                blur_radius: px(18.),
                spread_radius: px(0.),
            }])
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.error = None;
                cx.notify();
            }))
            .child(div().flex_none().text_color(theme::calls()).font_weight(FontWeight::BOLD).child("!"))
            .child(div().flex_1().min_w_0().text_color(theme::text_strong()).child(message.clone()))
            .child(div().flex_none().text_color(theme::text_faint()).child("×"));
        Some(toast.into_any_element())
    }
}

impl Render for BrainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = now_ms();

        // A selected session that just ended stays selected: the ended list opens for it, so `P`
        // or ⏎ right after `/exit` still mean that session.
        let selected_ended = self.selected_session().is_some_and(|s| s.phase() == Phase::Ended && self.model.resumable(s));
        if selected_ended && !self.include_ended() {
            self.show_ended = true;
        }
        // Keep the selection on a visible session.
        let visible: Vec<SessionKey> =
            self.groups(now).navigable(self.include_ended()).iter().map(|s| s.key.clone()).collect();
        if self.selected.as_ref().is_none_or(|k| !visible.contains(k)) {
            self.selected = visible.first().cloned();
        }
        self.sync_conversation();
        self.sync_host();
        self.sync_projects();
        self.conflicts = self.compute_conflicts();

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
        let own_frame = matches!(window.window_decorations(), Decorations::Client { .. }).then(|| window.window_controls());
        let resizable = own_frame.is_some() && !window.is_maximized();
        let titlebar = self.render_titlebar(&groups, own_frame, cx).into_any_element();
        let list = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(relative(0.4))
            .min_w(px(360.))
            .max_w(px(480.))
            .h_full()
            .bg(theme::chrome())
            .border_r_1()
            .border_color(theme::line())
            .child(self.render_list(&groups, now, cx))
            .children(self.render_usage(now))
            .into_any_element();
        let detail = if self.mode == Mode::NewSession {
            self.render_new_session(cx)
        } else if self.prefs.layout == Layout::Today {
            self.render_today(cx)
        } else {
            self.render_detail(now, cx)
        };
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
            .relative()
            .child(titlebar)
            .child(div().flex().flex_1().min_h_0().child(list).child(detail))
            .child(self.render_footer())
            .when_some(self.render_error(cx), |d, toast| d.child(toast))
            .when(own_frame.is_some() && !window.is_maximized(), |d| d.border_1().border_color(theme::line_strong()))
            .when(resizable, |d| {
                d.child(window_frame::resize_cursors()).on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, cx| {
                    if window_frame::start_resize(event.position, window) {
                        cx.stop_propagation();
                    }
                })
            })
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

/// `5h ▓▓▓░░ 46% · 15:20`: a plan limit window with its reset time.
fn limit_bar(label: &str, limit: brain_core::usage::Limit, now_ms: i64) -> AnyElement {
    let used = limit.used_percentage.clamp(0.0, 100.0);
    let color = if used >= 90.0 { theme::calls() } else if used >= 70.0 { theme::turn() } else { theme::working() };
    let resets = limit.resets_at.filter(|r| r * 1000 > now_ms).map(|r| {
        let at = chrono::DateTime::from_timestamp(r, 0).map(|t| t.with_timezone(&chrono::Local));
        match at {
            Some(t) if r * 1000 - now_ms < 24 * 3600 * 1000 => t.format("%H:%M").to_string(),
            Some(t) => t.format("%a").to_string(),
            None => String::new(),
        }
    });
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .text_size(px(11.))
        .child(div().text_color(theme::text_faint()).child(label.to_string()))
        .child(
            div()
                .w(px(46.))
                .h(px(5.))
                .rounded_full()
                .bg(theme::raised())
                .child(div().h_full().rounded_full().bg(color).w(px(46. * used as f32 / 100.))),
        )
        .child(div().text_color(theme::text()).child(format!("{used:.0}%")))
        .when_some(resets, |d, at| d.child(div().text_color(theme::text_faint()).child(format!("↻ {at}"))))
        .into_any_element()
}

/// One changed file: status code, path, added and removed lines.
fn change_row(file: &brain_core::changes::FileChange) -> AnyElement {
    let code = file.status.trim();
    let color = match code.chars().next() {
        Some('A') => theme::done(),
        Some('D') => theme::calls(),
        Some('R') => theme::working(),
        Some('?') => theme::text_faint(),
        _ => theme::turn(),
    };
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .py(px(3.))
        .text_size(px(12.))
        .child(div().flex_none().w(px(22.)).font_family(theme::MONO).text_color(color).child(if code.is_empty() { "M".to_string() } else { code.to_string() }))
        .child(div().flex_1().min_w_0().truncate().font_family(theme::MONO).text_size(px(11.5)).text_color(theme::text()).child(file.path.clone()))
        .when_some(file.added.filter(|a| *a > 0), |d, a| d.child(div().flex_none().text_color(theme::done()).child(format!("+{a}"))))
        .when_some(file.removed.filter(|r| *r > 0), |d, r| d.child(div().flex_none().text_color(theme::calls()).child(format!("−{r}"))))
        .into_any_element()
}

/// `#12 ✓`, `#12 ✗`, `#12 …`: the PR of the session's branch and its checks.
fn pr_marker(pr: &brain_core::github::PullRequest) -> AnyElement {
    let (symbol, color) = pr_look(pr);
    marker(format!("#{} {symbol}", pr.number), color)
}

fn pr_look(pr: &brain_core::github::PullRequest) -> (&'static str, gpui::Rgba) {
    use brain_core::github::Checks;
    match (pr.state.as_str(), pr.checks) {
        ("MERGED", _) => ("merged", theme::text_faint()),
        ("CLOSED", _) => ("closed", theme::text_faint()),
        (_, Checks::Failing) => ("✗", theme::calls()),
        (_, Checks::Pending) => ("…", theme::turn()),
        (_, Checks::Passing) => ("✓", theme::done()),
        (_, Checks::None) => ("", theme::text_muted()),
    }
}

/// A chip that opens the PR in the browser.
fn pr_chip(pr: brain_core::github::PullRequest) -> AnyElement {
    let (symbol, color) = pr_look(&pr);
    let failing = pr.failing.len();
    let mut label = format!("PR #{} {symbol}", pr.number);
    if failing > 0 {
        label.push_str(&tr!(" · {failing} Checks rot", " · {failing} checks failing"));
    }
    if pr.draft {
        label.push_str(t(" · Entwurf", " · draft"));
    }
    match pr.review.as_deref() {
        Some("APPROVED") => label.push_str(t(" · freigegeben", " · approved")),
        Some("CHANGES_REQUESTED") => label.push_str(t(" · Änderungen gewünscht", " · changes requested")),
        _ => {}
    }
    let url = pr.url.clone();
    div()
        .id("pr-chip")
        .flex_none()
        .px(px(7.))
        .py(px(2.))
        .rounded(px(5.))
        .cursor_pointer()
        .bg(theme::alpha(color, 0x1c))
        .border_1()
        .border_color(theme::alpha(color, 0x55))
        .text_size(px(11.5))
        .text_color(color)
        .on_click(move |_: &ClickEvent, _, cx| cx.open_url(&url))
        .child(label)
        .into_any_element()
}

/// Looks up the PR of every session's branch (blocking; runs off the UI thread).
/// Sessions on the same checkout and branch share one `gh` call.
fn lookup_prs(sessions: Vec<(SessionKey, String)>) -> HashMap<SessionKey, brain_core::github::PullRequest> {
    let mut by_branch: HashMap<(String, String), Option<brain_core::github::PullRequest>> = HashMap::new();
    let mut out = HashMap::new();
    for (key, cwd) in sessions {
        let path = std::path::Path::new(&cwd);
        let Some(branch) = brain_core::changes::branch_of(path) else { continue };
        if matches!(branch.as_str(), "main" | "master") {
            continue;
        }
        let pr = by_branch
            .entry((cwd.clone(), branch.clone()))
            .or_insert_with(|| brain_core::github::pull_request(path, &branch))
            .clone();
        if let Some(pr) = pr {
            out.insert(key, pr);
        }
    }
    out
}

/// Opens a file with the app the system uses for its type (your Markdown editor for templates).
fn open_in_editor(path: &std::path::Path) {
    let open = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(open).arg(path).spawn();
}

/// Single-quotes a word for `sh`.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
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

/// What to change so Brain can type into a terminal; the alternatives differ per platform.
fn limit_help(limit: &Limit) -> String {
    let supported = if cfg!(target_os = "macos") { t("iTerm2 oder tmux", "iTerm2 or tmux") } else { t("Konsole oder tmux", "Konsole or tmux") };
    match limit {
        Limit::KonsoleLocked => t(
            "Konsole lässt Brain nicht tippen. In Konsole: Einstellungen → Konsole einrichten → Allgemein → „Sicherheitsrelevante Teile der D-Bus-Schnittstelle aktivieren“. Danach dieses Konsole-Fenster schließen, neu öffnen und die Session mit claude --resume fortsetzen.",
            "Konsole doesn't let Brain type. In Konsole: Settings → Configure Konsole → General → “Enable the security sensitive parts of the DBus API”. Then close this Konsole window, open a new one and continue the session with claude --resume.",
        )
        .to_string(),
        Limit::TerminalApp => tr!(
            "In Terminal.app kann Brain nur das Fenster nach vorne holen. Zum Antworten aus Brain die Session in {supported} starten.",
            "In Terminal.app Brain can only bring the window forward. To reply from Brain, start the session in {supported}."
        ),
        Limit::Editor => tr!(
            "Ins Terminal von VS Code und Cursor kann Brain nicht tippen. Zum Antworten aus Brain die Session in {supported} starten.",
            "Brain can't type into the terminal of VS Code or Cursor. To reply from Brain, start the session in {supported}."
        ),
        Limit::Unknown(name) => tr!(
            "Das Terminal „{name}“ kann Brain nicht steuern. Zum Antworten aus Brain die Session in {supported} starten.",
            "Brain can't control the terminal “{name}”. To reply from Brain, start the session in {supported}."
        ),
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
