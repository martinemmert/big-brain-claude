//! "Restart with …": the selected session goes on with extra flags (`--chrome`, `--model opus`).
//! Claude takes flags only at start, so the session ends and its conversation is resumed with
//! them, where it ran: a background session in the background, a session in Brain's terminal
//! there, one in iTerm in a new iTerm tab.

use brain_core::state::{Phase, SessionKey};
use brain_terminal::{self as terminal, Outcome};
use iced::widget::operation;
use iced::Task;

use super::{off_thread, Brain, DetailTab, Message, Mode};
use crate::format::shell_quote;
use crate::i18n::t;
use crate::tr;

/// The flags field of the dialog.
pub const RESTART_FLAGS: &str = "restart-flags";

/// Flags that decide which conversation runs or how; Brain sets those itself.
const OWN_FLAGS: [&str; 11] = ["--resume", "-r", "--continue", "-c", "--print", "-p", "--fork-session", "--session-id", "--bg", "--background", "--from-pr"];

#[derive(Default)]
pub struct RestartDialog {
    pub flags: String,
    pub error: Option<String>,
}

/// A session asked to end (`/exit`), to go on with these flags once it has.
pub(super) struct PendingRestart {
    key: SessionKey,
    flags: Vec<String>,
    in_brain: bool,
    /// Asked for from the phone: it goes on in the background.
    from_phone: bool,
    /// Team on or off: it goes on as an agent team in tmux, or without one in the background.
    team: Option<bool>,
}

/// The flags as words, or why not: Brain's own flags are refused.
pub fn parse_flags(text: &str) -> Result<Vec<String>, String> {
    let words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Err(t("Keine Flags angegeben.", "No flags given.").into());
    }
    if let Some(own) = words.iter().find(|w| OWN_FLAGS.contains(&w.split('=').next().unwrap_or(w))) {
        return Err(tr!("{own} setzt Brain selbst.", "Brain sets {own} itself."));
    }
    Ok(words)
}

impl Brain {
    pub(super) fn open_restart_dialog(&mut self) -> Task<Message> {
        if self.selected_session().is_none() || self.blocked_in_demo() {
            return Task::none();
        }
        self.restart.error = None;
        self.mode = Mode::RestartFlags;
        Task::batch([operation::focus(RESTART_FLAGS), operation::select_all(RESTART_FLAGS)])
    }

    /// ⏎ in the dialog: end the session if it runs, and resume it with the flags.
    pub(super) fn submit_restart(&mut self) -> Task<Message> {
        let flags = match parse_flags(&self.restart.flags) {
            Ok(flags) => flags,
            Err(err) => {
                self.restart.error = Some(err);
                return Task::none();
            }
        };
        let Some(key) = self.selected.clone() else { return Task::none() };
        match self.restart_session(key, flags, false) {
            Ok(task) => {
                self.mode = Mode::Normal;
                task
            }
            Err(err) => {
                self.restart.error = Some(err);
                Task::none()
            }
        }
    }

    /// End the session if it runs, and resume it with the flags where it ran. From the phone
    /// (`from_phone`) it goes on in the background, so Brain on the Mac keeps its selection.
    pub(super) fn restart_session(&mut self, key: SessionKey, flags: Vec<String>, from_phone: bool) -> Result<Task<Message>, String> {
        let Some(session) = self.model.board.get(&key) else { return Err(t("Die Session gibt es nicht mehr.", "The session is gone.").into()) };
        let (Some(session_id), Some(cwd), Some(account)) = (session.session_id.clone(), session.cwd.clone(), self.model.account(&key.account).cloned()) else {
            return Err(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder.").into());
        };
        let ended = session.phase() == Phase::Ended;
        let agent = session.agent.clone();
        let name = session.name.clone();
        let accepts_input = session.accepts_input();

        // A background session (running or stopped) goes on in the background.
        if let Some(agent) = agent {
            let shown = flags.join(" ");
            self.set_status(tr!("Starte die Session neu mit {shown} …", "Restarting the session with {shown} …"));
            let id = account.id.clone();
            let restarted = off_thread(move || {
                if agent.is_active() {
                    brain_core::agents::stop(&account, &agent.id)?;
                }
                brain_core::agents::resume_with(&account, std::path::Path::new(&cwd), &session_id, name.as_deref(), &flags)
            });
            return Ok(Task::perform(restarted, move |result| Message::Started(id.clone(), result)));
        }
        if ended {
            if from_phone {
                return Ok(self.resume_in_background(&key, flags));
            }
            return Ok(self.resume_in_brain(key, flags));
        }
        if !accepts_input {
            self.set_status(t("Neu starten geht, sobald die Session fertig ist und auf dich wartet.", "Restarting works once the session has finished and waits for you."));
            return Ok(Task::none());
        }
        let in_brain = self.terminals.contains_key(&key);
        self.pending_restart = Some(PendingRestart { key: key.clone(), flags, in_brain, from_phone, team: None });
        Ok(self.type_into(&key, "/exit", t("Beende die Session für den Neustart …", "Ending the session for the restart …").into()))
    }

    /// Whether the session runs as an agent team Brain started (in tmux).
    pub fn in_team(&self, key: &SessionKey) -> bool {
        self.model.board.get(key).is_some_and(|s| s.phase() != Phase::Ended && self.team_panes.contains_key(&s.pid))
    }

    /// Team on: the conversation goes on as an agent team in tmux. Team off: it goes on without
    /// one in the background. Claude takes both only at start, so a running session ends first
    /// (a background one is stopped, any other gets `/exit` once it waits for you).
    pub(super) fn switch_team(&mut self, key: SessionKey, on: bool) -> Result<Task<Message>, String> {
        let Some(session) = self.model.board.get(&key) else { return Err(t("Die Session gibt es nicht mehr.", "The session is gone.").into()) };
        let (Some(session_id), Some(cwd), Some(account)) = (session.session_id.clone(), session.cwd.clone(), self.model.account(&key.account).cloned()) else {
            return Err(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder.").into());
        };
        if on == self.in_team(&key) {
            return Ok(Task::none());
        }
        let ended = session.phase() == Phase::Ended;
        let agent = session.agent.clone().filter(|a| a.is_active());
        let accepts_input = session.accepts_input();
        let name = session.name.clone();
        if on && (ended || agent.is_some()) {
            self.set_status(t("Setze die Session als Team fort …", "Going on as a team …"));
            let options = brain_core::agents::Start { name, team: Some(crate::config::team_start()), resume: Some(session_id), ..Default::default() };
            let started = off_thread(move || {
                if let Some(agent) = agent {
                    brain_core::agents::stop(&account, &agent.id)?;
                }
                brain_core::agents::start_team(&account, std::path::Path::new(&cwd), &options)
            });
            return Ok(Task::perform(started, Message::TeamStarted));
        }
        if !accepts_input {
            self.set_status(t("Umschalten geht, sobald die Session fertig ist und auf dich wartet.", "Switching works once the session has finished and waits for you."));
            return Ok(Task::none());
        }
        self.pending_restart = Some(PendingRestart { key: key.clone(), flags: Vec::new(), in_brain: false, from_phone: true, team: Some(on) });
        let note = if on { t("Beende die Session, sie geht als Team weiter …", "Ending the session, it goes on as a team …") } else { t("Beende das Team, die Session geht ohne weiter …", "Ending the team, the session goes on without one …") };
        Ok(self.type_into(&key, "/exit", note.into()))
    }

    /// An ended session goes on as an agent team in tmux.
    fn resume_as_team(&mut self, key: &SessionKey) -> Task<Message> {
        let Some(session) = self.model.board.get(key) else { return Task::none() };
        let (Some(session_id), Some(cwd), Some(account)) = (session.session_id.clone(), session.cwd.clone(), self.model.account(&key.account).cloned()) else { return Task::none() };
        let options = brain_core::agents::Start { name: session.name.clone(), team: Some(crate::config::team_start()), resume: Some(session_id), ..Default::default() };
        self.set_status(t("Setze die Session als Team fort …", "Going on as a team …"));
        let started = off_thread(move || brain_core::agents::start_team(&account, std::path::Path::new(&cwd), &options));
        Task::perform(started, Message::TeamStarted)
    }

    /// An ended session goes on in the background (`claude --bg --resume`), with flags if any.
    pub(super) fn resume_in_background(&mut self, key: &SessionKey, flags: Vec<String>) -> Task<Message> {
        let Some(session) = self.model.board.get(key) else { return Task::none() };
        let (Some(session_id), Some(cwd), Some(account)) = (session.session_id.clone(), session.cwd.clone(), self.model.account(&key.account).cloned()) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder."));
            return Task::none();
        };
        let name = session.name.clone();
        self.set_status(t("Setze die Session im Hintergrund fort …", "Resuming the session in the background …"));
        let id = account.id.clone();
        let resumed = off_thread(move || brain_core::agents::resume_with(&account, std::path::Path::new(&cwd), &session_id, name.as_deref(), &flags));
        Task::perform(resumed, move |result| Message::Started(id.clone(), result))
    }

    /// Once the session asked to end has ended: resume it with its flags.
    pub(super) fn continue_restart(&mut self) -> Task<Message> {
        let Some(pending) = &self.pending_restart else { return Task::none() };
        let Some(session) = self.model.board.get(&pending.key) else {
            self.pending_restart = None;
            return Task::none();
        };
        if session.phase() != Phase::Ended {
            return Task::none();
        }
        let Some(PendingRestart { key, flags, in_brain, from_phone, team }) = self.pending_restart.take() else { return Task::none() };
        if team == Some(true) {
            return self.resume_as_team(&key);
        }
        if from_phone {
            return self.resume_in_background(&key, flags);
        }
        if in_brain {
            return self.resume_in_brain(key, flags);
        }
        let (Some(session_id), Some(cwd)) = (session.session_id.clone(), session.cwd.clone()) else { return Task::none() };
        let config_dir = self.model.account(&key.account).filter(|a| a.id != "main").map(|a| a.config_dir.display().to_string());
        let quoted: Vec<String> = flags.iter().map(|f| shell_quote(f)).collect();
        let command = format!("claude --resume {} {}", shell_quote(&session_id), quoted.join(" "));
        let outcome = terminal::open_new(&cwd, config_dir.as_deref(), &command);
        let shown = flags.join(" ");
        self.report(outcome.clone(), matches!(outcome, Outcome::Done).then(|| tr!("In iTerm neu gestartet mit {shown}.", "Restarted in iTerm with {shown}.")));
        Task::none()
    }

    /// The ended session goes on in Brain's terminal with the flags.
    fn resume_in_brain(&mut self, key: SessionKey, flags: Vec<String>) -> Task<Message> {
        // The old terminal (its process has ended) makes way for the new one.
        self.terminals.remove(&key);
        self.restart_flags.insert(key.clone(), flags.clone());
        self.selected = Some(key.clone());
        self.tab = DetailTab::Terminal;
        let started = self.spawn_terminal(&key);
        self.restart_flags.remove(&key);
        if !started {
            return Task::none();
        }
        let shown = flags.join(" ");
        self.set_status(tr!("Neu gestartet mit {shown}.", "Restarted with {shown}."));
        self.focus_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_are_words_and_brains_own_are_refused() {
        assert_eq!(parse_flags(" --chrome  --model opus ").unwrap(), vec!["--chrome", "--model", "opus"]);
        assert!(parse_flags("").is_err());
        assert!(parse_flags("--chrome --resume x").is_err());
        assert!(parse_flags("--session-id=abc").is_err());
        assert!(parse_flags("-p hi").is_err());
    }
}
