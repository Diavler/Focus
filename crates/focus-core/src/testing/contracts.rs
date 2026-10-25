//! Shared contract suites. Each function panics with a message if an implementation breaks the
//! port's contract, so every adapter (and every fake) is held to the same behaviour.

use std::collections::HashSet;
use std::time::Duration;

use crate::auto_start::{AutoStartLog, AutoStartRule, PlanSource, PlannedStart};
use crate::config::{BlockConfig, Mode};
use crate::cooling::ConfigState;
use crate::editable::{ConfigEntry, EditableSettings, ListEntry, RulePatterns};
use crate::hotkey::Hotkey;
use crate::parking::ParkingLot;
use crate::plan::{CalendarEvent, Fetched, PlanCache, Task};
use crate::ports::{
    Autostart, CalendarSource, Clock, ConfigSource, ConfigSourceError, Enforcer, ForegroundApp,
    GlobalHotkey, History, HistoryEvent, HotkeyError, NotificationSettings, Notifier,
    PersistedState, ProcessList, SecretKey, Secrets, SettingsEditor, StateStore, TaskInbox,
    TaskSource, UiWatch, WindowedApps,
};
use crate::session::{EndReason, SessionSpec, SessionState};
use crate::strictness::Strictness;
use crate::time::Timestamp;

/// 2020-01-01T00:00:00Z; any working clock is past this.
const SANE_EPOCH: Timestamp = Timestamp::from_millis(1_577_836_800_000);

/// An app name no real system runs, so enforcer contracts never close anything real.
const MISSING_APP: &str = "focus-contract-test-no-such-app";

/// A clock reads a plausible time and doesn't go backwards between two quick reads.
pub fn clock_contract(clock: &dyn Clock) {
    let first = clock.now();
    let second = clock.now();
    assert!(first >= SANE_EPOCH, "clock reads {first:?}, before 2020");
    assert!(
        second >= first,
        "clock went backwards: {first:?} then {second:?}"
    );
}

/// A config source gives the same result twice, and loaded configs have unique, non-empty names.
pub fn config_source_contract(source: &dyn ConfigSource) {
    let first = source.load();
    assert_eq!(first, source.load(), "two loads disagree");
    let Ok(settings) = first else { return };
    let mut names = HashSet::new();
    for config in &settings.configs {
        assert!(!config.name.trim().is_empty(), "a config has an empty name");
        assert!(
            names.insert(&config.name),
            "duplicate config name {:?}",
            config.name
        );
    }
}

/// A fresh store is empty, and a load returns exactly the last state saved.
///
/// `make_empty` must return a new, empty store each time it is called.
pub fn state_store_contract<S: StateStore>(make_empty: impl Fn() -> S) {
    let store = make_empty();
    assert_eq!(store.load(), Ok(None), "a fresh store isn't empty");

    let idle = PersistedState::default();
    assert_eq!(store.save(&idle), Ok(()));
    assert_eq!(
        store.load(),
        Ok(Some(idle)),
        "load didn't return the saved state"
    );

    let running = running_state();
    assert_eq!(store.save(&running), Ok(()));
    assert_eq!(
        store.save(&running),
        Ok(()),
        "saving the same state twice failed"
    );
    assert_eq!(
        store.load(),
        Ok(Some(running)),
        "load didn't return the latest state"
    );

    assert_eq!(
        make_empty().load(),
        Ok(None),
        "stores from `make_empty` share state"
    );
}

/// Closing nothing, or an app that isn't running, closes nothing and succeeds.
///
/// Real enforcers should be run in dry-run mode; this suite never asks to close a real app.
pub fn enforcer_contract(enforcer: &dyn Enforcer) {
    assert_eq!(
        enforcer.close_apps(&[]),
        Ok(Vec::new()),
        "closing nothing closed something"
    );
    assert_eq!(
        enforcer.close_apps(&[MISSING_APP.to_owned()]),
        Ok(Vec::new()),
        "closing an app that isn't running reported a close"
    );
}

/// Listing nothing, or an app that isn't running, finds nothing.
pub fn process_list_contract(processes: &dyn ProcessList) {
    assert_eq!(
        processes.running_apps(&[]),
        Ok(Vec::new()),
        "listing nothing found something"
    );
    assert_eq!(
        processes.running_apps(&[MISSING_APP.to_owned()]),
        Ok(Vec::new()),
        "an app that isn't running was reported"
    );
}

/// Asking for the foreground app succeeds, and a reported name is never blank.
pub fn foreground_app_contract(foreground: &dyn ForegroundApp) {
    match foreground.foreground_app() {
        Ok(Some(app)) => assert!(!app.trim().is_empty(), "a blank foreground app name"),
        Ok(None) => {}
        Err(error) => panic!("foreground_app failed: {error}"),
    }
}

/// Switching start-at-login off always succeeds. Switching it on either succeeds or, on a platform
/// without support, fails with a reason; either way, repeating a call gives the same result.
///
/// This changes the real registration, so run it on a dry-run adapter or one pointed at a
/// test-only entry.
pub fn autostart_contract(autostart: &dyn Autostart) {
    let first_on = autostart.set_enabled(true);
    assert_eq!(
        autostart.set_enabled(true),
        first_on,
        "switching on again differed"
    );
    assert_eq!(autostart.set_enabled(false), Ok(()), "switching off failed");
    assert_eq!(
        autostart.set_enabled(false),
        Ok(()),
        "switching off twice failed"
    );
}

/// Asking whether the UI runs has no visible effect: asking twice gives the same answer.
///
/// It doesn't call `launch`, which would start a real UI.
pub fn ui_watch_contract(ui: &dyn UiWatch) {
    assert_eq!(
        ui.is_running(),
        ui.is_running(),
        "two quick checks disagree"
    );
}

/// Silencing and restoring both succeed, including repeated calls in either order.
pub fn notifier_contract(notifier: &dyn Notifier) {
    assert_eq!(notifier.restore(), Ok(()), "restore before silence failed");
    let first = notifier.silence();
    assert!(first.is_ok(), "silencing failed: {first:?}");
    assert_eq!(
        notifier.silence(),
        first,
        "silencing twice answered differently"
    );
    assert_eq!(notifier.restore(), Ok(()));
    assert_eq!(notifier.restore(), Ok(()), "restoring twice failed");
}

/// Opening the notification settings works, and works again.
pub fn notification_settings_contract(settings: &dyn NotificationSettings) {
    assert_eq!(settings.open(), Ok(()), "opening failed");
    assert_eq!(settings.open(), Ok(()), "opening again failed");
}

/// Sending an item answers the same way twice (a repeat after a lost answer must be harmless).
///
/// Run it against recorded fixtures or a fake, not the live service.
pub fn task_inbox_contract(inbox: &dyn TaskInbox) {
    let mut lot = ParkingLot::default();
    if let Err(error) = lot.park(
        "focus contract item",
        Some("https://example.org/"),
        SANE_EPOCH,
    ) {
        panic!("parking the contract item failed: {error}");
    }
    let item = &lot.items()[0];
    let first = inbox.add(item);
    assert_eq!(inbox.add(item), first, "sending again answered differently");
}

/// Off registers nothing; a shortcut held can't be taken again until its guard is dropped.
/// `free` should be a shortcut nothing else on the machine uses; if it's taken (or shortcuts
/// aren't supported here), the rest is skipped.
pub fn global_hotkey_contract(hotkey: &dyn GlobalHotkey, free: Hotkey) {
    assert!(
        hotkey.listen(Hotkey::Off, Box::new(|| {})).is_ok(),
        "listening to Off failed"
    );
    let guard = match hotkey.listen(free, Box::new(|| {})) {
        Ok(guard) => guard,
        Err(HotkeyError::Taken { .. } | HotkeyError::Unsupported { .. }) => return,
        Err(error) => panic!("registering {free} failed: {error}"),
    };
    assert_eq!(
        hotkey.listen(free, Box::new(|| {})).map(|_| ()),
        Err(HotkeyError::Taken { hotkey: free }),
        "a held shortcut was registered twice"
    );
    drop(guard);
    assert!(
        hotkey.listen(free, Box::new(|| {})).is_ok(),
        "the shortcut stayed taken after its guard was dropped"
    );
}

/// Two reads of `from..to` agree, and the events overlap the window, are sorted by start, end no
/// earlier than they start and have unique, non-empty IDs.
///
/// Run it against recorded fixtures, not the live service.
pub fn calendar_source_contract(source: &dyn CalendarSource, from: Timestamp, to: Timestamp) {
    let first = source.events(from, to);
    assert_eq!(first, source.events(from, to), "two reads disagree");
    let Ok(events) = first else { return };
    let mut ids = HashSet::new();
    for event in &events {
        assert!(!event.id.trim().is_empty(), "an event has an empty ID");
        assert!(ids.insert(&event.id), "duplicate event ID {:?}", event.id);
        assert!(event.end >= event.start, "{event:?} ends before it starts");
        assert!(event.overlaps(from, to), "{event:?} is outside the window");
    }
    assert!(
        events.windows(2).all(|pair| pair[0].start <= pair[1].start),
        "events aren't sorted by start"
    );
}

/// Two reads agree, and tasks have unique, non-empty IDs and labels without a leading `@`.
///
/// Run it against recorded fixtures, not the live service.
pub fn task_source_contract(source: &dyn TaskSource) {
    let first = source.tasks();
    assert_eq!(first, source.tasks(), "two reads disagree");
    let Ok(tasks) = first else { return };
    let mut ids = HashSet::new();
    for task in &tasks {
        assert!(!task.id.trim().is_empty(), "a task has an empty ID");
        assert!(ids.insert(&task.id), "duplicate task ID {:?}", task.id);
        for label in &task.labels {
            assert!(
                !label.is_empty() && !label.starts_with('@'),
                "bad label {label:?} on task {:?}",
                task.id
            );
        }
    }
}

/// A stored value reads back, removing it leaves nothing, and removing twice is fine. Each key's
/// original value is put back afterwards. On a platform without a keychain, storing may fail with
/// a reason, as long as nothing is then found.
///
/// This writes real credentials, so point a keychain adapter at a test-only service name.
pub fn secrets_contract(secrets: &dyn Secrets) {
    for key in SecretKey::ALL {
        let original = match secrets.load(key) {
            Ok(original) => original,
            Err(error) => panic!("reading {key:?} failed: {error}"),
        };
        let value = format!("contract value for {}", key.name());
        if let Err(error) = secrets.store(key, Some(&value)) {
            assert_eq!(
                secrets.load(key),
                Ok(None),
                "a failed store ({error}) left a value"
            );
            assert_eq!(secrets.store(key, None), Ok(()), "removing {key:?} failed");
            continue;
        }
        assert_eq!(
            secrets.load(key),
            Ok(Some(value)),
            "{key:?} didn't read back"
        );
        assert_eq!(secrets.store(key, None), Ok(()), "removing {key:?} failed");
        assert_eq!(
            secrets.load(key),
            Ok(None),
            "{key:?} is still there after removal"
        );
        assert_eq!(
            secrets.store(key, None),
            Ok(()),
            "removing {key:?} twice failed"
        );
        assert_eq!(
            secrets.store(key, original.as_deref()),
            Ok(()),
            "restoring {key:?} failed"
        );
    }
}

/// The editor reads the settings as written and saves edits that the source then loads: a new
/// list, configuration and automatic-start rule appear, a save from an old revision is refused
/// with `Changed`, an invalid save is refused, and refused saves change nothing. The original
/// settings are put back afterwards. `editor` and `source` must be the same settings. Skipped if
/// the settings can't be read.
pub fn settings_editor_contract(editor: &dyn SettingsEditor, source: &dyn ConfigSource) {
    let Ok(original) = editor.load_editable() else {
        return;
    };
    let stale = EditableSettings {
        revision: format!("{}-stale", original.revision),
        ..original.clone()
    };
    assert_eq!(
        editor.save_editable(&stale),
        Err(ConfigSourceError::Changed),
        "a save from another revision was accepted"
    );
    let mut invalid = original.clone();
    invalid.auto_start.push(AutoStartRule {
        config: "focus-contract-no-such-config".into(),
        strictness: Strictness::Light,
        calendar_keywords: vec!["#contract".into()],
        task_labels: Vec::new(),
        task_duration: AutoStartRule::DEFAULT_TASK_DURATION,
    });
    assert!(
        matches!(
            editor.save_editable(&invalid),
            Err(ConfigSourceError::Invalid { .. })
        ),
        "a rule for a missing config was accepted"
    );
    let mut invalid = original.clone();
    invalid.friction.tab_cap = 1;
    assert!(
        matches!(
            editor.save_editable(&invalid),
            Err(ConfigSourceError::Invalid { .. })
        ),
        "an out-of-range tab cap was accepted"
    );
    assert_eq!(
        editor.load_editable(),
        Ok(original.clone()),
        "a refused save changed the settings"
    );

    let mut edited = original.clone();
    edited.lists.push(ListEntry {
        apps: Vec::new(),
        name: "focus-contract-list".into(),
        patterns: RulePatterns {
            block: vec!["contract-list.example".into()],
            ..RulePatterns::default()
        },
    });
    edited.configs.push(ConfigEntry {
        name: "focus-contract".into(),
        mode: Mode::Blocklist,
        include: vec!["focus-contract-list".into()],
        patterns: RulePatterns {
            feed_only: vec!["contract.example".into()],
            ..RulePatterns::default()
        },
        apps: vec!["contract-app".into()],
    });
    let rule = AutoStartRule {
        config: "focus-contract".into(),
        strictness: Strictness::Deep,
        calendar_keywords: vec!["#contract".into()],
        task_labels: vec!["contract".into()],
        task_duration: Duration::from_secs(25 * 60),
    };
    edited.auto_start.push(rule.clone());
    edited.friction = crate::Friction {
        unlocks_per_day: 1,
        tab_cap: 7,
        ..edited.friction
    };
    assert_eq!(editor.save_editable(&edited), Ok(()), "a valid save failed");
    let loaded = source
        .load()
        .unwrap_or_else(|error| panic!("loading after a save: {error}"));
    let config = loaded
        .configs
        .iter()
        .find(|config| config.name == "focus-contract")
        .unwrap_or_else(|| panic!("the new config didn't load"));
    assert_eq!(
        config.verdict("https://contract-list.example/"),
        crate::rule::Verdict::Restrict(crate::rule::RuleAction::Block),
        "the included list's rules didn't load"
    );
    assert_eq!(config.apps, ["contract-app"]);
    assert_eq!(loaded.auto_start.last(), Some(&rule));
    assert_eq!(loaded.friction, edited.friction, "friction didn't save");
    let reread = editor
        .load_editable()
        .unwrap_or_else(|error| panic!("reading after a save: {error}"));
    assert_ne!(
        reread.revision, original.revision,
        "the revision didn't change"
    );
    assert_eq!(
        editor.save_editable(&edited),
        Err(ConfigSourceError::Changed),
        "saving twice from the same revision was accepted"
    );

    let restore = EditableSettings {
        revision: reread.revision,
        ..original.clone()
    };
    assert_eq!(editor.save_editable(&restore), Ok(()), "restoring failed");
    let restored = editor
        .load_editable()
        .unwrap_or_else(|error| panic!("reading after restoring: {error}"));
    assert_eq!(
        EditableSettings {
            revision: String::new(),
            ..restored
        },
        EditableSettings {
            revision: String::new(),
            ..original
        },
        "restoring didn't bring the settings back"
    );
}

/// Listing windowed apps succeeds with unique, non-blank names, sorted ignoring case, and never
/// lists Focus itself.
pub fn windowed_apps_contract(apps: &dyn WindowedApps) {
    let listed = apps
        .windowed_apps()
        .unwrap_or_else(|error| panic!("listing windowed apps failed: {error}"));
    let mut seen = HashSet::new();
    for app in &listed {
        assert!(!app.trim().is_empty(), "a blank app name");
        assert!(seen.insert(app.to_lowercase()), "{app} listed twice");
        assert!(
            !app.to_lowercase().starts_with("focus-"),
            "Focus itself was listed"
        );
    }
    assert!(
        listed
            .windows(2)
            .all(|pair| pair[0].to_lowercase() <= pair[1].to_lowercase()),
        "apps aren't sorted: {listed:?}"
    );
}

/// Every kind of event can be recorded.
pub fn history_contract(history: &dyn History) {
    let at = SANE_EPOCH;
    let events = [
        HistoryEvent::SessionStarted {
            at,
            config: "coding".into(),
            strictness: Strictness::Deep,
        },
        HistoryEvent::SessionEnded {
            at,
            reason: EndReason::EndedEarly {
                reason: Some("meeting moved".into()),
            },
        },
        HistoryEvent::SessionEnded {
            at,
            reason: EndReason::Completed,
        },
    ];
    for event in &events {
        assert_eq!(history.record(event), Ok(()), "recording {event:?} failed");
    }
}

/// A persisted state with a running Deep session, a pending end request and a queued config edit.
fn running_state() -> PersistedState {
    let mut session = SessionState::default();
    let spec = SessionSpec {
        config: BlockConfig::new("coding", Mode::Blocklist),
        strictness: Strictness::Deep,
        duration: Duration::from_secs(25 * 60),
    };
    let started = session.start(spec, SANE_EPOCH);
    assert_eq!(
        started,
        Ok(()),
        "contract fixture failed to start a session"
    );
    let requested = session.request_end_early(SANE_EPOCH);
    assert!(
        requested.is_ok(),
        "contract fixture failed to request an end"
    );
    let mut configs = ConfigState::default();
    let mut start_at_login = crate::login::LoginSetting::default();
    start_at_login.sync(true, SANE_EPOCH, Duration::ZERO);
    start_at_login.sync(false, SANE_EPOCH, ConfigState::DEFAULT_COOLING_OFF);
    let coding = BlockConfig::new("coding", Mode::Blocklist);
    configs.sync(vec![coding], &[], SANE_EPOCH, Duration::ZERO);
    configs.sync(
        Vec::new(),
        &["coding"],
        SANE_EPOCH,
        ConfigState::DEFAULT_COOLING_OFF,
    );
    assert_eq!(
        configs.pending().len(),
        1,
        "contract fixture failed to queue a change"
    );
    let planned = PlannedStart {
        key: "calendar:standup".into(),
        source: PlanSource::Calendar,
        title: "Standup".into(),
        config: "coding".into(),
        strictness: Strictness::Light,
        starts_at: SANE_EPOCH,
        ends_at: SANE_EPOCH.saturating_add(Duration::from_secs(15 * 60)),
    };
    let mut auto_started = AutoStartLog::default();
    auto_started.mark(&planned, SANE_EPOCH);
    let plan = PlanCache {
        calendar: Some(Fetched {
            at: SANE_EPOCH,
            items: vec![CalendarEvent {
                id: "standup".into(),
                title: planned.title.clone(),
                start: planned.starts_at,
                end: planned.ends_at,
                all_day: false,
            }],
        }),
        tasks: Some(Fetched {
            at: SANE_EPOCH,
            items: vec![Task {
                id: "1".into(),
                title: "Write the report".into(),
                labels: vec!["deep".into()],
                due_date: Some("2020-01-01".into()),
                due_at: Some(SANE_EPOCH),
                duration: Some(Duration::from_secs(25 * 60)),
                priority: crate::TaskPriority::default(),
                project: None,
                section: None,
            }],
        }),
    };
    let mut friction = crate::FrictionSetting::default();
    friction.sync(
        crate::Friction {
            unlocks_per_day: 1,
            tab_cap: 0,
            ..crate::Friction::default()
        },
        SANE_EPOCH,
        Duration::from_secs(3600),
    );
    let mut unlocks = crate::UnlockLedger::default();
    if let Err(error) = unlocks.unlock(
        crate::OverridePolicy::SpendUnlock,
        "the docs are there",
        SANE_EPOCH,
        friction.in_effect(),
    ) {
        panic!("a fresh ledger refused an unlock: {error}");
    }
    let mut delays = crate::DelayLadder::default();
    delays.visit(SANE_EPOCH, "https://news.ycombinator.com/");
    let mut parking_lot = ParkingLot::default();
    match parking_lot.park("read later", Some("https://docs.rs/serde"), SANE_EPOCH) {
        Ok(id) => {
            parking_lot.mark_sent(id, SANE_EPOCH);
        }
        Err(error) => panic!("parking the sample item failed: {error}"),
    }
    if let Err(error) = parking_lot.park("a stray thought", None, SANE_EPOCH) {
        panic!("parking the sample thought failed: {error}");
    }
    PersistedState {
        session,
        configs,
        start_at_login,
        plan,
        auto_started,
        friction,
        unlocks,
        delays,
        parking_lot,
    }
}
