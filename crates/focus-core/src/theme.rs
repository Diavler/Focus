//! Whether the window is light or dark. A closed set, so the settings offer a segmented control
//! instead of a string to type.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which colours the window uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follows the system's light or dark mode.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

impl Theme {
    /// Every choice, in the order the settings offer them.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// How the settings file writes it.
    pub const fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl fmt::Display for Theme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_serde() {
        for theme in Theme::ALL {
            let json = serde_json::to_string(&theme).unwrap();
            assert_eq!(json, format!("\"{}\"", theme.key()));
        }
        assert_eq!(Theme::default(), Theme::System);
    }
}
