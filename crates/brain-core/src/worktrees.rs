//! Git worktrees of a project: which exist, what is in them, and whether they can go. Reading
//! only; the two writing commands (`worktree remove`, `worktree prune`) run after the user said so,
//! and never with `--force`.

use std::path::{Path, PathBuf};

use crate::changes::{branch_of, git, git_command, parse_ahead_behind, suggested_target};

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// `None` on a detached HEAD.
    pub branch: Option<String>,
    /// The folder is gone (`git worktree prune` clears the entry).
    pub prunable: bool,
    pub locked: bool,
}

/// What Brain reads of a linked worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub worktree: Worktree,
    /// Epoch ms of the last commit.
    pub last_commit_ms: Option<i64>,
    /// Open changes (or `git status` failed: then it is not known to be clean).
    pub dirty: bool,
    /// Commits not pushed: ahead of the upstream, else of the default branch. `None` when neither exists.
    pub unpushed: Option<u64>,
    /// Its HEAD is part of the default branch.
    pub merged: bool,
}

/// How a worktree stands, for the user to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Clean, nothing unpushed, no living session in it.
    Removable,
    /// Open changes or unpushed commits.
    Check,
    /// A living session works in it.
    InUse,
    /// The folder is gone.
    Orphaned,
}

/// The worktree a session's folder is in, when that isn't the main checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRef {
    pub path: PathBuf,
    pub main: PathBuf,
    pub branch: Option<String>,
}

/// `git worktree list --porcelain`: the first entry is the main checkout.
pub fn parse_list(porcelain: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    for block in porcelain.split("\n\n") {
        let mut worktree = Worktree { path: PathBuf::new(), branch: None, prunable: false, locked: false };
        for line in block.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "worktree" => worktree.path = PathBuf::from(value),
                "branch" => worktree.branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_string()),
                "prunable" => worktree.prunable = true,
                "locked" => worktree.locked = true,
                _ => {}
            }
        }
        if !worktree.path.as_os_str().is_empty() {
            list.push(worktree);
        }
    }
    list
}

/// The worktrees of the repository `main` is in, the main checkout first. Empty outside git.
pub fn list(main: &Path) -> Vec<Worktree> {
    git(main, &["worktree", "list", "--porcelain"]).map(|out| parse_list(&out)).unwrap_or_default()
}

/// Every linked worktree (not the main checkout) of the repository at `main`, with what is in it.
pub fn scan(main: &Path) -> Vec<Inspection> {
    let all = list(main);
    let Some(first) = all.first().map(|w| w.path.clone()) else { return Vec::new() };
    all.into_iter().skip(1).map(|worktree| inspect(&first, worktree)).collect()
}

fn inspect(main: &Path, worktree: Worktree) -> Inspection {
    if worktree.prunable {
        return Inspection { worktree, last_commit_ms: None, dirty: false, unpushed: None, merged: false };
    }
    let path = worktree.path.clone();
    let branch = worktree.branch.as_deref();
    let last_commit_ms = git(&path, &["log", "-1", "--format=%ct"]).and_then(|s| s.trim().parse::<i64>().ok()).map(|s| s * 1000);
    let dirty = git(&path, &["status", "--porcelain"]).is_none_or(|out| !out.trim().is_empty());
    let target = suggested_target(main, branch);
    let unpushed = match git(&path, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]).and_then(|out| parse_ahead_behind(&out)) {
        Some((ahead, _)) => Some(ahead),
        None => target.as_deref().and_then(|t| git(&path, &["rev-list", "--count", &format!("{t}..HEAD")])).and_then(|n| n.trim().parse().ok()),
    };
    let merged = target.is_some_and(|t| git_command(&path).args(["merge-base", "--is-ancestor", "HEAD", &t]).output().is_ok_and(|o| o.status.success()));
    Inspection { worktree, last_commit_ms, dirty, unpushed, merged }
}

/// The verdict for a worktree; `in_use`: a living session works in it.
pub fn classify(inspection: &Inspection, in_use: bool) -> Verdict {
    if inspection.worktree.prunable {
        Verdict::Orphaned
    } else if in_use {
        Verdict::InUse
    } else if inspection.dirty || inspection.unpushed != Some(0) {
        Verdict::Check
    } else {
        Verdict::Removable
    }
}

/// The linked worktree `cwd` is in; `None` in a main checkout and outside git.
pub fn worktree_of(cwd: &Path) -> Option<WorktreeRef> {
    let project = crate::project::project_of(cwd);
    project.worktree.as_ref()?;
    let branch = branch_of(&project.checkout);
    Some(WorktreeRef { path: project.checkout, main: project.root, branch })
}

/// `git worktree remove <path>` from the main checkout, never forced: git refuses a worktree with
/// changes or one that is locked, and its message comes back.
pub fn remove(main: &Path, path: &Path) -> Result<(), String> {
    run(main, &["worktree", "remove"], Some(path))
}

/// `git worktree prune` from the main checkout: drops the entries whose folder is gone.
pub fn prune(main: &Path) -> Result<(), String> {
    run(main, &["worktree", "prune"], None)
}

fn run(main: &Path, args: &[&str], path: Option<&Path>) -> Result<(), String> {
    let mut command = git_command(main);
    command.args(args);
    if let Some(path) = path {
        // After `--`, so a folder named like an option is still the folder.
        command.arg("--").arg(path);
    }
    let out = command.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// What a session is asked before it ends (or clears, or is deleted) in a worktree.
pub fn check_prompt(worktree: &WorktreeRef) -> String {
    let (path, main) = (worktree.path.display(), worktree.main.display());
    let branch = worktree.branch.as_deref().map(|b| format!(" (Branch {b})")).unwrap_or_default();
    format!(
        "Diese Session wird gleich beendet. Prüfe bitte deinen Worktree {path}{branch}: Ist alles committet und gepusht? Ist die Arbeit gemergt oder noch offen? Wenn der Worktree nicht mehr gebraucht wird, entferne ihn (`git worktree remove`, aus dem Haupt-Checkout {main}), sonst sag kurz, was offen ist. Antworte in 1–3 Sätzen."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A repository on `main` with one commit, and a folder beside it for worktrees.
    fn repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().join("app");
        std::fs::create_dir(&root).unwrap();
        git_in(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        git_in(&root, &["add", "."]);
        git_in(&root, &["commit", "-q", "-m", "first"]);
        (dir, root)
    }

    fn add_worktree(root: &Path, name: &str) -> PathBuf {
        let path = root.parent().unwrap().join(name);
        git_in(root, &["worktree", "add", "-q", "-b", name, path.to_str().unwrap()]);
        path
    }

    fn only(root: &Path) -> Inspection {
        let mut found = scan(root);
        assert_eq!(found.len(), 1);
        found.remove(0)
    }

    #[test]
    fn lists_the_linked_worktrees_with_branch_and_leaves_out_the_main_checkout() {
        let (_dir, root) = repo();
        let wt = add_worktree(&root, "fix");
        git_in(&root, &["worktree", "add", "-q", "--detach", root.parent().unwrap().join("loose").to_str().unwrap()]);

        let all = list(&root);
        assert_eq!(all.len(), 3);
        assert_eq!((all[0].path.clone(), all[0].branch.as_deref()), (root.clone(), Some("main")));
        let found = scan(&root);
        let fix = found.iter().find(|i| i.worktree.path == wt).unwrap();
        assert_eq!(fix.worktree.branch.as_deref(), Some("fix"));
        assert!(found.iter().any(|i| i.worktree.branch.is_none()));
        assert!(fix.last_commit_ms.is_some());
    }

    #[test]
    fn a_clean_worktree_without_own_commits_can_go_even_though_it_counts_as_merged() {
        let (_dir, root) = repo();
        add_worktree(&root, "fix");
        let found = only(&root);
        assert_eq!((found.dirty, found.unpushed, found.merged), (false, Some(0), true));
        assert_eq!(classify(&found, false), Verdict::Removable);
    }

    #[test]
    fn open_changes_and_unpushed_commits_need_a_look_and_a_living_session_wins() {
        let (_dir, root) = repo();
        let wt = add_worktree(&root, "fix");
        std::fs::write(wt.join("new.txt"), "x\n").unwrap();
        let dirty = only(&root);
        assert!(dirty.dirty);
        assert_eq!(classify(&dirty, false), Verdict::Check);

        git_in(&wt, &["add", "."]);
        git_in(&wt, &["commit", "-q", "-m", "work"]);
        let ahead = only(&root);
        assert_eq!((ahead.dirty, ahead.unpushed, ahead.merged), (false, Some(1), false));
        assert_eq!(classify(&ahead, false), Verdict::Check);
        assert_eq!(classify(&ahead, true), Verdict::InUse);
    }

    #[test]
    fn a_worktree_whose_folder_is_gone_is_orphaned_until_pruned() {
        let (_dir, root) = repo();
        let wt = add_worktree(&root, "fix");
        std::fs::remove_dir_all(&wt).unwrap();

        let found = only(&root);
        assert!(found.worktree.prunable);
        assert_eq!(classify(&found, false), Verdict::Orphaned);

        prune(&root).unwrap();
        assert!(scan(&root).is_empty());
    }

    #[test]
    fn remove_takes_a_clean_worktree_and_never_forces_one_with_changes() {
        let (_dir, root) = repo();
        let wt = add_worktree(&root, "fix");
        std::fs::write(wt.join("new.txt"), "x\n").unwrap();

        let err = remove(&root, &wt).unwrap_err();
        assert!(!err.is_empty());
        assert!(wt.join("new.txt").exists());

        std::fs::remove_file(wt.join("new.txt")).unwrap();
        remove(&root, &wt).unwrap();
        assert!(!wt.exists());
        // The branch stays.
        assert!(git_in(&root, &["branch", "--list", "fix"]).contains("fix"));
    }

    #[test]
    fn knows_whether_a_folder_is_a_linked_worktree() {
        let (_dir, root) = repo();
        let wt = add_worktree(&root, "fix");
        std::fs::create_dir(wt.join("sub")).unwrap();

        assert_eq!(worktree_of(&root), None);
        assert_eq!(worktree_of(&root.parent().unwrap().join("nowhere")), None);
        let found = worktree_of(&wt.join("sub")).unwrap();
        assert_eq!((found.path, found.main, found.branch.as_deref()), (wt, root, Some("fix")));
    }

    #[test]
    fn parses_locked_and_prunable_entries() {
        let out = "worktree /r\nHEAD a\nbranch refs/heads/main\n\nworktree /w\nHEAD b\nbranch refs/heads/x\nlocked busy\nprunable gone\n\n";
        let list = parse_list(out);
        assert_eq!(list.len(), 2);
        assert_eq!((list[1].locked, list[1].prunable, list[1].branch.as_deref()), (true, true, Some("x")));
    }
}
