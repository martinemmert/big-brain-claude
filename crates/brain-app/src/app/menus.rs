//! The macOS menu bar: what each menu lists for the session and state at hand, and what its items
//! do. Every item runs a command Brain has anyway (the palette's, the session menu's); an item
//! that doesn't fit right now is greyed out. The menus are rebuilt on each tick and when the
//! keyboard moves in or out of a terminal, and only differences reach AppKit.

use std::collections::HashMap;

use brain_core::state::{Ask, Phase, Session, SessionKey};
use iced::Task;

use super::{unfocus, Action, Brain, Groups, Message, Mode, PaletteCommand};
use crate::appmenu::{Entry, Item, Menu, Role, CMD, CTRL, OPT, SHIFT};
use crate::format::now_ms;
use crate::i18n::t;
use crate::tr;

/// How long a pause lasts, as the Session menu offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnoozeFor {
    QuarterHour,
    Hour,
    Tomorrow,
    UntilSomethingHappens,
    Off,
}

/// Menu items without a message of their own.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuCommand {
    /// A command of the palette.
    Run(PaletteCommand),
    /// ⌘K.
    Palette,
    /// `~/.claude-brain/config.json` in the text editor.
    Settings,
    /// ⌘O: the session search, with the ended sessions in it.
    ResumeSearch,
    /// ⌥⌘F: the session search.
    SearchSessions,
    /// ⇧⌘U: the next session that needs you.
    NextNeeding,
    /// ⌃⇥ / ⌃⇧⇥: the next or previous session in the tree.
    Step(isize),
    FocusTerminal,
    FocusTree,
    ToggleSidebar,
    /// ⌘+ (`true`) and ⌘−: the next larger or smaller size of the window.
    Zoom(bool),
    /// ⌘0: back to 100 %.
    ZoomReset,
    /// "Ansehen" in the status popup: Brain comes forward with the session.
    OpenSession(SessionKey),
    /// ⌘C / ⌘V in a terminal that has the keyboard.
    Copy,
    Paste,
    PasteScreenshot,
    Snooze(SnoozeFor),
}

/// The ids of the items and what they send.
#[derive(Default)]
struct Commands(HashMap<String, Message>);

impl Commands {
    fn item(&mut self, title: impl Into<String>, id: impl Into<String>, message: Message) -> Item {
        let id = id.into();
        self.0.insert(id.clone(), message);
        Item::new(title, id)
    }

    fn menu(&mut self, command: MenuCommand, title: impl Into<String>, id: &str) -> Item {
        self.item(title, id, Message::Menu(command))
    }

    fn act(&mut self, action: Action, title: impl Into<String>, id: &str) -> Item {
        self.item(title, id, Message::Do(action))
    }

    fn palette(&mut self, command: PaletteCommand, title: impl Into<String>, id: &str) -> Item {
        self.item(title, id, Message::Menu(MenuCommand::Run(command)))
    }
}

fn submenu(title: impl Into<String>, entries: Vec<Entry>) -> Entry {
    Entry::Submenu { title: title.into(), enabled: !entries.is_empty(), entries }
}

impl Brain {
    /// The menus of the bar for the state now.
    fn main_menus(&self, commands: &mut Commands, needing: &[(SessionKey, String)]) -> Vec<Menu> {
        let session = self.selected_session();
        // A dialog on top takes the keys; the menus that act on a session wait for it to close.
        // (The new-session dialog has its own ⌘O and ⇧⌘N: those items stay off, the keys reach it.)
        let acts = matches!(self.mode, Mode::Normal | Mode::Search);
        let on_session = session.filter(|_| acts);
        let in_terminal = self.mode == Mode::Normal && (self.focused_terminal.is_some() || (self.shell_visible && self.focused_shell.is_some()));
        let project = session.map(|s| self.project_name(s));

        vec![
            self.brain_menu(commands),
            self.file_menu(commands, session, project.as_deref(), acts),
            Self::edit_menu(commands, session.is_some() && acts, in_terminal),
            self.session_menu(commands, on_session),
            self.view_menu(commands, session.is_some() && acts),
            Self::window_menu(commands, needing),
            self.help_menu(commands),
        ]
    }

    fn brain_menu(&self, commands: &mut Commands) -> Menu {
        let entries = vec![
            Item::standard(t("Über Brain", "About Brain"), c"orderFrontStandardAboutPanel:").into(),
            Entry::Separator,
            commands.menu(MenuCommand::Settings, t("Einstellungen …", "Settings …"), "settings").keys(",", CMD).into(),
            commands.palette(PaletteCommand::SetUpAccount, t("Neues Claude-Konto einrichten …", "Set up a new Claude account …"), "new-account").into(),
            commands.palette(PaletteCommand::SyncAccounts, t("Konten abgleichen …", "Sync accounts …"), "sync-accounts").into(),
            commands.palette(PaletteCommand::PairPhone, t("iPhone koppeln (Brain Link) …", "Pair a phone (Brain Link) …"), "pair-phone").into(),
            Entry::Separator,
            Item::standard(t("Brain ausblenden", "Hide Brain"), c"hide:").keys("h", CMD).into(),
            Item::standard(t("Andere ausblenden", "Hide Others"), c"hideOtherApplications:").keys("h", CMD | OPT).into(),
            Item::standard(t("Alle einblenden", "Show All"), c"unhideAllApplications:").into(),
            Entry::Separator,
            Item::standard(t("Brain beenden", "Quit Brain"), c"terminate:").keys("q", CMD).into(),
        ];
        Menu { title: "Brain".into(), role: Role::Plain, entries }
    }

    fn file_menu(&self, commands: &mut Commands, session: Option<&Session>, project: Option<&str>, acts: bool) -> Menu {
        let mut entries: Vec<Entry> = vec![
            commands.palette(PaletteCommand::NewSession, t("Neue Session …", "New session …"), "new-session").keys("n", CMD).into(),
        ];
        entries.push(match project {
            Some(name) => commands
                .item(tr!("Neue Session in „{name}“", "New session in “{name}”"), "new-session-in", Message::NewSessionIn(name.to_string()))
                .keys("n", CMD | OPT)
                .into(),
            None => Item::new(t("Neue Session im Projekt", "New session in the project"), "new-session-in").keys("n", CMD | OPT).enabled(false).into(),
        });
        entries.push(commands.item(t("Neues Projekt …", "New project …"), "new-project", Message::NewProject).keys("n", CMD | SHIFT).enabled(acts).into());
        entries.push(commands.palette(PaletteCommand::AdoptProjects, t("Projekte übernehmen …", "Adopt projects …"), "adopt-projects").into());
        entries.push(Entry::Separator);
        entries.push(commands.menu(MenuCommand::ResumeSearch, t("Session fortsetzen …", "Resume a session …"), "resume").keys("o", CMD).enabled(acts).into());
        let takes_over = session.is_some_and(|s| s.agent.is_none() && (s.phase() == Phase::Ended || s.accepts_input()));
        entries.push(
            commands.act(Action::TakeOver, t("Aus iTerm nach Brain holen …", "Move from iTerm into Brain …"), "take-over").keys("o", CMD | OPT).enabled(takes_over && acts).into(),
        );
        entries.push(Entry::Separator);
        entries.push(commands.palette(PaletteCommand::Later, t("Für später …", "For later …"), "later").keys("l", CMD).into());
        entries.push(match project {
            Some(name) => commands
                .item(t("Später-Notizen des Projekts", "The project's notes for later"), "project-notes", Message::ToggleNotes(name.to_string()))
                .keys("l", CMD | SHIFT)
                .checked(self.prefs.are_notes_open(name))
                .into(),
            None => Item::new(t("Später-Notizen des Projekts", "The project's notes for later"), "project-notes").keys("l", CMD | SHIFT).enabled(false).into(),
        });
        entries.push(Entry::Separator);
        entries.push(commands.palette(PaletteCommand::Cleanup, t("Aufräumen …", "Clean up …"), "cleanup").into());
        entries.push(match project {
            Some(name) => {
                let title = if self.prefs.is_project_hidden(name) { tr!("Projekt „{name}“ einblenden", "Show project “{name}”") } else { tr!("Projekt „{name}“ ausblenden", "Hide project “{name}”") };
                commands.item(title, "hide-project", Message::ToggleProjectHidden(name.to_string())).into()
            }
            None => Item::new(t("Projekt ausblenden", "Hide project"), "hide-project").enabled(false).into(),
        });
        entries.push(match project.filter(|name| self.prefs.is_project_listed(name)) {
            Some(name) => commands.item(tr!("Projekt „{name}“ entfernen …", "Remove project “{name}” …"), "remove-project", Message::RemoveProject(name.to_string())).into(),
            None => Item::new(t("Projekt entfernen …", "Remove project …"), "remove-project").enabled(false).into(),
        });
        entries.push(Entry::Separator);
        entries.push(Item::standard(t("Fenster schließen", "Close window"), c"performClose:").keys("w", CMD).into());
        Menu { title: t("Ablage", "File").into(), role: Role::Plain, entries }
    }

    /// Widerrufen, Wiederholen, Ausschneiden and Alles auswählen are the text fields' own: Iced
    /// handles those keys, so the items stay greyed out and let the keys through. Copy and paste
    /// are Brain's only while a terminal has the keyboard.
    fn edit_menu(commands: &mut Commands, session: bool, in_terminal: bool) -> Menu {
        let entries = vec![
            Item::standard(t("Widerrufen", "Undo"), c"undo:").keys("z", CMD).enabled(false).into(),
            Item::standard(t("Wiederholen", "Redo"), c"redo:").keys("z", CMD | SHIFT).enabled(false).into(),
            Entry::Separator,
            Item::standard(t("Ausschneiden", "Cut"), c"cut:").keys("x", CMD).enabled(false).into(),
            commands.menu(MenuCommand::Copy, t("Kopieren", "Copy"), "copy").keys("c", CMD).enabled(in_terminal).into(),
            commands.menu(MenuCommand::Paste, t("Einsetzen", "Paste"), "paste").keys("v", CMD).enabled(in_terminal).into(),
            commands.menu(MenuCommand::PasteScreenshot, t("Bildschirmfoto einsetzen", "Paste a screenshot"), "paste-screenshot").keys("v", CMD | CTRL).enabled(in_terminal).into(),
            Item::standard(t("Alles auswählen", "Select all"), c"selectAll:").keys("a", CMD).enabled(false).into(),
            Entry::Separator,
            commands.palette(PaletteCommand::Find, t("In der Session suchen …", "Find in the session …"), "find").keys("f", CMD).enabled(session).into(),
            commands.menu(MenuCommand::SearchSessions, t("Sessions suchen …", "Search sessions …"), "search").keys("f", CMD | OPT).into(),
        ];
        Menu { title: t("Bearbeiten", "Edit").into(), role: Role::Plain, entries }
    }

    fn session_menu(&self, commands: &mut Commands, session: Option<&Session>) -> Menu {
        let on = session.is_some();
        let ended = session.is_some_and(|s| s.phase() == Phase::Ended);
        let answers = session.is_some_and(|s| s.awaiting_permission() && self.capabilities().keys);
        let mut entries: Vec<Entry> = vec![
            commands.act(Action::Allow, t("Erlauben", "Allow"), "allow").keys("y", CMD).enabled(answers).into(),
            commands.act(Action::Deny, t("Ablehnen", "Deny"), "deny").keys("y", CMD | SHIFT).enabled(answers).into(),
            Entry::Separator,
            commands.menu(MenuCommand::NextNeeding, t("Nächste, die dich braucht", "Next that needs you"), "next-needing").keys("u", CMD | SHIFT).enabled(self.deciding > 0).into(),
            commands.menu(MenuCommand::Step(1), t("Nächste Session", "Next session"), "next").keys("\t", CTRL).into(),
            commands.menu(MenuCommand::Step(-1), t("Vorherige Session", "Previous session"), "previous").keys("\t", CTRL | SHIFT).into(),
            Entry::Separator,
        ];
        let can_rename = session.is_some_and(|s| self.can_send(&s.key));
        entries.push(commands.act(Action::StartRename, t("Umbenennen …", "Rename …"), "rename").keys("r", CMD).enabled(can_rename).into());
        if let Some(s) = session {
            let pin = match (self.prefs.is_pinned(&s.key), ended) {
                (false, true) => t("Zum Fortsetzen merken", "Save to resume"),
                (true, true) => t("Nicht mehr merken", "Don't save"),
                (false, false) => t("Anheften", "Pin"),
                (true, false) => t("Lösen", "Unpin"),
            };
            entries.push(commands.act(Action::Pin, pin, "pin").keys("p", CMD | OPT).checked(self.prefs.is_pinned(&s.key)).into());
            let muted = self.prefs.is_muted(&s.key);
            entries.push(commands.act(Action::Mute, if muted { t("Benachrichtigungen an", "Unmute") } else { t("Stummschalten", "Mute") }, "mute").keys("m", CMD | OPT).checked(muted).into());
        } else {
            entries.push(Item::new(t("Anheften", "Pin"), "pin").keys("p", CMD | OPT).enabled(false).into());
            entries.push(Item::new(t("Stummschalten", "Mute"), "mute").keys("m", CMD | OPT).enabled(false).into());
        }
        entries.push(self.snooze_menu(commands, session));
        entries.push(Entry::Separator);

        let moves: Vec<Entry> = match session {
            Some(_) => self
                .move_targets()
                .into_iter()
                .map(|index| commands.act(Action::MoveTo(index), self.model.accounts[index].id.clone(), &format!("move:{index}")).into())
                .collect(),
            None => Vec::new(),
        };
        entries.push(submenu(t("In anderes Konto", "Into another account"), moves));
        let forks: Vec<Entry> = match session {
            Some(_) => self
                .fork_targets()
                .into_iter()
                .map(|index| commands.act(Action::ForkTo(index), self.model.accounts[index].id.clone(), &format!("fork:{index}")).into())
                .collect(),
            None => Vec::new(),
        };
        entries.push(submenu(t("Forken in", "Fork into"), forks));
        entries.push(self.project_menu_entries(commands, session));
        entries.push(commands.act(Action::Reload, t("Neu laden (gleiches Gespräch)", "Reload (same conversation)"), "reload").keys("r", CMD | SHIFT).enabled(on).into());
        entries.push(commands.act(Action::RestartWith, t("Neu starten mit … (z. B. --chrome)", "Restart with … (e.g. --chrome)"), "restart").enabled(on).into());
        entries.push(commands.act(Action::OpenInITerm, t("In iTerm öffnen", "Open in iTerm"), "iterm").keys("i", CMD | OPT).enabled(on).into());
        entries.push(commands.palette(PaletteCommand::Composer, t("Composer …", "Composer …"), "composer").keys("e", CMD).enabled(on && self.mode == Mode::Normal).into());
        entries.push(Entry::Separator);

        let can_end = session.is_some_and(|s| s.agent.as_ref().is_some_and(|a| a.is_active()) || (s.phase() != Phase::Ended && s.accepts_input()));
        entries.push(commands.act(Action::End, t("Session beenden", "End session"), "end").keys("x", CMD | OPT).enabled(can_end).into());
        let can_force = session.is_some_and(|s| s.phase() != Phase::Ended && s.alive);
        entries.push(commands.act(Action::ForceEnd, t("Hart beenden (Prozess stoppen)", "End by force (stop the process)"), "force-end").enabled(can_force).into());
        let can_trash = session.is_some_and(|s| s.phase() == Phase::Ended || s.agent.as_ref().is_some_and(|a| !a.is_active()));
        entries.push(commands.act(Action::Trash, t("In den Papierkorb", "Move to Trash"), "trash").keys("\u{8}", CMD | OPT).enabled(can_trash).into());
        Menu { title: "Session".into(), role: Role::Plain, entries }
    }

    fn snooze_menu(&self, commands: &mut Commands, session: Option<&Session>) -> Entry {
        let Some(s) = session.filter(|s| s.phase() != Phase::Ended) else { return submenu(t("Pausieren", "Snooze"), Vec::new()) };
        let paused = self.prefs.snoozed_until(&s.key, now_ms()).is_some() || self.prefs.is_hidden(&s.key, s.last_activity_ms);
        let mut entries: Vec<Entry> = [
            (SnoozeFor::QuarterHour, t("15 Minuten", "15 minutes"), "snooze:15"),
            (SnoozeFor::Hour, t("1 Stunde", "1 hour"), "snooze:60"),
            (SnoozeFor::Tomorrow, t("Bis morgen 9:00", "Until tomorrow 9:00"), "snooze:tomorrow"),
            (SnoozeFor::UntilSomethingHappens, t("Bis sich etwas tut", "Until something happens"), "snooze:activity"),
        ]
        .into_iter()
        .map(|(length, title, id)| commands.menu(MenuCommand::Snooze(length), title, id).into())
        .collect();
        if paused {
            entries.push(Entry::Separator);
            entries.push(commands.menu(MenuCommand::Snooze(SnoozeFor::Off), t("Pause aufheben", "End the pause"), "snooze:off").into());
        }
        submenu(t("Pausieren", "Snooze"), entries)
    }

    /// "In anderes Projekt": the other projects, back into its own, and the palette for a new one.
    fn project_menu_entries(&self, commands: &mut Commands, session: Option<&Session>) -> Entry {
        let Some(s) = session else { return submenu(t("In anderes Projekt", "Into another project"), Vec::new()) };
        let current = self.project_name(s);
        let mut entries: Vec<Entry> = self
            .project_names()
            .into_iter()
            .filter(|p| *p != current)
            .map(|p| commands.palette(PaletteCommand::Project(Some(p.clone())), p.clone(), &format!("project:{p}")).into())
            .collect();
        if self.prefs.project(&s.key).is_some() {
            let own = self.own_project_name(s);
            entries.push(Entry::Separator);
            entries.push(commands.palette(PaletteCommand::Project(None), tr!("Zurück in „{own}“", "Back into “{own}”"), "project:own").into());
        }
        entries.push(Entry::Separator);
        entries.push(commands.act(Action::MoveToProject, t("Anderes Projekt …", "Another project …"), "project:other").into());
        submenu(t("In anderes Projekt", "Into another project"), entries)
    }

    fn view_menu(&self, commands: &mut Commands, session: bool) -> Menu {
        let entries = vec![
            commands.item(t("Befehlspalette …", "Command palette …"), "palette", Message::Menu(MenuCommand::Palette)).keys("k", CMD).into(),
            Entry::Separator,
            commands.menu(MenuCommand::ToggleSidebar, t("Seitenleiste", "Sidebar"), "sidebar").keys("s", CMD | CTRL).checked(!self.prefs.hide_sidebar).into(),
            commands.item(t("Änderungen zeigen", "Show the changes"), "changes", Message::ToggleChanges).keys("0", CMD | OPT).enabled(self.selected_changes().is_some()).into(),
            commands.palette(PaletteCommand::Split, t("Terminal teilen", "Split the terminal"), "split").keys("d", CMD).enabled(session).into(),
            commands.palette(PaletteCommand::Overview, t("Alle Terminals im Überblick", "All terminals at a glance"), "overview").keys("a", CMD | SHIFT).into(),
            commands.palette(PaletteCommand::Shell, t("Shell im Projektordner", "Shell in the project folder"), "shell").keys("t", CMD).enabled(session && self.mode == Mode::Normal).into(),
            Entry::Separator,
            commands.menu(MenuCommand::FocusTerminal, t("Terminal fokussieren", "Focus the terminal"), "focus-terminal").keys("j", CMD).into(),
            commands.menu(MenuCommand::FocusTree, t("Baum fokussieren", "Focus the tree"), "focus-tree").keys("[", CMD).into(),
            Entry::Separator,
            commands.menu(MenuCommand::Zoom(true), t("Größer", "Zoom in"), "zoom-in").keys("+", CMD).into(),
            commands.menu(MenuCommand::Zoom(false), t("Kleiner", "Zoom out"), "zoom-out").keys("-", CMD).into(),
            commands.menu(MenuCommand::ZoomReset, self.actual_size_title(), "zoom-reset").keys("0", CMD).enabled(self.zoom() != 1.0).into(),
            Entry::Separator,
            commands.item(t("Beendete Sessions zeigen", "Show ended sessions"), "show-older", Message::ToggleOlder).checked(self.prefs.show_older).into(),
            commands.palette(PaletteCommand::ToggleBackground, t("Ruhige Hintergrund-Sessions zeigen", "Show quiet background sessions"), "show-background").checked(self.prefs.show_background).into(),
            commands.palette(PaletteCommand::ToggleAutomated, t("Automatisch gestartete Sessions zeigen", "Show sessions started by programs"), "show-automated").checked(self.prefs.show_automated).into(),
        ];
        // macOS adds "Enter Full Screen" to the end of the View menu by itself.
        Menu { title: t("Darstellung", "View").into(), role: Role::Plain, entries }
    }

    fn window_menu(commands: &mut Commands, needing: &[(SessionKey, String)]) -> Menu {
        let mut entries: Vec<Entry> = vec![
            Item::standard(t("Im Dock ablegen", "Minimize"), c"performMiniaturize:").keys("m", CMD).into(),
            Item::standard(t("Zoomen", "Zoom"), c"performZoom:").into(),
            Entry::Separator,
        ];
        let keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
        for (key, (session_key, title)) in keys.into_iter().zip(needing) {
            entries.push(commands.item(title.clone(), format!("window:{key}"), Message::Select(session_key.clone())).keys(key, CMD).into());
        }
        if !needing.is_empty() {
            entries.push(Entry::Separator);
        }
        entries.push(Item::standard(t("Alle nach vorne bringen", "Bring all to front"), c"arrangeInFront:").into());
        Menu { title: t("Fenster", "Window").into(), role: Role::Windows, entries }
    }

    fn help_menu(&self, commands: &mut Commands) -> Menu {
        let entries = vec![
            commands.palette(PaletteCommand::Keys, t("Alle Tasten …", "All keys …"), "keys").keys("?", CMD).into(),
            commands.palette(PaletteCommand::OpenLog, t("Log öffnen", "Open the log"), "log").into(),
            commands.palette(PaletteCommand::Garden, t("Gärtner: kaputte Sessions finden …", "Gardener: find broken sessions …"), "garden").into(),
        ];
        Menu { title: t("Hilfe", "Help").into(), role: Role::Help, entries }
    }

    /// The sessions that need the user, most urgent first, with the title the menus show.
    fn needing_you(&self, groups: &Groups<'_>) -> Vec<(SessionKey, String)> {
        groups
            .decide
            .iter()
            .filter(|(_, ask)| *ask != Ask::Maybe)
            .map(|(s, _)| (s.key.clone(), format!("{} · {}", s.display_name(), self.project_name(s))))
            .collect()
    }

    /// Builds the menu bar again, and hands AppKit what changed. The
    /// list is the whole board: neither the search nor the account filter hides a decision.
    pub(super) fn refresh_menus(&mut self) {
        let now = now_ms();
        let (menus, commands) = {
            let groups = self.groups_for(now, "", None);
            let needing = self.needing_you(&groups);
            let mut commands = Commands::default();
            let menus = self.main_menus(&mut commands, &needing);
            (menus, commands.0)
        };
        self.menu_commands = commands;
        if let Some(bar) = self.appmenu.as_mut() {
            bar.show(&menus);
        }
        if let (Some(menubar), Some(bar)) = (self.menubar.as_mut(), self.appmenu.as_ref()) {
            menubar.connect(bar);
        }
    }

    /// An item of the bar was chosen: sends what it stands for.
    pub(super) fn menu_picked(&mut self, id: &str) -> Task<Message> {
        match id {
            crate::appmenu::STATUS_ITEM_CLICKED => return self.open_status_popup_in_front(),
            _ => {}
        }
        match self.menu_commands.get(id).cloned() {
            Some(message) => self.update(message),
            None => Task::none(),
        }
    }

    pub(super) fn run_menu(&mut self, command: MenuCommand) -> Task<Message> {
        match command {
            MenuCommand::Run(command) => self.run_palette(command),
            MenuCommand::Palette => self.open_palette(),
            MenuCommand::Settings => {
                self.open_settings();
                Task::none()
            }
            MenuCommand::ResumeSearch => {
                self.resume_search = true;
                self.start_search()
            }
            MenuCommand::SearchSessions => {
                self.resume_search = false;
                self.start_search()
            }
            MenuCommand::NextNeeding => self.select_next_needing(),
            MenuCommand::Step(by) => self.step_session(by),
            MenuCommand::FocusTerminal => {
                if self.has_terminal() {
                    self.focus_terminal()
                } else {
                    Task::none()
                }
            }
            MenuCommand::FocusTree => {
                self.focused_terminal = None;
                self.focused_shell = None;
                if self.mode == Mode::Search {
                    self.mode = Mode::Normal;
                }
                unfocus()
            }
            // While the Quick Look sheet is open, the shortcut folds its list of files.
            MenuCommand::ToggleSidebar if self.preview.is_some() && self.mode == Mode::Normal && !self.overview => self.toggle_sheet_files(),
            MenuCommand::ToggleSidebar => {
                self.prefs.hide_sidebar = !self.prefs.hide_sidebar;
                self.prefs.save();
                Task::none()
            }
            MenuCommand::OpenSession(key) => {
                self.selected = Some(key);
                self.mode = Mode::Normal;
                self.focus_on_select = true;
                Task::batch([self.bring_forward(), self.scroll_to_selected()])
            }
            MenuCommand::Zoom(up) => {
                self.set_zoom(super::next_zoom(self.zoom(), up));
                Task::none()
            }
            MenuCommand::ZoomReset => {
                self.set_zoom(1.0);
                Task::none()
            }
            MenuCommand::Copy => self.copy_from_terminal(),
            MenuCommand::Paste => self.paste_into_terminal(),
            MenuCommand::PasteScreenshot => {
                self.paste_screenshot();
                Task::none()
            }
            MenuCommand::Snooze(length) => {
                self.snooze_selected(length);
                Task::none()
            }
        }
    }

    /// "Originalgröße", with the current level when it isn't 100 %.
    fn actual_size_title(&self) -> String {
        let title = t("Originalgröße", "Actual size");
        match (self.zoom() * 100.0).round() as u32 {
            100 => title.to_string(),
            percent => format!("{title} ({percent} %)"),
        }
    }

    fn set_zoom(&mut self, zoom: f32) {
        let before = self.zoom();
        self.prefs.zoom = (zoom != 1.0).then_some(zoom);
        self.window_size = super::quick_look::rescaled(self.window_size, before, self.zoom());
        self.prefs.save();
        let percent = (zoom * 100.0).round() as u32;
        self.set_status(tr!("Darstellung auf {percent} %.", "Zoom {percent} %."));
    }

    /// `~/.claude-brain/config.json` in the text editor, created empty if it isn't there yet.
    fn open_settings(&mut self) {
        let path = brain_core::account::home_dir().join(".claude-brain/config.json");
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, "{\n}\n");
        }
        match std::process::Command::new("open").arg("-t").arg(&path).spawn() {
            Ok(_) => self.set_status(t("Die Einstellungen stehen im Texteditor.", "The settings are open in the text editor.")),
            Err(err) => self.set_status(tr!("Konnte den Texteditor nicht starten: {err}", "Couldn't start the text editor: {err}")),
        }
    }

    /// ⇧⌘U: the session after the selected one in the order of who needs the user; the first when
    /// none of them is selected.
    fn select_next_needing(&mut self) -> Task<Message> {
        let keys: Vec<SessionKey> = self.needing_you(&self.groups_for(now_ms(), "", None)).into_iter().map(|(key, _)| key).collect();
        let at = self.selected.as_ref().and_then(|k| keys.iter().position(|n| n == k));
        match keys.get(at.map_or(0, |i| (i + 1) % keys.len())) {
            Some(next) => {
                self.selected = Some(next.clone());
                self.focus_on_select = true;
                self.scroll_to_selected()
            }
            None => {
                self.set_status(t("Niemand braucht dich gerade.", "Nobody needs you right now."));
                Task::none()
            }
        }
    }

    /// ⌃⇥ / ⌃⇧⇥: the next or previous session in the tree's order, around at the ends.
    fn step_session(&mut self, by: isize) -> Task<Message> {
        let list = self.nav_keys();
        if list.is_empty() {
            return Task::none();
        }
        let at = self.selected.as_ref().and_then(|k| list.iter().position(|l| l == k));
        let next = match at {
            Some(i) => (i as isize + by).rem_euclid(list.len() as isize) as usize,
            None => 0,
        };
        self.selected = Some(list[next].clone());
        self.scroll_to_selected()
    }

    /// ⌘C in a terminal: the text Claude Code copied after a selection, if Brain holds it, else
    /// Brain's own selection.
    fn copy_from_terminal(&mut self) -> Task<Message> {
        let Some(key) = self.focused_terminal.clone().or_else(|| self.focused_shell.clone()) else { return Task::none() };
        if let Some((_, text)) = self.held_copy.take().filter(|(held_in, _)| *held_in == key) {
            let chars = text.chars().count();
            self.set_status(tr!("{chars} Zeichen kopiert.", "Copied {chars} characters."));
            return iced::clipboard::write(text);
        }
        let from_shell = self.focused_terminal.is_none();
        let text = if from_shell { self.shells.get(&key) } else { self.terminals.get(&key) }.map(|term| term.selection_text()).unwrap_or_default();
        if text.is_empty() {
            return Task::none();
        }
        iced::clipboard::write(text)
    }

    /// ⌘V in a terminal: a copied screenshot goes in as its path, text as a paste.
    fn paste_into_terminal(&mut self) -> Task<Message> {
        if self.paste_screenshot() {
            return Task::none();
        }
        iced::clipboard::read().map(Message::PasteText)
    }

    /// The picture on the clipboard, saved, as a path in the terminal. Returns whether there was one.
    fn paste_screenshot(&mut self) -> bool {
        match crate::clipboard::save_image() {
            Some(path) => {
                let escaped = super::escape_path(&path.display().to_string()) + " ";
                self.write_to_terminal(&escaped);
                true
            }
            None => false,
        }
    }

    /// Text from the clipboard into the terminal that has the keyboard.
    pub(super) fn paste_text(&mut self, text: Option<String>) {
        let Some(text) = text.filter(|t| !t.is_empty()) else { return };
        let key = self.focused_terminal.clone().or_else(|| self.focused_shell.clone());
        let from_shell = self.focused_terminal.is_none();
        let term = key.clone().and_then(|k| if from_shell { self.shells.get_mut(&k) } else { self.terminals.get_mut(&k) });
        if let Some(term) = term {
            term.paste(&text);
            if let (Some(key), false) = (key, from_shell) {
                self.note_pasted(&key, &text);
            }
        }
    }

    fn snooze_selected(&mut self, length: SnoozeFor) {
        let Some(session) = self.selected_session() else { return };
        let (key, last_activity) = (session.key.clone(), session.last_activity_ms);
        let now = now_ms();
        let until = match length {
            SnoozeFor::QuarterHour => Some(now + 15 * 60 * 1000),
            SnoozeFor::Hour => Some(now + 60 * 60 * 1000),
            SnoozeFor::Tomorrow => super::tomorrow_morning(),
            SnoozeFor::UntilSomethingHappens => {
                self.prefs.snooze(&key, None);
                self.prefs.hide(&key, now);
                self.prefs.save();
                self.set_status(t("Pausiert, bis sich in der Session etwas tut.", "Snoozed until something happens in the session."));
                return;
            }
            SnoozeFor::Off => {
                if self.prefs.is_hidden(&key, last_activity) {
                    self.prefs.unhide(&key);
                }
                None
            }
        };
        self.prefs.snooze(&key, until);
        self.prefs.save();
        self.set_status(match until {
            Some(until) => {
                let at = crate::format::clock(until);
                tr!("Pausiert bis {at}.", "Snoozed until {at}.")
            }
            None => t("Nicht mehr pausiert.", "No longer snoozed.").to_string(),
        });
    }
}
