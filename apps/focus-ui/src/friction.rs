//! The friction limits on Settings → General: unlocks, the tab cap and the drift prompt.

use focus_core::{Friction, Timestamp};
use iced::widget::{Column, column, row, slider, text};
use iced::{Alignment, Element};

use crate::help::info;
use crate::sessions::format_duration;
use crate::style::{Tone, card, heading, toned};
use crate::{Message, settings};

/// One change to the limits.
#[derive(Debug, Clone, Copy)]
pub enum Edit {
    Unlocks(u8),
    UnlockMinutes(u16),
    TabCap(u8),
    DriftHops(u8),
}

/// Applies `edit` to the limits being edited.
pub fn apply(friction: &mut Friction, edit: Edit) {
    match edit {
        Edit::Unlocks(unlocks) => friction.unlocks_per_day = unlocks,
        Edit::UnlockMinutes(minutes) => friction.unlock_minutes = minutes,
        Edit::TabCap(cap) => friction.tab_cap = from_slider(cap, Friction::TAB_CAP.0),
        Edit::DriftHops(hops) => friction.drift_hops = from_slider(hops, Friction::DRIFT_HOPS.0),
    }
}

/// A limit where 0 is off, as its slider shows it: one step below the smallest limit is "off".
fn to_slider(value: u8, smallest: u8) -> u8 {
    if value == 0 {
        smallest.saturating_sub(1)
    } else {
        value
    }
}

/// The limit a slider position stands for (0, off, below the smallest limit).
fn from_slider(position: u8, smallest: u8) -> u8 {
    if position < smallest { 0 } else { position }
}

/// The card: a slider with an explanation for each limit, and a pending loosening if any.
pub fn view<'a>(
    friction: &Friction,
    loosen_after: Option<(Timestamp, Timestamp)>,
) -> Element<'a, Message> {
    let wrap = |edit| Message::Settings(settings::Msg::Friction(edit));
    let (tab_low, tab_high) = Friction::TAB_CAP;
    let (hops_low, hops_high) = Friction::DRIFT_HOPS;
    let (minutes_low, minutes_high) = Friction::UNLOCK_MINUTES;
    let mut body: Column<'a, Message> = column![
        row![
            heading("Friction"),
            info(
                "Limits that make getting around a session slow and nudge you out of rabbit \
                 holes. Making them stricter applies right away; loosening them waits 24 hours \
                 (cooling off), so they can't be relaxed on impulse."
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        limit(
            format!(
                "Unlocks per day in Deep sessions: {}",
                friction.unlocks_per_day
            ),
            "How many blocked pages Deep sessions let you open \"just this once\" in any 24 \
             hours, each with a typed reason. Light sessions unlock for free; Locked sessions \
             never do.",
            slider(
                0..=Friction::MAX_UNLOCKS_PER_DAY,
                friction.unlocks_per_day,
                move |n| { wrap(Edit::Unlocks(n)) }
            ),
        ),
        limit(
            format!("Each unlock lasts: {} min", friction.unlock_minutes),
            "How long an unlocked page stays open in that tab.",
            slider(
                minutes_low..=minutes_high,
                friction.unlock_minutes,
                move |m| { wrap(Edit::UnlockMinutes(m)) }
            ),
        ),
        limit(
            match friction.tab_cap {
                0 => "Tab cap: off".to_owned(),
                cap => format!("Tab cap: {cap} tabs"),
            },
            "During a session, opening more tabs than this asks you to close one first. Slide \
             all the way left to turn it off.",
            slider(
                tab_low.saturating_sub(1)..=tab_high,
                to_slider(friction.tab_cap, tab_low),
                move |cap| wrap(Edit::TabCap(cap)),
            ),
        ),
        limit(
            match friction.drift_hops {
                0 => "Drift prompt: off".to_owned(),
                hops => format!("Drift prompt after {hops} hops"),
            },
            "When a tab hops this many times to new sites away from the ones you started on (the \
             sites you visit in a session's first 5 minutes, plus allowed sites), it shows the \
             trail and offers a way back. Slide all the way left to turn it off.",
            slider(
                hops_low.saturating_sub(1)..=hops_high,
                to_slider(friction.drift_hops, hops_low),
                move |hops| wrap(Edit::DriftHops(hops)),
            ),
        ),
    ]
    .spacing(14);
    if let Some((at, now)) = loosen_after {
        body = body.push(
            toned(
                format!(
                    "Looser limits apply in {} (cooling off).",
                    format_duration(at.saturating_duration_since(now))
                ),
                Tone::Warn,
            )
            .size(14),
        );
    }
    card(body).into()
}

/// One labelled slider with its explanation.
fn limit<'a>(
    label: String,
    help: &'static str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        row![text(label).size(14), info(help)]
            .spacing(8)
            .align_y(Alignment::Center),
        control.into(),
    ]
    .spacing(6)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leftmost_slider_position_turns_a_cap_off() {
        let mut friction = Friction::default();
        apply(&mut friction, Edit::TabCap(1));
        assert_eq!(friction.tab_cap, 0);
        assert_eq!(to_slider(friction.tab_cap, Friction::TAB_CAP.0), 1);
        apply(&mut friction, Edit::TabCap(2));
        assert_eq!(friction.tab_cap, 2);
        apply(&mut friction, Edit::DriftHops(1));
        assert_eq!(friction.drift_hops, 0);
        apply(&mut friction, Edit::Unlocks(0));
        assert_eq!(friction.unlocks_per_day, 0);
        assert!(friction.problems().is_empty(), "every position is valid");
    }
}
