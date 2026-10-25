//! Minimal mode: while a session runs, the window shrinks to a small card that stays above other
//! windows (session, time left, what to work on, End early, Expand). It grows back when the
//! session ends or Expand is pressed.

use chrono::Local;
use focus_core::EndEarlyPolicy;
use iced::widget::{button, column, container, row, space, text};
use iced::window::{Level, Mode};
use iced::{Alignment, Element, Length, Point, Size, Task};
use ipc_proto::StatusReport;

use crate::sessions::format_duration;
use crate::style::{self, bold, caption};
use crate::{App, MIN_WINDOW, Message, agenda};

/// The card's size.
const CARD: Size = Size::new(340.0, 250.0);

/// The card's gap from the screen's top and right edges.
const MARGIN: f32 = 24.0;

impl App {
    /// The card shown instead of the whole window in minimal mode.
    pub(crate) fn compact_card<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let expand = button(text("Expand").size(13))
            .padding([6, 12])
            .style(style::key)
            .on_press(Message::Expand);
        let Some(running) = status.session.running() else {
            return container(column![caption("No session running."), expand].spacing(12))
                .padding(16)
                .into();
        };
        let spec = running.spec();
        let wait = match running.end_allowed_at() {
            Some(at) if at > status.now => format!(
                "end in {}",
                format_duration(at.saturating_duration_since(status.now))
            ),
            Some(_) => "ready to end".to_owned(),
            None => "left".to_owned(),
        };
        let time = style::readout(
            row![
                style::lcd(format_duration(running.time_left(status.now))).size(40),
                space::horizontal(),
                style::label(&wait),
            ]
            .align_y(Alignment::Center),
        )
        .padding([10, 14])
        .width(Length::Fill);
        let end: Element<'a, Message> = match spec.strictness.end_early_policy() {
            EndEarlyPolicy::Immediate => button(text("End session").size(14))
                .style(style::key_danger)
                .on_press_maybe(self.idle_then(Message::EndEarly))
                .into(),
            // Ending needs a reason typed after a wait, which the full window has room for.
            EndEarlyPolicy::WaitAndReason { .. } => button(text("End early").size(14))
                .style(style::key)
                .on_press_maybe(self.idle_then(Message::EndFromCard))
                .into(),
            EndEarlyPolicy::Forbidden => caption("Locked until it ends").into(),
        };
        let task = self
            .plan
            .as_ref()
            .and_then(|plan| agenda::current_focus(plan, &Local))
            .unwrap_or_else(|| "Nothing on the plan right now".to_owned());
        container(
            column![
                row![
                    bold(spec.config.name.as_str()).size(18),
                    space::horizontal(),
                    style::tag(&spec.strictness.to_string()),
                ]
                .align_y(Alignment::Center),
                time,
                caption(task),
                row![end, space::horizontal(), expand].align_y(Alignment::Center),
            ]
            .spacing(10),
        )
        .padding(16)
        .into()
    }
}

/// Shrinks the window to the card in the screen's top-right corner, above other windows. Reports
/// the window's size first, so it can grow back to it.
pub(crate) fn shrink_window() -> Task<Message> {
    iced::window::oldest().and_then(|id| {
        iced::window::size(id)
            .map(Message::FullSize)
            .chain(Task::batch([
                iced::window::set_mode(id, Mode::Windowed),
                iced::window::minimize(id, false),
                iced::window::set_min_size(id, None),
                iced::window::resize(id, CARD),
                iced::window::set_resizable(id, false),
                iced::window::set_level(id, Level::AlwaysOnTop),
                iced::window::monitor_size(id).then(move |monitor| match monitor {
                    Some(screen) => iced::window::move_to(
                        id,
                        Point::new(screen.width - CARD.width - MARGIN, MARGIN),
                    ),
                    None => Task::none(),
                }),
            ]))
    })
}

/// Grows the window back to `full` in the middle of the screen, as an ordinary window.
pub(crate) fn grow_window(full: Size) -> Task<Message> {
    iced::window::oldest().and_then(move |id| {
        Task::batch([
            iced::window::set_level(id, Level::Normal),
            iced::window::set_resizable(id, true),
            iced::window::resize(id, full),
            iced::window::set_min_size(id, Some(MIN_WINDOW)),
            iced::window::monitor_size(id).then(move |monitor| match monitor {
                Some(screen) => iced::window::move_to(
                    id,
                    Point::new(
                        ((screen.width - full.width) / 2.0).max(0.0),
                        ((screen.height - full.height) / 2.0).max(0.0),
                    ),
                ),
                None => Task::none(),
            }),
        ])
    })
}
