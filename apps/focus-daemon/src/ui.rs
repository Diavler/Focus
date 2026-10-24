//! The `UiWatch` port: the UI installed next to this daemon.

use std::path::PathBuf;

use focus_core::ports::{UiWatch, UiWatchError};

/// The UI in the install folder. It holds `data/ui.lock` while running.
#[derive(Debug, Clone)]
pub struct InstalledUi {
    /// The folder holding the executables.
    pub install: PathBuf,
    /// The UI's lock file.
    pub lock: PathBuf,
    /// Passed on, so a daemon the UI restarts is a dry run too.
    pub dry_run: bool,
}

impl UiWatch for InstalledUi {
    fn is_running(&self) -> bool {
        // If the lock can't be checked, assume it's running rather than start UIs in a loop.
        focus_install::is_held(&self.lock).unwrap_or(true)
    }

    fn launch(&self) -> Result<(), UiWatchError> {
        let args: &[&str] = if self.dry_run {
            &["--minimized", "--dry-run"]
        } else {
            &["--minimized"]
        };
        focus_install::spawn_sibling(&self.install, "focus-ui", args).map_err(|error| {
            UiWatchError::Failed {
                reason: error.to_string(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::ui_watch_contract;
    use focus_install::InstanceLock;

    use super::*;

    #[test]
    fn follows_the_ui_lock_and_passes_the_contract() {
        let dir = tempfile::tempdir().unwrap();
        let ui = InstalledUi {
            install: dir.path().to_path_buf(),
            lock: dir.path().join("ui.lock"),
            dry_run: true,
        };
        ui_watch_contract(&ui);
        assert!(!ui.is_running());
        let _held = InstanceLock::acquire(&ui.lock).unwrap().unwrap();
        assert!(ui.is_running());
        ui_watch_contract(&ui);
    }

    #[test]
    fn a_missing_ui_program_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let ui = InstalledUi {
            install: dir.path().to_path_buf(),
            lock: dir.path().join("ui.lock"),
            dry_run: false,
        };
        assert!(ui.launch().is_err());
    }
}
