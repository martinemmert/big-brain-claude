//! Moving files to the macOS Trash, where Finder can restore them.

use std::path::Path;

use objc2_foundation::{NSFileManager, NSString, NSURL};

pub fn move_to_trash(path: &Path) -> Result<(), String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|err| err.localizedDescription().to_string())
}
