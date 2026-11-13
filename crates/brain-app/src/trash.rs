//! Moving files to the macOS Trash, where Finder can restore them, and back.

use std::path::{Path, PathBuf};

use objc2_foundation::{NSFileManager, NSString, NSURL};

/// Moves the file or folder to the Trash; returns where it lies there now.
pub fn move_to_trash(path: &Path) -> Result<PathBuf, String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let mut resulting: Option<objc2::rc::Retained<NSURL>> = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|err| err.localizedDescription().to_string())?;
    resulting.and_then(|url| url.path()).map(|path| PathBuf::from(path.to_string())).ok_or_else(|| "the Trash gave no path".to_string())
}

/// Puts what [`move_to_trash`] moved back where it was. Nothing is overwritten: a file at the
/// old place, or one the user already took out of the Trash, is an error.
pub fn put_back(in_trash: &Path, origin: &Path) -> Result<(), String> {
    if origin.exists() {
        return Err(format!("{} is there again", origin.display()));
    }
    if let Some(parent) = origin.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::rename(in_trash, origin).map_err(|err| format!("{}: {err}", in_trash.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_back_moves_the_file_to_its_place_and_refuses_to_overwrite() {
        let dir = std::env::temp_dir().join(format!("brain-trash-test-{}", std::process::id()));
        let (in_trash, origin) = (dir.join("trash/a.jsonl"), dir.join("projects/x/a.jsonl"));
        std::fs::create_dir_all(in_trash.parent().unwrap()).unwrap();
        std::fs::write(&in_trash, "one").unwrap();

        put_back(&in_trash, &origin).unwrap();
        assert_eq!(std::fs::read_to_string(&origin).unwrap(), "one");
        assert!(!in_trash.exists());

        std::fs::write(&in_trash, "two").unwrap();
        assert!(put_back(&in_trash, &origin).is_err());
        assert_eq!(std::fs::read_to_string(&origin).unwrap(), "one", "the file there stays");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
