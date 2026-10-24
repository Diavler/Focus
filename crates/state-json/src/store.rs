//! Atomic JSON state file with a one-save backup and corrupt-file quarantine.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use focus_core::ports::{PersistedState, StateStore, StateStoreError};
use serde::{Deserialize, Serialize};

/// The file format version this build reads and writes.
const FORMAT_VERSION: u32 = 1;

/// How many `.corrupt` names to try before giving up on quarantining a file.
const MAX_QUARANTINED: u32 = 100;

/// Stores [`PersistedState`] in a JSON file.
///
/// - **Save** writes `state.json.tmp`, flushes it to disk, moves the old `state.json` to
///   `state.json.bak`, then renames the temp file to `state.json`. A crash at any point leaves
///   either the new file or the previous one readable.
/// - **Load** reads `state.json`, falling back to `state.json.bak` if it is missing or corrupt. A
///   corrupt file is renamed to `state.json.corrupt` (or `.corrupt-2`, ...) so it can be inspected
///   and is never read again. If no readable copy is left, load returns
///   [`StateStoreError::Corrupt`] once; the next load returns `None`, so the caller can start fresh.
/// - A file written by a newer version is left alone and reported as unavailable.
#[derive(Debug, Clone)]
pub struct JsonStateStore {
    path: PathBuf,
}

/// The file's top level: a version tag around the state.
#[derive(Serialize)]
struct EnvelopeOut<'a> {
    version: u32,
    state: &'a PersistedState,
}

#[derive(Deserialize)]
struct EnvelopeIn {
    version: u32,
    state: serde_json::Value,
}

/// What reading one file found.
enum Read {
    Missing,
    // Boxed: the state is much larger than the other variants.
    Found(Box<PersistedState>),
    Corrupt(String),
}

impl JsonStateStore {
    /// A store whose main file is `path`; the directory is created on the first save.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The main state file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn backup_path(&self) -> PathBuf {
        with_suffix(&self.path, ".bak")
    }

    fn temp_path(&self) -> PathBuf {
        with_suffix(&self.path, ".tmp")
    }

    /// Reads and decodes one file.
    fn read(path: &Path) -> Result<Read, StateStoreError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Read::Missing),
            Err(error) => return Err(unavailable(path, "couldn't read", &error)),
        };
        let envelope: EnvelopeIn = match serde_json::from_slice(&bytes) {
            Ok(envelope) => envelope,
            Err(error) => return Ok(Read::Corrupt(error.to_string())),
        };
        if envelope.version != FORMAT_VERSION {
            return Err(StateStoreError::Unavailable {
                reason: format!(
                    "{} has format version {}, but this build reads version {FORMAT_VERSION}",
                    path.display(),
                    envelope.version
                ),
            });
        }
        Ok(serde_json::from_value(envelope.state).map_or_else(
            |error| Read::Corrupt(error.to_string()),
            |state| Read::Found(Box::new(state)),
        ))
    }

    /// Reads `path`; a corrupt file is moved aside and reported as `Err(Corrupt)`.
    fn read_or_quarantine(path: &Path) -> Result<Option<PersistedState>, StateStoreError> {
        match Self::read(path)? {
            Read::Missing => Ok(None),
            Read::Found(state) => Ok(Some(*state)),
            Read::Corrupt(reason) => {
                let moved_to = quarantine(path)?;
                Err(StateStoreError::Corrupt {
                    reason: format!(
                        "{}: {reason} (moved to {})",
                        path.display(),
                        moved_to.display()
                    ),
                })
            }
        }
    }
}

impl JsonStateStore {
    /// The saved state, read without changing any file: a corrupt main file falls back to the
    /// backup but isn't moved aside, and nothing is reported as corrupt for the next load. For
    /// processes that only look, such as the native messaging host.
    pub fn peek(&self) -> Result<Option<PersistedState>, StateStoreError> {
        for path in [self.path.clone(), self.backup_path()] {
            if let Read::Found(state) = Self::read(&path)? {
                return Ok(Some(*state));
            }
        }
        Ok(None)
    }
}

impl StateStore for JsonStateStore {
    fn load(&self) -> Result<Option<PersistedState>, StateStoreError> {
        let main = Self::read_or_quarantine(&self.path);
        if let Ok(Some(state)) = main {
            return Ok(Some(state));
        }
        if let Err(StateStoreError::Unavailable { .. }) = main {
            return main;
        }
        // Main is missing (a crash between the two renames in `save`) or was corrupt: use the backup.
        match (Self::read_or_quarantine(&self.backup_path())?, main) {
            (Some(state), _) => Ok(Some(state)),
            (None, main) => main,
        }
    }

    fn save(&self, state: &PersistedState) -> Result<(), StateStoreError> {
        let json = serde_json::to_vec_pretty(&EnvelopeOut {
            version: FORMAT_VERSION,
            state,
        })
        .map_err(|error| StateStoreError::Unavailable {
            reason: format!("couldn't encode state: {error}"),
        })?;
        if let Some(dir) = self.path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            fs::create_dir_all(dir).map_err(|error| unavailable(dir, "couldn't create", &error))?;
        }

        let temp = self.temp_path();
        write_synced(&temp, &json).map_err(|error| unavailable(&temp, "couldn't write", &error))?;
        match fs::rename(&self.path, self.backup_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(unavailable(&self.path, "couldn't back up", &error)),
        }
        fs::rename(&temp, &self.path)
            .map_err(|error| unavailable(&self.path, "couldn't replace", &error))?;
        sync_parent_dir(&self.path);
        Ok(())
    }
}

/// Writes `bytes` to a new file at `path` and waits until they reach the disk.
fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Asks the OS to persist the renames in `path`'s directory. Best effort: Linux supports syncing
/// a directory, Windows can't open one this way and already makes renames durable.
fn sync_parent_dir(path: &Path) {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        let _ = File::open(dir).and_then(|dir| dir.sync_all());
    }
}

/// Renames `path` to the first free `<name>.corrupt`, `<name>.corrupt-2`, ... and returns the new path.
fn quarantine(path: &Path) -> Result<PathBuf, StateStoreError> {
    for n in 1..=MAX_QUARANTINED {
        let suffix = if n == 1 {
            ".corrupt".to_owned()
        } else {
            format!(".corrupt-{n}")
        };
        let target = with_suffix(path, &suffix);
        if target.exists() {
            continue;
        }
        return fs::rename(path, &target)
            .map(|()| target)
            .map_err(|error| unavailable(path, "couldn't move aside corrupt", &error));
    }
    Err(StateStoreError::Unavailable {
        reason: format!(
            "{} is corrupt and {MAX_QUARANTINED} corrupt copies already exist; delete some",
            path.display()
        ),
    })
}

/// `path` with `suffix` appended to its file name (`state.json` → `state.json.bak`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map_or_else(OsString::new, OsString::from);
    name.push(suffix);
    path.with_file_name(name)
}

fn unavailable(path: &Path, action: &str, error: &std::io::Error) -> StateStoreError {
    StateStoreError::Unavailable {
        reason: format!("{action} {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use focus_core::testing::state_store_contract;
    use focus_core::{BlockConfig, Mode, SessionSpec, SessionState, Strictness, Timestamp};
    use tempfile::TempDir;

    use super::*;

    /// A store in its own temporary directory, which lives as long as the store.
    struct TempStore {
        _dir: TempDir,
        store: JsonStateStore,
    }

    impl TempStore {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = JsonStateStore::new(dir.path().join("focus").join("state.json"));
            Self { _dir: dir, store }
        }

        fn file(&self, suffix: &str) -> PathBuf {
            with_suffix(self.store.path(), suffix)
        }
    }

    impl StateStore for TempStore {
        fn load(&self) -> Result<Option<PersistedState>, StateStoreError> {
            self.store.load()
        }

        fn save(&self, state: &PersistedState) -> Result<(), StateStoreError> {
            self.store.save(state)
        }
    }

    fn running(minutes: u64) -> PersistedState {
        let mut session = SessionState::default();
        let spec = SessionSpec {
            config: BlockConfig::new("coding", Mode::Blocklist),
            strictness: Strictness::Locked,
            duration: Duration::from_secs(minutes * 60),
        };
        session
            .start(spec, Timestamp::from_millis(1_767_225_600_000))
            .unwrap();
        PersistedState {
            session,
            ..PersistedState::default()
        }
    }

    #[test]
    fn peeking_changes_no_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonStateStore::new(dir.path().join("state.json"));
        assert_eq!(store.peek(), Ok(None), "nothing saved yet");
        let state = PersistedState::default();
        store.save(&state).unwrap();
        store.save(&state).unwrap();
        assert_eq!(store.peek(), Ok(Some(state.clone())));
        std::fs::write(store.path(), b"{ not json").unwrap();
        assert_eq!(store.peek(), Ok(Some(state)), "falls back to the backup");
        assert_eq!(
            std::fs::read(store.path()).unwrap(),
            b"{ not json",
            "the corrupt file is left where it is"
        );
    }

    #[test]
    fn passes_the_state_store_contract() {
        state_store_contract(TempStore::new);
    }

    #[test]
    fn save_keeps_the_previous_state_as_backup_and_leaves_no_temp_file() {
        let store = TempStore::new();
        store.save(&running(25)).unwrap();
        store.save(&running(50)).unwrap();
        assert!(!store.file(".tmp").exists());
        assert_eq!(
            JsonStateStore::new(store.file(".bak")).load(),
            Ok(Some(running(25)))
        );
        assert_eq!(store.load(), Ok(Some(running(50))));
    }

    #[test]
    fn file_is_versioned_pretty_json() {
        let store = TempStore::new();
        store.save(&PersistedState::default()).unwrap();
        let text = fs::read_to_string(store.store.path()).unwrap();
        assert!(text.starts_with("{\n  \"version\": 1,"), "{text}");
    }

    #[test]
    fn corrupt_main_file_falls_back_to_the_backup() {
        let store = TempStore::new();
        store.save(&running(25)).unwrap();
        store.save(&running(50)).unwrap();
        let half_written = &fs::read(store.store.path()).unwrap()[..40];
        fs::write(store.store.path(), half_written).unwrap();

        assert_eq!(store.load(), Ok(Some(running(25))), "the session survives");
        assert_eq!(
            fs::read(store.file(".corrupt")).unwrap(),
            half_written,
            "kept for inspection"
        );
        assert!(!store.store.path().exists());
    }

    #[test]
    fn corrupt_file_without_backup_is_reported_once_then_starts_fresh() {
        let store = TempStore::new();
        store.save(&running(25)).unwrap();
        fs::write(store.store.path(), b"\x00\x00not json").unwrap();

        let Err(StateStoreError::Corrupt { reason }) = store.load() else {
            panic!("expected a corrupt-state error");
        };
        assert!(reason.contains("state.json.corrupt"), "{reason}");
        assert_eq!(store.load(), Ok(None));
        store.save(&running(5)).unwrap();
        assert_eq!(store.load(), Ok(Some(running(5))));
    }

    #[test]
    fn wrong_shape_is_corrupt_too() {
        let store = TempStore::new();
        store.save(&PersistedState::default()).unwrap();
        fs::write(
            store.store.path(),
            br#"{"version": 1, "state": {"session": "flying"}}"#,
        )
        .unwrap();
        assert!(matches!(store.load(), Err(StateStoreError::Corrupt { .. })));
    }

    #[test]
    fn quarantine_never_overwrites_an_earlier_corrupt_file() {
        let store = TempStore::new();
        for attempt in ["first", "second"] {
            store.save(&PersistedState::default()).unwrap();
            fs::remove_file(store.file(".bak")).ok();
            fs::write(store.store.path(), attempt).unwrap();
            assert!(store.load().is_err());
        }
        assert_eq!(fs::read_to_string(store.file(".corrupt")).unwrap(), "first");
        assert_eq!(
            fs::read_to_string(store.file(".corrupt-2")).unwrap(),
            "second"
        );
    }

    #[test]
    fn crash_between_renames_recovers_the_backup() {
        let store = TempStore::new();
        store.save(&running(25)).unwrap();
        store.save(&running(50)).unwrap();
        // As if the process died after moving state.json to .bak but before renaming .tmp.
        fs::rename(store.store.path(), store.file(".bak")).unwrap();
        fs::write(store.file(".tmp"), b"{\"version\": 1, \"sta").unwrap();

        assert_eq!(store.load(), Ok(Some(running(50))));
        store.save(&running(5)).unwrap();
        assert_eq!(
            store.load(),
            Ok(Some(running(5))),
            "a stale temp file doesn't block saving"
        );
    }

    #[test]
    fn newer_format_is_left_alone() {
        let store = TempStore::new();
        store.save(&PersistedState::default()).unwrap();
        let newer = br#"{"version": 2, "state": {}}"#;
        fs::write(store.store.path(), newer).unwrap();

        assert!(matches!(
            store.load(),
            Err(StateStoreError::Unavailable { .. })
        ));
        assert_eq!(fs::read(store.store.path()).unwrap(), newer);
        assert!(!store.file(".corrupt").exists());
    }
}
