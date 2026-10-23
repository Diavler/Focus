//! Wall-clock instants as plain numbers, so session logic never reads the system clock.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A point in time, in milliseconds since the Unix epoch. Produced by the `Clock` port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Builds a timestamp from milliseconds since the Unix epoch.
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Milliseconds since the Unix epoch.
    pub const fn as_millis(self) -> u64 {
        self.0
    }

    /// This instant moved forward by `duration`, clamped at the largest representable time.
    pub fn saturating_add(self, duration: Duration) -> Self {
        let millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        Self(self.0.saturating_add(millis))
    }

    /// Time elapsed from `earlier` to `self`, or zero if `earlier` is later.
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_millis(self.0.saturating_sub(earlier.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_difference_round_trip() {
        let start = Timestamp::from_millis(1_000);
        let later = start.saturating_add(Duration::from_secs(30));
        assert_eq!(later.as_millis(), 31_000);
        assert_eq!(
            later.saturating_duration_since(start),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn saturates_instead_of_overflowing() {
        let end = Timestamp::from_millis(u64::MAX - 1).saturating_add(Duration::MAX);
        assert_eq!(end.as_millis(), u64::MAX);
        assert_eq!(
            Timestamp::from_millis(0).saturating_duration_since(end),
            Duration::ZERO
        );
    }
}
