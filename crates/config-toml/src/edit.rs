//! Writing changes from the window into the settings text, keeping its comments and layout.

use focus_core::{AutoStartRule, Strictness};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

use crate::error::SettingsError;
use crate::parse::parse_settings;

/// `text` with its `[[auto_start]]` tables replaced by `rules` (appended at the end). Everything
/// else is kept as written. The result is checked like any settings file, so an edit that would
/// make the settings invalid is refused.
pub fn with_auto_start(text: &str, rules: &[AutoStartRule]) -> Result<String, SettingsError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut document: DocumentMut =
        text.parse()
            .map_err(|error: toml_edit::TomlError| SettingsError::Syntax {
                message: error.to_string(),
            })?;
    document.remove("auto_start");
    if !rules.is_empty() {
        let mut tables = ArrayOfTables::new();
        for rule in rules {
            tables.push(rule_table(rule));
        }
        document.insert("auto_start", Item::ArrayOfTables(tables));
    }
    let edited = document.to_string();
    parse_settings(&edited)?;
    Ok(edited)
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
    for (key, entries) in [
        ("calendar_keywords", &rule.calendar_keywords),
        ("task_labels", &rule.task_labels),
    ] {
        if !entries.is_empty() {
            let array: Array = entries.iter().map(String::as_str).collect();
            table.insert(key, value(array));
        }
    }
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

[configs.coding]
block = ["x.com"]

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
    fn replaces_the_rules_and_keeps_the_rest() {
        let edited = with_auto_start(TEXT, &[rule("writing")]).unwrap();
        assert!(
            edited.contains("start_at_login = false  # keep this comment"),
            "{edited}"
        );
        assert!(edited.contains("# a note about writing"), "{edited}");
        assert!(
            !edited.contains("\"old\""),
            "the old rule is gone: {edited}"
        );
        let settings = parse_settings(&edited).unwrap();
        assert_eq!(settings.auto_start, [rule("writing")]);
        assert_eq!(settings.configs.len(), 2);
        assert!(!settings.start_at_login);
    }

    #[test]
    fn defaults_are_left_out_and_no_rules_removes_the_tables() {
        let plain = AutoStartRule {
            task_duration: AutoStartRule::DEFAULT_TASK_DURATION,
            task_labels: Vec::new(),
            ..rule("coding")
        };
        let edited = with_auto_start(TEXT, &[plain]).unwrap();
        assert!(!edited.contains("task_minutes") && !edited.contains("task_labels"));
        let cleared = with_auto_start(TEXT, &[]).unwrap();
        assert!(!cleared.contains("auto_start"), "{cleared}");
        assert!(with_auto_start("", &[]).unwrap().trim().is_empty());
    }

    #[test]
    fn refuses_rules_that_make_the_settings_invalid() {
        assert!(matches!(
            with_auto_start(TEXT, &[rule("gaming")]),
            Err(SettingsError::Invalid { .. })
        ));
        let blank = AutoStartRule {
            calendar_keywords: vec!["  ".into()],
            ..rule("coding")
        };
        assert!(with_auto_start(TEXT, &[blank]).is_err());
        assert!(matches!(
            with_auto_start("[configs\n", &[]),
            Err(SettingsError::Syntax { .. })
        ));
    }
}
