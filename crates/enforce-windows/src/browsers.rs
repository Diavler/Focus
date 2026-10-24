//! The browsers Focus knows on Windows, and which extension heartbeats vouch for each.

use focus_core::KnownBrowser;

/// Browsers by executable name. Chromium also runs as `chrome.exe`, so `chrome` accepts either
/// heartbeat name. Firefox has no Focus extension yet, so it is closed during every session.
/// Vivaldi reports itself as plain Chromium.
pub fn known_browsers() -> Vec<KnownBrowser> {
    [
        ("chrome", &["chrome", "chromium"][..]),
        ("msedge", &["edge"][..]),
        ("brave", &["brave"][..]),
        ("opera", &["opera"][..]),
        ("vivaldi", &["chromium"][..]),
        ("firefox", &[][..]),
    ]
    .into_iter()
    .map(|(app, names)| KnownBrowser {
        heartbeat_names: names.iter().map(|&name| name.to_owned()).collect(),
        app: app.to_owned(),
    })
    .collect()
}
