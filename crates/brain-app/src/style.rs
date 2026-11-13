//! Brain's look: ink-blue surfaces, four signal colours that mean exactly one thing each (calls
//! you, your turn, working, done), per-account hues, and the small pieces every view uses.

use brain_core::state::Phase;
use iced::widget::{button, container, row, text, Space};
use iced::{border, color, font, Background, Border, Color, Element, Font, Shadow};

// Surfaces, darkest to lightest.
pub const INK: Color = color!(0x0f1322);
pub const CHROME: Color = color!(0x121728);
pub const SURFACE: Color = color!(0x161b2c);
pub const RAISED: Color = color!(0x1d2338);
pub const HOVER: Color = color!(0x1a2033);
/// A project's island on the sidebar: only a shade lighter than the sidebar, so the islands part
/// the projects without standing out.
pub const ISLAND: Color = color!(0x151b2e);
/// The selected session (or file): the accent blue at about 25 % on the island, like a Mac
/// sidebar's selection: found at a glance without shouting.
pub const ISLAND_SELECTED: Color = color!(0x1e3058);
/// The pointer over a session.
pub const ISLAND_HOVER: Color = Color::from_rgba8(0xff, 0xff, 0xff, 0.05);
pub const LINE: Color = color!(0x232a40);
pub const LINE_STRONG: Color = color!(0x2e3654);
/// The button that says yes (Allow), in the sidebar and in the popup alike.
pub const PRIMARY: Color = color!(0x3a6fd8);

// Text.
pub const TEXT: Color = color!(0xd5d9e6);
pub const TEXT_STRONG: Color = color!(0xf3f4f9);
pub const TEXT_MUTED: Color = color!(0x8c93aa);
pub const TEXT_FAINT: Color = color!(0x5f6782);

// The popup under the status item: a lighter, nearly opaque card with a fine light edge.
pub const MENU_BG: Color = Color::from_rgba8(0x27, 0x2c, 0x40, 0.96);
pub const MENU_LINE: Color = Color::from_rgba8(0xff, 0xff, 0xff, 0.10);

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
        // Quiet like an ended one, but it lives on.
        Phase::Asleep => BACKGROUND,
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


/// The embedded terminal's colours: iTerm2's default profile (foreground and the 16 ANSI
/// colours) on Brain's ink background; Brain's own tones where iTerm has none.
pub fn terminal_palette() -> iced_term::ColorPalette {
    let hex = |c: Color| format!("#{:02x}{:02x}{:02x}", (c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8);
    let iterm = |key: &str, fallback: Color| {
        crate::iterm::color(key).map(|(r, g, b)| hex(Color::from_rgb(r, g, b))).unwrap_or_else(|| hex(fallback))
    };
    let ansi = |n: u8, fallback: Color| iterm(&format!("Ansi {n} Color"), fallback);
    iced_term::ColorPalette {
        foreground: iterm("Foreground Color", TEXT),
        background: hex(INK),
        black: ansi(0, INK),
        red: ansi(1, CALLS),
        green: ansi(2, DONE),
        yellow: ansi(3, TURN),
        blue: ansi(4, WORKING),
        magenta: ansi(5, BACKGROUND),
        cyan: ansi(6, color!(0x5fd7d7)),
        white: ansi(7, TEXT),
        bright_black: ansi(8, TEXT_FAINT),
        bright_red: ansi(9, CALLS_SOFT),
        bright_green: ansi(10, DONE),
        bright_yellow: ansi(11, TURN),
        bright_blue: ansi(12, WORKING),
        bright_magenta: ansi(13, BACKGROUND),
        bright_cyan: ansi(14, color!(0x87ffff)),
        bright_white: ansi(15, TEXT_STRONG),
        bright_foreground: None,
        dim_foreground: hex(TEXT_MUTED),
        ..iced_term::ColorPalette::default()
    }
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

/// A button's label, centred in the button's height. Iced puts a button's content at its top edge,
/// so a button with a fixed height needs this around its text; give such a button only horizontal
/// padding. (Not for buttons that size themselves: the label would claim all the room there is.)
pub fn centered<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).height(iced::Fill).align_y(iced::Center).into()
}

/// A labelled button with its key; `strong` fills it with the alert colour.
pub fn action<'a, M: Clone + 'a>(label: impl Into<String>, key: impl Into<String>, strong: bool, on_press: Option<M>) -> Element<'a, M> {
    let enabled = on_press.is_some();
    let key = key.into();
    // Text on the alert colour is ink: white on red doesn't read.
    let on_alert = if strong { INK } else { TEXT_STRONG };
    let mut content = row![text(label.into()).size(12.5).color(on_alert).font(semibold())].spacing(8).align_y(iced::Center);
    // A button without a shortcut has no key cap.
    if !key.is_empty() {
        content = content.push(if strong {
            container(text(key).size(10.5).color(INK).font(UI))
                .padding([1, 5])
                .height(18)
                .center_y(18)
                .style(|_| boxed(alpha(INK, 0x2e), alpha(INK, 0x40), 4.0))
                .into()
        } else {
            kbd(key)
        });
    }
    button(centered(content))
        .padding([0, 12])
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
                text_color: faded(on_alert),
                border: Border { color: faded(edge), width: if edge.a > 0.0 { 1.0 } else { 0.0 }, radius: 8.0.into() },
                ..button::Style::default()
            }
        })
        .into()
}

/// The button for what the user most likely wants here: filled with the primary colour.
pub fn primary_action<'a, M: Clone + 'a>(label: impl Into<String>, key: impl Into<String>, on_press: Option<M>) -> Element<'a, M> {
    let enabled = on_press.is_some();
    let key = key.into();
    let mut content = row![text(label.into()).size(12.5).color(TEXT_STRONG).font(semibold())].spacing(8).align_y(iced::Center);
    if !key.is_empty() {
        content = content.push(
            container(text(key).size(10.5).color(TEXT_STRONG).font(UI))
                .padding([1, 5])
                .height(18)
                .center_y(18)
                .style(|_| boxed(alpha(TEXT_STRONG, 0x2e), alpha(TEXT_STRONG, 0x40), 4.0)),
        );
    }
    button(centered(content))
        .padding([0, 12])
        .height(30)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let faded = |c: Color| if enabled { c } else { Color { a: c.a * 0.4, ..c } };
            button::Style {
                background: Some(Background::Color(faded(if hovered { alpha(PRIMARY, 0xdd) } else { PRIMARY }))),
                text_color: faded(TEXT_STRONG),
                border: border::rounded(8),
                ..button::Style::default()
            }
        })
        .into()
}

/// A segmented control; picking an option produces its message.
pub fn segmented<'a, M: Clone + 'a>(options: Vec<(String, bool, M)>) -> Element<'a, M> {
    segmented_in(options, RAISED, Color::TRANSPARENT)
}

/// A segmented control whose active option stands out more: `active` fills it, `rim` is a fine
/// inner edge (transparent for none).
pub fn segmented_in<'a, M: Clone + 'a>(options: Vec<(String, bool, M)>, active_background: Color, rim: Color) -> Element<'a, M> {
    let buttons = options.into_iter().map(|(label, active, message)| {
        button(text(label).size(12).font(if active { medium() } else { UI }))
            .padding([3, 11])
            .on_press(message)
            .style(move |_, status| button::Style {
                background: active.then_some(Background::Color(active_background)),
                text_color: match (active, status) {
                    (true, _) => TEXT_STRONG,
                    (false, button::Status::Hovered) => TEXT,
                    _ => TEXT_MUTED,
                },
                border: Border { color: if active { rim } else { Color::TRANSPARENT }, width: if active && rim.a > 0.0 { 1.0 } else { 0.0 }, radius: 5.0.into() },
                ..button::Style::default()
            })
            .into()
    });
    container(row(buttons).spacing(2)).padding(3).style(|_| boxed(INK, LINE, 8.0)).into()
}

/// A quiet chevron drawn as a stroke (the system font's "▾" renders tiny): pointing down when
/// open, right when `folded`. `size` is its square in points.
pub fn chevron<'a, M: 'a>(folded: bool, size: f32, color: Color) -> Element<'a, M> {
    iced::widget::canvas(Chevron { folded, color }).width(size).height(size).into()
}

struct Chevron {
    folded: bool,
    color: Color,
}

impl<M> iced::widget::canvas::Program<M> for Chevron {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, LineCap, LineJoin, Path, Stroke};
        let mut frame = Frame::new(renderer, bounds.size());
        let k = bounds.width / 12.0;
        // The artifact's 12 × 12 box: a down chevron, or the same turned a quarter to the left.
        let points = if self.folded { [(4.5, 3.0), (7.5, 6.0), (4.5, 9.0)] } else { [(3.0, 4.5), (6.0, 7.5), (9.0, 4.5)] };
        let path = Path::new(|p| {
            p.move_to(iced::Point::new(points[0].0 * k, points[0].1 * k));
            p.line_to(iced::Point::new(points[1].0 * k, points[1].1 * k));
            p.line_to(iced::Point::new(points[2].0 * k, points[2].1 * k));
        });
        frame.stroke(&path, Stroke::default().with_color(self.color).with_width(1.5 * k).with_line_cap(LineCap::Round).with_line_join(LineJoin::Round));
        vec![frame.into_geometry()]
    }
}

/// A plus drawn as two strokes, exactly in the middle of its square: the "+" glyph of SF sits a
/// little high. `size` is the square in points (the arms are 60 % of it).
pub fn plus<'a, M: 'a>(size: f32, color: Color) -> Element<'a, M> {
    iced::widget::canvas(Plus { color }).width(size).height(size).into()
}

struct Plus {
    color: Color,
}

impl<M> iced::widget::canvas::Program<M> for Plus {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, LineCap, Path, Stroke};
        let mut frame = Frame::new(renderer, bounds.size());
        let k = bounds.width / 10.0;
        // A 10 × 10 box: the arms run from 2 to 8 around the middle at 5.
        let horizontal = Path::line(iced::Point::new(2.0 * k, 5.0 * k), iced::Point::new(8.0 * k, 5.0 * k));
        let vertical = Path::line(iced::Point::new(5.0 * k, 2.0 * k), iced::Point::new(5.0 * k, 8.0 * k));
        let stroke = Stroke::default().with_color(self.color).with_width(1.5 * k).with_line_cap(LineCap::Round);
        frame.stroke(&horizontal, stroke);
        frame.stroke(&vertical, stroke);
        vec![frame.into_geometry()]
    }
}

/// The sidebar symbol of a Mac title bar: a rounded window with its left part set off.
pub fn sidebar_icon<'a, M: 'a>(size: f32, color: Color) -> Element<'a, M> {
    iced::widget::canvas(SidebarIcon { color }).width(size).height(size).into()
}

struct SidebarIcon {
    color: Color,
}

impl<M> iced::widget::canvas::Program<M> for SidebarIcon {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Stroke};
        let mut frame = Frame::new(renderer, bounds.size());
        let k = bounds.width / 16.0;
        // A 16 × 16 box: the window 14 × 11, the sidebar's divider at a third.
        let window = Path::rounded_rectangle(iced::Point::new(1.0 * k, 2.5 * k), iced::Size::new(14.0 * k, 11.0 * k), (2.5 * k).into());
        let divider = Path::line(iced::Point::new(6.0 * k, 2.5 * k), iced::Point::new(6.0 * k, 13.5 * k));
        let stroke = Stroke::default().with_color(self.color).with_width(1.3 * k);
        frame.stroke(&window, stroke);
        frame.stroke(&divider, stroke);
        vec![frame.into_geometry()]
    }
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
