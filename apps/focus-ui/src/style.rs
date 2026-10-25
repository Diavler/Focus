//! The window's look, an instrument panel: keys with a ledge that lift on hover and sink when
//! pressed, raised panels, dark readouts, square switches and LED status dots. It comes in a warm
//! light "field recorder" version and a warm dark "bakelite" one, following the system's light or
//! dark mode. Widgets take their colours from the theme, so both versions work everywhere.

use std::f32::consts::PI;

use iced::gradient::Linear;
use iced::overlay::menu;
use iced::theme::{Mode, Palette};
use iced::widget::{
    Button, Container, Text, TextInput, button, container, pick_list, row, rule, scrollable,
    slider, space, stack, text, text_editor, text_input, toggler,
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
    /// A text box's well.
    inset: Color,
    /// A switch's track, and an LED that's off.
    well: Color,
    unlit: Color,
    /// Whether LEDs glow (they only show it on a dark panel).
    glow: bool,
    /// Secondary text: hints, labels, details. Dimmer than the text, still easy to read.
    muted: Color,
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
    inset: Color::from_rgb8(0xFD, 0xFB, 0xF7),
    well: Color::from_rgb8(0xD6, 0xCE, 0xC0),
    unlit: Color::from_rgb8(0xB9, 0xAF, 0x9F),
    glow: false,
    muted: Color::from_rgb8(0x5A, 0x51, 0x47),
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
    inset: Color::from_rgb8(0x1A, 0x15, 0x11),
    well: Color::from_rgb8(0x12, 0x0E, 0x0B),
    unlit: Color::from_rgb8(0x4A, 0x3D, 0x31),
    glow: true,
    muted: Color::from_rgb8(0xB5, 0xA5, 0x8F),
};

/// The mode the window shows: the `chosen` one, or the `system`'s when following it.
pub fn mode_for(chosen: focus_core::Theme, system: Mode) -> Mode {
    match chosen {
        focus_core::Theme::System => system,
        focus_core::Theme::Light => Mode::Light,
        focus_core::Theme::Dark => Mode::Dark,
    }
}

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

/// A card's heading: a [`label`], like the legend over a section of the panel.
pub fn heading<'a>(content: impl AsRef<str>) -> Text<'a> {
    label(content.as_ref())
}

/// Secondary text for details and hints: dimmer than the text, but easy to read.
pub fn caption<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(14).style(muted)
}

/// The colour of secondary text.
pub fn muted(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(look(theme).muted),
    }
}

/// Text in the readout font, for times and counts on a [`readout`].
pub fn mono<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).font(MONO)
}

/// A field's label: plain text at full contrast, for forms.
pub fn field_label<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(14)
}

/// A section label: small grey capitals in the readout font, like the legend on a panel.
pub fn label<'a>(content: &str) -> Text<'a> {
    text(content.to_uppercase())
        .font(MONO)
        .size(11)
        .style(muted)
}

/// Big readout digits the user can type into, on a [`readout`]'s glass.
pub fn lcd_input<'a>(value: &str) -> TextInput<'a, Message> {
    text_input("", value)
        .font(MONO)
        .size(54)
        .padding([0, 4])
        .width(Length::Fixed(240.0))
        .style(|theme: &Theme, status| {
            let look = look(theme);
            let focused = matches!(status, text_input::Status::Focused { .. });
            text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border {
                    radius: 4.0.into(),
                    width: if focused { 1.0 } else { 0.0 },
                    color: look.digits.scale_alpha(0.5),
                },
                icon: look.digits,
                placeholder: look.unlit,
                value: look.digits,
                selection: look.digits.scale_alpha(0.3),
            }
        })
}

/// Readout text in the glowing digit colour, for text on glass outside a [`readout`].
pub fn lcd<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    mono(content).style(|theme: &Theme| text::Style {
        color: Some(look(theme).digits),
    })
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

/// The height of a [`segmented`] key. Fixed, so the dividers between keys can match it (a
/// divider that fills would stretch to the height of a scrolling page).
const SEGMENT_HEIGHT: f32 = 36.0;

/// Keys joined in one bezel for picking one of `options`: the picked one is held down and lit.
pub fn segmented<'a, T: Copy + PartialEq>(
    options: &[T],
    picked: T,
    name: impl Fn(T) -> String,
    on_pick: impl Fn(T) -> Message,
) -> Element<'a, Message> {
    let last = options.len().saturating_sub(1);
    let keys = options
        .iter()
        .enumerate()
        .fold(row![], |keys, (index, &option)| {
            let keys = if index == 0 {
                keys
            } else {
                keys.push(segment_divider())
            };
            let held = option == picked;
            // Only the outer corners are round, inside the bezel's 1px edge.
            let radius = iced::border::Radius {
                top_left: if index == 0 { 7.0 } else { 0.0 },
                bottom_left: if index == 0 { 7.0 } else { 0.0 },
                top_right: if index == last { 7.0 } else { 0.0 },
                bottom_right: if index == last { 7.0 } else { 0.0 },
            };
            keys.push(
                button(
                    text(name(option))
                        .size(14)
                        .height(Length::Fill)
                        .align_y(Alignment::Center),
                )
                .height(Length::Fixed(SEGMENT_HEIGHT))
                .padding([0, 18])
                .on_press(on_pick(option))
                .style(move |theme: &Theme, status| segment_style(theme, status, held, radius)),
            )
        });
    container(keys)
        .padding(1)
        .style(|theme: &Theme| {
            let look = look(theme);
            container::Style {
                border: Border {
                    radius: 8.0.into(),
                    width: 1.0,
                    color: look.key_edge,
                },
                // The whole row of keys stands on one ledge.
                shadow: Shadow {
                    color: look.ledge,
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 0.0,
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// The line between two keys of a [`segmented`] control.
fn segment_divider<'a>() -> Element<'a, Message> {
    container(space::horizontal().width(Length::Fixed(1.0)))
        .height(Length::Fixed(SEGMENT_HEIGHT))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(look(theme).key_edge)),
            ..container::Style::default()
        })
        .into()
}

/// One key of a [`segmented`] control: muted until the pointer is over it, lit when `held`.
fn segment_style(
    theme: &Theme,
    status: button::Status,
    held: bool,
    radius: iced::border::Radius,
) -> button::Style {
    let look = look(theme);
    let palette = theme.extended_palette();
    let (background, text_color) = if held {
        (
            sunk(palette.primary.base.color, look.shade),
            palette.primary.base.text,
        )
    } else {
        match status {
            button::Status::Hovered => (
                vertical(look.hover.0, look.hover.1),
                palette.background.base.text,
            ),
            button::Status::Pressed => {
                (sunk(look.pressed, look.shade), palette.background.base.text)
            }
            button::Status::Active | button::Status::Disabled => (
                vertical(look.key.0, look.key.1),
                palette.background.base.text.scale_alpha(0.72),
            ),
        }
    };
    button::Style {
        background: Some(background),
        text_color,
        border: Border {
            radius,
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// A small tag outlined in the accent, in the readout font, like "REC" on a recorder.
pub fn tag<'a>(content: &str) -> Element<'a, Message> {
    container(mono(content.to_uppercase()).size(11))
        .padding([4, 8])
        .style(|theme: &Theme| {
            let accent = theme.extended_palette().primary.base.color;
            container::Style {
                text_color: Some(accent),
                border: Border {
                    radius: 4.0.into(),
                    width: 1.0,
                    color: accent,
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// The number of cells in a [`meter`].
const METER_CELLS: usize = 30;

/// A bar of LCD cells on readout glass, lit for `fraction` (0 to 1) of the way.
pub fn meter<'a>(fraction: f32) -> Element<'a, Message> {
    let lit = lit_cells(fraction, METER_CELLS);
    let cells = (0..METER_CELLS).fold(row![].spacing(3), |cells, index| {
        cells.push(
            container(space::horizontal().width(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fixed(10.0))
                .style(move |theme: &Theme| {
                    let look = look(theme);
                    let color = if index < lit {
                        look.digits
                    } else {
                        blend(look.glass, look.digits.scale_alpha(0.14))
                    };
                    container::Style {
                        background: Some(Background::Color(color)),
                        border: Border {
                            radius: 1.0.into(),
                            ..Border::default()
                        },
                        ..container::Style::default()
                    }
                }),
        )
    });
    readout(cells).padding([8, 10]).width(Length::Fill).into()
}

/// How many of `cells` are lit for `fraction`, rounding up so any progress lights one.
fn lit_cells(fraction: f32, cells: usize) -> usize {
    // The cast can't overflow or go negative: the value is clamped to 0..=cells first (and a
    // NaN fraction casts to 0).
    (fraction.clamp(0.0, 1.0) * cells as f32).ceil() as usize
}

/// A text box: a well sunk into the panel, rimmed in the accent while it has the cursor.
pub fn field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let look = look(theme);
    let palette = theme.extended_palette();
    let edge = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        text_input::Status::Hovered => look.hover_edge,
        text_input::Status::Active | text_input::Status::Disabled => look.key_edge,
    };
    text_input::Style {
        background: if status == text_input::Status::Disabled {
            Background::Color(look.disabled)
        } else {
            sunk(look.inset, look.shade)
        },
        border: Border {
            radius: 6.0.into(),
            width: 1.0,
            color: edge,
        },
        icon: look.disabled_text,
        placeholder: look.disabled_text,
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.3),
    }
}

/// A box for longer text: looks like a [`field`].
pub fn editor(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let field = field(
        theme,
        match status {
            text_editor::Status::Active => text_input::Status::Active,
            text_editor::Status::Hovered => text_input::Status::Hovered,
            text_editor::Status::Focused { is_hovered } => {
                text_input::Status::Focused { is_hovered }
            }
            text_editor::Status::Disabled => text_input::Status::Disabled,
        },
    );
    text_editor::Style {
        background: field.background,
        border: field.border,
        placeholder: field.placeholder,
        value: field.value,
        selection: field.selection,
    }
}

/// A drop-down: a key face with an accent arrow, held down while its list is open.
pub fn picker(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let look = look(theme);
    let key = key_style(
        theme,
        match status {
            pick_list::Status::Active => button::Status::Active,
            pick_list::Status::Hovered => button::Status::Hovered,
            pick_list::Status::Opened { .. } => button::Status::Pressed,
        },
        Face::Plain,
        false,
    );
    pick_list::Style {
        text_color: key.text_color,
        placeholder_color: look.disabled_text,
        handle_color: theme.extended_palette().primary.base.color,
        background: key.background.unwrap_or(Background::Color(look.key.0)),
        border: key.border,
    }
}

/// The list a drop-down opens: a raised panel with the hovered entry lit in the accent.
pub fn menu(theme: &Theme) -> menu::Style {
    let look = look(theme);
    let palette = theme.extended_palette();
    menu::Style {
        background: Background::Color(look.panel.0),
        border: Border {
            radius: 6.0.into(),
            width: 1.0,
            color: look.panel_edge,
        },
        text_color: palette.background.base.text,
        selected_text_color: palette.primary.base.text,
        selected_background: Background::Color(palette.primary.base.color),
        shadow: Shadow {
            color: look.panel_shadow,
            offset: Vector::new(0.0, 6.0),
            blur_radius: 14.0,
        },
    }
}

/// A slider: a sunk track lit in the accent up to a square, key-like knob.
pub fn fader(theme: &Theme, status: slider::Status) -> slider::Style {
    let look = look(theme);
    let (knob, edge) = match status {
        slider::Status::Active => (look.key.0, look.key_edge),
        slider::Status::Hovered => (look.hover.0, look.hover_edge),
        slider::Status::Dragged => (look.pressed, look.hover_edge),
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(theme.extended_palette().primary.base.color),
                Background::Color(look.well),
            ),
            width: 6.0,
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Rectangle {
                width: 14,
                border_radius: 3.0.into(),
            },
            background: Background::Color(knob),
            border_width: 1.0,
            border_color: edge,
        },
    }
}

/// A scroll bar: a slim well whose thumb lights in the accent while the pointer is on it.
pub fn scrollbar(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let look = look(theme);
    let lit = match status {
        scrollable::Status::Active { .. } => false,
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered,
            ..
        } => is_vertical_scrollbar_hovered,
        scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged,
            ..
        } => is_vertical_scrollbar_dragged,
    };
    let rail = scrollable::Rail {
        background: Some(Background::Color(look.well.scale_alpha(0.6))),
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        scroller: scrollable::Scroller {
            background: Background::Color(if lit {
                theme.extended_palette().primary.base.color
            } else {
                look.unlit
            }),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
        },
    };
    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        ..scrollable::default(theme, status)
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

/// The app's name in the sidebar, with a model number like a device's badge.
pub fn brand<'a>() -> Element<'a, Message> {
    row![bold("focus").size(18), mono("FX-1").size(11).style(muted),]
        .spacing(6)
        .align_y(Alignment::End)
        .into()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_setting_overrides_the_system() {
        use focus_core::Theme;
        assert_eq!(mode_for(Theme::System, Mode::Light), Mode::Light);
        assert_eq!(mode_for(Theme::System, Mode::Dark), Mode::Dark);
        assert_eq!(mode_for(Theme::Light, Mode::Dark), Mode::Light);
        assert_eq!(mode_for(Theme::Dark, Mode::Light), Mode::Dark);
    }

    #[test]
    fn a_meter_lights_a_cell_as_soon_as_progress_starts() {
        assert_eq!(lit_cells(0.0, 30), 0);
        assert_eq!(lit_cells(0.001, 30), 1);
        assert_eq!(lit_cells(0.5, 30), 15);
        assert_eq!(lit_cells(1.0, 30), 30);
    }

    #[test]
    fn a_meter_clamps_out_of_range_progress() {
        assert_eq!(lit_cells(-1.0, 30), 0);
        assert_eq!(lit_cells(2.0, 30), 30);
        assert_eq!(lit_cells(f32::NAN, 30), 0);
    }
}
