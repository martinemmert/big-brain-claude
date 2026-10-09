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
}

impl BackgroundAgent {
    /// Whether the agent still runs (as opposed to done, failed or stopped).
    pub fn is_active(&self) -> bool {
        !matches!(self.state.as_str(), "done" | "failed" | "stopped")
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

/// Starts a background session in `cwd` (`claude --bg`), with an optional model, name and first
/// prompt; returns its short id, or what `claude` said (e.g. an untrusted folder).
pub fn start(account: &Account, cwd: &Path, model: Option<&str>, name: Option<&str>, prompt: Option<&str>) -> Result<String, String> {
    let mut args = vec!["--bg"];
    if let Some(model) = model {
        args.extend(["--model", model]);
    }
    if let Some(name) = name {
        args.extend(["-n", name]);
    }
    if let Some(prompt) = prompt.filter(|p| !p.trim().is_empty()) {
        args.push(prompt);
    }
    started(claude(account, &args).current_dir(cwd))
}

/// Continues an existing session in the background under its id (`claude --bg --resume`),
/// keeping its name.
pub fn resume(account: &Account, cwd: &Path, session_id: &str, name: Option<&str>) -> Result<String, String> {
    let mut args = vec!["--bg", "--resume", session_id];
    if let Some(name) = name {
        args.extend(["-n", name]);
    }
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
            {"cwd":"/w/x","kind":"background","state":"done"}
        ]"#;

        let agents = parse(json);

        assert_eq!(agents.len(), 1);
        assert_eq!((agents[0].id.as_str(), agents[0].session_id.as_str(), agents[0].state.as_str()), ("1f82d2ba", "1f82d2ba-c668", "blocked"));
        assert!(agents[0].is_active());
        assert!(parse("not json").is_empty());
    }
}
