//! The app the user is looking at.

use super::enforcer::EnforceError;

/// Tells which app owns the window in front.
pub trait ForegroundApp: Send + Sync {
    /// The foreground window's app name (e.g. `chrome`), or `None` if there is no such window.
    fn foreground_app(&self) -> Result<Option<String>, EnforceError>;
}
