//! System-wide shortcuts for the `GlobalHotkey` port: `RegisterHotKey` on Windows. On Linux the
//! XDG GlobalShortcuts portal comes in build step 9; until then shortcuts report unsupported.
//!
//! The UI uses [`system_hotkey`], so it needs no platform code of its own.

use std::sync::Arc;

use focus_core::ports::GlobalHotkey;

#[cfg(not(windows))]
mod pending;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
pub use pending::PendingHotkey;
#[cfg(windows)]
pub use windows::WindowsHotkey;

/// The shortcut adapter for this platform.
pub fn system_hotkey() -> Arc<dyn GlobalHotkey> {
    #[cfg(windows)]
    return Arc::new(WindowsHotkey);
    #[cfg(not(windows))]
    return Arc::new(PendingHotkey);
}
