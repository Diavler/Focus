//! Where the daemon keeps its files, following each OS's per-user conventions.

use std::path::PathBuf;

/// The daemon's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The TOML settings the user edits.
    pub settings: PathBuf,
    /// The JSON state the daemon owns.
    pub state: PathBuf,
}

impl Paths {
    /// Paths from the environment: `%APPDATA%` and `%LOCALAPPDATA%` on Windows,
    /// `$XDG_CONFIG_HOME` and `$XDG_STATE_HOME` (or their `~/.config` and `~/.local/state`
    /// defaults) on Linux. `None` if none of them can be worked out.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let var = |name: &str| {
            var(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let home = var("HOME");
        let config = var("APPDATA")
            .map(|dir| dir.join("Focus"))
            .or_else(|| var("XDG_CONFIG_HOME").map(|dir| dir.join("focus")))
            .or_else(|| home.as_ref().map(|home| home.join(".config").join("focus")))?;
        let state = var("LOCALAPPDATA")
            .map(|dir| dir.join("Focus"))
            .or_else(|| var("XDG_STATE_HOME").map(|dir| dir.join("focus")))
            .or_else(|| home.map(|home| home.join(".local").join("state").join("focus")))?;
        Some(Self {
            settings: config.join("settings.toml"),
            state: state.join("state.json"),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;

    fn paths(vars: &[(&str, &str)]) -> Option<Paths> {
        let vars: HashMap<_, _> = vars.iter().copied().collect();
        Paths::from_env(|name| vars.get(name).map(|value| (*value).to_owned()))
    }

    #[test]
    fn windows_uses_appdata() {
        let paths = paths(&[("APPDATA", r"C:\a"), ("LOCALAPPDATA", r"C:\l")]).unwrap();
        assert_eq!(
            paths.settings,
            Path::new(r"C:\a").join("Focus").join("settings.toml")
        );
        assert_eq!(
            paths.state,
            Path::new(r"C:\l").join("Focus").join("state.json")
        );
    }

    #[test]
    fn linux_uses_xdg_dirs_then_home() {
        let xdg = paths(&[
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_STATE_HOME", "/s"),
            ("HOME", "/h"),
        ])
        .unwrap();
        assert_eq!(xdg.settings, Path::new("/c/focus/settings.toml"));
        assert_eq!(xdg.state, Path::new("/s/focus/state.json"));

        let home = paths(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "")]).unwrap();
        assert_eq!(home.settings, Path::new("/h/.config/focus/settings.toml"));
        assert_eq!(home.state, Path::new("/h/.local/state/focus/state.json"));
    }

    #[test]
    fn nothing_to_go_on() {
        assert_eq!(paths(&[]), None);
    }
}
