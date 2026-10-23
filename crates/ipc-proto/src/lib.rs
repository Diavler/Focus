//! Versioned IPC messages shared by `focus-ui`, `focus-daemon` and `focus-nmh`.
//!
//! On the wire, each message is one line of JSON: `{"version":1,"message":{"type":"status"}}`.
//! A client sends a [`Request`] and reads exactly one [`Response`] back, in order. This crate has
//! no I/O; `ipc-transport` moves the lines.

mod codec;
mod messages;

pub use codec::{DecodeError, PROTOCOL_VERSION, decode, encode};
pub use messages::{BrowserStatus, DaemonError, PendingSummary, Request, Response, StatusReport};
