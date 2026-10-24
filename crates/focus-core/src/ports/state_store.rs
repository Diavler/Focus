//! Persistence for state that must survive a restart.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cooling::ConfigState;
use crate::login::LoginSetting;
use crate::session::SessionState;

/// Everything the daemon persists between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedState {
    /// The session lifecycle, so a running session resumes after a restart.
    pub session: SessionState,
    /// Configurations in effect and edits still cooling off.
    #[serde(default)]
    pub configs: ConfigState,
    /// Whether Focus starts at login, and a switch-off still cooling off.
    #[serde(default)]
    pub start_at_login: LoginSetting,
}

/// Saves and loads [`PersistedState`].
pub trait StateStore: Send + Sync {
    /// The last saved state, or `None` if nothing has been saved yet.
    fn load(&self) -> Result<Option<PersistedState>, StateStoreError>;

    /// Replaces the saved state. Either the whole new state is saved or the old one is kept.
    fn save(&self, state: &PersistedState) -> Result<(), StateStoreError>;
}

/// Why state couldn't be loaded or saved.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StateStoreError {
    /// The store couldn't be read or written.
    #[error("state store unavailable: {reason}")]
    Unavailable {
        /// What went wrong.
        reason: String,
    },
    /// Saved state exists but can't be understood.
    #[error("saved state is corrupt: {reason}")]
    Corrupt {
        /// What is wrong with it.
        reason: String,
    },
}
