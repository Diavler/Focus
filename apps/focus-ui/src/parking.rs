//! The parking lot on the Sessions page, and the system-wide shortcut that brings the window up
//! to park a thought without opening a new tab.

use focus_core::{Hotkey, ParkedItem};
use iced::futures::SinkExt;
use iced::widget::{Column, button, column, row, space, text, text_input};
use iced::{Alignment, Element, Subscription};
use ipc_proto::StatusReport;

use crate::help::info;
use crate::style::{Tone, caption, card, heading, pill, toned};
use crate::{App, Message};

/// The id of the box the shortcut focuses.
pub(crate) const INPUT: &str = "park-input";

/// The most items listed; older ones are counted instead.
const SHOWN: usize = 8;

/// Listens for `hotkey` while the window runs. When the shortcut changes the subscription does
/// too: the old stream (and with it the registration) is dropped and a new one registered.
pub(crate) fn hotkey_subscription(hotkey: Hotkey) -> Subscription<Message> {
    Subscription::run_with(hotkey, listen)
}

/// The stream behind [`hotkey_subscription`]: registers, then forwards presses until dropped.
fn listen(hotkey: &Hotkey) -> impl iced::futures::Stream<Item = Message> + use<> {
    let hotkey = *hotkey;
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
        let _ = output.send(Message::HotkeyRegistered(problem)).await;
        // Holding the guard keeps the shortcut registered until iced drops this stream.
        let _guard = registered;
        std::future::pending::<()>().await;
    })
}

impl App {
    /// The parking lot: a box to park a thought, what's parked, and how sending it goes.
    pub(crate) fn parking_card<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let shortcut = match status.parking_hotkey {
            Hotkey::Off => {
                "Turn on a shortcut under Settings → General to park from any app.".into()
            }
            hotkey => format!("Press {hotkey} from any app to park a thought here."),
        };
        let park = (!self.park_input.trim().is_empty())
            .then_some(Message::Park)
            .and_then(|message| self.idle_then(message));
        let mut body: Column<'a, Message> = column![
            row![
                heading("Parking lot"),
                info(
                    "Park stray thoughts and links instead of following them now. They go to \
                     your Todoist inbox once Todoist is connected and you're online; until then \
                     they wait here. The browser's Later button parks the page you're on."
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            caption(shortcut),
            row![
                text_input("Park a thought for later…", &self.park_input)
                    .id(INPUT)
                    .on_input(Message::ParkInput)
                    .on_submit_maybe(park.clone()),
                button("Park").style(button::primary).on_press_maybe(park),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(12);
        if let Some(problem) = &self.hotkey_problem {
            body = body.push(toned(problem.as_str(), Tone::Warn).size(14));
        }
        body = body.push(sending_line(status));
        let mut list = column![].spacing(8);
        for item in self.parked.iter().rev().take(SHOWN) {
            list = list.push(item_row(item));
        }
        let hidden = self.parked.len().saturating_sub(SHOWN);
        if hidden > 0 {
            list = list.push(caption(format!("…and {hidden} older")));
        }
        body = body.push(list);
        if self.parked.iter().any(|item| item.sent_at.is_some()) {
            body = body.push(
                button(text("Clear what's in Todoist").size(14))
                    .style(button::secondary)
                    .on_press_maybe(self.idle_then(Message::ClearSentParked)),
            );
        }
        card(body).into()
    }
}

/// How sending to Todoist goes.
fn sending_line<'a>(status: &StatusReport) -> Element<'a, Message> {
    let lot = &status.parking_lot;
    let (line, tone) = match (lot.total, lot.waiting, &lot.problem) {
        (0, _, _) => ("Nothing parked.".to_owned(), Tone::Quiet),
        (_, 0, _) => (
            "Everything parked is in your Todoist inbox.".to_owned(),
            Tone::Good,
        ),
        (_, waiting, Some(problem)) if problem == "not set up" => (
            format!("{waiting} kept here: connect Todoist on the Todoist page to send them."),
            Tone::Quiet,
        ),
        (_, waiting, Some(problem)) => (
            format!("{waiting} waiting for Todoist ({problem}); Focus keeps trying."),
            Tone::Warn,
        ),
        (_, waiting, None) => (format!("{waiting} on the way to Todoist…"), Tone::Quiet),
    };
    toned(line, tone).size(14).into()
}

/// One parked item: its text, link, where it is, and a button to remove it.
fn item_row(item: &ParkedItem) -> Element<'_, Message> {
    let mut words = column![text(item.text.as_str()).size(14)].spacing(2);
    if let Some(url) = item.url.as_deref().filter(|url| *url != item.text) {
        words = words.push(caption(url));
    }
    let place = match item.sent_at {
        Some(_) => pill("In Todoist", Tone::Good),
        None => pill("Waiting", Tone::Quiet),
    };
    row![
        words,
        space::horizontal(),
        place,
        button(text("×").size(14))
            .style(button::text)
            .on_press(Message::Unpark(item.id)),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}
