//! [`PathItem`] — vector path with optional fill and stroke.
//!
//! Stroke-only paths use a per-segment distance hit-test so users can
//! click along the stroke even when the AABB is huge (the connector-
//! line workhorse).

use accesskit::Role;
use bastyde_canvas::{Canvas, Path, Point, Rect, StrokeSpace, StrokeStyle};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_tokens::Color;

use crate::flags::ItemFlags;
use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};

/// An arbitrary vector path with optional fill and stroke, in local
/// item coordinates.
///
/// The path's commands are evaluated in local space. A logical stroke scales
/// with the view zoom; a [`stroke_cosmetic`](Self::stroke_cosmetic) stroke
/// holds a constant device-pixel width at any zoom (crisp connectors). The
/// caller-provided `local_bounds` AABB is what the spatial index buckets on;
/// it must enclose the path's strokes (including stroke half-width on each
/// side).
#[derive(Debug)]
pub struct PathItem {
    path: Path,
    local_bounds: Rect,
    fill: Option<Color>,
    stroke: Option<(Color, StrokeStyle)>,
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

    /// Stroke color and width in **scene-coordinate** pixels — the stroke
    /// scales with the view zoom.
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, StrokeStyle::solid(width.max(0.0))));
        self
    }

    /// Cosmetic stroke: the connector holds a constant **device-pixel** width
    /// at any zoom (it never thins out or thickens). The renderer keeps the
    /// path body sharp at the current zoom, so joins/caps stay correct.
    pub fn stroke_cosmetic(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, StrokeStyle::hairline(width.max(0.0))));
        self
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Untranslated twin of [`label`](Self::label).
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(bastyde_i18n::LocalizedString::literal(label))
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
        if let Some((color, style)) = &self.stroke {
            canvas.stroke_path(&self.path, *color, style.clone());
        }
    }

    fn shape_contains(&self, local_pt: Point) -> bool {
        path_shape_contains(
            &self.path,
            self.local_bounds,
            self.fill.is_some(),
            self.stroke.as_ref().map(|(_, s)| s.width),
            local_pt,
        )
    }

    fn clone_shape_test(&self) -> Box<dyn Fn(Point, f32) -> bool + 'static> {
        // Capture the data needed for hit-test without holding a
        // borrow on `self`. The `SceneView` snapshot stores the
        // returned closure and consults it on every pointer event,
        // so we have to be cloneable and `'static`. `Path` is
        // `Clone`; the rest of the captured state is `Copy`.
        let path = self.path.clone();
        let local_bounds = self.local_bounds;
        let has_fill = self.fill.is_some();
        // (stroke width, is-cosmetic). A cosmetic stroke's width is in DEVICE
        // pixels, so its visual half-width in scene coordinates shrinks as the
        // view zooms in (and grows as it zooms out). Convert per-event using
        // the live view scale so the clickable band tracks the rendered line
        // at any zoom; a logical stroke's width is already in scene units.
        let stroke = self
            .stroke
            .as_ref()
            .map(|(_, s)| (s.width, s.space == StrokeSpace::Device));
        Box::new(move |local_pt, view_scale| {
            let scene_width = stroke.map(|(w, cosmetic)| {
                if cosmetic && view_scale > 1e-3 {
                    w / view_scale
                } else {
                    w
                }
            });
            path_shape_contains(&path, local_bounds, has_fill, scene_width, local_pt)
        })
    }

    fn thumbnail_color(&self) -> Color {
        // Connector-line and outline use cases dominate stroke-only
        // paths; fill takes precedence when present.
        self.fill
            .or_else(|| self.stroke.as_ref().map(|(c, _)| *c))
            .unwrap_or_else(|| Color::new(0.6, 0.6, 0.6, 1.0))
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

/// Hit-test logic shared between [`PathItem::shape_contains`] and
/// the snapshotted closure returned by
/// [`PathItem::clone_shape_test`]. Stroke-only paths walk each
/// segment and test point-to-segment distance against
/// `stroke_width/2 + 2px` tolerance; filled or mixed-fill paths
/// fall through to AABB; non-line segments (quad / cubic / arc)
/// fall through to AABB.
fn path_shape_contains(
    path: &Path,
    local_bounds: Rect,
    has_fill: bool,
    stroke_width: Option<f32>,
    local_pt: Point,
) -> bool {
    let stroke_width = match stroke_width {
        Some(w) => w,
        None => return local_bounds.contains(local_pt),
    };
    if has_fill {
        return local_bounds.contains(local_pt);
    }
    let tolerance = stroke_width.max(0.0) * 0.5 + 2.0;
    let mut current = Point::ZERO;
    let mut start = Point::ZERO;
    for cmd in &path.commands {
        match cmd {
            bastyde_canvas::PathCommand::MoveTo(p) => {
                current = *p;
                start = *p;
            }
            bastyde_canvas::PathCommand::LineTo(p) => {
                if point_to_segment_distance(local_pt, current, *p) <= tolerance {
                    return true;
                }
                current = *p;
            }
            bastyde_canvas::PathCommand::Close => {
                if point_to_segment_distance(local_pt, current, start) <= tolerance {
                    return true;
                }
                current = start;
            }
            _ => return local_bounds.contains(local_pt),
        }
    }
    false
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

    #[test]
    fn cosmetic_path_hit_band_tracks_zoom() {
        // A cosmetic stroke's width is in device px, so its scene-coord hit
        // band must shrink as the view zooms in. A point 3 scene-units off a
        // cosmetic 4px line is inside the band at 1× but outside at 4×.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0));
        let item =
            PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 8.0)).stroke_cosmetic(Color::BLACK, 4.0);
        let test = item.clone_shape_test();
        let p = Point::new(50.0, 3.0);
        assert!(
            test(p, 1.0),
            "cosmetic band at 1x: width 4 → tolerance 4 → hit"
        );
        assert!(
            !test(p, 4.0),
            "cosmetic band shrinks at 4x: width 1 → tolerance 2.5 → miss"
        );

        // A LOGICAL stroke's width is already in scene units, so its band is
        // unaffected by the view scale (regression guard).
        let mut path2 = Path::new();
        path2
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0));
        let logical =
            PathItem::new(path2, Rect::new(0.0, 0.0, 100.0, 8.0)).stroke(Color::BLACK, 4.0);
        let test_l = logical.clone_shape_test();
        assert!(test_l(p, 1.0), "logical band hit at 1x");
        assert!(test_l(p, 4.0), "logical band unchanged by zoom");
    }
}
