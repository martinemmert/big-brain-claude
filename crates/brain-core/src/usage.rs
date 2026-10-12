//! Plan limits, context fill and cost, taken from the JSON Claude Code passes to the status line
//! (documented in Claude Code's statusline docs: `rate_limits`, `context_window`, `cost`).
//! `brain statusline` stores one snapshot per session in `~/.claude-brain/status/`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A plan limit window, e.g. the five-hour or the seven-day one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    pub used_percentage: f64,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub account: String,
    pub pid: u32,
    pub session_id: Option<String>,
    /// When the status line last ran (epoch ms).
    pub ts: i64,
    pub context_percent: Option<f64>,
    pub context_size: Option<u64>,
    pub cost_usd: Option<f64>,
    pub five_hour: Option<Limit>,
    pub seven_day: Option<Limit>,
}

impl Snapshot {
    /// Picks the fields Brain uses from the status line JSON.
    pub fn from_statusline(account: &str, pid: u32, ts: i64, input: &Value) -> Self {
        let num = |path: &[&str]| path.iter().try_fold(input, |v, k| v.get(*k)).and_then(Value::as_f64);
        let limit = |kind: &str| {
            let used = num(&["rate_limits", kind, "used_percentage"])?;
            let resets_at = num(&["rate_limits", kind, "resets_at"]).map(|r| r as i64);
            Some(Limit { used_percentage: used, resets_at })
        };
        Self {
            account: account.to_string(),
            pid,
            session_id: input.get("session_id").and_then(Value::as_str).map(str::to_string),
            ts,
            context_percent: num(&["context_window", "used_percentage"]),
            context_size: num(&["context_window", "context_window_size"]).map(|n| n as u64),
            cost_usd: num(&["cost", "total_cost_usd"]),
            five_hour: limit("five_hour"),
            seven_day: limit("seven_day"),
        }
    }
}

/// The five-hour and the seven-day window, in seconds.
pub const FIVE_HOURS: i64 = 5 * 3600;
pub const SEVEN_DAYS: i64 = 7 * 24 * 3600;

impl Limit {
    /// When the limit will be used up if the pace so far in this window goes on (Unix seconds);
    /// `None` when it lasts until the reset, or the window is too young to tell (under a tenth).
    pub fn runs_out_at(&self, window_secs: i64, now_secs: i64) -> Option<i64> {
        let resets_at = self.resets_at?;
        let start = resets_at - window_secs;
        let elapsed = now_secs - start;
        if elapsed * 10 < window_secs || self.used_percentage <= 0.0 {
            return None;
        }
        if self.used_percentage >= 100.0 {
            return Some(now_secs);
        }
        let at = start + (elapsed as f64 * 100.0 / self.used_percentage) as i64;
        (at < resets_at).then_some(at)
    }
}

pub fn status_dir(home: &Path) -> PathBuf {
    home.join(".claude-brain/status")
}

fn file_for(dir: &Path, account: &str, pid: u32) -> PathBuf {
    dir.join(format!("{account}-{pid}.json"))
}

/// Replaces the session's snapshot (write to a temp file, then rename, so readers never see half).
pub fn store(dir: &Path, snapshot: &Snapshot) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = file_for(dir, &snapshot.account, snapshot.pid);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(snapshot)?)?;
    std::fs::rename(tmp, path)
}

pub fn read_all(dir: &Path) -> Vec<Snapshot> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect()
}

/// The plan limits of an account: the newest snapshot that has them (limits are per account,
/// every session reports the same numbers).
pub fn account_limits<'a>(snapshots: &'a [Snapshot], account: &str) -> Option<&'a Snapshot> {
    snapshots
        .iter()
        .filter(|s| s.account == account && (s.five_hour.is_some() || s.seven_day.is_some()))
        .max_by_key(|s| s.ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_runs_out_when_the_pace_outlasts_the_window() {
        let resets_at = 1_000_000;
        let start = resets_at - SEVEN_DAYS;
        // Three of seven days gone, 60 % used: at this pace 100 % after five days.
        let fast = Limit { used_percentage: 60.0, resets_at: Some(resets_at) };
        assert_eq!(fast.runs_out_at(SEVEN_DAYS, start + 3 * 86_400), Some(start + 5 * 86_400));
        // 30 % after three days lasts.
        let slow = Limit { used_percentage: 30.0, resets_at: Some(resets_at) };
        assert_eq!(slow.runs_out_at(SEVEN_DAYS, start + 3 * 86_400), None);
        // Half a day in, too early to tell.
        assert_eq!(fast.runs_out_at(SEVEN_DAYS, start + 43_200), None);
        // Used up already.
        let full = Limit { used_percentage: 100.0, resets_at: Some(resets_at) };
        assert_eq!(full.runs_out_at(SEVEN_DAYS, start + 4 * 86_400), Some(start + 4 * 86_400));
    }
    use serde_json::json;

    #[test]
    fn reads_the_documented_status_line_fields_and_finds_the_newest_limits() {
        let input = json!({
            "session_id": "s1",
            "context_window": { "used_percentage": 42.5, "context_window_size": 200000 },
            "cost": { "total_cost_usd": 1.25 },
            "rate_limits": {
                "five_hour": { "used_percentage": 30.0, "resets_at": 1790000000 },
                "seven_day": { "used_percentage": 12.0, "resets_at": 1790500000 }
            }
        });
        let newer = Snapshot::from_statusline("main", 7, 2000, &input);
        assert_eq!(newer.context_percent, Some(42.5));
        assert_eq!(newer.five_hour, Some(Limit { used_percentage: 30.0, resets_at: Some(1_790_000_000) }));

        let older = Snapshot { ts: 1000, five_hour: Some(Limit { used_percentage: 5.0, resets_at: None }), ..newer.clone() };
        let without = Snapshot::from_statusline("main", 8, 3000, &json!({ "session_id": "s2" }));
        let all = vec![older, newer.clone(), without];

        assert_eq!(account_limits(&all, "main"), Some(&newer));
        assert_eq!(account_limits(&all, "second"), None);
    }

    #[test]
    fn snapshots_round_trip_through_the_status_folder() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = Snapshot::from_statusline("second", 9, 5, &json!({ "cost": { "total_cost_usd": 0.5 } }));
        store(dir.path(), &snapshot).unwrap();
        store(dir.path(), &snapshot).unwrap();

        assert_eq!(read_all(dir.path()), vec![snapshot]);
    }
}
