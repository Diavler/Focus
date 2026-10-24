//! Domain logic (sessions, rules, configs) and the port traits every adapter implements. No I/O.

mod config;
mod cooling;
mod enforcement;
mod login;
mod matcher;
mod pattern;
pub mod ports;
mod rule;
mod session;
mod strictness;
mod time;
mod url;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use config::{BlockConfig, Mode, SharedList};
pub use cooling::{ConfigState, PendingChange, SyncReport};
pub use enforcement::{
    BrowserPolicy, BrowserWatch, Heartbeat, KnownBrowser, app_key, apps_to_close, same_app,
    select_apps,
};
pub use login::{LoginSetting, Settings};
pub use matcher::evaluate;
pub use pattern::{PatternError, UrlPattern};
pub use rule::{Rule, RuleAction, Verdict};
pub use session::{
    EndEarlyResponse, EndReason, EndedSession, RunningSession, SessionError, SessionSpec,
    SessionState,
};
pub use strictness::{EndEarlyPolicy, OverridePolicy, Strictness};
pub use time::Timestamp;
