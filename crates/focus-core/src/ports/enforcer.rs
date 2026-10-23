//! OS-level enforcement outside the browser.

use thiserror::Error;

/// Closes blocked desktop apps belonging to the current user.
pub trait Enforcer: Send + Sync {
    /// Closes running apps whose executable name matches one in `apps` (case-insensitive) and
    /// returns the names it closed, each at most once. Apps that aren't running are skipped.
    fn close_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError>;
}

/// Why enforcement failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EnforceError {
    /// The OS refused or the platform API failed.
    #[error("enforcement failed: {reason}")]
    Failed {
        /// What went wrong.
        reason: String,
    },
}
