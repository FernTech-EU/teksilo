// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`PathItem`] — vector path with optional fill and stroke.
//!
//! `PathItem` renders an arbitrary vector path in local item coordinates.
//! The path can be filled, stroked, or both. Stroke-only paths use a
//! per-segment distance hit-test so users can click precisely along the
//! stroke even when the axis-aligned bounding box is huge — making this
//! the natural workhorse for connector lines between cards in a node graph
//! or story corkboard.
//!
//! Strokes come in two flavours: a **logical** stroke (`.stroke`) scales
//! with the view zoom, making thick scene-space edges; a **cosmetic** stroke
//! (`.stroke_cosmetic`) holds a constant device-pixel width at any zoom,
//! ideal for hairline connector wires that should stay crisp and thin.
//!
//! ## When to use
//!
//! Use `PathItem` for connector lines, polygon overlays, freehand shapes,
//! or any vector decoration that needs exact-shape click detection along its
//! stroke. For solid rectangular regions, prefer the cheaper [`RectItem`](crate::RectItem).
//!
//! ## Example
//!
//! ```ignore
//! use teksilo_scene::{SceneModel, PathItem};
//! use teksilo_canvas::{Path, Point, Rect};
//! use teksilo_tokens::Color;
//!
//! let model = SceneModel::new();
//!
//! let mut path = Path::new();
//! path.move_to(Point::new(0.0, 0.0))
//!     .line_to(Point::new(200.0, 0.0))
//!     .line_to(Point::new(200.0, 100.0));
//!
//! let item = PathItem::new(path, Rect::new(0.0, 0.0, 200.0, 100.0))
//!     .stroke_cosmetic(Color::new(0.3, 0.3, 0.3, 1.0), 1.5);
//!
//! model.add_item(item, Point::new(50.0, 50.0));
//! ```

use accesskit::Role;
use teksilo_canvas::{Canvas, Path, Point, Rect, StrokeSpace, StrokeStyle};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::Color;

use crate::flags::ItemFlags;
use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};
use teksilo_i18n::LocalizedString;

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
    fill: Option<ColorProp>,
    stroke: Option<(ColorProp, StrokeStyle)>,
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

    /// Fill colour. Accepts a plain [`Color`], a theme role, a
    /// `Signal<Color>`, or a `Signal<Role>` — resolved against the active
    /// theme at paint time.
    pub fn fill(mut self, color: impl Into<ColorProp>) -> Self {
        self.fill = Some(color.into());
        self
    }

    /// Stroke colour and width in **scene-coordinate** pixels — the stroke
    /// scales with the view zoom.
    pub fn stroke(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.stroke = Some((color.into(), StrokeStyle::solid(width.max(0.0))));
        self
    }

    /// Cosmetic stroke: the connector holds a constant **device-pixel** width
    /// at any zoom (it never thins out or thickens). The renderer keeps the
    /// path body sharp at the current zoom, so joins/caps stay correct.
    pub fn stroke_cosmetic(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.stroke = Some((color.into(), StrokeStyle::hairline(width.max(0.0))));
        self
    }

    /// Stroke with an explicit [`StrokeStyle`] — dashed, dotted, or custom caps
    /// / joins. E.g. `.stroke_styled(color, StrokeStyle::dashed(2.0, 6.0, 4.0))`
    /// distinguishes a pending connector from a solid confirmed one. The style
    /// is stored verbatim (dash pattern/offset, `Logical` vs `Device` space).
    pub fn stroke_styled(mut self, color: impl Into<ColorProp>, style: StrokeStyle) -> Self {
        self.stroke = Some((color.into(), style));
        self
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
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

    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>) {
        if let Some(prop) = &self.fill {
            canvas.fill_path(&self.path, prop.resolve(ctx.theme, ctx.enabled));
        }
        if let Some((prop, style)) = &self.stroke {
            canvas.stroke_path(
                &self.path,
                prop.resolve(ctx.theme, ctx.enabled),
                style.clone(),
            );
        }
    }

    fn set_fill(&mut self, fill: Option<ColorProp>) -> bool {
        self.fill = fill;
        true
    }

    fn set_stroke(&mut self, stroke: Option<(ColorProp, StrokeStyle)>) -> bool {
        self.stroke = stroke;
        true
    }

    fn register_bindings(&self, ctx: &mut BuildContext, view_id: WidgetId) {
        let registry = ctx.binding_registry();
        if let Some(p) = &self.fill {
            p.register_if_bound(view_id, registry, BindingLevel::RepaintOnly);
        }
        if let Some((p, _)) = &self.stroke {
            p.register_if_bound(view_id, registry, BindingLevel::RepaintOnly);
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
        // paths; fill takes precedence when present. Role-based colours
        // have no theme here, so they fall through to the neutral grey.
        crate::items::fill_or_stroke_hint(self.fill.as_ref(), self.stroke.as_ref())
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
            teksilo_canvas::PathCommand::MoveTo(p) => {
                current = *p;
                start = *p;
            }
            teksilo_canvas::PathCommand::LineTo(p) => {
                if point_to_segment_distance(local_pt, current, *p) <= tolerance {
                    return true;
                }
                current = *p;
            }
            teksilo_canvas::PathCommand::Close => {
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
    fn path_item_stroke_styled_stores_dash_pattern() {
        // #5: a dashed connector keeps its dash pattern verbatim.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 4.0))
            .stroke_styled(Color::BLACK, StrokeStyle::dashed(2.0, 6.0, 4.0));
        let (_, style) = item.stroke.as_ref().expect("stroke set");
        assert!(style.dash_pattern.is_some(), "dashed stroke keeps pattern");
    }

    #[test]
    fn path_item_paint_resolves_colours() {
        // #1/#2: fill + stroke resolve against the ctx theme and emit.
        let theme = teksilo_core::presets::intui::light();
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(50.0, 50.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 50.0, 50.0)).stroke(Color::RED, 2.0);
        let mut canvas = teksilo_canvas::Canvas::new();
        let ctx = SceneItemPaintContext::new(teksilo_canvas::Transform2D::identity(), None, &theme);
        item.paint(&mut canvas, &ctx);
        assert!(!canvas.into_render_frame().draw_order.is_empty());
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
