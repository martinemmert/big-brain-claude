//! The tree on the left: the listed projects as fixed headings in a stable order, their sessions
//! below, the sessions of every other folder at the end ("Weitere"). Nothing here depends on
//! urgency for its place: a session that starts to ask only gets a mark.

use std::collections::HashMap;

use brain_core::changes::SessionChange;
use brain_core::later;
use brain_core::state::{Ask, Phase, Session, SessionKey};

use crate::app::Groups;
use crate::prefs::ListedProject;

/// What a session row shows besides its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Asks something of the user (a permission, a question, `--waiting`): a red ring and a line.
    Asks(Ask),
    /// Ended without a report and its last sentence ends with "?": an amber line, no ring.
    Guess,
    /// Finished and not looked at yet: an amber line.
    Unseen,
    Calm,
    Ended,
}

pub struct Row<'a> {
    pub session: &'a Session,
    pub mark: Mark,
}

pub struct Project<'a> {
    pub name: String,
    pub folded: bool,
    pub hidden: bool,
    /// Sessions the tree shows under it.
    pub rows: Vec<Row<'a>>,
    /// Sessions that live (shown or not): none means the project rests.
    pub live: usize,
    pub notes: Vec<&'a later::Item>,
    pub notes_open: bool,
}

impl Project<'_> {
    pub fn asking(&self) -> usize {
        self.rows.iter().filter(|r| matches!(r.mark, Mark::Asks(_))).count()
    }

    pub fn unseen(&self) -> usize {
        self.rows.iter().filter(|r| matches!(r.mark, Mark::Unseen | Mark::Guess)).count()
    }

    /// The account of its newest live session (else its newest session), for the chip.
    pub fn account(&self) -> Option<&str> {
        let newest = |live: bool| {
            self.rows.iter().filter(|r| (r.mark != Mark::Ended) == live).max_by_key(|r| r.session.last_activity_ms)
        };
        newest(true).or_else(|| newest(false)).map(|r| r.session.key.account.as_str())
    }
}

/// A folded list at the end of the tree: how many sessions it holds and whether it is open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Closed {
    pub count: usize,
    pub open: bool,
}

pub struct Tree<'a> {
    /// The listed projects, alphabetical (case-insensitive).
    pub projects: Vec<Project<'a>>,
    /// Sessions of folders that are not listed.
    pub others: Vec<Row<'a>>,
    pub other_notes: Vec<&'a later::Item>,
    pub others_open: bool,
    /// Listed projects left out of the tree because the user hid them.
    pub hidden_projects: usize,
    /// Snoozed or hidden sessions, quiet background sessions, sessions programs started, and
    /// those that ended over a day ago: each opens in place, under its project.
    pub paused: Closed,
    pub background: Closed,
    pub automated: Closed,
    pub older: Closed,
}

/// What the tree reads from the user's choices.
pub struct Views<'v> {
    pub folded: &'v dyn Fn(&str) -> bool,
    pub hidden: &'v dyn Fn(&str) -> bool,
    pub notes_open: &'v dyn Fn(&str) -> bool,
    pub others_open: bool,
    /// The hidden projects show for a look.
    pub show_hidden: bool,
}

impl<'a> Tree<'a> {
    /// `project_of`: the project name a session is listed under; `notes`: each note for later
    /// with its project's name. Under a search, projects without a match are left out.
    pub fn build(
        groups: &Groups<'a>,
        listed: &[ListedProject],
        project_of: &dyn Fn(&Session) -> String,
        notes: impl IntoIterator<Item = &'a (later::Item, String)>,
        views: &Views,
        searching: bool,
    ) -> Self {
        let marks = marks(groups);
        // Every session with the tree's mark and whether the tree shows it now.
        let mut sessions: Vec<(&'a Session, Mark, bool)> = Vec::new();
        let live = groups.decide.iter().map(|(s, _)| *s).chain(groups.read.iter().copied()).chain(groups.quiet.iter().copied());
        sessions.extend(live.map(|s| (s, marks.get(&s.key).copied().unwrap_or(Mark::Calm), true)));
        let rest = |s: &Session| if s.phase() == Phase::Ended { Mark::Ended } else { Mark::Calm };
        sessions.extend(groups.paused.iter().map(|s| (*s, rest(s), groups.paused_open)));
        sessions.extend(groups.background.iter().map(|s| (*s, rest(s), groups.background_open)));
        sessions.extend(groups.automated.iter().map(|s| (*s, rest(s), groups.automated_open)));
        // Pinned ended sessions stay; the last day's ended ones too, older ones when asked for.
        sessions.extend(groups.saved.iter().map(|s| (*s, Mark::Ended, true)));
        sessions.extend(groups.ended.iter().map(|s| (*s, Mark::Ended, true)));
        sessions.extend(groups.older.iter().map(|s| (*s, Mark::Ended, groups.older_open)));

        let mut projects: Vec<Project<'a>> = listed
            .iter()
            .map(|p| Project {
                name: p.name.clone(),
                folded: (views.folded)(&p.name) && !searching,
                hidden: (views.hidden)(&p.name),
                rows: Vec::new(),
                live: 0,
                notes: Vec::new(),
                notes_open: (views.notes_open)(&p.name) || searching,
            })
            .collect();
        // A fixed order: nothing moves when a session starts to ask.
        projects.sort_by_cached_key(|p| (p.name.to_lowercase(), p.name.clone()));

        let mut others: Vec<Row<'a>> = Vec::new();
        for (session, mark, shown) in sessions {
            let alive = mark != Mark::Ended;
            let name = project_of(session);
            match projects.iter_mut().find(|p| p.name == name) {
                Some(project) => {
                    project.live += usize::from(alive);
                    if shown {
                        project.rows.push(Row { session, mark });
                    }
                }
                None => {
                    if shown {
                        others.push(Row { session, mark });
                    }
                }
            }
        }
        let mut other_notes = Vec::new();
        for (note, name) in notes {
            match projects.iter_mut().find(|p| p.name == *name) {
                Some(project) => project.notes.push(note),
                None => other_notes.push(note),
            }
        }
        for project in &mut projects {
            project.rows.sort_by_key(|r| order(r));
        }
        others.sort_by_key(|r| order(r));

        let hidden_projects = projects.iter().filter(|p| p.hidden).count();
        projects.retain(|p| (!p.hidden || views.show_hidden || searching) && (!searching || !p.rows.is_empty() || !p.notes.is_empty()));
        let closed = |count: usize, open: bool| Closed { count, open };
        Tree {
            projects,
            others,
            other_notes,
            others_open: views.others_open || searching,
            hidden_projects,
            paused: closed(groups.paused.len(), groups.paused_open),
            background: closed(groups.background.len(), groups.background_open),
            automated: closed(groups.automated.len(), groups.automated_open),
            older: closed(groups.older.len(), groups.older_open),
        }
    }

    /// Every session the tree lists, folded or not.
    pub fn sessions(&self) -> Vec<&'a Session> {
        self.projects.iter().flat_map(|p| p.rows.iter().map(|r| r.session)).chain(self.others.iter().map(|r| r.session)).collect()
    }

    /// The sessions ↑↓ reach, in drawing order: not those of a folded project.
    pub fn navigable(&self) -> Vec<&'a Session> {
        let mut all: Vec<&'a Session> = self.projects.iter().filter(|p| !p.folded).flat_map(|p| p.rows.iter().map(|r| r.session)).collect();
        if self.others_open {
            all.extend(self.others.iter().map(|r| r.session));
        }
        all
    }
}

pub const PROJECT_H: f32 = 28.0;
pub const ROW_H: f32 = 26.0;
/// The line under a session that asks or has finished unseen.
pub const STATUS_H: f32 = 18.0;
/// Allow and Deny under a permission.
pub const BUTTONS_H: f32 = 30.0;
pub const NOTE_H: f32 = 24.0;
pub const FOOTER_H: f32 = 30.0;
pub const HINT_H: f32 = 34.0;
pub const GAP: f32 = 1.0;
pub const TOP: f32 = 4.0;

/// One line of the tree in drawing order, so the list's height is known (to keep the selected
/// session in view).
pub enum Node<'t, 'a> {
    /// Nothing to show: `true` after a search without a match, `false` without any project.
    Hint(bool),
    Project(&'t Project<'a>),
    /// A project's session; `nested` under a project (the others have no head of their own).
    Session(&'t Row<'a>),
    /// "Für später · n" under a project (`None`: among the others).
    NotesHead { project: Option<&'t str>, count: usize, open: bool },
    Note { note: &'t later::Item },
    OthersHead { count: usize, open: bool },
    Footer,
    NewProject,
    /// "Änderungen · n · +a −b" under the selected session.
    ChangesHead { count: usize, added: u64, removed: u64 },
    /// One file the selected session changed.
    Change(&'t SessionChange),
}

impl Node<'_, '_> {
    pub fn height(&self) -> f32 {
        match self {
            Node::Hint(_) => HINT_H,
            Node::Project(_) => PROJECT_H,
            Node::Session(row) => row_height(row.mark),
            Node::NotesHead { .. } | Node::Note { .. } | Node::OthersHead { .. } | Node::ChangesHead { .. } | Node::Change(_) => NOTE_H,
            Node::Footer => FOOTER_H,
            Node::NewProject => PROJECT_H,
        }
    }
}

/// A session's row: its name line, and below it what it adds.
pub fn row_height(mark: Mark) -> f32 {
    ROW_H
        + match mark {
            Mark::Asks(Ask::Permission) => STATUS_H + BUTTONS_H,
            Mark::Asks(_) | Mark::Guess | Mark::Unseen => STATUS_H,
            Mark::Calm | Mark::Ended => 0.0,
        }
}

impl<'a> Tree<'a> {
    /// The tree line by line. `searching` with nothing found says so. `changes`: the selected
    /// session and the files it changed, which stand under its row.
    pub fn nodes<'t>(&'t self, searching: bool, changes: Option<(&SessionKey, &'t [SessionChange])>) -> Vec<Node<'t, 'a>> {
        let mut nodes = Vec::new();
        let nothing = self.sessions().is_empty() && self.projects.iter().all(|p| p.notes.is_empty()) && self.other_notes.is_empty();
        if nothing && (searching || self.projects.is_empty()) {
            nodes.push(Node::Hint(searching));
        }
        for project in &self.projects {
            nodes.push(Node::Project(project));
            if project.folded {
                continue;
            }
            for row in &project.rows {
                push_session(&mut nodes, row, changes);
            }
            if !project.notes.is_empty() {
                nodes.push(Node::NotesHead { project: Some(&project.name), count: project.notes.len(), open: project.notes_open });
                if project.notes_open {
                    nodes.extend(project.notes.iter().map(|note| Node::Note { note }));
                }
            }
        }
        let others = self.others.len() + self.other_notes.len();
        if others > 0 {
            nodes.push(Node::OthersHead { count: self.others.len(), open: self.others_open });
            if self.others_open {
                for row in &self.others {
                    push_session(&mut nodes, row, changes);
                }
                if !self.other_notes.is_empty() {
                    nodes.push(Node::NotesHead { project: None, count: self.other_notes.len(), open: true });
                    nodes.extend(self.other_notes.iter().map(|note| Node::Note { note }));
                }
            }
        }
        if self.has_footer() {
            nodes.push(Node::Footer);
        }
        nodes.push(Node::NewProject);
        nodes
    }

    pub fn has_footer(&self) -> bool {
        self.hidden_projects > 0 || [self.paused, self.background, self.automated, self.older].iter().any(|c| c.count > 0)
    }
}

/// A session's row, and under it the files it changed when it is the one `changes` is about.
fn push_session<'t, 'a>(nodes: &mut Vec<Node<'t, 'a>>, row: &'t Row<'a>, changes: Option<(&SessionKey, &'t [SessionChange])>) {
    nodes.push(Node::Session(row));
    let Some((_, files)) = changes.filter(|(key, files)| **key == row.session.key && !files.is_empty()) else { return };
    let sum = |value: fn(&SessionChange) -> Option<u64>| files.iter().filter_map(value).sum::<u64>();
    nodes.push(Node::ChangesHead { count: files.len(), added: sum(|c| c.added), removed: sum(|c| c.removed) });
    nodes.extend(files.iter().map(Node::Change));
}

/// A session's place among its project's: running ones in the order they were started, then the
/// ended ones, newest first. Neither a question nor a finished turn moves a row.
fn order(row: &Row) -> (bool, i64, SessionKey) {
    let s = row.session;
    let ended = row.mark == Mark::Ended;
    let at = if ended { -s.last_activity_ms } else { s.started_ms.unwrap_or(s.last_activity_ms) };
    (ended, at, s.key.clone())
}

fn marks(groups: &Groups<'_>) -> HashMap<SessionKey, Mark> {
    let mut marks = HashMap::new();
    for (session, ask) in &groups.decide {
        marks.insert(session.key.clone(), if *ask == Ask::Maybe { Mark::Guess } else { Mark::Asks(*ask) });
    }
    for session in &groups.read {
        marks.insert(session.key.clone(), Mark::Unseen);
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::event::{Event, Kind, Source};
    use brain_core::state::Board;
    use chrono::TimeZone;

    fn session(id: &str, cwd: &str, started: i64, ended: bool) -> Session {
        let ev = |secs: i64, kind: Kind| Event {
            v: 1,
            ts: chrono::Utc.timestamp_opt(1_790_000_000 + secs, 0).unwrap(),
            account: "main".into(),
            pid: 100 + started as u32,
            session_id: Some(id.into()),
            cwd: Some(cwd.into()),
            source: Source::Hook,
            kind,
            text: None,
            tasks: None,
            name: None,
            ask: None,
        };
        let mut board = Board::default();
        board.apply_event(&ev(started, Kind::Prompt));
        if ended {
            board.apply_event(&ev(started + 5, Kind::SessionEnd));
        }
        board.get(&SessionKey { account: "main".into(), id: id.into() }).unwrap().clone()
    }

    fn groups<'a>(decide: Vec<(&'a Session, Ask)>, read: Vec<&'a Session>, quiet: Vec<&'a Session>, ended: Vec<&'a Session>) -> Groups<'a> {
        Groups {
            saved: vec![],
            decide,
            read,
            quiet,
            projects: vec![],
            projects_open: true,
            hidden_open: false,
            paused: vec![],
            background: vec![],
            automated: vec![],
            ended,
            older: vec![],
            older_open: false,
            paused_open: false,
            background_open: false,
            automated_open: false,
        }
    }

    fn listed(names: &[&str]) -> Vec<ListedProject> {
        names.iter().map(|n| ListedProject { name: n.to_string(), folder: Some(format!("/w/{n}")) }).collect()
    }

    fn by_folder(s: &Session) -> String {
        s.cwd.as_deref().and_then(|c| c.trim_start_matches("/w/").split('/').next()).unwrap_or_default().to_string()
    }

    fn never(_: &str) -> bool {
        false
    }

    fn views() -> Views<'static> {
        Views { folded: &never, hidden: &never, notes_open: &never, others_open: true, show_hidden: false }
    }

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| r.session.key.id.clone()).collect()
    }

    #[test]
    fn projects_stand_alphabetically_and_do_not_move_when_a_session_starts_to_ask() {
        let (a, z) = (session("a", "/w/api", 1, false), session("z", "/w/Zeta", 2, false));
        let list = listed(&["Zeta", "docs", "api"]);
        let quiet = Tree::build(&groups(vec![], vec![], vec![&a, &z], vec![]), &list, &by_folder, [], &views(), false);
        let asking = Tree::build(&groups(vec![(&z, Ask::Permission)], vec![], vec![&a], vec![]), &list, &by_folder, [], &views(), false);
        let order = |t: &Tree| t.projects.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        assert_eq!(order(&quiet), ["api", "docs", "Zeta"]);
        assert_eq!(order(&asking), order(&quiet));
        assert_eq!(asking.projects[2].rows[0].mark, Mark::Asks(Ask::Permission));
    }

    #[test]
    fn sessions_of_a_project_keep_their_order_whatever_their_mark() {
        let (first, second) = (session("first", "/w/api", 1, false), session("second", "/w/api", 2, false));
        let list = listed(&["api"]);
        let calm = Tree::build(&groups(vec![], vec![], vec![&first, &second], vec![]), &list, &by_folder, [], &views(), false);
        let swapped = Tree::build(&groups(vec![(&second, Ask::Question)], vec![&first], vec![], vec![]), &list, &by_folder, [], &views(), false);
        assert_eq!(names(&calm.projects[0].rows), ["first", "second"]);
        assert_eq!(names(&swapped.projects[0].rows), ["first", "second"]);
        assert_eq!(swapped.projects[0].rows[0].mark, Mark::Unseen);
    }

    #[test]
    fn sessions_of_a_folder_that_is_not_listed_go_to_the_others_and_ended_ones_come_last_dimmed() {
        let (mine, stray, gone) = (session("mine", "/w/api", 1, false), session("stray", "/w/notes", 2, false), session("gone", "/w/api", 0, true));
        let tree = Tree::build(&groups(vec![], vec![], vec![&mine, &stray], vec![&gone]), &listed(&["api"]), &by_folder, [], &views(), false);
        assert_eq!(names(&tree.others), ["stray"]);
        assert_eq!(names(&tree.projects[0].rows), ["mine", "gone"]);
        assert_eq!(tree.projects[0].rows[1].mark, Mark::Ended);
        assert_eq!(tree.navigable().iter().map(|s| s.key.id.as_str()).collect::<Vec<_>>(), ["mine", "gone", "stray"]);
    }

    #[test]
    fn a_project_without_a_live_session_rests_and_stays() {
        let gone = session("gone", "/w/api", 0, true);
        let tree = Tree::build(&groups(vec![], vec![], vec![], vec![&gone]), &listed(&["api", "legacy"]), &by_folder, [], &views(), false);
        assert_eq!(tree.projects.len(), 2);
        assert!(tree.projects.iter().all(|p| p.live == 0));
    }

    #[test]
    fn a_hidden_project_and_its_sessions_stay_out_of_the_tree_and_the_others_until_shown() {
        let s = session("s", "/w/api", 1, false);
        let hidden = |name: &str| name == "api";
        let mut v = views();
        v.hidden = &hidden;
        let tree = Tree::build(&groups(vec![], vec![], vec![&s], vec![]), &listed(&["api"]), &by_folder, [], &v, false);
        assert!(tree.projects.is_empty() && tree.others.is_empty());
        assert_eq!(tree.hidden_projects, 1);
        v.show_hidden = true;
        let shown = Tree::build(&groups(vec![], vec![], vec![&s], vec![]), &listed(&["api"]), &by_folder, [], &v, false);
        assert_eq!(shown.projects.len(), 1);
    }

    #[test]
    fn a_folded_project_hides_its_sessions_from_the_arrow_keys() {
        let s = session("s", "/w/api", 1, false);
        let folded = |name: &str| name == "api";
        let mut v = views();
        v.folded = &folded;
        let tree = Tree::build(&groups(vec![], vec![], vec![&s], vec![]), &listed(&["api"]), &by_folder, [], &v, false);
        assert!(tree.projects[0].folded && tree.navigable().is_empty());
    }

    #[test]
    fn notes_for_later_sit_in_their_project_or_among_the_others() {
        let note = |id: &str| later::Item { id: id.into(), ts: chrono::Utc::now(), text: id.into(), cwd: None, account: None, session_id: None, session_name: None };
        let notes = vec![(note("in-api"), "api".to_string()), (note("stray"), "notes".to_string())];
        let tree = Tree::build(&groups(vec![], vec![], vec![], vec![]), &listed(&["api"]), &by_folder, &notes, &views(), false);
        assert_eq!(tree.projects[0].notes.len(), 1);
        assert_eq!(tree.other_notes.len(), 1);
    }

    #[test]
    fn the_changed_files_stand_under_the_selected_session_only() {
        let (first, second) = (session("first", "/w/api", 1, false), session("second", "/w/api", 2, false));
        let change = |relative: &str, added: u64, removed: u64| SessionChange {
            path: format!("/w/api/{relative}"),
            relative: relative.into(),
            last_ms: 0,
            added: Some(added),
            removed: Some(removed),
            root: "/w/api".into(),
            git: None,
        };
        let files = vec![change("src/a.rs", 10, 2), change("b.rs", 5, 0)];
        let tree = Tree::build(&groups(vec![], vec![], vec![&first, &second], vec![]), &listed(&["api"]), &by_folder, [], &views(), false);

        let nodes = tree.nodes(false, Some((&second.key, &files)));

        let shape: Vec<String> = nodes
            .iter()
            .map(|n| match n {
                Node::Project(_) => "project".to_string(),
                Node::Session(row) => row.session.key.id.clone(),
                Node::ChangesHead { count, added, removed } => format!("changes {count} +{added} -{removed}"),
                Node::Change(c) => c.relative.clone(),
                _ => "other".to_string(),
            })
            .collect();
        assert_eq!(shape, ["project", "first", "second", "changes 2 +15 -2", "src/a.rs", "b.rs", "other"]);
        // No files read yet, or none changed: the tree is as it was.
        assert_eq!(tree.nodes(false, Some((&second.key, &[]))).len(), tree.nodes(false, None).len());
    }

    #[test]
    fn a_search_leaves_out_projects_without_a_match() {
        let s = session("s", "/w/api", 1, false);
        let tree = Tree::build(&groups(vec![], vec![], vec![&s], vec![]), &listed(&["api", "docs"]), &by_folder, [], &views(), true);
        assert_eq!(tree.projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["api"]);
    }
}
