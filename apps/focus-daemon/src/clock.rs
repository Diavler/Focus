//! The real wall clock, for the `Clock` port.

use std::time::{SystemTime, UNIX_EPOCH};

use focus_core::Timestamp;
use focus_core::ports::Clock;

/// Reads the system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        // A system clock set before 1970 reads as the epoch rather than failing.
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        Timestamp::from_millis(millis)
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::clock_contract;

    use super::*;

    #[test]
    fn passes_the_clock_contract() {
        clock_contract(&SystemClock);
    }
}
