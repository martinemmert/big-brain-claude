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

/// The checked-out branch, `None` outside git or on a detached HEAD.
pub fn branch_of(cwd: &Path) -> Option<String> {
    git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|b| b.trim().to_string()).filter(|b| b != "HEAD")
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

/// One line of a diff, by what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// `diff --git`, `index`, `---`, `+++`: which file.
    Meta,
    /// `@@ -1,3 +1,4 @@`: where the next lines are.
    Hunk,
    Added,
    Removed,
    Context,
}

/// The diff of one changed file against HEAD; a new, untracked file as all added lines.
pub fn diff_of(cwd: &Path, change: &FileChange) -> Option<String> {
    if change.status == "??" {
        // `--no-index` exits with 1 when the files differ, which they always do here.
        let out = Command::new("git").arg("-C").arg(cwd).args(["diff", "--no-color", "--no-index", "--", "/dev/null", &change.path]).output().ok()?;
        return (out.status.code() == Some(1)).then(|| String::from_utf8_lossy(&out.stdout).into_owned());
    }
    git(cwd, &["diff", "--no-color", "HEAD", "--", &change.path])
}

/// Splits a unified diff into lines and what they are.
pub fn diff_lines(diff: &str) -> Vec<(DiffKind, String)> {
    let mut in_header = true;
    diff.lines()
        .map(|line| {
            let kind = if line.starts_with("diff --git") {
                in_header = true;
                DiffKind::Meta
            } else if line.starts_with("@@") {
                in_header = false;
                DiffKind::Hunk
            } else if in_header {
                DiffKind::Meta
            } else if line.starts_with('+') {
                DiffKind::Added
            } else if line.starts_with('-') {
                DiffKind::Removed
            } else {
                DiffKind::Context
            };
            (kind, line.to_string())
        })
        .collect()
}

/// Whether Brain offers to discard the change: a modified, deleted, added or untracked file
/// (not renames or conflicts, which need git by hand).
pub fn discardable(change: &FileChange) -> bool {
    matches!(change.status.as_str(), " M" | "M " | "MM" | " D" | "D " | "A " | "AM" | "??")
}

/// After the current version is out of the way (in the Trash): the file as in HEAD again, or,
/// for a file HEAD doesn't have, no longer staged.
pub fn restore_from_head(cwd: &Path, change: &FileChange) -> Result<(), String> {
    let args: &[&str] = match change.status.as_str() {
        "??" => return Ok(()),
        "A " | "AM" => &["reset", "-q", "--"],
        _ => &["checkout", "HEAD", "--"],
    };
    let out = Command::new("git").arg("-C").arg(cwd).args(args).arg(&change.path).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
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
    fn diff_lines_say_what_each_line_is() {
        let diff = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n one\n+two\n-three\n";
        let kinds: Vec<DiffKind> = diff_lines(diff).into_iter().map(|(k, _)| k).collect();
        use DiffKind::*;
        assert_eq!(kinds, [Meta, Meta, Meta, Meta, Hunk, Context, Added, Removed]);
    }

    #[test]
    fn diffs_and_restores_in_a_real_repository() {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| Command::new("git").arg("-C").arg(dir.path()).args(args).output().unwrap();
        run(&["init", "-q"]);
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        run(&["add", "."]);
        run(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"]);
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new\n").unwrap();
        let changes = changes_of(dir.path()).unwrap();
        let modified = changes.files.iter().find(|f| f.path == "a.txt").unwrap();
        let untracked = changes.files.iter().find(|f| f.path == "b.txt").unwrap();

        assert!(diff_of(dir.path(), modified).unwrap().contains("+two"));
        assert!(diff_of(dir.path(), untracked).unwrap().contains("+new"));
        assert!(discardable(modified) && discardable(untracked));

        // The app moves the current version to the Trash first; here it's just removed.
        std::fs::remove_file(dir.path().join("a.txt")).unwrap();
        restore_from_head(dir.path(), modified).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "one\n");
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
