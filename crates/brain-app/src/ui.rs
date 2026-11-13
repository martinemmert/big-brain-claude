//! Everything Brain draws: title bar, session list, detail pane, dialogs, footer.

use brain_core::changes::SessionChange;
use brain_core::later;
use brain_core::state::{Ask, Phase, Session};
use iced::widget::{button, column, container, mouse_area, row, scrollable, stack, text, text_input, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Padding};

use crate::companion::Command;
use crate::app::{self, Action, Brain, Choice, Groups, Message, Mode, CLEANUP_DAYS};
use crate::tree::{self, Mark, Node, Project, Tree};
use crate::format::{ago, ago_phrase, background_summary, clip, clip_start, clock, format_tokens, now_ms, plain, short_model, tilde};
use crate::i18n::t;
use crate::markdown;
use crate::style::{
    self, account_badge, action, alpha, boxed, hint, kbd, marker, phase_color, segmented, CALLS,
    CALLS_SOFT, CHROME, DONE, INK, LINE, LINE_STRONG, MONO, RAISED, SURFACE, TEXT, TEXT_FAINT, TEXT_MUTED, TEXT_STRONG,
    TURN, UI, WORKING,
};
use crate::tr;
use crate::tree::HeadKey;

mod review;

/// Room for the window's traffic lights at the left of the title bar.
const TRAFFIC_LIGHTS: f32 = 78.0;

pub fn view(brain: &Brain) -> Element<'_, Message> {
    let now = now_ms();
    // The list, the title bar and the footer all read the same grouping: made once per view.
    let groups = brain.groups(now);
    let detail = match brain.mode {
        _ if brain.overview => overview(brain),
        Mode::Cleanup => cleanup(brain),
        Mode::Later => later(brain),
        Mode::Pairing => pairing(brain),
        Mode::AccountSetup => account_setup(brain),
        Mode::Sync => sync_accounts(brain),
        Mode::Keys => keys_sheet(brain),
        Mode::Garden => garden(brain),
        Mode::Relaunch => relaunch(brain),
        Mode::NewSession => new_session(brain),
        _ => detail(brain),
    };
    let detail = container(detail).width(Fill).height(Fill).style(|_| style::fill(INK));
    let body = if brain.prefs.hide_sidebar {
        row![detail]
    } else {
        let left = column![search(brain), tree(brain, &groups, now)];
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
        let titlebar_height = crate::chrome::TITLEBAR_HEIGHT / brain.zoom();
        let left = mouse_area(container(left).width(Length::Fixed(width - 3.0)).height(Fill).style(|_| style::fill(CHROME)))
            .on_move(move |p| Message::Pointer(iced::Point::new(p.x, p.y + titlebar_height + 1.0)));
        row![left, grip, detail]
    };
    let mut rows = column![titlebar(brain), divider(), body.height(Fill)];
    // Below only when a limit is nearly used up.
    if let Some(warning) = limit_warning_bar(brain, now) {
        rows = rows.push(divider()).push(warning);
    }
    let window = container(rows)
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style { background: Some(Background::Color(INK)), text_color: Some(TEXT), ..container::Style::default() });
    // Always a stack, with an empty layer when nothing lies on top: a root that changed shape
    // when a menu opened would cost the list its state, and with it its scroll position.
    let overlay: Element<'_, Message> = match brain.mode {
        Mode::Palette => palette(brain),
        Mode::Composer => composer(brain),
        _ => context_menu(brain).or_else(|| project_menu(brain)).unwrap_or_else(|| Space::new().into()),
    };
    // The status popup hangs under the title bar's right end, over the whole window.
    let popup: Element<'_, Message> = if brain.status_open { status_popup_layer(brain, now) } else { Space::new().into() };
    // So does the list of changed files; never both at once.
    let changes: Element<'_, Message> = if brain.changes_open {
        changes_popover_layer(brain)
    } else if brain.targets_open {
        targets_popover_layer(brain)
    } else {
        Space::new().into()
    };
    // A changed file in Quick Look lies over everything but the toast.
    let sheet: Element<'_, Message> = if brain.preview.is_some() { quick_look(brain) } else { Space::new().into() };
    stack![window, overlay, popup, changes, sheet, toast(brain)].into()
}

/// A message of the last command: a small card at the bottom, centred over the main area. It
/// takes no room, so nothing in the window moves when it comes and goes.
fn toast(brain: &Brain) -> Element<'_, Message> {
    let Some((message, _)) = &brain.status else { return Space::new().into() };
    let left = if brain.prefs.hide_sidebar { 0.0 } else { brain.prefs.sidebar_width.unwrap_or(app::SIDEBAR_DEFAULT) };
    let card = container(text(clip(message, 110)).size(12).color(TEXT_STRONG).font(UI).wrapping(text::Wrapping::None))
        .padding([8, 14])
        .style(|_| boxed(RAISED, LINE_STRONG, 10.0));
    container(card).width(Fill).height(Fill).align_x(iced::Center).align_y(iced::Bottom).padding(Padding { left, bottom: 24.0, ..Padding::ZERO }).into()
}

/// ⌘E: a longer prompt for the selected session, with templates and pasted screenshots.
fn composer(brain: &Brain) -> Element<'_, Message> {
    let name = brain.selected_session().map(|s| s.display_name()).unwrap_or_default();
    let editor = iced::widget::text_editor(&brain.composer)
        .id(app::COMPOSER)
        .on_action(Message::ComposerEdit)
        .placeholder(t("Prompt schreiben … (⌘V fügt einen Screenshot ein)", "Write the prompt … (⌘V adds a screenshot)"))
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
        button(row![text(label).size(13).color(if chosen { TEXT_STRONG } else { TEXT }).font(UI).width(Fill), text(keys).size(11.5).color(TEXT_FAINT).font(UI)].spacing(12).align_y(iced::Center))
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
    let panel = container(column![container(row![text("⌘K").size(12).color(TEXT_FAINT), field].spacing(10).align_y(iced::Center)).padding([0, 8]), divider(), list].spacing(6))
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

// ---- context menu -----------------------------------------------------------------

const MENU_WIDTH: f32 = 250.0;
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
        open.push((t("Nach Brain holen", "Move into Brain").to_string(), "⌥⌘O", Action::TakeOver, false));
    }
    // Beside the terminal that's on screen: two sessions side by side.
    let beside = brain.before_menu.as_ref().is_some_and(|k| brain.terminals.contains_key(k));
    if beside && (s.agent.is_some() || ended || brain.terminals.contains_key(&s.key)) {
        open.push((t("Daneben öffnen (geteilt)", "Open beside (split)").to_string(), "⌘D", Action::OpenBeside, false));
    }
    groups.push(open);

    let mut talk = Vec::new();
    if s.awaiting_permission() && caps.keys {
        talk.push((t("Erlauben", "Allow").to_string(), "⌘Y", Action::Allow, false));
        talk.push((t("Ablehnen", "Deny").to_string(), "⇧⌘Y", Action::Deny, false));
    }
    // Brain renames with `/rename` through the session's mod: only while the session waits.
    if s.waits_for_input() && brain.can_send(&s.key) {
        talk.push((t("Umbenennen", "Rename").to_string(), "⌘R", Action::StartRename, false));
    }
    groups.push(talk);

    let pinned = brain.prefs.is_pinned(&s.key);
    let pin = match (pinned, ended) {
        (false, true) => t("Zum Fortsetzen merken", "Save to resume"),
        (true, true) => t("Nicht mehr merken", "Don't save"),
        (false, false) => t("Anheften", "Pin"),
        (true, false) => t("Lösen", "Unpin"),
    };
    let mut keep = vec![(pin.to_string(), "⌥⌘P", Action::Pin, false)];
    let mute = if brain.prefs.is_muted(&s.key) { t("Benachrichtigungen an", "Unmute") } else { t("Stummschalten", "Mute") };
    keep.push((mute.to_string(), "", Action::Mute, false));
    let hidden = brain.prefs.is_hidden(&s.key, s.last_activity_ms);
    if hidden {
        keep.push((t("Wieder einblenden", "Show again").to_string(), "", Action::Snooze, false));
    } else if !ended {
        keep.push((t("Pausieren (15 min → 1 h → morgen → bis sich etwas tut)", "Snooze (15 min → 1 h → tomorrow → until something happens)").to_string(), "", Action::Snooze, false));
    }
    keep.push((t("In ein anderes Konto oder forken …", "Into another account or fork …").to_string(), "", Action::Relaunch, false));
    keep.push((t("In ein anderes Projekt …", "Into another project …").to_string(), "", Action::MoveToProject, false));
    if !brain.prefs.is_project_listed(&brain.project_name(s)) {
        keep.push((t("Als Projekt behalten", "Keep as a project").to_string(), "", Action::KeepProject, false));
    }
    if brain.is_guess(s) {
        keep.push((t("Nur Bericht, keine Frage", "Only a report, no question").to_string(), "", Action::OnlyReport, false));
    }
    groups.push(keep);

    let mut remove = Vec::new();
    if agent_active {
        remove.push((t("Hintergrund-Session stoppen", "Stop background session").to_string(), "⌥⌘X", Action::End, false));
    } else if !ended && s.accepts_input() {
        remove.push((t("Beenden", "End").to_string(), "⌥⌘X", Action::End, false));
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
        remove.push((t("In den Papierkorb", "Move to Trash").to_string(), "⌥⌘⌫", Action::Trash, true));
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
    let lines = menu_lines(brain, s)
        .into_iter()
        .map(|line| line.map(|(label, keys, action, danger)| (label, keys, Message::MenuPick(action), danger)))
        .collect();
    Some(menu_panel(brain, lines, *at))
}

/// What a right click on a project offers.
fn project_menu(brain: &Brain) -> Option<Element<'_, Message>> {
    let (name, at) = brain.project_menu.as_ref()?;
    let hidden = brain.prefs.is_project_hidden(name);
    let lines = vec![
        Some((t("Neue Session hier …", "New session here …").to_string(), "", Message::NewSessionIn(name.clone()), false)),
        None,
        Some((if hidden { t("Wieder einblenden", "Show again") } else { t("Projekt ausblenden", "Hide project") }.to_string(), "", Message::ToggleProjectHidden(name.clone()), false)),
    ];
    Some(menu_panel(brain, lines, *at))
}

/// A menu with these lines at `at`, kept inside the window, under a backdrop that closes it.
fn menu_panel<'a>(brain: &Brain, lines: Vec<Option<(String, &'static str, Message, bool)>>, at: iced::Point) -> Element<'a, Message> {
    let height = MENU_PADDING * 2.0 + lines.iter().map(|l| if l.is_some() { MENU_ROW } else { MENU_SEPARATOR }).sum::<f32>();
    let x = at.x.min(brain.window_size.width - MENU_WIDTH - 8.0).max(8.0);
    // Below the pointer when it fits, otherwise above it.
    let y = if at.y + height + 8.0 <= brain.window_size.height { at.y } else { (at.y - height).max(8.0) };

    let rows = lines.into_iter().map(|line| -> Element<'_, Message> {
        let Some((label, keys, message, danger)) = line else {
            return container(container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)))
                .padding([4, 6])
                .height(MENU_SEPARATOR)
                .into();
        };
        let color = if danger { CALLS_SOFT } else { TEXT };
        button(
            row![text(label).size(13).color(color).font(UI).width(Fill), text(keys).size(11.5).color(TEXT_FAINT).font(UI)]
                .spacing(12)
                .align_y(iced::Center),
        )
        .padding([0, 10])
        .height(MENU_ROW)
        .width(Fill)
        .on_press(message)
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
    stack![backdrop, iced::widget::pin(menu).position(iced::Point::new(x, y))].into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE)).into()
}

// ---- title bar --------------------------------------------------------------------

/// The sidebar button, as in Finder and Mail: folds the sidebar away and back (⌃⌘S).
fn sidebar_button<'a>(hidden: bool) -> Element<'a, Message> {
    let toggle = button(style::sidebar_icon(16.0, if hidden { TEXT_MUTED } else { TEXT_FAINT }))
        .padding([3, 5])
        .on_press(Message::Menu(crate::app::MenuCommand::ToggleSidebar))
        .style(|_, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed).then_some(Background::Color(style::HOVER)),
            border: iced::border::rounded(5),
            ..button::Style::default()
        });
    let tip = container(text(if hidden { t("Seitenleiste zeigen  ⌃⌘S", "Show sidebar  ⌃⌘S") } else { t("Seitenleiste ausblenden  ⌃⌘S", "Hide sidebar  ⌃⌘S") }).size(11.5).color(TEXT).font(UI))
        .padding([4, 8])
        .style(|_| boxed(RAISED, LINE_STRONG, 6.0));
    iced::widget::tooltip(toggle, tip, iced::widget::tooltip::Position::Bottom).gap(4).into()
}

/// Brain's mark in miniature: a grid of sessions, always quiet; the button at the right end of
/// the title bar is the window's one signal.
fn brand_mark<'a>() -> Element<'a, Message> {
    let cell = |color: Color| style::dot::<Message>(color, 4.0);
    let quiet = LINE_STRONG;
    container(
        column![
            row![cell(quiet), cell(WORKING), cell(quiet)].spacing(2),
            row![cell(WORKING), cell(quiet), cell(quiet)].spacing(2),
            row![cell(quiet), cell(quiet), cell(WORKING)].spacing(2),
        ]
        .spacing(2),
    )
    .padding(4)
    .style(|_| boxed(RAISED, Color::TRANSPARENT, 6.0))
    .into()
}

/// The title bar in two parts, like Mail's: over the sidebar Brain's mark and the sidebar button,
/// then a divider that carries the sidebar's edge up; over the terminal the selected session's
/// line (project › name, branch, account, context), at the right the account filter. Without the
/// sidebar a short divider follows the button instead.
fn titlebar(brain: &Brain) -> Element<'_, Message> {
    let hidden = brain.prefs.hide_sidebar;
    let titlebar_height = crate::chrome::TITLEBAR_HEIGHT / brain.zoom();
    // Right after the traffic lights (the row's spacing comes before the mark).
    let lights = Space::new().width(TRAFFIC_LIGHTS / brain.zoom() - 12.0);
    let edge = |height: f32| container(Space::new().width(1).height(height)).style(|_| style::fill(LINE));
    let mut bar = if hidden {
        // Without the sidebar: mark and button take what they need, a short line parts them from
        // the session.
        row![row![lights, brand_mark(), sidebar_button(hidden)].spacing(12).align_y(iced::Center), edge(18.0)]
    } else {
        // As wide as the sidebar, the button at its right end; the divider stands where the
        // sidebar's edge does (the row's spacing is the last 10 points).
        let width = brain.prefs.sidebar_width.unwrap_or(app::SIDEBAR_DEFAULT);
        let lead = row![lights, brand_mark(), Space::new().width(Fill), sidebar_button(hidden)].spacing(12).align_y(iced::Center);
        row![container(lead).width(Length::Fixed(width - 10.0)).padding(Padding { right: 2.0, ..Padding::ZERO }), edge(titlebar_height)]
    }
    .spacing(10)
    .align_y(iced::Center);
    // The selected session's line, where the window's title would be.
    match brain.selected_session() {
        Some(s) => {
            let line = container(header_line(brain, s, None)).width(Fill).padding(Padding { left: 4.0, right: 8.0, ..Padding::ZERO }).clip(true);
            bar = bar.push(iced::widget::tooltip(line, detail_card(brain, s), iced::widget::tooltip::Position::Bottom).gap(6));
            if let Some(cache) = brain.selected_cache().filter(|_| brain.has_changes_button()) {
                bar = bar.push(changes_button(brain, cache));
            }
        }
        None => bar = bar.push(Space::new().width(Fill)),
    }
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
    bar = bar.push(style::segmented_in(accounts, LINE_STRONG, Color::from_rgba8(0xff, 0xff, 0xff, 0.06)));
    bar = bar.push(status_button(brain));

    // The native title bar keeps its size in points when the window is zoomed.
    mouse_area(container(bar).height(titlebar_height).center_y(titlebar_height).padding(Padding { left: 0.0, right: 14.0, top: 0.0, bottom: 0.0 }).style(|_| style::fill(CHROME)))
        .on_press(Message::DragWindow)
        .on_double_click(Message::ZoomWindow)
        .into()
}

/// The status popup's button, right of the account filter: a dot for the state (red when someone
/// needs you, amber when sessions finished unseen), the number that needs you, and the account
/// with the least room left. Pressed while the popup is open.
fn status_button(brain: &Brain) -> Element<'_, Message> {
    let now = now_ms();
    let content = brain.popup_content(now);
    let (color, count) = if content.needs_total > 0 {
        (CALLS, Some(content.needs_total))
    } else if content.finished_total > 0 {
        (TURN, Some(content.finished_total))
    } else {
        (TEXT_FAINT, None)
    };
    // The fullest account, by the larger of its two limits.
    let tightest = content
        .accounts
        .iter()
        .filter_map(|a| match a.usage.five_hour {
            Some(five) => Some(five.max(a.usage.week.unwrap_or(0.0))),
            None => a.usage.week,
        })
        .fold(None, |most: Option<f64>, used| Some(most.map_or(used, |m| m.max(used))));
    let mut label = row![style::dot(color, 7.0)].spacing(6).align_y(iced::Center);
    if let Some(count) = count {
        label = label.push(text(count.to_string()).size(12).color(TEXT_STRONG).font(style::semibold()));
    }
    if let Some(used) = tightest {
        let used_color = if used >= 90.0 { CALLS } else if used >= 70.0 { TURN } else { TEXT_MUTED };
        label = label.push(text(format!("{used:.0} %")).size(12).color(used_color).font(UI));
    }
    let open = brain.status_open;
    iced::widget::tooltip(
        button(style::centered(label))
            .padding([0, 10])
            .height(28)
            .on_press(Message::ToggleStatus)
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(if open { LINE_STRONG } else if hovered { RAISED } else { INK })),
                    text_color: TEXT,
                    border: Border { color: LINE, width: 1.0, radius: 8.0.into() },
                    ..button::Style::default()
                }
            }),
        container(text(t("Wer dich braucht, wer fertig ist, Verbrauch", "Who needs you, who finished, usage")).size(11.5).color(TEXT).font(UI))
            .padding([4, 8])
            .style(|_| boxed(RAISED, LINE_STRONG, 6.0)),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// The sums of what a session changed: lines added and removed over all its files.
fn change_sums(files: &[SessionChange]) -> (u64, u64) {
    (files.iter().filter_map(|c| c.added).sum(), files.iter().filter_map(|c| c.removed).sum())
}

/// A merge request's mark for a file this session wrote itself.
fn session_mark<'a>() -> Element<'a, Message> {
    style::dot(WORKING, 6.0)
}

/// What a file's status says to the reader, if it is worth saying: new, deleted, renamed.
fn status_label(change: &SessionChange) -> Option<(&'static str, Color)> {
    match change.git.as_ref()?.status.as_str() {
        "??" | "A " | "AM" => Some((t("neu", "new"), DONE)),
        "D " | " D" => Some((t("gelöscht", "deleted"), CALLS_SOFT)),
        status if status.starts_with('R') => Some((t("umbenannt", "renamed"), WORKING)),
        _ => None,
    }
}

fn badge<'a>(label: &'static str, color: Color, size: f32) -> Element<'a, Message> {
    container(text(label).size(size).color(color).font(style::semibold())).padding([0, 5]).style(move |_| boxed(Color::TRANSPARENT, alpha(color, 0x73), 4.0)).into()
}

/// How many characters of the branch's name the title bar has room for: the button gets at most
/// half the window beside the sidebar, and what is left after `rest` characters of the target and
/// the counts goes to the name.
fn branch_room(brain: &Brain, rest: usize) -> usize {
    const GLYPH: f32 = 6.6;
    let sidebar = if brain.prefs.hide_sidebar { 0.0 } else { brain.prefs.sidebar_width.unwrap_or(app::SIDEBAR_DEFAULT) };
    let pane = brain.window_size.width / brain.zoom() - sidebar;
    let budget = pane * 0.5 - 48.0 - rest as f32 * GLYPH;
    ((budget / GLYPH).max(0.0) as usize).clamp(8, 60)
}

/// "feature/x → main · 4 Dateien +67 −3" right of the session's line, like the head of a merge
/// request. The branch and the counts open the list of the files, "→ main" the choice of the
/// target. Without git: "4 Dateien +67 −3" for the files the transcript names. The name of the
/// branch is cut before anything else is.
fn changes_button<'a>(brain: &Brain, cache: &app::ChangesCache) -> Element<'a, Message> {
    let files = &cache.files;
    let (added, removed) = change_sums(files);
    let count = files.len();
    let label = match count {
        0 => t("keine Änderungen", "no changes").to_string(),
        1 => t("1 Datei", "1 file").to_string(),
        _ => tr!("{count} Dateien", "{count} files"),
    };
    let mut tail = row![text(label.clone()).size(12).color(TEXT).font(UI)].spacing(8).align_y(iced::Center);
    if count > 0 {
        tail = tail.push(counts(Some(added), Some(removed)));
    }
    // A violet dot: a review of this session waits for it to want a prompt.
    if brain.selected.as_ref().is_some_and(|key| brain.review_waits(key)) {
        tail = tail.push(review::waiting_sign());
    }
    let (list_open, picking) = (brain.changes_open, brain.targets_open);
    let segment = |content: Element<'a, Message>, message: Message, active: bool| -> Element<'a, Message> {
        button(style::centered(content))
            .padding([0, 8])
            .height(26)
            .on_press(message)
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: (active || hovered).then_some(Background::Color(if active { LINE_STRONG } else { RAISED })),
                    text_color: TEXT,
                    border: iced::border::rounded(7),
                    ..button::Style::default()
                }
            })
            .into()
    };
    let Some(merge) = &cache.merge else {
        return container(segment(tail.into(), Message::ToggleChanges, list_open)).style(|_| boxed(INK, LINE, 8.0)).padding(1).into();
    };
    let target = merge.target.as_deref().map_or_else(|| t("Ziel wählen", "choose target").to_string(), |t| crate::format::clip_branch(t, 24));
    let rest = target.chars().count() + label.chars().count() + 14;
    let branch = crate::format::clip_branch(&merge.branch, branch_room(brain, rest));
    let arrow = row![text("→").size(12).color(TEXT_MUTED).font(UI), text(target).size(12).color(TEXT_STRONG).font(style::semibold())].spacing(5).align_y(iced::Center);
    let tail = row![text("·").size(12).color(TEXT_FAINT).font(UI), tail].spacing(8).align_y(iced::Center);
    container(
        row![
            segment(text(branch).size(12).color(TEXT_STRONG).font(style::semibold()).wrapping(text::Wrapping::None).into(), Message::ToggleChanges, list_open),
            segment(arrow.into(), Message::ToggleTargets, picking),
            segment(tail.into(), Message::ToggleChanges, list_open),
        ]
        .align_y(iced::Center),
    )
    .padding(1)
    .style(|_| boxed(INK, LINE, 8.0))
    .into()
}

/// The card hung under the title bar's right end: a backdrop over the window that closes it, the
/// card at the top right (a click on the card itself stays on it).
fn popover_card<'a>(card: impl Into<Element<'a, Message>>, width: f32, brain: &Brain) -> Element<'a, Message> {
    let top = crate::chrome::TITLEBAR_HEIGHT / brain.zoom() + 4.0;
    let card = container(card)
        .width(Length::Fixed(width))
        .padding(6)
        .clip(true)
        .style(|_| container::Style {
            shadow: iced::Shadow { color: Color::from_rgba8(0, 0, 0, 0.6), offset: iced::Vector::new(0.0, 12.0), blur_radius: 34.0 },
            ..boxed(style::MENU_BG, style::MENU_LINE, 10.0)
        });
    let card = mouse_area(card).on_press(Message::PopupClicked);
    let backdrop = mouse_area(Space::new().width(Fill).height(Fill)).on_press(Message::CloseChanges).interaction(iced::mouse::Interaction::Idle);
    stack![
        backdrop,
        container(card).width(Fill).height(Fill).align_x(iced::Right).align_y(iced::Top).padding(Padding { top, right: 14.0, ..Padding::ZERO }),
    ]
    .into()
}

/// The list of changed files under the title bar, over the whole window. A file closes the list
/// and opens its diff.
fn changes_popover_layer(brain: &Brain) -> Element<'_, Message> {
    let Some(cache) = brain.selected_cache() else { return Space::new().into() };
    let files = &cache.files;
    let (added, removed) = change_sums(files);
    let count = files.len();
    let title = match count {
        0 => t("Keine Änderungen", "No changes").to_string(),
        1 => t("1 Datei geändert", "1 file changed").to_string(),
        _ => tr!("{count} Dateien geändert", "{count} files changed"),
    };
    let head = row![text(title).size(12).color(TEXT_MUTED).font(style::semibold()), Space::new().width(Fill), counts(Some(added), Some(removed))]
        .spacing(8)
        .align_y(iced::Center);
    let mut top = column![container(head).padding([6, 8])];
    if let Some(merge) = &cache.merge {
        let target = merge.target.as_deref().unwrap_or("?");
        let line = format!("{} → {}", crate::format::clip_branch(&merge.branch, 28), crate::format::clip_branch(target, 28));
        top = top.push(container(text(line).size(11.5).color(TEXT_FAINT).font(MONO).wrapping(text::Wrapping::None)).padding([0, 8]));
    }
    let in_order = brain.changes_in_order();
    let open = brain.preview.as_ref().map(|p| p.change.path.as_str());
    let merged = cache.merge.is_some();
    let rows = in_order.into_iter().map(|change| {
        let mark: Element<'_, Message> = if change.from_session && merged { session_mark() } else { Space::new().width(6).into() };
        let mut line = row![
            container(mark).width(10),
            text(clip_start(&change.relative, 42)).size(12).color(TEXT).font(MONO).wrapping(text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(iced::Center);
        if let Some((label, color)) = status_label(change) {
            line = line.push(badge(label, color, 9.5));
        }
        line = line.push(Space::new().width(Fill)).push(counts(change.added, change.removed));
        let line = container(line).height(CHANGE_ROW_H).center_y(CHANGE_ROW_H).padding([0, 8]).style(move |_| {
            boxed(if open == Some(change.path.as_str()) { alpha(Color::WHITE, 0x10) } else { Color::TRANSPARENT }, Color::TRANSPARENT, 6.0)
        });
        row_button(line, Message::OpenPreview(change.path.clone()))
    });
    let body: Element<'_, Message> = if count == 0 {
        let text = match cache.merge.as_ref().and_then(|m| m.target.as_deref()) {
            Some(target) => tr!("Gegenüber {target} hat sich nichts geändert.", "Nothing differs from {target}."),
            None => t("Kein Ziel gewählt: Brain zeigt nur, was nicht committet ist.", "No target chosen: Brain shows only what isn't committed.").to_string(),
        };
        hint(text)
    } else {
        scrollable(column(rows.collect::<Vec<_>>()).width(Fill)).style(scrollbar).into()
    };
    let tip = if merged {
        row![session_mark(), text(t("von dieser Session  ·  Klick oder Leertaste zeigt die Datei groß", "from this session  ·  click or space shows the file large")).size(11).color(TEXT_MUTED).font(UI)]
            .spacing(6)
            .align_y(iced::Center)
    } else {
        row![text(t("Klick oder Leertaste zeigt die Datei groß", "Click or space shows the file large")).size(11).color(TEXT_MUTED).font(UI)]
    };
    let card = column![top, body, container(tip).padding([4, 8])].spacing(4);
    popover_card(container(card).max_height(CHANGE_ROW_H * 12.0 + 70.0), 460.0, brain)
}

/// The branches to choose the merge request's target from: local first, remote after, the current
/// target marked; a search field when there are many.
fn targets_popover_layer(brain: &Brain) -> Element<'_, Message> {
    let Some(cache) = brain.selected_cache() else { return Space::new().into() };
    let current = cache.merge.as_ref().and_then(|m| m.target.as_deref());
    let query = brain.target_query.trim().to_lowercase();
    let shown: Vec<&String> = cache.choices.iter().filter(|name| query.is_empty() || name.to_lowercase().contains(&query)).collect();
    let mut card = column![container(text(t("Ziel-Branch wählen", "Choose the target branch")).size(12).color(TEXT_MUTED).font(style::semibold())).padding([6, 8])].spacing(4);
    if cache.choices.len() > app::TARGETS_WITH_SEARCH || !query.is_empty() {
        let field = text_input(t("Branches durchsuchen", "Search branches"), &brain.target_query)
            .id(app::TARGET_SEARCH)
            .on_input(Message::TargetQuery)
            .size(12.5)
            .font(UI)
            .padding([6, 8])
            .style(field_style);
        card = card.push(container(field).padding([0, 2]));
    }
    let rows = shown.into_iter().map(|name| {
        let chosen = current == Some(name.as_str());
        let line = row![
            text(if chosen { "✓" } else { "" }).size(12).color(DONE).font(UI).width(14),
            text(clip_start(name, 46)).size(12).color(if chosen { TEXT_STRONG } else { TEXT }).font(MONO).wrapping(text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(iced::Center);
        row_button(container(line).height(CHANGE_ROW_H).center_y(CHANGE_ROW_H).padding([0, 8]), Message::PickTarget(name.clone()))
    });
    let rows: Vec<Element<'_, Message>> = rows.collect();
    if rows.is_empty() {
        let none = if cache.choices.is_empty() { t("Keine anderen Branches.", "No other branches.") } else { t("Kein Branch passt.", "No branch matches.") };
        card = card.push(hint(none));
    } else {
        card = card.push(scrollable(column(rows).width(Fill)).style(scrollbar));
    }
    popover_card(container(card).max_height(CHANGE_ROW_H * 10.0 + 90.0), 340.0, brain)
}

/// The sheet's head: which branch goes where.
fn merge_line<'a>(merge: Option<&brain_core::changes::MergeInfo>) -> Element<'a, Message> {
    let Some(merge) = merge else { return Space::new().into() };
    let target = merge.target.as_deref().map_or_else(|| "?".to_string(), |t| crate::format::clip_branch(t, 24));
    row![
        text(crate::format::clip_branch(&merge.branch, 32)).size(12).color(TEXT_STRONG).font(style::semibold()).wrapping(text::Wrapping::None),
        text("→").size(12).color(TEXT_MUTED).font(UI),
        text(target).size(12).color(TEXT_STRONG).font(style::semibold()).wrapping(text::Wrapping::None),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

/// A file's line in the list of changes.
const CHANGE_ROW_H: f32 = 28.0;

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
    container(
        container(if editing { row![text("⌕").size(13).color(TEXT_FAINT), field, kbd("esc")] } else { row![text("⌕").size(13).color(TEXT_FAINT), field] }.spacing(6).align_y(iced::Center))
            .padding([0, 10])
            .style(move |_| boxed(INK, edge, 8.0)),
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

/// The tree: the projects as headings, their sessions below, the others at the end.
fn tree<'a>(brain: &'a Brain, groups: &Groups<'a>, now: i64) -> Element<'a, Message> {
    let searching = brain.searching();
    let tree = brain.tree(groups);
    let node = |node: Node<'_, 'a>| -> Element<'a, Message> {
        match node {
            Node::Hint(true) => {
                let query = brain.search.clone();
                hint(tr!("Keine Session passt zu „{query}“. Esc leert die Suche.", "No session matches “{query}”. Esc clears the search."))
            }
            Node::Hint(false) => hint(t("Noch kein Projekt. Lege eins an, oder starte mit ⌘N eine Session.", "No project yet. Add one, or start a session with ⌘N.")),
            Node::Project(project) => project_row(brain, project),
            Node::Session(row) => session_row(brain, row.session, row.mark, now),
            Node::NotesHead { project, count, open } => notes_head(project, count, open),
            Node::Note { note } => note_row(note, tree_inner(brain)),
            Node::OthersHead { count } => others_head(count),
            Node::Footer => tree_footer(brain, &tree),
            Node::NewProject => new_project_row(),
        }
    };
    // Each project (and "Weitere") on an island of its own; the rest stands between them.
    let items: Vec<Element<'a, Message>> = tree::blocks(tree.nodes(searching))
        .into_iter()
        .map(|block| match block {
            tree::Block::Island(lines) => island(column(lines.into_iter().map(node)).spacing(tree::GAP)),
            tree::Block::Alone(line) => node(line),
        })
        .collect();
    // A thin scroller inside the tree's right padding: it floats, but never over a row.
    let bar = scrollable::Scrollbar::new().width(5).scroller_width(5).margin(3);
    scrollable(column(items).spacing(tree::BLOCK_GAP).padding(Padding { top: tree::TOP, right: 12.0, bottom: 16.0, left: 12.0 }))
        .id(app::LIST)
        .on_scroll(Message::ListScrolled)
        .direction(scrollable::Direction::Vertical(bar))
        .height(Fill)
        .style(scrollbar)
        .into()
}

/// A project's island: a softly tinted, rounded area under its head, sessions and notes. Its left
/// edge is the search field's.
fn island<'a>(lines: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(lines)
        .width(Fill)
        .padding(Padding { top: tree::ISLAND_PAD_Y, bottom: tree::ISLAND_PAD_Y, left: tree::ISLAND_PAD_X, right: tree::ISLAND_PAD_X })
        .style(|_| boxed(style::ISLAND, Color::TRANSPARENT, 10.0))
        .into()
}

/// Where a session's dot stands, from the island's left edge (12 pt in): under the project's name.
const NEST: f32 = 20.0;
/// Where a session's name starts (its dot and the gap after it): its status line and what stands
/// under it (changes, notes) begin there.
const NAME: f32 = NEST + 16.0;
/// The room a head keeps before its name, where an arrow could stand: the name and the dots of
/// the sessions below it stand on one line. A click on the head folds it; no arrow says so.
const HEAD_GUTTER: f32 = 14.0;
/// The "+" of a project's head.
const PLUS_W: f32 = 20.0;
/// Room kept free at the right end of a session's lines, so text ends before the edge.
const ROW_RIGHT: f32 = 10.0;

/// The width inside an island: the sidebar less the tree's own padding and the island's.
fn tree_inner(brain: &Brain) -> f32 {
    brain.prefs.sidebar_width.unwrap_or(app::SIDEBAR_DEFAULT) - 24.0 - 2.0 * tree::ISLAND_PAD_X
}

/// How many characters of text at `size` fit into `width` points. Iced cuts text hard at the
/// edge (it has no ellipsis), so a line is clipped to what fits, with `…`; SF's average glyph is a
/// little over half its size.
fn fitting(width: f32, size: f32) -> usize {
    (width / (size * 0.56)).max(6.0) as usize
}

/// A tree head's line (project, "Weitere"), centred in its row at the top of the island.
fn head_cell<'a>(line: impl Into<Element<'a, Message>>) -> iced::widget::Container<'a, Message> {
    container(line).height(tree::PROJECT_H).center_y(tree::PROJECT_H).padding(Padding { right: 6.0, ..Padding::ZERO }).width(Fill).clip(true)
}

/// The empty place of a head's fold arrow (there is none; the room stays).
fn head_gutter<'a>() -> Element<'a, Message> {
    Space::new().width(HEAD_GUTTER).height(HEAD_LINE_H).into()
}

/// Wraps a head so the pointer over it is known (its "+" and open arrow show then).
fn head_hover<'a>(head: Element<'a, Message>, key: HeadKey) -> Element<'a, Message> {
    mouse_area(head).on_enter(Message::HoverHead(Some(key))).on_exit(Message::HoverHead(None)).into()
}

/// A project's head: name, and at the right what is going on in it. A click folds it,
/// a right click opens its menu.
fn project_row<'a>(brain: &'a Brain, project: &Project<'_>) -> Element<'a, Message> {
    let name = project.name.clone();
    let color = if project.hidden { TEXT_FAINT } else { TEXT_STRONG };
    let hovered = brain.hovered_head.as_ref() == Some(&HeadKey::Project(name.clone()));
    // Folded, only what calls for you shows: a red dot when a session asks, an amber one when one
    // finished and wasn't looked at. Nothing else (counts, notes, "resting") helps decide.
    let dots: Vec<Color> = if project.folded {
        [(project.asking() > 0).then_some(CALLS), (project.unseen() > 0).then_some(TURN)].into_iter().flatten().collect()
    } else {
        Vec::new()
    };
    // Gutter, gap, "+", the head's right padding, the dots, and "hidden" when it is.
    let extra = dots.len() as f32 * 14.0 + if project.hidden { 70.0 } else { 0.0 };
    let room = tree_inner(brain) - HEAD_GUTTER - 6.0 - PLUS_W - 6.0 - extra;
    let mut line = row![
        head_gutter(),
        text(clip(&name, fitting(room, 15.0))).size(15).color(color).font(style::semibold()).wrapping(text::Wrapping::None),
        Space::new().width(Fill),
    ]
    .spacing(6)
    .height(HEAD_LINE_H)
    .align_y(iced::Center);
    if project.hidden {
        line = line.push(text(t("ausgeblendet", "hidden")).size(11.5).color(TEXT_FAINT).font(UI).wrapping(text::Wrapping::None));
    }
    for dot in dots {
        line = line.push(style::dot::<Message>(dot, 8.0));
    }
    // The "+" keeps its room (and the line its height) when it is out of sight, so nothing moves
    // as the pointer comes and goes.
    line = line.push(if hovered {
        Element::from(
            button(container(text("+").size(14).font(UI)).center(Fill))
                .padding(0)
                .width(PLUS_W)
                .height(HEAD_LINE_H)
                .on_press(Message::NewSessionIn(name.clone()))
                .style(|_, status| button::Style {
                    text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) { TEXT_STRONG } else { TEXT_MUTED },
                    border: iced::border::rounded(4),
                    ..button::Style::default()
                }),
        )
    } else {
        Element::from(Space::new().width(PLUS_W).height(HEAD_LINE_H))
    });
    let head = mouse_area(head_cell(line))
        .on_press(Message::ToggleProjectFolded(name.clone()))
        .on_right_press(Message::ProjectMenu(name.clone()))
        .interaction(iced::mouse::Interaction::Pointer);
    head_hover(head.into(), HeadKey::Project(name))
}

/// The height of a head's line: fixed, so the "+" and the arrow coming and going move nothing.
const HEAD_LINE_H: f32 = 20.0;

/// A session under its project: state dot and name. A line below
/// only when it asks (red, with a ring) or has finished unseen (amber); the rest on hover.
fn session_row<'a>(brain: &'a Brain, s: &'a Session, mark: Mark, now: i64) -> Element<'a, Message> {
    let selected = brain.selected.as_ref() == Some(&s.key);
    let hovered = brain.hovered.as_ref() == Some(&s.key);
    let ended = mark == Mark::Ended;
    let asking = matches!(mark, Mark::Asks(_));
    let name_color = match (ended, selected) {
        (true, _) => TEXT_FAINT,
        (false, true) => TEXT_STRONG,
        (false, false) => TEXT,
    };
    let pinned = brain.prefs.is_pinned(&s.key);
    let conflict = brain.conflicts.get(&s.key).is_some_and(|c| !c.shared_files.is_empty());
    // The name gets what the dot and the markers leave. No time: how long something lies there
    // helps nobody decide what to do next.
    let waiting_review = brain.review_waits(&s.key);
    let name_room = tree_inner(brain) - NAME - ROW_RIGHT - 18.0 * (pinned as u8 + conflict as u8 + waiting_review as u8) as f32;
    let mut head = row![
        state_dot(s, mark),
        text(clip(&s.display_name(), fitting(name_room, 13.0))).size(13).color(name_color).font(if selected { style::medium() } else { UI }).wrapping(text::Wrapping::None),
        Space::new().width(Fill),
    ]
    .spacing(8)
    .align_y(iced::Center);
    if pinned {
        head = head.push(marker("★", TURN));
    }
    if conflict {
        head = head.push(marker("⚠", CALLS));
    }
    if waiting_review {
        head = head.push(review::waiting_sign());
    }
    let mut body = column![container(head).height(tree::ROW_H).center_y(tree::ROW_H)];
    if let Some((prefix, line)) = status_line(s, mark) {
        let prefix_w = prefix.as_ref().map_or(0.0, |p| p.chars().count() as f32 * 12.0 * 0.56 + 3.0);
        let room = tree_inner(brain) - NAME - ROW_RIGHT - prefix_w;
        let line = text(clip(&line, fitting(room, 12.0))).size(12).color(TEXT_MUTED).font(UI).wrapping(text::Wrapping::None);
        // "Frage:" and "Freigabe:" stand out in red; what follows is as quiet as any line.
        let line: Element<'a, Message> = match prefix {
            Some(prefix) => row![text(prefix).size(12).color(CALLS_SOFT).font(UI).wrapping(text::Wrapping::None), line].spacing(3).into(),
            None => line.into(),
        };
        body = body.push(
            container(line)
                .height(tree::STATUS_H)
                .align_y(iced::Top)
                .padding(Padding { left: NAME - NEST, ..Padding::ZERO }),
        );
    }
    if mark == Mark::Asks(Ask::Permission) {
        let answer = |allow: bool| Message::Steer(Command::Permission { key: s.key.clone(), allow, since_ms: s.phase_since_ms() });
        body = body.push(
            container(row![answer_button(t("Erlauben", "Allow"), Answer::Yes, answer(true)), answer_button(t("Ablehnen", "Deny"), Answer::No, answer(false))].spacing(8))
                .height(tree::BUTTONS_H)
                .align_y(iced::Top)
                .padding(Padding { left: NAME - NEST, ..Padding::ZERO }),
        );
    }
    let background = if selected { style::ISLAND_SELECTED } else if asking { alpha(CALLS, 0x24) } else if hovered { style::ISLAND_HOVER } else { Color::TRANSPARENT };
    let content = container(body)
        .height(tree::row_height(mark))
        .width(Fill)
        .padding(Padding { left: NEST, right: ROW_RIGHT, ..Padding::ZERO })
        .clip(true)
        .style(move |_| boxed(background, Color::TRANSPARENT, 6.0));
    iced::widget::tooltip(clickable(content.into(), s), tooltip_card(brain, s, now), iced::widget::tooltip::Position::Right).gap(8).into()
}

/// The dot in front of a session. Solid in the phase's colour; a ring for a turn that was
/// looked at, a darker ring for one that rests.
fn state_dot<'a>(s: &Session, mark: Mark) -> Element<'a, Message> {
    let phase = s.phase();
    let color = phase_color(phase);
    let ring = |color: Color| -> Element<'a, Message> {
        container(Space::new().width(8).height(8))
            .style(move |_| container::Style { border: Border { color, width: 1.5, radius: 4.0.into() }, ..container::Style::default() })
            .into()
    };
    match (phase, mark) {
        (Phase::YourTurn, Mark::Calm) => ring(TEXT_MUTED),
        (Phase::Asleep, _) => ring(color),
        _ => style::dot(color, 8.0),
    }
}

/// What a session that asks or finished unseen says under its name: a lead word for what is
/// asked (if any), and the line itself.
fn status_line(s: &Session, mark: Mark) -> Option<(Option<String>, String)> {
    let said = |fallback: &str| plain(&s.closing_line().unwrap_or_else(|| fallback.to_string()));
    match mark {
        Mark::Asks(Ask::Permission) => {
            let what = s.insight.tool.clone().or_else(|| s.headline()).map(|h| plain(&h)).unwrap_or_default();
            Some((Some(format!("{}:", t("Freigabe", "Permission"))), what))
        }
        Mark::Asks(Ask::Options) | Mark::Asks(Ask::Question) | Mark::Asks(Ask::Maybe) => {
            let what = plain(&s.headline().unwrap_or_else(|| t("wartet auf deine Eingabe", "waiting for your input").into()));
            Some((Some(format!("{}:", t("Frage", "Question"))), what))
        }
        Mark::Guess => Some((None, format!("“{}”", said("")))),
        Mark::Unseen => Some((None, said(t("Fertig, du bist dran", "Done, your turn")))),
        Mark::Calm | Mark::Ended => None,
    }
}

/// A button on a card that asks: neutral, so the red of the card stays the one signal. Deny
/// keeps a fine edge.
/// How an answer button looks: the one that says yes in blue, the one that says no with an edge,
/// the rest plain.
#[derive(Clone, Copy, PartialEq)]
enum Answer {
    Yes,
    No,
    Plain,
}

fn answer_button<'a>(label: &str, kind: Answer, message: Message) -> Element<'a, Message> {
    button(style::centered(text(label.to_string()).size(12).color(TEXT_STRONG).font(style::semibold())))
        .padding([0, 10])
        .height(24)
        .on_press(message)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let background = match (kind, hovered) {
                (Answer::Yes, false) => style::PRIMARY,
                (Answer::Yes, true) => alpha(style::PRIMARY, 0xdd),
                (_, false) => RAISED,
                (_, true) => LINE_STRONG,
            };
            let edged = kind == Answer::No;
            button::Style {
                background: Some(Background::Color(background)),
                text_color: TEXT_STRONG,
                border: Border { color: if edged { LINE_STRONG } else { Color::TRANSPARENT }, width: if edged { 1.0 } else { 0.0 }, radius: 6.0.into() },
                ..button::Style::default()
            }
        })
        .into()
}

/// The details of a session on hover: what it is about, where it runs, its markers.
fn tooltip_card<'a>(brain: &'a Brain, s: &'a Session, now: i64) -> Element<'a, Message> {
    let phase = s.phase();
    let about = s.headline().map(|h| plain(&h)).unwrap_or_else(|| match phase {
        Phase::Background => background_summary(s.background_tasks()),
        Phase::Asleep | Phase::Ended => ago_phrase(s.last_activity_ms, now),
        _ => ago_phrase(s.phase_since_ms(), now),
    });
    let folder = s.cwd.as_deref().map(tilde).unwrap_or_default();
    let mut card = column![
        text(clip(&s.display_name(), 60)).size(12.5).color(TEXT_STRONG).font(style::semibold()),
        text(clip(&about, 140)).size(12).color(TEXT).font(UI),
        text(format!("{} · {}", clip(&folder, 48), phase_word(phase))).size(11).color(TEXT_MUTED).font(UI),
    ]
    .spacing(4)
    .max_width(380);
    let markers = markers(brain, s);
    if !markers.is_empty() {
        card = card.push(row(markers).spacing(8).wrap());
    }
    container(card).padding([8, 10]).style(|_| boxed(RAISED, LINE_STRONG, 8.0)).into()
}

/// `+18 −2`; what git doesn't know is left out.
fn counts<'a>(added: Option<u64>, removed: Option<u64>) -> Element<'a, Message> {
    let mut line = row![].spacing(5);
    if let Some(added) = added.filter(|a| *a > 0 || removed.unwrap_or(0) == 0) {
        line = line.push(text(format!("+{added}")).size(11).color(DONE).font(MONO));
    }
    if let Some(removed) = removed.filter(|r| *r > 0) {
        line = line.push(text(format!("−{removed}")).size(11).color(CALLS_SOFT).font(MONO));
    }
    line.into()
}

fn notes_head<'a>(project: Option<&str>, count: usize, open: bool) -> Element<'a, Message> {
    let label = text(format!("{} · {count}", t("Für später", "For later"))).size(12).color(TEXT_MUTED).font(UI).wrapping(text::Wrapping::None);
    // The arrow stands in the dots' column, the text flush under the session names.
    let line: Element<'a, Message> = if project.is_some() {
        row![Space::new().width(NEST - 1.0), style::chevron(!open, 10.0, TEXT_FAINT), Space::new().width(NAME - NEST - 9.0), label].align_y(iced::Center).into()
    } else {
        row![Space::new().width(NAME), label].into()
    };
    let head = container(line).height(tree::NOTE_H).center_y(tree::NOTE_H);
    match project {
        Some(project) => mouse_area(head).on_press(Message::ToggleNotes(project.to_string())).interaction(iced::mouse::Interaction::Pointer).into(),
        None => head.into(),
    }
}

/// A note for later: a click starts it as a new session in its folder, the check-off removes it.
fn note_row<'a>(note: &later::Item, inner: f32) -> Element<'a, Message> {
    let id = note.id.clone();
    let start = mouse_area(text(clip(&note.text, fitting(inner - NAME - 30.0, 12.0))).size(12).color(TEXT_MUTED).font(UI).wrapping(text::Wrapping::None))
        .on_press(Message::StartNote(id.clone()))
        .interaction(iced::mouse::Interaction::Pointer);
    let hint = container(text(t("Klick startet die Aufgabe als neue Session", "Click starts the task as a new session")).size(11.5).color(TEXT).font(UI))
        .padding([5, 8])
        .style(|_| boxed(RAISED, LINE_STRONG, 6.0));
    let line = row![
        iced::widget::tooltip(start, hint, iced::widget::tooltip::Position::Right).gap(6),
        Space::new().width(Fill),
        button(text("✓").size(11).color(TEXT_FAINT).font(UI))
            .padding([0, 6])
            .on_press(Message::CheckOffNote(id))
            .style(|_, status| button::Style {
                text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) { DONE } else { TEXT_FAINT },
                ..button::Style::default()
            }),
    ]
    .align_y(iced::Center);
    container(line).height(tree::NOTE_H).center_y(tree::NOTE_H).padding(Padding { left: NAME, right: 4.0, ..Padding::ZERO }).into()
}

/// "Weitere": sessions of folders that are not projects of the tree.
fn others_head<'a>(count: usize) -> Element<'a, Message> {
    let label = row![head_gutter(), text(format!("{} · {count}", t("Weitere", "Others"))).size(15).color(TEXT_MUTED).font(style::semibold())]
    .spacing(6)
    .align_y(iced::Center);
    let head = mouse_area(head_cell(label)).on_press(Message::ToggleSection("others")).interaction(iced::mouse::Interaction::Pointer);
    head_hover(head.into(), HeadKey::Others)
}

/// The folded lists at the end of the tree, each one click from open: nothing leaves the tree
/// without a trace.
fn tree_footer<'a>(brain: &'a Brain, tree: &Tree<'_>) -> Element<'a, Message> {
    let mut line = row![].spacing(4).align_y(iced::Center);
    let arrow = |open: bool| if open { "▾" } else { "▸" };
    let toggle = |label: String, open: bool, message: Message| style::text_button(label, open, message);
    if tree.paused.count > 0 {
        let (n, a) = (tree.paused.count, arrow(tree.paused.open));
        line = line.push(toggle(tr!("{a} {n} pausiert", "{a} {n} snoozed"), tree.paused.open, Message::ToggleSnoozed));
    }
    if tree.background.count > 0 {
        let (n, a) = (tree.background.count, arrow(tree.background.open));
        line = line.push(toggle(tr!("{a} {n} im Hintergrund", "{a} {n} in background"), tree.background.open, Message::ToggleBackground));
    }
    if tree.automated.count > 0 {
        let (n, a) = (tree.automated.count, arrow(tree.automated.open));
        line = line.push(toggle(tr!("{a} {n} automatisch", "{a} {n} automated"), tree.automated.open, Message::ToggleAutomated));
    }
    if tree.older.count > 0 {
        let (n, a) = (tree.older.count, arrow(tree.older.open));
        line = line.push(toggle(tr!("{a} {n} älter", "{a} {n} older"), tree.older.open, Message::ToggleOlder));
    }
    let hidden = tree.hidden_projects;
    if hidden > 0 {
        let a = arrow(brain.show_hidden_projects);
        let label = if hidden == 1 { tr!("{a} 1 Projekt ausgeblendet", "{a} 1 project hidden") } else { tr!("{a} {hidden} Projekte ausgeblendet", "{a} {hidden} projects hidden") };
        line = line.push(toggle(label, brain.show_hidden_projects, Message::ToggleHiddenProjects));
    }
    container(row(std::iter::once(line.wrap().into()))).padding(Padding { top: 6.0, ..Padding::ZERO }).into()
}

fn new_project_row<'a>() -> Element<'a, Message> {
    let label = container(text(t("+ Neues Projekt …", "+ New project …")).size(13).font(UI)).height(Fill).center_y(Fill);
    button(label)
        .padding(Padding { left: tree::ISLAND_PAD_X + NEST, right: 6.0, ..Padding::ZERO })
        .height(tree::PROJECT_H)
        .width(Fill)
        .on_press(Message::NewProject)
        .style(|_, status| button::Style {
            text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) { TEXT } else { TEXT_MUTED },
            border: iced::border::rounded(5),
            ..button::Style::default()
        })
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

/// The phase in a word, as the phone writes it under a session.
fn phase_word(phase: Phase) -> &'static str {
    match phase {
        Phase::NeedsYou => t("braucht dich", "needs you"),
        Phase::YourTurn => t("du bist dran", "your turn"),
        Phase::Working => t("arbeitet", "working"),
        Phase::Background => t("im Hintergrund", "in the background"),
        Phase::Asleep => t("schläft – wacht auf, wenn du schreibst", "asleep – wakes when you write"),
        Phase::Ended => t("beendet", "ended"),
    }
}

/// The five-hour limit runs out at the pace of its window so far.
fn five_hour_runs_out(limit: brain_core::usage::Limit, now_ms: i64) -> Option<i64> {
    limit.runs_out_at(brain_core::usage::FIVE_HOURS, now_ms / 1000)
}

/// The weekly limit runs out at the pace of the last 24 hours.
fn seven_day_runs_out(limit: brain_core::usage::Limit, history: &[brain_core::usage::Sample], now_ms: i64) -> Option<i64> {
    limit.runs_out_at_recent(brain_core::usage::SEVEN_DAYS, now_ms / 1000, history)
}

// ---- detail -----------------------------------------------------------------------

fn detail(brain: &Brain) -> Element<'_, Message> {
    let partner = brain.split.as_ref().filter(|k| Some(*k) != brain.selected.as_ref()).and_then(|k| brain.model.board.get(k));
    match partner {
        Some(other) => row![
            container(other_pane(brain, other)).width(Fill).height(Fill),
            container(Space::new().width(1).height(Fill)).style(|_| style::fill(LINE)),
            container(session_pane(brain)).width(Fill).height(Fill),
        ]
        .height(Fill)
        .into(),
        None => session_pane(brain),
    }
}

/// The split's other session: its header and its terminal. A click (or typing into it) makes
/// it the active one.
fn other_pane<'a>(brain: &'a Brain, s: &'a Session) -> Element<'a, Message> {
    let head = mouse_area(header(brain, s, Some(Message::ToggleSplit))).on_press(Message::ActivatePane).interaction(iced::mouse::Interaction::Pointer);
    column![head, terminal_or_hint(brain, s)].spacing(2).padding(Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 12.0 }).height(Fill).into()
}

/// The selected session: its slim header line and its terminal, which gets all the rest.
fn session_pane(brain: &Brain) -> Element<'_, Message> {
    let Some(s) = brain.selected_session() else {
        return container(
            column![
                text(t("Keine Session ausgewählt", "No session selected")).size(13).color(TEXT).font(UI),
                text(t("Wähle links eine Session aus oder starte eine neue mit ⌘N.", "Pick a session on the left, or start a new one with ⌘N."))
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
    let content: Element<'_, Message> = if !brain.find_open {
        terminal(brain, s)
    } else if brain.find.trim().is_empty() {
        column![find_bar(brain), terminal(brain, s)].spacing(8).height(Fill).into()
    } else {
        column![find_bar(brain), whole_session(brain)].spacing(8).height(Fill).into()
    };
    // Its line stands in the title bar; the terminal gets the whole height.
    container(content).padding(Padding { top: 8.0, right: 12.0, bottom: 0.0, left: 12.0 }).height(Fill).into()
}

/// Quick Look: one changed file of the session, large over the whole window. The diff has line
/// numbers, hunks say where they are; ← → page through the session's files, space and esc close.
/// A click beside the sheet closes it; the sheet swallows its own clicks.
fn quick_look(brain: &Brain) -> Element<'_, Message> {
    use app::DiffRow;
    use brain_core::changes::DiffKind;
    let Some(preview) = brain.preview.as_ref().filter(|_| brain.mode == Mode::Normal && !brain.overview) else { return Space::new().into() };
    let font = crate::chat_font::get();
    let change = &preview.change;
    let files = brain.changes_in_order();
    let (index, total) = brain.preview_position().unwrap_or((0, 1));
    let diff = preview.diffs.get(&change.path);
    let merge = brain.selected_cache().and_then(|c| c.merge.as_ref());
    // The review comments: under their lines in the diff, at the file's head, in the list.
    let reviewing = brain.review_of(&preview.key);
    let file_order: Vec<String> = files.iter().map(|c| c.path.clone()).collect();
    let placed = reviewing.map(|r| app::place_comments(r, &change.path, diff, &file_order)).unwrap_or_default();
    let editing = brain.review_editor.as_ref();
    let open_comments: Vec<&brain_core::review::Comment> = reviewing.map(|r| r.comments.iter().filter(|c| c.is_open()).collect()).unwrap_or_default();
    let open_comments = brain_core::review::ordered(&open_comments, &file_order);
    let numbered = |c: &brain_core::review::Comment| open_comments.iter().position(|x| x.id == c.id).map(|i| i + 1);

    let step = |glyph: &'static str, delta: isize, enabled: bool| -> Element<'static, Message> {
        button(style::centered(text(glyph).size(13).color(TEXT).font(UI)))
            .width(28)
            .height(28)
            .padding([0, 8])
            .on_press_maybe(enabled.then_some(Message::PreviewStep(delta)))
            .style(move |_, status| {
                let hovered = enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
                let faded = |c: Color| if enabled { c } else { Color { a: c.a * 0.35, ..c } };
                button::Style {
                    background: Some(Background::Color(faded(if hovered { LINE_STRONG } else { RAISED }))),
                    text_color: faded(TEXT),
                    border: Border { color: faded(LINE_STRONG), width: 1.0, radius: 7.0.into() },
                    ..button::Style::default()
                }
            })
            .into()
    };
    let (dir, base) = change.relative.rsplit_once('/').map_or(("", change.relative.as_str()), |(dir, base)| (dir, base));
    let dir = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    let place = index + 1;
    let mut title = row![
        row![step("←", -1, index > 0), step("→", 1, index + 1 < total)].spacing(4),
        text(tr!("{place} von {total}", "{place} of {total}")).size(12).color(TEXT_MUTED).font(UI),
        merge_line(merge),
        row![
            text(clip_start(&dir, 60)).size(14).color(TEXT_MUTED).font(MONO).wrapping(text::Wrapping::None),
            text(base.to_string()).size(14).color(TEXT_STRONG).font(MONO).wrapping(text::Wrapping::None),
        ],
    ]
    .spacing(10)
    .align_y(iced::Center);
    match status_label(change) {
        Some((label, color)) => title = title.push(badge(label, color, 10.5)),
        None if diff.is_some_and(|d| d.new_file) => title = title.push(badge(t("neu", "new"), DONE, 10.5)),
        None => {}
    }
    if let Some(original) = &change.original {
        title = title.push(text(format!("← {}", clip_start(original, 40))).size(12).color(TEXT_FAINT).font(MONO).wrapping(text::Wrapping::None));
    }
    title = title.push(counts(change.added, change.removed));
    title = title.push(Space::new().width(Fill));
    title = title.push(action(t("Diff kopieren", "Copy diff"), "⌘C", false, diff.is_some_and(|d| !d.raw.is_empty()).then_some(Message::CopyDiff)));
    title = title.push(action(t("Im Editor öffnen", "Open in editor"), "⌘⏎", false, preview.exists.then_some(Message::PreviewInEditor)));
    title = title.push(action(t("Schließen", "Close"), "esc", false, Some(Message::ClosePreview)));
    let head = mouse_area(container(title).padding([12, 16]).width(Fill).style(|_| style::fill(RAISED))).on_press(Message::SheetHead);
    // The sentence Claude said before writing the file, and the button for a comment on the whole
    // file; under them the cards of those comments and the field for a new one.
    let head_cards: Vec<(&brain_core::review::Comment, bool)> = placed.head.clone();
    let mut strip_items = review::cards(&head_cards, editing, &numbered);
    if let Some(field) = editing.filter(|e| e.edit_id.is_none() && e.place == brain_core::review::Place::File) {
        strip_items.push(review::editor(field));
    }
    let said_text: Element<'_, Message> = match change.said.as_ref() {
        Some(said) => text(said.clone()).size(13).color(TEXT).font(UI).width(Fill).into(),
        None => Space::new().width(Fill).into(),
    };
    let mut strip = column![row![said_text, review::file_button(Some(t("Zur Datei kommentieren", "Comment on the file").to_string()), index)].spacing(14).align_y(iced::Center)].spacing(10);
    if !strip_items.is_empty() {
        strip = strip.push(column(strip_items).spacing(6));
    }
    let said: Element<'_, Message> = container(strip).padding([10, 16]).width(Fill).style(|_| style::fill(RAISED)).into();

    let number = |n: Option<u32>, color: Color, tint: Color| -> Element<'_, Message> {
        container(text(n.map(|n| n.to_string()).unwrap_or_default()).size(font.size).color(color).font(font.regular))
            .width(46)
            .padding(Padding { right: 9.0, ..Padding::ZERO })
            .align_x(iced::alignment::Horizontal::Right)
            .style(move |_| style::fill(tint))
            .into()
    };
    let body: Element<'_, Message> = match diff {
        None => container(hint(t("Lade den Diff …", "Reading the diff …"))).padding(14).into(),
        Some(diff) if diff.rows.is_empty() => {
            let against = merge.and_then(|m| m.target.clone()).unwrap_or_else(|| "HEAD".to_string());
            container(hint(tr!("Gegenüber {against} hat sich an dieser Datei nichts geändert.", "This file does not differ from {against}."))).padding(14).into()
        }
        Some(diff) => {
            // The rows scroll sideways too, where Fill has no width to fill: they get the width of
            // the longest line (monospace: a glyph is about 0.62 of the size), and at least a wide
            // sheet's, so their tints run across.
            let longest = diff.rows.iter().map(|r| match r {
                DiffRow::Line { text: content, .. } => content.chars().count(),
                DiffRow::Hunk { context, raw, .. } => context.chars().count() + raw.chars().count() + 30,
            });
            let numbers = if diff.new_file { 46.0 } else { 92.0 };
            let gutters = numbers + 20.0 + 18.0 + review::PLUS_W;
            let row_w = Length::Fixed((longest.max().unwrap_or(0) as f32 * font.size * 0.62 + gutters).max(1100.0));
            let mut rows: Vec<Element<'_, Message>> = Vec::with_capacity(diff.rows.len() + 1);
            for (i, entry) in diff.rows.iter().enumerate() {
                match entry {
                    DiffRow::Hunk { from, len, context, raw } => {
                        if i > 0 {
                            rows.push(Space::new().height(10).into());
                        }
                        let place = if diff.new_file { tr!("Neue Datei, {len} Zeilen", "New file, {len} lines") } else { tr!("Ab Zeile {from}", "From line {from}") };
                        let line = row![
                            text(place).size(11.5).color(WORKING).font(style::semibold()).wrapping(text::Wrapping::None),
                            text(context.clone()).size(font.size).color(TEXT_MUTED).font(font.regular).wrapping(text::Wrapping::None),
                            Space::new().width(Fill),
                            text(raw.clone()).size(11).color(TEXT_FAINT).font(font.regular).wrapping(text::Wrapping::None),
                        ]
                        .spacing(10)
                        .align_y(iced::Center);
                        rows.push(container(line).padding([5, 14]).width(row_w).style(|_| style::fill(alpha(WORKING, 0x12))).into());
                    }
                    DiffRow::Line { kind, old, new, text: content } => {
                        let (tint, gutter, number_color, sign) = match kind {
                            DiffKind::Added => (alpha(DONE, 0x1a), alpha(DONE, 0x2b), Color::from_rgb8(0x7f, 0xcf, 0xae), "+"),
                            DiffKind::Removed => (alpha(CALLS, 0x1a), alpha(CALLS, 0x2b), Color::from_rgb8(0xe0, 0x9a, 0xa3), "−"),
                            _ => (Color::TRANSPARENT, Color::TRANSPARENT, TEXT_FAINT, ""),
                        };
                        let sign_color = if *kind == DiffKind::Added { DONE } else { CALLS_SOFT };
                        // An empty line still takes its height.
                        let shown = if content.is_empty() { " ".to_string() } else { content.clone() };
                        let hovered = brain.review_hover_row == Some(i);
                        let commented = placed.by_row.get(&i).map(|list| list.iter().any(|c| c.state == brain_core::review::State::Queued));
                        let mut line = row![review::plus(i, hovered, commented)];
                        if !diff.new_file {
                            line = line.push(number(*old, number_color, gutter));
                        }
                        line = line.push(number(*new, number_color, gutter));
                        line = line.push(container(text(sign).size(font.size).color(sign_color).font(font.regular)).width(20).align_x(iced::Center));
                        line = line.push(text(shown).size(font.size).color(TEXT).font(font.regular).wrapping(text::Wrapping::None));
                        let line = container(line).width(row_w).padding(Padding { right: 18.0, ..Padding::ZERO }).style(move |_| style::fill(tint));
                        rows.push(mouse_area(line).on_enter(Message::ReviewHoverRow(Some(i))).on_exit(Message::ReviewHoverRow(None)).into());
                        // The cards of the line's comments, and the field for a new one.
                        let here: Vec<(&brain_core::review::Comment, bool)> = placed.by_row.get(&i).map(|list| list.iter().map(|c| (*c, false)).collect()).unwrap_or_default();
                        let mut items = review::cards(&here, editing, &numbered);
                        if let Some(field) = editing.filter(|e| e.edit_id.is_none() && e.row == Some(i)) {
                            items.push(review::editor(field));
                        }
                        if !items.is_empty() {
                            rows.push(review::slot(items, numbers + 20.0 + review::PLUS_W, row_w));
                        }
                    }
                }
            }
            if diff.cut > 0 {
                let cut = diff.cut;
                rows.push(container(hint(tr!("… {cut} weitere Zeilen", "… {cut} more lines"))).padding([6, 14]).into());
            }
            scrollable(column(rows).padding(Padding { top: 8.0, bottom: 12.0, ..Padding::ZERO }))
                .direction(scrollable::Direction::Both { vertical: scrollable::Scrollbar::default(), horizontal: scrollable::Scrollbar::default() })
                .width(Fill)
                .height(Fill)
                .style(scrollbar)
                .into()
        }
    };

    // The list of files on the left, newest first: folder muted under a strong name, the counts and
    // a mark for new files; the open one is lit, a click goes there. Fixed width: dragging the
    // sheet gives the diff the rest.
    let (total_added, total_removed) = files.iter().fold((0, 0), |(a, r), f| (a + f.added.unwrap_or(0), r + f.removed.unwrap_or(0)));
    let file_count = files.len();
    let mut list = column![row![
        text(tr!("{file_count} Dateien", "{file_count} files")).size(12).color(TEXT_MUTED).font(style::semibold()),
        Space::new().width(Fill),
        counts(Some(total_added), Some(total_removed)),
    ]
    .padding([0, 8])
    .spacing(8)
    .align_y(iced::Center)]
    .spacing(2);
    for (i, file) in files.iter().enumerate() {
        let (dir, name) = file.relative.rsplit_once('/').map_or(("", file.relative.as_str()), |(d, n)| (d, n));
        let current = i == index;
        let mut first = row![].spacing(8).align_y(iced::Center);
        if merge.is_some() {
            first = first.push(if file.from_session { session_mark() } else { Space::new().width(6).into() });
        }
        first = first.push(text(name.to_string()).size(12.5).color(TEXT_STRONG).font(MONO).wrapping(text::Wrapping::None));
        if let Some((label, color)) = status_label(file) {
            first = first.push(badge(label, color, 9.5));
        }
        first = first.push(Space::new().width(Fill)).push(counts(file.added, file.removed));
        let mut entry = column![first].spacing(3);
        // The folder, the number of comments on the file, and on hover the button for one more.
        let count = review::file_count(reviewing, &file.path);
        let adding = brain.review_hover_file == Some(i);
        if !dir.is_empty() || count.is_some() || adding {
            let folder = if dir.is_empty() { String::new() } else { clip_start(&format!("{dir}/"), 34) };
            let mut second = row![text(folder).size(11).color(TEXT_FAINT).font(MONO).wrapping(text::Wrapping::None), Space::new().width(Fill)].spacing(6).align_y(iced::Center).height(22);
            if let Some(count) = count {
                second = second.push(count);
            }
            if adding {
                second = second.push(review::file_button(None, i));
            }
            entry = entry.push(second);
        }
        let entry = container(entry).padding([8, 10]).width(Fill).style(move |_| boxed(if current { style::ISLAND_SELECTED } else { Color::TRANSPARENT }, Color::TRANSPARENT, 8.0));
        list = list.push(
            mouse_area(entry)
                .on_press(Message::PreviewGo(i))
                .on_enter(Message::ReviewHoverFile(Some(i)))
                .on_exit(Message::ReviewHoverFile(None))
                .interaction(iced::mouse::Interaction::Pointer),
        );
    }
    let list = container(scrollable(list.padding([12, 10])).width(Fill).height(Fill).style(scrollbar))
        .width(240)
        .height(Fill)
        .style(|_| style::fill(CHROME));
    let mut hints = row![].spacing(6).align_y(iced::Center);
    if merge.is_some() {
        hints = hints.push(session_mark()).push(text(t("von dieser Session", "from this session")).size(11.5).color(TEXT_MUTED).font(UI)).push(Space::new().width(10));
    }
    let hints = hints.push(row![
        kbd("←"),
        kbd("→"),
        text(t("andere Datei", "other file")).size(11.5).color(TEXT_MUTED).font(UI),
        kbd(t("Leertaste", "Space")),
        text(t("oder", "or")).size(11.5).color(TEXT_MUTED).font(UI),
        kbd("esc"),
        text(t("schließt", "closes")).size(11.5).color(TEXT_MUTED).font(UI),
    ]
    .spacing(6)
    .align_y(iced::Center));
    let foot = container(row![Space::new().width(Fill), hints].align_y(iced::Center))
        .height(34)
        .center_y(34)
        .padding([0, 16])
        .width(Fill)
        .style(|_| style::fill(CHROME));
    let edge = || container(Space::new().width(Fill).height(1)).style(|_| style::fill(LINE_STRONG));

    let mut right = column![said, edge()];
    right = right.push(container(body).width(Fill).height(Fill).style(|_| style::fill(INK)));
    let vertical_edge = || container(Space::new().width(1).height(Fill)).style(|_| style::fill(LINE));
    let sheet = container(column![head, edge(), row![list, vertical_edge(), right.width(Fill)].height(Fill), review::bar(brain, &preview.key), foot])
        .width(Fill)
        .height(Fill)
        .clip(true)
        .style(|_| container::Style {
            shadow: iced::Shadow { color: alpha(Color::BLACK, 0xb3), offset: iced::Vector::new(0.0, 30.0), blur_radius: 90.0 },
            ..boxed(INK, alpha(Color::WHITE, 0x1a), 14.0)
        });
    // The sheet sits in a ring of grips: the edges and corners drag its size, symmetrically around
    // the window's middle.
    const GRIP: f32 = 6.0;
    use iced::mouse::Interaction as Cursor;
    let grip = |horizontal: bool, vertical: bool, cursor: Cursor, width: Length, height: Length| -> Element<'_, Message> {
        mouse_area(Space::new().width(width).height(height)).on_press(Message::SheetGrab(horizontal, vertical)).interaction(cursor).into()
    };
    let (g, size) = (Length::Fixed(GRIP), app::sheet_size(brain.prefs.quick_look_size, brain.window_size));
    let ring = column![
        row![
            grip(true, true, Cursor::ResizingDiagonallyDown, g, g),
            grip(false, true, Cursor::ResizingVertically, Fill, g),
            grip(true, true, Cursor::ResizingDiagonallyUp, g, g),
        ]
        .height(g),
        row![grip(true, false, Cursor::ResizingHorizontally, g, Fill), sheet, grip(true, false, Cursor::ResizingHorizontally, g, Fill)].height(Fill),
        row![
            grip(true, true, Cursor::ResizingDiagonallyUp, g, g),
            grip(false, true, Cursor::ResizingVertically, Fill, g),
            grip(true, true, Cursor::ResizingDiagonallyDown, g, g),
        ]
        .height(g),
    ];
    let centered = container(mouse_area(ring).on_press(Message::PreviewClicked))
        .width(Length::Fixed(size.width))
        .height(Length::Fixed(size.height))
        .center(Fill);
    // The backdrop claims the pointer, so what lies under it (the sidebar's tooltips) doesn't
    // think the pointer is over it.
    let backdrop = mouse_area(container(Space::new().width(Fill).height(Fill)).style(|_| style::fill(Color::from_rgba8(6, 8, 15, 0.55))))
        .on_press(Message::ClosePreview)
        .interaction(iced::mouse::Interaction::Idle);
    stack![backdrop, centered].into()
}

/// The line above the split's other terminal (the selected session's stands in the title bar).
/// Everything else on hover.
fn header<'a>(brain: &'a Brain, s: &'a Session, close: Option<Message>) -> Element<'a, Message> {
    let bar = container(header_line(brain, s, close)).height(HEADER_H).center_y(HEADER_H).padding([0, 6]).width(Fill).clip(true);
    iced::widget::tooltip(bar, detail_card(brain, s), iced::widget::tooltip::Position::Bottom).gap(2).into()
}

/// A session's line: project › name, branch, account, how full the context is, a sign when it
/// edits the same files as another session.
fn header_line<'a>(brain: &'a Brain, s: &'a Session, close: Option<Message>) -> Element<'a, Message> {
    let renaming = brain.mode == Mode::Rename && brain.selected.as_ref() == Some(&s.key);
    let name: Element<'a, Message> = if renaming {
        text_input("", &brain.rename)
            .id(app::RENAME)
            .on_input(Message::Rename)
            .on_submit(Message::RenameSubmit)
            .size(13)
            .font(style::semibold())
            .padding([1, 6])
            .width(Length::Fixed(320.0))
            .style(field_style)
            .into()
    } else {
        mouse_area(text(clip(&s.display_name(), 48)).size(13).color(TEXT_STRONG).font(style::semibold()).wrapping(text::Wrapping::None))
            .on_press(Message::Do(Action::StartRename))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    };
    let mut line = row![
        text(clip(&brain.project_name(s), 28)).size(12.5).color(TEXT_MUTED).font(UI).wrapping(text::Wrapping::None),
        text("›").size(12.5).color(TEXT_FAINT).font(UI),
        name,
    ]
    .spacing(8)
    .align_y(iced::Center);
    if renaming {
        line = line.push(text(t("⏎ schickt /rename an die Session, esc bricht ab", "⏎ sends /rename to the session, esc cancels")).size(11.5).color(TEXT_FAINT).font(UI));
    }
    if let Some(branch) = brain.branches.get(&s.key).and_then(|(branch, _)| branch.as_ref()) {
        line = line.push(text(format!("⎇ {}", clip(branch, 32))).size(11.5).color(TEXT_MUTED).font(MONO).wrapping(text::Wrapping::None));
    }
    line = line.push(account_badge(&s.key.account, brain.account_index(&s.key.account)));
    line = line.push(Space::new().width(Fill));
    if let Some(warning) = conflict_text(brain, s) {
        let card = container(text(warning).size(12).color(TEXT).font(UI)).padding([6, 10]).max_width(420).style(|_| boxed(RAISED, LINE_STRONG, 8.0));
        line = line.push(iced::widget::tooltip(text("⚠").size(13).color(CALLS).font(UI), card, iced::widget::tooltip::Position::Bottom).gap(6));
    }
    if let Some(percent) = brain.model.snapshot(&s.key).and_then(|status| status.context_percent) {
        line = line.push(context_gauge(brain, s, percent));
    } else if let Some(tokens) = s.insight.context_tokens {
        let tokens = format_tokens(tokens);
        line = line.push(text(tr!("{tokens} Kontext", "{tokens} context")).size(11.5).color(TEXT_MUTED).font(UI));
    }
    if let Some(close) = close {
        line = line.push(button(text("✕").size(12).color(TEXT_MUTED)).padding([2, 6]).on_press(close).style(|_, _| button::Style::default()));
    }
    line.into()
}

/// The height of the line above a terminal.
const HEADER_H: f32 = 32.0;

/// What the line above a terminal leaves out: folder, model, mode, cost, pid, start, PR, the days
/// an ended session can still be resumed.
fn detail_card<'a>(brain: &'a Brain, s: &Session) -> Element<'a, Message> {
    let info = &s.insight;
    let mut lines: Vec<String> = vec![s.cwd.as_deref().map(tilde).unwrap_or_default()];
    let mut facts: Vec<String> = Vec::new();
    if let Some(model) = &info.model {
        facts.push(short_model(model));
    }
    if let Some(mode) = info.permission_mode.as_deref().filter(|m| *m != "default") {
        facts.push(mode.to_string());
    }
    if let Some(cost) = brain.model.snapshot(&s.key).and_then(|s| s.cost_usd).or(info.cost_usd) {
        facts.push(format!("${cost:.2}"));
    }
    if s.alive {
        facts.push(format!("pid {}", s.pid));
    }
    if let Some(started) = s.started_ms {
        let at = clock(started);
        facts.push(tr!("seit {at}", "since {at}"));
    }
    if !s.alive {
        if let Some(days) = brain.model.days_left(s, now_ms()) {
            facts.push(tr!("noch {days} Tage fortsetzbar", "resumable for {days} more days"));
        }
    }
    if !facts.is_empty() {
        lines.push(facts.join("  ·  "));
    }
    if let Some(pr) = brain.prs.get(&s.key) {
        let (symbol, _) = pr_look(pr);
        lines.push(format!("PR #{} {symbol} {}", pr.number, pr.state.to_lowercase()));
    }
    container(column(lines.into_iter().map(|l| text(l).size(11.5).color(TEXT).font(UI).into())).spacing(3))
        .padding([6, 10])
        .style(|_| boxed(RAISED, LINE_STRONG, 8.0))
        .into()
}

/// "Edits the same files as X: a.rs, b.rs" and "Works in the same checkout as Y", one per line.
fn conflict_text(brain: &Brain, s: &Session) -> Option<String> {
    let conflict = brain.conflicts.get(&s.key)?;
    let name = |key| brain.model.board.get(key).map(|o: &Session| o.display_name()).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for (other, files) in &conflict.shared_files {
        let other = name(other);
        let list = files.iter().map(|f| f.rsplit('/').next().unwrap_or(f)).collect::<Vec<_>>().join(", ");
        out.push(tr!("Bearbeitet dieselben Dateien wie {other}: {list}", "Edits the same files as {other}: {list}"));
    }
    if !conflict.same_checkout.is_empty() {
        let others = conflict.same_checkout.iter().map(name).collect::<Vec<_>>().join(", ");
        out.push(tr!("Ändert Dateien im selben Checkout wie {others}", "Changes files in the same checkout as {others}"));
    }
    (!out.is_empty()).then(|| out.join("\n"))
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

/// A real terminal running the session inside Brain, or why there is none.
/// The session's terminal; split (⌘D), the kept session's terminal beside it; an open file shows
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
            "Diese Session läuft in einem iTerm-Tab. „Nach Brain holen“ (Rechtsklick auf die Session, oder I) holt sie hierher.",
            "This session runs in an iTerm tab. “Move into Brain” (right-click the session, or I) brings it here.",
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

/// ⌘F: find in the whole session. With text in the field, the matches replace the terminal.
fn find_bar(brain: &Brain) -> Element<'_, Message> {
    let field = text_input(t("In der ganzen Session suchen", "Find in the whole session"), &brain.find)
        .id(app::FIND)
        .on_input(Message::Find)
        .size(12.5)
        .font(UI)
        .padding([6, 2])
        .style(|theme, status| text_input::Style { border: Border::default(), background: Background::Color(Color::TRANSPARENT), ..field_style(theme, status) });
    let active = !brain.find.is_empty();
    container(row![text("⌕").size(13).color(TEXT_FAINT), field, kbd("esc".to_string())].spacing(6).align_y(iced::Center))
        .padding([0, 10])
        .width(Fill)
        .style(move |_| boxed(INK, if active { alpha(WORKING, 0x99) } else { LINE }, 8.0))
        .into()
}

/// Matches in the whole conversation; a click opens one fully.
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
    let indices: Vec<usize> = brain_core::history::find(messages, query);
    let now = now_ms();
    let found = indices.len();
    let summary = tr!("{found} Treffer", "{found} matches");
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
                column(markdown::conversation(std::slice::from_ref(message))).spacing(8),
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
    // The output style: as the account's settings say, or one picked here (⇧⇥).
    let mut styles = vec![(t("wie eingestellt", "as configured").to_string(), dialog.style.is_none(), Message::NewStyle(None))];
    styles.extend(brain.new_session_styles().into_iter().map(|s| (s.name.clone(), dialog.style.as_deref() == Some(s.name.as_str()), Message::NewStyle(Some(s.name)))));
    content = content.push(row![text(t("Profil", "Style")).size(12.5).color(TEXT_MUTED).font(UI), segmented(styles)].spacing(10).align_y(iced::Center));
    if let Some(c) = chosen.filter(|c| !c.template.prompt.is_empty()) {
        let prompt: String = c.template.fill(&c.values).lines().take(6).collect::<Vec<_>>().join("\n");
        content = content.push(container(text(prompt).size(12.5).color(TEXT).font(UI).line_height(1.5)).padding([9, 12]).width(Fill).style(|_| boxed(SURFACE, LINE, 8.0)));
    }
    let field = text_input(&placeholder, &dialog.folder)
        .id(app::NEW_SESSION)
        .on_input(Message::NewInput)
        .on_submit(Message::NewSubmit)
        .size(13)
        .font(UI)
        .padding([9, 12])
        .style(field_style);
    // Not while a template asks for a value: the field holds that value then.
    content = content.push(if asking.is_none() {
        row![field, action(t("Ordner wählen …", "Choose folder …"), "⌘O", false, Some(Message::NewChooseFolder))].spacing(8).align_y(iced::Center)
    } else {
        row![field]
    });
    content = content.push(text(t("Oder tippe ~/ und gehe mit → in die Ordner darin.", "Or type ~/ and go into the folders with →.")).size(11.5).color(TEXT_FAINT).font(UI));
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
            text(t("⏎ wählen/starten · ↑↓ · ~/ → Ordner · ⌘O Ordner wählen · ⇥ Konto · ⇧⇥ Profil · ⌘I Brain/iTerm · ⌘E Vorlage bearbeiten · ⌘⇧N neue Vorlage · esc", "⏎ pick/start · ↑↓ · ~/ → folders · ⌘O choose folder · ⇥ account · ⇧⇥ style · ⌘I Brain/iTerm · ⌘E edit template · ⌘⇧N new template · esc"))
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
            name_label(s, 12.5, TEXT_STRONG),
            account_badge(&s.key.account, brain.account_index(&s.key.account)),
            text(kind).size(12.5).color(TEXT_FAINT).font(UI),
            text(ago_phrase(s.last_activity_ms, now)).size(12.5).color(TEXT_MUTED).font(UI),
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
            text(t("H alle ausblenden · ⌘⌫ ⌘⌫ alle in den Papierkorb · ⇥ Alter · esc", "H hide all · ⌘⌫ ⌘⌫ move all to the Trash · ⇥ age · esc"))
                .size(11.5)
                .color(TEXT_FAINT)
                .font(UI),
        )
        .padding(Padding { bottom: 16.0, ..Padding::ZERO }),
    ]
    .spacing(14)
    .padding(Padding { top: 24.0, right: 28.0, bottom: 0.0, left: 28.0 })
    .into()
}

/// `L`: tasks noted for later (`/later …` in a session), started from here as a new session.
fn later(brain: &Brain) -> Element<'_, Message> {
    let now = now_ms();
    let summary = if brain.later.is_empty() {
        t("Nichts für später. In einer Claude-Session: /later <Aufgabe>.", "Nothing for later. In a Claude session: /later <task>.").to_string()
    } else {
        let count = brain.later.len();
        tr!("{count} Aufgaben für später.", "{count} tasks for later.")
    };
    let rows = brain.later.iter().enumerate().map(|(i, (task, _))| {
        let active = i == brain.later_pick;
        let mut place: Vec<String> = Vec::new();
        if let Some(cwd) = &task.cwd {
            place.push(tilde(cwd));
        }
        if let Some(name) = &task.session_name {
            place.push(name.clone());
        }
        place.push(ago_phrase(task.ts.timestamp_millis(), now));
        let mut line = row![
            text(task.text.clone()).size(12.5).color(if active { TEXT_STRONG } else { TEXT }).font(style::semibold()).width(Fill),
            text(place.join(" · ")).size(11.5).color(TEXT_FAINT).font(UI),
        ]
        .spacing(10)
        .align_y(iced::Center);
        if let Some(account) = &task.account {
            line = line.push(account_badge(account, brain.account_index(account)));
        }
        button(line).width(Fill).padding([6, 10]).on_press(Message::LaterPick(i)).style(style::entry_style(active, Color::TRANSPARENT, RAISED, Color::TRANSPARENT, 6.0)).into()
    });
    column![
        dialog_title(t("Für später", "For later")),
        text(summary).size(12.5).color(TEXT_MUTED).font(UI),
        scrollable(column(rows).spacing(2)).height(Fill).style(scrollbar),
        container(
            text(t("⏎ als neue Session starten · ⌫ abhaken · ↑↓ · esc", "⏎ start as a new session · ⌫ check off · ↑↓ · esc"))
                .size(11.5)
                .color(TEXT_FAINT)
                .font(UI),
        )
        .padding(Padding { bottom: 16.0, ..Padding::ZERO }),
    ]
    .spacing(14)
    .padding(Padding { top: 24.0, right: 28.0, bottom: 0.0, left: 28.0 })
    .into()
}

/// "Into another account or fork …" (`A`, `F`): the account (each with how much of its limits is
/// used, so the one with room is easy to spot), whether the original keeps running, flags for the
/// new start, and in one sentence what ⏎ does with that. "Restart with …": only the flags.
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
        Err(why) => (why, false),
    };
    let what = text(what).size(13).line_height(1.5).color(if ready { TEXT } else { TURN }).font(UI);
    let mut body = if dialog.restart {
        column![dialog_title(tr!("„{name}“ neu starten mit …", "Restart “{name}” with …")), field, what]
    } else {
        column![
            dialog_title(tr!("„{name}“ in ein anderes Konto oder forken", "“{name}” into another account or fork")),
            text(t("Konto (1, 2, …)", "Account (1, 2, …)")).size(12).color(TEXT_MUTED).font(style::semibold()),
            column(rows).spacing(6),
            row![original, text(t("⇥ wechselt", "⇥ switches")).size(11.5).color(TEXT_FAINT).font(UI)].spacing(10).align_y(iced::Center),
            field,
            what,
        ]
    }
    .spacing(12)
    .max_width(640);
    if let Some(err) = &dialog.error {
        body = body.push(text(err.clone()).size(12.5).color(CALLS).font(UI));
    }
    let label = if dialog.restart { t("Neu starten", "Restart") } else { t("Weiterführen", "Go on") };
    body = body.push(action(label, "⏎", true, ready.then_some(Message::RelaunchSubmit)));
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

// ---- status popup -----------------------------------------------------------------

/// The popup under the status item, after the mockup's `tpl-status`: who needs you (with Allow,
/// Deny and "Ansehen"), who finished unseen, the accounts' limits. Every piece has the height
/// that `PopupContent::height` counts, so the window fits its content.
fn status_popup(brain: &Brain, now: i64) -> Element<'_, Message> {
    use crate::app::status_popup::*;
    let content = brain.popup_content(now);
    let section = |title: &str| -> Element<'static, Message> {
        container(text(title.to_string()).size(12).color(TEXT_MUTED).font(style::semibold()))
            .height(SECTION_H)
            .align_y(iced::Bottom)
            .padding(Padding { left: 8.0, bottom: 4.0, ..Padding::ZERO })
            .into()
    };
    let more = |count: usize| -> Element<'static, Message> {
        container(text(tr!("… und {count} weitere", "… and {count} more")).size(11.5).color(TEXT_MUTED).font(UI))
            .height(MORE_H)
            .center_y(MORE_H)
            .padding(Padding { left: 8.0, ..Padding::ZERO })
            .into()
    };
    let titled = |name: &str, project: &str| -> Element<'static, Message> {
        container(
            row![
                text(clip(name, 20)).size(13).color(TEXT_STRONG).font(style::semibold()).wrapping(text::Wrapping::None),
                text(clip(project, 14)).size(12).color(TEXT_MUTED).font(UI).wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .height(18)
        .clip(true)
        .into()
    };
    let said = |line: &str| -> Element<'static, Message> {
        container(text(clip(line, fitting(250.0, 12.5))).size(12.5).color(TEXT).font(UI).wrapping(text::Wrapping::None))
            .height(20)
            .align_y(iced::Bottom)
            .clip(true)
            .into()
    };
    let dotted = |color: Color| container(style::dot::<Message>(color, 8.0)).padding(Padding { top: 5.0, ..Padding::ZERO });

    let mut list = column![container(text(content.summary()).size(12).color(TEXT_MUTED).font(UI)).height(HEAD_H).center_y(HEAD_H).padding([0, 8])];
    if content.is_calm() {
        list = list.push(
            container(text(t("Nichts wartet auf dich", "Nothing is waiting for you")).size(12.5).color(TEXT_MUTED).font(UI))
                .height(CALM_H)
                .center_y(CALM_H)
                .padding([0, 8]),
        );
    }
    if content.needs_total > 0 {
        list = list.push(section(t("Braucht dich", "Needs you")));
        for need in &content.needs {
            let text_part = column![titled(&need.name, &need.project), said(&need.line)];
            if need.permission {
                let buttons = row![
                    answer_button(t("Erlauben", "Allow"), Answer::Yes, Message::PopupAnswer(need.key.clone(), true, need.since_ms)),
                    answer_button(t("Ablehnen", "Deny"), Answer::No, Message::PopupAnswer(need.key.clone(), false, need.since_ms)),
                    answer_button(t("Ansehen", "Show"), Answer::Plain, Message::PopupSee(need.key.clone())),
                ]
                .spacing(6);
                let body = text_part.push(container(buttons).padding(Padding { top: 7.0, ..Padding::ZERO }));
                list = list.push(container(row![dotted(CALLS), body.width(Fill)].spacing(8)).height(ROW_H + BUTTONS_H).padding([6, 8]));
            } else {
                // A question: the whole row opens the session.
                let line = container(row![dotted(CALLS), text_part.width(Fill)].spacing(8)).height(ROW_H).padding([6, 8]);
                list = list.push(row_button(line, Message::PopupSee(need.key.clone())));
            }
        }
        if content.needs_total > content.needs.len() {
            list = list.push(more(content.needs_total - content.needs.len()));
        }
    }
    if content.finished_total > 0 {
        list = list.push(section(t("Fertig, nicht angesehen", "Finished, not looked at")));
        for done in &content.finished {
            let line = container(row![dotted(TURN), column![titled(&done.name, &done.project), said(&done.line)].width(Fill)].spacing(8))
                .height(ROW_H)
                .padding([6, 8]);
            list = list.push(row_button(line, Message::PopupSee(done.key.clone())));
        }
        if content.finished_total > content.finished.len() {
            list = list.push(more(content.finished_total - content.finished.len()));
        }
    }
    if !content.accounts.is_empty() {
        list = list.push(
            container(container(Space::new().width(Fill).height(1)).style(|_| style::fill(style::MENU_LINE))).height(SEPARATOR_H).center_y(SEPARATOR_H).padding([0, 8]),
        );
        list = list.push(section(t("Konten", "Accounts")));
        let mut usage = column![].spacing(ACCOUNT_GAP);
        for account in &content.accounts {
            usage = usage.push(popup_usage_row(account));
        }
        if let Some(advice) = &content.advice {
            usage = usage.push(container(text(clip(advice, 56)).size(11).color(TEXT_FAINT).font(UI).wrapping(text::Wrapping::None)).height(NOTE_H).clip(true));
        }
        list = list.push(container(usage).padding([USAGE_PADDING, 8.0]));
    }
    container(list)
        .width(popup_width())
        .height(content.height())
        .padding(PADDING)
        .clip(true)
        .style(|_| container::Style {
            shadow: iced::Shadow { color: Color::from_rgba8(0, 0, 0, 0.6), offset: iced::Vector::new(0.0, 12.0), blur_radius: 34.0 },
            ..boxed(style::MENU_BG, style::MENU_LINE, 10.0)
        })
        .into()
}

fn popup_width() -> f32 {
    360.0
}

/// The popup over the window: a backdrop that closes it, the card at the top right under the
/// title bar (a click on the card itself stays on it).
fn status_popup_layer(brain: &Brain, now: i64) -> Element<'_, Message> {
    let top = crate::chrome::TITLEBAR_HEIGHT / brain.zoom() + 4.0;
    let card = mouse_area(status_popup(brain, now)).on_press(Message::PopupClicked);
    let backdrop = mouse_area(Space::new().width(Fill).height(Fill)).on_press(Message::ClosePopup).interaction(iced::mouse::Interaction::Idle);
    stack![
        backdrop,
        container(card).width(Fill).height(Fill).align_x(iced::Right).align_y(iced::Top).padding(Padding { top, right: 14.0, ..Padding::ZERO }),
    ]
    .into()
}

/// A row of the popup that opens its session: no background until the pointer is over it.
fn row_button<'a>(row: impl Into<Element<'a, Message>>, message: Message) -> Element<'a, Message> {
    button(row)
        .padding(0)
        .width(Fill)
        .on_press(message)
        .style(|_, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed).then_some(Background::Color(Color::from_rgba8(0xff, 0xff, 0xff, 0.05))),
            border: iced::border::rounded(6),
            ..button::Style::default()
        })
        .into()
}

/// One account in the popup: name, five-hour and weekly limit as bars with their percent. Each
/// column has a fixed width, so the accounts line up.
fn popup_usage_row<'a>(account: &crate::app::status_popup::AccountRow) -> Element<'a, Message> {
    let (_, hue) = style::account_colors(account.index);
    let faint = |label: &str, width: f32| container(text(label.to_string()).size(11.5).color(TEXT_FAINT).font(UI)).width(width);
    let bar = move |used: f64| -> Element<'a, Message> {
        let used = used.clamp(0.0, 100.0) as f32;
        stack![
            container(Space::new().width(46).height(4)).style(|_| boxed(LINE_STRONG, Color::TRANSPARENT, 2.0)),
            container(Space::new().width(46.0 * used / 100.0).height(4)).style(move |_| boxed(hue, Color::TRANSPARENT, 2.0)),
        ]
        .into()
    };
    let num = |used: f64| container(text(format!("{used:.0} %")).size(11.5).color(TEXT).font(UI)).width(36);
    let mut line = row![container(account_badge(&account.account, account.index)).width(54).clip(true)].spacing(6).align_y(iced::Center);
    let usage = &account.usage;
    match usage.five_hour {
        Some(used) => line = line.push(faint("5 h", 24.0)).push(bar(used)).push(num(used)),
        None => line = line.push(Space::new().width(24.0 + 46.0 + 36.0 + 12.0)),
    }
    if let Some(used) = usage.week {
        line = line.push(faint(t("Woche", "Week"), 38.0)).push(bar(used)).push(num(used));
    }
    container(line).height(crate::app::status_popup::ACCOUNT_H).center_y(crate::app::status_popup::ACCOUNT_H).into()
}

// ---- limit warning ----------------------------------------------------------------

/// The height of the warning line at the bottom.
const STATUS_LINE_H: f32 = 26.0;

/// From this percent of a limit on, the line at the bottom appears.
const WARN_PERCENT: f64 = 80.0;

/// The limits of an account that are used up to the warning mark: (is the five-hour limit, percent).
pub fn tight_limits(usage: &UsageNow) -> Vec<(bool, f64)> {
    [(true, usage.five_hour), (false, usage.week)].into_iter().filter_map(|(five, used)| Some((five, used?))).filter(|(_, used)| *used >= WARN_PERCENT).collect()
}

/// A line at the bottom, only while some account has a limit used up to 80 % or more: it shows
/// those accounts, nothing else. Otherwise there is no line.
fn limit_warning_bar<'a>(brain: &'a Brain, now: i64) -> Option<Element<'a, Message>> {
    let segments: Vec<Element<'a, Message>> = brain.model.accounts.iter().filter_map(|account| usage_segment(brain, &account.id, now)).collect();
    if segments.is_empty() {
        return None;
    }
    let mut line = row![].spacing(14).align_y(iced::Center);
    for (i, segment) in segments.into_iter().enumerate() {
        if i > 0 {
            line = line.push(container(Space::new().width(1).height(12)).style(|_| style::fill(LINE_STRONG)));
        }
        line = line.push(segment);
    }
    Some(container(line).height(STATUS_LINE_H).center_y(STATUS_LINE_H).padding([0, 14]).width(Fill).clip(true).style(|_| style::fill(CHROME)).into())
}

/// What an account's limits come to now: the warning line and the status popup both show it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsageNow {
    /// Percent of the five-hour and the weekly limit used.
    pub five_hour: Option<f64>,
    pub week: Option<f64>,
    /// When the first of the limits runs out at the pace so far (epoch seconds).
    pub runs_out: Option<i64>,
    /// Neither window is half used.
    pub has_room: bool,
}

pub fn usage_now(brain: &Brain, account: &str, now: i64) -> Option<UsageNow> {
    let snapshot = brain_core::usage::account_limits(&brain.model.usage, account)?;
    let history = brain.model.usage_history.get(account).map(Vec::as_slice).unwrap_or_default();
    let runs_out = [snapshot.five_hour.and_then(|l| five_hour_runs_out(l, now)), snapshot.seven_day.and_then(|l| seven_day_runs_out(l, history, now))]
        .into_iter()
        .flatten()
        .min();
    Some(UsageNow {
        five_hour: snapshot.five_hour.map(|l| l.used_percentage.clamp(0.0, 100.0)),
        week: snapshot.seven_day.map(|l| l.used_percentage),
        runs_out,
        has_room: snapshot.five_hour.is_none_or(|l| l.used_percentage < 50.0) && snapshot.seven_day.is_none_or(|l| l.used_percentage < 50.0),
    })
}

/// When a limit runs out: the time within a day, otherwise the weekday and time.
pub fn runs_out_label(at: i64, now_ms: i64) -> String {
    let when = chrono::DateTime::from_timestamp(at, 0).map(|t| t.with_timezone(&chrono::Local));
    match when {
        Some(t) if at * 1000 - now_ms < 24 * 3600 * 1000 => t.format("%H:%M").to_string(),
        Some(t) => t.format("%a %H:%M").to_string(),
        None => String::new(),
    }
}

/// An account's tight limits: `main 5 h ▓▓▓ 84 %  week ▓▓▓ 81 %  voll ~Do 14:00`; amber from
/// 80 %, red from 90 %. Nothing for an account below the mark.
fn usage_segment<'a>(brain: &Brain, account: &str, now: i64) -> Option<Element<'a, Message>> {
    let usage = usage_now(brain, account, now)?;
    let tight = tight_limits(&usage);
    if tight.is_empty() {
        return None;
    }
    let faint = |label: String| text(label).size(11.5).color(TEXT_FAINT).font(UI);
    let mut line = row![text(account.to_string()).size(11.5).color(TEXT_MUTED).font(style::medium())].spacing(6).align_y(iced::Center);
    for (five, used) in tight {
        let color = if used >= 90.0 { CALLS } else { TURN };
        let bar = stack![
            container(Space::new().width(40).height(4)).style(|_| boxed(RAISED, Color::TRANSPARENT, 2.0)),
            container(Space::new().width(40.0 * used.clamp(0.0, 100.0) as f32 / 100.0).height(4)).style(move |_| boxed(color, Color::TRANSPARENT, 2.0)),
        ];
        let label = if five { "5 h" } else { t("Woche", "week") };
        line = line.push(faint(label.into())).push(bar).push(text(format!("{used:.0} %")).size(11.5).color(color).font(UI));
    }
    if let Some(at) = usage.runs_out {
        let when = runs_out_label(at, now);
        let color = if usage.five_hour.is_some_and(|used| used >= 90.0) { CALLS } else { TURN };
        line = line.push(text(tr!("voll ~{when}", "full ~{when}")).size(11.5).color(color).font(style::semibold()));
    }
    Some(line.into())
}

/// ⌘?: every key, by what it acts on.
fn keys_sheet(_brain: &Brain) -> Element<'_, Message> {
    let groups: [(&str, Vec<(&str, &str)>); 3] = [
        (
            t("Liste und Baum", "List and tree"),
            vec![
                ("↑↓", t("wählen", "select")),
                ("⏎", t("öffnen (beendete: fortsetzen)", "open (ended: resume)")),
                ("⌥⏎", t("in iTerm öffnen", "open in iTerm")),
                ("⌃⇥", t("nächste Session (⇧: vorherige)", "next session (⇧: previous)")),
                ("⇧⌘U", t("nächste Session, die dich braucht", "next session that needs you")),
                ("⌘1…9", t("die Sessions, die dich brauchen", "the sessions that need you")),
                ("⇥", t("Konto-Filter", "account filter")),
                ("⌥⌘F", t("Sessions suchen", "search sessions")),
                ("⌘O", t("Session fortsetzen (suchen, auch beendete)", "resume a session (search, ended ones too)")),
                ("⌘N", t("neue Session", "new session")),
                ("⌥⌘N", t("neue Session im Projekt", "new session in the project")),
                ("⇧⌘N", t("neues Projekt", "new project")),
                ("⌘L", t("für später: Aufgaben aus /later", "for later: tasks from /later")),
                ("⇧⌘L", t("Später-Notizen des Projekts", "the project's notes for later")),
            ],
        ),
        (
            t("Session", "Session"),
            vec![
                ("⌘Y  ⇧⌘Y", t("Freigabe erlauben · ablehnen", "allow · deny a permission")),
                ("⌘R", t("umbenennen", "rename")),
                ("⌥⌘P", t("anheften", "pin")),
                ("⌥⌘M", t("stummschalten", "mute")),
                ("⌫ ⌫", t("ausblenden, bis sich etwas tut", "hide until something happens")),
                ("⌥⌘O", t("aus iTerm in Brain holen", "move from iTerm into Brain")),
                ("⌥⌘I", t("in iTerm öffnen", "open in iTerm")),
                ("⌥⌘X ⌥⌘X", t("beenden", "end")),
                ("⌥⌘⌫ ⌥⌘⌫", t("in den Papierkorb", "move to the Trash")),
                ("⌘E", t("längerer Prompt (Composer)", "longer prompt (composer)")),
                ("⌘F", t("im ganzen Gespräch suchen", "find in the whole conversation")),
                ("⌘⇧C", t("letzte Antwort kopieren", "copy the last answer")),
            ],
        ),
        (
            t("Ansicht und Terminal", "View and terminal"),
            vec![
                ("⌘J", t("Tastatur ins Terminal", "keyboard into the terminal")),
                ("⌘[", t("Tastatur zurück in den Baum", "keyboard back to the tree")),
                ("⌘T", t("Shell im Projektordner", "shell in the project folder")),
                ("⌘D", t("Terminal teilen", "split the terminal")),
                ("⌘⇧A", t("alle Terminals im Überblick", "all terminals at a glance")),
                ("⌃⌘S", t("Seitenleiste ein-/ausblenden", "show / hide the sidebar")),
                ("⌘+  ⌘−  ⌘0", t("größer · kleiner · Originalgröße", "larger · smaller · actual size")),
                ("⌥⌘0", t("Änderungen unter der Session", "changes under the session")),
                ("⌘-Klick", t("Pfad oder Link öffnen", "open a path or link")),
                ("⌘C  ⌘V", t("kopieren · einsetzen im Terminal", "copy · paste in the terminal")),
                ("⌃⌘V", t("Screenshot ins Terminal", "paste a screenshot")),
                ("⌘K", t("Befehle", "commands")),
                ("⌘,", t("Einstellungen (config.json)", "settings (config.json)")),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(five_hour: Option<f64>, week: Option<f64>) -> UsageNow {
        UsageNow { five_hour, week, runs_out: None, has_room: false }
    }

    #[test]
    fn the_warning_line_needs_a_limit_at_eighty_percent() {
        assert!(tight_limits(&usage(Some(79.9), Some(46.0))).is_empty(), "below 80 % there is no line");
        assert_eq!(tight_limits(&usage(Some(80.0), Some(21.0))), vec![(true, 80.0)]);
        assert_eq!(tight_limits(&usage(Some(12.0), Some(93.0))), vec![(false, 93.0)], "only the limit that is tight");
        assert_eq!(tight_limits(&usage(Some(84.0), Some(85.0))).len(), 2);
        assert!(tight_limits(&usage(None, None)).is_empty());
    }
}
