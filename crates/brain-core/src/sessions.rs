use serde::Deserialize;

use crate::account::Account;

/// The status file Claude Code itself keeps at `<config>/sessions/<pid>.json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub pid: u32,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub name: Option<String>,
    /// `busy`, `idle`, `waiting`, `shell`, …
    pub status: Option<String>,
    /// Unix epoch milliseconds.
    pub started_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub status_updated_at: Option<i64>,
}

pub fn read_session_files(account: &Account) -> Vec<SessionFile> {
    std::fs::read_dir(account.sessions_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| std::fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect()
}
