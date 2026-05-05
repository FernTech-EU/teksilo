//! Four-pass walker over a text-typeset `RenderFrame`.
//!
//! The editor calls one of the typesetter's three render paths (full,
//! block-only, cursor-only) and hands the resulting frame to
//! [`paint_frame`] along with the Canvas it should draw into. The walker
//! translates every rect and glyph into fern-canvas primitives, applying
//! the scroll and zoom offset.
//!
//! Pass order (matches the godot-rich-text reference):
//!   1. Background decorations (Selection, CellSelection, Background,
//!      BlockBackground, TextBackground, TableCellBackground, TableBorder)
//!   2. Glyphs (offset + color baked into GlyphQuad)
//!   3. Inline images (resolved via the image cache)
//!   4. Foreground decorations (Cursor, Underline, Overline, Strikeout)

use fern_canvas::{Canvas, GlyphQuad as CanvasGlyphQuad, Point, StrokeStyle};
use fern_text::text_document::TextDocument;
use fern_text::{
    DecorationRect, ImageQuad, RenderFrame, TypesetterDecorationKind as DecorationKind,
    TypesetterGlyphQuad as GlyphQuad,
};
use fern_tokens::Color;

use super::image_cache::ImageCache;

/// Parameters forwarded from the editor widget to the paint pass.
pub struct PaintParams<'a> {
    pub frame: &'a RenderFrame,
    /// Widget's top-left in parent space. text-typeset's
    /// `render()` emits glyph and decoration coordinates that
    /// are already widget-relative (the typesetter subtracts
    /// `scroll_offset` internally before writing screen Y), so
    /// the paint walker only has to add this origin to land
    /// them in parent space — no further scroll math.
    pub origin: Point,
    pub document: &'a TextDocument,
    pub image_cache: &'a mut ImageCache,
    /// Whether to draw the caret this paint. The editor's frame loop
    /// sets this from `caret_visible && has_focus && caret_policy != Hidden`.
    pub draw_caret: bool,
}

/// Run the four-pass walker.
pub fn paint_frame(canvas: &mut Canvas, params: PaintParams<'_>) {
    let PaintParams {
        frame,
        origin,
        document,
        image_cache,
        draw_caret,
    } = params;

    let offset_x = origin.x;
    let offset_y = origin.y;

    paint_backgrounds(canvas, &frame.decorations, offset_x, offset_y);
    paint_glyphs(canvas, &frame.glyphs, offset_x, offset_y);
    paint_images(
        canvas,
        &frame.images,
        document,
        image_cache,
        offset_x,
        offset_y,
    );
    paint_foreground(canvas, &frame.decorations, offset_x, offset_y, draw_caret);
}

fn paint_backgrounds(canvas: &mut Canvas, decorations: &[DecorationRect], ox: f32, oy: f32) {
    for deco in decorations {
        let is_bg = matches!(
            deco.kind,
            DecorationKind::Selection
                | DecorationKind::CellSelection
                | DecorationKind::Background
                | DecorationKind::BlockBackground
                | DecorationKind::TextBackground
                | DecorationKind::TableCellBackground
                | DecorationKind::TableBorder
        );
        if !is_bg {
            continue;
        }

        let rect = shifted_rect(deco.rect, ox, oy);
        let color = Color::from_rgba(deco.color[0], deco.color[1], deco.color[2], deco.color[3]);

        if deco.kind == DecorationKind::TableBorder {
            stroked_rect(canvas, rect, color);
        } else {
            canvas.fill_rect(rect, color);
        }
    }
}

fn paint_glyphs(canvas: &mut Canvas, glyphs: &[GlyphQuad], ox: f32, oy: f32) {
    for g in glyphs {
        let quad = CanvasGlyphQuad {
            screen: [g.screen[0] + ox, g.screen[1] + oy, g.screen[2], g.screen[3]],
            atlas: g.atlas,
            color: g.color,
            is_color: g.is_color,
        };
        canvas.draw_glyph_quad(quad);
    }
}

fn paint_images(
    canvas: &mut Canvas,
    images: &[ImageQuad],
    document: &TextDocument,
    image_cache: &mut ImageCache,
    ox: f32,
    oy: f32,
) {
    for img in images {
        // Prime the cache so the renderer can later resolve the resource
        // name to bytes. On failure, silently skip.
        if image_cache.get_or_load(document, &img.name).is_none() {
            continue;
        }
        let rect = shifted_rect(img.screen, ox, oy);
        canvas.draw_image(rect, img.name.clone());
    }
}

fn paint_foreground(
    canvas: &mut Canvas,
    decorations: &[DecorationRect],
    ox: f32,
    oy: f32,
    draw_caret: bool,
) {
    for deco in decorations {
        let rect = shifted_rect(deco.rect, ox, oy);
        let color = Color::from_rgba(deco.color[0], deco.color[1], deco.color[2], deco.color[3]);

        match deco.kind {
            DecorationKind::Cursor if draw_caret => {
                canvas.fill_rect(rect, color);
            }
            DecorationKind::Underline | DecorationKind::Overline | DecorationKind::Strikeout => {
                let y_mid = rect.y + rect.height * 0.5;
                let start = Point::new(rect.x, y_mid);
                let end = Point::new(rect.x + rect.width, y_mid);
                canvas.draw_line(start, end, color, StrokeStyle::solid(rect.height.max(1.0)));
            }
            _ => {}
        }
    }
}

fn shifted_rect(raw: [f32; 4], ox: f32, oy: f32) -> fern_canvas::Rect {
    fern_canvas::Rect::new(raw[0] + ox, raw[1] + oy, raw[2], raw[3])
}

fn stroked_rect(canvas: &mut Canvas, rect: fern_canvas::Rect, color: Color) {
    let stroke = StrokeStyle::solid(1.0);
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = x0 + rect.width;
    let y1 = y0 + rect.height;
    canvas.draw_line(
        Point::new(x0, y0),
        Point::new(x1, y0),
        color,
        stroke.clone(),
    );
    canvas.draw_line(
        Point::new(x1, y0),
        Point::new(x1, y1),
        color,
        stroke.clone(),
    );
    canvas.draw_line(
        Point::new(x1, y1),
        Point::new(x0, y1),
        color,
        stroke.clone(),
    );
    canvas.draw_line(Point::new(x0, y1), Point::new(x0, y0), color, stroke);
}
