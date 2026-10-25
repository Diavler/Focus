//! Calendar events grouped by day and tasks grouped by when they're due, in local time, marking
//! what will start a session. Pure data first (tested), then how a row looks.

use std::fmt::Display;

use chrono::{DateTime, Days, NaiveDate, NaiveTime, TimeZone};
use focus_core::{CalendarEvent, PlanSource, PlannedStart, Task, TaskPriority, Timestamp};
use iced::widget::{Column, Text, column, container, row, text};
use iced::{Alignment, Element, Length};
use ipc_proto::PlanView;

use crate::Message;
use crate::style::{Tone, bold, caption, entry, pill, priority_flag, toned};

/// A titled group of rows: one day of events, or tasks due at a similar time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// `Today`, `Tomorrow`, `Monday`, `Overdue`, ...
    pub title: String,
    /// The date, e.g. `Saturday 24 October`, if the group is one day.
    pub detail: Option<String>,
    /// The events or tasks.
    pub rows: Vec<Row>,
}

/// One event or task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// When: `09:00–11:00`, `All day`, `17:00`, `Any time` or `Oct 22`.
    pub when: String,
    /// The event's or task's title.
    pub title: String,
    /// Labels, and whether it's on now.
    pub note: Option<String>,
    /// The session it starts, e.g. `Starts coding (Deep)`.
    pub starts: Option<String>,
    /// Already over.
    pub past: bool,
    /// Needs attention (an overdue task).
    pub warning: bool,
    /// A task's priority, if it's flagged (P1 to P3).
    pub priority: Option<TaskPriority>,
    /// A task's project, if known.
    pub project: Option<String>,
    /// A task's section within its project, if any.
    pub section: Option<String>,
}

/// Events for `days` days from the day `view.now` falls on in `zone`, one group per day.
pub fn calendar_days<Z: TimeZone>(view: &PlanView, zone: &Z, days: u64) -> Vec<Group>
where
    Z::Offset: Display,
{
    let today = local(view.now, zone).date_naive();
    (0..days)
        .map(|offset| {
            let day = today + Days::new(offset);
            let (title, detail) = match offset {
                0 => ("Today".to_owned(), day.format("%A %-d %B")),
                1 => ("Tomorrow".to_owned(), day.format("%A %-d %B")),
                _ => (day.format("%A").to_string(), day.format("%-d %B")),
            };
            Group {
                title,
                detail: Some(detail.to_string()),
                rows: events_on(view, day, zone),
            }
        })
        .collect()
}

fn events_on<Z: TimeZone>(view: &PlanView, day: NaiveDate, zone: &Z) -> Vec<Row>
where
    Z::Offset: Display,
{
    day_events(view, view.now, day, zone)
        .into_iter()
        .map(|(_, row)| row)
        .collect()
}

/// The events on `day` in `zone`, all-day ones first, each with its row (on now or over as of
/// `now`).
pub fn day_events<'a, Z: TimeZone>(
    view: &'a PlanView,
    now: Timestamp,
    day: NaiveDate,
    zone: &Z,
) -> Vec<(&'a CalendarEvent, Row)>
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
            let on_now = !event.all_day && event.start <= now && now < event.end;
            let planned = view.upcoming.iter().find(|start| {
                start.source == PlanSource::Calendar
                    && start.starts_at == event.start
                    && start.title == event.title
            });
            let row = Row {
                when,
                title: event.title.clone(),
                note: on_now.then(|| "On now".to_owned()),
                starts: planned.map(starts),
                past: event.end <= now,
                warning: false,
                priority: None,
                project: None,
                section: None,
            };
            (event, row)
        })
        .collect()
}

/// Tasks grouped as Overdue, Today, Tomorrow and Later this week (empty groups included, so the
/// page can say "nothing due").
pub fn task_groups<Z: TimeZone>(view: &PlanView, zone: &Z) -> Vec<Group>
where
    Z::Offset: Display,
{
    let due = TasksDue::new(view, zone);
    let (today, tomorrow) = (due.today, due.today + Days::new(1));
    vec![
        due.group("Overdue", None, |day| day < today),
        due.group("Today", Some(today), |day| day == today),
        due.group("Tomorrow", Some(tomorrow), |day| day == tomorrow),
        due.group("Later this week", None, |day| day > tomorrow),
    ]
}

/// Just the Overdue and Today groups of [`task_groups`].
pub fn overdue_and_today<Z: TimeZone>(view: &PlanView, zone: &Z) -> (Group, Group)
where
    Z::Offset: Display,
{
    let due = TasksDue::new(view, zone);
    let today = due.today;
    (
        due.group("Overdue", None, |day| day < today),
        due.group("Today", Some(today), |day| day == today),
    )
}

/// Tasks with a due day, sorted, ready to be split into groups.
struct TasksDue<'a, Z: TimeZone> {
    view: &'a PlanView,
    zone: &'a Z,
    today: NaiveDate,
    due: Vec<(NaiveDate, &'a Task)>,
}

impl<'a, Z: TimeZone> TasksDue<'a, Z>
where
    Z::Offset: Display,
{
    fn new(view: &'a PlanView, zone: &'a Z) -> Self {
        let mut due: Vec<(NaiveDate, &Task)> = view
            .tasks
            .iter()
            .filter_map(|task| Some((due_day(task, zone)?, task)))
            .collect();
        // Within a day, the most urgent first, then by time.
        due.sort_by_key(|(day, task)| {
            (
                *day,
                task.priority,
                task.due_at.is_none(),
                task.due_at,
                task.title.clone(),
            )
        });
        Self {
            view,
            zone,
            today: local(view.now, zone).date_naive(),
            due,
        }
    }

    /// The tasks due on days `keep` accepts.
    fn group(
        &self,
        title: &str,
        detail: Option<NaiveDate>,
        keep: impl Fn(NaiveDate) -> bool,
    ) -> Group {
        Group {
            title: title.to_owned(),
            detail: detail.map(|day| day.format("%A %-d %B").to_string()),
            rows: self
                .due
                .iter()
                .filter(|(day, _)| keep(*day))
                .map(|(day, task)| task_row(self.view, task, *day, self.today, self.zone))
                .collect(),
        }
    }
}

fn task_row<Z: TimeZone>(
    view: &PlanView,
    task: &Task,
    day: NaiveDate,
    today: NaiveDate,
    zone: &Z,
) -> Row
where
    Z::Offset: Display,
{
    let tomorrow = today + Days::new(1);
    let time = task.due_at.map(|at| clock(at, zone));
    let when = if day < today {
        day.format("%b %-d").to_string()
    } else if day > tomorrow {
        match &time {
            Some(time) => format!("{} · {time}", day.format("%a %-d")),
            None => day.format("%a %-d").to_string(),
        }
    } else {
        time.unwrap_or_else(|| "Any time".to_owned())
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
        note: labels,
        starts: planned.map(starts),
        past: false,
        warning: day < today,
        priority: (task.priority != TaskPriority::P4).then_some(task.priority),
        project: task.project.clone(),
        section: task.section.clone(),
    }
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

/// `at` as a date and time in `zone`.
pub fn local<Z: TimeZone>(at: Timestamp, zone: &Z) -> DateTime<Z> {
    let millis = i64::try_from(at.as_millis()).unwrap_or(i64::MAX);
    DateTime::from_timestamp_millis(millis)
        .unwrap_or_default()
        .with_timezone(zone)
}

/// `14:05` in `zone`.
pub fn clock<Z: TimeZone>(at: Timestamp, zone: &Z) -> String
where
    Z::Offset: Display,
{
    local(at, zone).format("%H:%M").to_string()
}

/// The start of `day` in `zone` (the first such instant if clocks change at midnight).
pub fn midnight<Z: TimeZone>(day: NaiveDate, zone: &Z) -> Timestamp {
    let start = zone
        .from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest()
        .map_or(0, |time| time.timestamp_millis());
    Timestamp::from_millis(u64::try_from(start).unwrap_or(0))
}

/// The rows of a group, one box each: when, title (with labels underneath), and a pill for the
/// session it starts.
pub fn rows_view(rows: Vec<Row>) -> Column<'static, Message> {
    boxes(rows, |item| {
        let when = container(toned(item.when.clone(), item.when_tone()).size(14))
            .width(Length::Fixed(110.0));
        let tags = [item.project, item.section].into_iter().flatten();
        let title = title_line(item.title, item.past, item.priority, tags);
        let mut detail = column![title].spacing(2);
        if let Some(note) = item.note {
            detail = detail.push(caption(note));
        }
        let mut line = row![when, detail.width(Length::Fill)]
            .spacing(12)
            .align_y(Alignment::Center);
        if let Some(starts) = item.starts {
            line = line.push(pill(starts, Tone::Good));
        }
        line.into()
    })
}

/// The rows as narrow boxes, for a column beside something else: the title first, then when,
/// the labels and the tags (wrapping onto more lines rather than squeezing), then the session it
/// starts.
pub fn compact_rows_view(rows: Vec<Row>) -> Column<'static, Message> {
    boxes(rows, |item| {
        let mut details = row![toned(item.when.clone(), item.when_tone()).size(13)]
            .spacing(8)
            .align_y(Alignment::Center);
        if let Some(note) = item.note {
            details = details.push(caption(note));
        }
        for tag in [item.project, item.section].into_iter().flatten() {
            details = details.push(pill(tag, Tone::Quiet));
        }
        let title = title_line(item.title, item.past, item.priority, []);
        let mut body = column![title, details.wrap().vertical_spacing(6)].spacing(4);
        if let Some(starts) = item.starts {
            body = body.push(pill(starts, Tone::Good));
        }
        body.into()
    })
}

/// Each row laid out by `layout`, in its box.
fn boxes(
    rows: Vec<Row>,
    layout: impl Fn(Row) -> Element<'static, Message>,
) -> Column<'static, Message> {
    rows.into_iter().fold(column![].spacing(8), |list, item| {
        let (tone, past) = (item.tone(), item.past);
        list.push(entry(layout(item), tone, past, Length::Shrink))
    })
}

impl Row {
    /// The colour of its box: red if it needs attention, green if it starts a session.
    pub fn tone(&self) -> Tone {
        if self.warning {
            Tone::Bad
        } else if self.starts.is_some() {
            Tone::Good
        } else {
            Tone::Quiet
        }
    }

    /// The colour of its time: red if it needs attention.
    pub fn when_tone(&self) -> Tone {
        if self.warning { Tone::Bad } else { Tone::Quiet }
    }
}

/// A row's title with a task's priority flag before it and its tags (project, section) after it.
fn title_line(
    title: String,
    past: bool,
    priority: Option<TaskPriority>,
    tags: impl IntoIterator<Item = String>,
) -> Element<'static, Message> {
    let mut line = row![].spacing(8).align_y(Alignment::Center);
    if let Some(priority) = priority {
        line = line.push(priority_flag(priority));
    }
    line = line.push(item_title(title, past));
    for tag in tags {
        line = line.push(pill(tag, Tone::Quiet));
    }
    // Tags drop to the next line when the title is long, instead of being squeezed.
    line.wrap().vertical_spacing(6).into()
}

/// A row's title: bold, or grey once it's over.
pub fn item_title(title: String, past: bool) -> Text<'static> {
    if past {
        text(title).size(15).style(crate::style::muted)
    } else {
        bold(title).size(15)
    }
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
            priority: focus_core::TaskPriority::default(),
            project: None,
            section: None,
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
                task("Call mum", "2026-10-25", None, &[]),
                task("Later", "2026-10-27", Some(at(27, 8, 0)), &[]),
            ],
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
    fn groups_events_by_local_day() {
        let days = calendar_days(&view(), &zone(), 7);
        let titles: Vec<&str> = days.iter().map(|day| day.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Today",
                "Tomorrow",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday"
            ]
        );
        assert_eq!(days[0].detail.as_deref(), Some("Saturday 24 October"));
        let today: Vec<(&str, &str, bool)> = days[0]
            .rows
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
        assert_eq!(days[0].rows[2].note.as_deref(), Some("On now"));
        assert_eq!(days[1].rows[0].when, "14:00–15:00");
        assert_eq!(days[6].rows[0].title, "Next week");
    }

    #[test]
    fn groups_tasks_by_when_they_are_due() {
        let groups = task_groups(&view(), &zone());
        let summary: Vec<(&str, Vec<(&str, &str)>)> = groups
            .iter()
            .map(|group| {
                let rows = group
                    .rows
                    .iter()
                    .map(|row| (row.when.as_str(), row.title.as_str()))
                    .collect();
                (group.title.as_str(), rows)
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Overdue", vec![("Oct 22", "Old chore")]),
                (
                    "Today",
                    vec![("17:00", "Write the report"), ("Any time", "Buy milk")]
                ),
                ("Tomorrow", vec![("Any time", "Call mum")]),
                ("Later this week", vec![("Tue 27 · 10:00", "Later")]),
            ]
        );
        assert!(groups[0].rows[0].warning);
        let report = &groups[1].rows[0];
        assert_eq!(report.note.as_deref(), Some("@deep"));
        assert_eq!(report.starts.as_deref(), Some("Starts coding (Deep)"));
        assert_eq!(report.priority, None, "P4 isn't flagged");
    }

    #[test]
    fn urgent_tasks_come_first_in_their_day_with_their_project() {
        let mut plan = view();
        let milk = plan
            .tasks
            .iter_mut()
            .find(|task| task.title == "Buy milk")
            .unwrap();
        milk.priority = TaskPriority::P1;
        milk.project = Some("Home".into());
        milk.section = Some("Errands".into());
        let today = &task_groups(&plan, &zone())[1];
        let titles: Vec<&str> = today.rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["Buy milk", "Write the report"]);
        assert_eq!(today.rows[0].priority, Some(TaskPriority::P1));
        assert_eq!(today.rows[0].project.as_deref(), Some("Home"));
        assert_eq!(today.rows[0].section.as_deref(), Some("Errands"));
    }
}
