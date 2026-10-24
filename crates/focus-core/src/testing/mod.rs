//! Fakes for every port and the contract suites every implementation must pass.
//!
//! Enabled by the `testing` feature. Adapters add
//! `focus-core = { workspace = true, features = ["testing"] }` to their dev-dependencies and run
//! the matching `*_contract` function from their own tests.

mod contracts;
mod fakes;

pub use contracts::{
    autostart_contract, calendar_source_contract, clock_contract, config_source_contract,
    enforcer_contract, foreground_app_contract, history_contract, notifier_contract,
    process_list_contract, secrets_contract, state_store_contract, task_source_contract,
    ui_watch_contract,
};
pub use fakes::{
    FakeAutostart, FakeCalendar, FakeClock, FakeConfigSource, FakeEnforcer, FakeForegroundApp,
    FakeNotifier, FakeTasks, FakeUi, MemorySecrets, MemoryStateStore, RecordingHistory,
};

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::ports::{CalendarSource, Clock, ConfigSourceError, NoopHistory, SourceError};
    use crate::{BlockConfig, CalendarEvent, Mode, Task};

    #[test]
    fn fakes_pass_their_contracts() {
        clock_contract(&FakeClock::default());
        config_source_contract(&FakeConfigSource::new(vec![
            BlockConfig::new("coding", Mode::Blocklist),
            BlockConfig::new("writing", Mode::Allowlist),
        ]));
        config_source_contract(&FakeConfigSource::failing(ConfigSourceError::Unavailable {
            reason: "no file".into(),
        }));
        state_store_contract(MemoryStateStore::default);
        enforcer_contract(&FakeEnforcer::default());
        enforcer_contract(&FakeEnforcer::with_running(["discord", "Steam"]));
        process_list_contract(&FakeEnforcer::with_running(["discord", "Steam"]));
        let foreground = FakeForegroundApp::default();
        foreground_app_contract(&foreground);
        foreground.set(Some("chrome"));
        foreground_app_contract(&foreground);
        notifier_contract(&FakeNotifier::default());
        history_contract(&RecordingHistory::default());
        history_contract(&NoopHistory);
        autostart_contract(&FakeAutostart::default());
        ui_watch_contract(&FakeUi::default());
        ui_watch_contract(&FakeUi::running());
        secrets_contract(&MemorySecrets::default());
    }

    #[test]
    fn plan_fakes_pass_their_contracts() {
        let tasks = FakeTasks::default();
        task_source_contract(&tasks);
        tasks.set(Ok(vec![Task {
            id: "1".into(),
            title: "Write".into(),
            labels: vec!["deep".into()],
            due_at: None,
            duration: None,
        }]));
        task_source_contract(&tasks);
        tasks.set(Err(SourceError::Unavailable {
            reason: "offline".into(),
        }));
        task_source_contract(&tasks);

        let start = FakeClock::default().now();
        let hour = Duration::from_secs(60 * 60);
        let event = |id: &str, from: Duration, to: Duration| CalendarEvent {
            id: id.into(),
            title: id.into(),
            start: start.saturating_add(from),
            end: start.saturating_add(to),
        };
        let calendar = FakeCalendar::default();
        calendar_source_contract(&calendar, start, start.saturating_add(hour));
        calendar.set(Ok(vec![
            event("late", hour / 2, hour),
            event("early", Duration::ZERO, hour / 4),
            event("tomorrow", hour * 24, hour * 25),
        ]));
        calendar_source_contract(&calendar, start, start.saturating_add(hour));
        let found = calendar.events(start, start.saturating_add(hour)).unwrap();
        let ids: Vec<&str> = found.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, ["early", "late"], "filtered to the window and sorted");
    }
}
