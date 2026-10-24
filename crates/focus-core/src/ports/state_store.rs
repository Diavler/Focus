//! Persistence for state that must survive a restart.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::auto_start::AutoStartLog;
use crate::cooling::ConfigState;
use crate::delay::DelayLadder;
use crate::friction::FrictionSetting;
use crate::login::LoginSetting;
use crate::plan::PlanCache;
use crate::session::SessionState;
use crate::unlock::UnlockLedger;

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
    /// The last calendar and task fetches, for automatic starts while offline.
    #[serde(default)]
    pub plan: PlanCache,
    /// Planned sessions that have already started.
    #[serde(default)]
    pub auto_started: AutoStartLog,
    /// The friction limits in effect, and a loosening still cooling off.
    #[serde(default)]
    pub friction: FrictionSetting,
    /// Unlocks spent in the last 24 hours.
    #[serde(default)]
    pub unlocks: UnlockLedger,
    /// Visits to Delay sites during the running session.
    #[serde(default)]
    pub delays: DelayLadder,
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
