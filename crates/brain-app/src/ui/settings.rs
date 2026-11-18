//! The settings sheet (⌘,): the areas on the left as in the macOS settings, their content on the
//! right, "config.json öffnen" and "Fertig" at the bottom. Every change is saved at once.

use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_editor, text_input, toggler, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Padding};

use super::{dialog_title, field_style, scrollbar};
use crate::app::settings::{Draft, Pane, FONT_SIZES};
use crate::app::{Brain, Message, SettingsMsg, TEMPLATE_NAME};
use crate::i18n::t;
use crate::style::{self, action, alpha, boxed, primary_action, INK, ISLAND_SELECTED, ISLAND_HOVER, LINE, LINE_STRONG, MENU_BG, MENU_LINE, PRIMARY, RAISED, TEXT, TEXT_FAINT, TEXT_MUTED, TEXT_STRONG, TURN, UI, WORKING};
use crate::tr;

fn msg(m: SettingsMsg) -> Message {
    Message::Settings(m)
}

pub fn view(brain: &Brain) -> Element<'_, Message> {
    let sheet = &brain.settings;
    let nav = column(
        [(Pane::General, t("Allgemein", "General")), (Pane::Terminal, t("Terminal", "Terminal")), (Pane::Accounts, t("Konten", "Accounts")), (Pane::Templates, t("Vorlagen", "Templates"))]
            .into_iter()
            .map(|(pane, label)| nav_item(label, sheet.pane == pane, msg(SettingsMsg::Pane(pane)))),
    )
    .spacing(2)
    .width(170);
    let content = match sheet.pane {
        Pane::General => general(brain),
        Pane::Terminal => terminal(brain),
        Pane::Accounts => accounts(brain),
        Pane::Templates => templates(brain),
    };
    let content = scrollable(container(content).max_width(640).padding(Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 0.0 })).height(Fill).style(scrollbar);
    let footer = row![
        action(t("config.json öffnen", "Open config.json"), "", false, Some(msg(SettingsMsg::OpenConfig))),
        text(t("für alles Übrige", "for everything else")).size(12).color(TEXT_FAINT).font(UI),
        Space::new().width(Fill),
        primary_action(t("Fertig", "Done"), "esc", Some(Message::Close)),
    ]
    .spacing(10)
    .align_y(iced::Center);
    column![
        dialog_title(t("Einstellungen", "Settings")),
        row![nav, container(Space::new().width(1).height(Fill)).style(|_| style::fill(LINE)), content].spacing(24).height(Fill),
        footer,
    ]
    .spacing(18)
    .padding(Padding { top: 24.0, right: 28.0, bottom: 20.0, left: 28.0 })
    .into()
}

fn nav_item<'a>(label: &'a str, active: bool, on_press: Message) -> Element<'a, Message> {
    button(text(label).size(13).color(if active { TEXT_STRONG } else { TEXT }).font(if active { style::semibold() } else { UI }))
        .width(Fill)
        .padding([7, 12])
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: match (active, status) {
                (true, _) => Some(Background::Color(ISLAND_SELECTED)),
                (false, button::Status::Hovered | button::Status::Pressed) => Some(Background::Color(ISLAND_HOVER)),
                _ => None,
            },
            text_color: TEXT,
            border: Border { radius: 7.0.into(), ..Border::default() },
            ..button::Style::default()
        })
        .into()
}

fn pane_head<'a>(title: &'a str, about: &'a str) -> Element<'a, Message> {
    column![text(title).size(17).color(TEXT_STRONG).font(style::bold()), text(about).size(12.5).color(TEXT_MUTED).font(UI)].spacing(4).into()
}

fn section<'a>(title: &'a str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(title.to_uppercase()).size(11).color(TEXT_FAINT).font(style::semibold()), container(body).width(Fill).padding(16).style(|_| boxed(RAISED, LINE, 10.0))].spacing(8).into()
}

fn label_with<'a>(label: &'a str, about: &'a str) -> Element<'a, Message> {
    column![text(label).size(13).color(TEXT_STRONG).font(style::semibold()), text(about).size(12).line_height(1.45).color(TEXT_MUTED).font(UI)].spacing(3).width(Fill).into()
}

fn hint<'a>(words: impl Into<String>) -> Element<'a, Message> {
    text(words.into()).size(11.5).line_height(1.45).color(TEXT_FAINT).font(UI).into()
}

// ---- General --------------------------------------------------------------------------------

fn general(brain: &Brain) -> Element<'_, Message> {
    let sheet = &brain.settings;
    let shown = match (&sheet.hotkey, sheet.recording) {
        (_, true) => t("Drück die Kombination …", "Press the combination …").to_string(),
        (Some(spec), false) => crate::hotkey::symbols(spec),
        (None, false) => t("Aus", "Off").to_string(),
    };
    let recording = sheet.recording;
    let set = sheet.hotkey.is_some() && !recording;
    let field = button(text(shown).size(13).color(if set || recording { TEXT_STRONG } else { TEXT_MUTED }).font(if set { style::semibold() } else { UI }))
        .padding([7, 12])
        .width(200)
        .on_press(msg(SettingsMsg::Record))
        .style(move |_, status| button::Style {
            background: Some(Background::Color(if recording { RAISED } else { INK })),
            text_color: TEXT,
            border: Border {
                color: if recording { alpha(WORKING, 0xcc) } else if matches!(status, button::Status::Hovered) { LINE_STRONG } else { LINE },
                width: 1.0,
                radius: 7.0.into(),
            },
            ..button::Style::default()
        });
    let mut key_row = row![field].spacing(8).align_y(iced::Center);
    if set {
        key_row = key_row.push(action(t("Aus", "Off"), "", false, Some(msg(SettingsMsg::HotkeyOff))));
    }
    let mut key = column![
        row![
            label_with(t("Kürzel", "Shortcut"), t("Holt Brain von überall nach vorn, mit dem Terminal der Session, die am längsten wartet.", "Brings Brain forward from anywhere, with the terminal of the session that has waited longest.")),
            key_row,
        ]
        .spacing(16)
        .align_y(iced::Center),
    ]
    .spacing(10);
    if let Some(note) = &sheet.hotkey_note {
        key = key.push(text(note.clone()).size(12).color(TURN).font(UI));
    }
    key = key.push(hint(if recording {
        t("esc bricht ab, ⌫ schaltet aus. Nötig ist ⌃, ⌥, ⇧ oder ⌘ mit einem Buchstaben oder der Leertaste.", "esc cancels, ⌫ turns it off. It takes ⌃, ⌥, ⇧ or ⌘ with a letter or space.")
    } else {
        t("Klick ins Feld und drück die Kombination.", "Click the field and press the combination.")
    }));
    let notifications = row![
        label_with(
            t("Mitteilung, wenn eine Session dich braucht", "Notify when a session needs you"),
            t("macOS meldet sich, wenn eine Session fragt oder eine Freigabe braucht. Ist Brain vorn, bleibt es still.", "macOS notifies you when a session asks or needs a permission. With Brain in front it stays quiet."),
        ),
        switch(sheet.notifications, |on| msg(SettingsMsg::Notifications(on))),
    ]
    .spacing(16)
    .align_y(iced::Center);
    column![
        pane_head(t("Allgemein", "General"), t("Wie Brain dich erreicht, wenn du gerade woanders bist.", "How Brain reaches you while you're elsewhere.")),
        section(t("Quick Terminal", "Quick terminal"), key),
        section(t("Mitteilungen", "Notifications"), notifications),
    ]
    .spacing(22)
    .into()
}

fn switch<'a>(on: bool, on_toggle: impl Fn(bool) -> Message + 'a) -> Element<'a, Message> {
    toggler(on)
        .on_toggle(on_toggle)
        .size(20)
        .style(|_, status| {
            let toggled = match status {
                toggler::Status::Active { is_toggled } | toggler::Status::Hovered { is_toggled } | toggler::Status::Disabled { is_toggled } => is_toggled,
            };
            toggler::Style {
                background: Background::Color(if toggled { PRIMARY } else { LINE_STRONG }),
                background_border_width: 0.0,
                background_border_color: Color::TRANSPARENT,
                foreground: Background::Color(TEXT_STRONG),
                foreground_border_width: 0.0,
                foreground_border_color: Color::TRANSPARENT,
                text_color: None,
                border_radius: None,
                padding_ratio: 0.12,
            }
        })
        .into()
}

// ---- Terminal -------------------------------------------------------------------------------

/// A choice in the font list: the default (named after the font it is), or a family.
#[derive(Debug, Clone, PartialEq)]
enum FontChoice {
    Default(String),
    Family(String),
}

impl std::fmt::Display for FontChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontChoice::Default(name) => write!(f, "{}", tr!("Wie in iTerm ({name})", "As in iTerm ({name})")),
            FontChoice::Family(name) => write!(f, "{name}"),
        }
    }
}

fn terminal(brain: &Brain) -> Element<'_, Message> {
    let sheet = &brain.settings;
    let (default_family, default_size) = sheet.default_font.clone();
    let mut choices = vec![FontChoice::Default(default_family.clone())];
    choices.extend(sheet.families.iter().cloned().map(FontChoice::Family));
    if let Some(font) = sheet.font.as_ref().filter(|f| !sheet.families.contains(f)) {
        choices.push(FontChoice::Family(font.clone()));
    }
    let chosen = match &sheet.font {
        Some(font) => FontChoice::Family(font.clone()),
        None => FontChoice::Default(default_family.clone()),
    };
    let family = select(choices, Some(chosen), |c| {
        msg(SettingsMsg::Font(match c {
            FontChoice::Default(_) => None,
            FontChoice::Family(name) => Some(name),
        }))
    });
    let size = sheet.font_size.unwrap_or(default_size);
    let smaller = (size > *FONT_SIZES.start() as f32).then(|| msg(SettingsMsg::FontSize(size - 1.0)));
    let larger = (size < *FONT_SIZES.end() as f32).then(|| msg(SettingsMsg::FontSize(size + 1.0)));
    let size_row = row![
        action("A−", "", false, smaller),
        container(text(format!("{size:.0} pt")).size(13).color(TEXT_STRONG).font(style::semibold())).width(56).center_x(56),
        action("A+", "", false, larger),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let font = crate::chat_font::get();
    let line = |words: &str, color: Color| text(words.to_string()).size(font.size).font(font.regular).color(color);
    let preview = container(
        column![
            line("> Fass die offenen Punkte zusammen", TEXT_STRONG),
            line("⏺ Read(crates/brain-app/src/config.rs)", TEXT),
            line("  ⎿  Read 31 lines", TEXT_MUTED),
            line("⏺ Offen sind noch Kürzel, Mitteilungen und Vorlagen.", TEXT),
        ]
        .spacing(font.size * 0.4),
    )
    .width(Fill)
    .padding(14)
    .style(|_| boxed(INK, LINE, 8.0));
    let changed = sheet.font.is_some() || sheet.font_size.is_some();
    let body = column![
        row![label_with(t("Schrift", "Font"), t("Im Terminal und im Verlauf der Sessions.", "In the terminal and in the sessions' history.")), container(family).width(260)].spacing(16).align_y(iced::Center),
        row![label_with(t("Größe", "Size"), ""), size_row].spacing(16).align_y(iced::Center),
        text(t("Vorschau", "Preview")).size(12).color(TEXT_MUTED).font(style::semibold()),
        preview,
        row![
            hint(tr!("Standard ist das iTerm-Profil: {default_family}, {default_size:.0} pt.", "The default is the iTerm profile: {default_family}, {default_size:.0} pt.")),
            Space::new().width(Fill),
            action(t("Zurücksetzen", "Reset"), "", false, changed.then(|| msg(SettingsMsg::FontReset))),
        ]
        .align_y(iced::Center),
    ]
    .spacing(14);
    column![pane_head(t("Terminal", "Terminal"), t("Schrift im Terminal und im Verlauf der Sessions.", "The font of the terminal and the sessions' history.")), section(t("Schrift", "Font"), body)].spacing(22).into()
}

fn select<'a, T: ToString + PartialEq + Clone + 'a>(options: Vec<T>, chosen: Option<T>, on_pick: impl Fn(T) -> Message + 'a) -> Element<'a, Message> {
    pick_list(options, chosen, on_pick)
        .width(Fill)
        .padding([6, 10])
        .text_size(12.5)
        .font(UI)
        .style(|_, status| {
            let (background, border) = match status {
                pick_list::Status::Active => (INK, LINE),
                pick_list::Status::Hovered => (RAISED, LINE_STRONG),
                pick_list::Status::Opened { .. } => (RAISED, alpha(WORKING, 0x99)),
            };
            pick_list::Style { text_color: TEXT_STRONG, placeholder_color: TEXT_FAINT, handle_color: TEXT_MUTED, background: Background::Color(background), border: Border { color: border, width: 1.0, radius: 7.0.into() } }
        })
        .menu_style(|_| iced::overlay::menu::Style {
            background: Background::Color(MENU_BG),
            border: Border { color: MENU_LINE, width: 1.0, radius: 8.0.into() },
            text_color: TEXT,
            selected_text_color: TEXT_STRONG,
            selected_background: Background::Color(LINE_STRONG),
            shadow: iced::Shadow { color: Color::from_rgba8(0, 0, 0, 0.6), offset: iced::Vector::new(0.0, 8.0), blur_radius: 24.0 },
        })
        .into()
}

// ---- Accounts -------------------------------------------------------------------------------

/// A choice in an account's profile list: a Chrome profile, or the default browser (`None`).
#[derive(Debug, Clone, PartialEq)]
struct ProfileChoice {
    dir: Option<String>,
    label: String,
}

impl std::fmt::Display for ProfileChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

fn accounts(brain: &Brain) -> Element<'_, Message> {
    let config = crate::config::path();
    let profiles = &brain.chrome_profiles;
    let head = row![
        text(t("KONTO", "ACCOUNT")).size(11).color(TEXT_FAINT).font(style::semibold()).width(110),
        text(t("FARBE", "COLOUR")).size(11).color(TEXT_FAINT).font(style::semibold()).width(150),
        text(t("CHROME-PROFIL", "CHROME PROFILE")).size(11).color(TEXT_FAINT).font(style::semibold()),
    ]
    .spacing(12);
    let mut rows = column![head].spacing(12);
    for account in &brain.model.accounts {
        let id = account.id.clone();
        let picked = brain.account_index(&id);
        let (_, hue) = style::account_colors(picked);
        let swatches = row((0..4).map(|i| {
            let (_, color) = style::account_colors(i);
            let chosen = i == picked;
            let id = id.clone();
            button(Space::new().width(18).height(18))
                .padding(0)
                .on_press(msg(SettingsMsg::AccountColor(id, i)))
                .style(move |_, status| button::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        color: if chosen { TEXT_STRONG } else if matches!(status, button::Status::Hovered) { alpha(TEXT_STRONG, 0x66) } else { Color::TRANSPARENT },
                        width: 2.0,
                        radius: 9.0.into(),
                    },
                    ..button::Style::default()
                })
                .into()
        }))
        .spacing(8)
        .width(150);
        let stored = crate::browser::stored_profile(&config, &id);
        let current = crate::browser::profile_for(&config, &id, profiles);
        let suggested = crate::browser::suggest_profile(&id, profiles).filter(|_| stored.is_none()).map(|p| p.dir.clone());
        let mut choices = vec![ProfileChoice { dir: None, label: t("Standardbrowser", "Default browser").to_string() }];
        choices.extend(profiles.iter().map(|p| {
            let name = &p.name;
            let label = if suggested.as_deref() == Some(p.dir.as_str()) { tr!("{name} (Vorschlag)", "{name} (suggested)") } else { name.clone() };
            ProfileChoice { dir: Some(p.dir.clone()), label }
        }));
        let chosen = choices.iter().find(|c| c.dir == current).cloned();
        let account_id = id.clone();
        let profile = select(choices, chosen, move |c| msg(SettingsMsg::ChromeProfile(account_id.clone(), c.dir)));
        rows = rows.push(row![text(id).size(13).color(hue).font(style::semibold()).width(110), swatches, container(profile).width(Fill)].spacing(12).align_y(iced::Center));
    }
    if profiles.is_empty() {
        rows = rows.push(hint(t("Chrome ist nicht installiert oder hat keine Profile: Links gehen im Standardbrowser auf.", "Chrome isn't installed or has no profiles: links open in the default browser.")));
    }
    let note = hint(t(
        "Neue Sessions bekommen das Profil beim Start gesagt. Laufende Sessions erfahren eine Änderung erst nach einem Neustart. „Vorschlag“ ist ein Profil, das Brain am Namen erkannt hat; gespeichert wird es erst, wenn du eins wählst.",
        "New sessions are told the profile at start. Running sessions learn a change only after a restart. “Suggested” is a profile Brain recognised by its name; it is stored only once you pick one.",
    ));
    column![
        pane_head(t("Konten", "Accounts"), t("Woran du ein Konto erkennst und in welchem Chrome-Profil seine Links aufgehen.", "How you recognise an account and which Chrome profile its links open in.")),
        container(rows).width(Fill).padding(16).style(|_| boxed(RAISED, LINE, 10.0)),
        note,
    ]
    .spacing(18)
    .into()
}

// ---- Templates ------------------------------------------------------------------------------

fn templates(brain: &Brain) -> Element<'_, Message> {
    if let Some(draft) = &brain.settings.draft {
        return template_editor(brain, draft);
    }
    let sheet = &brain.settings;
    let mut list = column![].spacing(2);
    for (i, template) in sheet.templates.iter().enumerate() {
        let details: Vec<String> = [template.folder.clone(), template.account.clone(), template.model.clone()].into_iter().flatten().collect();
        let mut lines = column![text(template.name.clone()).size(13).color(TEXT_STRONG).font(style::semibold())].spacing(2);
        if !details.is_empty() {
            lines = lines.push(text(details.join(" · ")).size(11.5).color(TEXT_MUTED).font(UI));
        }
        list = list.push(
            button(lines)
                .width(Fill)
                .padding([8, 12])
                .on_press(msg(SettingsMsg::EditTemplate(i)))
                .style(|_, status| button::Style {
                    background: matches!(status, button::Status::Hovered | button::Status::Pressed).then_some(Background::Color(ISLAND_HOVER)),
                    text_color: TEXT,
                    border: Border { radius: 7.0.into(), ..Border::default() },
                    ..button::Style::default()
                }),
        );
    }
    let body: Element<'_, Message> = if sheet.templates.is_empty() {
        hint(t("Noch keine Vorlage. Eine Vorlage startet eine Session mit Ordner, Konto, Modell und erstem Prompt.", "No template yet. A template starts a session with folder, account, model and first prompt."))
    } else {
        list.into()
    };
    column![
        pane_head(t("Vorlagen", "Templates"), t("Vorlagen stehen im Dialog „Neue Session“ zur Wahl. Jede ist eine Markdown-Datei in ~/.claude-brain/templates/.", "Templates are offered in the “New session” dialog. Each is a Markdown file in ~/.claude-brain/templates/.")),
        row![
            primary_action(t("Neue Vorlage", "New template"), "", Some(msg(SettingsMsg::NewTemplate))),
            action(t("Im Finder zeigen", "Show in Finder"), "", false, Some(msg(SettingsMsg::ShowTemplates))),
        ]
        .spacing(10),
        container(body).width(Fill).padding(8).style(|_| boxed(RAISED, LINE, 10.0)),
        hint(t("Platzhalter wie {branch} fragt Brain beim Start ab.", "Brain asks for placeholders like {branch} at start.")),
    ]
    .spacing(16)
    .into()
}

/// A choice of the template's account or model: "at start" / "the account's default" is `None`.
#[derive(Debug, Clone, PartialEq)]
struct Optional {
    value: Option<String>,
    label: String,
}

impl std::fmt::Display for Optional {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

fn optional_choices(none: &str, values: impl IntoIterator<Item = String>, current: Option<&String>) -> (Vec<Optional>, Option<Optional>) {
    let mut choices = vec![Optional { value: None, label: none.to_string() }];
    choices.extend(values.into_iter().map(|v| Optional { label: v.clone(), value: Some(v) }));
    // A value the file holds that isn't offered stays visible.
    if let Some(value) = current.filter(|v| !choices.iter().any(|c| c.value.as_ref() == Some(*v))) {
        choices.push(Optional { value: Some(value.clone()), label: value.clone() });
    }
    let chosen = choices.iter().find(|c| c.value.as_ref() == current).cloned();
    (choices, chosen)
}

fn field_row<'a>(label: &'a str, field: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![text(label).size(12.5).color(TEXT_MUTED).font(style::semibold()).width(80), container(field).width(Fill)].spacing(12).align_y(iced::Center).into()
}

fn template_editor<'a>(brain: &'a Brain, draft: &'a Draft) -> Element<'a, Message> {
    let title = if draft.is_new() { t("Neue Vorlage", "New template").to_string() } else { draft.name.clone() };
    let back = button(text(t("Vorlagen", "Templates")).size(13).color(WORKING).font(style::semibold())).padding(0).on_press(msg(SettingsMsg::CancelTemplate)).style(|_, _| button::Style::default());
    let crumbs = row![back, text("/").size(13).color(TEXT_FAINT).font(UI), text(title).size(13).color(TEXT_STRONG).font(style::semibold())].spacing(8).align_y(iced::Center);
    let input = |placeholder: &'a str, value: &'a str| text_input(placeholder, value).size(13).font(UI).padding([7, 10]).style(field_style);
    let name = input(t("Wie die Vorlage im Dialog heißt", "How the template is called in the dialog"), &draft.name).id(TEMPLATE_NAME).on_input(|v| msg(SettingsMsg::DraftName(v)));
    let folder = row![
        input(t("Beim Start wählen", "Pick at start"), &draft.folder).on_input(|v| msg(SettingsMsg::DraftFolder(v))),
        action(t("Wählen …", "Choose …"), "", false, Some(msg(SettingsMsg::PickFolder))),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let (accounts, account) = optional_choices(t("Beim Start wählen", "Pick at start"), brain.model.accounts.iter().map(|a| a.id.clone()), draft.account.as_ref());
    let (models, model) = optional_choices(t("Standard des Kontos", "The account's default"), ["opus", "sonnet", "haiku"].map(String::from), draft.model.as_ref());
    let prompt = text_editor(&draft.prompt)
        .on_action(|a| msg(SettingsMsg::DraftPrompt(a)))
        .placeholder(t("Der erste Prompt der Session. {branch} und andere Platzhalter fragt Brain beim Start ab.", "The session's first prompt. Brain asks for {branch} and other placeholders at start."))
        .height(180)
        .padding(12)
        .size(13)
        .font(UI)
        .style(|_, _| text_editor::Style { background: Background::Color(INK), border: Border { color: LINE_STRONG, width: 1.0, radius: 8.0.into() }, placeholder: TEXT_FAINT, value: TEXT, selection: alpha(WORKING, 0x55) });
    let placeholders = draft.placeholders();
    let mut form = column![
        field_row(t("Name", "Name"), name),
        field_row(t("Ordner", "Folder"), folder),
        field_row(t("Konto", "Account"), select(accounts, account, |c| msg(SettingsMsg::DraftAccount(c.value)))),
        field_row(t("Modell", "Model"), select(models, model, |c| msg(SettingsMsg::DraftModel(c.value)))),
        text(t("Prompt", "Prompt")).size(12.5).color(TEXT_MUTED).font(style::semibold()),
        prompt,
    ]
    .spacing(12);
    if !placeholders.is_empty() {
        let names = placeholders.iter().map(|p| format!("{{{p}}}")).collect::<Vec<_>>().join(", ");
        form = form.push(hint(tr!("Fragt beim Start: {names}", "Asks at start: {names}")));
    }
    let buttons: Element<'_, Message> = if draft.confirm_delete {
        let name = draft.name.clone();
        row![
            text(tr!("„{name}“ in den Papierkorb legen?", "Move “{name}” to the Trash?")).size(13).color(TEXT_STRONG).font(UI),
            Space::new().width(Fill),
            action(t("Behalten", "Keep"), "esc", false, Some(msg(SettingsMsg::KeepTemplate))),
            action(t("Löschen", "Delete"), "", true, Some(msg(SettingsMsg::DeleteTemplate))),
        ]
        .spacing(10)
        .align_y(iced::Center)
        .into()
    } else {
        let mut line = row![].spacing(10).align_y(iced::Center);
        if !draft.is_new() {
            line = line.push(action(t("Löschen …", "Delete …"), "", false, Some(msg(SettingsMsg::AskDelete))));
        }
        line.push(Space::new().width(Fill))
            .push(action(t("Abbrechen", "Cancel"), "esc", false, Some(msg(SettingsMsg::CancelTemplate))))
            .push(primary_action(t("Sichern", "Save"), "⌘S", (!draft.name.trim().is_empty()).then(|| msg(SettingsMsg::SaveTemplate))))
            .into()
    };
    column![crumbs, container(form).width(Fill).padding(16).style(|_| boxed(RAISED, LINE, 10.0)), buttons].spacing(16).width(Length::Fill).into()
}
