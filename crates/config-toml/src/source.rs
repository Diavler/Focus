//! The `ConfigSource` adapter that reads a settings file from disk.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use focus_core::ports::{ConfigSource, ConfigSourceError, SettingsEditor};
use focus_core::{AutoStartRule, Settings};

use crate::edit::with_auto_start;
use crate::parse::parse_settings;

/// Reads settings from a TOML file on each `load`. A missing file means default settings (no
/// configurations yet).
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
    fn load(&self) -> Result<Settings, ConfigSourceError> {
        let Some(text) = self.read()? else {
            return Ok(Settings::default());
        };
        parse_settings(&text).map_err(|error| ConfigSourceError::Invalid {
            reason: format!("{}: {error}", self.path.display()),
        })
    }
}

impl SettingsEditor for TomlConfigSource {
    fn set_auto_start(&self, rules: &[AutoStartRule]) -> Result<(), ConfigSourceError> {
        let text = self.read()?.unwrap_or_default();
        let edited = with_auto_start(&text, rules).map_err(|error| ConfigSourceError::Invalid {
            reason: error.to_string(),
        })?;
        self.write(&edited)
    }
}

impl TomlConfigSource {
    /// The file's text, or `None` if there is no file yet.
    fn read(&self) -> Result<Option<String>, ConfigSourceError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(self.unavailable("read", &error)),
        }
    }

    /// Replaces the file atomically: a temp file next to it, flushed, then renamed over it.
    fn write(&self, text: &str) -> Result<(), ConfigSourceError> {
        let temp = self.path.with_extension("toml.tmp");
        let written = std::fs::File::create(&temp).and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        });
        written
            .and_then(|()| std::fs::rename(&temp, &self.path))
            .map_err(|error| {
                let _ = std::fs::remove_file(&temp);
                self.unavailable("write", &error)
            })
    }

    fn unavailable(&self, action: &str, error: &std::io::Error) -> ConfigSourceError {
        ConfigSourceError::Unavailable {
            reason: format!("couldn't {action} {}: {error}", self.path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::{config_source_contract, settings_editor_contract};
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
    fn passes_the_editor_contract() {
        let (_dir, source) = source_with("# mine\n[configs.coding]\nblock = [\"x.com\"]\n");
        settings_editor_contract(&source, &source);
        let text = std::fs::read_to_string(source.path()).unwrap();
        assert!(text.starts_with("# mine"), "comments survive: {text}");
        assert!(!source.path().with_extension("toml.tmp").exists());

        let missing_dir = tempfile::tempdir().unwrap();
        let missing = TomlConfigSource::new(missing_dir.path().join("settings.toml"));
        settings_editor_contract(&missing, &missing);
    }

    #[test]
    fn missing_file_means_no_configs() {
        let dir = tempfile::tempdir().unwrap();
        let source = TomlConfigSource::new(dir.path().join("settings.toml"));
        assert_eq!(source.load(), Ok(Settings::default()));
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
