//! Git worktrees. Two things:
//! - "Worktrees prüfen …": a dialog that lists the worktrees of the tree's projects (read in the
//!   background, reading only), says which can go, and removes them after asking
//!   (`git worktree remove`, never forced; `git worktree prune` for those whose folder is gone).
//! - Before a session that works in a worktree ends, clears or is deleted, Brain offers to ask it
//!   first whether it still needs the worktree. It sends the question, waits until the session's
//!   turn is over, shows the answer, and only then does what was asked for.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use brain_core::state::{Phase, Session, SessionKey};
use brain_core::worktrees::{self, Inspection, Verdict, WorktreeRef};
use iced::Task;

use super::{off_thread, Brain, Confirm, Message, Mode};
use crate::format::now_ms;
use crate::i18n::t;
use crate::tr;

/// How long a session gets to answer (and, before that, to finish what it is doing).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// What the user wanted to do with the session before Brain asked about the worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    End,
    Clear,
    Delete,
}

impl Intent {
    /// "beenden" / "end", for "Einfach …" and "Jetzt …".
    pub fn verb(self) -> &'static str {
        match self {
            Intent::End => t("beenden", "end"),
            Intent::Clear => t("clear", "clear"),
            Intent::Delete => t("löschen", "delete"),
        }
    }
}

/// A linked worktree of a project, as the dialog lists it.
#[derive(Debug, Clone)]
pub struct WorktreeRow {
    pub project: String,
    pub main: PathBuf,
    pub inspection: Inspection,
}

#[derive(Default)]
pub struct WorktreeDialog {
    /// Only this project's worktrees.
    pub filter: Option<String>,
    pub rows: Vec<WorktreeRow>,
    pub loading: bool,
    /// Which read the answer belongs to: a newer one makes an older one void.
    scan: u64,
}

/// The question before ending, clearing or deleting a session in a worktree; once the session
/// answered, its answer. `intent: None` is a question from the worktree dialog: nothing follows.
#[derive(Debug, Clone)]
pub struct WorktreeAsk {
    pub key: SessionKey,
    pub intent: Option<Intent>,
    pub worktree: WorktreeRef,
    pub name: String,
    pub answer: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStep {
    /// Waits until the session can be asked: ready, and not working.
    Waiting,
    /// The question is sent; waits for the turn to end.
    Asked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckNext {
    Wait,
    Send,
    Answered,
    GiveUp,
}

/// A session Brain asked (or is about to ask) about its worktree.
pub struct PendingWorktreeCheck {
    pub ask: WorktreeAsk,
    pub step: CheckStep,
    /// Since when it is in this step.
    pub since: Instant,
    /// When the question went out (epoch ms).
    pub sent_ms: i64,
    /// The session was seen working on it: a turn that ends after that is its answer.
    pub worked: bool,
}

/// The next move: `ready`: the session can take the question; `answered`: its turn ended after
/// the question went out.
pub fn check_next(step: CheckStep, ready: bool, answered: bool, waited: Duration) -> CheckNext {
    match step {
        CheckStep::Waiting if ready => CheckNext::Send,
        CheckStep::Asked if answered => CheckNext::Answered,
        _ if waited >= ANSWER_TIMEOUT => CheckNext::GiveUp,
        _ => CheckNext::Wait,
    }
}

/// Whether a session lives: its process runs (asleep and ended ones don't).
fn lives(session: &Session) -> bool {
    !matches!(session.phase(), Phase::Ended | Phase::Asleep)
}

/// The sessions whose folder is in `worktree`, the living ones first.
fn sessions_in<'a>(sessions: Vec<&'a Session>, worktree: &Path) -> Vec<&'a Session> {
    let mut found: Vec<&Session> = sessions.into_iter().filter(|s| s.cwd.as_deref().is_some_and(|cwd| Path::new(cwd).starts_with(worktree))).collect();
    found.sort_by_key(|s| (!lives(s), std::cmp::Reverse(s.last_activity_ms)));
    found
}

impl Brain {
    /// The sessions working in `worktree`, the living ones first.
    pub fn sessions_in_worktree(&self, worktree: &Path) -> Vec<&Session> {
        sessions_in(self.model.board.sorted(), worktree)
    }

    /// What a row says, with the sessions of the moment.
    pub fn worktree_verdict(&self, row: &WorktreeRow) -> Verdict {
        let in_use = self.sessions_in_worktree(&row.inspection.worktree.path).iter().any(|s| lives(s));
        worktrees::classify(&row.inspection, in_use)
    }

    /// Opens the dialog, for one project or all, and reads the worktrees in the background.
    pub(super) fn open_worktrees(&mut self, project: Option<String>) -> Task<Message> {
        self.project_menu = None;
        self.worktrees = WorktreeDialog { filter: project, ..WorktreeDialog::default() };
        self.mode = Mode::Worktrees;
        self.focused_terminal = None;
        self.scan_worktrees()
    }

    fn scan_worktrees(&mut self) -> Task<Message> {
        let filter = self.worktrees.filter.clone();
        let mut folders: Vec<(String, PathBuf)> = Vec::new();
        for project in self.prefs.listed_projects() {
            let Some(folder) = project.folder.as_deref().map(PathBuf::from) else { continue };
            if filter.as_deref().is_some_and(|f| f != project.name) || folders.iter().any(|(_, f)| *f == folder) {
                continue;
            }
            folders.push((project.name.clone(), folder));
        }
        self.worktrees.scan += 1;
        self.worktrees.loading = true;
        let scan = self.worktrees.scan;
        let read = off_thread(move || {
            folders
                .into_iter()
                .flat_map(|(project, main)| worktrees::scan(&main).into_iter().map(move |inspection| WorktreeRow { project: project.clone(), main: main.clone(), inspection }))
                .collect::<Vec<_>>()
        });
        Task::perform(read, move |rows| Message::WorktreesRead(scan, rows))
    }

    pub(super) fn worktrees_read(&mut self, scan: u64, rows: Vec<WorktreeRow>) {
        if scan == self.worktrees.scan {
            self.worktrees.rows = rows;
            self.worktrees.loading = false;
        }
    }

    /// [Entfernen]: asks first.
    pub(super) fn ask_remove_worktree(&mut self, index: usize) {
        let Some(row) = self.worktrees.rows.get(index) else { return };
        let confirm = Confirm::RemoveWorktree { main: row.main.clone(), path: row.inspection.worktree.path.clone() };
        self.confirm = Some((confirm, self.mode));
        self.mode = Mode::Confirm;
    }

    /// [Aufräumen]: asks first.
    pub(super) fn ask_prune_worktrees(&mut self, index: usize) {
        let Some(row) = self.worktrees.rows.get(index) else { return };
        self.confirm = Some((Confirm::PruneWorktrees { main: row.main.clone() }, self.mode));
        self.mode = Mode::Confirm;
    }

    /// Confirmed: runs `git worktree remove` or `prune` off the UI thread; the dialog opens again
    /// with what git said.
    pub(super) fn run_worktree_command(&mut self, what: Confirm) -> Task<Message> {
        self.mode = Mode::Worktrees;
        self.set_status(t("Git arbeitet …", "Git is working …"));
        let work = off_thread(move || match what {
            Confirm::RemoveWorktree { main, path } => worktrees::remove(&main, &path).map(|_| t("Worktree entfernt (der Branch bleibt).", "Worktree removed (the branch stays).").to_string()),
            Confirm::PruneWorktrees { main } => worktrees::prune(&main).map(|_| t("Verwaiste Worktrees aufgeräumt.", "Orphaned worktrees cleaned up.").to_string()),
            _ => Err(String::new()),
        });
        Task::perform(work, Message::WorktreeDone)
    }

    pub(super) fn worktree_done(&mut self, result: Result<String, String>) -> Task<Message> {
        self.set_status(match result {
            Ok(done) => done,
            Err(err) => tr!("Git sagt: {err}", "Git says: {err}"),
        });
        if self.mode == Mode::Worktrees {
            return self.scan_worktrees();
        }
        Task::none()
    }

    /// [Session fragen] in the dialog: the question goes to the session of this row.
    pub(super) fn ask_worktree_session(&mut self, index: usize) -> Task<Message> {
        let Some(row) = self.worktrees.rows.get(index) else { return Task::none() };
        let path = row.inspection.worktree.path.clone();
        let worktree = WorktreeRef { path: path.clone(), main: row.main.clone(), branch: row.inspection.worktree.branch.clone() };
        let Some((key, name, ended)) = self.sessions_in_worktree(&path).first().map(|s| (s.key.clone(), s.display_name(), s.phase() == Phase::Ended)) else { return Task::none() };
        if self.blocked_in_demo() {
            return Task::none();
        }
        if ended {
            // An ended session first goes on, the existing way (a new tab); the question follows
            // once it is up.
            self.selected = Some(key.clone());
            self.resume_selected();
        }
        self.start_worktree_check(WorktreeAsk { key, intent: None, worktree, name, answer: None });
        Task::none()
    }

    fn start_worktree_check(&mut self, ask: WorktreeAsk) {
        let name = ask.name.clone();
        self.pending_worktree_check = Some(PendingWorktreeCheck { ask, step: CheckStep::Waiting, since: Instant::now(), sent_ms: 0, worked: false });
        self.set_status(tr!("„{name}“ wird zu ihrem Worktree gefragt …", "Asking “{name}” about its worktree …"));
    }

    /// The entry of "End", "Clear …" and "Delete …": a session that works in a worktree gets asked
    /// about it first; the check of its folder runs off the UI thread.
    pub(super) fn guarded(&mut self, intent: Intent) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        let (key, cwd, phase) = (session.key.clone(), session.cwd.clone(), session.phase());
        // Ended sessions can't be asked, and an asleep one that ends is only taken off the list.
        let askable = phase != Phase::Ended && !(intent == Intent::End && phase == Phase::Asleep) && self.can_send(&key);
        let Some(cwd) = cwd.filter(|_| askable && !self.model.is_demo()) else { return self.run_intent(&key, intent) };
        let found = off_thread(move || worktrees::worktree_of(Path::new(&cwd)));
        Task::perform(found, move |worktree| Message::WorktreeChecked(key.clone(), intent, worktree))
    }

    pub(super) fn worktree_checked(&mut self, key: SessionKey, intent: Intent, worktree: Option<WorktreeRef>) -> Task<Message> {
        if self.selected.as_ref() != Some(&key) || self.mode != Mode::Normal {
            return Task::none();
        }
        let Some(worktree) = worktree else { return self.run_intent(&key, intent) };
        let name = self.model.board.get(&key).map(|s| s.display_name()).unwrap_or_default();
        self.worktree_ask = Some(WorktreeAsk { key, intent: Some(intent), worktree, name, answer: None });
        self.mode = Mode::WorktreeAsk;
        self.focused_terminal = None;
        Task::none()
    }

    /// What was asked for, the existing way.
    pub(super) fn run_intent(&mut self, key: &SessionKey, intent: Intent) -> Task<Message> {
        self.selected = Some(key.clone());
        self.mode = Mode::Normal;
        match intent {
            Intent::End => self.end_selected(),
            Intent::Clear => self.open_fresh(),
            Intent::Delete => {
                self.ask_delete();
                Task::none()
            }
        }
    }

    /// ⏎ or the primary button of the question: Brain asks the session.
    pub(super) fn send_worktree_question(&mut self) -> Task<Message> {
        let Some(ask) = self.worktree_ask.take() else { return Task::none() };
        self.mode = Mode::Normal;
        self.start_worktree_check(ask);
        Task::none()
    }

    /// "Einfach beenden" and "Jetzt beenden": no more questions.
    pub(super) fn do_without_asking(&mut self) -> Task<Message> {
        let Some(WorktreeAsk { key, intent: Some(intent), .. }) = self.worktree_ask.take() else { return Task::none() };
        self.run_intent(&key, intent)
    }

    /// ⏎ in the dialog: ask, or, once it has answered, go on.
    pub(super) fn on_worktree_ask_enter(&mut self) -> Task<Message> {
        match self.worktree_ask.as_ref().map(|a| a.answer.is_some()) {
            Some(true) => self.do_without_asking(),
            Some(false) => self.send_worktree_question(),
            None => Task::none(),
        }
    }

    /// On every tick: takes a question through its steps (see [`check_next`]). Like a prompt
    /// from the composer, it waits until the session waits for input; an asleep one wakes for it.
    pub(super) fn continue_worktree_check(&mut self) -> Task<Message> {
        let Some(mut check) = self.pending_worktree_check.take() else { return Task::none() };
        let key = check.ask.key.clone();
        let name = check.ask.name.clone();
        let Some(session) = self.model.board.get(&key) else { return Task::none() };
        let ready = session.waits_for_input() && self.can_send(&key);
        let waits = session.waits_for_input();
        let since = session.phase_since_ms();
        check.worked |= check.step == CheckStep::Asked && !waits && !self.is_waking(&key);
        let answered = check.worked && waits && since > check.sent_ms;
        match check_next(check.step, ready, answered, check.since.elapsed()) {
            CheckNext::Wait => {}
            CheckNext::Send => {
                if !self.send_text(&key, &worktrees::check_prompt(&check.ask.worktree)) {
                    return Task::none();
                }
                check.step = CheckStep::Asked;
                check.since = Instant::now();
                check.sent_ms = now_ms();
            }
            CheckNext::Answered => {
                let ask = check.ask;
                let transcript = self.model.transcript_path(&key);
                let read = off_thread(move || {
                    let messages = transcript.map(|path| brain_core::transcript::read_recent_messages(&path, 20)).unwrap_or_default();
                    brain_core::history::last_answer(&messages).map(|m| m.text.trim().to_string())
                });
                return Task::perform(read, move |answer| Message::WorktreeAnswered(ask.clone(), answer));
            }
            CheckNext::GiveUp => {
                self.set_status(tr!("„{name}“ hat in 10 Minuten nicht geantwortet – nichts getan.", "“{name}” didn't answer within 10 minutes – nothing done."));
                return Task::none();
            }
        }
        self.pending_worktree_check = Some(check);
        Task::none()
    }

    /// The session answered: with an action waiting, its answer is shown first; asked from the
    /// worktree dialog, it goes to the status line and the dialog reads again.
    pub(super) fn worktree_answered(&mut self, mut ask: WorktreeAsk, answer: Option<String>) -> Task<Message> {
        let answer = answer.filter(|a| !a.is_empty()).unwrap_or_else(|| t("(Keine Antwort gefunden.)", "(No answer found.)").to_string());
        if ask.intent.is_some() {
            ask.answer = Some(answer);
            self.worktree_ask = Some(ask);
            self.mode = Mode::WorktreeAsk;
            self.focused_terminal = None;
            return Task::none();
        }
        let (name, short) = (ask.name, crate::format::clip(&answer, 240));
        self.set_status(tr!("„{name}“: {short}", "“{name}”: {short}"));
        if self.mode == Mode::Worktrees {
            return self.scan_worktrees();
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn waits_until_the_session_is_ready_then_asks_then_waits_for_the_end_of_its_turn() {
        assert_eq!(check_next(CheckStep::Waiting, false, false, 30 * S), CheckNext::Wait);
        assert_eq!(check_next(CheckStep::Waiting, true, false, S), CheckNext::Send);
        assert_eq!(check_next(CheckStep::Asked, true, false, 5 * S), CheckNext::Wait);
        assert_eq!(check_next(CheckStep::Asked, true, true, 20 * S), CheckNext::Answered);
    }

    #[test]
    fn a_session_that_stays_silent_is_given_up_after_ten_minutes() {
        assert_eq!(check_next(CheckStep::Asked, false, false, 599 * S), CheckNext::Wait);
        assert_eq!(check_next(CheckStep::Asked, false, false, 600 * S), CheckNext::GiveUp);
        assert_eq!(check_next(CheckStep::Waiting, false, false, 600 * S), CheckNext::GiveUp);
        // An answer that came in time still counts.
        assert_eq!(check_next(CheckStep::Asked, true, true, 600 * S), CheckNext::Answered);
    }
}
