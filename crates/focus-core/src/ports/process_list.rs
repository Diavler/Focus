//! Which apps are running.

use super::enforcer::EnforceError;

/// Tells which apps the current user is running.
pub trait ProcessList: Send + Sync {
    /// The names in `apps` with at least one running process of the current user, each once, in
    /// the order given. Names match case-insensitively, with or without `.exe`.
    fn running_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError>;
}
