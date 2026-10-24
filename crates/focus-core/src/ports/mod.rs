//! Port traits: the only way the core reaches the outside world. Adapters implement them and
//! `focus-daemon` injects them.
//!
//! Ports are synchronous and `Send + Sync`, so the daemon can share one adapter across threads
//! behind an `Arc<dyn Port>`.

mod autostart;
mod clock;
mod config_source;
mod enforcer;
mod foreground;
mod history;
mod notifier;
mod plan_source;
mod process_list;
mod secrets;
mod settings_editor;
mod state_store;
mod ui_watch;
mod windowed_apps;

pub use autostart::{Autostart, AutostartError};
pub use clock::Clock;
pub use config_source::{ConfigSource, ConfigSourceError};
pub use enforcer::{EnforceError, Enforcer};
pub use foreground::ForegroundApp;
pub use history::{History, HistoryError, HistoryEvent, NoopHistory};
pub use notifier::{Notifier, NotifyError};
pub use plan_source::{CalendarSource, SourceError, TaskSource};
pub use process_list::ProcessList;
pub use secrets::{SecretKey, Secrets, SecretsError};
pub use settings_editor::SettingsEditor;
pub use state_store::{PersistedState, StateStore, StateStoreError};
pub use ui_watch::{UiWatch, UiWatchError};
pub use windowed_apps::WindowedApps;
