//! Claude Code's own background sessions (`claude --bg`), as `claude agents --json --all` lists
//! them, and the `claude stop` / `claude rm` commands that act on them.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::account::Account;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAgent {
    /// The short id `claude attach`, `stop` and `rm` take.
    pub id: String,
    pub session_id: String,
    pub name: Option<String>,
    pub cwd: Option<String>,
    /// `starting`, `running`, `working`, `idle`, `blocked`, `done`, `failed` or `stopped`.
    pub state: String,
    pub started_ms: Option<i64>,
    /// The process, while it runs. `done` is also what a session reports whose task finished
    /// while its process stays and waits for the next prompt: only without a pid has it exited.
    pub pid: Option<u32>,
}

impl BackgroundAgent {
    /// Whether the agent still runs (as opposed to done, failed or stopped and exited).
    pub fn is_active(&self) -> bool {
        self.pid.is_some() || !self.has_exited_state()
    }

    /// A state that ends a session once its process is gone.
    pub fn has_exited_state(&self) -> bool {
        matches!(self.state.as_str(), "done" | "failed" | "stopped")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    kind: Option<String>,
    id: Option<String>,
    session_id: Option<String>,
    name: Option<String>,
    cwd: Option<String>,
    state: Option<String>,
    started_at: Option<i64>,
    pid: Option<u32>,
}

/// The background sessions in `claude agents --json` output; interactive ones are left out
/// (Brain knows them from their status files).
pub fn parse(json: &str) -> Vec<BackgroundAgent> {
    serde_json::from_str::<Vec<Row>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.kind.as_deref() == Some("background"))
        .filter_map(|row| {
            Some(BackgroundAgent {
                id: row.id?,
                session_id: row.session_id?,
                name: row.name,
                cwd: row.cwd,
                state: row.state.unwrap_or_default(),
                started_ms: row.started_at,
                pid: row.pid,
            })
        })
        .collect()
}

/// `claude <args>` for an account: `CLAUDE_CONFIG_DIR` set for every account but `main`.
fn claude(account: &Account, args: &[&str]) -> Command {
    let mut command = Command::new("claude");
    command.args(args);
    if account.id == "main" {
        command.env_remove("CLAUDE_CONFIG_DIR");
    } else {
        command.env("CLAUDE_CONFIG_DIR", &account.config_dir);
    }
    command
}

/// All background sessions of an account, finished ones included; `None` when `claude` could
/// not be run.
pub fn list(account: &Account) -> Option<Vec<BackgroundAgent>> {
    let out = claude(account, &["agents", "--json", "--all"]).output().ok()?;
    out.status.success().then(|| parse(&String::from_utf8_lossy(&out.stdout)))
}

fn run(account: &Account, args: &[&str]) -> Result<(), String> {
    let out = claude(account, args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr }).trim().to_string();
    Err(crate::hook::one_line(&text, 200))
}

/// The short id in `claude --bg`'s answer: `backgrounded · 824489b9 · name (idle …)`.
pub fn started_id(output: &str) -> Option<String> {
    let line = output.lines().find(|l| l.trim_start().starts_with("backgrounded"))?;
    let id = line.split('·').nth(1)?.trim();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())).then(|| id.to_string())
}

/// What a new background session starts with, each optional.
#[derive(Debug, Clone, Default)]
pub struct Start {
    pub model: Option<String>,
    pub name: Option<String>,
    /// Its output style (`--settings {"outputStyle": …}`).
    pub style: Option<String>,
    pub prompt: Option<String>,
    /// As an agent team, led by this agent.
    pub team: Option<TeamStart>,
    /// The conversation it continues (`--resume <session id>`).
    pub resume: Option<String>,
}

/// An agent team (experimental in Claude Code): the lead agent and a plugin folder to load with
/// it (a team chat), as `claude --agent <lead>` with agent teams turned on.
#[derive(Debug, Clone)]
pub struct TeamStart {
    pub lead: String,
    pub plugin_dir: Option<std::path::PathBuf>,
}

/// Starts a background session in `cwd` (`claude --bg`); returns its short id, or what `claude`
/// said (e.g. an untrusted folder). An agent team needs an interactive session: see
/// [`start_team`].
pub fn start(account: &Account, cwd: &Path, options: &Start) -> Result<String, String> {
    let mut args: Vec<String> = vec!["--bg".into()];
    args.extend(start_args(options));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut command = claude(account, &args);
    if options.team.is_some() {
        command.envs(TEAM_ENV);
    }
    started(command.current_dir(cwd))
}

/// What `claude-team` sets: agent teams on, the security plugin's stop review off.
const TEAM_ENV: [(&str, &str); 2] = [("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1"), ("ENABLE_STOP_REVIEW", "0")];

/// tmux sessions Brain starts an agent team in are named `brain-team-<when>`.
pub const TEAM_TMUX_PREFIX: &str = "brain-team-";

/// Starts an agent team: Claude Code makes teammates only in an interactive session, so it
/// runs in a detached tmux session (`brain-team-…`), Claude itself as the pane's program, and
/// goes on when Brain quits. Returns the tmux session's name.
pub fn start_team(account: &Account, cwd: &Path, options: &Start) -> Result<String, String> {
    let name = format!("{TEAM_TMUX_PREFIX}{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let mut command = Command::new("tmux");
    command.args(["new-session", "-d", "-s", &name, "-x", "200", "-y", "50", "-c"]).arg(cwd);
    if account.id != "main" {
        command.arg("-e").arg(format!("CLAUDE_CONFIG_DIR={}", account.config_dir.display()));
    }
    for (key, value) in TEAM_ENV {
        command.arg("-e").arg(format!("{key}={value}"));
    }
    // Several arguments: tmux runs them as they are, no shell in between.
    command.arg("claude").args(start_args(options));
    let out = command.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        // The largest client sizes the window: Brain's terminal joins it hidden too (for a reply
        // from the phone), and a hidden one is tiny.
        let _ = Command::new("tmux").args(["set-window-option", "-t", &name, "window-size", "largest"]).output();
        Ok(name)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The panes of Brain's team sessions in tmux: the pid of the program each runs (Claude) and
/// the tmux session's name. Empty without tmux or without such sessions.
pub fn team_panes() -> std::collections::HashMap<u32, String> {
    // A space between them: tmux writes a tab in a format out escaped. Brain's names have none.
    let Ok(out) = Command::new("tmux").args(["list-panes", "-a", "-F", "#{session_name} #{pane_pid}"]).output() else {
        return Default::default();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (session, pid) = line.rsplit_once(' ')?;
            session.starts_with(TEAM_TMUX_PREFIX).then(|| Some((pid.trim().parse().ok()?, session.to_string()))).flatten()
        })
        .collect()
}

/// The arguments for a new session's options, after `--bg` or as an interactive one.
fn start_args(options: &Start) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if let Some(session_id) = &options.resume {
        args.extend(["--resume".into(), session_id.clone()]);
    }
    if let Some(model) = &options.model {
        args.extend(["--model".into(), model.clone()]);
    }
    if let Some(name) = &options.name {
        args.extend(["-n".into(), name.clone()]);
    }
    if let Some(style) = &options.style {
        args.extend(["--settings".into(), crate::styles::settings(style)]);
    }
    if let Some(team) = &options.team {
        args.extend(["--agent".into(), team.lead.clone()]);
        // A flag, not CLAUDE_CODE_PLUGIN_DIRS: the settings' value of that (Brain's mod) wins.
        if let Some(dir) = &team.plugin_dir {
            args.extend(["--plugin-dir".into(), dir.display().to_string()]);
        }
    }
    // After `--` the prompt is never an option, even one that starts with `-` (a task for later,
    // a prompt from the phone).
    if let Some(prompt) = options.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
        args.extend(["--".into(), prompt.to_string()]);
    }
    args
}

/// Continues an existing session in the background under its id (`claude --bg --resume`),
/// keeping its name.
pub fn resume(account: &Account, cwd: &Path, session_id: &str, name: Option<&str>) -> Result<String, String> {
    resume_with(account, cwd, session_id, name, &[])
}

/// [`resume`] with extra flags for Claude (`--chrome`, `--model opus`), passed as arguments.
pub fn resume_with(account: &Account, cwd: &Path, session_id: &str, name: Option<&str>, extra: &[String]) -> Result<String, String> {
    let mut args = vec!["--bg", "--resume", session_id];
    if let Some(name) = name {
        args.extend(["-n", name]);
    }
    args.extend(extra.iter().map(String::as_str));
    started(claude(account, &args).current_dir(cwd))
}

/// Continues a copy of a session in the background under a new id (`--fork-session`): how a
/// conversation moves to another account, whose transcript folder it was copied into.
pub fn fork(account: &Account, cwd: &Path, session_id: &str, name: Option<&str>) -> Result<String, String> {
    let mut args = vec!["--bg", "--resume", session_id, "--fork-session"];
    if let Some(name) = name {
        args.extend(["-n", name]);
    }
    started(claude(account, &args).current_dir(cwd))
}

fn started(command: &mut Command) -> Result<String, String> {
    let out = command.output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    started_id(&text).ok_or_else(|| crate::hook::one_line(text.trim(), 240))
}

/// Stops a background session; its conversation is kept.
pub fn stop(account: &Account, id: &str) -> Result<(), String> {
    run(account, &["stop", id])
}

/// Deletes a background session (and its worktree when that is safe).
pub fn remove(account: &Account, id: &str) -> Result<(), String> {
    run(account, &["rm", id])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_id_from_the_background_start_message() {
        let out = "backgrounded · 824489b9 · bg-empty-test (idle — send a prompt to start)\n  claude agents             list sessions\n";
        assert_eq!(started_id(out).as_deref(), Some("824489b9"));
        assert_eq!(started_id("Workspace not trusted. Run `claude` in /tmp once"), None);
    }

    #[test]
    fn keeps_background_rows_with_ids_only() {
        let json = r#"[
            {"id":"1f82d2ba","cwd":"/w/app","kind":"background","startedAt":1782809377133,"sessionId":"1f82d2ba-c668","name":"Fix banking","state":"blocked"},
            {"pid":4200,"cwd":"/w/fin","kind":"interactive","sessionId":"0331","name":"fin","status":"idle"},
            {"cwd":"/w/x","kind":"background","state":"done"},
            {"id":"8d1d90d3","pid":1568,"kind":"background","sessionId":"8d1d90d3-2a87","state":"done","status":"idle"},
            {"id":"3fc9d989","kind":"background","sessionId":"3fc9d989-4f87","state":"done"}
        ]"#;

        let agents = parse(json);

        assert_eq!(agents.len(), 3);
        assert_eq!((agents[0].id.as_str(), agents[0].session_id.as_str(), agents[0].state.as_str()), ("1f82d2ba", "1f82d2ba-c668", "blocked"));
        assert!(agents[0].is_active());
        // Done with a process: it waits for the next prompt. Done without one: it exited.
        assert!(agents[1].is_active());
        assert!(!agents[2].is_active());
        assert!(parse("not json").is_empty());
    }
}
