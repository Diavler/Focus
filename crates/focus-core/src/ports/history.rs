//! A record of what happened, for the optional Insights module.

use thiserror::Error;

use crate::session::EndReason;
use crate::strictness::Strictness;
use crate::time::Timestamp;

/// Something worth recording about a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEvent {
    /// A session started.
    SessionStarted {
        /// When it started.
        at: Timestamp,
        /// The configuration it ran.
        config: String,
        /// Its strictness.
        strictness: Strictness,
    },
    /// A session ended.
    SessionEnded {
        /// When it ended.
        at: Timestamp,
        /// Why it ended.
        reason: EndReason,
    },
}

/// Records events. Ships as [`NoopHistory`] until the Insights module plugs in storage.
pub trait History: Send + Sync {
    /// Records one event.
    fn record(&self, event: &HistoryEvent) -> Result<(), HistoryError>;
}

/// Why an event couldn't be recorded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HistoryError {
    /// The history store couldn't be written.
    #[error("history unavailable: {reason}")]
    Unavailable {
        /// What went wrong.
        reason: String,
    },
}

/// A [`History`] that discards every event.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopHistory;

impl History for NoopHistory {
    fn record(&self, _event: &HistoryEvent) -> Result<(), HistoryError> {
        Ok(())
    }
}
