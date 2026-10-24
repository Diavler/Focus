//! One process per role and install: an exclusive lock on a file in `data/`, held for the
//! process lifetime.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::time::Duration;

/// How often, and how long apart, [`InstanceLock::acquire_patiently`] tries before deciding the
/// lock is really held.
const ATTEMPTS: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_millis(100);

/// Proof that this process holds a lock file. The OS releases it when the process exits, even
/// after a crash, so there is never a stale lock to clean up.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Takes the lock at `path`, or returns `None` if another process holds it.
    pub fn acquire(path: &Path) -> io::Result<Option<Self>> {
        Self::try_acquire(path)
    }

    /// Like [`acquire`](Self::acquire), but retries for half a second, for a lock that other
    /// processes check with [`is_held`]: their momentary hold mustn't look like an owner.
    pub fn acquire_patiently(path: &Path) -> io::Result<Option<Self>> {
        for attempt in 1..=ATTEMPTS {
            if let Some(lock) = Self::try_acquire(path)? {
                return Ok(Some(lock));
            }
            if attempt < ATTEMPTS {
                std::thread::sleep(RETRY_DELAY);
            }
        }
        Ok(None)
    }

    fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
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

/// Whether some process holds the lock at `path`. Takes the lock for a moment if it's free.
pub fn is_held(path: &Path) -> io::Result<bool> {
    Ok(InstanceLock::try_acquire(path)?.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_holder_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        assert!(!is_held(&path).unwrap());
        let first = InstanceLock::acquire(&path).unwrap().expect("first lock");
        assert!(is_held(&path).unwrap());
        assert!(
            InstanceLock::acquire(&path).unwrap().is_none(),
            "second is refused"
        );
        drop(first);
        // On Linux a lock belongs to the open file, and a child process being started by another
        // test inherits our open files until it runs its program. Until then the lock stays held,
        // so wait briefly for the release rather than expect it at once.
        let released = (0..20).any(|_| {
            let free = !is_held(&path).unwrap();
            if !free {
                std::thread::sleep(Duration::from_millis(50));
            }
            free
        });
        assert!(released, "free again after release");
        assert!(InstanceLock::acquire_patiently(&path).unwrap().is_some());
    }
}
