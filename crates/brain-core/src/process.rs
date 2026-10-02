use std::collections::HashMap;
use std::process::Command;

use crate::account::Account;
use crate::sessions::SessionFile;

/// pid → parent pid for every process on the machine.
#[derive(Debug, Default)]
pub struct ProcessTable {
    parents: HashMap<u32, u32>,
}

impl ProcessTable {
    pub fn snapshot() -> Self {
        let output = Command::new("ps").args(["-axo", "pid=,ppid="]).output();
        match output {
            Ok(out) => Self::parse(&String::from_utf8_lossy(&out.stdout)),
            Err(_) => Self::default(),
        }
    }

    pub fn parse(ps_output: &str) -> Self {
        let parents = ps_output
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let pid = parts.next()?.parse().ok()?;
                let ppid = parts.next()?.parse().ok()?;
                Some((pid, ppid))
            })
            .collect();
        Self { parents }
    }

    /// `start` itself, then its parent, grandparent, … up to (excluding) pid 0/1.
    pub fn ancestors(&self, start: u32) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut current = start;
        while current > 1 && !chain.contains(&current) {
            chain.push(current);
            match self.parents.get(&current) {
                Some(parent) => current = *parent,
                None => break,
            }
        }
        chain
    }
}

/// Walks up from `start` until a pid owns a `<config>/sessions/<pid>.json` file.
/// That pid is the Claude Code process; the directory tells which account it belongs to.
///
/// With a `session_id`, only a session file carrying that id matches. This keeps
/// a nested `claude` (started from another session's shell) from being
/// attributed to its parent session once its own file is gone.
pub fn find_claude_session(
    table: &ProcessTable,
    start: u32,
    accounts: &[Account],
    session_id: Option<&str>,
) -> Option<(Account, u32)> {
    table.ancestors(start).into_iter().find_map(|pid| {
        accounts
            .iter()
            .find(|account| session_file_matches(&account.session_file(pid), session_id))
            .map(|account| (account.clone(), pid))
    })
}

fn session_file_matches(path: &std::path::Path, session_id: Option<&str>) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let Some(expected) = session_id else {
        return true;
    };
    serde_json::from_slice::<SessionFile>(&bytes)
        .ok()
        .and_then(|file| file.session_id)
        .is_none_or(|actual| actual == expected)
}

pub fn pid_alive(pid: u32) -> bool {
    // Signal 0 performs the permission/existence check without sending anything.
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The controlling terminal of a process as a device path, e.g. `/dev/ttys018`.
pub fn tty_of(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", "tty=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if tty.is_empty() || tty == "??" {
        return None;
    }
    Some(format!("/dev/{tty}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_claude_ancestor_and_its_account() {
        let home = tempfile::tempdir().unwrap();
        let main = Account::from_config_dir(home.path().join(".claude"));
        let second = Account::from_config_dir(home.path().join(".claude-second"));
        for account in [&main, &second] {
            std::fs::create_dir_all(account.sessions_dir()).unwrap();
        }
        std::fs::write(second.session_file(23096), r#"{"pid":23096,"sessionId":"outer"}"#).unwrap();
        // brain (500) → zsh (49732) → claude (23096) → zsh (22855) → login (1)
        let table = ProcessTable::parse("  500 49732\n49732 23096\n23096 22855\n22855 1\n");
        let accounts = [main.clone(), second.clone()];

        assert_eq!(find_claude_session(&table, 500, &accounts, None), Some((second.clone(), 23096)));
        assert_eq!(find_claude_session(&table, 500, &accounts, Some("outer")), Some((second, 23096)));
        // A nested session whose own file is already gone must not match the outer one.
        assert_eq!(find_claude_session(&table, 500, &accounts, Some("nested")), None);
        assert_eq!(find_claude_session(&table, 22855, &[main], None), None);
    }

    #[test]
    fn ancestors_stop_on_cycles() {
        let table = ProcessTable::parse("10 11\n11 10\n");
        assert_eq!(table.ancestors(10), vec![10, 11]);
    }
}
