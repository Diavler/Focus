//! Today's events on an hour grid: which hours to show, where each event's box goes, and lanes
//! side by side for events that overlap. Pure layout first (tested), then how the grid looks.
//!
//! Positions are real minutes since midnight, not clock readings, so a day when the clocks change
//! has 23 or 25 hours and its hour labels say what the clock shows.

use std::cmp::Reverse;
use std::fmt::Display;
use std::time::Duration;

use chrono::{Days, TimeZone};
use focus_core::Timestamp;
use iced::widget::text::Wrapping;
use iced::widget::{Column, column, container, row, rule, space, stack};
use iced::{Alignment, Element, Length, Padding};
use ipc_proto::PlanView;

use crate::Message;
use crate::agenda::{self, Row};
use crate::style::{Tone, bold, caption, entry, now_rule, pill, toned};

/// The fewest hours the grid shows, so a quiet day still looks like a day.
const MIN_SPAN: u32 = 5;
/// No box is drawn shorter than this many minutes, so its title fits (except at the very end of
/// the day).
const MIN_MINUTES: u32 = 25;
/// Height of one hour on the grid, in pixels.
const HOUR_HEIGHT: f32 = 52.0;
/// Width of the hour labels down the left.
const LABEL_WIDTH: f32 = 46.0;
/// Height of an hour label; labels are centred on their line, so the grid starts half of it down.
const LABEL_HEIGHT: f32 = 16.0;
/// An hour label this close to now (in minutes) is hidden, so the now line's time shows instead.
const LABEL_CLEARANCE: u32 = 12;

/// One day as an hour grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    /// The first line, in hours since midnight.
    pub from_hour: u32,
    /// The last line, in hours since midnight (24 on most days).
    pub to_hour: u32,
    /// What the clock shows at each line, `from_hour` to `to_hour` inclusive.
    pub labels: Vec<String>,
    /// All-day events, shown above the grid.
    pub all_day: Vec<Row>,
    /// Events that ended before the grid starts, listed above it.
    pub earlier: Vec<Row>,
    /// The timed events, in groups that overlap each other, in time order.
    pub clusters: Vec<Cluster>,
    /// Now, in minutes since midnight.
    pub now: u32,
    /// Now on the clock, e.g. `14:05`.
    pub now_label: String,
}

/// Events that overlap, directly or through each other, in lanes drawn side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    /// When the first event starts, in minutes since midnight.
    pub start: u32,
    /// When the last box ends.
    pub end: u32,
    /// Each lane's events, in time order and never overlapping.
    pub lanes: Vec<Vec<Block>>,
}

/// One event's box on the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Where the box starts, in minutes since midnight.
    pub start: u32,
    /// Where it ends, stretched to the minimum height if the event is shorter.
    pub end: u32,
    /// What it says.
    pub row: Row,
}

/// The events of the day `now` falls on in `zone`, from `view`, marked on now or over as of `now`.
pub fn today<Z: TimeZone>(view: &PlanView, now: Timestamp, zone: &Z) -> Timeline
where
    Z::Offset: Display,
{
    let day = agenda::local(now, zone).date_naive();
    let midnight = agenda::midnight(day, zone);
    let day_length = minutes_between(midnight, agenda::midnight(day + Days::new(1), zone));
    let minute = |at: Timestamp| minutes_between(midnight, at).min(day_length);

    let (all_day, timed): (Vec<_>, Vec<_>) = agenda::day_events(view, now, day, zone)
        .into_iter()
        .partition(|(event, _)| event.all_day);
    let blocks: Vec<Block> = timed
        .into_iter()
        .map(|(event, row)| {
            let start = minute(event.start);
            let end = minute(event.end).max(start + MIN_MINUTES).min(day_length);
            Block { start, end, row }
        })
        .collect();

    let now_minute = minute(now);
    // The grid starts an hour before now; events over by then are listed above it instead, so
    // the afternoon isn't a tall stack of empty hours.
    let cut = (now_minute / 60).saturating_sub(1);
    let (mut earlier, blocks): (Vec<Block>, Vec<Block>) =
        blocks.into_iter().partition(|block| block.end <= cut * 60);
    earlier.sort_by_key(|block| block.start);
    let earliest = blocks.iter().map(|block| block.start / 60).min();
    let latest = blocks.iter().map(|block| block.end.div_ceil(60)).max();
    let from_hour = earliest.map_or(cut, |hour| hour.min(cut));
    let to_hour = latest
        .unwrap_or(0)
        .max(now_minute / 60 + 1)
        .max(from_hour + MIN_SPAN)
        .min(day_length.div_ceil(60));
    let labels = (from_hour..=to_hour)
        .map(|hour| {
            let at = midnight.saturating_add(Duration::from_secs(u64::from(hour) * 3600));
            agenda::clock(at, zone)
        })
        .collect();
    Timeline {
        from_hour,
        to_hour,
        labels,
        all_day: all_day.into_iter().map(|(_, row)| row).collect(),
        earlier: earlier.into_iter().map(|block| block.row).collect(),
        clusters: clusters(blocks),
        now: now_minute,
        now_label: agenda::clock(now, zone),
    }
}

/// Whole minutes from `from` to `to` (0 if `to` is earlier).
fn minutes_between(from: Timestamp, to: Timestamp) -> u32 {
    u32::try_from(to.saturating_duration_since(from).as_secs() / 60).unwrap_or(u32::MAX)
}

/// Groups overlapping blocks, putting each in the first lane that's free by its start.
fn clusters(mut blocks: Vec<Block>) -> Vec<Cluster> {
    // Longer events first when two start together, so they take the left lane.
    blocks.sort_by_key(|block| (block.start, Reverse(block.end)));
    let mut clusters: Vec<Cluster> = Vec::new();
    for block in blocks {
        match clusters.last_mut() {
            Some(cluster) if block.start < cluster.end => cluster.place(block),
            _ => clusters.push(Cluster {
                start: block.start,
                end: block.end,
                lanes: vec![vec![block]],
            }),
        }
    }
    clusters
}

impl Cluster {
    fn place(&mut self, block: Block) {
        self.end = self.end.max(block.end);
        let free = self
            .lanes
            .iter_mut()
            .find(|lane| lane.last().is_some_and(|last| last.end <= block.start));
        match free {
            Some(lane) => lane.push(block),
            None => self.lanes.push(vec![block]),
        }
    }
}

/// The day: all-day events and those already over in strips, then the hour grid with a line
/// at now.
pub fn view(timeline: Timeline) -> Column<'static, Message> {
    let mut page = column![].spacing(12);
    if !timeline.all_day.is_empty() {
        let mut strip = column![caption("All day")].spacing(6);
        for item in &timeline.all_day {
            let title = bold(item.title.clone()).size(13);
            strip = strip.push(entry(title, item.tone(), item.past, Length::Shrink));
        }
        page = page.push(strip);
    }
    if !timeline.earlier.is_empty() {
        let mut strip = column![caption("Earlier today")].spacing(6);
        for item in &timeline.earlier {
            let line = row![
                caption(item.when.clone()).size(13),
                bold(item.title.clone()).size(13),
            ]
            .spacing(10);
            strip = strip.push(entry(line, item.tone(), item.past, Length::Shrink));
        }
        page = page.push(strip);
    }
    page.push(grid(timeline))
}

/// Hour lines and labels underneath, the events' lanes over them, and the now line on top.
fn grid(timeline: Timeline) -> Element<'static, Message> {
    let Timeline {
        from_hour,
        to_hour,
        labels,
        clusters,
        now,
        now_label,
        ..
    } = timeline;
    let origin = from_hour * 60;
    let top = LABEL_HEIGHT / 2.0;
    let now_shown = (origin..=to_hour * 60).contains(&now);

    let mut lines = column![gap(top)];
    for _ in from_hour..to_hour {
        lines = lines.push(line_row().height(Length::Fixed(HOUR_HEIGHT)));
    }
    lines = lines.push(line_row());
    let height = 2.0 * top + pixels(origin, to_hour * 60);

    let mut hour_labels = column![];
    for (hour, label) in (from_hour..).zip(labels) {
        let near_now = now_shown && (hour * 60).abs_diff(now) < LABEL_CLEARANCE;
        let label = if near_now { String::new() } else { label };
        hour_labels = hour_labels.push(
            container(caption(label))
                .height(Length::Fixed(LABEL_HEIGHT))
                .align_y(Alignment::Center),
        );
        hour_labels = hour_labels.push(gap(HOUR_HEIGHT - LABEL_HEIGHT));
    }

    let mut events = column![];
    let mut at = origin;
    for cluster in clusters {
        events = events.push(gap(pixels(at, cluster.start)));
        let mut lanes = row![].spacing(4);
        for lane in cluster.lanes {
            let mut boxes = column![];
            let mut lane_at = cluster.start;
            for block in lane {
                boxes = boxes.push(gap(pixels(lane_at, block.start)));
                lane_at = block.end;
                boxes = boxes.push(block_view(block));
            }
            lanes = lanes.push(boxes.width(Length::Fill));
        }
        events = events.push(lanes.height(Length::Fixed(pixels(cluster.start, cluster.end))));
        at = cluster.end;
    }
    let events = column![
        gap(top),
        row![
            space::horizontal().width(Length::Fixed(LABEL_WIDTH + 4.0)),
            events.width(Length::Fill)
        ]
    ];

    let mut layers: Vec<Element<'static, Message>> = vec![
        lines.height(Length::Fixed(height)).into(),
        hour_labels.into(),
        events.into(),
    ];
    if now_shown {
        let tag = container(toned(now_label, Tone::Bad).size(11))
            .width(Length::Fixed(LABEL_WIDTH))
            .height(Length::Fixed(LABEL_HEIGHT))
            .align_y(Alignment::Center);
        let now_line = column![
            gap(top + pixels(origin, now) - LABEL_HEIGHT / 2.0),
            row![tag, rule::horizontal(2).style(now_rule)]
                .height(Length::Fixed(LABEL_HEIGHT))
                .align_y(Alignment::Center),
        ];
        layers.push(now_line.into());
    }
    stack(layers).width(Length::Fill).into()
}

/// An hour line, starting after the labels.
fn line_row() -> iced::widget::Row<'static, Message> {
    row![
        space::horizontal().width(Length::Fixed(LABEL_WIDTH)),
        column![rule::horizontal(1)].width(Length::Fill),
    ]
}

/// One event's box, as tall as its time on the grid less a small gap below so boxes that follow
/// each other don't touch: its title, then its times and the session it starts if there's room.
fn block_view(block: Block) -> Element<'static, Message> {
    let span = pixels(block.start, block.end);
    let height = span - 2.0;
    let item = block.row;
    let tone = item.tone();
    let mut body = column![
        agenda::item_title(item.title, item.past)
            .size(13)
            .wrapping(Wrapping::None)
    ]
    .spacing(2);
    if height >= 40.0 {
        body = body.push(caption(item.when).size(12).wrapping(Wrapping::None));
    }
    if let Some(starts) = item.starts.filter(|_| height >= 70.0) {
        body = body.push(pill(starts, tone));
    }
    // The slot keeps the full span, so the next box in the lane starts where its time says.
    container(entry(body, tone, item.past, Length::Fill))
        .height(Length::Fixed(span))
        .padding(Padding::ZERO.bottom(2.0))
        .into()
}

/// Empty space `height` pixels tall.
fn gap(height: f32) -> Element<'static, Message> {
    space::vertical()
        .height(Length::Fixed(height.max(0.0)))
        .into()
}

/// How tall the time from minute `from` to minute `to` is on the grid (0 if `to` is earlier).
fn pixels(from: u32, to: u32) -> f32 {
    // Minutes in a day fit an f32 exactly.
    to.saturating_sub(from) as f32 * HOUR_HEIGHT / 60.0
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, Utc};
    use focus_core::CalendarEvent;

    use super::*;

    /// UTC+2, like Berlin in summer.
    fn zone() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }

    /// `h:m` local time on October `d`.
    fn at(d: u32, h: u32, m: u32) -> Timestamp {
        let utc = Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap() - chrono::Duration::hours(2);
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

    fn view(events: Vec<CalendarEvent>) -> PlanView {
        PlanView {
            now: at(24, 11, 0),
            events,
            tasks: Vec::new(),
            upcoming: Vec::new(),
        }
    }

    /// Each lane's titles, cluster by cluster.
    fn lanes(timeline: &Timeline) -> Vec<Vec<Vec<&str>>> {
        timeline
            .clusters
            .iter()
            .map(|cluster| {
                cluster
                    .lanes
                    .iter()
                    .map(|lane| lane.iter().map(|block| block.row.title.as_str()).collect())
                    .collect()
            })
            .collect()
    }

    /// Every block, in lane order.
    fn blocks(timeline: &Timeline) -> Vec<&Block> {
        timeline
            .clusters
            .iter()
            .flat_map(|cluster| cluster.lanes.iter().flatten())
            .collect()
    }

    /// Every block's start and end.
    fn spans(timeline: &Timeline) -> Vec<(u32, u32)> {
        blocks(timeline)
            .iter()
            .map(|block| (block.start, block.end))
            .collect()
    }

    #[test]
    fn an_empty_day_shows_the_next_few_hours() {
        let timeline = today(&view(Vec::new()), at(24, 11, 7), &zone());
        assert_eq!((timeline.from_hour, timeline.to_hour), (10, 15));
        assert_eq!(timeline.labels.first().map(String::as_str), Some("10:00"));
        assert_eq!(timeline.labels.last().map(String::as_str), Some("15:00"));
        assert_eq!(
            timeline.labels.len(),
            6,
            "a label for every line, the last included"
        );
        assert_eq!(timeline.now, 11 * 60 + 7);
        assert_eq!(timeline.now_label, "11:07");
        assert!(timeline.clusters.is_empty());
    }

    #[test]
    fn the_grid_starts_before_now_and_stretches_to_later_events() {
        let events = vec![
            event("Gym", at(24, 6, 30), at(24, 7, 15), false),
            event("Film", at(24, 20, 0), at(24, 22, 10), false),
        ];
        let timeline = today(&view(events.clone()), at(24, 11, 0), &zone());
        assert_eq!((timeline.from_hour, timeline.to_hour), (10, 23));
        let earlier: Vec<&str> = timeline
            .earlier
            .iter()
            .map(|row| row.title.as_str())
            .collect();
        assert_eq!(
            earlier,
            ["Gym"],
            "over before the grid starts, so listed above it"
        );

        let morning = today(&view(events), at(24, 6, 0), &zone());
        assert_eq!(morning.from_hour, 5);
        assert!(morning.earlier.is_empty());

        let late = today(&view(Vec::new()), at(24, 23, 30), &zone());
        assert_eq!((late.from_hour, late.to_hour), (22, 24));
        assert_eq!(late.labels.last().map(String::as_str), Some("00:00"));
    }

    #[test]
    fn overlapping_events_share_the_width_and_others_get_it_all() {
        let events = vec![
            event("Lecture", at(24, 9, 0), at(24, 10, 30), false),
            event("Call", at(24, 10, 0), at(24, 11, 0), false),
            event("Coffee", at(24, 10, 30), at(24, 11, 0), false),
            event("Coding", at(24, 14, 0), at(24, 16, 0), false),
        ];
        let timeline = today(&view(events), at(24, 11, 0), &zone());
        assert_eq!(
            lanes(&timeline),
            [
                vec![vec!["Lecture", "Coffee"], vec!["Call"]],
                vec![vec!["Coding"]],
            ]
        );
        assert_eq!(
            (timeline.clusters[0].start, timeline.clusters[0].end),
            (9 * 60, 11 * 60)
        );
    }

    #[test]
    fn short_events_get_a_minimum_height_and_dont_overlap_the_next() {
        let events = vec![
            event("Standup", at(24, 9, 0), at(24, 9, 10), false),
            event("Reminder", at(24, 9, 15), at(24, 9, 15), false),
        ];
        let timeline = today(&view(events), at(24, 8, 0), &zone());
        let block = &timeline.clusters[0].lanes[0][0];
        assert_eq!((block.start, block.end), (9 * 60, 9 * 60 + MIN_MINUTES));
        // Stretched, Standup now reaches past 09:15, so Reminder sits beside it.
        assert_eq!(lanes(&timeline), [vec![vec!["Standup"], vec!["Reminder"]]]);
    }

    #[test]
    fn a_short_event_late_at_night_stays_where_it_is() {
        let events = vec![
            event("Wind down", at(24, 23, 0), at(24, 23, 40), false),
            event("Lights out", at(24, 23, 50), at(24, 23, 55), false),
        ];
        let timeline = today(&view(events), at(24, 11, 0), &zone());
        assert_eq!(
            spans(&timeline),
            [(23 * 60, 23 * 60 + 40), (23 * 60 + 50, 24 * 60)]
        );
        assert_eq!(timeline.clusters.len(), 2, "they don't overlap");
    }

    #[test]
    fn events_crossing_midnight_are_cut_to_today() {
        let events = vec![
            event("Night shift", at(23, 22, 0), at(24, 2, 0), false),
            event("Party", at(24, 23, 0), at(25, 1, 0), false),
            event("Holiday", at(24, 0, 0), at(25, 0, 0), true),
        ];
        let timeline = today(&view(events), at(24, 1, 0), &zone());
        assert_eq!(spans(&timeline), [(0, 2 * 60), (23 * 60, 24 * 60)]);
        assert_eq!((timeline.from_hour, timeline.to_hour), (0, 24));
        assert_eq!(timeline.all_day.len(), 1);
        assert_eq!(timeline.all_day[0].title, "Holiday");
    }

    #[test]
    fn over_and_on_now_follow_the_given_now_not_the_plans() {
        let events = vec![
            event("Lecture", at(24, 9, 0), at(24, 10, 0), false),
            event("Lab", at(24, 10, 0), at(24, 12, 0), false),
        ];
        // The plan was fetched at 09:30; it's 10:05 now.
        let mut plan = view(events);
        plan.now = at(24, 9, 30);
        let timeline = today(&plan, at(24, 10, 5), &zone());
        let rows: Vec<(&str, bool, Option<&str>)> = blocks(&timeline)
            .iter()
            .map(|block| {
                (
                    block.row.title.as_str(),
                    block.row.past,
                    block.row.note.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [("Lecture", true, None), ("Lab", false, Some("On now"))]
        );
    }
}
