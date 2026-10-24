//! KDE Plasma (Wayland) enforcement: `/proc` process control, KWin script, notification inhibit.
//!
//! Not built yet: the build plan does Linux after Windows (step 9). Until then
//! [`PendingEnforcer`] stands in, doing nothing and saying so once, so the daemon builds and its
//! tests run on Linux.

use std::sync::atomic::{AtomicBool, Ordering};

use focus_core::KnownBrowser;
use focus_core::ports::{
    EnforceError, Enforcer, ForegroundApp, Notifier, NotifyError, ProcessList,
};

/// No enforcement yet: closes nothing, sees no apps, and leaves notifications alone.
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
    fn silence(&self) -> Result<(), NotifyError> {
        self.warn_once();
        Ok(())
    }

    fn restore(&self) -> Result<(), NotifyError> {
        Ok(())
    }
}

/// Browsers to watch on Linux: none until step 9.
pub fn known_browsers() -> Vec<KnownBrowser> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use focus_core::testing::{
        enforcer_contract, foreground_app_contract, notifier_contract, process_list_contract,
    };

    use super::*;

    #[test]
    fn passes_every_contract() {
        let pending = PendingEnforcer::default();
        enforcer_contract(&pending);
        process_list_contract(&pending);
        foreground_app_contract(&pending);
        notifier_contract(&pending);
    }
}
