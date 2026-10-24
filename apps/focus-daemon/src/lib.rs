//! Per-user daemon that owns sessions and enforcement. The library holds the request handling and
//! server so tests can run the daemon in-process; `main.rs` is the composition root.

mod clock;
mod inbox_sync;
mod plan_sync;
mod server;
mod service;
mod ui;

pub use clock::SystemClock;
pub use inbox_sync::{InboxSend, InboxSent};
pub use plan_sync::{PlanFetch, PlanFetched};
pub use server::serve;
pub use service::{Ports, Service, StartupError};
pub use ui::InstalledUi;
