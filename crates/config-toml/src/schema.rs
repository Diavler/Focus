//! The settings file as written, before validation. Unknown keys are rejected so typos surface.

use std::collections::BTreeMap;

use focus_core::{Friction, Hotkey, Mode, RuleAction, Strictness, Theme, Verdict};
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
    #[serde(default)]
    pub(crate) friction: FrictionTable,
    #[serde(default)]
    pub(crate) parking_hotkey: Hotkey,
    #[serde(default)]
    pub(crate) theme: Theme,
}

/// `[friction]`: unlocks, the tab cap and the drift prompt. Read as plain integers, so an
/// out-of-range value gets a clear problem instead of a type error.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrictionTable {
    pub(crate) unlocks_per_day: Option<i64>,
    pub(crate) unlock_minutes: Option<i64>,
    pub(crate) tab_cap: Option<i64>,
    pub(crate) drift_hops: Option<i64>,
}

impl FrictionTable {
    /// The limits as written, defaults for keys left out. Values that don't fit become the
    /// type's maximum, which [`Friction::problems`] then reports.
    pub(crate) fn friction(&self) -> Friction {
        let defaults = Friction::default();
        let small = |written: Option<i64>, default: u8| {
            written.map_or(default, |value| u8::try_from(value).unwrap_or(u8::MAX))
        };
        Friction {
            unlocks_per_day: small(self.unlocks_per_day, defaults.unlocks_per_day),
            unlock_minutes: self
                .unlock_minutes
                .map_or(defaults.unlock_minutes, |value| {
                    u16::try_from(value).unwrap_or(u16::MAX)
                }),
            tab_cap: small(self.tab_cap, defaults.tab_cap),
            drift_hops: small(self.drift_hops, defaults.drift_hops),
        }
    }
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
    pub(crate) allow: Vec<String>,
    #[serde(default)]
    pub(crate) ask: Vec<String>,
    #[serde(default)]
    pub(crate) delay: Vec<String>,
    #[serde(default)]
    pub(crate) feed_only: Vec<String>,
    #[serde(default)]
    pub(crate) block: Vec<String>,
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
    pub(crate) allow: Vec<String>,
    #[serde(default)]
    pub(crate) ask: Vec<String>,
    #[serde(default)]
    pub(crate) delay: Vec<String>,
    #[serde(default)]
    pub(crate) feed_only: Vec<String>,
    #[serde(default)]
    pub(crate) block: Vec<String>,
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
