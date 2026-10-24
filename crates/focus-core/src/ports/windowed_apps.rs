//! The apps the user has open, for picking apps to close.

use super::enforcer::EnforceError;

/// Lists the current user's apps that have a window open.
pub trait WindowedApps: Send + Sync {
    /// App names (executable names without `.exe`) with at least one visible window, each once,
    /// sorted ignoring case. Never Focus itself or the parts of the desktop it must not close.
    fn windowed_apps(&self) -> Result<Vec<String>, EnforceError>;
}
