//! The parking lot on the Sessions page, and the system-wide shortcut that brings the window up
//! to park a thought without opening a new tab.

use focus_core::ports::HotkeyError;
use focus_core::{Hotkey, ParkedItem};
use iced::futures::SinkExt;
use iced::keyboard::{Key, key::Named};
use iced::widget::text_editor::{self, Binding};
use iced::widget::{Column, button, column, row, space, text};
use iced::{Alignment, Element, Subscription};
use ipc_proto::{SourceState, StatusReport};

use crate::help::info;
use crate::style::{self, Tone, caption, card, heading, pill, toned};
use crate::{App, Message};

/// The id of the box the shortcut focuses.
pub(crate) const INPUT: &str = "park-input";

/// How tall the parking box grows before it scrolls instead.
const MAX_HEIGHT: f32 = 200.0;

/// The most items listed; older ones are counted instead.
const SHOWN: usize = 8;

/// Listens for `hotkey` while the window runs. When the shortcut (or `attempt`) changes the
/// subscription does too: the old stream (and with it the registration) is dropped and a new one
/// registered, which is how a shortcut another app held is tried again.
pub(crate) fn hotkey_subscription(hotkey: Hotkey, attempt: u32) -> Subscription<Message> {
    Subscription::run_with((hotkey, attempt), listen)
}

/// The stream behind [`hotkey_subscription`]: registers, then forwards presses until dropped.
fn listen(key: &(Hotkey, u32)) -> impl iced::futures::Stream<Item = Message> + use<> {
    let hotkey = key.0;
    iced::stream::channel(4, async move |mut output| {
        let presses = output.clone();
        let registered = focus_hotkey::system_hotkey().listen(
            hotkey,
            Box::new(move || {
                // Called on the shortcut's own thread; a full queue just drops a press.
                let _ = presses.clone().try_send(Message::HotkeyPressed);
            }),
        );
        let problem = registered.as_ref().err().map(ToString::to_string);
        let retry = matches!(&registered, Err(HotkeyError::Taken { .. }));
        let _ = output
            .send(Message::HotkeyRegistered { problem, retry })
            .await;
        // Holding the guard keeps the shortcut registered until iced drops this stream.
        let _guard = registered;
        std::future::pending::<()>().await;
    })
}

impl App {
    /// The parking lot: a box to park a thought, what's parked, and how sending it goes.
    pub(crate) fn parking_card<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let shortcut = match status.parking_hotkey {
            Hotkey::Off => "Turn on a shortcut in Settings to park from any app.".into(),
            hotkey => format!("{hotkey} parks from any app."),
        };
        let park = (!self.park_input.text().trim().is_empty())
            .then_some(Message::Park)
            .and_then(|message| self.idle_then(message));
        let mut body: Column<'a, Message> = column![
            row![
                heading("Parking lot"),
                info(format!(
                    "Park stray thoughts instead of chasing them. They go to your Todoist inbox \
                     when connected. {shortcut}"
                )),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            // Grows downward as the thought gets longer. Enter parks it; Shift+Enter breaks
            // the line.
            row![
                iced::widget::text_editor(&self.park_input)
                    .id(INPUT)
                    .placeholder("Park a thought for later…")
                    .on_action(Message::ParkInput)
                    .max_height(MAX_HEIGHT)
                    // Long words and links break too, so the box grows instead of running off the
                    // side.
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                    .style(style::editor)
                    .key_binding({
                        let park = park.clone();
                        move |press| {
                            let enter = press.key == Key::Named(Named::Enter)
                                && !press.modifiers.shift()
                                && matches!(press.status, text_editor::Status::Focused { .. });
                            if enter {
                                park.clone().map(Binding::Custom)
                            } else {
                                Binding::from_key_press(press)
                            }
                        }
                    }),
                button("Park").style(style::key_accent).on_press_maybe(park),
            ]
            .spacing(10)
            .align_y(Alignment::Start),
        ]
        .spacing(12);
        if let Some(problem) = &self.hotkey_problem {
            body = body.push(toned(problem.as_str(), Tone::Warn).size(14));
        }
        body = body.push(sending_line(status));
        let mut list = column![].spacing(8);
        // Removing waits for an action on its way, so its answer isn't taken for the other's.
        let idle = !self.action_pending;
        for item in self.parked.iter().rev().take(SHOWN) {
            list = list.push(item_row(item, idle));
        }
        let hidden = self.parked.len().saturating_sub(SHOWN);
        if hidden > 0 {
            list = list.push(caption(format!("…and {hidden} older")));
        }
        body = body.push(list);
        if self.parked.iter().any(|item| item.sent_at.is_some()) {
            body = body.push(
                button(text("Clear what's in Todoist").size(14))
                    .style(style::key)
                    .on_press_maybe(self.idle_then(Message::ClearSentParked)),
            );
        }
        card(body).into()
    }
}

/// How sending to Todoist goes.
fn sending_line<'a>(status: &StatusReport) -> Element<'a, Message> {
    let lot = &status.parking_lot;
    let (line, tone) = match (lot.total, lot.waiting, &lot.inbox) {
        (0, _, _) => ("Nothing parked.".to_owned(), Tone::Quiet),
        (_, 0, _) if lot.refused > 0 => (
            format!(
                "{} not accepted by Todoist; everything else is in your inbox.",
                lot.refused
            ),
            Tone::Warn,
        ),
        (_, 0, _) => (
            "Everything parked is in your Todoist inbox.".to_owned(),
            Tone::Good,
        ),
        (_, waiting, SourceState::NotConfigured) => (
            format!("{waiting} kept here: connect Todoist on the Todoist page to send them."),
            Tone::Quiet,
        ),
        (_, waiting, SourceState::Failing { problem, .. }) => (
            format!("{waiting} waiting for Todoist ({problem}); Focus keeps trying."),
            Tone::Warn,
        ),
        (_, waiting, SourceState::Unknown | SourceState::Ok) => {
            (format!("{waiting} on the way to Todoist…"), Tone::Quiet)
        }
    };
    toned(line, tone).size(14).into()
}

/// One parked item: its text, link, where it is, and a button to remove it.
fn item_row(item: &ParkedItem, idle: bool) -> Element<'_, Message> {
    let mut words = column![text(item.text.as_str()).size(14)].spacing(2);
    if let Some(url) = item.url.as_deref().filter(|url| *url != item.text) {
        words = words.push(caption(url));
    }
    if let Some(reason) = &item.refused {
        words = words.push(toned(format!("{reason}; it stays here."), Tone::Warn).size(12));
    }
    let place = match (item.sent_at, &item.refused) {
        (Some(_), _) => pill("In Todoist", Tone::Good),
        (None, Some(_)) => pill("Not accepted", Tone::Warn),
        (None, None) => pill("Waiting", Tone::Quiet),
    };
    row![
        words,
        space::horizontal(),
        place,
        button(text("×").size(14))
            .style(style::key_flat)
            .on_press_maybe(idle.then_some(Message::Unpark(item.id))),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}
