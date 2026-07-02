// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use crate::geometry::{Point, Rect, Transform2D};
use bastyde_tokens::CornerRadius;

/// A path command for building arbitrary shapes (Tier 3 rendering).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadTo {
        control: Point,
        to: Point,
    },
    CubicTo {
        control1: Point,
        control2: Point,
        to: Point,
    },
    ArcTo {
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
    },
    Close,
}

/// A path composed of drawing commands. Used for Tier 3 (CPU rasterized) shapes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    pub commands: Vec<PathCommand>,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(&mut self, p: Point) -> &mut Self {
        self.commands.push(PathCommand::MoveTo(p));
        self
    }

    pub fn line_to(&mut self, p: Point) -> &mut Self {
        self.commands.push(PathCommand::LineTo(p));
        self
    }

    pub fn quad_to(&mut self, control: Point, to: Point) -> &mut Self {
        self.commands.push(PathCommand::QuadTo { control, to });
        self
    }

    pub fn cubic_to(&mut self, control1: Point, control2: Point, to: Point) -> &mut Self {
        self.commands.push(PathCommand::CubicTo {
            control1,
            control2,
            to,
        });
        self
    }

    pub fn arc_to(&mut self, rect: Rect, start_angle: f32, sweep_angle: f32) -> &mut Self {
        self.commands.push(PathCommand::ArcTo {
            rect,
            start_angle,
            sweep_angle,
        });
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.commands.push(PathCommand::Close);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Compute the axis-aligned bounding box of this path.
    /// Only considers control points (not exact curve bounds), which is
    /// sufficient for atlas allocation.
    pub fn bounds(&self) -> Rect {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        let mut include = |p: Point| {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        };

        for cmd in &self.commands {
            match *cmd {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => include(p),
                PathCommand::QuadTo { control, to } => {
                    include(control);
                    include(to);
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    include(control1);
                    include(control2);
                    include(to);
                }
                PathCommand::ArcTo { rect, .. } => {
                    include(Point::new(rect.x, rect.y));
                    include(Point::new(rect.right(), rect.bottom()));
                }
                PathCommand::Close => {}
            }
        }

        if min_x > max_x {
            return Rect::ZERO;
        }
        Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// Create a circle path.
    pub fn circle(center: Point, radius: f32) -> Self {
        let rect = Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        );
        let mut path = Self::new();
        // A subpath must open with a `MoveTo`, or a renderer has no start point
        // for the arc and draws a stray line from the origin. The arc begins at
        // angle 0 — the rightmost point (center.x + radius, center.y).
        path.move_to(Point::new(center.x + radius, center.y));
        path.arc_to(rect, 0.0, 360.0);
        path.close();
        path
    }

    /// Create a rounded rectangle path using arc segments for each corner.
    pub fn rounded_rect(rect: Rect, radii: CornerRadius) -> Self {
        let [tl, tr, br, bl] = radii.to_array();
        let mut path = Self::new();

        // Start at top edge after top-left radius
        path.move_to(Point::new(rect.x + tl, rect.y));

        // Top edge → top-right arc
        path.line_to(Point::new(rect.right() - tr, rect.y));
        if tr > 0.0 {
            let arc_rect = Rect::new(rect.right() - tr * 2.0, rect.y, tr * 2.0, tr * 2.0);
            path.arc_to(arc_rect, -90.0, 90.0);
        }

        // Right edge → bottom-right arc
        path.line_to(Point::new(rect.right(), rect.bottom() - br));
        if br > 0.0 {
            let arc_rect = Rect::new(
                rect.right() - br * 2.0,
                rect.bottom() - br * 2.0,
                br * 2.0,
                br * 2.0,
            );
            path.arc_to(arc_rect, 0.0, 90.0);
        }

        // Bottom edge → bottom-left arc
        path.line_to(Point::new(rect.x + bl, rect.bottom()));
        if bl > 0.0 {
            let arc_rect = Rect::new(rect.x, rect.bottom() - bl * 2.0, bl * 2.0, bl * 2.0);
            path.arc_to(arc_rect, 90.0, 90.0);
        }

        // Left edge → top-left arc
        path.line_to(Point::new(rect.x, rect.y + tl));
        if tl > 0.0 {
            let arc_rect = Rect::new(rect.x, rect.y, tl * 2.0, tl * 2.0);
            path.arc_to(arc_rect, 180.0, 90.0);
        }

        path.close();
        path
    }

    /// Create a star path.
    pub fn star(center: Point, outer_radius: f32, inner_radius: f32, points: u32) -> Self {
        let mut path = Self::new();
        let total = points * 2;
        for i in 0..total {
            let angle =
                (i as f32) * std::f32::consts::PI / points as f32 - std::f32::consts::FRAC_PI_2;
            let r = if i % 2 == 0 {
                outer_radius
            } else {
                inner_radius
            };
            let p = Point::new(center.x + r * angle.cos(), center.y + r * angle.sin());
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        path.close();
        path
    }

    /// Create a non-rounded rectangle path.
    pub fn rect(rect: Rect) -> Self {
        let mut path = Self::new();
        path.move_to(Point::new(rect.x, rect.y));
        path.line_to(Point::new(rect.right(), rect.y));
        path.line_to(Point::new(rect.right(), rect.bottom()));
        path.line_to(Point::new(rect.x, rect.bottom()));
        path.close();
        path
    }

    /// Create a single line segment path.
    pub fn line(from: Point, to: Point) -> Self {
        let mut path = Self::new();
        path.move_to(from);
        path.line_to(to);
        path
    }

    /// Create an ellipse inscribed in the given rectangle (4 cubic Bézier arcs).
    pub fn ellipse(rect: Rect) -> Self {
        // Approximate an ellipse with 4 cubic Bézier curves.
        // Magic number for quarter-circle cubic approximation: κ ≈ 0.5522847498
        const KAPPA: f32 = 0.552_284_8;
        let cx = rect.x + rect.width / 2.0;
        let cy = rect.y + rect.height / 2.0;
        let rx = rect.width / 2.0;
        let ry = rect.height / 2.0;
        let kx = rx * KAPPA;
        let ky = ry * KAPPA;

        let mut path = Self::new();
        // Start at top center
        path.move_to(Point::new(cx, cy - ry));
        // Top-right quadrant
        path.cubic_to(
            Point::new(cx + kx, cy - ry),
            Point::new(cx + rx, cy - ky),
            Point::new(cx + rx, cy),
        );
        // Bottom-right quadrant
        path.cubic_to(
            Point::new(cx + rx, cy + ky),
            Point::new(cx + kx, cy + ry),
            Point::new(cx, cy + ry),
        );
        // Bottom-left quadrant
        path.cubic_to(
            Point::new(cx - kx, cy + ry),
            Point::new(cx - rx, cy + ky),
            Point::new(cx - rx, cy),
        );
        // Top-left quadrant
        path.cubic_to(
            Point::new(cx - rx, cy - ky),
            Point::new(cx - kx, cy - ry),
            Point::new(cx, cy - ry),
        );
        path.close();
        path
    }

    /// Append all commands from another path.
    pub fn append(&mut self, other: &Path) {
        self.commands.extend_from_slice(&other.commands);
    }

    /// Create a copy of this path with all points transformed by the given
    /// affine transform. `ArcTo` commands are transformed via
    /// `apply_rect` which is only exact for translate/scale — for
    /// rotation, convert arcs to cubics before transforming.
    pub fn transformed(&self, transform: &Transform2D) -> Path {
        let mut result = Path::new();
        for cmd in &self.commands {
            match *cmd {
                PathCommand::MoveTo(p) => {
                    result.move_to(transform.apply_point(p));
                }
                PathCommand::LineTo(p) => {
                    result.line_to(transform.apply_point(p));
                }
                PathCommand::QuadTo { control, to } => {
                    result.quad_to(transform.apply_point(control), transform.apply_point(to));
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    result.cubic_to(
                        transform.apply_point(control1),
                        transform.apply_point(control2),
                        transform.apply_point(to),
                    );
                }
                PathCommand::ArcTo {
                    rect,
                    start_angle,
                    sweep_angle,
                } => {
                    result.arc_to(transform.apply_rect(rect), start_angle, sweep_angle);
                }
                PathCommand::Close => {
                    result.close();
                }
            }
        }
        result
    }

    /// Create a closed polygon from a list of points.
    pub fn polygon(points: &[Point]) -> Self {
        let mut path = Self::new();
        if let Some((&first, rest)) = points.split_first() {
            path.move_to(first);
            for &p in rest {
                path.line_to(p);
            }
            path.close();
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_path_not_empty() {
        let p = Path::circle(Point::new(50.0, 50.0), 25.0);
        assert!(!p.is_empty());
    }

    #[test]
    fn circle_path_opens_with_moveto() {
        // A subpath must open with a MoveTo or a renderer draws a stray line
        // from the origin to the arc. The circle starts at angle 0 (rightmost).
        let p = Path::circle(Point::new(50.0, 50.0), 25.0);
        match p.commands.first() {
            Some(PathCommand::MoveTo(pt)) => {
                assert!((pt.x - 75.0).abs() < 0.01 && (pt.y - 50.0).abs() < 0.01);
            }
            other => panic!("circle must open with MoveTo, got {other:?}"),
        }
    }

    #[test]
    fn star_path_has_correct_commands() {
        let p = Path::star(Point::new(50.0, 50.0), 30.0, 15.0, 5);
        assert!(!p.is_empty());
        // 5-point star: 10 vertices + 1 close
        assert_eq!(p.commands.len(), 11);
    }

    #[test]
    fn bounds_of_simple_path() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 20.0));
        path.line_to(Point::new(50.0, 80.0));
        path.line_to(Point::new(30.0, 40.0));
        let b = path.bounds();
        assert_eq!(b.x, 10.0);
        assert_eq!(b.y, 20.0);
        assert_eq!(b.width, 40.0);
        assert_eq!(b.height, 60.0);
    }

    #[test]
    fn bounds_of_empty_path_is_zero() {
        let path = Path::new();
        let b = path.bounds();
        assert_eq!(b, Rect::ZERO);
    }

    #[test]
    fn rounded_rect_uses_corner_radii() {
        use bastyde_tokens::CornerRadius;
        let p = Path::rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 50.0),
            CornerRadius::uniform(10.0),
        );
        // Should have arcs for each corner — more commands than a plain rect
        assert!(p.commands.len() > 5);
        // Should contain ArcTo commands
        let arc_count = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::ArcTo { .. }))
            .count();
        assert_eq!(arc_count, 4); // One per corner
    }

    #[test]
    fn rounded_rect_zero_radii_is_plain_rect() {
        use bastyde_tokens::CornerRadius;
        let p = Path::rounded_rect(Rect::new(0.0, 0.0, 100.0, 50.0), CornerRadius::uniform(0.0));
        // No arcs with zero radii
        let arc_count = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::ArcTo { .. }))
            .count();
        assert_eq!(arc_count, 0);
    }

    #[test]
    fn rect_path_has_four_lines() {
        let p = Path::rect(Rect::new(10.0, 20.0, 30.0, 40.0));
        // MoveTo + 3 LineTo + Close = 5 commands
        assert_eq!(p.commands.len(), 5);
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(p.commands[4], PathCommand::Close));
    }

    #[test]
    fn line_path() {
        let p = Path::line(Point::new(0.0, 0.0), Point::new(100.0, 50.0));
        assert_eq!(p.commands.len(), 2);
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(p.commands[1], PathCommand::LineTo(_)));
    }

    #[test]
    fn ellipse_path_uses_cubics() {
        let p = Path::ellipse(Rect::new(0.0, 0.0, 100.0, 50.0));
        let cubic_count = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::CubicTo { .. }))
            .count();
        assert_eq!(cubic_count, 4);
    }

    #[test]
    fn polygon_path() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(50.0, 80.0),
        ];
        let p = Path::polygon(&points);
        // MoveTo + 2 LineTo + Close = 4 commands
        assert_eq!(p.commands.len(), 4);
    }

    #[test]
    fn polygon_empty_points() {
        let p = Path::polygon(&[]);
        assert!(p.is_empty());
    }

    #[test]
    fn append_merges_paths() {
        let mut a = Path::new();
        a.move_to(Point::new(0.0, 0.0));
        a.line_to(Point::new(10.0, 10.0));

        let mut b = Path::new();
        b.move_to(Point::new(20.0, 20.0));
        b.line_to(Point::new(30.0, 30.0));

        a.append(&b);
        assert_eq!(a.commands.len(), 4);
        assert!(matches!(a.commands[2], PathCommand::MoveTo(p) if (p.x - 20.0).abs() < 0.01));
    }

    #[test]
    fn append_empty_is_noop() {
        let mut a = Path::new();
        a.move_to(Point::new(1.0, 2.0));
        let b = Path::new();
        a.append(&b);
        assert_eq!(a.commands.len(), 1);
    }

    #[test]
    fn transformed_translate() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 20.0));
        path.line_to(Point::new(30.0, 40.0));
        path.close();

        let t = Transform2D::translate(100.0, 200.0);
        let result = path.transformed(&t);

        assert_eq!(result.commands.len(), 3);
        match result.commands[0] {
            PathCommand::MoveTo(p) => {
                assert!((p.x - 110.0).abs() < 0.01);
                assert!((p.y - 220.0).abs() < 0.01);
            }
            _ => panic!("expected MoveTo"),
        }
        match result.commands[1] {
            PathCommand::LineTo(p) => {
                assert!((p.x - 130.0).abs() < 0.01);
                assert!((p.y - 240.0).abs() < 0.01);
            }
            _ => panic!("expected LineTo"),
        }
        assert!(matches!(result.commands[2], PathCommand::Close));
    }

    #[test]
    fn transformed_scale() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 20.0));
        path.quad_to(Point::new(15.0, 25.0), Point::new(30.0, 40.0));

        let t = Transform2D::scale(2.0, 3.0);
        let result = path.transformed(&t);

        match result.commands[1] {
            PathCommand::QuadTo { control, to } => {
                assert!((control.x - 30.0).abs() < 0.01);
                assert!((control.y - 75.0).abs() < 0.01);
                assert!((to.x - 60.0).abs() < 0.01);
                assert!((to.y - 120.0).abs() < 0.01);
            }
            _ => panic!("expected QuadTo"),
        }
    }

    #[test]
    fn transformed_cubic() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.cubic_to(
            Point::new(1.0, 0.0),
            Point::new(2.0, 1.0),
            Point::new(3.0, 3.0),
        );

        let t = Transform2D::translate(10.0, 20.0);
        let result = path.transformed(&t);

        match result.commands[1] {
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                assert!((control1.x - 11.0).abs() < 0.01);
                assert!((control2.x - 12.0).abs() < 0.01);
                assert!((to.x - 13.0).abs() < 0.01);
            }
            _ => panic!("expected CubicTo"),
        }
    }

    #[test]
    fn transformed_preserves_command_count() {
        let p = Path::circle(Point::new(50.0, 50.0), 25.0);
        let t = Transform2D::scale(2.0, 2.0);
        let result = p.transformed(&t);
        assert_eq!(result.commands.len(), p.commands.len());
    }
}
