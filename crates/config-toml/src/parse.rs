//! Turns settings text into validated configurations with shared lists included.

use std::collections::BTreeMap;
use std::time::Duration;

use focus_core::{
    AutoStartRule, BlockConfig, Mode, Rule, SessionState, Settings, SharedList, UrlPattern, Verdict,
};

use crate::error::{Problem, SettingsError};
use crate::schema::{AutoStartTable, RuleGroup, SettingsFile};

/// Parses and validates settings text. Returns the settings (configurations sorted by name), or
/// every problem found.
pub fn parse_settings(text: &str) -> Result<Settings, SettingsError> {
    // Some Windows editors start UTF-8 files with a byte-order mark, which TOML doesn't allow.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let file: SettingsFile = toml::from_str(text).map_err(|error| SettingsError::Syntax {
        message: error.to_string(),
    })?;

    let mut problems = Vec::new();
    let lists: BTreeMap<&str, SharedList> = file
        .lists
        .iter()
        .map(|(name, table)| {
            check_name("lists", name, &mut problems);
            let rules = build_rules(&format!("lists.{name}"), table.groups(), &mut problems);
            (
                name.as_str(),
                SharedList {
                    name: name.clone(),
                    rules,
                },
            )
        })
        .collect();

    let configs: Vec<BlockConfig> = file
        .configs
        .iter()
        .map(|(name, table)| {
            check_name("configs", name, &mut problems);
            let mut config = BlockConfig::new(name.clone(), table.mode);
            for list in &table.include {
                match lists.get(list.as_str()) {
                    Some(list) => config.include(list),
                    None => problems.push(Problem::UnknownList {
                        config: name.clone(),
                        list: list.clone(),
                    }),
                }
            }
            let own = build_rules(&format!("configs.{name}"), table.groups(), &mut problems);
            config.rules.extend(own);
            config.apps.clone_from(&table.apps);
            let allows_something = config
                .rules
                .iter()
                .any(|rule| rule.verdict == Verdict::Allow);
            if config.mode == Mode::Allowlist && !allows_something {
                problems.push(Problem::EmptyAllowlist {
                    config: name.clone(),
                });
            }
            config
        })
        .collect();

    let auto_start = file
        .auto_start
        .iter()
        .enumerate()
        .map(|(index, table)| build_auto_start(index + 1, table, &configs, &mut problems))
        .collect();

    let friction = file.friction.friction();
    problems.extend(
        friction
            .problems()
            .into_iter()
            .map(|(key, accepts)| Problem::Friction { key, accepts }),
    );

    if problems.is_empty() {
        Ok(Settings {
            configs,
            start_at_login: file.start_at_login,
            auto_start,
            friction,
            parking_hotkey: file.parking_hotkey,
            theme: file.theme,
            timer_on_top: file.timer_on_top,
        })
    } else {
        Err(SettingsError::Invalid { problems })
    }
}

/// Rules from each key of a table, recording a problem for every pattern that doesn't parse.
fn build_rules(table: &str, groups: [RuleGroup<'_>; 5], problems: &mut Vec<Problem>) -> Vec<Rule> {
    let mut rules = Vec::new();
    for (key, patterns, verdict) in groups {
        for pattern in patterns {
            match UrlPattern::parse(pattern) {
                Ok(pattern) => rules.push(Rule { pattern, verdict }),
                Err(error) => problems.push(Problem::BadPattern {
                    location: format!("{table}.{key}"),
                    pattern: pattern.clone(),
                    error,
                }),
            }
        }
    }
    rules
}

/// One automatic-start rule, recording a problem for anything that doesn't make sense.
fn build_auto_start(
    number: usize,
    table: &AutoStartTable,
    configs: &[BlockConfig],
    problems: &mut Vec<Problem>,
) -> AutoStartRule {
    if !configs.iter().any(|config| config.name == table.config) {
        problems.push(Problem::AutoStartUnknownConfig {
            number,
            config: table.config.clone(),
        });
    }
    if table.calendar_keywords.is_empty() && table.task_labels.is_empty() {
        problems.push(Problem::AutoStartNoTrigger { number });
    }
    for (key, entries) in [
        ("calendar_keywords", &table.calendar_keywords),
        ("task_labels", &table.task_labels),
    ] {
        let blank = entries
            .iter()
            .any(|entry| entry.trim().trim_start_matches('@').is_empty());
        if blank {
            problems.push(Problem::AutoStartBlankEntry { number, key });
        }
    }
    let max_minutes = SessionState::MAX_DURATION.as_secs() / 60;
    let task_duration = match table.task_minutes {
        None => AutoStartRule::DEFAULT_TASK_DURATION,
        Some(minutes) if (1..=max_minutes).contains(&minutes) => Duration::from_secs(minutes * 60),
        Some(_) => {
            problems.push(Problem::AutoStartBadMinutes {
                number,
                max: max_minutes,
            });
            AutoStartRule::DEFAULT_TASK_DURATION
        }
    };
    AutoStartRule {
        config: table.config.clone(),
        strictness: table.strictness,
        calendar_keywords: table
            .calendar_keywords
            .iter()
            .map(|keyword| keyword.trim().to_owned())
            .collect(),
        task_labels: table
            .task_labels
            .iter()
            .map(|label| label.trim().trim_start_matches('@').to_owned())
            .collect(),
        task_duration,
    }
}

fn check_name(table: &'static str, name: &str, problems: &mut Vec<Problem>) {
    if name.trim().is_empty() {
        problems.push(Problem::EmptyName { table });
    }
}

#[cfg(test)]
mod tests {
    use focus_core::{Hotkey, PatternError, RuleAction, Theme};

    use super::*;

    /// The example from `docs/DESIGN.md`, plus the `video` list it includes.
    const DESIGN_EXAMPLE: &str = r#"
        [lists.social]
        block = ["x.com", "instagram.com", "tiktok.com"]

        [lists.video]
        block = ["twitch.tv"]

        [configs.coding]
        mode = "blocklist"
        include = ["social", "video"]
        feed_only = ["reddit.com", "youtube.com"]  # hide feeds, allow single posts
        allow = ["reddit.com/r/rust/comments/*", "docs.rs", "github.com"]
        apps = ["discord", "steam"]

        [configs.writing]
        mode = "allowlist"
        allow = ["docs.google.com", "dictionary.com"]
    "#;

    fn problems(text: &str) -> Vec<Problem> {
        match parse_settings(text) {
            Err(SettingsError::Invalid { problems }) => problems,
            other => panic!("expected validation problems, got {other:?}"),
        }
    }

    #[test]
    fn parses_the_design_example() {
        let configs = parse_settings(DESIGN_EXAMPLE).unwrap().configs;
        let [coding, writing] = configs.as_slice() else {
            panic!("expected two configs, got {configs:?}");
        };
        assert_eq!(coding.name, "coding");
        assert_eq!(coding.apps, ["discord", "steam"]);
        assert_eq!(
            coding.verdict("https://twitch.tv/x"),
            Verdict::Restrict(RuleAction::Block)
        );
        assert_eq!(
            coding.verdict("https://www.reddit.com/"),
            Verdict::Restrict(RuleAction::HideFeed)
        );
        assert_eq!(
            coding.verdict("https://reddit.com/r/rust/comments/a/b"),
            Verdict::Allow
        );
        assert_eq!(writing.mode, Mode::Allowlist);
        assert_eq!(
            writing.verdict("https://example.com"),
            Verdict::Restrict(RuleAction::Block)
        );
    }

    #[test]
    fn every_key_maps_to_its_verdict_and_mode_defaults_to_blocklist() {
        let configs = parse_settings(
            r#"
            [configs.all]
            allow = ["a.com"]
            ask = ["b.com"]
            delay = ["c.com"]
            feed_only = ["d.com"]
            block = ["e.com"]
            "#,
        )
        .unwrap()
        .configs;
        let all = &configs[0];
        assert_eq!(all.mode, Mode::Blocklist);
        let verdicts: Vec<_> = ["a.com", "b.com", "c.com", "d.com", "e.com"]
            .iter()
            .map(|url| all.verdict(url))
            .collect();
        assert_eq!(
            verdicts,
            [
                Verdict::Allow,
                Verdict::Restrict(RuleAction::Ask),
                Verdict::Restrict(RuleAction::Delay),
                Verdict::Restrict(RuleAction::HideFeed),
                Verdict::Restrict(RuleAction::Block),
            ]
        );
    }

    #[test]
    fn the_parking_hotkey_is_one_of_the_choices() {
        assert_eq!(
            parse_settings("").unwrap().parking_hotkey,
            Hotkey::CtrlAltSpace
        );
        let off = parse_settings(
            "parking_hotkey = \"off\"
",
        )
        .unwrap();
        assert_eq!(off.parking_hotkey, Hotkey::Off);
        assert!(matches!(
            parse_settings(
                "parking_hotkey = \"ctrl+q\"
"
            ),
            Err(SettingsError::Syntax { .. })
        ));
    }

    #[test]
    fn the_theme_is_one_of_the_choices() {
        assert_eq!(parse_settings("").unwrap().theme, Theme::System);
        assert_eq!(
            parse_settings("theme = \"dark\"\n").unwrap().theme,
            Theme::Dark
        );
        assert!(matches!(
            parse_settings("theme = \"blue\"\n"),
            Err(SettingsError::Syntax { .. })
        ));
    }

    #[test]
    fn friction_limits_are_read_and_checked() {
        let settings =
            parse_settings("[friction]\nunlocks_per_day = 0\ntab_cap = 0\nunlock_minutes = 5\n")
                .unwrap();
        assert_eq!(settings.friction.unlocks_per_day, 0);
        assert_eq!(settings.friction.tab_cap, 0);
        assert_eq!(settings.friction.unlock_minutes, 5);
        assert_eq!(settings.friction.drift_hops, 4, "a default");

        let Err(SettingsError::Invalid { problems }) =
            parse_settings("[friction]\ntab_cap = -3\nunlock_minutes = 9000\n")
        else {
            panic!("out-of-range limits are refused");
        };
        let messages: Vec<String> = problems.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "friction.unlock_minutes: use 1 to 60",
                "friction.tab_cap: use 0 (off) or 2 to 50"
            ]
        );
        assert!(matches!(
            parse_settings("[friction]\ntabs = 4\n"),
            Err(SettingsError::Syntax { .. })
        ));
    }

    #[test]
    fn empty_settings_mean_no_configs() {
        assert_eq!(parse_settings(""), Ok(Settings::default()));
        assert_eq!(
            parse_settings("\u{feff}# just a comment\n"),
            Ok(Settings::default())
        );
        let off = parse_settings("start_at_login = false\n").unwrap();
        assert!(!off.start_at_login);
        assert!(off.configs.is_empty());
    }

    #[test]
    fn reports_every_problem_at_once() {
        let found = problems(
            r#"
            [lists.social]
            block = ["*.x.com"]

            [configs.coding]
            include = ["social", "videos"]
            allow = ["google.com/search?q=rust"]

            [configs.writing]
            mode = "allowlist"
            block = ["x.com"]
            "#,
        );
        assert_eq!(
            found,
            [
                Problem::BadPattern {
                    location: "lists.social.block".into(),
                    pattern: "*.x.com".into(),
                    error: PatternError::InvalidHost {
                        host: "*.x.com".into()
                    },
                },
                Problem::UnknownList {
                    config: "coding".into(),
                    list: "videos".into(),
                },
                Problem::BadPattern {
                    location: "configs.coding.allow".into(),
                    pattern: "google.com/search?q=rust".into(),
                    error: PatternError::QueryNotSupported {
                        pattern: "google.com/search?q=rust".into()
                    },
                },
                Problem::EmptyAllowlist {
                    config: "writing".into(),
                },
            ]
        );
    }

    #[test]
    fn problems_read_well() {
        let error = parse_settings("[configs.writing]\nmode = \"allowlist\"\n").unwrap_err();
        assert_eq!(
            error.to_string(),
            "the settings have problems:\n\
             - configs.writing: an allowlist needs at least one `allow` entry, or every site is blocked"
        );
    }

    #[test]
    fn parses_automatic_starts_in_file_order() {
        let settings = parse_settings(&format!(
            r#"{DESIGN_EXAMPLE}
            [[auto_start]]
            config = "coding"
            strictness = "deep"
            calendar_keywords = [" #focus "]
            task_labels = ["@deep"]
            task_minutes = 25

            [[auto_start]]
            config = "writing"
            calendar_keywords = ["Writing"]
            "#
        ))
        .unwrap();
        let [coding, writing] = settings.auto_start.as_slice() else {
            panic!("expected two rules, got {:?}", settings.auto_start);
        };
        assert_eq!(
            coding,
            &AutoStartRule {
                config: "coding".into(),
                strictness: focus_core::Strictness::Deep,
                calendar_keywords: vec!["#focus".into()],
                task_labels: vec!["deep".into()],
                task_duration: Duration::from_secs(25 * 60),
            }
        );
        assert_eq!(
            writing.strictness,
            focus_core::Strictness::Light,
            "defaults to light"
        );
        assert_eq!(writing.task_duration, AutoStartRule::DEFAULT_TASK_DURATION);
    }

    #[test]
    fn reports_automatic_start_problems() {
        let found = problems(&format!(
            r#"{DESIGN_EXAMPLE}
            [[auto_start]]
            config = "gaming"
            calendar_keywords = ["play"]

            [[auto_start]]
            config = "coding"

            [[auto_start]]
            config = "coding"
            calendar_keywords = ["  "]
            task_labels = ["@"]
            task_minutes = 0
            "#
        ));
        assert_eq!(
            found,
            [
                Problem::AutoStartUnknownConfig {
                    number: 1,
                    config: "gaming".into()
                },
                Problem::AutoStartNoTrigger { number: 2 },
                Problem::AutoStartBlankEntry {
                    number: 3,
                    key: "calendar_keywords"
                },
                Problem::AutoStartBlankEntry {
                    number: 3,
                    key: "task_labels"
                },
                Problem::AutoStartBadMinutes {
                    number: 3,
                    max: 720
                },
            ]
        );
        assert_eq!(
            found[1].to_string(),
            "auto_start #2: add `calendar_keywords` or `task_labels`, or it never starts"
        );
    }

    #[test]
    fn rejects_empty_names() {
        assert_eq!(
            problems("[configs.\"\"]\nblock = [\"x.com\"]\n"),
            [Problem::EmptyName { table: "configs" }]
        );
    }

    #[test]
    fn typos_and_bad_values_are_syntax_errors() {
        for text in [
            "[configs.coding]\nalow = [\"docs.rs\"]\n",
            "[configs.coding]\nmode = \"strict\"\n",
            "[config.coding]\n",
            "[configs.coding\n",
        ] {
            assert!(
                matches!(parse_settings(text), Err(SettingsError::Syntax { .. })),
                "{text:?} should be a syntax error"
            );
        }
        let message = parse_settings("[configs.coding]\nalow = [\"docs.rs\"]\n")
            .unwrap_err()
            .to_string();
        assert!(message.contains("alow"), "names the bad key: {message}");
    }
}
