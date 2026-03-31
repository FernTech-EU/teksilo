use std::cell::RefCell;
use std::rc::Rc;

use fern_tokens::{Color, CornerRadius, TextStyle};

use crate::geometry::{Point, Rect};
use crate::paint::Paint;
use crate::path::Path;
use crate::render_frame::{
    DecorationKind, DecorationRect, DrawCommand, GlyphQuad, PaintData, PathEntry, RenderFrame,
    ShapeKind, ShapeQuad,
};
use crate::text_backend::TextBackend;

/// High-level drawing API that widget authors program against.
/// Accumulates drawing operations and produces a RenderFrame.
pub struct Canvas {
    frame: RenderFrame,
    text_backend: Option<Rc<RefCell<dyn TextBackend>>>,
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            frame: RenderFrame::new(),
            text_backend: None,
        }
    }

    pub fn with_text_backend(text_backend: Rc<RefCell<dyn TextBackend>>) -> Self {
        Self {
            frame: RenderFrame::new(),
            text_backend: Some(text_backend),
        }
    }

    // --- Tier 1: Axis-aligned rectangles (DecorationRect) ---

    /// Fill an axis-aligned rectangle with a solid color.
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let idx = self.frame.decorations.len();
        self.frame.decorations.push(DecorationRect {
            rect: rect.to_array(),
            color: color.to_array(),
            kind: DecorationKind::WidgetBackground,
        });
        self.frame.draw_order.push(DrawCommand::Decoration(idx));
    }

    /// Fill a decoration rectangle with a specific kind.
    pub fn fill_decoration(&mut self, rect: Rect, color: Color, kind: DecorationKind) {
        let idx = self.frame.decorations.len();
        self.frame.decorations.push(DecorationRect {
            rect: rect.to_array(),
            color: color.to_array(),
            kind,
        });
        self.frame.draw_order.push(DrawCommand::Decoration(idx));
    }

    /// Draw a horizontal or vertical line as a thin decoration rect.
    pub fn draw_line(&mut self, from: Point, to: Point, color: Color, width: f32) {
        let rect = if (from.y - to.y).abs() < 0.001 {
            // Horizontal line
            let min_x = from.x.min(to.x);
            let max_x = from.x.max(to.x);
            Rect::new(min_x, from.y - width / 2.0, max_x - min_x, width)
        } else {
            // Vertical line
            let min_y = from.y.min(to.y);
            let max_y = from.y.max(to.y);
            Rect::new(from.x - width / 2.0, min_y, width, max_y - min_y)
        };
        let idx = self.frame.decorations.len();
        self.frame.decorations.push(DecorationRect {
            rect: rect.to_array(),
            color: color.to_array(),
            kind: DecorationKind::WidgetBackground,
        });
        self.frame.draw_order.push(DrawCommand::Decoration(idx));
    }

    /// Stroke the outline of an axis-aligned rectangle.
    pub fn stroke_rect(&mut self, rect: Rect, color: Color, width: f32) {
        // Top
        self.draw_line(
            Point::new(rect.x, rect.y),
            Point::new(rect.right(), rect.y),
            color,
            width,
        );
        // Bottom
        self.draw_line(
            Point::new(rect.x, rect.bottom()),
            Point::new(rect.right(), rect.bottom()),
            color,
            width,
        );
        // Left
        self.draw_line(
            Point::new(rect.x, rect.y),
            Point::new(rect.x, rect.bottom()),
            color,
            width,
        );
        // Right
        self.draw_line(
            Point::new(rect.right(), rect.y),
            Point::new(rect.right(), rect.bottom()),
            color,
            width,
        );
    }

    // --- Tier 2: SDF shapes (ShapeQuad) ---

    /// Fill a rounded rectangle using SDF rendering.
    pub fn fill_rounded_rect(&mut self, rect: Rect, corner_radius: CornerRadius, color: Color) {
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: rect.to_array(),
            color: color.to_array(),
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            corner_radii: corner_radius.to_array(),
            paint_data: PaintData::Solid,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Stroke a rounded rectangle outline using SDF rendering.
    pub fn stroke_rounded_rect(
        &mut self,
        rect: Rect,
        corner_radius: CornerRadius,
        color: Color,
        stroke_width: f32,
    ) {
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: rect.to_array(),
            color: color.to_array(),
            shape: ShapeKind::RoundedRect,
            stroke_width,
            corner_radii: corner_radius.to_array(),
            paint_data: PaintData::Solid,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Fill a circle using SDF rendering.
    pub fn fill_circle(&mut self, center: crate::geometry::Point, radius: f32, color: Color) {
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: [
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ],
            color: color.to_array(),
            shape: ShapeKind::Circle,
            stroke_width: 0.0,
            corner_radii: [radius; 4],
            paint_data: PaintData::Solid,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Fill an ellipse using SDF rendering.
    pub fn fill_ellipse(&mut self, rect: Rect, color: Color) {
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: rect.to_array(),
            color: color.to_array(),
            shape: ShapeKind::Ellipse,
            stroke_width: 0.0,
            corner_radii: [0.0; 4],
            paint_data: PaintData::Solid,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    // --- Tier 3: Arbitrary paths (CPU rasterized) ---

    /// Fill an arbitrary path with a solid color.
    /// The path will be CPU-rasterized (Tier 3) and cached in the shape atlas.
    pub fn fill_path(&mut self, path: &Path, color: Color) {
        let bounds = path.bounds();
        let idx = self.frame.paths.len();
        self.frame.paths.push(PathEntry {
            path: path.clone(),
            color: color.to_array(),
            stroke_width: 0.0,
            bounds: bounds.to_array(),
        });
        self.frame.draw_order.push(DrawCommand::Path(idx));
    }

    /// Stroke an arbitrary path outline with a solid color.
    /// The path will be CPU-rasterized (Tier 3) and cached in the shape atlas.
    pub fn stroke_path(&mut self, path: &Path, color: Color, stroke_width: f32) {
        let bounds = path.bounds().expand(stroke_width);
        let idx = self.frame.paths.len();
        self.frame.paths.push(PathEntry {
            path: path.clone(),
            color: color.to_array(),
            stroke_width,
            bounds: bounds.to_array(),
        });
        self.frame.draw_order.push(DrawCommand::Path(idx));
    }

    // --- Text ---

    /// Draw a single line of text at the given position.
    /// Returns the measured size of the text, or None if no text backend is available.
    pub fn draw_text(
        &mut self,
        text: &str,
        position: Rect,
        style: &TextStyle,
        color: Color,
    ) -> Option<crate::geometry::Size> {
        let backend = self.text_backend.as_ref()?;
        let mut backend = backend.borrow_mut();

        let layout = backend.layout_single_line(text, style, Some(position.width));
        let glyphs = backend.ensure_glyphs(&layout);

        // Offset glyphs to the target position
        for glyph in &glyphs {
            let mut offset_glyph = *glyph;
            offset_glyph.screen[0] += position.x;
            offset_glyph.screen[1] += position.y;
            offset_glyph.color = color.to_array();
            let idx = self.frame.glyphs.len();
            self.frame.glyphs.push(offset_glyph);
            self.frame.draw_order.push(DrawCommand::Glyph(idx));
        }

        Some(crate::geometry::Size::new(layout.width, layout.height))
    }

    /// Draw a pre-measured text layout at the given position.
    /// Use this when measurement and painting are separated (e.g., layout measured
    /// during `size_that_fits`, painted during `paint`).
    pub fn draw_text_layout(
        &mut self,
        layout: &crate::text_backend::TextLayout,
        position: Point,
        color: Color,
    ) -> bool {
        let Some(backend) = self.text_backend.as_ref() else {
            return false;
        };
        let mut backend = backend.borrow_mut();
        let glyphs = backend.ensure_glyphs(layout);

        for glyph in &glyphs {
            let mut offset_glyph = *glyph;
            offset_glyph.screen[0] += position.x;
            offset_glyph.screen[1] += position.y;
            offset_glyph.color = color.to_array();
            let idx = self.frame.glyphs.len();
            self.frame.glyphs.push(offset_glyph);
            self.frame.draw_order.push(DrawCommand::Glyph(idx));
        }
        true
    }

    /// Fill a rounded rectangle with a Paint (supports gradients in the future).
    pub fn fill_rounded_rect_with(
        &mut self,
        rect: Rect,
        corner_radius: CornerRadius,
        paint: &Paint,
    ) {
        let (color, paint_data) = paint_to_data(paint);
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: rect.to_array(),
            color,
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            corner_radii: corner_radius.to_array(),
            paint_data,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Embed a pre-built RenderFrame (e.g. from text-typeset) at the given offset.
    pub fn draw_render_frame(&mut self, other: &RenderFrame, offset: Point) {
        let mut shifted = other.clone();
        for g in &mut shifted.glyphs {
            g.screen[0] += offset.x;
            g.screen[1] += offset.y;
        }
        for d in &mut shifted.decorations {
            d.rect[0] += offset.x;
            d.rect[1] += offset.y;
        }
        for s in &mut shifted.shapes {
            s.screen[0] += offset.x;
            s.screen[1] += offset.y;
        }
        for r in &mut shifted.rasterized {
            r.screen[0] += offset.x;
            r.screen[1] += offset.y;
        }
        for p in &mut shifted.paths {
            p.bounds[0] += offset.x;
            p.bounds[1] += offset.y;
        }
        for i in &mut shifted.images {
            i.screen[0] += offset.x;
            i.screen[1] += offset.y;
        }
        self.frame.merge(&shifted);
    }

    /// Set a clip rectangle for subsequent drawing operations.
    pub fn set_clip(&mut self, rect: Rect) {
        self.frame.draw_order.push(DrawCommand::SetClip(rect));
    }

    /// Clear the current clip rectangle.
    pub fn clear_clip(&mut self) {
        self.frame.draw_order.push(DrawCommand::ClearClip);
    }

    /// Set opacity for subsequent drawing operations.
    pub fn set_opacity(&mut self, opacity: f32) {
        self.frame.draw_order.push(DrawCommand::SetOpacity(opacity));
    }

    /// Restore the previous opacity.
    pub fn restore_opacity(&mut self) {
        self.frame.draw_order.push(DrawCommand::RestoreOpacity);
    }

    /// Append pre-positioned glyph quads directly (for text-typeset integration).
    pub fn append_glyphs(&mut self, glyphs: &[GlyphQuad]) {
        for glyph in glyphs {
            let idx = self.frame.glyphs.len();
            self.frame.glyphs.push(*glyph);
            self.frame.draw_order.push(DrawCommand::Glyph(idx));
        }
    }

    /// Consume the canvas and produce the accumulated RenderFrame.
    pub fn into_render_frame(self) -> RenderFrame {
        self.frame
    }
}

/// Convert a high-level `Paint` to a GPU-ready `(color, PaintData)` pair.
fn paint_to_data(paint: &Paint) -> ([f32; 4], PaintData) {
    match paint {
        Paint::Solid(c) => (c.to_array(), PaintData::Solid),
        Paint::LinearGradient { start, end, stops } => {
            let color = stops
                .first()
                .map(|s| s.color.to_array())
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            (
                color,
                PaintData::LinearGradient {
                    start: [start.x, start.y],
                    end: [end.x, end.y],
                    stops: stops.clone(),
                },
            )
        }
        Paint::RadialGradient {
            center,
            radius,
            stops,
        } => {
            let color = stops
                .first()
                .map(|s| s.color.to_array())
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            (
                color,
                PaintData::RadialGradient {
                    center: [center.x, center.y],
                    radius: *radius,
                    stops: stops.clone(),
                },
            )
        }
        Paint::Image(_) => {
            // Image paint requires texture binding — fallback to transparent
            ([0.0, 0.0, 0.0, 0.0], PaintData::Solid)
        }
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rounded_rect_produces_shape_quad() {
        let mut canvas = Canvas::new();
        canvas.fill_rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            CornerRadius::uniform(6.0),
            Color::RED,
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].shape, ShapeKind::RoundedRect);
        assert_eq!(frame.shapes[0].color, Color::RED.to_array());
        assert_eq!(frame.shapes[0].corner_radii, [6.0, 6.0, 6.0, 6.0]);
        assert_eq!(frame.shapes[0].stroke_width, 0.0);
    }

    #[test]
    fn fill_rect_produces_decoration() {
        let mut canvas = Canvas::new();
        canvas.fill_rect(Rect::new(0.0, 0.0, 50.0, 20.0), Color::BLUE);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.decorations.len(), 1);
        assert_eq!(frame.decorations[0].kind, DecorationKind::WidgetBackground);
        assert_eq!(frame.decorations[0].color, Color::BLUE.to_array());
    }

    #[test]
    fn stroke_rounded_rect_has_stroke_width() {
        let mut canvas = Canvas::new();
        canvas.stroke_rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            CornerRadius::uniform(6.0),
            Color::BLACK,
            2.0,
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 1);
        assert!(frame.shapes[0].stroke_width > 0.0);
    }

    #[test]
    fn draw_order_preserved() {
        let mut canvas = Canvas::new();
        canvas.fill_rect(Rect::new(0.0, 0.0, 50.0, 20.0), Color::RED);
        canvas.fill_rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            CornerRadius::uniform(6.0),
            Color::BLUE,
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.draw_order.len(), 2);
        assert!(matches!(frame.draw_order[0], DrawCommand::Decoration(0)));
        assert!(matches!(frame.draw_order[1], DrawCommand::Shape(0)));
    }

    #[test]
    fn fill_circle_produces_shape() {
        let mut canvas = Canvas::new();
        canvas.fill_circle(crate::geometry::Point::new(50.0, 50.0), 25.0, Color::GREEN);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].shape, ShapeKind::Circle);
    }

    #[test]
    fn append_glyphs_adds_to_frame() {
        let mut canvas = Canvas::new();
        let glyph = GlyphQuad {
            screen: [10.0, 20.0, 8.0, 16.0],
            atlas: [0.0, 0.0, 0.5, 0.5],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        canvas.append_glyphs(&[glyph]);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.glyphs.len(), 1);
        assert_eq!(frame.draw_order.len(), 1);
        assert!(matches!(frame.draw_order[0], DrawCommand::Glyph(0)));
    }

    #[test]
    fn fill_rounded_rect_with_paint() {
        use crate::paint::Paint;
        let mut canvas = Canvas::new();
        canvas.fill_rounded_rect_with(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            CornerRadius::uniform(6.0),
            &Paint::Solid(Color::RED),
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 1);
    }

    #[test]
    fn set_clip_and_clear() {
        let mut canvas = Canvas::new();
        canvas.set_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
        canvas.fill_rect(Rect::new(10.0, 10.0, 50.0, 50.0), Color::RED);
        canvas.clear_clip();
        let frame = canvas.into_render_frame();
        assert_eq!(frame.draw_order.len(), 3);
        assert!(matches!(frame.draw_order[0], DrawCommand::SetClip(_)));
        assert!(matches!(frame.draw_order[2], DrawCommand::ClearClip));
    }

    #[test]
    fn canvas_without_text_backend_returns_none() {
        let mut canvas = Canvas::new();
        let result = canvas.draw_text(
            "Hello",
            Rect::new(0.0, 0.0, 100.0, 20.0),
            &TextStyle::default(),
            Color::BLACK,
        );
        assert!(result.is_none());
    }

    #[test]
    fn canvas_with_mock_text_backend() {
        use crate::text_backend::MockTextBackend;
        let backend = Rc::new(RefCell::new(MockTextBackend::new()));
        let mut canvas = Canvas::with_text_backend(backend);
        let result = canvas.draw_text(
            "Hello",
            Rect::new(0.0, 0.0, 100.0, 20.0),
            &TextStyle::default(),
            Color::BLACK,
        );
        // MockTextBackend returns empty glyphs but valid size
        let size = result.unwrap();
        assert_eq!(size.width, 40.0); // 5 chars × 8.0
        assert_eq!(size.height, 16.0);
    }

    // --- New method tests ---

    #[test]
    fn fill_path_produces_path_entry() {
        let mut canvas = Canvas::new();
        let star = crate::path::Path::star(
            crate::geometry::Point::new(50.0, 50.0),
            30.0,
            15.0,
            5,
        );
        canvas.fill_path(&star, Color::RED);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.paths.len(), 1);
        assert_eq!(frame.paths[0].stroke_width, 0.0);
        assert!(matches!(frame.draw_order[0], DrawCommand::Path(0)));
    }

    #[test]
    fn stroke_path_has_stroke_width() {
        let mut canvas = Canvas::new();
        let circle = crate::path::Path::circle(
            crate::geometry::Point::new(50.0, 50.0),
            25.0,
        );
        canvas.stroke_path(&circle, Color::BLACK, 2.0);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.paths.len(), 1);
        assert_eq!(frame.paths[0].stroke_width, 2.0);
    }

    #[test]
    fn fill_ellipse_produces_shape() {
        let mut canvas = Canvas::new();
        canvas.fill_ellipse(Rect::new(0.0, 0.0, 100.0, 50.0), Color::BLUE);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].shape, ShapeKind::Ellipse);
    }

    #[test]
    fn draw_line_produces_decoration() {
        let mut canvas = Canvas::new();
        let from = crate::geometry::Point::new(0.0, 10.0);
        let to = crate::geometry::Point::new(100.0, 10.0);
        canvas.draw_line(from, to, Color::BLACK, 1.0);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.decorations.len(), 1);
        // Horizontal line: width = 100, height = 1
        let d = &frame.decorations[0];
        assert!((d.rect[2] - 100.0).abs() < 0.01);
        assert!((d.rect[3] - 1.0).abs() < 0.01);
    }

    #[test]
    fn stroke_rect_produces_four_lines() {
        let mut canvas = Canvas::new();
        canvas.stroke_rect(Rect::new(10.0, 10.0, 80.0, 40.0), Color::BLACK, 1.0);
        let frame = canvas.into_render_frame();
        // 4 sides = 4 decoration rects
        assert_eq!(frame.decorations.len(), 4);
    }

    #[test]
    fn draw_text_layout_renders_pre_measured() {
        use crate::text_backend::MockTextBackend;
        let backend = Rc::new(RefCell::new(MockTextBackend::new()));
        let layout = backend.borrow_mut().layout_single_line(
            "Test",
            &TextStyle::default(),
            None,
        );
        let mut canvas = Canvas::with_text_backend(backend);
        let ok = canvas.draw_text_layout(&layout, Point::new(10.0, 20.0), Color::BLACK);
        assert!(ok);
    }

    #[test]
    fn draw_text_layout_without_backend_returns_false() {
        let layout = crate::text_backend::TextLayout {
            width: 40.0,
            height: 16.0,
            ascent: 12.0,
            descent: 4.0,
            layout_key: 0,
        };
        let mut canvas = Canvas::new();
        assert!(!canvas.draw_text_layout(&layout, Point::new(0.0, 0.0), Color::BLACK));
    }

    #[test]
    fn gradient_paint_produces_gradient_paint_data() {
        use crate::paint::{GradientStop, Paint};
        let paint = Paint::LinearGradient {
            start: Point::new(0.0, 0.0),
            end: Point::new(100.0, 0.0),
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Color::RED,
                },
                GradientStop {
                    offset: 1.0,
                    color: Color::BLUE,
                },
            ],
        };
        let mut canvas = Canvas::new();
        canvas.fill_rounded_rect_with(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            CornerRadius::uniform(0.0),
            &paint,
        );
        let frame = canvas.into_render_frame();
        assert!(matches!(
            frame.shapes[0].paint_data,
            PaintData::LinearGradient { .. }
        ));
    }

    #[test]
    fn radial_gradient_paint_data() {
        use crate::paint::{GradientStop, Paint};
        let paint = Paint::RadialGradient {
            center: Point::new(50.0, 50.0),
            radius: 50.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Color::WHITE,
                },
                GradientStop {
                    offset: 1.0,
                    color: Color::BLACK,
                },
            ],
        };
        let mut canvas = Canvas::new();
        canvas.fill_rounded_rect_with(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            CornerRadius::uniform(0.0),
            &paint,
        );
        let frame = canvas.into_render_frame();
        assert!(matches!(
            frame.shapes[0].paint_data,
            PaintData::RadialGradient { .. }
        ));
    }
}
