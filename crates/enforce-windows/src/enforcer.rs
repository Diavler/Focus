//! The `Enforcer` and `ProcessList` ports for Windows.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use focus_core::ports::{EnforceError, Enforcer, ProcessList};
use focus_core::{app_key, same_app, select_apps};
use windows_sys::Win32::Foundation::HWND;

use crate::Mode;
use crate::browsers::known_browsers;
use crate::closing::{ask_to_close, terminate, windows_by_process};
use crate::processes::{Process, own_matching};

/// How long an app gets to close its windows before it's terminated.
const CLOSE_GRACE: Duration = Duration::from_secs(4);

/// Closes the current user's blocked apps.
///
/// An ordinary app is first asked to close (`WM_CLOSE` to its windows, as clicking ✕ would), so it
/// can save or ask about unsaved work; a later call, once [`CLOSE_GRACE`] has passed, terminates
/// whatever is left. An app with no windows is terminated straight away.
///
/// Browsers are terminated straight away too: closing their windows one by one makes them forget
/// all but the last window, while after termination they offer to restore every window.
#[derive(Debug)]
pub struct WindowsEnforcer {
    mode: Mode,
    /// When each app (by [`app_key`]) was first asked to close. Tracked in dry runs too, so the
    /// log shows the same steps a live run would take.
    asked: Mutex<HashMap<String, Instant>>,
}

/// What to do with one app this time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    AskToClose,
    Wait,
    Terminate,
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

    /// The next step for `app`, given its windows, and records a polite request.
    fn next_step(&self, app: &str, has_windows: bool) -> Step {
        let key = app_key(app);
        let is_browser = known_browsers()
            .iter()
            .any(|browser| same_app(&browser.app, app));
        let mut asked = self.asked();
        let step = match asked.get(&key) {
            _ if is_browser || !has_windows => Step::Terminate,
            Some(at) if at.elapsed() >= CLOSE_GRACE => Step::Terminate,
            Some(_) => Step::Wait,
            None => Step::AskToClose,
        };
        match step {
            Step::AskToClose => {
                asked.insert(key, Instant::now());
            }
            Step::Terminate => {
                asked.remove(&key);
            }
            Step::Wait => {}
        }
        step
    }

    /// Closes one app's processes, politely first unless it's a browser.
    fn close_app(&self, app: &str, processes: &[&Process], windows: &HashMap<u32, Vec<HWND>>) {
        let own_windows: Vec<HWND> = processes
            .iter()
            .filter_map(|process| windows.get(&process.pid))
            .flatten()
            .copied()
            .collect();
        let step = self.next_step(app, !own_windows.is_empty());
        if self.mode == Mode::DryRun {
            if step != Step::Wait {
                let how = if step == Step::Terminate {
                    "terminate"
                } else {
                    "ask to close"
                };
                tracing::info!("dry run: would {how} {app} ({} processes)", processes.len());
            }
            return;
        }
        match step {
            Step::AskToClose => ask_to_close(&own_windows),
            Step::Wait => {}
            Step::Terminate => {
                for process in processes {
                    if let Err(error) = terminate(process.pid) {
                        tracing::warn!("couldn't end {app} ({}): {error}", process.pid);
                    }
                }
            }
        }
    }
}

impl Enforcer for WindowsEnforcer {
    fn close_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError> {
        if apps.is_empty() {
            return Ok(Vec::new());
        }
        let processes = own_matching(apps).map_err(failed)?;
        let closing = select_apps(apps, |app| {
            processes.iter().any(|process| same_app(app, &process.app))
        });
        if !closing.is_empty() {
            let windows = windows_by_process();
            for app in &closing {
                let mine: Vec<&Process> = processes
                    .iter()
                    .filter(|process| same_app(app, &process.app))
                    .collect();
                self.close_app(app, &mine, &windows);
            }
        }
        // Forget apps that are gone, so a relaunch is asked politely again.
        self.asked()
            .retain(|key, _| processes.iter().any(|process| same_app(key, &process.app)));
        Ok(closing)
    }
}

impl ProcessList for WindowsEnforcer {
    fn running_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError> {
        if apps.is_empty() {
            return Ok(Vec::new());
        }
        let processes = own_matching(apps).map_err(failed)?;
        Ok(select_apps(apps, |app| {
            processes.iter().any(|process| same_app(app, &process.app))
        }))
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
    fn escalates_from_asking_to_terminating() {
        let enforcer = WindowsEnforcer::new(Mode::DryRun);
        assert_eq!(enforcer.next_step("notepad", true), Step::AskToClose);
        assert_eq!(
            enforcer.next_step("Notepad.exe", true),
            Step::Wait,
            "same app, any spelling"
        );
        enforcer
            .asked()
            .insert(app_key("notepad"), Instant::now() - CLOSE_GRACE);
        assert_eq!(enforcer.next_step("notepad", true), Step::Terminate);
        assert_eq!(
            enforcer.next_step("notepad", true),
            Step::AskToClose,
            "starts over"
        );
    }

    #[test]
    fn browsers_and_windowless_apps_are_ended_at_once() {
        let enforcer = WindowsEnforcer::new(Mode::DryRun);
        assert_eq!(enforcer.next_step("chrome", true), Step::Terminate);
        assert_eq!(enforcer.next_step("MSEDGE.exe", true), Step::Terminate);
        assert_eq!(enforcer.next_step("some-tool", false), Step::Terminate);
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
