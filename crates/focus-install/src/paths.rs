//! Where Focus keeps its files: a `data` folder inside the install folder, so the whole program
//! lives in one place (portable install).

use std::path::{Path, PathBuf};

/// Focus's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The folder holding everything below.
    pub data: PathBuf,
    /// The TOML settings the user edits.
    pub settings: PathBuf,
    /// The JSON state the daemon owns.
    pub state: PathBuf,
    /// Locked by the running daemon, so only one runs per install.
    pub daemon_lock: PathBuf,
    /// Locked by the running UI, so only one runs and the daemon can tell whether it's up.
    pub ui_lock: PathBuf,
}

impl Paths {
    /// Everything under `<install>/data/`, where `install` is the folder holding the executables.
    pub fn portable(install: &Path) -> Self {
        let data = install.join("data");
        Self {
            settings: data.join("settings.toml"),
            state: data.join("state.json"),
            daemon_lock: data.join("daemon.lock"),
            ui_lock: data.join("ui.lock"),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_lives_under_the_install_folder() {
        let install = Path::new("Focus");
        let paths = Paths::portable(install);
        let data = install.join("data");
        assert_eq!(paths.data, data);
        assert_eq!(paths.settings, data.join("settings.toml"));
        assert_eq!(paths.state, data.join("state.json"));
        assert_eq!(paths.daemon_lock, data.join("daemon.lock"));
        assert_eq!(paths.ui_lock, data.join("ui.lock"));
    }
}
