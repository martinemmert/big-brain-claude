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

/// When a process started (epoch ms), from the kernel's process info.
#[cfg(target_os = "macos")]
pub fn process_started_ms(pid: u32) -> Option<i64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size)
    };
    (read == size).then(|| info.pbi_start_tvsec as i64 * 1000 + info.pbi_start_tvusec as i64 / 1000)
}

/// When a process started (epoch ms): `/proc/<pid>/stat` has it in clock ticks
/// since boot, `/proc/stat` the boot time.
#[cfg(target_os = "linux")]
pub fn process_started_ms(pid: u32) -> Option<i64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let boot = std::fs::read_to_string("/proc/stat").ok()?;
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    started_ms(&stat, &boot, ticks_per_second as i64)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn started_ms(pid_stat: &str, proc_stat: &str, ticks_per_second: i64) -> Option<i64> {
    // The command name (field 2) may contain spaces and parentheses; field 22
    // is the 20th after its closing parenthesis.
    let (_, fields) = pid_stat.rsplit_once(')')?;
    let ticks: i64 = fields.split_whitespace().nth(19)?.parse().ok()?;
    let boot_seconds: i64 = proc_stat.lines().find_map(|line| line.strip_prefix("btime "))?.trim().parse().ok()?;
    (ticks_per_second > 0).then(|| boot_seconds * 1000 + ticks * 1000 / ticks_per_second)
}

/// The controlling terminal of a process as a device path, e.g. `/dev/ttys018`
/// (macOS) or `/dev/pts/3` (Linux).
pub fn tty_of(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", "tty=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // No terminal: `??` on macOS, `?` on Linux.
    if tty.is_empty() || tty.chars().all(|c| c == '?') {
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
    fn reads_the_start_time_from_proc() {
        let pid_stat = "4200 (my (odd) name) S 1 4200 4200 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 12345 0 0";
        let proc_stat = "cpu  1 2 3\nbtime 1759480000\nprocesses 99\n";
        assert_eq!(started_ms(pid_stat, proc_stat, 100), Some(1_759_480_000_000 + 123_450));
        assert_eq!(started_ms("garbage", proc_stat, 100), None);
    }

    #[test]
    fn ancestors_stop_on_cycles() {
        let table = ProcessTable::parse("10 11\n11 10\n");
        assert_eq!(table.ancestors(10), vec![10, 11]);
    }
}
