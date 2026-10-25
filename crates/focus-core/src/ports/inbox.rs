//! Sending parked items to the task list's inbox.

use thiserror::Error;

use crate::parking::ParkedItem;
use crate::ports::SourceError;

/// Adds tasks to the task list's inbox.
pub trait TaskInbox: Send + Sync {
    /// Adds `item` to the inbox. Sending the same item again after a lost answer must not add it
    /// twice where the service can tell ([`ParkedItem::request_id`]).
    fn add(&self, item: &ParkedItem) -> Result<(), InboxError>;
}

/// Why an item didn't reach the inbox.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InboxError {
    /// Nothing can be sent now (not set up, offline, the token refused): try again later.
    #[error("{0}")]
    Unavailable(SourceError),
    /// The inbox refused this item; others can still be sent.
    #[error("the task inbox refused it: {reason}")]
    Refused {
        /// Why, worded for the user.
        reason: String,
    },
}

impl From<SourceError> for InboxError {
    fn from(error: SourceError) -> Self {
        Self::Unavailable(error)
    }
}
