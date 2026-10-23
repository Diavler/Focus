//! Silencing desktop notifications during a session.

use thiserror::Error;

/// Silences and restores desktop notifications.
pub trait Notifier: Send + Sync {
    /// Silences notifications. Calling it while already silenced is fine.
    fn silence(&self) -> Result<(), NotifyError>;

    /// Restores notifications. Calling it while not silenced is fine.
    fn restore(&self) -> Result<(), NotifyError>;
}

/// Why notifications couldn't be silenced or restored.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NotifyError {
    /// The notification service refused or isn't available.
    #[error("notifications unavailable: {reason}")]
    Unavailable {
        /// What went wrong.
        reason: String,
    },
}
