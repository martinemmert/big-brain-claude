//! What a session changed in its working directory, from git.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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

/// git in `cwd`. Paths are always literal: a file named `*` or `:(top)` is that file, never a
/// pattern that matches others.
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = git_command(cwd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).arg("--literal-pathspecs");
    command
}

/// The checked-out branch, `None` outside git or on a detached HEAD.
pub fn branch_of(cwd: &Path) -> Option<String> {
    git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|b| b.trim().to_string()).filter(|b| b != "HEAD")
}

/// `None` outside a git repository.
pub fn changes_of(cwd: &Path) -> Option<Changes> {
    // `-z`: paths exactly as they are, NUL-separated, never quoted or escaped.
    let status = git(cwd, &["status", "--porcelain=v1", "-z", "--untracked-files=normal"])?;
    let numstat = git(cwd, &["diff", "--numstat", "-z", "HEAD"]).unwrap_or_default();
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|b| b.trim().to_string());
    let ahead_behind = git(cwd, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"])
        .and_then(|out| parse_ahead_behind(&out));
    Some(Changes { branch, ahead_behind, files: parse(&status, &numstat) })
}

/// Joins `git status --porcelain=v1 -z` with `git diff --numstat -z HEAD` by path.
pub fn parse(status: &str, numstat: &str) -> Vec<FileChange> {
    let mut counts: BTreeMap<String, (Option<u64>, Option<u64>)> = BTreeMap::new();
    // `added<TAB>removed<TAB>path<NUL>`; a rename: `added<TAB>removed<TAB><NUL>old<NUL>new<NUL>`.
    let mut fields = numstat.split('\0');
    while let Some(field) = fields.next() {
        let mut parts = field.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let path = if path.is_empty() {
            let _old = fields.next();
            fields.next().unwrap_or_default().to_string()
        } else {
            path.to_string()
        };
        // Binary files show `-`.
        counts.insert(path, (added.parse().ok(), removed.parse().ok()));
    }
    // `XY path<NUL>`; a rename or copy is followed by its original path as its own entry.
    let mut entries = status.split('\0').filter(|e| !e.is_empty());
    let mut out = Vec::new();
    while let Some(entry) = entries.next() {
        if entry.len() < 4 || !entry.is_char_boundary(3) {
            continue;
        }
        let code = entry[..2].to_string();
        if code.starts_with('R') || code.starts_with('C') {
            let _original = entries.next();
        }
        let path = entry[3..].to_string();
        let (added, removed) = counts.get(&path).copied().unwrap_or((None, None));
        out.push(FileChange { path, status: code, added, removed });
    }
    out
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
        let out = git_command(cwd).args(["diff", "--no-color", "--no-index", "--", "/dev/null", &change.path]).output().ok()?;
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

/// A file one session wrote or edited, with what git says about it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionChange {
    /// Absolute path, as the repository sees it.
    pub path: String,
    /// Relative to the session's folder, else to the repository, else absolute.
    pub relative: String,
    /// When the session last wrote it (Unix ms); 0 when the transcript has no time.
    pub last_ms: i64,
    pub added: Option<u64>,
    pub removed: Option<u64>,
    /// What Claude said in one sentence right before it last wrote or edited the file.
    pub said: Option<String>,
    /// The repository the file is in (the folder to run git in).
    pub root: PathBuf,
    /// Its entry in git's status; `None` when it equals HEAD (or isn't in a repository).
    pub git: Option<FileChange>,
}

/// The files the session behind `transcript` wrote or edited (not those it only read), newest
/// first, with the lines added and removed against HEAD where git knows them. A new, untracked
/// file counts all its lines as added.
pub fn session_changes(transcript: &Path, cwd: &Path) -> Vec<SessionChange> {
    let files: Vec<crate::files::SessionFile> = crate::files::session_files(transcript).into_iter().filter(|f| f.changed()).collect();
    if files.is_empty() {
        return Vec::new();
    }
    let root = git(cwd, &["rev-parse", "--show-toplevel"]).map(|r| PathBuf::from(r.trim()));
    let git_root = root.as_deref().unwrap_or(cwd);
    let in_git = root.is_some();
    let status = if in_git { changes_of(git_root).map(|c| c.files).unwrap_or_default() } else { Vec::new() };
    let cwd_canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    files
        .into_iter()
        .map(|file| {
            let path = std::fs::canonicalize(&file.path).unwrap_or_else(|_| PathBuf::from(&file.path));
            let relative_to_root = path.strip_prefix(git_root).ok().map(|p| p.display().to_string());
            let mut entry = relative_to_root.as_ref().filter(|_| in_git).and_then(|rel| status.iter().find(|c| c.path == *rel).cloned());
            // A new file in a new folder is listed by git as that folder.
            if let (None, Some(rel), true) = (&entry, &relative_to_root, in_git) {
                let untracked = git(git_root, &["ls-files", "--others", "--exclude-standard", "--", rel]).is_some_and(|out| !out.trim().is_empty());
                if untracked {
                    entry = Some(FileChange { path: rel.clone(), status: "??".into(), added: None, removed: None });
                }
            }
            let (mut added, mut removed) = entry.as_ref().map_or((None, None), |c| (c.added, c.removed));
            if entry.as_ref().is_some_and(|c| c.status == "??") {
                added = line_count(&path);
                removed = added.map(|_| 0);
            }
            let relative = path
                .strip_prefix(&cwd_canonical)
                .ok()
                .map(|p| p.display().to_string())
                .or(relative_to_root)
                .unwrap_or_else(|| path.display().to_string());
            SessionChange { path: path.display().to_string(), relative, last_ms: file.last_ms, added, removed, said: file.said, root: git_root.to_path_buf(), git: entry }
        })
        .collect()
}

/// The diff of a session's file against HEAD; `None` when it equals HEAD or git can't tell.
pub fn session_diff(change: &SessionChange) -> Option<String> {
    diff_of(&change.root, change.git.as_ref()?)
}

/// Lines of a text file of up to 4 MB; `None` for anything else.
fn line_count(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 4 << 20 {
        return None;
    }
    let text = String::from_utf8(std::fs::read(path).ok()?).ok()?;
    Some(text.lines().count() as u64)
}

/// Whether Brain offers to discard the change: a modified, deleted, added or untracked file
/// (not renames or conflicts, which need git by hand).
pub fn discardable(change: &FileChange) -> bool {
    // An untracked folder (`dir/`) is never discarded as a whole.
    matches!(change.status.as_str(), " M" | "M " | "MM" | " D" | "D " | "A " | "AM" | "??") && !change.path.ends_with('/')
}

/// After the current version is out of the way (in the Trash): the file as in HEAD again, or,
/// for a file HEAD doesn't have, no longer staged.
pub fn restore_from_head(cwd: &Path, change: &FileChange) -> Result<(), String> {
    let args: &[&str] = match change.status.as_str() {
        "??" => return Ok(()),
        "A " | "AM" => &["reset", "-q", "--"],
        _ => &["checkout", "HEAD", "--"],
    };
    let out = git_command(cwd).args(args).arg(&change.path).output().map_err(|e| e.to_string())?;
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
        let status = " M src/view.rs\0A  src/new.rs\0R  renamed.rs\0old.rs\0?? notes.txt\0";
        let numstat = "12\t3\tsrc/view.rs\040\t0\tsrc/new.rs\00\t0\t\0old.rs\0renamed.rs\0";

        let files = parse(status, numstat);

        assert_eq!(files.len(), 4);
        assert_eq!((files[0].path.as_str(), files[0].added, files[0].removed), ("src/view.rs", Some(12), Some(3)));
        assert_eq!(files[2].path, "renamed.rs");
        assert_eq!((files[3].status.as_str(), files[3].added), ("??", None));
        assert_eq!(Changes { files, ..Changes::default() }.totals(), (52, 3));
        assert_eq!(parse_ahead_behind("2\t5\n"), Some((5, 2)));
    }

    #[test]
    fn odd_file_names_stay_exact_and_are_never_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| Command::new("git").arg("-C").arg(dir.path()).args(args).output().unwrap();
        run(&["init", "-q"]);
        for name in ["a.txt", "*", "quote\"d.txt", "ümlaut.txt"] {
            std::fs::write(dir.path().join(name), "one\n").unwrap();
        }
        run(&["add", "."]);
        run(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"]);
        for name in ["a.txt", "*", "quote\"d.txt", "ümlaut.txt"] {
            std::fs::write(dir.path().join(name), "two\n").unwrap();
        }
        let changes = changes_of(dir.path()).unwrap();
        let mut paths: Vec<&str> = changes.files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["*", "a.txt", "quote\"d.txt", "ümlaut.txt"]);

        // Restoring the file named `*` restores only that file.
        let star = changes.files.iter().find(|f| f.path == "*").unwrap();
        restore_from_head(dir.path(), star).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("*")).unwrap(), "one\n");
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "two\n");
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

    fn commit_all(dir: &Path) {
        let run = |args: &[&str]| Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        run(&["add", "."]);
        run(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"]);
    }

    fn tool(name: &str, path: &Path, ts: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"tool_use","name":"{name}","input":{{"file_path":{}}}}}]}}}}"#,
            serde_json::to_string(&path.display().to_string()).unwrap()
        )
    }

    #[test]
    fn a_sessions_changes_are_what_it_wrote_or_edited_newest_first_and_never_what_it_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        Command::new("git").arg("-C").arg(&root).args(["init", "-q"]).output().unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/config.rs"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("src/read_only.rs"), "x\n").unwrap();
        std::fs::write(root.join("src/committed.rs"), "y\n").unwrap();
        commit_all(&root);
        std::fs::write(root.join("src/config.rs"), "one\nthree\nfour\n").unwrap();
        std::fs::create_dir_all(root.join("tests/new")).unwrap();
        std::fs::write(root.join("tests/new/limit.rs"), "a\nb\nc\n").unwrap();

        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            transcript.path(),
            [
                tool("Edit", &root.join("src/config.rs"), "2026-10-09T10:00:00Z"),
                tool("Read", &root.join("src/read_only.rs"), "2026-10-09T10:01:00Z"),
                tool("Edit", &root.join("src/committed.rs"), "2026-10-09T10:02:00Z"),
                tool("Write", &root.join("tests/new/limit.rs"), "2026-10-09T10:03:00Z"),
            ]
            .join("\n"),
        )
        .unwrap();

        let changes = session_changes(transcript.path(), &root);

        let rows: Vec<(&str, Option<u64>, Option<u64>)> = changes.iter().map(|c| (c.relative.as_str(), c.added, c.removed)).collect();
        // The file only read is absent; the new file in a new folder counts all its lines; the
        // file that equals HEAD again is listed without counts.
        assert_eq!(rows, [("tests/new/limit.rs", Some(3), Some(0)), ("src/committed.rs", None, None), ("src/config.rs", Some(2), Some(1))]);
        assert!(session_diff(&changes[0]).unwrap().contains("+a"));
        assert!(session_diff(&changes[1]).is_none());
        assert!(session_diff(&changes[2]).unwrap().contains("-two"));
    }

    fn said(text: &str, ts: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"text","text":{}}}]}}}}"#,
            serde_json::to_string(text).unwrap()
        )
    }

    #[test]
    fn each_file_gets_the_sentence_claude_said_right_before_writing_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            std::fs::write(root.join(name), "x\n").unwrap();
        }
        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            transcript.path(),
            [
                said("Ich baue **zuerst** den `Limiter`. Danach kommen die Tests.", "2026-10-09T10:00:00Z"),
                tool("Write", &root.join("a.txt"), "2026-10-09T10:00:01Z"),
                // A sentence, then a file only looked at: that sentence was about the look.
                said("Jetzt die Konfiguration.", "2026-10-09T10:01:00Z"),
                tool("Read", &root.join("d.txt"), "2026-10-09T10:01:01Z"),
                tool("Edit", &root.join("b.txt"), "2026-10-09T10:01:02Z"),
                // Said once, two files written after it.
                said("Beide Dateien bekommen denselben Kopf.", "2026-10-09T10:02:00Z"),
                tool("Edit", &root.join("c.txt"), "2026-10-09T10:02:01Z"),
                tool("Edit", &root.join("a.txt"), "2026-10-09T10:02:02Z"),
            ]
            .join("\n"),
        )
        .unwrap();

        let changes = session_changes(transcript.path(), &root);

        let said_of = |name: &str| changes.iter().find(|c| c.relative == name).unwrap().said.as_deref();
        // a: its last write counts; b: the Read in between ended what was said.
        assert_eq!(said_of("a.txt"), Some("Beide Dateien bekommen denselben Kopf."));
        assert_eq!(said_of("b.txt"), None);
        assert_eq!(said_of("c.txt"), Some("Beide Dateien bekommen denselben Kopf."));
    }

    #[test]
    fn what_was_said_is_one_plain_sentence() {
        use crate::files::one_sentence;
        assert_eq!(
            one_sentence("Ich habe **fett** und `Code` und [einen Link](https://x.de) gebaut. Zweiter Satz.").as_deref(),
            Some("Ich habe fett und Code und einen Link gebaut.")
        );
        assert_eq!(one_sentence("```rust\nlet x = 1;\n```\nDanach läuft es:").as_deref(), Some("Danach läuft es"));
        assert_eq!(one_sentence("```\ncode only\n```"), None);
    }

    #[test]
    fn a_prompt_ends_what_was_said_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("a.txt"), "x\n").unwrap();
        let prompt = r#"{"type":"user","timestamp":"2026-10-09T10:00:30Z","message":{"role":"user","content":"Mach weiter"}}"#.to_string();
        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            transcript.path(),
            [said("Alter Satz.", "2026-10-09T10:00:00Z"), prompt, tool("Edit", &root.join("a.txt"), "2026-10-09T10:01:00Z")].join("\n"),
        )
        .unwrap();

        assert_eq!(session_changes(transcript.path(), &root)[0].said, None);
    }

    #[test]
    fn a_file_outside_any_repository_is_listed_without_counts() {
        let dir = tempfile::tempdir().unwrap();
        let file = std::fs::canonicalize(dir.path()).unwrap().join("notes.txt");
        std::fs::write(&file, "x\n").unwrap();
        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(transcript.path(), tool("Write", &file, "2026-10-09T10:00:00Z")).unwrap();

        let changes = session_changes(transcript.path(), dir.path());

        assert_eq!(changes.len(), 1);
        assert_eq!((changes[0].relative.as_str(), changes[0].added), ("notes.txt", None));
        assert!(session_diff(&changes[0]).is_none());
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
