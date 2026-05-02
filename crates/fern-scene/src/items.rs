//! Built-in [`SceneItem`] implementations.
//!
//! Phase 4 ships five items covering the common decoration cases:
//!
//! - [`RectItem`] — filled / stroked rectangle. Use for backgrounds,
//!   tile patterns, simple decorations.
//! - [`PathItem`] — arbitrary vector path with optional fill and
//!   stroke. The "connector lines between cards" workhorse.
//! - [`ImageItem`] — a raster image at a scene-coord rectangle.
//!   References the image by name (the Canvas image registry).
//! - [`TextItem`] — unstyled text at a scene-coord position. Uses
//!   the canvas's default text rendering path.
//! - [`GroupItem`] — a logical-only container. Phase 4 paints
//!   nothing; Phase 5 uses it to declare AT structure (Acts →
//!   Scenes etc.) without a visual counterpart.
//!
//! Custom items: implement [`SceneItem`] directly. The trait is
//! deliberately small (`bounds_in_scene` + `paint` + optional
//! `hit_test` + optional `label`).

use fern_canvas::{Canvas, Path, Point, Rect, StrokeStyle};
use fern_tokens::Color;

use crate::item::{SceneItem, SceneItemPaintContext};

// ---------------------------------------------------------------------------
// RectItem
// ---------------------------------------------------------------------------

/// A scene-coord rectangle with optional fill and stroke. Used for
/// backgrounds, tile patterns, simple decorations.
#[derive(Debug)]
pub struct RectItem {
    bounds: Rect,
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    label: Option<String>,
}

impl RectItem {
    /// A rectangle at the given scene-coord bounds. No fill, no
    /// stroke — set at least one or the item will be invisible.
    pub fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            fill: None,
            stroke: None,
            label: None,
        }
    }

    /// Fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Stroke color and width (in scene-coord pixels — they scale
    /// with the view zoom).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
        self
    }

    /// Human-readable label used for debug introspection and the
    /// Phase 5 a11y walker default name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl SceneItem for RectItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if let Some(fill) = self.fill {
            canvas.fill_rect(self.bounds, fill);
        }
        if let Some((color, width)) = self.stroke {
            canvas.stroke_rect(self.bounds, color, StrokeStyle::solid(width));
        }
    }

    fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
}

// ---------------------------------------------------------------------------
// PathItem
// ---------------------------------------------------------------------------

/// An arbitrary vector path (polyline, bezier, etc.) with optional
/// fill and stroke. Use for connector lines between cards, custom
/// shapes, hand-drawn-style decorations.
///
/// The path is evaluated in scene-coord space; fill / stroke widths
/// scale with the view zoom.
#[derive(Debug)]
pub struct PathItem {
    path: Path,
    bounds: Rect,
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    label: Option<String>,
}

impl PathItem {
    /// A path with a caller-provided AABB. The bounds are *not*
    /// derived from the path automatically — callers know the path's
    /// extent at construction time and pass it in. The bounds are
    /// what the spatial index buckets on, so they need to fully
    /// enclose the path's strokes (including stroke half-width on
    /// each side if you care about partial culling).
    pub fn new(path: Path, bounds: Rect) -> Self {
        Self {
            path,
            bounds,
            fill: None,
            stroke: None,
            label: None,
        }
    }

    /// Fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Stroke color and width (in scene-coord pixels).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
        self
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl SceneItem for PathItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if let Some(fill) = self.fill {
            canvas.fill_path(&self.path, fill);
        }
        if let Some((color, width)) = self.stroke {
            canvas.stroke_path(&self.path, color, StrokeStyle::solid(width));
        }
    }

    fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
}

// ---------------------------------------------------------------------------
// ImageItem
// ---------------------------------------------------------------------------

/// A raster image at a scene-coord rectangle. The image is
/// referenced by name — callers register images with the Canvas
/// before the first frame; `ImageItem` just records the lookup
/// string and the destination rect.
#[derive(Debug)]
pub struct ImageItem {
    bounds: Rect,
    name: String,
    label: Option<String>,
}

impl ImageItem {
    /// Construct an image item at `bounds`, referencing the image
    /// registered under `name`.
    pub fn new(bounds: Rect, name: impl Into<String>) -> Self {
        Self {
            bounds,
            name: name.into(),
            label: None,
        }
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl SceneItem for ImageItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        canvas.draw_image(self.bounds, self.name.clone());
    }

    fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
}

// ---------------------------------------------------------------------------
// TextItem
// ---------------------------------------------------------------------------

/// Unstyled text at a scene-coord position. Phase 4 ships a minimal
/// implementation: text + position + colour. Full styling (font
/// stack, weight, italic, line height) lands in Phase 7 polish or
/// when an app needs it; for now, anything fancier should use a
/// heavyweight `TextWidget` placed at `Scene::add_widget`.
#[derive(Debug)]
pub struct TextItem {
    text: String,
    bounds: Rect,
    color: Color,
    label: Option<String>,
}

impl TextItem {
    /// Construct a text item at `bounds`. The text wraps within the
    /// rectangle; height is callers' responsibility to set
    /// reasonably (Phase 4 doesn't auto-measure).
    pub fn new(text: impl Into<String>, bounds: Rect) -> Self {
        Self {
            text: text.into(),
            bounds,
            color: Color::BLACK,
            label: None,
        }
    }

    /// Override the foreground color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Human-readable label. Defaults to the text content if unset.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl SceneItem for TextItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        canvas.draw_text(
            &self.text,
            self.bounds,
            &fern_tokens::TextStyle::default(),
            self.color,
        );
    }

    fn label(&self) -> Option<&str> {
        // Fall back to the text itself so debug / a11y get a
        // sensible default.
        self.label.as_deref().or(Some(self.text.as_str()))
    }
}

// ---------------------------------------------------------------------------
// GroupItem
// ---------------------------------------------------------------------------

/// A logical-only group. Paints nothing in Phase 4 — its purpose is
/// to declare AT structure for the Phase 5b a11y-shaping API
/// (`Scene::add_a11y_group`, `set_a11y_parent`, etc.). Bounds are
/// stored so the spatial index can bucket the group for queries
/// like "what's at this scene coord, including grouping context".
///
/// Authors who want a visible group container should pair this
/// with a [`RectItem`] for the chrome.
#[derive(Debug)]
pub struct GroupItem {
    bounds: Rect,
    label: Option<String>,
}

impl GroupItem {
    /// A group covering `bounds` in scene coordinates.
    pub fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            label: None,
        }
    }

    /// Human-readable label. Used by the Phase 5 a11y walker as the
    /// default group name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl SceneItem for GroupItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, _canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        // Logical-only — no visual.
    }

    /// Hit-test default would AABB-contain. Override to
    /// `false` so `GroupItem` doesn't intercept pointer events
    /// meant for items behind it. Phase 5/6 will revisit if
    /// groups need pointer interaction.
    fn hit_test(&self, _scene_point: Point) -> bool {
        false
    }

    fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_item_bounds_round_trip() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        let item = RectItem::new(r);
        assert_eq!(item.bounds_in_scene(), r);
    }

    #[test]
    fn rect_item_default_hit_test() {
        let item = RectItem::new(Rect::new(10.0, 10.0, 50.0, 50.0));
        assert!(item.hit_test(Point::new(20.0, 20.0)));
        assert!(!item.hit_test(Point::new(5.0, 20.0)));
    }

    #[test]
    fn rect_item_paint_emits_fill_and_stroke() {
        // Sanity: paint adds draw commands to the canvas frame.
        let mut canvas = Canvas::new();
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .fill(Color::RED)
            .stroke(Color::BLUE, 2.0);
        let ctx = SceneItemPaintContext {
            view_transform: fern_canvas::Transform2D::IDENTITY,
            dirty_scene_rect: None,
        };
        item.paint(&mut canvas, &ctx);
        let frame = canvas.into_render_frame();
        assert!(
            !frame.draw_order.is_empty(),
            "paint must emit at least one draw command"
        );
    }

    #[test]
    fn path_item_holds_path_and_bounds() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 50.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 50.0))
            .stroke(Color::BLACK, 1.5);
        assert_eq!(item.bounds_in_scene(), Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn group_item_does_not_hit_test_through_aabb() {
        // Default `hit_test` AABB-contains; GroupItem overrides to
        // `false` so it never blocks pointer events on items
        // beneath. This is the contract Phase 5/6 will rely on.
        let g = GroupItem::new(Rect::new(0.0, 0.0, 1000.0, 1000.0));
        assert!(!g.hit_test(Point::new(500.0, 500.0)));
    }

    #[test]
    fn text_item_label_falls_back_to_text() {
        let item = TextItem::new("Hello", Rect::new(0.0, 0.0, 100.0, 30.0));
        assert_eq!(SceneItem::label(&item), Some("Hello"));
    }
}
