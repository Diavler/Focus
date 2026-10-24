//! Keeping the UI running. The UI is the daemon's watchdog (it restarts a killed daemon), so
//! during a session the daemon brings the UI back if it's gone (ADR 0008).

use thiserror::Error;

/// Tells whether the UI is running, and starts it.
pub trait UiWatch: Send + Sync {
    /// Whether a UI is running for this install. Asking has no side effects anyone can see.
    fn is_running(&self) -> bool;

    /// Starts the UI, minimised.
    fn launch(&self) -> Result<(), UiWatchError>;
}

/// Why the UI couldn't be started.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UiWatchError {
    /// The program couldn't be started.
    #[error("couldn't start the UI: {reason}")]
    Failed {
        /// What went wrong.
        reason: String,
    },
}
