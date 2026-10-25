//! Reading the settings as written and writing the window's changes back, keeping comments and
//! layout wherever the window didn't change something.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use focus_core::{
    ACTIONS, AutoStartRule, ConfigEntry, EditableSettings, Friction, Hotkey, ListEntry, Mode,
    RulePatterns, Strictness, Theme, Verdict,
};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

use crate::error::SettingsError;
use crate::parse::parse_settings;
use crate::schema::{AutoStartTable, SettingsFile};

/// The settings in `text` as written (lists and configs sorted by name). Only the syntax has to
/// be right; problems a save would refuse, such as an unknown list, are kept for the user to fix.
pub fn editable(text: &str) -> Result<EditableSettings, SettingsError> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let file: SettingsFile = toml::from_str(body).map_err(|error| SettingsError::Syntax {
        message: error.to_string(),
    })?;
    Ok(EditableSettings {
        start_at_login: file.start_at_login,
        lists: file
            .lists
            .into_iter()
            .map(|(name, table)| ListEntry {
                name,
                patterns: RulePatterns {
                    allow: table.allow,
                    ask: table.ask,
                    delay: table.delay,
                    feed_only: table.feed_only,
                    block: table.block,
                },
            })
            .collect(),
        configs: file
            .configs
            .into_iter()
            .map(|(name, table)| ConfigEntry {
                name,
                mode: table.mode,
                include: table.include,
                apps: table.apps,
                patterns: RulePatterns {
                    allow: table.allow,
                    ask: table.ask,
                    delay: table.delay,
                    feed_only: table.feed_only,
                    block: table.block,
                },
            })
            .collect(),
        auto_start: file.auto_start.iter().map(rule_from_table).collect(),
        friction: file.friction.friction(),
        parking_hotkey: file.parking_hotkey,
        theme: file.theme,
        timer_on_top: file.timer_on_top,
        revision: revision(text),
    })
}

/// Sets the top-level flag `key`, which defaults to on, written only when it changes (or is off),
/// so a comment beside it stays.
fn set_flag(document: &mut DocumentMut, key: &str, on: bool) {
    let written = document.get(key).and_then(Item::as_bool);
    if written != Some(on) && (written.is_some() || !on) {
        document.insert(key, value(on));
    }
}

/// Sets the top-level `key` to `wanted`, written only when it changes (or isn't `default`), so a
/// comment beside it stays.
fn set_choice(document: &mut DocumentMut, key: &str, wanted: &str, default: &str) {
    let written = document.get(key).and_then(Item::as_str);
    if written != Some(wanted) && (written.is_some() || wanted != default) {
        document.insert(key, value(wanted));
    }
}

/// Identifies a version of the settings text.
pub fn revision(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// `text` changed to say what `settings` says (its revision isn't checked here). Lists and
/// configs are updated in place, so comments around them stay; new ones go at the end, removed
/// ones disappear. The result is checked like any settings file, so an edit that would make the
/// settings invalid is refused.
pub fn with_settings(text: &str, settings: &EditableSettings) -> Result<String, SettingsError> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut document: DocumentMut =
        body.parse()
            .map_err(|error: toml_edit::TomlError| SettingsError::Syntax {
                message: error.to_string(),
            })?;

    set_flag(&mut document, "start_at_login", settings.start_at_login);
    set_flag(&mut document, "timer_on_top", settings.timer_on_top);

    let lists = parent_table(&mut document, "lists");
    lists.retain(|name, _| settings.lists.iter().any(|list| list.name == name));
    for list in &settings.lists {
        let table = child_table(lists, &list.name);
        set_patterns(table, &list.patterns);
    }
    drop_if_empty(&mut document, "lists");

    let configs = parent_table(&mut document, "configs");
    configs.retain(|name, _| settings.configs.iter().any(|config| config.name == name));
    for config in &settings.configs {
        let table = child_table(configs, &config.name);
        if config.mode != Mode::Blocklist || table.contains_key("mode") {
            table.insert("mode", value(mode_name(config.mode)));
        }
        set_array(table, "include", &config.include);
        set_patterns(table, &config.patterns);
        set_array(table, "apps", &config.apps);
    }
    drop_if_empty(&mut document, "configs");

    set_choice(
        &mut document,
        "parking_hotkey",
        settings.parking_hotkey.key(),
        Hotkey::default().key(),
    );
    set_choice(
        &mut document,
        "theme",
        settings.theme.key(),
        Theme::default().key(),
    );
    set_friction(&mut document, settings.friction);

    document.remove("auto_start");
    if !settings.auto_start.is_empty() {
        let mut tables = ArrayOfTables::new();
        for rule in &settings.auto_start {
            tables.push(rule_table(rule));
        }
        document.insert("auto_start", Item::ArrayOfTables(tables));
    }

    let edited = document.to_string();
    parse_settings(&edited)?;
    Ok(edited)
}

/// The top-level table `key` (`[lists]` or `[configs]`), created as an implicit parent (no
/// header of its own) if it's missing.
fn parent_table<'a>(document: &'a mut DocumentMut, key: &str) -> &'a mut Table {
    let item = document.entry(key).or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    });
    if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    // Just made sure it's a table.
    item.as_table_mut().unwrap_or_else(|| unreachable!())
}

/// The sub-table `parent.name`, created at the end if it's missing.
fn child_table<'a>(parent: &'a mut Table, name: &str) -> &'a mut Table {
    let item = parent
        .entry(name)
        .or_insert_with(|| Item::Table(Table::new()));
    if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    // Just made sure it's a table.
    item.as_table_mut().unwrap_or_else(|| unreachable!())
}

/// Writes `[friction]`: only keys that changed or aren't at their defaults, so comments beside
/// the others stay, and no table at all while everything is at its default.
fn set_friction(document: &mut DocumentMut, friction: Friction) {
    let defaults = Friction::default();
    let keys: [(&str, i64, i64); 4] = [
        (
            "unlocks_per_day",
            friction.unlocks_per_day.into(),
            defaults.unlocks_per_day.into(),
        ),
        (
            "unlock_minutes",
            friction.unlock_minutes.into(),
            defaults.unlock_minutes.into(),
        ),
        ("tab_cap", friction.tab_cap.into(), defaults.tab_cap.into()),
        (
            "drift_hops",
            friction.drift_hops.into(),
            defaults.drift_hops.into(),
        ),
    ];
    let all_defaults = keys.iter().all(|&(_, wanted, default)| wanted == default);
    if all_defaults && !document.contains_key("friction") {
        return;
    }
    let item = document
        .entry("friction")
        .or_insert_with(|| Item::Table(Table::new()));
    if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    // Just made sure it's a table.
    let table = item.as_table_mut().unwrap_or_else(|| unreachable!());
    for (key, wanted, default) in keys {
        let written: Option<i64> = table.get(key).and_then(Item::as_integer);
        if written != Some(wanted) && (written.is_some() || wanted != default) {
            table.insert(key, value(wanted));
        }
    }
    drop_if_empty(document, "friction");
}

fn drop_if_empty(document: &mut DocumentMut, key: &str) {
    if document
        .get(key)
        .and_then(Item::as_table)
        .is_some_and(Table::is_empty)
    {
        document.remove(key);
    }
}

/// Sets `key` to `entries`, or removes it when there are none. An unchanged array is left alone,
/// so its formatting and comments stay.
fn set_array(table: &mut Table, key: &str, entries: &[String]) {
    if entries.is_empty() {
        table.remove(key);
        return;
    }
    let unchanged = table
        .get(key)
        .and_then(Item::as_array)
        .is_some_and(|array| {
            array.len() == entries.len()
                && array
                    .iter()
                    .zip(entries)
                    .all(|(old, new)| old.as_str() == Some(new.as_str()))
        });
    if !unchanged {
        let array: Array = entries.iter().map(String::as_str).collect();
        table.insert(key, value(array));
    }
}

fn set_patterns(table: &mut Table, patterns: &RulePatterns) {
    for verdict in ACTIONS {
        set_array(table, action_key(verdict), patterns.get(verdict));
    }
}

/// The settings file's key for an action.
fn action_key(verdict: Verdict) -> &'static str {
    use focus_core::RuleAction;
    match verdict {
        Verdict::Allow => "allow",
        Verdict::Restrict(RuleAction::Ask) => "ask",
        Verdict::Restrict(RuleAction::Delay) => "delay",
        Verdict::Restrict(RuleAction::HideFeed) => "feed_only",
        Verdict::Restrict(RuleAction::Block) => "block",
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Blocklist => "blocklist",
        Mode::Allowlist => "allowlist",
    }
}

fn rule_from_table(table: &AutoStartTable) -> AutoStartRule {
    AutoStartRule {
        config: table.config.clone(),
        strictness: table.strictness,
        calendar_keywords: table.calendar_keywords.clone(),
        task_labels: table.task_labels.clone(),
        task_duration: table
            .task_minutes
            .map_or(AutoStartRule::DEFAULT_TASK_DURATION, |minutes| {
                std::time::Duration::from_secs(minutes.saturating_mul(60))
            }),
    }
}

/// One `[[auto_start]]` table, leaving out keys at their defaults.
fn rule_table(rule: &AutoStartRule) -> Table {
    let mut table = Table::new();
    table.insert("config", value(rule.config.as_str()));
    table.insert(
        "strictness",
        value(match rule.strictness {
            Strictness::Light => "light",
            Strictness::Deep => "deep",
            Strictness::Locked => "locked",
        }),
    );
    set_array(&mut table, "calendar_keywords", &rule.calendar_keywords);
    set_array(&mut table, "task_labels", &rule.task_labels);
    if rule.task_duration != AutoStartRule::DEFAULT_TASK_DURATION {
        let minutes = i64::try_from(rule.task_duration.as_secs() / 60).unwrap_or(i64::MAX);
        table.insert("task_minutes", value(minutes));
    }
    table
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const TEXT: &str = r##"# My settings
start_at_login = false  # keep this comment

[lists.social]
block = ["x.com"]

[configs.coding]
include = ["social"]
block = ["twitch.tv"]  # streams
apps = ["discord"]

[[auto_start]]
config = "coding"
calendar_keywords = ["old"]

# a note about writing
[configs.writing]
mode = "allowlist"
allow = ["docs.google.com"]
"##;

    fn rule(config: &str) -> AutoStartRule {
        AutoStartRule {
            config: config.into(),
            strictness: Strictness::Deep,
            calendar_keywords: vec!["#focus".into(), "Deep work".into()],
            task_labels: vec!["deep".into()],
            task_duration: Duration::from_secs(25 * 60),
        }
    }

    #[test]
    fn reads_the_settings_as_written() {
        let settings = editable(TEXT).unwrap();
        assert!(!settings.start_at_login);
        assert_eq!(settings.lists[0].name, "social");
        let coding = &settings.configs[0];
        assert_eq!(coding.include, ["social"]);
        assert_eq!(
            coding.patterns.block,
            ["twitch.tv"],
            "own rules only, not the list's"
        );
        assert_eq!(settings.configs[1].mode, Mode::Allowlist);
        assert_eq!(settings.auto_start[0].calendar_keywords, ["old"]);
        assert_eq!(settings.revision, revision(TEXT));
        assert_ne!(revision(TEXT), revision(&format!("{TEXT}\n")));
    }

    #[test]
    fn writing_back_unchanged_settings_keeps_the_text() {
        let settings = editable(TEXT).unwrap();
        let written = with_settings(TEXT, &settings).unwrap();
        for kept in [
            "start_at_login = false  # keep this comment",
            "block = [\"twitch.tv\"]  # streams",
            "# a note about writing",
        ] {
            assert!(written.contains(kept), "lost {kept:?}:\n{written}");
        }
        assert_eq!(editable(&written).unwrap().configs, settings.configs);
    }

    #[test]
    fn edits_add_change_and_remove() {
        let mut settings = editable(TEXT).unwrap();
        settings.start_at_login = true;
        settings.configs.retain(|config| config.name != "writing");
        settings.configs[0]
            .patterns
            .block
            .push("netflix.com".into());
        settings.configs[0].apps.clear();
        settings.lists.push(ListEntry {
            name: "news".into(),
            patterns: RulePatterns {
                delay: vec!["news.ycombinator.com".into()],
                ..RulePatterns::default()
            },
        });
        settings.configs.push(ConfigEntry {
            name: "reading".into(),
            mode: Mode::Allowlist,
            include: vec!["news".into()],
            patterns: RulePatterns {
                allow: vec!["wikipedia.org".into()],
                ..RulePatterns::default()
            },
            apps: Vec::new(),
        });
        settings.auto_start = vec![rule("reading")];
        let written = with_settings(TEXT, &settings).unwrap();
        assert!(!written.contains("[configs.writing]"), "{written}");
        assert!(written.contains("start_at_login = true"), "{written}");
        let reread = editable(&written).unwrap();
        assert_eq!(
            reread
                .configs
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["coding", "reading"]
        );
        assert_eq!(
            reread.configs[0].patterns.block,
            ["twitch.tv", "netflix.com"]
        );
        assert!(reread.configs[0].apps.is_empty());
        assert_eq!(reread.lists.len(), 2);
        assert_eq!(reread.auto_start, [rule("reading")]);
        let loaded = parse_settings(&written).unwrap();
        assert_eq!(loaded.configs.len(), 2);
    }

    #[test]
    fn writes_into_an_empty_file() {
        let settings = EditableSettings {
            start_at_login: true,
            lists: Vec::new(),
            configs: vec![ConfigEntry {
                name: "coding".into(),
                mode: Mode::Blocklist,
                include: Vec::new(),
                patterns: RulePatterns {
                    block: vec!["x.com".into()],
                    ..RulePatterns::default()
                },
                apps: Vec::new(),
            }],
            auto_start: Vec::new(),
            friction: Friction::default(),
            parking_hotkey: Hotkey::default(),
            theme: Theme::default(),
            timer_on_top: true,
            revision: String::new(),
        };
        let written = with_settings("", &settings).unwrap();
        assert!(written.contains("[configs.coding]"), "{written}");
        assert!(
            !written.contains("[configs]\n"),
            "no empty parent header: {written}"
        );
        assert!(
            !written.contains("start_at_login"),
            "the default isn't written"
        );
        assert!(!written.contains("[lists]"));
        assert!(!written.contains("[friction]"), "defaults aren't written");
        assert!(
            !written.contains("parking_hotkey"),
            "the default isn't written"
        );
        assert_eq!(editable(&written).unwrap().configs, settings.configs);
    }

    #[test]
    fn the_parking_hotkey_round_trips() {
        let text = "parking_hotkey = \"ctrl+alt+p\"  # Space is taken
";
        let mut settings = editable(text).unwrap();
        assert_eq!(settings.parking_hotkey, Hotkey::CtrlAltP);
        assert_eq!(with_settings(text, &settings).unwrap(), text, "unchanged");
        settings.parking_hotkey = Hotkey::Off;
        let written = with_settings(text, &settings).unwrap();
        assert!(written.contains("parking_hotkey = \"off\""), "{written}");
        assert_eq!(
            parse_settings(&written).unwrap().parking_hotkey,
            Hotkey::Off
        );
    }

    #[test]
    fn the_theme_round_trips() {
        let text = "theme = \"light\"  # bright room
";
        let mut settings = editable(text).unwrap();
        assert_eq!(settings.theme, Theme::Light);
        assert_eq!(with_settings(text, &settings).unwrap(), text, "unchanged");
        settings.theme = Theme::Dark;
        let written = with_settings(text, &settings).unwrap();
        assert!(written.contains("theme = \"dark\""), "{written}");
        assert_eq!(parse_settings(&written).unwrap().theme, Theme::Dark);
        settings.theme = Theme::System;
        assert!(
            !with_settings("", &settings).unwrap().contains("theme"),
            "the default isn't written"
        );
    }

    #[test]
    fn timer_on_top_defaults_on_and_round_trips() {
        let mut settings = editable("").unwrap();
        assert!(settings.timer_on_top);
        assert!(
            !with_settings("", &settings)
                .unwrap()
                .contains("timer_on_top"),
            "the default isn't written"
        );
        settings.timer_on_top = false;
        let written = with_settings("", &settings).unwrap();
        assert!(written.contains("timer_on_top = false"), "{written}");
        assert!(!parse_settings(&written).unwrap().timer_on_top);
    }

    #[test]
    fn friction_round_trips_and_keeps_comments() {
        let text = "[friction]\ntab_cap = 6  # tabs are my weakness\n";
        let mut settings = editable(text).unwrap();
        assert_eq!(settings.friction.tab_cap, 6);
        assert_eq!(settings.friction.unlocks_per_day, 3, "a default");
        settings.friction.unlocks_per_day = 1;
        let written = with_settings(text, &settings).unwrap();
        assert!(written.contains("# tabs are my weakness"), "{written}");
        assert!(written.contains("unlocks_per_day = 1"), "{written}");
        assert!(!written.contains("drift_hops"), "defaults stay out");
        assert_eq!(
            parse_settings(&written).unwrap().friction,
            settings.friction
        );

        settings.friction = Friction::default();
        let reset = with_settings(&written, &settings).unwrap();
        assert!(
            reset.contains("tab_cap = 10"),
            "a written key is kept: {reset}"
        );
        settings.friction.tab_cap = 1;
        assert!(
            with_settings(text, &settings).is_err(),
            "an out-of-range cap is refused"
        );
    }

    #[test]
    fn refuses_settings_that_would_be_invalid() {
        let mut settings = editable(TEXT).unwrap();
        settings.auto_start = vec![rule("gaming")];
        assert!(matches!(
            with_settings(TEXT, &settings),
            Err(SettingsError::Invalid { .. })
        ));
        let mut settings = editable(TEXT).unwrap();
        settings.lists.clear();
        assert!(
            with_settings(TEXT, &settings).is_err(),
            "coding includes a removed list"
        );
        let mut settings = editable(TEXT).unwrap();
        settings.configs[0].patterns.block.push("*.x.com".into());
        assert!(with_settings(TEXT, &settings).is_err(), "bad pattern");
        assert!(matches!(
            editable("[configs\n"),
            Err(SettingsError::Syntax { .. })
        ));
    }
}
