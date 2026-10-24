//! Sending parked items to the task list's inbox.

use crate::parking::ParkedItem;
use crate::ports::SourceError;

/// Adds tasks to the task list's inbox.
pub trait TaskInbox: Send + Sync {
    /// Adds `item` to the inbox. Sending the same item again after a lost answer must not add it
    /// twice where the service can tell ([`ParkedItem::request_id`]). [`SourceError::NotConfigured`]
    /// if no task list is set up.
    fn add(&self, item: &ParkedItem) -> Result<(), SourceError>;
}
