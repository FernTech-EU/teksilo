// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`PathItem`] — vector path with optional fill and stroke.
//!
//! `PathItem` renders an arbitrary vector path in local item coordinates.
//! The path can be filled, stroked, or both, and it hit-tests as exactly that
//! union: a stroke-only path is clickable along its drawn line and nowhere
//! else, so a connector whose bounding box is mostly empty does not swallow
//! clicks aimed past it — making this the natural workhorse for connector
//! lines between cards in a node graph or story corkboard.
//!
//! Strokes come in two flavours: a **logical** stroke (`.stroke`) scales
//! with the view zoom, making thick scene-space edges; a **cosmetic** stroke
//! (`.stroke_cosmetic`) holds a constant device-pixel width at any zoom,
//! ideal for hairline connector wires that should stay crisp and thin. A
//! cosmetic stroke's *clickable band* follows the rendered line at any zoom,
//! because the width is converted from device pixels at test time.
//!
//! ## Bounds are derived, not declared
//!
//! A `PathItem` computes its own `local_bounds` from its geometry (plus the
//! stroke's half-width and the grab slack). There is no caller-supplied AABB
//! to get wrong, which is what makes the rectangle the spatial index buckets
//! on and the shape the narrow phase tests provably the same geometry.
//! [`Scene::set_local_bounds`](crate::Scene::set_local_bounds) therefore
//! **moves and scales the path** to fit the rectangle you give it, and the
//! item re-derives its bounds from the result — landing on the rectangle you
//! asked for on every axis the path has extent on, and staying there if you
//! ask again. On an axis it has none — a perfectly horizontal stroke has no
//! height — the box is the band's own thickness and no request can widen it.
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
//! use teksilo_canvas::{Path, Point};
//! use teksilo_tokens::Color;
//!
//! let model = SceneModel::new();
//!
//! let mut path = Path::new();
//! path.move_to(Point::new(0.0, 0.0))
//!     .line_to(Point::new(200.0, 0.0))
//!     .line_to(Point::new(200.0, 100.0));
//!
//! let item = PathItem::new(path)
//!     .stroke_cosmetic(Color::new(0.3, 0.3, 0.3, 1.0), 1.5)
//!     .hit_stroke_width(12.0);
//!
//! model.add_item(item, Point::new(50.0, 50.0));
//! ```

use std::rc::Rc;

use accesskit::Role;
use teksilo_canvas::{Canvas, FillRule, Path, Rect, StrokeStyle, Transform2D};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::Color;

use crate::flags::ItemFlags;
use crate::item::{AppearanceWrite, SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};
use crate::shape::{HIT_BAND_SLACK, ItemShape, ShapeGeometry};
use teksilo_i18n::LocalizedString;

/// An arbitrary vector path with optional fill and stroke, in local
/// item coordinates.
///
/// The path's commands are evaluated in local space. A logical stroke scales
/// with the view zoom; a [`stroke_cosmetic`](Self::stroke_cosmetic) stroke
/// holds a constant device-pixel width at any zoom (crisp connectors). The
/// item's `local_bounds` — the rectangle the spatial index buckets on — is
/// **derived** from the geometry and the stroke, so it always encloses what
/// the item can be clicked on.
#[derive(Debug)]
pub struct PathItem {
    /// The path plus its memoised flattening. Shared by handle so publishing
    /// the item's [`SceneItem::shape`] every layout pass costs a refcount
    /// bump, not a copy of the command list.
    geometry: Rc<ShapeGeometry>,
    local_bounds: Rect,
    fill: Option<ColorProp>,
    /// The rule used to fill **and** to hit-test the interior. One field, so a
    /// ring painted with a hole is a ring you can click through.
    fill_rule: FillRule,
    stroke: Option<(ColorProp, StrokeStyle)>,
    /// Hit-only band width, independent of the painted stroke.
    hit_stroke_width: Option<f32>,
    label: Option<String>,
    flags: ItemFlags,
    a11y: ItemA11yOverrides,
}

impl PathItem {
    /// A path in local coordinates — `(0, 0)` is the item's anchor.
    ///
    /// `local_bounds` is derived from the path (and re-derived whenever the
    /// stroke or hit band changes), so it always encloses the clickable area.
    pub fn new(path: Path) -> Self {
        let mut item = Self {
            geometry: ShapeGeometry::shared(path),
            local_bounds: Rect::ZERO,
            fill: None,
            fill_rule: FillRule::Winding,
            stroke: None,
            hit_stroke_width: None,
            label: None,
            flags: ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        };
        item.recompute_bounds();
        item
    }

    /// Fill colour. Accepts a plain [`Color`], a theme role, a
    /// `Signal<Color>`, or a `Signal<Role>` — resolved against the active
    /// theme at paint time.
    pub fn fill(mut self, color: impl Into<ColorProp>) -> Self {
        self.fill = Some(color.into());
        self
    }

    /// The [`FillRule`] used for **both** painting the fill and hit-testing
    /// the interior.
    ///
    /// [`FillRule::EvenOdd`] gives a ring authored as two subpaths a real
    /// hole — one that is transparent *and* clicks through. There is
    /// deliberately no hit-only fill rule: a shape that paints as a disc and
    /// hit-tests as a ring is the two-sources-of-truth bug this whole type
    /// exists to prevent.
    pub fn fill_rule(mut self, rule: FillRule) -> Self {
        self.fill_rule = rule;
        self
    }

    /// Stroke colour and width in **scene-coordinate** pixels — the stroke
    /// scales with the view zoom.
    pub fn stroke(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.stroke = Some((color.into(), StrokeStyle::solid(width.max(0.0))));
        self.recompute_bounds();
        self
    }

    /// Cosmetic stroke: the connector holds a constant **device-pixel** width
    /// at any zoom (it never thins out or thickens). The renderer keeps the
    /// path body sharp at the current zoom, so joins/caps stay correct.
    pub fn stroke_cosmetic(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.stroke = Some((color.into(), StrokeStyle::hairline(width.max(0.0))));
        self.recompute_bounds();
        self
    }

    /// Stroke with an explicit [`StrokeStyle`] — dashed, dotted, or custom caps
    /// / joins. E.g. `.stroke_styled(color, StrokeStyle::dashed(2.0, 6.0, 4.0))`
    /// distinguishes a pending connector from a solid confirmed one. The style
    /// is stored verbatim (dash pattern/offset, `Logical` vs `Device` space).
    pub fn stroke_styled(mut self, color: impl Into<ColorProp>, style: StrokeStyle) -> Self {
        self.stroke = Some((color.into(), style));
        self.recompute_bounds();
        self
    }

    /// Widen the clickable band without widening the drawn line (Konva's
    /// `hitStrokeWidth`): a 1 dp wire, 12 dp grabbable. In local units, so it
    /// is a target size rather than a rendered thickness. Also widens the
    /// derived `local_bounds`, so the broad phase keeps up with it.
    pub fn hit_stroke_width(mut self, width: f32) -> Self {
        self.hit_stroke_width = Some(width.max(0.0));
        self.recompute_bounds();
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

    /// The path's commands, in local coordinates.
    pub fn path(&self) -> &Path {
        self.geometry.path()
    }

    crate::items::item_a11y_builders!();

    /// The hit band's width, hit-only override first. `None` when the item
    /// has no band at all.
    fn band_width(&self) -> Option<f32> {
        self.hit_stroke_width
            .or(self.stroke.as_ref().map(|(_, s)| s.width))
    }

    /// How far the band inflates the geometry's own box on every side. The
    /// one number `local_bounds` and `set_local_bounds` both go through, so
    /// the box the setter lands on is the box the setter aimed at.
    fn band_inflate(&self) -> f32 {
        self.band_width()
            .map(|w| w.max(0.0) * 0.5 + HIT_BAND_SLACK)
            .unwrap_or(0.0)
    }

    /// Rebuild the derived AABB from the current geometry + band. Called by
    /// every builder that can change either. Reads the geometry directly
    /// rather than going through `shape()`, which would be circular: the
    /// no-fill-no-band shape *is* `local_bounds`.
    fn recompute_bounds(&mut self) {
        self.local_bounds = self.geometry.bounds().expand(self.band_inflate());
    }
}

impl SceneItem for PathItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    /// Fit the path to `bounds` — a translate and a scale — and re-derive the
    /// box from the moved geometry.
    ///
    /// # It aims at the geometry, not at the box
    ///
    /// The requested rectangle is the *whole* clickable box, band included, so
    /// the fit targets `bounds` **deflated by the band's half-width** and the
    /// re-derived box then lands back on `bounds` exactly. Mapping the old box
    /// onto the new one instead would re-add the band to a rectangle that
    /// already contained it, overshooting by the band on every call: asking
    /// three times for the same rectangle used to give three different
    /// answers, each closer than the last and none of them the one asked for,
    /// and each one re-bucketed the spatial index and emitted an
    /// `ItemChange::LocalBoundsChanged` with a new value. Idempotent now: the
    /// second identical call finds the fit is the identity and returns without
    /// touching the geometry — which also keeps the memoised flattening that
    /// [`ShapeGeometry`] exists for, instead of throwing it away per call.
    ///
    /// # What it cannot honour
    ///
    /// An axis the path has no extent on — a perfectly horizontal stroke, a
    /// single point — cannot be stretched to fill one, so the box stays the
    /// band's own thickness there however tall a rectangle is asked for. That
    /// is reported rather than hidden: [`Scene::set_local_bounds`](crate::Scene::set_local_bounds)
    /// reads the item's box back and stores what the item settled on.
    fn set_local_bounds(&mut self, bounds: Rect) {
        let inflate = self.band_inflate();
        // The geometry's share of the requested box. A request narrower than
        // the band itself asks for a negative extent; clamp rather than
        // mirror the path.
        let target = Rect::new(
            bounds.x + inflate,
            bounds.y + inflate,
            (bounds.width - 2.0 * inflate).max(0.0),
            (bounds.height - 2.0 * inflate).max(0.0),
        );
        let src = self.geometry.bounds();
        let sx = if src.width.abs() > 1e-6 {
            target.width / src.width
        } else {
            1.0
        };
        let sy = if src.height.abs() > 1e-6 {
            target.height / src.height
        } else {
            1.0
        };
        // Within a float epsilon of the identity: the box is already where it
        // was asked to be, and rebuilding the geometry would only churn.
        if (sx - 1.0).abs() < 1e-5
            && (sy - 1.0).abs() < 1e-5
            && (src.x - target.x).abs() < 1e-4
            && (src.y - target.y).abs() < 1e-4
        {
            return;
        }
        let fit = Transform2D::translate(-src.x, -src.y)
            .then(&Transform2D::scale(sx, sy))
            .then(&Transform2D::translate(target.x, target.y));
        self.geometry = ShapeGeometry::shared(self.geometry.path().transformed(&fit));
        self.recompute_bounds();
    }

    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>) {
        if let Some(prop) = &self.fill {
            canvas.fill_path_with_rule(
                self.geometry.path(),
                prop.resolve(ctx.theme, ctx.enabled),
                self.fill_rule,
            );
        }
        if let Some((prop, style)) = &self.stroke {
            canvas.stroke_path(
                self.geometry.path(),
                prop.resolve(ctx.theme, ctx.enabled),
                style.clone(),
            );
        }
    }

    fn set_fill(&mut self, fill: Option<ColorProp>) -> AppearanceWrite<ColorProp> {
        AppearanceWrite::Accepted {
            was: std::mem::replace(&mut self.fill, fill),
        }
    }

    fn set_stroke(
        &mut self,
        stroke: Option<(ColorProp, StrokeStyle)>,
    ) -> AppearanceWrite<(ColorProp, StrokeStyle)> {
        let was = std::mem::replace(&mut self.stroke, stroke);
        self.recompute_bounds();
        AppearanceWrite::Accepted { was }
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

    /// The union of whatever the item actually draws: its filled interior
    /// under [`PathItem::fill_rule`] when it has a fill, and a stroke band
    /// when it has a stroke (or an explicit
    /// [`hit_stroke_width`](PathItem::hit_stroke_width)).
    ///
    /// A path with **neither** falls back to its local AABB. It draws nothing,
    /// so there is no silhouette to derive one from, and an invisible
    /// rectangle that still catches clicks is a legitimate use — an enlarged
    /// grab area parked behind a thin connector. An item that wants clicks to
    /// fall *through* says so with [`ItemShape::none`], which is what a
    /// logical-only [`GroupItem`](crate::GroupItem) returns.
    fn shape(&self) -> ItemShape {
        if self.fill.is_none() && self.band_width().is_none() {
            return ItemShape::bounds(self.local_bounds);
        }
        let mut shape = ItemShape::path(self.geometry.clone());
        shape = if self.fill.is_some() {
            shape.filled(self.fill_rule)
        } else {
            shape.unfilled()
        };
        match self.hit_stroke_width {
            Some(w) => shape.hit_stroke_width(w),
            None => match self.stroke.as_ref() {
                Some((_, s)) => shape.stroked(s.width, s.space),
                None => shape,
            },
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::Point;

    #[test]
    fn path_item_derives_local_bounds_from_geometry() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 50.0));
        let item = PathItem::new(path).stroke(Color::BLACK, 2.0);
        // Geometry box (0,0,100,50) inflated by half the stroke plus the grab
        // slack: 1 + 2 = 3.
        assert_eq!(item.local_bounds(), Rect::new(-3.0, -3.0, 106.0, 56.0));
    }

    #[test]
    fn path_item_local_bounds_always_enclose_its_shape() {
        // The one invariant that makes a broad phase on `local_bounds` and a
        // narrow phase on `shape()` provably the same geometry. Delete
        // `recompute_bounds` from `stroke()` and this reddens.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
        for item in [
            PathItem::new(path.clone()),
            PathItem::new(path.clone()).stroke(Color::BLACK, 6.0),
            PathItem::new(path.clone()).stroke_cosmetic(Color::BLACK, 1.0),
            PathItem::new(path.clone()).fill(Color::RED),
            PathItem::new(path.clone())
                .stroke(Color::BLACK, 1.0)
                .hit_stroke_width(20.0),
        ] {
            let lb = item.local_bounds();
            let sb = item.shape().bounding_rect();
            assert!(
                lb.x <= sb.x + 1e-3
                    && lb.y <= sb.y + 1e-3
                    && lb.right() >= sb.right() - 1e-3
                    && lb.bottom() >= sb.bottom() - 1e-3,
                "local_bounds {lb:?} must enclose shape bounds {sb:?}"
            );
        }
    }

    #[test]
    fn path_item_hit_tests_per_segment_when_stroke_only() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 100.0));
        let item = PathItem::new(path).stroke(Color::BLACK, 2.0);
        let shape = item.shape();

        assert!(shape.contains(Point::new(50.0, 50.0), 1.0));
        assert!(shape.contains(Point::new(52.0, 50.0), 1.0));
        assert!(!shape.contains(Point::new(80.0, 20.0), 1.0));
        assert!(!shape.contains(Point::new(200.0, 200.0), 1.0));
    }

    #[test]
    fn path_item_filled_hit_tests_its_interior_not_its_box() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(0.0, 100.0))
            .close();
        let item = PathItem::new(path).fill(Color::RED);
        let shape = item.shape();
        assert!(
            shape.contains(Point::new(20.0, 20.0), 1.0),
            "inside triangle"
        );
        // Inside the AABB, outside the triangle — this used to hit.
        assert!(
            !shape.contains(Point::new(90.0, 90.0), 1.0),
            "the empty corner of a triangle's box is not the triangle"
        );
        assert!(!shape.contains(Point::new(200.0, 50.0), 1.0));
    }

    #[test]
    fn path_item_fill_rule_drives_paint_and_hit_together() {
        // Two concentric squares. EvenOdd punches a hole; the hole must be
        // both un-painted and un-clickable, from the same one field.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 100.0))
            .line_to(Point::new(0.0, 100.0))
            .close();
        path.move_to(Point::new(25.0, 25.0))
            .line_to(Point::new(75.0, 25.0))
            .line_to(Point::new(75.0, 75.0))
            .line_to(Point::new(25.0, 75.0))
            .close();

        let solid = PathItem::new(path.clone()).fill(Color::RED);
        assert!(solid.shape().contains(Point::new(50.0, 50.0), 1.0));

        let ring = PathItem::new(path)
            .fill(Color::RED)
            .fill_rule(FillRule::EvenOdd);
        assert!(
            !ring.shape().contains(Point::new(50.0, 50.0), 1.0),
            "the hole must not be clickable"
        );
        assert!(
            ring.shape().contains(Point::new(10.0, 50.0), 1.0),
            "the band is"
        );

        // ... and the SAME rule reaches the painted fill.
        let theme = teksilo_core::presets::intui::light();
        let mut canvas = Canvas::new();
        let ctx = SceneItemPaintContext::new(Transform2D::identity(), None, &theme);
        ring.paint(&mut canvas, &ctx);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.paths[0].fill_rule, FillRule::EvenOdd);
    }

    #[test]
    fn path_item_close_segment_hit_tested() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(50.0, 100.0))
            .close();
        let item = PathItem::new(path).stroke(Color::BLACK, 2.0);
        assert!(item.shape().contains(Point::new(25.0, 50.0), 1.0));
    }

    #[test]
    fn path_item_curve_hit_tests_exactly() {
        // This is the inversion of the old `path_item_curve_falls_back_to_aabb`.
        // A quadratic with control (50,100) reaches y = 50 at its apex, so a
        // point at y = 99 is 49 units off the stroke and must MISS — the old
        // per-segment walk bailed to the AABB on the first curve command and
        // reported a hit there.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
        let item = PathItem::new(path).stroke(Color::BLACK, 2.0);
        let shape = item.shape();
        assert!(!shape.contains(Point::new(50.0, 99.0), 1.0));
        assert!(shape.contains(Point::new(50.0, 50.0), 1.0), "the apex hits");
    }

    #[test]
    fn path_item_with_neither_fill_nor_stroke_keeps_its_box() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 100.0));
        let item = PathItem::new(path);
        assert!(item.shape().contains(Point::new(90.0, 10.0), 1.0));
    }

    #[test]
    fn path_item_hit_stroke_width_widens_the_grab_band() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0));
        let thin = PathItem::new(path.clone()).stroke(Color::BLACK, 1.0);
        assert!(!thin.shape().contains(Point::new(50.0, 5.0), 1.0));
        let fat = PathItem::new(path)
            .stroke(Color::BLACK, 1.0)
            .hit_stroke_width(12.0);
        assert!(fat.shape().contains(Point::new(50.0, 5.0), 1.0));
    }

    #[test]
    fn path_item_stroke_styled_stores_dash_pattern() {
        // #5: a dashed connector keeps its dash pattern verbatim.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0));
        let item =
            PathItem::new(path).stroke_styled(Color::BLACK, StrokeStyle::dashed(2.0, 6.0, 4.0));
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
        let item = PathItem::new(path).stroke(Color::RED, 2.0);
        let mut canvas = Canvas::new();
        let ctx = SceneItemPaintContext::new(Transform2D::identity(), None, &theme);
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
        let item = PathItem::new(path).stroke_cosmetic(Color::BLACK, 4.0);
        let shape = item.shape();
        let p = Point::new(50.0, 3.0);
        assert!(
            shape.contains(p, 1.0),
            "cosmetic band at 1x: width 4 → tolerance 4 → hit"
        );
        assert!(
            !shape.contains(p, 4.0),
            "cosmetic band shrinks at 4x: width 1 → tolerance 2.5 → miss"
        );

        // A LOGICAL stroke's width is already in scene units, so its band is
        // unaffected by the view scale (regression guard).
        let mut path2 = Path::new();
        path2
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0));
        let logical = PathItem::new(path2).stroke(Color::BLACK, 4.0);
        assert!(logical.shape().contains(p, 1.0), "logical band hit at 1x");
        assert!(
            logical.shape().contains(p, 4.0),
            "logical band unchanged by zoom"
        );
    }

    #[test]
    fn set_local_bounds_moves_and_scales_the_geometry() {
        // Study §1.9: the old impl updated the AABB and left the path where it
        // was, so the index bucketed a rectangle the geometry had never
        // occupied. The geometry follows now.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0));
        let mut item = PathItem::new(path).stroke(Color::BLACK, 0.0);
        let before = item.local_bounds();
        // Slide it 200 to the right, same size.
        item.set_local_bounds(Rect::new(
            before.x + 200.0,
            before.y,
            before.width,
            before.height,
        ));
        let shape = item.shape();
        assert!(
            shape.contains(Point::new(250.0, 0.0), 1.0),
            "moved with its box"
        );
        assert!(
            !shape.contains(Point::new(50.0, 0.0), 1.0),
            "left the old place"
        );
        let lb = item.local_bounds();
        let sb = shape.bounding_rect();
        assert!((lb.x - sb.x).abs() < 1e-3 && (lb.width - sb.width).abs() < 1e-3);
    }

    #[test]
    fn set_local_bounds_on_a_degenerate_box_translates_without_dividing_by_zero() {
        // A single-point path has a zero-size box on both axes; the fit
        // transform must degrade to a pure translate rather than divide by it.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        let mut item = PathItem::new(path);
        assert_eq!(item.local_bounds(), Rect::new(0.0, 0.0, 0.0, 0.0));
        item.set_local_bounds(Rect::new(10.0, 20.0, 0.0, 0.0));
        let lb = item.local_bounds();
        assert!(lb.width.is_finite() && lb.height.is_finite());
        assert!((lb.x - 10.0).abs() < 1e-3 && (lb.y - 20.0).abs() < 1e-3);
    }

    #[test]
    fn a_long_ink_stroke_hit_tests_exactly() {
        // Flattening removed the old "bail to the AABB on the first curve"
        // guard, so a 500-segment ink stroke would otherwise walk every
        // segment per pointer sample to conclude a miss. Two levels of
        // rejection stand in front of that walk now (the shape's own box, then
        // a box per run of segments); what this asserts is that neither of
        // them changes an answer — their arithmetic is pinned in `shape/tests`.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        for i in 1..=500 {
            let x = i as f32;
            path.line_to(Point::new(x, (x * 0.2).sin() * 40.0));
        }
        let item = PathItem::new(path).stroke(Color::BLACK, 1.0);
        let shape = item.shape();
        // Far outside the box.
        assert!(!shape.contains(Point::new(250.0, 400.0), 1.0));
        // Inside the box, between two arcs of the wave.
        assert!(!shape.contains(Point::new(250.0, 20.0), 1.0));
        // On the stroke.
        assert!(shape.contains(Point::new(250.0, (250.0f32 * 0.2).sin() * 40.0), 1.0));
    }
}
