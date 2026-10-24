//! The `CalendarSource` adapter for a private iCal address, such as Google Calendar's "secret
//! address in iCal format".

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use focus_core::ports::{CalendarSource, SecretKey, Secrets, SourceError};
use focus_core::{CalendarEvent, Timestamp};
use icalendar::parser::{Component, Property, read_calendar, unfold};
use rrule::{RRule, RRuleSet, Tz, Unvalidated};

use crate::http::{HttpError, HttpGet, UreqHttp};
use crate::time::{from_timestamp, local, to_timestamp};

/// Most occurrences one recurring event can produce in the window (a few years of daily events
/// still fit; a broken rule can't run away).
const OCCURRENCE_LIMIT: u16 = 2_000;

/// Reads events from the iCal address stored as [`SecretKey::CalendarUrl`].
pub struct IcalCalendar<H = UreqHttp> {
    secrets: Arc<dyn Secrets>,
    http: H,
    zone: Tz,
}

impl IcalCalendar {
    /// Reads over HTTPS; times without a time zone are taken as the computer's local time.
    pub fn new(secrets: Arc<dyn Secrets>) -> Self {
        Self::with_http(secrets, UreqHttp::default(), Tz::LOCAL)
    }
}

impl<H: HttpGet> IcalCalendar<H> {
    /// Reads with `http`, taking times without a time zone to be in `zone`.
    pub fn with_http(secrets: Arc<dyn Secrets>, http: H, zone: Tz) -> Self {
        Self {
            secrets,
            http,
            zone,
        }
    }
}

impl<H: HttpGet> CalendarSource for IcalCalendar<H> {
    fn events(&self, from: Timestamp, to: Timestamp) -> Result<Vec<CalendarEvent>, SourceError> {
        let address = self
            .secrets
            .load(SecretKey::CalendarUrl)
            .map_err(|error| SourceError::Unavailable {
                reason: error.to_string(),
            })?
            .ok_or(SourceError::NotConfigured)?;
        let url = https_address(&address)?;
        let body = self
            .http
            .get(&url, &[], None)
            .map_err(|error| match error {
                HttpError::Network(reason) => SourceError::Unavailable { reason },
                HttpError::Status(code @ (401 | 403 | 404)) => SourceError::Rejected {
                    reason: format!(
                        "the calendar address was refused (HTTP {code}); copy the secret iCal \
                     address again"
                    ),
                },
                HttpError::Status(code) => SourceError::Unavailable {
                    reason: format!("the calendar server answered HTTP {code}"),
                },
            })?;
        parse_events(&body, from, to, self.zone).map_err(|reason| SourceError::Rejected {
            reason: format!("couldn't read the calendar: {reason}"),
        })
    }
}

/// The address to fetch: `webcal://` (as calendar apps share it) becomes `https://`, and anything
/// that isn't HTTPS is refused, since the address grants access to the calendar.
fn https_address(address: &str) -> Result<String, SourceError> {
    let address = address.trim();
    let lower = address.to_ascii_lowercase();
    if lower.starts_with("https://") {
        Ok(address.to_owned())
    } else if lower.starts_with("webcal://") {
        Ok(format!("https://{}", &address["webcal://".len()..]))
    } else {
        Err(SourceError::Rejected {
            reason: "the calendar address must start with https://".into(),
        })
    }
}

/// Events in `text` overlapping `from..to`, with recurring events expanded, sorted by start.
///
/// All-day events are skipped: they mark days, not times to focus. Cancelled events and
/// occurrences removed by `EXDATE` are skipped too.
pub fn parse_events(
    text: &str,
    from: Timestamp,
    to: Timestamp,
    zone: Tz,
) -> Result<Vec<CalendarEvent>, String> {
    let unfolded = unfold(text);
    let calendar = read_calendar(&unfolded)?;
    let vevents: Vec<&Component<'_>> = calendar
        .components
        .iter()
        .filter(|component| component.name.as_str().eq_ignore_ascii_case("VEVENT"))
        .collect();
    if vevents.is_empty() && !text.contains("BEGIN:VCALENDAR") {
        return Err("it isn't an iCal file".into());
    }

    let (window_from, window_to) = (from_timestamp(from), from_timestamp(to));
    let mut events = Vec::new();
    // Occurrences changed or cancelled individually (`RECURRENCE-ID`), by UID and original start.
    let mut overridden: HashMap<&str, HashSet<i64>> = HashMap::new();
    for vevent in &vevents {
        let Some(parsed) = Vevent::parse(vevent, zone) else {
            continue;
        };
        if let Some(original) = parsed.recurrence_id {
            overridden
                .entry(parsed.uid)
                .or_default()
                .insert(original.timestamp());
            if !parsed.cancelled {
                events.push(parsed.occurrence(parsed.start, original));
            }
        }
    }
    for vevent in &vevents {
        let Some(parsed) = Vevent::parse(vevent, zone) else {
            continue;
        };
        if parsed.recurrence_id.is_some() || parsed.cancelled {
            continue;
        }
        let skip = overridden.get(parsed.uid);
        for start in parsed.starts(window_from, window_to) {
            if skip.is_some_and(|skip| skip.contains(&start.timestamp())) {
                continue;
            }
            events.push(parsed.occurrence(start, start));
        }
    }

    events.retain(|event| event.overlaps(from, to));
    events.sort_by(|a, b| (a.start, &a.id).cmp(&(b.start, &b.id)));
    events.dedup_by(|a, b| a.id == b.id);
    Ok(events)
}

/// The parts of one `VEVENT` that matter here.
struct Vevent<'a> {
    uid: &'a str,
    title: String,
    start: DateTime<Tz>,
    length: ChronoDuration,
    rrule: Option<&'a str>,
    exdates: Vec<DateTime<Tz>>,
    recurrence_id: Option<DateTime<Tz>>,
    cancelled: bool,
}

impl<'a> Vevent<'a> {
    /// Reads a timed event; `None` for all-day events or ones without a usable start.
    fn parse(vevent: &'a Component<'a>, zone: Tz) -> Option<Self> {
        let start = match when(find(vevent, "DTSTART")?, zone)? {
            When::Time(start) => start,
            When::Date => return None,
        };
        let length = match (find(vevent, "DTEND"), find(vevent, "DURATION")) {
            (Some(end), _) => match when(end, zone)? {
                When::Time(end) => end.signed_duration_since(start),
                When::Date => return None,
            },
            (None, Some(duration)) => parse_duration(duration.val.as_str())?,
            (None, None) => ChronoDuration::zero(),
        }
        .max(ChronoDuration::zero());
        let exdates = vevent
            .properties
            .iter()
            .filter(|property| property.name.as_str().eq_ignore_ascii_case("EXDATE"))
            .flat_map(|property| exdates(property, zone))
            .collect();
        let recurrence_id = match find(vevent, "RECURRENCE-ID").and_then(|id| when(id, zone)) {
            Some(When::Time(original)) => Some(original),
            _ => None,
        };
        Some(Self {
            uid: find(vevent, "UID").map_or("", |uid| uid.val.as_str()),
            title: find(vevent, "SUMMARY")
                .map(|summary| unescape(summary.val.as_str()))
                .unwrap_or_default(),
            start,
            length,
            rrule: find(vevent, "RRULE").map(|rule| rule.val.as_str()),
            exdates,
            recurrence_id,
            cancelled: find(vevent, "STATUS")
                .is_some_and(|status| status.val.as_str().eq_ignore_ascii_case("CANCELLED")),
        })
    }

    /// Start times of occurrences that may overlap `from..to`. An event whose rule can't be read
    /// keeps just its first occurrence.
    fn starts(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<DateTime<Tz>> {
        let Some(rule) = self.rrule else {
            return vec![self.start];
        };
        let Ok(rule) = rule.parse::<RRule<Unvalidated>>() else {
            return vec![self.start];
        };
        let Ok(rule) = rule.validate(self.start) else {
            return vec![self.start];
        };
        // Start early enough to include occurrences already running at `from`.
        let after = (from - self.length - ChronoDuration::seconds(1)).with_timezone(&Tz::UTC);
        let mut set = RRuleSet::new(self.start)
            .rrule(rule)
            .after(after)
            .before(to.with_timezone(&Tz::UTC));
        for exdate in &self.exdates {
            set = set.exdate(*exdate);
        }
        set.all(OCCURRENCE_LIMIT).dates
    }

    /// The occurrence starting at `start`, identified by its UID and `original` start.
    fn occurrence(&self, start: DateTime<Tz>, original: DateTime<Tz>) -> CalendarEvent {
        let start_at = to_timestamp(start);
        let uid = if self.uid.is_empty() {
            self.title.as_str()
        } else {
            self.uid
        };
        CalendarEvent {
            id: format!("{uid}@{}", to_timestamp(original).as_millis()),
            title: self.title.clone(),
            start: start_at,
            end: to_timestamp(start + self.length).max(start_at),
        }
    }
}

/// A `DATE` or a `DATE-TIME` value.
enum When {
    Date,
    Time(DateTime<Tz>),
}

/// Reads a date or date-time property: UTC (`...Z`), with a `TZID`, or floating (in `zone`).
/// A `TZID` that isn't an IANA name (e.g. Outlook's Windows names) is also read in `zone`.
fn when(property: &Property<'_>, zone: Tz) -> Option<When> {
    let tzid = param(property, "TZID");
    let is_date = param(property, "VALUE").is_some_and(|value| value.eq_ignore_ascii_case("DATE"));
    parse_when(property.val.as_str(), tzid, is_date, zone)
}

fn parse_when(value: &str, tzid: Option<&str>, is_date: bool, zone: Tz) -> Option<When> {
    let value = value.trim();
    if is_date || value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d")
            .ok()
            .map(|_| When::Date);
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(When::Time(Tz::UTC.from_utc_datetime(&naive)));
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    let tz = tzid
        .and_then(|id| id.trim_matches('"').parse::<chrono_tz::Tz>().ok())
        .map_or(zone, Tz::Tz);
    Some(When::Time(local(tz, naive)?))
}

/// The date-times in one `EXDATE` line (comma-separated, sharing its `TZID`).
fn exdates(property: &Property<'_>, zone: Tz) -> Vec<DateTime<Tz>> {
    let tzid = param(property, "TZID");
    property
        .val
        .as_str()
        .split(',')
        .filter_map(|value| match parse_when(value, tzid, false, zone)? {
            When::Time(time) => Some(time),
            When::Date => None,
        })
        .collect()
}

/// Parses an iCal `DURATION` such as `PT1H30M` or `P1D`.
fn parse_duration(value: &str) -> Option<ChronoDuration> {
    let value = value.trim();
    let (negative, value) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let mut rest = value.strip_prefix('P')?;
    let mut total = ChronoDuration::zero();
    let mut in_time = false;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('T') {
            in_time = true;
            rest = after;
            continue;
        }
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        let amount: i64 = rest[..digits].parse().ok()?;
        let unit = rest[digits..].chars().next()?;
        total += match (unit, in_time) {
            ('W', false) => ChronoDuration::weeks(amount),
            ('D', false) => ChronoDuration::days(amount),
            ('H', true) => ChronoDuration::hours(amount),
            ('M', true) => ChronoDuration::minutes(amount),
            ('S', true) => ChronoDuration::seconds(amount),
            _ => return None,
        };
        rest = &rest[digits + unit.len_utf8()..];
    }
    Some(if negative { -total } else { total })
}

fn find<'a>(component: &'a Component<'a>, name: &str) -> Option<&'a Property<'a>> {
    component
        .properties
        .iter()
        .find(|property| property.name.as_str().eq_ignore_ascii_case(name))
}

fn param<'a>(property: &'a Property<'a>, key: &str) -> Option<&'a str> {
    property
        .params
        .iter()
        .find(|param| param.key.as_str().eq_ignore_ascii_case(key))
        .and_then(|param| param.val.as_ref())
        .map(|value| value.as_str())
}

/// Undoes iCal text escaping (`\,` `\;` `\n` `\\`).
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push(' '),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use focus_core::testing::{MemorySecrets, calendar_source_contract};

    use super::*;
    use crate::http::fixtures::Recorded;

    /// A Google Calendar export, trimmed: a weekly recurring focus block with one moved and one
    /// cancelled occurrence, a one-off event, an all-day event and a cancelled event.
    const GOOGLE: &str = include_str!("../tests/fixtures/google-basic.ics");
    const URL: &str =
        "https://calendar.google.com/calendar/ical/me%40example.com/private-0123/basic.ics";

    fn los_angeles() -> Tz {
        Tz::Tz(chrono_tz::America::Los_Angeles)
    }

    /// Milliseconds for a UTC date and time.
    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Timestamp {
        to_timestamp(
            Utc.with_ymd_and_hms(y, mo, d, h, mi, 0)
                .unwrap()
                .with_timezone(&Tz::UTC),
        )
    }

    fn calendar(response: Result<&str, HttpError>) -> (Arc<MemorySecrets>, IcalCalendar<Recorded>) {
        let secrets = Arc::new(MemorySecrets::default());
        secrets
            .store(
                SecretKey::CalendarUrl,
                Some(&URL.replace("https", "webcal")),
            )
            .unwrap();
        let http = Recorded::default().with(URL, &[], response);
        let source = IcalCalendar::with_http(secrets.clone(), http, los_angeles());
        (secrets, source)
    }

    /// The week of Monday 2026-10-19 in Los Angeles (UTC-7).
    fn week() -> (Timestamp, Timestamp) {
        (utc(2026, 10, 19, 7, 0), utc(2026, 10, 26, 7, 0))
    }

    #[test]
    fn expands_the_recorded_google_calendar() {
        let (from, to) = week();
        let (_secrets, source) = calendar(Ok(GOOGLE));
        let events = source.events(from, to).unwrap();
        let summary: Vec<(String, Timestamp, Timestamp)> = events
            .iter()
            .map(|event| (event.title.clone(), event.start, event.end))
            .collect();
        assert_eq!(
            summary,
            [
                // Mon 09:00–11:00 PDT, weekly.
                (
                    "Deep work #focus".into(),
                    utc(2026, 10, 19, 16, 0),
                    utc(2026, 10, 19, 18, 0)
                ),
                // Tue 14:00 UTC one-off, 30 minutes from DURATION.
                (
                    "Standup, team".into(),
                    utc(2026, 10, 20, 14, 0),
                    utc(2026, 10, 20, 14, 30)
                ),
                // Wed occurrence moved from 09:00 to 13:00 PDT.
                (
                    "Deep work #focus (moved)".into(),
                    utc(2026, 10, 21, 20, 0),
                    utc(2026, 10, 21, 22, 0)
                ),
                // Fri 09:00 PDT; Thursday's was cancelled with EXDATE.
                (
                    "Deep work #focus".into(),
                    utc(2026, 10, 23, 16, 0),
                    utc(2026, 10, 23, 18, 0)
                ),
            ]
        );
        assert_eq!(
            events[2].id,
            format!(
                "focus-block@google.com@{}",
                utc(2026, 10, 21, 16, 0).as_millis()
            ),
            "a moved occurrence keeps its original start in its ID"
        );
    }

    #[test]
    fn passes_the_contract_against_the_fixture() {
        let (from, to) = week();
        calendar_source_contract(&calendar(Ok(GOOGLE)).1, from, to);
        let later = to.saturating_add(Duration::from_secs(60 * 60 * 24 * 30));
        calendar_source_contract(&calendar(Ok(GOOGLE)).1, to, later);
        calendar_source_contract(&calendar(Err(HttpError::Status(404))).1, from, to);
    }

    #[test]
    fn includes_an_occurrence_already_running() {
        let (_secrets, source) = calendar(Ok(GOOGLE));
        let from = utc(2026, 10, 19, 17, 0);
        let events = source
            .events(from, from.saturating_add(Duration::from_secs(60)))
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].start, utc(2026, 10, 19, 16, 0));
    }

    #[test]
    fn errors_say_what_to_do() {
        let (from, to) = week();
        let secrets = Arc::new(MemorySecrets::default());
        let unset = IcalCalendar::with_http(secrets.clone(), Recorded::default(), los_angeles());
        assert_eq!(unset.events(from, to), Err(SourceError::NotConfigured));

        secrets
            .store(SecretKey::CalendarUrl, Some("http://example.com/cal.ics"))
            .unwrap();
        assert!(matches!(
            unset.events(from, to),
            Err(SourceError::Rejected { .. })
        ));

        let (_s, offline) = calendar(Err(HttpError::Network("dns error".into())));
        assert_eq!(
            offline.events(from, to),
            Err(SourceError::Unavailable {
                reason: "dns error".into()
            })
        );
        let (_s, refused) = calendar(Err(HttpError::Status(404)));
        assert!(matches!(
            refused.events(from, to),
            Err(SourceError::Rejected { .. })
        ));
        let (_s, garbage) = calendar(Ok("<html>Sign in</html>"));
        assert!(matches!(
            garbage.events(from, to),
            Err(SourceError::Rejected { .. })
        ));
    }

    #[test]
    fn floating_times_use_the_given_zone() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Focus\r\n\
                    DTSTART:20261020T090000\r\nDTEND:20261020T100000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (from, to) = week();
        let events = parse_events(text, from, to, los_angeles()).unwrap();
        assert_eq!(events[0].start, utc(2026, 10, 20, 16, 0));
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("PT1H30M"), Some(ChronoDuration::minutes(90)));
        assert_eq!(parse_duration("P1DT2H"), Some(ChronoDuration::hours(26)));
        assert_eq!(parse_duration("P2W"), Some(ChronoDuration::weeks(2)));
        assert_eq!(parse_duration("-PT15M"), Some(ChronoDuration::minutes(-15)));
        assert_eq!(parse_duration("PT"), Some(ChronoDuration::zero()));
        assert_eq!(parse_duration("1H"), None);
        assert_eq!(parse_duration("PT5X"), None);
    }

    #[test]
    fn unescapes_text() {
        assert_eq!(
            unescape(r"Standup\, team\; notes\nmore\\"),
            r"Standup, team; notes more\"
        );
    }
}
