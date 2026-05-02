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

/// A 2D affine transform stored as a 3×2 matrix: `[a, b, c, d, tx, ty]`.
///
/// The transform maps a point `(x, y)` to:
///   `(a*x + c*y + tx, b*x + d*y + ty)`
///
/// This is the standard 2D affine matrix layout compatible with GPU uniform buffers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D {
    /// Matrix entries: [a, b, c, d, tx, ty].
    pub m: [f32; 6],
}

impl Transform2D {
    pub const IDENTITY: Transform2D = Transform2D {
        m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };

    pub fn identity() -> Self {
        Self::IDENTITY
    }

    pub fn translate(dx: f32, dy: f32) -> Self {
        Self {
            m: [1.0, 0.0, 0.0, 1.0, dx, dy],
        }
    }

    pub fn rotate(angle_radians: f32) -> Self {
        let (s, c) = angle_radians.sin_cos();
        Self {
            m: [c, s, -s, c, 0.0, 0.0],
        }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Self {
            m: [sx, 0.0, 0.0, sy, 0.0, 0.0],
        }
    }

    /// Compose: apply `self` then `other` (i.e. `other * self`).
    pub fn then(&self, other: &Transform2D) -> Transform2D {
        let [a1, b1, c1, d1, tx1, ty1] = self.m;
        let [a2, b2, c2, d2, tx2, ty2] = other.m;
        Transform2D {
            m: [
                a2 * a1 + c2 * b1,
                b2 * a1 + d2 * b1,
                a2 * c1 + c2 * d1,
                b2 * c1 + d2 * d1,
                a2 * tx1 + c2 * ty1 + tx2,
                b2 * tx1 + d2 * ty1 + ty2,
            ],
        }
    }

    pub fn apply_point(&self, p: Point) -> Point {
        let [a, b, c, d, tx, ty] = self.m;
        Point::new(a * p.x + c * p.y + tx, b * p.x + d * p.y + ty)
    }

    /// Compute the axis-aligned bounding box of a transformed rectangle.
    pub fn apply_rect(&self, r: Rect) -> Rect {
        let corners = [
            self.apply_point(Point::new(r.x, r.y)),
            self.apply_point(Point::new(r.right(), r.y)),
            self.apply_point(Point::new(r.right(), r.bottom())),
            self.apply_point(Point::new(r.x, r.bottom())),
        ];
        let min_x = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let min_y = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let max_y = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// Convert to GPU-friendly layout: two columns of a 3×2 matrix.
    pub fn to_mat3x2(&self) -> [[f32; 2]; 3] {
        let [a, b, c, d, tx, ty] = self.m;
        [[a, b], [c, d], [tx, ty]]
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// Inverse of the affine transform, or `None` if the linear part is
    /// singular (determinant zero — a degenerate scale that collapses an
    /// axis). Used by hit-testing to map a screen-space point back into a
    /// transformed widget's pre-transform bounds.
    pub fn inverse(&self) -> Option<Transform2D> {
        let [a, b, c, d, tx, ty] = self.m;
        let det = a * d - c * b;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let inv_det = 1.0 / det;
        let ia = d * inv_det;
        let ib = -b * inv_det;
        let ic = -c * inv_det;
        let id = a * inv_det;
        let itx = (c * ty - d * tx) * inv_det;
        let ity = (b * tx - a * ty) * inv_det;
        Some(Transform2D {
            m: [ia, ib, ic, id, itx, ity],
        })
    }
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
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

    #[test]
    fn transform_identity() {
        let t = Transform2D::identity();
        assert!(t.is_identity());
        let p = t.apply_point(Point::new(3.0, 4.0));
        assert_eq!(p, Point::new(3.0, 4.0));
    }

    #[test]
    fn transform_translate() {
        let t = Transform2D::translate(10.0, 20.0);
        let p = t.apply_point(Point::new(3.0, 4.0));
        assert_eq!(p, Point::new(13.0, 24.0));
    }

    #[test]
    fn transform_scale() {
        let t = Transform2D::scale(2.0, 3.0);
        let p = t.apply_point(Point::new(5.0, 10.0));
        assert_eq!(p, Point::new(10.0, 30.0));
    }

    #[test]
    fn transform_rotate_90() {
        let t = Transform2D::rotate(std::f32::consts::FRAC_PI_2);
        let p = t.apply_point(Point::new(1.0, 0.0));
        assert!((p.x - 0.0).abs() < 1e-5);
        assert!((p.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn transform_compose_translate_then_scale() {
        let translate = Transform2D::translate(10.0, 0.0);
        let scale = Transform2D::scale(2.0, 2.0);
        // apply translate first, then scale: result = scale(translate(point))
        let composed = translate.then(&scale);
        let p = composed.apply_point(Point::new(5.0, 3.0));
        assert_eq!(p, Point::new(30.0, 6.0)); // (5+10)*2, 3*2
    }

    #[test]
    fn transform_apply_rect() {
        let t = Transform2D::translate(100.0, 200.0);
        let r = t.apply_rect(Rect::new(10.0, 20.0, 30.0, 40.0));
        assert!((r.x - 110.0).abs() < 1e-5);
        assert!((r.y - 220.0).abs() < 1e-5);
        assert!((r.width - 30.0).abs() < 1e-5);
        assert!((r.height - 40.0).abs() < 1e-5);
    }

    #[test]
    fn transform_inverse_identity() {
        let t = Transform2D::IDENTITY;
        assert_eq!(t.inverse().unwrap(), Transform2D::IDENTITY);
    }

    #[test]
    fn transform_inverse_translate_roundtrip() {
        let t = Transform2D::translate(10.0, 20.0);
        let inv = t.inverse().unwrap();
        let p = Point::new(3.0, 4.0);
        let q = inv.apply_point(t.apply_point(p));
        assert!((q.x - p.x).abs() < 1e-5);
        assert!((q.y - p.y).abs() < 1e-5);
    }

    #[test]
    fn transform_inverse_scale_roundtrip() {
        let t = Transform2D::scale(2.0, 3.0);
        let inv = t.inverse().unwrap();
        let p = Point::new(5.0, 7.0);
        let q = inv.apply_point(t.apply_point(p));
        assert!((q.x - p.x).abs() < 1e-5);
        assert!((q.y - p.y).abs() < 1e-5);
    }

    #[test]
    fn transform_inverse_rotation_roundtrip() {
        let t = Transform2D::rotate(std::f32::consts::FRAC_PI_3);
        let inv = t.inverse().unwrap();
        let p = Point::new(1.0, 2.0);
        let q = inv.apply_point(t.apply_point(p));
        assert!((q.x - p.x).abs() < 1e-4);
        assert!((q.y - p.y).abs() < 1e-4);
    }

    #[test]
    fn transform_inverse_compose_roundtrip() {
        // (translate then scale).inverse() should map any point back to itself.
        let t = Transform2D::translate(50.0, 0.0).then(&Transform2D::scale(2.0, 2.0));
        let inv = t.inverse().unwrap();
        let p = Point::new(7.5, -3.0);
        let q = inv.apply_point(t.apply_point(p));
        assert!((q.x - p.x).abs() < 1e-4);
        assert!((q.y - p.y).abs() < 1e-4);
    }

    #[test]
    fn transform_inverse_singular_returns_none() {
        // Scale by 0 collapses an axis: not invertible.
        let t = Transform2D::scale(0.0, 1.0);
        assert!(t.inverse().is_none());
    }

    #[test]
    fn transform_to_mat3x2() {
        let t = Transform2D::translate(10.0, 20.0);
        let m = t.to_mat3x2();
        assert_eq!(m, [[1.0, 0.0], [0.0, 1.0], [10.0, 20.0]]);
    }

    #[test]
    fn transform_not_identity() {
        let t = Transform2D::translate(1.0, 0.0);
        assert!(!t.is_identity());
    }
}
