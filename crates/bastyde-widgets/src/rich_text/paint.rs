// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Four-pass walker over a text-typeset `RenderFrame`.
//!
//! The editor calls one of the typesetter's three render paths (full,
//! block-only, cursor-only) and hands the resulting frame to
//! [`paint_frame`] along with the Canvas it should draw into. The walker
//! translates every rect and glyph into bastyde-canvas primitives, applying
//! the scroll and zoom offset.
//!
//! Pass order (matches the godot-rich-text reference):
//!   1. Background decorations (Selection, CellSelection, Background,
//!      BlockBackground, TextBackground, TableCellBackground, TableBorder)
//!   2. Glyphs (offset + color baked into GlyphQuad)
//!   3. Inline images (resolved via the image cache)
//!   4. Foreground decorations (Cursor, Underline, Overline, Strikeout)

use bastyde_canvas::{Canvas, GlyphQuad as CanvasGlyphQuad, Point, StrokeStyle};
use bastyde_text::text_document::TextDocument;
use bastyde_text::{
    DecorationRect, ImageQuad, RenderFrame, TypesetterDecorationKind as DecorationKind,
    TypesetterGlyphQuad as GlyphQuad,
};
use bastyde_tokens::Color;

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
    /// Asked for an image's bytes when the document has none under that name —
    /// see [`crate::rich_text::image_cache::ImageResolver`]. `None` on a host that supplies
    /// every image itself, which is every host that never pastes one in.
    pub image_resolver: Option<&'a crate::rich_text::image_cache::ImageResolver>,
    /// The current selection as `[start, end)` document character offsets, or
    /// `None` when there is none.
    ///
    /// The typesetter already emits a Selection rect spanning a selected image
    /// — it walks the image's synthetic glyph like any other — but that rect is
    /// painted in pass 1 and the image's own opaque draw in pass 3 covers it
    /// completely, so a selected image looked exactly like an unselected one.
    /// Given the range, `paint_images` can put the highlight back *over* the
    /// picture, which is the only place it can be seen.
    pub selection: Option<(usize, usize)>,
    /// The resolved selection colour, matching what the typesetter drew
    /// underneath — so an image reads as part of one continuous selection
    /// rather than as a separately-styled object.
    pub selection_color: [f32; 4],
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
        image_resolver,
        selection,
        selection_color,
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
        image_resolver,
        selection,
        selection_color,
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

/// How strongly a selected image is tinted.
///
/// The selection colour's *own* alpha is deliberately not used. Behind text it
/// is opaque and can afford to be — the glyphs paint on top of it, so the
/// colour is a background. Over an image the same rect is a foreground, and at
/// full strength it replaces the picture with a solid block of colour. A
/// selected photograph has to stay a recognisable photograph.
///
/// Low enough to read as a wash rather than a filter, and carried by the border
/// below, which is what actually makes the selection unmistakable.
const SELECTED_IMAGE_TINT_ALPHA: f32 = 0.28;

/// Border width for a selected image, in logical pixels.
///
/// The half of the marking that does the work: it sits outside the picture's
/// own content, so it stays legible over a busy or dark image where any tint
/// would be lost.
const SELECTED_IMAGE_BORDER_WIDTH: f32 = 2.0;

fn paint_images(
    canvas: &mut Canvas,
    images: &[ImageQuad],
    document: &TextDocument,
    image_cache: &mut ImageCache,
    image_resolver: Option<&crate::rich_text::image_cache::ImageResolver>,
    selection: Option<(usize, usize)>,
    selection_color: [f32; 4],
    ox: f32,
    oy: f32,
) {
    for img in images {
        // Decode and upload the pixels before emitting the draw. A draw command
        // naming a texture the canvas was never given is silently dropped by
        // the renderer, so skipping this registration makes an inline image
        // occupy its full layout box and paint nothing at all.
        if !image_cache.ensure_registered(canvas, document, &img.name, image_resolver) {
            continue;
        }
        let rect = shifted_rect(img.screen, ox, oy);
        canvas.draw_image(rect, img.name.clone());

        // The selection highlight the typesetter drew for this image is
        // underneath the pixels just painted, so it is invisible. Put it back
        // on top: a wash in the selection colour, plus an outline in the same
        // colour at full strength — the wash alone is easy to miss over a busy
        // or dark photograph, and an outline alone reads as a frame the writer
        // added rather than as a selection.
        //
        // Matched by offset, not by name: a document may hold one picture in
        // three places, and only one of them is selected.
        let selected =
            selection.is_some_and(|(start, end)| img.char_offset >= start && img.char_offset < end);
        if selected {
            let [r, g, b, _] = selection_color;
            canvas.fill_rect(rect, Color::from_rgba(r, g, b, SELECTED_IMAGE_TINT_ALPHA));
            canvas.stroke_rect(
                rect,
                Color::from_rgba(r, g, b, 1.0),
                bastyde_canvas::StrokeStyle::solid(SELECTED_IMAGE_BORDER_WIDTH),
            );
        }
    }
}

#[cfg(test)]
mod image_paint_tests {
    use super::*;
    use bastyde_canvas::{DrawCommand, RenderFrame};
    use bastyde_text::text_document::{ResourceType, TextDocument};

    /// A 2×2 opaque-red PNG, encoded here so the test owns its own input.
    fn red_png() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut buf, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 255].repeat(4)).unwrap();
        }
        buf
    }

    fn quad(name: &str) -> ImageQuad {
        ImageQuad {
            screen: [0.0, 0.0, 20.0, 20.0],
            name: name.to_string(),
            char_offset: 0,
        }
    }

    fn run(doc: &TextDocument, cache: &mut ImageCache, quads: &[ImageQuad]) -> RenderFrame {
        let mut canvas = Canvas::new();
        paint_images(
            &mut canvas,
            quads,
            doc,
            cache,
            None,
            None,
            [0.0; 4],
            0.0,
            0.0,
        );
        canvas.into_render_frame()
    }

    /// The same, with a host standing by to supply what the document lacks.
    fn run_with_resolver(
        doc: &TextDocument,
        cache: &mut ImageCache,
        quads: &[ImageQuad],
        resolver: &crate::rich_text::image_cache::ImageResolver,
    ) -> RenderFrame {
        let mut canvas = Canvas::new();
        paint_images(
            &mut canvas,
            quads,
            doc,
            cache,
            Some(resolver),
            None,
            [0.0; 4],
            0.0,
            0.0,
        );
        canvas.into_render_frame()
    }

    /// The selection rect the typesetter draws for an image is painted in
    /// pass 1 and buried by the image's own opaque draw in pass 3, so a
    /// selected image looked exactly like an unselected one.
    #[test]
    fn a_selected_image_is_marked_on_top_of_its_own_pixels() {
        let doc = TextDocument::new();
        doc.add_resource(ResourceType::Image, "a.png", "image/png", &red_png())
            .unwrap();
        let mut q = quad("a.png");
        q.char_offset = 7;

        let unselected = {
            let mut canvas = Canvas::new();
            paint_images(
                &mut canvas,
                &[q.clone()],
                &doc,
                &mut ImageCache::new(),
                None,
                None,
                [0.0; 4],
                0.0,
                0.0,
            );
            canvas.into_render_frame()
        };
        let selected = {
            let mut canvas = Canvas::new();
            paint_images(
                &mut canvas,
                &[q.clone()],
                &doc,
                &mut ImageCache::new(),
                None,
                Some((7, 8)),
                [0.2, 0.4, 0.9, 0.35],
                0.0,
                0.0,
            );
            canvas.into_render_frame()
        };

        assert_eq!(image_draws(&unselected), 1);
        assert_eq!(image_draws(&selected), 1, "the image is still drawn");
        assert!(
            selected.draw_order.len() > unselected.draw_order.len(),
            "a selected image must be marked: {} commands vs {}",
            selected.draw_order.len(),
            unselected.draw_order.len()
        );
    }

    /// The theme's selection colour is **opaque** (`editor_selection_bg` is a
    /// six-digit hex). Behind text that is correct — glyphs paint on top of it.
    /// Painted over an image at the same strength it replaces the picture with
    /// a solid block of colour, which is what this pins against.
    #[test]
    fn a_selected_image_is_tinted_not_covered() {
        let doc = TextDocument::new();
        doc.add_resource(ResourceType::Image, "a.png", "image/png", &red_png())
            .unwrap();
        let mut q = quad("a.png");
        q.char_offset = 4;

        let mut canvas = Canvas::new();
        paint_images(
            &mut canvas,
            &[q],
            &doc,
            &mut ImageCache::new(),
            None,
            Some((4, 5)),
            // Fully opaque, exactly as the theme supplies it.
            [0.66, 0.88, 0.91, 1.0],
            0.0,
            0.0,
        );
        let frame = canvas.into_render_frame();

        // The tint is the rect covering the whole quad; the border is four
        // thin edges of the same colour, and it is opaque on purpose.
        let quad_area = 20.0 * 20.0;
        let fills: Vec<[f32; 4]> = frame
            .decorations
            .iter()
            .filter(|d| (d.rect[2] * d.rect[3] - quad_area).abs() < 1.0)
            .map(|d| d.color)
            .collect();
        assert_eq!(fills.len(), 1, "expected exactly one tint over the image");
        assert!(
            fills[0][3] < 0.5,
            "the tint must leave the picture visible, got alpha {}",
            fills[0][3]
        );
        // …and the border is still full strength, which is what makes the
        // selection unmistakable over a busy or dark picture.
        assert!(
            frame
                .decorations
                .iter()
                .any(|d| (d.color[3] - 1.0).abs() < 0.01),
            "the border was lost"
        );
        // The hue is still the selection's, so the image reads as part of one
        // continuous selection rather than as a separately-styled object.
        assert!((fills[0][0] - 0.66).abs() < 0.01, "{:?}", fills[0]);
    }

    #[test]
    fn only_the_selected_placement_of_a_repeated_image_is_marked() {
        // The same picture three times, one of them selected. Matching on the
        // name would light all three.
        let doc = TextDocument::new();
        doc.add_resource(ResourceType::Image, "a.png", "image/png", &red_png())
            .unwrap();
        let quads: Vec<ImageQuad> = [3usize, 11, 20]
            .into_iter()
            .map(|off| {
                let mut q = quad("a.png");
                q.char_offset = off;
                q
            })
            .collect();

        let count = |sel: Option<(usize, usize)>| {
            let mut canvas = Canvas::new();
            paint_images(
                &mut canvas,
                &quads,
                &doc,
                &mut ImageCache::new(),
                None,
                sel,
                [0.2, 0.4, 0.9, 0.35],
                0.0,
                0.0,
            );
            canvas.into_render_frame().draw_order.len()
        };

        let none = count(None);
        let one = count(Some((11, 12)));
        let all = count(Some((0, 30)));
        assert!(one > none, "the selected one must be marked");
        assert!(all > one, "selecting all three must mark all three");
        assert_eq!(
            all - none,
            (one - none) * 3,
            "each selected image costs the same marking"
        );
    }

    #[test]
    fn a_name_the_document_does_not_know_is_asked_for_once() {
        // Pasting an image into a second editor brings the reference and not the
        // pixels — the interchange format carries names. Without a way to ask,
        // the picture lays out at full size and paints nothing.
        let doc = TextDocument::new();
        let asked = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let counter = asked.clone();
        let resolver: crate::rich_text::image_cache::ImageResolver =
            std::rc::Rc::new(move |name: &str| {
                counter.set(counter.get() + 1);
                (name == "pasted.png").then(|| ("image/png".to_string(), red_png()))
            });

        let mut cache = ImageCache::new();
        let frame = run_with_resolver(&doc, &mut cache, &[quad("pasted.png")], &resolver);
        assert_eq!(image_draws(&frame), 1, "the supplied image must be drawn");
        assert_eq!(asked.get(), 1);

        // And the answer is kept on the document, not just in this cache — a
        // save, an export, or a second view of the same document all read the
        // resource table.
        assert!(
            doc.resource("pasted.png").ok().flatten().is_some(),
            "the bytes were not written back to the document"
        );

        // Painting again must not ask a second time.
        let frame = run_with_resolver(&doc, &mut cache, &[quad("pasted.png")], &resolver);
        assert_eq!(image_draws(&frame), 1);
        assert_eq!(asked.get(), 1, "the resolver was consulted on every paint");
    }

    #[test]
    fn a_host_that_cannot_supply_it_either_is_not_asked_again() {
        let doc = TextDocument::new();
        let asked = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let counter = asked.clone();
        let resolver: crate::rich_text::image_cache::ImageResolver =
            std::rc::Rc::new(move |_: &str| {
                counter.set(counter.get() + 1);
                None
            });

        let mut cache = ImageCache::new();
        for _ in 0..3 {
            let frame = run_with_resolver(&doc, &mut cache, &[quad("gone.png")], &resolver);
            assert_eq!(image_draws(&frame), 0);
        }
        assert_eq!(
            asked.get(),
            1,
            "a missing image must not cost a lookup on every frame"
        );
    }

    #[test]
    fn a_resource_the_document_already_has_never_reaches_the_host() {
        let doc = TextDocument::new();
        doc.add_resource(ResourceType::Image, "own.png", "image/png", &red_png())
            .unwrap();
        let asked = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let counter = asked.clone();
        let resolver: crate::rich_text::image_cache::ImageResolver =
            std::rc::Rc::new(move |_: &str| {
                counter.set(counter.get() + 1);
                None
            });

        let mut cache = ImageCache::new();
        let frame = run_with_resolver(&doc, &mut cache, &[quad("own.png")], &resolver);
        assert_eq!(image_draws(&frame), 1);
        assert_eq!(asked.get(), 0, "the document's own resource was bypassed");
    }

    fn image_draws(frame: &RenderFrame) -> usize {
        frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, DrawCommand::Image(_)))
            .count()
    }

    /// The regression this whole pass exists for. Emitting a draw command
    /// without first registering the pixels produces a frame the renderer
    /// silently discards: the image reserves its layout box and paints nothing.
    #[test]
    fn a_resolvable_image_is_registered_and_drawn() {
        let doc = TextDocument::new();
        doc.add_resource(ResourceType::Image, "photo.png", "image/png", &red_png())
            .unwrap();
        let mut cache = ImageCache::new();

        let frame = run(&doc, &mut cache, &[quad("photo.png")]);

        assert_eq!(frame.pending_images.len(), 1, "pixels must be uploaded");
        assert_eq!(frame.pending_images[0].name, "photo.png");
        assert_eq!(image_draws(&frame), 1, "and a draw must be emitted");
        assert_eq!(cache.size_of("photo.png"), Some((2, 2)));
    }

    /// A name with no resource behind it must emit neither, or the frame
    /// carries a draw pointing at a texture that will never exist.
    #[test]
    fn an_unresolvable_image_emits_nothing() {
        let doc = TextDocument::new();
        let mut cache = ImageCache::new();

        let frame = run(&doc, &mut cache, &[quad("missing.png")]);

        assert!(frame.pending_images.is_empty());
        assert_eq!(image_draws(&frame), 0);
        assert_eq!(cache.len(), 1, "the failure is cached, not retried");
    }

    /// Bytes that are not a decodable image fail the same way as a missing
    /// resource — not by panicking somewhere inside the decoder.
    #[test]
    fn undecodable_bytes_are_negatively_cached() {
        let doc = TextDocument::new();
        doc.add_resource(
            ResourceType::Image,
            "broken.png",
            "image/png",
            b"not an image",
        )
        .unwrap();
        let mut cache = ImageCache::new();

        let frame = run(&doc, &mut cache, &[quad("broken.png")]);

        assert!(frame.pending_images.is_empty());
        assert_eq!(image_draws(&frame), 0);
    }

    /// Repainting must not re-upload: the second frame draws from the texture
    /// registered by the first.
    #[test]
    fn a_second_paint_draws_without_re_uploading() {
        let doc = TextDocument::new();
        doc.add_resource(ResourceType::Image, "photo.png", "image/png", &red_png())
            .unwrap();
        let mut cache = ImageCache::new();

        let first = run(&doc, &mut cache, &[quad("photo.png")]);
        let second = run(&doc, &mut cache, &[quad("photo.png")]);

        assert_eq!(first.pending_images.len(), 1);
        assert!(
            second.pending_images.is_empty(),
            "already-registered pixels must not be uploaded again"
        );
        assert_eq!(image_draws(&second), 1, "but it must still draw");
    }

    /// Two references to one image in a single frame upload once and draw twice.
    #[test]
    fn one_upload_serves_repeated_references() {
        let doc = TextDocument::new();
        doc.add_resource(ResourceType::Image, "photo.png", "image/png", &red_png())
            .unwrap();
        let mut cache = ImageCache::new();

        let frame = run(&doc, &mut cache, &[quad("photo.png"), quad("photo.png")]);

        assert_eq!(frame.pending_images.len(), 1);
        assert_eq!(image_draws(&frame), 2);
    }

    /// After the renderer loses its textures the decode is kept but the upload
    /// is redone — decoding again would be far more expensive.
    #[test]
    fn invalidating_registrations_re_uploads_without_re_decoding() {
        let doc = TextDocument::new();
        doc.add_resource(ResourceType::Image, "photo.png", "image/png", &red_png())
            .unwrap();
        let mut cache = ImageCache::new();

        run(&doc, &mut cache, &[quad("photo.png")]);
        cache.invalidate_registrations();
        let frame = run(&doc, &mut cache, &[quad("photo.png")]);

        assert_eq!(frame.pending_images.len(), 1, "re-uploaded");
        assert_eq!(cache.len(), 1, "still only one decoded entry");
    }

    /// Eviction reports what it dropped so the caller can free GPU textures;
    /// without it a long editing session grows both tables without bound.
    #[test]
    fn retain_only_drops_and_reports_dead_entries() {
        let doc = TextDocument::new();
        for name in ["a.png", "b.png"] {
            doc.add_resource(ResourceType::Image, name, "image/png", &red_png())
                .unwrap();
        }
        let mut cache = ImageCache::new();
        run(&doc, &mut cache, &[quad("a.png"), quad("b.png")]);
        assert_eq!(cache.len(), 2);

        let dropped = cache.retain_only(["a.png"]);

        assert_eq!(dropped, vec!["b.png".to_string()]);
        assert_eq!(cache.len(), 1);
        assert!(cache.size_of("b.png").is_none());
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

fn shifted_rect(raw: [f32; 4], ox: f32, oy: f32) -> bastyde_canvas::Rect {
    bastyde_canvas::Rect::new(raw[0] + ox, raw[1] + oy, raw[2], raw[3])
}

fn stroked_rect(canvas: &mut Canvas, rect: bastyde_canvas::Rect, color: Color) {
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
