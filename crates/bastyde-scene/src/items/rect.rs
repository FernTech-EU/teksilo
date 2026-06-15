// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`RectItem`] — filled / stroked rectangle in local item coords.

use accesskit::Role;
use bastyde_canvas::{Canvas, Rect, StrokeStyle};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_tokens::Color;

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
    fill: Option<Color>,
    stroke: Option<(Color, StrokeStyle)>,
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

    /// Stroke color and width in **scene-coordinate** pixels — the border
    /// scales with the view zoom (a 1px border becomes 2px at 2× zoom).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, StrokeStyle::solid(width.max(0.0))));
        self
    }

    /// Cosmetic stroke: the border holds a constant **device-pixel** width at
    /// any zoom (a hairline that never thins out or thickens). Ideal for grid
    /// cells and card outlines in a pannable/zoomable scene.
    pub fn stroke_cosmetic(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, StrokeStyle::hairline(width.max(0.0))));
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

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if let Some(fill) = self.fill {
            canvas.fill_rect(self.local_bounds, fill);
        }
        if let Some((color, style)) = &self.stroke {
            canvas.stroke_rect(self.local_bounds, *color, style.clone());
        }
    }

    fn thumbnail_color(&self) -> bastyde_tokens::Color {
        // Fill dominates; fall through to stroke; fall through to
        // the default grey if the rect has no visible chrome.
        self.fill
            .or_else(|| self.stroke.as_ref().map(|(c, _)| *c))
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
        let mut canvas = Canvas::new();
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .fill(Color::RED)
            .stroke(Color::BLUE, 2.0);
        let ctx = SceneItemPaintContext::new(Transform2D::identity(), None);
        item.paint(&mut canvas, &ctx);
        let frame = canvas.into_render_frame();
        assert!(
            !frame.draw_order.is_empty(),
            "paint must emit at least one draw command"
        );
    }
}
