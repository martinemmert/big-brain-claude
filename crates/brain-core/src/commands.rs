//! The slash commands a session offers (built-in, skills, plugins, MCP prompts), as Brain's mod
//! reads them inside Claude Code (`$.command.list()`), for completing `/…` while typing a reply.
//!
//! `~/.claude-brain/commands/<session id>.json`, written again at each session start.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlashCommand {
    /// Without the slash (`compact`, `fux`, `phoenix-framework:px`).
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// `builtin`, `plugin`, `user` or `mcp`.
    #[serde(default)]
    pub source: String,
}

fn file(home: &Path, session_id: &str) -> Option<PathBuf> {
    let plain = !session_id.is_empty() && session_id.len() <= 64 && session_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    plain.then(|| home.join(".claude-brain/commands").join(format!("{session_id}.json")))
}

/// Keeps a session's commands; an id that isn't a plain session id is refused.
pub fn save(home: &Path, session_id: &str, commands: &[SlashCommand]) -> std::io::Result<()> {
    let Some(file) = file(home, session_id) else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "session id"));
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = file.with_extension("json.part");
    std::fs::write(&partial, serde_json::to_vec(commands)?)?;
    std::fs::rename(partial, file)
}

/// A session's commands, empty while its mod hasn't reported them.
pub fn load(home: &Path, session_id: &str) -> Vec<SlashCommand> {
    let Some(file) = file(home, session_id) else { return Vec::new() };
    std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// The commands that complete what was typed after `/`: names starting with it first, then
/// those containing it, each group in the session's order.
pub fn completions<'a>(commands: &'a [SlashCommand], typed: &str) -> Vec<&'a SlashCommand> {
    let typed = typed.trim_start_matches('/').to_lowercase();
    let (mut starts, mut contains) = (Vec::new(), Vec::new());
    for command in commands {
        let name = command.name.to_lowercase();
        if name.starts_with(&typed) {
            starts.push(command);
        } else if name.contains(&typed) {
            contains.push(command);
        }
    }
    starts.extend(contains);
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str) -> SlashCommand {
        SlashCommand { name: name.into(), description: String::new(), source: "builtin".into() }
    }

    #[test]
    fn kept_per_session_and_completed_by_prefix_first() {
        let home = tempfile::tempdir().unwrap();
        let list = vec![command("compact"), command("phoenix-framework:px"), command("px-fe"), command("context")];
        save(home.path(), "a41a39e3-6b03", &list).unwrap();
        assert_eq!(load(home.path(), "a41a39e3-6b03"), list);
        assert!(load(home.path(), "other").is_empty());
        assert!(save(home.path(), "../x", &list).is_err());

        let names = |typed: &str| completions(&list, typed).iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert_eq!(names("/co"), ["compact", "context"]);
        assert_eq!(names("px"), ["px-fe", "phoenix-framework:px"]);
    }
}
