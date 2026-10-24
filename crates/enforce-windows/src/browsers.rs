//! The browsers Focus knows on Windows, and which extension heartbeats vouch for each.

use focus_core::KnownBrowser;

/// Browsers by executable name. Chromium also runs as `chrome.exe`, so `chrome` accepts either
/// heartbeat name (the two can't be told apart by process). Vivaldi reports itself as plain
/// Chromium, which would let any Chromium heartbeat vouch for it, so it accepts none for now.
/// Browsers without a heartbeat name (Vivaldi, Firefox) are closed during every session.
pub fn known_browsers() -> Vec<KnownBrowser> {
    [
        ("chrome", &["chrome", "chromium"][..]),
        ("msedge", &["edge"][..]),
        ("brave", &["brave"][..]),
        ("opera", &["opera"][..]),
        ("vivaldi", &[][..]),
        ("firefox", &[][..]),
    ]
    .into_iter()
    .map(|(app, names)| KnownBrowser {
        heartbeat_names: names.iter().map(|&name| name.to_owned()).collect(),
        app: app.to_owned(),
    })
    .collect()
}
