//! Port traits: the only way the core reaches the outside world. Adapters implement them and
//! `focus-daemon` injects them.
//!
//! Ports are synchronous and `Send + Sync`, so the daemon can share one adapter across threads
//! behind an `Arc<dyn Port>`.

mod clock;
mod config_source;
mod enforcer;
mod foreground;
mod history;
mod notifier;
mod process_list;
mod state_store;

pub use clock::Clock;
pub use config_source::{ConfigSource, ConfigSourceError};
pub use enforcer::{EnforceError, Enforcer};
pub use foreground::ForegroundApp;
pub use history::{History, HistoryError, HistoryEvent, NoopHistory};
pub use notifier::{Notifier, NotifyError};
pub use process_list::ProcessList;
pub use state_store::{PersistedState, StateStore, StateStoreError};
