//! Brain Link inside Brain: turns the companion server on and off, publishes the session list
//! to it on every refresh, and carries out what the phone asked for the way Brain does for a
//! reply typed here.

use std::time::Duration;

use brain_core::state::{Phase, Session, SessionKey};
use brain_terminal::{self as terminal, Key};
use iced::widget::image;
use iced::Task;

use super::{off_thread, Brain, Message, Mode};
use crate::companion::{self, Command, LimitView, Link, Pairing, Picked, Places, UsageView, View};
use brain_core::qa::OpenQuestion;
use crate::format::plain;
use crate::i18n::t;
use crate::tr;

/// How long a background session's terminal gets to attach before the phone's reply is typed.
const ATTACH_WAIT: Duration = Duration::from_millis(2500);
/// How long Esc takes to close a question dialog before the answers are typed.
const DIALOG_CLOSE: Duration = Duration::from_millis(700);
/// Pixels per QR module, and the quiet zone around the code (in modules).
const QR_SCALE: usize = 8;
const QR_QUIET: usize = 4;

/// When a session's open question was read: its transcript's length and the call of the
/// question its mod reported.
pub(super) type QuestionReadAt = (u64, Option<String>);

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
        link.publish_folders(self.model.board.keys().filter_map(|k| Some((k.id.clone(), std::path::PathBuf::from(self.model.board.get(k)?.cwd.clone()?)))).collect());
        let commands = link.take_commands();
        let tasks: Vec<Task<Message>> = commands.into_iter().map(|command| self.run_link_command(command, true)).collect();
        Task::batch(tasks)
    }

    /// The open question of each session that waits for the user, read again only when its
    /// transcript grew or its mod reported another one.
    fn refresh_link_questions(&mut self) {
        let waiting: Vec<_> = self.model.board.keys().filter(|k| self.model.board.get(k).is_some_and(|s| s.phase() == Phase::NeedsYou)).cloned().collect();
        self.link_questions.retain(|k, _| waiting.contains(k));
        for key in waiting {
            let Some(path) = self.model.transcript_path(&key) else { continue };
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let asked = self.model.board.get(&key).and_then(|s| s.asked()).map(|q| q.call.clone());
            let seen = (len, asked);
            if self.link_questions.get(&key).is_some_and(|(at, _)| *at == seen) {
                continue;
            }
            let question = self.open_question(&key);
            self.link_questions.insert(key, (seen, question));
        }
    }

    /// The AskUserQuestion a session waits on: the one its mod reported while the transcript
    /// has no answer to it (Claude Code writes the call there only with its answer), else an
    /// unanswered one in the transcript.
    fn open_question(&self, key: &SessionKey) -> Option<brain_core::qa::OpenQuestion> {
        let path = self.model.transcript_path(key)?;
        let reported = self.model.board.get(key).and_then(|s| s.asked()).filter(|q| !brain_core::qa::answered(&path, &q.call));
        reported.cloned().or_else(|| brain_core::qa::open_question(&path))
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
        let mut styles: Vec<brain_core::styles::Style> = Vec::new();
        for account in &self.model.accounts {
            for style in brain_core::styles::available(&account.config_dir, None) {
                if !styles.iter().any(|s| s.name == style.name) {
                    styles.push(style);
                }
            }
        }
        Places { accounts: self.model.accounts.iter().map(|a| a.id.clone()).collect(), folders, styles }
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
            team_questions: self.teams.get(&s.key).map(|t| t.open_questions().cloned().collect()).unwrap_or_default(),
            team_active: self.teams.get(&s.key).filter(|t| !t.members.is_empty()).map(|t| t.active().count()),
            in_team: self.in_team(&s.key),
        }
    }

    /// A reply or a permission answer from the phone. A background session without a terminal
    /// in Brain gets one first (`claude attach`), and the command is tried again once it is up.
    pub(super) fn run_link_command(&mut self, command: Command, first_try: bool) -> Task<Message> {
        if self.blocked_in_demo() {
            return Task::none();
        }
        let key = match &command {
            Command::Reply { key, .. }
            | Command::Permission { key, .. }
            | Command::Answer { key, .. }
            | Command::Interrupt { key }
            | Command::Resume { key }
            | Command::Restart { key, .. }
            | Command::End { key }
            | Command::Team { key, .. } => key.clone(),
            Command::Start { account, folder, prompt, style, team } => return self.link_start(account, folder, prompt.clone(), style.clone(), *team),
        };
        let Some(session) = self.model.board.get(&key) else { return Task::none() };
        let name = session.display_name();
        let phase = session.phase();
        let permission_open = session.awaiting_permission();
        let permission_since = permission_open.then(|| session.phase_since_ms());
        let pid = session.pid;
        // A team in tmux too: Brain's terminal joins it first (tmux attach), like `claude attach`.
        let in_background = session.agent.as_ref().is_some_and(|a| a.is_active()) || self.in_team(&key);
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
            // Started before a session was looked up (it has none yet).
            Command::Start { .. } => Task::none(),
            Command::Answer { call, answers, .. } => {
                // Only the question the phone showed, if it is still open right now.
                let open = self.open_question(&key).filter(|q| q.call == call);
                let Some(question) = open else {
                    self.set_status(tr!(
                        "Vom Handy: „{name}“ fragt inzwischen etwas anderes – Antwort nicht gesendet.",
                        "From the phone: “{name}” asks something else by now – answer not sent."
                    ));
                    return Task::none();
                };
                // One option per question and nothing written: the dialog's own keys. Several
                // picks or own words: Esc closes the dialog, and the answers go as a reply.
                let single: Option<Vec<usize>> = answers.iter().map(|a| (a.picks.len() == 1 && a.other.is_none()).then(|| a.picks[0])).collect();
                let keys = single.and_then(|picks| brain_core::qa::answer_keys(&question, &picks));
                let typed = if keys.is_none() { answer_text(&question, &answers) } else { None };
                if keys.is_none() && typed.is_none() {
                    self.set_status(tr!("Vom Handy: die Antwort passt nicht zu „{name}“s Frage.", "From the phone: the answer doesn't fit “{name}”'s question."));
                    return Task::none();
                }
                let answered = tr!("Antwort vom Handy an „{name}“ gegeben.", "Answered “{name}” from the phone.");
                if self.terminals.contains_key(&key) {
                    self.set_status(answered);
                    return match (keys, typed) {
                        (Some(keys), _) => Task::done(Message::TerminalKeys(key, keys.into_bytes())),
                        (None, Some(text)) => Task::batch([Task::done(Message::TerminalKeys(key.clone(), b"\x1b".to_vec())), type_later(key, text)]),
                        (None, None) => Task::none(),
                    };
                }
                if in_background && first_try && self.spawn_terminal(&key) {
                    return later(Command::Answer { key, call, answers });
                }
                // In another terminal app Esc and typed text work, the dialog's option keys don't.
                if let (Some(text), false) = (typed, in_background) {
                    let outcome = terminal::send_key(pid, Key::Escape);
                    let sent = matches!(outcome, terminal::Outcome::Done);
                    self.report(outcome, None);
                    return if sent { type_later(key, text) } else { Task::none() };
                }
                self.set_status(tr!(
                    "Vom Handy: „{name}“ läuft nicht in Brain – bitte dort antworten.",
                    "From the phone: “{name}” doesn't run in Brain – answer it there."
                ));
                Task::none()
            }
            Command::Interrupt { .. } => {
                // Esc on a dialog would deny it: only a session that works is interrupted.
                if phase != Phase::Working {
                    self.set_status(tr!("Vom Handy: „{name}“ arbeitet gerade nicht – nichts unterbrochen.", "From the phone: “{name}” isn't working – nothing interrupted."));
                    return Task::none();
                }
                let done = tr!("„{name}“ vom Handy unterbrochen.", "Interrupted “{name}” from the phone.");
                if self.terminals.contains_key(&key) {
                    self.set_status(done);
                    return Task::done(Message::TerminalKeys(key, b"\x1b".to_vec()));
                }
                if in_background {
                    if first_try && self.spawn_terminal(&key) {
                        return later(Command::Interrupt { key });
                    }
                    return Task::none();
                }
                let outcome = terminal::send_key(pid, Key::Escape);
                self.report(outcome, Some(done));
                Task::none()
            }
            Command::Resume { .. } => {
                if phase != Phase::Ended {
                    return Task::none();
                }
                self.resume_in_background(&key, Vec::new())
            }
            Command::Team { on, .. } => match self.switch_team(key, on) {
                Ok(task) => task,
                Err(err) => {
                    self.set_status(tr!("Vom Handy: Team umschalten geht nicht – {err}", "From the phone: can't switch the team – {err}"));
                    Task::none()
                }
            },
            Command::End { .. } => {
                // A background session stops as with X in Brain: its conversation stays.
                if let Some(agent) = self.model.board.get(&key).and_then(|s| s.agent.clone()).filter(|a| a.is_active()) {
                    let Some(account) = self.model.account(&key.account).cloned() else { return Task::none() };
                    match brain_core::agents::stop(&account, &agent.id) {
                        Ok(()) => {
                            self.model.relist_transcripts();
                            self.set_status(tr!("„{name}“ vom Handy gestoppt.", "Stopped “{name}” from the phone."));
                        }
                        Err(err) => self.set_status(tr!("Vom Handy: Stoppen fehlgeschlagen – {err}", "From the phone: stopping failed – {err}")),
                    }
                    return Task::none();
                }
                if phase == Phase::Ended {
                    return Task::none();
                }
                // Any other session ends itself, once it waits for a prompt.
                self.run_link_command(Command::Reply { key, text: "/exit".into() }, first_try)
            }
            Command::Restart { flags, .. } => match self.restart_session(key, flags, true) {
                Ok(task) => task,
                Err(err) => {
                    self.set_status(tr!("Vom Handy: Neustart geht nicht – {err}", "From the phone: can't restart – {err}"));
                    Task::none()
                }
            },
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
    fn link_start(&mut self, account: &str, folder: &str, prompt: Option<String>, style: Option<String>, team: bool) -> Task<Message> {
        let team = team.then(crate::config::team_start);
        let Some(account) = self.model.account(account).cloned() else { return Task::none() };
        // Only a style the account knows: anything else would quietly fall back to Default.
        let style = style.filter(|s| brain_core::styles::available(&account.config_dir, Some(std::path::Path::new(folder))).iter().any(|known| known.name == *s));
        let folder_name = folder.rsplit('/').find(|p| !p.is_empty()).unwrap_or(folder).to_string();
        self.set_status(tr!("Vom Handy: neue Session in {folder_name} …", "From the phone: new session in {folder_name} …"));
        let folder = folder.to_string();
        let options = brain_core::agents::Start { style, prompt, team, ..Default::default() };
        // A team needs an interactive session: in tmux, which Brain's terminal joins.
        if options.team.is_some() {
            let started = off_thread(move || brain_core::agents::start_team(&account, std::path::Path::new(&folder), &options));
            return Task::perform(started, Message::TeamStarted);
        }
        let started = off_thread(move || brain_core::agents::start(&account, std::path::Path::new(&folder), &options));
        Task::perform(started, Message::LinkStarted)
    }
}

impl Brain {
    /// The phone's answers, typed once Esc closed the question dialog: into Brain's terminal, or
    /// through the terminal app the session runs in. Not checked for "accepts input" first: the
    /// dialog just closed, Brain's state may not know yet.
    pub(super) fn type_after_dialog(&mut self, key: &SessionKey, text: &str) -> Task<Message> {
        if self.terminals.contains_key(key) {
            return self.send_text(key, text);
        }
        let Some(pid) = self.model.board.get(key).map(|s| s.pid) else { return Task::none() };
        let outcome = terminal::type_text(pid, text);
        self.report(outcome, Some(t("Antwort vom Handy gegeben.", "Answered from the phone.").into()));
        Task::none()
    }
}

/// The answers as one line Claude reads: each question with what was picked and written.
/// `None` when they don't fit the question (a pick that doesn't exist, several where one goes).
///
/// The line is typed into a terminal, and questions and options come from the transcript, so
/// from what Claude wrote: every control character (Esc, Return, a paste's end marker) becomes
/// a space, so no part of it can act as a key.
pub(crate) fn answer_text(question: &OpenQuestion, answers: &[Picked]) -> Option<String> {
    let text = answer_line(question, answers)?;
    Some(text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect())
}

fn answer_line(question: &OpenQuestion, answers: &[Picked]) -> Option<String> {
    if answers.len() != question.items.len() {
        return None;
    }
    let mut parts = Vec::new();
    for (item, answer) in question.items.iter().zip(answers) {
        if answer.picks.len() > 1 && !item.multi_select {
            return None;
        }
        let mut chosen: Vec<String> = answer.picks.iter().map(|&p| item.options.get(p).cloned()).collect::<Option<_>>()?;
        chosen.extend(answer.other.iter().cloned());
        if chosen.is_empty() {
            return None;
        }
        parts.push(format!("„{}“: {}", item.question, chosen.join(", ")));
    }
    Some(format!("{} {}", t("Meine Antworten auf deine Fragen –", "My answers to your questions –"), parts.join("; ")))
}

/// The command again, once a freshly started terminal had time to attach.
fn later(command: Command) -> Task<Message> {
    Task::perform(off_thread(|| std::thread::sleep(ATTACH_WAIT)), move |_| Message::LinkRetry(command.clone()))
}

/// The text, once Esc had time to close the dialog.
fn type_later(key: SessionKey, text: String) -> Task<Message> {
    Task::perform(off_thread(|| std::thread::sleep(DIALOG_CLOSE)), move |_| Message::LinkType(key.clone(), text.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::qa::QuestionItem;

    fn question() -> OpenQuestion {
        OpenQuestion {
            call: "toolu_1".into(),
            items: vec![
                QuestionItem { question: "Variante?".into(), options: vec!["Bereich".into(), "Monat".into()], multi_select: false },
                QuestionItem { question: "Ansichten?".into(), options: vec!["Tag".into(), "Woche".into(), "Monat".into()], multi_select: true },
            ],
            multi_select: true,
        }
    }

    #[test]
    fn answers_become_one_line_claude_reads() {
        let answers = [Picked { picks: vec![1], other: Some("mit Vorschau\nbitte".into()) }, Picked { picks: vec![0, 2], other: None }];
        let text = answer_text(&question(), &answers).unwrap();
        assert!(text.ends_with("„Variante?“: Monat, mit Vorschau bitte; „Ansichten?“: Tag, Monat"), "{text}");
        assert!(!text.contains('\n'));
    }

    /// A question from the transcript that carries keys (Esc, a bracketed paste's end, Return)
    /// is typed as plain text, never as keys.
    #[test]
    fn answers_carry_no_keys() {
        let mut sneaky = question();
        sneaky.items[0].options[0] = "Bereich\x1b[201~\r/exit\r".into();
        let answers = [Picked { picks: vec![0], other: Some("ok\x03\u{7f}".into()) }, Picked { picks: vec![0], other: None }];
        let text = answer_text(&sneaky, &answers).unwrap();
        assert!(!text.chars().any(char::is_control), "{text:?}");
        assert!(text.contains("Bereich [201~ /exit"), "{text:?}");
    }

    #[test]
    fn answers_that_dont_fit_are_refused() {
        // Several picks where one goes, a pick that doesn't exist, a question left out.
        assert_eq!(answer_text(&question(), &[Picked { picks: vec![0, 1], other: None }, Picked { picks: vec![0], other: None }]), None);
        assert_eq!(answer_text(&question(), &[Picked { picks: vec![5], other: None }, Picked { picks: vec![0], other: None }]), None);
        assert_eq!(answer_text(&question(), &[Picked { picks: vec![0], other: None }]), None);
        assert_eq!(answer_text(&question(), &[Picked { picks: vec![], other: None }, Picked { picks: vec![0], other: None }]), None);
    }
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
