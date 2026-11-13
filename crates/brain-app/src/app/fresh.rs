//! "Clear …" (`/clear`, done cleanly): the dialog asks for an optional next prompt; ⌘⏎ starts a
//! new background session in the same folder and account, with the same name and model, and ends
//! the old one, which stays in the list as an ended session. Plugins and Brain's mod load anew.
//! The new session starts empty; once it is ready (its mod takes requests and it waits for input)
//! Brain renames it like the old one (`/rename`), then feeds the prompt.

use brain_core::agents::Start;
use brain_core::state::{Phase, SessionKey};
use iced::widget::{operation, text_editor};
use iced::Task;
use std::time::{Duration, Instant};

use super::{off_thread, unfocus, Brain, Message, Mode};
use crate::format::now_ms;
use crate::i18n::t;
use crate::tr;

/// The prompt field of the dialog.
pub const FRESH_PROMPT: &str = "fresh-prompt";

/// The session the dialog clears and the next prompt typed.
#[derive(Default)]
pub struct FreshDialog {
    pub key: Option<SessionKey>,
    pub prompt: text_editor::Content,
}

/// How long the new session gets to become ready.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long Brain waits to see the rename before the prompt goes anyway.
const RENAME_TIMEOUT: Duration = Duration::from_secs(10);

/// The new session's options from the old one: its model only (empty, no prompt, no name).
pub fn fresh_start(model: Option<&str>, account: &str) -> Start {
    Start { model: model.map(str::to_string), extra: crate::browser::session_args(account), ..Default::default() }
}

/// Where a session started by "Clear …" is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshStep {
    /// Started; waits until it is ready.
    WaitingReady,
    /// `/rename` is sent; waits until the name shows.
    Renamed,
}

/// What Brain does next for such a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshNext {
    Wait,
    Rename,
    Prompt,
    Finish,
    GiveUp,
}

/// A session started by "Clear …" that Brain still takes through its steps.
pub struct PendingFresh {
    pub account: String,
    pub agent_id: String,
    pub name: String,
    pub prompt: Option<String>,
    pub key: Option<SessionKey>,
    pub step: FreshStep,
    /// Since when it is in this step.
    pub since: Instant,
}

/// The next move for a session in `step`, which has been there for `waited`. `ready`: its mod
/// takes requests and it waits for input; `renamed`: its name shows the new one.
pub fn fresh_next(step: FreshStep, ready: bool, renamed: bool, has_prompt: bool, waited: Duration) -> FreshNext {
    match step {
        FreshStep::WaitingReady if ready => FreshNext::Rename,
        FreshStep::WaitingReady if waited >= READY_TIMEOUT => FreshNext::GiveUp,
        FreshStep::Renamed if renamed || waited >= RENAME_TIMEOUT => if has_prompt { FreshNext::Prompt } else { FreshNext::Finish },
        _ => FreshNext::Wait,
    }
}

impl Brain {
    /// Opens the dialog for the selected session, unless it is busy: then it says so.
    pub(super) fn open_fresh(&mut self) -> Task<Message> {
        let Some((key, busy)) = self.selected_session().map(|s| (s.key.clone(), s.phase() != Phase::Ended && !s.waits_for_input())) else { return Task::none() };
        if self.blocked_in_demo() {
            return Task::none();
        }
        if busy {
            self.set_status(t("Frisch beginnen geht, sobald die Session fertig ist und auf dich wartet.", "Starting fresh works once the session has finished and waits for you."));
            return Task::none();
        }
        self.fresh = FreshDialog { key: Some(key), prompt: text_editor::Content::new() };
        self.mode = Mode::Fresh;
        self.focused_terminal = None;
        operation::focus(FRESH_PROMPT)
    }

    /// The dialog's title: the session's name.
    pub fn fresh_title(&self) -> String {
        let name = self.fresh.key.as_ref().and_then(|k| self.model.board.get(k)).map(|s| s.display_name()).unwrap_or_default();
        tr!("„{name}“ frisch beginnen", "Start “{name}” fresh")
    }

    /// ⌘⏎ in the dialog: the new session starts; once it has, `Message::Cleared` ends the old one.
    pub(super) fn submit_fresh(&mut self) -> Task<Message> {
        let Some(key) = self.fresh.key.clone() else { return Task::none() };
        let Some(session) = self.model.board.get(&key) else { return Task::none() };
        if session.phase() != Phase::Ended && !session.waits_for_input() {
            self.mode = Mode::Normal;
            self.set_status(t("Die Session arbeitet inzwischen wieder – nichts geändert.", "The session is working again – nothing changed."));
            return unfocus();
        }
        let (Some(cwd), Some(account)) = (session.cwd.clone(), self.model.account(&key.account).cloned()) else {
            self.set_status(t("Zu dieser Session fehlt der Ordner.", "This session has no folder."));
            return Task::none();
        };
        let options = fresh_start(session.insight.model.as_deref(), &account.id);
        let prompt = Some(self.fresh.prompt.text().trim().to_string()).filter(|p| !p.is_empty());
        let name = session.display_name();
        self.mode = Mode::Normal;
        self.fresh = FreshDialog::default();
        self.set_status(t("Beginne frisch …", "Starting fresh …"));
        let id = account.id.clone();
        let started = off_thread(move || brain_core::agents::start(&account, std::path::Path::new(&cwd), &options));
        Task::batch([unfocus(), Task::perform(started, move |result| Message::Cleared(key.clone(), id.clone(), name.clone(), prompt.clone(), result))])
    }

    /// The new session runs: it takes the old one's project, the old one ends (without a way
    /// back: its conversation stays as an ended session).
    pub(super) fn finish_fresh(&mut self, old: SessionKey, account: String, name: String, prompt: Option<String>, result: Result<String, String>) -> Task<Message> {
        let Ok(id) = &result else { return self.update(Message::Started(account, result)) };
        self.pending_fresh = Some(PendingFresh { account: account.clone(), agent_id: id.clone(), name: name.clone(), prompt, key: None, step: FreshStep::WaitingReady, since: Instant::now() });
        if let Some(project) = self.prefs.project(&old).map(str::to_string) {
            self.pending_project = Some((account.clone(), id.clone(), project));
        }
        let task = self.update(Message::Started(account, result));
        if let Some(session) = self.model.board.get(&old) {
            if let Some(agent) = session.agent.clone().filter(|a| a.is_active()) {
                if let Some(account) = self.model.account(&old.account).cloned() {
                    if let Err(err) = brain_core::agents::stop(&account, &agent.id) {
                        self.set_status(tr!("Die alte Session lässt sich nicht stoppen: {err}", "The old session can't be stopped: {err}"));
                        return task;
                    }
                }
            } else if session.phase() != Phase::Ended {
                self.send_text(&old, "/exit");
            }
        }
        // The new one takes the old one's place: the old conversation leaves the tree (it stays
        // findable through the search, and comes back by itself if something happens in it).
        self.prefs.hide(&old, now_ms());
        self.prefs.save();
        self.model.relist_transcripts();
        self.set_status(tr!("„{name}“ startet …", "“{name}” is starting …"));
        task
    }

    /// On every tick: takes the new session through its steps (see [`fresh_next`]). Requests
    /// reach the mod in order, and a `/rename` isn't a turn, so it is done before the prompt,
    /// which queues for an idle session; Brain still waits to see the new name.
    pub(super) fn continue_fresh(&mut self) {
        let Some(mut fresh) = self.pending_fresh.take() else { return };
        if fresh.key.is_none() {
            fresh.key = self.model.board.keys().find(|k| k.account == fresh.account && self.model.board.get(k).and_then(|s| s.agent.as_ref()).is_some_and(|a| a.id == fresh.agent_id)).cloned();
        }
        let waited = fresh.since.elapsed();
        let (ready, renamed) = match fresh.key.as_ref().and_then(|k| self.model.board.get(k)) {
            Some(s) => (self.request_taker(&s.key).is_some() && s.waits_for_input(), s.display_name() == fresh.name),
            None => (false, false),
        };
        let name = fresh.name.clone();
        match fresh_next(fresh.step, ready, renamed, fresh.prompt.is_some(), waited) {
            FreshNext::Wait => {}
            FreshNext::Rename => {
                let Some(key) = fresh.key.clone() else { return };
                if !self.send_text(&key, &format!("/rename {name}")) {
                    return;
                }
                fresh.step = FreshStep::Renamed;
                fresh.since = Instant::now();
            }
            FreshNext::Prompt => {
                let (Some(key), Some(prompt)) = (fresh.key.clone(), fresh.prompt.clone()) else { return };
                if self.send_text(&key, &prompt) {
                    self.set_status(tr!("„{name}“ beginnt frisch und hat deinen Prompt bekommen.", "“{name}” starts fresh and got your prompt."));
                }
                return;
            }
            FreshNext::Finish => {
                self.set_status(tr!("„{name}“ beginnt frisch.", "“{name}” starts fresh."));
                return;
            }
            FreshNext::GiveUp => {
                self.set_status(tr!("„{name}“ wurde in 60 s nicht bereit – ohne Namen und Prompt gelassen.", "“{name}” wasn't ready within 60 s – left without name and prompt."));
                return;
            }
        }
        self.pending_fresh = Some(fresh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn new_session_starts_empty_with_the_model_only() {
        let start = fresh_start(Some("claude-opus-4"), "main");
        assert_eq!((start.model.as_deref(), start.name, start.prompt, start.resume), (Some("claude-opus-4"), None, None, None));
    }

    #[test]
    fn waits_until_ready_then_renames_then_feeds_the_prompt() {
        assert_eq!(fresh_next(FreshStep::WaitingReady, false, false, true, 30 * S), FreshNext::Wait);
        assert_eq!(fresh_next(FreshStep::WaitingReady, true, false, true, S), FreshNext::Rename);
        assert_eq!(fresh_next(FreshStep::Renamed, true, false, true, 2 * S), FreshNext::Wait);
        assert_eq!(fresh_next(FreshStep::Renamed, true, true, true, 2 * S), FreshNext::Prompt);
    }

    #[test]
    fn without_a_prompt_it_ends_after_the_rename() {
        assert_eq!(fresh_next(FreshStep::Renamed, true, true, false, S), FreshNext::Finish);
    }

    #[test]
    fn a_rename_that_doesnt_show_holds_the_prompt_only_for_a_while() {
        assert_eq!(fresh_next(FreshStep::Renamed, true, false, true, 11 * S), FreshNext::Prompt);
    }

    #[test]
    fn a_session_not_ready_after_60_seconds_is_given_up() {
        assert_eq!(fresh_next(FreshStep::WaitingReady, false, false, true, 60 * S), FreshNext::GiveUp);
        assert_eq!(fresh_next(FreshStep::WaitingReady, true, false, true, 60 * S), FreshNext::Rename);
    }
}
