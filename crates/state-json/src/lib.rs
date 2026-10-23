//! Persists the active session and pending changes as JSON; implements the `StateStore` port.
//!
//! Files, next to each other: `state.json` (current), `state.json.bak` (the previous save),
//! `state.json.tmp` (a save in progress) and `state.json.corrupt*` (unreadable files moved aside).

mod store;

pub use store::JsonStateStore;
