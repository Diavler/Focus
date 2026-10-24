//! Sending parked items to the task inbox without holding the service's lock, and deciding when
//! to try again.

use std::sync::Arc;
use std::time::Duration;

use focus_core::ports::{SourceError, TaskInbox};
use focus_core::{ParkedItem, Timestamp};

/// How long to wait after a failed send (offline, or the service refused).
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// How long to wait when no task inbox is set up; parking right after one is set up sends at once.
const UNCONFIGURED_RETRY: Duration = Duration::from_secs(10 * 60);

/// One send of the items waiting, run on a blocking thread without the service's lock, because
/// the network can take many seconds.
pub struct InboxSend {
    inbox: Arc<dyn TaskInbox>,
    items: Vec<ParkedItem>,
}

/// What an [`InboxSend`] managed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxSent {
    /// The items that reached the inbox.
    pub(crate) sent: Vec<u64>,
    /// Why the send stopped early, if it did.
    pub(crate) failure: Option<SourceError>,
}

impl InboxSend {
    /// Sends each item in order, stopping at the first failure (the rest would fail the same way).
    /// Blocks on the network.
    pub fn run(self) -> InboxSent {
        let mut sent = Vec::new();
        for item in &self.items {
            match self.inbox.add(item) {
                Ok(()) => sent.push(item.id),
                Err(failure) => {
                    return InboxSent {
                        sent,
                        failure: Some(failure),
                    };
                }
            }
        }
        InboxSent {
            sent,
            failure: None,
        }
    }
}

/// When to send next, and why the last send failed.
#[derive(Debug, Default)]
pub(crate) struct InboxSchedule {
    /// No send before this, after a failure (or while one is running).
    not_before: Option<Timestamp>,
    /// Something was parked, or a credential changed: send at the next check.
    requested: bool,
    /// Why the last send failed, for `Status`.
    problem: Option<String>,
}

impl InboxSchedule {
    /// The send to run for `waiting` at `now`, if one is due; it then counts as started.
    pub(crate) fn take_due(
        &mut self,
        inbox: &Arc<dyn TaskInbox>,
        waiting: Vec<ParkedItem>,
        now: Timestamp,
    ) -> Option<InboxSend> {
        if waiting.is_empty() {
            return None;
        }
        // A wait "in the future" past the retry (the clock went back) counts as over.
        let waiting_out = self.not_before.is_some_and(|not_before| {
            now < not_before && not_before.saturating_duration_since(now) <= UNCONFIGURED_RETRY
        });
        if waiting_out && !self.requested {
            return None;
        }
        self.requested = false;
        // Until the result is stored, another check mustn't start a second send.
        self.not_before = Some(now.saturating_add(RETRY_AFTER));
        Some(InboxSend {
            inbox: Arc::clone(inbox),
            items: waiting,
        })
    }

    /// Records how a send went at `now`.
    pub(crate) fn finish(&mut self, sent: &InboxSent, now: Timestamp) {
        match &sent.failure {
            None => {
                self.problem = None;
                self.not_before = None;
            }
            Some(failure) => {
                let wait = match failure {
                    SourceError::NotConfigured => UNCONFIGURED_RETRY,
                    _ => RETRY_AFTER,
                };
                self.problem = Some(failure.to_string());
                self.not_before = Some(now.saturating_add(wait));
            }
        }
    }

    /// Makes a send due at the next check.
    pub(crate) fn request(&mut self) {
        self.requested = true;
    }

    /// Why the last send failed, if it did.
    pub(crate) fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use focus_core::ParkingLot;
    use focus_core::testing::FakeInbox;

    use super::*;

    const NOW: Timestamp = Timestamp::from_millis(10_000_000);

    fn waiting(texts: &[&str]) -> Vec<ParkedItem> {
        let mut lot = ParkingLot::default();
        for text in texts {
            lot.park(text, None, NOW).unwrap();
        }
        lot.unsent()
    }

    #[test]
    fn sends_in_order_and_stops_at_a_failure() {
        let fake = Arc::new(FakeInbox::default());
        let inbox: Arc<dyn TaskInbox> = fake.clone();
        let mut schedule = InboxSchedule::default();
        let send = schedule
            .take_due(&inbox, waiting(&["a", "b"]), NOW)
            .unwrap();
        assert_eq!(
            send.run(),
            InboxSent {
                sent: vec![0, 1],
                failure: None
            }
        );
        fake.fail_with(Some(SourceError::Unavailable {
            reason: "offline".into(),
        }));
        let send = InboxSend {
            inbox: Arc::clone(&inbox),
            items: waiting(&["c", "d"]),
        };
        let sent = send.run();
        assert!(sent.sent.is_empty());
        assert!(sent.failure.is_some());
    }

    #[test]
    fn waits_after_a_failure_unless_asked() {
        let inbox: Arc<dyn TaskInbox> = Arc::new(FakeInbox::default());
        let mut schedule = InboxSchedule::default();
        assert!(
            schedule.take_due(&inbox, Vec::new(), NOW).is_none(),
            "nothing waiting"
        );
        assert!(schedule.take_due(&inbox, waiting(&["a"]), NOW).is_some());
        assert!(
            schedule.take_due(&inbox, waiting(&["a"]), NOW).is_none(),
            "one send at a time"
        );
        let failed = InboxSent {
            sent: Vec::new(),
            failure: Some(SourceError::NotConfigured),
        };
        schedule.finish(&failed, NOW);
        assert_eq!(schedule.problem(), Some("not set up"));
        let later = NOW.saturating_add(RETRY_AFTER);
        assert!(schedule.take_due(&inbox, waiting(&["a"]), later).is_none());
        schedule.request();
        assert!(
            schedule.take_due(&inbox, waiting(&["a"]), later).is_some(),
            "a request (new item or token) sends at once"
        );
        schedule.finish(
            &InboxSent {
                sent: vec![0],
                failure: None,
            },
            later,
        );
        assert_eq!(schedule.problem(), None);
        assert!(schedule.take_due(&inbox, waiting(&["b"]), later).is_some());
    }

    #[test]
    fn a_clock_turned_back_doesnt_stop_sending() {
        let inbox: Arc<dyn TaskInbox> = Arc::new(FakeInbox::default());
        let mut schedule = InboxSchedule::default();
        schedule.take_due(&inbox, waiting(&["a"]), NOW);
        let back = Timestamp::from_millis(1_000);
        assert!(schedule.take_due(&inbox, waiting(&["a"]), back).is_some());
    }
}
