//! Errors worded for the person editing the settings file.

use focus_core::PatternError;
use thiserror::Error;

/// Why the settings text couldn't be turned into configurations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettingsError {
    /// Not valid TOML, or a key or value the settings don't support.
    #[error("{message}")]
    Syntax {
        /// The TOML parser's message, with line and column.
        message: String,
    },
    /// Valid TOML whose contents don't make sense; lists every problem found.
    #[error("{}", bullet_list(.problems))]
    Invalid {
        /// Every problem, in file order (lists first, then configurations, each sorted by name).
        problems: Vec<Problem>,
    },
}

/// One thing wrong with otherwise well-formed settings.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Problem {
    /// A list or configuration named `""`.
    #[error("[{table}.\"\"] needs a name")]
    EmptyName {
        /// `lists` or `configs`.
        table: &'static str,
    },
    /// `include` names a list that isn't defined.
    #[error("configs.{config}.include: there is no list named \"{list}\" (add [lists.{list}])")]
    UnknownList {
        /// The configuration with the bad include.
        config: String,
        /// The missing list.
        list: String,
    },
    /// An allowlist with nothing allowed would block every site.
    #[error(
        "configs.{config}: an allowlist needs at least one `allow` entry, or every site is blocked"
    )]
    EmptyAllowlist {
        /// The configuration.
        config: String,
    },
    /// A URL pattern that doesn't parse.
    #[error("{location}: \"{pattern}\": {error}")]
    BadPattern {
        /// Where it is, e.g. `configs.coding.allow`.
        location: String,
        /// The pattern as written.
        pattern: String,
        /// Why it was rejected.
        error: PatternError,
    },
}

fn bullet_list(problems: &[Problem]) -> String {
    let lines: Vec<String> = problems
        .iter()
        .map(|problem| format!("- {problem}"))
        .collect();
    format!("the settings have problems:\n{}", lines.join("\n"))
}
