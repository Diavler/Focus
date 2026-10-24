//! Whether Focus starts at login, with cooling-off for turning it off: starting at login is what
//! brings Focus back after a reboot, so switching it off loosens protection.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::BlockConfig;
use crate::time::Timestamp;

/// Everything the settings file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Block configurations, with shared lists already included.
    pub configs: Vec<BlockConfig>,
    /// Whether Focus should start when the user logs in.
    pub start_at_login: bool,
}

impl Default for Settings {
    /// No configurations, starting at login (the protective choice).
    fn default() -> Self {
        Self {
            configs: Vec::new(),
            start_at_login: true,
        }
    }
}

/// The start-at-login setting in effect, plus a pending switch-off. Persisted, so a restart
/// can't skip the wait.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginSetting {
    enabled: bool,
    /// When a requested switch-off takes effect.
    off_after: Option<Timestamp>,
}

impl LoginSetting {
    /// Whether Focus starts at login now.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// When a requested switch-off takes effect, if one is waiting.
    pub fn off_after(&self) -> Option<Timestamp> {
        self.off_after
    }

    /// Moves towards `desired`: switching on applies now, switching off waits `cooling_off`
    /// (re-asking keeps the original time; asking for "on" again cancels it). Also applies a
    /// switch-off whose time has come. Returns whether `enabled` changed.
    pub fn sync(&mut self, desired: bool, now: Timestamp, cooling_off: Duration) -> bool {
        if desired {
            self.off_after = None;
            let changed = !self.enabled;
            self.enabled = true;
            return changed;
        }
        if !self.enabled {
            self.off_after = None;
            return false;
        }
        let off_after = *self
            .off_after
            .get_or_insert(now.saturating_add(cooling_off));
        if now >= off_after {
            self.enabled = false;
            self.off_after = None;
            return true;
        }
        false
    }

    /// Applies a switch-off whose time has come, without re-reading the settings. Returns whether
    /// `enabled` changed.
    pub fn apply_due(&mut self, now: Timestamp) -> bool {
        match self.off_after {
            Some(off_after) if now >= off_after => {
                self.enabled = false;
                self.off_after = None;
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
    fn switching_on_applies_now() {
        let clock = FakeClock::default();
        let mut login = LoginSetting::default();
        assert!(!login.enabled());
        assert!(login.sync(true, clock.now(), DAY));
        assert!(login.enabled());
        assert!(
            !login.sync(true, clock.now(), DAY),
            "no change the second time"
        );
    }

    #[test]
    fn switching_off_waits_out_the_cooling_off() {
        let clock = FakeClock::default();
        let mut login = LoginSetting::default();
        login.sync(true, clock.now(), DAY);

        assert!(!login.sync(false, clock.now(), DAY));
        assert!(login.enabled());
        let off_after = login.off_after().unwrap();
        assert_eq!(off_after, clock.now().saturating_add(DAY));

        clock.advance(DAY / 2);
        assert!(!login.sync(false, clock.now(), DAY));
        assert_eq!(
            login.off_after(),
            Some(off_after),
            "re-asking keeps the time"
        );

        clock.advance(DAY / 2);
        assert!(login.apply_due(clock.now()));
        assert!(!login.enabled());
        assert_eq!(login.off_after(), None);
    }

    #[test]
    fn switching_back_on_cancels_a_pending_switch_off() {
        let clock = FakeClock::default();
        let mut login = LoginSetting::default();
        login.sync(true, clock.now(), DAY);
        login.sync(false, clock.now(), DAY);
        assert!(!login.sync(true, clock.now(), DAY));
        assert_eq!(login.off_after(), None);
        clock.advance(DAY * 2);
        assert!(!login.apply_due(clock.now()));
        assert!(login.enabled());
    }

    #[test]
    fn settings_default_to_starting_at_login() {
        assert!(Settings::default().start_at_login);
    }
}
