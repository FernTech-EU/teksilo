//! [`GroupItem`] — labelled box / logical AT container.
//!
//! Visually a labelled rectangle with optional fill, stroke, and
//! inline label. Without any chrome it's a logical-only container
//! that announces itself to AT but draws nothing — the lightweight
//! analogue of an `A11yGroup`.

use accesskit::Role;
use fern_canvas::{Canvas, Point, Rect, StrokeStyle};
use fern_core::accessibility::AccessNodeBuilder;
use fern_tokens::Color;

use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};

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
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    corner_radius: f32,
    label_inset: (f32, f32),
    label_color: Option<Color>,
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

    /// Override the inline label color. Defaults to the stroke
    /// color if set, else `Color::BLACK`.
    pub fn label_color(mut self, color: Color) -> Self {
        self.label_color = Some(color);
        self
    }

    /// Background fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Border stroke (color + scene-coord pixel width).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
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

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if !self.is_visual() {
            return;
        }
        let lb = self.local_bounds;
        if let Some(fill) = self.fill {
            if self.corner_radius > 0.0 {
                canvas.fill_rounded_rect(
                    lb,
                    fern_tokens::CornerRadius::uniform(self.corner_radius),
                    fill,
                );
            } else {
                canvas.fill_rect(lb, fill);
            }
        }
        if let Some((color, width)) = self.stroke {
            if self.corner_radius > 0.0 {
                canvas.stroke_rounded_rect(
                    lb,
                    fern_tokens::CornerRadius::uniform(self.corner_radius),
                    color,
                    StrokeStyle::solid(width),
                );
            } else {
                canvas.stroke_rect(lb, color, StrokeStyle::solid(width));
            }
        }
        if self.show_label
            && let Some(label) = &self.label
        {
            let color = self
                .label_color
                .or_else(|| self.stroke.map(|(c, _)| c))
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
                &fern_tokens::TextStyle::default(),
                color,
            );
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
    use fern_canvas::Transform2D;

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
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).label("Act 1");
        assert!(!g.is_visual());
        assert!(!g.shape_contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn group_item_with_show_label_is_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0))
            .label("Act 1")
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

        let ctx = SceneItemPaintContext::new(Transform2D::identity(), None);

        let mut c1 = fern_canvas::Canvas::new();
        invisible.paint(&mut c1, &ctx);
        let f1 = c1.into_render_frame();
        assert!(f1.draw_order.is_empty(), "invisible group emitted draws");

        let mut c2 = fern_canvas::Canvas::new();
        visible.paint(&mut c2, &ctx);
        let f2 = c2.into_render_frame();
        assert!(!f2.draw_order.is_empty(), "visible group emitted no draws");
    }
}
