//! The native macOS folder dialog, for "Choose folder …" in the new-session dialog.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{NSString, NSURL};

/// Asks for a folder, starting in the home folder. Blocks while the dialog is open; `None` when
/// cancelled, or when not called on the main thread.
pub fn choose_folder() -> Option<std::path::PathBuf> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
    let home = brain_core::account::home_dir();
    panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&home.display().to_string()))));
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    let path = panel.URL()?.path()?;
    Some(std::path::PathBuf::from(path.to_string()))
}
