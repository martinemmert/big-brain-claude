//! Folders under the home folder, to choose where a new session starts: in Brain's dialog and
//! from the phone, which may look only inside the home folder.

use std::path::{Path, PathBuf};

/// `path` as a folder inside `home` (links resolved), or `None` outside it, when it is no folder,
/// or when it lies in a hidden one (`~/.ssh`, Brain's and Claude's own folders), which nobody
/// browses to.
pub fn inside(home: &Path, path: &Path) -> Option<PathBuf> {
    let home = home.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    let hidden = path.strip_prefix(&home).ok()?.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
    (!hidden && path.is_dir()).then_some(path)
}

/// The folders in `dir` by name, without hidden ones.
pub fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    folders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_visible_folders_and_stays_inside_home() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        for dir in ["Work/b-app", "Work/A-app", "Work/.git", "outside"] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        std::fs::write(home.join("Work/notes.txt"), "").unwrap();
        let names: Vec<String> = subfolders(&home.join("Work")).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["A-app", "b-app"]);

        assert!(inside(&home, &home.join("Work/b-app")).is_some());
        assert!(inside(&home, &home.join("Work/../Work")).is_some());
        assert_eq!(inside(&home, &home.join("Work/notes.txt")), None);
        assert_eq!(inside(&home, &home.join("..")), None);
        assert_eq!(inside(&home, &home.join("missing")), None);
        assert_eq!(inside(&home, &home.join("Work/.git")), None, "nothing hidden");
    }
}
