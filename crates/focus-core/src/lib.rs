//! Domain logic (sessions, rules, configs) and the port traits every adapter implements. No I/O.

mod auto_start;
mod config;
mod cooling;
mod editable;
mod enforcement;
mod kill_switch;
mod login;
mod matcher;
mod pattern;
mod plan;
pub mod ports;
mod rule;
mod session;
mod strictness;
mod time;
mod url;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use auto_start::{
    AutoStartLog, AutoStartRule, PlanSource, PlannedStart, planned_starts, scheduled_configs,
};
pub use config::{BlockConfig, Mode, SharedList};
pub use cooling::{ConfigState, PendingChange, SyncReport};
pub use editable::{ACTIONS, ConfigEntry, EditableSettings, ListEntry, RulePatterns};
pub use enforcement::{
    BrowserPolicy, BrowserWatch, Heartbeat, KnownBrowser, app_key, apps_to_close, same_app,
    select_apps,
};
pub use kill_switch::{KillSwitch, StopError, StopPolicy};
pub use login::{LoginSetting, Settings};
pub use matcher::evaluate;
pub use pattern::{PatternError, UrlPattern};
pub use plan::{CalendarEvent, Fetched, PlanCache, Task};
pub use rule::{Rule, RuleAction, Verdict};
pub use session::{
    EndEarlyResponse, EndReason, EndedSession, RunningSession, SessionError, SessionSpec,
    SessionState,
};
pub use strictness::{EndEarlyPolicy, OverridePolicy, Strictness};
pub use time::Timestamp;
