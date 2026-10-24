//! Where the plan comes from: calendar events and tasks.

use thiserror::Error;

use crate::plan::{CalendarEvent, Task};
use crate::time::Timestamp;

/// Reads calendar events.
pub trait CalendarSource: Send + Sync {
    /// Events overlapping `from..to` (recurring events expanded into occurrences), sorted by
    /// start. Each event ends no earlier than it starts and has a unique, non-empty ID.
    fn events(&self, from: Timestamp, to: Timestamp) -> Result<Vec<CalendarEvent>, SourceError>;
}

/// Reads open tasks.
pub trait TaskSource: Send + Sync {
    /// Open tasks that are overdue or due soon, each with a unique, non-empty ID.
    fn tasks(&self) -> Result<Vec<Task>, SourceError>;
}

/// Why the plan couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceError {
    /// The user hasn't set this source up (no address or token).
    #[error("not set up")]
    NotConfigured,
    /// The service couldn't be reached (e.g. offline); cached data stays usable.
    #[error("couldn't reach the service: {reason}")]
    Unavailable {
        /// What went wrong.
        reason: String,
    },
    /// The service answered, but refused (e.g. a wrong token) or sent something unreadable.
    #[error("{reason}")]
    Rejected {
        /// What went wrong, worded for the user.
        reason: String,
    },
}
