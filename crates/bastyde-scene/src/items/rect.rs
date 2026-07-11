// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`RectItem`] — filled / stroked rectangle in local item coords.
//!
//! `RectItem` is the simplest and cheapest lightweight scene item: a rectangle
//! in local item coordinates with an optional fill and/or stroke. It uses the
//! default AABB hit-test (exact for a rectangle) and has zero arena overhead.
//!
//! Like all lightweight items, `RectItem` is constructed with its geometry
//! relative to a local origin (`Rect::new(0.0, 0.0, w, h)`) and placed in
//! the scene by `Scene::add_item(item, scene_pos)`, where `scene_pos` becomes
//! the item's anchor in scene coordinates.
//!
//! Fill and stroke colours are [`ColorProp`]s, so they accept a plain
//! [`Color`](bastyde_tokens::Color), a theme role
//! ([`SurfaceRole`](bastyde_tokens::SurfaceRole) / `TextRole` / `BorderRole`),
//! a reactive `Signal<Color>`, or a `Signal<Role>` — resolved against the
//! active theme at paint time (so role-based fills desaturate automatically in
//! an inactive window). Change a colour live via
//! [`SceneModel::set_item_fill`](crate::SceneModel::set_item_fill) /
//! [`set_item_stroke`](crate::SceneModel::set_item_stroke).
//!
//! ## When to use
//!
//! Use `RectItem` for background tiles, card backgrounds, selection highlights,
//! grid cells, or any rectangular decoration in the lightweight tier. For
//! arbitrary shapes, use [`PathItem`](crate::PathItem); for interactive content needing focus
//! or event handlers, embed a full widget with `Scene::add_widget`.
//!
//! ## Example
//!
//! ```ignore
//! use bastyde_scene::{SceneModel, RectItem};
//! use bastyde_canvas::{Point, Rect};
//! use bastyde_tokens::Color;
//! use bastyde_i18n::lit;
//!
//! let model = SceneModel::new();
//!
//! let item = RectItem::new(Rect::new(0.0, 0.0, 120.0, 80.0))
//!     .fill(Color::new(0.9, 0.95, 1.0, 1.0))
//!     .corner_radius(8.0)
//!     .stroke_cosmetic(Color::new(0.6, 0.7, 0.85, 1.0), 1.0)
//!     .label(lit!("Card background"))
//!     .draggable(true);
//!
//! model.add_item(item, Point::new(40.0, 40.0));
//! ```

use accesskit::Role;
use bastyde_canvas::{Canvas, Rect, StrokeStyle};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

use crate::flags::ItemFlags;
use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};
use bastyde_i18n::LocalizedString;

/// A rectangle with optional fill and stroke, in local item coordinates.
///
/// Construct with `RectItem::new(Rect::new(0.0, 0.0, w, h))` and place
/// in the scene via `Scene::add_item(rect, local_pos)`.
#[derive(Debug)]
pub struct RectItem {
    local_bounds: Rect,
    fill: Option<ColorProp>,
    stroke: Option<(ColorProp, StrokeStyle)>,
    corner_radius: f32,
    label: Option<String>,
    flags: ItemFlags,
    a11y: ItemA11yOverrides,
}

impl RectItem {
    /// A rectangle of the given size in local item coordinates. The
    /// passed `local_bounds` is stored verbatim — typically
    /// `Rect::new(0.0, 0.0, w, h)`. No fill, no stroke — set at least
    /// one or the item is invisible.
    pub fn new(local_bounds: Rect) -> Self {
        Self {
            local_bounds,
            fill: None,
            stroke: None,
            corner_radius: 0.0,
            label: None,
            flags: ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Fill colour. Accepts a plain [`Color`](bastyde_tokens::Color), a theme
    /// role, a `Signal<Color>`, or a `Signal<Role>` — resolved against the
    /// active theme at paint time.
    pub fn fill(mut self, color: impl Into<ColorProp>) -> Self {
        self.fill = Some(color.into());
        self
    }

    /// Stroke colour and width in **scene-coordinate** pixels — the border
    /// scales with the view zoom (a 1px border becomes 2px at 2× zoom).
    pub fn stroke(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.stroke = Some((color.into(), StrokeStyle::solid(width.max(0.0))));
        self
    }

    /// Cosmetic stroke: the border holds a constant **device-pixel** width at
    /// any zoom (a hairline that never thins out or thickens). Ideal for grid
    /// cells and card outlines in a pannable/zoomable scene.
    pub fn stroke_cosmetic(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.stroke = Some((color.into(), StrokeStyle::hairline(width.max(0.0))));
        self
    }

    /// Stroke with an explicit [`StrokeStyle`] — dashed, dotted, or custom caps
    /// / joins. E.g. `.stroke_styled(color, StrokeStyle::dashed(2.0, 6.0, 4.0))`
    /// for a dashed outline, or `StrokeStyle::dotted(1.5, 3.0)` for a dotted
    /// guide. The style is stored verbatim, so all of `StrokeStyle`'s knobs
    /// (dash pattern/offset, `Logical` vs `Device` space) apply.
    pub fn stroke_styled(mut self, color: impl Into<ColorProp>, style: StrokeStyle) -> Self {
        self.stroke = Some((color.into(), style));
        self
    }

    /// Rounded corners for fill and stroke, in scene-coordinate pixels.
    /// Default `0.0` (square corners). A positive radius routes fill/stroke
    /// through the SDF rounded-rect path.
    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius.max(0.0);
        self
    }

    /// Human-readable label used for debug and the default AT name.
    /// Accepts anything convertible into `LocalizedString` — most
    /// commonly `tr!(...)`. Plain strings auto-convert.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Opt the rectangle into drag-to-move.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.flags.set(ItemFlags::IS_DRAGGABLE, draggable);
        self
    }

    crate::items::item_a11y_builders!();
}

impl SceneItem for RectItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>) {
        let lb = self.local_bounds;
        let radius = self.corner_radius;
        if let Some(prop) = &self.fill {
            let fill = prop.resolve(ctx.theme, ctx.enabled);
            if radius > 0.0 {
                canvas.fill_rounded_rect(lb, CornerRadius::uniform(radius), fill);
            } else {
                canvas.fill_rect(lb, fill);
            }
        }
        if let Some((prop, style)) = &self.stroke {
            let color = prop.resolve(ctx.theme, ctx.enabled);
            if radius > 0.0 {
                canvas.stroke_rounded_rect(lb, CornerRadius::uniform(radius), color, style.clone());
            } else {
                canvas.stroke_rect(lb, color, style.clone());
            }
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

    fn thumbnail_color(&self) -> bastyde_tokens::Color {
        // Fill dominates; fall through to stroke; fall through to the default
        // grey if the rect has no visible chrome or its colour is role-based
        // (role colours can't resolve without a theme here).
        crate::items::fill_or_stroke_hint(self.fill.as_ref(), self.stroke.as_ref())
            .unwrap_or_else(|| bastyde_tokens::Color::new(0.6, 0.6, 0.6, 1.0))
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
    use bastyde_canvas::{Canvas, Point, Transform2D};
    use bastyde_core::signal::Signal;
    use bastyde_tokens::{Color, SurfaceRole};

    fn test_ctx<'a>(theme: &'a bastyde_core::styles::Theme) -> SceneItemPaintContext<'a> {
        SceneItemPaintContext::new(Transform2D::identity(), None, theme)
    }

    #[test]
    fn rect_item_local_bounds_round_trip() {
        let r = Rect::new(0.0, 0.0, 30.0, 40.0);
        let item = RectItem::new(r);
        assert_eq!(item.local_bounds(), r);
    }

    #[test]
    fn rect_item_default_shape_contains() {
        let item = RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0));
        assert!(item.shape_contains(Point::new(20.0, 20.0)));
        assert!(!item.shape_contains(Point::new(-5.0, 20.0)));
    }

    #[test]
    fn rect_item_paint_emits_fill_and_stroke() {
        let theme = bastyde_core::presets::intui::light();
        let mut canvas = Canvas::new();
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .fill(Color::RED)
            .stroke(Color::BLUE, 2.0);
        item.paint(&mut canvas, &test_ctx(&theme));
        let frame = canvas.into_render_frame();
        assert!(
            !frame.draw_order.is_empty(),
            "paint must emit at least one draw command"
        );
    }

    #[test]
    fn rect_item_static_fill_paints_its_colour() {
        let theme = bastyde_core::presets::intui::light();
        let mut canvas = Canvas::new();
        RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .fill(Color::RED)
            .paint(&mut canvas, &test_ctx(&theme));
        let frame = canvas.into_render_frame();
        assert!(
            frame
                .decorations
                .iter()
                .any(|d| d.color == Color::RED.to_array()),
            "static fill must emit its exact colour"
        );
    }

    #[test]
    fn rect_item_role_fill_resolves_against_theme() {
        // #1 keystone: the paint ctx carries the theme, so a role fill resolves
        // to the theme's surface colour rather than a frozen constant.
        let theme = bastyde_core::presets::intui::light();
        let expected = ColorProp::from(SurfaceRole::Sunken).resolve(&theme, true);
        let mut canvas = Canvas::new();
        RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .fill(SurfaceRole::Sunken)
            .paint(&mut canvas, &test_ctx(&theme));
        let frame = canvas.into_render_frame();
        assert!(
            frame
                .decorations
                .iter()
                .any(|d| d.color == expected.to_array()),
            "role fill must resolve against ctx.theme"
        );
    }

    #[test]
    fn rect_item_signal_fill_re_resolves_on_change() {
        // #2 reactive: a Signal<Color> fill re-resolves each paint.
        let theme = bastyde_core::presets::intui::light();
        let sig = Signal::new(Color::GREEN);
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(sig.clone());

        let mut c1 = Canvas::new();
        item.paint(&mut c1, &test_ctx(&theme));
        assert!(
            c1.into_render_frame()
                .decorations
                .iter()
                .any(|d| d.color == Color::GREEN.to_array())
        );

        sig.set(Color::RED);
        let mut c2 = Canvas::new();
        item.paint(&mut c2, &test_ctx(&theme));
        assert!(
            c2.into_render_frame()
                .decorations
                .iter()
                .any(|d| d.color == Color::RED.to_array()),
            "signal fill must re-resolve to the new value"
        );
    }

    #[test]
    fn rect_item_corner_radius_emits_rounded_shape() {
        // #4: a positive corner radius routes the fill through the SDF
        // rounded-rect path (a Shape), not a plain rect Decoration.
        let theme = bastyde_core::presets::intui::light();
        let mut canvas = Canvas::new();
        RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0))
            .fill(Color::RED)
            .corner_radius(6.0)
            .paint(&mut canvas, &test_ctx(&theme));
        let frame = canvas.into_render_frame();
        assert!(
            !frame.shapes.is_empty(),
            "rounded fill must emit an SDF shape"
        );
    }

    #[test]
    fn rect_item_stroke_styled_stores_dash_pattern() {
        // #5: a styled stroke stores the caller's StrokeStyle verbatim.
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .stroke_styled(Color::BLUE, StrokeStyle::dashed(2.0, 6.0, 4.0));
        let (_, style) = item.stroke.as_ref().expect("stroke set");
        assert!(
            style.dash_pattern.is_some(),
            "dashed stroke must keep its dash pattern"
        );
    }

    #[test]
    fn rect_item_set_fill_replaces_colour() {
        // #2: the SceneItem mutation hook swaps the fill in place.
        let theme = bastyde_core::presets::intui::light();
        let mut item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(Color::RED);
        assert!(item.set_fill(Some(ColorProp::from(Color::BLUE))));
        let mut canvas = Canvas::new();
        item.paint(&mut canvas, &test_ctx(&theme));
        assert!(
            canvas
                .into_render_frame()
                .decorations
                .iter()
                .any(|d| d.color == Color::BLUE.to_array())
        );
    }
}
