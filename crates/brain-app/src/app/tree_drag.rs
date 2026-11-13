//! Dragging in the tree: a session onto another project's island puts it there, a project's head
//! dragged between the islands sorts the list. A press only holds; from `START_DRAG` pt on it is
//! a drag, and nothing else in the window gets the pointer or the keys until it ends.

use brain_core::state::SessionKey;
use iced::widget::{operation, scrollable};
use iced::{Point, Task};

use super::{unfocus, Brain, Groups, Message, LIST};
use crate::format::now_ms;
use crate::i18n::t;
use crate::tree::{self, HeadKey};

/// How far the pointer goes before a press becomes a drag; less is a click.
const START_DRAG: f32 = 4.0;
/// How close to the list's edge the pointer scrolls it, and the most it scrolls per step.
const EDGE: f32 = 28.0;
const SCROLL_STEP: f32 = 14.0;

/// What a press in the tree holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Grab {
    Session(SessionKey),
    Project(String),
}

/// A press in the tree and where it has got to. Window coordinates; `list_top` is where the
/// list's content begins in them.
pub struct TreeDrag {
    pub grab: Grab,
    pub start: Point,
    pub at: Point,
    pub list_top: f32,
    pub active: bool,
}

/// What a drop would do now; the y values are window coordinates, already scrolled.
#[derive(Debug, Clone, PartialEq)]
pub enum DragTarget {
    /// The island of this head, from y to y: the session goes into it.
    Island(HeadKey, f32, f32),
    /// The line between two islands at y: the project goes before the named one (none: last).
    Gap(Option<String>, f32),
}

impl Brain {
    /// A press on a session's row or a project's head. A project's sorting needs the whole list
    /// in view, so a search leaves it a plain click.
    pub(super) fn grab(&mut self, grab: Grab) -> Task<Message> {
        let select = match &grab {
            Grab::Session(key) => self.update(Message::Select(key.clone())),
            Grab::Project(name) if self.searching() => return self.update(Message::ToggleProjectFolded(name.clone())),
            Grab::Project(_) => Task::none(),
        };
        if let Some(inside) = self.tree_pointer {
            self.tree_drag = Some(TreeDrag { grab, start: self.pointer, at: self.pointer, list_top: self.pointer.y - inside.y, active: false });
        }
        select
    }

    pub(super) fn drag_to(&mut self, at: Point) -> Task<Message> {
        let Some(drag) = self.tree_drag.as_mut() else { return Task::none() };
        drag.at = at;
        if drag.active || at.distance(drag.start) < START_DRAG {
            return Task::none();
        }
        drag.active = true;
        // The terminal doesn't get the keys while the drag lasts.
        self.focused_terminal = None;
        self.context_menu = None;
        self.project_menu = None;
        unfocus()
    }

    /// The button went up: a click on a head folds it, a drag drops.
    pub(super) fn drag_done(&mut self) -> Task<Message> {
        let groups = self.groups(now_ms());
        let target = self.drag_target(&groups);
        let Some(drag) = self.tree_drag.take() else { return Task::none() };
        if !drag.active {
            return match drag.grab {
                Grab::Project(name) => self.update(Message::ToggleProjectFolded(name)),
                Grab::Session(_) => Task::none(),
            };
        }
        match (drag.grab, target) {
            (Grab::Session(key), Some(DragTarget::Island(HeadKey::Project(name), ..))) => self.put_in_project(&key, Some(name)),
            (Grab::Session(key), Some(DragTarget::Island(HeadKey::Others, ..))) => self.put_in_project(&key, None),
            (Grab::Project(name), Some(DragTarget::Gap(before, _))) => {
                if self.prefs.move_project(&name, before.as_deref()) {
                    self.prefs.save();
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// How far the list scrolls in one step: the pointer within `EDGE` of its top or bottom edge.
    pub(super) fn drag_scroll_step(&self) -> Option<f32> {
        let drag = self.tree_drag.as_ref().filter(|d| d.active)?;
        if drag.at.x > self.sidebar_width() {
            return None;
        }
        let height = self.list_viewport.map_or(self.window_size.height - drag.list_top, |(_, h)| h);
        let inside = drag.at.y - drag.list_top;
        let speed = |gap: f32| ((EDGE - gap.max(0.0)) / EDGE * SCROLL_STEP).max(2.0);
        if inside < EDGE {
            Some(-speed(inside))
        } else if inside > height - EDGE {
            Some(speed(height - inside))
        } else {
            None
        }
    }

    pub(super) fn drag_scroll(&mut self) -> Task<Message> {
        match self.drag_scroll_step() {
            Some(y) => operation::scroll_by(LIST, scrollable::AbsoluteOffset { x: 0.0, y }),
            None => Task::none(),
        }
    }

    fn sidebar_width(&self) -> f32 {
        self.prefs.sidebar_width.unwrap_or(super::SIDEBAR_DEFAULT)
    }

    /// What dropping would do where the pointer is: none when it is outside the list or the
    /// drop would change nothing.
    pub fn drag_target(&self, groups: &Groups<'_>) -> Option<DragTarget> {
        let drag = self.tree_drag.as_ref().filter(|d| d.active)?;
        let (offset, height) = self.list_viewport.unwrap_or((0.0, self.window_size.height - drag.list_top));
        let inside = drag.at.y - drag.list_top;
        if drag.at.x > self.sidebar_width() || inside < 0.0 || inside > height {
            return None;
        }
        let tree = self.tree(groups);
        let spans = tree::spans(&tree::blocks(tree.nodes(self.searching())));
        let y = inside + offset;
        // Drawn only inside the list's view.
        let (shown_top, shown_bottom) = (drag.list_top, drag.list_top + height);
        let window = |y: f32| (drag.list_top + y - offset).clamp(shown_top, shown_bottom);
        match &drag.grab {
            Grab::Session(key) => {
                let head = tree::island_at(&spans, y)?;
                let session = self.model.board.get(key)?;
                let unchanged = match head {
                    HeadKey::Project(name) => *name == self.project_name(session),
                    HeadKey::Others => self.prefs.project(key).is_none(),
                };
                let span = spans.iter().find(|s| s.head.as_ref() == Some(head))?;
                (!unchanged).then(|| DragTarget::Island(head.clone(), window(span.top), window(span.bottom)))
            }
            Grab::Project(_) => {
                let at = tree::insert_at(&spans, y);
                let line = tree::insert_line_y(&spans, at).filter(|l| (offset..=offset + height).contains(l))?;
                Some(DragTarget::Gap(tree.projects.get(at).map(|p| p.name.clone()), window(line)))
            }
        }
    }

    /// What the card at the pointer says.
    pub fn drag_label(&self) -> Option<String> {
        match &self.tree_drag.as_ref().filter(|d| d.active)?.grab {
            Grab::Session(key) => Some(self.model.board.get(key)?.display_name()),
            Grab::Project(name) => Some(name.clone()),
        }
    }

    /// Takes the session back to where it stood before the move.
    pub(super) fn undo_move(&mut self, key: SessionKey, previous: Option<String>) {
        self.prefs.set_project(&key, previous);
        self.prefs.save();
        self.set_status(t("Wieder zurückgelegt.", "Put back."));
    }
}
