//! A session's agents (an agent team's members, subagents) and what they write each other, as
//! Brain's mod sees them inside Claude Code (`$.agent.list()`, `session.send`/`receive`).
//!
//! `~/.claude-brain/teams/<session id>.json`, written again whenever something changes; only the
//! user can read it (it holds the messages).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Team {
    #[serde(default)]
    pub members: Vec<Member>,
    /// Oldest first; the mod keeps the last 200.
    #[serde(default)]
    pub messages: Vec<TeamMessage>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    /// The agent type it was spawned as (`planer`, `Explore`, `teammate` …).
    #[serde(default)]
    pub r#type: String,
    /// `pending`, `running`, `waiting`, `idle` (a teammate waiting for a message), `completed`,
    /// `failed`, `killed`.
    #[serde(default)]
    pub status: String,
    /// `name@team` for a member of an agent team, none for a subagent.
    #[serde(default)]
    pub teammate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TeamMessage {
    pub id: String,
    /// `lead` for the main conversation.
    pub from: String,
    pub to: String,
    pub text: String,
    /// Epoch milliseconds.
    pub at: i64,
    /// "Frage an Martin: …": a question for the person.
    #[serde(default)]
    pub question: bool,
    /// The lead wrote to its sender since.
    #[serde(default)]
    pub answered: bool,
}

impl Team {
    /// The questions for the person nobody answered yet, oldest first.
    pub fn open_questions(&self) -> impl Iterator<Item = &TeamMessage> {
        self.messages.iter().filter(|m| m.question && !m.answered)
    }

    /// Members still at work or waiting for a message, not the finished ones.
    pub fn active(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| !matches!(m.status.as_str(), "completed" | "failed" | "killed"))
    }
}

fn file(home: &Path, session_id: &str) -> Option<PathBuf> {
    let plain = !session_id.is_empty() && session_id.len() <= 64 && session_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    plain.then(|| home.join(".claude-brain/teams").join(format!("{session_id}.json")))
}

/// Keeps a session's team; an id that isn't a plain session id is refused.
pub fn save(home: &Path, session_id: &str, team: &Team) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let Some(file) = file(home, session_id) else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "session id"));
    };
    if let Some(dir) = file.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    let partial = file.with_extension("json.part");
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&partial)?.write_all(&serde_json::to_vec(team)?)?;
    std::fs::rename(partial, file)
}

/// A session's team, none while its mod hasn't seen an agent.
pub fn load(home: &Path, session_id: &str) -> Option<Team> {
    let bytes = std::fs::read(file(home, session_id)?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, from: &str, question: bool, answered: bool) -> TeamMessage {
        TeamMessage { id: id.into(), from: from.into(), to: "lead".into(), text: "…".into(), at: 1, question, answered }
    }

    #[test]
    fn kept_private_per_session_with_open_questions_and_active_members() {
        let home = tempfile::tempdir().unwrap();
        let member = |name: &str, status: &str| Member { id: name.into(), name: name.into(), r#type: "planer".into(), status: status.into(), teammate: Some(format!("{name}@t")) };
        let team = Team {
            members: vec![member("planer", "running"), member("bote", "completed"), member("tester", "idle")],
            messages: vec![message("1", "planer", true, false), message("2", "tester", false, false), message("3", "bote", true, true)],
        };
        save(home.path(), "5c76ded6-b8dd", &team).unwrap();
        let loaded = load(home.path(), "5c76ded6-b8dd").unwrap();
        assert_eq!(loaded, team);
        assert_eq!(loaded.open_questions().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["1"]);
        assert_eq!(loaded.active().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["planer", "tester"]);
        assert!(load(home.path(), "missing").is_none());
        assert!(save(home.path(), "../x", &team).is_err());
        use std::os::unix::fs::PermissionsExt;
        let file = home.path().join(".claude-brain/teams/5c76ded6-b8dd.json");
        assert_eq!(std::fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
