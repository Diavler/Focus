//! Where the daemon keeps its files: a `data` folder inside the install folder, so the whole
//! program lives in one place (portable install).

use std::path::{Path, PathBuf};

/// The daemon's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The folder holding everything below.
    pub data: PathBuf,
    /// The TOML settings the user edits.
    pub settings: PathBuf,
    /// The JSON state the daemon owns.
    pub state: PathBuf,
}

impl Paths {
    /// `<install>/data/settings.toml` and `<install>/data/state.json`, where `install` is the
    /// folder holding the executables.
    pub fn portable(install: &Path) -> Self {
        let data = install.join("data");
        Self {
            settings: data.join("settings.toml"),
            state: data.join("state.json"),
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
        assert_eq!(paths.data, install.join("data"));
        assert_eq!(paths.settings, install.join("data").join("settings.toml"));
        assert_eq!(paths.state, install.join("data").join("state.json"));
    }
}
