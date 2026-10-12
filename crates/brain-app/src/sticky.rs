//! A header that stays at the top of the visible area while its block scrolls past.

use iced::advanced::widget::{self, Tree};
use iced::advanced::{layout, mouse, renderer, Clipboard, Layout, Shell, Widget};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

pub struct Sticky<'a, M, T, R> {
    header: Element<'a, M, T, R>,
    body: Element<'a, M, T, R>,
}

pub fn sticky<'a, M, T, R>(header: impl Into<Element<'a, M, T, R>>, body: impl Into<Element<'a, M, T, R>>) -> Sticky<'a, M, T, R> {
    Sticky { header: header.into(), body: body.into() }
}

/// How far the header moves down so it stays visible.
fn offset(layout: Layout<'_>, viewport: &Rectangle) -> Vector {
    let bounds = layout.bounds();
    let header = layout.children().next().map_or(0.0, |h| h.bounds().height);
    Vector::new(0.0, (viewport.y - bounds.y).clamp(0.0, (bounds.height - header).max(0.0)))
}

impl<M, T, R: renderer::Renderer> Widget<M, T, R> for Sticky<'_, M, T, R> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.header), Tree::new(&self.body)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.header, &self.body]);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) -> layout::Node {
        let width = limits.width(Length::Fill).max().width;
        let child = layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY));
        let header = self.header.as_widget_mut().layout(&mut tree.children[0], renderer, &child);
        let top = header.size().height;
        let body = self.body.as_widget_mut().layout(&mut tree.children[1], renderer, &child).move_to((0.0, top));
        let height = top + body.size().height;
        layout::Node::with_children(Size::new(width, height), vec![header, body])
    }

    fn draw(&self, tree: &Tree, renderer: &mut R, theme: &T, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        let mut parts = layout.children();
        let (header, body) = (parts.next().unwrap(), parts.next().unwrap());
        self.body.as_widget().draw(&tree.children[1], renderer, theme, style, body, cursor, viewport);
        let shift = offset(layout, viewport);
        // Own layer, so the header's background covers the code it floats over.
        renderer.with_layer(header.bounds() + shift, |renderer| {
            renderer.with_translation(shift, |renderer| {
                self.header.as_widget().draw(&tree.children[0], renderer, theme, style, header, cursor - shift, &(*viewport - shift));
            });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &R,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let shift = offset(layout, viewport);
        let mut parts = layout.children();
        let (header, body) = (parts.next().unwrap(), parts.next().unwrap());
        self.header.as_widget_mut().update(&mut tree.children[0], event, header, cursor - shift, renderer, clipboard, shell, &(*viewport - shift));
        if !shell.is_event_captured() {
            self.body.as_widget_mut().update(&mut tree.children[1], event, body, cursor, renderer, clipboard, shell, viewport);
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &R) -> mouse::Interaction {
        let shift = offset(layout, viewport);
        let mut parts = layout.children();
        let (header, body) = (parts.next().unwrap(), parts.next().unwrap());
        let over_header = self.header.as_widget().mouse_interaction(&tree.children[0], header, cursor - shift, &(*viewport - shift), renderer);
        if over_header != mouse::Interaction::None {
            return over_header;
        }
        self.body.as_widget().mouse_interaction(&tree.children[1], body, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &R, operation: &mut dyn widget::Operation) {
        let mut parts = layout.children();
        let (header, body) = (parts.next().unwrap(), parts.next().unwrap());
        self.header.as_widget_mut().operate(&mut tree.children[0], header, renderer, operation);
        self.body.as_widget_mut().operate(&mut tree.children[1], body, renderer, operation);
    }
}

impl<'a, M: 'a, T: 'a, R: renderer::Renderer + 'a> From<Sticky<'a, M, T, R>> for Element<'a, M, T, R> {
    fn from(sticky: Sticky<'a, M, T, R>) -> Self {
        Element::new(sticky)
    }
}
