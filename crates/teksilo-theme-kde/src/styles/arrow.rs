// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze arrow: the chevron a combo box and a spin box draw.
//!
//! A polyline in a square of side `min(width, height, 10)` centred in the
//! box it is drawn in. Its two ends lie a quarter and three quarters of the
//! way across the square, on its centre line, and its apex on the square's
//! other centre line, a quarter of the side away from the ends, so the arms
//! meet at a right angle. It is drawn with a 1 px pen, square caps and a
//! mitred join, and moved along the way it points by -0.25, +0.5 or
//! +0.25 px when the side is 1, 2 or 3 more than a multiple of four, which
//! puts the apex on a pixel.
//!
//! An arrow is drawn in the arrow colour of a text role: that text moved
//! 15 % of the way toward its background.

use teksilo_canvas::{Canvas, LineCap, LineJoin, Path, Point, Rect, StrokeStyle};
use teksilo_tokens::Color;

use crate::geometry::ARROW_SIZE;
use crate::styles::chrome::kde_mix;

/// `[Hp]` How far an arrow's colour moves from its text toward the text's
/// background.
pub const ARROW_TOWARD_BACKGROUND: f64 = 0.15;

/// `[Hp]` The pen an arrow is drawn with.
pub const ARROW_PEN: f32 = 1.0;

/// Which way an arrow points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowDirection {
    /// Up, the spin box's step up.
    Up,
    /// Down, the combo box's arrow and the spin box's step down.
    Down,
}

/// `[Hp]` The side of the square an arrow is drawn in inside `rect`.
pub fn arrow_side(rect: Rect) -> f32 {
    rect.width.min(rect.height).clamp(0.0, ARROW_SIZE)
}

/// `[Hp]` How far an arrow of side `side` is moved along the way it points,
/// so that its apex lands on a pixel.
pub fn arrow_nudge(side: f32) -> f32 {
    match (side.round() as i64).rem_euclid(4) {
        1 => -0.25,
        2 => 0.5,
        3 => 0.25,
        _ => 0.0,
    }
}

/// `[Hp]` The three points of an arrow pointing `direction` centred in
/// `rect`: the leading end, the apex and the trailing end.
///
/// `[measured-qt]` The nudge moves an up arrow up and a down arrow down:
/// Plasma's spin box puts the ends of its up arrow half a pixel above the
/// centre of its half and those of its down arrow half a pixel below.
pub fn arrow_points(rect: Rect, direction: ArrowDirection) -> [Point; 3] {
    let s = arrow_side(rect);
    let cx = rect.x + rect.width * 0.5;
    let cy = rect.y + rect.height * 0.5;
    let quarter = s * 0.25;
    let (ends, apex) = match direction {
        ArrowDirection::Down => {
            let ends = cy + arrow_nudge(s);
            (ends, ends + quarter)
        }
        ArrowDirection::Up => {
            let ends = cy - arrow_nudge(s);
            (ends, ends - quarter)
        }
    };
    [
        Point::new(cx - quarter, ends),
        Point::new(cx, apex),
        Point::new(cx + quarter, ends),
    ]
}

/// The arrow colour of the text `text` on `background`.
pub fn arrow_color(text: Color, background: Color) -> Color {
    kde_mix(text, background, ARROW_TOWARD_BACKGROUND)
}

/// Draw an arrow pointing `direction` centred in `rect` in `color`.
pub fn paint_arrow(canvas: &mut Canvas, rect: Rect, direction: ArrowDirection, color: Color) {
    if color.a() <= 0.0 || arrow_side(rect) <= 0.0 {
        return;
    }
    let [a, b, c] = arrow_points(rect, direction);
    let mut path = Path::new();
    path.move_to(a);
    path.line_to(b);
    path.line_to(c);
    let mut style = StrokeStyle::solid(ARROW_PEN);
    style.line_cap = LineCap::Square;
    style.line_join = LineJoin::Miter;
    canvas.stroke_path(&path, color, style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColor;

    fn xy(p: Point) -> [f32; 2] {
        [p.x, p.y]
    }

    /// The combo box's arrow, in its 20 px box at (128, 6) of a 150 x 32
    /// combo box: its ends on pixel row 16, five pixels apart, as Plasma
    /// paints them, and its apex 2.5 px below.
    #[test]
    fn the_combo_arrow_lies_where_plasma_paints_it() {
        let p = arrow_points(Rect::new(128.0, 6.0, 20.0, 20.0), ArrowDirection::Down);
        assert_eq!(p.map(xy), [[135.5, 16.5], [138.0, 19.0], [140.5, 16.5]]);
    }

    /// The spin box's two arrows, in the 20 x 10 halves at (78, 6) and
    /// (78, 16) of a 100 x 32 spin box: the ends on pixel rows 10 and 21.
    #[test]
    fn the_spin_arrows_lie_where_plasma_paints_them() {
        let up = arrow_points(Rect::new(78.0, 6.0, 20.0, 10.0), ArrowDirection::Up);
        assert_eq!(up.map(xy), [[85.5, 10.5], [88.0, 8.0], [90.5, 10.5]]);
        let down = arrow_points(Rect::new(78.0, 16.0, 20.0, 10.0), ArrowDirection::Down);
        assert_eq!(down.map(xy), [[85.5, 21.5], [88.0, 24.0], [90.5, 21.5]]);
    }

    #[test]
    fn the_side_is_at_most_ten_and_the_nudge_follows_it() {
        assert_eq!(arrow_side(Rect::new(0.0, 0.0, 20.0, 20.0)), 10.0);
        assert_eq!(arrow_side(Rect::new(0.0, 0.0, 20.0, 7.0)), 7.0);
        assert_eq!(arrow_nudge(10.0), 0.5);
        assert_eq!(arrow_nudge(9.0), -0.25);
        assert_eq!(arrow_nudge(7.0), 0.25);
        assert_eq!(arrow_nudge(8.0), 0.0);
    }

    #[test]
    fn the_arms_meet_at_a_right_angle() {
        let [a, b, c] = arrow_points(Rect::new(0.0, 0.0, 20.0, 20.0), ArrowDirection::Down);
        let (u, v) = ((a.x - b.x, a.y - b.y), (c.x - b.x, c.y - b.y));
        assert!((u.0 * v.0 + u.1 * v.1).abs() < 1e-6);
    }

    /// Breeze Light's combo arrow: the button text moved 15 % toward the
    /// button colour.
    #[test]
    fn the_arrow_colour_moves_the_text_toward_its_background() {
        let text = Color::from_hex("#232629");
        let button = Color::from_hex("#FCFCFC");
        let c = KdeColor::from_color(arrow_color(text, button)).to_rgba8();
        assert_eq!(c, [68, 70, 73, 255]);
    }
}
