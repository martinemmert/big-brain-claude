//! What a session changed in its working directory, from git.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    /// Two-letter `git status --porcelain` code, e.g. ` M`, `A `, `??`.
    pub status: String,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub branch: Option<String>,
    /// Commits ahead of and behind the upstream, if there is one.
    pub ahead_behind: Option<(u64, u64)>,
    pub files: Vec<FileChange>,
}

impl Changes {
    pub fn totals(&self) -> (u64, u64) {
        self.files.iter().fold((0, 0), |(a, r), f| (a + f.added.unwrap_or(0), r + f.removed.unwrap_or(0)))
    }
}

fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(cwd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `None` outside a git repository.
pub fn changes_of(cwd: &Path) -> Option<Changes> {
    let status = git(cwd, &["status", "--porcelain=v1", "--untracked-files=normal"])?;
    let numstat = git(cwd, &["diff", "--numstat", "HEAD"]).unwrap_or_default();
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|b| b.trim().to_string());
    let ahead_behind = git(cwd, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"])
        .and_then(|out| parse_ahead_behind(&out));
    Some(Changes { branch, ahead_behind, files: parse(&status, &numstat) })
}

/// Joins `git status --porcelain=v1` with `git diff --numstat HEAD` by path.
pub fn parse(status: &str, numstat: &str) -> Vec<FileChange> {
    let mut counts: BTreeMap<String, (Option<u64>, Option<u64>)> = BTreeMap::new();
    for line in numstat.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next()) else { continue };
        // Binary files show `-`; renames show `old => new`.
        let path = path.rsplit(" => ").next().unwrap_or(path).trim_end_matches('}').to_string();
        counts.insert(path, (added.parse().ok(), removed.parse().ok()));
    }
    status
        .lines()
        .filter(|line| line.len() > 3)
        .map(|line| {
            let code = line[..2].to_string();
            let path = line[3..].rsplit(" -> ").next().unwrap_or(&line[3..]).trim_matches('"').to_string();
            let (added, removed) = counts.get(&path).copied().unwrap_or((None, None));
            FileChange { path, status: code, added, removed }
        })
        .collect()
}

/// `git rev-list --left-right --count @{upstream}...HEAD` prints `behind<TAB>ahead`.
fn parse_ahead_behind(out: &str) -> Option<(u64, u64)> {
    let mut parts = out.split_whitespace();
    let behind = parts.next()?.parse().ok()?;
    let ahead = parts.next()?.parse().ok()?;
    Some((ahead, behind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_status_and_line_counts_including_renames_and_untracked_files() {
        let status = " M src/view.rs\nA  src/new.rs\nR  old.rs -> renamed.rs\n?? notes.txt\n";
        let numstat = "12\t3\tsrc/view.rs\n40\t0\tsrc/new.rs\n0\t0\told.rs => renamed.rs\n";

        let files = parse(status, numstat);

        assert_eq!(files.len(), 4);
        assert_eq!((files[0].path.as_str(), files[0].added, files[0].removed), ("src/view.rs", Some(12), Some(3)));
        assert_eq!(files[2].path, "renamed.rs");
        assert_eq!((files[3].status.as_str(), files[3].added), ("??", None));
        assert_eq!(Changes { files, ..Changes::default() }.totals(), (52, 3));
        assert_eq!(parse_ahead_behind("2\t5\n"), Some((5, 2)));
    }

    #[test]
    fn reads_a_real_repository() {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| Command::new("git").arg("-C").arg(dir.path()).args(args).output().unwrap();
        run(&["init", "-q"]);
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        run(&["add", "."]);
        run(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"]);
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new\n").unwrap();

        let changes = changes_of(dir.path()).unwrap();

        assert_eq!(changes.files.len(), 2);
        assert_eq!(changes.totals(), (1, 0));
        assert!(changes_of(&dir.path().join("missing")).is_none());
    }
}
