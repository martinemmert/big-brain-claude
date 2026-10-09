use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// Where an event came from: a Claude Code hook or an explicit `brain report`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Hook,
    Report,
    /// Brain's Claude Code mod, which sends the same events from inside Claude Code.
    Mod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SessionStart,
    /// The user submitted a prompt; a new turn starts.
    Prompt,
    /// Claude Code needs permission or input (`Notification` hook).
    Permission,
    /// The turn ended (`Stop` hook).
    Stop,
    SessionEnd,
    Doing,
    Waiting,
    Done,
}

/// Work a session left running when its turn ended (Stop hook `background_tasks`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: String,
    /// `shell`, `subagent`, `monitor`, `workflow`, … (see Claude Code's hooks docs).
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
}

/// One line in `~/.claude-brain/events/YYYY-MM-DD.jsonl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub v: u32,
    pub ts: DateTime<Utc>,
    pub account: String,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub source: Source,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// On stop events: what still runs in the background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<BackgroundTask>>,
    /// The session's name at the time (Claude Code's own or set with `/rename`), so ended
    /// sessions keep it after their status file is gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
