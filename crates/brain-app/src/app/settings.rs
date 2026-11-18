//! The settings sheet (⌘,): General (quick-terminal key, notifications), Terminal (font and
//! size), Accounts (colour and Chrome profile) and Templates. Every change is saved at once into
//! `config.json` and takes effect while Brain runs; templates are saved with "Save" as files in
//! `~/.claude-brain/templates/`.

use std::path::PathBuf;

use brain_core::templates::{self, Template};
use iced::widget::{operation, text_editor};
use iced::Task;
use serde_json::Value;

use super::{Brain, Message, Mode};
use crate::i18n::t;
use crate::tr;

/// The line height of session terminals and of shells, as their font is set up.
pub const SESSION_LINE: f32 = 1.4;
pub const SHELL_LINE: f32 = 1.3;
/// The font sizes the sheet offers.
pub const FONT_SIZES: std::ops::RangeInclusive<u32> = 9..=24;
/// The name field of the template editor.
pub const TEMPLATE_NAME: &str = "settings-template-name";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    General,
    Terminal,
    Accounts,
    Templates,
}

/// A template being edited: a new one has an empty path.
pub struct Draft {
    pub path: PathBuf,
    pub name: String,
    pub folder: String,
    pub account: Option<String>,
    pub model: Option<String>,
    pub prompt: text_editor::Content,
    /// "Delete …" asked: the row shows Keep / Delete.
    pub confirm_delete: bool,
}

impl Draft {
    fn of(template: &Template) -> Self {
        Draft {
            path: template.path.clone(),
            name: template.name.clone(),
            folder: template.folder.clone().unwrap_or_default(),
            account: template.account.clone(),
            model: template.model.clone(),
            prompt: text_editor::Content::with_text(&template.prompt),
            confirm_delete: false,
        }
    }

    pub fn is_new(&self) -> bool {
        self.path.as_os_str().is_empty()
    }

    /// The placeholders the prompt asks for at start.
    pub fn placeholders(&self) -> Vec<String> {
        self.template().placeholders()
    }

    fn template(&self) -> Template {
        let some = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        Template { path: self.path.clone(), name: self.name.trim().to_string(), folder: some(&self.folder), account: self.account.clone(), model: self.model.clone(), prompt: self.prompt.text() }
    }
}

/// What the sheet shows; the settings themselves are read from and written to the config.
#[derive(Default)]
pub struct SettingsSheet {
    pub pane: Pane,
    /// The key field listens: the next combination becomes the key.
    pub recording: bool,
    pub hotkey: Option<String>,
    /// macOS refused the key, or it collides with a known one.
    pub hotkey_note: Option<String>,
    pub notifications: bool,
    pub font: Option<String>,
    pub font_size: Option<f32>,
    /// The installed monospace families, read when the sheet opens.
    pub families: Vec<String>,
    /// The font without settings (iTerm's, else Menlo 13).
    pub default_font: (String, f32),
    pub templates: Vec<Template>,
    pub draft: Option<Draft>,
}

/// What the sheet asks for.
#[derive(Debug, Clone)]
pub enum SettingsMsg {
    Pane(Pane),
    Record,
    HotkeyOff,
    Notifications(bool),
    /// A family, or `None` for the default.
    Font(Option<String>),
    FontSize(f32),
    FontReset,
    AccountColor(String, usize),
    ChromeProfile(String, Option<String>),
    NewTemplate,
    EditTemplate(usize),
    DraftName(String),
    DraftFolder(String),
    PickFolder,
    DraftAccount(Option<String>),
    DraftModel(Option<String>),
    DraftPrompt(text_editor::Action),
    SaveTemplate,
    CancelTemplate,
    AskDelete,
    KeepTemplate,
    DeleteTemplate,
    ShowTemplates,
    OpenConfig,
}

fn templates_dir() -> PathBuf {
    templates::templates_dir(&brain_core::account::home_dir())
}

impl Brain {
    /// ⌘,: opens the sheet with what the config holds now.
    pub(super) fn open_settings(&mut self) -> Task<Message> {
        self.chrome_profiles = crate::browser::chrome_profiles();
        self.settings = SettingsSheet {
            pane: self.settings.pane,
            hotkey: crate::config::quick_terminal(),
            notifications: crate::config::notifications(),
            font: crate::config::chat_font(),
            font_size: crate::config::chat_font_size(),
            families: crate::chat_font::monospace_families(),
            default_font: crate::chat_font::default_font(),
            templates: templates::load_all(&templates_dir()),
            ..SettingsSheet::default()
        };
        self.settings.hotkey_note = self.settings.hotkey.as_deref().and_then(crate::hotkey::warning).map(str::to_string);
        self.mode = Mode::Settings;
        self.focused_terminal = None;
        Task::none()
    }

    /// Writes one setting; a failure says so and changes nothing else.
    fn save_setting(&mut self, result: std::io::Result<()>) -> bool {
        if let Err(err) = result {
            self.set_status(tr!("Konnte die Einstellung nicht speichern: {err}", "Couldn't save the setting: {err}"));
            return false;
        }
        true
    }

    pub(super) fn update_settings(&mut self, msg: SettingsMsg) -> Task<Message> {
        // Demo screenshots show the sheet; nothing of it is written.
        let writes = matches!(
            msg,
            SettingsMsg::HotkeyOff
                | SettingsMsg::Notifications(_)
                | SettingsMsg::Font(_)
                | SettingsMsg::FontSize(_)
                | SettingsMsg::FontReset
                | SettingsMsg::AccountColor(..)
                | SettingsMsg::ChromeProfile(..)
                | SettingsMsg::SaveTemplate
                | SettingsMsg::DeleteTemplate
        );
        if writes && self.blocked_in_demo() {
            return Task::none();
        }
        match msg {
            SettingsMsg::Pane(pane) => {
                self.settings.pane = pane;
                self.settings.recording = false;
            }
            SettingsMsg::Record => self.settings.recording = !self.settings.recording,
            SettingsMsg::HotkeyOff => self.set_hotkey(None),
            SettingsMsg::Notifications(on) => {
                if self.save_setting(crate::config::set("notifications", Some(Value::Bool(on)))) {
                    self.settings.notifications = on;
                }
            }
            SettingsMsg::Font(family) => {
                let value = family.clone().map(Value::String);
                if self.save_setting(crate::config::set("chat_font", value)) {
                    self.settings.font = family;
                    self.apply_font();
                }
            }
            SettingsMsg::FontSize(size) => {
                let size = size.clamp(*FONT_SIZES.start() as f32, *FONT_SIZES.end() as f32);
                if self.save_setting(crate::config::set("chat_font_size", Some(Value::from(size as f64)))) {
                    self.settings.font_size = Some(size);
                    self.apply_font();
                }
            }
            SettingsMsg::FontReset => {
                let saved = crate::config::set("chat_font", None).and_then(|_| crate::config::set("chat_font_size", None));
                if self.save_setting(saved) {
                    self.settings.font = None;
                    self.settings.font_size = None;
                    self.apply_font();
                }
            }
            SettingsMsg::AccountColor(account, index) => {
                if self.save_setting(crate::config::set_in(&crate::config::path(), "account_colors", &account, Value::from(index as u64))) {
                    self.account_colors.insert(account, index);
                }
            }
            SettingsMsg::ChromeProfile(account, dir) => {
                if self.save_setting(crate::browser::set_profile(&crate::config::path(), &account, dir.as_deref())) {
                    self.refresh_menus();
                }
            }
            SettingsMsg::NewTemplate => {
                self.settings.draft = Some(Draft::of(&Template { path: PathBuf::new(), name: String::new(), folder: None, account: None, model: None, prompt: String::new() }));
                return operation::focus(TEMPLATE_NAME);
            }
            SettingsMsg::EditTemplate(index) => {
                if let Some(template) = self.settings.templates.get(index) {
                    self.settings.draft = Some(Draft::of(template));
                }
            }
            SettingsMsg::DraftName(name) => self.with_draft(|d| d.name = name),
            SettingsMsg::DraftFolder(folder) => self.with_draft(|d| d.folder = folder),
            SettingsMsg::PickFolder => {
                if let Some(path) = crate::folder_picker::choose_folder() {
                    let home = brain_core::account::home_dir();
                    let shown = path.strip_prefix(&home).map(|rest| format!("~/{}", rest.display())).unwrap_or_else(|_| path.display().to_string());
                    self.with_draft(|d| d.folder = shown);
                }
            }
            SettingsMsg::DraftAccount(account) => self.with_draft(|d| d.account = account),
            SettingsMsg::DraftModel(model) => self.with_draft(|d| d.model = model),
            SettingsMsg::DraftPrompt(action) => self.with_draft(|d| d.prompt.perform(action)),
            SettingsMsg::SaveTemplate => self.save_template(),
            SettingsMsg::CancelTemplate => self.settings.draft = None,
            SettingsMsg::AskDelete => self.with_draft(|d| d.confirm_delete = true),
            SettingsMsg::KeepTemplate => self.with_draft(|d| d.confirm_delete = false),
            SettingsMsg::DeleteTemplate => self.delete_template(),
            SettingsMsg::ShowTemplates => {
                let dir = templates_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("open").arg(&dir).spawn();
            }
            SettingsMsg::OpenConfig => self.open_config_file(),
        }
        Task::none()
    }

    fn with_draft(&mut self, change: impl FnOnce(&mut Draft)) {
        if let Some(draft) = self.settings.draft.as_mut() {
            change(draft);
        }
    }

    /// A key press while the key field listens: esc stops, ⌫ turns the key off, a combination
    /// with a modifier becomes the key. True when the press was the field's.
    pub(super) fn record_hotkey(&mut self, key: &iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> bool {
        use iced::keyboard::{key::Named, Key};
        if !self.settings.recording {
            return false;
        }
        match key {
            Key::Named(Named::Escape) => self.settings.recording = false,
            Key::Named(Named::Backspace | Named::Delete) => self.set_hotkey(None),
            _ => match crate::hotkey::spec_of(key, modifiers) {
                Some(spec) => self.set_hotkey(Some(spec)),
                // A bare key or one the hotkey can't take: keep listening.
                None => {}
            },
        }
        true
    }

    fn set_hotkey(&mut self, spec: Option<String>) {
        self.settings.recording = false;
        if self.blocked_in_demo() {
            return;
        }
        let value = Some(Value::String(spec.clone().unwrap_or_else(|| "off".into())));
        if !self.save_setting(crate::config::set("quick_terminal", value)) {
            return;
        }
        let taken = !crate::hotkey::set(spec.as_deref());
        self.settings.hotkey_note = match (&spec, taken) {
            (Some(spec), true) => {
                let keys = crate::hotkey::symbols(spec);
                Some(tr!("{keys} ist vergeben – eine andere App hat die Kombination.", "{keys} is taken – another app owns the combination."))
            }
            (Some(spec), false) => crate::hotkey::warning(spec).map(str::to_string),
            (None, _) => None,
        };
        self.settings.hotkey = spec;
    }

    /// The font from now on: new terminals and the history take it, open terminals change.
    fn apply_font(&mut self) {
        crate::chat_font::apply(self.settings.font.clone(), self.settings.font_size);
        let font = crate::chat_font::get();
        let settings = |line: f32| iced_term::settings::FontSettings { size: font.size, font_type: font.regular, scale_factor: line };
        for term in self.terminals.values_mut() {
            term.handle(iced_term::Command::ChangeFont(settings(SESSION_LINE)));
        }
        for term in self.shells.values_mut() {
            term.handle(iced_term::Command::ChangeFont(settings(SHELL_LINE)));
        }
    }

    fn save_template(&mut self) {
        let Some(draft) = self.settings.draft.as_ref() else { return };
        let template = draft.template();
        if template.name.is_empty() {
            self.set_status(t("Die Vorlage braucht einen Namen.", "The template needs a name."));
            return;
        }
        match templates::save(&templates_dir(), &template) {
            Ok(_) => {
                self.settings.templates = templates::load_all(&templates_dir());
                self.settings.draft = None;
                let name = template.name;
                self.set_status(tr!("Vorlage „{name}“ gesichert.", "Template “{name}” saved."));
            }
            Err(err) => self.set_status(tr!("Konnte die Vorlage nicht sichern: {err}", "Couldn't save the template: {err}")),
        }
    }

    /// Moves the template's file to the Trash (it can be put back from there).
    fn delete_template(&mut self) {
        let Some(draft) = self.settings.draft.take() else { return };
        if draft.is_new() {
            return;
        }
        match crate::trash::move_to_trash(&draft.path) {
            Ok(_) => {
                self.settings.templates = templates::load_all(&templates_dir());
                let name = draft.name;
                self.set_status(tr!("Vorlage „{name}“ liegt im Papierkorb.", "Template “{name}” is in the Trash."));
            }
            Err(err) => {
                self.set_status(tr!("Konnte die Vorlage nicht löschen: {err}", "Couldn't delete the template: {err}"));
                self.settings.draft = Some(draft);
            }
        }
    }

    /// "config.json öffnen": the file in the text editor, for everything the sheet doesn't show.
    fn open_config_file(&mut self) {
        let path = crate::config::path();
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, "{\n}\n");
        }
        if let Err(err) = std::process::Command::new("open").arg("-t").arg(&path).spawn() {
            self.set_status(tr!("Konnte den Texteditor nicht starten: {err}", "Couldn't start the text editor: {err}"));
        }
    }

    /// The colour of an account: the one it picked, else that of its place in the list.
    pub fn account_index(&self, account: &str) -> usize {
        self.account_colors.get(account).copied().unwrap_or_else(|| self.model.accounts.iter().position(|a| a.id == account).unwrap_or(0))
    }
}
