// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`GroupItem`] — labelled box / logical AT container.
//!
//! Visually a labelled rectangle with optional fill, stroke, and
//! inline label. Without any chrome it's a logical-only container
//! that announces itself to AT but draws nothing — the lightweight
//! analogue of an `A11yGroup`.
//!
//! ## When to use
//!
//! Use [`GroupItem`] when you need to:
//! - Draw a visible boundary box around a cluster of related items
//!   (e.g. a lane in a Kanban board, an "Act 1" region on a corkboard).
//! - Provide a named AT group that screen readers announce without
//!   any visible chrome — call [`GroupItem::label`] but omit `fill`
//!   and `stroke`, leaving `is_visual()` false.
//!
//! ## Example
//!
//! ```ignore
//! use bastyde_scene::{Scene, GroupItem};
//! use bastyde_canvas::{Point, Rect};
//! use bastyde_tokens::Color;
//! use bastyde_i18n::lit;
//!
//! let mut scene = Scene::new();
//! // A visible "Act 1" box with a rounded border.
//! let group = GroupItem::new(Rect::new(0.0, 0.0, 400.0, 600.0))
//!     .label(lit!("Act 1"))
//!     .show_label(true)
//!     .stroke(Color::new(0.6, 0.6, 0.6, 1.0), 1.5)
//!     .corner_radius(8.0);
//! let _id = scene.add_item(group, Point::new(20.0, 20.0));
//! ```

use accesskit::Role;
use bastyde_canvas::{Canvas, Point, Rect, StrokeStyle};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Color;

use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};
use bastyde_i18n::LocalizedString;

/// A group container with optional fill / stroke / inline label, in
/// local item coordinates.
///
/// Visually, GroupItem renders a labelled box around its members.
/// Logically, it's the AT-grouping primitive: with no chrome and a
/// label set, it announces itself to AT but draws nothing.
#[derive(Debug)]
pub struct GroupItem {
    local_bounds: Rect,
    label: Option<String>,
    show_label: bool,
    fill: Option<ColorProp>,
    stroke: Option<(ColorProp, StrokeStyle)>,
    corner_radius: f32,
    label_inset: (f32, f32),
    label_color: Option<ColorProp>,
    a11y: ItemA11yOverrides,
}

impl GroupItem {
    /// A group covering `local_bounds` in local coordinates. No
    /// chrome by default — call `fill` / `stroke` / `show_label` to
    /// give it visible outline / background / inline label.
    pub fn new(local_bounds: Rect) -> Self {
        Self {
            local_bounds,
            label: None,
            show_label: false,
            fill: None,
            stroke: None,
            corner_radius: 0.0,
            label_inset: (8.0, 4.0),
            label_color: None,
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Human-readable label, used as the default AT group name and
    /// (when `show_label` is enabled) rendered inline at top-leading.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Render the label inline at paint time.
    pub fn show_label(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }

    /// Override the inset of the inline label from the local origin.
    pub fn label_inset(mut self, dx: f32, dy: f32) -> Self {
        self.label_inset = (dx, dy);
        self
    }

    /// Override the inline label colour. Defaults to the stroke colour if set,
    /// else `Color::BLACK`. Accepts a plain [`Color`], a theme role, or a
    /// reactive signal.
    pub fn label_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.label_color = Some(color.into());
        self
    }

    /// Background fill colour. Accepts a plain [`Color`], a theme role, a
    /// `Signal<Color>`, or a `Signal<Role>` — resolved against the active theme
    /// at paint time.
    pub fn fill(mut self, color: impl Into<ColorProp>) -> Self {
        self.fill = Some(color.into());
        self
    }

    /// Border stroke (colour + scene-coord pixel width) — scales with zoom.
    pub fn stroke(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.stroke = Some((color.into(), StrokeStyle::solid(width.max(0.0))));
        self
    }

    /// Cosmetic border stroke: holds a constant **device-pixel** width at any
    /// zoom. With `corner_radius > 0` the rounded outline goes through the SDF
    /// cosmetic path; otherwise `stroke_rect` emits four `CosmeticLine` edges
    /// (one per side), which are hard-edged and crisp at any zoom.
    pub fn stroke_cosmetic(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.stroke = Some((color.into(), StrokeStyle::hairline(width.max(0.0))));
        self
    }

    /// Border stroke with an explicit [`StrokeStyle`] — dashed / dotted /
    /// custom caps. E.g. `.stroke_styled(color, StrokeStyle::dashed(2.0, 6.0, 4.0))`
    /// for a dashed lane boundary.
    pub fn stroke_styled(mut self, color: impl Into<ColorProp>, style: StrokeStyle) -> Self {
        self.stroke = Some((color.into(), style));
        self
    }

    /// Rounded corners for fill and stroke. Default `0.0`.
    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius.max(0.0);
        self
    }

    /// Whether the group has any visual chrome configured.
    pub fn is_visual(&self) -> bool {
        self.fill.is_some() || self.stroke.is_some() || self.show_label
    }

    crate::items::item_a11y_builders!();
}

impl SceneItem for GroupItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>) {
        if !self.is_visual() {
            return;
        }
        let lb = self.local_bounds;
        if let Some(prop) = &self.fill {
            let fill = prop.resolve(ctx.theme, ctx.enabled);
            if self.corner_radius > 0.0 {
                canvas.fill_rounded_rect(
                    lb,
                    bastyde_tokens::CornerRadius::uniform(self.corner_radius),
                    fill,
                );
            } else {
                canvas.fill_rect(lb, fill);
            }
        }
        if let Some((prop, style)) = &self.stroke {
            let color = prop.resolve(ctx.theme, ctx.enabled);
            if self.corner_radius > 0.0 {
                canvas.stroke_rounded_rect(
                    lb,
                    bastyde_tokens::CornerRadius::uniform(self.corner_radius),
                    color,
                    style.clone(),
                );
            } else {
                canvas.stroke_rect(lb, color, style.clone());
            }
        }
        if self.show_label
            && let Some(label) = &self.label
        {
            // Label colour: explicit override, else the stroke colour, else
            // black — each resolved against the active theme.
            let color = self
                .label_color
                .as_ref()
                .or_else(|| self.stroke.as_ref().map(|(c, _)| c))
                .map(|p| p.resolve(ctx.theme, ctx.enabled))
                .unwrap_or(Color::BLACK);
            let (dx, dy) = self.label_inset;
            let label_bounds = Rect::new(
                lb.x + dx,
                lb.y + dy,
                (lb.width - 2.0 * dx).max(0.0),
                (lb.height - 2.0 * dy).max(0.0),
            );
            canvas.draw_text(
                label,
                label_bounds,
                &bastyde_tokens::TextStyle::default(),
                color,
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
        if let Some(p) = &self.label_color {
            p.register_if_bound(view_id, registry, BindingLevel::RepaintOnly);
        }
    }

    /// Non-visual GroupItems pass clicks through to items beneath.
    /// Visual groups (with fill / stroke / inline label) AABB-hit-test
    /// so apps can wire group-level click handlers.
    fn shape_contains(&self, local_pt: Point) -> bool {
        if self.is_visual() {
            self.local_bounds.contains(local_pt)
        } else {
            false
        }
    }

    /// Override the default AABB-only snapshot: logical-only groups
    /// (no fill, no stroke, no inline label) must MISS for dispatch
    /// so clicks fall through to items beneath. Without this
    /// override the snapshot would AABB-hit and capture every event
    /// over the group's rect, blocking the items it contains.
    fn clone_shape_test(&self) -> Box<dyn Fn(Point, f32) -> bool + 'static> {
        let is_visual = self.is_visual();
        let bounds = self.local_bounds;
        Box::new(move |p, _view_scale| is_visual && bounds.contains(p))
    }

    fn thumbnail_color(&self) -> Color {
        // Visual groups: fill dominates, then stroke (role-based colours have
        // no theme here and fall through). Logical groups are invisible —
        // paint as fully transparent so minimap consumers can suppress them.
        if let Some(c) = crate::items::fill_or_stroke_hint(self.fill.as_ref(), self.stroke.as_ref())
        {
            return c;
        }
        if self.is_visual() {
            return Color::new(0.6, 0.6, 0.6, 1.0);
        }
        Color::new(0.0, 0.0, 0.0, 0.0)
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(Role::Group);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }

    fn access_subtree_mode(&self) -> AccessSubtreeMode {
        self.a11y.subtree_mode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::Transform2D;
    use bastyde_i18n::lit;

    #[test]
    fn group_item_does_not_hit_test_through_aabb() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 1000.0, 1000.0));
        assert!(!g.shape_contains(Point::new(500.0, 500.0)));
    }

    #[test]
    fn group_item_default_is_not_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!(!g.is_visual());
    }

    #[test]
    fn group_item_with_fill_is_visual_and_hit_tests() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(Color::RED);
        assert!(g.is_visual());
        assert!(g.shape_contains(Point::new(50.0, 50.0)));
        assert!(!g.shape_contains(Point::new(150.0, 50.0)));
    }

    #[test]
    fn group_item_with_stroke_only_is_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).stroke(Color::BLACK, 1.0);
        assert!(g.is_visual());
        assert!(g.shape_contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn group_item_with_label_only_is_not_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).label(lit!("Act 1"));
        assert!(!g.is_visual());
        assert!(!g.shape_contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn group_item_with_show_label_is_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0))
            .label(lit!("Act 1"))
            .show_label(true);
        assert!(g.is_visual());
        assert!(g.shape_contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn group_item_visual_paint_emits_draws() {
        let invisible = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        let visible = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0))
            .fill(Color::RED)
            .stroke(Color::BLACK, 2.0)
            .corner_radius(8.0);

        let theme = bastyde_core::presets::intui::light();
        let ctx = SceneItemPaintContext::new(Transform2D::identity(), None, &theme);

        let mut c1 = bastyde_canvas::Canvas::new();
        invisible.paint(&mut c1, &ctx);
        let f1 = c1.into_render_frame();
        assert!(f1.draw_order.is_empty(), "invisible group emitted draws");

        let mut c2 = bastyde_canvas::Canvas::new();
        visible.paint(&mut c2, &ctx);
        let f2 = c2.into_render_frame();
        assert!(!f2.draw_order.is_empty(), "visible group emitted no draws");
    }

    #[test]
    fn group_item_stroke_styled_stores_dash_pattern() {
        // #5: a dashed lane boundary keeps its pattern.
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0))
            .stroke_styled(Color::BLACK, StrokeStyle::dashed(2.0, 6.0, 4.0));
        let (_, style) = g.stroke.as_ref().expect("stroke set");
        assert!(style.dash_pattern.is_some());
    }

    #[test]
    fn group_item_role_fill_resolves_against_theme() {
        // #1/#2: a role fill resolves against the ctx theme.
        use bastyde_tokens::SurfaceRole;
        let theme = bastyde_core::presets::intui::light();
        let expected = ColorProp::from(SurfaceRole::Container).resolve(&theme, true);
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(SurfaceRole::Container);
        let ctx = SceneItemPaintContext::new(Transform2D::identity(), None, &theme);
        let mut c = bastyde_canvas::Canvas::new();
        g.paint(&mut c, &ctx);
        assert!(
            c.into_render_frame()
                .decorations
                .iter()
                .any(|d| d.color == expected.to_array())
        );
    }
}
