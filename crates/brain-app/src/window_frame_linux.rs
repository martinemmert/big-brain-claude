//! The window frame where the desktop leaves it to the app (GNOME): resize edges and window
//! buttons. GPUI tells at runtime when that applies.

use gpui::{
    canvas, div, point, prelude::*, px, Bounds, ClickEvent, CursorStyle, HitboxBehavior, MouseButton, Pixels, Point,
    ResizeEdge, Size, Window, WindowControls,
};

use crate::theme;

/// How far into the window the edges resize it.
const RESIZE_BORDER: f32 = 5.;

/// The edge or corner at `pos`, if any.
pub fn resize_edge(pos: Point<Pixels>, size: Size<Pixels>) -> Option<ResizeEdge> {
    let border = px(RESIZE_BORDER);
    let corner = px(RESIZE_BORDER * 3.);
    let top = pos.y < border;
    let bottom = pos.y > size.height - border;
    let left = pos.x < border;
    let right = pos.x > size.width - border;
    let near_top = pos.y < corner;
    let near_bottom = pos.y > size.height - corner;
    let near_left = pos.x < corner;
    let near_right = pos.x > size.width - corner;
    let edge = if (top && near_left) || (left && near_top) {
        ResizeEdge::TopLeft
    } else if (top && near_right) || (right && near_top) {
        ResizeEdge::TopRight
    } else if (bottom && near_left) || (left && near_bottom) {
        ResizeEdge::BottomLeft
    } else if (bottom && near_right) || (right && near_bottom) {
        ResizeEdge::BottomRight
    } else if top {
        ResizeEdge::Top
    } else if bottom {
        ResizeEdge::Bottom
    } else if left {
        ResizeEdge::Left
    } else if right {
        ResizeEdge::Right
    } else {
        return None;
    };
    Some(edge)
}

/// Starts resizing when a press lands on an edge; returns whether it did.
pub fn start_resize(position: Point<Pixels>, window: &mut Window) -> bool {
    let size = window.window_bounds().get_bounds().size;
    match resize_edge(position, size) {
        Some(edge) => {
            window.start_window_resize(edge);
            true
        }
        None => false,
    }
}

/// An invisible layer over the window that shows the resize cursor at the edges.
pub fn resize_cursors() -> impl IntoElement {
    canvas(
        |_, window, _| {
            let size = window.window_bounds().get_bounds().size;
            window.insert_hitbox(Bounds::new(point(px(0.), px(0.)), size), HitboxBehavior::Normal)
        },
        |_, hitbox, window, _| {
            let size = window.window_bounds().get_bounds().size;
            let Some(edge) = resize_edge(window.mouse_position(), size) else { return };
            let cursor = match edge {
                ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
                ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
                ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
                ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
            };
            window.set_cursor_style(cursor, &hitbox);
        },
    )
    .absolute()
    .size_full()
}

/// Minimize, maximize and close, as far as the desktop supports them.
pub fn window_buttons(controls: WindowControls) -> impl IntoElement {
    let button = |id: &'static str, glyph: &'static str, close: bool| {
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded_full()
            .text_size(px(13.))
            .text_color(theme::text_muted())
            .cursor_pointer()
            .hover(move |d| {
                if close {
                    d.bg(theme::calls()).text_color(theme::text_strong())
                } else {
                    d.bg(theme::hover()).text_color(theme::text_strong())
                }
            })
            // Keeps the press from starting a window move in the title bar.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(glyph)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .ml(px(4.))
        .when(controls.minimize, |d| {
            d.child(button("window-minimize", "–", false).on_click(|_: &ClickEvent, window, _| window.minimize_window()))
        })
        .when(controls.maximize, |d| {
            d.child(button("window-maximize", "□", false).on_click(|_: &ClickEvent, window, _| window.zoom_window()))
        })
        .child(
            button("window-close", "×", true)
                .on_click(|_: &ClickEvent, window, _| window.remove_window()),
        )
}
