//! The `ConfigSource` adapter that reads a settings file from disk.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use focus_core::BlockConfig;
use focus_core::ports::{ConfigSource, ConfigSourceError};

use crate::parse::parse_settings;

/// Reads configurations from a TOML file on each `load`. A missing file means no configurations yet.
#[derive(Debug, Clone)]
pub struct TomlConfigSource {
    path: PathBuf,
}

impl TomlConfigSource {
    /// A source reading the settings file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The settings file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ConfigSource for TomlConfigSource {
    fn load(&self) -> Result<Vec<BlockConfig>, ConfigSourceError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(ConfigSourceError::Unavailable {
                    reason: format!("couldn't read {}: {error}", self.path.display()),
                });
            }
        };
        parse_settings(&text).map_err(|error| ConfigSourceError::Invalid {
            reason: format!("{}: {error}", self.path.display()),
        })
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::config_source_contract;
    use tempfile::TempDir;

    use super::*;

    fn source_with(text: &str) -> (TempDir, TomlConfigSource) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, text).unwrap();
        (dir, TomlConfigSource::new(path))
    }

    #[test]
    fn passes_the_contract_for_valid_missing_and_invalid_files() {
        let (_valid_dir, valid) = source_with("[configs.coding]\nblock = [\"x.com\"]\n");
        config_source_contract(&valid);

        let missing_dir = tempfile::tempdir().unwrap();
        config_source_contract(&TomlConfigSource::new(missing_dir.path().join("none.toml")));

        let (_invalid_dir, invalid) = source_with("[configs.coding]\nmode = \"allowlist\"\n");
        config_source_contract(&invalid);
    }

    #[test]
    fn missing_file_means_no_configs() {
        let dir = tempfile::tempdir().unwrap();
        let source = TomlConfigSource::new(dir.path().join("settings.toml"));
        assert_eq!(source.load(), Ok(Vec::new()));
    }

    #[test]
    fn invalid_settings_name_the_file() {
        let (_dir, source) = source_with("[configs.coding]\ninclude = [\"nope\"]\n");
        let Err(ConfigSourceError::Invalid { reason }) = source.load() else {
            panic!("expected invalid settings");
        };
        assert!(reason.contains("settings.toml"), "{reason}");
        assert!(reason.contains("no list named \"nope\""), "{reason}");
    }

    #[test]
    fn unreadable_path_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let source = TomlConfigSource::new(dir.path());
        assert!(matches!(
            source.load(),
            Err(ConfigSourceError::Unavailable { .. })
        ));
    }
}
