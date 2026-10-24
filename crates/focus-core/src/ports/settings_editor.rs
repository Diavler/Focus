//! Changing the settings on the user's behalf, from the window instead of a text editor.

use crate::auto_start::AutoStartRule;

use super::config_source::ConfigSourceError;

/// Writes changes into the user's settings, leaving the rest of them as they were. Changes take
/// effect the way hand edits do: the daemon re-reads the settings, with cooling-off where it
/// applies.
pub trait SettingsEditor: Send + Sync {
    /// Replaces every automatic-start rule. If the result wouldn't be valid settings, nothing is
    /// written and the problem is returned.
    fn set_auto_start(&self, rules: &[AutoStartRule]) -> Result<(), ConfigSourceError>;
}
