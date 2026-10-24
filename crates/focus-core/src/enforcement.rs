//! What to close during a session: the config's blocked apps, plus browsers that can't be trusted
//! to enforce site rules because their extension isn't checking in.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::time::Timestamp;

/// A browser the platform knows how to find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBrowser {
    /// The name its Focus extension reports in heartbeats, or `None` if there is no Focus
    /// extension for it yet (it is then always closed during a session).
    pub heartbeat_name: Option<String>,
    /// Its app (process) name, as `ProcessList` and `Enforcer` understand it.
    pub app: String,
}

/// The last heartbeat from one browser's extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    /// The browser name the extension reported.
    pub browser: String,
    /// When it last checked in.
    pub last_seen: Timestamp,
    /// Whether the extension may run in private windows.
    pub incognito_allowed: bool,
}

/// Timing for closing browsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserPolicy {
    /// A heartbeat older than this no longer counts as live.
    pub heartbeat_timeout: Duration,
    /// How long a browser may run without a live heartbeat after it's first seen, so its
    /// extension has time to start and check in.
    pub grace: Duration,
}

impl Default for BrowserPolicy {
    fn default() -> Self {
        Self {
            heartbeat_timeout: Duration::from_secs(15),
            grace: Duration::from_secs(20),
        }
    }
}

/// Remembers when each running browser was first seen, for the grace period.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserWatch {
    first_seen: BTreeMap<String, Timestamp>,
}

impl BrowserWatch {
    /// Forgets every browser, e.g. when no session is running.
    pub fn clear(&mut self) {
        self.first_seen.clear();
    }

    /// The running browsers to close now. A browser is kept if its extension has a live
    /// heartbeat with incognito access allowed (otherwise private windows would be unfiltered),
    /// or if it was first seen less than `policy.grace` ago. Browsers that stopped running are
    /// forgotten, so a relaunch gets a fresh grace period.
    pub fn browsers_to_close(
        &mut self,
        running: &[String],
        browsers: &[KnownBrowser],
        heartbeats: &[Heartbeat],
        now: Timestamp,
        policy: BrowserPolicy,
    ) -> Vec<String> {
        self.first_seen
            .retain(|app, _| running.iter().any(|name| same_app(name, app)));
        let mut close = Vec::new();
        for browser in browsers {
            if !running.iter().any(|name| same_app(name, &browser.app)) {
                continue;
            }
            let first_seen = *self.first_seen.entry(app_key(&browser.app)).or_insert(now);
            let trusted = browser.heartbeat_name.as_ref().is_some_and(|name| {
                heartbeats.iter().any(|beat| {
                    beat.browser.eq_ignore_ascii_case(name)
                        && beat.incognito_allowed
                        && now.saturating_duration_since(beat.last_seen) <= policy.heartbeat_timeout
                })
            });
            let in_grace = now.saturating_duration_since(first_seen) < policy.grace;
            if !trusted && !in_grace {
                close.push(browser.app.clone());
            }
        }
        close
    }
}

/// The apps to close: `blocked` (from the config) then `browsers`, each once, ignoring case and a
/// trailing `.exe`.
pub fn apps_to_close(blocked: &[String], browsers: Vec<String>) -> Vec<String> {
    let mut apps: Vec<String> = Vec::new();
    for app in blocked.iter().cloned().chain(browsers) {
        if !app.trim().is_empty() && !apps.iter().any(|seen| same_app(seen, &app)) {
            apps.push(app);
        }
    }
    apps
}

/// Whether two app names mean the same app: case-insensitive, with or without `.exe`.
pub fn same_app(a: &str, b: &str) -> bool {
    app_key(a) == app_key(b)
}

fn app_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::Clock;
    use crate::testing::FakeClock;

    fn browsers() -> Vec<KnownBrowser> {
        vec![
            KnownBrowser {
                heartbeat_name: Some("chrome".into()),
                app: "chrome".into(),
            },
            KnownBrowser {
                heartbeat_name: None,
                app: "firefox".into(),
            },
        ]
    }

    fn beat(at: Timestamp, incognito_allowed: bool) -> Heartbeat {
        Heartbeat {
            browser: "chrome".into(),
            last_seen: at,
            incognito_allowed,
        }
    }

    fn running(apps: &[&str]) -> Vec<String> {
        apps.iter().map(|&app| app.to_owned()).collect()
    }

    #[test]
    fn new_browsers_get_a_grace_period() {
        let clock = FakeClock::default();
        let mut watch = BrowserWatch::default();
        let policy = BrowserPolicy::default();
        let both = running(&["Chrome.exe", "firefox"]);
        assert!(
            watch
                .browsers_to_close(&both, &browsers(), &[], clock.now(), policy)
                .is_empty()
        );

        clock.advance(policy.grace);
        assert_eq!(
            watch.browsers_to_close(&both, &browsers(), &[], clock.now(), policy),
            ["chrome", "firefox"]
        );
    }

    #[test]
    fn a_live_heartbeat_with_incognito_access_keeps_a_browser() {
        let clock = FakeClock::default();
        let mut watch = BrowserWatch::default();
        let policy = BrowserPolicy::default();
        let chrome = running(&["chrome"]);
        watch.browsers_to_close(&chrome, &browsers(), &[], clock.now(), policy);
        clock.advance(policy.grace);

        let live = [beat(clock.now(), true)];
        assert!(
            watch
                .browsers_to_close(&chrome, &browsers(), &live, clock.now(), policy)
                .is_empty()
        );

        let no_incognito = [beat(clock.now(), false)];
        assert_eq!(
            watch.browsers_to_close(&chrome, &browsers(), &no_incognito, clock.now(), policy),
            ["chrome"],
            "private windows would bypass the rules"
        );

        clock.advance(policy.heartbeat_timeout + Duration::from_secs(1));
        assert_eq!(
            watch.browsers_to_close(&chrome, &browsers(), &live, clock.now(), policy),
            ["chrome"],
            "a stale heartbeat doesn't count"
        );
    }

    #[test]
    fn a_relaunched_browser_gets_a_fresh_grace_period() {
        let clock = FakeClock::default();
        let mut watch = BrowserWatch::default();
        let policy = BrowserPolicy::default();
        watch.browsers_to_close(&running(&["chrome"]), &browsers(), &[], clock.now(), policy);
        clock.advance(policy.grace);
        watch.browsers_to_close(&[], &browsers(), &[], clock.now(), policy);
        assert!(
            watch
                .browsers_to_close(&running(&["chrome"]), &browsers(), &[], clock.now(), policy)
                .is_empty()
        );
    }

    #[test]
    fn apps_to_close_dedupes_names() {
        let blocked = vec!["Discord.exe".to_owned(), "steam".to_owned(), " ".to_owned()];
        assert_eq!(
            apps_to_close(&blocked, vec!["discord".into(), "chrome".into()]),
            ["Discord.exe", "steam", "chrome"]
        );
        assert!(same_app("CHROME.EXE", "chrome"));
        assert!(!same_app("chrome", "chromium"));
    }
}
