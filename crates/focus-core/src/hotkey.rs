//! The shortcut that opens the parking lot from anywhere. A closed set, so the settings offer a
//! pick list instead of a key string to type.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which shortcut opens the parking lot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Hotkey {
    /// No shortcut.
    #[serde(rename = "off")]
    Off,
    /// Ctrl+Alt+Space.
    #[default]
    #[serde(rename = "ctrl+alt+space")]
    CtrlAltSpace,
    /// Ctrl+Shift+Space.
    #[serde(rename = "ctrl+shift+space")]
    CtrlShiftSpace,
    /// Ctrl+Alt+P.
    #[serde(rename = "ctrl+alt+p")]
    CtrlAltP,
}

impl Hotkey {
    /// Every choice, in the order the settings offer them.
    pub const ALL: [Self; 4] = [
        Self::CtrlAltSpace,
        Self::CtrlShiftSpace,
        Self::CtrlAltP,
        Self::Off,
    ];

    /// How the settings file writes it.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::CtrlAltSpace => "ctrl+alt+space",
            Self::CtrlShiftSpace => "ctrl+shift+space",
            Self::CtrlAltP => "ctrl+alt+p",
        }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Off => "Off",
            Self::CtrlAltSpace => "Ctrl+Alt+Space",
            Self::CtrlShiftSpace => "Ctrl+Shift+Space",
            Self::CtrlAltP => "Ctrl+Alt+P",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_serde() {
        for hotkey in Hotkey::ALL {
            let json = serde_json::to_string(&hotkey).unwrap();
            assert_eq!(json, format!("\"{}\"", hotkey.key()));
        }
        assert_eq!(Hotkey::default(), Hotkey::CtrlAltSpace);
    }
}
