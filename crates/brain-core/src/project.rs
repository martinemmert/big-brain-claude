//! Which project a session works in: the git repository of its directory, with worktrees
//! grouped under their main repository.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Project {
    /// The main repository's top level (or the directory itself outside git).
    pub root: PathBuf,
    pub name: String,
    /// The worktree's directory name when the session runs in a linked worktree.
    pub worktree: Option<String>,
}

pub fn project_of(cwd: &Path) -> Project {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir", "--show-toplevel"])
        .output();
    match out {
        Ok(out) if out.status.success() => from_rev_parse(cwd, &String::from_utf8_lossy(&out.stdout)),
        _ => outside_git(cwd),
    }
}

/// `git rev-parse --git-common-dir --show-toplevel` → project. The main repository is the
/// parent of the common `.git` directory; a different top level means a linked worktree.
pub fn from_rev_parse(cwd: &Path, output: &str) -> Project {
    let mut lines = output.lines().map(str::trim).filter(|l| !l.is_empty());
    let (Some(common), Some(toplevel)) = (lines.next(), lines.next()) else {
        return outside_git(cwd);
    };
    let common = Path::new(common);
    let toplevel = PathBuf::from(toplevel);
    let root = match common.file_name() {
        Some(name) if name == ".git" => common.parent().map(Path::to_path_buf).unwrap_or_else(|| toplevel.clone()),
        _ => toplevel.clone(), // bare repositories and unusual layouts
    };
    let worktree = (toplevel != root).then(|| file_name(&toplevel));
    Project { name: file_name(&root), root, worktree }
}

fn outside_git(cwd: &Path) -> Project {
    Project { root: cwd.to_path_buf(), name: file_name(cwd), worktree: None }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktrees_belong_to_their_main_repository() {
        let main = from_rev_parse(Path::new("/w/app"), "/w/app/.git\n/w/app\n");
        assert_eq!((main.name.as_str(), main.worktree), ("app", None));

        let linked = from_rev_parse(Path::new("/w/app/.worktrees/fix"), "/w/app/.git\n/w/app/.worktrees/fix\n");
        assert_eq!(linked.root, PathBuf::from("/w/app"));
        assert_eq!(linked.worktree.as_deref(), Some("fix"));

        let none = from_rev_parse(Path::new("/tmp/notes"), "");
        assert_eq!((none.name.as_str(), none.root), ("notes", PathBuf::from("/tmp/notes")));
    }
}
