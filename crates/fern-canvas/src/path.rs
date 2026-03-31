use crate::geometry::{Point, Rect};
use fern_tokens::CornerRadius;

/// A path command for building arbitrary shapes (Tier 3 rendering).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadTo { control: Point, to: Point },
    CubicTo { control1: Point, control2: Point, to: Point },
    ArcTo { rect: Rect, start_angle: f32, sweep_angle: f32 },
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
        self.commands.push(PathCommand::CubicTo { control1, control2, to });
        self
    }

    pub fn arc_to(&mut self, rect: Rect, start_angle: f32, sweep_angle: f32) -> &mut Self {
        self.commands.push(PathCommand::ArcTo { rect, start_angle, sweep_angle });
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
        let rect = Rect::new(center.x - radius, center.y - radius, radius * 2.0, radius * 2.0);
        let mut path = Self::new();
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
            let angle = (i as f32) * std::f32::consts::PI / points as f32 - std::f32::consts::FRAC_PI_2;
            let r = if i % 2 == 0 { outer_radius } else { inner_radius };
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
        use fern_tokens::CornerRadius;
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
        use fern_tokens::CornerRadius;
        let p = Path::rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 50.0),
            CornerRadius::uniform(0.0),
        );
        // No arcs with zero radii
        let arc_count = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::ArcTo { .. }))
            .count();
        assert_eq!(arc_count, 0);
    }
}
