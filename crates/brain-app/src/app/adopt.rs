//! "Projekte übernehmen": the tree's project list starts empty, and the sessions of every folder
//! stand among the others. This dialog (the card at the top of the tree, or File → "Adopt
//! projects …") lists those folders as projects and takes the chosen ones into the list, once;
//! after that the list grows by hand again.

use std::path::Path;

use brain_core::account::home_dir;
use iced::keyboard::key::Named;

use super::{listed_entry, Brain, Mode};
use crate::format::now_ms;
use crate::tr;

/// Folders with a session this recent are chosen at first.
const RECENT_DAYS: i64 = 7;

/// A project among the others: its name, the folder of its main checkout (none for a name the
/// user gave a session by hand), the sessions it holds, and whether it gets taken over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptEntry {
    pub name: String,
    pub folder: Option<String>,
    pub sessions: usize,
    pub chosen: bool,
}

#[derive(Default)]
pub struct AdoptDialog {
    pub entries: Vec<AdoptEntry>,
}

/// The projects of sessions as `(project name, folder of its main checkout, last activity in ms)`:
/// one entry per project, most recent first. Chosen are those with a session of the last seven
/// days, except the home folder and temporary folders, which hold no project of their own.
pub fn adoption_entries(sessions: &[(String, Option<String>, i64)], now_ms: i64, home: &Path) -> Vec<AdoptEntry> {
    let mut grouped: Vec<(AdoptEntry, i64)> = Vec::new();
    for (name, folder, last_ms) in sessions {
        match grouped.iter_mut().find(|(e, _)| e.name == *name) {
            Some((entry, latest)) => {
                entry.sessions += 1;
                *latest = (*latest).max(*last_ms);
                entry.folder = entry.folder.take().or_else(|| folder.clone());
            }
            None => grouped.push((AdoptEntry { name: name.clone(), folder: folder.clone(), sessions: 1, chosen: false }, *last_ms)),
        }
    }
    let recent_after = now_ms - RECENT_DAYS * 86_400_000;
    for (entry, latest) in &mut grouped {
        let plain = entry.folder.as_deref().is_some_and(|f| is_no_project_folder(Path::new(f), home));
        entry.chosen = *latest >= recent_after && !plain;
    }
    grouped.sort_by(|(a, a_at), (b, b_at)| b_at.cmp(a_at).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    grouped.into_iter().map(|(entry, _)| entry).collect()
}

/// The home folder itself and temporary folders: sessions run there without a project.
fn is_no_project_folder(folder: &Path, home: &Path) -> bool {
    folder == home || folder.starts_with("/tmp") || folder.starts_with("/private/tmp") || folder.starts_with("/var/folders")
}

impl Brain {
    /// Opens the dialog with the projects of the sessions among the others.
    pub(super) fn open_adopt(&mut self) {
        let groups = self.groups(now_ms());
        let others: Vec<(String, Option<String>, i64)> = {
            let tree = self.tree(&groups);
            tree.others
                .iter()
                .filter_map(|row| {
                    let s = row.session;
                    Some((s.cwd.clone()?, self.prefs.project(&s.key).map(str::to_string), s.last_activity_ms))
                })
                .collect()
        };
        let sessions: Vec<(String, Option<String>, i64)> = others
            .into_iter()
            .map(|(cwd, moved, at)| match moved {
                // A session the user put into a project by hand has that project's name only.
                Some(name) => (name, None, at),
                None => {
                    let (name, root) = listed_entry(self.model.project(&cwd));
                    (name, Some(root), at)
                }
            })
            .collect();
        self.adopt = AdoptDialog { entries: adoption_entries(&sessions, now_ms(), &home_dir()) };
        self.armed = None;
        self.mode = Mode::Adopt;
    }

    pub(super) fn adopt_toggle(&mut self, index: usize) {
        if let Some(entry) = self.adopt.entries.get_mut(index) {
            entry.chosen = !entry.chosen;
        }
    }

    pub(super) fn on_adopt_key(&mut self, named: Option<Named>) -> iced::Task<super::Message> {
        if named == Some(Named::Enter) {
            self.adopt_confirm();
        }
        iced::Task::none()
    }

    /// Puts the chosen projects into the tree's list.
    pub(super) fn adopt_confirm(&mut self) {
        let chosen: Vec<AdoptEntry> = self.adopt.entries.iter().filter(|e| e.chosen).cloned().collect();
        self.mode = Mode::Normal;
        if chosen.is_empty() {
            return;
        }
        for entry in &chosen {
            self.prefs.list_project(&entry.name, entry.folder.as_deref());
        }
        self.prefs.save();
        let count = chosen.len();
        self.set_status(tr!("{count} Projekte übernommen.", "Adopted {count} projects."));
    }

    /// "Später": the card is gone for this run of Brain; nothing is remembered.
    pub(super) fn adopt_later(&mut self) {
        self.adopt_dismissed = true;
        self.mode = Mode::Normal;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::project::from_rev_parse;

    const DAY: i64 = 86_400_000;

    /// A session's project as `open_adopt` reads it from the folder.
    fn session(cwd: &str, git: &str, days_ago: i64, now: i64) -> (String, Option<String>, i64) {
        let (name, root) = listed_entry(&from_rev_parse(Path::new(cwd), git));
        (name, Some(root), now - days_ago * DAY)
    }

    #[test]
    fn recent_folders_are_chosen_a_worktree_counts_to_its_main_checkout_and_home_is_left_out() {
        let now = 1_790_000_000_000;
        let sessions = vec![
            session("/w/api", "/w/api/.git\n/w/api\n", 2, now),
            session("/w/api/.worktrees/fix", "/w/api/.git\n/w/api/.worktrees/fix\n", 20, now),
            session("/w/old", "/w/old/.git\n/w/old\n", 30, now),
            session("/Users/me", "", 1, now),
            session("/tmp/scratch", "", 1, now),
        ];
        let entries = adoption_entries(&sessions, now, Path::new("/Users/me"));
        let by_name = |name: &str| entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("no entry {name}"));

        let api = by_name("api");
        assert_eq!((api.sessions, api.chosen, api.folder.as_deref()), (2, true, Some("/w/api")), "the worktree is one project with its main checkout; its recent session decides");
        assert!(!by_name("old").chosen, "only sessions of the last 7 days choose a folder");
        assert!(!by_name("me").chosen, "the home folder is no project");
        assert!(!by_name("scratch").chosen, "nor is a temporary folder");
        assert_eq!(entries.len(), 4);
    }
}
