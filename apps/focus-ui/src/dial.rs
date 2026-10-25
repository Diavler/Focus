//! The running session's dial: a ring on readout glass whose lit arc is the time left, with the
//! time itself in the middle.

use std::f32::consts::{FRAC_PI_2, TAU};

use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke, path::Arc};
use iced::widget::{container, stack};
use iced::{Element, Length, Point, Radians, Rectangle, Renderer, Theme, Vector, mouse};

use crate::Message;
use crate::style;

/// The ring's thickness.
const RING: f32 = 10.0;

/// The number of marks around the ring (one per five minutes of an hour).
const MARKS: u16 = 12;

/// Draws the dial for `left` (0 to 1) of the session still to run.
struct Dial {
    left: f32,
}

impl canvas::Program<Message> for Dial {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let colors = style::dial_colors(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        let radius = bounds.width.min(bounds.height) / 2.0;
        frame.fill(&Path::circle(center, radius), colors.rim);
        frame.fill(&Path::circle(center, radius - 1.5), colors.glass);

        let ring = radius - 18.0;
        let stroke = |color| Stroke::default().with_width(RING).with_color(color);
        frame.stroke(&Path::circle(center, ring), stroke(colors.track));
        if self.left > 0.0 {
            // Angles run clockwise from the right; the arc starts at the top.
            let arc = Path::new(|path| {
                path.arc(Arc {
                    center,
                    radius: ring,
                    start_angle: Radians(-FRAC_PI_2),
                    end_angle: Radians(-FRAC_PI_2 + TAU * self.left),
                });
            });
            frame.stroke(&arc, stroke(colors.lit).with_line_cap(LineCap::Butt));
        }

        for mark in 0..MARKS {
            let angle = f32::from(mark) * TAU / f32::from(MARKS) - FRAC_PI_2;
            let toward = Vector::new(angle.cos(), angle.sin());
            let from = center + toward * (radius - 10.0);
            let to = center + toward * (radius - 6.0);
            frame.stroke(
                &Path::line(Point::new(from.x, from.y), Point::new(to.x, to.y)),
                Stroke::default().with_width(1.5).with_color(colors.track),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// A dial `size` across showing `left` (0 to 1) of the session still to run, with `content`
/// in its middle.
pub fn dial<'a>(
    left: f32,
    content: impl Into<Element<'a, Message>>,
    size: f32,
) -> Element<'a, Message> {
    stack![
        canvas::Canvas::new(Dial {
            left: left.clamp(0.0, 1.0),
        })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size)),
        container(content).center(Length::Fixed(size)),
    ]
    .into()
}
