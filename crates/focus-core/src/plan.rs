//! What the user planned: calendar events and tasks, and the last fetch of each kept for offline
//! use.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

/// One calendar event, or one occurrence of a recurring event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// Stable for this occurrence across fetches (the event's UID plus its start).
    pub id: String,
    /// The event's title.
    pub title: String,
    /// When it starts (local midnight for an all-day event).
    pub start: Timestamp,
    /// When it ends; never before `start`.
    pub end: Timestamp,
    /// Whether it marks whole days rather than a time. All-day events are shown but never start
    /// sessions.
    #[serde(default)]
    pub all_day: bool,
}

impl CalendarEvent {
    /// Whether the event overlaps `from..to`. An event with no length counts if it starts there.
    pub fn overlaps(&self, from: Timestamp, to: Timestamp) -> bool {
        self.start < to && (self.end > from || self.start >= from)
    }
}

/// How urgent a task is, ranked as Todoist does: P1 is the most urgent, P4 (the default) isn't
/// flagged. Sorts most urgent first.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum TaskPriority {
    /// Most urgent.
    P1,
    /// High.
    P2,
    /// Medium.
    P3,
    /// Not flagged.
    #[default]
    P4,
}

/// One open task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// The task's ID in its service.
    pub id: String,
    /// The task's text.
    pub title: String,
    /// Its labels, without a leading `@`.
    pub labels: Vec<String>,
    /// The day it's due, as `YYYY-MM-DD` in the user's time zone, if it has a due date.
    #[serde(default)]
    pub due_date: Option<String>,
    /// When it's due, if it's due at a time of day (not just on a day).
    pub due_at: Option<Timestamp>,
    /// How long it's planned to take, if set.
    pub duration: Option<Duration>,
    /// How urgent it is.
    #[serde(default)]
    pub priority: TaskPriority,
    /// The project it's in, by name, if known.
    #[serde(default)]
    pub project: Option<String>,
}

/// The items from one successful fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fetched<T> {
    /// When the fetch succeeded.
    pub at: Timestamp,
    /// What it returned.
    pub items: Vec<T>,
}

/// The last successful fetch from each source. Persisted, so automatic starts work offline.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanCache {
    /// Calendar events, or `None` if the calendar has never been fetched (or isn't set up).
    pub calendar: Option<Fetched<CalendarEvent>>,
    /// Open tasks, or `None` if tasks have never been fetched (or aren't set up).
    pub tasks: Option<Fetched<Task>>,
}

impl PlanCache {
    /// The cached calendar events (empty if there are none).
    pub fn events(&self) -> &[CalendarEvent] {
        self.calendar.as_ref().map_or(&[], |fetched| &fetched.items)
    }

    /// The cached tasks (empty if there are none).
    pub fn tasks(&self) -> &[Task] {
        self.tasks.as_ref().map_or(&[], |fetched| &fetched.items)
    }
}
