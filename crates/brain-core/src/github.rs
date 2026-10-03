//! The pull request of a session's branch and its checks, via the GitHub CLI (`gh pr view`).

use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    Passing,
    Failing,
    Pending,
    /// The PR has no checks.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    /// `OPEN`, `MERGED` or `CLOSED`.
    pub state: String,
    pub draft: bool,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, or none.
    pub review: Option<String>,
    pub checks: Checks,
    /// Names of the checks that failed.
    pub failing: Vec<String>,
}

const FIELDS: &str = "number,url,state,isDraft,reviewDecision,statusCheckRollup";

/// The PR whose head is `branch`, asked in `repo_dir` so `gh` picks the right repository.
/// `None` without a PR, outside GitHub, or when `gh` is missing or not logged in.
pub fn pull_request(repo_dir: &Path, branch: &str) -> Option<PullRequest> {
    let out = Command::new("gh")
        .args(["pr", "view", branch, "--json", FIELDS])
        .current_dir(repo_dir)
        .output()
        .ok()?;
    out.status.success().then(|| parse(&String::from_utf8_lossy(&out.stdout))).flatten()
}

pub fn parse(json: &str) -> Option<PullRequest> {
    let v: Value = serde_json::from_str(json).ok()?;
    let checks: Vec<&Value> = v.get("statusCheckRollup").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let field = |c: &Value, k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default().to_string();

    let mut failing = Vec::new();
    let mut pending = false;
    for check in &checks {
        // Check runs have status + conclusion, commit status contexts a state.
        let conclusion = field(check, "conclusion");
        let state = field(check, "state");
        let status = field(check, "status");
        let failed = matches!(conclusion.as_str(), "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE")
            || matches!(state.as_str(), "FAILURE" | "ERROR");
        if failed {
            let name = field(check, "name");
            failing.push(if name.is_empty() { field(check, "context") } else { name });
        } else if (!status.is_empty() && status != "COMPLETED") || matches!(state.as_str(), "PENDING" | "EXPECTED") {
            pending = true;
        }
    }
    let checks_state = if !failing.is_empty() {
        Checks::Failing
    } else if pending {
        Checks::Pending
    } else if checks.is_empty() {
        Checks::None
    } else {
        Checks::Passing
    };
    Some(PullRequest {
        number: v.get("number")?.as_u64()?,
        url: field(&v, "url"),
        state: field(&v, "state"),
        draft: v.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        review: Some(field(&v, "reviewDecision")).filter(|r| !r.is_empty()),
        checks: checks_state,
        failing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_up_check_runs_and_status_contexts() {
        // Shape as returned by `gh pr view --json statusCheckRollup` (gh 2.83).
        let passing = r#"{"number":14583,"url":"https://github.com/cli/cli/pull/14583","state":"OPEN","isDraft":false,"reviewDecision":"REVIEW_REQUIRED",
            "statusCheckRollup":[{"__typename":"CheckRun","name":"lint","status":"COMPLETED","conclusion":"SUCCESS"},
                                 {"__typename":"CheckRun","name":"label","status":"COMPLETED","conclusion":"SKIPPED"}]}"#;
        let pr = parse(passing).unwrap();
        assert_eq!((pr.number, pr.checks, pr.review.as_deref()), (14583, Checks::Passing, Some("REVIEW_REQUIRED")));

        let mixed = r#"{"number":2,"url":"u","state":"OPEN","isDraft":true,"reviewDecision":"",
            "statusCheckRollup":[{"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE"},
                                 {"__typename":"CheckRun","name":"build","status":"IN_PROGRESS","conclusion":""},
                                 {"__typename":"StatusContext","context":"ci/legacy","state":"ERROR"}]}"#;
        let pr = parse(mixed).unwrap();
        assert_eq!(pr.checks, Checks::Failing);
        assert_eq!(pr.failing, vec!["test", "ci/legacy"]);
        assert!(pr.draft && pr.review.is_none());

        let running = r#"{"number":3,"url":"u","state":"OPEN","isDraft":false,"statusCheckRollup":[{"status":"QUEUED","conclusion":""}]}"#;
        assert_eq!(parse(running).unwrap().checks, Checks::Pending);
        let none = r#"{"number":4,"url":"u","state":"MERGED","isDraft":false,"statusCheckRollup":[]}"#;
        assert_eq!(parse(none).unwrap().checks, Checks::None);
    }
}
