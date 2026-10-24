//! Where block configurations come from.

use thiserror::Error;

use crate::login::Settings;

/// Loads the user's settings: block configurations (with shared lists already included) and
/// app settings.
pub trait ConfigSource: Send + Sync {
    /// The settings; configurations come in a stable order with unique names.
    fn load(&self) -> Result<Settings, ConfigSourceError>;
}

/// Why configurations couldn't be loaded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigSourceError {
    /// The settings couldn't be read at all.
    #[error("settings unavailable: {reason}")]
    Unavailable {
        /// What went wrong.
        reason: String,
    },
    /// The settings were read but aren't valid.
    #[error("invalid settings: {reason}")]
    Invalid {
        /// What is wrong, worded for the user.
        reason: String,
    },
    /// An edit was refused because the settings changed since they were read (e.g. edited by
    /// hand in the meantime).
    #[error("the settings changed since they were opened; reload them and try again")]
    Changed,
}
