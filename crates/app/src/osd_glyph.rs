//! Vector glyphs for the overlay, rasterised into the premultiplied bitmap.
//!
//! Icons are described as normalised line segments and arcs inside a 48-unit
//! box, then stroked with an analytic distance so the edges stay smooth at any
//! scale. Nothing here depends on vendor artwork.

/// One drawing primitive in the 48-unit glyph box.
#[derive(Clone, Copy)]
pub(crate) enum Prim {
    /// Straight stroke between two points.
    Line(f32, f32, f32, f32),
    /// Circular arc around a centre, angles in turns (0..1 is a full circle).
    Arc(f32, f32, f32, f32, f32),
    /// Closed circle.
    Ring(f32, f32, f32),
    /// Rounded rectangle outline.
    Rect(f32, f32, f32, f32, f32),
    /// Filled disc, used for level dots and indicator lamps.
    Dot(f32, f32, f32),
}

pub(crate) const GLYPH_BOX: f32 = 48.0;

/// Icon identifiers carried by the presentation request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    None,
    Keyboard,
    Lock,
    Fan,
    Bolt,
    Gauge,
    Key,
    Touchpad,
    Camera,
    Microphone,
    Dock,
    Waves,
    Window,
    Moon,
    Alert,
    Film,
    Leaf,
}

impl Glyph {
    /// Numeric id used only where a plain integer has to cross into the
    /// compositor; every caller keeps the enum.
    pub(crate) const fn id(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Keyboard => 1,
            Self::Lock => 2,
            Self::Fan => 3,
            Self::Bolt => 4,
            Self::Gauge => 5,
            Self::Key => 6,
            Self::Touchpad => 7,
            Self::Camera => 8,
            Self::Microphone => 9,
            Self::Dock => 10,
            Self::Waves => 11,
            Self::Window => 12,
            Self::Moon => 13,
            Self::Alert => 14,
            Self::Film => 15,
            Self::Leaf => 16,
        }
    }

    pub(crate) fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Keyboard,
            2 => Self::Lock,
            3 => Self::Fan,
            4 => Self::Bolt,
            5 => Self::Gauge,
            6 => Self::Key,
            7 => Self::Touchpad,
            8 => Self::Camera,
            9 => Self::Microphone,
            10 => Self::Dock,
            11 => Self::Waves,
            12 => Self::Window,
            13 => Self::Moon,
            14 => Self::Alert,
            15 => Self::Film,
            16 => Self::Leaf,
            _ => Self::None,
        }
    }

    /// Primitives for this glyph, in the normalised 48-unit box.
    pub(crate) fn prims(self) -> &'static [Prim] {
        match self {
            Self::None => &[],
            // Keyboard: body outline plus three key ticks along the top.
            Self::Keyboard => &[
                Prim::Rect(6.0, 16.0, 36.0, 18.0, 4.0),
                Prim::Line(12.0, 25.0, 15.0, 25.0),
                Prim::Line(20.0, 25.0, 24.0, 25.0),
                Prim::Line(29.0, 25.0, 33.0, 25.0),
                Prim::Line(24.0, 8.0, 24.0, 3.0),
                Prim::Line(14.0, 10.0, 11.0, 6.0),
                Prim::Line(34.0, 10.0, 37.0, 6.0),
            ],
            // Lock: shackle arc over a body.
            Self::Lock => &[
                Prim::Arc(24.0, 17.0, 8.0, 0.0, 0.5),
                Prim::Rect(12.0, 20.0, 24.0, 20.0, 3.0),
                Prim::Line(12.0, 20.0, 12.0, 17.0),
                Prim::Line(36.0, 20.0, 36.0, 17.0),
            ],
            // Fan: hub with three blades.
            Self::Fan => &[
                Prim::Ring(24.0, 24.0, 4.0),
                Prim::Arc(24.0, 24.0, 11.0, 0.08, 0.30),
                Prim::Arc(24.0, 24.0, 11.0, 0.41, 0.63),
                Prim::Arc(24.0, 24.0, 11.0, 0.74, 0.96),
                Prim::Line(24.0, 13.0, 24.0, 8.0),
                Prim::Line(33.5, 29.5, 37.7, 32.0),
                Prim::Line(14.5, 29.5, 10.3, 32.0),
            ],
            // Bolt: two strokes forming a lightning shape.
            Self::Bolt => &[
                Prim::Line(28.0, 6.0, 16.0, 26.0),
                Prim::Line(16.0, 26.0, 24.0, 26.0),
                Prim::Line(24.0, 26.0, 20.0, 42.0),
                Prim::Line(20.0, 42.0, 33.0, 20.0),
                Prim::Line(33.0, 20.0, 25.0, 20.0),
                Prim::Line(25.0, 20.0, 28.0, 6.0),
            ],
            // Gauge:半圆 dial with a needle.
            Self::Gauge => &[
                Prim::Arc(24.0, 30.0, 15.0, 0.5, 1.0),
                Prim::Line(9.0, 30.0, 39.0, 30.0),
                Prim::Line(24.0, 30.0, 33.0, 18.0),
            ],
            // Key cap: rounded square with a bar.
            Self::Key => &[
                Prim::Rect(10.0, 12.0, 28.0, 24.0, 5.0),
                Prim::Line(24.0, 20.0, 24.0, 28.0),
            ],
            // Touchpad: rounded surface with a click seam.
            Self::Touchpad => &[
                Prim::Rect(8.0, 12.0, 32.0, 26.0, 6.0),
                Prim::Line(8.0, 30.0, 40.0, 30.0),
            ],
            // Camera: body plus lens.
            Self::Camera => &[
                Prim::Rect(8.0, 16.0, 32.0, 20.0, 4.0),
                Prim::Ring(24.0, 26.0, 6.0),
                Prim::Line(18.0, 10.0, 21.0, 6.0),
                Prim::Line(18.0, 10.0, 28.0, 10.0),
            ],
            // Microphone: capsule over a stand.
            Self::Microphone => &[
                Prim::Rect(18.0, 6.0, 12.0, 22.0, 6.0),
                Prim::Arc(24.0, 28.0, 10.0, 0.5, 1.0),
                Prim::Line(24.0, 38.0, 24.0, 44.0),
                Prim::Line(17.0, 44.0, 31.0, 44.0),
            ],
            // Dock: connector plug.
            Self::Dock => &[
                Prim::Rect(8.0, 20.0, 32.0, 14.0, 4.0),
                Prim::Line(16.0, 20.0, 16.0, 12.0),
                Prim::Line(24.0, 20.0, 24.0, 12.0),
                Prim::Line(32.0, 20.0, 32.0, 12.0),
            ],
            // Radio: three expanding arcs.
            Self::Waves => &[
                Prim::Dot(24.0, 34.0, 2.5),
                Prim::Arc(24.0, 34.0, 10.0, 0.6, 0.9),
                Prim::Arc(24.0, 34.0, 17.0, 0.62, 0.88),
            ],
            // Window: frame with a title bar.
            Self::Window => &[
                Prim::Rect(8.0, 10.0, 32.0, 28.0, 4.0),
                Prim::Line(8.0, 19.0, 40.0, 19.0),
            ],
            // Moon: crescent from two arcs.
            Self::Moon => &[Prim::Arc(24.0, 24.0, 14.0, 0.12, 0.62)],
            // Film: media frame with sprocket holes.
            Self::Film => &[
                Prim::Rect(8.0, 12.0, 32.0, 24.0, 3.0),
                Prim::Dot(14.0, 18.0, 1.5),
                Prim::Dot(14.0, 30.0, 1.5),
                Prim::Dot(34.0, 18.0, 1.5),
                Prim::Dot(34.0, 30.0, 1.5),
            ],
            // Leaf: two arcs meeting at a stem, for power saving.
            Self::Leaf => &[
                Prim::Arc(24.0, 24.0, 15.0, 0.25, 0.75),
                Prim::Arc(24.0, 24.0, 8.0, 0.25, 0.75),
                Prim::Line(24.0, 8.0, 24.0, 40.0),
            ],
            // Alert: ring with a bang.
            Self::Alert => &[
                Prim::Ring(24.0, 26.0, 13.0),
                Prim::Line(24.0, 18.0, 24.0, 28.0),
                Prim::Dot(24.0, 34.0, 2.0),
            ],
        }
    }
}

/// Shortest distance from a point to a line segment.
fn distance_to_segment(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return (px - x0).hypot(py - y0);
    }
    let t = (((px - x0) * dx + (py - y0) * dy) / length_squared).clamp(0.0, 1.0);
    (px - (x0 + t * dx)).hypot(py - (y0 + t * dy))
}

/// Shortest distance from a point to an arc of the given radius and sweep.
fn distance_to_arc(px: f32, py: f32, cx: f32, cy: f32, radius: f32, start: f32, end: f32) -> f32 {
    let dx = px - cx;
    let dy = py - cy;
    let mut angle = dy.atan2(dx) / std::f32::consts::TAU;
    if angle < 0.0 {
        angle += 1.0;
    }
    let inside =
        (start..=end).contains(&angle) || (start > end && (angle >= start || angle <= end));
    if inside {
        (dx.hypot(dy) - radius).abs()
    } else {
        let point = |turn: f32| {
            let a = turn * std::f32::consts::TAU;
            (cx + radius * a.cos(), cy + radius * a.sin())
        };
        let (x0, y0) = point(start);
        let (x1, y1) = point(end);
        // Distance to an arc outside its sweep is the distance to whichever
        // endpoint is nearer. Projecting onto a segment that ends at the
        // sample point would always give zero and flood the glyph.
        (px - x0).hypot(py - y0).min((px - x1).hypot(py - y1))
    }
}

/// Coverage of a rounded-rectangle outline.
fn distance_to_rect_outline(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, radius: f32) -> f32 {
    let cx = px - (x + w * 0.5);
    let cy = py - (y + h * 0.5);
    let half_w = w * 0.5 - radius;
    let half_h = h * 0.5 - radius;
    let qx = cx.abs() - half_w;
    let qy = cy.abs() - half_h;
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let signed = outside + qx.max(qy).min(0.0) - radius;
    signed.abs()
}

/// Coverage in 0..=1 for one primitive at a point, given a stroke half-width.
pub(crate) fn coverage(prim: &Prim, px: f32, py: f32, half_width: f32) -> f32 {
    let distance = match *prim {
        Prim::Line(x0, y0, x1, y1) => distance_to_segment(px, py, x0, y0, x1, y1),
        Prim::Arc(cx, cy, radius, start, end) => {
            distance_to_arc(px, py, cx, cy, radius, start, end)
        }
        Prim::Ring(cx, cy, radius) => ((px - cx).hypot(py - cy) - radius).abs(),
        Prim::Rect(x, y, w, h, radius) => distance_to_rect_outline(px, py, x, y, w, h, radius),
        Prim::Dot(cx, cy, radius) => (px - cx).hypot(py - cy) - radius,
    };
    (half_width + 0.5 - distance).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_has_primitives_except_none() {
        for glyph in [
            Glyph::Keyboard,
            Glyph::Lock,
            Glyph::Fan,
            Glyph::Bolt,
            Glyph::Gauge,
            Glyph::Key,
            Glyph::Touchpad,
            Glyph::Camera,
            Glyph::Microphone,
            Glyph::Dock,
            Glyph::Waves,
            Glyph::Window,
            Glyph::Moon,
            Glyph::Alert,
            Glyph::Film,
            Glyph::Leaf,
        ] {
            assert!(!glyph.prims().is_empty(), "glyph {glyph:?} has no geometry");
        }
        assert!(Glyph::None.prims().is_empty());
    }

    #[test]
    fn coverage_is_bounded_and_peaks_on_the_stroke() {
        let line = Prim::Line(0.0, 0.0, 10.0, 0.0);
        assert_eq!(coverage(&line, 5.0, 0.0, 1.5), 1.0);
        assert_eq!(coverage(&line, 5.0, 20.0, 1.5), 0.0);
        let dot = Prim::Dot(4.0, 4.0, 3.0);
        assert_eq!(coverage(&dot, 4.0, 4.0, 1.0), 1.0);
    }

    const ALL_GLYPHS: [Glyph; 17] = [
        Glyph::None,
        Glyph::Keyboard,
        Glyph::Lock,
        Glyph::Fan,
        Glyph::Bolt,
        Glyph::Gauge,
        Glyph::Key,
        Glyph::Touchpad,
        Glyph::Camera,
        Glyph::Microphone,
        Glyph::Dock,
        Glyph::Waves,
        Glyph::Window,
        Glyph::Moon,
        Glyph::Alert,
        Glyph::Film,
        Glyph::Leaf,
    ];

    /// The integer id is the only place a glyph crosses into the compositor,
    /// so it has to be unique and reversible. A duplicate here would silently
    /// show the wrong icon, which is exactly the drift the enum prevents.
    #[test]
    fn glyph_ids_are_unique_and_round_trip() {
        let mut ids = Vec::new();
        for glyph in ALL_GLYPHS {
            let id = glyph.id();
            assert_eq!(Glyph::from_id(id), glyph, "id {id} maps to another glyph");
            assert!(!ids.contains(&id), "duplicate glyph id {id}");
            ids.push(id);
        }
    }

    /// The fan glyph must be an outline: the centre is empty and only the
    /// strokes light up. A filled icon means the distance maths collapsed.
    #[test]
    fn fan_glyph_is_an_outline_not_a_filled_block() {
        let glyph = Glyph::Fan;
        let prims = glyph.prims();
        let half = 0.95_f32;
        let at = |x: f32, y: f32| {
            prims
                .iter()
                .map(|prim| coverage(prim, x, y, half))
                .fold(0.0_f32, f32::max)
        };
        assert!(
            at(24.0, 24.0) < 0.2,
            "centre should be empty, got {}",
            at(24.0, 24.0)
        );
        assert!(at(24.0, 13.0) > 0.5, "ring top should be stroked");
        assert!(at(8.0, 8.0) < 0.1, "corner should be empty");
        let mut filled = 0;
        for y in 0..48 {
            for x in 0..48 {
                if at(x as f32, y as f32) > 0.5 {
                    filled += 1;
                }
            }
        }
        assert!(filled < 900, "glyph covers {filled} of 2304 samples");
    }

    #[test]
    fn ids_round_trip_to_glyphs() {
        assert_eq!(Glyph::from_id(3), Glyph::Fan);
        assert_eq!(Glyph::from_id(0), Glyph::None);
        assert_eq!(Glyph::from_id(-7), Glyph::None);
    }
}
