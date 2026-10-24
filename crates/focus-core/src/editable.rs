//! The settings as the user wrote them, for editing in the window. Unlike [`crate::Settings`],
//! configurations keep their own rules and the names of the lists they include, so saving writes
//! back what the user sees.

use serde::{Deserialize, Serialize};

use crate::auto_start::AutoStartRule;
use crate::config::{BlockConfig, Mode, SharedList};
use crate::friction::Friction;
use crate::pattern::UrlPattern;
use crate::rule::{Rule, RuleAction, Verdict};

/// Every editable setting, plus the revision of the text it was read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditableSettings {
    /// Whether Focus starts when the user logs in.
    pub start_at_login: bool,
    /// Shared lists, sorted by name.
    pub lists: Vec<ListEntry>,
    /// Block configurations, sorted by name.
    pub configs: Vec<ConfigEntry>,
    /// Automatic-start rules, in order.
    pub auto_start: Vec<AutoStartRule>,
    /// The friction limits (unlocks, tab cap, drift prompt).
    #[serde(default)]
    pub friction: Friction,
    /// Identifies the text these were read from, so saving can refuse to overwrite edits made to
    /// the file in the meantime.
    pub revision: String,
}

/// URL patterns grouped by what happens to matching pages, as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulePatterns {
    /// Always allowed.
    pub allow: Vec<String>,
    /// Asks whether the page is part of the task.
    pub ask: Vec<String>,
    /// Opens after a countdown.
    pub delay: Vec<String>,
    /// Works, but feeds and recommendations are hidden.
    pub feed_only: Vec<String>,
    /// Blocked.
    pub block: Vec<String>,
}

/// One shared list, as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListEntry {
    /// The list's name, used by `include`.
    pub name: String,
    /// Its patterns.
    pub patterns: RulePatterns,
}

/// One block configuration, as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigEntry {
    /// The configuration's name.
    pub name: String,
    /// What happens to pages no rule matches.
    pub mode: Mode,
    /// Names of the shared lists it includes.
    pub include: Vec<String>,
    /// Its own patterns.
    pub patterns: RulePatterns,
    /// Apps to close during a session, by executable name.
    pub apps: Vec<String>,
}

/// Where a rule's verdict goes, for every action and in the order the settings file lists them.
pub const ACTIONS: [Verdict; 5] = [
    Verdict::Allow,
    Verdict::Restrict(RuleAction::Ask),
    Verdict::Restrict(RuleAction::Delay),
    Verdict::Restrict(RuleAction::HideFeed),
    Verdict::Restrict(RuleAction::Block),
];

impl RulePatterns {
    /// The patterns for one action.
    pub fn get(&self, verdict: Verdict) -> &[String] {
        match verdict {
            Verdict::Allow => &self.allow,
            Verdict::Restrict(RuleAction::Ask) => &self.ask,
            Verdict::Restrict(RuleAction::Delay) => &self.delay,
            Verdict::Restrict(RuleAction::HideFeed) => &self.feed_only,
            Verdict::Restrict(RuleAction::Block) => &self.block,
        }
    }

    /// The patterns for one action, to change.
    pub fn get_mut(&mut self, verdict: Verdict) -> &mut Vec<String> {
        match verdict {
            Verdict::Allow => &mut self.allow,
            Verdict::Restrict(RuleAction::Ask) => &mut self.ask,
            Verdict::Restrict(RuleAction::Delay) => &mut self.delay,
            Verdict::Restrict(RuleAction::HideFeed) => &mut self.feed_only,
            Verdict::Restrict(RuleAction::Block) => &mut self.block,
        }
    }

    /// The rules these patterns make; patterns that don't parse are left out.
    pub fn rules(&self) -> Vec<Rule> {
        ACTIONS
            .iter()
            .flat_map(|&verdict| {
                self.get(verdict).iter().filter_map(move |pattern| {
                    UrlPattern::parse(pattern)
                        .ok()
                        .map(|pattern| Rule { pattern, verdict })
                })
            })
            .collect()
    }
}

impl EditableSettings {
    /// What `config` would block if saved now: its own rules plus its included lists, the way
    /// the settings file is read. Lists that don't exist and patterns that don't parse are left
    /// out. `None` if there's no such configuration.
    pub fn resolve(&self, config: &str) -> Option<BlockConfig> {
        let entry = self.configs.iter().find(|entry| entry.name == config)?;
        let mut resolved = BlockConfig::new(entry.name.clone(), entry.mode);
        for name in &entry.include {
            if let Some(list) = self.lists.iter().find(|list| &list.name == name) {
                resolved.include(&SharedList {
                    name: list.name.clone(),
                    rules: list.patterns.rules(),
                });
            }
        }
        resolved.rules.extend(entry.patterns.rules());
        resolved.apps.clone_from(&entry.apps);
        Some(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> EditableSettings {
        EditableSettings {
            start_at_login: true,
            lists: vec![ListEntry {
                name: "social".into(),
                patterns: RulePatterns {
                    block: vec!["x.com".into()],
                    ..RulePatterns::default()
                },
            }],
            configs: vec![ConfigEntry {
                name: "coding".into(),
                mode: Mode::Blocklist,
                include: vec!["social".into(), "missing".into()],
                patterns: RulePatterns {
                    feed_only: vec!["reddit.com".into()],
                    allow: vec!["reddit.com/r/rust/comments/*".into(), "*.bad".into()],
                    ..RulePatterns::default()
                },
                apps: vec!["discord".into()],
            }],
            auto_start: Vec::new(),
            friction: Friction::default(),
            revision: "1".into(),
        }
    }

    #[test]
    fn resolves_a_config_like_the_settings_file() {
        let coding = settings().resolve("coding").unwrap();
        assert_eq!(
            coding.verdict("https://x.com/home"),
            Verdict::Restrict(RuleAction::Block),
            "from the included list"
        );
        assert_eq!(
            coding.verdict("https://reddit.com/"),
            Verdict::Restrict(RuleAction::HideFeed)
        );
        assert_eq!(
            coding.verdict("https://reddit.com/r/rust/comments/a"),
            Verdict::Allow
        );
        assert_eq!(coding.rules.len(), 3, "the bad pattern is left out");
        assert_eq!(coding.apps, ["discord"]);
        assert_eq!(settings().resolve("gaming"), None);
    }

    #[test]
    fn patterns_are_reachable_by_action() {
        let mut patterns = RulePatterns::default();
        for (index, verdict) in ACTIONS.iter().enumerate() {
            patterns.get_mut(*verdict).push(format!("site{index}.com"));
        }
        assert_eq!(patterns.ask, ["site1.com"]);
        assert_eq!(
            patterns.get(Verdict::Restrict(RuleAction::Block)),
            ["site4.com"]
        );
    }
}
