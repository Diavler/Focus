//! KDE Plasma (Wayland) enforcement: `/proc` process control, KWin script, notification inhibit.
//!
//! Not built yet: the build plan does Linux after Windows (step 9). Until then
//! [`PendingEnforcer`] stands in, doing nothing and saying so once, so the daemon builds and its
//! tests run on Linux.

use std::sync::atomic::{AtomicBool, Ordering};

use focus_core::KnownBrowser;
use focus_core::ports::{
    Autostart, AutostartError, EnforceError, Enforcer, ForegroundApp, NotificationSettings,
    Notifier, NotifyError, ProcessList, Silencing, WindowedApps,
};

/// No enforcement yet: closes nothing, sees no apps, leaves notifications alone and doesn't
/// register for login.
#[derive(Debug, Default)]
pub struct PendingEnforcer {
    warned: AtomicBool,
}

impl PendingEnforcer {
    fn warn_once(&self) {
        if !self.warned.swap(true, Ordering::Relaxed) {
            tracing::warn!("app and notification enforcement on Linux arrives in build step 9");
        }
    }
}

impl Enforcer for PendingEnforcer {
    fn close_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError> {
        if !apps.is_empty() {
            self.warn_once();
        }
        Ok(Vec::new())
    }
}

impl ProcessList for PendingEnforcer {
    fn running_apps(&self, _apps: &[String]) -> Result<Vec<String>, EnforceError> {
        Ok(Vec::new())
    }
}

impl ForegroundApp for PendingEnforcer {
    fn foreground_app(&self) -> Result<Option<String>, EnforceError> {
        Ok(None)
    }
}

impl Notifier for PendingEnforcer {
    /// Unsupported, so the window offers no Do Not Disturb button. KDE will silence notifications
    /// by itself (`Notifications.Inhibit`) in step 9.
    fn silence(&self) -> Result<Silencing, NotifyError> {
        self.warn_once();
        Ok(Silencing::Unsupported)
    }

    fn restore(&self) -> Result<(), NotifyError> {
        Ok(())
    }
}

impl NotificationSettings for PendingEnforcer {
    /// Never asked for, since silencing doesn't ask the user; logs if it is.
    fn open(&self) -> Result<(), NotifyError> {
        tracing::info!("no notification settings page to open on Linux yet");
        Ok(())
    }
}

impl Autostart for PendingEnforcer {
    /// Switching on says it isn't supported yet, so the UI doesn't claim Focus starts at login.
    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError> {
        if enabled {
            return Err(AutostartError::Failed {
                reason: "not available on Linux until build step 9".into(),
            });
        }
        Ok(())
    }
}

impl WindowedApps for PendingEnforcer {
    fn windowed_apps(&self) -> Result<Vec<String>, EnforceError> {
        Ok(Vec::new())
    }
}

/// Browsers to watch on Linux: none until step 9.
pub fn known_browsers() -> Vec<KnownBrowser> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use focus_core::testing::{
        autostart_contract, enforcer_contract, foreground_app_contract,
        notification_settings_contract, notifier_contract, process_list_contract,
        windowed_apps_contract,
    };

    use super::*;

    #[test]
    fn passes_every_contract() {
        let pending = PendingEnforcer::default();
        enforcer_contract(&pending);
        process_list_contract(&pending);
        foreground_app_contract(&pending);
        notifier_contract(&pending);
        notification_settings_contract(&pending);
        autostart_contract(&pending);
        windowed_apps_contract(&pending);
    }
}
