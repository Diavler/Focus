//! The `Enforcer` and `ProcessList` ports for Windows.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use focus_core::ports::{EnforceError, Enforcer, ProcessList};
use focus_core::same_app;

use crate::Mode;
use crate::closing::{ask_to_close, terminate, top_level_windows};
use crate::processes::{Process, own_matching};

/// How long an app gets to close its windows before it's terminated.
const CLOSE_GRACE: Duration = Duration::from_secs(4);

/// Closes the current user's blocked apps.
///
/// The first call for an app posts `WM_CLOSE` to its windows, so it can save state (browsers
/// restore tabs). A later call, once [`CLOSE_GRACE`] has passed, terminates whatever is left.
/// An app with no windows at all is terminated straight away.
#[derive(Debug)]
pub struct WindowsEnforcer {
    mode: Mode,
    /// When each app (by lower-cased name) was first asked to close.
    asked: Mutex<HashMap<String, Instant>>,
}

impl WindowsEnforcer {
    /// An enforcer that acts, or only logs in [`Mode::DryRun`].
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            asked: Mutex::new(HashMap::new()),
        }
    }

    fn asked(&self) -> MutexGuard<'_, HashMap<String, Instant>> {
        self.asked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Closes one app's processes, politely first.
    fn close_app(&self, app: &str, processes: &[&Process]) {
        let key = app.to_lowercase();
        let asked_at = self.asked().get(&key).copied();
        let windows: Vec<_> = processes
            .iter()
            .flat_map(|process| top_level_windows(process.pid))
            .collect();
        let overdue = asked_at.is_some_and(|at| at.elapsed() >= CLOSE_GRACE);
        if self.mode == Mode::DryRun {
            let how = if windows.is_empty() || overdue {
                "terminate"
            } else {
                "ask to close"
            };
            tracing::info!("dry run: would {how} {app} ({} processes)", processes.len());
            return;
        }
        if windows.is_empty() || overdue {
            for process in processes {
                if let Err(error) = terminate(process.pid) {
                    tracing::warn!("couldn't end {app} ({}): {error}", process.pid);
                }
            }
            self.asked().remove(&key);
        } else if asked_at.is_none() {
            ask_to_close(&windows);
            self.asked().insert(key, Instant::now());
        }
    }
}

impl Enforcer for WindowsEnforcer {
    fn close_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError> {
        if apps.is_empty() {
            return Ok(Vec::new());
        }
        let processes = own_matching(apps).map_err(failed)?;
        let mut closed: Vec<String> = Vec::new();
        for app in apps {
            let mine: Vec<&Process> = processes
                .iter()
                .filter(|process| same_app(app, &process.app))
                .collect();
            if mine.is_empty() || closed.iter().any(|done| same_app(done, app)) {
                continue;
            }
            self.close_app(app, &mine);
            closed.push(app.clone());
        }
        // Forget apps that are gone, so a relaunch is asked politely again.
        self.asked()
            .retain(|key, _| processes.iter().any(|process| same_app(key, &process.app)));
        Ok(closed)
    }
}

impl ProcessList for WindowsEnforcer {
    fn running_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError> {
        if apps.is_empty() {
            return Ok(Vec::new());
        }
        let processes = own_matching(apps).map_err(failed)?;
        let mut running: Vec<String> = Vec::new();
        for app in apps {
            let found = processes.iter().any(|process| same_app(app, &process.app));
            if found && !running.iter().any(|seen| same_app(seen, app)) {
                running.push(app.clone());
            }
        }
        Ok(running)
    }
}

fn failed(error: std::io::Error) -> EnforceError {
    EnforceError::Failed {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::{Child, Command};

    use focus_core::testing::{enforcer_contract, process_list_contract};

    use super::*;

    /// Runs a copy of `ping.exe` under a name nothing else uses, so tests close only their own
    /// process. Waits for it to show up.
    fn spawn_target(dir: &Path, name: &str) -> Child {
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let exe = dir.join(format!("{name}.exe"));
        std::fs::copy(Path::new(&system).join("System32").join("PING.EXE"), &exe).unwrap();
        let mut child = Command::new(&exe)
            .args(["-n", "60", "127.0.0.1"])
            .spawn()
            .unwrap();
        let enforcer = WindowsEnforcer::new(Mode::DryRun);
        for _ in 0..50 {
            if !enforcer
                .running_apps(&[name.to_owned()])
                .unwrap()
                .is_empty()
            {
                return child;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        panic!("{name} never appeared");
    }

    fn exited(child: &mut Child) -> bool {
        for _ in 0..50 {
            if child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    #[test]
    fn passes_the_contracts_live_and_dry() {
        for mode in [Mode::Live, Mode::DryRun] {
            enforcer_contract(&WindowsEnforcer::new(mode));
            process_list_contract(&WindowsEnforcer::new(mode));
        }
    }

    #[test]
    fn dry_run_reports_but_leaves_the_app_running() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = spawn_target(dir.path(), "focus-test-dry");
        let enforcer = WindowsEnforcer::new(Mode::DryRun);
        let closed = enforcer.close_apps(&["focus-test-dry.exe".into()]).unwrap();
        assert_eq!(closed, ["focus-test-dry.exe"]);
        assert!(child.try_wait().unwrap().is_none(), "dry run closed it");
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn closes_a_running_app_of_this_user() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = spawn_target(dir.path(), "focus-test-live");
        let enforcer = WindowsEnforcer::new(Mode::Live);
        assert_eq!(
            enforcer
                .running_apps(&["FOCUS-TEST-LIVE".into(), "focus-test-missing".into()])
                .unwrap(),
            ["FOCUS-TEST-LIVE"]
        );
        // A console program has no windows of its own, so it's terminated at once.
        let closed = enforcer.close_apps(&["focus-test-live".into()]).unwrap();
        assert_eq!(closed, ["focus-test-live"]);
        assert!(exited(&mut child), "still running");
        assert!(
            enforcer
                .running_apps(&["focus-test-live".into()])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn never_touches_protected_apps() {
        let enforcer = WindowsEnforcer::new(Mode::DryRun);
        assert!(
            enforcer
                .running_apps(&["explorer".into()])
                .unwrap()
                .is_empty()
        );
        assert!(
            enforcer
                .close_apps(&["explorer".into(), "focus-daemon".into()])
                .unwrap()
                .is_empty()
        );
    }
}
