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
//!
//! [friction]                     # each key optional; these are the defaults
//! unlocks_per_day = 3           # Deep sessions' unlock budget per 24 hours
//! unlock_minutes = 10
//! tab_cap = 10                  # 0 = off
//! drift_hops = 4                # 0 = off
//! ```

mod edit;
mod error;
mod parse;
mod schema;
mod source;

pub use edit::{editable, revision, with_settings};
pub use error::{Problem, SettingsError};
pub use parse::parse_settings;
pub use source::TomlConfigSource;
