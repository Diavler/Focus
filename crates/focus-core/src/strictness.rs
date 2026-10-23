//! Strictness levels and the bypass rules each one implies.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How hard a session is to end early or bypass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strictness {
    /// End with one click; overrides allowed.
    Light,
    /// End after a wait plus a typed reason; overrides spend the daily unlock budget.
    Deep,
    /// No ending early and no overrides.
    Locked,
}

/// How a running session may be ended before its timer runs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndEarlyPolicy {
    /// Ends as soon as it's asked.
    Immediate,
    /// Ends only after waiting `wait` since the request, and with a typed reason.
    WaitAndReason {
        /// How long after the request the session may be ended.
        wait: Duration,
    },
    /// Can't be ended early.
    Forbidden,
}

/// Whether a restricted page can be opened "just this once" during a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverridePolicy {
    /// Overrides cost nothing.
    Free,
    /// Each override spends one unlock from the daily budget.
    SpendUnlock,
    /// No overrides.
    Forbidden,
}

impl Strictness {
    /// Every level, from least to most strict.
    pub const ALL: [Self; 3] = [Self::Light, Self::Deep, Self::Locked];

    /// How long a Deep session waits between asking to end and being allowed to.
    pub const DEEP_END_WAIT: Duration = Duration::from_secs(30);

    /// The end-early rule for this level.
    pub const fn end_early_policy(self) -> EndEarlyPolicy {
        match self {
            Self::Light => EndEarlyPolicy::Immediate,
            Self::Deep => EndEarlyPolicy::WaitAndReason {
                wait: Self::DEEP_END_WAIT,
            },
            Self::Locked => EndEarlyPolicy::Forbidden,
        }
    }

    /// The override rule for this level.
    pub const fn override_policy(self) -> OverridePolicy {
        match self {
            Self::Light => OverridePolicy::Free,
            Self::Deep => OverridePolicy::SpendUnlock,
            Self::Locked => OverridePolicy::Forbidden,
        }
    }
}

impl fmt::Display for Strictness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Light => "Light",
            Self::Deep => "Deep",
            Self::Locked => "Locked",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policies_match_the_design_table() {
        use EndEarlyPolicy as End;
        use OverridePolicy as Over;
        let table = [
            (Strictness::Light, End::Immediate, Over::Free),
            (
                Strictness::Deep,
                End::WaitAndReason {
                    wait: Duration::from_secs(30),
                },
                Over::SpendUnlock,
            ),
            (Strictness::Locked, End::Forbidden, Over::Forbidden),
        ];
        for (strictness, end, over) in table {
            assert_eq!(strictness.end_early_policy(), end, "{strictness}");
            assert_eq!(strictness.override_policy(), over, "{strictness}");
        }
    }
}
