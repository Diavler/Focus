//! Calls the real services over the network, so they only run on request:
//! `cargo test -p integrations --test live -- --ignored`.

// Test helpers outside `#[test]` functions aren't covered by clippy's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use focus_core::ports::{CalendarSource, SecretKey, Secrets, SourceError, TaskInbox, TaskSource};
use focus_core::testing::{MemorySecrets, calendar_source_contract};
use focus_core::{ParkingLot, Timestamp};
use integrations::{IcalCalendar, TodoistInbox, TodoistTasks};

/// Google's public US holidays calendar: same server and format as a private address.
const GOOGLE_HOLIDAYS: &str = "https://calendar.google.com/calendar/ical/en.usa%23holiday%40group.v.calendar.google.com/public/basic.ics";

fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Timestamp::from_millis(u64::try_from(millis).unwrap())
}

#[test]
#[ignore = "uses the network"]
fn reads_a_real_google_calendar() {
    let secrets = Arc::new(MemorySecrets::default());
    secrets
        .store(SecretKey::CalendarUrl, Some(GOOGLE_HOLIDAYS))
        .unwrap();
    let calendar = IcalCalendar::new(secrets);
    let (from, to) = (
        now(),
        now().saturating_add(Duration::from_secs(400 * 24 * 3600)),
    );
    // Holidays are all-day events: success shows TLS, the download and the parse all worked on
    // Google's real output.
    let events = calendar.events(from, to).unwrap();
    assert!(!events.is_empty(), "a year has holidays");
    assert!(events.iter().all(|event| event.all_day));
    calendar_source_contract(&calendar, from, to);
}

#[test]
#[ignore = "uses the network"]
fn todoist_refuses_a_made_up_token() {
    let secrets = Arc::new(MemorySecrets::default());
    secrets
        .store(
            SecretKey::TodoistToken,
            Some("0000000000000000000000000000000000000000"),
        )
        .unwrap();
    let result = TodoistTasks::new(secrets).tasks();
    // 401/403 (not 404 or 410) shows the v1 endpoint exists and reads the token.
    assert!(
        matches!(&result, Err(SourceError::Rejected { reason }) if reason.contains("refused the token")),
        "{result:?}"
    );
}

#[test]
#[ignore = "uses the network"]
fn todoist_inbox_refuses_a_made_up_token() {
    let secrets = Arc::new(MemorySecrets::default());
    secrets
        .store(
            SecretKey::TodoistToken,
            Some("0000000000000000000000000000000000000000"),
        )
        .unwrap();
    let mut lot = ParkingLot::default();
    lot.park("focus live test", None, now()).unwrap();
    let result = TodoistInbox::new(secrets).add(&lot.items()[0]);
    // 401/403 (not 404 or 410) shows the v1 endpoint exists and takes the token.
    assert!(
        matches!(&result, Err(SourceError::Rejected { reason }) if reason.contains("refused the token")),
        "{result:?}"
    );
}
