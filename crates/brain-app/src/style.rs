//! Brain's look: ink-blue surfaces, four signal colours that mean exactly one thing each (calls
//! you, your turn, working, done), per-account hues, and the small pieces every view uses.

use brain_core::state::Phase;
use iced::widget::{button, container, row, text, Space};
use iced::{border, color, font, Background, Border, Color, Element, Font, Length, Padding, Shadow, Vector};

// Surfaces, darkest to lightest.
pub const INK: Color = color!(0x0f1322);
pub const CHROME: Color = color!(0x121728);
pub const SURFACE: Color = color!(0x161b2c);
pub const RAISED: Color = color!(0x1d2338);
pub const HOVER: Color = color!(0x1a2033);
pub const LINE: Color = color!(0x232a40);
pub const LINE_STRONG: Color = color!(0x2e3654);

// Text.
pub const TEXT: Color = color!(0xd5d9e6);
pub const TEXT_STRONG: Color = color!(0xf3f4f9);
pub const TEXT_MUTED: Color = color!(0x8c93aa);
pub const TEXT_FAINT: Color = color!(0x5f6782);

// Signals.
pub const CALLS: Color = color!(0xff5c6c);
pub const CALLS_SOFT: Color = color!(0xff8f9a);
pub const TURN: Color = color!(0xf2b84b);
pub const WORKING: Color = color!(0x5aa9ff);
/// Working, but only in the background (subagents, shells): a quieter blue.
pub const BACKGROUND: Color = color!(0x8f9cf5);
pub const DONE: Color = color!(0x45d19a);
pub const ENDED: Color = color!(0x4a5272);
pub const CODE: Color = color!(0xc8d4ff);

/// macOS's system font (San Francisco) and Menlo for paths and code.
pub const UI: Font = Font::with_name(".SF NS");
pub const MONO: Font = Font::with_name("Menlo");

pub fn weight(weight: font::Weight) -> Font {
    Font { weight, ..UI }
}

pub fn medium() -> Font {
    weight(font::Weight::Medium)
}

pub fn semibold() -> Font {
    weight(font::Weight::Semibold)
}

/// The system font's bold cut doesn't resolve from its variable font file; semibold does and
/// reads as bold at title sizes.
pub fn bold() -> Font {
    weight(font::Weight::Semibold)
}

/// `color` at the given alpha (0–255).
pub fn alpha(color: Color, a: u8) -> Color {
    Color { a: a as f32 / 255.0, ..color }
}

pub fn theme() -> iced::Theme {
    iced::Theme::custom(
        "Brain",
        iced::theme::Palette { background: INK, text: TEXT, primary: WORKING, success: DONE, warning: TURN, danger: CALLS },
    )
}

pub fn phase_color(phase: Phase) -> Color {
    match phase {
        Phase::NeedsYou => CALLS,
        Phase::YourTurn => TURN,
        Phase::Working => WORKING,
        Phase::Background => BACKGROUND,
        Phase::Ended => ENDED,
    }
}

/// Background and text colour of an account badge: main is blue, then violet, teal, rose.
pub fn account_colors(index: usize) -> (Color, Color) {
    const PALETTE: [(Color, Color); 4] = [
        (Color::from_rgba8(0x5a, 0xa9, 0xff, 0.14), color!(0x9ccaff)),
        (Color::from_rgba8(0xa7, 0x8b, 0xfa, 0.15), color!(0xc9b8ff)),
        (Color::from_rgba8(0x2d, 0xd4, 0xbf, 0.14), color!(0x86eadb)),
        (Color::from_rgba8(0xfb, 0x71, 0x85, 0.14), color!(0xffa8b8)),
    ];
    PALETTE[index % PALETTE.len()]
}

/// A filled, rounded box with an optional border.
pub fn boxed(background: Color, border_color: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(background)),
        border: Border { color: border_color, width: if border_color.a > 0.0 { 1.0 } else { 0.0 }, radius: radius.into() },
        ..container::Style::default()
    }
}

pub fn fill(background: Color) -> container::Style {
    container::Style { background: Some(Background::Color(background)), ..container::Style::default() }
}

pub fn glow(color: Color) -> Shadow {
    Shadow { color: alpha(color, 0x30), offset: Vector::new(0.0, 0.0), blur_radius: 18.0 }
}

/// A key cap like `⏎` or `/`.
pub fn kbd<'a, M: 'a>(label: impl Into<String>) -> Element<'a, M> {
    container(text(label.into()).size(10.5).color(TEXT_MUTED).font(UI))
        .padding([1, 5])
        .height(18)
        .center_y(18)
        .style(|_| boxed(RAISED, LINE_STRONG, 4.0))
        .into()
}

/// The account name as a tinted badge.
pub fn account_badge<'a, M: 'a>(account: &str, index: usize) -> Element<'a, M> {
    let (bg, fg) = account_colors(index);
    container(text(account.to_string()).size(10.5).color(fg).font(medium()))
        .padding([1, 6])
        .style(move |_| boxed(bg, Color::TRANSPARENT, 5.0))
        .into()
}

/// A small rounded chip for metadata (path, pid, start time).
pub fn chip<'a, M: 'a>(content: impl Into<String>, mono: bool) -> Element<'a, M> {
    let label = text(content.into()).color(TEXT_MUTED);
    let label = if mono { label.font(MONO).size(11) } else { label.font(UI).size(11.5) };
    container(label).padding([2, 7]).style(|_| boxed(SURFACE, LINE, 5.0)).into()
}

/// Small coloured text after a session's name: worktree, PR, pinned, muted …
pub fn marker<'a, M: 'a>(label: impl Into<String>, color: Color) -> Element<'a, M> {
    text(label.into()).size(11).color(color).font(UI).into()
}

/// A round status dot.
pub fn dot<'a, M: 'a>(color: Color, size: f32) -> Element<'a, M> {
    container(Space::new().width(size).height(size)).style(move |_| boxed(color, Color::TRANSPARENT, size / 2.0)).into()
}

/// A muted line of guidance in a list.
pub fn hint<'a, M: 'a>(label: impl Into<String>) -> Element<'a, M> {
    container(text(label.into()).size(12.5).color(TEXT_MUTED).font(UI)).padding([10, 6]).into()
}

/// Section title with its count, e.g. "Braucht dich 3". `alert` turns the count red.
pub fn section_title<'a, M: 'a>(title: impl Into<String>, count: usize, alert: bool) -> Element<'a, M> {
    let count: Element<'a, M> = if alert && count > 0 {
        container(text(count.to_string()).size(11).color(TEXT_STRONG).font(bold()))
            .padding([0, 6])
            .style(|_| boxed(CALLS, Color::TRANSPARENT, 8.0))
            .into()
    } else {
        text(count.to_string()).size(12).color(TEXT_FAINT).font(medium()).into()
    };
    container(row![text(title.into()).size(12).color(TEXT_MUTED).font(semibold()), count].spacing(7).align_y(iced::Center))
        .padding(Padding { top: 14.0, right: 4.0, bottom: 4.0, left: 4.0 })
        .into()
}

/// A labelled button with its key; `strong` fills it with the alert colour.
pub fn action<'a, M: Clone + 'a>(label: impl Into<String>, key: impl Into<String>, strong: bool, on_press: Option<M>) -> Element<'a, M> {
    let enabled = on_press.is_some();
    let content = row![text(label.into()).size(12.5).color(TEXT_STRONG).font(semibold()), kbd(key)].spacing(8).align_y(iced::Center);
    button(content)
        .padding([6, 12])
        .height(30)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let (bg, edge) = match (strong, hovered) {
                (true, false) => (CALLS, Color::TRANSPARENT),
                (true, true) => (CALLS_SOFT, Color::TRANSPARENT),
                (false, false) => (RAISED, LINE_STRONG),
                (false, true) => (LINE_STRONG, LINE_STRONG),
            };
            let faded = |c: Color| if enabled { c } else { Color { a: c.a * 0.4, ..c } };
            button::Style {
                background: Some(Background::Color(faded(bg))),
                text_color: faded(TEXT_STRONG),
                border: Border { color: faded(edge), width: if edge.a > 0.0 { 1.0 } else { 0.0 }, radius: 8.0.into() },
                ..button::Style::default()
            }
        })
        .into()
}

/// A segmented control; picking an option produces its message.
pub fn segmented<'a, M: Clone + 'a>(options: Vec<(String, bool, M)>) -> Element<'a, M> {
    let buttons = options.into_iter().map(|(label, active, message)| {
        button(text(label).size(12).font(if active { medium() } else { UI }))
            .padding([3, 11])
            .on_press(message)
            .style(move |_, status| button::Style {
                background: active.then_some(Background::Color(RAISED)),
                text_color: match (active, status) {
                    (true, _) => TEXT_STRONG,
                    (false, button::Status::Hovered) => TEXT,
                    _ => TEXT_MUTED,
                },
                border: border::rounded(5),
                ..button::Style::default()
            })
            .into()
    });
    container(row(buttons).spacing(2)).padding(3).style(|_| boxed(INK, LINE, 8.0)).into()
}

/// A clickable text that only looks like a link-ish label (e.g. the background filter).
pub fn text_button<'a, M: Clone + 'a>(label: impl Into<String>, on: bool, message: M) -> Element<'a, M> {
    button(text(label.into()).size(12).font(UI))
        .padding([3, 9])
        .on_press(message)
        .style(move |_, status| button::Style {
            background: match (on, status) {
                (true, _) => Some(Background::Color(RAISED)),
                (false, button::Status::Hovered) => Some(Background::Color(HOVER)),
                _ => None,
            },
            text_color: if on { TEXT_STRONG } else { TEXT_FAINT },
            border: border::rounded(6),
            ..button::Style::default()
        })
        .into()
}

/// A full-width transparent button around a list entry: hover and selection backgrounds.
pub fn entry_style(selected: bool, base: Color, selected_bg: Color, edge: Color, radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match (selected, status) {
            (true, _) => selected_bg,
            (false, button::Status::Hovered | button::Status::Pressed) => HOVER,
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: TEXT,
            border: Border { color: edge, width: if edge.a > 0.0 { 1.0 } else { 0.0 }, radius: radius.into() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// The phase colour as a bar along the left edge of a rounded entry.
pub fn rail<'a, M: 'a>(color: Color, radius: f32) -> Element<'a, M> {
    container(Space::new().width(4).height(Length::Fill))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border { radius: border::Radius { top_left: radius, bottom_left: radius, top_right: 0.0, bottom_right: 0.0 }, ..Border::default() },
            ..container::Style::default()
        })
        .into()
}
