//! Per-user daemon that owns sessions and enforcement. The library holds the request handling and
//! server so tests can run the daemon in-process; `main.rs` is the composition root.

mod clock;
mod server;
mod service;

pub use clock::SystemClock;
pub use server::serve;
pub use service::{Ports, Service, StartupError, UiWatch};
