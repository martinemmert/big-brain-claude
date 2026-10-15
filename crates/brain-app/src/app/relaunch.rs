//! "Go on …" (`A`, `F`, `I`, "Restart with …"): where and how the selected session goes on, in
//! one dialog: the account (its own or another), whether the original keeps running, and flags
//! for the new start. What ⏎ does follows from the choice:
//! - the original keeps running: a fork into the account, in the background in Brain;
//! - another account: the session moves there (transcript copied, resumed, original closed);
//! - its own account with flags: it ends and resumes with them where it ran (Claude takes flags
//!   only at start: a background session in the background, one in Brain's terminal there, one
//!   in iTerm in a new iTerm tab);
//! - its own account without flags: a session in iTerm moves into Brain.

use brain_core::state::{Phase, SessionKey};
use brain_terminal::{self as terminal, Outcome};
use iced::widget::operation;
use iced::Task;

use super::{off_thread, Brain, DetailTab, Message, Mode};
use crate::format::shell_quote;
use crate::i18n::t;
use crate::tr;

/// The flags field of the dialog.
pub const RELAUNCH_FLAGS: &str = "relaunch-flags";

/// Flags that decide which conversation runs or how; Brain sets those itself.
const OWN_FLAGS: [&str; 11] = ["--resume", "-r", "--continue", "-c", "--print", "-p", "--fork-session", "--session-id", "--bg", "--background", "--from-pr"];

/// What the dialog opens with, by the key or menu entry that opened it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// `A`: another account, the original ends.
    Move,
    /// `F`: its own account, the original keeps running.
    Fork,
    /// "Restart with …": its own account, flags.
    Restart,
    /// `I`: its own account, into Brain.
    TakeOver,
}

#[derive(Default)]
pub struct RelaunchDialog {
    /// The account it goes on in, as an index among the accounts.
    pub target: usize,
    /// The original keeps running (a fork).
    pub keep: bool,
    pub flags: String,
    pub error: Option<String>,
}

/// What ⏎ would do with the dialog's choice.
#[derive(Debug, Clone, PartialEq)]
pub enum Relaunch {
    Fork(Vec<String>),
    Move(Vec<String>),
    Restart(Vec<String>),
    TakeOver,
}

/// A session asked to end (`/exit`), to go on with these flags once it has.
pub(super) struct PendingRestart {
    key: SessionKey,
    flags: Vec<String>,
    in_brain: bool,
}

/// The flags as words (none is fine), or why not: Brain's own flags are refused.
pub fn parse_flags(text: &str) -> Result<Vec<String>, String> {
    let words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    if let Some(own) = words.iter().find(|w| OWN_FLAGS.contains(&w.split('=').next().unwrap_or(w))) {
        return Err(tr!("{own} setzt Brain selbst.", "Brain sets {own} itself."));
    }
    Ok(words)
}

impl Brain {
    pub(super) fn open_relaunch(&mut self, preset: Preset) -> Task<Message> {
        let Some(account) = self.selected_session().map(|s| s.key.account.clone()) else { return Task::none() };
        if self.blocked_in_demo() {
            return Task::none();
        }
        let own = self.account_index(&account);
        let target = match preset {
            Preset::Move => self.move_targets().first().copied().unwrap_or(own),
            _ => own,
        };
        self.armed = None;
        self.relaunch = RelaunchDialog { target, keep: preset == Preset::Fork, flags: String::new(), error: None };
        self.mode = Mode::Relaunch;
        if preset == Preset::Restart {
            return operation::focus(RELAUNCH_FLAGS);
        }
        Task::none()
    }

    /// What the dialog's choice does, or why it does nothing.
    pub fn relaunch_choice(&self) -> Result<Relaunch, String> {
        let Some(session) = self.selected_session() else { return Err(String::new()) };
        let flags = parse_flags(&self.relaunch.flags)?;
        let own = self.account_index(&session.key.account);
        if self.relaunch.keep {
            return Ok(Relaunch::Fork(flags));
        }
        if self.relaunch.target != own {
            return Ok(Relaunch::Move(flags));
        }
        if !flags.is_empty() {
            return Ok(Relaunch::Restart(flags));
        }
        if session.agent.is_some() || self.terminals.contains_key(&session.key) {
            return Err(t(
                "Sie läuft schon hier in Brain: Wähle ein anderes Konto, gib Flags an oder lass das Original weiterlaufen.",
                "It already runs here in Brain: pick another account, give flags, or keep the original running.",
            )
            .into());
        }
        Ok(Relaunch::TakeOver)
    }

    /// ⏎ in the dialog.
    pub(super) fn submit_relaunch(&mut self) -> Task<Message> {
        let choice = match self.relaunch_choice() {
            Ok(choice) => choice,
            Err(err) => {
                self.relaunch.error = Some(err);
                return Task::none();
            }
        };
        self.mode = Mode::Normal;
        let target = self.relaunch.target;
        match choice {
            Relaunch::Fork(flags) => self.fork_selected(target, flags),
            Relaunch::Move(flags) => self.move_selected(target, flags),
            Relaunch::Restart(flags) => self.restart_with(flags),
            Relaunch::TakeOver => {
                self.take_over();
                Task::none()
            }
        }
    }

    /// End the session if it runs, and resume it with the flags where it ran.
    fn restart_with(&mut self, flags: Vec<String>) -> Task<Message> {
        let Some(session) = self.selected_session() else { return Task::none() };
        let key = session.key.clone();
        let (Some(session_id), Some(cwd), Some(account)) = (session.session_id.clone(), session.cwd.clone(), self.model.account(&key.account).cloned()) else {
            self.set_status(t("Zu dieser Session fehlt die ID oder der Ordner.", "This session has no id or folder."));
            return Task::none();
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
            return Task::perform(restarted, move |result| Message::Started(id.clone(), result));
        }
        if ended {
            return self.resume_in_brain(key, flags);
        }
        if !accepts_input {
            self.set_status(t("Neu starten geht, sobald die Session fertig ist und auf dich wartet.", "Restarting works once the session has finished and waits for you."));
            return Task::none();
        }
        let in_brain = self.terminals.contains_key(&key);
        self.pending_restart = Some(PendingRestart { key, flags, in_brain });
        self.type_into_selected("/exit", t("Beende die Session für den Neustart …", "Ending the session for the restart …").into())
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
        let Some(PendingRestart { key, flags, in_brain }) = self.pending_restart.take() else { return Task::none() };
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
        assert!(parse_flags("").unwrap().is_empty());
        assert!(parse_flags("--chrome --resume x").is_err());
        assert!(parse_flags("--session-id=abc").is_err());
        assert!(parse_flags("-p hi").is_err());
    }
}
