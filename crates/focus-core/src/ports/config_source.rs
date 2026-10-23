//! Where block configurations come from.

use thiserror::Error;

use crate::config::BlockConfig;

/// Loads the user's block configurations, with shared lists already included.
pub trait ConfigSource: Send + Sync {
    /// Every configuration, in a stable order with unique names.
    fn load(&self) -> Result<Vec<BlockConfig>, ConfigSourceError>;
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
}
