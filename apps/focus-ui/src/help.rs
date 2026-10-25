//! Explanations that appear when hovering, so the window explains itself.

use iced::Element;
use iced::widget::{container, text, tooltip};

use crate::Message;

/// `content` with `explanation` shown in a box above it while the pointer is over it.
pub fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    explanation: impl text::IntoFragment<'a>,
) -> Element<'a, Message> {
    let bubble = container(text(explanation).size(13))
        .max_width(340)
        .padding(10)
        .style(container::rounded_box);
    tooltip(content, bubble, tooltip::Position::Top)
        .gap(6)
        .into()
}

/// A small "(?)" that explains the thing next to it on hover.
pub fn info<'a>(explanation: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    tip(text("(?)").size(13).style(crate::style::muted), explanation)
}
