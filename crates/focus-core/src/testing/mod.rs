//! Fakes for every port and the contract suites every implementation must pass.
//!
//! Enabled by the `testing` feature. Adapters add
//! `focus-core = { workspace = true, features = ["testing"] }` to their dev-dependencies and run
//! the matching `*_contract` function from their own tests.

mod contracts;
mod fakes;

pub use contracts::{
    autostart_contract, clock_contract, config_source_contract, enforcer_contract,
    foreground_app_contract, history_contract, notifier_contract, process_list_contract,
    state_store_contract, ui_watch_contract,
};
pub use fakes::{
    FakeAutostart, FakeClock, FakeConfigSource, FakeEnforcer, FakeForegroundApp, FakeNotifier,
    FakeUi, MemoryStateStore, RecordingHistory,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{ConfigSourceError, NoopHistory};
    use crate::{BlockConfig, Mode};

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
    }
}
