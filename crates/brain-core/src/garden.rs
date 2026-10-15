//! The gardener: sessions Brain lists but can't steer any more, and ending them for good.
//!
//! What counts as broken:
//! - **left over**: the same conversation runs twice. Either with the same id, or after a move:
//!   its transcript was copied to another account and resumed there with `--fork-session`
//!   (a new id, the same name), but the original kept running. The older process is the
//!   leftover.
//! - **unreachable**: Brain's terminal closed right after attaching although the process lives
//!   on, so nothing typed reaches it.
//! - **stuck** (a guess, so not preselected): it says "working" but nothing happened for hours.
//!
//! Ending one: `claude stop` for a background session, then SIGTERM, then SIGKILL, each only
//! while the process is still there. The transcript stays, so the conversation can be resumed.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::account::Account;
use crate::state::{Phase, Session, SessionKey};

/// "Working" without a sign of life for this long is likely stuck.
pub const STUCK_AFTER_MS: i64 = 3 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// Runs again as `newer` (account and id); this one is the old copy.
    LeftOver { newer: SessionKey },
    /// Brain's terminal closed right after attaching; what it showed last.
    Unreachable { said: Vec<String> },
    /// "Working" but quiet for this long.
    Stuck { quiet_ms: i64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub key: SessionKey,
    pub reason: Reason,
    /// Sure enough to be preselected; a stuck one is only a guess.
    pub sure: bool,
}

/// The broken ones among `sessions`. `unreachable`: sessions whose terminal closed right after
/// attaching, with the terminal's last lines. `copied_to`: the other accounts that hold a copy of
/// a conversation's transcript (by its id).
pub fn findings(sessions: &[&Session], now_ms: i64, unreachable: &HashMap<SessionKey, Vec<String>>, copied_to: &dyn Fn(&str) -> Vec<String>) -> Vec<Finding> {
    // Only sessions with a process of their own (a background session listed without one has
    // pid 0): there is something to end.
    let live: Vec<&Session> = sessions.iter().copied().filter(|s| s.alive && s.pid > 1 && s.phase() != Phase::Ended).collect();
    let mut out: Vec<Finding> = Vec::new();
    let mut by_conversation: HashMap<&str, Vec<&Session>> = HashMap::new();
    for s in &live {
        if let Some(id) = s.session_id.as_deref() {
            by_conversation.entry(id).or_default().push(s);
        }
    }
    for copies in by_conversation.values().filter(|c| c.len() > 1) {
        let mut copies = copies.clone();
        copies.sort_by_key(|s| s.started_ms.unwrap_or(i64::MAX));
        let newest = copies.last().map(|s| s.key.clone()).expect("two or more");
        for old in &copies[..copies.len() - 1] {
            out.push(Finding { key: old.key.clone(), reason: Reason::LeftOver { newer: newest.clone() }, sure: true });
        }
    }
    // Moved: a newer session of the same name runs in an account its transcript was copied to.
    for s in &live {
        let (Some(id), Some(started)) = (s.session_id.as_deref(), s.started_ms) else { continue };
        if out.iter().any(|f| f.key == s.key) {
            continue;
        }
        let targets = copied_to(id);
        let newer = live.iter().find(|t| {
            t.key.account != s.key.account
                && targets.contains(&t.key.account)
                && t.display_name() == s.display_name()
                && t.started_ms.is_some_and(|t_started| t_started > started)
        });
        if let Some(newer) = newer {
            out.push(Finding { key: s.key.clone(), reason: Reason::LeftOver { newer: newer.key.clone() }, sure: true });
        }
    }
    for s in &live {
        if out.iter().any(|f| f.key == s.key) {
            continue;
        }
        if let Some(said) = unreachable.get(&s.key) {
            out.push(Finding { key: s.key.clone(), reason: Reason::Unreachable { said: said.clone() }, sure: true });
        } else if s.phase() == Phase::Working && now_ms - s.last_activity_ms > STUCK_AFTER_MS {
            out.push(Finding { key: s.key.clone(), reason: Reason::Stuck { quiet_ms: now_ms - s.last_activity_ms }, sure: false });
        }
    }
    out.sort_by_key(|f| (!f.sure, f.key.account.clone(), f.key.id.clone()));
    out
}

/// Ends a session for good and says how: `claude stop` for a background one, then SIGTERM, then
/// SIGKILL, each only while the process is still there. A pid that no longer runs Claude Code
/// (reused by another program) is left alone.
pub fn end(account: &Account, pid: u32, agent_id: Option<&str>) -> Result<String, String> {
    // 0 and 1 aren't a session's process: a signal to 0 reaches the whole process group.
    if pid <= 1 {
        return Err(format!("no process of its own (pid {pid}); left alone"));
    }
    if !crate::process::pid_alive(pid) {
        return Ok("already gone".into());
    }
    if !crate::process::is_claude(pid) {
        return Err(format!("pid {pid} is no Claude Code process any more; left alone"));
    }
    let mut tried = Vec::new();
    if let Some(id) = agent_id {
        match crate::agents::stop(account, id) {
            Ok(()) => tried.push("claude stop".to_string()),
            Err(err) => tried.push(format!("claude stop failed ({err})")),
        }
        if gone_within(pid, Duration::from_secs(3)) {
            return Ok(tried.join(", "));
        }
    }
    for (signal, name, wait) in [(libc::SIGTERM, "SIGTERM", 3), (libc::SIGKILL, "SIGKILL", 2)] {
        // SAFETY: sending a signal to a pid checked above to run Claude Code.
        unsafe { libc::kill(pid as libc::pid_t, signal) };
        tried.push(name.to_string());
        if gone_within(pid, Duration::from_secs(wait)) {
            return Ok(tried.join(", "));
        }
    }
    Err(format!("still running after {}", tried.join(", ")))
}

fn gone_within(pid: u32, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if !crate::process::pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    !crate::process::pid_alive(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(account: &str, id: &str, conversation: &str, started: i64, last: i64, working: bool) -> Session {
        let mut s = Session::for_test(SessionKey { account: account.into(), id: id.into() }, 100);
        s.session_id = Some(conversation.into());
        s.alive = true;
        s.started_ms = Some(started);
        s.last_activity_ms = last;
        // The phase as `claude agents` reports it, which `phase()` takes first.
        s.agent = Some(crate::agents::BackgroundAgent {
            id: id.into(),
            session_id: conversation.into(),
            name: None,
            cwd: None,
            state: if working { "working" } else { "idle" }.into(),
            started_ms: Some(started),
            pid: Some(100),
        });
        s
    }

    #[test]
    fn the_older_copy_is_left_over_unreachable_is_sure_stuck_only_a_guess() {
        let now = 10 * STUCK_AFTER_MS;
        let original = session("third", "a788", "conv-1", 1_000, now - 60_000, false);
        let moved = session("second", "a788", "conv-1", 5_000, now - 60_000, false);
        let stuck = session("third", "6919", "conv-2", 1_000, now - STUCK_AFTER_MS - 1, true);
        let busy = session("third", "fb13", "conv-3", 1_000, now - 60_000, true);
        let deaf = session("second", "33b2", "conv-4", 1_000, now - 60_000, false);
        let unreachable = HashMap::from([(deaf.key.clone(), vec!["Error: no pty".to_string()])]);
        let mut without_process = session("main", "f1x0", "conv-1", 9_000, now - 60_000, false);
        without_process.pid = 0;
        let all = [&original, &moved, &stuck, &busy, &deaf, &without_process];

        let found = findings(&all, now, &unreachable, &|_| Vec::new());

        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found.contains(&Finding { key: original.key.clone(), reason: Reason::LeftOver { newer: moved.key.clone() }, sure: true }));
        assert!(found.contains(&Finding { key: deaf.key.clone(), reason: Reason::Unreachable { said: vec!["Error: no pty".into()] }, sure: true }));
        assert!(found.iter().any(|f| f.key == stuck.key && !f.sure));
        assert!(!found.iter().any(|f| f.key == busy.key));
        assert!(!found.iter().any(|f| f.key == without_process.key), "nothing to end without a process");
    }

    #[test]
    fn a_moved_conversation_leaves_its_original_over() {
        let now = 10 * STUCK_AFTER_MS;
        let mut original = session("third", "a788", "a788fe21", 1_000, now - 60_000, false);
        original.name = Some("FUX Session".into());
        // Resumed in second with `--fork-session`: a new id, the same name, started later.
        let mut moved = session("second", "d3d8", "d3d8eba1", 5_000, now - 60_000, false);
        moved.name = Some("FUX Session".into());
        let mut other = session("second", "0f82", "0f82c3ac", 9_000, now - 60_000, false);
        other.name = Some("slack thread".into());
        let copied = |id: &str| if id == "a788fe21" { vec!["second".to_string()] } else { Vec::new() };

        let found = findings(&[&original, &moved, &other], now, &HashMap::new(), &copied);

        assert_eq!(found, vec![Finding { key: original.key.clone(), reason: Reason::LeftOver { newer: moved.key.clone() }, sure: true }]);
    }

    #[test]
    fn never_signals_pid_0_or_1() {
        let account = Account { id: "main".into(), config_dir: std::path::PathBuf::from("/nowhere") };
        assert!(end(&account, 0, None).is_err());
        assert!(end(&account, 1, None).is_err());
    }
}
