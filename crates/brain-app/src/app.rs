//! Brain's state and behaviour: what the keys and buttons do, what runs in the background, and
//! what the list shows. Drawing lives in `ui`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use brain_core::state::{Ask, Phase, Session, SessionKey};
use brain_terminal::{self as terminal, Capabilities, Key, Outcome};
use iced::keyboard::{self, key::Named};
use iced::widget::{operation, scrollable};
use iced::{window, Subscription, Task};

use crate::config;

mod account;
pub(crate) mod relaunch;
pub use relaunch::{Relaunch, RELAUNCH_FLAGS};
mod sync;
mod garden;
mod link;
mod menus;

pub use account::ACCOUNT_NAME;
use crate::conversation::Conversation;
use crate::format::{ago, clock, now_ms, plain, shell_quote};
use crate::i18n::t;
use crate::menubar::MenuBar;
pub use menus::MenuCommand;
use crate::model::Model;
use crate::notify::{self, Notifier};
use crate::prefs::Prefs;
use crate::tree::{self, Tree, Views};
use crate::tr;

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
pub const RENAME: &str = "rename";
pub const NEW_SESSION: &str = "new-session";
pub const LIST: &str = "session-list";
pub const FIND: &str = "find";
pub const PALETTE: &str = "palette";
pub const COMPOSER: &str = "composer";

/// Where typed keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    Rename,
    NewSession,
    Cleanup,
    /// Tasks noted for later (`/later`, `brain later`).
    Later,
    /// The command palette (⌘K).
    Palette,
    /// The composer (⌘E): a longer prompt for the selected session.
    Composer,
    /// The code a phone scans to pair with Brain Link.
    Pairing,
    /// A new Claude account made from the existing ones.
    AccountSetup,
    /// Where and how the selected session goes on: account, fork, flags (`A`, `F`, `I`).
    Relaunch,
    /// What syncing the accounts would do, and the user's choice where they conflict.
    Sync,
    /// `?`: every key.
    Keys,
    /// The gardener: broken sessions to end for good.
    Garden,
}

/// What a line of the command palette does.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteCommand {
    Act(Action),
    /// Text (a slash command, a prompt) typed into the selected session.
    Send(String),
    /// A prompt typed into every session that waits for its turn.
    Broadcast(String),
    NewSession,
    Find,
    Cleanup,
    Later,
    ToggleBackground,
    ToggleChanges,
    /// Show or hide sessions programs started (SDK reviews, `claude -p`).
    ToggleAutomated,
    Split,
    Overview,
    Composer,
    Shell,
    /// Turn Brain Link on and show the pairing code.
    PairPhone,
    /// Set up a new Claude account from the existing ones.
    SetUpAccount,
    /// Bring the accounts' setup and memory in step.
    SyncAccounts,
    /// The gardener: sessions Brain can't steer any more.
    Garden,
    /// Today's log in the editor.
    OpenLog,
    /// The selected session into this project, or back into its own (`None`).
    Project(Option<String>),
    /// Leave this project out of the list, or bring it back.
    HideProject(String),
    /// Show every key.
    Keys,
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

/// The whole conversation of one session, as of a transcript size.
pub struct HistoryCache {
    pub key: SessionKey,
    pub len: u64,
    pub messages: Vec<brain_core::transcript::Message>,
}

/// The files the selected session changed, as of a transcript size.
pub struct ChangesCache {
    pub key: SessionKey,
    pub len: u64,
    pub loaded: Instant,
    pub files: Vec<brain_core::changes::SessionChange>,
}

/// The floating card over the terminal with one changed file's diff.
pub struct Preview {
    pub key: SessionKey,
    pub change: brain_core::changes::SessionChange,
    /// The diff's lines without git's file header; `None` while it is read.
    pub lines: Option<Vec<(brain_core::changes::DiffKind, String)>>,
    /// Lines left out because the diff is longer than the card shows.
    pub cut: usize,
    /// The file is still there to open in the editor.
    pub exists: bool,
}

/// The most diff lines the card draws.
const PREVIEW_LINES: usize = 1200;

/// The reading of a session's files is repeated at most this often while its transcript grows.
const CHANGES_EVERY: Duration = Duration::from_secs(4);

/// Buttons in the detail pane and the list's context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Resume,
    Allow,
    Deny,
    StartRename,
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
    /// Continue the selected session in this account (its index among the accounts).
    MoveTo(usize),
    /// Fork the selected session into this account; the original goes on.
    ForkTo(usize),
    /// The account dialog, opened for a move or a restart with flags.
    Relaunch,
    RestartWith,
    End,
    Hide,
    Trash,
    /// End a session that won't end, by force (the gardener's way).
    ForceEnd,
    /// ⌘K with the selected session's projects to move it to.
    MoveToProject,
    /// Keep the project of a session from the others as a project of the tree.
    KeepProject,
    /// A guess at a question counts as a report: no amber line any more.
    OnlyReport,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Key(keyboard::Event, bool),
    Select(SessionKey),
    OpenEntry(SessionKey),
    /// A right click on a list entry: select it, open its menu at the pointer.
    ContextMenu(SessionKey),
    /// A right click on a project: its menu opens at the pointer.
    ProjectMenu(String),
    /// "+ Neues Projekt …": the folder dialog, the folder's project joins the tree.
    NewProject,
    /// A new session in this project (its folder is the dialog's first choice).
    NewSessionIn(String),
    /// Fold or open a project's "Für später" notes.
    ToggleNotes(String),
    /// A note for later starts as a new session; check it off.
    StartNote(String),
    CheckOffNote(String),
    /// A choice in the context menu; the click is the confirmation keys get by pressing twice.
    MenuPick(Action),
    CloseMenu,
    /// The pointer over the list, in window coordinates (where a context menu opens).
    Pointer(iced::Point),
    WindowSized(iced::Size),
    Hover(Option<SessionKey>),
    /// The pointer over a project's head (or "Weitere"): its "+" and fold arrow show.
    HoverHead(Option<crate::tree::HeadKey>),
    Search(String),
    SearchSubmit,
    Rename(String),
    RenameSubmit,
    NewInput(String),
    /// "Choose folder …": the native folder dialog.
    NewChooseFolder,
    NewSubmit,
    NewAccount(usize),
    NewPick(usize),
    /// A click on a task for later picks it.
    LaterPick(usize),
    Filter(Option<String>),
    ToggleBackground,
    ToggleAutomated,
    ToggleSnoozed,
    /// A card's button: the same command the phone sends.
    Steer(crate::companion::Command),
    /// A click on a list section's title: fold or open it (`others`).
    ToggleSection(&'static str),
    /// A click on a project's head: fold or open it.
    ToggleProjectFolded(String),
    /// Leave a project out of the list, or bring it back.
    ToggleProjectHidden(String),
    /// Show the hidden projects for a look, or not.
    ToggleHiddenProjects,
    /// Open or fold the sessions that ended over a day ago.
    ToggleOlder,
    Do(Action),
    CleanupAge(usize),
    OpenUrl(String),
    ListScrolled(scrollable::Viewport),
    CheckPrs,
    PrsLoaded(HashMap<SessionKey, brain_core::github::PullRequest>),
    BriefLoaded(SessionKey, brain_core::brief::Brief),
    Palette(String),
    PaletteRun(PaletteCommand),
    ComposerEdit(iced::widget::text_editor::Action),
    /// A template's prompt into the composer.
    ComposerTemplate(usize),
    ComposerRemoveImage(usize),
    ComposerSend,
    /// Text typed into a session and submitted (e.g. `/compact` from the context gauge).
    SendTo(SessionKey, String),
    PaletteClose,
    /// Bytes for a session's terminal, sent a moment after its text (the Return that submits).
    TerminalKeys(SessionKey, Vec<u8>),
    /// The find field (⌘F).
    Find(String),
    HistoryLoaded(SessionKey, u64, Vec<brain_core::transcript::Message>),
    /// The files the session changed, read in the background for a transcript of this size.
    ChangesLoaded(SessionKey, u64, Vec<brain_core::changes::SessionChange>),
    /// Show or leave out the changed files under the selected session.
    ToggleChanges,
    /// A changed file clicked in the tree: its diff floats over the terminal (again: closes).
    OpenPreview(String),
    /// The file's diff against HEAD, read in the background.
    PreviewLoaded(SessionKey, String, Option<String>),
    ClosePreview,
    PreviewInEditor,
    /// A click on the card itself, which must not reach the backdrop that closes it.
    PreviewClicked,
    /// A search hit or a prompt clicked open (its index in the history), or closed again.
    Expand(Option<usize>),
    CopyText(String),
    Exported(Result<std::path::PathBuf, String>),
    CheckUpdate,
    UpdateFound(Option<crate::links::Update>),
    DragWindow,
    ZoomWindow,
    Term(iced_term::Event),
    /// What Brain can do with the terminal hosting a process (its pid).
    HostFound(u32, Capabilities),
    /// The branch a session's folder has checked out (none outside git).
    BranchFound(SessionKey, Option<String>),
    /// A background session Brain started or took over: its account and short id, or why not.
    Started(String, Result<String, String>),
    /// A background session restarted with new flags goes on as Claude Code's copy under a new
    /// id: the original, then the copy's account and short id, or why not.
    Replaced(SessionKey, String, Result<String, String>),
    /// A Markdown file was handed to YAMV (`true`), or to the text editor because YAMV is missing.
    MarkdownOpened(String, bool),
    MousePressed,
    /// The quick-terminal hotkey was pressed (from anywhere).
    QuickTerminal,
    /// The left mouse button went up (a selection may have ended).
    MouseReleased,
    /// A terminal's request to put text on the clipboard, and whether it has the keyboard.
    TerminalCopy(SessionKey, String, bool, CopyAsk),
    /// Whether this session's shell has the keyboard (asked after a click).
    ShellFocused(SessionKey, bool),
    /// ⌘T: show or hide the session's shell.
    ToggleShell,
    /// Whether this session's terminal has the keyboard (asked after a click).
    TermFocused(SessionKey, bool),
    /// ⌘D: the current terminal stays on the left, the next selected session opens beside it.
    ToggleSplit,
    /// A click on the split's other pane: its session becomes the active one.
    ActivatePane,
    /// Close the dialog or view on top, like esc.
    Close,
    /// Brain Link: turn it off, or sign out every paired phone.
    LinkOff,
    LinkForget,
    /// A phone's command, again once a background session's terminal attached.
    LinkRetry(crate::companion::Command),
    /// The phone's answers typed into a session, once Esc closed its question dialog.
    LinkType(SessionKey, String),
    /// A session the phone started: its short id, or why not.
    LinkStarted(Result<String, String>),
    /// ⌘⇧A: live previews of every session that runs in Brain.
    ToggleOverview,
    /// A preview clicked in the overview: open that session's terminal.
    OpenFromOverview(SessionKey),
    FileHover(bool),
    FileDrop(std::path::PathBuf),
    CloseReader,
    ReaderToEditor,
    NewPlace(bool),
    /// The new session's output style; none keeps the account's.
    NewStyle(Option<String>),
    /// The new-account dialog: the name typed, its preview, create, and the outcome.
    AccountSetupName(String),
    AccountSetupPlanned(u64, brain_core::account_setup::Plan),
    AccountSetupCreate,
    AccountSetupDone(Result<String, String>),
    /// The sync dialog: open it, its plan, a version picked or kept for a row, apply, and the outcome.
    OpenSync,
    /// The gardener: open it, a finding (un)chosen, end the chosen, and how each went.
    OpenGarden,
    GardenToggle(usize),
    GardenEnd,
    GardenDone(Vec<(SessionKey, Result<String, String>)>),
    SyncPlanned(brain_core::sync::Plan),
    SyncPick(usize, String),
    SyncKeep(usize),
    SyncApply,
    SyncDone(Result<brain_core::sync::Outcome, String>),
    /// The account dialog: the account picked, whether the original keeps running, the flags
    /// typed, and ⏎.
    RelaunchTarget(usize),
    RelaunchKeep(bool),
    RelaunchFlags(String),
    RelaunchSubmit,
    SidebarDrag,
    SidebarTo(f32),
    SidebarDone,
    /// An item of the menu bar was chosen (its id).
    MenuPicked(String),
    /// A menu item without a message of its own.
    Menu(MenuCommand),
    /// The clipboard's text, on its way into the terminal that has the keyboard.
    PasteText(Option<String>),
}

impl Message {
    /// Whether handling this message can change what `sync` keeps in step (the selection, the
    /// terminals, the conversation). Terminal output, pointer moves, hovering and scrolling can't;
    /// a terminal's shutdown, title and bell sync themselves. "Seen" marking runs in `sync`, so
    /// the next tick catches up with it.
    fn may_change_synced_state(&self) -> bool {
        !matches!(
            self,
            Message::Term(_)
                | Message::HostFound(..)
                | Message::BranchFound(..)
                | Message::Pointer(_)
                | Message::WindowSized(_)
                | Message::Hover(_)
                | Message::HoverHead(_)
                | Message::ListScrolled(_)
                | Message::FileHover(_)
                | Message::MousePressed
                | Message::MouseReleased
        )
    }
}

/// The session list's width bounds when dragged.
pub const SIDEBAR_DEFAULT: f32 = 320.0;
const SIDEBAR_MIN: f32 = 240.0;
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
    /// ⇧⇥: the output style it starts with; none keeps what the account's settings say.
    pub style: Option<String>,
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

/// Where a running session stands in the list.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    Decide(Ask),
    Read,
    Quiet,
}

/// Where a session belongs. `held`: the place it had when the user selected it. A selected
/// session counts as seen, but stays there until the selection moves on, so ↓ reaches every
/// report and nothing jumps away under the cursor. It never moves into "Zum Lesen" by a turn that
/// ends while it is open. A real ask (permission, question) shows at once, a mere "Frage?" only
/// while it was one when selected and "Nur Bericht" wasn't chosen.
fn place(session: &Session, held: Option<Place>, open_question: bool, prefs: &Prefs, now: i64) -> Place {
    let since = session.phase_since_ms();
    let snoozed = prefs.snoozed_until(&session.key, now).is_some();
    let ask = session.ask(open_question).filter(|_| !snoozed);
    if let Some(ask) = ask.filter(|ask| *ask != Ask::Maybe) {
        return Place::Decide(ask);
    }
    let maybe = ask == Some(Ask::Maybe) && !prefs.is_report(&session.key, since);
    let reading = session.phase() == Phase::YourTurn && !snoozed;
    match held {
        Some(Place::Decide(_)) if maybe => Place::Decide(Ask::Maybe),
        Some(Place::Read) if reading => Place::Read,
        Some(_) => Place::Quiet,
        None if maybe && !prefs.is_seen(&session.key, since) => Place::Decide(Ask::Maybe),
        None if reading && !prefs.is_seen(&session.key, since) => Place::Read,
        None => Place::Quiet,
    }
}

/// How long a woken session gets for its mod to take requests again.
const WAKE_TIMEOUT: Duration = Duration::from_secs(90);

/// An asleep session Brain woke for a message.
struct Waking {
    session_id: String,
    /// When the wake began: the mod's `.done` written since says it is up.
    since: std::time::SystemTime,
    started: Instant,
    /// What goes in once it is awake, in order.
    texts: Vec<String>,
}

/// The left column, already filtered and sorted.
pub struct Groups<'a> {
    /// Ended sessions the user marked with `P` to resume later.
    pub saved: Vec<&'a Session>,
    /// Sessions that ask something of the user, by kind and then longest waiting first. A
    /// question that is only a guess ([`Ask::Maybe`]) comes last.
    pub decide: Vec<(&'a Session, Ask)>,
    /// Finished and not yet looked at, newest first.
    pub read: Vec<&'a Session>,
    /// Everything else that lives.
    pub quiet: Vec<&'a Session>,
    /// The live sessions by project, in a fixed order (by name); in a project its decisions, its
    /// reports, then its quiet sessions.
    pub projects: Vec<Block<'a>>,
    /// The "Projects" section is open (a search opens it).
    pub projects_open: bool,
    /// The hidden projects show, for a look.
    pub hidden_open: bool,
    /// Snoozed while waiting, or hidden with ⌫ until something happens: folded at the end.
    pub paused: Vec<&'a Session>,
    /// Background sessions without activity for a day.
    pub background: Vec<&'a Session>,
    /// Sessions programs started (SDK reviews, `claude -p`).
    pub automated: Vec<&'a Session>,
    pub ended: Vec<&'a Session>,
    /// Ended over a day ago: a footer link opens them (a search puts them with `ended`).
    pub older: Vec<&'a Session>,
    pub older_open: bool,
    /// Which of the folded lists at the end are open (a search opens all).
    pub paused_open: bool,
    pub background_open: bool,
    pub automated_open: bool,
}

/// A project's sessions in the list.
pub struct Block<'a> {
    pub name: String,
    pub entries: Vec<Entry<'a>>,
    /// Folded: its head shows, its sessions don't.
    pub folded: bool,
    /// Left out of the list unless the hidden projects are shown.
    pub hidden: bool,
}

impl Block<'_> {
    /// Whether its sessions are drawn: not folded, and not hidden unless those show.
    fn shows(&self, hidden_open: bool) -> bool {
        !self.folded && (!self.hidden || hidden_open)
    }
}

/// A live session in its project's block, with the card it gets.
#[derive(Clone, Copy)]
pub enum Entry<'a> {
    Decide(&'a Session, Ask),
    Read(&'a Session),
    Quiet(&'a Session),
}

impl<'a> Entry<'a> {
    pub fn session(&self) -> &'a Session {
        match self {
            Entry::Decide(s, _) | Entry::Read(s) | Entry::Quiet(s) => s,
        }
    }
}

impl<'a> Groups<'a> {
    /// The running sessions of the projects the list draws, in drawing order: not in a folded or
    /// hidden project.
    pub fn live(&self) -> Vec<&'a Session> {
        if !self.projects_open {
            return Vec::new();
        }
        self.projects.iter().filter(|b| b.shows(self.hidden_open)).flat_map(|b| b.entries.iter().map(Entry::session)).collect()
    }

    /// How many sessions wait for a decision. A guess ([`Ask::Maybe`]) doesn't count.
    pub fn deciding(&self) -> usize {
        self.decide.iter().filter(|(_, ask)| *ask != Ask::Maybe).count()
    }
}

pub struct Brain {
    pub model: Model,
    pub prefs: Prefs,
    notifier: Notifier,
    menubar: Option<MenuBar>,
    /// The menu bar of the app, and what its items stand for.
    appmenu: Option<crate::appmenu::AppMenu>,
    menu_commands: HashMap<String, Message>,
    /// ⌘O: the session search lists the ended sessions too.
    resume_search: bool,
    /// `None` shows every account.
    pub filter: Option<String>,
    pub selected: Option<SessionKey>,
    pub hovered: Option<SessionKey>,
    pub hovered_head: Option<crate::tree::HeadKey>,
    /// Whether the snoozed-and-hidden section is open.
    pub show_snoozed: bool,
    /// The projects hidden from the list show for a look (not remembered).
    pub show_hidden_projects: bool,
    /// The clean-up dialog's age choice (index into CLEANUP_DAYS).
    pub cleanup_age: usize,
    /// The tasks noted for later with the project each belongs to, read again when the file
    /// changes; the picked one while their dialog is open.
    pub later: Vec<(brain_core::later::Item, String)>,
    pub later_pick: usize,
    later_stamp: Option<(std::time::SystemTime, u64)>,
    pub status: Option<(String, Instant)>,
    pub conversation: Option<Conversation>,
    /// The selected ended session in a few lines (shown before resuming it).
    pub brief: Option<(SessionKey, brain_core::brief::Brief)>,
    brief_loading: Option<SessionKey>,
    pub palette: String,
    /// The composer's text, the screenshots pasted into it, and the templates it offers.
    pub composer: iced::widget::text_editor::Content,
    pub composer_images: Vec<std::path::PathBuf>,
    pub composer_templates: Vec<brain_core::templates::Template>,
    pub palette_index: usize,
    /// The selected session's output styles, read when the palette opens.
    palette_styles: Vec<String>,
    /// Text to find in the selected session's whole conversation (⌘F).
    pub find: String,
    /// The find bar over the terminal is open (⌘F).
    pub find_open: bool,
    /// The whole conversation, loaded for finding.
    pub history: Option<HistoryCache>,
    history_loading: bool,
    /// The files the selected session changed, for the tree.
    pub changes: Option<ChangesCache>,
    changes_loading: bool,
    /// The diff card over the terminal; while it is open the terminal gets no keys.
    pub preview: Option<Preview>,
    pub expanded: Option<usize>,
    pub mode: Mode,
    pub search: String,
    pub rename: String,
    /// How many sessions wait for a decision (not counting a question that is only a guess), as of
    /// the last sync: the window title and the menu bar read it.
    deciding: usize,
    /// What Brain can do with the terminal hosting a session's process, by pid (found off the UI
    /// thread, once per pid).
    hosts: HashMap<u32, Capabilities>,
    /// The pids whose host is being looked up.
    hosts_pending: HashSet<u32>,
    /// Project name and worktree per session.
    pub projects: HashMap<SessionKey, (String, Option<String>)>,
    /// The checked-out branch per shown session, asked in the background every so often.
    pub branches: HashMap<SessionKey, (Option<String>, Instant)>,
    branches_pending: HashSet<SessionKey>,
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
    pub new_session: NewSession,
    /// ⌥⌘X, ⌫ or ⌥⌘⌫ was chosen once on this session: a second press within 5 s acts.
    armed: Option<(char, SessionKey, Instant)>,
    /// Sessions a move just started for: until the board lists them as moved, a second move
    /// (a double tap on the phone) would fork them into the target twice.
    moving: HashMap<SessionKey, Instant>,
    /// The session whose context menu is open, and where it opens.
    pub context_menu: Option<(SessionKey, iced::Point)>,
    /// The project whose menu is open, and where it opens.
    pub project_menu: Option<(String, iced::Point)>,
    /// The session selected before a right click selected another one: "open beside" keeps it.
    pub before_menu: Option<SessionKey>,
    pointer: iced::Point,
    pub window_size: iced::Size,
    /// A newer release on GitHub, if the daily check found one.
    pub update: Option<crate::links::Update>,
    /// What the list shows, to keep the selection in view.
    list_viewport: Option<(f32, f32)>,
    /// Terminals running sessions inside Brain, per session; they keep running while another
    /// session is selected.
    pub terminals: HashMap<SessionKey, iced_term::Terminal>,
    next_terminal: u64,
    /// Whether the selected session's terminal has the keyboard (drawn as a ring).
    /// The terminal that has the keyboard (Brain's shortcuts are off while one has it).
    pub focused_terminal: Option<SessionKey>,
    /// ⌘T: a login shell per session, in its folder, below its Claude terminal.
    pub shells: HashMap<SessionKey, iced_term::Terminal>,
    pub shell_visible: bool,
    /// The session whose shell has the keyboard.
    pub focused_shell: Option<SessionKey>,
    /// The terminal the user last typed into, and when.
    last_terminal_input: Option<(SessionKey, Instant)>,
    /// The terminal the user last released the mouse in (the end of a selection), and when.
    last_terminal_release: Option<(SessionKey, Instant)>,
    /// Text Claude Code copied right after a mouse selection: Brain keeps it until ⌘C.
    held_copy: Option<(SessionKey, String)>,
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
    /// A click selected a session: once its terminal shows, it gets the keys.
    focus_on_select: bool,
    /// The new-account dialog.
    pub account_setup: account::AccountSetup,
    /// The sync dialog, and the conflicts the last sync left open.
    pub sync: sync::SyncDialog,
    /// The gardener's dialog, and how many sessions it is sure are broken (for the title bar).
    pub garden: garden::GardenDialog,
    pub garden_count: usize,
    /// Sessions whose terminal closed right after attaching, with its last lines.
    unreachable: HashMap<SessionKey, Vec<String>>,
    /// When each session terminal started.
    terminal_started: HashMap<SessionKey, Instant>,
    /// The restart dialog, a session asked to end for a restart, and the flags a terminal about
    /// to start resumes with.
    pub relaunch: relaunch::RelaunchDialog,
    pending_restart: Option<relaunch::PendingRestart>,
    restart_flags: HashMap<SessionKey, Vec<String>>,
    /// Sessions ended to go on that were copies without a message yet: the conversation they
    /// were started from, read from their command line while it was there.
    started_from: HashMap<SessionKey, String>,
    /// Per session that waits: what it was read at, and the question it asks.
    /// Reminders sent per waiting session: (when its wait began, how many).
    reminders: HashMap<SessionKey, (i64, u32)>,
    /// The selected session and where it stood in the list before it was selected.
    held: Option<(SessionKey, Place)>,
    /// The sessions that asked for a decision at the last refresh (none before the first one).
    asking: Option<HashSet<SessionKey>>,
    pub(crate) questions: HashMap<SessionKey, (link::QuestionReadAt, Option<brain_core::qa::OpenQuestion>)>,
    /// While a command from the phone runs: what Brain says about it, for the phone's answer.
    pub(crate) link_heard: Option<link::Heard>,
    /// Session ids whose mod takes Brain's requests (replies, commands, interrupts) itself.
    request_takers: HashSet<String>,
    /// Asleep sessions Brain woke for a message, and what goes in once their mod is up.
    waking: HashMap<SessionKey, Waking>,
    /// How each wake went (`claude respawn`), from its thread.
    wake_results: (std::sync::mpsc::Sender<(SessionKey, Result<(), String>)>, std::sync::mpsc::Receiver<(SessionKey, Result<(), String>)>),
    /// Brain Link, the companion app's server, while it runs.
    link: Option<crate::companion::Link>,
    /// The pairing code as an image.
    pub link_qr: Option<iced::widget::image::Handle>,
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
        if !demo {
            brain_core::requests::prune(&brain_core::requests::dir(&brain_core::account::home_dir()));
            let now = now_ms();
            let finished: Vec<(SessionKey, i64)> =
                model.board.sorted().into_iter().filter(|s| s.phase() == Phase::YourTurn).map(|s| (s.key.clone(), s.phase_since_ms())).collect();
            if prefs.start_seen(finished, now) {
                prefs.save();
            }
        }
        restore_saved(&mut model, &prefs);
        let seeded = if demo { crate::demo::list_projects(&mut prefs) } else { seed_project_list(&mut model, &mut prefs) };
        if seeded {
            prefs.save();
        }
        let mut brain = Self {
            model,
            prefs,
            notifier: Notifier::new(),
            menubar: MenuBar::new(),
            appmenu: crate::appmenu::AppMenu::new(),
            menu_commands: HashMap::new(),
            resume_search: false,
            filter: None,
            selected: None,
            hovered: None,
            hovered_head: None,
            show_snoozed: false,
            show_hidden_projects: false,
            cleanup_age: 1,
            later: Vec::new(),
            later_pick: 0,
            later_stamp: None,
            status: None,
            conversation: None,
            mode: Mode::Normal,
            search: String::new(),
            rename: String::new(),
            deciding: 0,
            hosts: HashMap::new(),
            hosts_pending: HashSet::new(),
            projects: HashMap::new(),
            branches: HashMap::new(),
            branches_pending: HashSet::new(),
            checkouts: HashMap::new(),
            prs: HashMap::new(),
            conflicts: HashMap::new(),
            terminal_titles: HashMap::new(),
            shown_terminals: HashSet::new(),
            split: None,
            overview: false,
            brief: None,
            brief_loading: None,
            find: String::new(),
            palette: String::new(),
            composer: iced::widget::text_editor::Content::new(),
            composer_images: Vec::new(),
            composer_templates: Vec::new(),
            palette_index: 0,
            palette_styles: Vec::new(),
            find_open: false,
            history: None,
            changes: None,
            changes_loading: false,
            preview: None,
            history_loading: false,
            expanded: None,
            bells: HashSet::new(),
            new_session: NewSession::default(),
            armed: None,
            moving: HashMap::new(),
            context_menu: None,
            project_menu: None,
            before_menu: None,
            pointer: iced::Point::ORIGIN,
            window_size: iced::Size::new(1180.0, 760.0),
            update: None,
            list_viewport: None,
            terminals: HashMap::new(),
            next_terminal: 1,
            focused_terminal: None,
            shells: HashMap::new(),
            shell_visible: false,
            focused_shell: None,
            last_terminal_input: None,
            last_terminal_release: None,
            held_copy: None,
            file_hover: false,
            reader: None,
            pending_attach: None,
            takeover: None,
            synced_selection: None,
            dragging_sidebar: false,
            titlebar_ready: false,
            focus_on_select: false,
            account_setup: account::AccountSetup::default(),
            questions: HashMap::new(),
            link_heard: None,
            request_takers: HashSet::new(),
            waking: HashMap::new(),
            wake_results: std::sync::mpsc::channel(),
            reminders: HashMap::new(),
            held: None,
            asking: None,
            sync: sync::SyncDialog::default(),
            garden: garden::GardenDialog::default(),
            garden_count: 0,
            unreachable: HashMap::new(),
            terminal_started: HashMap::new(),
            relaunch: relaunch::RelaunchDialog::default(),
            pending_restart: None,
            restart_flags: HashMap::new(),
            started_from: HashMap::new(),
            link: None,
            link_qr: None,
        };
        brain.resume_link();
        brain.refresh_later();
        brain.selected = brain.nav_keys().first().cloned();
        if let Some(spec) = crate::config::quick_terminal().filter(|_| !demo) {
            if !crate::hotkey::register(&spec) {
                brain.set_status(tr!("Die Tastenkombination „{spec}“ ist vergeben oder ungültig.", "The hotkey “{spec}” is taken or invalid."));
            }
        }
        let synced = brain.sync();
        // For screenshots of the reader: `BRAIN_DEMO=1 BRAIN_DEMO_READER=notes.md`.
        if let Some(path) = std::env::var_os("BRAIN_DEMO_READER").filter(|_| demo) {
            brain.reader = Some(read_file(std::path::PathBuf::from(path)));
        }
        let checks = if demo { Task::none() } else { Task::batch([Task::done(Message::CheckPrs), Task::done(Message::CheckUpdate)]) };
        (brain, Task::batch([synced, checks]))
    }

    pub fn title(&self) -> String {
        let waiting = self.deciding;
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
            Subscription::run(crate::appmenu::picks).map(Message::MenuPicked),
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
        subscriptions.extend(self.shells.values().map(|term| term.subscription().map(Message::Term)));
        if !self.model.is_demo() {
            subscriptions.push(iced::time::every(Duration::from_secs(120)).map(|_| Message::CheckPrs));
            subscriptions.push(iced::time::every(Duration::from_secs(24 * 60 * 60)).map(|_| Message::CheckUpdate));
        }
        Subscription::batch(subscriptions)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let syncs = message.may_change_synced_state();
        let task = match message {
            Message::Tick => self.tick(),
            Message::Key(event, captured) => self.on_key(event, captured),
            Message::Select(key) => {
                // A click means this session: its terminal gets the keys, so typing goes there and
                // not to the list's one-letter shortcuts. (Arrow keys keep them for the list.)
                self.selected = Some(key);
                self.focus_on_select = true;
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
                self.project_menu = None;
                Task::none()
            }
            Message::ProjectMenu(name) => {
                self.context_menu = None;
                self.project_menu = Some((name, self.pointer));
                Task::none()
            }
            Message::NewProject => self.new_project(),
            Message::NewSessionIn(name) => {
                self.project_menu = None;
                self.new_session_in(&name)
            }
            Message::ToggleNotes(project) => {
                self.prefs.toggle_notes_open(&project);
                self.prefs.save();
                Task::none()
            }
            Message::StartNote(id) => self.start_note(&id),
            Message::CheckOffNote(id) => {
                self.check_off_note(&id);
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
            Message::HoverHead(head) => {
                self.hovered_head = head;
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
            Message::NewChooseFolder => self.choose_folder(),
            Message::NewSubmit => self.confirm_new_session(),
            Message::NewAccount(index) => {
                self.new_session.account = index;
                Task::none()
            }
            Message::NewPick(index) => {
                self.new_session.pick = index;
                self.confirm_new_session()
            }
            Message::LaterPick(index) => {
                self.later_pick = index;
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
            Message::ToggleSection(section) => {
                self.prefs.toggle_section(section);
                self.prefs.save();
                Task::none()
            }
            Message::ToggleProjectFolded(name) => {
                self.prefs.toggle_project_folded(&name);
                self.prefs.save();
                Task::none()
            }
            Message::ToggleProjectHidden(name) => {
                self.project_menu = None;
                self.prefs.toggle_project_hidden(&name);
                self.prefs.save();
                let hidden = self.prefs.is_project_hidden(&name);
                self.set_status(if hidden {
                    tr!("Projekt „{name}“ ausgeblendet – unten zeigt „ausgeblendet“ es wieder.", "Project “{name}” hidden – “hidden” at the bottom shows it again.")
                } else {
                    tr!("Projekt „{name}“ wieder eingeblendet.", "Project “{name}” shown again.")
                });
                Task::none()
            }
            Message::ToggleHiddenProjects => {
                self.show_hidden_projects = !self.show_hidden_projects;
                Task::none()
            }
            Message::ToggleOlder => {
                self.prefs.show_older = !self.prefs.show_older;
                self.prefs.save();
                Task::none()
            }
            Message::Steer(command) => self.run_link_command(command, true),
            Message::ToggleSnoozed => {
                self.show_snoozed = !self.show_snoozed;
                Task::none()
            }
            Message::Do(action) => self.act(action),
            Message::MenuPicked(id) => self.menu_picked(&id),
            Message::Menu(command) => self.run_menu(command),
            Message::PasteText(text) => {
                self.paste_text(text);
                Task::none()
            }
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
            Message::SendTo(key, text) => {
                self.send_text(&key, &text);
                Task::none()
            }
            Message::ComposerEdit(action) => {
                self.composer.perform(action);
                Task::none()
            }
            Message::ComposerTemplate(index) => {
                if let Some(template) = self.composer_templates.get(index) {
                    let prompt = template.prompt.clone();
                    self.composer.perform(iced::widget::text_editor::Action::Edit(iced::widget::text_editor::Edit::Paste(std::sync::Arc::new(prompt))));
                }
                operation::focus(COMPOSER)
            }
            Message::ComposerRemoveImage(index) => {
                if index < self.composer_images.len() {
                    self.composer_images.remove(index);
                }
                Task::none()
            }
            Message::ComposerSend => self.send_composer(),
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
            Message::HistoryLoaded(key, len, messages) => {
                self.history = Some(HistoryCache { key, len, messages });
                self.history_loading = false;
                Task::none()
            }
            Message::ChangesLoaded(key, len, files) => {
                self.changes = Some(ChangesCache { key, len, loaded: Instant::now(), files });
                self.changes_loading = false;
                Task::none()
            }
            Message::ToggleChanges => {
                self.prefs.hide_changes = !self.prefs.hide_changes;
                self.prefs.save();
                Task::none()
            }
            Message::OpenPreview(path) => self.open_preview(&path),
            Message::PreviewLoaded(key, path, diff) => {
                if let Some(preview) = self.preview.as_mut().filter(|p| p.key == key && p.change.path == path) {
                    let (lines, cut) = preview_lines(diff.as_deref().unwrap_or_default(), PREVIEW_LINES);
                    preview.cut = cut;
                    preview.lines = Some(lines);
                }
                Task::none()
            }
            Message::ClosePreview => {
                self.preview = None;
                Task::none()
            }
            Message::PreviewInEditor => {
                if let Some(preview) = self.preview.as_ref().filter(|p| p.exists) {
                    open_in_text_editor(std::path::Path::new(&preview.change.path));
                }
                Task::none()
            }
            Message::PreviewClicked => Task::none(),
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
            Message::BriefLoaded(key, brief) => {
                self.brief_loading = None;
                self.brief = Some((key, brief));
                Task::none()
            }
            Message::CheckUpdate => Task::perform(off_thread(crate::links::check_for_update), Message::UpdateFound),
            Message::UpdateFound(update) => {
                self.update = update;
                Task::none()
            }
            Message::BranchFound(key, branch) => {
                self.branches_pending.remove(&key);
                self.branches.insert(key, (branch, Instant::now()));
                Task::none()
            }
            Message::HostFound(pid, caps) => {
                self.hosts_pending.remove(&pid);
                self.hosts.insert(pid, caps);
                Task::none()
            }
            Message::DragWindow => window::latest().and_then(window::drag),
            Message::ZoomWindow => window::latest().and_then(window::toggle_maximize),
            Message::Term(event) => {
                let iced_term::Event::BackendCall(id, command) = event;
                // A shell's event: its own map; it ends by itself (exit), nothing else to do.
                if let Some(key) = self.shells.iter().find(|(_, term)| term.id == id).map(|(key, _)| key.clone()) {
                    let action = self.shells.get_mut(&key).map(|term| term.handle(iced_term::Command::ProxyToBackend(command)));
                    if action == Some(iced_term::actions::Action::Shutdown) {
                        self.shells.remove(&key);
                        if self.focused_shell.as_ref() == Some(&key) {
                            self.focused_shell = None;
                        }
                        return self.sync();
                    }
                    return Task::none();
                }
                let key = self.terminals.iter().find(|(_, term)| term.id == id).map(|(key, _)| key.clone());
                let mut task = Task::none();
                // Output only moves the terminal's own screen; shutdown, a title and a bell change
                // what the list and the detail show, so those sync.
                let mut sync = false;
                if let Some(key) = key {
                    // Only the terminal on screen copies its screen per event; the others catch
                    // up when they're shown (`sync_shown_terminal`).
                    let shown = self.shown_terminals.contains(&key);
                    let command = iced_term::Command::ProxyToBackend(command);
                    match self.terminals.get_mut(&key).map(|term| if shown { term.handle(command) } else { term.handle_quiet(command) }) {
                        Some(iced_term::actions::Action::Shutdown) => {
                            sync = true;
                            let lines = self.terminals.remove(&key).map(|term| term.text()).unwrap_or_default();
                            self.terminal_titles.remove(&key);
                            self.session_terminal_closed(&key, lines);
                        }
                        Some(iced_term::actions::Action::OpenLink(link)) => task = self.open_link(&key, &link),
                        Some(iced_term::actions::Action::ChangeTitle(title)) => {
                            sync = true;
                            let title = title.trim().to_string();
                            if title.is_empty() {
                                self.terminal_titles.remove(&key);
                            } else {
                                self.terminal_titles.insert(key, title);
                            }
                        }
                        Some(iced_term::actions::Action::Bell) if self.selected.as_ref() != Some(&key) => {
                            sync = true;
                            let name = self.model.board.get(&key).map(|s| s.display_name()).unwrap_or_default();
                            self.set_status(tr!("„{name}“ klingelt.", "“{name}” rang the bell."));
                            self.bells.insert(key);
                        }
                        // Output can carry clipboard requests too (a printed file, a fetched page):
                        // only a terminal you typed into a moment ago may write the clipboard, and
                        // only if it really has the keyboard now (asked below), and it says so.
                        Some(iced_term::actions::Action::CopyToClipboard(text)) => {
                            let age = |at: &Option<(SessionKey, Instant)>| at.as_ref().filter(|(k, _)| *k == key).map(|(_, at)| at.elapsed());
                            let ask = copy_decision(age(&self.last_terminal_input), age(&self.last_terminal_release));
                            if let (Some(term), true) = (self.terminals.get(&key), ask != CopyAsk::Ignore) {
                                let key = key.clone();
                                task = operation::is_focused(term.widget_id().clone()).map(move |focused| Message::TerminalCopy(key.clone(), text.clone(), focused, ask));
                            }
                        }
                        _ => {}
                    }
                }
                if sync { Task::batch([task, self.sync()]) } else { task }
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
            // The copy takes the original's place: one session in the list, not two.
            Message::Replaced(original, account, result) => {
                if result.is_ok() {
                    self.prefs.hide(&original, now_ms());
                    self.prefs.save();
                }
                self.update(Message::Started(account, result))
            }
            Message::MousePressed => {
                // A new selection (or a click that ends one) replaces the held text.
                self.held_copy = None;
                // Clicks move the keyboard in or out of a terminal; ask each shown one.
                let mut asks: Vec<Task<Message>> = self
                    .shown_terminals
                    .iter()
                    .filter_map(|key| {
                        let term = self.terminals.get(key)?;
                        let key = key.clone();
                        Some(operation::is_focused(term.widget_id().clone()).map(move |focused| Message::TermFocused(key.clone(), focused)))
                    })
                    .collect();
                if let Some((key, shell)) = self.selected.as_ref().filter(|_| self.shell_visible).and_then(|k| Some((k.clone(), self.shells.get(k)?))) {
                    asks.push(operation::is_focused(shell.widget_id().clone()).map(move |focused| Message::ShellFocused(key.clone(), focused)));
                }
                Task::batch(asks)
            }
            Message::ShellFocused(key, focused) => {
                if focused {
                    self.focused_shell = Some(key);
                } else if self.focused_shell.as_ref() == Some(&key) {
                    self.focused_shell = None;
                }
                self.refresh_menus();
                Task::none()
            }
            Message::ToggleShell => self.toggle_shell(),
            Message::QuickTerminal => self.quick_terminal(),
            Message::MouseReleased => {
                if let Some(key) = self.focused_terminal.clone() {
                    self.last_terminal_release = Some((key, Instant::now()));
                }
                Task::none()
            }
            Message::TerminalCopy(key, text, focused, ask) => {
                if !focused {
                    return Task::none();
                }
                self.focused_terminal = Some(key.clone());
                let chars = text.chars().count();
                if ask == CopyAsk::Hold {
                    // Selected with the mouse: nothing reaches the clipboard before ⌘C.
                    self.held_copy = Some((key, text));
                    self.set_status(tr!("Markiert ({chars} Zeichen) – ⌘C kopiert.", "Selected ({chars} characters) – ⌘C copies."));
                    self.refresh_menus();
                    return Task::none();
                }
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
                self.refresh_menus();
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
            Message::Close => self.escape(),
            Message::AccountSetupName(name) => self.account_setup_name(name),
            Message::AccountSetupPlanned(asked, plan) => {
                self.account_setup_planned(asked, plan);
                Task::none()
            }
            Message::AccountSetupCreate => self.account_setup_create(),
            Message::RelaunchTarget(index) => {
                self.relaunch.target = index;
                self.relaunch.error = None;
                Task::none()
            }
            Message::RelaunchKeep(keep) => {
                self.relaunch.keep = keep;
                self.relaunch.error = None;
                Task::none()
            }
            Message::RelaunchFlags(flags) => {
                self.relaunch.flags = flags;
                self.relaunch.error = None;
                Task::none()
            }
            Message::RelaunchSubmit => self.submit_relaunch(),
            Message::OpenSync => self.open_sync(),
            Message::OpenGarden => self.open_garden(),
            Message::GardenToggle(index) => {
                self.garden_toggle(index);
                Task::none()
            }
            Message::GardenEnd => self.garden_end(),
            Message::GardenDone(results) => {
                self.garden_done(results);
                Task::none()
            }
            Message::SyncPlanned(plan) => {
                self.sync_planned(plan);
                Task::none()
            }
            Message::SyncPick(row, account) => {
                self.sync_pick(row, account);
                Task::none()
            }
            Message::SyncKeep(row) => {
                self.sync_keep(row);
                Task::none()
            }
            Message::SyncApply => self.sync_apply(),
            Message::SyncDone(result) => {
                self.sync_done(result);
                Task::none()
            }
            Message::AccountSetupDone(result) => {
                self.mode = Mode::Normal;
                self.account_setup_done(result)
            }
            Message::LinkOff => {
                self.stop_link();
                Task::none()
            }
            Message::LinkForget => {
                self.forget_phones();
                Task::none()
            }
            Message::LinkRetry(command) => self.run_link_command(command, false),
            Message::LinkType(key, text) => self.type_after_dialog(&key, &text),
            Message::LinkStarted(result) => {
                self.model.relist_transcripts();
                match result {
                    Ok(_) => self.set_status(t("Session vom Handy läuft im Hintergrund.", "The session from the phone runs in the background.")),
                    Err(why) => self.set_status(tr!("Session vom Handy startet nicht: {why}", "The session from the phone didn't start: {why}")),
                }
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
            Message::NewStyle(style) => {
                self.new_session.style = style;
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
        if !syncs {
            return task;
        }
        let synced = self.sync();
        Task::batch([task, synced])
    }

    // ---- periodic work ------------------------------------------------------------

    fn tick(&mut self) -> Task<Message> {
        if !self.titlebar_ready {
            self.titlebar_ready = crate::chrome::unified_titlebar();
        }
        let now = now_ms();
        let refreshed = self.model.refresh();
        self.refresh_menus();
        self.refresh_later();
        self.refresh_questions();
        if !self.model.is_demo() {
            self.request_takers = brain_core::requests::takers(&brain_core::requests::dir(&brain_core::account::home_dir()));
            self.deliver_to_woken();
        }
        let entered: HashSet<SessionKey> = refreshed.iter().map(|item| item.key.clone()).collect();
        // A question reported after the stop (the phase stays) calls too: announce each session
        // when it starts to ask, unless its phase change just did.
        let asking: HashSet<SessionKey> = self.model.board.keys().filter(|key| self.real_ask(key).is_some()).cloned().collect();
        let newly_asking: Vec<SessionKey> = match &self.asking {
            Some(before) => asking.difference(before).filter(|key| !entered.contains(*key)).cloned().collect(),
            None => Vec::new(),
        };
        self.asking = Some(asking);
        for key in newly_asking {
            if self.prefs.snoozed_until(&key, now).is_some() || self.prefs.is_muted(&key) || self.is_quiet(&key) {
                continue;
            }
            let Some(session) = self.model.board.get(&key) else { continue };
            let account = key.account.clone();
            let subtitle = tr!("{account} · braucht dich", "{account} · needs you");
            let body = plain(&session.headline().unwrap_or_default());
            self.notifier.post(&key, &session.display_name(), &subtitle, &body, true);
        }
        // Only what asks the person directly notifies: a question, a permission, a `--waiting`
        // question of an idle session. A turn that just ended shows in the list.
        for item in refreshed {
            if self.prefs.snoozed_until(&item.key, now).is_some() || self.prefs.is_muted(&item.key) || self.is_quiet(&item.key) || self.real_ask(&item.key).is_none() {
                continue;
            }
            let account = item.key.account.clone();
            let subtitle = tr!("{account} · braucht dich", "{account} · needs you");
            let body = plain(item.headline.as_deref().unwrap_or(""));
            self.notifier.post(&item.key, &item.name, &subtitle, &body, true);
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
                notify::Response::Reply(key, text) => tasks.push(self.reply_from_notification(key, text)),
            }
        }
        tasks.push(self.serve_link(now));
        self.refresh_sync_conflicts();
        self.garden_count = self.garden_findings().iter().filter(|f| f.sure).count();
        self.remind(now);
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
        tasks.push(self.continue_takeover());
        tasks.push(self.continue_restart());
        if self.status.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(6)) {
            self.status = None;
        }
        Task::batch(tasks)
    }

    /// Keeps derived state in step after every message: the selection on a visible session,
    /// the conversation, the terminal capabilities, projects, conflicts and the menu bar.
    fn sync(&mut self) -> Task<Message> {
        let now = now_ms();
        // A selected session that just ended stays in the tree (the last day's ended sessions
        // show under their project), so `P` or ⏎ right after `/exit` still mean that session.
        let (visible, first, mut deciding) = {
            let groups = self.groups(now);
            let tree = self.tree(&groups);
            let visible: Vec<SessionKey> = tree.sessions().iter().map(|s| s.key.clone()).collect();
            (visible, tree.navigable().first().map(|s| s.key.clone()), groups.deciding())
        };
        if self.selected.as_ref().is_none_or(|k| !visible.contains(k)) {
            self.selected = first;
        }
        // Where the newly selected session stood, before looking at it makes it seen.
        let held_changes = self.held.as_ref().map(|(key, _)| key) != self.selected.as_ref();
        if held_changes {
            self.held = self.selected.clone().and_then(|key| {
                let session = self.model.board.get(&key)?;
                let open_question = self.questions.get(&key).is_some_and(|(_, q)| q.is_some());
                Some((key, place(session, None, open_question, &self.prefs, now)))
            });
        }
        if let Some(key) = &self.selected {
            self.bells.remove(key);
            // Looking at a session in the front window is what makes its report read.
            if app_is_active() {
                if let Some(since) = self.model.board.get(key).map(|s| s.phase_since_ms()) {
                    if self.prefs.see(key, now, since) {
                        self.prefs.save();
                    }
                }
            }
        }
        if self.preview.as_ref().is_some_and(|p| self.selected.as_ref() != Some(&p.key)) {
            self.preview = None;
        }
        self.sync_conversation();
        let attach = self.sync_terminal();
        self.sync_shown_terminal();
        let host = self.sync_host();
        let branches = self.sync_branches();
        self.sync_projects();
        self.conflicts = self.compute_conflicts();
        // The grouping above stands unless the selection changed where a session is placed.
        if held_changes {
            deciding = self.groups(now).deciding();
        }
        self.deciding = deciding;
        if let Some(menubar) = self.menubar.as_mut() {
            menubar.show(deciding, deciding);
        }
        // The find results follow the transcript as it grows.
        let history = self.load_history();
        let brief = self.load_brief();
        let changes = self.load_changes();
        Task::batch([attach, host, branches, history, brief, changes])
    }

    /// Background sessions open in Brain's terminal: when one gets selected (or Brain started it
    /// a moment ago), its terminal attaches.
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
        let clicked = std::mem::take(&mut self.focus_on_select);
        if current == self.synced_selection {
            // A click on the session that is already selected enters its terminal too.
            return if clicked && self.has_terminal() { self.focus_terminal() } else { Task::none() };
        }
        self.synced_selection = current;
        self.focused_terminal = None;
        let has_terminal = self.has_terminal();
        if attachable || has_terminal {
            // Selecting a session with the keys shows its terminal but keeps the keys for the
            // list; a click, or a session started a moment ago, gives them to the terminal.
            return self.open_terminal(just_started || clicked);
        }
        Task::none()
    }

    /// Reminds again about sessions that still ask for a decision: every `remind_after_minutes`,
    /// at most three times per waiting period, never for muted or snoozed ones, and never for a
    /// guess at a question or a report.
    fn remind(&mut self, now: i64) {
        const MAX_REMINDERS: u32 = 3;
        let Some(interval) = config::remind_after_ms() else { return };
        // Straight from the board: neither the search nor the account filter hides a session
        // that waits for a decision.
        let waiting: Vec<(SessionKey, i64, String, Option<String>)> = self
            .model
            .board
            .keys()
            .filter(|key| self.real_ask(key).is_some())
            .filter_map(|key| self.model.board.get(key))
            .map(|s| (s.key.clone(), s.phase_since_ms(), s.display_name(), s.headline()))
            .collect();
        self.reminders.retain(|key, _| waiting.iter().any(|(k, ..)| k == key));
        for (key, since, name, headline) in waiting {
            if self.prefs.is_muted(&key) || self.prefs.snoozed_until(&key, now).is_some() || self.is_quiet(&key) {
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
            self.notifier.post(&key, &name, &subtitle, &body, true);
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

    /// Loads the selected session's whole conversation while the find field needs it, again when
    /// the transcript grew.
    fn load_history(&mut self) -> Task<Message> {
        if self.history_loading || self.find.trim().is_empty() {
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

    /// Reads the files the selected session changed when it is selected and when its transcript
    /// grew (at most every few seconds: the reading runs git).
    fn load_changes(&mut self) -> Task<Message> {
        if self.prefs.hide_changes || self.changes_loading {
            return Task::none();
        }
        let Some(key) = self.selected.clone() else { return Task::none() };
        if self.model.is_demo() {
            if self.changes.as_ref().is_none_or(|c| c.key != key) {
                let files = self.model.demo_changes.get(&key).cloned().unwrap_or_default();
                self.changes = Some(ChangesCache { key, len: 0, loaded: Instant::now(), files });
            }
            return Task::none();
        }
        let Some((path, len)) = self.conversation.as_ref().filter(|c| c.key == key).and_then(|c| c.transcript()) else {
            return Task::none();
        };
        if self.changes.as_ref().is_some_and(|c| c.key == key && (c.len == len || c.loaded.elapsed() < CHANGES_EVERY)) {
            return Task::none();
        }
        let Some(cwd) = self.selected_session().and_then(|s| s.cwd.clone()) else { return Task::none() };
        let path = path.to_path_buf();
        self.changes_loading = true;
        Task::perform(off_thread(move || brain_core::changes::session_changes(&path, std::path::Path::new(&cwd))), move |files| {
            Message::ChangesLoaded(key.clone(), len, files)
        })
    }

    /// Opens the card for a file of the selected session's changes, or closes it when that file's
    /// card is open already. The terminal gives up the keyboard: esc and the rest are the card's.
    fn open_preview(&mut self, path: &str) -> Task<Message> {
        if self.preview.as_ref().is_some_and(|p| p.change.path == path) {
            self.preview = None;
            return Task::none();
        }
        let Some(key) = self.selected.clone() else { return Task::none() };
        let Some(change) = self.changes.as_ref().filter(|c| c.key == key).and_then(|c| c.files.iter().find(|f| f.path == path)).cloned() else {
            return Task::none();
        };
        let exists = !self.model.is_demo() && std::path::Path::new(&change.path).is_file();
        let demo_diff = self.model.is_demo().then(|| self.model.demo_diffs.get(&change.path).cloned());
        self.focused_terminal = None;
        self.focused_shell = None;
        let task = match demo_diff {
            Some(diff) => {
                self.preview = Some(Preview { key: key.clone(), change: change.clone(), lines: None, cut: 0, exists });
                return Task::batch([unfocus(), self.update(Message::PreviewLoaded(key, change.path, diff))]);
            }
            None => {
                let (loaded_key, loaded_path, reading) = (key.clone(), change.path.clone(), change.clone());
                Task::perform(off_thread(move || brain_core::changes::session_diff(&reading)), move |diff| {
                    Message::PreviewLoaded(loaded_key.clone(), loaded_path.clone(), diff)
                })
            }
        };
        self.preview = Some(Preview { key, change, lines: None, cut: 0, exists });
        Task::batch([unfocus(), task])
    }

    /// The selected session and the files it changed, once they are read: they stand under its
    /// row in the tree.
    pub fn tree_changes(&self) -> Option<(&SessionKey, &[brain_core::changes::SessionChange])> {
        let key = self.selected.as_ref()?;
        let cache = self.changes.as_ref().filter(|c| c.key == *key && !self.prefs.hide_changes)?;
        Some((key, cache.files.as_slice()))
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

    /// A reply typed into a notification: straight to the session when its mod takes it and no
    /// permission dialog is open; otherwise Brain comes forward with the text in the composer, so
    /// nothing typed is lost.
    fn reply_from_notification(&mut self, key: SessionKey, text: String) -> Task<Message> {
        let permission_open = self.model.board.get(&key).is_some_and(|s| s.awaiting_permission());
        if self.can_send(&key) && !permission_open {
            let name = self.model.board.get(&key).map(|s| s.display_name()).unwrap_or_default();
            if self.send_text(&key, &text) {
                self.set_status(tr!("Antwort an „{name}“ gesendet.", "Reply sent to “{name}”."));
                return Task::none();
            }
        }
        self.selected = Some(key);
        self.composer = iced::widget::text_editor::Content::with_text(&text);
        Task::batch([self.bring_forward(), self.open_composer()])
    }

    /// ⌘T: a login shell in the selected session's folder, below its Claude terminal; again
    /// hides it (the shell keeps running).
    fn toggle_shell(&mut self) -> Task<Message> {
        if self.shell_visible {
            self.shell_visible = false;
            self.focused_shell = None;
            return unfocus();
        }
        let Some(key) = self.selected.clone() else { return Task::none() };
        if self.model.is_demo() {
            return Task::none();
        }
        if !self.shells.contains_key(&key) {
            let cwd = self.selected_session().and_then(|s| s.cwd.clone()).unwrap_or_else(|| brain_core::account::home_dir().display().to_string());
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
            let mut env = HashMap::new();
            env.insert("TERM".to_string(), "xterm-256color".to_string());
            let font = crate::chat_font::get();
            let settings = iced_term::settings::Settings {
                font: iced_term::settings::FontSettings { size: font.size, font_type: font.regular, scale_factor: 1.3 },
                theme: iced_term::settings::ThemeSettings::new(Box::new(crate::style::terminal_palette())),
                backend: iced_term::settings::BackendSettings { program: shell, args: vec!["-l".into()], env, working_directory: Some(cwd.into()) },
            };
            match iced_term::Terminal::new(self.next_terminal, settings) {
                Ok(term) => {
                    self.next_terminal += 1;
                    self.shells.insert(key.clone(), term);
                }
                Err(err) => {
                    self.set_status(tr!("Shell startet nicht: {err}", "Shell didn't start: {err}"));
                    return Task::none();
                }
            }
        }
        self.shell_visible = true;
        self.focused_terminal = None;
        self.focused_shell = Some(key.clone());
        match self.shells.get(&key) {
            Some(shell) => iced_term::TerminalView::focus(shell.widget_id().clone()),
            None => Task::none(),
        }
    }

    /// ⌘E: the composer for the selected session.
    fn open_composer(&mut self) -> Task<Message> {
        if self.selected.is_none() {
            return Task::none();
        }
        self.mode = Mode::Composer;
        self.focused_terminal = None;
        self.composer_templates = brain_core::templates::load_all(&brain_core::templates::templates_dir(&brain_core::account::home_dir()));
        operation::focus(COMPOSER)
    }

    /// ⌘⏎ in the composer: the text and the screenshots' paths to the session, line breaks and
    /// all.
    fn send_composer(&mut self) -> Task<Message> {
        let Some(key) = self.selected.clone() else { return Task::none() };
        let mut text = self.composer.text().trim_end().to_string();
        for image in &self.composer_images {
            text.push(' ');
            text.push_str(&escape_path(&image.display().to_string()));
        }
        if text.trim().is_empty() {
            return Task::none();
        }
        if self.blocked_in_demo() {
            return Task::none();
        }
        // The text stays in the composer when it didn't go.
        if !self.send_text(&key, &text) {
            return Task::none();
        }
        self.composer = iced::widget::text_editor::Content::new();
        self.composer_images.clear();
        self.mode = Mode::Normal;
        self.set_status(t("Gesendet.", "Sent."));
        self.focus_terminal()
    }

    /// ⌘K.
    fn open_palette(&mut self) -> Task<Message> {
        self.mode = Mode::Palette;
        self.palette.clear();
        self.palette_styles = self.selected_styles().into_iter().map(|s| s.name).collect();
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
                // `/output-style`: how the session answers, from its next message on.
                for style in &self.palette_styles {
                    all.push((tr!("Antwort-Profil von „{name}“: {style}", "Answer style of “{name}”: {style}"), "", PaletteCommand::Send(format!("/output-style {style}"))));
                }
            }
            all.push((tr!("Längeren Prompt an „{name}“ schreiben", "Write a longer prompt for “{name}”"), "⌘E", PaletteCommand::Composer));
            all.push((t("Letzte Antwort kopieren", "Copy the last answer").to_string(), "⌘⇧C", PaletteCommand::Act(Action::CopyAnswer)));
            all.push((t("Gespräch als Markdown öffnen", "Open the conversation as Markdown").to_string(), "", PaletteCommand::Act(Action::Export)));
            all.push((t("Shell im Projektordner", "Shell in the project folder").to_string(), "⌘T", PaletteCommand::Shell));
            all.push((t("In der Session suchen", "Find in the session").to_string(), "⌘F", PaletteCommand::Find));
            all.push((t("Umbenennen", "Rename").to_string(), "⌘R", PaletteCommand::Act(Action::StartRename)));
            all.push((t("Anheften / lösen", "Pin / unpin").to_string(), "⌥⌘P", PaletteCommand::Act(Action::Pin)));
            all.push((t("Stumm / laut", "Mute / unmute").to_string(), "", PaletteCommand::Act(Action::Mute)));
            all.push((t("Pausieren", "Snooze").to_string(), "", PaletteCommand::Act(Action::Snooze)));
            for index in self.move_targets() {
                let id = self.model.accounts[index].id.clone();
                all.push((tr!("In {id} fortsetzen", "Continue in {id}"), "", PaletteCommand::Act(Action::MoveTo(index))));
            }
            for index in self.fork_targets() {
                let id = self.model.accounts[index].id.clone();
                all.push((tr!("In {id} forken", "Fork into {id}"), "", PaletteCommand::Act(Action::ForkTo(index))));
            }
            all.push((t("In ein anderes Konto oder forken …", "Into another account or fork …").to_string(), "", PaletteCommand::Act(Action::Relaunch)));
            all.push((t("Neu starten mit … (z. B. --chrome)", "Restart with … (e.g. --chrome)").to_string(), "", PaletteCommand::Act(Action::RestartWith)));
            all.push((t("Beenden / Hintergrund-Session stoppen", "End / stop the background session").to_string(), "⌥⌘X", PaletteCommand::Act(Action::End)));
            all.push((t("Ausblenden", "Hide").to_string(), "⌫", PaletteCommand::Act(Action::Hide)));
            all.push((t("In den Papierkorb", "Move to Trash").to_string(), "⌥⌘⌫", PaletteCommand::Act(Action::Trash)));
        }
        all.push((t("Terminal teilen", "Split the terminal").to_string(), "⌘D", PaletteCommand::Split));
        all.push((t("Alle Terminals im Überblick", "All terminals at a glance").to_string(), "⌘⇧A", PaletteCommand::Overview));
        all.push((t("Neue Session", "New session").to_string(), "⌘N", PaletteCommand::NewSession));
        all.push((t("Aufräumen", "Clean up").to_string(), "", PaletteCommand::Cleanup));
        all.push((t("Für später", "For later").to_string(), "⌘L", PaletteCommand::Later));
        all.push((t("iPhone koppeln (Brain Link)", "Pair a phone (Brain Link)").to_string(), "", PaletteCommand::PairPhone));
        all.push((t("Neues Claude-Konto einrichten", "Set up a new Claude account").to_string(), "", PaletteCommand::SetUpAccount));
        all.push((t("Konten abgleichen (Einstellungen, Skills, Gedächtnis)", "Sync accounts (settings, skills, memory)").to_string(), "", PaletteCommand::SyncAccounts));
        all.push((t("Alle Tasten", "All keys").to_string(), "⌘?", PaletteCommand::Keys));
        all.push((t("Gärtner: kaputte Sessions finden und beenden", "Gardener: find and end broken sessions").to_string(), "", PaletteCommand::Garden));
        all.push((t("Log öffnen (Meldungen und Fehler)", "Open the log (messages and errors)").to_string(), "", PaletteCommand::OpenLog));
        all.push((t("Änderungen unter der Session zeigen / verbergen", "Show / hide the changes under the session").to_string(), "", PaletteCommand::ToggleChanges));
        all.push((t("Ruhige Hintergrund-Sessions auf- / zuklappen", "Open / fold quiet background sessions").to_string(), "", PaletteCommand::ToggleBackground));
        all.push((t("Automatisch gestartete Sessions (Reviews, Skripte) auf- / zuklappen", "Open / fold sessions started by programs (reviews, scripts)").to_string(), "", PaletteCommand::ToggleAutomated));

        // Leave a project out of the list, or bring it back.
        for name in self.project_names() {
            let label = if self.prefs.is_project_hidden(&name) { tr!("Projekt „{name}“ wieder einblenden", "Show project “{name}” again") } else { tr!("Projekt „{name}“ ausblenden", "Hide project “{name}”") };
            all.push((label, "", PaletteCommand::HideProject(name)));
        }
        // Into another project: each one that has sessions, back into its own, or a new one typed.
        if let Some(session) = self.selected_session() {
            let name = session.display_name();
            let current = self.project_name(session);
            for project in self.project_names().into_iter().filter(|p| *p != current) {
                all.push((tr!("„{name}“ ins Projekt „{project}“ verschieben", "Move “{name}” to project “{project}”"), "", PaletteCommand::Project(Some(project))));
            }
            if self.prefs.project(&session.key).is_some() {
                let own = self.own_project_name(session);
                all.push((tr!("„{name}“ zurück in sein Projekt „{own}“", "Move “{name}” back to its project “{own}”"), "", PaletteCommand::Project(None)));
            }
        }

        let query = self.palette.trim().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut shown: Vec<_> = all.into_iter().filter(|(label, _, _)| {
            let label = label.to_lowercase();
            words.iter().all(|w| label.contains(w))
        }).collect();
        let text = self.palette.trim();
        if !text.is_empty() {
            // Not for a project that exists, nor for the word the menu fills in to list them.
            let offers_new = !self.prefs.is_project_listed(text) && !text.eq_ignore_ascii_case(t("Projekt", "project"));
            if let Some(session) = self.selected_session().filter(|_| offers_new) {
                let name = session.display_name();
                shown.push((tr!("„{name}“ ins neue Projekt „{text}“ verschieben", "Move “{name}” to a new project “{text}”"), "", PaletteCommand::Project(Some(text.to_string()))));
            }
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
            PaletteCommand::HideProject(name) => self.update(Message::ToggleProjectHidden(name)),
            PaletteCommand::Project(project) => {
                if let Some(key) = self.selected.clone() {
                    self.put_in_project(&key, project);
                }
                Task::none()
            }
            PaletteCommand::Act(action) => self.act(action),
            PaletteCommand::Send(text) => {
                if let Some(key) = self.selected.clone() {
                    if self.send_text(&key, &text) {
                        self.set_status(t("Gesendet.", "Sent."));
                    }
                }
                Task::none()
            }
            PaletteCommand::Broadcast(text) => {
                let targets = self.broadcast_targets();
                let count = targets.into_iter().filter(|key| self.send_text(key, &text)).count();
                self.set_status(tr!("An {count} Sessions gesendet.", "Sent to {count} sessions."));
                Task::none()
            }
            PaletteCommand::NewSession => self.open_new_session_dialog(),
            PaletteCommand::Find => self.start_find(),
            PaletteCommand::Cleanup => {
                self.armed = None;
                self.mode = Mode::Cleanup;
                Task::none()
            }
            PaletteCommand::Later => {
                self.open_later();
                Task::none()
            }
            PaletteCommand::SetUpAccount => self.open_account_setup(),
            PaletteCommand::SyncAccounts => self.open_sync(),
            PaletteCommand::Garden => self.open_garden(),
            PaletteCommand::OpenLog => {
                self.open_log();
                Task::none()
            }
            PaletteCommand::Keys => {
                self.mode = Mode::Keys;
                Task::none()
            }
            PaletteCommand::PairPhone => {
                self.pair_phone();
                Task::none()
            }
            PaletteCommand::ToggleBackground => {
                self.toggle_background();
                Task::none()
            }
            PaletteCommand::ToggleChanges => self.update(Message::ToggleChanges),
            PaletteCommand::ToggleAutomated => {
                self.toggle_automated();
                Task::none()
            }
            PaletteCommand::Split => self.update(Message::ToggleSplit),
            PaletteCommand::Overview => self.update(Message::ToggleOverview),
            PaletteCommand::Composer => self.open_composer(),
            PaletteCommand::Shell => self.update(Message::ToggleShell),
        }
    }

    /// Whether Brain can send the session text: its mod takes it, wherever the session runs, or
    /// it is asleep and wakes for it.
    pub fn can_send(&self, key: &SessionKey) -> bool {
        self.request_taker(key).is_some() || self.wakeable(key)
    }

    /// An asleep background session Claude Code wakes again (`claude respawn`).
    /// Brain woke the session for a message that waits for it to be up.
    pub(super) fn is_waking(&self, key: &SessionKey) -> bool {
        self.waking.contains_key(key)
    }

    fn wakeable(&self, key: &SessionKey) -> bool {
        self.model.board.get(key).is_some_and(|s| s.phase() == Phase::Asleep && s.agent.is_some() && s.session_id.is_some())
    }

    /// The session's id when its mod takes requests: a reply, a command or an interrupt goes to
    /// the mod, and nothing is typed. An asleep one has no mod running.
    pub(super) fn request_taker(&self, key: &SessionKey) -> Option<String> {
        let session = self.model.board.get(key).filter(|s| !matches!(s.phase(), Phase::Ended | Phase::Asleep))?;
        // Demo sessions show the controls; what they would send is blocked elsewhere.
        if self.model.is_demo() {
            return Some(key.id.clone());
        }
        session.session_id.clone().filter(|id| self.request_takers.contains(id))
    }

    /// Hands `request` to the session's mod; says so in the status line only when it failed.
    pub(super) fn send_request(&mut self, session_id: &str, request: brain_core::requests::Request) -> bool {
        match brain_core::requests::send(&brain_core::requests::dir(&brain_core::account::home_dir()), session_id, request) {
            Ok(()) => true,
            Err(err) => {
                self.refuse(tr!("Nicht an die Session übergeben: {err}", "Not handed to the session: {err}"));
                false
            }
        }
    }

    /// Sessions whose turn it is and that Brain can type into. Sessions with a permission
    /// dialog open are left out: typed text would answer the dialog.
    fn broadcast_targets(&self) -> Vec<SessionKey> {
        let groups = self.groups(now_ms());
        groups
            .live()
            .into_iter()
            .filter(|s| s.phase() == Phase::YourTurn && !s.awaiting_permission() && self.can_send(&s.key))
            .map(|s| s.key.clone())
            .collect()
    }

    /// Sends `text` to a session through its mod, which submits it once the session is idle (a
    /// known `/command` runs as that command). Line breaks stay inside the prompt. An asleep
    /// session is woken first, and the text goes in once its mod is up.
    fn send_text(&mut self, key: &SessionKey, text: &str) -> bool {
        if self.blocked_in_demo() {
            return false;
        }
        if let Some(waking) = self.waking.get_mut(key) {
            waking.texts.push(text.to_string());
            return true;
        }
        if let Some(id) = self.request_taker(key) {
            return self.send_request(&id, brain_core::requests::Request::Prompt { text: text.to_string() });
        }
        if self.wake(key, text) {
            return true;
        }
        self.refuse(self.why_brain_cant_send());
        false
    }

    /// Wakes an asleep session (`claude respawn`, off the UI thread), with `text` waiting for it.
    fn wake(&mut self, key: &SessionKey, text: &str) -> bool {
        if !self.wakeable(key) {
            return false;
        }
        let Some(session) = self.model.board.get(key) else { return false };
        let (Some(agent), Some(session_id), Some(account)) = (session.agent.clone(), session.session_id.clone(), self.model.account(&key.account).cloned()) else { return false };
        let name = session.display_name();
        self.waking.insert(key.clone(), Waking { session_id, since: std::time::SystemTime::now(), started: Instant::now(), texts: vec![text.to_string()] });
        let (sender, woken) = (self.wake_results.0.clone(), key.clone());
        std::thread::spawn(move || {
            let _ = sender.send((woken, brain_core::agents::respawn(&account, &agent.id)));
        });
        self.set_status(tr!("„{name}“ wird geweckt – die Nachricht geht hinein, sobald sie wach ist.", "Waking “{name}” – the message goes in once it's awake."));
        true
    }

    /// On every refresh: what waited for a woken session goes in once its mod takes requests
    /// again; a wake that failed or took too long says so.
    fn deliver_to_woken(&mut self) {
        while let Ok((key, result)) = self.wake_results.1.try_recv() {
            if let Err(err) = result {
                if self.waking.remove(&key).is_some() {
                    let name = self.model.board.get(&key).map(|s| s.display_name()).unwrap_or_default();
                    self.set_status(tr!("„{name}“ ließ sich nicht wecken: {err}", "Couldn't wake “{name}”: {err}"));
                }
            }
        }
        let dir = brain_core::requests::dir(&brain_core::account::home_dir());
        let keys: Vec<SessionKey> = self.waking.keys().cloned().collect();
        for key in keys {
            let Some(waking) = self.waking.get(&key) else { continue };
            let ready = brain_core::requests::taken_since(&dir, &waking.session_id, waking.since);
            if !ready && waking.started.elapsed() < WAKE_TIMEOUT {
                continue;
            }
            let Some(waking) = self.waking.remove(&key) else { continue };
            let name = self.model.board.get(&key).map(|s| s.display_name()).unwrap_or_default();
            if !ready {
                self.set_status(tr!("„{name}“ ist nicht aufgewacht – die Nachricht ging nicht hinein.", "“{name}” didn't wake up – the message didn't go in."));
                continue;
            }
            let delivered = waking.texts.into_iter().all(|text| self.send_request(&waking.session_id, brain_core::requests::Request::Prompt { text }));
            if delivered {
                self.set_status(tr!("„{name}“ ist wach – die Nachricht ist drin.", "“{name}” is awake – the message is in."));
            }
        }
    }

    /// Why a session takes no text from Brain, for the status line.
    pub(super) fn why_brain_cant_send(&self) -> &'static str {
        t(
            "Die Session hat Brains Mod nicht geladen – „/reload-plugins“ in der Session holt ihn.",
            "The session hasn't loaded Brain's mod – “/reload-plugins” in the session loads it.",
        )
    }

    /// ⌘F: find in the selected session's conversation.
    fn start_find(&mut self) -> Task<Message> {
        self.find_open = true;
        Task::batch([operation::focus(FIND), self.load_history()])
    }

    /// Sums up the selected session once it has ended, before resuming it.
    fn load_brief(&mut self) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        if session.phase() != Phase::Ended || self.has_terminal() {
            return Task::none();
        }
        let key = session.key.clone();
        if self.brief.as_ref().is_some_and(|(k, _)| *k == key) || self.brief_loading.as_ref() == Some(&key) {
            return Task::none();
        }
        let Some((path, _)) = self.conversation.as_ref().filter(|c| c.key == key).and_then(|c| c.transcript()) else {
            return Task::none();
        };
        let path = path.to_path_buf();
        self.brief_loading = Some(key.clone());
        Task::perform(off_thread(move || brain_core::brief::brief(&path)), move |brief| Message::BriefLoaded(key.clone(), brief))
    }

    /// What the terminal would run for the selected session: `claude attach` for a
    /// background session, `claude --resume` for an ended one. Sessions running in an iTerm tab
    /// can't be joined from outside, so they get none.
    pub fn terminal_command(&self) -> Option<(&'static str, Vec<String>, String)> {
        self.terminal_command_for(self.selected_session()?)
    }

    fn terminal_command_for(&self, session: &Session) -> Option<(&'static str, Vec<String>, String)> {
        let cwd = session.cwd.clone().unwrap_or_else(|| brain_core::account::home_dir().display().to_string());
        if let Some(agent) = &session.agent {
            return Some(("claude", vec!["attach".into(), agent.id.clone()], cwd));
        }
        if session.phase() == Phase::Ended {
            // A restart's flags come after the resume.
            let mut args = vec!["--resume".into(), self.conversation_id(session)?];
            args.extend(self.restart_flags.get(&session.key).cloned().unwrap_or_default());
            return Some(("claude", args, cwd));
        }
        None
    }

    /// The selected session and whether Brain can attach to it: what `sync_terminal` follows.
    fn selection_state(&self) -> Option<(SessionKey, bool)> {
        let attachable = self
            .selected_session()
            .is_some_and(|s| s.agent.as_ref().is_some_and(|a| !matches!(a.state.as_str(), "failed" | "stopped")));
        self.selected.clone().map(|k| (k, attachable))
    }

    /// In a split, the other pane becomes the active one (its session selected); the keyboard
    /// stays where it is.
    fn activate_split_pane(&mut self) {
        let Some(other) = self.split.clone() else { return };
        self.split = self.selected.replace(other);
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
            // The active pane and the split's other one.
            self.selected.iter().chain(self.split.iter()).filter(|k| self.terminals.contains_key(*k)).cloned().collect()
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

    /// Gives a running terminal the keyboard; starts nothing.
    fn focus_terminal(&mut self) -> Task<Message> {
        let Some(key) = self.selected.clone() else { return Task::none() };
        match self.terminals.get(&key) {
            Some(term) => {
                self.preview = None;
                let task = iced_term::TerminalView::focus(term.widget_id().clone());
                self.focused_terminal = Some(key);
                task
            }
            None => Task::none(),
        }
    }

    /// Pastes text into the selected session's terminal (a dropped or pasted file's path), like
    /// iTerm does: as one paste, without control characters.
    fn write_to_terminal(&mut self, text: &str) -> bool {
        // The terminal with the keyboard (in a split, maybe not the selected session's).
        let key = self.focused_terminal.clone().or_else(|| self.selected.clone());
        match key.and_then(|k| self.terminals.get_mut(&k)) {
            Some(term) => {
                term.paste(text);
                true
            }
            None => false,
        }
    }

    /// A file dropped on the window goes into the session like in iTerm: its path, escaped for
    /// the shell. Claude Code turns image paths into attachments (`[Image #1]`).
    fn drop_file(&mut self, path: &std::path::Path) -> Task<Message> {
        let escaped = escape_path(&path.display().to_string()) + " ";
        if self.write_to_terminal(&escaped) {
            return self.focus_terminal();
        }
        self.set_status(t(
            "Dateien lassen sich in Sessions ziehen, die in Brain laufen.",
            "Files can be dropped into sessions that run in Brain.",
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

    /// Starts the selected session's terminal (⏎ or the button), and
    /// focuses it.
    fn open_terminal(&mut self, focus: bool) -> Task<Message> {
        if self.model.is_demo() {
            return Task::none();
        }
        let Some(key) = self.selected.clone() else { return Task::none() };
        if !self.terminals.contains_key(&key) && !self.spawn_terminal(&key) {
            return Task::none();
        }
        if focus {
            self.focus_terminal()
        } else {
            Task::none()
        }
    }

    /// Starts a session's terminal in Brain (`claude attach` or `claude --resume`), shown or
    /// not. False when the session has none or it didn't start.
    fn spawn_terminal(&mut self, key: &SessionKey) -> bool {
        let Some((program, args, cwd)) = self.model.board.get(key).and_then(|s| self.terminal_command_for(s)) else { return false };
        {
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
                    program: program.into(),
                    args,
                    env,
                    working_directory: Some(std::path::PathBuf::from(cwd)),
                },
            };
            match iced_term::Terminal::new(self.next_terminal, settings) {
                Ok(term) => {
                    self.next_terminal += 1;
                    self.terminals.insert(key.clone(), term);
                    self.note_terminal_started(key);
                    true
                }
                Err(err) => {
                    self.set_status(tr!("Terminal startet nicht: {err}", "Terminal didn't start: {err}"));
                    false
                }
            }
        }
    }

    /// Keeps the conversation in step with the selected session (⇧⌘C, the export and the find
    /// results read it).
    fn sync_conversation(&mut self) {
        let Some(key) = self.selected.clone() else {
            self.conversation = None;
            return;
        };
        let switched = self.conversation.as_ref().is_none_or(|c| c.key != key);
        if let Some(demo) = &self.model.demo_messages {
            if switched {
                self.conversation = Some(Conversation::fixed(key.clone(), demo.get(&key).cloned().unwrap_or_default()));
            }
            return;
        }
        let session_id = self.model.board.get(&key).and_then(|s| s.session_id.clone());
        let conversation = self.conversation.get_or_insert_with(|| Conversation::empty(key.clone()));
        conversation.sync(&key, session_id.as_deref(), &self.model.accounts);
    }

    /// Looks up the terminal hosting a newly selected session's process, in the background (`ps`
    /// takes 100 ms). A session in Brain's own terminal needs none; a pid is looked up once.
    fn sync_host(&mut self) -> Task<Message> {
        if self.model.is_demo() || self.has_terminal() {
            return Task::none();
        }
        let Some(pid) = self.selected_session().map(|s| s.pid) else { return Task::none() };
        if self.hosts.contains_key(&pid) || !self.hosts_pending.insert(pid) {
            return Task::none();
        }
        Task::perform(
            off_thread(move || terminal::host_of(pid).map(|host| terminal::capabilities(&host)).unwrap_or(Capabilities { focus: false, keys: false })),
            move |caps| Message::HostFound(pid, caps),
        )
    }

    /// Asks git for the branch of the sessions on screen (the selected one and a split's other),
    /// in the background, again after twenty seconds.
    fn sync_branches(&mut self) -> Task<Message> {
        if self.model.is_demo() {
            return Task::none();
        }
        let wanted: Vec<(SessionKey, String)> = self
            .selected
            .iter()
            .chain(self.split.iter())
            .filter(|k| !self.branches_pending.contains(*k) && self.branches.get(*k).is_none_or(|(_, at)| at.elapsed() > Duration::from_secs(20)))
            .filter_map(|k| Some((k.clone(), self.model.board.get(k)?.cwd.clone()?)))
            .collect();
        let tasks: Vec<Task<Message>> = wanted
            .into_iter()
            .map(|(key, cwd)| {
                self.branches_pending.insert(key.clone());
                Task::perform(off_thread(move || brain_core::changes::branch_of(std::path::Path::new(&cwd))), move |branch| Message::BranchFound(key.clone(), branch))
            })
            .collect();
        Task::batch(tasks)
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

    /// The project a session is listed under: the one the user put it in, else its own.
    pub fn project_name(&self, s: &Session) -> String {
        self.prefs.project(&s.key).map(str::to_string).unwrap_or_else(|| self.own_project_name(s))
    }

    /// The project its folder says (the repository, for a worktree its main checkout).
    fn own_project_name(&self, s: &Session) -> String {
        self.projects.get(&s.key).map(|(n, _)| n.clone()).unwrap_or_else(|| s.display_name())
    }

    /// The projects of the tree, by name.
    pub fn project_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.prefs.listed_projects().iter().map(|p| p.name.clone()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// Puts the project of a folder in the tree, a worktree as its main checkout. Returns whether
    /// it was new.
    fn list_project_of(&mut self, folder: &str) -> bool {
        let (name, root) = listed_entry(self.model.project(folder));
        let added = self.prefs.list_project(&name, Some(&root));
        if added {
            self.prefs.save();
        }
        added
    }

    /// Lists the session under `project`, or under its own again (`None`, or its own name).
    pub(super) fn put_in_project(&mut self, key: &SessionKey, project: Option<String>) {
        let Some(session) = self.model.board.get(key) else {
            self.refuse(t("Diese Session gibt es nicht mehr.", "This session is gone."));
            return;
        };
        let name = session.display_name();
        let own = self.own_project_name(session);
        let project = project.map(|p| p.trim().to_string()).filter(|p| !p.is_empty() && *p != own);
        let status = match &project {
            Some(p) => tr!("„{name}“ steht jetzt im Projekt „{p}“.", "“{name}” is now in project “{p}”."),
            None => tr!("„{name}“ steht wieder im Projekt „{own}“.", "“{name}” is back in project “{own}”."),
        };
        if let Some(project) = &project {
            self.prefs.list_project(project, None);
        }
        self.prefs.set_project(key, project);
        self.prefs.save();
        self.set_status(status);
    }

    // ---- the list ---------------------------------------------------------------

    pub fn groups(&self, now: i64) -> Groups<'_> {
        self.groups_with(now, &self.search, self.filter.as_deref(), self.searching())
    }

    /// Whether the list is being searched: text typed, or ⌘O waiting for it (which lists the
    /// ended sessions too).
    pub fn searching(&self) -> bool {
        !self.search.is_empty() || (self.resume_search && self.mode == Mode::Search)
    }

    /// The list as grouped for a search and an account (none: every account). The phone gets it
    /// without the Mac's search and account filter: it has its own.
    pub(super) fn groups_for(&self, now: i64, search: &str, account: Option<&str>) -> Groups<'_> {
        self.groups_with(now, search, account, !search.is_empty())
    }

    fn groups_with(&self, now: i64, search: &str, account: Option<&str>, searching: bool) -> Groups<'_> {
        let mut groups = Groups {
            saved: vec![],
            decide: vec![],
            read: vec![],
            quiet: vec![],
            projects: vec![],
            paused: vec![],
            background: vec![],
            automated: vec![],
            ended: vec![],
            older: vec![],
            older_open: self.prefs.show_older,
            projects_open: !self.prefs.is_section_folded("projects") || searching,
            hidden_open: self.show_hidden_projects || searching,
            paused_open: self.show_snoozed || searching,
            background_open: self.prefs.show_background || searching,
            automated_open: self.prefs.show_automated || searching,
        };
        let visible = self
            .model
            .board
            .sorted()
            .into_iter()
            .filter(|s| account.is_none_or(|f| f == s.key.account))
            .filter(|s| s.matches(search));
        for session in visible {
            let ended = session.phase() == Phase::Ended;
            if ended && (session.is_empty() || !self.model.resumable(session)) {
                continue;
            }
            // Background sessions with something happening show; stale ones (a day without
            // activity) go to their own folded list.
            if session.agent.is_some() && now - session.last_activity_ms > ENDED_RECENT_MS {
                groups.background.push(session);
                continue;
            }
            // Sessions a program started (a security review after each commit, scripts with
            // `claude -p`) too, unless pinned.
            if session.insight.is_automated() && !self.prefs.is_pinned(&session.key) {
                groups.automated.push(session);
                continue;
            }
            if self.prefs.is_hidden(&session.key, session.last_activity_ms) && !searching {
                groups.paused.push(session);
                continue;
            }
            if ended {
                if self.prefs.is_pinned(&session.key) {
                    groups.saved.push(session);
                } else if searching || now - session.last_activity_ms < ENDED_RECENT_MS {
                    groups.ended.push(session);
                } else {
                    groups.older.push(session);
                }
                continue;
            }
            if matches!(session.phase(), Phase::NeedsYou | Phase::YourTurn) && !self.prefs.is_pinned(&session.key) && self.prefs.snoozed_until(&session.key, now).is_some() {
                groups.paused.push(session);
                continue;
            }
            let held = self.held.as_ref().filter(|(key, _)| *key == session.key).map(|(_, place)| *place);
            let open_question = self.questions.get(&session.key).is_some_and(|(_, q)| q.is_some());
            match place(session, held, open_question, &self.prefs, now) {
                Place::Decide(ask) => groups.decide.push((session, ask)),
                Place::Read => groups.read.push(session),
                Place::Quiet => groups.quiet.push(session),
            }
        }
        groups.decide.sort_by_key(|(s, ask)| (*ask, s.phase_since_ms()));
        groups.read.sort_by_key(|s| std::cmp::Reverse(s.phase_since_ms()));
        // In a project its decisions, reports, then the rest.
        let entries = groups
            .decide
            .iter()
            .map(|(s, ask)| Entry::Decide(s, *ask))
            .chain(groups.read.iter().map(|s| Entry::Read(s)))
            .chain(groups.quiet.iter().map(|s| Entry::Quiet(s)));
        let mut projects: Vec<Block> = Vec::new();
        for entry in entries {
            let session = entry.session();
            let name = self.project_name(session);
            match projects.iter_mut().find(|b| b.name == name) {
                Some(block) => block.entries.push(entry),
                None => {
                    let (folded, hidden) = (self.prefs.is_project_folded(&name) && !searching, self.prefs.is_project_hidden(&name));
                    projects.push(Block { name, entries: vec![entry], folded, hidden });
                }
            }
        }
        // A fixed order: projects don't jump when one of their sessions starts to ask.
        projects.sort_by_key(|b| b.name.to_lowercase());
        groups.projects = projects;
        groups
    }

    /// The tree on the left: the listed projects with their sessions, the others at the end.
    pub fn tree<'a>(&'a self, groups: &Groups<'a>) -> Tree<'a> {
        let searching = self.searching();
        let query = self.search.to_lowercase();
        let notes = self.later.iter().filter(|(note, project)| {
            query.split_whitespace().all(|term| format!("{} {project}", note.text).to_lowercase().contains(term))
        });
        let folded = |name: &str| self.prefs.is_project_folded(name);
        let hidden = |name: &str| self.prefs.is_project_hidden(name);
        let notes_open = |name: &str| self.prefs.are_notes_open(name);
        let views = Views {
            folded: &folded,
            hidden: &hidden,
            notes_open: &notes_open,
            others_open: !self.prefs.is_section_folded("others"),
            show_hidden: self.show_hidden_projects,
        };
        Tree::build(groups, self.prefs.listed_projects(), &|s| self.project_name(s), notes, &views, searching)
    }

    /// The sessions ↑↓ reach, in the tree's drawing order.
    pub fn nav_keys(&self) -> Vec<SessionKey> {
        let groups = self.groups(now_ms());
        self.tree(&groups).navigable().iter().map(|s| s.key.clone()).collect()
    }

    /// Scrolls the tree so the selected session is visible (the first one: to the very top).
    fn scroll_to_selected(&self) -> Task<Message> {
        let Some(key) = self.selected.clone() else { return Task::none() };
        let groups = self.groups(now_ms());
        let tree = self.tree(&groups);
        let first = tree.navigable().first().map(|s| s.key.clone());
        let mut top = tree::TOP;
        for node in tree.nodes(self.searching(), self.tree_changes()) {
            let height = node.height();
            if matches!(&node, tree::Node::Session(row) if row.session.key == key) {
                let bottom = top + height;
                let target = match self.list_viewport {
                    _ if first.as_ref() == Some(&key) => Some(0.0),
                    Some((offset, _)) if top < offset => Some(top - 8.0),
                    Some((offset, viewport)) if bottom > offset + viewport => Some(bottom - viewport + 8.0),
                    Some(_) => None,
                    None => Some(top - 8.0),
                };
                return match target {
                    Some(y) => operation::scroll_to(LIST, scrollable::AbsoluteOffset { x: 0.0, y: y.max(0.0) }),
                    None => Task::none(),
                };
            }
            top += height + tree::GAP;
        }
        Task::none()
    }

    /// What the session asks of the user, a guess at a question left out.
    fn real_ask(&self, key: &SessionKey) -> Option<Ask> {
        let open_question = self.questions.get(key).is_some_and(|(_, q)| q.is_some());
        self.model.board.get(key)?.ask(open_question).filter(|ask| *ask != Ask::Maybe)
    }

    /// A background session without activity for a day, or one a program started: listed in
    /// its folded section, never announced.
    fn is_quiet(&self, key: &SessionKey) -> bool {
        let Some(s) = self.model.board.get(key) else { return false };
        let background = s.agent.is_some() && now_ms() - s.last_activity_ms > ENDED_RECENT_MS;
        let automated = s.insight.is_automated() && !self.prefs.is_pinned(key);
        background || automated
    }

    /// The folded sections remember whether they are open.
    fn toggle_automated(&mut self) {
        self.prefs.show_automated = !self.prefs.show_automated;
        self.prefs.save();
    }

    fn toggle_background(&mut self) {
        self.prefs.show_background = !self.prefs.show_background;
        self.prefs.save();
    }

    pub fn account_index(&self, account: &str) -> usize {
        self.model.accounts.iter().position(|a| a.id == account).unwrap_or(0)
    }

    /// How large the window is drawn: 1.0 is 100 %.
    pub fn zoom(&self) -> f32 {
        self.prefs.zoom.unwrap_or(1.0).clamp(ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1])
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.selected.as_ref().and_then(|k| self.model.board.get(k))
    }

    /// What Brain may do with the selected session's terminal. Demo sessions get everything so
    /// screenshots show the controls; their actions are blocked elsewhere.
    pub fn capabilities(&self) -> Capabilities {
        if self.model.is_demo() {
            return Capabilities { focus: true, keys: true };
        }
        self.selected_session().and_then(|s| self.hosts.get(&s.pid)).copied().unwrap_or(Capabilities { focus: false, keys: false })
    }

    // ---- keys -------------------------------------------------------------------

    fn on_key(&mut self, event: keyboard::Event, captured: bool) -> Task<Message> {
        let keyboard::Event::KeyPressed { key, modifiers, text, .. } = event else { return Task::none() };
        // Keys only count while Brain is the active app: with another app in front they're that
        // app's, never Brain's shortcuts.
        if !app_is_active() {
            return Task::none();
        }
        if let Some(focused) = self.focused_terminal.clone() {
            self.last_terminal_input = Some((focused, Instant::now()));
        }
        if !modifiers.command() && !modifiers.control() && (text.as_ref().is_some_and(|t| !t.is_empty()) || matches!(key.as_ref(), keyboard::Key::Named(Named::Enter | Named::Backspace | Named::Escape | Named::Tab | Named::Delete))) {
            // Typing goes on: the selection held for ⌘C is no longer what the user means.
            self.held_copy = None;
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
        let menu_open = self.context_menu.take().is_some() | self.project_menu.take().is_some();
        if menu_open && named == Some(Named::Escape) {
            return Task::none();
        }

        // The diff card has the keyboard while it is open: esc closes it and goes no further.
        if named == Some(Named::Escape) && self.preview.is_some() && self.mode == Mode::Normal && !self.overview {
            self.preview = None;
            return Task::none();
        }
        // A focused terminal keeps Esc (it interrupts Claude); ⌘[ leaves it.
        // Esc always leaves a dialog or view on top (clean-up, new session, palette, overview);
        // only in the normal view does a terminal with the keyboard keep it (it interrupts Claude).
        let terminal_keeps_keys = self.mode == Mode::Normal
            && !self.overview
            && (self.focused_terminal.is_some() || (self.shell_visible && self.focused_shell.is_some()));
        if named == Some(Named::Escape) && !terminal_keeps_keys {
            return self.escape();
        }
        if cmd && self.focused_terminal.is_some() && character.as_deref() == Some("v") {
            // A copied screenshot: the terminal pastes text only, so Brain saves the picture and
            // pastes its path, which Claude Code turns into an attachment.
            if let Some(path) = crate::clipboard::save_image() {
                let escaped = escape_path(&path.display().to_string()) + " ";
                self.write_to_terminal(&escaped);
            }
            return Task::none();
        }
        if cmd && named == Some(Named::Enter) && self.reader.is_some() {
            if let Some(reader) = &self.reader {
                open_reader_file(reader);
            }
            return Task::none();
        }
        if self.mode == Mode::Composer {
            if cmd && named == Some(Named::Enter) {
                return self.send_composer();
            }
            // A copied screenshot goes in as a picture; text pastes into the editor itself.
            if cmd && character.as_deref() == Some("v") {
                if let Some(path) = crate::clipboard::save_image() {
                    self.composer_images.push(path);
                }
            }
            return Task::none();
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
        // The dialog's field takes → for its cursor; in a typed path it goes into a folder.
        if self.mode == Mode::NewSession && named == Some(Named::ArrowRight) {
            if let Some(task) = self.enter_picked_folder() {
                return task;
            }
        }
        // While the terminal has the keyboard, Brain's shortcuts are off: everything is Claude's.
        // A dialog on top (clean-up, new session) has the keys, though.
        if terminal_keeps_keys && (self.has_terminal() || self.focused_shell.is_some()) {
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
            Mode::Later => return self.on_later_key(named, cmd),
            Mode::Pairing | Mode::AccountSetup => return Task::none(),
            Mode::Sync => return self.on_sync_key(named, character.as_deref()),
            Mode::Garden => return self.on_garden_key(named),
            Mode::Keys => {
                if text.as_deref() == Some("?") {
                    self.mode = Mode::Normal;
                }
                return Task::none();
            }
            Mode::Relaunch => {
                match named {
                    Some(Named::Enter) => return self.submit_relaunch(),
                    // "Restart with …" asks only for the flags.
                    _ if self.relaunch.restart => {}
                    Some(Named::Tab) => self.relaunch.keep = !self.relaunch.keep,
                    _ => {
                        // 1, 2, … pick the account in the order the dialog lists them.
                        let digit = character.as_deref().and_then(|c| c.parse::<usize>().ok()).filter(|d| *d >= 1);
                        if let Some(index) = digit.map(|d| d - 1).filter(|i| *i < self.model.accounts.len()) {
                            self.relaunch.target = index;
                        }
                    }
                }
                self.relaunch.error = None;
                return Task::none();
            }
            Mode::NewSession => return self.on_new_session_key(named, character.as_deref(), cmd, modifiers.shift()),
            Mode::Rename => {
                // The field lost its focus (a click elsewhere): give it back instead of acting.
                return operation::focus(RENAME);
            }
            Mode::Search if !matches!(named, Some(Named::ArrowUp | Named::ArrowDown | Named::Enter)) => {
                // Not captured, so the field isn't focused any more: keys act on the list again.
                self.mode = Mode::Normal;
            }
            _ => {}
        }

        if cmd && modifiers.shift() && character.as_deref() == Some("c") {
            return self.act(Action::CopyAnswer);
        }
        if cmd || modifiers.control() {
            return Task::none();
        }

        let list: Vec<SessionKey> = self.nav_keys();
        let current = self.selected.as_ref().and_then(|k| list.iter().position(|l| l == k));
        let next = current.map_or(0, |i| (i + 1).min(list.len().saturating_sub(1)));
        let previous = current.map_or(0, |i| i.saturating_sub(1));

        match named {
            Some(Named::ArrowDown) => return self.select_index(&list, next),
            Some(Named::ArrowUp) => return self.select_index(&list, previous),
            // ⏎ gives the terminal the keyboard (starting it if needed).
            Some(Named::Enter) if (self.has_terminal() || self.terminal_command().is_some()) => return self.open_terminal(true),
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
            _ => {}
        }
        Task::none()
    }

    /// Esc: leaves whatever is open — search (cleared), rename, a dialog.
    fn escape(&mut self) -> Task<Message> {
        // A dialog takes the keyboard from any terminal.
        if self.mode != Mode::Normal {
            self.focused_terminal = None;
        }
        if self.overview {
            self.overview = false;
            return Task::none();
        }
        // The find bar closes back to the terminal.
        if self.mode == Mode::Normal && (!self.find.is_empty() || self.find_open) {
            self.find.clear();
            self.find_open = false;
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
            Mode::Rename | Mode::NewSession | Mode::Cleanup | Mode::Later | Mode::Palette | Mode::Composer | Mode::Pairing | Mode::AccountSetup | Mode::Relaunch | Mode::Sync | Mode::Keys | Mode::Garden => {
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
        let first = self.nav_keys().first().cloned();
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
        let message = message.into();
        if !self.model.is_demo() {
            brain_core::log::write(&brain_core::account::home_dir(), "status", &message);
        }
        if let Some(heard) = self.link_heard.as_mut() {
            heard.said = Some(message.clone());
        }
        self.status = Some((message, Instant::now()));
    }

    /// Says why a command from the phone (or anything) wasn't carried out: in the status line,
    /// and to the phone as a refusal.
    pub(super) fn refuse(&mut self, message: impl Into<String>) {
        self.set_status(message);
        if let Some(heard) = self.link_heard.as_mut() {
            heard.refused = true;
        }
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
            Outcome::NoTerminal => self.refuse(t(
                "Kein Terminal zu dieser Session gefunden (beendet oder ohne Terminal gestartet).",
                "No terminal found for this session (it ended or runs without one).",
            )),
            Outcome::Unsupported(reason) => self.refuse(reason),
            Outcome::Failed(reason) => self.refuse(tr!("Fehlgeschlagen: {reason}", "Failed: {reason}")),
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
            .live()
            .into_iter()
            .filter(|s| matches!(s.phase(), Phase::NeedsYou | Phase::YourTurn))
            .find(|s| self.terminals.contains_key(&s.key) || s.agent.is_some())
            .map(|s| s.key.clone());
        if let Some(key) = waiting {
            self.selected = Some(key);
        }
        self.overview = false;
        self.mode = Mode::Normal;
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
                return self.open_terminal(true);
            }
            Action::Open => self.open_selected(),
            Action::OpenInITerm => self.open_selected(),
            Action::TakeOver => {
                self.take_over();
                return Task::none();
            }
            Action::Pin => self.toggle_pin(),
            Action::KeepProject => self.keep_selected_project(),
            Action::OnlyReport => {
                if let Some(key) = self.selected.clone() {
                    self.prefs.mark_report(&key, now_ms());
                    self.prefs.save();
                }
            }
            Action::MoveToProject => {
                let task = self.open_palette();
                self.palette = t("Projekt", "project").into();
                return task;
            }
            Action::OpenBeside => {
                // The terminal that was on screen stays on the left, this one opens beside it.
                if let Some(kept) = self.before_menu.take().filter(|k| self.terminals.contains_key(k)) {
                    self.split = Some(kept);
                }
                self.overview = false;
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
            Action::Relaunch => return self.open_relaunch(relaunch::Preset::Move),
            Action::RestartWith => return self.open_relaunch(relaunch::Preset::Restart),
            Action::MoveTo(index) => return self.move_selected(index, Vec::new()),
            Action::ForkTo(index) => return self.fork_selected(index, Vec::new()),
            Action::End => return self.end_selected(confirmed),
            Action::Hide => self.hide_selected(confirmed),
            Action::Trash => self.trash_selected(confirmed),
            Action::ForceEnd => return self.force_end_selected(),
            Action::Resume => self.resume_selected(),
            Action::Allow => self.answer_permission(true),
            Action::Deny => self.answer_permission(false),
            Action::StartRename => return self.start_rename(),
            Action::StartTerminal => return self.open_terminal(true),
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
        if !self.can_type_into_selected() {
            self.set_status(self.why_brain_cant_send());
            return Task::none();
        }
        self.rename = session.display_name();
        self.mode = Mode::Rename;
        Task::batch([operation::focus(RENAME), operation::select_all(RENAME)])
    }

    /// `/rename` goes through the session's mod, which submits it once the session is idle: a
    /// working session can be renamed too.
    fn submit_rename(&mut self) -> Task<Message> {
        self.mode = Mode::Normal;
        let name = self.rename.trim().to_string();
        if !name.is_empty() && !self.blocked_in_demo() {
            if let Some(key) = self.selected.clone() {
                if self.send_text(&key, &format!("/rename {name}")) {
                    self.set_status(tr!("„{name}“ an die Session geschickt.", "Sent “{name}” to the session."));
                }
            }
        }
        unfocus()
    }

    /// Sends text, but only if the session still waits for input right now.
    fn type_into_selected(&mut self, text: &str, done: String) -> Task<Message> {
        let Some(key) = self.selected.clone() else { return Task::none() };
        self.type_into(&key, text, done)
    }

    /// [`type_into_selected`] for any session.
    fn type_into(&mut self, key: &SessionKey, text: &str, done: String) -> Task<Message> {
        let key = key.clone();
        if !self.model.board.get(&key).is_some_and(|s| s.waits_for_input()) {
            self.set_status(t("Die Session arbeitet inzwischen wieder – nichts geschickt.", "The session is working again – nothing sent."));
            return Task::none();
        }
        if self.send_text(&key, text) {
            self.set_status(done);
        }
        Task::none()
    }

    /// Whether Brain can send the selected session text.
    fn can_type_into_selected(&self) -> bool {
        self.selected.as_ref().is_some_and(|key| self.can_send(key))
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

    /// `S`: not snoozed → 15 min → 1 h → until tomorrow 9:00 → until something happens in it
    /// (hidden, as `⌫ ⌫`) → back.
    fn cycle_snooze(&mut self) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        if self.prefs.is_hidden(&key, session.last_activity_ms) {
            self.prefs.unhide(&key);
            self.prefs.save();
            self.set_status(t("Wieder da.", "Back."));
            return;
        }
        let now = now_ms();
        let tomorrow = tomorrow_morning();
        let current = self.prefs.snoozed_until(&key, now);
        let quarter = now + 15 * 60 * 1000;
        let hour = now + 60 * 60 * 1000;
        let next = match current {
            None => Some(quarter),
            Some(until) if until <= quarter + 60_000 => Some(hour),
            Some(until) if until <= hour + 60_000 => tomorrow,
            Some(_) => {
                self.prefs.snooze(&key, None);
                self.prefs.hide(&key, now);
                self.prefs.save();
                self.set_status(t("Pausiert, bis sich in der Session etwas tut.", "Snoozed until something happens in the session."));
                return;
            }
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

    /// ⌥⌘X twice: ends a waiting session with `/exit` (a background one with `claude stop`).
    fn end_selected(&mut self, confirmed: bool) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        let key = session.key.clone();
        // Asleep, it has no process to stop: out of the list, until it wakes.
        if session.phase() == Phase::Asleep {
            self.prefs.hide(&key, now_ms());
            self.prefs.save();
            self.set_status(t("Die Session schläft schon – aus der Liste genommen.", "The session is asleep already – taken off the list."));
            return Task::none();
        }
        if let Some(agent) = session.agent.clone().filter(|a| a.is_active()) {
            if !confirmed && !self.arm('x', &key) {
                self.set_status(t("Nochmal ⌥⌘X stoppt die Hintergrund-Session (das Gespräch bleibt).", "Press ⌥⌘X again to stop the background session (its conversation is kept)."));
                return Task::none();
            }
            if self.blocked_in_demo() {
                return Task::none();
            }
            let Some(account) = self.model.account(&key.account).cloned() else { return Task::none() };
            match brain_core::agents::stop(&account, &agent.id) {
                Ok(()) => {
                    self.model.relist_transcripts();
                    // Stopped on purpose: out of the list, not lying there asleep. It comes back
                    // when something happens in it.
                    self.prefs.hide(&key, now_ms());
                    self.prefs.save();
                    self.set_status(t("Hintergrund-Session gestoppt.", "Stopped the background session."));
                }
                Err(err) => self.set_status(tr!("Stoppen fehlgeschlagen: {err} – Rechtsklick → Hart beenden hilft.", "Stopping failed: {err} – right-click → End by force helps.")),
            }
            return Task::none();
        }
        if session.phase() == Phase::Ended {
            self.set_status(t("Die Session ist schon beendet.", "The session has already ended."));
            return Task::none();
        }
        if !session.accepts_input() {
            self.set_status(t("Beenden geht, sobald die Session fertig ist und auf dich wartet. Hängt sie: Rechtsklick → Hart beenden.", "Ending works once the session has finished and waits for you. If it hangs: right-click → End by force."));
            return Task::none();
        }
        if !confirmed && !self.arm('x', &key) {
            self.set_status(t("Nochmal ⌥⌘X beendet die Session (fortsetzen mit ⏎).", "Press ⌥⌘X again to end the session (⏎ resumes it)."));
            return Task::none();
        }
        if self.blocked_in_demo() {
            return Task::none();
        }
        self.type_into_selected("/exit", t("Session beendet – ⏎ setzt sie fort, ⌥⌘P merkt sie dir.", "Session ended – ⏎ resumes it, ⌥⌘P saves it for later.").into())
    }

    /// `A` twice: continues a session in the next account. The transcript is copied and resumed
    /// there in a new tab (`--fork-session`); a running original gets `/exit`.
    /// A session that runs in Brain moves within Brain: it stops here and a copy continues in
    /// the other account's background, attached in Brain.
    /// The accounts the selected session can move to, as indices among the accounts.
    pub fn move_targets(&self) -> Vec<usize> {
        let Some(session) = self.selected_session() else { return Vec::new() };
        (0..self.model.accounts.len()).filter(|&i| self.model.accounts[i].id != session.key.account).collect()
    }

    /// The accounts the selected session can be forked into: all of them, its own too.
    pub fn fork_targets(&self) -> Vec<usize> {
        if self.selected_session().is_none() {
            return Vec::new();
        }
        (0..self.model.accounts.len()).collect()
    }

    /// `F`: the conversation so far goes on as a new background session in the account at
    /// `target_index`, attached in Brain; the original keeps running. Into another account the
    /// transcript is copied first, as for a move.
    pub(super) fn fork_selected(&mut self, target_index: usize, flags: Vec<String>) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        let (Some(session_id), Some(cwd)) = (session.session_id.clone(), session.cwd.clone()) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder."));
            return Task::none();
        };
        let name = session.name.as_ref().map(|n| format!("{n} (fork)"));
        let (Some(source), Some(target)) = (self.model.account(&session.key.account).cloned(), self.model.accounts.get(target_index).cloned()) else { return Task::none() };
        if self.blocked_in_demo() {
            return Task::none();
        }
        let transcript = if source.id == target.id {
            None
        } else {
            let _ = brain_core::account_setup::carry_trust(&source, &target, &brain_core::account::home_dir(), &cwd);
            let Some(found) = brain_core::transcript::find_transcript(&source, &session_id) else {
                self.set_status(t("Transkript nicht gefunden.", "Transcript not found."));
                return Task::none();
            };
            Some(found)
        };
        let target_id = target.id.clone();
        self.set_status(tr!("Forke die Session in {target_id} …", "Forking the session into {target_id} …"));
        let forked = off_thread(move || {
            if let Some(transcript) = transcript {
                brain_core::transcript::copy_to_account(&transcript, &target).map_err(|e| e.to_string())?;
            }
            brain_core::agents::fork(&target, std::path::Path::new(&cwd), &session_id, name.as_deref(), &flags)
        });
        Task::perform(forked, move |result| Message::Started(target_id.clone(), result))
    }

    pub(super) fn move_selected(&mut self, target_index: usize, flags: Vec<String>) -> Task<Message> {
        let Some(key) = self.selected.clone() else { return Task::none() };
        self.move_session(key, target_index, flags)
    }

    /// Continues the session in the account at `target_index`, with `flags` for its new start.
    pub(super) fn move_session(&mut self, key: SessionKey, target_index: usize, flags: Vec<String>) -> Task<Message> {
        let Some(session) = self.model.board.get(&key) else { return Task::none() };
        let Some(target) = self.model.accounts.get(target_index).cloned().filter(|t| t.id != key.account) else {
            self.set_status(t("Es gibt kein zweites Konto.", "There is no other account."));
            return Task::none();
        };
        let ended = session.phase() == Phase::Ended;
        if self.moving.get(&key).is_some_and(|at| at.elapsed() < MOVE_SETTLE) {
            return Task::none();
        }
        if !movable(session) {
            self.set_status(t("Umziehen geht, sobald die Session fertig ist und auf dich wartet.", "Moving works once the session has finished and waits for you."));
            return Task::none();
        }
        let (session_id, cwd, name) = (self.conversation_id(session), session.cwd.clone(), session.name.clone());
        let running_agent = session.agent.clone().filter(|a| a.is_active());
        let in_brain = session.agent.is_some();
        let (Some(session_id), Some(cwd)) = (session_id, cwd) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder."));
            return Task::none();
        };
        if self.blocked_in_demo() {
            return Task::none();
        }
        let Some(source) = self.model.account(&key.account).cloned() else { return Task::none() };
        self.moving.insert(key.clone(), Instant::now());
        // A folder trusted there is trusted here: the moved session doesn't stop at the question.
        let _ = brain_core::account_setup::carry_trust(&source, &target, &brain_core::account::home_dir(), &cwd);
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
            self.hide_move_leftover(&target_id, &session_id);
            let original = key.clone();
            let moved = off_thread(move || {
                if let Some(agent) = running_agent {
                    brain_core::agents::stop(&source, &agent.id)?;
                }
                brain_core::transcript::copy_to_account(&transcript, &target).map_err(|e| e.to_string())?;
                brain_core::agents::fork(&target, std::path::Path::new(&cwd), &session_id, name.as_deref(), &flags)
            });
            // The session goes on in the target account: the original leaves the list.
            return Task::perform(moved, move |result| Message::Replaced(original.clone(), target_id.clone(), result));
        }
        if let Err(err) = brain_core::transcript::copy_to_account(&transcript, &target) {
            self.set_status(tr!("Kopieren fehlgeschlagen: {err}", "Copy failed: {err}"));
            return Task::none();
        }
        let config_dir = (target.id != "main").then(|| target.config_dir.display().to_string());
        let extra: String = flags.iter().map(|f| format!(" {}", crate::format::shell_quote(f))).collect();
        let outcome = terminal::open_new(&cwd, config_dir.as_deref(), &format!("claude --resume {session_id} --fork-session{extra}"));
        if !matches!(outcome, Outcome::Done) {
            self.report(outcome, None);
            return Task::none();
        }
        if self.prefs.is_pinned(&key) && ended {
            self.prefs.toggle_pin(&key);
            self.prefs.save();
        }
        // It goes on in the target account: the original and the transcript left there for the
        // copy to start from leave the list.
        self.hide_move_leftover(&target.id, &session_id);
        self.prefs.hide(&key, now_ms());
        self.prefs.save();
        let target = target.id;
        if ended {
            self.set_status(tr!("In {target} fortgesetzt.", "Resumed in {target}."));
            return Task::none();
        }
        let closed = self.send_text(&key, "/exit");
        self.set_status(if closed {
            tr!("Nach {target} umgezogen, das Original ist geschlossen.", "Moved to {target}; the original is closed.")
        } else {
            tr!("Nach {target} umgezogen. Das Original konnte Brain nicht schließen.", "Moved to {target}. Brain could not close the original.")
        });
        Task::none()
    }

    /// A move copies the transcript into the target account for the copy to start from: Brain
    /// would list it there as an ended session beside the copy. It is hidden (until something
    /// happens in it, which nothing does).
    fn hide_move_leftover(&mut self, target: &str, session_id: &str) {
        self.prefs.hide(&SessionKey { account: target.to_string(), id: session_id.to_string() }, now_ms());
        self.prefs.save();
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
        self.set_status(t("Ausgeblendet – über ⌥⌘F findest du sie wieder.", "Hidden – ⌥⌘F finds it again."));
    }

    /// ⌥⌘⌫ twice: an ended session's transcript goes to the Trash; a background session is
    /// removed with `claude rm` after its transcript went to the Trash.
    fn trash_selected(&mut self, confirmed: bool) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        let deletable = session.phase() == Phase::Ended || session.agent.as_ref().is_some_and(|a| !a.is_active());
        if !deletable {
            self.set_status(t("Löschen geht nur bei beendeten Sessions – erst beenden (⌥⌘X).", "Only ended sessions can be deleted – end it first (⌥⌘X)."));
            return;
        }
        if !confirmed && !self.arm('d', &key) {
            self.set_status(t("Nochmal ⌥⌘⌫ legt die Session in den Papierkorb.", "Press ⌥⌘⌫ again to move the session to the Trash."));
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
            // A file already gone (moved, deleted elsewhere) is no reason to keep the entry.
            for path in brain_core::transcript::session_files(&account, &session_id).into_iter().filter(|p| p.exists()) {
                crate::trash::move_to_trash(&path)?;
            }
        }
        // Claude Code may not know the background session any more (removed elsewhere, moved):
        // the conversation is in the Trash, so the entry goes all the same; the log says why.
        if let Some(agent) = agent {
            if let Err(err) = brain_core::agents::remove(&account, &agent.id) {
                brain_core::log::write(&brain_core::account::home_dir(), "trash", &format!("claude rm {}: {err}", agent.id));
            }
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
            .filter(|s| s.phase() == Phase::Ended || s.agent.is_some())
            .filter(|s| s.last_activity_ms < cutoff)
            .filter(|s| s.phase() != Phase::Ended || (self.model.resumable(s) && !s.is_empty()))
            .collect()
    }

    /// Reads the tasks noted for later again when their file changed, each with the project of
    /// its folder. The demo has made-up ones.
    fn refresh_later(&mut self) {
        if self.model.is_demo() {
            if self.later.is_empty() {
                self.later = crate::demo::later();
            }
            return;
        }
        let file = brain_core::later::file(&brain_core::account::home_dir());
        let stamp = std::fs::metadata(&file).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
        if stamp == self.later_stamp {
            return;
        }
        self.later_stamp = stamp;
        let tasks = brain_core::later::open(&file);
        self.later = tasks
            .into_iter()
            .map(|task| {
                let project = task.cwd.clone().map(|cwd| self.model.project(&cwd).name.clone()).unwrap_or_default();
                (task, project)
            })
            .collect();
        self.later_pick = self.later_pick.min(self.later.len().saturating_sub(1));
    }

    /// `L`: the tasks noted for later, read again each time the dialog opens.
    fn open_later(&mut self) {
        self.armed = None;
        self.later_stamp = None;
        self.refresh_later();
        self.mode = Mode::Later;
    }

    /// In the dialog for later: ↑↓ pick, ⏎ starts the task as a new session where it was noted,
    /// ⌫ checks it off.
    fn on_later_key(&mut self, named: Option<Named>, cmd: bool) -> Task<Message> {
        let count = self.later.len();
        match named {
            Some(Named::ArrowDown) => self.later_pick = (self.later_pick + 1).min(count.saturating_sub(1)),
            Some(Named::ArrowUp) => self.later_pick = self.later_pick.saturating_sub(1),
            Some(Named::Enter) => {
                let Some((task, _)) = self.later.get(self.later_pick).cloned() else { return Task::none() };
                return self.start_task(task);
            }
            Some(Named::Backspace | Named::Delete) if !cmd => {
                let Some((task, _)) = self.later.get(self.later_pick).cloned() else { return Task::none() };
                self.check_off(&task);
            }
            _ => {}
        }
        Task::none()
    }

    /// Checks a task off (⌫ in the dialog, the tick in the tree).
    fn check_off(&mut self, task: &brain_core::later::Item) {
        if self.blocked_in_demo() {
            return;
        }
        match brain_core::later::done(&brain_core::later::file(&brain_core::account::home_dir()), &task.id) {
            Ok(()) => {
                let text = brain_core::hook::one_line(&task.text, 60);
                self.set_status(tr!("Abgehakt: {text}", "Checked off: {text}"));
            }
            Err(err) => self.set_status(err.to_string()),
        }
        self.later_stamp = None;
        self.refresh_later();
    }

    fn check_off_note(&mut self, id: &str) {
        if let Some((task, _)) = self.later.iter().find(|(task, _)| task.id == id).cloned() {
            self.check_off(&task);
        }
    }

    fn start_note(&mut self, id: &str) -> Task<Message> {
        match self.later.iter().find(|(task, _)| task.id == id).cloned() {
            Some((task, _)) => self.start_task(task),
            None => Task::none(),
        }
    }

    /// The task as a new background session in its folder and account, the task as its first
    /// prompt; it is checked off once started. Its project joins the tree.
    fn start_task(&mut self, task: brain_core::later::Item) -> Task<Message> {
        let Some(folder) = task.cwd.clone().filter(|f| std::path::Path::new(f).is_dir()) else {
            self.set_status(t("Der Ordner dieser Aufgabe fehlt – ⌘N startet sie woanders.", "This task's folder is missing – ⌘N starts it elsewhere."));
            return Task::none();
        };
        let account = task.account.as_deref().and_then(|id| self.model.account(id)).or_else(|| self.model.accounts.first()).cloned();
        let Some(account) = account else { return Task::none() };
        if self.blocked_in_demo() {
            return Task::none();
        }
        self.list_project_of(&folder);
        let _ = brain_core::later::done(&brain_core::later::file(&brain_core::account::home_dir()), &task.id);
        self.later_stamp = None;
        self.refresh_later();
        self.mode = Mode::Normal;
        self.set_status(t("Starte die Aufgabe als neue Session …", "Starting the task as a new session …"));
        let id = account.id.clone();
        let started = off_thread(move || brain_core::agents::start(&account, std::path::Path::new(&folder), &brain_core::agents::Start { prompt: Some(task.text.clone()), ..Default::default() }));
        Task::perform(started, move |result| Message::Started(id.clone(), result))
    }

    /// "+ Neues Projekt …": the folder dialog; the folder's project (a worktree's main checkout)
    /// joins the tree, and shows again if it was hidden.
    fn new_project(&mut self) -> Task<Message> {
        let Some(path) = crate::folder_picker::choose_folder() else { return Task::none() };
        let folder = path.display().to_string();
        let (name, root) = listed_entry(self.model.project(&folder));
        self.prefs.list_project(&name, Some(&root));
        if self.prefs.is_project_hidden(&name) {
            self.prefs.toggle_project_hidden(&name);
        }
        self.prefs.save();
        self.set_status(tr!("Projekt „{name}“ steht jetzt im Baum.", "Project “{name}” is in the tree now."));
        Task::none()
    }

    /// A new session in a project: the dialog with the project's folder as the first choice.
    fn new_session_in(&mut self, name: &str) -> Task<Message> {
        let folder = self.prefs.listed_projects().iter().find(|p| p.name == name).and_then(|p| p.folder.clone());
        let task = self.open_new_session_dialog();
        if let Some(folder) = folder {
            self.new_session.folder = folder;
        }
        task
    }

    /// "Als Projekt behalten": the project of a session among the others joins the tree.
    fn keep_selected_project(&mut self) {
        let Some(cwd) = self.selected_session().and_then(|s| s.cwd.clone()) else { return };
        let (name, root) = listed_entry(self.model.project(&cwd));
        self.prefs.list_project(&name, Some(&root));
        self.prefs.save();
        self.set_status(tr!("Projekt „{name}“ steht jetzt im Baum.", "Project “{name}” is in the tree now."));
    }

    /// A session whose last sentence ends with a question mark, not yet taken for a report.
    pub fn is_guess(&self, s: &Session) -> bool {
        let open_question = self.questions.get(&s.key).is_some_and(|(_, q)| q.is_some());
        s.ask(open_question) == Some(Ask::Maybe) && !self.prefs.is_report(&s.key, s.phase_since_ms())
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
    pub(crate) fn folder_suggestions(&self, query: &str) -> Vec<String> {
        let mut sessions: Vec<&Session> = self.model.board.keys().filter_map(|k| self.model.board.get(k)).collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity_ms));
        let mut folders: Vec<String> = Vec::new();
        if query.starts_with('/') || query.starts_with('~') {
            let home = brain_core::account::home_dir();
            let typed = query.replacen('~', &home.display().to_string(), 1);
            let first = if typed.len() > 1 { typed.trim_end_matches('/').to_string() } else { typed.clone() };
            folders.push(first.clone());
            // A path lists the folders it leads to (inside the home folder): `~/Work/` all of
            // Work's, `~/Work/ap` those that start with "ap". → goes into the picked one.
            let (dir, start) = match typed.rsplit_once('/') {
                Some((dir, start)) => (if dir.is_empty() { "/" } else { dir }, start.to_lowercase()),
                None => (typed.as_str(), String::new()),
            };
            if let Some(dir) = brain_core::folders::inside(&home, std::path::Path::new(dir)) {
                let found = brain_core::folders::subfolders(&dir).into_iter().map(|p| p.display().to_string());
                let matching = found.filter(|p| p.rsplit('/').next().is_some_and(|name| name.to_lowercase().starts_with(&start)));
                folders.extend(matching.filter(|p| *p != first).take(30));
            }
            return folders;
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
        if cmd && character == Some("o") {
            return self.choose_folder();
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
            Some(Named::Tab) if shift => self.new_session.style = self.next_style(self.new_session.style.as_deref()),
            Some(Named::Tab) => self.new_session.account = (self.new_session.account + 1) % self.model.accounts.len().max(1),
            // Every other key goes to the field (it lost its focus to a click).
            _ => return operation::focus(NEW_SESSION),
        }
        Task::none()
    }

    /// Opens the native folder dialog; the folder lands in the dialog's field, where ⏎ starts a
    /// session in it. Not while a template asks for a value: the field holds that value then.
    fn choose_folder(&mut self) -> Task<Message> {
        if self.pending_placeholder().is_some() {
            return Task::none();
        }
        let Some(path) = crate::folder_picker::choose_folder() else { return Task::none() };
        self.new_session.folder = path.display().to_string();
        self.new_session.pick = 0;
        // The dialog took the keyboard from the field: ⏎ goes to it again.
        operation::focus(NEW_SESSION)
    }

    /// The output styles of the dialog's account (built-in and its own).
    pub fn new_session_styles(&self) -> Vec<brain_core::styles::Style> {
        let config_dir = self.model.accounts.get(self.new_session.account).map(|a| a.config_dir.clone()).unwrap_or_default();
        brain_core::styles::available(&config_dir, None)
    }

    /// The output styles the selected session can switch to (its account's and project's).
    fn selected_styles(&self) -> Vec<brain_core::styles::Style> {
        let Some(session) = self.selected_session() else { return Vec::new() };
        let Some(account) = self.model.account(&session.key.account) else { return Vec::new() };
        brain_core::styles::available(&account.config_dir, session.cwd.as_deref().map(std::path::Path::new))
    }

    /// ⇧⇥: as the account's settings say → each style in turn → as the settings say.
    fn next_style(&self, current: Option<&str>) -> Option<String> {
        let names: Vec<String> = self.new_session_styles().into_iter().map(|s| s.name).collect();
        match current.and_then(|c| names.iter().position(|n| n == c)) {
            None => names.first().cloned(),
            Some(i) => names.get(i + 1).cloned(),
        }
    }

    /// → in the dialog while it holds a typed path: goes into the picked folder and lists its
    /// folders. `None` when there is no path or no folder to go into.
    fn enter_picked_folder(&mut self) -> Option<Task<Message>> {
        if !self.new_session.folder.starts_with(['/', '~']) || self.pending_placeholder().is_some() {
            return None;
        }
        let Some(Choice::Folder(folder)) = self.new_session_choices().get(self.new_session.pick).cloned() else { return None };
        self.new_session.folder = format!("{}/", crate::format::tilde(&folder).trim_end_matches('/'));
        self.new_session.pick = 0;
        Some(Task::batch([operation::focus(NEW_SESSION), operation::move_cursor_to_end(NEW_SESSION)]))
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
        let style = self.new_session.style.clone();
        self.mode = Mode::Normal;
        let Some(account) = self.model.accounts.get(self.new_session.account).cloned() else { return unfocus() };
        if self.blocked_in_demo() {
            return unfocus();
        }
        self.list_project_of(&folder);
        if in_iterm {
            let mut command = String::from("claude");
            if let Some(model) = &model {
                command.push_str(&format!(" --model {}", shell_quote(model)));
            }
            if let Some(style) = &style {
                command.push_str(&format!(" --settings {}", shell_quote(&brain_core::styles::settings(style))));
            }
            if let Some(prompt) = &prompt {
                command.push_str(&format!(" -- {}", shell_quote(prompt)));
            }
            let config_dir = (account.id != "main").then(|| account.config_dir.display().to_string());
            let outcome = terminal::open_new(&folder, config_dir.as_deref(), &command);
            self.report(outcome, Some(t("Neue Session in iTerm gestartet.", "Started a new session in iTerm.").into()));
            return unfocus();
        }
        let options = brain_core::agents::Start { model, name, style, prompt, resume: None };
        self.set_status(t("Starte die Session in Brain …", "Starting the session in Brain …"));
        let id = account.id.clone();
        let started = off_thread(move || brain_core::agents::start(&account, std::path::Path::new(&folder), &options));
        Task::batch([unfocus(), Task::perform(started, move |result| Message::Started(id.clone(), result))])
    }

    /// `I` twice: moves a session running in an iTerm tab into Brain. Brain sends it `/exit`,
    /// waits until it ended, and continues it in the background under the same id.
    pub(super) fn take_over(&mut self) {
        let Some(session) = self.selected_session() else { return };
        let key = session.key.clone();
        if session.agent.is_some() || self.terminals.contains_key(&key) {
            self.set_status(t("Diese Session läuft schon in Brain.", "This session already runs in Brain."));
            return;
        }
        let ended = session.phase() == Phase::Ended;
        if !ended && !session.accepts_input() {
            self.set_status(t("Übernehmen geht, sobald die Session auf dich wartet.", "Taking over works once the session waits for you."));
            return;
        }
        if self.blocked_in_demo() {
            return;
        }
        self.note_started_from(&key);
        if !ended {
            // Never one in Brain's terminal (see above): typing goes through iTerm, no task.
            let _ = self.type_into_selected("/exit", t("Beende die Session in iTerm …", "Ending the session in iTerm …").into());
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
        let copy = self.goes_on_from_elsewhere(&key);
        let (Some(session_id), Some(cwd), Some(account)) = (self.conversation_id(session), session.cwd.clone(), self.model.account(&key.account).cloned()) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder."));
            return Task::none();
        };
        self.started_from.remove(&key);
        self.set_status(t("Setze die Session in Brain fort …", "Continuing the session in Brain …"));
        let id = account.id.clone();
        let resumed = off_thread(move || brain_core::agents::resume(&account, std::path::Path::new(&cwd), &session_id, name.as_deref()));
        // A copy without a message goes on as the conversation it came from, which takes its place.
        Task::perform(resumed, move |result| if copy { Message::Replaced(key.clone(), id.clone(), result) } else { Message::Started(id.clone(), result) })
    }

    /// The conversation to resume or move for a session: its own id, or, for a copy
    /// (`--resume … --fork-session`) that has had no message yet, the one it was started from.
    /// Claude Code writes a copy's transcript only with its first message: resuming the copy's
    /// id fails ("source session … not found").
    pub(super) fn conversation_id(&self, session: &Session) -> Option<String> {
        let own = session.session_id.clone()?;
        let account = self.model.account(&session.key.account)?;
        if brain_core::transcript::find_transcript(account, &own).is_some() {
            return Some(own);
        }
        let running = session.alive && brain_core::process::pid_alive(session.pid);
        let from = if running { brain_core::process::resumed_from(session.pid) } else { self.started_from.get(&session.key).cloned() };
        Some(from.unwrap_or(own))
    }

    /// Before a session is ended to go on: the conversation it goes on from, while its command
    /// line is still there to tell.
    pub(super) fn note_started_from(&mut self, key: &SessionKey) {
        let Some(session) = self.model.board.get(key) else { return };
        match self.conversation_id(session).filter(|id| Some(id) != session.session_id.as_ref()) {
            Some(id) => self.started_from.insert(key.clone(), id),
            None => self.started_from.remove(key),
        };
    }

    /// Whether the session goes on as another conversation (see `conversation_id`).
    pub(super) fn goes_on_from_elsewhere(&self, key: &SessionKey) -> bool {
        self.model.board.get(key).is_some_and(|s| self.conversation_id(s).is_some_and(|id| Some(&id) != s.session_id.as_ref()))
    }
}

/// A project as the tree lists it: its name and the folder of its main checkout.
fn listed_entry(project: &brain_core::project::Project) -> (String, String) {
    (project.name.clone(), project.root.display().to_string())
}

/// The first start with a project list: the projects of the sessions running in Brain right now
/// (background sessions, which is what Brain's terminals attach to) go in, once; nothing else.
fn seed_project_list(model: &mut Model, prefs: &mut Prefs) -> bool {
    let running: Vec<(SessionKey, String)> = model
        .board
        .sorted()
        .into_iter()
        .filter(|s| s.agent.is_some() && s.phase() != Phase::Ended)
        .filter_map(|s| Some((s.key.clone(), s.cwd.clone()?)))
        .collect();
    let projects: Vec<(String, Option<String>)> = running
        .into_iter()
        .map(|(key, cwd)| {
            let (name, root) = listed_entry(model.project(&cwd));
            match prefs.project(&key) {
                Some(moved) => (moved.to_string(), None),
                None => (name, Some(root)),
            }
        })
        .collect();
    prefs.seed_projects(projects)
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

/// How long a session just moved counts as moving, until the board has caught up.
const MOVE_SETTLE: Duration = Duration::from_secs(30);

/// Whether the session can move to another account now. One that runs in Brain is stopped, so
/// it only must not be mid-turn; one in iTerm gets `/exit` typed, so its prompt box must be
/// ready; an ended one just resumes there.
fn movable(session: &Session) -> bool {
    match session.phase() {
        Phase::Ended => true,
        phase if session.agent.is_some() => phase != Phase::Working,
        _ => session.accepts_input(),
    }
}

/// What to do with text a terminal asks to copy (OSC 52).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyAsk {
    /// Nobody asked for it: output, not the user.
    Ignore,
    /// Claude Code's own command (`/copy`): to the clipboard at once.
    Now,
    /// Claude Code copied its mouse selection: Brain holds it until ⌘C.
    Hold,
}

/// Decides from how long ago the user last typed into the terminal and let go of the mouse in it.
/// A mouse release counts only when it came after the last key: that was a selection, not a click
/// before typing `/copy`.
pub fn copy_decision(since_typing: Option<Duration>, since_release: Option<Duration>) -> CopyAsk {
    let typed = since_typing.is_some_and(|d| d < CLIPBOARD_AFTER_TYPING);
    let selected = since_release.is_some_and(|r| r < CLIPBOARD_AFTER_SELECTING && since_typing.is_none_or(|k| r < k));
    match (selected, typed) {
        (true, _) => CopyAsk::Hold,
        (false, true) => CopyAsk::Now,
        (false, false) => CopyAsk::Ignore,
    }
}

/// The sizes ⌘+ and ⌘− step through.
const ZOOM_STEPS: [f32; 12] = [0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

/// The next larger (`up`) or smaller size after `current`; the ends stay.
pub fn next_zoom(current: f32, up: bool) -> f32 {
    const EPSILON: f32 = 0.005;
    if up {
        ZOOM_STEPS.iter().copied().find(|step| *step > current + EPSILON).unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1])
    } else {
        ZOOM_STEPS.iter().copied().rev().find(|step| *step < current - EPSILON).unwrap_or(ZOOM_STEPS[0])
    }
}

/// Tomorrow at 9:00, local time (epoch ms).
fn tomorrow_morning() -> Option<i64> {
    (chrono::Local::now() + chrono::Duration::days(1))
        .date_naive()
        .and_hms_opt(9, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).single())
        .map(|t| t.timestamp_millis())
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

/// What the diff card draws: the diff without git's file header, at most `limit` lines, and how
/// many lines were left out.
fn preview_lines(diff: &str, limit: usize) -> (Vec<(brain_core::changes::DiffKind, String)>, usize) {
    let mut lines = brain_core::changes::diff_lines(diff);
    lines.retain(|(kind, _)| *kind != brain_core::changes::DiffKind::Meta);
    let cut = lines.len().saturating_sub(limit);
    lines.truncate(limit);
    (lines, cut)
}

/// A file a session changed, in the default text editor (`open -t`, never plain `open`: the path
/// came from a transcript, and a `.command` or `.app` would run).
fn open_in_text_editor(path: &std::path::Path) {
    let _ = std::process::Command::new("open").arg("-t").arg(path).spawn();
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

/// Whether macOS has Brain as the active app (the one that gets the keyboard).
fn app_is_active() -> bool {
    objc2::MainThreadMarker::new().is_none_or(|mtm| objc2_app_kit::NSApplication::sharedApplication(mtm).isActive())
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
    use brain_core::event::{Event, Kind, Source};
    use chrono::TimeZone;

    #[test]
    fn the_diff_card_drops_gits_file_header_and_cuts_a_long_diff() {
        use brain_core::changes::DiffKind;
        let diff = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,3 @@\n one\n+two\n+three\n";

        let (lines, cut) = preview_lines(diff, 3);

        let kinds: Vec<DiffKind> = lines.iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, [DiffKind::Hunk, DiffKind::Context, DiffKind::Added]);
        assert_eq!(cut, 1);
        assert_eq!(preview_lines("", 3), (vec![], 0));
    }

    #[test]
    fn text_claude_copies_after_a_mouse_selection_is_held_and_a_slash_copy_goes_at_once() {
        let secs = |s: f64| Some(Duration::from_secs_f64(s));
        // /copy: typed a moment ago, no mouse release before it.
        assert_eq!(copy_decision(secs(0.3), None), CopyAsk::Now);
        // A selection: the mouse went up a moment ago, typing was long before.
        assert_eq!(copy_decision(secs(30.0), secs(0.1)), CopyAsk::Hold);
        assert_eq!(copy_decision(None, secs(0.1)), CopyAsk::Hold);
        // Typed within the typing window, but the release came after the last key.
        assert_eq!(copy_decision(secs(8.0), secs(0.1)), CopyAsk::Hold);
        // A click before `/copy` was typed: the keys came after the release.
        assert_eq!(copy_decision(secs(0.2), secs(0.9)), CopyAsk::Now);
        // The release is too old to be the selection.
        assert_eq!(copy_decision(secs(1.0), secs(5.0)), CopyAsk::Now);
        // Neither: output carrying a clipboard request is not the user's wish.
        assert_eq!(copy_decision(secs(60.0), secs(60.0)), CopyAsk::Ignore);
        assert_eq!(copy_decision(None, None), CopyAsk::Ignore);
    }

    #[test]
    fn zoom_steps_up_and_down_from_any_size_and_stops_at_the_ends() {
        assert_eq!(next_zoom(1.0, true), 1.1);
        assert_eq!(next_zoom(1.0, false), 0.9);
        assert_eq!(next_zoom(1.05, true), 1.1, "between two steps goes to the next one");
        assert_eq!(next_zoom(1.05, false), 1.0);
        assert_eq!(next_zoom(3.0, true), 3.0);
        assert_eq!(next_zoom(0.5, false), 0.5);
    }

    #[test]
    fn pointer_and_scroll_messages_skip_sync_but_ticks_and_selection_keep_it() {
        assert!(!Message::Hover(None).may_change_synced_state());
        assert!(!Message::HoverHead(None).may_change_synced_state());
        assert!(!Message::Pointer(iced::Point::ORIGIN).may_change_synced_state());
        assert!(!Message::MousePressed.may_change_synced_state());
        assert!(Message::Tick.may_change_synced_state());
        assert!(Message::Close.may_change_synced_state());
    }

    fn finished_session() -> Session {
        let ev = |secs: i64, kind: Kind, text: &str| Event {
            v: 1,
            ts: chrono::Utc.timestamp_opt(1_790_000_000 + secs, 0).unwrap(),
            account: "second".into(),
            pid: 40461,
            session_id: Some("s1".into()),
            cwd: Some("/w/checkout-app".into()),
            source: Source::Hook,
            kind,
            text: Some(text.into()),
            tasks: None,
            name: None,
            ask: None,
        };
        let mut board = brain_core::state::Board::default();
        board.apply_event(&ev(0, Kind::Prompt, "Baue es"));
        board.apply_event(&ev(5, Kind::Stop, "Fertig."));
        let key = SessionKey { account: "second".into(), id: "s1".into() };
        board.get(&key).unwrap().clone()
    }

    #[test]
    fn a_selected_report_stays_in_the_reading_zone_until_the_selection_moves_on() {
        let session = finished_session();
        let mut prefs = Prefs::default();
        let now = 1_790_001_000_000;
        let before = place(&session, None, false, &prefs, now);
        assert_eq!(before, Place::Read);
        // Looking at it makes it seen, but while selected it stays.
        prefs.see(&session.key, now, session.phase_since_ms());
        assert_eq!(place(&session, Some(before), false, &prefs, now), Place::Read);
        assert_eq!(place(&session, None, false, &prefs, now), Place::Quiet, "gone once deselected");
    }

    #[test]
    fn a_seen_session_does_not_jump_into_the_reading_zone_when_selected() {
        let session = finished_session();
        let mut prefs = Prefs::default();
        let now = 1_790_001_000_000;
        prefs.see(&session.key, now, session.phase_since_ms());
        let before = place(&session, None, false, &prefs, now);
        assert_eq!(before, Place::Quiet);
        assert_eq!(place(&session, Some(before), false, &prefs, now), Place::Quiet);
    }

    #[test]
    fn a_turn_that_ends_while_the_session_is_open_does_not_move_it_to_the_reading_zone() {
        let session = finished_session();
        let prefs = Prefs::default();
        let now = 1_790_001_000_000;
        // It was quiet (working) when selected; now its turn has ended and nobody saw it.
        assert_eq!(place(&session, Some(Place::Quiet), false, &prefs, now), Place::Quiet);
    }

    #[test]
    fn a_selected_maybe_question_stays_until_the_selection_moves_on_or_only_a_report_is_chosen() {
        let mut session = finished_session();
        session.insight.asks = true;
        let mut prefs = Prefs::default();
        let now = 1_790_001_000_000;
        let before = place(&session, None, false, &prefs, now);
        assert_eq!(before, Place::Decide(Ask::Maybe));
        prefs.see(&session.key, now, session.phase_since_ms());
        assert_eq!(place(&session, Some(before), false, &prefs, now), Place::Decide(Ask::Maybe));
        assert_eq!(place(&session, None, false, &prefs, now), Place::Quiet);
        prefs.mark_report(&session.key, now);
        assert_eq!(place(&session, Some(before), false, &prefs, now), Place::Quiet);
    }

    #[test]
    fn a_real_question_shows_at_once_also_in_a_selected_session() {
        let session = finished_session();
        let prefs = Prefs::default();
        let now = 1_790_001_000_000;
        assert_eq!(place(&session, Some(Place::Quiet), true, &prefs, now), Place::Decide(Ask::Options));
    }

    #[test]
    fn a_worktree_is_listed_as_its_main_checkout() {
        use brain_core::project::from_rev_parse;
        let linked = from_rev_parse(std::path::Path::new("/w/app/.worktrees/fix"), "/w/app/.git\n/w/app/.worktrees/fix\n");
        assert_eq!(listed_entry(&linked), ("app".to_string(), "/w/app".to_string()));
    }

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
