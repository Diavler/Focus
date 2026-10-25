//! The friction limits on the Rules page's Limits tab: unlocks, the tab cap and the drift prompt.

use focus_core::{Friction, Timestamp};
use iced::widget::{Column, column, container, row, slider, space, toggler};
use iced::{Alignment, Element, Length};

use crate::help::info;
use crate::sessions::format_duration;
use crate::style::{self, Tone, card, heading, toned};
use crate::{Message, settings};

/// One change to the limits.
#[derive(Debug, Clone, Copy)]
pub enum Edit {
    Unlocks(u8),
    UnlockMinutes(u16),
    TabCapOn(bool),
    TabCap(u8),
    DriftHopsOn(bool),
    DriftHops(u8),
}

/// Applies `edit` to the limits being edited. Turning a cap or the drift prompt on brings back
/// its default (0 means off).
pub fn apply(friction: &mut Friction, edit: Edit) {
    let defaults = Friction::default();
    match edit {
        Edit::Unlocks(unlocks) => friction.unlocks_per_day = unlocks,
        Edit::UnlockMinutes(minutes) => friction.unlock_minutes = minutes,
        Edit::TabCapOn(on) => {
            friction.tab_cap = if on {
                defaults.tab_cap.max(Friction::TAB_CAP.0)
            } else {
                0
            };
        }
        Edit::TabCap(cap) => friction.tab_cap = cap,
        Edit::DriftHopsOn(on) => {
            friction.drift_hops = if on {
                defaults.drift_hops.max(Friction::DRIFT_HOPS.0)
            } else {
                0
            };
        }
        Edit::DriftHops(hops) => friction.drift_hops = hops,
    }
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
            heading("Limits"),
            info(
                "Limits that make escaping a session slow. Stricter applies now; looser waits 24 hours."
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        limit(
            "Unlocks per day",
            "Blocked pages a Deep session lets you open \"just this once\" in any 24 hours, \
             each with a typed reason. Light sessions unlock freely; Locked never do.",
            None,
            Some(
                slider(
                    0..=Friction::MAX_UNLOCKS_PER_DAY,
                    friction.unlocks_per_day,
                    move |n| wrap(Edit::Unlocks(n)),
                )
                .style(style::fader)
                .into(),
            ),
            friction.unlocks_per_day.to_string(),
        ),
        limit(
            "Unlock length",
            "How long an unlocked page stays open in its tab.",
            None,
            Some(
                slider(
                    minutes_low..=minutes_high,
                    friction.unlock_minutes,
                    move |m| wrap(Edit::UnlockMinutes(m)),
                )
                .style(style::fader)
                .into(),
            ),
            format!("{} MIN", friction.unlock_minutes),
        ),
        limit(
            "Tab cap",
            "During a session, opening more tabs than this asks you to close one first.",
            Some(switch(friction.tab_cap > 0, move |on| {
                wrap(Edit::TabCapOn(on))
            })),
            (friction.tab_cap > 0).then(|| {
                slider(tab_low..=tab_high, friction.tab_cap, move |cap| {
                    wrap(Edit::TabCap(cap))
                })
                .style(style::fader)
                .into()
            }),
            match friction.tab_cap {
                0 => "OFF".to_owned(),
                cap => format!("{cap} TABS"),
            },
        ),
        limit(
            "Drift prompt",
            "When a tab hops this many times away from the sites you started on (those of a \
             session's first 5 min, plus allowed sites), Focus shows the trail and offers a \
             way back.",
            Some(switch(friction.drift_hops > 0, move |on| {
                wrap(Edit::DriftHopsOn(on))
            })),
            (friction.drift_hops > 0).then(|| {
                slider(hops_low..=hops_high, friction.drift_hops, move |hops| {
                    wrap(Edit::DriftHops(hops))
                })
                .style(style::fader)
                .into()
            }),
            match friction.drift_hops {
                0 => "OFF".to_owned(),
                hops => format!("{hops} HOPS"),
            },
        ),
    ]
    .spacing(16);
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

/// One limit on one line: its name, an on/off switch if it can be off, its slider while it's
/// on, and its value on a readout.
fn limit<'a>(
    name: &'a str,
    help: &'static str,
    switch: Option<Element<'a, Message>>,
    control: Option<Element<'a, Message>>,
    value: String,
) -> Element<'a, Message> {
    row![
        row![style::field_label(name), info(help)]
            .spacing(8)
            .align_y(Alignment::Center)
            .width(Length::Fixed(170.0)),
        container(switch.unwrap_or_else(|| space::horizontal().into())).width(Length::Fixed(52.0)),
        control.unwrap_or_else(|| space::horizontal().into()),
        style::readout(style::mono(value).size(13))
            .width(Length::Fixed(96.0))
            .align_x(Alignment::End),
    ]
    .spacing(16)
    .align_y(Alignment::Center)
    .into()
}

/// A square on/off switch.
fn switch<'a>(on: bool, on_toggle: impl Fn(bool) -> Message + 'a) -> Element<'a, Message> {
    toggler(on)
        .size(20)
        .style(style::switch)
        .on_toggle(on_toggle)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_a_cap_off_and_on_brings_back_its_default() {
        let mut friction = Friction::default();
        apply(&mut friction, Edit::TabCapOn(false));
        assert_eq!(friction.tab_cap, 0);
        apply(&mut friction, Edit::TabCapOn(true));
        assert_eq!(
            friction.tab_cap,
            Friction::default().tab_cap.max(Friction::TAB_CAP.0)
        );
        apply(&mut friction, Edit::TabCap(Friction::TAB_CAP.1));
        assert_eq!(friction.tab_cap, Friction::TAB_CAP.1);
        apply(&mut friction, Edit::DriftHopsOn(false));
        assert_eq!(friction.drift_hops, 0);
        apply(&mut friction, Edit::DriftHopsOn(true));
        assert!(friction.drift_hops >= Friction::DRIFT_HOPS.0);
        apply(&mut friction, Edit::Unlocks(0));
        assert_eq!(friction.unlocks_per_day, 0);
        assert!(friction.problems().is_empty(), "every setting is valid");
    }
}
