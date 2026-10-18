//! `brain sessions`, `brain open` and `brain show`: session lists for Alfred and scripts,
//! and the `<account>:<pid>` address they hand back.

use std::fmt;

use brain_core::hook::one_line;
use brain_core::state::{Phase, Session};
use serde_json::{json, Value};

/// A session as addressed on the command line: `<account>:<pid>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub account: String,
    pub pid: u32,
}

impl Target {
    pub fn parse(text: &str) -> Result<Self, String> {
        let invalid = || format!("expected <account>:<pid>, got {text:?}");
        // Split at the last colon: the pid never contains one.
        let (account, pid) = text.trim().rsplit_once(':').ok_or_else(invalid)?;
        let pid = pid.parse().map_err(|_| invalid())?;
        if account.is_empty() {
            return Err(invalid());
        }
        Ok(Self { account: account.to_string(), pid })
    }

    fn of(session: &Session) -> Self {
        Self { account: session.key.account.clone(), pid: session.pid }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.account, self.pid)
    }
}

const HEADLINE_MAX: usize = 200;

/// An Alfred Script Filter result list
/// (<https://www.alfredapp.com/help/workflows/inputs/script-filter/json/>).
/// `skipknowledge` keeps Brain's triage order instead of Alfred's learned order.
pub fn alfred_items(sessions: &[&Session]) -> Value {
    let items: Vec<Value> = sessions
        .iter()
        .map(|session| {
            let target = Target::of(session).to_string();
            let name = session.display_name();
            let headline = session.headline().map(|h| one_line(&h, HEADLINE_MAX));
            let subtitle = [Some(phase_label(session.phase()).to_string()), Some(session.key.account.clone()), headline.clone()]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            let matched = [Some(name.as_str()), session.cwd.as_deref(), headline.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            json!({
                "uid": target,
                "title": name,
                "subtitle": subtitle,
                "arg": target,
                "match": matched,
                "mods": {
                    "alt": { "arg": target, "subtitle": "Show in Brain" }
                }
            })
        })
        .collect();
    json!({ "skipknowledge": true, "items": items })
}

/// A running Claude Code session another session can message, in any account.
#[derive(Debug, Clone, PartialEq)]
pub struct Peer {
    /// What `SendMessage` takes as `to`: `uds:<socket>`.
    pub address: String,
    pub name: String,
    pub account: String,
    pub cwd: Option<String>,
    /// Claude Code's own: `busy`, `idle`, `waiting`.
    pub status: Option<String>,
}

/// The running sessions of every account that take messages, from Claude Code's status files:
/// not the calling one, not a spare process, not one parked in the background (its process shows
/// another session). `query` keeps those whose name, folder or account has it.
pub fn peers(accounts: &[brain_core::account::Account], calling: Option<(String, u32)>, query: Option<&str>) -> Vec<Peer> {
    let query = query.map(str::to_lowercase).filter(|q| !q.trim().is_empty());
    let mut out: Vec<Peer> = accounts
        .iter()
        .flat_map(|account| brain_core::sessions::read_session_files(account).into_iter().map(move |file| (account.id.clone(), file)))
        .filter(|(account, file)| calling.as_ref() != Some(&(account.clone(), file.pid)))
        .filter(|(_, file)| !file.spare && file.parked_job_id.is_none() && brain_core::process::pid_alive(file.pid))
        .filter_map(|(account, file)| {
            let socket = file.messaging_socket_path.clone()?;
            let name = file.name.clone().or_else(|| file.cwd.as_deref().and_then(|c| c.rsplit('/').find(|p| !p.is_empty())).map(str::to_string)).unwrap_or_else(|| format!("pid {}", file.pid));
            Some(Peer { address: format!("uds:{socket}"), name, account, cwd: file.cwd, status: file.status })
        })
        .filter(|peer| {
            query.as_ref().is_none_or(|q| {
                [Some(peer.name.as_str()), peer.cwd.as_deref(), Some(peer.account.as_str())].into_iter().flatten().any(|f| f.to_lowercase().contains(q.as_str()))
            })
        })
        .collect();
    out.sort_by(|a, b| a.account.cmp(&b.account).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

impl Peer {
    pub fn to_json(&self) -> Value {
        json!({ "address": self.address, "name": self.name, "account": self.account, "cwd": self.cwd, "status": self.status })
    }
}

/// A plain list for scripts.
pub fn json_list(sessions: &[&Session]) -> Value {
    sessions
        .iter()
        .map(|session| {
            json!({
                "account": session.key.account,
                "pid": session.pid,
                "name": session.display_name(),
                "phase": phase_id(session.phase()),
                "headline": session.headline(),
                "cwd": session.cwd,
            })
        })
        .collect()
}

fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::NeedsYou => "Needs you",
        Phase::YourTurn => "Done, your turn",
        Phase::Working => "Working",
        Phase::Background => "Working in the background",
        Phase::Asleep => "Asleep, wakes when written to",
        Phase::Ended => "Ended",
    }
}

fn phase_id(phase: Phase) -> &'static str {
    match phase {
        Phase::NeedsYou => "needs_you",
        Phase::YourTurn => "your_turn",
        Phase::Working => "working",
        Phase::Background => "background",
        Phase::Asleep => "asleep",
        Phase::Ended => "ended",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::sessions::SessionFile;
    use brain_core::state::Board;

    #[test]
    fn parses_targets_at_the_last_colon_and_rejects_garbage() {
        assert_eq!(Target::parse("second:40461"), Ok(Target { account: "second".into(), pid: 40461 }));
        assert_eq!(Target::parse("a:b:7"), Ok(Target { account: "a:b".into(), pid: 7 }));
        for bad in ["40461", "second:", ":40461", "second:-1", "second:pid"] {
            assert!(Target::parse(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn alfred_items_carry_the_target_phase_and_a_one_line_headline() {
        let mut board = Board::default();
        let file = SessionFile {
            pid: 40461,
            session_id: Some("s1".into()),
            cwd: Some("/w/checkout-app".into()),
            name: None,
            status: Some("busy".into()),
            started_at: None,
            updated_at: Some(1_790_000_000_000),
            status_updated_at: Some(1_790_000_000_000),
            spare: false,
            parked_job_id: None,
            messaging_socket_path: None,
        };
        board.apply_session_file("second", &file, true);
        let event = brain_core::event::Event {
            v: 1,
            ts: chrono::DateTime::from_timestamp(1_790_000_001, 0).unwrap(),
            account: "second".into(),
            pid: 40461,
            session_id: Some("s1".into()),
            cwd: None,
            source: brain_core::event::Source::Report,
            kind: brain_core::event::Kind::Doing,
            text: Some("Migrating\nthe   store".into()),
            tasks: None,
            name: None,
            ask: None,
        };
        board.apply_event(&event);

        let json = alfred_items(&board.sorted());
        let item = &json["items"][0];

        assert_eq!(item["title"], "checkout-app");
        assert_eq!(item["subtitle"], "Working · second · Migrating the store");
        assert_eq!(item["arg"], "second:40461");
        assert_eq!(item["uid"], "second:40461");
        assert_eq!(item["match"], "checkout-app /w/checkout-app Migrating the store");
        assert_eq!(item["mods"]["alt"]["arg"], "second:40461");
        assert_eq!(item["mods"]["alt"]["subtitle"], "Show in Brain");
    }
}
