//! Plan limits, context fill and cost, taken from the JSON Claude Code passes to the status line
//! (documented in Claude Code's statusline docs: `rate_limits`, `context_window`, `cost`).
//! `brain statusline` stores one snapshot per session in `~/.claude-brain/status/`, and every ten
//! minutes at most the weekly limit's state in `~/.claude-brain/usage-history/`: the forecast
//! goes by the pace of the last 24 hours, which takes knowing where the limit stood then.

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

    /// When the limit will be used up if the pace of the last 24 hours goes on (Unix seconds).
    /// The pace runs from where the limit stood a day ago in this window, or, early in a window
    /// or after a gap in the history, from its oldest state at least six hours back; without
    /// one it is the whole window's ([`Limit::runs_out_at`]). Nothing used since: `None`.
    pub fn runs_out_at_recent(&self, window_secs: i64, now_secs: i64, history: &[Sample]) -> Option<i64> {
        let resets_at = self.resets_at?;
        if self.used_percentage >= 100.0 {
            return Some(now_secs);
        }
        let window = history.iter().filter(|s| s.resets_at == resets_at && s.ts <= now_secs);
        let day_ago = window.clone().filter(|s| s.ts <= now_secs - RECENT).max_by_key(|s| s.ts);
        let since = day_ago.or_else(|| window.filter(|s| s.ts <= now_secs - RECENT_AT_LEAST).min_by_key(|s| s.ts));
        let Some(since) = since else { return self.runs_out_at(window_secs, now_secs) };
        let rate = (self.used_percentage - since.used_percentage) / (now_secs - since.ts) as f64;
        if rate <= 0.0 {
            return None;
        }
        let at = now_secs + ((100.0 - self.used_percentage) / rate) as i64;
        (at < resets_at).then_some(at)
    }
}

/// The span the forecast's pace covers, and the least history it needs for one.
pub const RECENT: i64 = 24 * 3600;
const RECENT_AT_LEAST: i64 = 6 * 3600;
/// The history keeps a state every ten minutes at most, for eight days (a window and a bit).
const SAMPLE_EVERY: std::time::Duration = std::time::Duration::from_secs(600);
const KEEP: i64 = 8 * 24 * 3600;

/// Where the weekly limit stood at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Unix seconds.
    pub ts: i64,
    pub used_percentage: f64,
    /// Which window it belongs to (Unix seconds of its reset).
    pub resets_at: i64,
}

pub fn history_dir(home: &Path) -> PathBuf {
    home.join(".claude-brain/usage-history")
}

fn history_file(dir: &Path, account: &str) -> PathBuf {
    dir.join(format!("{account}.jsonl"))
}

/// Adds the snapshot's weekly state to the account's history, unless the last one is less
/// than ten minutes old (the status line runs every few seconds). Drops states older than
/// eight days now and then.
pub fn record_history(dir: &Path, snapshot: &Snapshot) -> std::io::Result<()> {
    let Some(Limit { used_percentage, resets_at: Some(resets_at) }) = snapshot.seven_day else { return Ok(()) };
    let path = history_file(dir, &snapshot.account);
    let recent = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < SAMPLE_EVERY);
    if recent {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let ts = snapshot.ts / 1000;
    let mut kept: Vec<Sample> = read_history(dir, &snapshot.account).into_iter().filter(|s| s.ts > ts - KEEP).collect();
    kept.push(Sample { ts, used_percentage, resets_at });
    let lines: String = kept.iter().filter_map(|s| serde_json::to_string(s).ok()).map(|l| l + "\n").collect();
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, lines)?;
    std::fs::rename(tmp, path)
}

/// An account's weekly states, oldest first; unreadable lines are skipped.
pub fn read_history(dir: &Path, account: &str) -> Vec<Sample> {
    std::fs::read_to_string(history_file(dir, account))
        .map(|text| text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
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
    fn the_forecast_goes_by_the_last_24_hours() {
        let resets_at = 1_000_000;
        let start = resets_at - SEVEN_DAYS;
        let now = start + 3 * 86_400;
        let sample = |days_ago: f64, used: f64| Sample { ts: now - (days_ago * 86_400.0) as i64, used_percentage: used, resets_at };
        // 60 % after three days, but only 6 % of it in the last day: lasts at today's pace,
        // where the whole window's pace would run out after five days.
        let limit = Limit { used_percentage: 60.0, resets_at: Some(resets_at) };
        let calm = [sample(2.0, 40.0), sample(1.0, 54.0), sample(0.5, 57.0)];
        assert_eq!(limit.runs_out_at_recent(SEVEN_DAYS, now, &calm), None);
        // 30 % in the last day: the remaining 40 % last another 32 hours.
        let busy = [sample(1.2, 25.0), sample(1.0, 30.0)];
        assert_eq!(limit.runs_out_at_recent(SEVEN_DAYS, now, &busy), Some(now + 32 * 3600));
        // Only eight hours of history: their pace (16 % in 8 h, 2 %/h) gives 20 hours.
        let young = [sample(8.0 / 24.0, 44.0)];
        assert_eq!(limit.runs_out_at_recent(SEVEN_DAYS, now, &young), Some(now + 20 * 3600));
        // Less than six hours, or another window's states: the whole window's pace.
        let fresh = [sample(0.1, 59.0), Sample { resets_at: resets_at - SEVEN_DAYS, ..sample(1.0, 10.0) }];
        assert_eq!(limit.runs_out_at_recent(SEVEN_DAYS, now, &fresh), limit.runs_out_at(SEVEN_DAYS, now));
        // Nothing used in the last day: no forecast.
        assert_eq!(limit.runs_out_at_recent(SEVEN_DAYS, now, &[sample(1.0, 60.0)]), None);
    }

    #[test]
    fn the_history_keeps_a_state_every_ten_minutes_for_eight_days() {
        let dir = tempfile::tempdir().unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = |used: f64, ts_ms: i64| Snapshot {
            seven_day: Some(Limit { used_percentage: used, resets_at: Some(2_000_000_000) }),
            ..Snapshot::from_statusline("main", 1, ts_ms, &json!({}))
        };
        record_history(dir.path(), &snapshot(10.0, now_ms)).unwrap();
        // A minute later the file is fresh: nothing added.
        record_history(dir.path(), &snapshot(11.0, now_ms + 60_000)).unwrap();
        assert_eq!(read_history(dir.path(), "main").len(), 1);
        // Once the last state is older than ten minutes, the next one is added, and states
        // older than eight days go.
        let old = Sample { ts: now_ms / 1000 - 9 * 86_400, used_percentage: 1.0, resets_at: 1 };
        let file = dir.path().join("main.jsonl");
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, format!("{}\n{text}", serde_json::to_string(&old).unwrap())).unwrap();
        let eleven_minutes_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(660);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(eleven_minutes_ago).unwrap();
        record_history(dir.path(), &snapshot(12.0, now_ms + 700_000)).unwrap();
        let kept: Vec<f64> = read_history(dir.path(), "main").iter().map(|s| s.used_percentage).collect();
        assert_eq!(kept, vec![10.0, 12.0]);
    }

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
