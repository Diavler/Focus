//! Friction limits: how many unlocks Deep sessions allow and how long one lasts, and when the
//! browser nudges you out of a rabbit hole (tab cap, drift prompt). Tightening a limit applies at
//! once; loosening one waits out the cooling-off delay, like every other way to weaken Focus.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

/// The friction limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Friction {
    /// Unlocks Deep sessions may spend in any 24 hours.
    pub unlocks_per_day: u8,
    /// How long one unlock keeps a blocked page open, in minutes.
    pub unlock_minutes: u16,
    /// Most tabs open in the browser during a session; 0 turns the cap off.
    pub tab_cap: u8,
    /// Hops away from the session's starting sites before the drift prompt; 0 turns it off.
    pub drift_hops: u8,
}

impl Default for Friction {
    fn default() -> Self {
        Self {
            unlocks_per_day: 3,
            unlock_minutes: 10,
            tab_cap: 10,
            drift_hops: 4,
        }
    }
}

impl Friction {
    /// The most unlocks per day the settings accept.
    pub const MAX_UNLOCKS_PER_DAY: u8 = 20;
    /// The shortest and longest unlock, in minutes.
    pub const UNLOCK_MINUTES: (u16, u16) = (1, 60);
    /// The smallest and largest tab cap (besides 0, off).
    pub const TAB_CAP: (u8, u8) = (2, 50);
    /// The fewest and most hops before the drift prompt (besides 0, off).
    pub const DRIFT_HOPS: (u8, u8) = (2, 20);

    /// How long one unlock lasts.
    pub fn unlock_length(&self) -> Duration {
        Duration::from_secs(u64::from(self.unlock_minutes) * 60)
    }

    /// Each limit that's out of range, as its settings key and what it accepts.
    pub fn problems(&self) -> Vec<(&'static str, String)> {
        let mut problems = Vec::new();
        if self.unlocks_per_day > Self::MAX_UNLOCKS_PER_DAY {
            problems.push((
                "unlocks_per_day",
                format!("0 to {}", Self::MAX_UNLOCKS_PER_DAY),
            ));
        }
        let (shortest, longest) = Self::UNLOCK_MINUTES;
        if !(shortest..=longest).contains(&self.unlock_minutes) {
            problems.push(("unlock_minutes", format!("{shortest} to {longest}")));
        }
        for (key, value, (low, high)) in [
            ("tab_cap", self.tab_cap, Self::TAB_CAP),
            ("drift_hops", self.drift_hops, Self::DRIFT_HOPS),
        ] {
            if value != 0 && !(low..=high).contains(&value) {
                problems.push((key, format!("0 (off) or {low} to {high}")));
            }
        }
        problems
    }

    /// The stricter value of each limit in `self` and `other`.
    fn tightest(self, other: Self) -> Self {
        Self {
            unlocks_per_day: self.unlocks_per_day.min(other.unlocks_per_day),
            unlock_minutes: self.unlock_minutes.min(other.unlock_minutes),
            tab_cap: tightest_cap(self.tab_cap, other.tab_cap),
            drift_hops: tightest_cap(self.drift_hops, other.drift_hops),
        }
    }
}

/// The stricter of two limits where 0 means off (the loosest).
fn tightest_cap(a: u8, b: u8) -> u8 {
    match (a, b) {
        (0, other) | (other, 0) => other,
        (a, b) => a.min(b),
    }
}

/// Loosened limits waiting out the cooling-off delay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingFriction {
    /// The limits as the settings ask for them.
    pub friction: Friction,
    /// When they take effect.
    pub apply_after: Timestamp,
}

/// The friction limits in effect, plus a loosening that's cooling off. Persisted, so a restart
/// can't skip the wait.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrictionSetting {
    in_effect: Friction,
    pending: Option<PendingFriction>,
}

impl FrictionSetting {
    /// The limits in effect now.
    pub fn in_effect(&self) -> Friction {
        self.in_effect
    }

    /// A loosening that's cooling off, if there is one.
    pub fn pending(&self) -> Option<PendingFriction> {
        self.pending
    }

    /// Moves towards `desired`: every limit it tightens applies now; if it loosens any, all of
    /// `desired` applies after `cooling_off` (asking for the same again keeps the time). Also
    /// applies a loosening whose time has come. Returns whether the limits in effect changed.
    pub fn sync(&mut self, desired: Friction, now: Timestamp, cooling_off: Duration) -> bool {
        let tightened = self.in_effect.tightest(desired);
        let changed = tightened != self.in_effect;
        self.in_effect = tightened;
        if desired == tightened {
            self.pending = None;
            return changed;
        }
        if self
            .pending
            .is_none_or(|pending| pending.friction != desired)
        {
            self.pending = Some(PendingFriction {
                friction: desired,
                apply_after: now.saturating_add(cooling_off),
            });
        }
        self.apply_due(now) || changed
    }

    /// Applies a loosening whose time has come, without re-reading the settings. Returns whether
    /// the limits in effect changed.
    pub fn apply_due(&mut self, now: Timestamp) -> bool {
        match self.pending {
            Some(pending) if now >= pending.apply_after => {
                self.in_effect = pending.friction;
                self.pending = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::Clock;
    use crate::testing::FakeClock;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    #[test]
    fn tightening_applies_now() {
        let clock = FakeClock::default();
        let mut setting = FrictionSetting::default();
        let tighter = Friction {
            unlocks_per_day: 1,
            tab_cap: 5,
            ..Friction::default()
        };
        assert!(setting.sync(tighter, clock.now(), DAY));
        assert_eq!(setting.in_effect(), tighter);
        assert_eq!(setting.pending(), None);
    }

    #[test]
    fn loosening_waits_out_the_cooling_off() {
        let clock = FakeClock::default();
        let mut setting = FrictionSetting::default();
        let looser = Friction {
            unlocks_per_day: 6,
            drift_hops: 0,
            ..Friction::default()
        };
        assert!(!setting.sync(looser, clock.now(), DAY));
        assert_eq!(setting.in_effect(), Friction::default());
        let apply_after = clock.now().saturating_add(DAY);
        assert_eq!(setting.pending().map(|p| p.apply_after), Some(apply_after));

        clock.advance(DAY / 2);
        assert!(!setting.sync(looser, clock.now(), DAY));
        assert_eq!(
            setting.pending().map(|p| p.apply_after),
            Some(apply_after),
            "asking again keeps the time"
        );
        clock.advance(DAY / 2);
        assert!(setting.apply_due(clock.now()));
        assert_eq!(setting.in_effect(), looser);
        assert_eq!(setting.pending(), None);
    }

    #[test]
    fn a_mixed_change_tightens_now_and_loosens_later() {
        let clock = FakeClock::default();
        let mut setting = FrictionSetting::default();
        let mixed = Friction {
            unlocks_per_day: 1,
            tab_cap: 0,
            ..Friction::default()
        };
        assert!(setting.sync(mixed, clock.now(), DAY));
        assert_eq!(setting.in_effect().unlocks_per_day, 1, "fewer unlocks now");
        assert_eq!(setting.in_effect().tab_cap, 10, "the cap stays on for now");
        clock.advance(DAY);
        assert!(setting.apply_due(clock.now()));
        assert_eq!(setting.in_effect(), mixed);
    }

    #[test]
    fn going_back_cancels_a_pending_loosening() {
        let clock = FakeClock::default();
        let mut setting = FrictionSetting::default();
        let looser = Friction {
            unlock_minutes: 30,
            ..Friction::default()
        };
        setting.sync(looser, clock.now(), DAY);
        assert!(setting.pending().is_some());
        setting.sync(Friction::default(), clock.now(), DAY);
        assert_eq!(setting.pending(), None);
    }

    #[test]
    fn off_is_the_loosest_cap() {
        assert_eq!(tightest_cap(0, 8), 8);
        assert_eq!(tightest_cap(8, 0), 8);
        assert_eq!(tightest_cap(5, 8), 5);
        assert_eq!(tightest_cap(0, 0), 0);
    }

    #[test]
    fn out_of_range_limits_are_reported() {
        assert!(Friction::default().problems().is_empty());
        let off = Friction {
            unlocks_per_day: 0,
            tab_cap: 0,
            drift_hops: 0,
            ..Friction::default()
        };
        assert!(off.problems().is_empty(), "0 turns them off");
        let bad = Friction {
            unlocks_per_day: 21,
            unlock_minutes: 0,
            tab_cap: 1,
            drift_hops: 99,
        };
        let keys: Vec<&str> = bad.problems().into_iter().map(|(key, _)| key).collect();
        assert_eq!(
            keys,
            ["unlocks_per_day", "unlock_minutes", "tab_cap", "drift_hops"]
        );
    }
}
