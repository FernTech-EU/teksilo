//! [`PathItem`] — vector path with optional fill and stroke.
//!
//! Stroke-only paths use a per-segment distance hit-test so users can
//! click along the stroke even when the AABB is huge (the connector-
//! line workhorse).

use accesskit::Role;
use fern_canvas::{Canvas, Path, Point, Rect, StrokeStyle};
use fern_core::accessibility::AccessNodeBuilder;
use fern_tokens::Color;

use crate::flags::ItemFlags;
use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};

/// An arbitrary vector path with optional fill and stroke, in local
/// item coordinates.
///
/// The path's commands are evaluated in local space. Stroke widths
/// scale with view zoom. The caller-provided `local_bounds` AABB is
/// what the spatial index buckets on; it must enclose the path's
/// strokes (including stroke half-width on each side).
#[derive(Debug)]
pub struct PathItem {
    path: Path,
    local_bounds: Rect,
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    label: Option<String>,
    flags: ItemFlags,
    a11y: ItemA11yOverrides,
}

impl PathItem {
    /// A path with a caller-provided AABB in local coordinates. The
    /// path's points are interpreted as local — `(0, 0)` is the
    /// item's anchor.
    pub fn new(path: Path, local_bounds: Rect) -> Self {
        Self {
            path,
            local_bounds,
            fill: None,
            stroke: None,
            label: None,
            flags: ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Stroke color and width (scene-coord pixels).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
        self
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Untranslated twin of [`label`](Self::label).
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(fern_i18n::LocalizedString::literal(label))
    }

    /// Opt the path into drag-to-move.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.flags.set(ItemFlags::IS_DRAGGABLE, draggable);
        self
    }

    crate::items::item_a11y_builders!();
}

impl SceneItem for PathItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        // The path's geometry is in local coords and stays fixed; only
        // the AABB tracks. Apps that want to *move* a path move the
        // item via `Scene::set_local_pos`. Apps that want to *resize*
        // a path rebuild the item from scratch.
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if let Some(fill) = self.fill {
            canvas.fill_path(&self.path, fill);
        }
        if let Some((color, width)) = self.stroke {
            canvas.stroke_path(&self.path, color, StrokeStyle::solid(width));
        }
    }

    fn shape_contains(&self, local_pt: Point) -> bool {
        // Stroke-only paths use per-segment distance to match what
        // users see; filled and mixed fill+stroke paths use AABB
        // (the fill region is the dominant target). Quad/cubic/arc
        // segments fall back to AABB.
        let stroke_width = match self.stroke {
            Some((_, w)) => w,
            None => return self.local_bounds.contains(local_pt),
        };
        if self.fill.is_some() {
            return self.local_bounds.contains(local_pt);
        }
        let tolerance = stroke_width.max(0.0) * 0.5 + 2.0;
        let mut current = Point::ZERO;
        let mut start = Point::ZERO;
        for cmd in &self.path.commands {
            match cmd {
                fern_canvas::PathCommand::MoveTo(p) => {
                    current = *p;
                    start = *p;
                }
                fern_canvas::PathCommand::LineTo(p) => {
                    if point_to_segment_distance(local_pt, current, *p) <= tolerance {
                        return true;
                    }
                    current = *p;
                }
                fern_canvas::PathCommand::Close => {
                    if point_to_segment_distance(local_pt, current, start) <= tolerance {
                        return true;
                    }
                    current = start;
                }
                _ => return self.local_bounds.contains(local_pt),
            }
        }
        false
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn initial_flags(&self) -> ItemFlags {
        self.flags
    }

    fn access_subtree_mode(&self) -> AccessSubtreeMode {
        self.a11y.subtree_mode()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(Role::GraphicsObject);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }
}

/// Shortest distance from a point to a line segment.
fn point_to_segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len2 = abx * abx + aby * aby;
    if len2 < 1e-6 {
        let dx = p.x - a.x;
        let dy = p.y - a.y;
        return (dx * dx + dy * dy).sqrt();
    }
    let apx = p.x - a.x;
    let apy = p.y - a.y;
    let t = ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0);
    let cx = a.x + t * abx;
    let cy = a.y + t * aby;
    let dx = p.x - cx;
    let dy = p.y - cy;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_item_holds_path_and_local_bounds() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 50.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 50.0)).stroke(Color::BLACK, 1.5);
        assert_eq!(item.local_bounds(), Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn path_item_per_segment_shape_contains_stroke_only() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 100.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0)).stroke(Color::BLACK, 2.0);

        assert!(item.shape_contains(Point::new(50.0, 50.0)));
        assert!(item.shape_contains(Point::new(52.0, 50.0)));
        assert!(!item.shape_contains(Point::new(80.0, 20.0)));
        assert!(!item.shape_contains(Point::new(200.0, 200.0)));
    }

    #[test]
    fn path_item_filled_uses_aabb_shape_contains() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 100.0))
            .line_to(Point::new(0.0, 100.0))
            .close();
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0)).fill(Color::RED);
        assert!(item.shape_contains(Point::new(50.0, 50.0)));
        assert!(!item.shape_contains(Point::new(200.0, 50.0)));
    }

    #[test]
    fn path_item_close_segment_hit_tested() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(50.0, 100.0))
            .close();
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0)).stroke(Color::BLACK, 2.0);
        assert!(item.shape_contains(Point::new(25.0, 50.0)));
    }

    #[test]
    fn path_item_curve_falls_back_to_aabb() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0)).stroke(Color::BLACK, 2.0);
        assert!(item.shape_contains(Point::new(50.0, 99.0)));
    }
}
