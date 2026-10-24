//! Converting between `chrono` times and the core's `Timestamp`.

use chrono::{DateTime, NaiveDateTime, TimeDelta, TimeZone, Utc};
use focus_core::Timestamp;
use rrule::Tz;

/// The instant as a `Timestamp`; times before 1970 read as the epoch.
pub(crate) fn to_timestamp<Z: TimeZone>(time: DateTime<Z>) -> Timestamp {
    Timestamp::from_millis(u64::try_from(time.timestamp_millis()).unwrap_or(0))
}

/// The `Timestamp` as a UTC time; one too large for `chrono` reads as its latest time.
pub(crate) fn from_timestamp(time: Timestamp) -> DateTime<Utc> {
    i64::try_from(time.as_millis())
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// A wall-clock time in `zone`. A time that occurs twice (clocks going back) is the first one; a
/// time skipped by clocks going forward is read an hour later.
pub(crate) fn local(zone: Tz, naive: NaiveDateTime) -> Option<DateTime<Tz>> {
    zone.from_local_datetime(&naive).earliest().or_else(|| {
        zone.from_local_datetime(&(naive + TimeDelta::hours(1)))
            .earliest()
    })
}
