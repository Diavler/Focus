//! A system-wide shortcut, for opening the parking lot from any app.

use std::fmt;

use thiserror::Error;

use crate::hotkey::Hotkey;

/// Listens for a system-wide shortcut.
pub trait GlobalHotkey: Send + Sync {
    /// Calls `pressed` (on another thread) each time `hotkey` is pressed, until the returned guard
    /// is dropped. [`Hotkey::Off`] listens to nothing.
    fn listen(
        &self,
        hotkey: Hotkey,
        pressed: Box<dyn Fn() + Send + Sync>,
    ) -> Result<HotkeyGuard, HotkeyError>;
}

/// Keeps a shortcut registered; dropping it unregisters the shortcut.
pub struct HotkeyGuard {
    _registration: Option<Box<dyn Send>>,
}

impl HotkeyGuard {
    /// A guard that unregisters by dropping `registration` (whose `Drop` does the platform work).
    pub fn new(registration: impl Send + 'static) -> Self {
        Self {
            _registration: Some(Box::new(registration)),
        }
    }

    /// A guard for nothing registered (the shortcut is off).
    pub fn none() -> Self {
        Self {
            _registration: None,
        }
    }
}

impl fmt::Debug for HotkeyGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HotkeyGuard").finish_non_exhaustive()
    }
}

/// Why a shortcut couldn't be registered.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HotkeyError {
    /// Another app (or another Focus window) already uses it.
    #[error("{hotkey} is already used by another app; pick another shortcut in Settings")]
    Taken {
        /// The shortcut asked for.
        hotkey: Hotkey,
    },
    /// Not supported on this platform yet.
    #[error("system-wide shortcuts aren't supported here yet: {reason}")]
    Unsupported {
        /// Why.
        reason: String,
    },
    /// The platform refused for another reason.
    #[error("couldn't register the shortcut: {reason}")]
    Failed {
        /// What went wrong.
        reason: String,
    },
}
