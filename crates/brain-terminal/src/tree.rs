//! The process tree with executable names and ttys, and which terminal an
//! ancestor belongs to.

use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ITerm,
    TerminalApp,
    Tmux,
    VsCode,
    Cursor,
}

/// What walking up from a process found.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    /// The nearest ancestor (or the process itself) that belongs to a known terminal.
    Known(Kind, u32),
    /// No known terminal; carries the name of the topmost ancestor below launchd.
    Unknown(String),
}

struct Process {
    ppid: u32,
    tty: Option<String>,
    comm: String,
}

#[derive(Default)]
pub struct Tree {
    processes: HashMap<u32, Process>,
}

impl Tree {
    pub fn snapshot() -> Self {
        match Command::new("ps").args(["-axo", "pid=,ppid=,tty=,comm="]).output() {
            Ok(out) => Self::parse(&String::from_utf8_lossy(&out.stdout)),
            Err(_) => Self::default(),
        }
    }

    /// Parses `ps -axo pid=,ppid=,tty=,comm=`. On macOS `comm` is the full
    /// executable path and may contain spaces, so it is the rest of the line.
    pub fn parse(ps_output: &str) -> Self {
        let processes = ps_output
            .lines()
            .filter_map(|line| {
                let mut rest = line.trim_start();
                let mut field = || {
                    let (value, tail) = rest.split_once(char::is_whitespace)?;
                    rest = tail.trim_start();
                    Some(value)
                };
                let pid = field()?.parse().ok()?;
                let ppid = field()?.parse().ok()?;
                let tty = field()?;
                let tty = (tty != "??").then(|| format!("/dev/{tty}"));
                Some((pid, Process { ppid, tty, comm: rest.trim_end().to_string() }))
            })
            .collect();
        Self { processes }
    }

    pub fn contains(&self, pid: u32) -> bool {
        self.processes.contains_key(&pid)
    }

    pub fn parent(&self, pid: u32) -> Option<u32> {
        Some(self.processes.get(&pid)?.ppid)
    }

    /// The controlling terminal as a device path, e.g. `/dev/ttys018`.
    pub fn tty(&self, pid: u32) -> Option<&str> {
        self.processes.get(&pid)?.tty.as_deref()
    }

    /// Walks from `pid` up to (excluding) launchd.
    pub fn classify(&self, pid: u32) -> Option<Found> {
        if !self.contains(pid) {
            return None;
        }
        let mut current = pid;
        let mut seen = Vec::new();
        let mut topmost = "";
        while current > 1 && !seen.contains(&current) {
            seen.push(current);
            let Some(process) = self.processes.get(&current) else { break };
            if let Some(kind) = kind_of(&process.comm) {
                return Some(Found::Known(kind, current));
            }
            topmost = &process.comm;
            current = process.ppid;
        }
        Some(Found::Unknown(basename(topmost).to_string()))
    }
}

fn kind_of(comm: &str) -> Option<Kind> {
    let name = basename(comm);
    if name == "tmux" {
        Some(Kind::Tmux)
    } else if comm.contains("/iTerm.app/") || name.starts_with("iTermServer") {
        Some(Kind::ITerm)
    } else if comm.contains("/Terminal.app/") {
        Some(Kind::TerminalApp)
    } else if comm.contains("/Visual Studio Code.app/") {
        Some(Kind::VsCode)
    } else if comm.contains("/Cursor.app/") {
        Some(Kind::Cursor)
    } else {
        None
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = "\
  705     1 ??       /Applications/iTerm.app/Contents/MacOS/iTerm2
  840   705 ??       /Users/me/Library/Application Support/iTerm2/iTermServer-3.6.10
  841   840 ttys000  /usr/bin/login
  845   841 ttys000  -zsh
 4200   845 ttys000  claude
  900     1 ??       /System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal
  901   900 ttys001  login
  902   901 ttys001  -zsh
  903   902 ttys001  claude
64104     1 ??       tmux
64107 64104 ttys014  -zsh
64110 64107 ttys014  claude
  768     1 ??       /Applications/Visual Studio Code.app/Contents/MacOS/Code
  770   768 ??       /Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)
  771   770 ttys020  /bin/zsh
  772   771 ttys020  claude
  500     1 ??       /usr/sbin/sshd
  501   500 ttys030  -zsh
  502   501 ttys030  claude
";

    #[test]
    fn classifies_the_nearest_terminal_ancestor() {
        let tree = Tree::parse(PS);
        assert_eq!(tree.classify(4200), Some(Found::Known(Kind::ITerm, 840)));
        assert_eq!(tree.classify(903), Some(Found::Known(Kind::TerminalApp, 900)));
        // The tmux server is the nearest; whatever hosts its client is resolved separately.
        assert_eq!(tree.classify(64110), Some(Found::Known(Kind::Tmux, 64104)));
        assert_eq!(tree.classify(772), Some(Found::Known(Kind::VsCode, 770)));
        assert_eq!(tree.classify(502), Some(Found::Unknown("sshd".into())));
        assert_eq!(tree.classify(99999), None);
        assert_eq!(tree.tty(4200), Some("/dev/ttys000"));
        assert_eq!(tree.tty(64104), None);
    }
}
