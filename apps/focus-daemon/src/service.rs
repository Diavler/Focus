//! The daemon's state and how each request changes it. No sockets here, so it's tested directly.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use focus_core::ports::{
    Autostart, CalendarSource, Clock, ConfigSource, Enforcer, ForegroundApp, History, HistoryEvent,
    Notifier, PersistedState, ProcessList, SecretKey, Secrets, StateStore, StateStoreError,
    TaskSource, UiWatch,
};
use focus_core::{
    AutoStartRule, BrowserPolicy, BrowserWatch, Heartbeat, KnownBrowser, SessionError, SessionSpec,
    SessionState, Strictness, Timestamp, Verdict, apps_to_close, planned_starts, scheduled_configs,
};
use ipc_proto::{
    BrowserStatus, DaemonError, PendingSummary, PlanStatus, Request, Response, SourceState,
    StatusReport,
};
use thiserror::Error;

use crate::plan_sync::{PlanFetch, PlanFetched, SyncSchedule, merge, source_state, source_status};

/// The adapters the daemon runs on, chosen by the composition root (or a test).
#[derive(Clone)]
pub struct Ports {
    /// The current time.
    pub clock: Arc<dyn Clock>,
    /// The settings file.
    pub configs: Arc<dyn ConfigSource>,
    /// Where state is saved.
    pub store: Arc<dyn StateStore>,
    /// Where events are recorded.
    pub history: Arc<dyn History>,
    /// Closes blocked apps.
    pub enforcer: Arc<dyn Enforcer>,
    /// Tells which apps are running.
    pub processes: Arc<dyn ProcessList>,
    /// Tells which app is in front.
    pub foreground: Arc<dyn ForegroundApp>,
    /// Silences notifications during sessions.
    pub notifier: Arc<dyn Notifier>,
    /// The browsers this platform knows, closed during a session unless their extension checks in.
    pub browsers: Vec<KnownBrowser>,
    /// Registers Focus to start at login.
    pub autostart: Arc<dyn Autostart>,
    /// Keeps the UI running during sessions.
    pub ui: Arc<dyn UiWatch>,
    /// The calendar, for automatic starts.
    pub calendar: Arc<dyn CalendarSource>,
    /// The task list, for automatic starts.
    pub tasks: Arc<dyn TaskSource>,
    /// The OS keychain, holding the calendar address and task token.
    pub secrets: Arc<dyn Secrets>,
}

/// Why the daemon couldn't start.
#[derive(Debug, Error)]
pub enum StartupError {
    /// Saved state exists but can't be used (e.g. it's from a newer version).
    #[error("couldn't load saved state: {0}")]
    State(#[source] StateStoreError),
}

/// Owns the persisted state and answers requests.
///
/// Every change is made to a copy, saved, and only then kept, so a failed save never leaves the
/// daemon claiming something that isn't on disk.
pub struct Service {
    ports: Ports,
    state: PersistedState,
    settings_problem: Option<String>,
    browsers: BTreeMap<String, BrowserStatus>,
    cooling_off: Duration,
    watch: BrowserWatch,
    browser_policy: BrowserPolicy,
    /// When the UI was last launched, so a UI that won't start isn't relaunched every check.
    ui_launched_at: Option<Timestamp>,
    /// The start-at-login value last registered successfully; `None` until the first success,
    /// so each start registers once (fixing a moved install folder).
    autostart_applied: Option<bool>,
    /// Why registering start-at-login failed, shown in `Status`.
    autostart_problem: Option<String>,
    /// The `[[auto_start]]` rules from the last good settings.
    auto_rules: Vec<AutoStartRule>,
    /// When the plan is fetched next.
    sync: SyncSchedule,
    /// How the last calendar sync went.
    calendar_state: SourceState,
    /// How the last task sync went.
    tasks_state: SourceState,
}

/// The shortest time between two UI launches.
const UI_RELAUNCH_GAP: Duration = Duration::from_secs(15);

impl Service {
    /// Loads saved state (a running session resumes) and the settings file.
    ///
    /// Corrupt state that the store couldn't recover is replaced by a fresh state.
    pub fn open(ports: Ports, cooling_off: Duration) -> Result<Self, StartupError> {
        let state = match ports.store.load() {
            Ok(state) => state.unwrap_or_default(),
            Err(StateStoreError::Corrupt { reason }) => {
                tracing::warn!("saved state was corrupt, starting fresh: {reason}");
                PersistedState::default()
            }
            Err(error) => return Err(StartupError::State(error)),
        };
        let mut service = Self {
            ports,
            state,
            settings_problem: None,
            browsers: BTreeMap::new(),
            cooling_off,
            watch: BrowserWatch::default(),
            browser_policy: BrowserPolicy::default(),
            ui_launched_at: None,
            autostart_applied: None,
            autostart_problem: None,
            auto_rules: Vec::new(),
            sync: SyncSchedule::default(),
            calendar_state: SourceState::Unknown,
            tasks_state: SourceState::Unknown,
        };
        service.reload_settings();
        service.ensure_autostart();
        Ok(service)
    }

    /// The persisted state as it is now.
    pub fn state(&self) -> &PersistedState {
        &self.state
    }

    /// Answers one request. Expired sessions and due config changes are handled first.
    pub fn handle(&mut self, request: Request) -> Response {
        self.tick();
        let now = self.now();
        let result = match request {
            Request::Status => Ok(Response::Status(Box::new(self.status()))),
            Request::ListConfigs => {
                self.reload_settings();
                Ok(Response::Configs {
                    configs: self.state.configs.applied().to_vec(),
                })
            }
            Request::StartSession {
                config,
                strictness,
                duration_secs,
            } => self
                .start(&config, strictness, Duration::from_secs(duration_secs))
                .map(|()| Response::Done),
            Request::EndSession => self
                .update(|state| state.session.request_end_early(now).map_err(session_error))
                .map(|outcome| Response::EndSession { outcome }),
            Request::ConfirmEndSession { reason } => self
                .update(|state| {
                    state
                        .session
                        .confirm_end_early(&reason, now)
                        .map_err(session_error)
                })
                .map(|()| Response::Done),
            Request::CancelEndSession => self
                .update(|state| state.session.cancel_end_request().map_err(session_error))
                .map(|()| Response::Done),
            Request::Heartbeat {
                browser,
                incognito_allowed,
            } => {
                let status = BrowserStatus {
                    browser: browser.clone(),
                    last_seen: now,
                    incognito_allowed,
                };
                self.browsers.insert(browser, status);
                Ok(Response::Done)
            }
            Request::RulesFor { url, config } => self.rules_for(&url, config.as_deref()),
            Request::SetSecret { key, value } => self
                .set_secret(key, value.as_deref())
                .map(|()| Response::Done),
        };
        result.unwrap_or_else(|error| Response::Error { error })
    }

    /// Ends an expired session and applies due config changes, saving if anything changed.
    pub fn tick(&mut self) {
        let now = self.now();
        // Checked first so the common case (nothing due) doesn't copy the whole state.
        let expired = self
            .state
            .session
            .running()
            .is_some_and(|running| now >= running.ends_at());
        let due = self
            .state
            .configs
            .pending()
            .iter()
            .any(|change| change.apply_after <= now);
        let login_due = self
            .state
            .start_at_login
            .off_after()
            .is_some_and(|off_after| off_after <= now);
        if !expired && !due && !login_due {
            return;
        }
        let result = self.update(|state| {
            state.session.tick(now);
            state.configs.apply_due(now);
            state.start_at_login.apply_due(now);
            Ok(())
        });
        if let Err(error) = result {
            tracing::warn!("{error}");
        }
    }

    /// Re-reads the settings file and syncs it into the configs in effect and into start-at-login
    /// (with cooling-off for switching it off). Edits to the running session's config, and to
    /// configs the plan starts within the cooling-off delay, cool off. If the file can't be used,
    /// the settings in effect stay.
    pub fn reload_settings(&mut self) {
        let desired = match self.ports.configs.load() {
            Ok(desired) => desired,
            Err(error) => {
                let problem = error.to_string();
                if self.settings_problem.as_ref() != Some(&problem) {
                    tracing::warn!("keeping the current configs: {problem}");
                }
                self.settings_problem = Some(problem);
                return;
            }
        };
        self.settings_problem = None;
        let (now, cooling_off) = (self.now(), self.cooling_off);
        let planned = planned_starts(&desired.auto_start, &self.state.plan);
        let scheduled = scheduled_configs(&planned, now, cooling_off);
        let result = self.update(|state| {
            state.session.tick(now);
            let protected: Vec<&str> = state
                .session
                .running()
                .map(|running| running.spec().config.name.as_str())
                .into_iter()
                .chain(scheduled.iter().copied())
                .collect();
            let report = state
                .configs
                .sync(desired.configs, &protected, now, cooling_off);
            state
                .start_at_login
                .sync(desired.start_at_login, now, cooling_off);
            Ok(report)
        });
        self.auto_rules = desired.auto_start;
        match result {
            Ok(report) => {
                for name in &report.queued {
                    tracing::info!("edit to \"{name}\" is cooling off");
                }
            }
            Err(error) => tracing::warn!("{error}"),
        }
    }

    fn start(
        &mut self,
        name: &str,
        strictness: Strictness,
        duration: Duration,
    ) -> Result<(), DaemonError> {
        self.reload_settings();
        let config =
            self.state
                .configs
                .get(name)
                .cloned()
                .ok_or_else(|| DaemonError::UnknownConfig {
                    name: name.to_owned(),
                })?;
        let spec = SessionSpec {
            config,
            strictness,
            duration,
        };
        let now = self.now();
        self.update(|state| state.session.start(spec, now).map_err(session_error))
    }

    /// Stores or removes a credential, then fetches the plan again soon so the change shows.
    fn set_secret(&mut self, key: SecretKey, value: Option<&str>) -> Result<(), DaemonError> {
        let value = value.map(str::trim).filter(|value| !value.is_empty());
        self.ports
            .secrets
            .store(key, value)
            .map_err(|error| DaemonError::Credentials {
                message: error.to_string(),
            })?;
        self.sync.request();
        Ok(())
    }

    /// The plan fetch to run now, if one is due (at startup, every few minutes, and after a
    /// credential changes). Run it without holding the service, then pass the result to
    /// [`Service::store_plan`].
    pub fn plan_fetch_due(&mut self) -> Option<PlanFetch> {
        let now = self.now();
        self.sync.take_due(now).then(|| {
            PlanFetch::new(
                Arc::clone(&self.ports.calendar),
                Arc::clone(&self.ports.tasks),
                now,
            )
        })
    }

    /// Caches what a plan fetch found. A source that failed keeps its last data, so automatic
    /// starts work offline; one that isn't set up any more drops it.
    pub fn store_plan(&mut self, fetched: &PlanFetched) {
        let now = self.now();
        for (name, before, after) in [
            (
                "calendar",
                &self.calendar_state,
                source_state(&fetched.calendar),
            ),
            ("tasks", &self.tasks_state, source_state(&fetched.tasks)),
        ] {
            if let SourceState::Failing { problem, .. } = &after
                && before != &after
            {
                tracing::warn!("couldn't sync {name}: {problem}");
            }
        }
        self.calendar_state = source_state(&fetched.calendar);
        self.tasks_state = source_state(&fetched.tasks);
        let result = self.update(|state| {
            merge(&mut state.plan.calendar, &fetched.calendar, now);
            merge(&mut state.plan.tasks, &fetched.tasks, now);
            Ok(())
        });
        if let Err(error) = result {
            tracing::warn!("{error}");
        }
    }

    /// Starts the session the plan asks for now, unless one is already running (it then starts
    /// when that one ends, for whatever time is left). Each planned session starts at most once,
    /// so ending it early doesn't bring it back.
    pub fn auto_start(&mut self) {
        let now = self.now();
        let planned = planned_starts(&self.auto_rules, &self.state.plan);
        let mut log = self.state.auto_started.clone();
        let pruned = log.prune(now);
        let due = log.due(&planned, now).cloned();
        let running = self.state.session.running().is_some();
        let Some(due) = due.filter(|_| !running) else {
            let forget = |state: &mut PersistedState| {
                state.auto_started = log;
                Ok(())
            };
            if pruned && let Err(error) = self.update(forget) {
                tracing::warn!("{error}");
            }
            return;
        };
        let config = self.state.configs.get(&due.config).cloned();
        // Marked even if it can't start, so a broken rule isn't retried every second.
        let result = self.update(|state| {
            state.auto_started = log;
            state.auto_started.mark(&due);
            let Some(config) = config else {
                return Ok(Err(format!("config \"{}\" isn't in effect", due.config)));
            };
            let spec = SessionSpec {
                config,
                strictness: due.strictness,
                duration: due.duration_from(now),
            };
            Ok(state
                .session
                .start(spec, now)
                .map_err(|error| error.to_string()))
        });
        match result {
            Ok(Ok(())) => tracing::info!("started \"{}\" for \"{}\"", due.config, due.title),
            Ok(Err(problem)) => tracing::warn!("couldn't start \"{}\": {problem}", due.title),
            Err(error) => tracing::warn!("{error}"),
        }
    }

    fn rules_for(&self, url: &str, config: Option<&str>) -> Result<Response, DaemonError> {
        let config =
            match config {
                Some(name) => Some(self.state.configs.get(name).ok_or_else(|| {
                    DaemonError::UnknownConfig {
                        name: name.to_owned(),
                    }
                })?),
                None => self
                    .state
                    .session
                    .running()
                    .map(|running| &running.spec().config),
            };
        Ok(match config {
            Some(config) => Response::Verdict {
                verdict: config.verdict(url),
                config: Some(config.name.clone()),
            },
            None => Response::Verdict {
                verdict: Verdict::Allow,
                config: None,
            },
        })
    }

    /// During a session, closes the config's blocked apps and any browser whose extension isn't
    /// checking in (after a grace period). Outside a session, does nothing.
    pub fn enforce(&mut self) {
        let now = self.now();
        let Some(running) = self.state.session.running() else {
            self.watch.clear();
            return;
        };
        let blocked = running.spec().config.apps.clone();
        let browser_apps: Vec<String> = self.ports.browsers.iter().map(|b| b.app.clone()).collect();
        let heartbeats: Vec<Heartbeat> = self
            .browsers
            .values()
            .map(|status| Heartbeat {
                browser: status.browser.clone(),
                last_seen: status.last_seen,
                incognito_allowed: status.incognito_allowed,
            })
            .collect();
        // If listing fails, skip browsers this time rather than treating them all as closed,
        // which would reset their grace periods.
        let browsers = match self.ports.processes.running_apps(&browser_apps) {
            Ok(running_browsers) => self.watch.browsers_to_close(
                &running_browsers,
                &self.ports.browsers,
                &heartbeats,
                now,
                self.browser_policy,
            ),
            Err(error) => {
                tracing::warn!("couldn't list browsers: {error}");
                Vec::new()
            }
        };
        let close = apps_to_close(&blocked, browsers);
        if close.is_empty() {
            return;
        }
        match self.ports.enforcer.close_apps(&close) {
            Ok(closed) if !closed.is_empty() => tracing::info!("closing {closed:?}"),
            Ok(_) => {}
            Err(error) => tracing::warn!("couldn't close apps: {error}"),
        }
    }

    fn status(&self) -> StatusReport {
        StatusReport {
            now: self.now(),
            session: self.state.session.clone(),
            pending_changes: self
                .state
                .configs
                .pending()
                .iter()
                .map(|change| PendingSummary {
                    config: change.config.clone(),
                    apply_after: change.apply_after,
                    deleted: change.replacement.is_none(),
                })
                .collect(),
            settings_problem: self.settings_problem.clone(),
            browsers: self.browsers.values().cloned().collect(),
            foreground_app: self.ports.foreground.foreground_app().ok().flatten(),
            start_at_login: self.state.start_at_login.enabled(),
            start_at_login_off_after: self.state.start_at_login.off_after(),
            start_at_login_problem: self.autostart_problem.clone(),
            plan: self.plan_status(),
        }
    }

    fn plan_status(&self) -> PlanStatus {
        let now = self.now();
        let mut upcoming = planned_starts(&self.auto_rules, &self.state.plan);
        upcoming.retain(|start| start.ends_at > now);
        PlanStatus {
            calendar: source_status(&self.calendar_state, self.state.plan.calendar.as_ref()),
            tasks: source_status(&self.tasks_state, self.state.plan.tasks.as_ref()),
            upcoming,
        }
    }

    /// Applies `change` to a copy of the state; if it succeeds and changed something, saves the
    /// copy and keeps it. Otherwise the state is untouched.
    fn update<T>(
        &mut self,
        change: impl FnOnce(&mut PersistedState) -> Result<T, DaemonError>,
    ) -> Result<T, DaemonError> {
        let mut next = self.state.clone();
        let value = change(&mut next)?;
        if next != self.state {
            self.ports
                .store
                .save(&next)
                .map_err(|error| DaemonError::Storage {
                    message: error.to_string(),
                })?;
            let previous = std::mem::replace(&mut self.state, next);
            self.record_history(&previous.session);
            self.silence_while_running(&previous.session);
            if previous.start_at_login.enabled() != self.state.start_at_login.enabled() {
                self.ensure_autostart();
            }
        }
        Ok(value)
    }

    /// Records session starts and ends between `before` and the current state.
    fn record_history(&self, before: &SessionState) {
        let after = &self.state.session;
        let mut events = Vec::new();
        if let Some(running) = after.running() {
            let already_running = before
                .running()
                .is_some_and(|earlier| earlier.started_at() == running.started_at());
            if !already_running {
                events.push(HistoryEvent::SessionStarted {
                    at: running.started_at(),
                    config: running.spec().config.name.clone(),
                    strictness: running.spec().strictness,
                });
            }
        }
        if let (Some(ended), Some(_)) = (after.ended(), before.running()) {
            events.push(HistoryEvent::SessionEnded {
                at: ended.ended_at(),
                reason: ended.reason().clone(),
            });
        }
        for event in &events {
            if let Err(error) = self.ports.history.record(event) {
                tracing::warn!("couldn't record {event:?}: {error}");
            }
        }
    }

    /// Makes the OS's start-at-login registration match the setting in effect, unless it already
    /// does. A failure is kept for `Status` and retried on the next call (every few seconds).
    pub fn ensure_autostart(&mut self) {
        let enabled = self.state.start_at_login.enabled();
        if self.autostart_applied == Some(enabled) {
            return;
        }
        match self.ports.autostart.set_enabled(enabled) {
            Ok(()) => {
                self.autostart_applied = Some(enabled);
                self.autostart_problem = None;
            }
            Err(error) => {
                let problem = error.to_string();
                if self.autostart_problem.as_ref() != Some(&problem) {
                    tracing::warn!("{problem}");
                }
                self.autostart_problem = Some(problem);
            }
        }
    }

    /// During a session, starts the UI if it isn't running (at most once per
    /// [`UI_RELAUNCH_GAP`]). The UI watches the daemon in turn, so killing one brings it back.
    pub fn watch_ui(&mut self) {
        if self.state.session.running().is_some() {
            self.ensure_ui();
        }
    }

    /// Starts the UI if it isn't running (at most once per [`UI_RELAUNCH_GAP`]).
    pub fn ensure_ui(&mut self) {
        let now = self.now();
        // A launch "in the future" (the clock went back) counts as long ago, not as recent.
        let recently = self
            .ui_launched_at
            .is_some_and(|at| now >= at && now.saturating_duration_since(at) < UI_RELAUNCH_GAP);
        if recently || self.ports.ui.is_running() {
            return;
        }
        self.ui_launched_at = Some(now);
        match self.ports.ui.launch() {
            Ok(()) => tracing::info!("started the UI"),
            Err(error) => tracing::warn!("{error}"),
        }
    }

    /// Silences notifications when a session starts and restores them when it ends.
    fn silence_while_running(&self, before: &SessionState) {
        let (was, is) = (
            before.running().is_some(),
            self.state.session.running().is_some(),
        );
        let result = match (was, is) {
            (false, true) => self.ports.notifier.silence(),
            (true, false) => self.ports.notifier.restore(),
            _ => return,
        };
        if let Err(error) = result {
            tracing::warn!("{error}");
        }
    }

    fn now(&self) -> Timestamp {
        self.ports.clock.now()
    }
}

fn session_error(error: SessionError) -> DaemonError {
    DaemonError::Session { error }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};

    use config_toml::TomlConfigSource;
    use focus_core::ports::NoopHistory;
    use focus_core::ports::SourceError;
    use focus_core::testing::{
        FakeAutostart, FakeCalendar, FakeClock, FakeEnforcer, FakeForegroundApp, FakeNotifier,
        FakeTasks, FakeUi, MemorySecrets, MemoryStateStore, RecordingHistory, state_store_contract,
    };
    use focus_core::{CalendarEvent, EndEarlyResponse, EndReason, RuleAction, Task};
    use tempfile::TempDir;

    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);
    const SETTINGS: &str = r#"
        [configs.coding]
        feed_only = ["reddit.com"]
        allow = ["reddit.com/r/rust/comments/*"]
        apps = ["discord"]

        [configs.writing]
        mode = "allowlist"
        allow = ["docs.google.com"]
    "#;

    /// A service on fakes, plus handles to them so tests can move time and edit settings.
    struct Harness {
        _dir: TempDir,
        settings: PathBuf,
        clock: Arc<FakeClock>,
        store: Arc<MemoryStateStore>,
        history: Arc<RecordingHistory>,
        apps: Arc<FakeEnforcer>,
        notifier: Arc<FakeNotifier>,
        foreground: Arc<FakeForegroundApp>,
        autostart: Arc<FakeAutostart>,
        ui: Arc<FakeUi>,
        calendar: Arc<FakeCalendar>,
        tasks: Arc<FakeTasks>,
        secrets: Arc<MemorySecrets>,
    }

    impl Harness {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let settings = dir.path().join("settings.toml");
            std::fs::write(&settings, SETTINGS).unwrap();
            Self {
                _dir: dir,
                settings,
                clock: Arc::new(FakeClock::default()),
                store: Arc::new(MemoryStateStore::default()),
                history: Arc::new(RecordingHistory::default()),
                apps: Arc::new(FakeEnforcer::with_running([
                    "Discord.exe",
                    "chrome",
                    "notepad",
                ])),
                notifier: Arc::new(FakeNotifier::default()),
                foreground: Arc::new(FakeForegroundApp::default()),
                autostart: Arc::new(FakeAutostart::default()),
                ui: Arc::new(FakeUi::default()),
                calendar: Arc::new(FakeCalendar::default()),
                tasks: Arc::new(FakeTasks::default()),
                secrets: Arc::new(MemorySecrets::default()),
            }
        }

        fn ports(&self) -> Ports {
            Ports {
                clock: self.clock.clone(),
                configs: Arc::new(TomlConfigSource::new(&self.settings)),
                store: self.store.clone(),
                history: self.history.clone(),
                enforcer: self.apps.clone(),
                processes: self.apps.clone(),
                foreground: self.foreground.clone(),
                notifier: self.notifier.clone(),
                browsers: vec![KnownBrowser {
                    heartbeat_names: vec!["chrome".into()],
                    app: "chrome".into(),
                }],
                autostart: self.autostart.clone(),
                ui: self.ui.clone(),
                calendar: self.calendar.clone(),
                tasks: self.tasks.clone(),
                secrets: self.secrets.clone(),
            }
        }

        /// Opens a service, as the daemon does at startup.
        fn open(&self) -> Service {
            Service::open(self.ports(), Duration::from_secs(24 * 60 * 60)).unwrap()
        }

        fn write_settings(&self, text: &str) {
            write(&self.settings, text);
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    fn start(service: &mut Service, config: &str, strictness: Strictness) -> Response {
        service.handle(Request::StartSession {
            config: config.into(),
            strictness,
            duration_secs: 25 * 60,
        })
    }

    fn status(service: &mut Service) -> StatusReport {
        match service.handle(Request::Status) {
            Response::Status(status) => *status,
            other => panic!("expected status, got {other:?}"),
        }
    }

    fn error(response: Response) -> DaemonError {
        match response {
            Response::Error { error } => error,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn config_names(service: &mut Service) -> Vec<String> {
        match service.handle(Request::ListConfigs) {
            Response::Configs { configs } => configs.into_iter().map(|c| c.name).collect(),
            other => panic!("expected configs, got {other:?}"),
        }
    }

    #[test]
    fn starts_a_session_saves_it_and_records_history() {
        let harness = Harness::new();
        let mut service = harness.open();
        assert_eq!(config_names(&mut service), ["coding", "writing"]);
        assert_eq!(
            start(&mut service, "coding", Strictness::Deep),
            Response::Done
        );

        let running = status(&mut service).session;
        assert_eq!(
            running.running().unwrap().spec().config.name,
            "coding",
            "status shows the session"
        );
        let saved = harness.store.load().unwrap().unwrap();
        assert_eq!(saved.session, running, "the session was saved");
        assert!(matches!(
            harness.history.events()[..],
            [HistoryEvent::SessionStarted { .. }]
        ));
    }

    #[test]
    fn unknown_config_is_refused() {
        let harness = Harness::new();
        let mut service = harness.open();
        assert_eq!(
            error(start(&mut service, "gaming", Strictness::Light)),
            DaemonError::UnknownConfig {
                name: "gaming".into()
            }
        );
    }

    #[test]
    fn a_restarted_service_resumes_the_session() {
        let harness = Harness::new();
        let mut first = harness.open();
        start(&mut first, "coding", Strictness::Locked);
        drop(first);

        harness.clock.advance(10 * MINUTE);
        let mut second = harness.open();
        let status = status(&mut second);
        let running = status.session.running().expect("still running");
        assert_eq!(running.time_left(status.now), 15 * MINUTE);
        assert_eq!(
            error(second.handle(Request::EndSession)),
            DaemonError::Session {
                error: SessionError::Locked
            }
        );
    }

    #[test]
    fn deep_end_flow_over_requests() {
        let harness = Harness::new();
        let mut service = harness.open();
        start(&mut service, "coding", Strictness::Deep);
        let Response::EndSession {
            outcome: EndEarlyResponse::WaitUntil(_),
        } = service.handle(Request::EndSession)
        else {
            panic!("Deep sessions wait before ending");
        };
        let confirm = || Request::ConfirmEndSession {
            reason: "meeting".into(),
        };
        assert!(matches!(
            error(service.handle(confirm())),
            DaemonError::Session {
                error: SessionError::StillWaiting { .. }
            }
        ));
        harness.clock.advance(Duration::from_secs(30));
        assert_eq!(service.handle(confirm()), Response::Done);
        assert!(matches!(
            harness.history.events().last(),
            Some(HistoryEvent::SessionEnded {
                reason: EndReason::EndedEarly { .. },
                ..
            })
        ));
    }

    #[test]
    fn the_timer_completes_the_session() {
        let harness = Harness::new();
        let mut service = harness.open();
        start(&mut service, "coding", Strictness::Locked);
        harness.clock.advance(30 * MINUTE);
        service.tick();
        assert_eq!(
            service.state().session.ended().unwrap().reason(),
            &EndReason::Completed
        );
        assert_eq!(harness.store.load().unwrap().unwrap(), *service.state());
    }

    #[test]
    fn edits_to_the_running_config_cool_off() {
        let harness = Harness::new();
        let mut service = harness.open();
        start(&mut service, "coding", Strictness::Light);
        harness.write_settings(
            "[configs.coding]\n[configs.writing]\nallow = [\"x.com\"]\nmode = \"allowlist\"\n",
        );
        service.reload_settings();

        let status = status(&mut service);
        assert_eq!(status.pending_changes.len(), 1);
        assert_eq!(status.pending_changes[0].config, "coding");
        let coding = service.state().configs.get("coding").unwrap();
        assert_eq!(coding.apps, ["discord"], "the old version stays in effect");
        let writing = service.state().configs.get("writing").unwrap();
        assert_eq!(
            writing.verdict("x.com"),
            Verdict::Allow,
            "other configs change now"
        );

        service.handle(Request::EndSession);
        service.reload_settings();
        assert_eq!(
            service.state().configs.get("coding").unwrap().apps,
            ["discord"]
        );

        harness.clock.advance(Duration::from_secs(24 * 60 * 60));
        service.tick();
        assert!(
            service
                .state()
                .configs
                .get("coding")
                .unwrap()
                .apps
                .is_empty()
        );
    }

    #[test]
    fn broken_settings_keep_the_current_configs() {
        let harness = Harness::new();
        let mut service = harness.open();
        harness.write_settings("[configs.coding]\ninclude = [\"nope\"]\n");
        assert_eq!(config_names(&mut service), ["coding", "writing"]);
        let problem = status(&mut service).settings_problem.unwrap();
        assert!(problem.contains("nope"), "{problem}");

        harness.write_settings(SETTINGS);
        service.reload_settings();
        assert_eq!(status(&mut service).settings_problem, None);
    }

    #[test]
    fn rules_follow_the_running_session_or_the_named_config() {
        let harness = Harness::new();
        let mut service = harness.open();
        let rules = |service: &mut Service, config: Option<&str>| {
            service.handle(Request::RulesFor {
                url: "https://www.reddit.com/".into(),
                config: config.map(str::to_owned),
            })
        };
        let hidden = Verdict::Restrict(RuleAction::HideFeed);
        assert_eq!(
            rules(&mut service, None),
            Response::Verdict {
                verdict: Verdict::Allow,
                config: None
            },
            "no session, nothing restricted"
        );
        assert_eq!(
            rules(&mut service, Some("coding")),
            Response::Verdict {
                verdict: hidden,
                config: Some("coding".into())
            }
        );
        start(&mut service, "coding", Strictness::Light);
        assert_eq!(
            rules(&mut service, None),
            Response::Verdict {
                verdict: hidden,
                config: Some("coding".into())
            }
        );
        assert!(matches!(
            error(rules(&mut service, Some("gaming"))),
            DaemonError::UnknownConfig { .. }
        ));
    }

    #[test]
    fn heartbeats_show_in_status() {
        let harness = Harness::new();
        let mut service = harness.open();
        let beat = |browser: &str| Request::Heartbeat {
            browser: browser.into(),
            incognito_allowed: false,
        };
        assert_eq!(service.handle(beat("edge")), Response::Done);
        assert_eq!(service.handle(beat("chrome")), Response::Done);
        let browsers: Vec<_> = status(&mut service)
            .browsers
            .into_iter()
            .map(|b| b.browser)
            .collect();
        assert_eq!(browsers, ["chrome", "edge"]);
    }

    /// A memory store whose saves start failing when told to.
    #[derive(Default)]
    struct FlakyStore {
        inner: MemoryStateStore,
        failing: AtomicBool,
    }

    impl StateStore for FlakyStore {
        fn load(&self) -> Result<Option<PersistedState>, StateStoreError> {
            self.inner.load()
        }

        fn save(&self, state: &PersistedState) -> Result<(), StateStoreError> {
            if self.failing.load(Ordering::SeqCst) {
                return Err(StateStoreError::Unavailable {
                    reason: "disk full".into(),
                });
            }
            self.inner.save(state)
        }
    }

    #[test]
    fn flaky_store_passes_the_contract_while_healthy() {
        state_store_contract(FlakyStore::default);
    }

    #[test]
    fn nothing_changes_when_saving_fails() {
        let harness = Harness::new();
        let store = Arc::new(FlakyStore::default());
        let ports = Ports {
            store: store.clone(),
            history: Arc::new(NoopHistory),
            ..harness.ports()
        };
        let mut service = Service::open(ports, Duration::ZERO).unwrap();
        store.failing.store(true, Ordering::SeqCst);

        assert!(matches!(
            error(start(&mut service, "coding", Strictness::Light)),
            DaemonError::Storage { .. }
        ));
        assert_eq!(service.state().session, SessionState::Idle);
        assert_eq!(
            store.inner.load().unwrap().unwrap().session,
            SessionState::Idle
        );
    }

    /// A store whose saved state is corrupt beyond recovery. Like `NewerStore` below, it injects a
    /// failure on purpose, so it is exempt from the contract suite (see CLAUDE.md).
    struct CorruptStore;

    impl StateStore for CorruptStore {
        fn load(&self) -> Result<Option<PersistedState>, StateStoreError> {
            Err(StateStoreError::Corrupt {
                reason: "garbage".into(),
            })
        }

        fn save(&self, _state: &PersistedState) -> Result<(), StateStoreError> {
            Ok(())
        }
    }

    #[test]
    fn corrupt_state_starts_fresh_but_newer_state_refuses_to_start() {
        let harness = Harness::new();
        let ports = Ports {
            store: Arc::new(CorruptStore),
            ..harness.ports()
        };
        let service = Service::open(ports, Duration::ZERO).unwrap();
        assert_eq!(service.state().session, SessionState::Idle);

        struct NewerStore;
        impl StateStore for NewerStore {
            fn load(&self) -> Result<Option<PersistedState>, StateStoreError> {
                Err(StateStoreError::Unavailable {
                    reason: "version 2".into(),
                })
            }
            fn save(&self, _state: &PersistedState) -> Result<(), StateStoreError> {
                Ok(())
            }
        }
        let ports = Ports {
            store: Arc::new(NewerStore),
            ..harness.ports()
        };
        assert!(matches!(
            Service::open(ports, Duration::ZERO),
            Err(StartupError::State(_))
        ));
    }

    #[test]
    fn closes_blocked_apps_only_during_a_session() {
        let harness = Harness::new();
        let mut service = harness.open();
        service.enforce();
        assert_eq!(
            harness.apps.running().len(),
            3,
            "nothing closed without a session"
        );

        start(&mut service, "coding", Strictness::Light);
        service.enforce();
        assert!(
            !harness
                .apps
                .running()
                .iter()
                .any(|app| app == "Discord.exe")
        );
        assert!(harness.apps.running().iter().any(|app| app == "notepad"));
    }

    #[test]
    fn closes_a_browser_without_a_live_heartbeat_after_the_grace_period() {
        let harness = Harness::new();
        let mut service = harness.open();
        start(&mut service, "writing", Strictness::Light);
        service.enforce();
        assert!(
            harness.apps.running().iter().any(|app| app == "chrome"),
            "grace period"
        );

        harness.clock.advance(Duration::from_secs(30));
        service.handle(Request::Heartbeat {
            browser: "chrome".into(),
            incognito_allowed: true,
        });
        service.enforce();
        assert!(
            harness.apps.running().iter().any(|app| app == "chrome"),
            "checked in"
        );

        harness.clock.advance(Duration::from_secs(60));
        service.enforce();
        assert!(
            !harness.apps.running().iter().any(|app| app == "chrome"),
            "went quiet"
        );
    }

    /// A process list that always fails. It injects a failure on purpose, so it's exempt from the
    /// contract suite (see CLAUDE.md).
    struct BrokenProcessList;

    impl ProcessList for BrokenProcessList {
        fn running_apps(
            &self,
            _apps: &[String],
        ) -> Result<Vec<String>, focus_core::ports::EnforceError> {
            Err(focus_core::ports::EnforceError::Failed {
                reason: "snapshot failed".into(),
            })
        }
    }

    #[test]
    fn a_failed_listing_skips_browsers_but_still_closes_blocked_apps() {
        let harness = Harness::new();
        let ports = Ports {
            processes: Arc::new(BrokenProcessList),
            ..harness.ports()
        };
        let mut service = Service::open(ports, Duration::from_secs(60)).unwrap();
        start(&mut service, "coding", Strictness::Light);
        harness.clock.advance(Duration::from_secs(120));
        service.enforce();
        let running = harness.apps.running();
        assert!(
            running.iter().any(|app| app == "chrome"),
            "browsers skipped"
        );
        assert!(
            !running.iter().any(|app| app == "Discord.exe"),
            "blocked apps still closed"
        );
    }

    #[test]
    fn silences_notifications_while_a_session_runs() {
        let harness = Harness::new();
        let mut service = harness.open();
        start(&mut service, "coding", Strictness::Light);
        assert!(harness.notifier.is_silenced());
        service.handle(Request::EndSession);
        assert!(!harness.notifier.is_silenced());

        start(&mut service, "coding", Strictness::Locked);
        harness.clock.advance(30 * MINUTE);
        service.tick();
        assert!(
            !harness.notifier.is_silenced(),
            "restored when the timer ends it"
        );
    }

    #[test]
    fn status_reports_the_foreground_app() {
        let harness = Harness::new();
        let mut service = harness.open();
        assert_eq!(status(&mut service).foreground_app, None);
        harness.foreground.set(Some("code"));
        assert_eq!(status(&mut service).foreground_app.as_deref(), Some("code"));
    }

    #[test]
    fn registers_start_at_login_and_cools_off_switching_it_off() {
        let harness = Harness::new();
        let mut service = harness.open();
        assert_eq!(
            harness.autostart.enabled(),
            Some(true),
            "on by default, applied at start"
        );

        harness.write_settings(&format!("start_at_login = false\n{SETTINGS}"));
        service.reload_settings();
        let report = status(&mut service);
        assert!(report.start_at_login, "still on while cooling off");
        assert!(report.start_at_login_off_after.is_some());
        assert_eq!(harness.autostart.enabled(), Some(true));

        harness.clock.advance(Duration::from_secs(24 * 60 * 60));
        service.tick();
        assert_eq!(harness.autostart.enabled(), Some(false));
        assert!(!status(&mut service).start_at_login);

        harness.write_settings(SETTINGS);
        service.reload_settings();
        assert_eq!(
            harness.autostart.enabled(),
            Some(true),
            "switching on is immediate"
        );
    }

    #[test]
    fn keeps_the_ui_running_during_a_session() {
        let harness = Harness::new();
        let mut service = harness.open();
        service.watch_ui();
        assert_eq!(harness.ui.launches(), 0, "not outside a session");

        start(&mut service, "coding", Strictness::Locked);
        service.watch_ui();
        assert_eq!(harness.ui.launches(), 1);
        service.watch_ui();
        assert_eq!(harness.ui.launches(), 1, "already running");

        harness.ui.kill();
        service.watch_ui();
        assert_eq!(harness.ui.launches(), 1, "not again within the gap");
        harness.clock.advance(UI_RELAUNCH_GAP);
        service.watch_ui();
        assert_eq!(harness.ui.launches(), 2, "brought back after being killed");
    }

    #[test]
    fn a_clock_going_back_does_not_block_relaunching_the_ui() {
        let harness = Harness::new();
        let mut service = harness.open();
        start(&mut service, "coding", Strictness::Light);
        service.watch_ui();
        harness.ui.kill();
        harness.clock.set(Timestamp::from_millis(
            harness.clock.now().as_millis() - 60 * 60 * 1000,
        ));
        service.watch_ui();
        assert_eq!(harness.ui.launches(), 2);
    }

    /// An autostart that can't register. Injects a failure on purpose, so it's exempt from the
    /// contract suite (see CLAUDE.md).
    struct UnsupportedAutostart;

    impl Autostart for UnsupportedAutostart {
        fn set_enabled(&self, _enabled: bool) -> Result<(), focus_core::ports::AutostartError> {
            Err(focus_core::ports::AutostartError::Failed {
                reason: "not here".into(),
            })
        }
    }

    #[test]
    fn reports_and_retries_a_failed_registration() {
        let harness = Harness::new();
        let ports = Ports {
            autostart: Arc::new(UnsupportedAutostart),
            ..harness.ports()
        };
        let mut service = Service::open(ports, Duration::from_secs(60)).unwrap();
        let problem = status(&mut service).start_at_login_problem.unwrap();
        assert!(problem.contains("not here"), "{problem}");
    }

    /// Settings with a rule starting `coding` (Deep) from `#focus` events and `deep` tasks.
    const AUTO_SETTINGS: &str = r##"
        [[auto_start]]
        config = "coding"
        strictness = "deep"
        calendar_keywords = ["#focus"]
        task_labels = ["deep"]
    "##;

    fn auto_harness() -> Harness {
        let harness = Harness::new();
        harness.write_settings(&format!("{SETTINGS}{AUTO_SETTINGS}"));
        harness
    }

    fn focus_event(harness: &Harness, in_minutes: u32, minutes: u32) -> CalendarEvent {
        let start = harness.clock.now().saturating_add(MINUTE * in_minutes);
        CalendarEvent {
            id: format!("focus@{}", start.as_millis()),
            title: "Deep work #focus".into(),
            start,
            end: start.saturating_add(MINUTE * minutes),
        }
    }

    /// Runs one sync the way the server does: fetch without the lock, then store.
    fn sync(service: &mut Service) {
        let fetch = service.plan_fetch_due().expect("a fetch is due");
        service.store_plan(&fetch.run());
    }

    #[test]
    fn a_tagged_calendar_event_starts_its_config() {
        let harness = auto_harness();
        harness
            .calendar
            .set(Ok(vec![focus_event(&harness, 10, 60)]));
        let mut service = harness.open();
        sync(&mut service);
        let plan = status(&mut service).plan;
        assert_eq!(plan.calendar.state, SourceState::Ok);
        assert_eq!(plan.upcoming.len(), 1, "shows the planned session");

        service.auto_start();
        assert!(
            service.state().session.running().is_none(),
            "not before it starts"
        );

        harness.clock.advance(MINUTE * 15);
        service.auto_start();
        let running = service.state().session.running().expect("started");
        assert_eq!(running.spec().config.name, "coding");
        assert_eq!(running.spec().strictness, Strictness::Deep);
        assert_eq!(
            running.time_left(harness.clock.now()),
            MINUTE * 55,
            "until the event ends"
        );
        assert!(
            harness.notifier.is_silenced(),
            "starts like any other session"
        );
    }

    #[test]
    fn a_timed_task_with_a_label_starts_its_config() {
        let harness = auto_harness();
        harness.tasks.set(Ok(vec![Task {
            id: "1".into(),
            title: "Write the report".into(),
            labels: vec!["Deep".into()],
            due_at: Some(harness.clock.now()),
            duration: Some(MINUTE * 25),
        }]));
        let mut service = harness.open();
        sync(&mut service);
        service.auto_start();
        let running = service.state().session.running().expect("started");
        assert_eq!(running.time_left(harness.clock.now()), MINUTE * 25);
    }

    #[test]
    fn offline_starts_use_the_cached_plan_after_a_restart() {
        let harness = auto_harness();
        harness
            .calendar
            .set(Ok(vec![focus_event(&harness, 60, 30)]));
        let mut first = harness.open();
        sync(&mut first);
        drop(first);

        let offline = SourceError::Unavailable {
            reason: "no network".into(),
        };
        harness.calendar.set(Err(offline));
        harness.clock.advance(MINUTE * 61);
        let mut second = harness.open();
        sync(&mut second);
        let plan = status(&mut second).plan;
        assert!(
            matches!(
                plan.calendar.state,
                SourceState::Failing { offline: true, .. }
            ),
            "{:?}",
            plan.calendar.state
        );
        assert_eq!(plan.calendar.items, 1, "the cache is kept");
        second.auto_start();
        assert!(
            second.state().session.running().is_some(),
            "started from the cache"
        );
    }

    #[test]
    fn ending_an_automatic_session_early_doesnt_restart_it() {
        let harness = auto_harness();
        let light = AUTO_SETTINGS.replace(r#"strictness = "deep""#, r#"strictness = "light""#);
        harness.write_settings(&format!("{SETTINGS}{light}"));
        harness.calendar.set(Ok(vec![focus_event(&harness, 0, 60)]));
        let mut service = harness.open();
        sync(&mut service);
        service.auto_start();
        assert_eq!(
            service.state().session.running().unwrap().spec().strictness,
            Strictness::Light
        );
        service.handle(Request::EndSession);
        service.auto_start();
        assert!(service.state().session.running().is_none(), "stays ended");

        harness.clock.advance(MINUTE * 61);
        service.auto_start();
        assert_eq!(
            service.state().auto_started,
            focus_core::AutoStartLog::default(),
            "forgotten once the event is over"
        );
    }

    #[test]
    fn an_event_during_a_manual_session_starts_when_it_ends() {
        let harness = auto_harness();
        harness.calendar.set(Ok(vec![focus_event(&harness, 0, 60)]));
        let mut service = harness.open();
        sync(&mut service);
        start(&mut service, "writing", Strictness::Light);
        service.auto_start();
        assert_eq!(
            service
                .state()
                .session
                .running()
                .unwrap()
                .spec()
                .config
                .name,
            "writing"
        );

        service.handle(Request::EndSession);
        service.auto_start();
        let running = service
            .state()
            .session
            .running()
            .expect("the event's session");
        assert_eq!(running.spec().config.name, "coding");
    }

    #[test]
    fn removing_a_credential_drops_the_cached_plan() {
        let harness = auto_harness();
        harness
            .calendar
            .set(Ok(vec![focus_event(&harness, 60, 30)]));
        let mut service = harness.open();
        sync(&mut service);
        assert!(service.plan_fetch_due().is_none(), "not again right away");

        assert_eq!(
            service.handle(Request::SetSecret {
                key: SecretKey::CalendarUrl,
                value: Some(
                    "  https://example.com/basic.ics 
"
                    .into()
                ),
            }),
            Response::Done
        );
        assert_eq!(
            harness.secrets.load(SecretKey::CalendarUrl),
            Ok(Some("https://example.com/basic.ics".into())),
            "stored trimmed"
        );
        service.handle(Request::SetSecret {
            key: SecretKey::CalendarUrl,
            value: Some("   ".into()),
        });
        assert_eq!(
            harness.secrets.load(SecretKey::CalendarUrl),
            Ok(None),
            "blank removes"
        );

        harness.calendar.set(Err(SourceError::NotConfigured));
        sync(&mut service);
        let plan = status(&mut service).plan;
        assert_eq!(plan.calendar.state, SourceState::NotConfigured);
        assert_eq!((plan.calendar.items, plan.upcoming.len()), (0, 0));
    }

    #[test]
    fn edits_to_a_config_with_a_planned_session_cool_off() {
        let harness = auto_harness();
        harness
            .calendar
            .set(Ok(vec![focus_event(&harness, 60 * 3, 30)]));
        let mut service = harness.open();
        sync(&mut service);
        let loosened = format!(
            "{}{AUTO_SETTINGS}",
            SETTINGS.replace(r#"apps = ["discord"]"#, "")
        );
        harness.write_settings(&loosened);
        service.reload_settings();
        let pending = status(&mut service).pending_changes;
        assert_eq!(pending.len(), 1, "queued because the event is within 24 h");
        assert_eq!(pending[0].config, "coding");
    }

    #[test]
    fn registers_once_per_start() {
        let harness = Harness::new();
        let mut service = harness.open();
        assert_eq!(harness.autostart.enabled(), Some(true));
        harness.autostart.set_enabled(false).unwrap();
        service.ensure_autostart();
        assert_eq!(
            harness.autostart.enabled(),
            Some(false),
            "not rewritten while unchanged"
        );
        let _restarted = harness.open();
        assert_eq!(
            harness.autostart.enabled(),
            Some(true),
            "rewritten on the next start"
        );
    }
}
