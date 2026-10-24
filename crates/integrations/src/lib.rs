//! Google Calendar (iCal) and Todoist adapters for the `CalendarSource` and `TaskSource` ports.
//!
//! Both read their address or token from the `Secrets` port on every fetch, so a change made in
//! the UI applies at the next sync. HTTP sits behind [`HttpGet`] so tests replay recorded
//! responses instead of touching the network.

mod http;
mod ical;
mod time;
mod todoist;

pub use http::{HttpError, HttpGet, UreqHttp};
pub use ical::{IcalCalendar, parse_events};
pub use rrule::Tz;
pub use todoist::TodoistTasks;
