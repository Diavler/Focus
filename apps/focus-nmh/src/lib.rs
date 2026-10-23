//! Native messaging host: bridges the browser extension (length-prefixed JSON on stdin/stdout)
//! to `focus-daemon` IPC, and registers itself with Chromium browsers.

pub mod framing;
pub mod host;
pub mod register;
