//! `brain statusline [--then <command>]`: records what Claude Code tells the status line
//! (plan limits, context, cost) for Brain, then runs the user's own status line command with the
//! same input. Its output and exit code are passed through unchanged.

use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};

use brain_core::account::{home_dir, Account};
use brain_core::process::{find_claude_session, ProcessTable};
use brain_core::sessions::read_session_files;
use brain_core::usage::{self, Snapshot};

pub fn run(then: Option<String>, json: Option<String>, accounts: &[Account]) -> ExitCode {
    // From Brain's mod: only record, there's no status line to pass the input on to.
    if let Some(input) = json {
        let _ = record(&input, accounts);
        return ExitCode::SUCCESS;
    }
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);

    // Recording must never break the status line.
    let _ = record(&input, accounts);

    let Some(command) = then else { return ExitCode::SUCCESS };
    let child = Command::new("sh")
        .args(["-c", &command])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn();
    let Ok(mut child) = child else { return ExitCode::FAILURE };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    match child.wait() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(1, 255) as u8),
        Err(_) => ExitCode::FAILURE,
    }
}

fn record(input: &str, accounts: &[Account]) -> Option<()> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    let session_id = value.get("session_id").and_then(|v| v.as_str());
    // The session id names the session file directly; the process tree is the fallback.
    let (account, pid) = session_id
        .and_then(|id| {
            accounts.iter().find_map(|account| {
                read_session_files(account)
                    .into_iter()
                    .find(|f| f.session_id.as_deref() == Some(id))
                    .map(|f| (account.clone(), f.pid))
            })
        })
        .or_else(|| find_claude_session(&ProcessTable::snapshot(), std::process::id(), accounts, session_id))?;
    let snapshot = Snapshot::from_statusline(&account.id, pid, chrono::Utc::now().timestamp_millis(), &value);
    usage::store(&usage::status_dir(&home_dir()), &snapshot).ok()
}
