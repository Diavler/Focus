//! In-memory fakes for every port. Interior mutability (`Mutex`) lets them take `&self` like the
//! real adapters do.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::config::BlockConfig;
use crate::enforcement::{same_app, select_apps};
use crate::login::Settings;
use crate::ports::{
    Autostart, AutostartError, Clock, ConfigSource, ConfigSourceError, EnforceError, Enforcer,
    ForegroundApp, History, HistoryError, HistoryEvent, Notifier, NotifyError, PersistedState,
    ProcessList, StateStore, StateStoreError, UiWatch, UiWatchError,
};
use crate::time::Timestamp;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Locks a fake's mutex; a panicking test can't leave a fake's plain data half-updated.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A clock that only moves when told to. Defaults to 2026-01-01T00:00:00Z.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<Timestamp>,
}

impl FakeClock {
    /// A clock stopped at `now`.
    pub fn new(now: Timestamp) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    /// Moves the clock forward.
    pub fn advance(&self, by: Duration) {
        let mut now = lock(&self.now);
        *now = now.saturating_add(by);
    }

    /// Sets the clock to `now`, which may be earlier than before.
    pub fn set(&self, now: Timestamp) {
        *lock(&self.now) = now;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new(Timestamp::from_millis(1_767_225_600_000))
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *lock(&self.now)
    }
}

/// A config source that returns a fixed result.
#[derive(Debug)]
pub struct FakeConfigSource {
    result: Result<Settings, ConfigSourceError>,
}

impl FakeConfigSource {
    /// A source that loads `configs`, with the other settings at their defaults.
    pub fn new(configs: Vec<BlockConfig>) -> Self {
        Self {
            result: Ok(Settings {
                configs,
                ..Settings::default()
            }),
        }
    }

    /// A source that always fails with `error`.
    pub fn failing(error: ConfigSourceError) -> Self {
        Self { result: Err(error) }
    }
}

impl ConfigSource for FakeConfigSource {
    fn load(&self) -> Result<Settings, ConfigSourceError> {
        self.result.clone()
    }
}

/// An autostart registration that only remembers the last request.
#[derive(Debug, Default)]
pub struct FakeAutostart {
    enabled: Mutex<Option<bool>>,
}

impl FakeAutostart {
    /// The last value set, or `None` if it was never set.
    pub fn enabled(&self) -> Option<bool> {
        *lock(&self.enabled)
    }
}

impl Autostart for FakeAutostart {
    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError> {
        *lock(&self.enabled) = Some(enabled);
        Ok(())
    }
}

/// A state store that keeps the state in memory.
#[derive(Debug, Default)]
pub struct MemoryStateStore {
    state: Mutex<Option<PersistedState>>,
}

impl StateStore for MemoryStateStore {
    fn load(&self) -> Result<Option<PersistedState>, StateStoreError> {
        Ok(lock(&self.state).clone())
    }

    fn save(&self, state: &PersistedState) -> Result<(), StateStoreError> {
        *lock(&self.state) = Some(state.clone());
        Ok(())
    }
}

/// An enforcer with a pretend process list; closing an app removes it from the list.
#[derive(Debug, Default)]
pub struct FakeEnforcer {
    running: Mutex<Vec<String>>,
}

impl FakeEnforcer {
    /// An enforcer that pretends `apps` are running.
    pub fn with_running<I, S>(apps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            running: Mutex::new(apps.into_iter().map(Into::into).collect()),
        }
    }

    /// The apps still "running".
    pub fn running(&self) -> Vec<String> {
        lock(&self.running).clone()
    }
}

impl Enforcer for FakeEnforcer {
    fn close_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError> {
        let mut running = lock(&self.running);
        let mut closed: Vec<String> = Vec::new();
        for app in apps {
            let before = running.len();
            running.retain(|name| !same_app(name, app));
            let already = closed.iter().any(|name| same_app(name, app));
            if running.len() < before && !already {
                closed.push(app.clone());
            }
        }
        Ok(closed)
    }
}

impl ProcessList for FakeEnforcer {
    fn running_apps(&self, apps: &[String]) -> Result<Vec<String>, EnforceError> {
        let running = lock(&self.running);
        Ok(select_apps(apps, |app| {
            running.iter().any(|name| same_app(name, app))
        }))
    }
}

/// A foreground-app source that reports whatever it was last told.
#[derive(Debug, Default)]
pub struct FakeForegroundApp {
    app: Mutex<Option<String>>,
}

impl FakeForegroundApp {
    /// Makes `app` the foreground app (`None`: no window in front).
    pub fn set(&self, app: Option<&str>) {
        *lock(&self.app) = app.map(str::to_owned);
    }
}

impl ForegroundApp for FakeForegroundApp {
    fn foreground_app(&self) -> Result<Option<String>, EnforceError> {
        Ok(lock(&self.app).clone())
    }
}

/// A notifier that remembers whether it is silenced.
#[derive(Debug, Default)]
pub struct FakeNotifier {
    silenced: Mutex<bool>,
}

impl FakeNotifier {
    /// Whether notifications are currently silenced.
    pub fn is_silenced(&self) -> bool {
        *lock(&self.silenced)
    }
}

impl Notifier for FakeNotifier {
    fn silence(&self) -> Result<(), NotifyError> {
        *lock(&self.silenced) = true;
        Ok(())
    }

    fn restore(&self) -> Result<(), NotifyError> {
        *lock(&self.silenced) = false;
        Ok(())
    }
}

/// A history that keeps every event in memory.
#[derive(Debug, Default)]
pub struct RecordingHistory {
    events: Mutex<Vec<HistoryEvent>>,
}

impl RecordingHistory {
    /// The events recorded so far, oldest first.
    pub fn events(&self) -> Vec<HistoryEvent> {
        lock(&self.events).clone()
    }
}

impl History for RecordingHistory {
    fn record(&self, event: &HistoryEvent) -> Result<(), HistoryError> {
        lock(&self.events).push(event.clone());
        Ok(())
    }
}

/// A UI that "runs" once launched, until killed.
#[derive(Debug, Default)]
pub struct FakeUi {
    running: AtomicBool,
    launches: AtomicU32,
}

impl FakeUi {
    /// A UI that is already running.
    pub fn running() -> Self {
        let ui = Self::default();
        ui.running.store(true, Ordering::SeqCst);
        ui
    }

    /// Stops the pretend UI, as if the user killed it.
    pub fn kill(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// How many times it was launched.
    pub fn launches(&self) -> u32 {
        self.launches.load(Ordering::SeqCst)
    }
}

impl UiWatch for FakeUi {
    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn launch(&self) -> Result<(), UiWatchError> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EndReason;

    #[test]
    fn fake_clock_moves_only_when_told() {
        let clock = FakeClock::default();
        let start = clock.now();
        clock.advance(Duration::from_secs(5));
        assert_eq!(
            clock.now().saturating_duration_since(start),
            Duration::from_secs(5)
        );
        clock.set(start);
        assert_eq!(clock.now(), start);
    }

    #[test]
    fn fake_enforcer_lists_running_apps_with_or_without_exe() {
        let enforcer = FakeEnforcer::with_running(["Discord.exe", "steam"]);
        let found = enforcer
            .running_apps(&["discord".into(), "STEAM.exe".into(), "slack".into()])
            .unwrap();
        assert_eq!(found, ["discord", "STEAM.exe"]);
    }

    #[test]
    fn fake_enforcer_closes_matching_apps_once() {
        let enforcer = FakeEnforcer::with_running(["Discord", "steam", "code"]);
        let closed = enforcer
            .close_apps(&["discord".into(), "DISCORD".into(), "slack".into()])
            .unwrap();
        assert_eq!(closed, ["discord"]);
        assert_eq!(enforcer.running(), ["steam", "code"]);
    }

    #[test]
    fn fake_notifier_and_history_record_calls() {
        let notifier = FakeNotifier::default();
        notifier.silence().unwrap();
        assert!(notifier.is_silenced());
        notifier.restore().unwrap();
        assert!(!notifier.is_silenced());

        let history = RecordingHistory::default();
        let event = HistoryEvent::SessionEnded {
            at: Timestamp::from_millis(1),
            reason: EndReason::Completed,
        };
        history.record(&event).unwrap();
        assert_eq!(history.events(), [event]);
    }
}
