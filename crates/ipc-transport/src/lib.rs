//! Local-socket transport for Focus IPC: where the daemon listens, how lines are framed, and a
//! typed client. Message types live in `ipc-proto`; this crate only moves them.

mod client;
mod connection;
mod endpoint;

pub use client::{Client, ClientError};
pub use connection::{Connection, MAX_LINE_BYTES};
pub use endpoint::{BindError, Endpoint, Listener, bind};
