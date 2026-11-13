//! Artifacts the sessions published (Claude Code's `Artifact` tool), read from their transcripts
//! in the background. Two places show them:
//! - the title bar's "Artefakte · n" for the selected session, a popover like the list of changed
//!   files;
//! - "Artefakte …" (Window menu, palette): the artifacts of all sessions of the last 14 days,
//!   by project, with a field that filters by title and session name; a click or ⏎ opens one.

use std::time::{Duration, Instant};

use brain_core::artifacts::{self, Artifact};
use brain_core::state::SessionKey;
use iced::keyboard::key::Named;
use iced::widget::operation;
use iced::Task;

use super::{off_thread, Brain, Message, Mode, ARTIFACT_SEARCH};
use crate::format::now_ms;

/// The collected list goes back this far, by the session's last activity.
const DIALOG_DAYS: i64 = 14;
/// A growing transcript is read again at most this often.
const EVERY: Duration = Duration::from_secs(10);

/// The artifacts of the selected session, as of a transcript size.
pub struct ArtifactCache {
    pub key: SessionKey,
    pub len: u64,
    pub loaded: Instant,
    pub list: Vec<Artifact>,
}

/// One artifact of the collected dialog, with the session it belongs to.
#[derive(Debug, Clone)]
pub struct ArtifactRow {
    pub project: String,
    pub session: String,
    pub artifact: Artifact,
}

#[derive(Default)]
pub struct ArtifactDialog {
    pub query: String,
    pub index: usize,
    pub rows: Vec<ArtifactRow>,
    pub loading: bool,
    /// Which read the answer belongs to: a newer one makes an older one void.
    scan: u64,
}

/// The rows a query leaves (every typed word is in the title or the session's name), grouped by
/// project: the project with the newest artifact first, inside it the newest first.
pub fn arranged<'a>(rows: &'a [ArtifactRow], query: &str) -> Vec<&'a ArtifactRow> {
    let lowered = query.to_lowercase();
    let words: Vec<&str> = lowered.split_whitespace().collect();
    let mut found: Vec<&ArtifactRow> = rows
        .iter()
        .filter(|row| {
            let hay = format!("{} {}", row.artifact.title.as_deref().unwrap_or_default(), row.session).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect();
    let newest = |project: &str| found.iter().filter(|r| r.project == project).map(|r| r.artifact.last_ms).max().unwrap_or(0);
    let mut order: Vec<(i64, String)> = Vec::new();
    for row in &found {
        if !order.iter().any(|(_, p)| *p == row.project) {
            order.push((newest(&row.project), row.project.clone()));
        }
    }
    order.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.sort_by(|a, b| {
        let place = |r: &ArtifactRow| order.iter().position(|(_, p)| *p == r.project);
        place(a).cmp(&place(b)).then_with(|| b.artifact.last_ms.cmp(&a.artifact.last_ms)).then_with(|| a.artifact.url.cmp(&b.artifact.url))
    });
    found
}

impl Brain {
    /// The selected session's artifacts, once read; empty while there are none.
    pub fn selected_artifacts(&self) -> &[Artifact] {
        match (&self.selected, &self.artifacts) {
            (Some(key), Some(cache)) if cache.key == *key => &cache.list,
            _ => &[],
        }
    }

    /// Reads the selected session's artifacts when it is selected and when its transcript grew
    /// (at most every few seconds).
    pub(super) fn load_artifacts(&mut self) -> Task<Message> {
        if self.artifacts_loading || self.model.is_demo() {
            return Task::none();
        }
        let Some(key) = self.selected.clone() else { return Task::none() };
        let Some((path, len)) = self.conversation.as_ref().filter(|c| c.key == key).and_then(|c| c.transcript().map(|(p, l)| (p.to_path_buf(), l))) else { return Task::none() };
        if self.artifacts.as_ref().is_some_and(|c| c.key == key && (c.len == len || c.loaded.elapsed() < EVERY)) {
            return Task::none();
        }
        self.artifacts_loading = true;
        Task::perform(off_thread(move || artifacts::session_artifacts(&path)), move |list| Message::ArtifactsLoaded(key.clone(), len, list))
    }

    pub(super) fn artifacts_loaded(&mut self, key: SessionKey, len: u64, list: Vec<Artifact>) {
        self.artifacts_loading = false;
        self.artifacts = Some(ArtifactCache { key, len, loaded: Instant::now(), list });
        if self.artifacts_open && self.selected_artifacts().is_empty() {
            self.artifacts_open = false;
        }
    }

    /// The title bar's button: the popover opens or closes. Never open together with the list of
    /// changes or the status popup; the open one has the keyboard.
    pub(super) fn toggle_artifacts_popover(&mut self) -> Task<Message> {
        if self.artifacts_open {
            self.artifacts_open = false;
            return Task::none();
        }
        if self.selected_artifacts().is_empty() {
            return Task::none();
        }
        self.artifacts_open = true;
        self.changes_open = false;
        self.status_open = false;
        self.keys_leave_terminal()
    }

    /// A click on an artifact: the browser opens it.
    pub(super) fn open_artifact(&mut self, url: String) -> Task<Message> {
        self.artifacts_open = false;
        self.update(Message::OpenUrl(url))
    }

    /// Opens the collected list and reads the sessions of the last 14 days in the background.
    pub(super) fn open_artifact_dialog(&mut self) -> Task<Message> {
        self.project_menu = None;
        self.artifacts_open = false;
        let since = now_ms() - DIALOG_DAYS * 86_400_000;
        let sessions: Vec<(brain_core::account::Account, String, String, String)> = self
            .model
            .board
            .sorted()
            .into_iter()
            .filter(|s| s.last_activity_ms >= since)
            .filter_map(|s| {
                let account = self.model.accounts.iter().find(|a| a.id == s.key.account)?.clone();
                Some((account, s.key.id.clone(), self.project_name(s), s.display_name()))
            })
            .collect();
        self.artifact_dialog = ArtifactDialog { scan: self.artifact_dialog.scan + 1, loading: true, ..ArtifactDialog::default() };
        let scan = self.artifact_dialog.scan;
        self.mode = Mode::Artifacts;
        self.focused_terminal = None;
        let read = off_thread(move || {
            sessions
                .into_iter()
                .filter_map(|(account, id, project, session)| Some((brain_core::transcript::find_transcript(&account, &id)?, project, session)))
                .flat_map(|(transcript, project, session)| {
                    artifacts::session_artifacts(&transcript).into_iter().map(move |artifact| ArtifactRow { project: project.clone(), session: session.clone(), artifact })
                })
                .collect::<Vec<_>>()
        });
        Task::batch([Task::perform(read, move |rows| Message::ArtifactDialogRead(scan, rows)), operation::focus(ARTIFACT_SEARCH)])
    }

    pub(super) fn artifact_dialog_read(&mut self, scan: u64, rows: Vec<ArtifactRow>) {
        if scan == self.artifact_dialog.scan {
            self.artifact_dialog.rows = rows;
            self.artifact_dialog.loading = false;
        }
    }

    pub(super) fn artifact_query(&mut self, query: String) {
        self.artifact_dialog.query = query;
        self.artifact_dialog.index = 0;
    }

    pub(super) fn artifact_pick(&mut self, index: usize) -> Task<Message> {
        let url = arranged(&self.artifact_dialog.rows, &self.artifact_dialog.query).get(index).map(|r| r.artifact.url.clone());
        url.map_or(Task::none(), |url| self.update(Message::OpenUrl(url)))
    }

    pub(super) fn on_artifact_dialog_key(&mut self, named: Option<Named>) -> Task<Message> {
        let count = arranged(&self.artifact_dialog.rows, &self.artifact_dialog.query).len();
        match named {
            Some(Named::ArrowDown) => self.artifact_dialog.index = (self.artifact_dialog.index + 1).min(count.saturating_sub(1)),
            Some(Named::ArrowUp) => self.artifact_dialog.index = self.artifact_dialog.index.saturating_sub(1),
            Some(Named::Enter) => return self.artifact_pick(self.artifact_dialog.index),
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(project: &str, session: &str, title: &str, last_ms: i64) -> ArtifactRow {
        ArtifactRow {
            project: project.into(),
            session: session.into(),
            artifact: Artifact { url: format!("https://claude.ai/artifact/{title}"), title: Some(title.into()), last_ms },
        }
    }

    #[test]
    fn projects_by_their_newest_artifact_and_the_filter_reads_title_and_session() {
        let rows = vec![row("api", "Umbau", "alt", 10), row("brain", "Layout", "neu", 50), row("api", "Umbau", "mittel", 30)];
        let all: Vec<&str> = arranged(&rows, "").iter().map(|r| r.artifact.title.as_deref().unwrap()).collect();
        assert_eq!(all, ["neu", "mittel", "alt"], "brain has the newest, then api's by time");
        let by_session: Vec<&str> = arranged(&rows, "umbau MITTEL").iter().map(|r| r.artifact.title.as_deref().unwrap()).collect();
        assert_eq!(by_session, ["mittel"]);
    }
}
