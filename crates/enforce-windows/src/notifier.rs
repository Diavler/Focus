//! The `Notifier` and `NotificationSettings` ports for Windows.
//!
//! Windows has no supported API for an app to turn on Do Not Disturb, so silencing answers
//! [`Silencing::AskUser`]: the window then offers a button, and pressing it opens the notification
//! settings page for the user to switch Do Not Disturb on. Nothing opens by itself.

use std::path::PathBuf;
use std::process::Command;

use focus_core::ports::{NotificationSettings, Notifier, NotifyError, Silencing};

use crate::Mode;

/// Where Windows 11 keeps the Do Not Disturb switch.
const SETTINGS_PAGE: &str = "ms-settings:notifications";

/// Answers that the user has to turn on Do Not Disturb.
#[derive(Debug, Default)]
pub struct WindowsNotifier;

impl Notifier for WindowsNotifier {
    fn silence(&self) -> Result<Silencing, NotifyError> {
        Ok(Silencing::AskUser)
    }

    fn restore(&self) -> Result<(), NotifyError> {
        Ok(())
    }
}

/// Opens the notification settings page.
#[derive(Debug)]
pub struct WindowsNotificationSettings {
    mode: Mode,
}

impl WindowsNotificationSettings {
    /// Opens Settings, or only logs in [`Mode::DryRun`].
    pub fn new(mode: Mode) -> Self {
        Self { mode }
    }
}

impl NotificationSettings for WindowsNotificationSettings {
    fn open(&self) -> Result<(), NotifyError> {
        if self.mode == Mode::DryRun {
            tracing::info!("dry run: would open {SETTINGS_PAGE} to turn on Do Not Disturb");
            return Ok(());
        }
        // explorer.exe by full path: a bare name would be searched for in the (user-writable)
        // install folder first.
        let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        Command::new(PathBuf::from(system_root).join("explorer.exe"))
            .arg(SETTINGS_PAGE)
            .spawn()
            .map(|_| ())
            .map_err(|error| NotifyError::Unavailable {
                reason: format!("couldn't open notification settings: {error}"),
            })
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::{notification_settings_contract, notifier_contract};

    use super::*;

    #[test]
    fn asks_the_user_and_passes_the_contracts() {
        notifier_contract(&WindowsNotifier);
        assert_eq!(WindowsNotifier.silence(), Ok(Silencing::AskUser));
        // Live mode would open the Settings app, so only the dry run is tested automatically.
        notification_settings_contract(&WindowsNotificationSettings::new(Mode::DryRun));
    }
}
