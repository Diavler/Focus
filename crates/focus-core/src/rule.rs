//! Rules: a URL pattern and what happens when a visited page matches it.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::pattern::UrlPattern;

/// What to do with a page that a restricting rule matches.
///
/// Variants are declared from least to most strict; that order breaks ties between equally
/// specific rules, so the stricter rule wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    /// Ask whether the page is part of the session's task before showing it.
    Ask,
    /// Show a countdown before the page loads.
    Delay,
    /// Let the site work but hide feeds and recommendations.
    HideFeed,
    /// Don't show the page.
    Block,
}

/// The outcome for a URL: allowed, or restricted by an action. Ordered from least to most strict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The page loads normally.
    Allow,
    /// The page is restricted by this action.
    Restrict(RuleAction),
}

/// A pattern plus the verdict for pages that match it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rule {
    /// Which pages the rule covers.
    pub pattern: UrlPattern,
    /// What happens to those pages.
    pub verdict: Verdict,
}

impl Rule {
    /// A rule that allows pages matching `pattern`.
    pub fn allow(pattern: UrlPattern) -> Self {
        Self {
            pattern,
            verdict: Verdict::Allow,
        }
    }

    /// A rule that applies `action` to pages matching `pattern`.
    pub fn restrict(pattern: UrlPattern, action: RuleAction) -> Self {
        Self {
            pattern,
            verdict: Verdict::Restrict(action),
        }
    }
}

impl fmt::Display for RuleAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ask => "Ask",
            Self::Delay => "Delay",
            Self::HideFeed => "Hide feed",
            Self::Block => "Block",
        })
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow => f.write_str("Allow"),
            Self::Restrict(action) => action.fmt(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_are_ordered_by_strictness() {
        let ordered = [
            Verdict::Allow,
            Verdict::Restrict(RuleAction::Ask),
            Verdict::Restrict(RuleAction::Delay),
            Verdict::Restrict(RuleAction::HideFeed),
            Verdict::Restrict(RuleAction::Block),
        ];
        assert!(ordered.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
