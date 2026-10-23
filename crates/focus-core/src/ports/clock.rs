//! The source of the current time.

use crate::time::Timestamp;

/// Tells the current wall-clock time. The core never reads the system clock directly.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> Timestamp;
}
