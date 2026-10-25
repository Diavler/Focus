//! The Overview page: today's calendar as an hour grid beside the tasks due today or overdue.

use iced::widget::{Column, button, column, row, scrollable, space, text};
use iced::{Alignment, Element, Length};
use ipc_proto::{SourceState, StatusReport};

use crate::agenda::{self, Group};
use crate::help::info;
use crate::sessions::{format_duration, no_configurations};
use crate::style::{self, bold, caption, card, heading, title};
use crate::{App, Message, Page, timeline};

impl App {
    /// Today at a glance.
    pub(crate) fn overview_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let zone = &chrono::Local;
        let date = agenda::local(status.now, zone)
            .format("%A %-d %B")
            .to_string();
        let header = row![
            title("Today"),
            info("Today at a glance. Green events start sessions; the red line is now."),
            space::horizontal(),
            caption(date),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        // The tasks card matches the calendar's height (scrolling if it's longer), but only
        // beside the hour grid: next to a short card it would be squeezed.
        let mut grid = false;
        let calendar: Element<'a, Message> = match (&status.plan.calendar.state, &self.plan) {
            (SourceState::NotConfigured, _) => not_connected("Calendar", Page::Calendar),
            (_, None) => caption("Loading…").into(),
            (_, Some(plan)) => {
                let today = timeline::today(plan, status.now, zone);
                if today.is_empty() {
                    caption("Nothing on your calendar today.").into()
                } else {
                    grid = true;
                    timeline::view(today).into()
                }
            }
        };
        let tasks: Element<'a, Message> = match (&status.plan.tasks.state, &self.plan) {
            (SourceState::NotConfigured, _) => not_connected("Todoist", Page::Todoist),
            (_, None) => caption("Loading…").into(),
            (_, Some(plan)) => due_today(agenda::overdue_and_today(plan, zone)),
        };
        let tasks_card = if grid {
            card(
                column![
                    heading("Today's tasks"),
                    scrollable(tasks)
                        .style(style::scrollbar)
                        .height(Length::Fill)
                ]
                .spacing(12),
            )
            .height(Length::Fill)
        } else {
            card(column![heading("Today's tasks"), tasks].spacing(12))
        };
        column![
            header,
            self.now_strip(status),
            row![
                card(column![heading("Today's calendar"), calendar].spacing(12))
                    .width(Length::FillPortion(3)),
                tasks_card.width(Length::FillPortion(2)),
            ]
            .spacing(18),
        ]
        .spacing(18)
        .into()
    }

    /// What's happening now: the running session with its time left, or a session ready to
    /// start with the Sessions page's choices.
    fn now_strip<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let content: Element<'a, Message> = match (status.session.running(), &self.selected) {
            (Some(running), _) => row![
                column![
                    style::label("Running"),
                    bold(running.spec().config.name.as_str()).size(18)
                ]
                .spacing(2),
                space::horizontal(),
                style::readout(
                    style::mono(format_duration(running.time_left(status.now))).size(20)
                )
                .padding([6, 12]),
                button(text("Shrink to timer").size(14))
                    .style(style::key)
                    .on_press(Message::Shrink),
            ]
            .spacing(14)
            .align_y(Alignment::Center)
            .into(),
            (None, Some(config)) => row![
                column![
                    style::label("Ready to focus"),
                    text(format!(
                        "{config} · {} min · {}",
                        self.minutes, self.strictness
                    ))
                    .size(16),
                ]
                .spacing(2),
                space::horizontal(),
                button(text("Change").size(14))
                    .style(style::key)
                    .on_press(Message::Navigate(Page::Sessions)),
                button(text("Start session").size(15))
                    .padding([10, 20])
                    .style(style::key_accent)
                    .on_press_maybe(self.idle_then(Message::Start)),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            (None, None) => no_configurations(),
        };
        card(content).into()
    }
}

/// Overdue tasks (if any), then today's. The card's heading already says today, so today's
/// tasks only get their own caption to set them apart from overdue ones.
fn due_today((overdue, today): (Group, Group)) -> Element<'static, Message> {
    let mut list: Column<'static, Message> = column![].spacing(14);
    let any_overdue = !overdue.rows.is_empty();
    if any_overdue {
        list = list.push(
            column![
                caption(overdue.title),
                agenda::compact_rows_view(overdue.rows)
            ]
            .spacing(6),
        );
    }
    let body: Element<'static, Message> = if today.rows.is_empty() {
        caption("Nothing due today.").into()
    } else {
        agenda::compact_rows_view(today.rows).into()
    };
    list = list.push(if any_overdue {
        column![caption(today.title), body].spacing(6)
    } else {
        column![body]
    });
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
