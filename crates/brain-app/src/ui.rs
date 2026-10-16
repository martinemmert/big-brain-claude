//! Everything Brain draws: title bar, session list, detail pane, dialogs, footer.

use brain_core::state::{Phase, Session};
use brain_core::transcript::Role;
use iced::widget::{button, column, container, mouse_area, row, scrollable, stack, text, text_input, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Padding};

use crate::app::{self, Action, Brain, Choice, DetailTab, Item, Message, Mode, CLEANUP_DAYS};
use crate::config;
use crate::format::{ago, ago_phrase, background_summary, clip, clock, format_tokens, now_ms, phase_label, plain, short_model, tilde};
use crate::i18n::t;
use crate::markdown;
use crate::prefs::Layout;
use crate::style::{
    self, account_badge, action, alpha, boxed, chip, hint, kbd, marker, phase_color, rail, section_title, segmented, CALLS,
    CALLS_SOFT, CHROME, DONE, INK, LINE, LINE_STRONG, MONO, RAISED, SURFACE, TEXT, TEXT_FAINT, TEXT_MUTED, TEXT_STRONG,
    TURN, UI, WORKING,
};
use crate::tr;

/// Room for the window's traffic lights at the left of the title bar.
const TRAFFIC_LIGHTS: f32 = if cfg!(target_os = "macos") { 78.0 } else { 14.0 };

pub fn view(brain: &Brain) -> Element<'_, Message> {
    let now = now_ms();
    let detail = match brain.mode {
        _ if brain.overview => overview(brain),
        Mode::Cleanup => cleanup(brain),
        Mode::Pairing => pairing(brain),
        Mode::AccountSetup => account_setup(brain),
        Mode::Sync => sync_accounts(brain),
        Mode::Keys => keys_sheet(brain),
        Mode::Garden => garden(brain),
        Mode::Relaunch => relaunch(brain),
        Mode::NewSession => new_session(brain),
        _ if brain.prefs.layout == Layout::Today => today(brain),
        _ => detail(brain, now),
    };
    let mut left = column![search(brain), list(brain, now)];
    if let Some(usage) = usage(brain, now) {
        left = left.push(usage);
    }
    let width = brain.prefs.sidebar_width.unwrap_or(app::SIDEBAR_DEFAULT);
    let edge = if brain.dragging_sidebar { WORKING } else { LINE };
    // A 7 pt grab area around the 1 pt divider line.
    let grip = mouse_area(
        container(container(Space::new().width(1).height(Fill)).style(move |_| style::fill(edge))).width(7).height(Fill).center_x(7),
    )
    .on_press(Message::SidebarDrag)
    .interaction(iced::mouse::Interaction::ResizingHorizontally);
    // The pointer is followed over the list only (that's where context menus open); the list
    // starts below the title bar and its divider.
    let left = mouse_area(container(left).width(Length::Fixed(width - 3.0)).height(Fill).style(|_| style::fill(CHROME)))
        .on_move(|p| Message::Pointer(iced::Point::new(p.x, p.y + crate::chrome::TITLEBAR_HEIGHT + 1.0)));
    let body = row![
        left,
        grip,
        container(detail).width(Fill).height(Fill).style(|_| style::fill(INK)),
    ];
    let mut chrome = column![titlebar(brain), divider(), body.height(Fill), divider()];
    if let Some(offer) = screenshot_offer(brain) {
        chrome = chrome.push(offer);
    }
    let window = container(chrome.push(footer(brain)))
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style { background: Some(Background::Color(INK)), text_color: Some(TEXT), ..container::Style::default() });
    // Always a stack, with an empty layer when nothing lies on top: a root that changed shape
    // when a menu opened would cost the list its state, and with it its scroll position.
    let overlay: Element<'_, Message> = match brain.mode {
        Mode::Palette => palette(brain),
        Mode::Composer => composer(brain),
        _ => context_menu(brain).unwrap_or_else(|| Space::new().into()),
    };
    stack![window, overlay, error_toast(brain)].into()
}

/// Above the footer at the bottom right; a click dismisses it.
fn error_toast(brain: &Brain) -> Element<'_, Message> {
    let Some((message, _)) = &brain.error else { return Space::new().into() };
    let toast = mouse_area(
        container(row![text("!").size(13).color(CALLS).font(style::bold()), text(message.as_str()).size(12.5).color(TEXT_STRONG).font(UI).line_height(1.4)].spacing(10))
            .padding([10, 14])
            .max_width(460)
            .style(|_| container::Style {
                shadow: iced::Shadow { color: alpha(CALLS, 0x40), offset: iced::Vector::new(0.0, 4.0), blur_radius: 18.0 },
                ..boxed(RAISED, CALLS, 8.0)
            }),
    )
    .on_press(Message::DismissError)
    .interaction(iced::mouse::Interaction::Pointer);
    container(toast).width(Fill).height(Fill).align_right(Fill).align_bottom(Fill).padding(Padding { bottom: 44.0, right: 16.0, ..Padding::ZERO }).into()
}

/// ⌘E: a longer prompt for the selected session, with templates and pasted screenshots.
fn composer(brain: &Brain) -> Element<'_, Message> {
    let name = brain.selected_session().map(|s| s.display_name()).unwrap_or_default();
    let editor = iced::widget::text_editor(&brain.composer)
        .id(app::COMPOSER)
        .on_action(Message::ComposerEdit)
        .placeholder(style::keys(t("Prompt schreiben … (⌘V fügt einen Screenshot ein)", "Write the prompt … (⌘V adds a screenshot)")))
        .height(260)
        .padding(12)
        .size(14)
        .font(UI)
        .style(|_, _| iced::widget::text_editor::Style {
            background: Background::Color(INK),
            border: Border { color: LINE_STRONG, width: 1.0, radius: 8.0.into() },
            placeholder: TEXT_FAINT,
            value: TEXT,
            selection: alpha(WORKING, 0x55),
        });
    let mut body = column![
        row![
            text(tr!("Prompt an „{name}“", "Prompt for “{name}”")).size(16).color(TEXT_STRONG).font(style::semibold()),
            Space::new().width(Fill),
            action(t("Schließen", "Close"), "esc", false, Some(Message::Close)),
        ]
        .align_y(iced::Center),
        editor,
    ]
    .spacing(12);
    if !brain.composer_images.is_empty() {
        let thumbs = brain.composer_images.iter().enumerate().map(|(i, path)| -> Element<'_, Message> {
            mouse_area(
                container(iced::widget::image(iced::widget::image::Handle::from_path(path)).height(64))
                    .padding(3)
                    .style(|_| boxed(SURFACE, LINE, 6.0)),
            )
            .on_press(Message::ComposerRemoveImage(i))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
        });
        body = body.push(column![
            row(thumbs).spacing(8),
            text(t("Klick auf ein Bild entfernt es.", "Click a picture to remove it.")).size(11).color(TEXT_FAINT).font(UI),
        ].spacing(4));
    }
    if !brain.composer_templates.is_empty() {
        let buttons = brain.composer_templates.iter().enumerate().take(8).map(|(i, template)| style::text_button(template.name.clone(), false, Message::ComposerTemplate(i)));
        body = body.push(row![text(t("Vorlagen", "Templates")).size(11.5).color(TEXT_FAINT).font(UI)].push(row(buttons).spacing(6).wrap()).spacing(8).align_y(iced::Center));
    }
    body = body.push(row![Space::new().width(Fill), action(t("Senden", "Send"), "⌘⏎", true, Some(Message::ComposerSend))]);
    let panel = container(body)
        .padding(18)
        .width(760)
        .style(|_| container::Style {
            shadow: iced::Shadow { color: Color { a: 0.5, ..Color::BLACK }, offset: iced::Vector::new(0.0, 10.0), blur_radius: 30.0 },
            ..boxed(RAISED, LINE_STRONG, 12.0)
        });
    let backdrop = mouse_area(container(Space::new().width(Fill).height(Fill)).style(|_| style::fill(Color { a: 0.35, ..Color::BLACK }))).interaction(iced::mouse::Interaction::Idle);
    stack![backdrop, container(panel).padding(Padding { top: 80.0, ..Padding::ZERO }).center_x(Fill)].into()
}

/// ⌘K: a field and the matching commands; ↑↓ choose, ⏎ runs, esc closes.
fn palette(brain: &Brain) -> Element<'_, Message> {
    const SHOWN: usize = 12;
    let entries = brain.palette_entries();
    let field = text_input(t("Befehl oder Text für die Session …", "A command, or text for the session …"), &brain.palette)
        .id(app::PALETTE)
        .on_input(Message::Palette)
        .size(15)
        .font(UI)
        .padding([10, 4])
        .style(|theme, status| text_input::Style { border: Border::default(), background: Background::Color(Color::TRANSPARENT), ..field_style(theme, status) });
    // Keep the chosen line in the window of shown lines.
    let first = brain.palette_index.saturating_sub(SHOWN - 1);
    let rows = entries.into_iter().enumerate().skip(first).take(SHOWN).map(|(i, (label, keys, command))| -> Element<'_, Message> {
        let chosen = i == brain.palette_index;
        button(row![text(label).size(13).color(if chosen { TEXT_STRONG } else { TEXT }).font(UI).width(Fill), text(style::keys(keys)).size(11.5).color(TEXT_FAINT).font(UI)].spacing(12).align_y(iced::Center))
            .padding([7, 12])
            .width(Fill)
            .on_press(Message::PaletteRun(command))
            .style(move |_, status| button::Style {
                background: (chosen || matches!(status, button::Status::Hovered)).then(|| Background::Color(alpha(WORKING, if chosen { 0x33 } else { 0x1a }))),
                border: Border { radius: 6.0.into(), ..Border::default() },
                ..button::Style::default()
            })
            .into()
    });
    let list = column(rows).spacing(1);
    let panel = container(column![container(row![text(style::keys("⌘K")).size(12).color(TEXT_FAINT), field].spacing(10).align_y(iced::Center)).padding([0, 8]), divider(), list].spacing(6))
        .padding(8)
        .width(620)
        .style(|_| container::Style {
            shadow: iced::Shadow { color: Color { a: 0.5, ..Color::BLACK }, offset: iced::Vector::new(0.0, 10.0), blur_radius: 30.0 },
            ..boxed(RAISED, LINE_STRONG, 12.0)
        });
    let backdrop = mouse_area(container(Space::new().width(Fill).height(Fill)).style(|_| style::fill(Color { a: 0.35, ..Color::BLACK })))
        .on_press(Message::PaletteClose)
        .interaction(iced::mouse::Interaction::Idle);
    stack![backdrop, container(panel).padding(Padding { top: 90.0, ..Padding::ZERO }).center_x(Fill)].into()
}

/// A screenshot taken a moment ago: into a session's terminal, into the composer, or away.
fn screenshot_offer(brain: &Brain) -> Option<Element<'_, Message>> {
    let path = brain.screenshot_offer.as_ref()?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut bar = row![
        container(iced::widget::image(iced::widget::image::Handle::from_path(path)).height(34)).padding(2).style(|_| boxed(SURFACE, LINE, 4.0)),
        text(tr!("Neuer Screenshot: {name}", "New screenshot: {name}")).size(12.5).color(TEXT).font(UI).width(Fill),
    ]
    .spacing(10)
    .align_y(iced::Center);
    if let Some(target) = brain.screenshot_target().and_then(|k| brain.model.board.get(&k)) {
        let session = clip(&target.display_name(), 28);
        bar = bar.push(action(tr!("In „{session}“", "Into “{session}”"), "", true, Some(Message::ScreenshotTo(true))));
    }
    bar = bar
        .push(action(t("In den Composer", "Into the composer"), "", false, Some(Message::ScreenshotTo(false))))
        .push(button(text("✕").size(13).color(TEXT_MUTED)).padding([4, 8]).on_press(Message::ScreenshotDismiss).style(|_, _| button::Style::default()));
    Some(container(bar).padding([6, 14]).width(Fill).style(|_| style::fill(alpha(WORKING, 0x1c))).into())
}

// ---- context menu -----------------------------------------------------------------

const MENU_WIDTH: f32 = 300.0;
const MENU_ROW: f32 = 28.0;
const MENU_PADDING: f32 = 5.0;
/// A separator's height: 1 pt line, 4 pt above and below.
const MENU_SEPARATOR: f32 = 9.0;

/// One line of a context menu; `None` is a separator.
type MenuLine = Option<(String, &'static str, Action, bool)>;

/// What a right click on a list entry offers: the same actions as the keys, in groups.
fn menu_lines(brain: &Brain, s: &Session) -> Vec<MenuLine> {
    let phase = s.phase();
    let ended = phase == Phase::Ended;
    let caps = brain.capabilities();
    let agent_active = s.agent.as_ref().is_some_and(|a| a.is_active());
    let mut groups: Vec<Vec<(String, &'static str, Action, bool)>> = Vec::new();

    let mut open = Vec::new();
    if s.agent.is_some() {
        open.push((t("Öffnen", "Attach").to_string(), "⏎", Action::Open, false));
        open.push((t("In iTerm öffnen", "Open in iTerm").to_string(), "⌥⏎", Action::OpenInITerm, false));
    } else if ended {
        open.push((t("Fortsetzen", "Resume").to_string(), "⏎", Action::Resume, false));
        open.push((t("In iTerm fortsetzen", "Resume in iTerm").to_string(), "⌥⏎", Action::OpenInITerm, false));
    } else if caps.focus {
        open.push((t("Öffnen", "Open").to_string(), "⏎", Action::Open, false));
    }
    if s.agent.is_none() && (ended || s.accepts_input()) {
        open.push((t("Nach Brain holen", "Move into Brain").to_string(), "I", Action::TakeOver, false));
    }
    // Beside the terminal that's on screen: two sessions side by side.
    let beside = brain.before_menu.as_ref().is_some_and(|k| brain.terminals.contains_key(k));
    if beside && (s.agent.is_some() || ended || brain.terminals.contains_key(&s.key)) {
        open.push((t("Daneben öffnen (geteilt)", "Open beside (split)").to_string(), "⌘D", Action::OpenBeside, false));
    }
    groups.push(open);

    let mut talk = Vec::new();
    if s.awaiting_permission() && caps.keys {
        talk.push((t("Erlauben", "Allow").to_string(), "Y", Action::Allow, false));
        talk.push((t("Ablehnen", "Deny").to_string(), "N", Action::Deny, false));
    } else if s.accepts_input() && caps.type_text {
        talk.push((t("Antworten", "Reply").to_string(), "T", Action::StartReply, false));
    }
    // Brain renames by typing `/rename`: only while the session waits.
    if s.accepts_input() {
        talk.push((t("Umbenennen", "Rename").to_string(), "R", Action::StartRename, false));
    }
    groups.push(talk);

    let pinned = brain.prefs.is_pinned(&s.key);
    let pin = match (pinned, ended) {
        (false, true) => t("Zum Fortsetzen merken", "Save to resume"),
        (true, true) => t("Nicht mehr merken", "Don't save"),
        (false, false) => t("Anheften", "Pin"),
        (true, false) => t("Lösen", "Unpin"),
    };
    let mut keep = vec![(pin.to_string(), "P", Action::Pin, false)];
    let mute = if brain.prefs.is_muted(&s.key) { t("Benachrichtigungen an", "Unmute") } else { t("Stummschalten", "Mute") };
    keep.push((mute.to_string(), "", Action::Mute, false));
    let hidden = brain.prefs.is_hidden(&s.key, s.last_activity_ms);
    if hidden {
        keep.push((t("Wieder einblenden", "Show again").to_string(), "S", Action::Snooze, false));
    } else if !ended {
        keep.push((t("Pausieren (15 min → 1 h → morgen → bis sich etwas tut)", "Snooze (15 min → 1 h → tomorrow → until something happens)").to_string(), "S", Action::Snooze, false));
    }
    keep.push((t("Weiterführen … (Konto, Fork, Flags)", "Go on … (account, fork, flags)").to_string(), "A", Action::Relaunch, false));
    groups.push(keep);

    let mut remove = Vec::new();
    if agent_active {
        remove.push((t("Hintergrund-Session stoppen", "Stop background session").to_string(), "X", Action::End, false));
    } else if !ended && s.accepts_input() {
        remove.push((t("Beenden", "End").to_string(), "X", Action::End, false));
    }
    // When ending doesn't work (it hangs, or Brain can't reach it): stop the process.
    if !ended && s.alive {
        remove.push((t("Hart beenden (Prozess stoppen)", "End by force (stop the process)").to_string(), "", Action::ForceEnd, true));
    }
    // A running session is put away with S; an ended one is hidden until something happens.
    if ended && !hidden {
        remove.push((t("Ausblenden", "Hide").to_string(), "⌫", Action::Hide, false));
    }
    if ended || s.agent.as_ref().is_some_and(|a| !a.is_active()) {
        remove.push((t("In den Papierkorb", "Move to Trash").to_string(), "⌘⌫", Action::Trash, true));
    }
    groups.push(remove);

    let mut lines = Vec::new();
    for group in groups.into_iter().filter(|g| !g.is_empty()) {
        if !lines.is_empty() {
            lines.push(None);
        }
        lines.extend(group.into_iter().map(Some));
    }
    lines
}

/// The open context menu at the pointer, kept inside the window; a click beside it closes it.
fn context_menu(brain: &Brain) -> Option<Element<'_, Message>> {
    let (key, at) = brain.context_menu.as_ref()?;
    let s = brain.model.board.get(key)?;
    let lines = menu_lines(brain, s);
    let height = MENU_PADDING * 2.0 + lines.iter().map(|l| if l.is_some() { MENU_ROW } else { MENU_SEPARATOR }).sum::<f32>();
    let x = at.x.min(brain.window_size.width - MENU_WIDTH - 8.0).max(8.0);
    // Below the pointer when it fits, otherwise above it.
    let y = if at.y + height + 8.0 <= brain.window_size.height { at.y } else { (at.y - height).max(8.0) };

    let rows = lines.into_iter().map(|line| -> Element<'_, Message> {
        let Some((label, keys, action, danger)) = line else {
            return container(container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)))
                .padding([4, 6])
                .height(MENU_SEPARATOR)
                .into();
        };
        let color = if danger { CALLS_SOFT } else { TEXT };
        button(
            row![text(label).size(13).color(color).font(UI).width(Fill), kbd(keys)]
                .spacing(12)
                .height(Fill)
                .align_y(iced::Center),
        )
        .padding([0, 10])
        .height(MENU_ROW)
        .width(Fill)
        .on_press(Message::MenuPick(action))
        .style(move |_, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then(|| Background::Color(if danger { alpha(CALLS, 0x2e) } else { alpha(WORKING, 0x2e) })),
            border: Border { radius: 5.0.into(), ..Border::default() },
            ..button::Style::default()
        })
        .into()
    });
    let menu = container(column(rows))
        .padding(MENU_PADDING)
        .width(MENU_WIDTH)
        .style(|_| container::Style {
            shadow: iced::Shadow { color: Color { a: 0.45, ..Color::BLACK }, offset: iced::Vector::new(0.0, 6.0), blur_radius: 18.0 },
            ..boxed(RAISED, LINE_STRONG, 8.0)
        });
    let backdrop = mouse_area(Space::new().width(Fill).height(Fill))
        .on_press(Message::CloseMenu)
        .on_right_press(Message::CloseMenu)
        .interaction(iced::mouse::Interaction::Idle);
    Some(stack![backdrop, iced::widget::pin(menu).position(iced::Point::new(x, y))].into())
}

fn divider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)).into()
}

// ---- title bar --------------------------------------------------------------------

fn titlebar(brain: &Brain) -> Element<'_, Message> {
    let (_, calling) = brain.waiting_counts();
    let mut bar = row![
        Space::new().width(TRAFFIC_LIGHTS),
        brand_mark(calling > 0),
        text("Brain").size(14).color(TEXT_STRONG).font(style::bold()),
        Space::new().width(Fill),
    ]
    .spacing(12)
    .align_y(iced::Center);
    if let Some(update) = &brain.update {
        let version = update.version.clone();
        bar = bar.push(
            button(text(tr!("Update {version}", "Update {version}")).size(12).color(DONE).font(UI))
                .padding([3, 9])
                .on_press(Message::OpenUrl(update.url.clone()))
                .style(|_, _| button::Style { background: Some(Background::Color(alpha(DONE, 0x22))), border: iced::border::rounded(6), ..button::Style::default() }),
        );
    }
    // Sessions Brain can't steer any more: the gardener ends them.
    if brain.garden_count > 0 {
        let count = brain.garden_count;
        bar = bar.push(style::text_button(tr!("✂ {count} kaputt", "✂ {count} broken"), false, Message::OpenGarden));
    }
    // A session start synced the accounts but left conflicts for the user.
    if brain.sync.open > 0 {
        let count = brain.sync.open;
        bar = bar.push(style::text_button(tr!("⇄ {count} Konto-Konflikte", "⇄ {count} account conflicts"), false, Message::OpenSync));
    }
    let mut accounts = vec![(t("Alle", "All").to_string(), brain.filter.is_none(), Message::Filter(None))];
    accounts.extend(brain.model.accounts.iter().map(|a| (a.id.clone(), brain.filter.as_deref() == Some(&a.id), Message::Filter(Some(a.id.clone())))));
    bar = bar.push(segmented(accounts));

    mouse_area(container(bar).height(crate::chrome::TITLEBAR_HEIGHT).center_y(crate::chrome::TITLEBAR_HEIGHT).padding(Padding { left: 0.0, right: 14.0, top: 0.0, bottom: 0.0 }).style(|_| style::fill(CHROME)))
        .on_press(Message::DragWindow)
        .on_double_click(Message::ZoomWindow)
        .into()
}

/// Brain's mark in miniature: a grid of sessions, one of them calling.
fn brand_mark<'a>(calling: bool) -> Element<'a, Message> {
    let cell = |color: Color| style::dot::<Message>(color, 4.0);
    let quiet = LINE_STRONG;
    container(
        column![
            row![cell(quiet), cell(WORKING), cell(if calling { CALLS } else { quiet })].spacing(2),
            row![cell(WORKING), cell(quiet), cell(quiet)].spacing(2),
            row![cell(quiet), cell(quiet), cell(WORKING)].spacing(2),
        ]
        .spacing(2),
    )
    .padding(4)
    .style(|_| boxed(RAISED, Color::TRANSPARENT, 6.0))
    .into()
}

// ---- list -------------------------------------------------------------------------

fn search(brain: &Brain) -> Element<'_, Message> {
    let editing = brain.mode == Mode::Search;
    let field = text_input(t("Sessions durchsuchen", "Search sessions"), &brain.search)
        .id(app::SEARCH)
        .on_input(Message::Search)
        .on_submit(Message::SearchSubmit)
        .size(13)
        .font(UI)
        .padding([7, 2])
        .style(|theme, status| text_input::Style { border: Border::default(), background: Background::Color(Color::TRANSPARENT), ..field_style(theme, status) });
    let edge = if editing { alpha(WORKING, 0x99) } else { LINE };
    let by_project = brain.prefs.layout == Layout::Projects;
    let grouping = style::text_button(
        t("Projekte", "Projects"),
        by_project,
        Message::Layout(if by_project { Layout::Status } else { Layout::Projects }),
    );
    container(
        row![
            container(row![text("⌕").size(13).color(TEXT_FAINT), field, kbd(if editing { "esc" } else { "/" })].spacing(6).align_y(iced::Center))
                .padding([0, 10])
                .width(Fill)
                .style(move |_| boxed(INK, edge, 8.0)),
            grouping,
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding(Padding { top: 12.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .into()
}

fn field_style(_: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(INK),
        border: Border { color: if focused { alpha(WORKING, 0x99) } else { LINE }, width: 1.0, radius: 8.0.into() },
        icon: TEXT_FAINT,
        placeholder: TEXT_FAINT,
        value: TEXT_STRONG,
        selection: alpha(WORKING, 0x55),
    }
}

fn list(brain: &Brain, now: i64) -> Element<'_, Message> {
    let groups = brain.groups(now);
    let items: Vec<Element<'_, Message>> = brain
        .items(&groups, now)
        .into_iter()
        .map(|item| match item {
            Item::Section { title, count, alert, toggle } => {
                let title = container(section_title(title, count, alert)).height(app::SECTION_H);
                match toggle {
                    Some(message) => mouse_area(title).on_press(message).interaction(iced::mouse::Interaction::Pointer).into(),
                    None => title.into(),
                }
            }
            Item::Hint(label) => container(hint(clip(&label, 70))).height(app::HINT_H).into(),
            Item::Card(s, nav) => card(brain, s, nav, now),
            Item::Row(s, _) => list_row(brain, s, now),
        })
        .collect();
    scrollable(column(items).spacing(app::LIST_GAP).padding(Padding { top: app::LIST_TOP, right: 12.0, bottom: 16.0, left: 12.0 }))
        .id(app::LIST)
        .on_scroll(Message::ListScrolled)
        .height(Fill)
        .style(scrollbar)
        .into()
}

fn scrollbar(theme: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller { background: Background::Color(LINE_STRONG), border: iced::border::rounded(3) },
    };
    style.vertical_rail = rail;
    style.horizontal_rail = rail;
    style
}

/// Small markers after a session's name: worktree, PR, conflict, background, pinned, muted …
fn markers<'a>(brain: &'a Brain, s: &Session) -> Vec<Element<'a, Message>> {
    let mut out = Vec::new();
    if s.insight.is_automated() {
        out.push(marker(t("⚙ automatisch", "⚙ automated"), TEXT_FAINT));
    }
    if brain.bells.contains(&s.key) {
        out.push(marker(t("● klingelt", "● rang"), CALLS));
    }
    if let Some(title) = brain.terminal_titles.get(&s.key) {
        out.push(marker(format!("▸ {}", clip(title, 40)), TEXT_MUTED));
    }
    if let Some((_, Some(worktree))) = brain.projects.get(&s.key) {
        out.push(marker(format!("⑂ {}", clip(worktree, 18)), TEXT_FAINT));
    }
    if let Some(pr) = brain.prs.get(&s.key) {
        let (symbol, color) = pr_look(pr);
        out.push(marker(format!("#{} {symbol}", pr.number), color));
    }
    if brain.conflicts.get(&s.key).is_some_and(|c| !c.shared_files.is_empty()) {
        out.push(marker(t("⚠ Konflikt", "⚠ conflict"), CALLS));
    }
    if let Some(agent) = &s.agent {
        let label = match agent.state.as_str() {
            "blocked" => t("Hintergrund · wartet", "background · blocked"),
            "failed" => t("Hintergrund · fehlgeschlagen", "background · failed"),
            "done" if agent.pid.is_none() => t("Hintergrund · fertig", "background · done"),
            "stopped" => t("Hintergrund · gestoppt", "background · stopped"),
            _ => t("Hintergrund", "background"),
        };
        out.push(marker(format!("◌ {label}"), TEXT_FAINT));
    }
    if brain.prefs.is_pinned(&s.key) {
        out.push(marker("★", TURN));
    }
    if brain.prefs.is_hidden(&s.key, s.last_activity_ms) {
        out.push(marker(t("ausgeblendet", "hidden"), TEXT_FAINT));
    }
    if brain.prefs.is_muted(&s.key) {
        out.push(marker(t("stumm", "muted"), TEXT_FAINT));
    }
    if let Some(until) = brain.prefs.snoozed_until(&s.key, now_ms()) {
        let at = clock(until);
        out.push(marker(tr!("pausiert bis {at}", "snoozed until {at}"), TEXT_FAINT));
    }
    out
}

fn name_label<'a>(s: &Session, size: f32, color: Color) -> Element<'a, Message> {
    text(clip(&s.display_name(), 30)).size(size).color(color).font(style::semibold()).wrapping(text::Wrapping::None).into()
}

/// Wraps a list entry so a click selects it and a double click opens it.
fn clickable<'a>(content: Element<'a, Message>, s: &Session) -> Element<'a, Message> {
    mouse_area(content)
        .on_press(Message::Select(s.key.clone()))
        .on_double_click(Message::OpenEntry(s.key.clone()))
        .on_right_press(Message::ContextMenu(s.key.clone()))
        .on_enter(Message::Hover(Some(s.key.clone())))
        .on_exit(Message::Hover(None))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// A session that waits for the user: colour rail, name, waiting time, headline.
fn card<'a>(brain: &'a Brain, s: &'a Session, nav: usize, now: i64) -> Element<'a, Message> {
    let selected = brain.selected.as_ref() == Some(&s.key);
    let hovered = brain.hovered.as_ref() == Some(&s.key);
    let phase = s.phase();
    let color = phase_color(phase);
    let headline = plain(&s.headline().unwrap_or_else(|| match phase {
        Phase::NeedsYou => t("Wartet auf deine Eingabe", "Waiting for your input").into(),
        _ => t("Fertig, du bist dran", "Done, your turn").into(),
    }));
    let headline = if s.headline_is_reported() && phase == Phase::NeedsYou { format!("“{headline}”") } else { headline };

    let mut top = row![name_label(s, 13.5, TEXT_STRONG), account_badge(&s.key.account, brain.account_index(&s.key.account))].spacing(8).align_y(iced::Center);
    for m in markers(brain, s) {
        top = top.push(m);
    }
    top = top.push(Space::new().width(Fill)).push(text(ago(s.phase_since_ms(), now)).size(12.5).color(color).font(style::semibold()));
    if nav < 9 {
        top = top.push(kbd(format!("{}", nav + 1)));
    }
    let body = column![
        top,
        text(clip(&headline, 150)).size(12.5).line_height(1.45).color(if phase == Phase::NeedsYou { TEXT_STRONG } else { TEXT }).font(UI),
    ]
    .spacing(5);
    let background = if selected { RAISED } else if hovered { style::HOVER } else { SURFACE };
    let edge = if selected { alpha(color, 0x77) } else { LINE };
    let content = container(row![rail(color, 10.0), container(body).padding([11, 12]).width(Fill).clip(true)].height(Fill))
        .height(app::CARD_H)
        .width(Fill)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(background)),
            border: Border { color: edge, width: 1.0, radius: 10.0.into() },
            shadow: if selected { style::glow(color) } else { iced::Shadow::default() },
            ..container::Style::default()
        });
    clickable(content.into(), s)
}

fn list_row<'a>(brain: &'a Brain, s: &'a Session, now: i64) -> Element<'a, Message> {
    let selected = brain.selected.as_ref() == Some(&s.key);
    let hovered = brain.hovered.as_ref() == Some(&s.key);
    let phase = s.phase();
    let ended = phase == Phase::Ended;
    let detail = match phase {
        Phase::Working => plain(&s.headline().unwrap_or_else(|| t("arbeitet …", "working …").into())),
        Phase::Background => background_summary(s.background_tasks()),
        Phase::YourTurn => {
            let ago = ago(s.phase_since_ms(), now);
            tr!("fertig seit {ago}", "done for {ago}")
        }
        Phase::Ended => {
            let ago = ago_phrase(s.last_activity_ms, now);
            match brain.model.days_left(s, now) {
                Some(days) => tr!("{ago} · noch {days} T fortsetzbar", "{ago} · resumable for {days} more days"),
                None => ago,
            }
        }
        _ => ago_phrase(s.last_activity_ms, now),
    };
    // Ended rows are dimmed like the GPUI version's 60 % opacity.
    let dim = |c: Color| if ended { Color { a: 0.6, ..c } } else { c };
    let mut line = row![style::dot(dim(phase_color(phase)), 7.0), name_label(s, 13.0, dim(TEXT_STRONG)), account_badge(&s.key.account, brain.account_index(&s.key.account))]
        .spacing(9)
        .align_y(iced::Center);
    for m in markers(brain, s) {
        line = line.push(m);
    }
    line = line.push(text(clip(&detail, 60)).size(12).color(dim(TEXT_MUTED)).font(UI).wrapping(text::Wrapping::None));
    let content = container(row![
        if selected { rail(phase_color(phase), 8.0) } else { Space::new().width(4).into() },
        container(line).padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 }).center_y(Fill).width(Fill).clip(true),
    ])
    .height(app::ROW_H)
    .width(Fill)
    .clip(true)
    .style(move |_| boxed(if selected { RAISED } else if hovered { style::HOVER } else { Color::TRANSPARENT }, Color::TRANSPARENT, 8.0));
    clickable(content.into(), s)
}

/// Plan limits per account (from the status line) and today's cost, under the list.
fn usage(brain: &Brain, now: i64) -> Option<Element<'_, Message>> {
    let today = chrono::Local::now().date_naive();
    let rows: Vec<Element<'_, Message>> = brain
        .model
        .accounts
        .iter()
        .filter_map(|account| {
            let limits = brain_core::usage::account_limits(&brain.model.usage, &account.id);
            let cost: f64 = brain
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
            let mut line = row![account_badge(&account.id, brain.account_index(&account.id))].spacing(10).align_y(iced::Center);
            let history = brain.model.usage_history.get(&account.id).map(Vec::as_slice).unwrap_or_default();
            if let Some(snapshot) = limits {
                if let Some(limit) = snapshot.five_hour {
                    line = line.push(limit_bar("5h", limit, five_hour_runs_out(limit, now), now));
                }
                if let Some(limit) = snapshot.seven_day {
                    line = line.push(limit_bar("7d", limit, seven_day_runs_out(limit, history, now), now));
                }
            }
            // Running out at this pace while another account has room: say where to go.
            let runs_out = limits.is_some_and(|snapshot| {
                snapshot.five_hour.and_then(|l| five_hour_runs_out(l, now)).is_some()
                    || snapshot.seven_day.and_then(|l| seven_day_runs_out(l, history, now)).is_some()
            });
            let roomy = brain.model.accounts.iter().filter(|a| a.id != account.id).find(|other| {
                brain_core::usage::account_limits(&brain.model.usage, &other.id).is_some_and(|s| {
                    s.five_hour.is_none_or(|l| l.used_percentage < 50.0) && s.seven_day.is_none_or(|l| l.used_percentage < 50.0)
                })
            });
            if let (true, Some(other)) = (runs_out, roomy) {
                let other = other.id.clone();
                line = line.push(text(tr!("→ {other} hat Luft", "→ {other} has room")).size(11).color(DONE).font(UI));
            }
            if cost > 0.0 {
                // Claude Code's per-session totals at API prices: a measure of use, not a bill.
                line = line
                    .push(Space::new().width(Fill))
                    .push(text(tr!("≈ ${cost:.0} API-Wert", "≈ ${cost:.0} API value")).size(11.5).color(TEXT_MUTED).font(UI).wrapping(text::Wrapping::None));
            }
            Some(line.into())
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    Some(
        column![divider(), column![text(t("Nutzung", "Usage")).size(11).color(TEXT_FAINT).font(style::semibold())].extend(rows).spacing(8).padding([10, 16])]
            .into(),
    )
}

/// The five-hour limit runs out at the pace of its window so far.
fn five_hour_runs_out(limit: brain_core::usage::Limit, now_ms: i64) -> Option<i64> {
    limit.runs_out_at(brain_core::usage::FIVE_HOURS, now_ms / 1000)
}

/// The weekly limit runs out at the pace of the last 24 hours.
fn seven_day_runs_out(limit: brain_core::usage::Limit, history: &[brain_core::usage::Sample], now_ms: i64) -> Option<i64> {
    limit.runs_out_at_recent(brain_core::usage::SEVEN_DAYS, now_ms / 1000, history)
}

/// `5h ▓▓▓░░ 46% · 15:20`: a plan limit window with its reset time, and when it runs out at
/// the current pace (`runs_out`, Unix seconds) if that comes first.
fn limit_bar<'a>(label: &str, limit: brain_core::usage::Limit, runs_out: Option<i64>, now_ms: i64) -> Element<'a, Message> {
    let used = limit.used_percentage.clamp(0.0, 100.0);
    let color = if used >= 90.0 { CALLS } else if used >= 70.0 { TURN } else { WORKING };
    let resets = limit.resets_at.filter(|r| r * 1000 > now_ms).map(|r| {
        let at = chrono::DateTime::from_timestamp(r, 0).map(|t| t.with_timezone(&chrono::Local));
        match at {
            Some(t) if r * 1000 - now_ms < 24 * 3600 * 1000 => t.format("%H:%M").to_string(),
            Some(t) => t.format("%a").to_string(),
            None => String::new(),
        }
    });
    let filled = (46.0 * used as f32 / 100.0).max(0.0);
    let bar = stack![
        container(Space::new().width(46).height(5)).style(|_| boxed(RAISED, Color::TRANSPARENT, 3.0)),
        container(Space::new().width(filled).height(5)).style(move |_| boxed(color, Color::TRANSPARENT, 3.0)),
    ];
    let mut out = row![text(label.to_string()).size(11).color(TEXT_FAINT).font(UI), bar, text(format!("{used:.0}%")).size(11).color(TEXT).font(UI)]
        .spacing(5)
        .align_y(iced::Center);
    if let Some(at) = resets {
        out = out.push(text(format!("↻ {at}")).size(11).color(TEXT_FAINT).font(UI));
    }
    // At this pace the limit runs out before it resets: when.
    if let Some(at) = runs_out.filter(|_| used < 100.0) {
        let when = chrono::DateTime::from_timestamp(at, 0).map(|t| t.with_timezone(&chrono::Local));
        let when = match when {
            Some(t) if at * 1000 - now_ms < 24 * 3600 * 1000 => t.format("%H:%M").to_string(),
            Some(t) => t.format("%a %H:%M").to_string(),
            None => String::new(),
        };
        out = out.push(text(tr!("voll ~{when}", "full ~{when}")).size(11).color(TURN).font(style::semibold()));
    }
    out.into()
}

// ---- detail -----------------------------------------------------------------------

fn detail(brain: &Brain, now: i64) -> Element<'_, Message> {
    let partner = brain.split.as_ref().filter(|k| Some(*k) != brain.selected.as_ref()).and_then(|k| brain.model.board.get(k));
    match partner {
        Some(other) => row![
            container(other_pane(brain, other)).width(Fill).height(Fill),
            container(Space::new().width(1).height(Fill)).style(|_| style::fill(LINE)),
            container(session_pane(brain, now)).width(Fill).height(Fill),
        ]
        .height(Fill)
        .into(),
        None => session_pane(brain, now),
    }
}

/// The split's other session: its header and its terminal. A click (or typing into it) makes
/// it the active one.
fn other_pane<'a>(brain: &'a Brain, s: &'a Session) -> Element<'a, Message> {
    let mut header = row![
        text(clip(&s.display_name(), 40)).size(18).color(TEXT_STRONG).font(style::bold()).wrapping(text::Wrapping::None),
        account_badge(&s.key.account, brain.account_index(&s.key.account)),
    ]
    .spacing(10)
    .align_y(iced::Center);
    for m in markers(brain, s) {
        header = header.push(m);
    }
    header = header
        .push(Space::new().width(Fill))
        .push(button(text("✕").size(13).color(TEXT_MUTED)).padding([4, 8]).on_press(Message::ToggleSplit).style(|_, _| button::Style::default()));
    let head = mouse_area(column![header, subtitle(brain, s)].spacing(10))
        .on_press(Message::ActivatePane)
        .interaction(iced::mouse::Interaction::Pointer);
    column![head, terminal_or_hint(brain, s)]
        .spacing(14)
        .padding(Padding { top: 20.0, right: 20.0, bottom: 0.0, left: 28.0 })
        .height(Fill)
        .into()
}

/// The selected session: header, details, tabs and the tab's content.
fn session_pane(brain: &Brain, now: i64) -> Element<'_, Message> {
    let Some(s) = brain.selected_session() else {
        return container(
            column![
                text(t("Keine Session ausgewählt", "No session selected")).size(13).color(TEXT).font(UI),
                text(t("Wähle links eine Session aus oder starte eine neue im Terminal.", "Pick a session on the left, or start a new one in your terminal."))
                    .size(12.5)
                    .color(TEXT_MUTED)
                    .font(UI),
            ]
            .spacing(6)
            .align_x(iced::Center),
        )
        .center(Fill)
        .into();
    };
    let phase = s.phase();
    let caps = brain.capabilities();
    // One verb for getting to the session wherever it runs; never the loud button: when the
    // session waits, the callout's Reply or Allow is the next step.
    let primary = if phase == Phase::Ended {
        action(t("Fortsetzen", "Resume"), "⏎", false, Some(Message::Do(Action::Resume)))
    } else if s.agent.is_some() {
        action(t("Öffnen", "Open"), "⏎", false, Some(Message::Do(Action::Open)))
    } else {
        action(t("Öffnen", "Open"), "⏎", false, caps.focus.then_some(Message::Do(Action::Open)))
    };

    let mut header = row![name(brain, s), account_badge(&s.key.account, brain.account_index(&s.key.account))].spacing(10).align_y(iced::Center);
    for m in markers(brain, s) {
        header = header.push(m);
    }
    header = header.push(Space::new().width(Fill)).push(primary);

    let content: Element<'_, Message> = match brain.tab {
        DetailTab::Messages | DetailTab::Timeline | DetailTab::Questions => messages(brain, s),
        DetailTab::Changes => column![files_bar(brain, s), changes(brain, s)].spacing(10).height(Fill).into(),
        DetailTab::Files => with_reader(brain, files(brain, s)),
        DetailTab::Konsil => scrollable(crate::konsil::view(brain, &s.key)).height(Fill).style(scrollbar).into(),
        DetailTab::Terminal => terminal(brain, s),
    };
    column![
        header,
        subtitle(brain, s),
        callout(brain, s, now, caps),
        tabs(brain, s),
        content,
    ]
    .spacing(14)
    .padding(Padding { top: 20.0, right: 28.0, bottom: 0.0, left: 28.0 })
    .height(Fill)
    .into()
}

fn name<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    if brain.mode == Mode::Rename {
        return text_input("", &brain.rename)
            .id(app::RENAME)
            .on_input(Message::Rename)
            .on_submit(Message::RenameSubmit)
            .size(20)
            .font(style::bold())
            .padding([1, 8])
            .width(Length::Fixed(420.0))
            .style(field_style)
            .into();
    }
    mouse_area(row![text(clip(&s.display_name(), 48)).size(20).color(TEXT_STRONG).font(style::bold()).wrapping(text::Wrapping::None), text("✎").size(13).color(TEXT_FAINT)].spacing(8).align_y(iced::Center))
        .on_press(Message::Do(Action::StartRename))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// How full the context is, as a bar: amber from 80 %, red from 90 %, with a button that
/// sends `/compact` once it's getting full.
fn context_gauge<'a>(brain: &'a Brain, s: &Session, percent: f64) -> Element<'a, Message> {
    const WIDTH: f32 = 72.0;
    let color = if percent >= 90.0 {
        CALLS
    } else if percent >= 80.0 {
        TURN
    } else {
        WORKING
    };
    let filled = (WIDTH * (percent.clamp(0.0, 100.0) / 100.0) as f32).max(2.0);
    let bar = container(container(Space::new().width(filled).height(5)).style(move |_| boxed(color, Color::TRANSPARENT, 2.5)))
        .width(WIDTH)
        .height(5)
        .style(|_| boxed(RAISED, Color::TRANSPARENT, 2.5));
    let mut line = row![bar, text(tr!("{percent:.0}% Kontext", "{percent:.0}% context")).size(11.5).color(if percent >= 80.0 { color } else { TEXT_MUTED }).font(UI)]
        .spacing(7)
        .align_y(iced::Center);
    if percent >= 80.0 && brain.can_send(&s.key) {
        line = line.push(style::text_button("/compact", false, Message::SendTo(s.key.clone(), "/compact".to_string())));
    }
    line.into()
}

/// One quiet line under the name: where it runs, the model, how full the context is, what to
/// know (a permission mode, days left to resume, the pull request). Cost, pid and start time
/// on hover: they are for looking up, not for deciding.
fn subtitle<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    if brain.mode == Mode::Rename {
        return text(t("⏎ schickt /rename an die Session, esc bricht ab", "⏎ sends /rename to the session, esc cancels")).size(12).color(TEXT_MUTED).font(UI).into();
    }
    let info = &s.insight;
    let status = brain.model.snapshot(&s.key);
    let quiet = |label: String| -> Element<'a, Message> { text(label).size(12).color(TEXT_MUTED).font(UI).into() };
    let mut parts: Vec<Element<'a, Message>> = vec![text(s.cwd.as_deref().map(tilde).unwrap_or_default()).size(11.5).color(TEXT_MUTED).font(MONO).into()];
    if let Some(model) = &info.model {
        parts.push(quiet(short_model(model)));
    }
    // The status line knows the real fill level; the transcript only the token count.
    if let Some(percent) = status.and_then(|s| s.context_percent) {
        parts.push(context_gauge(brain, s, percent));
    } else if let Some(tokens) = info.context_tokens {
        let tokens = format_tokens(tokens);
        parts.push(quiet(tr!("{tokens} Kontext", "{tokens} context")));
    }
    if let Some(mode) = info.permission_mode.as_deref().filter(|m| *m != "default") {
        parts.push(text(mode.to_string()).size(12).color(TURN).font(UI).into());
    }
    if !s.alive {
        if let Some(days) = brain.model.days_left(s, now_ms()) {
            parts.push(quiet(tr!("noch {days} Tage fortsetzbar", "resumable for {days} more days")));
        }
    }
    let mut line = row![].spacing(8).align_y(iced::Center);
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            line = line.push(text("·").size(12).color(TEXT_FAINT));
        }
        line = line.push(part);
    }
    if let Some(pr) = brain.prs.get(&s.key) {
        line = line.push(pr_chip(pr));
    }
    let mut details = Vec::new();
    if let Some(cost) = status.and_then(|s| s.cost_usd).or(info.cost_usd) {
        details.push(format!("${cost:.2}"));
    }
    if s.alive {
        details.push(format!("pid {}", s.pid));
    }
    if let Some(started) = s.started_ms {
        let at = clock(started);
        details.push(tr!("seit {at}", "since {at}"));
    }
    if details.is_empty() {
        return line.into();
    }
    let card = container(text(details.join("  ·  ")).size(12).color(TEXT).font(UI)).padding([5, 9]).style(|_| boxed(RAISED, LINE_STRONG, 6.0));
    iced::widget::tooltip(line, card, iced::widget::tooltip::Position::Bottom).gap(6).into()
}

/// The state box: what the session waits for, plus the actions that fit.
fn callout<'a>(brain: &'a Brain, s: &'a Session, now: i64, caps: brain_terminal::Capabilities) -> Element<'a, Message> {
    let phase = s.phase();
    let color = phase_color(phase);
    let last_reply = brain
        .conversation
        .as_ref()
        .filter(|c| c.key == s.key)
        .and_then(|c| c.messages.iter().rev().find(|m| m.role == Role::Assistant))
        .map(|m| brain_core::hook::one_line(&m.text, 320));
    let headline = plain(&s.headline().or(last_reply).unwrap_or_else(|| t("Noch keine Meldung von dieser Session.", "No message from this session yet.").into()));
    let since = ago(s.phase_since_ms(), now);

    let mut body = column![
        row![
            style::dot(color, 7.0),
            text(phase_label(phase)).size(12).color(color).font(style::semibold()),
            text(tr!("seit {since}", "for {since}")).size(12).color(TEXT_MUTED).font(UI),
        ]
        .spacing(8)
        .align_y(iced::Center),
        text(clip(&headline, 420)).size(13).color(TEXT_STRONG).font(UI).line_height(1.5),
    ]
    .spacing(8);
    for line in conflict_lines(brain, s) {
        body = body.push(line);
    }
    if phase == Phase::Background {
        body = body.push(text(background_summary(s.background_tasks())).size(12).color(TEXT_MUTED).font(UI));
        for task in s.background_tasks() {
            let what = task.description.clone().unwrap_or_else(|| task.id.clone());
            let kind = task.agent_type.clone().unwrap_or_else(|| task.kind.clone());
            body = body.push(row![text(kind).size(12).color(style::BACKGROUND).font(UI), text(clip(&what, 90)).size(12).color(TEXT).font(UI)].spacing(8));
        }
    }
    if brain.mode == Mode::Reply {
        let field = text_input(t("Antwort an die Session …", "Reply to the session …"), &brain.reply)
            .id(app::REPLY)
            .on_input(Message::Reply)
            .on_submit(Message::ReplySubmit)
            .size(13)
            .font(UI)
            .padding([7, 10])
            .style(field_style);
        let hint = if brain.reply.is_empty() { t("1–9 übernimmt · ⏎ senden · esc", "1–9 fills in · ⏎ send · esc") } else { t("⏎ senden · esc", "⏎ send · esc") };
        body = body.push(row![field, text(hint).size(11).color(TEXT_MUTED).font(UI)].spacing(8).align_y(iced::Center));
        if brain.reply.is_empty() {
            let quick = config::quick_replies().into_iter().enumerate().map(|(i, reply)| {
                container(row![kbd(format!("{}", i + 1)), text(reply).size(12).color(TEXT).font(UI)].spacing(6).align_y(iced::Center))
                    .padding([3, 8])
                    .style(|_| boxed(SURFACE, LINE, 6.0))
                    .into()
            });
            body = body.push(column(quick).spacing(4));
        }
    }
    let mut actions: Vec<Element<'a, Message>> = Vec::new();
    let questions = brain.open_questions();
    if !questions.is_empty() {
        for q in questions {
            body = body.push(text(q.question.clone()).size(13).color(TEXT_STRONG).font(style::semibold()));
            for (i, option) in q.options.iter().enumerate() {
                let description = if option.description.is_empty() { String::new() } else { format!(" – {}", option.description) };
                body = body.push(text(format!("{}. {}{description}", i + 1, option.label)).size(12.5).color(TEXT).font(UI));
            }
        }
        actions.push(action(t("Zur Session", "Go to session"), "⏎", true, Some(Message::Do(Action::Open))));
    } else if s.awaiting_permission() {
        actions.push(action(t("Erlauben", "Allow"), "Y", true, caps.keys.then_some(Message::Do(Action::Allow))));
        actions.push(action(t("Ablehnen", "Deny"), "N", false, caps.keys.then_some(Message::Do(Action::Deny))));
    } else if s.accepts_input() && brain.mode != Mode::Reply {
        actions.push(action(t("Antworten", "Reply"), "T", true, caps.type_text.then_some(Message::Do(Action::StartReply))));
    }
    if !actions.is_empty() {
        body = body.push(row(actions).spacing(8));
        let blocked = !brain.terminals.contains_key(&s.key) && if s.awaiting_permission() { !caps.keys } else { !caps.type_text };
        if let Some(limit) = brain.typing_limit().filter(|_| blocked) {
            body = body.push(text(limit_help(limit)).size(12).color(TEXT_MUTED).font(UI).line_height(1.4));
        }
    }
    container(row![rail(color, 10.0), container(body).padding(Padding { top: 12.0, right: 14.0, bottom: 12.0, left: 12.0 }).width(Fill)].height(Length::Shrink))
        .width(Fill)
        .clip(true)
        .style(move |_| boxed(alpha(color, 0x14), alpha(color, 0x3a), 10.0))
        .into()
}

/// What to change so Brain can type into the terminal.
fn limit_help(limit: &brain_terminal::Limit) -> String {
    use brain_terminal::Limit;
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

/// "Edits the same files as X: a.rs, b.rs" and "Works in the same checkout as Y".
fn conflict_lines<'a>(brain: &'a Brain, s: &Session) -> Vec<Element<'a, Message>> {
    let Some(conflict) = brain.conflicts.get(&s.key) else { return Vec::new() };
    let name = |key| brain.model.board.get(key).map(|o: &Session| o.display_name()).unwrap_or_default();
    let mut out = Vec::new();
    for (other, files) in &conflict.shared_files {
        let other = name(other);
        let list = files.iter().map(|f| f.rsplit('/').next().unwrap_or(f)).collect::<Vec<_>>().join(", ");
        out.push(text(tr!("⚠ Bearbeitet dieselben Dateien wie {other}: {list}", "⚠ Edits the same files as {other}: {list}")).size(12).color(CALLS_SOFT).font(UI).into());
    }
    if !conflict.same_checkout.is_empty() {
        let others = conflict.same_checkout.iter().map(name).collect::<Vec<_>>().join(", ");
        out.push(text(tr!("Ändert Dateien im selben Checkout wie {others}", "Changes files in the same checkout as {others}")).size(12).color(TURN).font(UI).into());
    }
    out
}

fn tabs<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    let message_count = brain.conversation.as_ref().map_or(0, |c| c.messages.len());
    let file_count = brain.files.as_ref().filter(|f| f.key == s.key).map(|f| f.files.len());
    let konsil_count = brain.konsil.as_ref().filter(|k| k.key == s.key).map_or(0, |k| k.councils.len());
    let mut tabs = vec![(DetailTab::Messages, t("Nachrichten", "Messages"), Some(message_count)), (DetailTab::Files, t("Dateien", "Files"), file_count)];
    // Only sessions that held a council have the tab.
    if konsil_count > 0 || brain.tab == DetailTab::Konsil {
        tabs.push((DetailTab::Konsil, "Konsil", Some(konsil_count)));
    }
    tabs.push((DetailTab::Terminal, "Terminal", None));
    // The lenses count as Messages, the git changes as Files.
    let shown = match brain.tab {
        DetailTab::Timeline | DetailTab::Questions => DetailTab::Messages,
        DetailTab::Changes => DetailTab::Files,
        tab => tab,
    };
    let buttons = tabs.into_iter().map(|(tab, label, count)| {
        let active = shown == tab;
        let content = column![
            row![
                text(label).size(13).font(if active { style::semibold() } else { style::medium() }).color(if active { TEXT_STRONG } else { TEXT_MUTED }),
                text(count.map(|c| c.to_string()).unwrap_or_default()).size(11.5).color(TEXT_FAINT).font(UI),
            ]
            .spacing(6)
            .align_y(iced::Center),
            container(Space::new().width(Fill).height(2)).style(move |_| style::fill(if active { WORKING } else { Color::TRANSPARENT })),
        ]
        .spacing(7)
        .width(Length::Shrink);
        mouse_area(content).on_press(Message::Tab(tab)).interaction(iced::mouse::Interaction::Pointer).into()
    });
    column![row(buttons).spacing(18), divider()].into()
}

/// A real terminal running the session inside Brain, or why there is none.
/// The Terminal tab; split (⌘D), the kept session's terminal beside it; an open file shows
/// in the reader (also once the terminal ended).
fn terminal<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    let main = terminal_or_hint(brain, s);
    let main = match brain.shells.get(&s.key).filter(|_| brain.shell_visible) {
        Some(shell) => column![container(main).height(Length::FillPortion(3)), shell_pane(brain, s, shell)].spacing(8).height(Fill).into(),
        None => main,
    };
    with_reader(brain, main)
}

/// ⌘T: the session's shell, below its Claude terminal.
fn shell_pane<'a>(brain: &'a Brain, s: &Session, shell: &'a iced_term::Terminal) -> Element<'a, Message> {
    let focused = brain.focused_shell.as_ref() == Some(&s.key);
    let edge = if focused { alpha(WORKING, 0x88) } else { LINE };
    let head = row![
        text(t("Shell", "Shell")).size(12).color(TEXT_MUTED).font(style::semibold()),
        text(s.cwd.as_deref().map(tilde).unwrap_or_default()).size(11).color(TEXT_FAINT).font(MONO),
        Space::new().width(Fill),
        kbd("⌘T"),
        text(t("ausblenden", "hide")).size(11).color(TEXT_FAINT).font(UI),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let screen = container(iced_term::TerminalView::show(shell).map(Message::Term)).padding([10, 14]).width(Fill).height(Fill).style(move |_| boxed(INK, edge, 10.0));
    column![head, screen].spacing(6).height(Length::FillPortion(2)).padding(Padding { bottom: 12.0, ..Padding::ZERO }).into()
}

/// ⌘⇧A: every session that runs in Brain as a live preview; a click opens it.
fn overview(brain: &Brain) -> Element<'_, Message> {
    let mut keys: Vec<&brain_core::state::SessionKey> = brain.terminals.keys().collect();
    keys.sort_by_key(|k| brain.model.board.get(k).map(|s| s.display_name().to_lowercase()));
    let per_row = if keys.len() <= 4 { 2 } else { 3 };
    let tiles: Vec<Element<'_, Message>> = keys
        .into_iter()
        .filter_map(|key| {
            let term = brain.terminals.get(key)?;
            let s = brain.model.board.get(key)?;
            let phase = s.phase();
            let mut head = row![style::dot(phase_color(phase), 8.0), text(clip(&s.display_name(), 40)).size(12.5).color(TEXT_STRONG).font(style::semibold())].spacing(8).align_y(iced::Center);
            if let Some(title) = brain.terminal_titles.get(key) {
                head = head.push(text(clip(title, 40)).size(11).color(TEXT_FAINT).font(UI));
            }
            let screen = container(iced_term::TerminalPreview::show(term)).padding(8).width(Fill).height(Fill).style(|_| boxed(INK, LINE, 8.0));
            let tile = container(column![head, screen].spacing(6)).padding(8).width(Fill).height(260).style(|_| boxed(SURFACE, LINE, 10.0));
            Some(mouse_area(tile).on_press(Message::OpenFromOverview(key.clone())).interaction(iced::mouse::Interaction::Pointer).into())
        })
        .collect();
    let mut rows = column![row![
        text(t("Alle Terminals", "All terminals")).size(15).color(TEXT_STRONG).font(style::semibold()),
        Space::new().width(Fill),
        kbd("⌘⇧A"),
        text(t("schließen", "close")).size(11).color(TEXT_FAINT).font(UI),
    ]
    .spacing(8)
    .align_y(iced::Center)]
    .spacing(12);
    let mut tiles = tiles.into_iter().peekable();
    while tiles.peek().is_some() {
        let mut line = row![].spacing(12);
        for _ in 0..per_row {
            line = line.push(match tiles.next() {
                Some(tile) => tile,
                None => Space::new().width(Fill).into(),
            });
        }
        rows = rows.push(line);
    }
    scrollable(rows.padding(Padding { top: 16.0, right: 20.0, bottom: 20.0, left: 20.0 })).height(Fill).style(scrollbar).into()
}

/// `main` with the reader beside it when a file is open.
fn with_reader<'a>(brain: &'a Brain, main: Element<'a, Message>) -> Element<'a, Message> {
    match &brain.reader {
        Some(reader) => row![container(main).width(Length::FillPortion(11)).height(Fill), reader_pane(reader)].spacing(12).height(Fill).into(),
        None => main,
    }
}

/// Every file the session wrote, edited, read or was given, newest first; a click opens it.
fn files<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    use brain_core::files::Touch;
    let Some(cache) = brain.files.as_ref().filter(|f| f.key == s.key) else {
        let why = if brain.conversation.as_ref().and_then(|c| c.transcript()).is_some() {
            t("Lade Dateien …", "Loading files …")
        } else {
            t("Zu dieser Session gibt es kein Transkript.", "This session has no transcript.")
        };
        return hint(why);
    };
    let filter = brain.file_filter;
    let bar = files_bar(brain, s);
    let now = now_ms();
    let cwd = s.cwd.clone().unwrap_or_default();
    let rows: Vec<Element<'a, Message>> = cache
        .files
        .iter()
        .filter(|file| shows(filter, file))
        .map(|file| {
            let path = std::path::Path::new(&file.path);
            let exists = path.exists();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| file.path.clone());
            // Inside the session's folder the path reads relative to it, elsewhere from ~.
            let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
            let folder = match folder.strip_prefix(&cwd) {
                Some(rest) if !cwd.is_empty() => format!(".{rest}"),
                _ => tilde(&folder),
            };
            let mut badges = row![].spacing(4);
            for touch in &file.touches {
                let (label, color) = match touch {
                    Touch::Written => (t("geschrieben", "written"), DONE),
                    Touch::Edited => (t("geändert", "edited"), TURN),
                    Touch::Read => (t("gelesen", "read"), TEXT_FAINT),
                    Touch::Attached => (t("angehängt", "attached"), style::BACKGROUND),
                    Touch::Shared => (t("von dir", "from you"), WORKING),
                };
                badges = badges.push(marker(label, color));
            }
            if !exists {
                badges = badges.push(marker(t("gelöscht", "deleted"), CALLS));
            }
            let line = row![
                column![
                    text(name).size(13).color(if exists { TEXT_STRONG } else { TEXT_FAINT }).font(style::medium()).wrapping(text::Wrapping::None),
                    text(clip(&folder, 70)).size(11).color(TEXT_FAINT).font(MONO).wrapping(text::Wrapping::None),
                ]
                .spacing(2)
                .width(Fill),
                badges,
                text(if file.last_ms > 0 { ago(file.last_ms, now) } else { String::new() }).size(11.5).color(TEXT_FAINT).font(UI),
            ]
            .spacing(10)
            .align_y(iced::Center)
            .padding([6, 8]);
            button(line)
                .padding(0)
                .width(Fill)
                .on_press_maybe(exists.then(|| Message::OpenFile(file.path.clone())))
                .style(|_, status| button::Style {
                    background: matches!(status, button::Status::Hovered).then_some(Background::Color(style::HOVER)),
                    border: Border { radius: 6.0.into(), ..Border::default() },
                    ..button::Style::default()
                })
                .into()
        })
        .collect();
    let list: Element<'a, Message> = if rows.is_empty() {
        hint(t("Keine Dateien in dieser Auswahl.", "No files in this view."))
    } else {
        scrollable(column(rows).spacing(1).padding(Padding { top: 0.0, right: 8.0, bottom: 20.0, left: 0.0 })).height(Fill).style(scrollbar).into()
    };
    column![container(bar).padding(Padding { top: 4.0, bottom: 8.0, ..Padding::ZERO }), list].height(Fill).into()
}

/// Claude's questions, newest first, each with the answer given (or "open").
fn questions<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    use brain_core::qa::Asked;
    let Some(cache) = brain.qa.as_ref().filter(|q| q.key == s.key) else {
        let why = if brain.conversation.as_ref().and_then(|c| c.transcript()).is_some() {
            t("Lade Fragen …", "Loading questions …")
        } else {
            t("Zu dieser Session gibt es kein Transkript.", "This session has no transcript.")
        };
        return hint(why);
    };
    if cache.exchanges.is_empty() {
        return hint(t("Claude hat in dieser Session nichts gefragt.", "Claude asked nothing in this session."));
    }
    let now = now_ms();
    let cards = cache.exchanges.iter().rev().map(|exchange| -> Element<'a, Message> {
        let when = exchange.ts.map(|ts| ago(ts.timestamp_millis(), now)).unwrap_or_default();
        let how = match exchange.asked {
            Asked::Tool => t("Rückfrage", "asked"),
            Asked::Text => t("im Text", "in the reply"),
        };
        let mut card = column![
            row![text(how).size(11).color(TEXT_FAINT).font(UI), Space::new().width(Fill), text(when).size(11).color(TEXT_FAINT).font(UI)].align_y(iced::Center),
            text(exchange.question.clone()).size(13).color(TEXT_STRONG).font(style::semibold()),
        ]
        .spacing(6);
        if !exchange.options.is_empty() {
            card = card.push(text(exchange.options.join(" · ")).size(11.5).color(TEXT_FAINT).font(UI));
        }
        card = card.push(match &exchange.answer {
            Some(answer) => row![
                text(t("Du:", "You:")).size(12.5).color(WORKING).font(style::semibold()),
                text(clip(answer, 600)).size(12.5).color(TEXT).font(UI),
            ]
            .spacing(6),
            None => row![text(t("noch offen", "still open")).size(12.5).color(TURN).font(style::semibold())],
        });
        container(card).padding([10, 12]).width(Fill).style(|_| boxed(SURFACE, LINE, 8.0)).into()
    });
    scrollable(column(cards).spacing(8).padding(Padding { top: 4.0, right: 12.0, bottom: 24.0, left: 0.0 })).height(Fill).style(scrollbar).into()
}

/// The Files tab's choice: the files the session touched, by how, or the folder's git changes.
fn files_bar<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    let on_changes = brain.tab == DetailTab::Changes;
    let cache = brain.files.as_ref().filter(|f| f.key == s.key);
    let count = |f: app::FileFilter| cache.map_or(0, |c| c.files.iter().filter(|file| shows(f, file)).count());
    let options = [
        (app::FileFilter::All, t("Alle", "All")),
        (app::FileFilter::Changed, t("Geschrieben", "Written")),
        (app::FileFilter::Read, t("Gelesen", "Read")),
        (app::FileFilter::Given, t("Mitgegeben", "Given")),
    ];
    let filters = segmented(options.into_iter().map(|(f, label)| (format!("{label} {}", count(f)), !on_changes && brain.file_filter == f, Message::FileFilter(f))).collect());
    let changed = brain.changes.as_ref().filter(|c| c.key == s.key).and_then(|c| c.changes.as_ref()).map(|c| c.files.len());
    let git = match changed {
        Some(n) => tr!("Git-Änderungen {n}", "Git changes {n}"),
        None => t("Git-Änderungen", "Git changes").to_string(),
    };
    row![filters, segmented(vec![(git, on_changes, Message::Tab(DetailTab::Changes))])].spacing(8).align_y(iced::Center).into()
}

fn shows(filter: app::FileFilter, file: &brain_core::files::SessionFile) -> bool {
    use brain_core::files::Touch;
    match filter {
        app::FileFilter::All => true,
        app::FileFilter::Changed => file.has(Touch::Written) || file.has(Touch::Edited),
        app::FileFilter::Read => file.has(Touch::Read),
        app::FileFilter::Given => file.has(Touch::Attached) || file.has(Touch::Shared),
    }
}

fn terminal_or_hint<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    if let Some(term) = brain.terminals.get(&s.key) {
        let focused = brain.focused_terminal.as_ref() == Some(&s.key);
        let hovering = brain.file_hover;
        let edge = if hovering { WORKING } else if focused { alpha(WORKING, 0x88) } else { LINE };
        let screen = container(iced_term::TerminalView::show(term).map(Message::Term))
            .padding([14, 18])
            .width(Fill)
            .height(Fill)
            .style(move |_| boxed(INK, edge, 10.0));
        let mut layers = stack![screen];
        if hovering {
            let name = s.display_name();
            layers = layers.push(
                container(
                    container(text(tr!("Loslassen fügt die Datei in „{name}“ ein", "Drop to add the file to “{name}”")).size(13).color(TEXT_STRONG).font(style::semibold()))
                        .padding([10, 16])
                        .style(|_| boxed(alpha(WORKING, 0xdd), Color::TRANSPARENT, 8.0)),
                )
                .center(Fill),
            );
        }
        return container(layers).padding(Padding { top: 6.0, bottom: 12.0, ..Padding::ZERO }).width(Fill).height(Fill).into();
    }
    let why = if brain.model.is_demo() {
        t("Im Demo-Modus startet Brain keine Terminals.", "In demo mode Brain starts no terminals.")
    } else if brain.terminal_command().is_none() {
        t(
            "Diese Session läuft in einem iTerm-Tab – dort kann sich Brain nicht einklinken. Hier laufen Hintergrund-Sessions (claude attach) und beendete Sessions (claude --resume).",
            "This session runs in an iTerm tab, which Brain can't join. Background sessions (claude attach) and ended sessions (claude --resume) run here.",
        )
    } else {
        let command = if s.agent.is_some() { "claude attach" } else { "claude --resume" };
        let mut body = column![].spacing(10);
        if let Some((_, brief)) = brain.brief.as_ref().filter(|(k, _)| *k == s.key) {
            body = body.push(brief_card(brief));
        }
        return scrollable(
            body.push(hint(tr!("Startet die Session hier mit {command}.", "Runs the session here with {command}.")))
                .push(action(t("Hier fortsetzen", "Resume here"), "⏎", false, Some(Message::Do(Action::StartTerminal))))
                .padding(Padding { top: 8.0, right: 12.0, bottom: 20.0, left: 0.0 }),
        )
        .height(Fill)
        .style(scrollbar)
        .into();
    };
    hint(why)
}

/// An ended session in a few lines: how it began and ended, an open question, what it changed.
fn brief_card<'a>(brief: &brain_core::brief::Brief) -> Element<'a, Message> {
    let now = now_ms();
    let line = |label: &str, value: String| -> Element<'a, Message> {
        row![
            container(text(label.to_string()).size(11.5).color(TEXT_FAINT).font(UI)).width(110),
            text(value).size(12.5).color(TEXT).font(UI).width(Fill),
        ]
        .spacing(8)
        .into()
    };
    let mut card = column![].spacing(8);
    if let Some(title) = &brief.title {
        card = card.push(text(title.clone()).size(14).color(TEXT_STRONG).font(style::semibold()));
    }
    let span = match (brief.started, brief.last) {
        (Some(start), Some(last)) => {
            let prompts = brief.prompts;
            let when = ago(last.timestamp_millis(), now);
            let since = start.with_timezone(&chrono::Local).format("%d.%m. %H:%M").to_string();
            tr!("{prompts} Prompts, seit {since} · zuletzt {when}", "{prompts} prompts since {since} · last {when}")
        }
        _ => {
            let prompts = brief.prompts;
            tr!("{prompts} Prompts", "{prompts} prompts")
        }
    };
    card = card.push(text(span).size(11.5).color(TEXT_MUTED).font(UI));
    if let Some(first) = &brief.first_prompt {
        card = card.push(line(t("Begonnen mit", "Began with"), clip(first, 220)));
    }
    if let Some(last) = brief.last_prompt.as_ref().filter(|l| Some(*l) != brief.first_prompt.as_ref()) {
        card = card.push(line(t("Zuletzt gefragt", "Last asked"), clip(last, 220)));
    }
    if let Some(reply) = &brief.last_reply {
        card = card.push(line(t("Letzte Antwort", "Last answer"), clip(reply, 320)));
    }
    if let Some(question) = &brief.open_question {
        card = card.push(row![
            container(text(t("Offen", "Open")).size(11.5).color(TURN).font(style::semibold())).width(110),
            text(clip(question, 260)).size(12.5).color(TEXT_STRONG).font(UI).width(Fill),
        ].spacing(8));
    }
    if !brief.changed.is_empty() {
        let names: Vec<String> = brief
            .changed
            .iter()
            .take(8)
            .map(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone()))
            .collect();
        let more = brief.changed.len().saturating_sub(8);
        let mut list = names.join(", ");
        if more > 0 {
            list.push_str(&tr!(" und {more} weitere", " and {more} more"));
        }
        card = card.push(line(t("Geändert", "Changed"), list));
    }
    container(card).padding([12, 14]).width(Fill).style(|_| boxed(SURFACE, LINE, 10.0)).into()
}

/// The file opened from a path in the terminal: Markdown rendered, code highlighted, images.
fn reader_pane(reader: &crate::app::Reader) -> Element<'_, Message> {
    use crate::app::ReaderKind;
    let name = reader.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let header = row![
        column![
            text(name).size(13).color(TEXT_STRONG).font(style::semibold()),
            text(clip(&tilde(&reader.path.parent().map(|p| p.display().to_string()).unwrap_or_default()), 48)).size(11).color(TEXT_FAINT).font(MONO).wrapping(text::Wrapping::None),
        ]
        .spacing(2)
        .width(Fill),
        action(t("Im Editor", "In editor"), "⌘⏎", false, Some(Message::ReaderToEditor)),
        button(text("✕").size(13).color(TEXT_MUTED)).padding([4, 8]).on_press(Message::CloseReader).style(|_, _| button::Style::default()),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let font = crate::chat_font::get();
    let body: Element<'_, Message> = match &reader.kind {
        ReaderKind::Code(content, extension) => iced::widget::text_editor(content)
            .highlight(extension, iced::highlighter::Theme::Base16Ocean)
            .font(font.regular)
            .size(font.size - 1.0)
            .height(Fill)
            .padding(10)
            .style(|_, _| iced::widget::text_editor::Style {
                background: Background::Color(INK),
                border: Border { color: LINE, width: 1.0, radius: 8.0.into() },
                placeholder: TEXT_FAINT,
                value: TEXT,
                selection: alpha(WORKING, 0x55),
            })
            .into(),
        ReaderKind::Image(handle) => scrollable(iced::widget::image(handle.clone()).width(Fill)).height(Fill).style(scrollbar).into(),
        ReaderKind::Unreadable(why) => hint(why.clone()),
    };
    container(column![header, body].spacing(10))
        .padding(12)
        .width(Length::FillPortion(9))
        .height(Fill)
        .style(|_| boxed(style::CHROME, LINE, 10.0))
        .into()
}

/// The latest messages; with text in the find field, the matches in the whole session; with
/// "Your prompts", everything you asked.
fn messages<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    let body: Element<'a, Message> = if brain.tab == DetailTab::Questions {
        questions(brain, s)
    } else if brain.tab == DetailTab::Timeline {
        scrollable(column(markdown::timeline(s)).padding(Padding { top: 12.0, right: 8.0, bottom: 20.0, left: 0.0 })).height(Fill).style(scrollbar).into()
    } else if brain.find.trim().is_empty() && !brain.prompts_only {
        let list = brain.conversation.as_ref().map(|c| c.messages.as_slice()).unwrap_or_default();
        let children = if list.is_empty() { vec![hint(t("Diese Session hat noch keine Nachrichten.", "This session has no messages yet."))] } else { markdown::conversation(list, Message::CopyText) };
        let list = scrollable(column(children).spacing(14).padding(Padding { top: 4.0, right: 12.0, bottom: 24.0, left: 0.0 }))
            .id(app::MESSAGES)
            .on_scroll(Message::MessagesScrolled)
            .height(Fill)
            .style(scrollbar);
        let to_end: Element<'_, Message> = if brain.messages_at_bottom {
            Space::new().into()
        } else {
            container(style::text_button(t("↓ Ans Ende", "↓ To the end"), true, Message::MessagesToEnd))
                .width(Fill)
                .height(Fill)
                .align_right(Fill)
                .align_bottom(Fill)
                .padding(Padding { bottom: 12.0, right: 24.0, ..Padding::ZERO })
                .into()
        };
        stack![list, to_end].into()
    } else {
        whole_session(brain)
    };
    let lens_open = brain.tab != DetailTab::Messages || brain.prompts_only || !brain.find.is_empty();
    if !brain.find_open && !lens_open {
        return body;
    }
    column![messages_bar(brain), body].spacing(8).height(Fill).into()
}

/// ⌘F: find in the whole session, and the lenses on it: the conversation, only your prompts,
/// Claude's questions with your answers, the session's events.
fn messages_bar(brain: &Brain) -> Element<'_, Message> {
    let field = text_input(t("In der ganzen Session suchen", "Find in the whole session"), &brain.find)
        .id(app::FIND)
        .on_input(Message::Find)
        .size(12.5)
        .font(UI)
        .padding([6, 2])
        .style(|theme, status| text_input::Style { border: Border::default(), background: Background::Color(Color::TRANSPARENT), ..field_style(theme, status) });
    let active = !brain.find.is_empty() || brain.prompts_only;
    let find = container(row![text("⌕").size(13).color(TEXT_FAINT), field, kbd(if active { "esc" } else { "⌘F" })].spacing(6).align_y(iced::Center))
        .padding([0, 10])
        .width(Fill)
        .style(move |_| boxed(INK, if active { alpha(WORKING, 0x99) } else { LINE }, 8.0));
    use app::Lens;
    let lens = match brain.tab {
        DetailTab::Questions => Lens::Questions,
        DetailTab::Timeline => Lens::Events,
        _ if brain.prompts_only => Lens::Prompts,
        _ => Lens::Conversation,
    };
    let view = segmented(
        [
            (Lens::Conversation, t("Gespräch", "Conversation")),
            (Lens::Prompts, t("Deine Prompts", "Your prompts")),
            (Lens::Questions, t("Fragen", "Questions")),
            (Lens::Events, t("Ereignisse", "Events")),
        ]
        .into_iter()
        .map(|(l, label)| (label.to_string(), lens == l, Message::Lens(l)))
        .collect(),
    );
    row![find, view]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// Matches or prompts from the whole conversation; a click opens one fully.
fn whole_session(brain: &Brain) -> Element<'_, Message> {
    use brain_core::transcript::Role;
    let Some(history) = brain.history.as_ref().filter(|h| Some(&h.key) == brain.selected.as_ref()) else {
        let why = if brain.conversation.as_ref().and_then(|c| c.transcript()).is_some() {
            t("Lade die ganze Session …", "Loading the whole session …")
        } else {
            t("Zu dieser Session gibt es kein Transkript.", "This session has no transcript.")
        };
        return hint(why);
    };
    let messages = &history.messages;
    let query = brain.find.trim();
    let mut indices: Vec<usize> = if query.is_empty() { (0..messages.len()).collect() } else { brain_core::history::find(messages, query) };
    if brain.prompts_only {
        indices.retain(|&i| messages[i].role == Role::User);
    }
    let now = now_ms();
    let found = indices.len();
    let summary = match (brain.prompts_only, query.is_empty()) {
        (true, true) => tr!("{found} Prompts", "{found} prompts"),
        _ => tr!("{found} Treffer", "{found} matches"),
    };
    let mut rows: Vec<Element<'_, Message>> = vec![text(summary).size(11.5).color(TEXT_FAINT).font(UI).into()];
    for i in indices.into_iter().rev() {
        let message = &messages[i];
        let (who, color) = match message.role {
            Role::User => (t("Du", "You"), WORKING),
            Role::Assistant => ("Claude", TEXT),
            Role::Tool => (message.tool.as_deref().unwrap_or("Tool"), TEXT_MUTED),
            Role::System => (t("Hinweis", "Note"), TEXT_FAINT),
        };
        let when = message.ts.map(|ts| ago(ts.timestamp_millis(), now)).unwrap_or_default();
        let head = row![
            text(who).size(12).color(color).font(style::semibold()),
            Space::new().width(Fill),
            text(when).size(11).color(TEXT_FAINT).font(UI),
        ]
        .align_y(iced::Center);
        let open = brain.expanded == Some(i);
        let content: Element<'_, Message> = if open {
            column![
                column(markdown::conversation(std::slice::from_ref(message), Message::CopyText)).spacing(8),
                row![action(t("Kopieren", "Copy"), "", false, Some(Message::CopyText(message.text.clone())))],
            ]
            .spacing(8)
            .into()
        } else {
            let (line, range) = brain_core::history::snippet(&message.text, query, 160);
            highlighted(line, range)
        };
        let card = button(column![head, content].spacing(4))
            .padding([8, 10])
            .width(Fill)
            .on_press(Message::Expand(if open { None } else { Some(i) }))
            .style(move |_, status| button::Style {
                background: Some(Background::Color(if open || matches!(status, button::Status::Hovered) { SURFACE } else { Color::TRANSPARENT })),
                border: Border { color: if open { LINE } else { Color::TRANSPARENT }, width: 1.0, radius: 8.0.into() },
                ..button::Style::default()
            });
        rows.push(card.into());
    }
    scrollable(column(rows).spacing(4).padding(Padding { top: 0.0, right: 12.0, bottom: 24.0, left: 0.0 })).height(Fill).style(scrollbar).into()
}

/// A line with the matched part marked.
fn highlighted<'a>(line: String, range: Option<std::ops::Range<usize>>) -> Element<'a, Message> {
    let Some(range) = range.filter(|r| line.is_char_boundary(r.start) && line.is_char_boundary(r.end)) else {
        return text(line).size(12.5).color(TEXT).font(UI).into();
    };
    let spans: Vec<iced::widget::text::Span<'a, ()>> = vec![
        iced::widget::span(line[..range.start].to_string()).color(TEXT),
        iced::widget::span(line[range.clone()].to_string()).color(TEXT_STRONG).background(alpha(TURN, 0x55)),
        iced::widget::span(line[range.end..].to_string()).color(TEXT),
    ];
    iced::widget::rich_text(spans).size(12.5).font(UI).into()
}

fn changes<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    let cache = brain.changes.as_ref().filter(|c| c.key == s.key);
    let body: Vec<Element<'a, Message>> = match cache.map(|c| c.changes.as_ref()) {
        None => vec![hint(t("Lade Änderungen …", "Loading changes …"))],
        Some(None) => vec![hint(t("Der Ordner dieser Session ist kein Git-Repository.", "This session's folder is not a git repository."))],
        Some(Some(changes)) => {
            let (added, removed) = changes.totals();
            let mut head = Vec::new();
            if let Some(branch) = &changes.branch {
                head.push(chip(format!("⑂ {branch}"), true));
            }
            if let Some((ahead, behind)) = changes.ahead_behind {
                head.push(chip(format!("↑{ahead} ↓{behind}"), false));
            }
            let files = changes.files.len();
            head.push(chip(tr!("{files} Dateien · +{added} −{removed}", "{files} files · +{added} −{removed}"), false));
            let mut out = vec![container(iced::widget::row(head).spacing(6).wrap()).padding(Padding { bottom: 6.0, ..Padding::ZERO }).into()];
            if changes.files.is_empty() {
                out.push(hint(t("Keine offenen Änderungen.", "No uncommitted changes.")));
            }
            for file in &changes.files {
                let open = brain.diff.as_ref().filter(|(k, p, _)| *k == s.key && *p == file.path);
                out.push(
                    button(change_row(file))
                        .padding([0, 6])
                        .width(Fill)
                        .on_press(Message::ShowDiff(file.path.clone()))
                        .style(move |_, status| button::Style {
                            background: (open.is_some() || matches!(status, button::Status::Hovered)).then_some(Background::Color(style::HOVER)),
                            border: Border { radius: 5.0.into(), ..Border::default() },
                            ..button::Style::default()
                        })
                        .into(),
                );
                if let Some((_, _, lines)) = open {
                    out.push(diff_view(file, lines.as_deref()));
                }
            }
            out
        }
    };
    scrollable(column(body).spacing(2).padding(Padding { top: 4.0, right: 8.0, bottom: 20.0, left: 0.0 })).height(Fill).style(scrollbar).into()
}

/// A file's diff, colored by line; long ones are cut. Below it, discarding the change.
fn diff_view<'a>(file: &brain_core::changes::FileChange, lines: Option<&[(brain_core::changes::DiffKind, String)]>) -> Element<'a, Message> {
    use brain_core::changes::DiffKind;
    const SHOWN: usize = 400;
    let Some(lines) = lines else { return container(hint(t("Lade Diff …", "Loading diff …"))).padding([4, 10]).into() };
    if lines.is_empty() {
        return container(hint(t("Kein Diff (binär oder unverändert).", "No diff (binary or unchanged)."))).padding([4, 10]).into();
    }
    let mut rows: Vec<Element<'a, Message>> = lines
        .iter()
        .take(SHOWN)
        .map(|(kind, line)| {
            let color = match kind {
                DiffKind::Added => DONE,
                DiffKind::Removed => CALLS_SOFT,
                DiffKind::Hunk => WORKING,
                DiffKind::Meta => TEXT_FAINT,
                DiffKind::Context => TEXT_MUTED,
            };
            let background = match kind {
                DiffKind::Added => Some(alpha(DONE, 0x14)),
                DiffKind::Removed => Some(alpha(CALLS, 0x14)),
                _ => None,
            };
            container(text(line.clone()).size(11.5).color(color).font(MONO).wrapping(text::Wrapping::None))
                .width(Fill)
                .padding([0, 8])
                .style(move |_| container::Style { background: background.map(Background::Color), ..container::Style::default() })
                .into()
        })
        .collect();
    if lines.len() > SHOWN {
        let more = lines.len() - SHOWN;
        rows.push(hint(tr!("… {more} weitere Zeilen", "… {more} more lines")));
    }
    let mut body = column![container(column(rows)).padding([6, 0]).width(Fill).clip(true).style(|_| boxed(INK, LINE, 6.0))].spacing(6);
    if brain_core::changes::discardable(file) {
        body = body.push(row![action(t("Änderung verwerfen", "Discard change"), "", false, Some(Message::DiscardChange(file.path.clone())))]);
    }
    container(body).padding(Padding { top: 2.0, right: 0.0, bottom: 10.0, left: 6.0 }).into()
}

/// One changed file: status code, path, added and removed lines.
fn change_row<'a>(file: &brain_core::changes::FileChange) -> Element<'a, Message> {
    let code = file.status.trim();
    let color = match code.chars().next() {
        Some('A') => DONE,
        Some('D') => CALLS,
        Some('R') => WORKING,
        Some('?') => TEXT_FAINT,
        _ => TURN,
    };
    let mut line = row![
        container(text(if code.is_empty() { "M".to_string() } else { code.to_string() }).size(12).color(color).font(MONO)).width(22),
        container(text(clip(&file.path, 110)).size(11.5).color(TEXT).font(MONO).wrapping(text::Wrapping::None)).width(Fill).clip(true),
    ]
    .spacing(10)
    .align_y(iced::Center)
    .padding([3, 0]);
    if let Some(a) = file.added.filter(|a| *a > 0) {
        line = line.push(text(format!("+{a}")).size(12).color(DONE).font(UI));
    }
    if let Some(r) = file.removed.filter(|r| *r > 0) {
        line = line.push(text(format!("−{r}")).size(12).color(CALLS).font(UI));
    }
    line.into()
}

fn pr_look(pr: &brain_core::github::PullRequest) -> (&'static str, Color) {
    use brain_core::github::Checks;
    match (pr.state.as_str(), pr.checks) {
        ("MERGED", _) => ("merged", TEXT_FAINT),
        ("CLOSED", _) => ("closed", TEXT_FAINT),
        (_, Checks::Failing) => ("✗", CALLS),
        (_, Checks::Pending) => ("…", TURN),
        (_, Checks::Passing) => ("✓", DONE),
        (_, Checks::None) => ("", TEXT_MUTED),
    }
}

/// A chip that opens the PR in the browser.
fn pr_chip<'a>(pr: &brain_core::github::PullRequest) -> Element<'a, Message> {
    let (symbol, color) = pr_look(pr);
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
    button(text(label).size(11.5).color(color).font(UI))
        .padding([2, 7])
        .on_press(Message::OpenUrl(pr.url.clone()))
        .style(move |_, _| button::Style {
            background: Some(Background::Color(alpha(color, 0x1c))),
            border: Border { color: alpha(color, 0x55), width: 1.0, radius: 5.0.into() },
            ..button::Style::default()
        })
        .into()
}

// ---- today ------------------------------------------------------------------------

fn today(brain: &Brain) -> Element<'_, Message> {
    let projects = brain.today_digest();
    let date = chrono::Local::now().format("%d.%m.%Y").to_string();
    let title = tr!("Heute, {date}", "Today, {date}");
    let mut body: Vec<Element<'_, Message>> = Vec::new();
    if projects.is_empty() {
        body.push(hint(t("Heute hat noch keine Session etwas als erledigt gemeldet.", "No session has reported anything as done today.")));
    }
    for project in &projects {
        body.push(container(text(project.project.clone()).size(15).color(TEXT_STRONG).font(style::semibold())).padding(Padding { top: 14.0, ..Padding::ZERO }).into());
        for session in &project.sessions {
            body.push(
                container(row![text(session.name.clone()).size(13).color(TEXT).font(style::semibold()), account_badge(&session.account, brain.account_index(&session.account))].spacing(8).align_y(iced::Center))
                    .padding(Padding { top: 6.0, ..Padding::ZERO })
                    .into(),
            );
            for entry in &session.entries {
                body.push(
                    row![
                        container(text(entry.at.format("%H:%M").to_string()).size(12.5).color(TEXT_FAINT).font(UI)).width(40),
                        container(text(plain(&entry.text)).size(12.5).color(TEXT).font(UI).line_height(1.45)).width(Fill),
                    ]
                    .spacing(10)
                    .into(),
                );
            }
        }
    }
    column![
        row![
            text(title).size(20).color(TEXT_STRONG).font(style::bold()),
            Space::new().width(Fill),
            action(t("Als Markdown kopieren", "Copy as Markdown"), "⌘C", false, (!projects.is_empty()).then_some(Message::Do(Action::CopyDigest))),
        ]
        .align_y(iced::Center),
        scrollable(column(body).spacing(2).padding(Padding { bottom: 24.0, right: 8.0, ..Padding::ZERO })).height(Fill).style(scrollbar),
    ]
    .spacing(10)
    .padding(Padding { top: 20.0, right: 28.0, bottom: 0.0, left: 28.0 })
    .into()
}

// ---- dialogs ----------------------------------------------------------------------

/// A dialog's title with a close button (esc does the same).
fn dialog_title<'a>(label: impl Into<String>) -> Element<'a, Message> {
    row![
        text(label.into()).size(20).color(TEXT_STRONG).font(style::bold()),
        Space::new().width(Fill),
        action(t("Schließen", "Close"), "esc", false, Some(Message::Close)),
    ]
    .align_y(iced::Center)
    .into()
}

fn new_session(brain: &Brain) -> Element<'_, Message> {
    let dialog = &brain.new_session;
    let chosen = dialog.chosen.as_ref();
    let asking = brain.pending_placeholder();
    let placeholder: String = match (&asking, chosen) {
        (Some(name), _) => tr!("Wert für {{{name}}} …", "Value for {{{name}}} …"),
        (None, Some(_)) => t("Ordner für die Vorlage suchen oder Pfad tippen …", "Search a folder for the template or type a path …").into(),
        (None, None) => t("Vorlage oder Ordner suchen, oder Pfad tippen …", "Search templates or folders, or type a path …").into(),
    };
    let title = match chosen {
        Some(c) => format!("{} · {}", t("Neue Session", "New session"), c.template.name),
        None => t("Neue Session", "New session").to_string(),
    };
    let accounts = brain.model.accounts.iter().enumerate().map(|(i, a)| (a.id.clone(), i == dialog.account, Message::NewAccount(i))).collect();
    let place = vec![
        (t("In Brain", "In Brain").to_string(), !dialog.in_iterm, Message::NewPlace(false)),
        (t("In iTerm", "In iTerm").to_string(), dialog.in_iterm, Message::NewPlace(true)),
    ];
    let mut content = column![
        dialog_title(title),
        row![
            text(t("Konto", "Account")).size(12.5).color(TEXT_MUTED).font(UI),
            segmented(accounts),
            Space::new().width(12),
            text(t("Läuft", "Runs")).size(12.5).color(TEXT_MUTED).font(UI),
            segmented(place),
        ]
        .spacing(10)
        .align_y(iced::Center),
    ]
    .spacing(14);
    if let Some(c) = chosen.filter(|c| !c.template.prompt.is_empty()) {
        let prompt: String = c.template.fill(&c.values).lines().take(6).collect::<Vec<_>>().join("\n");
        content = content.push(container(text(prompt).size(12.5).color(TEXT).font(UI).line_height(1.5)).padding([9, 12]).width(Fill).style(|_| boxed(SURFACE, LINE, 8.0)));
    }
    content = content.push(
        text_input(&placeholder, &dialog.folder)
            .id(app::NEW_SESSION)
            .on_input(Message::NewInput)
            .on_submit(Message::NewSubmit)
            .size(13)
            .font(UI)
            .padding([9, 12])
            .style(field_style),
    );
    let choices = brain.new_session_choices().into_iter().enumerate().map(|(i, choice)| {
        let active = i == dialog.pick;
        let label: Element<'_, Message> = match choice {
            Choice::Template(index) => {
                let template = &dialog.templates[index];
                let mut line = row![text("▸").size(12.5).color(TURN), text(template.name.clone()).size(12.5).color(if active { TEXT_STRONG } else { TEXT }).font(style::semibold())]
                    .spacing(10)
                    .align_y(iced::Center);
                if let Some(folder) = &template.folder {
                    line = line.push(text(folder.clone()).size(11.5).color(TEXT_FAINT).font(MONO));
                }
                line.into()
            }
            Choice::Folder(folder) => text(tilde(&folder)).size(12).color(if active { TEXT_STRONG } else { TEXT }).font(MONO).into(),
        };
        button(label).width(Fill).padding([6, 10]).on_press(Message::NewPick(i)).style(style::entry_style(active, Color::TRANSPARENT, RAISED, Color::TRANSPARENT, 6.0)).into()
    });
    content = content.push(scrollable(column(choices).spacing(2)).height(Fill).style(scrollbar));
    content = content.push(
        container(
            text(style::keys(t("⏎ wählen/starten · ↑↓ · ⇥ Konto, bei Pfad ergänzen · ⌘I Brain/iTerm · ⌘E Vorlage bearbeiten · ⌘⇧N neue Vorlage · esc", "⏎ pick/start · ↑↓ · ⇥ account, completes a path · ⌘I Brain/iTerm · ⌘E edit template · ⌘⇧N new template · esc")))
                .size(11.5)
                .color(TEXT_FAINT)
                .font(UI),
        )
        .padding(Padding { bottom: 16.0, ..Padding::ZERO }),
    );
    content.padding(Padding { top: 24.0, right: 28.0, bottom: 0.0, left: 28.0 }).into()
}

fn cleanup(brain: &Brain) -> Element<'_, Message> {
    let candidates = brain.cleanup_candidates();
    let now = now_ms();
    let count = candidates.len();
    let ages = CLEANUP_DAYS.iter().enumerate().map(|(i, days)| (tr!("{days} T", "{days} d"), i == brain.cleanup_age, Message::CleanupAge(i))).collect();
    let summary = if count == 0 {
        t("Nichts aufzuräumen. Angeheftete und gemerkte Sessions bleiben immer.", "Nothing to clean up. Pinned and saved sessions always stay.").to_string()
    } else {
        tr!("{count} beendete oder Hintergrund-Sessions. Angeheftete und gemerkte bleiben.", "{count} ended or background sessions. Pinned and saved ones stay.")
    };
    let rows = candidates.iter().map(|s| {
        let kind = match &s.agent {
            Some(agent) => {
                let state = &agent.state;
                tr!("Hintergrund · {state}", "background · {state}")
            }
            None => t("beendet", "ended").to_string(),
        };
        row![
            style::dot(phase_color(s.phase()), 7.0),
            container(name_label(s, 12.5, TEXT_STRONG)).width(Fill),
            account_badge(&s.key.account, brain.account_index(&s.key.account)),
            container(text(kind).size(12.5).color(TEXT_FAINT).font(UI)).width(220),
            container(text(ago_phrase(s.last_activity_ms, now)).size(12.5).color(TEXT_MUTED).font(UI)).width(140),
        ]
        .spacing(10)
        .align_y(iced::Center)
        .padding([5, 10])
        .into()
    });
    column![
        dialog_title(t("Aufräumen", "Clean up")),
        row![text(t("Ohne Aktivität seit", "Nothing happened for")).size(12.5).color(TEXT_MUTED).font(UI), segmented(ages)].spacing(10).align_y(iced::Center),
        text(summary).size(12.5).color(TEXT_MUTED).font(UI),
        scrollable(column(rows).spacing(2)).height(Fill).style(scrollbar),
        container(
            text(style::keys(t("H alle ausblenden · ⌘⌫ ⌘⌫ alle in den Papierkorb · ⇥ Alter · esc", "H hide all · ⌘⌫ ⌘⌫ move all to the Trash · ⇥ age · esc")))
                .size(11.5)
                .color(TEXT_FAINT)
                .font(UI),
        )
        .padding(Padding { bottom: 16.0, ..Padding::ZERO }),
    ]
    .spacing(14)
    .width(Fill)
    .padding(Padding { top: 24.0, right: 28.0, bottom: 0.0, left: 28.0 })
    .into()
}

/// "Go on …" (`A`, `F`, `I`, "Restart with …"): the account (each with how much of its limits is
/// used, so the one with room is easy to spot), whether the original keeps running, flags for the
/// new start, and in one sentence what ⏎ does with that.
fn relaunch(brain: &Brain) -> Element<'_, Message> {
    let Some(session) = brain.selected_session() else { return Space::new().into() };
    let name = session.display_name();
    let dialog = &brain.relaunch;
    let own = brain.account_index(&session.key.account);
    let rows = brain.model.accounts.iter().enumerate().map(|(index, account)| {
        let usage = brain_core::usage::account_limits(&brain.model.usage, &account.id).map(|s| {
            let part = |label: &str, limit: Option<brain_core::usage::Limit>| limit.map(|l| format!("{label} {:.0}%", l.used_percentage));
            [part("5h", s.five_hour), part("7d", s.seven_day)].into_iter().flatten().collect::<Vec<_>>().join("  ·  ")
        });
        let usage = usage.filter(|u| !u.is_empty()).unwrap_or_else(|| t("noch keine Nutzungsdaten", "no usage yet").to_string());
        let chosen = index == dialog.target;
        let mut line = row![kbd(format!("{}", index + 1)), account_badge(&account.id, index)].spacing(10).align_y(iced::Center);
        if index == own {
            line = line.push(text(t("ihr Konto", "its account")).size(12).color(TEXT_FAINT).font(UI));
        }
        let line = line.push(Space::new().width(Fill)).push(text(usage).size(12.5).color(TEXT_MUTED).font(UI)).padding([8, 12]);
        let edge = if chosen { WORKING } else { LINE };
        mouse_area(container(line).style(move |_| boxed(if chosen { RAISED } else { SURFACE }, edge, 8.0)))
            .on_press(Message::RelaunchTarget(index))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    });
    let original = segmented(vec![
        (t("Original beenden", "End the original").to_string(), !dialog.keep, Message::RelaunchKeep(false)),
        (t("Original weiterlaufen lassen (Fork)", "Keep the original running (fork)").to_string(), dialog.keep, Message::RelaunchKeep(true)),
    ]);
    let field = text_input(t("Flags, optional: --chrome  --model opus", "Flags, optional: --chrome  --model opus"), &dialog.flags)
        .id(crate::app::RELAUNCH_FLAGS)
        .on_input(Message::RelaunchFlags)
        .on_submit(Message::RelaunchSubmit)
        .padding([8, 12])
        .size(13.5)
        .font(style::MONO)
        .style(field_style);
    let target = brain.model.accounts.get(dialog.target).map(|a| a.id.clone()).unwrap_or_default();
    let (what, ready) = match brain.relaunch_choice() {
        Ok(app::Relaunch::Fork(_)) => (
            tr!(
                "Forkt in {target}: Eine neue Session macht mit dem Verlauf bis jetzt weiter, im Hintergrund und hier in Brain. Das Original läuft weiter.",
                "Forks into {target}: a new session goes on with the history so far, in the background and here in Brain. The original keeps running."
            ),
            true,
        ),
        Ok(app::Relaunch::Move(_)) => (
            tr!(
                "Zieht nach {target} um: Die Session läuft dort mit ihrem ganzen Verlauf weiter, das Original wird beendet.",
                "Moves to {target}: the session goes on there with all of its history, the original ends."
            ),
            true,
        ),
        Ok(app::Relaunch::Restart(flags)) => {
            let shown = flags.join(" ");
            (
                tr!(
                    "Startet neu mit {shown}: Claude nimmt Flags nur beim Start. Die Session endet und läuft weiter, wo sie lief.",
                    "Restarts with {shown}: Claude takes flags only at start. The session ends and goes on where it ran."
                ),
                true,
            )
        }
        Ok(app::Relaunch::TakeOver) => (
            t("Holt die Session aus iTerm in Brain: Das iTerm-Tab endet, die Session läuft hier im Hintergrund weiter.", "Moves the session from iTerm into Brain: its iTerm tab ends, the session goes on here in the background.").to_string(),
            true,
        ),
        Err(why) => (why, false),
    };
    let mut body = column![
        dialog_title(tr!("„{name}“ weiterführen", "Go on with “{name}”")),
        text(t("Konto (1, 2, …)", "Account (1, 2, …)")).size(12).color(TEXT_MUTED).font(style::semibold()),
        column(rows).spacing(6),
        row![original, text(t("⇥ wechselt", "⇥ switches")).size(11.5).color(TEXT_FAINT).font(UI)].spacing(10).align_y(iced::Center),
        field,
        text(what).size(13).line_height(1.5).color(if ready { TEXT } else { TURN }).font(UI),
    ]
    .spacing(12)
    .max_width(640);
    if let Some(err) = &dialog.error {
        body = body.push(text(err.clone()).size(12.5).color(CALLS).font(UI));
    }
    body = body.push(action(t("Weiterführen", "Go on"), "⏎", true, ready.then_some(Message::RelaunchSubmit)));
    body.padding(Padding { top: 24.0, right: 28.0, bottom: 20.0, left: 28.0 }).into()
}

/// A new Claude account from the existing ones: its name, what it gets from where, and where
/// the accounts differ.
fn account_setup(brain: &Brain) -> Element<'_, Message> {
    let dialog = &brain.account_setup;
    let sources: Vec<&str> = brain.model.accounts.iter().map(|a| a.id.as_str()).collect();
    let first = sources.first().copied().unwrap_or("main");
    let all = sources.join(" + ");
    let intro = tr!(
        "Brain legt ~/.claude-<name> an und übernimmt aus {all} Einstellungen, CLAUDE.md, Skills, Agents, Commands, Plugins, MCP-Server und das Gedächtnis pro Projekt. Wo sie sich unterscheiden, gewinnt {first}; bei Gedächtnis-Notizen die neuere. Keine Anmeldung, keine Gespräche: Melde dich in der ersten Session mit /login an.",
        "Brain makes ~/.claude-<name> with the settings, CLAUDE.md, skills, agents, commands, plugins, MCP servers and per-project memory of {all}. Where they differ, {first} wins; for memory notes the newer one. No login, no conversations: log in with /login in its first session."
    );
    let field = text_input(t("Name, z. B. work", "Name, e.g. work"), &dialog.name)
        .id(crate::app::ACCOUNT_NAME)
        .on_input(Message::AccountSetupName)
        .on_submit(Message::AccountSetupCreate)
        .padding([8, 12])
        .size(14)
        .style(field_style);
    let mut body = column![dialog_title(t("Neues Claude-Konto", "New Claude account")), text(intro).size(13).line_height(1.5).color(TEXT).font(UI), field].spacing(14);
    match &dialog.plan {
        Some(Err(err)) => body = body.push(text(err.clone()).size(12.5).color(CALLS).font(UI)),
        Some(Ok(plan)) => {
            let differences = plan.conflicts().count();
            let (folder, count) = (plan.target.display().to_string(), plan.items.len());
            let summary = tr!("{folder} → {count} Einträge, {differences} Unterschiede", "{folder} → {count} items, {differences} differences");
            body = body.push(text(summary).size(12.5).color(TEXT_MUTED).font(UI));
            let rows = plan.items.iter().map(|item| {
                let mut lines = column![row![
                    text(item.what.clone()).size(12.5).color(TEXT_STRONG).font(UI),
                    text(item.from.join(" + ")).size(11.5).color(TEXT_FAINT).font(UI),
                ]
                .spacing(10)]
                .spacing(2);
                for conflict in &item.conflicts {
                    lines = lines.push(text(format!("≠ {conflict}")).size(11.5).color(TURN).font(UI));
                }
                lines.into()
            });
            body = body.push(scrollable(column(rows).spacing(6)).height(Fill).style(scrollbar));
            let label = if dialog.busy { t("Wird eingerichtet …", "Setting up …") } else { t("Konto einrichten", "Set up account") };
            body = body.push(action(label, "⏎", true, (!dialog.busy).then_some(Message::AccountSetupCreate)));
        }
        None => {}
    }
    body.padding(Padding { top: 24.0, right: 28.0, bottom: 20.0, left: 28.0 }).into()
}

/// ⌘K "Sync accounts": the conflicts with each account's version (the chosen one marked), what
/// is taken over from where, the memory notes as a count, and what accounts keep different.
fn sync_accounts(brain: &Brain) -> Element<'_, Message> {
    let dialog = &brain.sync;
    let mut body = column![
        dialog_title(t("Konten abgleichen", "Sync accounts")),
        text(t(
            "Was ein Konto seit dem letzten Abgleich geändert hat, bekommen die anderen: Einstellungen, CLAUDE.md, Skills, Agents, Commands, MCP-Server und das Gedächtnis pro Projekt. Wo Konten Verschiedenes geändert haben, entscheidest du; vorgeschlagen ist die neueste Änderung. Nach dem ersten Abgleich gleicht jeder Session-Start mit ab, Konflikte bleiben dann für hier.",
            "What an account changed since the last sync goes to the others: settings, CLAUDE.md, skills, agents, commands, MCP servers and per-project memory. Where accounts changed things differently, you decide; the newest change is suggested. After the first sync every session start syncs too, conflicts are left for here.",
        ))
        .size(13)
        .line_height(1.5)
        .color(TEXT)
        .font(UI),
    ]
    .spacing(14);
    let Some(plan) = &dialog.plan else {
        return body.push(text(t("Lese die Konten …", "Reading the accounts …")).size(12.5).color(TEXT_MUTED).font(UI)).padding(Padding { top: 24.0, right: 28.0, bottom: 20.0, left: 28.0 }).into();
    };
    if plan.first {
        body = body.push(
            text(t(
                "Erster Abgleich: Noch ist nichts als gelöscht bekannt, die Konten werden zusammengeführt.",
                "First sync: nothing is known to be deleted yet, so the accounts are joined.",
            ))
            .size(12.5)
            .color(TURN)
            .font(UI),
        );
    }
    let rows = dialog.rows();
    let (notes, projects) = dialog.memory();
    let conflicts = plan.conflicts();
    let taken = rows.len() - conflicts;
    body = body.push(
        text(tr!(
            "{conflicts} Konflikte · {taken} Übernahmen · {notes} Gedächtnis-Einträge in {projects} Projekten. Ziffern wählen die Version, K: Konten mit anderer Version behalten ihre, ⏎ gleicht ab, esc bricht ab.",
            "{conflicts} conflicts · {taken} taken over · {notes} memory items in {projects} projects. Digits pick the version, K: accounts with another version keep theirs, ⏎ syncs, esc cancels."
        ))
        .size(12.5)
        .color(TEXT_MUTED)
        .font(UI),
    );
    if rows.is_empty() && notes == 0 {
        body = body.push(text(t("Die Konten sind abgeglichen.", "The accounts are in step.")).size(13).color(DONE).font(UI));
    }
    let lines = rows.iter().zip(&dialog.choices).enumerate().map(|(index, (item, choice))| {
        let taken = item.versions.iter().find(|v| v.account == choice.take).map(|v| v.value.clone());
        let versions = item.versions.iter().enumerate().map(|(nth, version)| {
            let chosen = version.account == choice.take;
            let keeps = choice.keep && Some(version.value.clone()) != taken;
            let mut label = format!("{} {}", version.account, version.describe(item.kind));
            if keeps {
                label.push_str(&format!(" · {}", t("behält", "keeps")));
            }
            let border = if chosen { WORKING } else { LINE };
            let line = row![kbd(format!("{}", nth + 1)), text(label).size(12).color(if chosen { TEXT_STRONG } else { TEXT_MUTED }).font(UI)].spacing(6).align_y(iced::Center);
            mouse_area(container(line).padding([3, 8]).style(move |_| boxed(SURFACE, border, 6.0)))
                .on_press(Message::SyncPick(index, version.account.clone()))
                .interaction(iced::mouse::Interaction::Pointer)
                .into()
        });
        // ⚑: grants rights or runs commands; a session start never takes it over unseen.
        let title = match (item.conflict, item.review) {
            (true, _) => format!("≠ {}", item.label),
            (false, true) => format!("⚑ {}", item.label),
            _ => item.label.clone(),
        };
        let selected = index == dialog.selected;
        let keep = style::text_button(t("K andere behalten ihre", "K others keep theirs"), choice.keep, Message::SyncKeep(index));
        let content = column![
            text(title).size(12.5).color(if item.conflict { TURN } else { TEXT_STRONG }).font(UI),
            row(versions).push(keep).spacing(6).wrap(),
        ]
        .spacing(6);
        let background = if selected { RAISED } else { SURFACE };
        container(content).padding([8, 12]).width(Fill).style(move |_| boxed(background, if selected { LINE_STRONG } else { LINE }, 8.0)).into()
    });
    body = body.push(scrollable(column(lines).spacing(6)).height(Fill).style(scrollbar));
    for (account, label) in &plan.kept {
        body = body.push(text(tr!("= {label} – {account} behält seine Version", "= {label} – {account} keeps its own")).size(11.5).color(TEXT_FAINT).font(UI));
    }
    if let Some(err) = &dialog.error {
        body = body.push(text(err.clone()).size(12.5).color(CALLS).font(UI));
    }
    let label = if dialog.busy { t("Gleiche ab …", "Syncing …") } else { t("Abgleichen", "Sync") };
    // The first sync also records the base, even when nothing differs.
    let ready = !dialog.busy && (plan.first || !rows.is_empty() || notes > 0);
    body = body.push(action(label, "⏎", true, ready.then_some(Message::SyncApply)));
    body.padding(Padding { top: 24.0, right: 28.0, bottom: 20.0, left: 28.0 }).into()
}

/// The gardener: the sessions Brain lists but can't steer any more, why, and which get ended.
fn garden(brain: &Brain) -> Element<'_, Message> {
    use brain_core::garden::Reason;
    let dialog = &brain.garden;
    let mut body = column![
        dialog_title(t("Gärtner", "Gardener")),
        text(t(
            "Sessions, die Brain listet, aber nicht mehr steuern kann. Beenden heißt: claude stop, sonst SIGTERM, zuletzt SIGKILL. Das Gespräch bleibt und lässt sich fortsetzen. Was dabei passiert, steht im Log (⌘K → Log öffnen).",
            "Sessions Brain lists but can't steer any more. Ending means: claude stop, else SIGTERM, last SIGKILL. The conversation stays and can be resumed. What happens is in the log (⌘K → Open the log).",
        ))
        .size(13)
        .line_height(1.5)
        .color(TEXT)
        .font(UI),
    ]
    .spacing(14)
    .max_width(720);
    if dialog.findings.is_empty() {
        body = body.push(text(t("Alles in Ordnung: keine kaputten Sessions.", "All good: no broken sessions.")).size(13).color(DONE).font(UI));
        return body.padding(Padding { top: 24.0, right: 28.0, bottom: 20.0, left: 28.0 }).into();
    }
    let rows = dialog.findings.iter().zip(&dialog.chosen).enumerate().map(|(index, (finding, chosen))| {
        let session = brain.model.board.get(&finding.key);
        let name = session.map(|s| s.display_name()).unwrap_or_else(|| finding.key.id.clone());
        let why = match &finding.reason {
            Reason::LeftOver { newer } => {
                let account = &newer.account;
                tr!("Läuft doppelt: Dasselbe Gespräch läuft jetzt auch in {account}; das hier ist das alte Exemplar.", "Runs twice: the same conversation now runs in {account} too; this is the old copy.")
            }
            Reason::Unreachable { .. } => t("Nicht erreichbar: Ihr Terminal ging gleich nach dem Anhängen wieder zu.", "Unreachable: its terminal closed right after attaching.").to_string(),
            Reason::Stuck { quiet_ms } => {
                let hours = quiet_ms / 3_600_000;
                tr!("Hängt vermutlich: „arbeitet“, aber seit {hours} h tut sich nichts.", "Probably stuck: “working”, but nothing happened for {hours} h.")
            }
        };
        let mark = if *chosen { "☑" } else { "☐" };
        let mut lines = column![
            row![
                text(mark).size(15).color(if *chosen { WORKING } else { TEXT_MUTED }),
                text(name).size(13).color(TEXT_STRONG).font(style::semibold()),
                account_badge(&finding.key.account, brain.account_index(&finding.key.account)),
            ]
            .spacing(10)
            .align_y(iced::Center),
            text(why).size(12.5).color(if finding.sure { TURN } else { TEXT_MUTED }).font(UI),
        ]
        .spacing(4);
        if let Reason::Unreachable { said } = &finding.reason {
            for line in said.iter().rev().take(4).rev() {
                lines = lines.push(text(clip(line, 110)).size(11.5).color(TEXT_FAINT).font(MONO));
            }
        }
        let selected = index == dialog.selected;
        mouse_area(container(lines).padding([8, 12]).width(Fill).style(move |_| boxed(if selected { RAISED } else { SURFACE }, if selected { LINE_STRONG } else { LINE }, 8.0)))
            .on_press(Message::GardenToggle(index))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    });
    body = body.push(scrollable(column(rows).spacing(6)).height(Fill).style(scrollbar));
    body = body.push(text(t("↑↓ wählen · Leertaste oder Klick: an/aus · ⏎ beenden · esc", "↑↓ select · space or click: on/off · ⏎ end · esc")).size(11.5).color(TEXT_FAINT).font(UI));
    let count = dialog.chosen.iter().filter(|c| **c).count();
    let label = if dialog.busy { t("Beende …", "Ending …").to_string() } else { tr!("{count} beenden", "End {count}") };
    body = body.push(action(label, "⏎", true, (!dialog.busy && count > 0).then_some(Message::GardenEnd)));
    body.padding(Padding { top: 24.0, right: 28.0, bottom: 20.0, left: 28.0 }).into()
}

/// Brain Link: the code the iPhone camera scans, where Brain listens, and the way out.
fn pairing(brain: &Brain) -> Element<'_, Message> {
    let mut body = column![
        dialog_title(t("iPhone koppeln", "Pair a phone")),
        text(t(
            "Richte die Kamera deines iPhones auf den Code und tippe auf „In Brain öffnen“. Die App merkt sich Brain und verbindet sich verschlüsselt.",
            "Point your iPhone's camera at the code and tap “Open in Brain”. The app remembers Brain and connects encrypted.",
        ))
        .size(13)
        .line_height(1.5)
        .color(TEXT)
        .font(UI),
    ]
    .spacing(16)
    .max_width(560);
    if let Some(qr) = &brain.link_qr {
        let code = iced::widget::image(qr.clone()).width(280).height(280).filter_method(iced::widget::image::FilterMethod::Nearest);
        body = body.push(container(code).padding(10).style(|_| boxed(Color::WHITE, LINE, 12.0)));
    }
    if let Some(pairing) = brain.link_pairing() {
        let code = pairing.code.clone();
        body = body.push(
            row![
                text(t("Kennung", "Code")).size(13).color(TEXT_MUTED).font(UI),
                text(code).size(17).color(TEXT_STRONG).font(style::bold()),
                text(t("– das iPhone zeigt sie vor dem Koppeln, sie muss gleich sein.", "– the iPhone shows it before pairing; it must match."))
                    .size(12.5)
                    .color(TEXT_MUTED)
                    .font(UI),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
        let hosts = pairing.hosts.join(" · ");
        body = body.push(text(tr!("Erreichbar unter {hosts}", "Reachable at {hosts}")).size(12).color(TEXT_MUTED).font(UI));
    }
    body = body
        .push(
            text(t(
                "Die App erreicht Brain, solange iPhone und Mac im selben WLAN sind; nach einem Wechsel findet sie Brain von selbst wieder. Der Code enthält den Schlüssel – zeig ihn niemandem.",
                "The app reaches Brain while the iPhone and the Mac are in the same Wi-Fi; after a change it finds Brain again by itself. The code holds the key – don't show it to anyone.",
            ))
            .size(12)
            .line_height(1.5)
            .color(TEXT_FAINT)
            .font(UI),
        )
        .push(
            row![
                action(t("Alle Handys abmelden", "Sign out all phones"), "", false, Some(Message::LinkForget)),
                action(t("Brain Link ausschalten", "Turn Brain Link off"), "", false, Some(Message::LinkOff)),
            ]
            .spacing(10),
        );
    scrollable(body.padding(Padding { top: 24.0, right: 28.0, bottom: 24.0, left: 28.0 })).height(Fill).style(scrollbar).into()
}

// ---- footer -----------------------------------------------------------------------

/// The few keys that matter for the selected session right now; everything else is behind ⌘K
/// and `?`.
fn footer_hints(brain: &Brain) -> Vec<(&'static str, &'static str)> {
    if brain.tab == DetailTab::Terminal && brain.selected.as_ref().is_some_and(|k| brain.terminals.contains_key(k)) {
        return vec![
            ("⏎ ⌘2", t("ins Terminal", "into terminal")),
            ("⌘1", t("zur Liste", "to the list")),
            ("⌘T", t("Shell", "shell")),
            ("⌘D", t("teilen", "split")),
        ];
    }
    let mut hints = vec![("↑↓", t("wählen", "select"))];
    match brain.selected_session() {
        None => hints.push(("⌘N", t("neue Session", "new session"))),
        Some(s) if s.phase() == Phase::Ended => hints.push(("⏎", t("fortsetzen", "resume"))),
        Some(s) => {
            if s.awaiting_permission() {
                hints.push(("Y N", t("erlauben · ablehnen", "allow · deny")));
            } else if s.accepts_input() {
                hints.push(("T", t("antworten", "reply")));
            }
            hints.push(("⏎", t("öffnen", "open")));
        }
    }
    hints.push(("/", t("suchen", "search")));
    hints
}

fn footer(brain: &Brain) -> Element<'_, Message> {
    let shown = footer_hints(brain);
    // A status message takes the whole footer while it shows: the hints would push it off the
    // edge, and "press again" prompts must not be missed.
    if let Some((message, _)) = &brain.status {
        let band = row![
            text("●").size(11).color(CALLS),
            text(clip(message, 160)).size(13).color(TEXT_STRONG).font(style::semibold()).wrapping(text::Wrapping::None),
        ]
        .spacing(8)
        .align_y(iced::Center);
        return container(band)
            .height(32)
            .padding([0, 14])
            .center_y(32)
            .width(Fill)
            .clip(true)
            .style(|_| container::Style {
                background: Some(Background::Color(alpha(CALLS, 0x2a))),
                border: Border { color: alpha(CALLS, 0x80), width: 1.0, radius: 0.0.into() },
                ..container::Style::default()
            })
            .into();
    }
    let hint = |(key, label): (&'static str, &'static str)| -> Element<'_, Message> {
        row![kbd(key), text(label).size(11.5).color(TEXT_FAINT).font(UI)].spacing(5).align_y(iced::Center).into()
    };
    let always = [("⌘K", t("Befehle", "commands")), ("?", t("alle Tasten", "all keys"))];
    let bar = row![
        row(shown.into_iter().map(hint)).spacing(12).align_y(iced::Center),
        Space::new().width(Fill),
        row(always.into_iter().map(hint)).spacing(12).align_y(iced::Center),
    ]
    .align_y(iced::Center);
    container(bar).height(32).padding([0, 14]).center_y(32).width(Fill).clip(true).style(|_| style::fill(CHROME)).into()
}

/// `?`: every key, by what it acts on. The footer only shows the few the moment needs.
fn keys_sheet(_brain: &Brain) -> Element<'_, Message> {
    let groups: [(&str, Vec<(&str, &str)>); 3] = [
        (
            t("Liste", "List"),
            vec![
                ("↑↓  j k", t("wählen", "select")),
                ("1–9", t("n-te Session öffnen", "open the n-th session")),
                ("⏎", t("öffnen (beendete: fortsetzen)", "open (ended: resume)")),
                ("⌥⏎", t("in iTerm öffnen", "open in iTerm")),
                ("/", t("Liste durchsuchen", "search the list")),
                ("⇥", t("Konto-Filter", "account filter")),
                ("G", t("nach Projekt gruppieren", "group by project")),
                ("D", t("Heute", "today")),
                ("C", t("aufräumen", "clean up")),
                ("⌘N", t("neue Session", "new session")),
            ],
        ),
        (
            t("Session", "Session"),
            vec![
                ("T", t("antworten (1–9 übernimmt eine Schnellantwort)", "reply (1–9 fills in a quick reply)")),
                ("Y  N", t("Freigabe erlauben · ablehnen", "allow · deny a permission")),
                ("R", t("umbenennen", "rename")),
                ("P", t("anheften", "pin")),
                ("S", t("pausieren: 15 min → 1 h → morgen → bis sich etwas tut", "snooze: 15 min → 1 h → tomorrow → until something happens")),
                ("⌫ ⌫", t("gleich bis sich etwas tut", "straight to until something happens")),
                ("A", t("weiterführen: Konto, Fork, Flags", "go on: account, fork, flags")),
                ("F", t("forken (Original läuft weiter)", "fork (the original keeps running)")),
                ("I", t("aus iTerm in Brain holen", "move from iTerm into Brain")),
                ("X X", t("beenden", "end")),
                ("⌘⌫ ⌘⌫", t("in den Papierkorb", "move to the Trash")),
                ("⌘E", t("längerer Prompt (Composer)", "longer prompt (composer)")),
                ("⌘F", t("im ganzen Gespräch suchen", "find in the whole conversation")),
                ("⌘⇧C", t("letzte Antwort kopieren", "copy the last answer")),
            ],
        ),
        (
            t("Ansicht und Terminal", "View and terminal"),
            vec![
                ("← →", t("Tabs wechseln", "switch tabs")),
                ("⌘2  ⌘J", t("Tastatur ins Terminal", "keyboard into the terminal")),
                ("⌘1  ⌘[", t("zurück zur Liste", "back to the list")),
                ("⌘T", t("Shell im Projektordner", "shell in the project folder")),
                ("⌘D", t("Terminal teilen", "split the terminal")),
                ("⌘⇧A", t("alle Terminals im Überblick", "all terminals at a glance")),
                ("⌘-Klick", t("Pfad oder Link öffnen", "open a path or link")),
                ("⌘V", t("Screenshot ins Terminal", "paste a screenshot")),
                ("⌘K", t("Befehle", "commands")),
                ("esc", t("schließen", "close")),
            ],
        ),
    ];
    let columns = groups.into_iter().map(|(title, keys)| {
        let lines = keys.into_iter().map(|(key, label)| {
            row![container(kbd(key)).width(76), text(label).size(12.5).color(TEXT).font(UI)].spacing(10).align_y(iced::Center).into()
        });
        column![text(title).size(12).color(TEXT_MUTED).font(style::semibold()), column(lines).spacing(7)].spacing(10).width(Fill).into()
    });
    column![dialog_title(t("Tasten", "Keys")), scrollable(row(columns).spacing(28)).height(Fill).style(scrollbar)]
        .spacing(18)
        .padding(Padding { top: 24.0, right: 28.0, bottom: 20.0, left: 28.0 })
        .into()
}
