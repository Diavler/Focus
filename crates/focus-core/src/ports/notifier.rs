//! Silencing desktop notifications during a session, and, where the platform can't do it for the
//! user, opening the notification settings so they can.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What silencing did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Silencing {
    /// Notifications are silenced.
    Silenced,
    /// The platform can't silence them for an app, so the user should: the window offers a button
    /// that opens the notification settings ([`NotificationSettings`]).
    AskUser,
    /// Not supported on this platform yet.
    Unsupported,
}

/// Silences and restores desktop notifications.
pub trait Notifier: Send + Sync {
    /// Silences notifications, or says the user has to. Calling it while silenced is fine.
    fn silence(&self) -> Result<Silencing, NotifyError>;

    /// Restores notifications. Calling it while not silenced is fine.
    fn restore(&self) -> Result<(), NotifyError>;
}

/// Opens the system's notification settings, for platforms where the user has to turn Do Not
/// Disturb on themselves.
pub trait NotificationSettings: Send + Sync {
    /// Opens the settings page.
    fn open(&self) -> Result<(), NotifyError>;
}

/// Why notifications couldn't be silenced or restored, or their settings opened.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NotifyError {
    /// The notification service refused or isn't available.
    #[error("notifications unavailable: {reason}")]
    Unavailable {
        /// What went wrong.
        reason: String,
    },
}
