//! Today's events on an hour grid: which hours to show, where each event's box goes, and lanes
//! side by side for events that overlap. Pure layout first (tested), then how the grid looks.

use std::cmp::Reverse;
use std::fmt::Display;
use std::ops::Range;

use chrono::{NaiveDate, TimeZone, Timelike};
use focus_core::Timestamp;
use iced::widget::text::Wrapping;
use iced::widget::{Column, column, container, row, rule, space, stack};
use iced::{Element, Length};
use ipc_proto::PlanView;

use crate::Message;
use crate::agenda::{self, Row};
use crate::style::{bold, caption, entry, now_rule, pill};

/// Minutes in a day.
const DAY: u32 = 24 * 60;
/// The hours always shown, even on an empty day: 08:00 to 18:00.
const WORKDAY: (u32, u32) = (8, 18);
/// No box is drawn shorter than this many minutes, so its title fits.
const MIN_MINUTES: u32 = 25;
/// Height of one hour on the grid, in pixels.
const HOUR_HEIGHT: f32 = 52.0;
/// Width of the hour labels down the left.
const LABEL_WIDTH: f32 = 46.0;

/// One day as an hour grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    /// The first hour shown.
    pub from_hour: u32,
    /// The hour the grid ends at (not shown itself; at most 24).
    pub to_hour: u32,
    /// All-day events, shown above the grid.
    pub all_day: Vec<Row>,
    /// The timed events, in groups that overlap each other, in time order.
    pub clusters: Vec<Cluster>,
    /// Now, in minutes since midnight.
    pub now: u32,
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

/// The events of the day `now` falls on in `zone`, from `view`.
pub fn today<Z: TimeZone>(view: &PlanView, now: Timestamp, zone: &Z) -> Timeline
where
    Z::Offset: Display,
{
    let day = agenda::local(now, zone).date_naive();
    let (all_day, timed): (Vec<_>, Vec<_>) = agenda::day_events(view, day, zone)
        .into_iter()
        .partition(|(event, _)| event.all_day);
    let blocks: Vec<Block> = timed
        .into_iter()
        .map(|(event, row)| {
            let start = minute_of_day(event.start, day, zone).min(DAY - MIN_MINUTES);
            let end = minute_of_day(event.end, day, zone).clamp(start + MIN_MINUTES, DAY);
            Block { start, end, row }
        })
        .collect();
    let now = minute_of_day(now, day, zone);
    let earliest = blocks.iter().map(|block| block.start / 60).min();
    let latest = blocks.iter().map(|block| block.end.div_ceil(60)).max();
    Timeline {
        from_hour: earliest.unwrap_or(WORKDAY.0).min(WORKDAY.0).min(now / 60),
        to_hour: latest
            .unwrap_or(WORKDAY.1)
            .max(WORKDAY.1)
            .max(now / 60 + 1)
            .min(24),
        all_day: all_day.into_iter().map(|(_, row)| row).collect(),
        clusters: clusters(blocks),
        now,
    }
}

/// Minutes since the start of `day` on the wall clock in `zone`, clamped to the day.
fn minute_of_day<Z: TimeZone>(at: Timestamp, day: NaiveDate, zone: &Z) -> u32 {
    let local = agenda::local(at, zone);
    match local.date_naive().cmp(&day) {
        std::cmp::Ordering::Less => 0,
        std::cmp::Ordering::Greater => DAY,
        std::cmp::Ordering::Equal => local.hour() * 60 + local.minute(),
    }
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

/// The day: all-day events in a strip, then the hour grid with a line at now.
pub fn view(timeline: Timeline) -> Column<'static, Message> {
    let Timeline {
        from_hour,
        to_hour,
        all_day,
        clusters,
        now,
    } = timeline;
    let mut page = column![].spacing(12);
    if !all_day.is_empty() {
        let mut strip = column![caption("All day")].spacing(6);
        for item in all_day {
            let tone = item.tone();
            strip = strip.push(entry(bold(item.title).size(13), tone, item.past));
        }
        page = page.push(strip);
    }
    page.push(grid(from_hour..to_hour, now, clusters))
}

/// Hour lines underneath, the events' lanes over them, and the now line on top.
fn grid(hours: Range<u32>, now: u32, clusters: Vec<Cluster>) -> Element<'static, Message> {
    let origin = hours.start * 60;
    let end = hours.end * 60;
    let mut lines = column![];
    for hour in hours {
        lines = lines.push(
            row![
                container(caption(format!("{hour:02}:00"))).width(Length::Fixed(LABEL_WIDTH)),
                column![rule::horizontal(1)].width(Length::Fill),
            ]
            .height(Length::Fixed(HOUR_HEIGHT)),
        );
    }

    let mut events = column![];
    let mut at = origin;
    for cluster in clusters {
        events = events.push(gap(cluster.start.saturating_sub(at)));
        let mut lanes = row![].spacing(4);
        for lane in cluster.lanes {
            let mut boxes = column![];
            let mut lane_at = cluster.start;
            for block in lane {
                boxes = boxes.push(gap(block.start.saturating_sub(lane_at)));
                lane_at = block.end;
                boxes = boxes.push(block_view(block));
            }
            lanes = lanes.push(boxes.width(Length::Fill));
        }
        events = events.push(lanes.height(Length::Fixed(pixels(cluster.end - cluster.start))));
        at = cluster.end;
    }
    let events = row![
        space::horizontal().width(Length::Fixed(LABEL_WIDTH + 4.0)),
        events.width(Length::Fill)
    ];

    let mut layers: Vec<Element<'static, Message>> = vec![lines.into(), events.into()];
    if (origin..end).contains(&now) {
        let now_line = column![
            gap(now - origin),
            row![
                space::horizontal().width(Length::Fixed(LABEL_WIDTH)),
                rule::horizontal(2).style(now_rule),
            ],
        ];
        layers.push(now_line.into());
    }
    stack(layers).width(Length::Fill).into()
}

/// One event's box: its title, then its times and the session it starts if there's room.
fn block_view(block: Block) -> Element<'static, Message> {
    // A small gap below, so boxes that follow each other don't touch.
    let height = pixels(block.end - block.start) - 2.0;
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
    entry(body, tone, item.past)
        .height(Length::Fixed(height.max(1.0)))
        .into()
}

/// Empty space `minutes` tall.
fn gap(minutes: u32) -> Element<'static, Message> {
    space::vertical()
        .height(Length::Fixed(pixels(minutes)))
        .into()
}

/// How tall `minutes` are on the grid.
fn pixels(minutes: u32) -> f32 {
    // Minutes in a day fit an f32 exactly.
    minutes as f32 * HOUR_HEIGHT / 60.0
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

    #[test]
    fn an_empty_day_shows_the_working_hours() {
        let timeline = today(&view(Vec::new()), at(24, 11, 0), &zone());
        assert_eq!((timeline.from_hour, timeline.to_hour), (8, 18));
        assert_eq!(timeline.now, 11 * 60);
        assert!(timeline.clusters.is_empty());
    }

    #[test]
    fn the_grid_stretches_to_early_and_late_events_and_to_now() {
        let events = vec![
            event("Gym", at(24, 6, 30), at(24, 7, 15), false),
            event("Film", at(24, 20, 0), at(24, 22, 10), false),
        ];
        let timeline = today(&view(events), at(24, 11, 0), &zone());
        assert_eq!((timeline.from_hour, timeline.to_hour), (6, 23));

        let late = today(&view(Vec::new()), at(24, 23, 30), &zone());
        assert_eq!((late.from_hour, late.to_hour), (8, 24));
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
        let timeline = today(&view(events), at(24, 11, 0), &zone());
        let block = &timeline.clusters[0].lanes[0][0];
        assert_eq!((block.start, block.end), (9 * 60, 9 * 60 + MIN_MINUTES));
        // Stretched, Standup now reaches past 09:15, so Reminder sits beside it.
        assert_eq!(lanes(&timeline), [vec![vec!["Standup"], vec!["Reminder"]]]);
    }

    #[test]
    fn events_crossing_midnight_are_cut_to_today() {
        let events = vec![
            event("Night shift", at(23, 22, 0), at(24, 2, 0), false),
            event("Party", at(24, 23, 0), at(25, 1, 0), false),
            event("Holiday", at(24, 0, 0), at(25, 0, 0), true),
        ];
        let timeline = today(&view(events), at(24, 11, 0), &zone());
        let blocks: Vec<(u32, u32)> = timeline
            .clusters
            .iter()
            .flat_map(|cluster| cluster.lanes.iter().flatten())
            .map(|block| (block.start, block.end))
            .collect();
        assert_eq!(blocks, [(0, 2 * 60), (23 * 60, DAY)]);
        assert_eq!((timeline.from_hour, timeline.to_hour), (0, 24));
        assert_eq!(timeline.all_day.len(), 1);
        assert_eq!(timeline.all_day[0].title, "Holiday");
    }
}
