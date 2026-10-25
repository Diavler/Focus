//! The window's look, an instrument panel: keys with a ledge that lift on hover and sink when
//! pressed, raised panels, dark readouts, square switches and LED status dots. It comes in a warm
//! light "field recorder" version and a warm dark "bakelite" one, following the system's light or
//! dark mode. Widgets take their colours from the theme, so both versions work everywhere.

use std::f32::consts::PI;

use iced::gradient::Linear;
use iced::theme::{Mode, Palette};
use iced::widget::{
    Button, Container, Text, button, container, row, rule, space, stack, text, toggler,
};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Gradient, Length, Padding, Radians,
    Shadow, Theme, Vector, font,
};

use crate::Message;

/// The text font (bundled, so the window looks the same on every machine).
pub const SANS: Font = Font::with_name("IBM Plex Sans");

/// The font of readouts.
const MONO: Font = Font {
    weight: font::Weight::Medium,
    ..Font::with_name("IBM Plex Mono")
};

/// The bundled font files, loaded at start (IBM Plex, SIL Open Font License; see `fonts/OFL.txt`).
pub const FONTS: [&[u8]; 3] = [
    include_bytes!("../fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../fonts/IBMPlexMono-Medium.ttf"),
];

/// The light palette ("field recorder"): warm aluminium with a burnt-orange accent.
const FIELD: Palette = Palette {
    background: Color::from_rgb8(0xE6, 0xE0, 0xD5),
    text: Color::from_rgb8(0x2A, 0x25, 0x20),
    primary: Color::from_rgb8(0xB4, 0x41, 0x0C),
    success: Color::from_rgb8(0x27, 0x6B, 0x32),
    warning: Color::from_rgb8(0x7A, 0x55, 0x00),
    danger: Color::from_rgb8(0xA6, 0x1B, 0x29),
};

/// The dark palette ("bakelite"): warm brown-black with an amber accent.
const BAKELITE: Palette = Palette {
    background: Color::from_rgb8(0x1B, 0x16, 0x12),
    text: Color::from_rgb8(0xF1, 0xE6, 0xD6),
    primary: Color::from_rgb8(0xF2, 0xA3, 0x3A),
    success: Color::from_rgb8(0x8F, 0xCB, 0x7F),
    warning: Color::from_rgb8(0xE8, 0xC5, 0x47),
    danger: Color::from_rgb8(0xF0, 0x7A, 0x63),
};

/// The colours a palette doesn't derive: key faces, ledges, panels and readouts.
struct Look {
    /// Top and bottom of a key's face, and its edge and ledge.
    key: (Color, Color),
    key_edge: Color,
    ledge: Color,
    /// The same when the pointer is over the key.
    hover: (Color, Color),
    hover_edge: Color,
    hover_ledge: Color,
    /// A pressed (or selected) key's face, and the shade at its top that makes it look sunk.
    pressed: Color,
    shade: Color,
    /// A key that can't be pressed: face and label.
    disabled: Color,
    disabled_text: Color,
    /// Top and bottom of a raised panel, its edge and the shadow under it.
    panel: (Color, Color),
    panel_edge: Color,
    panel_shadow: Color,
    /// The sidebar's background.
    sidebar: Color,
    /// A readout's glass, its edge and its digits.
    glass: Color,
    glass_edge: Color,
    digits: Color,
    /// A switch's track, and an LED that's off.
    well: Color,
    unlit: Color,
    /// Whether LEDs glow (they only show it on a dark panel).
    glow: bool,
}

/// What `FIELD` needs besides its palette.
const FIELD_LOOK: Look = Look {
    key: (
        Color::from_rgb8(0xFB, 0xF8, 0xF2),
        Color::from_rgb8(0xED, 0xE7, 0xDC),
    ),
    key_edge: Color::from_rgb8(0xC9, 0xBF, 0xAE),
    ledge: Color::from_rgb8(0xBF, 0xB5, 0xA3),
    hover: (Color::WHITE, Color::from_rgb8(0xF4, 0xEF, 0xE6)),
    hover_edge: Color::from_rgb8(0xB0, 0xA5, 0x92),
    hover_ledge: Color::from_rgb8(0xB0, 0xA5, 0x92),
    pressed: Color::from_rgb8(0xE3, 0xDC, 0xCF),
    shade: Color::from_rgba8(0x50, 0x3C, 0x1E, 0.18),
    disabled: Color::from_rgb8(0xE6, 0xE0, 0xD5),
    disabled_text: Color::from_rgb8(0x7A, 0x70, 0x64),
    panel: (
        Color::from_rgb8(0xF8, 0xF5, 0xEF),
        Color::from_rgb8(0xF4, 0xF0, 0xE8),
    ),
    panel_edge: Color::from_rgb8(0xCF, 0xC6, 0xB6),
    panel_shadow: Color::from_rgba8(0x50, 0x3C, 0x1E, 0.12),
    sidebar: Color::from_rgb8(0xDC, 0xD5, 0xC8),
    glass: Color::from_rgb8(0x2B, 0x2A, 0x26),
    glass_edge: Color::from_rgb8(0x1F, 0x1E, 0x1B),
    digits: Color::from_rgb8(0xFF, 0x8A, 0x3D),
    well: Color::from_rgb8(0xD6, 0xCE, 0xC0),
    unlit: Color::from_rgb8(0xB9, 0xAF, 0x9F),
    glow: false,
};

/// What `BAKELITE` needs besides its palette.
const BAKELITE_LOOK: Look = Look {
    key: (
        Color::from_rgb8(0x3A, 0x30, 0x28),
        Color::from_rgb8(0x2C, 0x24, 0x1D),
    ),
    key_edge: Color::from_rgb8(0x4A, 0x3D, 0x31),
    ledge: Color::from_rgb8(0x0D, 0x0A, 0x08),
    hover: (
        Color::from_rgb8(0x46, 0x39, 0x2E),
        Color::from_rgb8(0x36, 0x2C, 0x23),
    ),
    hover_edge: Color::from_rgb8(0x5A, 0x4B, 0x3D),
    hover_ledge: Color::from_rgb8(0x0D, 0x0A, 0x08),
    pressed: Color::from_rgb8(0x22, 0x1B, 0x15),
    shade: Color::from_rgba8(0, 0, 0, 0.55),
    disabled: Color::from_rgb8(0x24, 0x1D, 0x17),
    disabled_text: Color::from_rgb8(0x8C, 0x7D, 0x6B),
    panel: (
        Color::from_rgb8(0x2C, 0x24, 0x1D),
        Color::from_rgb8(0x26, 0x1F, 0x19),
    ),
    panel_edge: Color::from_rgb8(0x3A, 0x30, 0x28),
    panel_shadow: Color::from_rgba8(0, 0, 0, 0.45),
    sidebar: Color::from_rgb8(0x1F, 0x19, 0x14),
    glass: Color::from_rgb8(0x12, 0x0E, 0x0B),
    glass_edge: Color::from_rgb8(0x2E, 0x26, 0x1F),
    digits: Color::from_rgb8(0xF2, 0xA3, 0x3A),
    well: Color::from_rgb8(0x12, 0x0E, 0x0B),
    unlit: Color::from_rgb8(0x4A, 0x3D, 0x31),
    glow: true,
};

/// The theme for the system's `mode` (dark unless the system says light).
pub fn theme(mode: Mode) -> Theme {
    match mode {
        Mode::Light => Theme::custom("Focus Field Recorder", FIELD),
        Mode::Dark | Mode::None => Theme::custom("Focus Bakelite", BAKELITE),
    }
}

/// The extra colours for `theme`.
fn look(theme: &Theme) -> &'static Look {
    if theme.extended_palette().is_dark {
        &BAKELITE_LOOK
    } else {
        &FIELD_LOOK
    }
}

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
        weight: font::Weight::Semibold,
        ..SANS
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

/// Text in the readout font, for times and counts on a [`readout`].
pub fn mono<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).font(MONO)
}

/// `content` on a raised panel that fills the width.
pub fn card<'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content)
        .padding(18)
        .width(Length::Fill)
        .style(card_style)
}

fn card_style(theme: &Theme) -> container::Style {
    let look = look(theme);
    container::Style {
        background: Some(vertical(look.panel.0, look.panel.1)),
        text_color: Some(theme.extended_palette().background.base.text),
        border: Border {
            radius: 12.0.into(),
            width: 1.0,
            color: look.panel_edge,
        },
        shadow: Shadow {
            color: look.panel_shadow,
            offset: Vector::new(0.0, 6.0),
            blur_radius: 14.0,
        },
        ..container::Style::default()
    }
}

/// The panel for the running session: a card with more room.
pub fn hero<'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    card(content).padding(24)
}

/// `content` behind the dark glass of a readout, in its glowing digit colour.
pub fn readout<'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content).padding([10, 12]).style(|theme: &Theme| {
        let look = look(theme);
        container::Style {
            background: Some(sunk(look.glass, look.shade)),
            text_color: Some(look.digits),
            border: Border {
                radius: 6.0.into(),
                width: 1.0,
                color: look.glass_edge,
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
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(look(theme).sidebar)),
            ..container::Style::default()
        })
}

/// A sidebar entry: a key that stays down, with its LED lit, while its page is `selected`.
pub fn nav<'a>(label: &'a str, selected: bool, on_press: Message) -> Button<'a, Message> {
    let mut content = row![text(label).size(15), space::horizontal()].align_y(Alignment::Center);
    if selected {
        content = content.push(led(|theme| theme.extended_palette().primary.base.color));
    }
    button(content)
        .width(Length::Fill)
        .padding([10, 12])
        .on_press(on_press)
        .style(move |theme: &Theme, status| key_style(theme, status, Face::Plain, selected))
}

/// A tab in a row of tabs: lit in the accent and held down when `selected`.
pub fn tab<'a>(label: &'a str, selected: bool, on_press: Message) -> Button<'a, Message> {
    let face = if selected { Face::Accent } else { Face::Plain };
    button(text(label).size(14))
        .padding([7, 14])
        .on_press(on_press)
        .style(move |theme: &Theme, status| key_style(theme, status, face, selected))
}

/// An ordinary key: lifts on hover, sinks when pressed.
pub fn key(theme: &Theme, status: button::Status) -> button::Style {
    key_style(theme, status, Face::Plain, false)
}

/// A key in the accent colour, for the main action on a page or card.
pub fn key_accent(theme: &Theme, status: button::Status) -> button::Style {
    key_style(theme, status, Face::Accent, false)
}

/// A red key, for actions that end, stop or delete something.
pub fn key_danger(theme: &Theme, status: button::Status) -> button::Style {
    key_style(theme, status, Face::Danger, false)
}

/// A key with no face, like a link: only its label shows until the pointer is over it.
pub fn key_flat(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let background = match status {
        button::Status::Hovered => Some(Background::Color(look(theme).hover.1)),
        button::Status::Pressed => Some(Background::Color(look(theme).pressed)),
        button::Status::Active | button::Status::Disabled => None,
    };
    button::Style {
        background,
        text_color: if status == button::Status::Disabled {
            look(theme).disabled_text
        } else {
            palette.background.base.text
        },
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// The colour of a key's face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    Plain,
    Accent,
    Danger,
}

/// A key's look for `status`; `held` keys look pressed whatever the pointer does.
fn key_style(theme: &Theme, status: button::Status, face: Face, held: bool) -> button::Style {
    let look = look(theme);
    let palette = theme.extended_palette();
    let (rest, hover, edge, hover_edge, ledge, hover_ledge, pressed, label) = match face {
        Face::Plain => (
            look.key,
            look.hover,
            look.key_edge,
            look.hover_edge,
            look.ledge,
            look.hover_ledge,
            look.pressed,
            palette.background.base.text,
        ),
        Face::Accent | Face::Danger => {
            let pair = if face == Face::Accent {
                palette.primary.base
            } else {
                palette.danger.base
            };
            let lit = blend(pair.color, Color::WHITE.scale_alpha(0.12));
            let dark = darken(pair.color);
            (
                (pair.color, pair.color),
                (lit, pair.color),
                dark,
                dark,
                dark,
                dark,
                pair.color,
                pair.text,
            )
        }
    };
    let status = if held && status != button::Status::Disabled {
        button::Status::Pressed
    } else {
        status
    };
    let (background, border, ledge, height) = match status {
        button::Status::Active => (vertical(rest.0, rest.1), edge, ledge, 3.0),
        button::Status::Hovered => (vertical(hover.0, hover.1), hover_edge, hover_ledge, 4.0),
        button::Status::Pressed => (sunk(pressed, look.shade), edge, Color::TRANSPARENT, 0.0),
        button::Status::Disabled => (
            Background::Color(look.disabled),
            look.key_edge,
            Color::TRANSPARENT,
            0.0,
        ),
    };
    button::Style {
        background: Some(background),
        text_color: if status == button::Status::Disabled {
            look.disabled_text
        } else {
            label
        },
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: border,
        },
        // The ledge: a hard shadow straight down, taller as the key lifts, gone once it's down.
        shadow: Shadow {
            color: ledge,
            offset: Vector::new(0.0, height),
            blur_radius: 0.0,
        },
        ..button::Style::default()
    }
}

/// A square on/off switch: the knob slides right and lights in the accent when on.
pub fn switch(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let look = look(theme);
    let palette = theme.extended_palette();
    let (on, hovered, disabled) = match status {
        toggler::Status::Active { is_toggled } => (is_toggled, false, false),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true, false),
        toggler::Status::Disabled { is_toggled } => (is_toggled, false, true),
    };
    let knob = if on {
        palette.primary.base.color
    } else {
        look.key.0
    };
    let alpha = if disabled { 0.5 } else { 1.0 };
    toggler::Style {
        background: sunk(look.well, look.shade),
        background_border_width: 1.0,
        background_border_color: if hovered {
            look.hover_edge
        } else {
            look.key_edge
        },
        foreground: Background::Color(knob.scale_alpha(alpha)),
        foreground_border_width: if on { 0.0 } else { 1.0 },
        foreground_border_color: look.key_edge,
        text_color: None,
        border_radius: Some(4.0.into()),
        padding_ratio: 0.12,
    }
}

/// A gradient from `top` to `bottom`.
fn vertical(top: Color, bottom: Color) -> Background {
    // In iced, an angle of π runs from the top edge down.
    Background::Gradient(Gradient::Linear(
        Linear::new(Radians(PI))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom),
    ))
}

/// `face` with `shade` along its top edge, so it looks sunk into the panel (iced has no inset
/// shadows, so a short gradient stands in for one).
fn sunk(face: Color, shade: Color) -> Background {
    Background::Gradient(Gradient::Linear(
        Linear::new(Radians(PI))
            .add_stop(0.0, blend(face, shade))
            .add_stop(0.3, face)
            .add_stop(1.0, face),
    ))
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

/// `color` at 60% brightness, for the ledge under a coloured key.
fn darken(color: Color) -> Color {
    Color::from_rgb(color.r * 0.6, color.g * 0.6, color.b * 0.6)
}

/// The app's name in the sidebar.
pub fn brand<'a>() -> Text<'a> {
    bold("Focus").size(22)
}

/// A small label outlined in its tone, e.g. "Synced" or "Offline".
pub fn pill<'a>(label: impl text::IntoFragment<'a>, tone: Tone) -> Element<'a, Message> {
    container(text(label).size(12))
        .padding([2, 8])
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            let color = match tone {
                Tone::Quiet => palette.background.base.text.scale_alpha(0.7),
                _ => tone_color(theme, tone),
            };
            container::Style {
                background: Some(Background::Color(color.scale_alpha(0.10))),
                text_color: Some(color),
                border: Border {
                    radius: 4.0.into(),
                    width: 1.0,
                    color,
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// An LED for a status line, unlit for Quiet.
pub fn dot<'a>(tone: Tone) -> Container<'a, Message> {
    led(move |theme| match tone {
        Tone::Quiet => look(theme).unlit,
        _ => tone_color(theme, tone),
    })
}

/// A round LED in the colour `color` picks from the theme, glowing on a dark panel.
fn led<'a>(color: impl Fn(&Theme) -> Color + 'a) -> Container<'a, Message> {
    container(space::horizontal().width(Length::Fixed(8.0)))
        .height(Length::Fixed(8.0))
        .style(move |theme: &Theme| {
            let lit = color(theme);
            container::Style {
                background: Some(Background::Color(lit)),
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                shadow: if look(theme).glow && lit != look(theme).unlit {
                    Shadow {
                        color: lit.scale_alpha(0.7),
                        offset: Vector::ZERO,
                        blur_radius: 6.0,
                    }
                } else {
                    Shadow::default()
                },
                ..container::Style::default()
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

/// One event or task in a box `height` tall (`Shrink` to fit its content) with a coloured bar
/// down its left side (the accent for Quiet), faded once it's over.
pub fn entry<'a>(
    content: impl Into<Element<'a, Message>>,
    tone: Tone,
    faded: bool,
    height: Length,
) -> Container<'a, Message> {
    let bar = container(space::horizontal().width(Length::Fixed(4.0)))
        .height(Length::Fill)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(fade(tone_color(theme, tone), faded, 0.4))),
            ..container::Style::default()
        });
    let body = container(content)
        .padding(Padding {
            top: 7.0,
            right: 12.0,
            bottom: 7.0,
            left: 16.0,
        })
        .width(Length::Fill)
        .height(height);
    // A stack takes its size from its first layer, so the box fits (or fills) as the content
    // does and the Fill-height bar laid over it is exactly as tall. In a row, the bar would make
    // the box Fill-height, or get no height at all.
    container(stack![body, bar])
        .width(Length::Fill)
        .height(height)
        .clip(true)
        .style(move |theme: &Theme| {
            let tint = Color {
                a: if faded { 0.05 } else { 0.14 },
                ..tone_color(theme, tone)
            };
            container::Style {
                background: Some(Background::Color(blend(look(theme).panel.1, tint))),
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }
        })
}

/// The line across the day's grid at the current time.
pub fn now_rule(theme: &Theme) -> rule::Style {
    rule::Style {
        color: theme.extended_palette().danger.base.color,
        ..rule::default(theme)
    }
}

/// The strong colour of `tone`, with the accent standing in for Quiet.
fn tone_color(theme: &Theme, tone: Tone) -> Color {
    let palette = theme.extended_palette();
    match tone {
        Tone::Good => palette.success.base.color,
        Tone::Warn => palette.warning.base.color,
        Tone::Bad => palette.danger.base.color,
        Tone::Quiet => palette.primary.base.color,
    }
}

/// `color` at `alpha` if `faded`, else as it is.
fn fade(color: Color, faded: bool, alpha: f32) -> Color {
    if faded {
        Color { a: alpha, ..color }
    } else {
        color
    }
}
