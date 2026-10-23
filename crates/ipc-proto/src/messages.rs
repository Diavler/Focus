//! The requests clients send and the responses the daemon returns.

use focus_core::{
    BlockConfig, EndEarlyResponse, SessionError, SessionState, Strictness, Timestamp, Verdict,
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
}

/// The daemon's answer to one [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Answer to [`Request::Status`].
    Status(StatusReport),
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
    /// The daemon is stopping and no longer takes requests.
    #[error("the daemon is shutting down")]
    ShuttingDown,
    /// The change couldn't be saved, so it wasn't made.
    #[error("couldn't save state: {message}")]
    Storage {
        /// What went wrong.
        message: String,
    },
}
