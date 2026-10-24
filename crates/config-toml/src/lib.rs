//! Loads and validates the TOML settings file; implements the `ConfigSource` port.
//!
//! ```toml
//! [lists.social]
//! block = ["x.com", "instagram.com"]
//!
//! [configs.coding]
//! mode = "blocklist"            # default; or "allowlist"
//! include = ["social"]
//! feed_only = ["reddit.com"]    # also: allow, ask, delay, block
//! allow = ["reddit.com/r/rust/comments/*"]
//! apps = ["discord"]
//!
//! [[auto_start]]                 # start sessions from the calendar or Todoist
//! config = "coding"
//! strictness = "deep"           # default "light"
//! calendar_keywords = ["#focus"]
//! task_labels = ["deep"]
//! task_minutes = 50             # for tasks without a duration
//! ```

mod edit;
mod error;
mod parse;
mod schema;
mod source;

pub use edit::with_auto_start;
pub use error::{Problem, SettingsError};
pub use parse::parse_settings;
pub use source::TomlConfigSource;
