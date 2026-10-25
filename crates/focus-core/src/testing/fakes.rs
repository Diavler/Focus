//! In-memory fakes for every port. Interior mutability (`Mutex`) lets them take `&self` like the
//! real adapters do.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::config::BlockConfig;
use crate::config::Mode;
use crate::editable::{ACTIONS, EditableSettings};
use crate::enforcement::{same_app, select_apps};
use crate::hotkey::Hotkey;
use crate::login::Settings;
use crate::parking::ParkedItem;
use crate::pattern::UrlPattern;
use crate::plan::{CalendarEvent, Task};
use crate::ports::{
    Autostart, AutostartError, CalendarSource, Clock, ConfigSource, ConfigSourceError,
    EnforceError, Enforcer, ForegroundApp, GlobalHotkey, History, HistoryError, HistoryEvent,
    HotkeyError, HotkeyGuard, InboxError, NotificationSettings, Notifier, NotifyError,
    PersistedState, ProcessList, SecretKey, Secrets, SecretsError, SettingsEditor, Silencing,
    SourceError, StateStore, StateStoreError, TaskInbox, TaskSource, UiWatch, UiWatchError,
    WindowedApps,
};
use crate::rule::Verdict;
use crate::time::Timestamp;
use std::collections::HashMap;
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

/// A config source that returns a fixed result, and edits it when asked.
#[derive(Debug)]
pub struct FakeConfigSource {
    result: Mutex<Result<Settings, ConfigSourceError>>,
}

impl FakeConfigSource {
    /// A source that loads `configs`, with the other settings at their defaults.
    pub fn new(configs: Vec<BlockConfig>) -> Self {
        Self {
            result: Mutex::new(Ok(Settings {
                configs,
                ..Settings::default()
            })),
        }
    }

    /// A source that always fails with `error`.
    pub fn failing(error: ConfigSourceError) -> Self {
        Self {
            result: Mutex::new(Err(error)),
        }
    }
}

impl ConfigSource for FakeConfigSource {
    fn load(&self) -> Result<Settings, ConfigSourceError> {
        lock(&self.result).clone()
    }
}

/// Keeps editable settings in memory and loads them as [`Settings`], checking edits the way the
/// settings file is checked (unknown configs and lists, empty allowlists, bad patterns), and
/// refusing a save from an old revision.
#[derive(Debug)]
pub struct FakeSettingsFile {
    settings: Mutex<EditableSettings>,
    revision: AtomicU32,
}

impl FakeSettingsFile {
    /// A settings file with these contents.
    pub fn new(mut settings: EditableSettings) -> Self {
        settings.revision = "0".into();
        Self {
            settings: Mutex::new(settings),
            revision: AtomicU32::new(0),
        }
    }

    fn problem(settings: &EditableSettings) -> Option<String> {
        if let Some((key, accepts)) = settings.friction.problems().into_iter().next() {
            return Some(format!("friction.{key}: use {accepts}"));
        }
        for config in &settings.configs {
            if let Some(missing) = config
                .include
                .iter()
                .find(|name| !settings.lists.iter().any(|list| &list.name == *name))
            {
                return Some(format!("no list named \"{missing}\""));
            }
            let resolved = settings.resolve(&config.name)?;
            let allows = resolved
                .rules
                .iter()
                .any(|rule| rule.verdict == Verdict::Allow);
            if config.mode == Mode::Allowlist && !allows {
                return Some(format!("{} allows nothing", config.name));
            }
        }
        let patterns = settings
            .configs
            .iter()
            .map(|config| &config.patterns)
            .chain(settings.lists.iter().map(|list| &list.patterns));
        for patterns in patterns {
            for verdict in ACTIONS {
                if let Some(bad) = patterns
                    .get(verdict)
                    .iter()
                    .find(|pattern| UrlPattern::parse(pattern).is_err())
                {
                    return Some(format!("bad pattern \"{bad}\""));
                }
            }
        }
        settings
            .auto_start
            .iter()
            .find(|rule| {
                !settings
                    .configs
                    .iter()
                    .any(|config| config.name == rule.config)
            })
            .map(|rule| format!("no configuration named \"{}\"", rule.config))
    }
}

impl ConfigSource for FakeSettingsFile {
    fn load(&self) -> Result<Settings, ConfigSourceError> {
        let settings = lock(&self.settings).clone();
        Ok(Settings {
            configs: settings
                .configs
                .iter()
                .filter_map(|config| settings.resolve(&config.name))
                .collect(),
            start_at_login: settings.start_at_login,
            auto_start: settings.auto_start,
            friction: settings.friction,
            parking_hotkey: settings.parking_hotkey,
        })
    }
}

impl SettingsEditor for FakeSettingsFile {
    fn load_editable(&self) -> Result<EditableSettings, ConfigSourceError> {
        Ok(lock(&self.settings).clone())
    }

    fn save_editable(&self, edited: &EditableSettings) -> Result<(), ConfigSourceError> {
        let mut settings = lock(&self.settings);
        if edited.revision != settings.revision {
            return Err(ConfigSourceError::Changed);
        }
        if let Some(reason) = Self::problem(edited) {
            return Err(ConfigSourceError::Invalid { reason });
        }
        let revision = self.revision.fetch_add(1, Ordering::Relaxed) + 1;
        *settings = edited.clone();
        settings.revision = revision.to_string();
        Ok(())
    }
}

/// Windowed apps that are whatever the test says.
#[derive(Debug, Default)]
pub struct FakeWindowedApps {
    apps: Mutex<Vec<String>>,
}

impl FakeWindowedApps {
    /// Pretends these apps have windows open.
    pub fn with<I, S>(apps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut apps: Vec<String> = apps.into_iter().map(Into::into).collect();
        apps.sort_by_key(|app| app.to_lowercase());
        apps.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        Self {
            apps: Mutex::new(apps),
        }
    }
}

impl WindowedApps for FakeWindowedApps {
    fn windowed_apps(&self) -> Result<Vec<String>, EnforceError> {
        Ok(lock(&self.apps).clone())
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

/// A calendar holding whatever events (or error) it was last given.
#[derive(Debug)]
pub struct FakeCalendar {
    result: Mutex<Result<Vec<CalendarEvent>, SourceError>>,
}

impl FakeCalendar {
    /// A calendar with these events.
    pub fn new(events: Vec<CalendarEvent>) -> Self {
        Self {
            result: Mutex::new(Ok(events)),
        }
    }

    /// Replaces the events, or makes every read fail.
    pub fn set(&self, result: Result<Vec<CalendarEvent>, SourceError>) {
        *lock(&self.result) = result;
    }
}

impl Default for FakeCalendar {
    /// A calendar that isn't set up.
    fn default() -> Self {
        Self {
            result: Mutex::new(Err(SourceError::NotConfigured)),
        }
    }
}

impl CalendarSource for FakeCalendar {
    fn events(&self, from: Timestamp, to: Timestamp) -> Result<Vec<CalendarEvent>, SourceError> {
        let mut events: Vec<CalendarEvent> = lock(&self.result)
            .clone()?
            .into_iter()
            .filter(|event| event.overlaps(from, to))
            .collect();
        events.sort_by_key(|event| event.start);
        Ok(events)
    }
}

/// A task list holding whatever tasks (or error) it was last given.
#[derive(Debug)]
pub struct FakeTasks {
    result: Mutex<Result<Vec<Task>, SourceError>>,
}

impl FakeTasks {
    /// A task list with these tasks.
    pub fn new(tasks: Vec<Task>) -> Self {
        Self {
            result: Mutex::new(Ok(tasks)),
        }
    }

    /// Replaces the tasks, or makes every read fail.
    pub fn set(&self, result: Result<Vec<Task>, SourceError>) {
        *lock(&self.result) = result;
    }
}

impl Default for FakeTasks {
    /// A task list that isn't set up.
    fn default() -> Self {
        Self {
            result: Mutex::new(Err(SourceError::NotConfigured)),
        }
    }
}

impl TaskSource for FakeTasks {
    fn tasks(&self) -> Result<Vec<Task>, SourceError> {
        lock(&self.result).clone()
    }
}

/// A keychain in memory.
#[derive(Debug, Default)]
pub struct MemorySecrets {
    values: Mutex<HashMap<SecretKey, String>>,
}

impl Secrets for MemorySecrets {
    fn load(&self, key: SecretKey) -> Result<Option<String>, SecretsError> {
        Ok(lock(&self.values).get(&key).cloned())
    }

    fn store(&self, key: SecretKey, value: Option<&str>) -> Result<(), SecretsError> {
        let mut values = lock(&self.values);
        match value {
            Some(value) => values.insert(key, value.to_owned()),
            None => values.remove(&key),
        };
        Ok(())
    }
}

/// A notifier that remembers whether it is silenced. By default it silences by itself; one made
/// with [`FakeNotifier::asking`] answers like Windows, where the user has to.
#[derive(Debug)]
pub struct FakeNotifier {
    silenced: Mutex<bool>,
    answer: Silencing,
}

impl Default for FakeNotifier {
    fn default() -> Self {
        Self {
            silenced: Mutex::new(false),
            answer: Silencing::Silenced,
        }
    }
}

impl FakeNotifier {
    /// A notifier that asks the user to silence notifications, like Windows.
    pub fn asking() -> Self {
        Self {
            silenced: Mutex::new(false),
            answer: Silencing::AskUser,
        }
    }

    /// Whether `silence` was called more recently than `restore`.
    pub fn is_silenced(&self) -> bool {
        *lock(&self.silenced)
    }
}

impl Notifier for FakeNotifier {
    fn silence(&self) -> Result<Silencing, NotifyError> {
        *lock(&self.silenced) = true;
        Ok(self.answer)
    }

    fn restore(&self) -> Result<(), NotifyError> {
        *lock(&self.silenced) = false;
        Ok(())
    }
}

/// Notification settings that count how often they were opened.
#[derive(Debug, Default)]
pub struct FakeNotificationSettings {
    opened: AtomicU32,
}

impl FakeNotificationSettings {
    /// How often the settings were opened.
    pub fn opened(&self) -> u32 {
        self.opened.load(Ordering::SeqCst)
    }
}

impl NotificationSettings for FakeNotificationSettings {
    fn open(&self) -> Result<(), NotifyError> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// A task inbox that keeps what it was sent, once per request id, or fails with a set error. Items
/// whose text is set with [`FakeInbox::refuse`] are refused.
#[derive(Debug, Default)]
pub struct FakeInbox {
    added: Mutex<Vec<ParkedItem>>,
    failure: Mutex<Option<SourceError>>,
    refused_text: Mutex<Option<String>>,
}

impl FakeInbox {
    /// What reached the inbox, in order.
    pub fn added(&self) -> Vec<ParkedItem> {
        lock(&self.added).clone()
    }

    /// Makes every later `add` fail with `failure` (or work again with `None`).
    pub fn fail_with(&self, failure: Option<SourceError>) {
        *lock(&self.failure) = failure;
    }

    /// Refuses every later item whose text is `text`.
    pub fn refuse(&self, text: &str) {
        *lock(&self.refused_text) = Some(text.to_owned());
    }
}

impl TaskInbox for FakeInbox {
    fn add(&self, item: &ParkedItem) -> Result<(), InboxError> {
        if let Some(failure) = lock(&self.failure).clone() {
            return Err(failure.into());
        }
        if lock(&self.refused_text).as_deref() == Some(item.text.as_str()) {
            return Err(InboxError::Refused {
                reason: "the fake refuses it".into(),
            });
        }
        let mut added = lock(&self.added);
        if !added
            .iter()
            .any(|seen| seen.request_id() == item.request_id())
        {
            added.push(item.clone());
        }
        Ok(())
    }
}

/// The callback a shortcut calls when pressed.
type Pressed = Box<dyn Fn() + Send + Sync>;

/// A shortcut registry for one shortcut at a time, pressed by hand with [`FakeHotkey::press`].
#[derive(Default)]
pub struct FakeHotkey {
    held: Arc<Mutex<Option<(Hotkey, Pressed)>>>,
}

impl FakeHotkey {
    /// The shortcut registered now, if any.
    pub fn registered(&self) -> Option<Hotkey> {
        lock(&self.held).as_ref().map(|(hotkey, _)| *hotkey)
    }

    /// Presses the registered shortcut; false if none is registered.
    pub fn press(&self) -> bool {
        match lock(&self.held).as_ref() {
            Some((_, pressed)) => {
                pressed();
                true
            }
            None => false,
        }
    }
}

impl std::fmt::Debug for FakeHotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeHotkey")
            .field("registered", &self.registered())
            .finish()
    }
}

/// Unregisters the fake's shortcut when dropped.
struct FakeRegistration(Arc<Mutex<Option<(Hotkey, Pressed)>>>);

impl Drop for FakeRegistration {
    fn drop(&mut self) {
        *lock(&self.0) = None;
    }
}

impl GlobalHotkey for FakeHotkey {
    fn listen(&self, hotkey: Hotkey, pressed: Pressed) -> Result<HotkeyGuard, HotkeyError> {
        if hotkey == Hotkey::Off {
            return Ok(HotkeyGuard::none());
        }
        let mut held = lock(&self.held);
        if held.is_some() {
            return Err(HotkeyError::Taken { hotkey });
        }
        *held = Some((hotkey, pressed));
        Ok(HotkeyGuard::new(FakeRegistration(Arc::clone(&self.held))))
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
