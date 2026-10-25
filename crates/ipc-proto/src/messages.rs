//! The requests clients send and the responses the daemon returns.

use focus_core::ports::{SecretKey, Silencing};
use focus_core::{
    BlockConfig, CalendarEvent, EditableSettings, EndEarlyResponse, Friction, Hotkey, ParkError,
    ParkedItem, PlannedStart, SessionError, SessionState, StopError, Strictness, Task, Timestamp,
    UnlockError, Verdict,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Something a client asks the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// The session, pending config changes and connected browsers. Answered with [`Response::Status`].
    Status,
    /// The configurations in effect. Answered with [`Response::Configs`].
    ListConfigs,
    /// Starts a session. Answered with [`Response::Done`].
    StartSession {
        /// Name of the configuration to run.
        config: String,
        /// How hard the session is to end early.
        strictness: Strictness,
        /// Length of the session in seconds.
        duration_secs: u64,
    },
    /// Asks to end the running session early. Answered with [`Response::EndSession`].
    EndSession,
    /// Ends a Deep session after its wait, with the typed reason. Answered with [`Response::Done`].
    ConfirmEndSession {
        /// Why the session is ending early.
        reason: String,
    },
    /// Withdraws a pending end request. Answered with [`Response::Done`].
    CancelEndSession,
    /// The browser extension is alive. Answered with [`Response::Done`].
    Heartbeat {
        /// Which browser, e.g. `chrome` or `edge`.
        browser: String,
        /// Whether the extension may run in private windows.
        incognito_allowed: bool,
    },
    /// The verdict for a URL. Answered with [`Response::Verdict`].
    RulesFor {
        /// The page's URL.
        url: String,
        /// Check against this configuration instead of the running session's.
        config: Option<String>,
    },
    /// Opens a blocked page for a while: free in a Light session, one unlock from the budget
    /// (with a reason) in a Deep one, refused in a Locked one. Answered with
    /// [`Response::Unlocked`].
    Unlock {
        /// The blocked page.
        url: String,
        /// Why; required in a Deep session.
        reason: String,
    },
    /// Counts a visit to a page behind a Delay rule. Answered with [`Response::Delay`], the wait,
    /// which grows with each visit to the same site during the session.
    DelayVisit {
        /// The page.
        url: String,
    },
    /// Parks a thought or a link (Later), to be sent to the task inbox when it's reachable.
    /// Answered with [`Response::Done`].
    Park {
        /// What to remember; may be blank when there's a link.
        text: String,
        /// The page, for Later.
        url: Option<String>,
    },
    /// Everything parked. Answered with [`Response::ParkingLot`]. Not relayed from the browser.
    ParkingLot,
    /// Removes one parked item. Answered with [`Response::Done`]. Not relayed from the browser.
    Unpark {
        /// Which item.
        id: u64,
    },
    /// Removes every parked item that reached the task inbox. Answered with [`Response::Done`].
    /// Not relayed from the browser.
    ClearSentParked,
    /// Opens the system's notification settings, for turning on Do Not Disturb where Focus can't.
    /// Answered with [`Response::Done`]. Not relayed from the browser.
    OpenNotificationSettings,
    /// Stores or (with `None` or a blank value) removes a credential in the OS keychain, then
    /// syncs the plan. Answered with [`Response::Done`]. Not relayed from the browser.
    SetSecret {
        /// Which credential.
        key: SecretKey,
        /// The new value.
        value: Option<String>,
    },
    /// The cached calendar events and tasks, the automatic-start rules and what they'll start.
    /// Answered with [`Response::Plan`]. Not relayed from the browser.
    Plan,
    /// The settings as written, for the window's settings editor. Answered with
    /// [`Response::Settings`]. Not relayed from the browser.
    Settings,
    /// Saves the edited settings into the settings file (keeping comments). Refused with
    /// [`DaemonError::SettingsChanged`] if the file changed since they were read. Answered with
    /// [`Response::Done`]. Not relayed from the browser.
    SaveSettings {
        /// The edited settings, with the revision they were read at.
        settings: EditableSettings,
    },
    /// The apps the user has a window open in, for picking apps to close. Answered with
    /// [`Response::Apps`]. Not relayed from the browser.
    WindowedApps,
    /// Asks to stop Focus (the kill switch). Answered with [`Response::Stop`]. Not relayed from the
    /// browser.
    RequestStop,
    /// Stops Focus once the wait is over: ends any session, then the daemon exits. Answered with
    /// [`Response::Done`] just before it does. Not relayed from the browser.
    ConfirmStop {
        /// Why; required during a Locked session.
        reason: String,
    },
    /// Withdraws a request to stop Focus. Answered with [`Response::Done`].
    CancelStop,
}

/// The daemon's answer to one [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Answer to [`Request::Status`]. Boxed because it's much larger than the other answers; the
    /// JSON is the same.
    Status(Box<StatusReport>),
    /// Answer to [`Request::ListConfigs`].
    Configs {
        /// The configurations in effect, sorted by name.
        configs: Vec<BlockConfig>,
    },
    /// Answer to [`Request::EndSession`].
    EndSession {
        /// Whether the session ended or must wait first.
        outcome: EndEarlyResponse,
    },
    /// Answer to [`Request::RulesFor`].
    Verdict {
        /// What happens to the page.
        verdict: Verdict,
        /// The configuration that decided, or `None` if no session is running (everything is allowed).
        config: Option<String>,
    },
    /// Answer to [`Request::Plan`]. Boxed because it's much larger than the other answers.
    Plan(Box<PlanView>),
    /// Answer to [`Request::Settings`]. Boxed like [`Response::Plan`].
    Settings(Box<EditableSettings>),
    /// Answer to [`Request::WindowedApps`].
    Apps {
        /// App names, sorted.
        apps: Vec<String>,
    },
    /// Answer to [`Request::ParkingLot`].
    ParkingLot {
        /// Every parked item, oldest first.
        items: Vec<ParkedItem>,
    },
    /// Answer to [`Request::Unlock`].
    Unlocked {
        /// When the unlock ends.
        until: Timestamp,
    },
    /// Answer to [`Request::DelayVisit`].
    Delay {
        /// How long to wait before continuing.
        seconds: u64,
    },
    /// Answer to [`Request::RequestStop`].
    Stop {
        /// When the stop may be confirmed.
        allowed_at: Timestamp,
        /// Whether confirming needs a typed reason.
        reason_required: bool,
    },
    /// The request succeeded and has nothing else to report.
    Done,
    /// The request failed; nothing changed.
    Error {
        /// What went wrong.
        error: DaemonError,
    },
}

/// What the daemon is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    /// The daemon's current time; use it with the session's timestamps.
    pub now: Timestamp,
    /// The session lifecycle.
    pub session: SessionState,
    /// Config edits still cooling off.
    pub pending_changes: Vec<PendingSummary>,
    /// Why the settings file couldn't be loaded, if it couldn't. The last good configs stay in effect.
    pub settings_problem: Option<String>,
    /// Browsers whose extension has sent a heartbeat, sorted by name.
    pub browsers: Vec<BrowserStatus>,
    /// The app owning the window in front, if known.
    #[serde(default)]
    pub foreground_app: Option<String>,
    /// Whether Focus starts at login.
    #[serde(default)]
    pub start_at_login: bool,
    /// When a requested switch-off of start-at-login takes effect, if one is cooling off.
    #[serde(default)]
    pub start_at_login_off_after: Option<Timestamp>,
    /// Why start-at-login couldn't be registered, if it couldn't.
    #[serde(default)]
    pub start_at_login_problem: Option<String>,
    /// The calendar and task sources, and the sessions they'll start.
    #[serde(default)]
    pub plan: PlanStatus,
    /// When a requested stop (kill switch) may be confirmed, if one is pending.
    #[serde(default)]
    pub stop_allowed_at: Option<Timestamp>,
    /// Whether stopping now needs a typed reason (a Locked session is running).
    #[serde(default)]
    pub stop_reason_required: bool,
    /// The friction limits in effect (unlocks, tab cap, drift prompt).
    #[serde(default)]
    pub friction: Friction,
    /// When a loosening of the friction limits takes effect, if one is cooling off.
    #[serde(default)]
    pub friction_loosen_after: Option<Timestamp>,
    /// Unlocks left for Deep sessions out of the last 24 hours' budget.
    #[serde(default)]
    pub unlocks_left: u8,
    /// The parking lot: how much is in it and how sending to the task inbox goes.
    #[serde(default)]
    pub parking_lot: ParkingStatus,
    /// What silencing notifications answered for the running session; `None` without one.
    #[serde(default)]
    pub notifications: Option<Silencing>,
    /// The shortcut that opens the parking lot, from the settings.
    #[serde(default)]
    pub parking_hotkey: Hotkey,
}

/// The parking lot at a glance.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkingStatus {
    /// Items parked.
    pub total: usize,
    /// Items still to go to the task inbox.
    pub waiting: usize,
    /// Items the task inbox refused; they stay in the parking lot.
    pub refused: usize,
    /// How the last send to the task inbox went.
    pub inbox: SourceState,
    /// Goes up with every change to the parking lot, so the window knows to read it again.
    pub revision: u64,
}

/// How the plan sources are doing, and what they'll start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStatus {
    /// The calendar.
    pub calendar: SourceStatus,
    /// The task list.
    pub tasks: SourceStatus,
}

/// The plan itself, for the window's agenda. Kept out of [`StatusReport`], which the browser
/// extension also reads, so event and task titles stay with the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanView {
    /// The daemon's current time.
    pub now: Timestamp,
    /// Cached calendar events (about 12 h back to 7 days ahead), sorted by start.
    pub events: Vec<CalendarEvent>,
    /// Cached open tasks (overdue or due within 7 days).
    pub tasks: Vec<Task>,
    /// Planned sessions that haven't started and still have time left, soonest first.
    pub upcoming: Vec<PlannedStart>,
}

/// How one plan source is doing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    /// The result of the last sync.
    pub state: SourceState,
    /// When the cached data was fetched, if there is any.
    pub fetched_at: Option<Timestamp>,
    /// How many events or tasks are cached.
    pub items: usize,
}

/// The result of the last sync of one source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SourceState {
    /// Not synced since the daemon started.
    #[default]
    Unknown,
    /// No address or token stored.
    NotConfigured,
    /// The last sync worked.
    Ok,
    /// The last sync failed; cached data (if any) is still used.
    Failing {
        /// What went wrong.
        problem: String,
        /// Whether it looked like being offline (rather than a refused token or bad data).
        offline: bool,
    },
}

/// A config edit waiting out its cooling-off delay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSummary {
    /// The configuration being changed.
    pub config: String,
    /// When the change takes effect.
    pub apply_after: Timestamp,
    /// Whether the change deletes the configuration.
    pub deleted: bool,
}

/// The last heartbeat from one browser's extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserStatus {
    /// Which browser.
    pub browser: String,
    /// When its extension last checked in.
    pub last_seen: Timestamp,
    /// Whether the extension may run in private windows.
    pub incognito_allowed: bool,
}

/// Why the daemon refused a request.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DaemonError {
    /// The client speaks a different protocol version.
    #[error("the daemon speaks protocol version {daemon}, but this client sent version {client}")]
    UnsupportedVersion {
        /// The daemon's version.
        daemon: u32,
        /// The version the client sent.
        client: u32,
    },
    /// The request wasn't valid JSON or not a known request.
    #[error("bad request: {message}")]
    BadRequest {
        /// What was wrong with it.
        message: String,
    },
    /// No configuration with that name is in effect.
    #[error("there is no configuration named \"{name}\"")]
    UnknownConfig {
        /// The name asked for.
        name: String,
    },
    /// The session state machine refused the transition.
    #[error("{error}")]
    Session {
        /// Why.
        error: SessionError,
    },
    /// The daemon hit a bug while handling the request; nothing changed.
    #[error("internal daemon error: {message}")]
    Internal {
        /// What failed.
        message: String,
    },
    /// The daemon isn't running or didn't answer (reported by a relay such as `focus-nmh`).
    #[error("the Focus daemon isn't reachable: {message}")]
    Unreachable {
        /// What went wrong.
        message: String,
    },
    /// The daemon is stopping and no longer takes requests.
    #[error("the daemon is shutting down")]
    ShuttingDown,
    /// The settings file changed since the window read it, so the edit wasn't saved.
    #[error("settings.toml changed since Settings was opened; reload and make the change again")]
    SettingsChanged,
    /// The kill switch refused to stop Focus yet.
    #[error("{error}")]
    Stop {
        /// Why.
        error: StopError,
    },
    /// Something couldn't be parked.
    #[error("{error}")]
    Park {
        /// Why.
        error: ParkError,
    },
    /// An unlock was refused.
    #[error("{error}")]
    Unlock {
        /// Why.
        error: UnlockError,
    },
    /// The daemon isn't running and no session is saved as running: Focus was stopped (reported
    /// by `focus-nmh`, so the extension can lift its blocks).
    #[error("Focus is stopped: {message}")]
    Stopped {
        /// What the relay saw.
        message: String,
    },
    /// The settings file couldn't be changed (it would be invalid, or can't be written).
    #[error("couldn't change the settings: {message}")]
    Settings {
        /// What went wrong.
        message: String,
    },
    /// The OS keychain refused to store a credential.
    #[error("couldn't store the credential: {message}")]
    Credentials {
        /// What went wrong.
        message: String,
    },
    /// The change couldn't be saved, so it wasn't made.
    #[error("couldn't save state: {message}")]
    Storage {
        /// What went wrong.
        message: String,
    },
}
