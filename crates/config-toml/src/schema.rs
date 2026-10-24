//! The settings file as written, before validation. Unknown keys are rejected so typos surface.

use std::collections::BTreeMap;

use focus_core::{Mode, RuleAction, Strictness, Verdict};
use serde::Deserialize;

/// The whole settings file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsFile {
    #[serde(default = "start_at_login_default")]
    pub(crate) start_at_login: bool,
    #[serde(default)]
    pub(crate) lists: BTreeMap<String, ListTable>,
    #[serde(default)]
    pub(crate) configs: BTreeMap<String, ConfigTable>,
    #[serde(default)]
    pub(crate) auto_start: Vec<AutoStartTable>,
}

/// `[[auto_start]]`: starts a configuration from calendar events or tasks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutoStartTable {
    pub(crate) config: String,
    #[serde(default = "default_strictness")]
    pub(crate) strictness: Strictness,
    #[serde(default)]
    pub(crate) calendar_keywords: Vec<String>,
    #[serde(default)]
    pub(crate) task_labels: Vec<String>,
    pub(crate) task_minutes: Option<u64>,
}

/// `[lists.<name>]`: rules shared by several configurations.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListTable {
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    ask: Vec<String>,
    #[serde(default)]
    delay: Vec<String>,
    #[serde(default)]
    feed_only: Vec<String>,
    #[serde(default)]
    block: Vec<String>,
}

/// `[configs.<name>]`: a block configuration. Repeats the rule keys of [`ListTable`] because serde
/// can't combine `flatten` with `deny_unknown_fields`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigTable {
    #[serde(default = "default_mode")]
    pub(crate) mode: Mode,
    #[serde(default)]
    pub(crate) include: Vec<String>,
    #[serde(default)]
    pub(crate) apps: Vec<String>,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    ask: Vec<String>,
    #[serde(default)]
    delay: Vec<String>,
    #[serde(default)]
    feed_only: Vec<String>,
    #[serde(default)]
    block: Vec<String>,
}

/// One rule key: its name in the file, its patterns and the verdict they get.
pub(crate) type RuleGroup<'a> = (&'static str, &'a [String], Verdict);

impl ListTable {
    /// The rule keys of this list.
    pub(crate) fn groups(&self) -> [RuleGroup<'_>; 5] {
        groups(
            &self.allow,
            &self.ask,
            &self.delay,
            &self.feed_only,
            &self.block,
        )
    }
}

impl ConfigTable {
    /// The rule keys of this configuration.
    pub(crate) fn groups(&self) -> [RuleGroup<'_>; 5] {
        groups(
            &self.allow,
            &self.ask,
            &self.delay,
            &self.feed_only,
            &self.block,
        )
    }
}

fn groups<'a>(
    allow: &'a [String],
    ask: &'a [String],
    delay: &'a [String],
    feed_only: &'a [String],
    block: &'a [String],
) -> [RuleGroup<'a>; 5] {
    [
        ("allow", allow, Verdict::Allow),
        ("ask", ask, Verdict::Restrict(RuleAction::Ask)),
        ("delay", delay, Verdict::Restrict(RuleAction::Delay)),
        (
            "feed_only",
            feed_only,
            Verdict::Restrict(RuleAction::HideFeed),
        ),
        ("block", block, Verdict::Restrict(RuleAction::Block)),
    ]
}

/// Start at login unless the settings say otherwise: it's what brings Focus back after a reboot.
fn start_at_login_default() -> bool {
    true
}

fn default_strictness() -> Strictness {
    Strictness::Light
}

fn default_mode() -> Mode {
    Mode::Blocklist
}
