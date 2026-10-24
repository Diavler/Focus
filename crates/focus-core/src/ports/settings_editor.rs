//! Changing the settings on the user's behalf, from the window instead of a text editor.

use crate::editable::EditableSettings;

use super::config_source::ConfigSourceError;

/// Reads the settings as written and writes the window's changes back, leaving anything it
/// doesn't manage (comments, layout) as it was. Changes take effect the way hand edits do: the
/// daemon re-reads the settings, with cooling-off where it applies.
pub trait SettingsEditor: Send + Sync {
    /// The settings as written, with a revision identifying the text they came from. Works as
    /// long as the text is well-formed, even if it has problems a save would refuse.
    fn load_editable(&self) -> Result<EditableSettings, ConfigSourceError>;

    /// Replaces the settings with `settings`. Refused with [`ConfigSourceError::Changed`] if the
    /// text changed since `settings.revision` was read, and with
    /// [`ConfigSourceError::Invalid`] if the result wouldn't be valid; nothing is written then.
    fn save_editable(&self, settings: &EditableSettings) -> Result<(), ConfigSourceError>;
}
