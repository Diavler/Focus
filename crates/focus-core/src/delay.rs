//! The growing Delay: within a session, each visit to a site behind a Delay rule waits longer
//! than the last. Counted by the daemon and persisted, so restarting the browser doesn't reset it.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;
use crate::url::ParsedUrl;

/// The first wait.
pub const DELAY_BASE: Duration = Duration::from_secs(10);

/// The longest wait.
pub const DELAY_MAX: Duration = Duration::from_secs(120);

/// The wait for a site visited `visits` times before in this session.
pub fn delay_for(visits: u32) -> Duration {
    DELAY_BASE
        .saturating_mul(visits.saturating_add(1))
        .min(DELAY_MAX)
}

/// Visits per site during one session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelayLadder {
    /// When the session the counts belong to started.
    session: Option<Timestamp>,
    visits: BTreeMap<String, u32>,
}

impl DelayLadder {
    /// Counts a visit to `url`'s site during the session that started at `session_started_at`
    /// and returns how long it waits. A new session starts the counts again.
    pub fn visit(&mut self, session_started_at: Timestamp, url: &str) -> Duration {
        if self.session != Some(session_started_at) {
            self.session = Some(session_started_at);
            self.visits.clear();
        }
        let visits = self.visits.entry(site_of(url)).or_default();
        let wait = delay_for(*visits);
        *visits = visits.saturating_add(1);
        wait
    }
}

/// The site a visit counts for: the host without a leading `www.`.
fn site_of(url: &str) -> String {
    match ParsedUrl::parse(url) {
        Some(parsed) => parsed
            .host
            .strip_prefix("www.")
            .map_or_else(|| parsed.host.clone(), str::to_owned),
        None => url.trim().to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn waits_grow_per_site_up_to_the_cap() {
        let session = Timestamp::from_millis(1_000);
        let mut ladder = DelayLadder::default();
        let waits: Vec<u64> = (0..3)
            .map(|_| {
                ladder
                    .visit(session, "https://news.ycombinator.com/item?id=1")
                    .as_secs()
            })
            .collect();
        assert_eq!(waits, [10, 20, 30]);
        assert_eq!(
            ladder.visit(session, "https://www.reddit.com/"),
            10 * SECOND,
            "another site starts at the base"
        );
        assert_eq!(
            ladder.visit(session, "https://reddit.com/r/all"),
            20 * SECOND,
            "www. is the same site"
        );
        assert_eq!(delay_for(1_000), DELAY_MAX);
        assert_eq!(delay_for(u32::MAX), DELAY_MAX);
    }

    #[test]
    fn a_new_session_starts_again() {
        let mut ladder = DelayLadder::default();
        ladder.visit(Timestamp::from_millis(1), "https://x.com/");
        ladder.visit(Timestamp::from_millis(1), "https://x.com/");
        assert_eq!(
            ladder.visit(Timestamp::from_millis(2), "https://x.com/"),
            DELAY_BASE
        );
    }
}
