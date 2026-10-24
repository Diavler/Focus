//! The `TaskSource` adapter for Todoist's API (v1), using a personal API token.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime};
use focus_core::Task;
use focus_core::ports::{SecretKey, Secrets, SourceError, TaskSource};
use rrule::Tz;
use serde::Deserialize;

use crate::http::{HttpError, HttpGet, UreqHttp};
use crate::time::{local, to_timestamp};

/// Todoist's task search, which takes the same filter queries as the app.
const FILTER_URL: &str = "https://api.todoist.com/api/v1/tasks/filter";

/// Open tasks that could start a session soon. Tasks due later are fetched at a later sync.
const QUERY: &str = "overdue | next 7 days";

/// Largest page Todoist allows.
const PAGE_SIZE: &str = "200";

/// Pages read before giving up, so a misbehaving cursor can't loop forever.
const MAX_PAGES: usize = 20;

/// Reads open tasks with the token stored as [`SecretKey::TodoistToken`].
pub struct TodoistTasks<H = UreqHttp> {
    secrets: Arc<dyn Secrets>,
    http: H,
    zone: Tz,
}

impl TodoistTasks {
    /// Reads over HTTPS; due times without a time zone are taken as the computer's local time.
    pub fn new(secrets: Arc<dyn Secrets>) -> Self {
        Self::with_http(secrets, UreqHttp::default(), Tz::LOCAL)
    }
}

impl<H: HttpGet> TodoistTasks<H> {
    /// Reads with `http`, taking due times without a time zone to be in `zone`.
    pub fn with_http(secrets: Arc<dyn Secrets>, http: H, zone: Tz) -> Self {
        Self {
            secrets,
            http,
            zone,
        }
    }

    fn page(&self, token: &str, cursor: Option<&str>) -> Result<Page, SourceError> {
        let mut query = vec![("query", QUERY), ("limit", PAGE_SIZE)];
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor));
        }
        let body = self
            .http
            .get(FILTER_URL, &query, Some(token))
            .map_err(|error| match error {
                HttpError::Network(reason) => SourceError::Unavailable { reason },
                HttpError::Status(code @ (401 | 403)) => SourceError::Rejected {
                    reason: format!(
                        "Todoist refused the token (HTTP {code}); copy it again from Todoist's \
                         Settings → Integrations → Developer"
                    ),
                },
                HttpError::Status(410) => SourceError::Rejected {
                    reason: "Todoist retired the API this version uses; update Focus".into(),
                },
                HttpError::Status(code) => SourceError::Unavailable {
                    reason: format!("Todoist answered HTTP {code}"),
                },
            })?;
        serde_json::from_str(&body).map_err(|error| SourceError::Rejected {
            reason: format!("couldn't read Todoist's answer: {error}"),
        })
    }
}

impl<H: HttpGet> TaskSource for TodoistTasks<H> {
    fn tasks(&self) -> Result<Vec<Task>, SourceError> {
        let token = self
            .secrets
            .load(SecretKey::TodoistToken)
            .map_err(|error| SourceError::Unavailable {
                reason: error.to_string(),
            })?
            .ok_or(SourceError::NotConfigured)?;
        let token = token.trim();
        let mut tasks: Vec<Task> = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let page = self.page(token, cursor.as_deref())?;
            for raw in page.results {
                if !tasks.iter().any(|task| task.id == raw.id) {
                    tasks.push(raw.into_task(self.zone));
                }
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(tasks)
    }
}

/// One page of `tasks/filter`.
#[derive(Debug, Deserialize)]
struct Page {
    results: Vec<RawTask>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTask {
    id: String,
    content: String,
    #[serde(default)]
    labels: Vec<String>,
    due: Option<RawDue>,
    duration: Option<RawDuration>,
}

#[derive(Debug, Deserialize)]
struct RawDue {
    /// `2026-10-23` (a day), `2026-10-23T10:00:00` (floating local time) or
    /// `2026-10-23T17:00:00Z` (a fixed time).
    date: String,
}

#[derive(Debug, Deserialize)]
struct RawDuration {
    amount: u64,
    /// `minute` or `day`.
    unit: String,
}

impl RawTask {
    fn into_task(self, zone: Tz) -> Task {
        Task {
            due_at: self.due.and_then(|due| due_time(&due.date, zone)),
            duration: self.duration.and_then(|duration| {
                let unit = match duration.unit.as_str() {
                    "minute" => 60,
                    "day" => 24 * 60 * 60,
                    _ => return None,
                };
                Some(Duration::from_secs(duration.amount.saturating_mul(unit)))
            }),
            labels: self
                .labels
                .into_iter()
                .map(|label| label.trim_start_matches('@').to_owned())
                .filter(|label| !label.is_empty())
                .collect(),
            id: self.id,
            title: self.content,
        }
    }
}

/// The due time, or `None` for a task due on a day without a time.
fn due_time(date: &str, zone: Tz) -> Option<focus_core::Timestamp> {
    if date.ends_with('Z') {
        return DateTime::parse_from_rfc3339(date).ok().map(to_timestamp);
    }
    let naive = NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    local(zone, naive).map(to_timestamp)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use focus_core::Timestamp;
    use focus_core::testing::{MemorySecrets, task_source_contract};

    use super::*;
    use crate::http::fixtures::Recorded;

    const PAGE_1: &str = include_str!("../tests/fixtures/todoist-filter-page1.json");
    const PAGE_2: &str = include_str!("../tests/fixtures/todoist-filter-page2.json");
    const TOKEN: &str = "0123456789abcdef0123456789abcdef01234567";

    fn first_page() -> [(&'static str, &'static str); 2] {
        [("query", QUERY), ("limit", PAGE_SIZE)]
    }

    fn recorded() -> Recorded {
        let second = [("query", QUERY), ("limit", PAGE_SIZE), ("cursor", "c2")];
        Recorded::default()
            .with(FILTER_URL, &first_page(), Ok(PAGE_1))
            .with(FILTER_URL, &second, Ok(PAGE_2))
    }

    fn source(http: Recorded) -> TodoistTasks<Recorded> {
        let secrets = Arc::new(MemorySecrets::default());
        secrets
            .store(SecretKey::TodoistToken, Some(&format!(" {TOKEN}\n")))
            .unwrap();
        TodoistTasks::with_http(secrets, http, Tz::Tz(chrono_tz::Europe::Berlin))
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Timestamp {
        to_timestamp(Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap())
    }

    #[test]
    fn reads_every_page_of_the_recorded_answer() {
        let source = source(recorded());
        let tasks = source.tasks().unwrap();
        let ids: Vec<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
        assert_eq!(
            ids,
            ["6X7rM8997g3RQmvh", "6X7rfFVPjhvv84XG", "6X7rfEVP8hvv25ZQ"]
        );

        let [fixed, floating, day] = tasks.as_slice() else {
            unreachable!()
        };
        assert_eq!(fixed.title, "Write the auth PR");
        assert_eq!(fixed.labels, ["deep", "work"]);
        assert_eq!(
            fixed.due_at,
            Some(utc(2026, 10, 23, 15, 0)),
            "fixed time in UTC"
        );
        assert_eq!(fixed.duration, Some(Duration::from_secs(90 * 60)));
        assert_eq!(
            floating.due_at,
            Some(utc(2026, 10, 24, 7, 30)),
            "floating 09:30 in Berlin (UTC+2)"
        );
        assert_eq!(floating.duration, None);
        assert_eq!(day.due_at, None, "due on a day, not at a time");
        assert_eq!(
            source.http.tokens(),
            [Some(TOKEN.to_owned()), Some(TOKEN.to_owned())],
            "sends the trimmed token on every page"
        );
    }

    #[test]
    fn passes_the_contract_against_the_fixture() {
        task_source_contract(&source(recorded()));
        let refused =
            Recorded::default().with(FILTER_URL, &first_page(), Err(HttpError::Status(401)));
        task_source_contract(&source(refused));
    }

    #[test]
    fn errors_say_what_to_do() {
        let unset = TodoistTasks::with_http(
            Arc::new(MemorySecrets::default()),
            Recorded::default(),
            Tz::UTC,
        );
        assert_eq!(unset.tasks(), Err(SourceError::NotConfigured));

        for (error, offline) in [
            (HttpError::Status(401), false),
            (HttpError::Status(410), false),
            (HttpError::Status(503), true),
            (HttpError::Network("timed out".into()), true),
        ] {
            let http = Recorded::default().with(FILTER_URL, &first_page(), Err(error.clone()));
            let result = source(http).tasks();
            assert_eq!(
                matches!(result, Err(SourceError::Unavailable { .. })),
                offline,
                "{error:?} gave {result:?}"
            );
        }
        let garbage = Recorded::default().with(FILTER_URL, &first_page(), Ok("{\"error\":1}"));
        assert!(matches!(
            source(garbage).tasks(),
            Err(SourceError::Rejected { .. })
        ));
    }

    #[test]
    fn parses_due_dates() {
        let berlin = Tz::Tz(chrono_tz::Europe::Berlin);
        assert_eq!(due_time("2026-10-23", berlin), None);
        assert_eq!(
            due_time("2026-10-23T17:00:00Z", berlin),
            Some(utc(2026, 10, 23, 17, 0))
        );
        assert_eq!(
            due_time("2026-10-23T17:00:00.000000Z", berlin),
            Some(utc(2026, 10, 23, 17, 0))
        );
        assert_eq!(
            due_time("2026-12-01T09:00:00", berlin),
            Some(utc(2026, 12, 1, 8, 0))
        );
    }
}
