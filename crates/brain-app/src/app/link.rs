//! Brain Link inside Brain: turns the companion server on and off, publishes the session list
//! to it on every refresh, and carries out what the phone asked for the way Brain does for a
//! reply typed here.

use std::time::Duration;

use brain_core::state::{Phase, Session, SessionKey};
use brain_terminal::{self as terminal, Key};
use iced::widget::image;
use iced::Task;

use super::{movable, off_thread, Brain, Message, Mode};
use crate::companion::{self, Command, LimitView, Link, Pairing, Places, UsageView, View};
use crate::format::plain;
use crate::i18n::t;
use crate::tr;

/// How long a background session's terminal gets to attach before the phone's reply is typed.
const ATTACH_WAIT: Duration = Duration::from_millis(2500);
/// Pixels per QR module, and the quiet zone around the code (in modules).
const QR_SCALE: usize = 8;
const QR_QUIET: usize = 4;

impl Brain {
    /// Starts Brain Link if the user turned it on before.
    pub(super) fn resume_link(&mut self) {
        if companion::is_enabled() && !self.model.is_demo() {
            self.start_link();
        }
    }

    fn start_link(&mut self) -> bool {
        match Link::start(self.model.accounts.clone()) {
            Ok(link) => {
                self.link_qr = qr_image(&link.pairing.url);
                self.link = Some(link);
                true
            }
            Err(err) => {
                self.set_status(tr!("Brain Link startet nicht: {err}", "Brain Link didn't start: {err}"));
                false
            }
        }
    }

    /// ⌘K "Pair a phone": turns Brain Link on and shows the code to scan.
    pub(super) fn pair_phone(&mut self) {
        if self.blocked_in_demo() {
            return;
        }
        match self.link.as_mut() {
            // The Mac may be in another network since Brain Link started.
            Some(link) => {
                if link.refresh_addresses().is_ok() {
                    self.link_qr = qr_image(&link.pairing.url);
                }
            }
            None => {
                if !self.start_link() {
                    return;
                }
                companion::set_enabled(true);
            }
        }
        self.mode = Mode::Pairing;
    }

    pub(super) fn stop_link(&mut self) {
        self.link = None;
        self.link_qr = None;
        companion::set_enabled(false);
        self.mode = Mode::Normal;
        self.set_status(t("Brain Link ist aus – kein Handy erreicht Brain mehr.", "Brain Link is off – no phone reaches Brain any more."));
    }

    pub(super) fn forget_phones(&mut self) {
        let Some(link) = self.link.as_mut() else { return };
        match link.forget_phones() {
            Ok(()) => {
                self.link_qr = qr_image(&link.pairing.url);
                self.set_status(t("Alle Handys abgemeldet – zum Koppeln den neuen Code scannen.", "All phones signed out – scan the new code to pair."));
            }
            Err(err) => self.set_status(tr!("Abmelden fehlgeschlagen: {err}", "Signing out failed: {err}")),
        }
    }

    pub fn link_pairing(&self) -> Option<&Pairing> {
        self.link.as_ref().map(|l| &l.pairing)
    }

    /// On every refresh: the phone's view of the list, and what it asked for since.
    pub(super) fn serve_link(&mut self, now: i64) -> Task<Message> {
        if self.link.is_none() {
            return Task::none();
        }
        self.refresh_link_questions();
        let Some(link) = &self.link else { return Task::none() };
        link.publish(self.link_views(now));
        link.publish_more(self.link_usage(now), self.link_places());
        let commands = link.take_commands();
        let tasks: Vec<Task<Message>> = commands.into_iter().map(|command| self.run_link_command(command, true)).collect();
        Task::batch(tasks)
    }

    /// The open question of each session that waits for the user, read again only when its
    /// transcript grew.
    fn refresh_link_questions(&mut self) {
        let waiting: Vec<_> = self.model.board.keys().filter(|k| self.model.board.get(k).is_some_and(|s| s.phase() == Phase::NeedsYou)).cloned().collect();
        self.link_questions.retain(|k, _| waiting.contains(k));
        for key in waiting {
            let Some(path) = self.model.transcript_path(&key) else { continue };
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if self.link_questions.get(&key).is_some_and(|(seen, _)| *seen == len) {
                continue;
            }
            self.link_questions.insert(key, (len, brain_core::qa::open_question(&path)));
        }
    }

    /// Each account's plan limits, with when they run out at Brain's forecast.
    fn link_usage(&self, now: i64) -> Vec<UsageView> {
        let now_secs = now / 1000;
        self.model
            .accounts
            .iter()
            .filter_map(|account| {
                let snapshot = brain_core::usage::account_limits(&self.model.usage, &account.id)?;
                let history = self.model.usage_history.get(&account.id).map(Vec::as_slice).unwrap_or_default();
                let view = |limit: brain_core::usage::Limit, runs_out: Option<i64>| LimitView {
                    used_percentage: limit.used_percentage,
                    resets_at: limit.resets_at,
                    runs_out_at: runs_out.filter(|_| limit.used_percentage < 100.0),
                };
                Some(UsageView {
                    account: account.id.clone(),
                    five_hour: snapshot.five_hour.map(|l| view(l, l.runs_out_at(brain_core::usage::FIVE_HOURS, now_secs))),
                    seven_day: snapshot.seven_day.map(|l| view(l, l.runs_out_at_recent(brain_core::usage::SEVEN_DAYS, now_secs, history))),
                })
            })
            .collect()
    }

    /// The accounts and the folders sessions ran in lately, most recent first.
    fn link_places(&self) -> Places {
        let folders = self.folder_suggestions("").into_iter().take(20).collect();
        Places { accounts: self.model.accounts.iter().map(|a| a.id.clone()).collect(), folders }
    }

    /// The sessions as Brain lists them, section by section.
    fn link_views(&self, now: i64) -> Vec<View> {
        let groups = self.groups(now);
        let sections: [(&'static str, &Vec<&Session>); 6] = [
            ("pinned", &groups.pinned),
            ("attention", &groups.attention),
            ("working", &groups.working),
            ("resting", &groups.resting),
            ("resting", &groups.snoozed),
            ("ended", &groups.ended),
        ];
        sections.iter().flat_map(|(group, sessions)| sessions.iter().map(move |s| self.link_view(s, group))).collect()
    }

    fn link_view(&self, s: &Session, group: &'static str) -> View {
        let phase = s.phase();
        let question = if phase == Phase::NeedsYou { self.link_questions.get(&s.key).and_then(|(_, q)| q.clone()) } else { None };
        View {
            account: s.key.account.clone(),
            id: s.key.id.clone(),
            name: s.display_name(),
            phase: match phase {
                Phase::NeedsYou => "needs_you",
                Phase::YourTurn => "your_turn",
                Phase::Working => "working",
                Phase::Background => "background",
                Phase::Ended => "ended",
            },
            group,
            headline: s.headline().map(|h| plain(&h)),
            since_ms: s.phase_since_ms(),
            folder: s.cwd.as_deref().and_then(|c| c.rsplit('/').find(|p| !p.is_empty())).map(str::to_string),
            permission_open: s.awaiting_permission() && question.is_none(),
            permission_since_ms: (s.awaiting_permission() && question.is_none()).then(|| s.phase_since_ms()),
            question,
            can_reply: phase != Phase::Ended && (self.can_send(&s.key) || s.agent.as_ref().is_some_and(|a| a.is_active())),
            can_move: self.model.accounts.len() > 1 && movable(s) && s.session_id.is_some() && s.cwd.is_some(),
        }
    }

    /// A reply or a permission answer from the phone. A background session without a terminal
    /// in Brain gets one first (`claude attach`), and the command is tried again once it is up.
    pub(super) fn run_link_command(&mut self, command: Command, first_try: bool) -> Task<Message> {
        if self.blocked_in_demo() {
            return Task::none();
        }
        let key = match &command {
            Command::Reply { key, .. } | Command::Permission { key, .. } | Command::Answer { key, .. } => key.clone(),
            Command::Start { account, folder, prompt } => return self.link_start(account, folder, prompt.clone()),
            Command::Move { key, account } => return self.link_move(key.clone(), account),
        };
        let Some(session) = self.model.board.get(&key) else { return Task::none() };
        let name = session.display_name();
        let permission_open = session.awaiting_permission();
        let permission_since = permission_open.then(|| session.phase_since_ms());
        let pid = session.pid;
        let in_background = session.agent.as_ref().is_some_and(|a| a.is_active());
        match command {
            Command::Reply { text, .. } => {
                if permission_open {
                    self.set_status(tr!(
                        "Vom Handy: „{name}“ wartet auf eine Freigabe – Antwort nicht gesendet.",
                        "From the phone: “{name}” waits for a permission – reply not sent."
                    ));
                    return Task::none();
                }
                if self.can_send(&key) {
                    self.set_status(tr!("Antwort vom Handy an „{name}“ gesendet.", "Reply from the phone sent to “{name}”."));
                    return self.send_text(&key, &text);
                }
                if in_background && first_try && self.spawn_terminal(&key) {
                    return later(Command::Reply { key, text });
                }
                self.set_status(tr!("Vom Handy: „{name}“ nimmt gerade keine Eingabe an.", "From the phone: “{name}” doesn't take input right now."));
                Task::none()
            }
            // Handled before a session was looked up.
            Command::Start { .. } | Command::Move { .. } => Task::none(),
            Command::Answer { call, picks, .. } => {
                // Only the question the phone showed, read from the transcript right now.
                let open = self.model.transcript_path(&key).and_then(|p| brain_core::qa::open_question(&p));
                let Some(keys) = open.filter(|q| q.call == call).and_then(|q| brain_core::qa::answer_keys(&q, &picks)) else {
                    self.set_status(tr!(
                        "Vom Handy: „{name}“ fragt inzwischen etwas anderes – Antwort nicht gesendet.",
                        "From the phone: “{name}” asks something else by now – answer not sent."
                    ));
                    return Task::none();
                };
                if self.terminals.contains_key(&key) {
                    self.set_status(tr!("Antwort vom Handy an „{name}“ gegeben.", "Answered “{name}” from the phone."));
                    return Task::done(Message::TerminalKeys(key, keys.into_bytes()));
                }
                if in_background && first_try && self.spawn_terminal(&key) {
                    return later(Command::Answer { key, call, picks });
                }
                // A session in another terminal app: its option keys can't be sent there.
                self.set_status(tr!(
                    "Vom Handy: „{name}“ läuft nicht in Brain – bitte dort antworten.",
                    "From the phone: “{name}” doesn't run in Brain – answer it there."
                ));
                Task::none()
            }
            Command::Permission { allow, since_ms, .. } => {
                // A question isn't a permission: Allow would pick its first option.
                let asking = self.link_questions.get(&key).is_some_and(|(_, q)| q.is_some());
                // Only the dialog the phone showed: one that opened since was never seen there.
                if asking || permission_since != Some(since_ms) {
                    if permission_open {
                        self.set_status(tr!(
                            "Vom Handy: „{name}“ fragt inzwischen etwas anderes – Freigabe nicht gesendet.",
                            "From the phone: “{name}” asks something else by now – permission not sent."
                        ));
                    }
                    return Task::none();
                }
                let done = if allow { tr!("Freigabe vom Handy für „{name}“ erteilt.", "Permission for “{name}” allowed from the phone.") } else { tr!("Freigabe vom Handy für „{name}“ abgelehnt.", "Permission for “{name}” denied from the phone.") };
                if self.terminals.contains_key(&key) {
                    self.set_status(done);
                    // Return picks the dialog's default "Yes", Esc declines.
                    let bytes = if allow { b"\r".to_vec() } else { b"\x1b".to_vec() };
                    return Task::done(Message::TerminalKeys(key, bytes));
                }
                if !in_background {
                    let outcome = terminal::send_key(pid, if allow { Key::Return } else { Key::Escape });
                    self.report(outcome, Some(done));
                    return Task::none();
                }
                if first_try && self.spawn_terminal(&key) {
                    return later(Command::Permission { key, allow, since_ms });
                }
                Task::none()
            }
        }
    }
}

impl Brain {
    /// A session started from the phone runs in the background; Brain lists it without taking
    /// the selection on the Mac.
    fn link_start(&mut self, account: &str, folder: &str, prompt: Option<String>) -> Task<Message> {
        let Some(account) = self.model.account(account).cloned() else { return Task::none() };
        let folder_name = folder.rsplit('/').find(|p| !p.is_empty()).unwrap_or(folder).to_string();
        self.set_status(tr!("Vom Handy: neue Session in {folder_name} …", "From the phone: new session in {folder_name} …"));
        let folder = folder.to_string();
        let started = off_thread(move || brain_core::agents::start(&account, std::path::Path::new(&folder), None, None, prompt.as_deref()));
        Task::perform(started, Message::LinkStarted)
    }

    /// A move from the phone: as `A` in Brain, the phone's choice of account being the
    /// confirmation.
    fn link_move(&mut self, key: SessionKey, account: &str) -> Task<Message> {
        let Some(index) = self.model.accounts.iter().position(|a| a.id == account) else { return Task::none() };
        self.move_session(key, index, true)
    }
}

/// The command again, once a freshly started terminal had time to attach.
fn later(command: Command) -> Task<Message> {
    Task::perform(off_thread(|| std::thread::sleep(ATTACH_WAIT)), move |_| Message::LinkRetry(command.clone()))
}

/// The pairing URL as a black-on-white image with its quiet zone, ready to scan.
fn qr_image(url: &str) -> Option<image::Handle> {
    let (width, modules) = companion::qr_modules(url)?;
    let side = (width + 2 * QR_QUIET) * QR_SCALE;
    let mut pixels = vec![255u8; side * side * 4];
    for (index, _) in modules.iter().enumerate().filter(|(_, dark)| **dark) {
        let (mx, my) = (index % width + QR_QUIET, index / width + QR_QUIET);
        for y in my * QR_SCALE..(my + 1) * QR_SCALE {
            for x in mx * QR_SCALE..(mx + 1) * QR_SCALE {
                let at = (y * side + x) * 4;
                pixels[at..at + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
    }
    Some(image::Handle::from_rgba(side as u32, side as u32, pixels))
}
