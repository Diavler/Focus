//! The `Notifier` port for Windows.
//!
//! Windows has no supported API for an app to turn on Do Not Disturb, so "silencing" opens the
//! notification settings page once per session for the user to switch it on. Restoring can't be
//! done for them either; it just allows the next session to prompt again.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, PoisonError};

use focus_core::ports::{Notifier, NotifyError};

use crate::Mode;

/// Where Windows 11 keeps the Do Not Disturb switch.
const SETTINGS_PAGE: &str = "ms-settings:notifications";

/// Prompts the user to turn on Do Not Disturb.
#[derive(Debug)]
pub struct WindowsNotifier {
    mode: Mode,
    prompted: Mutex<bool>,
}

impl WindowsNotifier {
    /// A notifier that opens Settings, or only logs in [`Mode::DryRun`].
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            prompted: Mutex::new(false),
        }
    }
}

impl Notifier for WindowsNotifier {
    fn silence(&self) -> Result<(), NotifyError> {
        let mut prompted = self.prompted.lock().unwrap_or_else(PoisonError::into_inner);
        if *prompted {
            return Ok(());
        }
        *prompted = true;
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

    fn restore(&self) -> Result<(), NotifyError> {
        *self.prompted.lock().unwrap_or_else(PoisonError::into_inner) = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::notifier_contract;

    use super::*;

    #[test]
    fn dry_run_passes_the_contract() {
        // Live mode would open the Settings app, so only the dry run is tested automatically.
        notifier_contract(&WindowsNotifier::new(Mode::DryRun));
    }
}
