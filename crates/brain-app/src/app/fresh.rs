//! "Clear …" (`/clear`, done cleanly): the dialog asks for an optional next prompt; ⌘⏎ starts a
//! new background session in the same folder and account, with the same name and model, and ends
//! the old one, which stays in the list as an ended session. Plugins and Brain's mod load anew.

use brain_core::agents::Start;
use brain_core::state::{Phase, SessionKey};
use iced::widget::{operation, text_editor};
use iced::Task;

use super::{off_thread, unfocus, Brain, Message, Mode};
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

/// The new session's options from the old one: its name and model, the typed prompt (none if blank).
pub fn fresh_start(name: Option<&str>, model: Option<&str>, prompt: &str) -> Start {
    Start {
        name: name.map(str::to_string),
        model: model.map(str::to_string),
        prompt: Some(prompt.trim().to_string()).filter(|p| !p.is_empty()),
        ..Default::default()
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
        let options = fresh_start(session.name.as_deref(), session.insight.model.as_deref(), &self.fresh.prompt.text());
        self.mode = Mode::Normal;
        self.fresh = FreshDialog::default();
        self.set_status(t("Beginne frisch …", "Starting fresh …"));
        let id = account.id.clone();
        let started = off_thread(move || brain_core::agents::start(&account, std::path::Path::new(&cwd), &options));
        Task::batch([unfocus(), Task::perform(started, move |result| Message::Cleared(key.clone(), id.clone(), result))])
    }

    /// The new session runs: it takes the old one's project, the old one ends (without a way
    /// back: its conversation stays as an ended session).
    pub(super) fn finish_fresh(&mut self, old: SessionKey, account: String, result: Result<String, String>) -> Task<Message> {
        let Ok(id) = &result else { return self.update(Message::Started(account, result)) };
        let name = self.model.board.get(&old).map(|s| s.display_name()).unwrap_or_default();
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
        self.model.relist_transcripts();
        self.set_status(tr!("„{name}“ beginnt frisch.", "“{name}” starts fresh."));
        task
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_takes_name_and_model_and_the_prompt_if_any() {
        let start = fresh_start(Some("api"), Some("claude-opus-4"), "  weiter mit Tests\n");
        assert_eq!((start.name.as_deref(), start.model.as_deref(), start.prompt.as_deref()), (Some("api"), Some("claude-opus-4"), Some("weiter mit Tests")));
        let blank = fresh_start(None, None, " \n ");
        assert!(blank.name.is_none() && blank.model.is_none() && blank.prompt.is_none() && blank.resume.is_none());
    }
}
