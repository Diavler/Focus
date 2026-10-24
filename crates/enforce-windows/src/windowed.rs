//! The `WindowedApps` port for Windows: the current user's apps with a visible window.

use focus_core::ports::{EnforceError, WindowedApps};

use crate::closing::windows_by_process;
use crate::processes::{all_processes, is_protected};

/// Hosts that own windows but aren't apps anyone would block.
const NOT_APPS: [&str; 5] = [
    "applicationframehost",
    "textinputhost",
    "lockapp",
    "systemsettings",
    "widgets",
];

/// Lists apps from the visible top-level windows on the desktop.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsWindowedApps;

impl WindowedApps for WindowsWindowedApps {
    fn windowed_apps(&self) -> Result<Vec<String>, EnforceError> {
        let windows = windows_by_process();
        let processes = all_processes().map_err(|error| EnforceError::Failed {
            reason: error.to_string(),
        })?;
        let mut apps: Vec<String> = processes
            .into_iter()
            .filter(|process| windows.contains_key(&process.pid))
            .filter(|process| !is_protected(&process.app))
            .filter(|process| {
                !NOT_APPS
                    .iter()
                    .any(|host| host.eq_ignore_ascii_case(&process.app))
            })
            .filter(|process| win_user::is_current_user_process(process.pid))
            .map(|process| process.app)
            .collect();
        apps.sort_by_key(|app| app.to_lowercase());
        apps.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        Ok(apps)
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::windowed_apps_contract;

    use super::*;

    #[test]
    fn passes_the_contract() {
        // Read-only: it only lists windows, so it runs live.
        windowed_apps_contract(&WindowsWindowedApps);
    }
}
