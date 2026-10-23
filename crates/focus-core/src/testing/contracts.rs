//! Shared contract suites. Each function panics with a message if an implementation breaks the
//! port's contract, so every adapter (and every fake) is held to the same behaviour.

use std::collections::HashSet;
use std::time::Duration;

use crate::config::{BlockConfig, Mode};
use crate::ports::{
    Clock, ConfigSource, Enforcer, History, HistoryEvent, Notifier, PersistedState, StateStore,
};
use crate::session::{EndReason, SessionSpec, SessionState};
use crate::strictness::Strictness;
use crate::time::Timestamp;

/// 2020-01-01T00:00:00Z; any working clock is past this.
const SANE_EPOCH: Timestamp = Timestamp::from_millis(1_577_836_800_000);

/// An app name no real system runs, so enforcer contracts never close anything real.
const MISSING_APP: &str = "focus-contract-test-no-such-app";

/// A clock reads a plausible time and doesn't go backwards between two quick reads.
pub fn clock_contract(clock: &dyn Clock) {
    let first = clock.now();
    let second = clock.now();
    assert!(first >= SANE_EPOCH, "clock reads {first:?}, before 2020");
    assert!(
        second >= first,
        "clock went backwards: {first:?} then {second:?}"
    );
}

/// A config source gives the same result twice, and loaded configs have unique, non-empty names.
pub fn config_source_contract(source: &dyn ConfigSource) {
    let first = source.load();
    assert_eq!(first, source.load(), "two loads disagree");
    let Ok(configs) = first else { return };
    let mut names = HashSet::new();
    for config in &configs {
        assert!(!config.name.trim().is_empty(), "a config has an empty name");
        assert!(
            names.insert(&config.name),
            "duplicate config name {:?}",
            config.name
        );
    }
}

/// A fresh store is empty, and a load returns exactly the last state saved.
///
/// `make_empty` must return a new, empty store each time it is called.
pub fn state_store_contract<S: StateStore>(make_empty: impl Fn() -> S) {
    let store = make_empty();
    assert_eq!(store.load(), Ok(None), "a fresh store isn't empty");

    let idle = PersistedState::default();
    assert_eq!(store.save(&idle), Ok(()));
    assert_eq!(
        store.load(),
        Ok(Some(idle)),
        "load didn't return the saved state"
    );

    let running = running_state();
    assert_eq!(store.save(&running), Ok(()));
    assert_eq!(
        store.save(&running),
        Ok(()),
        "saving the same state twice failed"
    );
    assert_eq!(
        store.load(),
        Ok(Some(running)),
        "load didn't return the latest state"
    );

    assert_eq!(
        make_empty().load(),
        Ok(None),
        "stores from `make_empty` share state"
    );
}

/// Closing nothing, or an app that isn't running, closes nothing and succeeds.
///
/// Real enforcers should be run in dry-run mode; this suite never asks to close a real app.
pub fn enforcer_contract(enforcer: &dyn Enforcer) {
    assert_eq!(
        enforcer.close_apps(&[]),
        Ok(Vec::new()),
        "closing nothing closed something"
    );
    assert_eq!(
        enforcer.close_apps(&[MISSING_APP.to_owned()]),
        Ok(Vec::new()),
        "closing an app that isn't running reported a close"
    );
}

/// Silencing and restoring both succeed, including repeated calls in either order.
pub fn notifier_contract(notifier: &dyn Notifier) {
    assert_eq!(notifier.restore(), Ok(()), "restore before silence failed");
    assert_eq!(notifier.silence(), Ok(()));
    assert_eq!(notifier.silence(), Ok(()), "silencing twice failed");
    assert_eq!(notifier.restore(), Ok(()));
    assert_eq!(notifier.restore(), Ok(()), "restoring twice failed");
}

/// Every kind of event can be recorded.
pub fn history_contract(history: &dyn History) {
    let at = SANE_EPOCH;
    let events = [
        HistoryEvent::SessionStarted {
            at,
            config: "coding".into(),
            strictness: Strictness::Deep,
        },
        HistoryEvent::SessionEnded {
            at,
            reason: EndReason::EndedEarly {
                reason: Some("meeting moved".into()),
            },
        },
        HistoryEvent::SessionEnded {
            at,
            reason: EndReason::Completed,
        },
    ];
    for event in &events {
        assert_eq!(history.record(event), Ok(()), "recording {event:?} failed");
    }
}

/// A persisted state with a running Deep session and a pending end request.
fn running_state() -> PersistedState {
    let mut session = SessionState::default();
    let spec = SessionSpec {
        config: BlockConfig::new("coding", Mode::Blocklist),
        strictness: Strictness::Deep,
        duration: Duration::from_secs(25 * 60),
    };
    let started = session.start(spec, SANE_EPOCH);
    assert_eq!(
        started,
        Ok(()),
        "contract fixture failed to start a session"
    );
    let requested = session.request_end_early(SANE_EPOCH);
    assert!(
        requested.is_ok(),
        "contract fixture failed to request an end"
    );
    PersistedState { session }
}
