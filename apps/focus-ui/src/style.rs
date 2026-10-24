//! The window's look: Focus's own colour palette (a "night" and a "day" version, following the
//! system's light or dark mode), cards, the sidebar, headings and coloured status pills. Widgets
//! take their colours from the theme's palette, so both versions work everywhere.

use iced::gradient::Linear;
use iced::theme::{Mode, Palette};
use iced::widget::{Button, Container, Text, button, container, text};
use iced::{Background, Border, Color, Element, Font, Gradient, Length, Radians, Theme, font};

/// Focus's dark palette: deep navy with a violet accent, mint, amber and rose.
const NIGHT: Palette = Palette {
    background: Color::from_rgb8(0x12, 0x14, 0x20),
    text: Color::from_rgb8(0xE8, 0xE6, 0xF3),
    primary: Color::from_rgb8(0x8B, 0x7C, 0xF6),
    success: Color::from_rgb8(0x34, 0xD3, 0x99),
    warning: Color::from_rgb8(0xFB, 0xBF, 0x24),
    danger: Color::from_rgb8(0xFB, 0x71, 0x85),
};

/// Focus's light palette: the same accents, deeper so they read on a pale lavender background.
const DAY: Palette = Palette {
    background: Color::from_rgb8(0xF7, 0xF6, 0xFC),
    text: Color::from_rgb8(0x1D, 0x1A, 0x2E),
    primary: Color::from_rgb8(0x6D, 0x4A, 0xE8),
    success: Color::from_rgb8(0x10, 0x9F, 0x7A),
    warning: Color::from_rgb8(0xC2, 0x6A, 0x05),
    danger: Color::from_rgb8(0xE1, 0x1D, 0x48),
};

/// The second colour of the running session's gradient (the first is the accent).
const CYAN_NIGHT: Color = Color::from_rgb8(0x22, 0xD3, 0xEE);
const CYAN_DAY: Color = Color::from_rgb8(0x0E, 0xA5, 0xE9);

/// The theme for the system's `mode` (dark unless the system says light).
pub fn theme(mode: Mode) -> Theme {
    match mode {
        Mode::Light => Theme::custom("Focus Day", DAY),
        Mode::Dark | Mode::None => Theme::custom("Focus Night", NIGHT),
    }
}

use crate::Message;

/// How good or bad a status is, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Working as it should.
    Good,
    /// Worth a look, not broken.
    Warn,
    /// Needs fixing.
    Bad,
    /// Neutral information.
    Quiet,
}

/// Bold text.
pub fn bold<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).font(Font {
        weight: font::Weight::Bold,
        ..Font::DEFAULT
    })
}

/// A page title.
pub fn title<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    bold(content).size(28)
}

/// A heading inside a page or card.
pub fn heading<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    bold(content).size(17)
}

/// Small grey text for details.
pub fn caption<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(13).style(text::secondary)
}

/// `content` in a rounded card that fills the width.
pub fn card<'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content)
        .padding(18)
        .width(Length::Fill)
        .style(card_style)
}

fn card_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        text_color: Some(palette.background.weak.text),
        border: Border {
            radius: 12.0.into(),
            width: 1.0,
            color: palette.background.strong.color,
        },
        ..container::Style::default()
    }
}

/// A card that stands out, for the running session: a violet-to-cyan gradient.
pub fn hero<'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content)
        .padding(24)
        .width(Length::Fill)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            let cyan = if palette.is_dark {
                CYAN_NIGHT
            } else {
                CYAN_DAY
            };
            let gradient = Linear::new(Radians(2.2))
                .add_stop(0.0, palette.primary.strong.color)
                .add_stop(1.0, cyan);
            container::Style {
                background: Some(Background::Gradient(Gradient::Linear(gradient))),
                text_color: Some(Color::WHITE),
                border: Border {
                    radius: 16.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }
        })
}

/// The sidebar's background.
pub fn sidebar<'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content)
        .padding(18)
        .width(Length::Fixed(210.0))
        .height(Length::Fill)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            // A faint violet tint from top to bottom, so the sidebar reads as its own place.
            let tint = Color {
                a: if palette.is_dark { 0.16 } else { 0.10 },
                ..palette.primary.base.color
            };
            let gradient = Linear::new(Radians(std::f32::consts::PI))
                .add_stop(0.0, blend(palette.background.weaker.color, tint))
                .add_stop(1.0, palette.background.weaker.color);
            container::Style {
                background: Some(Background::Gradient(Gradient::Linear(gradient))),
                ..container::Style::default()
            }
        })
}

/// A sidebar entry, highlighted when `selected`.
pub fn nav<'a>(label: &'a str, selected: bool, on_press: Message) -> Button<'a, Message> {
    button(text(label).size(15))
        .width(Length::Fill)
        .padding([9, 12])
        .on_press(on_press)
        .style(move |theme: &Theme, status| {
            let palette = theme.extended_palette();
            let background = match (selected, status) {
                (true, _) => Some(palette.primary.base.color),
                (false, button::Status::Hovered | button::Status::Pressed) => {
                    Some(palette.background.strong.color)
                }
                _ => None,
            };
            button::Style {
                background: background.map(Background::Color),
                text_color: if selected {
                    palette.primary.base.text
                } else {
                    palette.background.base.text
                },
                border: Border {
                    radius: 10.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
}

/// A tab in a row of tabs, highlighted when `selected`.
pub fn tab<'a>(label: &'a str, selected: bool, on_press: Message) -> Button<'a, Message> {
    button(text(label).size(14))
        .padding([7, 14])
        .on_press(on_press)
        .style(move |theme: &Theme, status| {
            let palette = theme.extended_palette();
            let background = match (selected, status) {
                (true, _) => Some(palette.primary.base.color),
                (false, button::Status::Hovered | button::Status::Pressed) => {
                    Some(palette.background.strong.color)
                }
                _ => Some(palette.background.weak.color),
            };
            button::Style {
                background: background.map(Background::Color),
                text_color: if selected {
                    palette.primary.base.text
                } else {
                    palette.background.base.text
                },
                border: Border {
                    radius: 20.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
}

/// `tint` painted over `base` (which is opaque), as an opaque colour.
fn blend(base: Color, tint: Color) -> Color {
    let mix = |b: f32, t: f32| b + (t - b) * tint.a;
    Color::from_rgb(
        mix(base.r, tint.r),
        mix(base.g, tint.g),
        mix(base.b, tint.b),
    )
}

/// The app's name in the sidebar, in the accent colour.
pub fn brand<'a>() -> Text<'a> {
    bold("Focus").size(26).style(text::primary)
}

/// A small coloured label, e.g. "Synced" or "Offline".
pub fn pill<'a>(label: impl text::IntoFragment<'a>, tone: Tone) -> Element<'a, Message> {
    container(text(label).size(12))
        .padding([3, 10])
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            let pair = match tone {
                Tone::Good => palette.success.weak,
                Tone::Warn => palette.warning.weak,
                Tone::Bad => palette.danger.weak,
                Tone::Quiet => palette.background.strong,
            };
            container::Style {
                background: Some(Background::Color(pair.color)),
                text_color: Some(pair.text),
                border: Border {
                    radius: 10.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// A coloured dot for a status line.
pub fn dot<'a>(tone: Tone) -> Text<'a> {
    text("●").size(11).style(move |theme: &Theme| {
        let palette = theme.extended_palette();
        text::Style {
            color: Some(match tone {
                Tone::Good => palette.success.base.color,
                Tone::Warn => palette.warning.base.color,
                Tone::Bad => palette.danger.base.color,
                Tone::Quiet => palette.background.strongest.color,
            }),
        }
    })
}

/// Text coloured by `tone` (Quiet is the normal colour).
pub fn toned<'a>(content: impl text::IntoFragment<'a>, tone: Tone) -> Text<'a> {
    let label = text(content);
    match tone {
        Tone::Good => label.style(text::success),
        Tone::Warn => label.style(text::warning),
        Tone::Bad => label.style(text::danger),
        Tone::Quiet => label,
    }
}
