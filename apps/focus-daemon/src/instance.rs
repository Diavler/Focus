//! One daemon per install: an exclusive lock on `data/daemon.lock`, held for the process lifetime.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// Proof that this process is the only daemon for the install. The OS releases the lock when the
/// process exits, even after a crash, so there is never a stale lock to clean up.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Takes the lock at `path`, or returns `None` if another process holds it.
    pub fn acquire(path: &Path) -> io::Result<Option<Self>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_holder_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        let first = InstanceLock::acquire(&path).unwrap().expect("first lock");
        assert!(
            InstanceLock::acquire(&path).unwrap().is_none(),
            "second is refused"
        );
        drop(first);
        assert!(
            InstanceLock::acquire(&path).unwrap().is_some(),
            "free again after release"
        );
    }
}
