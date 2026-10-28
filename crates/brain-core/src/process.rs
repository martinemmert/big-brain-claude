use std::process::Command;

use crate::account::Account;
use crate::sessions::read_session_files;

/// The Claude Code session that runs this process (`brain report`, `brain later`, the mod's
/// `brain hook`), as Claude Code names it in the environment of every command it starts:
/// `CLAUDE_CODE_SESSION_ID`, whose session file holds the pid, else `CLAUDE_PID`, whose session
/// file says the account.
pub fn calling_session(accounts: &[Account]) -> Option<(Account, u32)> {
    let session_id = std::env::var("CLAUDE_CODE_SESSION_ID").ok();
    let pid = std::env::var("CLAUDE_PID").ok();
    calling_session_from(accounts, session_id.as_deref(), pid.as_deref())
}

fn calling_session_from(accounts: &[Account], session_id: Option<&str>, pid: Option<&str>) -> Option<(Account, u32)> {
    session_id.filter(|id| !id.is_empty()).and_then(|id| session_by_id(accounts, id)).or_else(|| {
        let pid: u32 = pid?.trim().parse().ok()?;
        accounts.iter().find(|a| a.session_file(pid).exists()).map(|a| (a.clone(), pid))
    })
}

/// The account and pid of the running session with this id, from Claude Code's session files.
pub fn session_by_id(accounts: &[Account], session_id: &str) -> Option<(Account, u32)> {
    accounts.iter().find_map(|account| {
        read_session_files(account).into_iter().find(|f| f.session_id.as_deref() == Some(session_id)).map(|f| (account.clone(), f.pid))
    })
}

pub fn pid_alive(pid: u32) -> bool {
    // A session known only from `claude agents` has no pid (0): `kill(0, …)` would ask Brain's
    // own process group.
    if pid == 0 {
        return false;
    }
    // Signal 0 performs the permission/existence check without sending anything.
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Whether `pid` runs Claude Code: a pid Brain remembers may belong to another program by now.
pub fn is_claude(pid: u32) -> bool {
    let Ok(out) = Command::new("ps").args(["-o", "command=", "-p", &pid.to_string()]).output() else { return false };
    claude_program(&String::from_utf8_lossy(&out.stdout))
}

/// Its command is `claude …`, or the versioned binary Claude Code's daemon starts background
/// sessions with (`~/.local/share/claude/versions/2.1.312 …`).
fn claude_program(command: &str) -> bool {
    command.split_whitespace().next().is_some_and(|program| {
        let parent = std::path::Path::new(program).parent().and_then(|p| p.to_str()).unwrap_or("");
        program == "claude" || program.ends_with("/claude") || parent.ends_with("/claude/versions")
    })
}

/// When a process started (epoch ms), from the kernel's process info.
pub fn process_started_ms(pid: u32) -> Option<i64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size)
    };
    (read == size).then(|| info.pbi_start_tvsec as i64 * 1000 + info.pbi_start_tvusec as i64 / 1000)
}

/// The conversation a running session was started from (`claude --resume <id>`, also with
/// `--fork-session`), from its command line.
pub fn resumed_from(pid: u32) -> Option<String> {
    let out = Command::new("ps").args(["-o", "args=", "-p", &pid.to_string()]).output().ok()?;
    resume_arg(&String::from_utf8_lossy(&out.stdout))
}

fn resume_arg(command_line: &str) -> Option<String> {
    let mut words = command_line.split_whitespace();
    while let Some(word) = words.next() {
        if word == "--resume" || word == "-r" {
            return words.next().filter(|id| !id.starts_with('-')).map(str::to_string);
        }
        if let Some(id) = word.strip_prefix("--resume=") {
            return Some(id.to_string());
        }
    }
    None
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
    fn the_calling_session_is_the_one_claude_code_names() {
        let home = tempfile::tempdir().unwrap();
        let main = Account::from_config_dir(home.path().join(".claude"));
        let second = Account::from_config_dir(home.path().join(".claude-second"));
        for account in [&main, &second] {
            std::fs::create_dir_all(account.sessions_dir()).unwrap();
        }
        std::fs::write(second.session_file(23096), r#"{"pid":23096,"sessionId":"outer"}"#).unwrap();
        let accounts = [main.clone(), second.clone()];

        assert_eq!(calling_session_from(&accounts, Some("outer"), None), Some((second.clone(), 23096)));
        // After a /clear the id may be newer than the file: the pid still finds it.
        assert_eq!(calling_session_from(&accounts, Some("newer"), Some("23096")), Some((second, 23096)));
        assert_eq!(calling_session_from(&accounts, Some("newer"), Some("500")), None);
        assert_eq!(calling_session_from(&accounts, None, None), None);
    }

    #[test]
    fn claude_code_is_known_by_its_program_also_the_versioned_one() {
        assert!(claude_program("claude --resume x"));
        assert!(claude_program("/Users/m/.local/bin/claude"));
        assert!(claude_program("/Users/m/.local/share/claude/ClaudeCode.app/Contents/MacOS/claude --bg"));
        assert!(claude_program("/Users/m/.local/share/claude/versions/2.1.312 --resume /Users/m/x.jsonl"));
        // The desktop app and anything else that took over a pid.
        assert!(!claude_program("/Applications/Claude.app/Contents/MacOS/Claude"));
        assert!(!claude_program("/usr/bin/vim notes"));
    }

    #[test]
    fn a_resumed_session_names_the_conversation_it_was_started_from() {
        assert_eq!(resume_arg("claude --resume 9fa5e20b --fork-session"), Some("9fa5e20b".into()));
        assert_eq!(resume_arg("claude -r 9fa5e20b"), Some("9fa5e20b".into()));
        assert_eq!(resume_arg("claude --resume=9fa5e20b --model haiku"), Some("9fa5e20b".into()));
        // `--resume` alone opens the picker: no conversation is named.
        assert_eq!(resume_arg("claude --resume --model haiku"), None);
        assert_eq!(resume_arg("claude"), None);
    }
}
