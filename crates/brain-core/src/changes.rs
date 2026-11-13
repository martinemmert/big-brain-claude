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

/// `git diff --numstat -z` as counts by path (a rename by its new path). Binary files count `None`.
fn parse_numstat(numstat: &str) -> BTreeMap<String, (Option<u64>, Option<u64>)> {
    let mut counts = BTreeMap::new();
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
    counts
}

/// Joins `git status --porcelain=v1 -z` with `git diff --numstat -z HEAD` by path.
pub fn parse(status: &str, numstat: &str) -> Vec<FileChange> {
    let counts = parse_numstat(numstat);
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
    /// This session wrote or edited the file (its transcript says so); only then there is a `said`.
    pub from_session: bool,
    /// The commit its diff starts from (the merge base with the target); `None` compares with HEAD.
    pub base: Option<String>,
    /// Where a renamed file was before.
    pub original: Option<String>,
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
            SessionChange { path: path.display().to_string(), relative, last_ms: file.last_ms, added, removed, said: file.said, root: git_root.to_path_buf(), git: entry, from_session: true, base: None, original: None }
        })
        .collect()
}

/// The diff of a session's file against HEAD; `None` when it equals HEAD or git can't tell.
pub fn session_diff(change: &SessionChange) -> Option<String> {
    let file = change.git.as_ref()?;
    match &change.base {
        // A new, untracked file has no old version in a merge request either.
        Some(base) if file.status != "??" => {
            let mut args = vec!["diff", "--no-color", "-M", base.as_str(), "--"];
            args.extend(change.original.as_deref());
            args.push(&file.path);
            git(&change.root, &args)
        }
        _ => diff_of(&change.root, file),
    }
}

/// Lines of a text file of up to 1 MB; `None` for anything else (a binary file has a NUL byte).
fn line_count(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 1 << 20 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    Some(String::from_utf8(bytes).ok()?.lines().count() as u64)
}

/// What a merge request compares: the branch checked out in the session's folder against a
/// target, from their merge base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeInfo {
    /// The repository's top folder.
    pub root: PathBuf,
    /// The checked-out branch; the short commit id on a detached HEAD.
    pub branch: String,
    /// The branch the changes are compared with; `None` when there is none to propose (the default
    /// branch without a remote): then only what isn't committed shows.
    pub target: Option<String>,
    /// The commit the diffs start from.
    pub base: String,
}

/// The files of a session's folder to show: those of its merge request, or without git the files
/// its transcript says it changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changeset {
    pub files: Vec<SessionChange>,
    /// `None` outside git (or in a repository without a commit): no merge request, no diff base.
    pub merge: Option<MergeInfo>,
}

/// The top folder of the repository `cwd` is in.
pub fn repo_root(cwd: &Path) -> Option<PathBuf> {
    git(cwd, &["rev-parse", "--show-toplevel"]).map(|r| PathBuf::from(r.trim())).filter(|r| !r.as_os_str().is_empty())
}

fn ref_exists(root: &Path, name: &str) -> bool {
    git(root, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).is_some()
}

/// The branch a merge request of `branch` goes to unless chosen otherwise: the repository's
/// default branch (`origin/HEAD`, else `main`, else `master`). On the default branch itself, its
/// copy at `origin` (the commits not pushed yet), or nothing without one.
pub fn suggested_target(root: &Path, branch: Option<&str>) -> Option<String> {
    let from_origin = git(root, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
        .and_then(|name| name.trim().strip_prefix("origin/").map(str::to_string));
    let exists = |name: &str| ref_exists(root, &format!("refs/heads/{name}")) || ref_exists(root, &format!("refs/remotes/origin/{name}"));
    let default = from_origin.into_iter().chain(["main", "master"].map(String::from)).find(|name| exists(name))?;
    let at_origin = format!("origin/{default}");
    if branch == Some(default.as_str()) {
        return ref_exists(root, &format!("refs/remotes/{at_origin}")).then_some(at_origin);
    }
    if ref_exists(root, &format!("refs/heads/{default}")) {
        Some(default)
    } else {
        Some(at_origin)
    }
}

/// The local and remote branches to choose a target from, local first, without the checked-out
/// branch and the remotes' `HEAD`.
pub fn target_choices(root: &Path, branch: Option<&str>) -> Vec<String> {
    let list = |namespace: &str| -> Vec<String> {
        git(root, &["for-each-ref", "--format=%(refname:short)", namespace])
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            // A remote's `HEAD` shows as `origin/HEAD`, or as `origin` alone, depending on git.
            .filter(|name| !name.is_empty() && !name.ends_with("/HEAD") && (namespace == "refs/heads" || name.contains('/')))
            .collect()
    };
    list("refs/heads").into_iter().chain(list("refs/remotes")).filter(|name| Some(name.as_str()) != branch).collect()
}

struct Entry {
    path: String,
    original: Option<String>,
    status: String,
    added: Option<u64>,
    removed: Option<u64>,
}

/// `git diff --name-status -z -M` as (path, status, original): `M<NUL>path<NUL>`, a rename
/// `R087<NUL>old<NUL>new<NUL>`.
fn parse_name_status(out: &str) -> Vec<(String, String, Option<String>)> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let Some(code) = fields.next() {
        match code.chars().next() {
            Some('R' | 'C') => {
                let (Some(old), Some(new)) = (fields.next(), fields.next()) else { break };
                files.push((new.to_string(), "R ".to_string(), Some(old.to_string())));
            }
            Some(letter) => {
                let Some(path) = fields.next() else { break };
                let status = match letter {
                    'A' => "A ",
                    'D' => "D ",
                    _ => "M ",
                };
                files.push((path.to_string(), status.to_string(), None));
            }
            None => {}
        }
    }
    files
}

/// Most untracked files listed; a folder of thousands is no merge request.
const MOST_UNTRACKED: usize = 1000;

/// Every file that differs from `base` in the working tree: the tracked ones (committed or not)
/// and the untracked ones that aren't ignored, by path.
fn merge_entries(root: &Path, base: &str) -> Vec<Entry> {
    let names = git(root, &["diff", "--name-status", "-z", "-M", base]).unwrap_or_default();
    let counts = parse_numstat(&git(root, &["diff", "--numstat", "-z", "-M", base]).unwrap_or_default());
    let mut entries: Vec<Entry> = parse_name_status(&names)
        .into_iter()
        .map(|(path, status, original)| {
            let (added, removed) = counts.get(&path).copied().unwrap_or((None, None));
            Entry { path, original, status, added, removed }
        })
        .collect();
    let status = git(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]).unwrap_or_default();
    for file in parse(&status, "").into_iter().filter(|f| f.status == "??").take(MOST_UNTRACKED) {
        let added = line_count(&root.join(&file.path));
        entries.push(Entry { removed: added.map(|_| 0), added, path: file.path, original: None, status: "??".into() });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

/// A path as the file system names it; a file that is gone by the folder it was in.
fn canonical_lenient(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => std::fs::canonicalize(dir).map(|d| d.join(name)).unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    })
}

/// The merge request of the branch checked out in `cwd`: all files that differ from the merge base
/// with the target, in the working tree. The target is the one `chosen` knows for the repository
/// and branch (`HEAD` on a detached one), else the suggested one. `transcript` marks the files
/// the session wrote itself and what it said before. Outside git: the files the transcript says
/// the session changed.
pub fn merge_request(transcript: Option<&Path>, cwd: &Path, chosen: &dyn Fn(&Path, &str) -> Option<String>) -> Changeset {
    let fallback = || Changeset { files: transcript.map(|t| session_changes(t, cwd)).unwrap_or_default(), merge: None };
    let Some(root) = repo_root(cwd) else { return fallback() };
    let Some(head) = git(&root, &["rev-parse", "--verify", "HEAD"]).map(|h| h.trim().to_string()) else { return fallback() };
    let branch = branch_of(&root);
    let label = branch.clone().unwrap_or_else(|| head.chars().take(7).collect());
    let target = chosen(&root, branch.as_deref().unwrap_or("HEAD"))
        .filter(|t| ref_exists(&root, t))
        .or_else(|| suggested_target(&root, branch.as_deref()));
    let base = target
        .as_deref()
        .and_then(|t| git(&root, &["merge-base", t, "HEAD"]))
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or(head);

    let mut by_path: BTreeMap<PathBuf, crate::files::SessionFile> = BTreeMap::new();
    for file in transcript.map(crate::files::session_files).unwrap_or_default().into_iter().filter(|f| f.changed()) {
        by_path.insert(canonical_lenient(Path::new(&file.path)), file);
    }
    let cwd_canonical = canonical_lenient(cwd);
    let files = merge_entries(&root, &base)
        .into_iter()
        .map(|entry| {
            let path = canonical_lenient(&root.join(&entry.path));
            let session = by_path.remove(&path);
            let relative = path.strip_prefix(&cwd_canonical).map_or_else(|_| entry.path.clone(), |p| p.display().to_string());
            let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            let last_ms = session.as_ref().map(|s| s.last_ms).filter(|ms| *ms > 0).or(modified.map(|d| d.as_millis() as i64)).unwrap_or(0);
            SessionChange {
                path: path.display().to_string(),
                relative,
                last_ms,
                added: entry.added,
                removed: entry.removed,
                from_session: session.is_some(),
                said: session.and_then(|s| s.said),
                root: root.clone(),
                git: Some(FileChange { path: entry.path, status: entry.status, added: entry.added, removed: entry.removed }),
                base: Some(base.clone()),
                original: entry.original,
            }
        })
        .collect();
    Changeset { files, merge: Some(MergeInfo { root, branch: label, target, base }) }
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

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn repo_on_main() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        git_in(&root, &["init", "-q", "-b", "main"]);
        (dir, root)
    }

    fn commit(root: &Path, message: &str) {
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", message]);
    }

    fn none(_: &Path, _: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_merge_request_is_everything_since_the_merge_base_committed_or_not_and_said_only_for_the_sessions_files() {
        let (_dir, root) = repo_on_main();
        for (name, body) in [("a.txt", "one\n"), ("b.txt", "one\n"), ("gone.txt", "x\ny\n"), ("old.txt", "keep\nthis\nfile\nas it is\n"), ("z.txt", "z\n")] {
            std::fs::write(root.join(name), body).unwrap();
        }
        commit(&root, "base");
        git_in(&root, &["checkout", "-q", "-b", "feature/x"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("committed.txt"), "c1\nc2\nc3\n").unwrap();
        commit(&root, "on the branch");
        // The target moves on: its own commit is not part of the merge request.
        git_in(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("z.txt"), "z\nmain moved\n").unwrap();
        commit(&root, "main moves");
        git_in(&root, &["checkout", "-q", "feature/x"]);
        // Not committed: an edit, a deletion, a rename, a new untracked file.
        std::fs::write(root.join("b.txt"), "one\nthree\nfour\n").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        git_in(&root, &["mv", "old.txt", "new_name.txt"]);
        std::fs::write(root.join("untracked.txt"), "u1\nu2\n").unwrap();

        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            transcript.path(),
            [said("Ich ergänze die zweite Zeile.", "2026-10-09T10:00:00Z"), tool("Edit", &root.join("a.txt"), "2026-10-09T10:00:01Z")].join("\n"),
        )
        .unwrap();

        let set = merge_request(Some(transcript.path()), &root, &none);

        let merge = set.merge.clone().unwrap();
        assert_eq!((merge.branch.as_str(), merge.target.as_deref()), ("feature/x", Some("main")));
        let rows: Vec<(&str, &str, Option<u64>, Option<u64>)> =
            set.files.iter().map(|c| (c.relative.as_str(), c.git.as_ref().unwrap().status.as_str(), c.added, c.removed)).collect();
        assert_eq!(
            rows,
            [
                ("a.txt", "M ", Some(1), Some(0)),
                ("b.txt", "M ", Some(2), Some(0)),
                ("committed.txt", "A ", Some(3), Some(0)),
                ("gone.txt", "D ", Some(0), Some(2)),
                ("new_name.txt", "R ", Some(0), Some(0)),
                ("untracked.txt", "??", Some(2), Some(0)),
            ]
        );
        // Only a.txt is the session's, and only it says something.
        let said_by: Vec<(&str, bool, Option<&str>)> = set.files.iter().map(|c| (c.relative.as_str(), c.from_session, c.said.as_deref())).collect();
        assert_eq!(said_by[0], ("a.txt", true, Some("Ich ergänze die zweite Zeile.")));
        assert!(said_by[1..].iter().all(|(_, from_session, said)| !from_session && said.is_none()));

        let diff = |name: &str| session_diff(set.files.iter().find(|c| c.relative == name).unwrap()).unwrap();
        assert!(diff("a.txt").contains("+two"));
        assert!(diff("committed.txt").contains("+c3"));
        assert!(diff("gone.txt").contains("-x") && !diff("gone.txt").contains("\n+x"));
        assert!(diff("new_name.txt").contains("rename from old.txt"));
        assert!(diff("untracked.txt").contains("+u2"));
        assert!(!set.files.iter().any(|c| c.relative == "z.txt"));
    }

    #[test]
    fn the_target_is_proposed_from_origin_head_then_main_then_master_and_on_the_default_branch_from_origin() {
        // Only `master`: that one.
        let (_dir, root) = repo_on_main();
        git_in(&root, &["branch", "-m", "master"]);
        std::fs::write(root.join("a.txt"), "1\n").unwrap();
        commit(&root, "init");
        git_in(&root, &["checkout", "-q", "-b", "feature/y"]);
        assert_eq!(suggested_target(&root, Some("feature/y")).as_deref(), Some("master"));
        // On the default branch without a remote there is nothing to compare with but the edits.
        assert_eq!(suggested_target(&root, Some("master")), None);

        // `origin/HEAD` names the default, even when `main` exists too.
        let (_dir2, root2) = repo_on_main();
        let remote = tempfile::tempdir().unwrap();
        git_in(remote.path(), &["init", "-q", "--bare", "-b", "trunk"]);
        std::fs::write(root2.join("a.txt"), "1\n").unwrap();
        commit(&root2, "init");
        git_in(&root2, &["remote", "add", "origin", &remote.path().display().to_string()]);
        git_in(&root2, &["branch", "trunk"]);
        git_in(&root2, &["push", "-q", "origin", "main", "trunk"]);
        git_in(&root2, &["fetch", "-q", "origin"]);
        git_in(&root2, &["remote", "set-head", "origin", "trunk"]);
        git_in(&root2, &["checkout", "-q", "-b", "feature/z"]);
        assert_eq!(suggested_target(&root2, Some("feature/z")).as_deref(), Some("trunk"));
        // On `trunk` itself: its copy at origin shows the commits not pushed yet.
        git_in(&root2, &["checkout", "-q", "trunk"]);
        assert_eq!(suggested_target(&root2, Some("trunk")).as_deref(), Some("origin/trunk"));
        std::fs::write(root2.join("b.txt"), "unpushed\n").unwrap();
        commit(&root2, "not pushed");
        std::fs::write(root2.join("c.txt"), "edit\n").unwrap();
        let set = merge_request(None, &root2, &none);
        assert_eq!(set.merge.unwrap().target.as_deref(), Some("origin/trunk"));
        let names: Vec<&str> = set.files.iter().map(|c| c.relative.as_str()).collect();
        assert_eq!(names, ["b.txt", "c.txt"]);

        // The choices: local first, remote after, no HEAD, not the branch itself.
        assert_eq!(target_choices(&root2, Some("trunk")), ["feature/z", "main", "origin/main", "origin/trunk"]);
    }

    #[test]
    fn a_chosen_target_wins_unless_it_is_gone_and_a_detached_head_still_has_a_merge_request() {
        let (_dir, root) = repo_on_main();
        std::fs::write(root.join("a.txt"), "1\n").unwrap();
        commit(&root, "init");
        git_in(&root, &["checkout", "-q", "-b", "develop"]);
        std::fs::write(root.join("dev.txt"), "d\n").unwrap();
        commit(&root, "dev");
        git_in(&root, &["checkout", "-q", "-b", "feature/w"]);
        std::fs::write(root.join("w.txt"), "w\n").unwrap();
        commit(&root, "w");

        let against_develop = merge_request(None, &root, &|_, branch| (branch == "feature/w").then(|| "develop".to_string()));
        let names = |set: &Changeset| set.files.iter().map(|c| c.relative.clone()).collect::<Vec<_>>();
        assert_eq!(names(&against_develop), ["w.txt"]);
        assert_eq!(names(&merge_request(None, &root, &none)), ["dev.txt", "w.txt"]);
        let vanished = merge_request(None, &root, &|_, _| Some("deleted-branch".to_string()));
        assert_eq!(vanished.merge.unwrap().target.as_deref(), Some("main"));

        git_in(&root, &["checkout", "-q", "--detach"]);
        let asked = std::cell::RefCell::new(String::new());
        let detached = merge_request(None, &root, &|_, branch| {
            *asked.borrow_mut() = branch.to_string();
            None
        });
        assert_eq!(asked.into_inner(), "HEAD");
        assert_eq!(detached.merge.clone().unwrap().branch.len(), 7);
        assert_eq!(names(&detached), ["dev.txt", "w.txt"]);
    }

    #[test]
    fn without_git_the_merge_request_falls_back_to_the_files_the_transcript_names() {
        let dir = tempfile::tempdir().unwrap();
        let file = std::fs::canonicalize(dir.path()).unwrap().join("notes.txt");
        std::fs::write(&file, "x\n").unwrap();
        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(transcript.path(), tool("Write", &file, "2026-10-09T10:00:00Z")).unwrap();

        let set = merge_request(Some(transcript.path()), dir.path(), &none);

        assert!(set.merge.is_none());
        assert_eq!(set.files.len(), 1);
        assert!(set.files[0].from_session && set.files[0].base.is_none());
    }
}
