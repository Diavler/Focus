//! Block configurations: named, reusable rule sets that a session runs with.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::matcher::evaluate;
use crate::rule::{Rule, RuleAction, Verdict};

/// What happens to pages that no rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Unmatched pages are allowed; rules list what to restrict.
    Blocklist,
    /// Unmatched pages are blocked; rules list what to allow.
    Allowlist,
}

/// A named list of rules that several configurations can include, such as *Social media*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedList {
    /// The list's name, used by `include`.
    pub name: String,
    /// The list's rules.
    pub rules: Vec<Rule>,
}

/// A named rule set: mode, site rules (with shared lists already included) and apps to close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockConfig {
    /// The configuration's name, e.g. `coding`.
    pub name: String,
    /// What happens to pages no rule matches.
    pub mode: Mode,
    /// Site and URL-pattern rules.
    pub rules: Vec<Rule>,
    /// Apps to close during a session, by executable name.
    pub apps: Vec<String>,
}

impl BlockConfig {
    /// An empty configuration with no rules or apps.
    pub fn new(name: impl Into<String>, mode: Mode) -> Self {
        Self {
            name: name.into(),
            mode,
            rules: Vec::new(),
            apps: Vec::new(),
        }
    }

    /// Adds a shared list's rules to this configuration.
    pub fn include(&mut self, list: &SharedList) {
        self.rules.extend(list.rules.iter().cloned());
    }

    /// The verdict for `url` under this configuration.
    pub fn verdict(&self, url: &str) -> Verdict {
        evaluate(self.mode, &self.rules, url)
    }
}

impl Mode {
    /// The verdict for a page that no rule matches.
    pub fn default_verdict(self) -> Verdict {
        match self {
            Self::Blocklist => Verdict::Allow,
            Self::Allowlist => Verdict::Restrict(RuleAction::Block),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Blocklist => "Blocklist",
            Self::Allowlist => "Allowlist",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UrlPattern;

    #[test]
    fn include_appends_the_lists_rules() {
        let social = SharedList {
            name: "social".into(),
            rules: vec![Rule::restrict(
                UrlPattern::parse("x.com").unwrap(),
                RuleAction::Block,
            )],
        };
        let mut config = BlockConfig::new("coding", Mode::Blocklist);
        config.include(&social);
        assert_eq!(config.rules, social.rules);
        assert_eq!(
            config.verdict("https://x.com/home"),
            Verdict::Restrict(RuleAction::Block)
        );
    }
}
