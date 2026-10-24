//! Starting Focus when the user logs in.

use thiserror::Error;

/// Registers (or removes) Focus to start at login, for the current user only.
pub trait Autostart: Send + Sync {
    /// Makes Focus start at login, or stop doing so. Repeating a call is fine; registering again
    /// also updates where Focus is installed.
    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError>;
}

/// Why start-at-login couldn't be changed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AutostartError {
    /// The OS refused or the platform API failed.
    #[error("couldn't change start at login: {reason}")]
    Failed {
        /// What went wrong.
        reason: String,
    },
}
