//! "In ein anderes Projekt": the small dialog behind the context menu's "In ein anderes Projekt …"
//! and Session → "Anderes Projekt …". A field filters the tree's projects; ↑/↓ choose, ⏎ or a
//! click moves the session, esc closes.

use brain_core::state::SessionKey;
use iced::keyboard::key::Named;
use iced::widget::operation;
use iced::Task;

use super::{unfocus, Brain, Message, Mode, MOVE_PROJECT};

/// The session the dialog moves, what is typed and the chosen line.
#[derive(Default)]
pub struct MoveDialog {
    pub key: Option<SessionKey>,
    pub query: String,
    pub index: usize,
}

/// A line of the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectChoice {
    /// Back into the session's own project (named).
    Back(String),
    /// A project of the tree.
    Existing(String),
    /// A new project, named as typed.
    New(String),
}

impl ProjectChoice {
    /// The project to put the session in; none: back into its own.
    pub fn target(&self) -> Option<String> {
        match self {
            ProjectChoice::Back(_) => None,
            ProjectChoice::Existing(name) | ProjectChoice::New(name) => Some(name.clone()),
        }
    }
}

/// The lines for what is typed. `projects` are the tree's, sorted; `current` is the session's
/// project (left out); `own` is its own project when the user put it elsewhere by hand (then
/// "back into" comes first). Lines must have every typed word; a typed name that is no project
/// adds "new project" at the end.
pub fn project_choices(projects: &[String], current: &str, own: Option<&str>, query: &str) -> Vec<ProjectChoice> {
    let query = query.trim();
    let lowered = query.to_lowercase();
    let words: Vec<&str> = lowered.split_whitespace().collect();
    let matches = |name: &str| {
        let name = name.to_lowercase();
        words.iter().all(|w| name.contains(w))
    };
    let mut choices: Vec<ProjectChoice> = Vec::new();
    if let Some(own) = own.filter(|o| matches(o)) {
        choices.push(ProjectChoice::Back(own.to_string()));
    }
    choices.extend(projects.iter().filter(|p| p.as_str() != current && matches(p)).map(|p| ProjectChoice::Existing(p.clone())));
    let exact = projects.iter().any(|p| p.eq_ignore_ascii_case(query)) || current.eq_ignore_ascii_case(query) || own.is_some_and(|o| o.eq_ignore_ascii_case(query));
    if !query.is_empty() && !exact {
        choices.push(ProjectChoice::New(query.to_string()));
    }
    choices
}

impl Brain {
    /// Opens the dialog for the selected session (the one a context menu was opened on).
    pub(super) fn open_move_project(&mut self) -> Task<Message> {
        let Some(key) = self.selected.clone().filter(|k| self.model.board.get(k).is_some()) else { return Task::none() };
        self.move_project = MoveDialog { key: Some(key), query: String::new(), index: 0 };
        self.mode = Mode::MoveProject;
        self.focused_terminal = None;
        self.armed = None;
        operation::focus(MOVE_PROJECT)
    }

    /// The dialog's lines.
    pub fn move_choices(&self) -> Vec<ProjectChoice> {
        let Some(session) = self.move_project.key.as_ref().and_then(|k| self.model.board.get(k)) else { return Vec::new() };
        let own = self.prefs.project(&session.key).map(|_| self.own_project_name(session));
        project_choices(&self.project_names(), &self.project_name(session), own.as_deref(), &self.move_project.query)
    }

    pub(super) fn move_project_query(&mut self, query: String) {
        self.move_project.query = query;
        self.move_project.index = 0;
    }

    pub(super) fn move_project_pick(&mut self, index: usize) -> Task<Message> {
        let choice = self.move_choices().into_iter().nth(index);
        let key = self.move_project.key.take();
        self.mode = Mode::Normal;
        if let (Some(choice), Some(key)) = (choice, key) {
            self.put_in_project(&key, choice.target());
        }
        unfocus()
    }

    pub(super) fn on_move_project_key(&mut self, named: Option<Named>) -> Task<Message> {
        let count = self.move_choices().len();
        match named {
            Some(Named::ArrowDown) => self.move_project.index = (self.move_project.index + 1).min(count.saturating_sub(1)),
            Some(Named::ArrowUp) => self.move_project.index = self.move_project.index.saturating_sub(1),
            Some(Named::Enter) => return self.move_project_pick(self.move_project.index),
            _ => {}
        }
        Task::none()
    }

    /// The dialog's title: the session's name.
    pub fn move_project_title(&self) -> String {
        let name = self.move_project.key.as_ref().and_then(|k| self.model.board.get(k)).map(|s| s.display_name()).unwrap_or_default();
        crate::tr!("„{name}“ in ein anderes Projekt", "“{name}” into another project")
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectChoice::*;
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_current_project_is_left_out_and_the_rest_stays_in_order() {
        let choices = project_choices(&names(&["api", "brain", "web"]), "brain", None, "");
        assert_eq!(choices, vec![Existing("api".into()), Existing("web".into())]);
    }

    #[test]
    fn typing_filters_by_every_word_ignoring_case() {
        let projects = names(&["Alpha Api", "alpha web", "beta"]);
        assert_eq!(project_choices(&projects, "beta", None, "ALPHA a"), vec![Existing("Alpha Api".into()), Existing("alpha web".into()), New("ALPHA a".into())]);
        assert_eq!(project_choices(&projects, "beta", None, "web alpha"), vec![Existing("alpha web".into()), New("web alpha".into())]);
    }

    #[test]
    fn back_into_its_own_comes_first_and_only_for_a_project_given_by_hand() {
        let projects = names(&["api", "web"]);
        assert_eq!(project_choices(&projects, "web", Some("tools"), ""), vec![Back("tools".into()), Existing("api".into())]);
        assert_eq!(project_choices(&projects, "web", None, ""), vec![Existing("api".into())]);
        assert!(!project_choices(&projects, "web", Some("tools"), "api").contains(&Back("tools".into())), "the filter applies to it too");
    }

    #[test]
    fn a_new_project_is_offered_only_without_an_exact_match() {
        let projects = names(&["api", "web"]);
        assert_eq!(project_choices(&projects, "web", None, "  mobile "), vec![New("mobile".into())]);
        assert_eq!(project_choices(&projects, "web", None, "API"), vec![Existing("api".into())], "an exact match, whatever its case");
        assert_eq!(project_choices(&projects, "web", None, "web"), Vec::<ProjectChoice>::new(), "the current project is no new one");
        assert_eq!(project_choices(&projects, "web", Some("tools"), "tools"), vec![Back("tools".into())], "nor is the own one");
        assert_eq!(project_choices(&projects, "web", None, "ap"), vec![Existing("api".into()), New("ap".into())]);
    }

    #[test]
    fn a_choice_names_where_the_session_goes() {
        assert_eq!(Back("x".into()).target(), None);
        assert_eq!(Existing("x".into()).target(), Some("x".into()));
        assert_eq!(New("x".into()).target(), Some("x".into()));
    }
}
