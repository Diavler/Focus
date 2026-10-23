//! Stand-ins the demo needs until the daemon exists (ADR 0002): a system clock and the example
//! configurations from `docs/DESIGN.md`. Both move behind IPC in build step 3.

use std::time::{SystemTime, UNIX_EPOCH};

use focus_core::ports::Clock;
use focus_core::{
    BlockConfig, Mode, PatternError, Rule, RuleAction, SharedList, Timestamp, UrlPattern, Verdict,
};

/// The real wall clock.
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

/// The `coding` and `writing` configurations from the design doc, with shared lists included.
pub fn demo_configs() -> Result<Vec<BlockConfig>, PatternError> {
    let social = shared_list("social", &["x.com", "instagram.com", "tiktok.com"])?;
    let video = shared_list("video", &["twitch.tv", "netflix.com"])?;

    let mut coding = BlockConfig::new("coding", Mode::Blocklist);
    coding.include(&social);
    coding.include(&video);
    coding.rules.extend(rules(
        &["reddit.com", "youtube.com"],
        Verdict::Restrict(RuleAction::HideFeed),
    )?);
    coding.rules.extend(rules(
        &["reddit.com/r/rust/comments/*", "docs.rs", "github.com"],
        Verdict::Allow,
    )?);
    coding.apps = vec!["discord".into(), "steam".into()];

    let mut writing = BlockConfig::new("writing", Mode::Allowlist);
    writing.rules = rules(&["docs.google.com", "dictionary.com"], Verdict::Allow)?;

    Ok(vec![coding, writing])
}

/// A shared list that blocks every pattern in `patterns`.
fn shared_list(name: &str, patterns: &[&str]) -> Result<SharedList, PatternError> {
    Ok(SharedList {
        name: name.into(),
        rules: rules(patterns, Verdict::Restrict(RuleAction::Block))?,
    })
}

/// One rule per pattern, all with the same verdict.
fn rules(patterns: &[&str], verdict: Verdict) -> Result<Vec<Rule>, PatternError> {
    patterns
        .iter()
        .map(|pattern| {
            Ok(Rule {
                pattern: UrlPattern::parse(pattern)?,
                verdict,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use focus_core::testing::clock_contract;

    use super::*;

    #[test]
    fn system_clock_passes_the_clock_contract() {
        clock_contract(&SystemClock);
    }

    #[test]
    fn demo_configs_are_valid_and_behave_as_documented() {
        let configs = demo_configs().unwrap();
        let coding = &configs[0];
        assert_eq!(
            coding.verdict("https://www.reddit.com/"),
            Verdict::Restrict(RuleAction::HideFeed)
        );
        assert_eq!(
            coding.verdict("https://www.reddit.com/r/rust/comments/abc/title"),
            Verdict::Allow
        );
        assert_eq!(
            coding.verdict("https://x.com"),
            Verdict::Restrict(RuleAction::Block)
        );
        let writing = &configs[1];
        assert_eq!(
            writing.verdict("https://docs.google.com/d/1"),
            Verdict::Allow
        );
        assert_eq!(
            writing.verdict("https://news.ycombinator.com"),
            Verdict::Restrict(RuleAction::Block)
        );
    }
}
