//! Credentials kept out of the settings file.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Which credential. A closed set, so the daemon can't be asked to store arbitrary secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKey {
    /// The calendar's private iCal address, which anyone holding it can read.
    CalendarUrl,
    /// The Todoist personal API token.
    TodoistToken,
}

impl SecretKey {
    /// Every key.
    pub const ALL: [Self; 2] = [Self::CalendarUrl, Self::TodoistToken];

    /// A stable name for storage, e.g. `calendar-url`.
    pub fn name(self) -> &'static str {
        match self {
            Self::CalendarUrl => "calendar-url",
            Self::TodoistToken => "todoist-token",
        }
    }
}

/// Stores credentials for the current user (the OS keychain in production).
pub trait Secrets: Send + Sync {
    /// The stored value, or `None` if there isn't one.
    fn load(&self, key: SecretKey) -> Result<Option<String>, SecretsError>;

    /// Stores `value`, or with `None` removes the credential. Removing a missing one is fine.
    fn store(&self, key: SecretKey, value: Option<&str>) -> Result<(), SecretsError>;
}

/// Why a credential couldn't be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretsError {
    /// The keychain refused or isn't available.
    #[error("the keychain is unavailable: {reason}")]
    Unavailable {
        /// What went wrong.
        reason: String,
    },
}
