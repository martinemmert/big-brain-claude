//! Brain's state and behaviour: what the keys and buttons do, what runs in the background, and
//! what the list shows. Drawing lives in `ui`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use brain_core::state::{Phase, Session, SessionKey};
use brain_terminal::{self as terminal, Capabilities, Key, Outcome};
use iced::keyboard::{self, key::Named};
use iced::widget::{operation, scrollable};
use iced::{window, Subscription, Task};

use crate::config;
use crate::conversation::Conversation;
use crate::format::{ago, clock, now_ms, plain, shell_quote};
use crate::i18n::t;
use crate::menubar::MenuBar;
use crate::model::Model;
use crate::notify::{self, Notifier};
use crate::prefs::{Layout, Prefs};
use crate::tr;

/// Sessions on "your turn" for longer than this move to the "resting" section.
pub const RESTING_AFTER_MS: i64 = 2 * 60 * 60 * 1000;
/// Ended sessions show in the list for a day; older ones through the search.
pub const ENDED_RECENT_MS: i64 = 24 * 60 * 60 * 1000;
const SNOOZE_MS: i64 = 15 * 60 * 1000;
/// How long after typing into a terminal its program may still write the clipboard (OSC 52):
/// long enough for `/copy` and its picker, too short for output that arrives later.
const CLIPBOARD_AFTER_TYPING: Duration = Duration::from_secs(10);
/// How long after releasing the mouse in a terminal: Claude Code copies a mouse selection the
/// moment the button goes up. A click only to focus the terminal opens no longer window.
const CLIPBOARD_AFTER_SELECTING: Duration = Duration::from_secs(2);

/// Ages the clean-up dialog offers, in days.
pub const CLEANUP_DAYS: [i64; 4] = [1, 3, 7, 14];

pub const SEARCH: &str = "search";
pub const REPLY: &str = "reply";
pub const RENAME: &str = "rename";
pub const NEW_SESSION: &str = "new-session";
pub const LIST: &str = "session-list";
pub const MESSAGES: &str = "messages";
pub const FIND: &str = "find";
pub const PALETTE: &str = "palette";

/// Where typed keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    Rename,
    Reply,
    NewSession,
    Cleanup,
    /// The command palette (⌘K).
    Palette,
}

/// What a line of the command palette does.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteCommand {
    Act(Action),
    Tab(DetailTab),
    /// Text (a slash command, a prompt) typed into the selected session.
    Send(String),
    /// A prompt typed into every session that waits for its turn.
    Broadcast(String),
    NewSession,
    Find,
    Cleanup,
    ToggleEnded,
    ToggleProjects,
    ToggleToday,
    ToggleBackground,
    /// Show or hide sessions programs started (SDK reviews, `claude -p`).
    ToggleAutomated,
    Split,
    Overview,
}

/// Slash commands the palette offers for the selected session.
const SLASH_COMMANDS: [(&str, &str, &str); 6] = [
    ("/compact", "Kontext zusammenfassen", "summarise the context"),
    ("/context", "Kontext-Belegung zeigen", "show what fills the context"),
    ("/model", "Modell wechseln", "switch the model"),
    ("/usage", "Verbrauch zeigen", "show usage"),
    ("/review", "Änderungen reviewen", "review the changes"),
    ("/clear", "Gespräch leeren", "clear the conversation"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Messages,
    Timeline,
    Changes,
    /// Every file the session wrote, edited, read or was given.
    Files,
    /// A real terminal running the session inside Brain (`claude attach` / `claude --resume`).
    Terminal,
}

/// Which files the Files tab lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFilter {
    All,
    /// Written or edited by Claude.
    Changed,
    Read,
    /// Attached to the session or dropped into a prompt.
    Given,
}

/// The whole conversation of one session, as of a transcript size.
pub struct HistoryCache {
    pub key: SessionKey,
    pub len: u64,
    pub messages: Vec<brain_core::transcript::Message>,
}

/// The files of one session, as of a transcript size.
pub struct FilesCache {
    pub key: SessionKey,
    pub len: u64,
    pub files: Vec<brain_core::files::SessionFile>,
}

/// Buttons in the detail pane and the list's context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Resume,
    Allow,
    Deny,
    StartReply,
    StartRename,
    CopyDigest,
    StartTerminal,
    OpenInITerm,
    TakeOver,
    Pin,
    Mute,
    Snooze,
    /// Claude's last answer to the clipboard (⌘⇧C).
    CopyAnswer,
    /// Open the session's terminal beside the one that was on screen (split).
    OpenBeside,
    /// The whole conversation as a Markdown file, opened in YAMV.
    Export,
    OtherAccount,
    End,
    Hide,
    Trash,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Key(keyboard::Event, bool),
    Select(SessionKey),
    OpenEntry(SessionKey),
    /// A right click on a list entry: select it, open its menu at the pointer.
    ContextMenu(SessionKey),
    /// A choice in the context menu; the click is the confirmation keys get by pressing twice.
    MenuPick(Action),
    CloseMenu,
    /// The pointer over the list, in window coordinates (where a context menu opens).
    Pointer(iced::Point),
    WindowSized(iced::Size),
    Hover(Option<SessionKey>),
    Search(String),
    SearchSubmit,
    Reply(String),
    ReplySubmit,
    Rename(String),
    RenameSubmit,
    NewInput(String),
    NewSubmit,
    NewAccount(usize),
    NewPick(usize),
    Layout(Layout),
    Filter(Option<String>),
    ToggleBackground,
    ToggleAutomated,
    ToggleResting,
    ToggleEnded,
    Tab(DetailTab),
    Do(Action),
    CleanupAge(usize),
    OpenUrl(String),
    ListScrolled(scrollable::Viewport),
    MessagesScrolled(scrollable::Viewport),
    CheckPrs,
    PrsLoaded(HashMap<SessionKey, brain_core::github::PullRequest>),
    ChangesLoaded(SessionKey, Option<brain_core::changes::Changes>),
    FilesLoaded(SessionKey, u64, Vec<brain_core::files::SessionFile>),
    Palette(String),
    PaletteRun(PaletteCommand),
    PaletteClose,
    /// Bytes for a session's terminal, sent a moment after its text (the Return that submits).
    TerminalKeys(SessionKey, Vec<u8>),
    /// The find field in the Messages tab.
    Find(String),
    PromptsOnly(bool),
    HistoryLoaded(SessionKey, u64, Vec<brain_core::transcript::Message>),
    /// A search hit or a prompt clicked open (its index in the history), or closed again.
    Expand(Option<usize>),
    CopyText(String),
    Exported(Result<std::path::PathBuf, String>),
    FileFilter(FileFilter),
    /// A file from the Files tab: Markdown opens in YAMV, the rest in the reader.
    OpenFile(String),
    CheckUpdate,
    UpdateFound(Option<crate::links::Update>),
    DragWindow,
    ZoomWindow,
    Term(iced_term::Event),
    /// A background session Brain started or took over: its account and short id, or why not.
    Started(String, Result<String, String>),
    /// A Markdown file was handed to YAMV (`true`), or to the text editor because YAMV is missing.
    MarkdownOpened(String, bool),
    MousePressed,
    /// The quick-terminal hotkey was pressed (from anywhere).
    QuickTerminal,
    /// The left mouse button went up (a selection may have ended).
    MouseReleased,
    /// A terminal's request to put text on the clipboard, and whether it has the keyboard.
    TerminalCopy(SessionKey, String, bool),
    /// Whether this session's terminal has the keyboard (asked after a click).
    TermFocused(SessionKey, bool),
    /// ⌘D: the current terminal stays on the left, the next selected session opens beside it.
    ToggleSplit,
    /// A click on the split's other pane: its session becomes the active one.
    ActivatePane,
    /// ⌘⇧A: live previews of every session that runs in Brain.
    ToggleOverview,
    /// A preview clicked in the overview: open that session's terminal.
    OpenFromOverview(SessionKey),
    FileHover(bool),
    FileDrop(std::path::PathBuf),
    CloseReader,
    ReaderToEditor,
    NewPlace(bool),
    SidebarDrag,
    SidebarTo(f32),
    SidebarDone,
}

/// The session list's width bounds when dragged.
pub const SIDEBAR_DEFAULT: f32 = 460.0;
const SIDEBAR_MIN: f32 = 300.0;
const SIDEBAR_MAX: f32 = 1100.0;

/// A file opened from a path in the terminal output.
pub struct Reader {
    pub path: std::path::PathBuf,
    pub kind: ReaderKind,
}

pub enum ReaderKind {
    Code(iced::widget::text_editor::Content, String),
    Image(iced::widget::image::Handle),
    Unreadable(String),
}

/// The "new session" dialog: a folder (picked from recent ones or typed) and an account.
#[derive(Default)]
pub struct NewSession {
    /// The search field, or the value of the placeholder being asked for.
    pub folder: String,
    pub account: usize,
    pub pick: usize,
    pub templates: Vec<brain_core::templates::Template>,
    /// The picked template and its placeholder values so far.
    pub chosen: Option<Chosen>,
    /// ⌥⏎ starts it in an iTerm tab instead of in Brain.
    pub in_iterm: bool,
}

pub struct Chosen {
    pub template: brain_core::templates::Template,
    pub values: Vec<(String, String)>,
}

/// One row of the new-session list.
#[derive(Debug, Clone)]
pub enum Choice {
    Template(usize),
    Folder(String),
}

/// The selected session's git changes, loaded off the UI thread.
pub struct ChangesCache {
    pub key: SessionKey,
    pub loaded: Instant,
    pub changes: Option<brain_core::changes::Changes>,
}

/// The left column, already filtered and sorted.
pub struct Groups<'a> {
    /// Ended sessions the user marked with `P` to resume later.
    pub saved: Vec<&'a Session>,
    pub pinned: Vec<&'a Session>,
    pub attention: Vec<&'a Session>,
    pub working: Vec<&'a Session>,
    pub resting: Vec<&'a Session>,
    pub snoozed: Vec<&'a Session>,
    pub ended: Vec<&'a Session>,
    /// Whether the resting group is open (or a search shows everything).
    pub resting_open: bool,
    /// Ended over a day ago, shown only while searching.
    pub older_ended: usize,
    /// Hidden with ⌫, shown only while searching.
    pub hidden: usize,
    /// Background sessions left out while the filter is off.
    pub background: usize,
    /// Sessions programs started (SDK reviews, `claude -p`), left out while the filter is off.
    pub automated: usize,
}

impl<'a> Groups<'a> {
    pub fn navigable(&self, include_ended: bool) -> Vec<&'a Session> {
        let mut all: Vec<&Session> = self.saved.clone();
        all.extend(self.live().into_iter().filter(|s| self.resting_open || !self.resting.iter().any(|r| r.key == s.key)));
        if include_ended {
            all.extend(&self.ended);
        }
        all
    }

    pub fn live(&self) -> Vec<&'a Session> {
        let mut all: Vec<&Session> = Vec::new();
        all.extend(&self.pinned);
        all.extend(&self.attention);
        all.extend(&self.working);
        all.extend(&self.resting);
        all.extend(&self.snoozed);
        all
    }

    pub fn live_count(&self) -> usize {
        self.pinned.len() + self.attention.len() + self.working.len() + self.resting.len() + self.snoozed.len()
    }
}

/// One entry of the session list, in drawing order. The list's height is known from these, so
/// keyboard navigation can keep the selection in view.
pub enum Item<'a> {
    Section { title: String, count: usize, alert: bool, toggle: Option<Message> },
    Hint(String),
    Card(&'a Session, usize),
    Row(&'a Session, usize),
}

pub const SECTION_H: f32 = 34.0;
pub const HINT_H: f32 = 36.0;
pub const ROW_H: f32 = 36.0;
pub const CARD_H: f32 = 76.0;
pub const LIST_GAP: f32 = 6.0;
pub const LIST_TOP: f32 = 4.0;

impl Item<'_> {
    pub fn height(&self) -> f32 {
        match self {
            Item::Section { .. } => SECTION_H,
            Item::Hint(_) => HINT_H,
            Item::Card(..) => CARD_H,
            Item::Row(..) => ROW_H,
        }
    }
}

pub struct Brain {
    pub model: Model,
    pub prefs: Prefs,
    notifier: Notifier,
    menubar: Option<MenuBar>,
    /// `None` shows every account.
    pub filter: Option<String>,
    pub selected: Option<SessionKey>,
    pub hovered: Option<SessionKey>,
    pub show_ended: bool,
    /// The "resting for over 2 h" group is open.
    pub show_resting: bool,
    /// The clean-up dialog's age choice (index into CLEANUP_DAYS).
    pub cleanup_age: usize,
    pub status: Option<(String, Instant)>,
    pub tab: DetailTab,
    pub conversation: Option<Conversation>,
    pub files: Option<FilesCache>,
    pub palette: String,
    pub palette_index: usize,
    /// Text to find in the selected session's whole conversation (⌘F).
    pub find: String,
    /// The Messages tab lists only the user's prompts, all of them.
    pub prompts_only: bool,
    /// The whole conversation, loaded for finding and the prompt list.
    pub history: Option<HistoryCache>,
    history_loading: bool,
    pub expanded: Option<usize>,
    files_loading: bool,
    pub file_filter: FileFilter,
    pub mode: Mode,
    pub search: String,
    pub rename: String,
    pub reply: String,
    /// Reminders sent while a session keeps waiting: since when it waits, and how many.
    reminders: HashMap<SessionKey, (i64, u32)>,
    /// The terminal hosting the selected session and what Brain can do with it.
    host: Option<(SessionKey, Capabilities)>,
    /// Project name and worktree per session.
    pub projects: HashMap<SessionKey, (String, Option<String>)>,
    /// The checked-out working tree per session (for conflicts and PR lookups).
    checkouts: HashMap<SessionKey, std::path::PathBuf>,
    /// The PR of each session's branch, refreshed every two minutes in the background.
    pub prs: HashMap<SessionKey, brain_core::github::PullRequest>,
    /// Sessions editing the same files or checkout.
    pub conflicts: HashMap<SessionKey, brain_core::conflicts::Conflict>,
    /// The title each terminal's program set (Claude Code names its current task there).
    pub terminal_titles: HashMap<SessionKey, String>,
    /// The terminals on screen; the others handle their output without drawing it.
    shown_terminals: HashSet<SessionKey>,
    /// ⌘D: this session's terminal stays on screen beside the selected one.
    pub split: Option<SessionKey>,
    /// ⌘⇧A: live previews of every terminal instead of the detail pane.
    pub overview: bool,
    /// Sessions whose terminal rang the bell while another session was selected.
    pub bells: HashSet<SessionKey>,
    /// Pairs already notified about shared files.
    conflicts_notified: HashSet<(SessionKey, SessionKey)>,
    pub changes: Option<ChangesCache>,
    changes_loading: bool,
    pub new_session: NewSession,
    /// `A`, `X`, `⌫` or `⌘⌫` was pressed once on this session: a second press within 5 s acts.
    armed: Option<(char, SessionKey, Instant)>,
    /// The session whose context menu is open, and where it opens.
    pub context_menu: Option<(SessionKey, iced::Point)>,
    /// The session selected before a right click selected another one: "open beside" keeps it.
    pub before_menu: Option<SessionKey>,
    pointer: iced::Point,
    pub window_size: iced::Size,
    /// A newer release on GitHub, if the daily check found one.
    pub update: Option<crate::links::Update>,
    /// What the list and the message pane show, to keep the selection and new messages in view.
    list_viewport: Option<(f32, f32)>,
    messages_at_bottom: bool,
    /// Terminals running sessions inside Brain, per session; they keep running while another
    /// session is selected.
    pub terminals: HashMap<SessionKey, iced_term::Terminal>,
    next_terminal: u64,
    /// Whether the selected session's terminal has the keyboard (drawn as a ring).
    /// The terminal that has the keyboard (Brain's shortcuts are off while one has it).
    pub focused_terminal: Option<SessionKey>,
    /// The terminal the user last typed into, and when.
    last_terminal_input: Option<(SessionKey, Instant)>,
    /// The terminal the user last released the mouse in (the end of a selection), and when.
    last_terminal_release: Option<(SessionKey, Instant)>,
    /// A file is being dragged over the window.
    pub file_hover: bool,
    /// The file the reader beside the terminal shows.
    pub reader: Option<Reader>,
    /// A background session Brain just started or took over: select and attach it once
    /// `claude agents` lists it (account, short id).
    pending_attach: Option<(String, String)>,
    /// An iTerm session being moved into Brain: its `/exit` was sent; once the process ended it
    /// continues in the background (`claude --bg --resume`).
    takeover: Option<SessionKey>,
    /// The session list's divider is being dragged.
    pub dragging_sidebar: bool,
    /// The selection the derived state was last synced for, and whether it ran in Brain then
    /// (auto-attach when either changes: another session, or this one just moved into Brain).
    synced_selection: Option<(SessionKey, bool)>,
    /// The native title bar is set up once the window exists.
    titlebar_ready: bool,
    /// What the detail pane showed last; when it changes, the message list is built anew and
    /// starts at its newest message again.
    detail_shape: (Layout, Mode, DetailTab),
}

impl Brain {
    pub fn new() -> (Self, Task<Message>) {
        let mut model = Model::load();
        model.refresh();
        let mut prefs = Prefs::load();
        if prefs.migrate_pid_entries(|account, pid| Some(model.board.live_by_pid(account, pid)?.key.clone())) {
            prefs.save();
        }
        let demo = model.is_demo();
        restore_saved(&mut model, &prefs);
        let mut brain = Self {
            model,
            prefs,
            notifier: Notifier::new(),
            menubar: MenuBar::new(),
            filter: None,
            selected: None,
            hovered: None,
            show_ended: false,
            show_resting: false,
            cleanup_age: 1,
            status: None,
            tab: DetailTab::Messages,
            conversation: None,
            mode: Mode::Normal,
            search: String::new(),
            rename: String::new(),
            reply: String::new(),
            reminders: HashMap::new(),
            host: None,
            projects: HashMap::new(),
            checkouts: HashMap::new(),
            prs: HashMap::new(),
            conflicts: HashMap::new(),
            terminal_titles: HashMap::new(),
            shown_terminals: HashSet::new(),
            split: None,
            overview: false,
            files: None,
            find: String::new(),
            palette: String::new(),
            palette_index: 0,
            prompts_only: false,
            history: None,
            history_loading: false,
            expanded: None,
            files_loading: false,
            file_filter: FileFilter::All,
            bells: HashSet::new(),
            conflicts_notified: HashSet::new(),
            changes: None,
            changes_loading: false,
            new_session: NewSession::default(),
            armed: None,
            context_menu: None,
            before_menu: None,
            pointer: iced::Point::ORIGIN,
            window_size: iced::Size::new(1180.0, 760.0),
            update: None,
            list_viewport: None,
            messages_at_bottom: true,
            terminals: HashMap::new(),
            next_terminal: 1,
            focused_terminal: None,
            last_terminal_input: None,
            last_terminal_release: None,
            file_hover: false,
            reader: None,
            pending_attach: None,
            takeover: None,
            synced_selection: None,
            dragging_sidebar: false,
            titlebar_ready: false,
            detail_shape: (Layout::Status, Mode::Normal, DetailTab::Messages),
        };
        brain.selected = brain.groups(now_ms()).navigable(false).first().map(|s| s.key.clone());
        if let Some(spec) = crate::config::quick_terminal().filter(|_| !demo) {
            if !crate::hotkey::register(&spec) {
                brain.set_status(tr!("Die Tastenkombination „{spec}“ ist vergeben oder ungültig.", "The hotkey “{spec}” is taken or invalid."));
            }
        }
        let synced = brain.sync();
        // For screenshots of the reader: `BRAIN_DEMO=1 BRAIN_DEMO_READER=notes.md`.
        if let Some(path) = std::env::var_os("BRAIN_DEMO_READER").filter(|_| demo) {
            brain.reader = Some(read_file(std::path::PathBuf::from(path)));
            brain.tab = DetailTab::Terminal;
        }
        let checks = if demo { Task::none() } else { Task::batch([Task::done(Message::CheckPrs), Task::done(Message::CheckUpdate)]) };
        (brain, Task::batch([synced, checks]))
    }

    pub fn title(&self) -> String {
        let waiting = self.waiting_counts().0;
        if waiting > 0 { tr!("Brain – {waiting} warten", "Brain – {waiting} waiting") } else { "Brain".into() }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keys = iced::event::listen_with(|event, status, _| match event {
            iced::Event::Keyboard(event @ keyboard::Event::KeyPressed { .. }) => {
                Some(Message::Key(event, status == iced::event::Status::Captured))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => Some(Message::MousePressed),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => Some(Message::MouseReleased),
            iced::Event::Window(window::Event::Opened { size, .. } | window::Event::Resized(size)) => Some(Message::WindowSized(size)),
            iced::Event::Window(window::Event::FileHovered(_)) => Some(Message::FileHover(true)),
            iced::Event::Window(window::Event::FilesHoveredLeft) => Some(Message::FileHover(false)),
            iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::FileDrop(path)),
            _ => None,
        });
        let mut subscriptions = vec![
            keys,
            iced::time::every(Duration::from_millis(800)).map(|_| Message::Tick),
            Subscription::run(crate::hotkey::presses).map(|_| Message::QuickTerminal),
        ];
        if self.dragging_sidebar {
            // Pointer moves only while the divider is held; otherwise every move would redraw.
            subscriptions.push(iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => Some(Message::SidebarTo(position.x)),
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(_)) => Some(Message::SidebarDone),
                _ => None,
            }));
        }
        subscriptions.extend(self.terminals.values().map(|term| term.subscription().map(Message::Term)));
        if !self.model.is_demo() {
            subscriptions.push(iced::time::every(Duration::from_secs(120)).map(|_| Message::CheckPrs));
            subscriptions.push(iced::time::every(Duration::from_secs(24 * 60 * 60)).map(|_| Message::CheckUpdate));
        }
        Subscription::batch(subscriptions)
    }

    /// Sessions waiting for the user (in the attention and pinned groups), and how many call.
    pub fn waiting_counts(&self) -> (usize, usize) {
        let groups = self.groups(now_ms());
        let waiting: Vec<&Session> =
            groups.attention.iter().chain(&groups.pinned).copied().filter(|s| matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn)).collect();
        let calling = waiting.iter().filter(|s| s.phase() == Phase::NeedsYou).count();
        (waiting.len(), calling)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = match message {
            Message::Tick => self.tick(),
            Message::Key(event, captured) => self.on_key(event, captured),
            Message::Select(key) => {
                self.selected = Some(key);
                Task::none()
            }
            Message::OpenEntry(key) => {
                self.selected = Some(key);
                self.act(Action::Open)
            }
            Message::ContextMenu(key) => {
                self.before_menu = self.selected.clone().filter(|k| *k != key);
                self.selected = Some(key.clone());
                self.context_menu = Some((key, self.pointer));
                Task::none()
            }
            Message::MenuPick(action) => {
                self.context_menu = None;
                self.perform(action, true)
            }
            Message::CloseMenu => {
                self.context_menu = None;
                Task::none()
            }
            Message::Pointer(position) => {
                self.pointer = position;
                Task::none()
            }
            Message::WindowSized(size) => {
                self.window_size = size;
                Task::none()
            }
            Message::Hover(key) => {
                self.hovered = key;
                Task::none()
            }
            Message::Search(text) => {
                self.search = text;
                self.mode = Mode::Search;
                self.select_first_visible();
                self.scroll_to_selected()
            }
            Message::SearchSubmit => {
                self.open_selected();
                Task::none()
            }
            Message::Reply(text) => {
                // A digit typed into the empty reply field picks that quick reply.
                let quick = self.reply.is_empty() && text.len() == 1 && text.chars().all(|c| ('1'..='9').contains(&c));
                match quick.then(|| config::quick_replies().get(text.parse::<usize>().unwrap_or(1) - 1).cloned()).flatten() {
                    Some(reply) => {
                        self.reply = reply;
                        return self.submit_reply();
                    }
                    None => self.reply = text,
                }
                Task::none()
            }
            Message::ReplySubmit => self.submit_reply(),
            Message::Rename(text) => {
                self.rename = text;
                Task::none()
            }
            Message::RenameSubmit => self.submit_rename(),
            Message::NewInput(text) => {
                self.new_session.folder = text;
                self.new_session.pick = 0;
                Task::none()
            }
            Message::NewSubmit => self.confirm_new_session(),
            Message::NewAccount(index) => {
                self.new_session.account = index;
                Task::none()
            }
            Message::NewPick(index) => {
                self.new_session.pick = index;
                self.confirm_new_session()
            }
            Message::Layout(layout) => {
                self.prefs.layout = layout;
                self.prefs.save();
                Task::none()
            }
            Message::Filter(filter) => {
                self.filter = filter;
                Task::none()
            }
            Message::ToggleBackground => {
                self.toggle_background();
                Task::none()
            }
            Message::ToggleAutomated => {
                self.toggle_automated();
                Task::none()
            }
            Message::ToggleResting => {
                self.show_resting = !self.show_resting;
                Task::none()
            }
            Message::ToggleEnded => {
                self.toggle_ended();
                Task::none()
            }
            Message::Tab(tab) => {
                self.tab = tab;
                Task::batch([self.load_changes(), self.load_files()])
            }
            Message::Do(action) => self.act(action),
            Message::CleanupAge(index) => {
                self.cleanup_age = index;
                Task::none()
            }
            Message::OpenUrl(url) => {
                let _ = std::process::Command::new("open").arg(url).spawn();
                Task::none()
            }
            Message::ListScrolled(viewport) => {
                self.list_viewport = Some((viewport.absolute_offset().y, viewport.bounds().height));
                Task::none()
            }
            Message::MessagesScrolled(viewport) => {
                self.messages_at_bottom = viewport.relative_offset().y >= 0.98 || viewport.content_bounds().height <= viewport.bounds().height;
                Task::none()
            }
            Message::CheckPrs => {
                let work = self.pr_lookups();
                Task::perform(off_thread(move || lookup_prs(work)), Message::PrsLoaded)
            }
            Message::PrsLoaded(prs) => {
                self.prs = prs;
                Task::none()
            }
            Message::Palette(text) => {
                self.palette = text;
                self.palette_index = 0;
                Task::none()
            }
            Message::PaletteRun(command) => {
                self.mode = Mode::Normal;
                let task = self.run_palette(command);
                Task::batch([unfocus(), task])
            }
            Message::PaletteClose => {
                self.mode = Mode::Normal;
                unfocus()
            }
            Message::TerminalKeys(key, bytes) => {
                if let Some(term) = self.terminals.get_mut(&key) {
                    term.handle(iced_term::Command::ProxyToBackend(iced_term::BackendCommand::Write(bytes)));
                }
                Task::none()
            }
            Message::Find(text) => {
                self.find = text;
                self.expanded = None;
                self.load_history()
            }
            Message::PromptsOnly(on) => {
                self.prompts_only = on;
                self.expanded = None;
                self.load_history()
            }
            Message::HistoryLoaded(key, len, messages) => {
                self.history = Some(HistoryCache { key, len, messages });
                self.history_loading = false;
                Task::none()
            }
            Message::Expand(index) => {
                self.expanded = index;
                Task::none()
            }
            Message::CopyText(text) => {
                self.set_status(t("Kopiert.", "Copied."));
                iced::clipboard::write(text)
            }
            Message::Exported(result) => match result {
                Ok(path) => open_markdown(path),
                Err(err) => {
                    self.set_status(tr!("Export fehlgeschlagen: {err}", "Export failed: {err}"));
                    Task::none()
                }
            },
            Message::FilesLoaded(key, len, files) => {
                self.files = Some(FilesCache { key, len, files });
                self.files_loading = false;
                Task::none()
            }
            Message::FileFilter(filter) => {
                self.file_filter = filter;
                Task::none()
            }
            Message::OpenFile(path) => {
                let path = std::path::PathBuf::from(path);
                if !path.is_file() {
                    self.set_status(t("Die Datei gibt es nicht mehr.", "That file no longer exists."));
                    return Task::none();
                }
                if is_markdown(&path) {
                    return open_markdown(path);
                }
                self.reader = Some(read_file(path));
                Task::none()
            }
            Message::ChangesLoaded(key, changes) => {
                self.changes = Some(ChangesCache { key, loaded: Instant::now(), changes });
                self.changes_loading = false;
                Task::none()
            }
            Message::CheckUpdate => Task::perform(off_thread(crate::links::check_for_update), Message::UpdateFound),
            Message::UpdateFound(update) => {
                self.update = update;
                Task::none()
            }
            Message::DragWindow => window::latest().and_then(window::drag),
            Message::ZoomWindow => window::latest().and_then(window::toggle_maximize),
            Message::Term(event) => {
                let iced_term::Event::BackendCall(id, command) = event;
                let key = self.terminals.iter().find(|(_, term)| term.id == id).map(|(key, _)| key.clone());
                let mut task = Task::none();
                if let Some(key) = key {
                    // Only the terminal on screen copies its screen per event; the others catch
                    // up when they're shown (`sync_shown_terminal`).
                    let shown = self.shown_terminals.contains(&key);
                    let command = iced_term::Command::ProxyToBackend(command);
                    match self.terminals.get_mut(&key).map(|term| if shown { term.handle(command) } else { term.handle_quiet(command) }) {
                        Some(iced_term::actions::Action::Shutdown) => {
                            self.terminals.remove(&key);
                            self.terminal_titles.remove(&key);
                        }
                        Some(iced_term::actions::Action::OpenLink(link)) => task = self.open_link(&key, &link),
                        Some(iced_term::actions::Action::ChangeTitle(title)) => {
                            let title = title.trim().to_string();
                            if title.is_empty() {
                                self.terminal_titles.remove(&key);
                            } else {
                                self.terminal_titles.insert(key, title);
                            }
                        }
                        Some(iced_term::actions::Action::Bell) if self.selected.as_ref() != Some(&key) => {
                            let name = self.model.board.get(&key).map(|s| s.display_name()).unwrap_or_default();
                            self.set_status(tr!("„{name}“ klingelt.", "“{name}” rang the bell."));
                            self.bells.insert(key);
                        }
                        // Output can carry clipboard requests too (a printed file, a fetched page):
                        // only a terminal you typed into a moment ago may write the clipboard, and
                        // only if it really has the keyboard now (asked below), and it says so.
                        Some(iced_term::actions::Action::CopyToClipboard(text))
                            if self.last_terminal_input.as_ref().is_some_and(|(k, at)| *k == key && at.elapsed() < CLIPBOARD_AFTER_TYPING)
                                || self.last_terminal_release.as_ref().is_some_and(|(k, at)| *k == key && at.elapsed() < CLIPBOARD_AFTER_SELECTING) =>
                        {
                            if let Some(term) = self.terminals.get(&key) {
                                let key = key.clone();
                                task = operation::is_focused(term.widget_id().clone()).map(move |focused| Message::TerminalCopy(key.clone(), text.clone(), focused));
                            }
                        }
                        _ => {}
                    }
                }
                task
            }
            Message::Started(account, result) => match result {
                Ok(id) => {
                    self.pending_attach = Some((account, id));
                    self.model.relist_transcripts();
                    self.set_status(t("Session läuft in Brain – sie erscheint gleich.", "The session runs in Brain – it shows up in a moment."));
                    Task::none()
                }
                Err(why) => {
                    self.set_status(tr!("Konnte die Session nicht starten: {why}", "Couldn't start the session: {why}"));
                    Task::none()
                }
            },
            Message::MousePressed => {
                // Clicks move the keyboard in or out of a terminal; ask each shown one.
                let asks: Vec<Task<Message>> = self
                    .shown_terminals
                    .iter()
                    .filter_map(|key| {
                        let term = self.terminals.get(key)?;
                        let key = key.clone();
                        Some(operation::is_focused(term.widget_id().clone()).map(move |focused| Message::TermFocused(key.clone(), focused)))
                    })
                    .collect();
                Task::batch(asks)
            }
            Message::QuickTerminal => self.quick_terminal(),
            Message::MouseReleased => {
                if let Some(key) = self.focused_terminal.clone() {
                    self.last_terminal_release = Some((key, Instant::now()));
                }
                Task::none()
            }
            Message::TerminalCopy(key, text, focused) => {
                if !focused {
                    return Task::none();
                }
                self.focused_terminal = Some(key);
                let chars = text.chars().count();
                self.set_status(tr!("Die Session hat {chars} Zeichen in die Zwischenablage kopiert.", "The session copied {chars} characters to the clipboard."));
                iced::clipboard::write(text)
            }
            Message::TermFocused(key, focused) => {
                if focused {
                    // Typing into the split's other pane makes it the active session.
                    if self.split.as_ref() == Some(&key) {
                        self.activate_split_pane();
                    }
                    self.focused_terminal = Some(key);
                } else if self.focused_terminal.as_ref() == Some(&key) {
                    self.focused_terminal = None;
                }
                Task::none()
            }
            Message::ToggleSplit => {
                self.split = match &self.split {
                    Some(_) => None,
                    None if self.has_terminal() => {
                        self.set_status(t("Geteilt – wähle die zweite Session.", "Split – pick the second session."));
                        self.selected.clone()
                    }
                    None => {
                        self.set_status(t("Teilen geht mit einer Session, die hier im Terminal läuft.", "Splitting works with a session running in Brain's terminal."));
                        None
                    }
                };
                Task::none()
            }
            Message::ActivatePane => {
                self.activate_split_pane();
                Task::none()
            }
            Message::ToggleOverview => {
                self.overview = !self.overview;
                if self.overview && self.terminals.is_empty() {
                    self.overview = false;
                    self.set_status(t("Noch läuft keine Session hier im Terminal.", "No session runs in Brain's terminal yet."));
                }
                Task::none()
            }
            Message::OpenFromOverview(key) => {
                self.overview = false;
                self.selected = Some(key);
                self.tab = DetailTab::Terminal;
                self.open_terminal(true)
            }
            Message::MarkdownOpened(name, in_yamv) => {
                if !in_yamv {
                    self.set_status(tr!(
                        "YAMV ist nicht installiert – {name} ist im Texteditor offen.",
                        "YAMV isn't installed – {name} opened in the text editor."
                    ));
                }
                Task::none()
            }
            Message::FileHover(hovering) => {
                self.file_hover = hovering;
                Task::none()
            }
            Message::FileDrop(path) => {
                self.file_hover = false;
                self.drop_file(&path)
            }
            Message::CloseReader => {
                self.reader = None;
                self.focus_terminal()
            }
            Message::SidebarDrag => {
                self.dragging_sidebar = true;
                Task::none()
            }
            Message::SidebarTo(x) => {
                self.prefs.sidebar_width = Some(x.clamp(SIDEBAR_MIN, SIDEBAR_MAX));
                Task::none()
            }
            Message::SidebarDone => {
                self.dragging_sidebar = false;
                self.prefs.save();
                Task::none()
            }
            Message::NewPlace(in_iterm) => {
                self.new_session.in_iterm = in_iterm;
                Task::none()
            }
            Message::ReaderToEditor => {
                if let Some(reader) = &self.reader {
                    open_reader_file(reader);
                }
                Task::none()
            }
        };
        let synced = self.sync();
        Task::batch([task, synced])
    }

    // ---- periodic work ------------------------------------------------------------

    fn tick(&mut self) -> Task<Message> {
        if !self.titlebar_ready {
            self.titlebar_ready = crate::chrome::unified_titlebar();
        }
        let now = now_ms();
        for item in self.model.refresh() {
            if self.prefs.snoozed_until(&item.key, now).is_some() || self.prefs.is_muted(&item.key) || self.filtered_background(&item.key) {
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

        let mut tasks = Vec::new();
        for response in notify::take_responses() {
            match response {
                notify::Response::Open(key) => {
                    self.selected = Some(key);
                    tasks.push(self.bring_forward());
                    tasks.push(self.scroll_to_selected());
                }
                notify::Response::Snooze(key) => {
                    self.prefs.snooze(&key, Some(now + SNOOZE_MS));
                    self.prefs.save();
                }
            }
        }
        self.remind(now);
        self.notify_conflicts();
        tasks.push(self.load_changes());
        let linked: Vec<SessionKey> = crate::links::take_sessions()
            .into_iter()
            .filter_map(|(account, pid)| Some(self.model.board.live_by_pid(&account, pid)?.key.clone()))
            .collect();
        for key in linked {
            self.selected = Some(key);
            self.mode = Mode::Normal;
            tasks.push(self.bring_forward());
            tasks.push(self.scroll_to_selected());
        }
        if MenuBar::take_click() {
            tasks.push(self.bring_forward());
        }
        tasks.push(self.continue_takeover());
        if self.status.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(6)) {
            self.status = None;
        }
        Task::batch(tasks)
    }

    /// Keeps derived state in step after every message: the selection on a visible session,
    /// the conversation, the terminal capabilities, projects, conflicts and the menu bar.
    fn sync(&mut self) -> Task<Message> {
        let now = now_ms();
        // A selected session that just ended stays selected: the ended list opens for it, so `P`
        // or ⏎ right after `/exit` still mean that session.
        let selected_ended = self.selected_session().is_some_and(|s| s.phase() == Phase::Ended && self.model.resumable(s));
        if selected_ended && !self.include_ended() {
            self.show_ended = true;
        }
        let visible: Vec<SessionKey> = self.groups(now).navigable(self.include_ended()).iter().map(|s| s.key.clone()).collect();
        if self.selected.as_ref().is_none_or(|k| !visible.contains(k)) {
            self.selected = visible.first().cloned();
        }
        if let Some(key) = &self.selected {
            self.bells.remove(key);
        }
        let mut scroll = self.sync_conversation();
        let attach = self.sync_terminal();
        self.sync_shown_terminal();
        let shape = (self.prefs.layout, if matches!(self.mode, Mode::NewSession | Mode::Cleanup) { self.mode } else { Mode::Normal }, self.tab);
        if shape != self.detail_shape {
            self.detail_shape = shape;
            self.messages_at_bottom = true;
            scroll = operation::snap_to_end(MESSAGES);
        }
        self.sync_host();
        self.sync_projects();
        self.conflicts = self.compute_conflicts();
        let (waiting, calling) = self.waiting_counts();
        if let Some(menubar) = self.menubar.as_mut() {
            menubar.show(waiting, calling);
        }
        // The Files tab and the find results follow the transcript as it grows.
        let files = self.load_files();
        let history = self.load_history();
        Task::batch([scroll, attach, files, history])
    }

    /// Background sessions open in Brain's terminal: when one gets selected (or Brain started it
    /// a moment ago), the Terminal tab shows and attaches. Leaving it for a session without a
    /// terminal goes back to the messages.
    fn sync_terminal(&mut self) -> Task<Message> {
        let mut just_started = false;
        if let Some((account, id)) = self.pending_attach.clone() {
            let found = self.model.board.keys().find(|k| {
                k.account == account && self.model.board.get(k).and_then(|s| s.agent.as_ref()).is_some_and(|a| a.id == id)
            });
            if let Some(key) = found.cloned() {
                self.pending_attach = None;
                just_started = true;
                self.selected = Some(key);
                self.filter = None;
            }
        }
        let current = self.selection_state();
        let attachable = current.as_ref().is_some_and(|(_, a)| *a);
        if current == self.synced_selection {
            return Task::none();
        }
        self.synced_selection = current;
        self.focused_terminal = None;
        let has_terminal = self.has_terminal();
        if attachable || has_terminal {
            self.tab = DetailTab::Terminal;
            // Selecting a session shows its terminal but keeps the keys for the list; a session
            // started a moment ago gets them, its prompt is next.
            return self.open_terminal(just_started);
        }
        if self.tab == DetailTab::Terminal {
            self.tab = DetailTab::Messages;
        }
        Task::none()
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
            if self.prefs.is_muted(&key) || self.prefs.snoozed_until(&key, now).is_some() || self.filtered_background(&key) {
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
            let waited = ago(since, now);
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
            .map(|s| brain_core::conflicts::Work { key: s.key.clone(), checkout: self.checkouts.get(&s.key).cloned(), edited: &s.insight.edited })
            .collect();
        brain_core::conflicts::conflicts(&work)
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
    fn load_changes(&mut self) -> Task<Message> {
        if self.tab != DetailTab::Changes || self.changes_loading || self.model.is_demo() {
            return Task::none();
        }
        let Some(session) = self.selected_session() else { return Task::none() };
        let fresh = self.changes.as_ref().is_some_and(|c| c.key == session.key && c.loaded.elapsed() < Duration::from_secs(5));
        let Some(cwd) = session.cwd.clone().filter(|_| !fresh) else { return Task::none() };
        let key = session.key.clone();
        self.changes_loading = true;
        Task::perform(off_thread(move || brain_core::changes::changes_of(std::path::Path::new(&cwd))), move |changes| {
            Message::ChangesLoaded(key.clone(), changes)
        })
    }

    /// Loads the selected session's whole conversation while the find field or the prompt list
    /// needs it, again when the transcript grew.
    fn load_history(&mut self) -> Task<Message> {
        if self.tab != DetailTab::Messages || self.history_loading || (self.find.trim().is_empty() && !self.prompts_only) {
            return Task::none();
        }
        let Some(key) = self.selected.clone() else { return Task::none() };
        let Some((path, len)) = self.conversation.as_ref().filter(|c| c.key == key).and_then(|c| c.transcript()) else {
            return Task::none();
        };
        if self.history.as_ref().is_some_and(|h| h.key == key && h.len == len) {
            return Task::none();
        }
        let path = path.to_path_buf();
        self.history_loading = true;
        Task::perform(off_thread(move || brain_core::transcript::read_all_messages(&path)), move |messages| {
            Message::HistoryLoaded(key.clone(), len, messages)
        })
    }

    /// Writes the selected session's conversation to `~/.claude-brain/exports/` as Markdown.
    fn export_selected(&mut self) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        let Some((path, _)) = self.conversation.as_ref().filter(|c| c.key == session.key).and_then(|c| c.transcript()) else {
            self.set_status(t("Zu dieser Session gibt es kein Transkript.", "This session has no transcript."));
            return Task::none();
        };
        let path = path.to_path_buf();
        let title = session.display_name();
        let exported = off_thread(move || {
            let messages = brain_core::transcript::read_all_messages(&path);
            let dir = brain_core::account::home_dir().join(".claude-brain/exports");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let safe: String = title.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
            let file = dir.join(format!("{}-{}.md", safe.trim_matches('-'), chrono::Local::now().format("%Y%m%d-%H%M")));
            std::fs::write(&file, brain_core::history::export_markdown(&title, &messages)).map_err(|e| e.to_string())?;
            Ok(file)
        });
        Task::perform(exported, Message::Exported)
    }

    /// ⌘K.
    fn open_palette(&mut self) -> Task<Message> {
        self.mode = Mode::Palette;
        self.palette.clear();
        self.palette_index = 0;
        self.focused_terminal = None;
        operation::focus(PALETTE)
    }

    /// The palette's lines for what's typed: commands whose label has every typed word, then,
    /// for text that isn't a command, sending it to the session or to all waiting ones.
    pub fn palette_entries(&self) -> Vec<(String, &'static str, PaletteCommand)> {
        let mut all: Vec<(String, &'static str, PaletteCommand)> = Vec::new();
        if let Some(session) = self.selected_session() {
            let name = session.display_name();
            let reachable = self.can_send(&session.key);
            let ended = session.phase() == Phase::Ended;
            if session.agent.is_some() || ended {
                all.push((tr!("„{name}“ hier im Terminal öffnen", "Open “{name}” here in the terminal"), "⏎", PaletteCommand::Act(Action::Open)));
            }
            all.push((tr!("„{name}“ in iTerm öffnen", "Open “{name}” in iTerm"), "⌥⏎", PaletteCommand::Act(Action::OpenInITerm)));
            if reachable {
                for (command, de, en) in SLASH_COMMANDS {
                    let what = t(de, en);
                    all.push((tr!("{command} an „{name}“ – {what}", "{command} to “{name}” – {what}"), "", PaletteCommand::Send(command.to_string())));
                }
            }
            all.push((t("Letzte Antwort kopieren", "Copy the last answer").to_string(), "⌘⇧C", PaletteCommand::Act(Action::CopyAnswer)));
            all.push((t("Gespräch als Markdown öffnen", "Open the conversation as Markdown").to_string(), "", PaletteCommand::Act(Action::Export)));
            all.push((t("In der Session suchen", "Find in the session").to_string(), "⌘F", PaletteCommand::Find));
            all.push((t("Dateien der Session", "The session's files").to_string(), "", PaletteCommand::Tab(DetailTab::Files)));
            all.push((t("Änderungen (git)", "Changes (git)").to_string(), "", PaletteCommand::Tab(DetailTab::Changes)));
            all.push((t("Umbenennen", "Rename").to_string(), "R", PaletteCommand::Act(Action::StartRename)));
            all.push((t("Anheften / lösen", "Pin / unpin").to_string(), "P", PaletteCommand::Act(Action::Pin)));
            all.push((t("Stumm / laut", "Mute / unmute").to_string(), "M", PaletteCommand::Act(Action::Mute)));
            all.push((t("Pausieren", "Snooze").to_string(), "S", PaletteCommand::Act(Action::Snooze)));
            if self.model.accounts.len() > 1 {
                all.push((t("Im anderen Konto fortsetzen", "Continue in the other account").to_string(), "A", PaletteCommand::Act(Action::OtherAccount)));
            }
            all.push((t("Beenden / Hintergrund-Session stoppen", "End / stop the background session").to_string(), "X", PaletteCommand::Act(Action::End)));
            all.push((t("Ausblenden", "Hide").to_string(), "⌫", PaletteCommand::Act(Action::Hide)));
            all.push((t("In den Papierkorb", "Move to Trash").to_string(), "⌘⌫", PaletteCommand::Act(Action::Trash)));
        }
        all.push((t("Terminal teilen", "Split the terminal").to_string(), "⌘D", PaletteCommand::Split));
        all.push((t("Alle Terminals im Überblick", "All terminals at a glance").to_string(), "⌘⇧A", PaletteCommand::Overview));
        all.push((t("Neue Session", "New session").to_string(), "⌘N", PaletteCommand::NewSession));
        all.push((t("Aufräumen", "Clean up").to_string(), "C", PaletteCommand::Cleanup));
        all.push((t("Beendete Sessions zeigen / verbergen", "Show / hide ended sessions").to_string(), "E", PaletteCommand::ToggleEnded));
        all.push((t("Nach Projekten gruppieren", "Group by project").to_string(), "G", PaletteCommand::ToggleProjects));
        all.push((t("Heute", "Today").to_string(), "D", PaletteCommand::ToggleToday));
        all.push((t("Hintergrund-Sessions zeigen / verbergen", "Show / hide background sessions").to_string(), "B", PaletteCommand::ToggleBackground));
        all.push((t("Automatische Sessions (Reviews, Skripte) zeigen / verbergen", "Show / hide automated sessions (reviews, scripts)").to_string(), "U", PaletteCommand::ToggleAutomated));

        let query = self.palette.trim().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut shown: Vec<_> = all.into_iter().filter(|(label, _, _)| {
            let label = label.to_lowercase();
            words.iter().all(|w| label.contains(w))
        }).collect();
        let text = self.palette.trim();
        if !text.is_empty() {
            if let Some(session) = self.selected_session().filter(|s| self.can_send(&s.key)) {
                let name = session.display_name();
                shown.push((tr!("An „{name}“ senden: {text}", "Send to “{name}”: {text}"), "", PaletteCommand::Send(text.to_string())));
            }
            let waiting = self.broadcast_targets().len();
            if waiting > 0 {
                shown.push((tr!("An alle {waiting} wartenden Sessions senden: {text}", "Send to all {waiting} waiting sessions: {text}"), "", PaletteCommand::Broadcast(text.to_string())));
            }
        }
        shown
    }

    fn run_palette(&mut self, command: PaletteCommand) -> Task<Message> {
        match command {
            PaletteCommand::Act(action) => self.act(action),
            PaletteCommand::Tab(tab) => {
                self.tab = tab;
                Task::batch([self.load_changes(), self.load_files()])
            }
            PaletteCommand::Send(text) => match self.selected.clone() {
                Some(key) => self.send_text(&key, &text),
                None => Task::none(),
            },
            PaletteCommand::Broadcast(text) => {
                let targets = self.broadcast_targets();
                let count = targets.len();
                let tasks: Vec<Task<Message>> = targets.iter().map(|key| self.send_text(key, &text)).collect();
                self.set_status(tr!("An {count} Sessions gesendet.", "Sent to {count} sessions."));
                Task::batch(tasks)
            }
            PaletteCommand::NewSession => self.open_new_session_dialog(),
            PaletteCommand::Find => self.start_find(),
            PaletteCommand::Cleanup => {
                self.armed = None;
                self.mode = Mode::Cleanup;
                Task::none()
            }
            PaletteCommand::ToggleEnded => {
                self.toggle_ended();
                Task::none()
            }
            PaletteCommand::ToggleProjects => {
                self.prefs.layout = if self.prefs.layout == Layout::Projects { Layout::Status } else { Layout::Projects };
                self.prefs.save();
                Task::none()
            }
            PaletteCommand::ToggleToday => {
                self.prefs.layout = if self.prefs.layout == Layout::Today { Layout::Status } else { Layout::Today };
                self.prefs.save();
                Task::none()
            }
            PaletteCommand::ToggleBackground => {
                self.toggle_background();
                Task::none()
            }
            PaletteCommand::ToggleAutomated => {
                self.toggle_automated();
                Task::none()
            }
            PaletteCommand::Split => {
                self.tab = DetailTab::Terminal;
                self.update(Message::ToggleSplit)
            }
            PaletteCommand::Overview => self.update(Message::ToggleOverview),
        }
    }

    /// Whether Brain can type into the session: its terminal runs in Brain, or it waits in an
    /// iTerm tab Brain can type into.
    fn can_send(&self, key: &SessionKey) -> bool {
        self.terminals.contains_key(key) || self.model.board.get(key).is_some_and(|s| s.accepts_input())
    }

    /// Sessions whose turn it is and that Brain can type into. Sessions with a permission
    /// dialog open are left out: typed text would answer the dialog.
    fn broadcast_targets(&self) -> Vec<SessionKey> {
        let groups = self.groups(now_ms());
        groups
            .attention
            .iter()
            .chain(&groups.pinned)
            .filter(|s| s.phase() == Phase::YourTurn && !s.awaiting_permission() && self.can_send(&s.key))
            .map(|s| s.key.clone())
            .collect()
    }

    /// Types `text` into a session and submits it: into Brain's terminal (Return a moment later,
    /// so Claude Code doesn't take it for a pasted newline), or into its iTerm tab.
    fn send_text(&mut self, key: &SessionKey, text: &str) -> Task<Message> {
        if self.blocked_in_demo() {
            return Task::none();
        }
        if let Some(term) = self.terminals.get_mut(key) {
            term.handle(iced_term::Command::ProxyToBackend(iced_term::BackendCommand::Write(text.as_bytes().to_vec())));
            let key = key.clone();
            return Task::perform(off_thread(|| std::thread::sleep(Duration::from_millis(150))), move |_| Message::TerminalKeys(key.clone(), b"\r".to_vec()));
        }
        let Some(session) = self.model.board.get(key).filter(|s| s.accepts_input()) else {
            self.set_status(t("Die Session nimmt gerade keine Eingabe an.", "The session doesn't take input right now."));
            return Task::none();
        };
        let outcome = terminal::type_text(session.pid, text);
        self.report(outcome, Some(t("Gesendet.", "Sent.").into()));
        Task::none()
    }

    /// ⌘F: find in the selected session's conversation.
    fn start_find(&mut self) -> Task<Message> {
        self.tab = DetailTab::Messages;
        Task::batch([operation::focus(FIND), self.load_history()])
    }

    /// Reads the selected session's files when the Files tab shows and its transcript grew.
    fn load_files(&mut self) -> Task<Message> {
        if self.tab != DetailTab::Files || self.files_loading {
            return Task::none();
        }
        let Some(key) = self.selected.clone() else { return Task::none() };
        let Some((path, len)) = self.conversation.as_ref().filter(|c| c.key == key).and_then(|c| c.transcript()) else {
            return Task::none();
        };
        if self.files.as_ref().is_some_and(|f| f.key == key && f.len == len) {
            return Task::none();
        }
        let path = path.to_path_buf();
        self.files_loading = true;
        Task::perform(off_thread(move || brain_core::files::session_files(&path)), move |files| Message::FilesLoaded(key.clone(), len, files))
    }

    /// What the Terminal tab would run for the selected session: `claude attach` for a
    /// background session, `claude --resume` for an ended one. Sessions running in an iTerm tab
    /// can't be joined from outside, so they get none.
    pub fn terminal_command(&self) -> Option<(Vec<String>, String)> {
        let session = self.selected_session()?;
        let cwd = session.cwd.clone().unwrap_or_else(|| brain_core::account::home_dir().display().to_string());
        if let Some(agent) = &session.agent {
            return Some((vec!["attach".into(), agent.id.clone()], cwd));
        }
        if session.phase() == Phase::Ended {
            return Some((vec!["--resume".into(), session.session_id.clone()?], cwd));
        }
        None
    }

    /// The selected session and whether Brain can attach to it: what `sync_terminal` follows.
    fn selection_state(&self) -> Option<(SessionKey, bool)> {
        let attachable = self
            .selected_session()
            .and_then(|s| s.agent.as_ref())
            .is_some_and(|a| !matches!(a.state.as_str(), "failed" | "stopped"));
        self.selected.clone().map(|k| (k, attachable))
    }

    /// In a split, the other pane becomes the active one (its session selected); the keyboard
    /// stays where it is.
    fn activate_split_pane(&mut self) {
        let Some(other) = self.split.clone() else { return };
        self.split = self.selected.replace(other);
        self.tab = DetailTab::Terminal;
        // Already showing its terminal: don't let the selection change reset the keyboard.
        self.synced_selection = self.selection_state();
    }

    /// Which terminals are on screen; the ones that just came on screen catch up.
    fn sync_shown_terminal(&mut self) {
        if self.split.as_ref().is_some_and(|k| !self.terminals.contains_key(k)) {
            self.split = None;
        }
        let shown: HashSet<SessionKey> = if self.overview {
            self.terminals.keys().cloned().collect()
        } else {
            // The split's other pane always shows its terminal; the active one on its tab.
            let active = self.selected.iter().filter(|_| self.tab == DetailTab::Terminal);
            active.chain(self.split.iter()).filter(|k| self.terminals.contains_key(*k)).cloned().collect()
        };
        for key in shown.difference(&self.shown_terminals) {
            if let Some(term) = self.terminals.get_mut(key) {
                term.refresh();
            }
        }
        self.shown_terminals = shown;
    }

    /// Whether the selected session has a terminal in Brain.
    fn has_terminal(&self) -> bool {
        self.selected.as_ref().is_some_and(|k| self.terminals.contains_key(k))
    }

    /// Gives a running terminal the keyboard when its tab is shown; starts nothing.
    fn focus_terminal(&mut self) -> Task<Message> {
        let Some(key) = self.selected.clone() else { return Task::none() };
        match self.terminals.get(&key) {
            Some(term) if self.tab == DetailTab::Terminal => {
                let task = iced_term::TerminalView::focus(term.widget_id().clone());
                self.focused_terminal = Some(key);
                task
            }
            _ => Task::none(),
        }
    }

    /// Writes bytes into the selected session's terminal (typed text, a dropped file's path).
    fn write_to_terminal(&mut self, bytes: Vec<u8>) -> bool {
        // The terminal with the keyboard (in a split, maybe not the selected session's).
        let key = self.focused_terminal.clone().or_else(|| self.selected.clone());
        match key.and_then(|k| self.terminals.get_mut(&k)) {
            Some(term) => {
                term.handle(iced_term::Command::ProxyToBackend(iced_term::BackendCommand::Write(bytes)));
                true
            }
            None => false,
        }
    }

    /// A file dropped on the window goes into the session like in iTerm: its path, escaped for
    /// the shell. Claude Code turns image paths into attachments (`[Image #1]`).
    fn drop_file(&mut self, path: &std::path::Path) -> Task<Message> {
        let escaped = escape_path(&path.display().to_string()) + " ";
        if self.mode == Mode::Reply {
            self.reply.push_str(&escaped);
            return Task::none();
        }
        if self.write_to_terminal(escaped.into_bytes()) {
            self.tab = DetailTab::Terminal;
            return self.focus_terminal();
        }
        self.set_status(t(
            "Dateien lassen sich in Sessions ziehen, die in Brain laufen (Terminal-Reiter).",
            "Files can be dropped into sessions that run in Brain (Terminal tab).",
        ));
        Task::none()
    }

    /// ⌘-click on a link in the terminal: URLs open in the browser, file paths in the reader
    /// beside the terminal (relative ones from the session's folder).
    fn open_link(&mut self, key: &SessionKey, link: &str) -> Task<Message> {
        if link.contains("://") || link.starts_with("mailto:") {
            // Only web and mail links: the output may come from anywhere (a fetched page, a file),
            // and `open` would hand other schemes (file://, ssh:, app schemes) to whatever app.
            let lower = link.to_ascii_lowercase();
            if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:") {
                let _ = std::process::Command::new("open").arg(link).spawn();
            } else {
                self.set_status(tr!("Nur Web-Links öffnet Brain: {link}", "Brain opens web links only: {link}"));
            }
            return Task::none();
        }
        let cwd = self.model.board.get(key).and_then(|s| s.cwd.clone()).unwrap_or_default();
        let Some(path) = resolve_path(link, &cwd) else {
            self.set_status(tr!("Datei nicht gefunden: {link}", "File not found: {link}"));
            return Task::none();
        };
        if is_markdown(&path) {
            return open_markdown(path);
        }
        self.reader = Some(read_file(path));
        Task::none()
    }

    /// Starts the selected session's terminal on the Terminal tab (⏎ or the button), and
    /// focuses it.
    fn open_terminal(&mut self, focus: bool) -> Task<Message> {
        if self.tab != DetailTab::Terminal || self.model.is_demo() {
            return Task::none();
        }
        let Some(key) = self.selected.clone() else { return Task::none() };
        if !self.terminals.contains_key(&key) {
            let Some((args, cwd)) = self.terminal_command() else { return Task::none() };
            let mut env = HashMap::new();
            if let Some(account) = self.model.account(&key.account).filter(|a| a.id != "main") {
                env.insert("CLAUDE_CONFIG_DIR".to_string(), account.config_dir.display().to_string());
            }
            env.insert("TERM".to_string(), "xterm-256color".to_string());
            let font = crate::chat_font::get();
            let settings = iced_term::settings::Settings {
                // A little more line spacing than the terminal default reads better at length.
                font: iced_term::settings::FontSettings { size: font.size, font_type: font.regular, scale_factor: 1.4 },
                theme: iced_term::settings::ThemeSettings::new(Box::new(crate::style::terminal_palette())),
                backend: iced_term::settings::BackendSettings {
                    program: "claude".into(),
                    args,
                    env,
                    working_directory: Some(std::path::PathBuf::from(cwd)),
                },
            };
            match iced_term::Terminal::new(self.next_terminal, settings) {
                Ok(term) => {
                    self.next_terminal += 1;
                    self.terminals.insert(key.clone(), term);
                }
                Err(err) => {
                    self.set_status(tr!("Terminal startet nicht: {err}", "Terminal didn't start: {err}"));
                    return Task::none();
                }
            }
        }
        if focus {
            self.focus_terminal()
        } else {
            Task::none()
        }
    }

    /// Keeps the message list in step with the selected session. New messages scroll into view
    /// unless the user scrolled up to read older ones.
    fn sync_conversation(&mut self) -> Task<Message> {
        let Some(key) = self.selected.clone() else {
            self.conversation = None;
            return Task::none();
        };
        let switched = self.conversation.as_ref().is_none_or(|c| c.key != key);
        if let Some(demo) = &self.model.demo_messages {
            if switched {
                self.conversation = Some(Conversation::fixed(key.clone(), demo.get(&key).cloned().unwrap_or_default()));
                return operation::snap_to_end(MESSAGES);
            }
            return Task::none();
        }
        let session_id = self.model.board.get(&key).and_then(|s| s.session_id.clone());
        let conversation = self.conversation.get_or_insert_with(|| Conversation::empty(key.clone()));
        let changed = conversation.sync(&key, session_id.as_deref(), &self.model.accounts);
        if switched || (changed && self.messages_at_bottom) {
            self.messages_at_bottom = true;
            return operation::snap_to_end(MESSAGES);
        }
        Task::none()
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
        let Some(pid) = self.model.board.get(&key).map(|s| s.pid) else { return };
        let caps = terminal::host_of(pid).map(|host| terminal::capabilities(&host)).unwrap_or(Capabilities { focus: false, type_text: false, keys: false });
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
            let checkout = project.checkout.clone();
            self.projects.insert(key.clone(), entry);
            self.checkouts.insert(key, checkout);
        }
    }

    // ---- the list ---------------------------------------------------------------

    pub fn groups(&self, now: i64) -> Groups<'_> {
        let searching = !self.search.is_empty();
        let mut groups = Groups {
            saved: vec![],
            pinned: vec![],
            attention: vec![],
            working: vec![],
            resting: vec![],
            snoozed: vec![],
            ended: vec![],
            resting_open: self.show_resting || searching,
            older_ended: 0,
            hidden: 0,
            background: 0,
            automated: 0,
        };
        let visible = self
            .model
            .board
            .sorted()
            .into_iter()
            .filter(|s| self.filter.as_ref().is_none_or(|f| *f == s.key.account))
            .filter(|s| s.matches(&self.search));
        for session in visible {
            let ended = session.phase() == Phase::Ended;
            if ended && (session.is_empty() || !self.model.resumable(session)) {
                continue;
            }
            // Background sessions with something happening show; stale ones (a day without
            // activity) only with the filter on.
            if session.agent.is_some() && !self.prefs.show_background && now - session.last_activity_ms > ENDED_RECENT_MS {
                groups.background += 1;
                continue;
            }
            // Sessions a program started (a security review after each commit, scripts with
            // `claude -p`) only with their filter on, or when pinned.
            if session.insight.is_automated() && !self.prefs.show_automated && !self.prefs.is_pinned(&session.key) {
                groups.automated += 1;
                continue;
            }
            if self.prefs.is_hidden(&session.key, session.last_activity_ms) && !searching {
                groups.hidden += 1;
                continue;
            }
            if self.prefs.is_pinned(&session.key) {
                if ended {
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
                Phase::Ended if searching || now - session.last_activity_ms < ENDED_RECENT_MS => groups.ended.push(session),
                Phase::Ended => groups.older_ended += 1,
            }
        }
        // Resting: most recently finished first.
        groups.resting.reverse();
        groups
    }

    /// The list in drawing order: sections, hints and sessions.
    pub fn items<'a>(&'a self, groups: &Groups<'a>, now: i64) -> Vec<Item<'a>> {
        let searching = !self.search.is_empty();
        let mut items = Vec::new();
        let mut nav = 0usize;
        let mut push = |items: &mut Vec<Item<'a>>, s: &'a Session| {
            let snoozed = self.prefs.snoozed_until(&s.key, now).is_some();
            let waiting = matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn) && !snoozed;
            let recent = now - s.phase_since_ms() <= RESTING_AFTER_MS || s.phase() == Phase::NeedsYou;
            items.push(if waiting && recent { Item::Card(s, nav) } else { Item::Row(s, nav) });
            nav += 1;
        };
        if searching && groups.navigable(true).is_empty() {
            let query = self.search.clone();
            items.push(Item::Hint(tr!("Keine Session passt zu „{query}“. Esc leert die Suche.", "No session matches “{query}”. Esc clears the search.")));
        }
        if !groups.saved.is_empty() {
            items.push(Item::Section { title: t("Zum Fortsetzen gemerkt", "Saved to resume").into(), count: groups.saved.len(), alert: false, toggle: None });
            for s in &groups.saved {
                push(&mut items, s);
            }
        }
        if self.prefs.layout != Layout::Status {
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
            if projects.is_empty() && !searching {
                items.push(Item::Hint(t("Keine laufenden Sessions.", "No running sessions.").into()));
            }
            for (name, sessions) in projects {
                let alert = sessions.iter().any(|s| s.phase() == Phase::NeedsYou);
                items.push(Item::Section { title: name, count: sessions.len(), alert, toggle: None });
                for s in sessions {
                    push(&mut items, s);
                }
            }
        } else {
            if !groups.pinned.is_empty() {
                items.push(Item::Section { title: t("Angeheftet", "Pinned").into(), count: groups.pinned.len(), alert: false, toggle: None });
                for s in &groups.pinned {
                    push(&mut items, s);
                }
            }
            items.push(Item::Section { title: t("Braucht dich", "Needs you").into(), count: groups.attention.len(), alert: true, toggle: None });
            if groups.attention.is_empty() && !searching {
                items.push(Item::Hint(t("Gerade wartet keine Session auf dich.", "No session is waiting for you right now.").into()));
            }
            for s in &groups.attention {
                push(&mut items, s);
            }
            if !groups.working.is_empty() {
                items.push(Item::Section { title: t("Arbeitet", "Working").into(), count: groups.working.len(), alert: false, toggle: None });
                for s in &groups.working {
                    push(&mut items, s);
                }
            }
            if !groups.resting.is_empty() {
                let open = groups.resting_open;
                let title = format!("{} {}", t("Ruht seit über 2 h", "Resting for over 2 h"), if open { "▾" } else { "▸" });
                items.push(Item::Section { title, count: groups.resting.len(), alert: false, toggle: Some(Message::ToggleResting) });
                if open {
                    for s in &groups.resting {
                        push(&mut items, s);
                    }
                }
            }
            if !groups.snoozed.is_empty() {
                items.push(Item::Section { title: t("Pausiert", "Snoozed").into(), count: groups.snoozed.len(), alert: false, toggle: None });
                for s in &groups.snoozed {
                    push(&mut items, s);
                }
            }
        }
        let mut notes: Vec<String> = Vec::new();
        if groups.older_ended > 0 {
            let n = groups.older_ended;
            notes.push(tr!("{n} älter als ein Tag", "{n} older than a day"));
        }
        if groups.hidden > 0 {
            let n = groups.hidden;
            notes.push(tr!("{n} ausgeblendet", "{n} hidden"));
        }
        if !notes.is_empty() {
            let notes = notes.join(" · ");
            items.push(Item::Hint(tr!("{notes} – / sucht darin, C räumt auf.", "{notes} – / searches them, C cleans up.")));
        }
        if !groups.ended.is_empty() {
            let open = self.include_ended();
            let title = format!("{} {}", t("Beendet", "Ended"), if open { "▾" } else { "▸" });
            items.push(Item::Section { title, count: groups.ended.len(), alert: false, toggle: Some(Message::ToggleEnded) });
            if open {
                for s in &groups.ended {
                    push(&mut items, s);
                }
            }
        }
        items
    }

    /// Scrolls the list so the selected session is visible (the first one: to the very top).
    fn scroll_to_selected(&self) -> Task<Message> {
        let Some(key) = self.selected.clone() else { return Task::none() };
        let now = now_ms();
        let groups = self.groups(now);
        let mut top = LIST_TOP;
        for item in self.items(&groups, now) {
            let found = match &item {
                Item::Card(s, nav) | Item::Row(s, nav) if s.key == key => Some(*nav),
                _ => None,
            };
            if let Some(nav) = found {
                let bottom = top + item.height();
                let target = match self.list_viewport {
                    _ if nav == 0 => Some(0.0),
                    Some((offset, _)) if top < offset => Some(top - 8.0),
                    Some((offset, height)) if bottom > offset + height => Some(bottom - height + 8.0),
                    Some(_) => None,
                    None => Some(top - 8.0),
                };
                return match target {
                    Some(y) => operation::scroll_to(LIST, scrollable::AbsoluteOffset { x: 0.0, y: y.max(0.0) }),
                    None => Task::none(),
                };
            }
            top += item.height() + LIST_GAP;
        }
        Task::none()
    }

    /// A background or automated session while its filter is off: not listed, not announced.
    fn filtered_background(&self, key: &SessionKey) -> bool {
        let Some(s) = self.model.board.get(key) else { return false };
        let background = !self.prefs.show_background && s.agent.is_some() && now_ms() - s.last_activity_ms > ENDED_RECENT_MS;
        let automated = !self.prefs.show_automated && s.insight.is_automated() && !self.prefs.is_pinned(key);
        background || automated
    }

    fn toggle_automated(&mut self) {
        self.prefs.show_automated = !self.prefs.show_automated;
        self.prefs.save();
        self.set_status(if self.prefs.show_automated {
            t("Automatische Sessions (Reviews, Skripte) werden angezeigt.", "Showing automated sessions (reviews, scripts).")
        } else {
            t("Automatische Sessions ausgeblendet.", "Automated sessions hidden.")
        });
    }

    fn toggle_background(&mut self) {
        self.prefs.show_background = !self.prefs.show_background;
        self.prefs.save();
        self.set_status(if self.prefs.show_background {
            t("Hintergrund-Sessions werden angezeigt.", "Showing background sessions.")
        } else {
            t("Hintergrund-Sessions ausgeblendet.", "Background sessions hidden.")
        });
    }

    fn toggle_ended(&mut self) {
        self.show_ended = !self.show_ended;
        if !self.show_ended && self.selected_session().is_some_and(|s| s.phase() == Phase::Ended) {
            self.selected = None;
        }
    }

    pub fn include_ended(&self) -> bool {
        self.show_ended || !self.search.is_empty()
    }

    pub fn account_index(&self, account: &str) -> usize {
        self.model.accounts.iter().position(|a| a.id == account).unwrap_or(0)
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.selected.as_ref().and_then(|k| self.model.board.get(k))
    }

    /// What Brain may do with the selected session's terminal. Demo sessions get everything so
    /// screenshots show the controls; their actions are blocked elsewhere.
    pub fn capabilities(&self) -> Capabilities {
        if self.model.is_demo() {
            return Capabilities { focus: true, type_text: true, keys: true };
        }
        match &self.host {
            Some((key, caps)) if Some(key) == self.selected.as_ref() => *caps,
            _ => Capabilities { focus: false, type_text: false, keys: false },
        }
    }

    // ---- keys -------------------------------------------------------------------

    fn on_key(&mut self, event: keyboard::Event, captured: bool) -> Task<Message> {
        let keyboard::Event::KeyPressed { key, modifiers, text, .. } = event else { return Task::none() };
        if let Some(focused) = self.focused_terminal.clone() {
            self.last_terminal_input = Some((focused, Instant::now()));
        }
        let named = match key.as_ref() {
            keyboard::Key::Named(named) => Some(named),
            _ => None,
        };
        let character = match key.as_ref() {
            keyboard::Key::Character(c) => Some(c.to_lowercase()),
            _ => None,
        };
        let cmd = modifiers.command();
        if self.context_menu.take().is_some() && named == Some(Named::Escape) {
            return Task::none();
        }

        // A focused terminal keeps Esc (it interrupts Claude); ⌘[ leaves it.
        if named == Some(Named::Escape) && !(self.tab == DetailTab::Terminal && self.focused_terminal.is_some()) {
            return self.escape();
        }
        if cmd && self.tab == DetailTab::Terminal && self.focused_terminal.is_some() && character.as_deref() == Some("v") {
            // A copied screenshot: the terminal pastes text only, so Brain saves the picture and
            // pastes its path, which Claude Code turns into an attachment.
            if let Some(path) = crate::clipboard::save_image() {
                let escaped = escape_path(&path.display().to_string()) + " ";
                self.write_to_terminal(escaped.into_bytes());
            }
            return Task::none();
        }
        if cmd && named == Some(Named::Enter) && self.reader.is_some() {
            if let Some(reader) = &self.reader {
                open_reader_file(reader);
            }
            return Task::none();
        }
        // ⌘2 / ⌘J: keyboard into the terminal; ⌘1 / ⌘[: back to the session list.
        if cmd && matches!(character.as_deref(), Some("2") | Some("j")) {
            return if self.has_terminal() {
                self.tab = DetailTab::Terminal;
                self.focus_terminal()
            } else {
                Task::none()
            };
        }
        // ⌘D splits the terminal, ⌘⇧A shows all of them: also from inside a terminal.
        if cmd && character.as_deref() == Some("d") && self.tab == DetailTab::Terminal {
            return self.update(Message::ToggleSplit);
        }
        if cmd && modifiers.shift() && character.as_deref() == Some("a") {
            return self.update(Message::ToggleOverview);
        }
        // ⌘K: the command palette, also from inside the terminal.
        if cmd && character.as_deref() == Some("k") {
            return self.open_palette();
        }
        if self.mode == Mode::Palette {
            let count = self.palette_entries().len();
            match named {
                Some(Named::ArrowDown) => self.palette_index = (self.palette_index + 1).min(count.saturating_sub(1)),
                Some(Named::ArrowUp) => self.palette_index = self.palette_index.saturating_sub(1),
                Some(Named::Enter) => {
                    if let Some((_, _, command)) = self.palette_entries().into_iter().nth(self.palette_index) {
                        return self.update(Message::PaletteRun(command));
                    }
                }
                _ => {}
            }
            return Task::none();
        }
        if cmd && matches!(character.as_deref(), Some("1") | Some("[")) {
            self.focused_terminal = None;
            if self.mode == Mode::Search {
                self.mode = Mode::Normal;
            }
            return unfocus();
        }
        // While the terminal has the keyboard, Brain's shortcuts are off: everything is Claude's.
        if self.tab == DetailTab::Terminal && self.focused_terminal.is_some() && self.has_terminal() {
            return Task::none();
        }
        if captured {
            return Task::none();
        }

        match self.mode {
            Mode::Cleanup => {
                self.on_cleanup_key(named, character.as_deref(), cmd);
                return Task::none();
            }
            Mode::NewSession => return self.on_new_session_key(named, character.as_deref(), cmd, modifiers.shift()),
            Mode::Reply | Mode::Rename => {
                // The field lost its focus (a click elsewhere): give it back instead of acting.
                return operation::focus(if self.mode == Mode::Reply { REPLY } else { RENAME });
            }
            Mode::Search if !matches!(named, Some(Named::ArrowUp | Named::ArrowDown | Named::Enter)) => {
                // Not captured, so the field isn't focused any more: keys act on the list again.
                self.mode = Mode::Normal;
            }
            _ => {}
        }

        if cmd && character.as_deref() == Some("f") {
            return self.start_find();
        }
        if cmd && modifiers.shift() && character.as_deref() == Some("c") {
            return self.act(Action::CopyAnswer);
        }
        if cmd && character.as_deref() == Some("n") {
            return self.open_new_session_dialog();
        }
        if cmd && named == Some(Named::Backspace) {
            self.trash_selected(false);
            return Task::none();
        }
        if cmd && character.as_deref() == Some("c") && self.prefs.layout == Layout::Today {
            return self.act(Action::CopyDigest);
        }
        if cmd || modifiers.control() {
            return Task::none();
        }

        let list: Vec<SessionKey> = self.groups(now_ms()).navigable(self.include_ended()).iter().map(|s| s.key.clone()).collect();
        let current = self.selected.as_ref().and_then(|k| list.iter().position(|l| l == k));
        let next = current.map_or(0, |i| (i + 1).min(list.len().saturating_sub(1)));
        let previous = current.map_or(0, |i| i.saturating_sub(1));

        match named {
            Some(Named::ArrowDown) => return self.select_index(&list, next),
            Some(Named::ArrowUp) => return self.select_index(&list, previous),
            // On the Terminal tab ⏎ gives the terminal the keyboard (starting it if needed).
            Some(Named::Enter) if self.tab == DetailTab::Terminal && (self.has_terminal() || self.terminal_command().is_some()) => return self.open_terminal(true),
            Some(Named::Enter) if modifiers.alt() => return self.act(Action::OpenInITerm),
            Some(Named::Enter) => return self.act(Action::Open),
            Some(Named::Tab) => {
                self.cycle_filter(modifiers.shift());
                return Task::none();
            }
            Some(Named::Backspace) => {
                self.hide_selected(false);
                return Task::none();
            }
            Some(Named::ArrowRight) => {
                self.tab = match self.tab {
                    DetailTab::Messages => DetailTab::Timeline,
                    DetailTab::Timeline => DetailTab::Changes,
                    DetailTab::Changes => DetailTab::Files,
                    DetailTab::Files => DetailTab::Terminal,
                    DetailTab::Terminal => DetailTab::Messages,
                };
                return Task::batch([self.load_changes(), self.load_files()]);
            }
            Some(Named::ArrowLeft) => {
                self.tab = match self.tab {
                    DetailTab::Messages => DetailTab::Terminal,
                    DetailTab::Timeline => DetailTab::Messages,
                    DetailTab::Changes => DetailTab::Timeline,
                    DetailTab::Files => DetailTab::Changes,
                    DetailTab::Terminal => DetailTab::Files,
                };
                return Task::batch([self.load_changes(), self.load_files()]);
            }
            _ => {}
        }
        if text.as_deref() == Some("/") {
            return self.start_search();
        }
        let Some(character) = character else { return Task::none() };
        match character.as_str() {
            "j" => return self.select_index(&list, next),
            "k" => return self.select_index(&list, previous),
            "e" => self.toggle_ended(),
            "r" => return self.start_rename(),
            "t" => return self.start_reply(),
            "y" => self.answer_permission(true),
            "n" => self.answer_permission(false),
            "p" => self.toggle_pin(),
            "m" => self.toggle_mute(),
            "s" => self.cycle_snooze(),
            "a" => return self.move_to_other_account(false),
            "x" => self.end_selected(false),
            "c" => {
                self.armed = None;
                self.mode = Mode::Cleanup;
            }
            "i" => return self.act(Action::TakeOver),
            "h" => self.show_resting = !self.show_resting,
            "b" => self.toggle_background(),
            "u" => self.toggle_automated(),
            "g" => {
                self.prefs.layout = if self.prefs.layout == Layout::Projects { Layout::Status } else { Layout::Projects };
                self.prefs.save();
            }
            "d" => {
                self.prefs.layout = if self.prefs.layout == Layout::Today { Layout::Status } else { Layout::Today };
                self.prefs.save();
            }
            digit if digit.len() == 1 && ('1'..='9').contains(&digit.chars().next().unwrap()) => {
                let index = digit.parse::<usize>().unwrap() - 1;
                if index < list.len() {
                    let scroll = self.select_index(&list, index);
                    self.open_selected();
                    return scroll;
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Esc: leaves whatever is open — search (cleared), reply, rename, a dialog.
    fn escape(&mut self) -> Task<Message> {
        if self.overview {
            self.overview = false;
            return Task::none();
        }
        if self.mode == Mode::Normal && (!self.find.is_empty() || self.prompts_only) {
            self.find.clear();
            self.prompts_only = false;
            self.expanded = None;
            return unfocus();
        }
        if self.mode == Mode::Normal && self.reader.is_some() {
            self.reader = None;
            return Task::none();
        }
        match self.mode {
            Mode::Search => {
                self.search.clear();
                self.mode = Mode::Normal;
                unfocus()
            }
            Mode::Reply | Mode::Rename | Mode::NewSession | Mode::Cleanup | Mode::Palette => {
                self.mode = Mode::Normal;
                unfocus()
            }
            Mode::Normal if !self.search.is_empty() => {
                self.search.clear();
                Task::none()
            }
            Mode::Normal => Task::none(),
        }
    }

    fn start_search(&mut self) -> Task<Message> {
        self.mode = Mode::Search;
        operation::focus(SEARCH)
    }

    fn select_first_visible(&mut self) {
        let first = self.groups(now_ms()).navigable(self.include_ended()).first().map(|s| s.key.clone());
        if first.is_some() {
            self.selected = first;
        }
    }

    fn select_index(&mut self, list: &[SessionKey], index: usize) -> Task<Message> {
        match list.get(index) {
            Some(key) => {
                self.selected = Some(key.clone());
                self.scroll_to_selected()
            }
            None => Task::none(),
        }
    }

    fn cycle_filter(&mut self, backwards: bool) {
        let mut options: Vec<Option<String>> = vec![None];
        options.extend(self.model.accounts.iter().map(|a| Some(a.id.clone())));
        let at = options.iter().position(|o| *o == self.filter).unwrap_or(0);
        let next = if backwards { (at + options.len() - 1) % options.len() } else { (at + 1) % options.len() };
        self.filter = options[next].clone();
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
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

    /// The hotkey: with Brain in front and a terminal typing, Brain hides again; otherwise it
    /// comes forward with the longest-waiting session that runs (or can run) here, keyboard in
    /// its terminal.
    fn quick_terminal(&mut self) -> Task<Message> {
        let Some(mtm) = objc2::MainThreadMarker::new() else { return Task::none() };
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if app.isActive() && !app.isHidden() && self.focused_terminal.is_some() {
            app.hide(None);
            return Task::none();
        }
        app.unhide(None);
        let groups = self.groups(now_ms());
        let waiting = groups
            .attention
            .iter()
            .chain(&groups.pinned)
            .filter(|s| matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn))
            .find(|s| self.terminals.contains_key(&s.key) || s.agent.is_some())
            .map(|s| s.key.clone());
        if let Some(key) = waiting {
            self.selected = Some(key);
        }
        self.overview = false;
        self.mode = Mode::Normal;
        self.tab = DetailTab::Terminal;
        Task::batch([self.bring_forward(), self.open_terminal(true)])
    }

    /// Brings Brain's window to the front (notification click, menu bar, `brain://` link).
    fn bring_forward(&self) -> Task<Message> {
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
        }
        window::latest().and_then(window::gain_focus)
    }

    // ---- actions ----------------------------------------------------------------

    /// A button or a key. Actions that ask to be pressed twice ask here.
    fn act(&mut self, action: Action) -> Task<Message> {
        self.perform(action, false)
    }

    /// `confirmed`: the user already chose deliberately (a menu click), so actions that
    /// normally want a second press run at once.
    fn perform(&mut self, action: Action, confirmed: bool) -> Task<Message> {
        match action {
            // A session that runs in Brain opens here; everything else in its terminal app.
            // Whatever Brain can run opens in its terminal: background sessions attach, ended
            // ones resume. Sessions that live in an iTerm tab come forward there; ⌥⏎ goes to iTerm.
            Action::Open | Action::Resume if self.has_terminal() || self.terminal_command().is_some() => {
                self.tab = DetailTab::Terminal;
                return self.open_terminal(true);
            }
            Action::Open => self.open_selected(),
            Action::OpenInITerm => self.open_selected(),
            Action::TakeOver => self.take_over(confirmed),
            Action::Pin => self.toggle_pin(),
            Action::OpenBeside => {
                // The terminal that was on screen stays on the left, this one opens beside it.
                if let Some(kept) = self.before_menu.take().filter(|k| self.terminals.contains_key(k)) {
                    self.split = Some(kept);
                }
                self.overview = false;
                self.tab = DetailTab::Terminal;
                return self.open_terminal(true);
            }
            Action::CopyAnswer => {
                let answer = self.conversation.as_ref().and_then(|c| brain_core::history::last_answer(&c.messages)).map(|m| m.text.clone());
                return match answer {
                    Some(answer) => {
                        self.set_status(t("Letzte Antwort kopiert.", "Copied the last answer."));
                        iced::clipboard::write(answer)
                    }
                    None => {
                        self.set_status(t("Diese Session hat noch keine Antwort.", "This session has no answer yet."));
                        Task::none()
                    }
                };
            }
            Action::Export => return self.export_selected(),
            Action::Mute => self.toggle_mute(),
            Action::Snooze => self.cycle_snooze(),
            Action::OtherAccount => return self.move_to_other_account(confirmed),
            Action::End => self.end_selected(confirmed),
            Action::Hide => self.hide_selected(confirmed),
            Action::Trash => self.trash_selected(confirmed),
            Action::Resume => self.resume_selected(),
            Action::Allow => self.answer_permission(true),
            Action::Deny => self.answer_permission(false),
            Action::StartReply => return self.start_reply(),
            Action::StartRename => return self.start_rename(),
            Action::StartTerminal => return self.open_terminal(true),
            Action::CopyDigest => {
                let date = chrono::Local::now().format("%d.%m.%Y").to_string();
                let markdown = brain_core::digest::markdown(&tr!("Heute, {date}", "Today, {date}"), &self.today_digest());
                self.set_status(t("Tagesübersicht kopiert.", "Copied the day's digest."));
                return iced::clipboard::write(markdown);
            }
        }
        Task::none()
    }

    /// ⏎: a live session's terminal comes forward; an ended one is resumed in a new tab.
    fn open_selected(&mut self) {
        let Some(session) = self.selected_session() else { return };
        if let Some(agent) = session.agent.clone() {
            let (account, cwd) = (session.key.account.clone(), session.cwd.clone());
            self.attach_agent(&account, &agent, cwd);
            return;
        }
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
        let config_dir = self.model.account(&session.key.account).filter(|a| a.id != "main").map(|a| a.config_dir.display().to_string());
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

    /// ⏎ on a background session: `claude attach` in a new tab.
    fn attach_agent(&mut self, account: &str, agent: &brain_core::agents::BackgroundAgent, cwd: Option<String>) {
        let config_dir = self.model.account(account).filter(|a| a.id != "main").map(|a| a.config_dir.display().to_string());
        let cwd = cwd.unwrap_or_else(|| brain_core::account::home_dir().display().to_string());
        if self.blocked_in_demo() {
            return;
        }
        let outcome = terminal::open_new(&cwd, config_dir.as_deref(), &format!("claude attach {}", agent.id));
        self.report(outcome, Some(t("Hintergrund-Session in einem neuen Tab geöffnet.", "Opened the background session in a new tab.").into()));
    }

    fn start_rename(&mut self) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        if !session.accepts_input() {
            self.set_status(t(
                "Umbenennen geht nur, wenn die Session fertig ist und auf dich wartet.",
                "Renaming works only while the session has finished and waits for you.",
            ));
            return Task::none();
        }
        if !self.capabilities().type_text {
            self.set_status(t("In dieses Terminal kann Brain nicht tippen.", "Brain can't type into this terminal."));
            return Task::none();
        }
        self.rename = session.display_name();
        self.mode = Mode::Rename;
        Task::batch([operation::focus(RENAME), operation::select_all(RENAME)])
    }

    fn submit_rename(&mut self) -> Task<Message> {
        self.mode = Mode::Normal;
        let name = self.rename.trim().to_string();
        if !name.is_empty() && !self.blocked_in_demo() {
            self.type_into_selected(&format!("/rename {name}"), tr!("„{name}“ an die Session geschickt.", "Sent “{name}” to the session."));
        }
        unfocus()
    }

    fn start_reply(&mut self) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        if !session.accepts_input() {
            self.set_status(t(
                "Antworten geht, sobald die Session fertig ist und auf dich wartet.",
                "You can reply once the session has finished and waits for you.",
            ));
            return Task::none();
        }
        if !self.capabilities().type_text {
            self.set_status(t("In dieses Terminal kann Brain nicht tippen.", "Brain can't type into this terminal."));
            return Task::none();
        }
        self.reply.clear();
        self.mode = Mode::Reply;
        operation::focus(REPLY)
    }

    fn submit_reply(&mut self) -> Task<Message> {
        self.mode = Mode::Normal;
        let text = self.reply.trim().to_string();
        if !text.is_empty() && !self.blocked_in_demo() {
            self.type_into_selected(&text, t("Antwort geschickt.", "Reply sent.").into());
        }
        self.reply.clear();
        unfocus()
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
            self.set_status(t("In diesem Terminal kann Brain keine Freigaben beantworten.", "Brain can't answer permissions in this terminal."));
            return;
        }
        let pid = session.pid;
        if self.blocked_in_demo() {
            return;
        }
        let (key, done) = if allow { (Key::Return, t("Freigabe erteilt.", "Allowed.")) } else { (Key::Escape, t("Freigabe abgelehnt.", "Denied.")) };
        let outcome = terminal::send_key(pid, key);
        self.report(outcome, Some(done.into()));
    }

    fn toggle_pin(&mut self) {
        let Some(key) = self.selected.clone() else { return };
        let ended = self.model.board.get(&key).is_some_and(|s| s.phase() == Phase::Ended);
        let pinned = self.prefs.toggle_pin(&key);
        self.prefs.save();
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

    /// The first press of a two-press action arms it; returns whether this press is the second,
    /// within 5 s, on the same session.
    fn arm(&mut self, action: char, key: &SessionKey) -> bool {
        let confirmed = self.armed.as_ref().is_some_and(|(a, k, at)| *a == action && k == key && at.elapsed() < Duration::from_secs(5));
        self.armed = if confirmed { None } else { Some((action, key.clone(), Instant::now())) };
        confirmed
    }

    /// `X` twice: ends a waiting session with `/exit` (a background one with `claude stop`).
    fn end_selected(&mut self, confirmed: bool) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        if let Some(agent) = session.agent.clone().filter(|a| a.is_active()) {
            if !confirmed && !self.arm('x', &key) {
                self.set_status(t("Nochmal X stoppt die Hintergrund-Session (das Gespräch bleibt).", "Press X again to stop the background session (its conversation is kept)."));
                return;
            }
            if self.blocked_in_demo() {
                return;
            }
            let Some(account) = self.model.account(&key.account).cloned() else { return };
            match brain_core::agents::stop(&account, &agent.id) {
                Ok(()) => {
                    self.model.relist_transcripts();
                    self.set_status(t("Hintergrund-Session gestoppt.", "Stopped the background session."));
                }
                Err(err) => self.set_status(tr!("Stoppen fehlgeschlagen: {err}", "Stopping failed: {err}")),
            }
            return;
        }
        if session.phase() == Phase::Ended {
            self.set_status(t("Die Session ist schon beendet.", "The session has already ended."));
            return;
        }
        if !session.accepts_input() {
            self.set_status(t("Beenden geht, sobald die Session fertig ist und auf dich wartet.", "Ending works once the session has finished and waits for you."));
            return;
        }
        if !confirmed && !self.arm('x', &key) {
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
    /// A session that runs in Brain moves within Brain: it stops here and a copy continues in
    /// the other account's background, attached in Brain.
    fn move_to_other_account(&mut self, confirmed: bool) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        let key = session.key.clone();
        let Some(index) = self.model.accounts.iter().position(|a| a.id == key.account) else { return Task::none() };
        let next = self.model.accounts.get((index + 1) % self.model.accounts.len()).cloned();
        let Some(target) = next.filter(|t| t.id != key.account) else {
            self.set_status(t("Es gibt kein zweites Konto.", "There is no other account."));
            return Task::none();
        };
        let ended = session.phase() == Phase::Ended;
        // One that runs in Brain is stopped, so it only must not be mid-turn; one in iTerm gets
        // `/exit` typed, so its prompt box must be ready.
        let movable = ended || if session.agent.is_some() { session.phase() != Phase::Working } else { session.accepts_input() };
        if !movable {
            self.set_status(t("Umziehen geht, sobald die Session fertig ist und auf dich wartet.", "Moving works once the session has finished and waits for you."));
            return Task::none();
        }
        let (session_id, cwd, pid, name) = (session.session_id.clone(), session.cwd.clone(), session.pid, session.name.clone());
        let running_agent = session.agent.clone().filter(|a| a.is_active());
        let in_brain = session.agent.is_some();
        if !confirmed && !self.arm('a', &key) {
            let target = target.id.clone();
            self.set_status(if ended {
                tr!("Nochmal A setzt die Session in {target} fort.", "Press A again to resume the session in {target}.")
            } else {
                tr!("Nochmal A zieht die Session nach {target} um.", "Press A again to move the session to {target}.")
            });
            return Task::none();
        }
        let (Some(session_id), Some(cwd)) = (session_id, cwd) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder."));
            return Task::none();
        };
        if self.blocked_in_demo() {
            return Task::none();
        }
        let Some(source) = self.model.account(&key.account).cloned() else { return Task::none() };
        let Some(transcript) = brain_core::transcript::find_transcript(&source, &session_id) else {
            self.set_status(t("Transkript nicht gefunden.", "Transcript not found."));
            return Task::none();
        };
        if in_brain {
            if self.prefs.is_pinned(&key) && ended {
                self.prefs.toggle_pin(&key);
                self.prefs.save();
            }
            let target_id = target.id.clone();
            self.set_status(tr!("Ziehe die Session nach {target_id} um …", "Moving the session to {target_id} …"));
            let moved = off_thread(move || {
                if let Some(agent) = running_agent {
                    brain_core::agents::stop(&source, &agent.id)?;
                }
                brain_core::transcript::copy_to_account(&transcript, &target).map_err(|e| e.to_string())?;
                brain_core::agents::fork(&target, std::path::Path::new(&cwd), &session_id, name.as_deref())
            });
            return Task::perform(moved, move |result| Message::Started(target_id.clone(), result));
        }
        if let Err(err) = brain_core::transcript::copy_to_account(&transcript, &target) {
            self.set_status(tr!("Kopieren fehlgeschlagen: {err}", "Copy failed: {err}"));
            return Task::none();
        }
        let config_dir = (target.id != "main").then(|| target.config_dir.display().to_string());
        let outcome = terminal::open_new(&cwd, config_dir.as_deref(), &format!("claude --resume {session_id} --fork-session"));
        if !matches!(outcome, Outcome::Done) {
            self.report(outcome, None);
            return Task::none();
        }
        if self.prefs.is_pinned(&key) && ended {
            self.prefs.toggle_pin(&key);
            self.prefs.save();
        }
        let target = target.id;
        if ended {
            self.set_status(tr!("In {target} fortgesetzt.", "Resumed in {target}."));
            return Task::none();
        }
        let closed = matches!(terminal::type_text(pid, "/exit"), Outcome::Done);
        self.set_status(if closed {
            tr!("Nach {target} umgezogen, das Original ist geschlossen.", "Moved to {target}; the original is closed.")
        } else {
            tr!("Nach {target} umgezogen. Das Original konnte Brain nicht schließen.", "Moved to {target}. Brain could not close the original.")
        });
        Task::none()
    }

    /// `⌫` twice: hides the session (or shows it again) until something new happens in it.
    fn hide_selected(&mut self, confirmed: bool) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        if self.prefs.is_hidden(&key, session.last_activity_ms) {
            self.prefs.unhide(&key);
            self.prefs.save();
            self.set_status(t("Wieder eingeblendet.", "Shown again."));
            return;
        }
        if !confirmed && !self.arm('h', &key) {
            self.set_status(t("Nochmal ⌫ blendet die Session aus, bis sich in ihr etwas tut.", "Press ⌫ again to hide the session until something happens in it."));
            return;
        }
        self.prefs.hide(&key, now_ms());
        self.prefs.save();
        self.selected = None;
        self.set_status(t("Ausgeblendet – über / findest du sie wieder.", "Hidden – / finds it again."));
    }

    /// `⌘⌫` twice: an ended session's transcript goes to the Trash; a background session is
    /// removed with `claude rm` after its transcript went to the Trash.
    fn trash_selected(&mut self, confirmed: bool) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        let deletable = session.phase() == Phase::Ended || session.agent.as_ref().is_some_and(|a| !a.is_active());
        if !deletable {
            self.set_status(t("Löschen geht nur bei beendeten Sessions – erst X X.", "Only ended sessions can be deleted – end it with X X first."));
            return;
        }
        if !confirmed && !self.arm('d', &key) {
            self.set_status(t("Nochmal ⌘⌫ legt die Session in den Papierkorb.", "Press ⌘⌫ again to move the session to the Trash."));
            return;
        }
        if self.blocked_in_demo() {
            return;
        }
        match self.delete_session(&key) {
            Ok(()) => self.set_status(t("In den Papierkorb gelegt.", "Moved to the Trash.")),
            Err(err) => self.set_status(tr!("Löschen fehlgeschlagen: {err}", "Deleting failed: {err}")),
        }
    }

    /// Moves a session's transcript and its folder to the Trash, removes a background session
    /// with `claude rm`, and forgets the session.
    fn delete_session(&mut self, key: &SessionKey) -> Result<(), String> {
        let session = self.model.board.get(key).ok_or("unknown session")?;
        let account = self.model.account(&key.account).cloned().ok_or("unknown account")?;
        let agent = session.agent.clone();
        // A background session's process writes its last lines when it exits: stop it first, so
        // nothing is written after its transcript went to the Trash. (Already stopped is fine.)
        if let Some(agent) = &agent {
            let _ = brain_core::agents::stop(&account, &agent.id);
        }
        if let Some(session_id) = session.session_id.clone() {
            for path in brain_core::transcript::session_files(&account, &session_id) {
                crate::trash::move_to_trash(&path)?;
            }
        }
        if let Some(agent) = agent {
            brain_core::agents::remove(&account, &agent.id)?;
        }
        self.prefs.forget(key);
        self.prefs.save();
        self.model.board.remove(key);
        self.model.relist_transcripts();
        if self.selected.as_ref() == Some(key) {
            self.selected = None;
        }
        Ok(())
    }

    // ---- clean-up dialog ----------------------------------------------------------

    /// Sessions not pinned that ended (or are background sessions) and saw nothing for the
    /// chosen number of days.
    pub fn cleanup_candidates(&self) -> Vec<&Session> {
        let cutoff = now_ms() - CLEANUP_DAYS[self.cleanup_age] * 24 * 60 * 60 * 1000;
        self.model
            .board
            .sorted()
            .into_iter()
            .filter(|s| self.filter.as_ref().is_none_or(|f| *f == s.key.account))
            .filter(|s| !self.prefs.is_pinned(&s.key))
            .filter(|s| s.agent.is_none() || self.prefs.show_background)
            .filter(|s| s.phase() == Phase::Ended || s.agent.is_some())
            .filter(|s| s.last_activity_ms < cutoff)
            .filter(|s| s.phase() != Phase::Ended || (self.model.resumable(s) && !s.is_empty()))
            .collect()
    }

    fn on_cleanup_key(&mut self, named: Option<Named>, character: Option<&str>, cmd: bool) {
        if cmd && named == Some(Named::Backspace) {
            let keys: Vec<SessionKey> = self.cleanup_candidates().iter().map(|s| s.key.clone()).collect();
            if keys.is_empty() || !self.arm('d', &SessionKey { account: String::new(), id: "cleanup".into() }) {
                if !keys.is_empty() {
                    let n = keys.len();
                    self.set_status(tr!("Nochmal ⌘⌫ legt {n} Sessions in den Papierkorb.", "Press ⌘⌫ again to move {n} sessions to the Trash."));
                }
                return;
            }
            if self.blocked_in_demo() {
                return;
            }
            let (mut done, mut failed) = (0, Vec::new());
            for key in keys {
                match self.delete_session(&key) {
                    Ok(()) => done += 1,
                    Err(err) => failed.push(err),
                }
            }
            self.mode = Mode::Normal;
            self.set_status(match failed.first() {
                None => tr!("{done} Sessions in den Papierkorb gelegt.", "Moved {done} sessions to the Trash."),
                Some(err) => {
                    let n = failed.len();
                    tr!("{done} gelöscht, {n} fehlgeschlagen: {err}", "{done} deleted, {n} failed: {err}")
                }
            });
            return;
        }
        if named == Some(Named::Tab) {
            self.cleanup_age = (self.cleanup_age + 1) % CLEANUP_DAYS.len();
        } else if !cmd && character == Some("h") {
            let keys: Vec<SessionKey> = self.cleanup_candidates().iter().map(|s| s.key.clone()).collect();
            let n = keys.len();
            for key in &keys {
                self.prefs.hide(key, now_ms());
            }
            self.prefs.save();
            self.mode = Mode::Normal;
            self.set_status(tr!("{n} Sessions ausgeblendet.", "Hid {n} sessions."));
        }
    }

    // ---- new-session dialog ---------------------------------------------------------

    fn open_new_session_dialog(&mut self) -> Task<Message> {
        let templates = brain_core::templates::load_all(&brain_core::templates::templates_dir(&brain_core::account::home_dir()));
        self.new_session = NewSession { templates, ..NewSession::default() };
        self.mode = Mode::NewSession;
        operation::focus(NEW_SESSION)
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
    pub fn new_session_choices(&self) -> Vec<Choice> {
        let query = self.new_session.folder.trim().to_string();
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
    pub fn pending_placeholder(&self) -> Option<String> {
        let chosen = self.new_session.chosen.as_ref()?;
        chosen.template.placeholders().into_iter().find(|name| !chosen.values.iter().any(|(n, _)| n == name))
    }

    fn on_new_session_key(&mut self, named: Option<Named>, character: Option<&str>, cmd: bool, shift: bool) -> Task<Message> {
        if cmd && character == Some("e") {
            if let Some(Choice::Template(i)) = self.new_session_choices().get(self.new_session.pick).cloned() {
                open_in_editor(&self.new_session.templates[i].path);
            }
            return Task::none();
        }
        if cmd && character == Some("i") {
            self.new_session.in_iterm = !self.new_session.in_iterm;
            return Task::none();
        }
        if cmd && shift && character == Some("n") {
            let dir = brain_core::templates::templates_dir(&brain_core::account::home_dir());
            let prompt = t(
                "Beschreibe hier die Aufgabe. Platzhalter wie {ticket} fragt Brain beim Start ab.",
                "Describe the task here. Brain asks for placeholders like {ticket} when it starts.",
            );
            match brain_core::templates::create(&dir, t("Neue Vorlage", "New template"), prompt) {
                Ok(path) => {
                    open_in_editor(&path);
                    self.set_status(t("Vorlage angelegt – nach dem Speichern ⌘N erneut öffnen.", "Template created – reopen ⌘N after saving it."));
                    self.mode = Mode::Normal;
                    return unfocus();
                }
                Err(err) => self.set_status(err.to_string()),
            }
            return Task::none();
        }
        let count = self.new_session_choices().len();
        match named {
            Some(Named::ArrowDown) => self.new_session.pick = (self.new_session.pick + 1).min(count.saturating_sub(1)),
            Some(Named::ArrowUp) => self.new_session.pick = self.new_session.pick.saturating_sub(1),
            Some(Named::Tab) => self.new_session.account = (self.new_session.account + 1) % self.model.accounts.len().max(1),
            // Every other key goes to the field (it lost its focus to a click).
            _ => return operation::focus(NEW_SESSION),
        }
        Task::none()
    }

    /// ⏎ in the dialog: pick a template or folder, take a placeholder value, or start.
    fn confirm_new_session(&mut self) -> Task<Message> {
        if let Some(name) = self.pending_placeholder() {
            let value = self.new_session.folder.trim().to_string();
            if let Some(chosen) = self.new_session.chosen.as_mut() {
                chosen.values.push((name, value));
            }
            self.new_session.folder.clear();
            self.new_session.pick = 0;
            let has_folder = self.new_session.chosen.as_ref().is_some_and(|c| c.template.folder.is_some());
            if self.pending_placeholder().is_none() && has_folder {
                return self.start_new_session(None);
            }
            return Task::none();
        }
        match self.new_session_choices().get(self.new_session.pick).cloned() {
            Some(Choice::Template(i)) => {
                let template = self.new_session.templates[i].clone();
                if let Some(index) = template.account.as_ref().and_then(|id| self.model.accounts.iter().position(|a| a.id == *id)) {
                    self.new_session.account = index;
                }
                let ready = template.folder.is_some() && template.placeholders().is_empty();
                self.new_session.chosen = Some(Chosen { template, values: Vec::new() });
                self.new_session.folder.clear();
                self.new_session.pick = 0;
                if ready {
                    return self.start_new_session(None);
                }
                Task::none()
            }
            Some(Choice::Folder(folder)) => self.start_new_session(Some(folder)),
            None => {
                self.set_status(t("Wähle einen Ordner oder tippe einen Pfad.", "Pick a folder or type a path."));
                Task::none()
            }
        }
    }

    /// Opens a new tab running `claude` (with the template's model and filled-in prompt, if any).
    fn start_new_session(&mut self, folder: Option<String>) -> Task<Message> {
        let chosen = self.new_session.chosen.as_ref();
        let folder = folder.or_else(|| chosen.and_then(|c| c.template.folder.clone())).map(|f| {
            let home = brain_core::account::home_dir().display().to_string();
            match f.strip_prefix('~') {
                Some(rest) => format!("{home}{rest}"),
                None => f,
            }
        });
        let Some(folder) = folder else { return Task::none() };
        if !std::path::Path::new(&folder).is_dir() {
            self.set_status(tr!("Ordner nicht gefunden: {folder}", "Folder not found: {folder}"));
            return Task::none();
        }
        let model = chosen.and_then(|c| c.template.model.clone());
        let prompt = chosen.map(|c| c.template.fill(&c.values)).filter(|p| !p.is_empty());
        let name = chosen.map(|c| c.template.name.clone());
        let in_iterm = self.new_session.in_iterm;
        self.mode = Mode::Normal;
        let Some(account) = self.model.accounts.get(self.new_session.account).cloned() else { return unfocus() };
        if self.blocked_in_demo() {
            return unfocus();
        }
        if in_iterm {
            let mut command = String::from("claude");
            if let Some(model) = &model {
                command.push_str(&format!(" --model {}", shell_quote(model)));
            }
            if let Some(prompt) = &prompt {
                command.push_str(&format!(" {}", shell_quote(prompt)));
            }
            let config_dir = (account.id != "main").then(|| account.config_dir.display().to_string());
            let outcome = terminal::open_new(&folder, config_dir.as_deref(), &command);
            self.report(outcome, Some(t("Neue Session in iTerm gestartet.", "Started a new session in iTerm.").into()));
            return unfocus();
        }
        self.set_status(t("Starte die Session in Brain …", "Starting the session in Brain …"));
        let id = account.id.clone();
        let started = off_thread(move || {
            brain_core::agents::start(&account, std::path::Path::new(&folder), model.as_deref(), name.as_deref(), prompt.as_deref())
        });
        Task::batch([unfocus(), Task::perform(started, move |result| Message::Started(id.clone(), result))])
    }

    /// `I` twice: moves a session running in an iTerm tab into Brain. Brain sends it `/exit`,
    /// waits until it ended, and continues it in the background under the same id.
    fn take_over(&mut self, confirmed: bool) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        if session.agent.is_some() {
            self.set_status(t("Diese Session läuft schon in Brain.", "This session already runs in Brain."));
            return;
        }
        let ended = session.phase() == Phase::Ended;
        if !ended && !session.accepts_input() {
            self.set_status(t("Übernehmen geht, sobald die Session auf dich wartet.", "Taking over works once the session waits for you."));
            return;
        }
        if !confirmed && !self.arm('i', &key) {
            self.set_status(t("Nochmal I holt die Session nach Brain (das iTerm-Tab wird beendet).", "Press I again to move the session into Brain (its iTerm tab ends)."));
            return;
        }
        if self.blocked_in_demo() {
            return;
        }
        if !ended {
            self.type_into_selected("/exit", t("Beende die Session in iTerm …", "Ending the session in iTerm …").into());
        }
        self.takeover = Some(key);
    }

    /// Once a taken-over session's iTerm process ended: continue it in the background.
    fn continue_takeover(&mut self) -> Task<Message> {
        let Some(key) = self.takeover.clone() else { return Task::none() };
        let Some(session) = self.model.board.get(&key) else {
            self.takeover = None;
            return Task::none();
        };
        if session.phase() != Phase::Ended {
            return Task::none();
        }
        self.takeover = None;
        let name = session.name.clone();
        let (Some(session_id), Some(cwd), Some(account)) = (session.session_id.clone(), session.cwd.clone(), self.model.account(&key.account).cloned()) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder."));
            return Task::none();
        };
        self.set_status(t("Setze die Session in Brain fort …", "Continuing the session in Brain …"));
        let id = account.id.clone();
        let resumed = off_thread(move || brain_core::agents::resume(&account, std::path::Path::new(&cwd), &session_id, name.as_deref()));
        Task::perform(resumed, move |result| Message::Started(id.clone(), result))
    }

    // ---- today ------------------------------------------------------------------

    /// The day's digest: what each session reported as done, per project.
    pub fn today_digest(&self) -> Vec<brain_core::digest::ProjectDay> {
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
}

/// Saved sessions Brain has no events for (they ended before its hooks, or the hooks never
/// fired for them) are loaded from their transcripts, so "saved to resume" keeps them.
fn restore_saved(model: &mut Model, prefs: &Prefs) {
    for key in prefs.pinned_keys() {
        if model.board.get(&key).is_some() {
            continue;
        }
        let Some(path) = model.account(&key.account).and_then(|a| brain_core::transcript::find_transcript(a, &key.id)) else { continue };
        let modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as i64);
        let cwd = brain_core::transcript::cwd_of(&path);
        let mut insight = brain_core::transcript::insight(&path);
        insight.title = brain_core::transcript::title_of(&path).or(insight.title);
        model.board.add_ended(key, cwd, insight, modified);
    }
}

/// Takes the keyboard focus away from every text field.
fn unfocus() -> Task<Message> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
}

/// Runs blocking work (git, gh, curl) on its own thread and hands back the result.
fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> impl std::future::Future<Output = T> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    async move { rx.await.expect("the worker thread sends exactly one result") }
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
        let pr = by_branch.entry((cwd.clone(), branch.clone())).or_insert_with(|| brain_core::github::pull_request(path, &branch)).clone();
        if let Some(pr) = pr {
            out.insert(key, pr);
        }
    }
    out
}

/// Opens a file with the app macOS uses for its type (your Markdown editor for templates).
fn open_in_editor(path: &std::path::Path) {
    let _ = std::process::Command::new("open").arg(path).spawn();
}

/// "In editor" for a file from the reader. The path came from a session's output, so it never
/// goes to plain `open` (a `.command` or `.app` would run): text opens in the default text
/// editor (`open -t`), pictures and PDFs in Preview, anything else not at all.
fn open_reader_file(reader: &Reader) {
    let extension = reader.path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut command = std::process::Command::new("open");
    match &reader.kind {
        ReaderKind::Code(..) => command.arg("-t"),
        ReaderKind::Image(_) => command.args(["-a", "Preview"]),
        ReaderKind::Unreadable(_) if extension == "pdf" => command.args(["-a", "Preview"]),
        ReaderKind::Unreadable(_) => return,
    };
    let _ = command.arg(&reader.path).spawn();
}

/// A path as a shell word, like iTerm pastes dropped files: spaces and specials backslashed.
fn escape_path(path: &str) -> String {
    let mut out = String::new();
    for c in path.chars() {
        if c.is_whitespace() || "\\'\"()[]{}&;|<>*?$`!#~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `src/app.rs:12`, `~/notes.md` or `/tmp/x.png` as an existing file, relative paths from `cwd`.
fn resolve_path(link: &str, cwd: &str) -> Option<std::path::PathBuf> {
    let trimmed = link.trim_end_matches(|c: char| c == '.' || c == ',' || c == ')');
    let without_line = match trimmed.rsplit_once(':') {
        Some((path, line)) if line.chars().all(|c| c.is_ascii_digit()) => path,
        _ => trimmed,
    };
    let home = brain_core::account::home_dir();
    let path = match without_line.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if without_line.starts_with('/') => std::path::PathBuf::from(without_line),
        None => std::path::Path::new(cwd).join(without_line),
    };
    path.is_file().then(|| path.canonicalize().unwrap_or(path))
}

/// Loads a file for the reader: Markdown rendered, images shown, other text highlighted.
/// Markdown is read in YAMV (Yet Another Markdown Viewer): math, Mermaid, footnotes and the
/// rest of extended Markdown, which Brain's own reader doesn't draw.
const YAMV: &str = "de.martinemmert.projects.yamv";

fn is_markdown(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "md" | "markdown"))
}

/// Opens a Markdown file in YAMV; without YAMV, in the default text editor.
fn open_markdown(path: std::path::PathBuf) -> Task<Message> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let opened = off_thread(move || {
        let in_yamv = std::process::Command::new("open").args(["-b", YAMV]).arg(&path).status().is_ok_and(|s| s.success());
        if !in_yamv {
            let _ = std::process::Command::new("open").arg("-t").arg(&path).status();
        }
        in_yamv
    });
    Task::perform(opened, move |in_yamv| Message::MarkdownOpened(name.clone(), in_yamv))
}

fn read_file(path: std::path::PathBuf) -> Reader {
    const LIMIT: u64 = 2 * 1024 * 1024;
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let kind = if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp") {
        ReaderKind::Image(iced::widget::image::Handle::from_path(&path))
    } else if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > LIMIT {
        ReaderKind::Unreadable(t("Die Datei ist zu groß für den Reader.", "The file is too large for the reader.").into())
    } else {
        match std::fs::read_to_string(&path) {
            Ok(text) => ReaderKind::Code(iced::widget::text_editor::Content::with_text(&text), extension),
            Err(_) if extension == "pdf" => ReaderKind::Unreadable(t("Ein PDF – ⌘⏎ öffnet es in der Vorschau.", "A PDF – ⌘⏎ opens it in Preview.").into()),
            Err(_) => ReaderKind::Unreadable(t("Keine Textdatei – der Reader zeigt sie nicht.", "Not a text file – the reader can't show it.").into()),
        }
    };
    Reader { path, kind }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_paths_are_escaped_like_iterm_does() {
        assert_eq!(escape_path("/Users/me/Desktop/Bildschirmfoto 2026-10-09 um 19.01.png"), "/Users/me/Desktop/Bildschirmfoto\\ 2026-10-09\\ um\\ 19.01.png");
        assert_eq!(escape_path("/tmp/a(1).png"), "/tmp/a\\(1\\).png");
    }

    #[test]
    fn printed_paths_resolve_from_the_session_folder_with_or_without_a_line() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/app.rs"), "x").unwrap();
        let cwd = dir.path().display().to_string();
        let file = dir.path().join("src/app.rs").canonicalize().unwrap();
        assert_eq!(resolve_path("src/app.rs:12", &cwd), Some(file.clone()));
        assert_eq!(resolve_path("src/app.rs.", &cwd), Some(file));
        assert_eq!(resolve_path("src/missing.rs", &cwd), None);
    }
}
