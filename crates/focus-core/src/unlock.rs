//! Unlocks: opening a blocked page "just this once" during a session. Light sessions unlock for
//! free, Deep ones spend from a budget per rolling 24 hours (with a typed reason), and Locked ones
//! can't unlock at all. A rolling window needs no time zone and can't be doubled at midnight.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::friction::Friction;
use crate::strictness::OverridePolicy;
use crate::time::Timestamp;

/// The window the unlock budget counts over.
pub const UNLOCK_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

/// Why an unlock was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnlockError {
    /// No session is running, so nothing is blocked.
    #[error("no session is running")]
    NoSession,
    /// The page isn't blocked (Ask and Delay pages have their own Continue).
    #[error("this page isn't blocked")]
    NotBlocked,
    /// A Locked session is running.
    #[error("a locked session can't be unlocked")]
    Forbidden,
    /// Deep sessions need a typed reason.
    #[error("type a reason to unlock")]
    ReasonRequired,
    /// Every unlock of the last 24 hours is spent.
    #[error("no unlocks left")]
    BudgetSpent {
        /// When the next unlock comes back, or `None` if the budget is 0.
        next_at: Option<Timestamp>,
    },
}

/// The unlocks spent in the last 24 hours. Persisted, so restarting doesn't refill the budget.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnlockLedger {
    spent: Vec<Timestamp>,
}

impl UnlockLedger {
    /// How many unlocks are left at `now` out of `friction`'s budget.
    pub fn left(&self, now: Timestamp, friction: Friction) -> u8 {
        let spent = self.spent.iter().filter(|&&at| counts(at, now)).count();
        let budget = usize::from(friction.unlocks_per_day);
        // At most `budget`, which is a `u8`, so this can't fail.
        u8::try_from(budget.saturating_sub(spent)).unwrap_or(u8::MAX)
    }

    /// Unlocks under `policy` at `now`, spending from the budget when the policy says so, and
    /// returns when the unlock ends.
    pub fn unlock(
        &mut self,
        policy: OverridePolicy,
        reason: &str,
        now: Timestamp,
        friction: Friction,
    ) -> Result<Timestamp, UnlockError> {
        let until = now.saturating_add(friction.unlock_length());
        match policy {
            OverridePolicy::Forbidden => Err(UnlockError::Forbidden),
            OverridePolicy::Free => Ok(until),
            OverridePolicy::SpendUnlock => {
                if reason.trim().is_empty() {
                    return Err(UnlockError::ReasonRequired);
                }
                self.spent.retain(|&at| counts(at, now));
                if self.left(now, friction) == 0 {
                    let next_at = (friction.unlocks_per_day > 0)
                        .then(|| self.spent.iter().min())
                        .flatten()
                        .map(|&oldest| oldest.saturating_add(UNLOCK_WINDOW));
                    return Err(UnlockError::BudgetSpent { next_at });
                }
                self.spent.push(now);
                Ok(until)
            }
        }
    }
}

/// Whether an unlock spent `at` still counts at `now`. One "in the future" (the clock went back)
/// still counts, so turning the clock back doesn't refill the budget.
fn counts(at: Timestamp, now: Timestamp) -> bool {
    at.saturating_add(UNLOCK_WINDOW) > now
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::Clock;
    use crate::testing::FakeClock;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn deep_spends_the_budget_with_a_reason() {
        let clock = FakeClock::default();
        let friction = Friction {
            unlocks_per_day: 2,
            ..Friction::default()
        };
        let mut ledger = UnlockLedger::default();
        let deep = OverridePolicy::SpendUnlock;
        assert_eq!(
            ledger.unlock(deep, "  ", clock.now(), friction),
            Err(UnlockError::ReasonRequired)
        );
        assert_eq!(ledger.left(clock.now(), friction), 2, "nothing spent");
        let first = clock.now();
        assert_eq!(
            ledger.unlock(deep, "docs are on that site", first, friction),
            Ok(first.saturating_add(friction.unlock_length()))
        );
        clock.advance(MINUTE);
        assert!(ledger.unlock(deep, "again", clock.now(), friction).is_ok());
        assert_eq!(ledger.left(clock.now(), friction), 0);
        assert_eq!(
            ledger.unlock(deep, "once more", clock.now(), friction),
            Err(UnlockError::BudgetSpent {
                next_at: Some(first.saturating_add(UNLOCK_WINDOW))
            })
        );
    }

    #[test]
    fn unlocks_come_back_after_24_hours() {
        let clock = FakeClock::default();
        let friction = Friction {
            unlocks_per_day: 1,
            ..Friction::default()
        };
        let mut ledger = UnlockLedger::default();
        let deep = OverridePolicy::SpendUnlock;
        ledger.unlock(deep, "why", clock.now(), friction).unwrap();
        clock.advance(UNLOCK_WINDOW - MINUTE);
        assert_eq!(ledger.left(clock.now(), friction), 0);
        clock.advance(MINUTE);
        assert_eq!(ledger.left(clock.now(), friction), 1);
        assert!(ledger.unlock(deep, "why", clock.now(), friction).is_ok());
    }

    #[test]
    fn light_is_free_and_locked_is_refused() {
        let clock = FakeClock::default();
        let friction = Friction {
            unlocks_per_day: 0,
            ..Friction::default()
        };
        let mut ledger = UnlockLedger::default();
        for _ in 0..5 {
            assert!(
                ledger
                    .unlock(OverridePolicy::Free, "", clock.now(), friction)
                    .is_ok()
            );
        }
        assert_eq!(
            ledger.unlock(OverridePolicy::Forbidden, "please", clock.now(), friction),
            Err(UnlockError::Forbidden)
        );
        assert_eq!(
            ledger.unlock(OverridePolicy::SpendUnlock, "why", clock.now(), friction),
            Err(UnlockError::BudgetSpent { next_at: None }),
            "a budget of 0 never comes back"
        );
    }

    #[test]
    fn turning_the_clock_back_keeps_unlocks_spent() {
        let friction = Friction {
            unlocks_per_day: 1,
            ..Friction::default()
        };
        let mut ledger = UnlockLedger::default();
        let later = Timestamp::from_millis(10_000_000);
        ledger
            .unlock(OverridePolicy::SpendUnlock, "why", later, friction)
            .unwrap();
        assert_eq!(ledger.left(Timestamp::from_millis(1_000), friction), 0);
    }
}
