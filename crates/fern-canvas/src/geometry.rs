/// A 2D point in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A 2D vector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A 2D size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };

    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// A rectangle defined by its origin (top-left) and size, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self {
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
        }
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    pub fn expand(&self, amount: f32) -> Self {
        Self {
            x: self.x - amount,
            y: self.y - amount,
            width: self.width + amount * 2.0,
            height: self.height + amount * 2.0,
        }
    }

    pub fn inset(&self, top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            x: self.x + left,
            y: self.y + top,
            width: (self.width - left - right).max(0.0),
            height: (self.height - top - bottom).max(0.0),
        }
    }

    pub fn to_array(&self) -> [f32; 4] {
        [self.x, self.y, self.width, self.height]
    }
}

/// A size proposal from a parent to a child during layout negotiation.
/// `None` means "use your ideal size" for that dimension.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeProposal {
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl SizeProposal {
    pub fn exact(width: f32, height: f32) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
        }
    }

    pub fn unspecified() -> Self {
        Self {
            width: None,
            height: None,
        }
    }

    pub fn with_width(width: f32) -> Self {
        Self {
            width: Some(width),
            height: None,
        }
    }

    pub fn with_height(height: f32) -> Self {
        Self {
            width: None,
            height: Some(height),
        }
    }

    /// Resolve to a concrete size, using the provided defaults for unspecified dimensions.
    pub fn resolve(&self, default_width: f32, default_height: f32) -> Size {
        Size::new(
            self.width.unwrap_or(default_width),
            self.height.unwrap_or(default_height),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_point_inside() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert!(r.contains(Point::new(50.0, 30.0)));
    }

    #[test]
    fn rect_does_not_contain_point_outside() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert!(!r.contains(Point::new(5.0, 5.0)));
        assert!(!r.contains(Point::new(200.0, 30.0)));
    }

    #[test]
    fn rect_contains_point_on_edge() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert!(r.contains(Point::new(10.0, 10.0))); // top-left
        assert!(r.contains(Point::new(110.0, 60.0))); // bottom-right
    }

    #[test]
    fn rect_center() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert_eq!(r.center(), Point::new(50.0, 25.0));
    }

    #[test]
    fn rect_center_with_offset() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.center(), Point::new(60.0, 45.0));
    }

    #[test]
    fn rect_expand() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        let expanded = r.expand(5.0);
        assert_eq!(expanded.x, 5.0);
        assert_eq!(expanded.y, 5.0);
        assert_eq!(expanded.width, 110.0);
        assert_eq!(expanded.height, 60.0);
    }

    #[test]
    fn rect_inset() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        let inset = r.inset(10.0, 10.0, 10.0, 10.0);
        assert_eq!(inset.x, 10.0);
        assert_eq!(inset.y, 10.0);
        assert_eq!(inset.width, 80.0);
        assert_eq!(inset.height, 30.0);
    }

    #[test]
    fn rect_inset_clamped_to_zero() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let inset = r.inset(20.0, 20.0, 20.0, 20.0);
        assert_eq!(inset.width, 0.0);
        assert_eq!(inset.height, 0.0);
    }

    #[test]
    fn size_proposal_exact() {
        let p = SizeProposal::exact(200.0, 40.0);
        assert_eq!(p.width, Some(200.0));
        assert_eq!(p.height, Some(40.0));
    }

    #[test]
    fn size_proposal_unspecified() {
        let p = SizeProposal::unspecified();
        assert_eq!(p.width, None);
        assert_eq!(p.height, None);
    }

    #[test]
    fn size_proposal_resolve_with_defaults() {
        let p = SizeProposal::with_width(200.0);
        let size = p.resolve(100.0, 50.0);
        assert_eq!(size.width, 200.0);
        assert_eq!(size.height, 50.0);
    }

    #[test]
    fn rect_to_array() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(r.to_array(), [10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn rect_from_origin_size() {
        let r = Rect::from_origin_size(Point::new(10.0, 20.0), Size::new(30.0, 40.0));
        assert_eq!(r.x, 10.0);
        assert_eq!(r.y, 20.0);
        assert_eq!(r.width, 30.0);
        assert_eq!(r.height, 40.0);
    }
}
