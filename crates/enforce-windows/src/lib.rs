//! Windows enforcement: closes blocked apps, watches the foreground window, prompts for Do Not
//! Disturb, and registers Focus to start at login.
//!
//! Everything acts only on the current user's processes and never on Focus itself or the shell.
//! In [`Mode::DryRun`] nothing is changed; what would happen is logged instead.
//! Empty on other platforms.

#![cfg(windows)]

mod autostart;
mod browsers;
mod closing;
mod enforcer;
mod foreground;
mod notifier;
mod processes;
mod windowed;

pub use autostart::WindowsAutostart;
pub use browsers::known_browsers;
pub use enforcer::WindowsEnforcer;
pub use foreground::WindowsForeground;
pub use notifier::WindowsNotifier;
pub use windowed::WindowsWindowedApps;

/// Whether adapters act or only log what they would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Close apps and open settings for real.
    Live,
    /// Log instead of acting, for testing on a real machine.
    DryRun,
}
