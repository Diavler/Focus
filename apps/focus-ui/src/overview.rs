//! The Overview page: today's calendar as an hour grid beside the tasks due today or overdue.

use iced::widget::{Column, button, column, row, space};
use iced::{Alignment, Element, Length};
use ipc_proto::{SourceState, StatusReport};

use crate::agenda::{self, Group};
use crate::help::info;
use crate::style::{self, caption, card, heading, title};
use crate::{App, Message, Page, timeline};

impl App {
    /// Today at a glance.
    pub(crate) fn overview_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let zone = &chrono::Local;
        let date = agenda::local(status.now, zone)
            .format("%A %-d %B")
            .to_string();
        let header = row![
            title("Overview"),
            info(
                "Today's calendar and tasks. Events in green start a session by themselves; \
                 the red line is now. The Calendar and Todoist pages show the week ahead."
            ),
            space::horizontal(),
            caption(date),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let calendar: Element<'a, Message> = match (&status.plan.calendar.state, &self.plan) {
            (SourceState::NotConfigured, _) => not_connected("Calendar", Page::Calendar),
            (_, None) => caption("Loading…").into(),
            (_, Some(plan)) => timeline::view(timeline::today(plan, status.now, zone)).into(),
        };
        let tasks: Element<'a, Message> = match (&status.plan.tasks.state, &self.plan) {
            (SourceState::NotConfigured, _) => not_connected("Todoist", Page::Todoist),
            (_, None) => caption("Loading…").into(),
            (_, Some(plan)) => due_today(agenda::overdue_and_today(plan, zone)),
        };
        column![
            header,
            row![
                card(column![heading("Calendar"), calendar].spacing(12))
                    .width(Length::FillPortion(3)),
                card(column![heading("Tasks"), tasks].spacing(12)).width(Length::FillPortion(2)),
            ]
            .spacing(18),
        ]
        .spacing(18)
        .into()
    }
}

/// Overdue tasks (if any), then today's.
fn due_today((overdue, today): (Group, Group)) -> Element<'static, Message> {
    let mut list: Column<'static, Message> = column![].spacing(14);
    for group in [overdue, today] {
        let empty = group.rows.is_empty();
        if empty && group.title == "Overdue" {
            continue;
        }
        let body: Element<'static, Message> = if empty {
            caption("Nothing due today.").into()
        } else {
            agenda::compact_rows_view(group.rows).into()
        };
        list = list.push(column![caption(group.title), body].spacing(6));
    }
    list.into()
}

/// Says the source isn't connected, with a button to its page.
fn not_connected(name: &'static str, page: Page) -> Element<'static, Message> {
    column![
        caption(format!("{name} isn't connected yet.")),
        button("Connect")
            .style(style::key_accent)
            .on_press(Message::Navigate(page)),
    ]
    .spacing(10)
    .into()
}
