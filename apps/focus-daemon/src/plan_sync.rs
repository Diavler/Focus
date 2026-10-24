//! Fetching the plan (calendar events and tasks) without holding the service's lock, deciding
//! when to fetch again, and folding results into the cache.

use std::sync::Arc;
use std::time::Duration;

use focus_core::ports::{CalendarSource, SourceError, TaskSource};
use focus_core::{CalendarEvent, Fetched, Task, Timestamp};
use ipc_proto::{SourceState, SourceStatus};

/// How often the plan is fetched (and right away after a credential changes).
pub(crate) const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);

/// Fetched events reach back this far, so one that started earlier and is still running counts.
const LOOK_BEHIND: Duration = Duration::from_secs(12 * 60 * 60);

/// Fetched events reach this far ahead: how long automatic starts keep working offline.
const LOOK_AHEAD: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// One fetch of both sources, run on a blocking thread without the service's lock, because the
/// network can take many seconds.
pub struct PlanFetch {
    calendar: Arc<dyn CalendarSource>,
    tasks: Arc<dyn TaskSource>,
    from: Timestamp,
    to: Timestamp,
}

/// What a [`PlanFetch`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFetched {
    pub(crate) calendar: Result<Vec<CalendarEvent>, SourceError>,
    pub(crate) tasks: Result<Vec<Task>, SourceError>,
}

impl PlanFetch {
    /// A fetch of the window around `now`.
    pub(crate) fn new(
        calendar: Arc<dyn CalendarSource>,
        tasks: Arc<dyn TaskSource>,
        now: Timestamp,
    ) -> Self {
        Self {
            calendar,
            tasks,
            from: now.saturating_sub(LOOK_BEHIND),
            to: now.saturating_add(LOOK_AHEAD),
        }
    }

    /// Reads both sources. Blocks on the network. A source that panics (a parser bug on odd data)
    /// counts as rejected, so the other source's result is still kept.
    pub fn run(self) -> PlanFetched {
        let (calendar, tasks) = (&self.calendar, &self.tasks);
        PlanFetched {
            calendar: guarded(|| calendar.events(self.from, self.to)),
            tasks: guarded(|| tasks.tasks()),
        }
    }
}

/// Runs one source's read, turning a panic into an error.
fn guarded<T>(read: impl FnOnce() -> Result<T, SourceError>) -> Result<T, SourceError> {
    // `AssertUnwindSafe`: nothing the closure touches is used again after a panic except the
    // adapter, which holds no state a half-finished read could corrupt.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).unwrap_or_else(|_| {
        Err(SourceError::Rejected {
            reason: "reading it failed unexpectedly (see daemon.log)".into(),
        })
    })
}

/// When the next fetch is due.
#[derive(Debug, Default)]
pub(crate) struct SyncSchedule {
    last: Option<Timestamp>,
    requested: bool,
}

impl SyncSchedule {
    /// Whether a fetch is due at `now`; if so, it counts as started.
    pub(crate) fn take_due(&mut self, now: Timestamp) -> bool {
        // A last fetch "in the future" (the clock went back) counts as long ago.
        let stale = self
            .last
            .is_none_or(|last| now < last || now.saturating_duration_since(last) >= SYNC_EVERY);
        if !(stale || self.requested) {
            return false;
        }
        self.requested = false;
        self.last = Some(now);
        true
    }

    /// Makes a fetch due at the next check.
    pub(crate) fn request(&mut self) {
        self.requested = true;
    }
}

/// Folds one source's result into its cache: new items replace the old, an unset source clears
/// it, and a failure keeps it for offline use.
pub(crate) fn merge<T>(
    cache: &mut Option<Fetched<T>>,
    result: &Result<Vec<T>, SourceError>,
    now: Timestamp,
) where
    T: Clone,
{
    match result {
        Ok(items) => {
            *cache = Some(Fetched {
                at: now,
                items: items.clone(),
            });
        }
        Err(SourceError::NotConfigured) => *cache = None,
        Err(_) => {}
    }
}

/// What a sync result means for the status line.
pub(crate) fn source_state<T>(result: &Result<Vec<T>, SourceError>) -> SourceState {
    match result {
        Ok(_) => SourceState::Ok,
        Err(SourceError::NotConfigured) => SourceState::NotConfigured,
        Err(error) => SourceState::Failing {
            problem: error.to_string(),
            offline: matches!(error, SourceError::Unavailable { .. }),
        },
    }
}

/// The status of one source, from its last sync and its cache.
pub(crate) fn source_status<T>(state: &SourceState, cache: Option<&Fetched<T>>) -> SourceStatus {
    SourceStatus {
        state: state.clone(),
        fetched_at: cache.map(|fetched| fetched.at),
        items: cache.map_or(0, |fetched| fetched.items.len()),
    }
}

#[cfg(test)]
mod tests {
    use focus_core::ports::Clock;
    use focus_core::testing::FakeClock;

    use super::*;

    #[test]
    fn fetches_at_start_then_every_few_minutes_or_on_request() {
        let clock = FakeClock::default();
        let mut schedule = SyncSchedule::default();
        assert!(schedule.take_due(clock.now()), "due at startup");
        assert!(!schedule.take_due(clock.now()), "not twice in a row");

        clock.advance(SYNC_EVERY - Duration::from_secs(1));
        assert!(!schedule.take_due(clock.now()));
        schedule.request();
        assert!(schedule.take_due(clock.now()), "due once requested");

        clock.advance(SYNC_EVERY);
        assert!(schedule.take_due(clock.now()), "due after the interval");

        clock.set(Timestamp::from_millis(0));
        assert!(
            schedule.take_due(clock.now()),
            "a clock that went back doesn't stall it"
        );
    }

    #[test]
    fn a_panicking_source_doesnt_lose_the_other() {
        struct Broken;
        impl TaskSource for Broken {
            fn tasks(&self) -> Result<Vec<Task>, SourceError> {
                panic!("parser bug");
            }
        }
        let now = FakeClock::default().now();
        let calendar = Arc::new(focus_core::testing::FakeCalendar::new(Vec::new()));
        let fetched = PlanFetch::new(calendar, Arc::new(Broken), now).run();
        assert_eq!(fetched.calendar, Ok(Vec::new()));
        assert!(matches!(fetched.tasks, Err(SourceError::Rejected { .. })));
    }

    #[test]
    fn failures_keep_the_cache_and_unset_sources_clear_it() {
        let now = FakeClock::default().now();
        let mut cache: Option<Fetched<u8>> = None;
        merge(&mut cache, &Ok(vec![1, 2]), now);
        assert_eq!(cache.as_ref().map(|c| c.items.clone()), Some(vec![1, 2]));

        let offline = Err(SourceError::Unavailable {
            reason: "offline".into(),
        });
        merge(&mut cache, &offline, now.saturating_add(SYNC_EVERY));
        assert_eq!(cache.as_ref().map(|c| c.at), Some(now), "kept from before");
        assert_eq!(
            source_state(&offline),
            SourceState::Failing {
                problem: "couldn't reach the service: offline".into(),
                offline: true
            }
        );
        assert_eq!(
            source_status(&source_state(&offline), cache.as_ref()).items,
            2
        );

        merge(&mut cache, &Err(SourceError::NotConfigured), now);
        assert_eq!(cache, None, "removing the token drops the cached plan");
    }
}
