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
        Phase::Ended => "Ended",
    }
}

fn phase_id(phase: Phase) -> &'static str {
    match phase {
        Phase::NeedsYou => "needs_you",
        Phase::YourTurn => "your_turn",
        Phase::Working => "working",
        Phase::Background => "background",
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
