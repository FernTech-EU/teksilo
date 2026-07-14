// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_tokens::{Color, CornerRadius, Shadow, TextStyle};

use crate::geometry::{Point, Rect, Transform2D};
use crate::paint::{FillRule, Paint, StrokeSpace, StrokeStyle};
use crate::path::Path;
use crate::render_frame::{
    AnimatedQuadClass, AnimatedQuadDraw, BlendMode, CosmeticLine, DecorationKind, DecorationRect,
    DrawCommand, GlyphQuad, ImageQuad, PaintData, PathEntry, RenderFrame, ShadowQuad, ShapeKind,
    ShapeQuad,
};
use crate::text_backend::TextBackend;

/// High-level drawing API that widget authors program against.
/// Accumulates drawing operations and produces a RenderFrame.
pub struct Canvas {
    frame: RenderFrame,
    text_backend: Option<Rc<RefCell<dyn TextBackend>>>,
    transform_stack: Vec<Transform2D>,
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            frame: RenderFrame::new(),
            text_backend: None,
            transform_stack: vec![Transform2D::identity()],
        }
    }

    pub fn with_text_backend(text_backend: Rc<RefCell<dyn TextBackend>>) -> Self {
        Self {
            frame: RenderFrame::new(),
            text_backend: Some(text_backend),
            transform_stack: vec![Transform2D::identity()],
        }
    }

    /// Access the canvas's text backend, when one is set. Used by widgets
    /// that need to measure or ellipsize text outside of the built-in
    /// draw_text / draw_paragraph helpers.
    pub fn text_backend(&self) -> Option<&Rc<RefCell<dyn TextBackend>>> {
        self.text_backend.as_ref()
    }

    /// Emit a shader-driven animated quad. The `slot` comes from an
    /// `AnimatedQuadHandle` the widget obtained in `build()` via
    /// `ctx.animated_quad(kind)`; the fragment shader reads the
    /// per-frame state (phase, frame index, resolved colors, …) from
    /// the renderer's uniform buffer indexed by `slot`. This means
    /// `paint()` only runs when the widget's layout changes, not every
    /// frame — the animation keeps moving while `paint()` sits idle.
    pub fn draw_animated_quad(&mut self, bounds: Rect, slot: u32, class: AnimatedQuadClass) {
        let idx = self.frame.animated_quads.len();
        self.frame.animated_quads.push(AnimatedQuadDraw {
            screen: bounds.to_array(),
            slot,
            class,
        });
        self.frame.draw_order.push(DrawCommand::AnimatedQuad(idx));
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
    pub fn draw_line(
        &mut self,
        from: Point,
        to: Point,
        color: Color,
        style: impl Into<StrokeStyle>,
    ) {
        let style = style.into();
        if style.space == StrokeSpace::Device {
            // Cosmetic / hairline: keep the width un-baked so the renderer can
            // apply a transform-invariant device-pixel thickness.
            let idx = self.frame.cosmetic_lines.len();
            self.frame.cosmetic_lines.push(CosmeticLine {
                from: [from.x, from.y],
                to: [to.x, to.y],
                width: style.width,
                color: color.to_array(),
            });
            self.frame.draw_order.push(DrawCommand::CosmeticLine(idx));
            return;
        }
        let width = style.width;
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
    pub fn stroke_rect(&mut self, rect: Rect, color: Color, style: impl Into<StrokeStyle>) {
        let style = style.into();
        // Pass the full style (not just the width) so `StrokeSpace::Device`
        // (cosmetic) propagates to each edge line.
        // Top
        self.draw_line(
            Point::new(rect.x, rect.y),
            Point::new(rect.right(), rect.y),
            color,
            style.clone(),
        );
        // Bottom
        self.draw_line(
            Point::new(rect.x, rect.bottom()),
            Point::new(rect.right(), rect.bottom()),
            color,
            style.clone(),
        );
        // Left
        self.draw_line(
            Point::new(rect.x, rect.y),
            Point::new(rect.x, rect.bottom()),
            color,
            style.clone(),
        );
        // Right
        self.draw_line(
            Point::new(rect.right(), rect.y),
            Point::new(rect.right(), rect.bottom()),
            color,
            style,
        );
    }

    // --- Tier 2: SDF shapes (ShapeQuad) ---

    /// Fill a rounded rectangle using SDF rendering.
    pub fn fill_rounded_rect(
        &mut self,
        rect: Rect,
        corner_radius: CornerRadius,
        paint: impl Into<Paint>,
    ) {
        let (color, paint_data) = paint_to_data(&paint.into());
        let clamped = corner_radius.clamped(rect.width, rect.height);
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: rect.to_array(),
            color,
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            stroke_space: StrokeSpace::Logical,
            corner_radii: clamped.to_array(),
            paint_data,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Stroke a rounded rectangle outline using SDF rendering.
    ///
    /// A [`StrokeSpace::Device`] style renders a cosmetic (hairline) border:
    /// its width stays constant in device pixels regardless of the view
    /// transform's zoom, while the shape body still zooms.
    pub fn stroke_rounded_rect(
        &mut self,
        rect: Rect,
        corner_radius: CornerRadius,
        paint: impl Into<Paint>,
        style: impl Into<StrokeStyle>,
    ) {
        let style = style.into();
        let (color, paint_data) = paint_to_data(&paint.into());
        let clamped = corner_radius.clamped(rect.width, rect.height);
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: rect.to_array(),
            color,
            shape: ShapeKind::RoundedRect,
            stroke_width: style.width,
            stroke_space: style.space,
            corner_radii: clamped.to_array(),
            paint_data,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Fill a circle using SDF rendering.
    pub fn fill_circle(&mut self, center: Point, radius: f32, paint: impl Into<Paint>) {
        let (color, paint_data) = paint_to_data(&paint.into());
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: [
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ],
            color,
            shape: ShapeKind::Circle,
            stroke_width: 0.0,
            stroke_space: StrokeSpace::Logical,
            corner_radii: [radius; 4],
            paint_data,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Stroke a circle outline using SDF rendering.
    ///
    /// A [`StrokeSpace::Device`] style renders a cosmetic (hairline) border —
    /// see [`stroke_rounded_rect`](Self::stroke_rounded_rect).
    pub fn stroke_circle(
        &mut self,
        center: Point,
        radius: f32,
        paint: impl Into<Paint>,
        style: impl Into<StrokeStyle>,
    ) {
        let style = style.into();
        let (color, paint_data) = paint_to_data(&paint.into());
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: [
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ],
            color,
            shape: ShapeKind::Circle,
            stroke_width: style.width,
            stroke_space: style.space,
            corner_radii: [radius; 4],
            paint_data,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Fill an ellipse using SDF rendering.
    pub fn fill_ellipse(&mut self, rect: Rect, paint: impl Into<Paint>) {
        let (color, paint_data) = paint_to_data(&paint.into());
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: rect.to_array(),
            color,
            shape: ShapeKind::Ellipse,
            stroke_width: 0.0,
            stroke_space: StrokeSpace::Logical,
            corner_radii: [0.0; 4],
            paint_data,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    /// Stroke an ellipse outline using SDF rendering.
    ///
    /// A [`StrokeSpace::Device`] style renders a cosmetic (hairline) border —
    /// see [`stroke_rounded_rect`](Self::stroke_rounded_rect).
    pub fn stroke_ellipse(
        &mut self,
        rect: Rect,
        paint: impl Into<Paint>,
        style: impl Into<StrokeStyle>,
    ) {
        let style = style.into();
        let (color, paint_data) = paint_to_data(&paint.into());
        let idx = self.frame.shapes.len();
        self.frame.shapes.push(ShapeQuad {
            screen: rect.to_array(),
            color,
            shape: ShapeKind::Ellipse,
            stroke_width: style.width,
            stroke_space: style.space,
            corner_radii: [0.0; 4],
            paint_data,
        });
        self.frame.draw_order.push(DrawCommand::Shape(idx));
    }

    // --- Tier 3: Arbitrary paths (CPU rasterized) ---

    /// Fill an arbitrary path with a paint (solid color or gradient),
    /// using the non-zero winding rule. The path will be CPU-rasterized
    /// (Tier 3, as an opaque coverage mask) and cached in the shape atlas;
    /// the paint is applied by the GPU at draw time (solid fills go
    /// through the lean quad pipeline, gradients through a dedicated
    /// `path_gradient` pipeline — see `Canvas::fill_rounded_rect` for the
    /// analogous Tier-2 SDF path).
    pub fn fill_path(&mut self, path: &Path, paint: impl Into<Paint>) {
        self.fill_path_with_rule(path, paint, FillRule::Winding);
    }

    /// Fill an arbitrary path with a paint and an explicit [`FillRule`] —
    /// use [`FillRule::EvenOdd`] for SVG `fill-rule="evenodd"` geometry
    /// (rings / donuts / counters). The path will be CPU-rasterized
    /// (Tier 3) and cached in the shape atlas.
    pub fn fill_path_with_rule(
        &mut self,
        path: &Path,
        paint: impl Into<Paint>,
        fill_rule: FillRule,
    ) {
        let (color, paint_data) = paint_to_data(&paint.into());
        let bounds = path.bounds();
        let idx = self.frame.paths.len();
        self.frame.paths.push(PathEntry {
            path: path.clone(),
            color,
            stroke_style: StrokeStyle::solid(0.0),
            fill_rule,
            bounds: bounds.to_array(),
            paint_data,
        });
        self.frame.draw_order.push(DrawCommand::Path(idx));
    }

    /// Stroke an arbitrary path outline with a solid color.
    /// The path will be CPU-rasterized (Tier 3) and cached in the shape atlas.
    ///
    /// A [`StrokeSpace::Device`] style renders a cosmetic (hairline) stroke:
    /// the renderer rasterizes the path body at the current view zoom while
    /// baking the stroke at a constant device-pixel width, so connectors stay
    /// crisp and uniform at any scene zoom.
    pub fn stroke_path(&mut self, path: &Path, color: Color, style: impl Into<StrokeStyle>) {
        self.stroke_path_with_paint(path, Paint::Solid(color), style);
    }

    /// Stroke an arbitrary path outline with a paint — a solid color, or a
    /// **gradient** (an SVG `stroke="url(#grad)"`, a gradient-ruled underline).
    ///
    /// Gradient geometry is in the same space as for a fill: **path-bounds-local
    /// pixels**, `(0, 0)` at the top-left of `path.bounds()`. A stroke's drawn
    /// rect is wider than that — the outline spills `style.width` beyond the
    /// path on every side, and the shader normalizes the gradient against *that*
    /// expanded rect — so the endpoints are re-based here. Without the shift a
    /// stroke gradient would sit visibly offset from the identical fill gradient
    /// on the same path, and by an amount that changes with the stroke width.
    pub fn stroke_path_with_paint(
        &mut self,
        path: &Path,
        paint: impl Into<Paint>,
        style: impl Into<StrokeStyle>,
    ) {
        let style = style.into();
        let path_bounds = path.bounds();
        let bounds = path_bounds.expand(style.width);
        let shifted = rebase_paint(
            paint.into(),
            path_bounds.x - bounds.x,
            path_bounds.y - bounds.y,
        );
        let (color, paint_data) = paint_to_data(&shifted);
        let idx = self.frame.paths.len();
        self.frame.paths.push(PathEntry {
            path: path.clone(),
            color,
            stroke_style: style,
            fill_rule: FillRule::Winding,
            bounds: bounds.to_array(),
            paint_data,
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

        // Add a small epsilon to the bounds width before using it as max_width.
        // The same epsilon is applied in TextWidget::size_that_fits so that
        // both paths produce the same TypesetterBridge cache key, avoiding
        // duplicate cache entries and inconsistent truncation.
        let max_width = Some(position.width + 0.5);
        let layout = backend.layout_single_line(text, style, max_width);
        let glyphs = backend.ensure_glyphs(&layout);
        self.frame.layout_keys.push(layout.layout_key);

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
        debug_warn_raster_scale_mismatch("draw_text_layout", layout, &*backend);
        let glyphs = backend.ensure_glyphs(layout);
        if glyphs.is_empty() {
            // A retained layout that measured to non-zero width but now
            // resolves to zero glyphs means its glyphs were evicted from
            // the backend's cache (atlas pressure, scale-factor reset).
            // This is recoverable — the caller MUST re-shape (`draw_text`)
            // or re-layout when we return `false`, otherwise the text
            // silently vanishes until the next relayout. Warn in debug so a
            // new cached-layout widget that forgets the fallback surfaces
            // loudly instead of dropping text. NOT a panic: the condition
            // is legitimate at runtime and correctly-written callers handle
            // it (see `MenuLabel::paint`).
            debug_warn_evicted_layout("draw_text_layout", layout, &*backend);
            return false;
        }
        self.frame.layout_keys.push(layout.layout_key);

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

    /// Draw a pre-measured text layout that contains per-span metadata
    /// (from a `layout_*_markup` call), applying `text_color` to Text
    /// spans and `link_color` to Link spans.
    ///
    /// Glyphs that don't fall inside any span fall back to `text_color`.
    pub fn draw_text_layout_markup(
        &mut self,
        layout: &crate::text_backend::TextLayout,
        position: Point,
        text_color: Color,
        link_color: Color,
    ) -> bool {
        let Some(backend) = self.text_backend.as_ref() else {
            return false;
        };
        let mut backend = backend.borrow_mut();
        debug_warn_raster_scale_mismatch("draw_text_layout_markup", layout, &*backend);
        let glyphs = backend.ensure_glyphs(layout);
        if glyphs.is_empty() {
            return false;
        }
        self.frame.layout_keys.push(layout.layout_key);

        let text_rgba = text_color.to_array();
        let link_rgba = link_color.to_array();

        for glyph in &glyphs {
            // Determine the glyph's center point relative to the layout
            // origin and check it against each span's rect. This is O(G*S)
            // but S is typically small for tooltip labels.
            let gx = glyph.screen[0] + glyph.screen[2] * 0.5;
            let gy = glyph.screen[1] + glyph.screen[3] * 0.5;
            let mut color_rgba = text_rgba;
            for sp in &layout.spans {
                let [sx, sy, sw, sh] = sp.rect;
                if gx >= sx && gx < sx + sw && gy >= sy && gy < sy + sh {
                    if matches!(sp.kind, crate::text_backend::TextSpanKind::Link { .. }) {
                        color_rgba = link_rgba;
                    }
                    break;
                }
            }
            let mut offset_glyph = *glyph;
            offset_glyph.screen[0] += position.x;
            offset_glyph.screen[1] += position.y;
            offset_glyph.color = color_rgba;
            let idx = self.frame.glyphs.len();
            self.frame.glyphs.push(offset_glyph);
            self.frame.draw_order.push(DrawCommand::Glyph(idx));
        }
        true
    }

    // --- Shadows ---

    /// Draw a shadow behind a rounded rectangle.
    /// The shadow quad is expanded by blur + spread + offset to cover the penumbra.
    pub fn draw_shadow(&mut self, rect: Rect, corner_radius: CornerRadius, shadow: &Shadow) {
        let expand = shadow.blur + shadow.spread;
        let shadow_rect = Rect::new(
            rect.x + shadow.offset_x - expand,
            rect.y + shadow.offset_y - expand,
            rect.width + expand * 2.0,
            rect.height + expand * 2.0,
        );
        let idx = self.frame.shadows.len();
        self.frame.shadows.push(ShadowQuad {
            screen: shadow_rect.to_array(),
            color: shadow.color.to_array(),
            corner_radii: corner_radius.to_array(),
            shape_rect: rect.to_array(),
            blur_radius: shadow.blur,
            spread: shadow.spread,
        });
        self.frame.draw_order.push(DrawCommand::Shadow(idx));
    }

    // --- Per-side borders ---

    /// Draw a border on the top edge of a rectangle.
    pub fn draw_border_top(&mut self, rect: Rect, color: Color, width: f32) {
        self.fill_rect(Rect::new(rect.x, rect.y, rect.width, width), color);
    }

    /// Draw a border on the bottom edge of a rectangle.
    pub fn draw_border_bottom(&mut self, rect: Rect, color: Color, width: f32) {
        self.fill_rect(
            Rect::new(rect.x, rect.bottom() - width, rect.width, width),
            color,
        );
    }

    /// Draw a border on the left edge of a rectangle.
    ///
    /// This is a direction-agnostic primitive. Widgets that need RTL-aware
    /// borders must resolve leading/trailing into left/right themselves
    /// before calling this.
    pub fn draw_border_left(&mut self, rect: Rect, color: Color, width: f32) {
        self.fill_rect(Rect::new(rect.x, rect.y, width, rect.height), color);
    }

    /// Draw a border on the right edge of a rectangle.
    ///
    /// This is a direction-agnostic primitive. Widgets that need RTL-aware
    /// borders must resolve leading/trailing into left/right themselves
    /// before calling this.
    pub fn draw_border_right(&mut self, rect: Rect, color: Color, width: f32) {
        self.fill_rect(
            Rect::new(rect.right() - width, rect.y, width, rect.height),
            color,
        );
    }

    /// Draw borders on all four sides with independent widths.
    ///
    /// Parameters are direction-agnostic (`left`/`right`, not leading/trailing).
    /// Widgets that need RTL-aware borders must swap left/right themselves.
    pub fn draw_border(
        &mut self,
        rect: Rect,
        color: Color,
        top: f32,
        right: f32,
        bottom: f32,
        left: f32,
    ) {
        if top > 0.0 {
            self.draw_border_top(rect, color, top);
        }
        if bottom > 0.0 {
            self.draw_border_bottom(rect, color, bottom);
        }
        if left > 0.0 {
            self.draw_border_left(rect, color, left);
        }
        if right > 0.0 {
            self.draw_border_right(rect, color, right);
        }
    }

    // --- Text decoration helpers ---

    /// Draw an underline below text at the given baseline.
    ///
    /// `offset` is the distance from baseline to the top of the underline
    /// (positive = below baseline), typically sourced from font metrics via
    /// [`TextLayout::underline_offset`](crate::TextLayout). `thickness` is
    /// the line height in logical pixels.
    pub fn draw_underline(
        &mut self,
        rect: Rect,
        baseline_y: f32,
        offset: f32,
        color: Color,
        thickness: f32,
    ) {
        let y = rect.y + baseline_y + offset;
        self.fill_rect(Rect::new(rect.x, y, rect.width, thickness), color);
    }

    /// Draw a strikethrough line through text.
    pub fn draw_strikethrough(
        &mut self,
        rect: Rect,
        baseline_y: f32,
        ascent: f32,
        color: Color,
        width: f32,
    ) {
        let y = rect.y + baseline_y - ascent * 0.35; // roughly mid-x-height
        self.fill_rect(Rect::new(rect.x, y, rect.width, width), color);
    }

    // --- Image drawing ---

    /// Draw an image at the given rectangle (stretched to fit, full color).
    pub fn draw_image(&mut self, rect: Rect, name: impl Into<String>) {
        let idx = self.frame.images.len();
        self.frame.images.push(ImageQuad {
            screen: rect.to_array(),
            name: name.into(),
            tint: None,
        });
        self.frame.draw_order.push(DrawCommand::Image(idx));
    }

    /// Draw an image as an alpha mask tinted with the given color.
    /// The image texture is treated as a monochrome mask — only the
    /// alpha channel matters. The fill color comes from `tint`.
    pub fn draw_tinted_image(
        &mut self,
        rect: Rect,
        name: impl Into<String>,
        tint: bastyde_tokens::Color,
    ) {
        let idx = self.frame.images.len();
        self.frame.images.push(ImageQuad {
            screen: rect.to_array(),
            name: name.into(),
            tint: Some([tint.r(), tint.g(), tint.b(), tint.a()]),
        });
        self.frame.draw_order.push(DrawCommand::Image(idx));
    }

    /// Check if an image is already queued for registration this frame.
    pub fn has_pending_image(&self, name: &str) -> bool {
        self.frame.pending_images.iter().any(|p| p.name == name)
    }

    /// Queue an image for GPU registration. The renderer uploads the
    /// texture if not already present. For compile-time embedded data,
    /// use `Cow::Borrowed` for zero-copy.
    pub fn ensure_image_registered(
        &mut self,
        name: impl Into<String>,
        width: u32,
        height: u32,
        pixels: std::borrow::Cow<'static, [u8]>,
    ) {
        let name = name.into();
        // Skip if already queued this frame
        if self.frame.pending_images.iter().any(|p| p.name == name) {
            return;
        }
        self.frame
            .pending_images
            .push(crate::render_frame::PendingImage {
                name,
                width,
                height,
                pixels,
            });
    }

    // --- Paragraph drawing ---

    /// Draw a multi-line paragraph of text within the given rectangle,
    /// wrapping at `rect.width`. Optionally capped at `max_lines` lines —
    /// lines beyond the cap are silently dropped.
    ///
    /// Returns the total paragraph size, or `None` if no text backend.
    pub fn draw_paragraph(
        &mut self,
        text: &str,
        rect: Rect,
        style: &TextStyle,
        color: Color,
        max_lines: Option<usize>,
    ) -> Option<crate::geometry::Size> {
        let backend = self.text_backend.as_ref()?;
        let mut backend = backend.borrow_mut();

        let max_width = (rect.width + 0.5).max(0.0);
        let layout = backend.layout_paragraph(text, style, max_width, max_lines);
        let glyphs = backend.ensure_glyphs(&layout);
        self.frame.layout_keys.push(layout.layout_key);

        for glyph in &glyphs {
            let mut offset_glyph = *glyph;
            offset_glyph.screen[0] += rect.x;
            offset_glyph.screen[1] += rect.y;
            offset_glyph.color = color.to_array();
            let idx = self.frame.glyphs.len();
            self.frame.glyphs.push(offset_glyph);
            self.frame.draw_order.push(DrawCommand::Glyph(idx));
        }

        Some(crate::geometry::Size::new(layout.width, layout.height))
    }

    // --- Blend modes ---

    /// Set the blend mode for subsequent drawing operations.
    pub fn set_blend_mode(&mut self, mode: BlendMode) {
        self.frame.draw_order.push(DrawCommand::SetBlendMode(mode));
    }

    /// Restore the previous blend mode.
    pub fn restore_blend_mode(&mut self) {
        self.frame.draw_order.push(DrawCommand::RestoreBlendMode);
    }

    // --- Transform stack ---

    /// Save the current transform state (push onto stack).
    pub fn save(&mut self) {
        let current = self.current_transform();
        self.transform_stack.push(current);
    }

    /// Restore the previous transform state (pop from stack).
    pub fn restore(&mut self) {
        if self.transform_stack.len() > 1 {
            self.transform_stack.pop();
        }
        self.frame
            .draw_order
            .push(DrawCommand::SetTransform(self.current_transform()));
    }

    /// Apply a translation to the current transform.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let current = self.current_transform();
        let new = current.then(&Transform2D::translate(dx, dy));
        if let Some(top) = self.transform_stack.last_mut() {
            *top = new;
        }
        self.frame.draw_order.push(DrawCommand::SetTransform(new));
    }

    /// Apply a rotation (in radians) to the current transform.
    pub fn rotate(&mut self, angle: f32) {
        let current = self.current_transform();
        let new = current.then(&Transform2D::rotate(angle));
        if let Some(top) = self.transform_stack.last_mut() {
            *top = new;
        }
        self.frame.draw_order.push(DrawCommand::SetTransform(new));
    }

    /// Apply a scale to the current transform.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        let current = self.current_transform();
        let new = current.then(&Transform2D::scale(sx, sy));
        if let Some(top) = self.transform_stack.last_mut() {
            *top = new;
        }
        self.frame.draw_order.push(DrawCommand::SetTransform(new));
    }

    /// Compose `t` **beneath** the current transform on the stack so
    /// subsequent draws use `t` first, then the existing transform.
    /// Mathematically: `new = current ∘ t` — i.e. a point `p` drawn
    /// after this call lands at `current(t(p))`. This is the
    /// "pre-multiply" semantic, opposite of [`Canvas::translate`],
    /// which post-multiplies in output space. Use this when you want
    /// to add a transform that operates in the **local** coordinates
    /// of the about-to-be-drawn content (e.g. pushing a per-item
    /// scene-graph transform whose input is item-local coords).
    pub fn apply_transform(&mut self, t: Transform2D) {
        let current = self.current_transform();
        let new = t.then(&current);
        if let Some(top) = self.transform_stack.last_mut() {
            *top = new;
        }
        self.frame.draw_order.push(DrawCommand::SetTransform(new));
    }

    /// Get the current transform.
    pub fn current_transform(&self) -> Transform2D {
        self.transform_stack
            .last()
            .copied()
            .unwrap_or_else(Transform2D::identity)
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
        for c in &mut shifted.cosmetic_lines {
            c.from[0] += offset.x;
            c.from[1] += offset.y;
            c.to[0] += offset.x;
            c.to[1] += offset.y;
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
        for s in &mut shifted.shadows {
            s.screen[0] += offset.x;
            s.screen[1] += offset.y;
            s.shape_rect[0] += offset.x;
            s.shape_rect[1] += offset.y;
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

    /// Append one pre-positioned glyph quad. Used by the rich text editor's
    /// paint walker to emit glyphs while applying a scroll/zoom offset.
    pub fn draw_glyph_quad(&mut self, quad: GlyphQuad) {
        let idx = self.frame.glyphs.len();
        self.frame.glyphs.push(quad);
        self.frame.draw_order.push(DrawCommand::Glyph(idx));
    }

    /// Consume the canvas and produce the accumulated RenderFrame.
    pub fn into_render_frame(self) -> RenderFrame {
        self.frame
    }
}

/// Debug-build check for the retained-layout draw paths: a `TextLayout`
/// produced under one ambient raster scale whose glyphs are drawn under
/// another samples bitmaps of the wrong density (soft or over-sharp text
/// under a scene zoom). Cosmetic, not corruption — log loudly instead of
/// aborting; the fix is to re-layout at paint time (what `draw_text` /
/// `draw_paragraph` do internally).
#[cfg_attr(not(debug_assertions), allow(unused_variables))]
fn debug_warn_raster_scale_mismatch(
    caller: &str,
    layout: &crate::text_backend::TextLayout,
    backend: &dyn crate::text_backend::TextBackend,
) {
    #[cfg(debug_assertions)]
    {
        let ambient = backend.raster_scale();
        if (layout.raster_scale - ambient).abs() > 0.001 {
            eprintln!(
                "[bastyde-canvas] {caller}: retained TextLayout (key {}) was produced at \
                 raster_scale {} but is drawn at ambient raster_scale {ambient} — its glyphs \
                 sample the wrong bitmap density. Re-layout in paint() instead of reusing a \
                 measure-time layout.",
                layout.layout_key, layout.raster_scale,
            );
        }
    }
}

/// Debug-only warning when a retained [`TextLayout`](crate::text_backend::TextLayout)
/// that measured to non-zero width resolves to zero glyphs — i.e. its glyphs were
/// evicted from the backend's glyph cache since it was laid out. Callers of
/// `draw_text_layout` must treat the `false` return as "re-shape needed" (e.g.
/// fall back to `draw_text`), or the text silently disappears until the next
/// relayout. A zero-width layout (empty / whitespace-only) legitimately produces
/// no glyphs and is not flagged.
#[cfg_attr(not(debug_assertions), allow(unused_variables))]
fn debug_warn_evicted_layout(
    caller: &str,
    layout: &crate::text_backend::TextLayout,
    backend: &dyn crate::text_backend::TextBackend,
) {
    #[cfg(debug_assertions)]
    {
        if layout.width > 0.0 {
            let text = match backend.debug_layout_text(layout.layout_key) {
                Some(t) => format!(" — text {:?}", DebugElide(&t)),
                None => String::new(),
            };
            eprintln!(
                "[bastyde-canvas] {caller}: retained TextLayout (key {}) resolved to zero \
                 glyphs despite measuring {:.1}px wide{text} — its glyphs were evicted from the \
                 backend cache. The caller must re-shape (e.g. fall back to draw_text) when \
                 draw_text_layout returns false, or the text will silently vanish until the \
                 next relayout.",
                layout.layout_key, layout.width,
            );
        }
    }
}

/// Wrapper that elides long strings in `{:?}` output so the eviction
/// warning stays a single readable line for paragraph-sized layouts.
#[cfg(debug_assertions)]
struct DebugElide<'a>(&'a str);

#[cfg(debug_assertions)]
impl std::fmt::Debug for DebugElide<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const MAX: usize = 80;
        if self.0.chars().count() <= MAX {
            std::fmt::Debug::fmt(self.0, f)
        } else {
            let head: String = self.0.chars().take(MAX).collect();
            std::fmt::Debug::fmt(&head, f)?;
            f.write_str("…")
        }
    }
}

/// Translate a paint's gradient geometry by `(dx, dy)`.
///
/// Gradient coordinates are relative to the rect the shader normalizes against,
/// and for a stroke that rect is the *expanded* one — see
/// [`Canvas::stroke_path_with_paint`], the only caller. A solid paint has no
/// geometry and passes through.
fn rebase_paint(paint: Paint, dx: f32, dy: f32) -> Paint {
    if dx == 0.0 && dy == 0.0 {
        return paint;
    }
    let shift = |p: Point| Point::new(p.x + dx, p.y + dy);
    match paint {
        Paint::LinearGradient { start, end, stops } => Paint::LinearGradient {
            start: shift(start),
            end: shift(end),
            stops,
        },
        Paint::RadialGradient {
            center,
            radius,
            stops,
        } => Paint::RadialGradient {
            center: shift(center),
            radius,
            stops,
        },
        Paint::ConicGradient {
            center,
            start_angle,
            stops,
        } => Paint::ConicGradient {
            center: shift(center),
            start_angle,
            stops,
        },
        other => other,
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
        Paint::ConicGradient {
            center,
            start_angle,
            stops,
        } => {
            let color = stops
                .first()
                .map(|s| s.color.to_array())
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            (
                color,
                PaintData::ConicGradient {
                    center: [center.x, center.y],
                    start_angle: *start_angle,
                    stops: stops.clone(),
                },
            )
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
            is_color: false,
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
        canvas.fill_rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            CornerRadius::uniform(6.0),
            Paint::Solid(Color::RED),
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
        let star = crate::path::Path::star(crate::geometry::Point::new(50.0, 50.0), 30.0, 15.0, 5);
        canvas.fill_path(&star, Color::RED);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.paths.len(), 1);
        assert_eq!(frame.paths[0].stroke_style.width, 0.0);
        assert!(matches!(frame.draw_order[0], DrawCommand::Path(0)));
        // A flat Color paint records PaintData::Solid.
        assert_eq!(frame.paths[0].paint_data, PaintData::Solid);
    }

    #[test]
    fn fill_path_with_gradient_records_gradient_paint_data() {
        use crate::paint::{GradientStop, Paint};
        let mut canvas = Canvas::new();
        let star = crate::path::Path::star(crate::geometry::Point::new(50.0, 50.0), 30.0, 15.0, 5);
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
        canvas.fill_path(&star, paint);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.paths.len(), 1);
        assert!(matches!(
            frame.paths[0].paint_data,
            PaintData::LinearGradient { .. }
        ));
    }

    #[test]
    fn fill_path_preserves_color_alpha() {
        // A translucent flat-color fill_path still records the same
        // PathEntry.color alpha — the atlas rasterization is now always
        // opaque-white (C3), but the API-visible color must be unchanged
        // (the GPU applies the tint, including its alpha, at draw time).
        let mut canvas = Canvas::new();
        let star = crate::path::Path::star(crate::geometry::Point::new(50.0, 50.0), 30.0, 15.0, 5);
        let translucent = Color::new(1.0, 0.0, 0.0, 0.3);
        canvas.fill_path(&star, translucent);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.paths[0].color, translucent.to_array());
        assert!((frame.paths[0].color[3] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn stroke_path_has_stroke_width() {
        let mut canvas = Canvas::new();
        let circle = crate::path::Path::circle(crate::geometry::Point::new(50.0, 50.0), 25.0);
        canvas.stroke_path(&circle, Color::BLACK, 2.0);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.paths.len(), 1);
        assert_eq!(frame.paths[0].stroke_style.width, 2.0);
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
        let layout = backend
            .borrow_mut()
            .layout_single_line("Test", &TextStyle::default(), None);
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
            underline_offset: 2.0,
            underline_thickness: 1.0,
            layout_key: 0,
            line_count: 1,
            spans: Vec::new(),
            raster_scale: 1.0,
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
        canvas.fill_rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            CornerRadius::uniform(0.0),
            paint,
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
        canvas.fill_rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            CornerRadius::uniform(0.0),
            paint,
        );
        let frame = canvas.into_render_frame();
        assert!(matches!(
            frame.shapes[0].paint_data,
            PaintData::RadialGradient { .. }
        ));
    }

    #[test]
    fn draw_shadow_produces_shadow_quad() {
        let mut canvas = Canvas::new();
        let shadow = Shadow {
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 4.0,
            spread: 0.0,
            color: Color::new(0.0, 0.0, 0.0, 0.3),
        };
        canvas.draw_shadow(
            Rect::new(10.0, 10.0, 100.0, 40.0),
            CornerRadius::uniform(6.0),
            &shadow,
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shadows.len(), 1);
        assert!(matches!(frame.draw_order[0], DrawCommand::Shadow(0)));
    }

    #[test]
    fn draw_border_produces_decorations() {
        let mut canvas = Canvas::new();
        canvas.draw_border(
            Rect::new(10.0, 10.0, 100.0, 50.0),
            Color::BLACK,
            1.0,
            1.0,
            1.0,
            1.0,
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.decorations.len(), 4);
    }

    #[test]
    fn draw_image_produces_image_quad() {
        let mut canvas = Canvas::new();
        canvas.draw_image(Rect::new(0.0, 0.0, 64.0, 64.0), "icon.png");
        let frame = canvas.into_render_frame();
        assert_eq!(frame.images.len(), 1);
        assert_eq!(frame.images[0].name, "icon.png");
        assert!(matches!(frame.draw_order[0], DrawCommand::Image(0)));
    }

    #[test]
    fn blend_mode_commands() {
        let mut canvas = Canvas::new();
        canvas.set_blend_mode(BlendMode::Multiply);
        canvas.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::RED);
        canvas.restore_blend_mode();
        let frame = canvas.into_render_frame();
        assert_eq!(frame.draw_order.len(), 3);
        assert!(matches!(
            frame.draw_order[0],
            DrawCommand::SetBlendMode(BlendMode::Multiply)
        ));
        assert!(matches!(frame.draw_order[2], DrawCommand::RestoreBlendMode));
    }

    #[test]
    fn transform_stack_save_restore() {
        let mut canvas = Canvas::new();
        assert!(canvas.current_transform().is_identity());
        canvas.save();
        canvas.translate(10.0, 20.0);
        let t = canvas.current_transform();
        assert!((t.m[4] - 10.0).abs() < 0.001);
        assert!((t.m[5] - 20.0).abs() < 0.001);
        canvas.restore();
        assert!(canvas.current_transform().is_identity());
    }

    #[test]
    fn transform_stack_compound() {
        let mut canvas = Canvas::new();
        canvas.save();
        canvas.translate(100.0, 0.0);
        canvas.scale(2.0, 2.0);
        let t = canvas.current_transform();
        // translate(100,0) then scale(2,2) → scale * translate: sx=2, tx=200
        assert!((t.m[0] - 2.0).abs() < 0.001);
        assert!((t.m[4] - 200.0).abs() < 0.001);
        canvas.restore();
        assert!(canvas.current_transform().is_identity());
    }

    #[test]
    fn draw_paragraph_without_backend() {
        let mut canvas = Canvas::new();
        let result = canvas.draw_paragraph(
            "Hello world",
            Rect::new(0.0, 0.0, 200.0, 100.0),
            &TextStyle::default(),
            Color::BLACK,
            None,
        );
        assert!(result.is_none());
    }

    #[test]
    fn draw_paragraph_with_mock_backend() {
        use crate::text_backend::MockTextBackend;
        let backend = Rc::new(RefCell::new(MockTextBackend::new()));
        let mut canvas = Canvas::with_text_backend(backend);
        let result = canvas.draw_paragraph(
            "Hello world",
            Rect::new(0.0, 0.0, 200.0, 100.0),
            &TextStyle::default(),
            Color::BLACK,
            None,
        );
        assert!(result.is_some());
    }

    #[test]
    fn draw_underline_produces_decoration() {
        let mut canvas = Canvas::new();
        canvas.draw_underline(
            Rect::new(0.0, 0.0, 100.0, 20.0),
            14.0,
            2.0,
            Color::BLACK,
            1.0,
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.decorations.len(), 1);
    }

    #[test]
    fn stroke_circle_produces_shape() {
        let mut canvas = Canvas::new();
        canvas.stroke_circle(Point::new(50.0, 50.0), 25.0, Color::RED, 2.0);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].shape, ShapeKind::Circle);
        assert!(frame.shapes[0].stroke_width > 0.0);
    }

    #[test]
    fn stroke_ellipse_produces_shape() {
        let mut canvas = Canvas::new();
        canvas.stroke_ellipse(Rect::new(0.0, 0.0, 100.0, 50.0), Color::BLUE, 1.5);
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].shape, ShapeKind::Ellipse);
        assert!(frame.shapes[0].stroke_width > 0.0);
    }

    #[test]
    fn cosmetic_draw_line_emits_unbaked_line() {
        let mut canvas = Canvas::new();
        canvas.draw_line(
            Point::new(0.0, 50.0),
            Point::new(200.0, 50.0),
            Color::BLACK,
            StrokeStyle::hairline(1.0),
        );
        let frame = canvas.into_render_frame();
        // Routed to the cosmetic path, NOT baked into a decoration rect.
        assert_eq!(frame.cosmetic_lines.len(), 1);
        assert!(frame.decorations.is_empty());
        assert!(matches!(frame.draw_order[0], DrawCommand::CosmeticLine(0)));
        assert_eq!(frame.cosmetic_lines[0].width, 1.0);
    }

    #[test]
    fn cosmetic_line_width_is_unbaked_under_transform() {
        // A canvas transform must not bake into the cosmetic width: the
        // stored width is identical regardless of any prior scale (the
        // transform-invariance is realized later, in the renderer).
        let mut canvas = Canvas::new();
        canvas.draw_line(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Color::BLACK,
            StrokeStyle::hairline(1.0),
        );
        canvas.scale(4.0, 4.0);
        canvas.draw_line(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Color::BLACK,
            StrokeStyle::hairline(1.0),
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.cosmetic_lines.len(), 2);
        assert_eq!(frame.cosmetic_lines[0].width, frame.cosmetic_lines[1].width);
        assert_eq!(frame.cosmetic_lines[1].width, 1.0);
    }

    #[test]
    fn logical_draw_line_still_bakes_decoration() {
        let mut canvas = Canvas::new();
        canvas.draw_line(
            Point::new(0.0, 10.0),
            Point::new(100.0, 10.0),
            Color::BLACK,
            1.0_f32, // From<f32> => logical solid
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.decorations.len(), 1);
        assert!(frame.cosmetic_lines.is_empty());
        assert!(matches!(frame.draw_order[0], DrawCommand::Decoration(0)));
    }

    #[test]
    fn cosmetic_stroke_rect_emits_four_cosmetic_lines() {
        let mut canvas = Canvas::new();
        canvas.stroke_rect(
            Rect::new(0.0, 0.0, 50.0, 30.0),
            Color::BLACK,
            StrokeStyle::hairline(1.0),
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.cosmetic_lines.len(), 4);
        assert!(frame.decorations.is_empty());
    }

    #[test]
    fn cosmetic_stroke_rounded_rect_marks_shape_device() {
        // A cosmetic rounded-rect border tags its ShapeQuad as Device so the
        // renderer holds the width constant under zoom; the width is carried
        // un-baked.
        let mut canvas = Canvas::new();
        canvas.stroke_rounded_rect(
            Rect::new(0.0, 0.0, 80.0, 40.0),
            CornerRadius::uniform(8.0),
            Color::BLACK,
            StrokeStyle::hairline(1.5),
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].stroke_space, StrokeSpace::Device);
        assert_eq!(frame.shapes[0].stroke_width, 1.5);
        assert!(matches!(frame.draw_order[0], DrawCommand::Shape(0)));
    }

    #[test]
    fn logical_stroke_rounded_rect_marks_shape_logical() {
        // The default (solid) stroke stays Logical (scales with the transform).
        let mut canvas = Canvas::new();
        canvas.stroke_rounded_rect(
            Rect::new(0.0, 0.0, 80.0, 40.0),
            CornerRadius::uniform(8.0),
            Color::BLACK,
            StrokeStyle::solid(1.5),
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes[0].stroke_space, StrokeSpace::Logical);
    }

    #[test]
    fn cosmetic_stroke_circle_and_ellipse_mark_device() {
        let mut canvas = Canvas::new();
        canvas.stroke_circle(
            Point::new(50.0, 50.0),
            20.0,
            Color::BLACK,
            StrokeStyle::hairline(1.0),
        );
        canvas.stroke_ellipse(
            Rect::new(0.0, 0.0, 60.0, 30.0),
            Color::BLACK,
            StrokeStyle::hairline(2.0),
        );
        let frame = canvas.into_render_frame();
        assert_eq!(frame.shapes.len(), 2);
        assert_eq!(frame.shapes[0].stroke_space, StrokeSpace::Device);
        assert_eq!(frame.shapes[1].stroke_space, StrokeSpace::Device);
        // Fills remain Logical (the default).
        canvas = Canvas::new();
        canvas.fill_circle(Point::new(10.0, 10.0), 5.0, Color::RED);
        let fill_frame = canvas.into_render_frame();
        assert_eq!(fill_frame.shapes[0].stroke_space, StrokeSpace::Logical);
    }
}
