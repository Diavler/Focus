//! Today's calendar and tasks (and tomorrow's events), in local time, marking what will start a
//! session.

use std::fmt::Display;

use chrono::{DateTime, Days, NaiveDate, NaiveTime, TimeZone};
use focus_core::{CalendarEvent, PlanSource, PlannedStart, Task, Timestamp};
use iced::widget::{Column, column, row, text};
use iced::{Alignment, Element, Length};
use ipc_proto::{PlanStatus, PlanView, SourceState};

use crate::Message;
use crate::help::info;

/// The agenda, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agenda {
    /// Today, e.g. `Saturday 24 October`.
    pub day: String,
    /// Today's events, all-day ones first.
    pub today: Vec<Row>,
    /// Overdue tasks and tasks due today.
    pub tasks: Vec<Row>,
    /// Tomorrow's events.
    pub tomorrow: Vec<Row>,
}

/// One line of the agenda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// When: `09:00–11:00`, `All day`, `17:00` or `Overdue · Oct 22`.
    pub when: String,
    /// The event's or task's title.
    pub title: String,
    /// Labels, whether it's on now, and the session it starts.
    pub note: Option<String>,
    /// Already over.
    pub past: bool,
    /// Needs attention (an overdue task).
    pub warning: bool,
}

/// Builds the agenda for the day `view.now` falls on in `zone`.
pub fn build<Z: TimeZone>(view: &PlanView, zone: &Z) -> Agenda
where
    Z::Offset: Display,
{
    let today = local(view.now, zone).date_naive();
    let tomorrow = today + Days::new(1);
    Agenda {
        day: today.format("%A %-d %B").to_string(),
        today: events_on(view, today, zone),
        tasks: tasks_due(view, today, zone),
        tomorrow: events_on(view, tomorrow, zone),
    }
}

fn events_on<Z: TimeZone>(view: &PlanView, day: NaiveDate, zone: &Z) -> Vec<Row>
where
    Z::Offset: Display,
{
    let (from, to) = (midnight(day, zone), midnight(day + Days::new(1), zone));
    let mut events: Vec<&CalendarEvent> = view
        .events
        .iter()
        .filter(|event| event.overlaps(from, to))
        .collect();
    events.sort_by_key(|event| (!event.all_day, event.start));
    events
        .into_iter()
        .map(|event| {
            let when = if event.all_day {
                "All day".to_owned()
            } else {
                format!("{}–{}", clock(event.start, zone), clock(event.end, zone))
            };
            let now = !event.all_day && event.start <= view.now && view.now < event.end;
            let planned = view.upcoming.iter().find(|start| {
                start.source == PlanSource::Calendar
                    && start.starts_at == event.start
                    && start.title == event.title
            });
            let note = join([now.then(|| "On now".to_owned()), planned.map(starts)]);
            Row {
                when,
                title: event.title.clone(),
                note,
                past: event.end <= view.now,
                warning: false,
            }
        })
        .collect()
}

fn tasks_due<Z: TimeZone>(view: &PlanView, today: NaiveDate, zone: &Z) -> Vec<Row>
where
    Z::Offset: Display,
{
    let mut due: Vec<(NaiveDate, &Task)> = view
        .tasks
        .iter()
        .filter_map(|task| Some((due_day(task, zone)?, task)))
        .filter(|(day, _)| *day <= today)
        .collect();
    due.sort_by_key(|(day, task)| (*day, task.due_at.is_none(), task.due_at, task.title.clone()));
    due.into_iter()
        .map(|(day, task)| {
            let overdue = day < today;
            let when = match (overdue, task.due_at) {
                (true, _) => format!("Overdue · {}", day.format("%b %-d")),
                (false, Some(at)) => clock(at, zone),
                (false, None) => "Today".to_owned(),
            };
            let planned = view.upcoming.iter().find(|start| {
                start.source == PlanSource::Task
                    && Some(start.starts_at) == task.due_at
                    && start.title == task.title
            });
            let labels = (!task.labels.is_empty()).then(|| {
                task.labels
                    .iter()
                    .map(|label| format!("@{label}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            Row {
                when,
                title: task.title.clone(),
                note: join([labels, planned.map(starts)]),
                past: false,
                warning: overdue,
            }
        })
        .collect()
}

/// The day a task is due in `zone`.
fn due_day<Z: TimeZone>(task: &Task, zone: &Z) -> Option<NaiveDate> {
    task.due_date
        .as_deref()
        .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
        .or_else(|| task.due_at.map(|at| local(at, zone).date_naive()))
}

/// `Starts coding (Deep)`.
fn starts(start: &PlannedStart) -> String {
    format!("Starts {} ({})", start.config, start.strictness)
}

fn join<const N: usize>(parts: [Option<String>; N]) -> Option<String> {
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn local<Z: TimeZone>(at: Timestamp, zone: &Z) -> DateTime<Z> {
    let millis = i64::try_from(at.as_millis()).unwrap_or(i64::MAX);
    DateTime::from_timestamp_millis(millis)
        .unwrap_or_default()
        .with_timezone(zone)
}

/// `14:05` in `zone`.
fn clock<Z: TimeZone>(at: Timestamp, zone: &Z) -> String
where
    Z::Offset: Display,
{
    local(at, zone).format("%H:%M").to_string()
}

/// The start of `day` in `zone` (the first such instant if clocks change at midnight).
fn midnight<Z: TimeZone>(day: NaiveDate, zone: &Z) -> Timestamp {
    let start = zone
        .from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest()
        .map_or(0, |time| time.timestamp_millis());
    Timestamp::from_millis(u64::try_from(start).unwrap_or(0))
}

/// The agenda section, built from an agenda made just for this frame.
pub fn view_owned(agenda: Agenda, sources: &PlanStatus) -> Element<'static, Message> {
    let calendar_hint = match sources.calendar.state {
        SourceState::NotConfigured => "Connect your calendar below to see your events here.",
        _ => "Nothing on your calendar today.",
    };
    let tasks_hint = match sources.tasks.state {
        SourceState::NotConfigured => "Connect Todoist below to see your tasks here.",
        _ => "No tasks due today.",
    };
    let header = row![
        text(format!("Today · {}", agenda.day)).size(20),
        info(
            "Your calendar and Todoist tasks, refreshed every few minutes and kept for when \
             you're offline. \"Starts ...\" marks events and tasks that will start a session \
             by themselves; set that up under Automatic starts."
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let mut section = column![
        header,
        rows(column![], agenda.today, calendar_hint),
        rows(column![text("Tasks").size(16)], agenda.tasks, tasks_hint),
    ]
    .spacing(12);
    if !agenda.tomorrow.is_empty() {
        section = section.push(rows(
            column![text("Tomorrow").size(16)],
            agenda.tomorrow,
            "",
        ));
    }
    section.into()
}

fn rows(
    mut list: Column<'static, Message>,
    rows: Vec<Row>,
    empty: &'static str,
) -> Column<'static, Message> {
    if rows.is_empty() && !empty.is_empty() {
        return list.push(text(empty).size(13).style(text::secondary));
    }
    for item in rows {
        let when = text(item.when).size(14).width(Length::Fixed(130.0));
        let when = if item.warning {
            when.style(text::danger)
        } else {
            when.style(text::secondary)
        };
        let title = text(item.title).size(14);
        let mut detail = column![if item.past {
            title.style(text::secondary)
        } else {
            title
        }];
        if let Some(note) = item.note {
            detail = detail.push(text(note).size(12).style(text::secondary));
        }
        list = list.push(row![when, detail].spacing(8));
    }
    list.spacing(6)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{FixedOffset, Utc};
    use focus_core::Strictness;

    use super::*;

    /// UTC+2, like Berlin in summer.
    fn zone() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }

    fn at(d: u32, h: u32, m: u32) -> Timestamp {
        let utc = Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap();
        Timestamp::from_millis(u64::try_from(utc.timestamp_millis()).unwrap())
    }

    fn event(title: &str, start: Timestamp, end: Timestamp, all_day: bool) -> CalendarEvent {
        CalendarEvent {
            id: title.into(),
            title: title.into(),
            start,
            end,
            all_day,
        }
    }

    fn task(title: &str, day: &str, due_at: Option<Timestamp>, labels: &[&str]) -> Task {
        Task {
            id: title.into(),
            title: title.into(),
            labels: labels.iter().map(|l| (*l).into()).collect(),
            due_date: Some(day.into()),
            due_at,
            duration: None,
        }
    }

    fn view() -> PlanView {
        PlanView {
            // 11:00 local on Saturday the 24th.
            now: at(24, 9, 0),
            events: vec![
                event("Standup", at(24, 6, 0), at(24, 6, 15), false),
                event("Deep work #focus", at(24, 8, 0), at(24, 10, 0), false),
                event("Holiday", at(23, 22, 0), at(24, 22, 0), true),
                event("Review", at(25, 12, 0), at(25, 13, 0), false),
                event("Next week", at(30, 12, 0), at(30, 13, 0), false),
            ],
            tasks: vec![
                task(
                    "Write the report",
                    "2026-10-24",
                    Some(at(24, 15, 0)),
                    &["deep"],
                ),
                task("Old chore", "2026-10-22", None, &[]),
                task("Buy milk", "2026-10-24", None, &[]),
                task("Later", "2026-10-26", None, &[]),
            ],
            rules: Vec::new(),
            upcoming: vec![PlannedStart {
                key: "task:Write the report".into(),
                source: PlanSource::Task,
                title: "Write the report".into(),
                config: "coding".into(),
                strictness: Strictness::Deep,
                starts_at: at(24, 15, 0),
                ends_at: at(24, 15, 0).saturating_add(Duration::from_secs(3000)),
            }],
        }
    }

    #[test]
    fn lists_today_in_local_time() {
        let agenda = build(&view(), &zone());
        assert_eq!(agenda.day, "Saturday 24 October");
        let today: Vec<(&str, &str, bool)> = agenda
            .today
            .iter()
            .map(|row| (row.when.as_str(), row.title.as_str(), row.past))
            .collect();
        assert_eq!(
            today,
            [
                ("All day", "Holiday", false),
                ("08:00–08:15", "Standup", true),
                ("10:00–12:00", "Deep work #focus", false),
            ]
        );
        assert_eq!(agenda.today[2].note.as_deref(), Some("On now"));
        assert_eq!(agenda.tomorrow.len(), 1, "only tomorrow, not next week");
        assert_eq!(agenda.tomorrow[0].when, "14:00–15:00");
    }

    #[test]
    fn lists_overdue_and_todays_tasks() {
        let agenda = build(&view(), &zone());
        let tasks: Vec<(&str, &str, bool)> = agenda
            .tasks
            .iter()
            .map(|row| (row.when.as_str(), row.title.as_str(), row.warning))
            .collect();
        assert_eq!(
            tasks,
            [
                ("Overdue · Oct 22", "Old chore", true),
                ("17:00", "Write the report", false),
                ("Today", "Buy milk", false),
            ]
        );
        assert_eq!(
            agenda.tasks[1].note.as_deref(),
            Some("@deep · Starts coding (Deep)")
        );
    }
}
