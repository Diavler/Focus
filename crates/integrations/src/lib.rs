//! Google Calendar (iCal) and Todoist adapters for the `CalendarSource`, `TaskSource` and
//! `TaskInbox` ports.
//!
//! They read their address or token from the `Secrets` port on every call, so a change made in
//! the UI applies at once. HTTP sits behind [`HttpGet`] and [`HttpPost`] so tests replay recorded
//! responses instead of touching the network.

mod http;
mod ical;
mod time;
mod todoist;
mod todoist_inbox;

pub use http::{HttpError, HttpGet, HttpPost, UreqHttp};
pub use ical::IcalCalendar;
pub use rrule::Tz;
pub use todoist::TodoistTasks;
pub use todoist_inbox::TodoistInbox;
