//! Cooling-off: edits to a protected configuration wait before they take effect, so rules can't be
//! loosened on impulse.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::BlockConfig;
use crate::time::Timestamp;

/// An edit to a configuration that is waiting out its cooling-off delay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingChange {
    /// The configuration being changed.
    pub config: String,
    /// The new version, or `None` if the configuration was deleted.
    pub replacement: Option<BlockConfig>,
    /// When the change takes effect.
    pub apply_after: Timestamp,
}

/// The configurations in effect plus edits still cooling off. Persisted, so a restart (or deleting
/// the settings file) can't skip the wait.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigState {
    /// In effect, sorted by name.
    applied: Vec<BlockConfig>,
    /// At most one per configuration.
    pending: Vec<PendingChange>,
}

/// What a [`ConfigState::sync`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Configurations whose new version took effect (added, edited or deleted).
    pub applied: BTreeSet<String>,
    /// Configurations with a newly queued change (a fresh cooling-off period started).
    pub queued: BTreeSet<String>,
}

impl ConfigState {
    /// The cooling-off delay from the design: 24 hours.
    pub const DEFAULT_COOLING_OFF: Duration = Duration::from_secs(24 * 60 * 60);

    /// The configurations in effect, sorted by name.
    pub fn applied(&self) -> &[BlockConfig] {
        &self.applied
    }

    /// The configuration in effect with this name.
    pub fn get(&self, name: &str) -> Option<&BlockConfig> {
        self.applied.iter().find(|config| config.name == name)
    }

    /// Edits still cooling off.
    pub fn pending(&self) -> &[PendingChange] {
        &self.pending
    }

    /// Applies every pending change whose time has come and returns their names.
    pub fn apply_due(&mut self, now: Timestamp) -> BTreeSet<String> {
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|change| change.apply_after <= now);
        self.pending = waiting;
        due.into_iter()
            .map(|change| {
                self.set(&change.config, change.replacement);
                change.config
            })
            .collect()
    }

    /// Brings the applied configurations towards `desired` (what the settings file says now).
    ///
    /// Due changes apply first. Then, per configuration:
    /// - unchanged, or edited back to what's in effect: nothing to do, and any pending change is dropped;
    /// - new, or edited while neither protected nor already cooling off: applies now;
    /// - edited or deleted while `protected` (e.g. the running session's config) or while an earlier
    ///   change is still cooling off: queued until `now + cooling_off`. Re-syncing the same edit keeps
    ///   its original time; a different edit starts the wait again.
    ///
    /// A queued change keeps waiting after protection ends, so ending a session can't rush it through.
    pub fn sync(
        &mut self,
        desired: Vec<BlockConfig>,
        protected: &[&str],
        now: Timestamp,
        cooling_off: Duration,
    ) -> SyncReport {
        let mut report = SyncReport {
            applied: self.apply_due(now),
            queued: BTreeSet::new(),
        };
        let mut desired: BTreeMap<String, BlockConfig> = desired
            .into_iter()
            .map(|config| (config.name.clone(), config))
            .collect();
        let names: BTreeSet<String> = self
            .applied
            .iter()
            .map(|config| config.name.clone())
            .chain(desired.keys().cloned())
            .collect();

        for name in names {
            let wanted = desired.remove(&name);
            let current = self.get(&name);
            if current == wanted.as_ref() {
                self.drop_pending(&name);
                continue;
            }
            let cooling = self.pending.iter().any(|change| change.config == name);
            let guarded = current.is_some() && (cooling || protected.contains(&name.as_str()));
            if !guarded {
                self.set(&name, wanted);
                report.applied.insert(name);
                continue;
            }
            let same_edit_waiting = self
                .pending
                .iter()
                .any(|change| change.config == name && change.replacement == wanted);
            if !same_edit_waiting {
                self.drop_pending(&name);
                self.pending.push(PendingChange {
                    config: name.clone(),
                    replacement: wanted,
                    apply_after: now.saturating_add(cooling_off),
                });
                report.queued.insert(name);
            }
        }
        report
    }

    /// Replaces, adds or (with `None`) removes the applied configuration `name`.
    fn set(&mut self, name: &str, replacement: Option<BlockConfig>) {
        self.applied.retain(|config| config.name != name);
        if let Some(config) = replacement {
            self.applied.push(config);
            self.applied.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }

    fn drop_pending(&mut self, name: &str) {
        self.pending.retain(|change| change.config != name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mode;
    use crate::ports::Clock;
    use crate::testing::FakeClock;

    const DELAY: Duration = ConfigState::DEFAULT_COOLING_OFF;

    fn config(name: &str, apps: &[&str]) -> BlockConfig {
        let mut config = BlockConfig::new(name, Mode::Blocklist);
        config.apps = apps.iter().map(|&app| app.to_owned()).collect();
        config
    }

    fn names(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|&name| name.to_owned()).collect()
    }

    /// A state with `coding` (closes discord) and `writing` already applied.
    fn applied_state(clock: &FakeClock) -> ConfigState {
        let mut state = ConfigState::default();
        state.sync(
            vec![config("coding", &["discord"]), config("writing", &[])],
            &[],
            clock.now(),
            DELAY,
        );
        state
    }

    #[test]
    fn first_sync_applies_everything_sorted() {
        let clock = FakeClock::default();
        let mut state = ConfigState::default();
        let report = state.sync(
            vec![config("writing", &[]), config("coding", &[])],
            &["coding"],
            clock.now(),
            DELAY,
        );
        assert_eq!(report.applied, names(&["coding", "writing"]));
        let applied: Vec<_> = state.applied().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(applied, ["coding", "writing"]);
        assert!(state.pending().is_empty());
    }

    #[test]
    fn unprotected_edits_apply_now() {
        let clock = FakeClock::default();
        let mut state = applied_state(&clock);
        let report = state.sync(
            vec![
                config("coding", &[]),
                config("writing", &[]),
                config("new", &[]),
            ],
            &["writing"],
            clock.now(),
            DELAY,
        );
        assert_eq!(report.applied, names(&["coding", "new"]));
        assert_eq!(state.get("coding"), Some(&config("coding", &[])));
        assert!(state.pending().is_empty());
    }

    #[test]
    fn protected_edits_wait_out_the_delay() {
        let clock = FakeClock::default();
        let mut state = applied_state(&clock);
        let loosened = vec![config("coding", &[]), config("writing", &[])];

        let report = state.sync(loosened.clone(), &["coding"], clock.now(), DELAY);
        assert_eq!(report.queued, names(&["coding"]));
        assert_eq!(state.get("coding"), Some(&config("coding", &["discord"])));
        let apply_after = clock.now().saturating_add(DELAY);
        assert_eq!(state.pending()[0].apply_after, apply_after);

        clock.advance(DELAY / 2);
        let report = state.sync(loosened.clone(), &["coding"], clock.now(), DELAY);
        assert_eq!(
            report,
            SyncReport::default(),
            "re-syncing keeps the original time"
        );
        assert_eq!(state.pending()[0].apply_after, apply_after);

        clock.advance(DELAY / 2);
        let report = state.sync(loosened, &["coding"], clock.now(), DELAY);
        assert_eq!(report.applied, names(&["coding"]));
        assert_eq!(state.get("coding"), Some(&config("coding", &[])));
        assert!(state.pending().is_empty());
    }

    #[test]
    fn a_different_edit_restarts_the_wait() {
        let clock = FakeClock::default();
        let mut state = applied_state(&clock);
        state.sync(vec![config("coding", &[])], &["coding"], clock.now(), DELAY);
        clock.advance(DELAY - Duration::from_secs(1));

        let edited_again = vec![config("coding", &["steam"]), config("writing", &[])];
        let report = state.sync(edited_again, &["coding"], clock.now(), DELAY);
        assert_eq!(report.queued, names(&["coding"]));
        assert_eq!(state.pending().len(), 1, "one pending change per config");
        assert_eq!(
            state.pending()[0].apply_after,
            clock.now().saturating_add(DELAY)
        );
    }

    #[test]
    fn editing_back_cancels_the_pending_change() {
        let clock = FakeClock::default();
        let mut state = applied_state(&clock);
        state.sync(
            vec![config("coding", &[]), config("writing", &[])],
            &["coding"],
            clock.now(),
            DELAY,
        );
        assert_eq!(state.pending().len(), 1);

        let original = vec![config("coding", &["discord"]), config("writing", &[])];
        let report = state.sync(original, &["coding"], clock.now(), DELAY);
        assert_eq!(report, SyncReport::default());
        assert!(state.pending().is_empty());
    }

    #[test]
    fn ending_the_session_does_not_rush_a_queued_change() {
        let clock = FakeClock::default();
        let mut state = applied_state(&clock);
        state.sync(
            vec![config("coding", &[]), config("writing", &[])],
            &["coding"],
            clock.now(),
            DELAY,
        );

        clock.advance(Duration::from_secs(60));
        let report = state.sync(
            vec![config("coding", &["steam"]), config("writing", &[])],
            &[],
            clock.now(),
            DELAY,
        );
        assert!(
            report.applied.is_empty(),
            "still cooling off without a session"
        );
        assert_eq!(report.queued, names(&["coding"]));
        assert_eq!(state.get("coding"), Some(&config("coding", &["discord"])));
    }

    #[test]
    fn deleting_a_protected_config_is_queued() {
        let clock = FakeClock::default();
        let mut state = applied_state(&clock);
        state.sync(
            vec![config("writing", &[])],
            &["coding"],
            clock.now(),
            DELAY,
        );
        assert!(state.get("coding").is_some());
        assert_eq!(state.pending()[0].replacement, None);

        clock.advance(DELAY);
        assert_eq!(state.apply_due(clock.now()), names(&["coding"]));
        assert!(state.get("coding").is_none());
    }

    #[test]
    fn survives_a_json_round_trip() {
        let clock = FakeClock::default();
        let mut state = applied_state(&clock);
        state.sync(vec![config("coding", &[])], &["coding"], clock.now(), DELAY);
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<ConfigState>(&json).unwrap(), state);
    }
}
