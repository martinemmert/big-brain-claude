//! Terminal adapters: find the terminal hosting a Claude Code process and
//! focus it, type into it or open a new tab.

// The macOS adapters build everywhere; on Linux no process is ever classified as theirs.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "applescript_macos.rs"]
mod applescript;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "iterm_macos.rs"]
mod iterm;
#[cfg(target_os = "linux")]
#[path = "launch_linux.rs"]
mod launch;
#[cfg_attr(target_os = "linux", path = "konsole_linux.rs")]
#[cfg_attr(target_os = "macos", path = "konsole_macos.rs")]
mod konsole;
mod script;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "terminal_app_macos.rs"]
mod terminal_app;
mod tmux;
mod tree;

use std::process::Command;

use tree::{Found, Kind, Tree};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    ITerm,
    TerminalApp,
    /// `typing` is false while Konsole's settings block input over D-Bus.
    Konsole { typing: bool },
    /// `target` is `$session:@window.%pane`; `client_tty` is the most recently
    /// active client attached to that session.
    Tmux { target: String, client_tty: Option<String> },
    VsCode,
    Cursor,
    /// No known terminal; the name of the topmost ancestor below launchd or systemd.
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub focus: bool,
    pub type_text: bool,
    pub keys: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Return,
    Escape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    NoTerminal,
    Unsupported(String),
    Failed(String),
}

/// How deep tmux clients are followed to the terminal showing them.
const MAX_NESTING: usize = 4;

/// What `host_of` found, with what the actions need.
enum Located {
    ITerm,
    TerminalApp,
    Konsole(konsole::Tab),
    Tmux(tmux::Pane),
    VsCode,
    Cursor,
    Unknown(String),
}

pub fn host_of(pid: u32) -> Option<Host> {
    let host = match locate(&Tree::snapshot(), pid)? {
        Located::ITerm => Host::ITerm,
        Located::TerminalApp => Host::TerminalApp,
        Located::Konsole(tab) => Host::Konsole { typing: tab.accepts_typing() },
        Located::Tmux(pane) => Host::Tmux { target: pane.target, client_tty: pane.client.map(|c| c.tty) },
        Located::VsCode => Host::VsCode,
        Located::Cursor => Host::Cursor,
        Located::Unknown(name) => Host::Unknown(name),
    };
    Some(host)
}

/// - iTerm2, Konsole, tmux: everything (Konsole: focus only while its settings block typing).
/// - Terminal.app: focus only. Its only way to send input, `do script … in tab`,
///   delivers the text and the newline in one write, which Claude Code can take
///   as a paste instead of a submit.
/// - VS Code, Cursor: `focus` brings the project window forward; the terminal
///   tab inside it is not reachable from outside, and neither is input.
/// - Unknown: nothing.
pub fn capabilities(host: &Host) -> Capabilities {
    let (focus, type_text, keys) = match host {
        Host::ITerm | Host::Konsole { typing: true } | Host::Tmux { .. } => (true, true, true),
        Host::Konsole { typing: false } | Host::TerminalApp | Host::VsCode | Host::Cursor => (true, false, false),
        Host::Unknown(_) => (false, false, false),
    };
    Capabilities { focus, type_text, keys }
}

/// Why Brain can't type into a host, so the app can say what to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Limit {
    /// Konsole's "security sensitive parts of the DBus API" are switched off.
    KonsoleLocked,
    TerminalApp,
    /// VS Code or Cursor.
    Editor,
    /// A terminal Brain does not know; its name.
    Unknown(String),
}

pub fn limit(host: &Host) -> Option<Limit> {
    match host {
        Host::ITerm | Host::Konsole { typing: true } | Host::Tmux { .. } => None,
        Host::Konsole { typing: false } => Some(Limit::KonsoleLocked),
        Host::TerminalApp => Some(Limit::TerminalApp),
        Host::VsCode | Host::Cursor => Some(Limit::Editor),
        Host::Unknown(name) => Some(Limit::Unknown(name.clone())),
    }
}

/// Brings the session's pane to the front. `cwd` picks the project window in
/// VS Code and Cursor.
pub fn focus(pid: u32, cwd: Option<&str>) -> Outcome {
    focus_in(&Tree::snapshot(), pid, cwd, 0)
}

/// Types `text` and presses Return, without focusing.
pub fn type_text(pid: u32, text: &str) -> Outcome {
    let tree = Tree::snapshot();
    let Some(located) = locate(&tree, pid) else { return Outcome::NoTerminal };
    match located {
        Located::ITerm => with_tty(&tree, pid, |tty| iterm::type_text(tty, text)),
        Located::Konsole(tab) => tab.type_text(text),
        Located::Tmux(pane) => pane.type_text(text),
        other => unsupported(&other, "typing"),
    }
}

/// Sends a single key, without focusing.
pub fn send_key(pid: u32, key: Key) -> Outcome {
    let tree = Tree::snapshot();
    let Some(located) = locate(&tree, pid) else { return Outcome::NoTerminal };
    match located {
        Located::ITerm => with_tty(&tree, pid, |tty| iterm::send_key(tty, key)),
        Located::Konsole(tab) => tab.send_key(key),
        Located::Tmux(pane) => pane.send_key(key),
        other => unsupported(&other, "sending keys"),
    }
}

/// Runs `cd <cwd> && CLAUDE_CONFIG_DIR=<dir> <command>` in a new iTerm2 tab if
/// iTerm2 is installed, else in a new Terminal.app window. `command` is passed
/// to the shell as is.
#[cfg(target_os = "macos")]
pub fn open_new(cwd: &str, config_dir: Option<&str>, command: &str) -> Outcome {
    let shell_command = script::shell_command(cwd, config_dir, command);
    if iterm::is_installed() {
        iterm::open_new(&shell_command)
    } else {
        terminal_app::open_new(&shell_command)
    }
}

/// Runs it in a new tab of Konsole, else of a GNOME terminal or the default terminal.
#[cfg(target_os = "linux")]
pub fn open_new(cwd: &str, config_dir: Option<&str>, command: &str) -> Outcome {
    launch::open_new(cwd, &script::shell_command(cwd, config_dir, command))
}

fn locate(tree: &Tree, pid: u32) -> Option<Located> {
    let located = match tree.classify(pid)? {
        Found::Known(Kind::ITerm, _) => Located::ITerm,
        Found::Known(Kind::TerminalApp, _) => Located::TerminalApp,
        // The tab is found by its shell, the process right below Konsole.
        Found::Known(Kind::Konsole, konsole) => match tree.child_toward(konsole, pid).and_then(|shell| konsole::locate(konsole, shell)) {
            Some(tab) => Located::Konsole(tab),
            None => Located::Unknown("konsole (tab not found)".into()),
        },
        Found::Known(Kind::VsCode, _) => Located::VsCode,
        Found::Known(Kind::Cursor, _) => Located::Cursor,
        Found::Known(Kind::Tmux, server) => match tree.tty(pid).and_then(|tty| tmux::locate(server, tty)) {
            Some(pane) => Located::Tmux(pane),
            None => Located::Unknown("tmux (pane not found)".into()),
        },
        Found::Unknown(name) => Located::Unknown(name),
    };
    Some(located)
}

fn focus_in(tree: &Tree, pid: u32, cwd: Option<&str>, depth: usize) -> Outcome {
    let Some(located) = locate(tree, pid) else { return Outcome::NoTerminal };
    match located {
        Located::ITerm => with_tty(tree, pid, iterm::focus),
        Located::TerminalApp => with_tty(tree, pid, terminal_app::focus),
        Located::Konsole(tab) => tab.focus(),
        Located::Tmux(pane) => {
            if let Err(err) = pane.select() {
                return Outcome::Failed(err);
            }
            match &pane.client {
                // The client runs in a terminal of its own (or in another tmux).
                // Its parent is the shell there; the client itself is a `tmux` process.
                Some(client) if depth < MAX_NESTING => match tree.parent(client.pid) {
                    Some(shell) => focus_in(tree, shell, cwd, depth + 1),
                    None => Outcome::NoTerminal,
                },
                Some(_) => Outcome::Failed("tmux clients nested too deeply".into()),
                None => Outcome::Failed(format!("no tmux client is attached to session {}", pane.session)),
            }
        }
        Located::VsCode => open_app("Visual Studio Code", "code", cwd),
        Located::Cursor => open_app("Cursor", "cursor", cwd),
        other => unsupported(&other, "focusing"),
    }
}

fn with_tty(tree: &Tree, pid: u32, action: impl FnOnce(&str) -> Outcome) -> Outcome {
    match tree.tty(pid) {
        Some(tty) => action(tty),
        None => Outcome::NoTerminal,
    }
}

/// `open -a <app> <cwd>` (macOS) or `<cli> <cwd>` (Linux) brings the window
/// with that folder forward (or opens one).
fn open_app(app: &str, cli: &str, cwd: Option<&str>) -> Outcome {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.args(["-a", app]);
        command
    } else {
        Command::new(cli)
    };
    command.args(cwd);
    match command.output() {
        Ok(out) if out.status.success() => Outcome::Done,
        Ok(out) => Outcome::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(err) => Outcome::Failed(err.to_string()),
    }
}

fn unsupported(located: &Located, what: &str) -> Outcome {
    let name = match located {
        Located::ITerm => "iTerm2",
        Located::TerminalApp => "Terminal.app",
        Located::Konsole(_) => "Konsole",
        Located::Tmux(_) => "tmux",
        Located::VsCode => "VS Code",
        Located::Cursor => "Cursor",
        Located::Unknown(name) => name,
    };
    Outcome::Unsupported(format!("{what} is not supported in {name}"))
}
