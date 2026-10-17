//! `brain statusline --json-file <file>`: records what Brain's mod measured in a session (plan
//! limits, context, cost), in the shape of Claude Code's status line JSON.

use std::process::ExitCode;

use brain_core::account::{home_dir, Account};
use brain_core::process::{calling_session, session_by_id};
use brain_core::usage::{self, Snapshot};

pub fn run(json: Option<String>, accounts: &[Account]) -> ExitCode {
    if let Some(input) = json {
        let _ = record(&input, accounts);
    }
    ExitCode::SUCCESS
}

fn record(input: &str, accounts: &[Account]) -> Option<()> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    let session_id = value.get("session_id").and_then(|v| v.as_str());
    // The session id names Claude Code's session file.
    let (account, pid) = session_id.and_then(|id| session_by_id(accounts, id)).or_else(|| calling_session(accounts))?;
    let snapshot = Snapshot::from_statusline(&account.id, pid, chrono::Utc::now().timestamp_millis(), &value);
    // The forecast's pace needs where the weekly limit stood a day ago.
    let _ = usage::record_history(&usage::history_dir(&home_dir()), &snapshot);
    usage::store(&usage::status_dir(&home_dir()), &snapshot).ok()
}
