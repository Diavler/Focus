//! Telling Chromium browsers where the host is, per user and without admin.
//!
//! Registration is built as a list of [`Action`]s first and only then carried out, so it can be
//! printed instead (`--dry-run`) and the plan is unit-tested without touching the machine.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use thiserror::Error;

/// The host's name as browsers know it.
pub const HOST_NAME: &str = "dev.focus.nmh";

/// Where registration happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// A registry key per browser points at a manifest in the install folder.
    Windows,
    /// A manifest file in each browser's config folder.
    Linux,
}

impl Platform {
    /// The platform this binary runs on.
    pub fn current() -> Self {
        if std::env::consts::FAMILY == "windows" {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    fn host_file_name(self) -> &'static str {
        match self {
            Self::Windows => "focus-nmh.exe",
            Self::Linux => "focus-nmh",
        }
    }
}

/// A Chromium-based browser and where it looks for native messaging hosts.
struct Browser {
    /// Under `HKCU\Software\` on Windows.
    registry: &'static str,
    /// Under `~/.config/` on Linux.
    config_dir: &'static str,
}

const BROWSERS: [Browser; 4] = [
    Browser {
        registry: r"Google\Chrome",
        config_dir: "google-chrome",
    },
    Browser {
        registry: "Chromium",
        config_dir: "chromium",
    },
    Browser {
        registry: r"Microsoft\Edge",
        config_dir: "microsoft-edge",
    },
    Browser {
        registry: r"BraveSoftware\Brave-Browser",
        config_dir: "BraveSoftware/Brave-Browser",
    },
];

/// One change to the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Create or replace a file.
    WriteFile {
        /// Where.
        path: PathBuf,
        /// What.
        contents: String,
    },
    /// Delete a file if it exists.
    RemoveFile {
        /// Which.
        path: PathBuf,
    },
    /// Set a registry key's default value, creating the key (`HKCU` only).
    SetRegistryDefault {
        /// The full key, starting with `HKCU\`.
        key: String,
        /// The value.
        value: String,
    },
    /// Delete a registry key if it exists (`HKCU` only).
    DeleteRegistryKey {
        /// The full key, starting with `HKCU\`.
        key: String,
    },
}

/// Why registration couldn't be planned or carried out.
#[derive(Debug, Error)]
pub enum RegisterError {
    /// Extension IDs are 32 letters from `a` to `p`.
    #[error("\"{0}\" isn't an extension ID (32 letters a-p, shown on chrome://extensions)")]
    BadExtensionId(String),
    /// Linux registration needs the home folder.
    #[error("HOME isn't set, so the browsers' config folders can't be found")]
    NoHome,
    /// No supported browser's config folder exists.
    #[error("no Chromium-based browser found under ~/.config (Chrome, Chromium, Edge or Brave)")]
    NoBrowser,
    /// Carrying out an action failed.
    #[error("{action}: {source}")]
    Failed {
        /// What was being done.
        action: String,
        /// Why it failed.
        source: io::Error,
    },
}

/// What `--register` and `--unregister` need to know about the machine.
pub struct Machine<'a> {
    /// Windows or Linux.
    pub platform: Platform,
    /// The folder holding the executables.
    pub install: &'a Path,
    /// The user's home folder (Linux).
    pub home: Option<&'a Path>,
    /// Whether a path exists; a parameter so tests needn't touch the disk.
    pub exists: &'a dyn Fn(&Path) -> bool,
}

impl Machine<'_> {
    /// The manifest Windows registry keys point at, kept in the install folder.
    fn windows_manifest(&self) -> PathBuf {
        self.install
            .join("native-messaging")
            .join(format!("{HOST_NAME}.json"))
    }

    /// `(browser config folder, manifest path)` for each browser installed on Linux.
    fn linux_manifests(&self) -> Result<Vec<PathBuf>, RegisterError> {
        let config = self.home.ok_or(RegisterError::NoHome)?.join(".config");
        Ok(BROWSERS
            .iter()
            .map(|browser| config.join(browser.config_dir))
            .filter(|dir| (self.exists)(dir))
            .map(|dir| {
                dir.join("NativeMessagingHosts")
                    .join(format!("{HOST_NAME}.json"))
            })
            .collect())
    }
}

/// The manifest a browser reads to find and trust the host.
#[derive(Serialize)]
struct Manifest<'a> {
    name: &'a str,
    description: &'a str,
    path: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    allowed_origins: [String; 1],
}

/// The actions that register the host for `extension_id`.
pub fn register_plan(
    machine: &Machine<'_>,
    extension_id: &str,
) -> Result<Vec<Action>, RegisterError> {
    if extension_id.len() != 32 || !extension_id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
        return Err(RegisterError::BadExtensionId(extension_id.to_owned()));
    }
    let host = machine.install.join(machine.platform.host_file_name());
    let manifest = Manifest {
        name: HOST_NAME,
        description: "Focus: connects the browser extension to the Focus daemon",
        path: &host.to_string_lossy(),
        kind: "stdio",
        allowed_origins: [format!("chrome-extension://{extension_id}/")],
    };
    // A struct of strings always serializes.
    let contents = serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n";
    match machine.platform {
        Platform::Windows => {
            let path = machine.windows_manifest();
            let value = path.to_string_lossy().into_owned();
            let mut actions = vec![Action::WriteFile { path, contents }];
            actions.extend(BROWSERS.iter().map(|browser| Action::SetRegistryDefault {
                key: registry_key(browser),
                value: value.clone(),
            }));
            Ok(actions)
        }
        Platform::Linux => {
            let manifests = machine.linux_manifests()?;
            if manifests.is_empty() {
                return Err(RegisterError::NoBrowser);
            }
            Ok(manifests
                .into_iter()
                .map(|path| Action::WriteFile {
                    path,
                    contents: contents.clone(),
                })
                .collect())
        }
    }
}

/// The actions that undo [`register_plan`].
pub fn unregister_plan(machine: &Machine<'_>) -> Result<Vec<Action>, RegisterError> {
    match machine.platform {
        Platform::Windows => {
            let mut actions: Vec<Action> = BROWSERS
                .iter()
                .map(|browser| Action::DeleteRegistryKey {
                    key: registry_key(browser),
                })
                .collect();
            actions.push(Action::RemoveFile {
                path: machine.windows_manifest(),
            });
            Ok(actions)
        }
        Platform::Linux => Ok(machine
            .linux_manifests()?
            .into_iter()
            .map(|path| Action::RemoveFile { path })
            .collect()),
    }
}

fn registry_key(browser: &Browser) -> String {
    format!(
        r"HKCU\Software\{}\NativeMessagingHosts\{HOST_NAME}",
        browser.registry
    )
}

/// Carries out `actions` in order, stopping at the first failure.
pub fn execute(actions: &[Action]) -> Result<(), RegisterError> {
    for action in actions {
        run(action).map_err(|source| RegisterError::Failed {
            action: action.to_string(),
            source,
        })?;
    }
    Ok(())
}

fn run(action: &Action) -> io::Result<()> {
    match action {
        Action::WriteFile { path, contents } => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, contents)
        }
        Action::RemoveFile { path } => match std::fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
        Action::SetRegistryDefault { key, value } => {
            reg(&["add", key, "/ve", "/t", "REG_SZ", "/d", value, "/f"])
        }
        Action::DeleteRegistryKey { key } => {
            // `reg delete` fails for a key that isn't there; that's already the goal.
            if reg(&["query", key]).is_err() {
                return Ok(());
            }
            reg(&["delete", key, "/f"])
        }
    }
}

/// Runs Windows' built-in `reg.exe`, which only needs admin for `HKLM`; these keys are `HKCU`.
fn reg(args: &[&str]) -> io::Result<()> {
    let output = Command::new("reg").args(args).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteFile { path, .. } => write!(f, "write {}", path.display()),
            Self::RemoveFile { path } => write!(f, "remove {}", path.display()),
            Self::SetRegistryDefault { key, value } => write!(f, "set {key} = {value}"),
            Self::DeleteRegistryKey { key } => write!(f, "delete {key}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    fn machine<'a>(platform: Platform, exists: &'a dyn Fn(&Path) -> bool) -> Machine<'a> {
        Machine {
            platform,
            install: Path::new("Focus"),
            home: Some(Path::new("home")),
            exists,
        }
    }

    #[test]
    fn windows_writes_one_manifest_in_the_install_folder_and_points_each_browser_at_it() {
        let nothing = |_: &Path| false;
        let actions = register_plan(&machine(Platform::Windows, &nothing), ID).unwrap();
        let manifest = Path::new("Focus")
            .join("native-messaging")
            .join("dev.focus.nmh.json");
        let Action::WriteFile { path, contents } = &actions[0] else {
            panic!("first action writes the manifest: {actions:?}");
        };
        assert_eq!(path, &manifest);
        let json: serde_json::Value = serde_json::from_str(contents).unwrap();
        assert_eq!(json["name"], "dev.focus.nmh");
        assert_eq!(json["type"], "stdio");
        assert_eq!(
            json["path"],
            Path::new("Focus")
                .join("focus-nmh.exe")
                .to_string_lossy()
                .as_ref()
        );
        assert_eq!(
            json["allowed_origins"][0],
            format!("chrome-extension://{ID}/")
        );
        assert_eq!(actions.len(), 1 + BROWSERS.len());
        assert!(actions[1..].iter().all(|action| matches!(
            action,
            Action::SetRegistryDefault { key, value }
                if key.starts_with(r"HKCU\Software\") && key.ends_with(r"\NativeMessagingHosts\dev.focus.nmh")
                    && *value == manifest.to_string_lossy()
        )));
    }

    #[test]
    fn linux_writes_a_manifest_only_for_installed_browsers() {
        let chrome = Path::new("home").join(".config").join("google-chrome");
        let only_chrome = |path: &Path| path == chrome;
        let actions = register_plan(&machine(Platform::Linux, &only_chrome), ID).unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            Action::WriteFile { path, .. }
                if *path == chrome.join("NativeMessagingHosts").join("dev.focus.nmh.json")
        ));

        let nothing = |_: &Path| false;
        assert!(matches!(
            register_plan(&machine(Platform::Linux, &nothing), ID),
            Err(RegisterError::NoBrowser)
        ));
    }

    #[test]
    fn unregister_undoes_register() {
        let nothing = |_: &Path| false;
        let windows = unregister_plan(&machine(Platform::Windows, &nothing)).unwrap();
        assert_eq!(windows.len(), BROWSERS.len() + 1);
        assert!(matches!(windows.last(), Some(Action::RemoveFile { .. })));

        let everything = |_: &Path| true;
        let linux = unregister_plan(&machine(Platform::Linux, &everything)).unwrap();
        assert_eq!(linux.len(), BROWSERS.len());
        assert!(
            linux
                .iter()
                .all(|action| matches!(action, Action::RemoveFile { .. }))
        );
    }

    #[test]
    fn rejects_bad_extension_ids() {
        let nothing = |_: &Path| false;
        let windows = machine(Platform::Windows, &nothing);
        for id in [
            "",
            "short",
            "ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP",
            "abcdefghijklmnopabcdefghijklmnoz",
        ] {
            assert!(
                matches!(
                    register_plan(&windows, id),
                    Err(RegisterError::BadExtensionId(_))
                ),
                "{id:?}"
            );
        }
    }

    #[test]
    fn file_actions_write_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b.json");
        execute(&[Action::WriteFile {
            path: path.clone(),
            contents: "{}".into(),
        }])
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        let remove = [Action::RemoveFile { path: path.clone() }];
        execute(&remove).unwrap();
        execute(&remove).unwrap();
        assert!(!path.exists());
    }
}
