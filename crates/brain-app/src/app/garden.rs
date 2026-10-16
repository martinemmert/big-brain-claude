//! ⌘K "Gardener" (or the title bar's "✂ N broken"): the sessions Brain lists but can't steer
//! any more (`brain_core::garden`), and ending the chosen ones for good. Also what feeds it: a
//! session terminal that closes right after attaching makes its session unreachable, and what
//! the terminal showed last goes to the log.

use std::time::{Duration, Instant};

use brain_core::account::home_dir;
use brain_core::garden::{self, Finding};
use brain_core::state::SessionKey;
use iced::keyboard::key::Named;
use iced::Task;

use super::{off_thread, Brain, Message, Mode};
use crate::format::now_ms;
use crate::i18n::t;
use crate::tr;

/// A session terminal that closes sooner than this after starting didn't get through.
const ATTACH_FAILS_WITHIN: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct GardenDialog {
    pub findings: Vec<Finding>,
    /// Per finding: whether it gets ended.
    pub chosen: Vec<bool>,
    pub selected: usize,
    pub busy: bool,
}

impl Brain {
    /// The broken sessions right now.
    pub fn garden_findings(&self) -> Vec<Finding> {
        let sessions: Vec<_> = self.model.board.sorted();
        garden::findings(&sessions, now_ms(), &self.unreachable, &|id| self.model.accounts_holding(id))
    }

    pub(super) fn open_garden(&mut self) -> Task<Message> {
        let findings = self.garden_findings();
        self.garden = GardenDialog { chosen: findings.iter().map(|f| f.sure).collect(), findings, selected: 0, busy: false };
        self.mode = Mode::Garden;
        Task::none()
    }

    pub(super) fn garden_toggle(&mut self, index: usize) {
        if let Some(chosen) = self.garden.chosen.get_mut(index) {
            *chosen = !*chosen;
        }
        self.garden.selected = index;
    }

    pub(super) fn on_garden_key(&mut self, named: Option<Named>) -> Task<Message> {
        let count = self.garden.findings.len();
        match named {
            Some(Named::ArrowDown) if self.garden.selected + 1 < count => self.garden.selected += 1,
            Some(Named::ArrowUp) => self.garden.selected = self.garden.selected.saturating_sub(1),
            Some(Named::Space) => self.garden_toggle(self.garden.selected),
            Some(Named::Enter) => return self.garden_end(),
            _ => {}
        }
        Task::none()
    }

    /// ⏎: ends the chosen sessions, off the UI thread (each may take a few seconds).
    pub(super) fn garden_end(&mut self) -> Task<Message> {
        if self.garden.busy || self.blocked_in_demo() {
            return Task::none();
        }
        let targets: Vec<(SessionKey, brain_core::account::Account, u32, Option<String>)> = self
            .garden
            .findings
            .iter()
            .zip(&self.garden.chosen)
            .filter(|(_, chosen)| **chosen)
            .filter_map(|(f, _)| {
                let s = self.model.board.get(&f.key)?;
                let account = self.model.account(&f.key.account)?.clone();
                Some((f.key.clone(), account, s.pid, s.agent.as_ref().map(|a| a.id.clone())))
            })
            .collect();
        if targets.is_empty() {
            self.mode = Mode::Normal;
            return Task::none();
        }
        self.garden.busy = true;
        Task::perform(
            off_thread(move || targets.into_iter().map(|(key, account, pid, agent)| (key, garden::end(&account, pid, agent.as_deref()))).collect()),
            Message::GardenDone,
        )
    }

    pub(super) fn garden_done(&mut self, results: Vec<(SessionKey, Result<String, String>)>) {
        self.garden.busy = false;
        self.mode = Mode::Normal;
        let home = home_dir();
        let mut failed = 0;
        for (key, result) in &results {
            let name = self.model.board.get(key).map(|s| s.display_name()).unwrap_or_else(|| key.id.clone());
            match result {
                Ok(how) => brain_core::log::write(&home, "garden", &format!("{name} ({}): ended – {how}", key.account)),
                Err(why) => {
                    failed += 1;
                    brain_core::log::write(&home, "garden", &format!("{name} ({}): NOT ended – {why}", key.account));
                }
            }
            self.unreachable.remove(key);
            self.terminals.remove(key);
        }
        self.model.relist_transcripts();
        let ended = results.len() - failed;
        self.set_status(if failed == 0 {
            tr!("Gärtner: {ended} Sessions beendet. Fortsetzen geht weiterhin.", "Gardener: ended {ended} sessions. They can still be resumed.")
        } else {
            tr!("Gärtner: {ended} beendet, {failed} nicht – Details im Log (⌘K → Log öffnen).", "Gardener: {ended} ended, {failed} not – details in the log (⌘K → Open the log).")
        });
    }

    /// Right-click "End by force": the gardener's way for one session that won't end.
    pub(super) fn force_end_selected(&mut self) -> Task<Message> {
        let Some(key) = self.selected.clone() else { return Task::none() };
        let finding = Finding { key, reason: garden::Reason::Stuck { quiet_ms: 0 }, sure: true };
        self.garden = GardenDialog { findings: vec![finding], chosen: vec![true], selected: 0, busy: false };
        self.garden_end()
    }

    /// A session terminal closed: what it showed last goes to the log, and if it closed right
    /// after starting while its session lives on, Brain can't reach that session.
    pub(super) fn session_terminal_closed(&mut self, key: &SessionKey, lines: Vec<String>) {
        let lived = self.terminal_started.remove(key).map(|at| at.elapsed());
        let Some(session) = self.model.board.get(key) else { return };
        let name = session.display_name();
        let alive = session.alive && session.phase() != brain_core::state::Phase::Ended;
        let secs = lived.map_or(0, |d| d.as_secs());
        let last: Vec<String> = lines.into_iter().map(|l| l.trim_end().to_string()).filter(|l| !l.trim().is_empty()).rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
        let mut text = format!("{name} ({}): its terminal closed after {secs} s{}", key.account, if alive { ", the session still runs" } else { "" });
        for line in &last {
            text.push('\n');
            text.push_str(line);
        }
        brain_core::log::write(&home_dir(), "terminal", &text);
        if alive && lived.is_some_and(|d| d < ATTACH_FAILS_WITHIN) {
            self.unreachable.insert(key.clone(), last);
            self.set_status(tr!(
                "„{name}“ ist nicht erreichbar: Ihr Terminal ging sofort wieder zu. Der Gärtner (⌘K) kann sie beenden.",
                "“{name}” can't be reached: its terminal closed at once. The gardener (⌘K) can end it."
            ));
        } else {
            self.unreachable.remove(key);
        }
    }

    pub(super) fn note_terminal_started(&mut self, key: &SessionKey) {
        self.terminal_started.insert(key.clone(), Instant::now());
    }

    /// ⌘K "Open the log": today's file in the editor.
    pub(super) fn open_log(&mut self) {
        let path = brain_core::log::today(&home_dir());
        if !path.exists() {
            brain_core::log::write(&home_dir(), "brain", t("Log geöffnet.", "Log opened."));
        }
        super::open_in_editor(&path);
    }
}
