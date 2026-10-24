//! The `TaskInbox` adapter for Todoist's API (v1): parked items become tasks in the Inbox.

use std::sync::Arc;

use focus_core::ParkedItem;
use focus_core::ports::{Secrets, SourceError, TaskInbox};
use serde::Serialize;

use crate::http::{HttpPost, UreqHttp};
use crate::todoist::{todoist_error, todoist_token};

/// Where new tasks go; without a project they land in the Inbox.
const TASKS_URL: &str = "https://api.todoist.com/api/v1/tasks";

/// Adds tasks with the token stored as [`focus_core::ports::SecretKey::TodoistToken`].
pub struct TodoistInbox<H = UreqHttp> {
    secrets: Arc<dyn Secrets>,
    http: H,
}

impl TodoistInbox {
    /// Sends over HTTPS.
    pub fn new(secrets: Arc<dyn Secrets>) -> Self {
        Self::with_http(secrets, UreqHttp::default())
    }
}

impl<H: HttpPost> TodoistInbox<H> {
    /// Sends with `http`.
    pub fn with_http(secrets: Arc<dyn Secrets>, http: H) -> Self {
        Self { secrets, http }
    }
}

/// The new task: the parked text, with the link (if any) as its description.
#[derive(Serialize)]
struct NewTask<'a> {
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
}

impl<H: HttpPost> TaskInbox for TodoistInbox<H> {
    fn add(&self, item: &ParkedItem) -> Result<(), SourceError> {
        let token = todoist_token(self.secrets.as_ref())?;
        let body = serde_json::to_string(&NewTask {
            content: &item.text,
            description: item.url.as_deref().filter(|url| *url != item.text),
        })
        .map_err(|error| SourceError::Rejected {
            reason: format!("couldn't write the task: {error}"),
        })?;
        // Todoist drops a request that repeats an earlier one's id, so a send retried after a
        // lost answer doesn't add the task twice.
        let request_id = item.request_id();
        self.http
            .post_json(TASKS_URL, &body, &token, &[("X-Request-Id", &request_id)])
            .map(|_| ())
            .map_err(todoist_error)
    }
}

#[cfg(test)]
mod tests {
    use focus_core::ParkingLot;
    use focus_core::Timestamp;
    use focus_core::ports::SecretKey;
    use focus_core::testing::{MemorySecrets, task_inbox_contract};
    use serde_json::json;

    use super::*;
    use crate::http::HttpError;
    use crate::http::fixtures::Recorded;

    const CREATED: &str = r#"{"id": "6X7rM8997g3RQmvh", "content": "Read later"}"#;

    fn inbox(http: Recorded) -> TodoistInbox<Recorded> {
        let secrets = Arc::new(MemorySecrets::default());
        secrets
            .store(SecretKey::TodoistToken, Some(" token\n"))
            .unwrap();
        TodoistInbox::with_http(secrets, http)
    }

    fn parked(text: &str, url: Option<&str>) -> ParkedItem {
        let mut lot = ParkingLot::default();
        lot.park(text, url, Timestamp::from_millis(42)).unwrap();
        lot.items()[0].clone()
    }

    #[test]
    fn sends_the_text_the_link_and_a_request_id() {
        let inbox = inbox(Recorded::default().with_post(TASKS_URL, Ok(CREATED)));
        let item = parked(
            "Serde flatten docs",
            Some("https://serde.rs/attr-flatten.html"),
        );
        assert_eq!(inbox.add(&item), Ok(()));
        let thought = parked("call the dentist", None);
        assert_eq!(inbox.add(&thought), Ok(()));

        let posted = inbox.http.posted();
        assert_eq!(posted[0].url, TASKS_URL);
        assert_eq!(
            posted[0].body,
            json!({"content": "Serde flatten docs", "description": "https://serde.rs/attr-flatten.html"})
        );
        assert_eq!(
            posted[0].headers,
            [("X-Request-Id".to_owned(), item.request_id())]
        );
        assert_eq!(posted[1].body, json!({"content": "call the dentist"}));
        assert_eq!(inbox.http.tokens()[0].as_deref(), Some("token"), "trimmed");
    }

    #[test]
    fn a_bare_link_is_not_repeated_as_the_description() {
        let inbox = inbox(Recorded::default().with_post(TASKS_URL, Ok(CREATED)));
        inbox.add(&parked("", Some("https://docs.rs/"))).unwrap();
        assert_eq!(
            inbox.http.posted()[0].body,
            json!({"content": "https://docs.rs/"})
        );
    }

    #[test]
    fn errors_say_whether_to_retry() {
        let unset =
            TodoistInbox::with_http(Arc::new(MemorySecrets::default()), Recorded::default());
        assert_eq!(
            unset.add(&parked("a", None)),
            Err(SourceError::NotConfigured)
        );
        let refused = inbox(Recorded::default().with_post(TASKS_URL, Err(HttpError::Status(401))));
        assert!(matches!(
            refused.add(&parked("a", None)),
            Err(SourceError::Rejected { .. })
        ));
        let offline = inbox(
            Recorded::default().with_post(TASKS_URL, Err(HttpError::Network("no route".into()))),
        );
        assert!(matches!(
            offline.add(&parked("a", None)),
            Err(SourceError::Unavailable { .. })
        ));
    }

    #[test]
    fn passes_the_contract_against_the_fixture() {
        task_inbox_contract(&inbox(
            Recorded::default().with_post(TASKS_URL, Ok(CREATED)),
        ));
        task_inbox_contract(&inbox(
            Recorded::default().with_post(TASKS_URL, Err(HttpError::Status(503))),
        ));
    }
}
