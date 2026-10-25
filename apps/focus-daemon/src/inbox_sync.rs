//! Sending parked items to the task inbox without holding the service's lock, and deciding when
//! to try again.

use std::sync::Arc;
use std::time::Duration;

use focus_core::ports::{InboxError, SourceError, TaskInbox};
use focus_core::{ParkedItem, Timestamp};
use ipc_proto::SourceState;

/// How long to wait after a failed send (offline, or the service refused the token).
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
    /// The items the inbox refused, and why; they aren't sent again.
    pub(crate) refused: Vec<(u64, String)>,
    /// Why the send stopped early, if it did.
    pub(crate) failure: Option<SourceError>,
}

impl InboxSend {
    /// Sends each item in order. An item the inbox refuses is skipped; the send stops when the
    /// inbox can't take anything (the rest would fail the same way). Blocks on the network.
    pub fn run(self) -> InboxSent {
        let mut outcome = InboxSent {
            sent: Vec::new(),
            refused: Vec::new(),
            failure: None,
        };
        for item in &self.items {
            match self.inbox.add(item) {
                Ok(()) => outcome.sent.push(item.id),
                Err(InboxError::Refused { reason }) => outcome.refused.push((item.id, reason)),
                Err(InboxError::Unavailable(failure)) => {
                    outcome.failure = Some(failure);
                    break;
                }
            }
        }
        outcome
    }
}

/// When to send next, and how the last send went.
#[derive(Debug, Default)]
pub(crate) struct InboxSchedule {
    /// No send before this, after a failure (or while one is running).
    not_before: Option<Timestamp>,
    /// Something was parked, or a credential changed: send at the next check.
    requested: bool,
    /// How the last send went, for `Status`.
    state: SourceState,
}

impl InboxSchedule {
    /// Whether a send is due at `now` (checked before copying anything to send).
    pub(crate) fn due(&self, now: Timestamp) -> bool {
        // A wait "in the future" past the longest retry (the clock went back) counts as over.
        let waiting_out = self.not_before.is_some_and(|not_before| {
            now < not_before && not_before.saturating_duration_since(now) <= UNCONFIGURED_RETRY
        });
        self.requested || !waiting_out
    }

    /// Starts a send of `items` at `now`.
    pub(crate) fn start(
        &mut self,
        inbox: &Arc<dyn TaskInbox>,
        items: Vec<ParkedItem>,
        now: Timestamp,
    ) -> InboxSend {
        self.requested = false;
        // Until the result is stored, another check mustn't start a second send.
        self.not_before = Some(now.saturating_add(RETRY_AFTER));
        InboxSend {
            inbox: Arc::clone(inbox),
            items,
        }
    }

    /// Records how a send went at `now`.
    pub(crate) fn finish(&mut self, sent: &InboxSent, now: Timestamp) {
        match &sent.failure {
            None => {
                self.state = SourceState::Ok;
                self.not_before = None;
            }
            Some(failure) => {
                let wait = match failure {
                    SourceError::NotConfigured => UNCONFIGURED_RETRY,
                    _ => RETRY_AFTER,
                };
                self.state = match failure {
                    SourceError::NotConfigured => SourceState::NotConfigured,
                    other => SourceState::Failing {
                        problem: other.to_string(),
                        offline: matches!(other, SourceError::Unavailable { .. }),
                    },
                };
                self.not_before = Some(now.saturating_add(wait));
            }
        }
    }

    /// Makes a send due at the next check.
    pub(crate) fn request(&mut self) {
        self.requested = true;
    }

    /// How the last send went.
    pub(crate) fn state(&self) -> &SourceState {
        &self.state
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

    fn send(inbox: &Arc<dyn TaskInbox>, texts: &[&str]) -> InboxSent {
        InboxSchedule::default()
            .start(inbox, waiting(texts), NOW)
            .run()
    }

    #[test]
    fn sends_in_order_and_stops_when_the_inbox_is_unavailable() {
        let fake = Arc::new(FakeInbox::default());
        let inbox: Arc<dyn TaskInbox> = fake.clone();
        assert_eq!(
            send(&inbox, &["a", "b"]),
            InboxSent {
                sent: vec![0, 1],
                refused: Vec::new(),
                failure: None
            }
        );
        fake.fail_with(Some(SourceError::Unavailable {
            reason: "offline".into(),
        }));
        let sent = send(&inbox, &["c", "d"]);
        assert!(sent.sent.is_empty());
        assert!(sent.failure.is_some());
    }

    #[test]
    fn a_refused_item_doesnt_hold_up_the_rest() {
        let fake = Arc::new(FakeInbox::default());
        fake.refuse("way too long");
        let inbox: Arc<dyn TaskInbox> = fake.clone();
        let sent = send(&inbox, &["way too long", "fine"]);
        assert_eq!(sent.sent, [1]);
        assert_eq!(sent.refused.len(), 1);
        assert_eq!(sent.refused[0].0, 0);
        assert_eq!(sent.failure, None);
    }

    #[test]
    fn waits_after_a_failure_unless_asked() {
        let inbox: Arc<dyn TaskInbox> = Arc::new(FakeInbox::default());
        let mut schedule = InboxSchedule::default();
        assert!(schedule.due(NOW));
        schedule.start(&inbox, waiting(&["a"]), NOW);
        assert!(!schedule.due(NOW), "one send at a time");
        let failed = InboxSent {
            sent: Vec::new(),
            refused: Vec::new(),
            failure: Some(SourceError::NotConfigured),
        };
        schedule.finish(&failed, NOW);
        assert_eq!(schedule.state(), &SourceState::NotConfigured);
        let later = NOW.saturating_add(RETRY_AFTER);
        assert!(!schedule.due(later), "not set up: waits longer");
        schedule.request();
        assert!(
            schedule.due(later),
            "a request (new item or token) sends at once"
        );
        schedule.start(&inbox, waiting(&["a"]), later);
        schedule.finish(
            &InboxSent {
                sent: vec![0],
                refused: Vec::new(),
                failure: None,
            },
            later,
        );
        assert_eq!(schedule.state(), &SourceState::Ok);
        assert!(schedule.due(later));
    }

    #[test]
    fn a_clock_turned_back_doesnt_stop_sending() {
        let inbox: Arc<dyn TaskInbox> = Arc::new(FakeInbox::default());
        let mut schedule = InboxSchedule::default();
        schedule.start(&inbox, waiting(&["a"]), NOW);
        assert!(schedule.due(Timestamp::from_millis(1_000)));
    }
}
