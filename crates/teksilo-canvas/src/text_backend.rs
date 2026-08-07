// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_tokens::TextStyle;

use crate::geometry::Point;
use crate::render_frame::GlyphQuad;

/// How a [`TextWidget`](../teksilo_widgets/primitives/struct.TextWidget.html)
/// should handle text that doesn't fit in the proposed width.
///
/// The default is [`Wrap`](TextOverflow::Wrap): text flows onto multiple
/// lines and the widget grows vertically. Widgets that must stay on a
/// single line (buttons, menu items, tab headers) opt out by setting
/// [`Ellipsis(Trailing)`](EllipsisMode::Trailing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextOverflow {
    /// Keep the text on one line; replace the overflowing region with an
    /// ellipsis ("…") at the position indicated by [`EllipsisMode`].
    Ellipsis(EllipsisMode),
    /// Wrap the text across multiple lines. The widget grows vertically
    /// to fit every line; horizontal width is bounded by the layout
    /// proposal.
    #[default]
    Wrap,
}

/// Where the ellipsis character goes when a single-line text is too wide
/// for its layout proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EllipsisMode {
    /// `"Lorem ipsum do…"` — truncate at the right edge.
    Trailing,
    /// `"Lorem…dolor"` — keep the beginning and the end, ellipsize the
    /// middle.
    Middle,
    /// `"…dolor sit amet"` — truncate at the left edge.
    Leading,
}

/// Result of measuring text.
#[derive(Debug, Clone)]
pub struct TextLayout {
    pub width: f32,
    pub height: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Distance from baseline to the top of the underline, in logical pixels.
    /// Positive = below the baseline. Sourced from the primary font's
    /// `post` table via the underlying shaper.
    pub underline_offset: f32,
    /// Underline line thickness in logical pixels. Sourced from the
    /// primary font's stroke size.
    pub underline_thickness: f32,
    /// Opaque key for the backend to identify the cached layout.
    pub layout_key: u64,
    /// Number of lines (1 for single-line, ≥1 for paragraph).
    pub line_count: usize,
    /// Per-span rectangles produced by the markup-aware layout path.
    /// Empty for plain-text layouts.
    pub spans: Vec<TextLayoutSpan>,
    /// The backend's ambient raster scale at the time this layout was
    /// produced (see [`TextBackend::set_raster_scale`]). Metrics are
    /// raster-scale-independent, but the glyph quads behind
    /// `layout_key` sample bitmaps of this density — drawing a
    /// retained layout under a *different* ambient scale renders
    /// soft/oversharp glyphs. `Canvas::draw_text_layout` debug-asserts
    /// on the mismatch; widgets that retain layouts across paints
    /// should re-layout when the scale changed.
    pub raster_scale: f32,
}

/// One laid-out span inside a [`TextLayout`]. Populated by
/// `layout_*_markup` calls; each span carries its bounding rectangle in
/// the layout's local coordinate space (origin at top-left of the
/// widget's text region).
#[derive(Debug, Clone)]
pub struct TextLayoutSpan {
    pub kind: TextSpanKind,
    pub line_index: usize,
    /// Local-space rectangle: `[x, y, width, height]`.
    pub rect: [f32; 4],
    /// Byte range into the original markup source string.
    pub byte_range: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSpanKind {
    Text,
    Link { url: String },
}

/// What the hit-test found at a particular point inside a [`TextLayout`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitTarget {
    Text,
    Link { url: String },
}

impl TextLayout {
    /// Hit-test a point against the per-span rectangles. The point is in
    /// the same local coordinate space as `rect` — the caller must
    /// translate window-space into the text region before calling.
    ///
    /// Returns `None` if the point is outside every span.
    pub fn hit_test(&self, point: Point) -> Option<HitTarget> {
        for sp in self.spans.iter().rev() {
            // Walk in reverse so the last-emitted span (visually on top)
            // wins in the edge case of overlapping rects.
            let [x, y, w, h] = sp.rect;
            if point.x >= x && point.x < x + w && point.y >= y && point.y < y + h {
                return Some(match &sp.kind {
                    TextSpanKind::Link { url } => HitTarget::Link { url: url.clone() },
                    TextSpanKind::Text => HitTarget::Text,
                });
            }
        }
        None
    }
}

/// Quantize an accumulated transform scale onto a geometric ladder of
/// 1.25ⁿ steps, `n ∈ [0, 6]` (so the value lands in `[1.0, ~3.81]`),
/// for use as the glyph raster densification under a scale transform
/// (see [`TextBackend::set_raster_scale`]).
///
/// The ladder bounds the number of distinct atlas entries a continuous
/// zoom gesture can create (7 buckets) and the bucket value is derived
/// from an integer index, so the same input always yields the
/// bit-identical f32 — cache keys stay stable across frames — and the
/// function is idempotent (a bucket value maps to itself; the cap is
/// clamped on the *index* so it is a ladder value too). Between buckets
/// the residual GPU scaling is at most ~12%, invisible under the glyph
/// atlas's linear filtering. Scales below 1 clamp to 1: zoomed-out text
/// relies on linear minification rather than rasterizing below logical
/// size.
pub fn quantize_raster_scale(scale: f32) -> f32 {
    if !scale.is_finite() || scale <= 1.0 {
        return 1.0;
    }
    const STEP: f32 = 1.25;
    /// 1.25⁶ ≈ 3.81 — the densest raster bucket. Deep zoom beyond it
    /// rides linear magnification; an unbounded ladder would explode
    /// atlas area quadratically.
    const MAX_BUCKET: i32 = 6;
    let bucket = ((scale.ln() / STEP.ln()).round() as i32).clamp(0, MAX_BUCKET);
    STEP.powi(bucket)
}

/// Trait for text layout and glyph rasterization backends.
/// Implemented by teksilo-text (wrapping text-typeset) for real rendering,
/// and by a mock for headless tests.
pub trait TextBackend {
    /// Set the display scale factor (e.g. 2.0 for HiDPI/Retina).
    /// Implementations should rasterize glyphs at `font_size * scale_factor`
    /// while returning metrics in logical pixels.
    fn set_scale_factor(&mut self, _scale_factor: f32) {}

    /// Set the ambient raster scale for subsequent `layout_*` /
    /// `ensure_glyphs` calls.
    ///
    /// The paint walker sets this to the accumulated (quantized) scale
    /// of the transform scopes enclosing the widget being painted, so
    /// text drawn under a scale transform (scene zoom, `Scale` wrapper)
    /// rasterizes at `font_size × scale_factor × raster_scale` and
    /// stays sharp once the GPU transform stretches it. Layout metrics
    /// are raster-scale-independent — only the bitmaps behind the
    /// returned quads densify — so this never causes reflow.
    ///
    /// Unlike [`set_scale_factor`](Self::set_scale_factor) this is
    /// cheap to flip per widget: implementations key their caches by
    /// it instead of clearing them. Backends without a glyph raster
    /// (the mock) keep the default no-op.
    fn set_raster_scale(&mut self, _raster_scale: f32) {}

    /// Current ambient raster scale (see
    /// [`set_raster_scale`](Self::set_raster_scale)). `1.0` = unscaled.
    fn raster_scale(&self) -> f32 {
        1.0
    }

    /// Measure and layout a single line of text.
    fn layout_single_line(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextLayout;

    /// Measure and layout a paragraph of text with word wrapping.
    /// Default implementation delegates to `layout_single_line`.
    fn layout_paragraph(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: f32,
        _max_lines: Option<usize>,
    ) -> TextLayout {
        self.layout_single_line(text, style, Some(max_width))
    }

    /// Single-line layout for minimal-markdown source text.
    ///
    /// `source` is a raw string containing the supported subset
    /// (`[label](url)`, `*italic*`, `**bold**`). The backend parses the
    /// markup internally and returns a `TextLayout` whose `spans` field
    /// is populated with per-run rectangles (including link positions)
    /// for hit-testing.
    ///
    /// Default implementation falls back to the plain path, dropping
    /// span metadata — override in real backends.
    fn layout_single_line_markup(
        &mut self,
        source: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextLayout {
        self.layout_single_line(source, style, max_width)
    }

    /// Paragraph layout for minimal-markdown source text. See
    /// [`layout_single_line_markup`](Self::layout_single_line_markup).
    fn layout_paragraph_markup(
        &mut self,
        source: &str,
        style: &TextStyle,
        max_width: f32,
        max_lines: Option<usize>,
    ) -> TextLayout {
        self.layout_paragraph(source, style, max_width, max_lines)
    }

    /// Produce GPU-ready glyph quads for a previously laid-out text.
    /// The quads are positioned relative to (0, 0); the caller offsets them.
    fn ensure_glyphs(&mut self, layout: &TextLayout) -> Vec<GlyphQuad>;

    /// Refresh the backend's last-used timestamp for every glyph produced
    /// by the layout identified by `layout_key`. Called by the widget-tree
    /// renderer when a widget's `cached_paint` is reused without invoking
    /// `widget.paint()` — the normal `ensure_glyphs` touch path is
    /// bypassed in that case, and without this call still-visible glyphs
    /// can age out of the backend's atlas cache and have their atlas
    /// slots re-used by newly rasterized glyphs, producing garbled text.
    ///
    /// Implementations that don't maintain a glyph cache (the mock
    /// backend) can leave the default no-op.
    fn touch_layout(&mut self, _layout_key: u64) {}

    /// Monotonic counter bumped every time the backend's glyph atlas
    /// drops or relocates entries (LRU eviction, scale-factor reset).
    ///
    /// Any cache that retains glyph quads across frames *outside* the
    /// widget arena's `cached_paint` (e.g. the scene per-item cache)
    /// must compare this against the epoch it stored at bake time and
    /// rebuild when it moved — baked-in atlas UVs may now point at
    /// pixels owned by unrelated glyphs. Backends without a glyph
    /// cache never bump it.
    fn glyph_epoch(&self) -> u64 {
        0
    }

    /// Debug-build corruption check: verify that the glyph quads the
    /// backend handed out for `layout_key` still match the live glyph
    /// atlas.
    ///
    /// Called (under `cfg(debug_assertions)`) by the widget-tree
    /// renderer whenever a retained paint cache is replayed without
    /// re-running `paint()`. See [`GlyphValidation`] for how callers
    /// should react to each outcome. Backends without a glyph cache
    /// keep the default (always [`GlyphValidation::Valid`]).
    fn debug_validate_layout(&self, _layout_key: u64) -> GlyphValidation {
        GlyphValidation::Valid
    }

    /// Monotonic counter bumped every time the backend's retained
    /// layout→glyph cache (the map behind [`ensure_glyphs`](Self::ensure_glyphs))
    /// is cleared wholesale: a scale-factor reset, or an explicit
    /// invalidate on the eviction-recovery path.
    ///
    /// Distinct from [`glyph_epoch`](Self::glyph_epoch): that tracks atlas
    /// *eviction/relocation* (including LRU, which leaves the layout→glyph
    /// map intact), and drives the per-frame atlas re-upload decision.
    /// This tracks the *map clear* — the only event after which a retained
    /// `TextLayout`'s `layout_key` stops resolving and `ensure_glyphs`
    /// returns empty. A widget that caches a `TextLayout` across the
    /// layout→paint boundary records this at layout time and compares at
    /// paint time, so it can re-shape *before* drawing a dangling key
    /// (rather than discovering it via a `false` return from
    /// `draw_text_layout`). Backends without such a cache keep the
    /// default `0`.
    fn layout_cache_generation(&self) -> u64 {
        0
    }

    /// Debug-build diagnostic: recover the source text behind a
    /// `layout_key` so warnings can name the impacted string.
    ///
    /// Used by the evicted-layout warning in
    /// [`Canvas::draw_text_layout`](crate::canvas::Canvas::draw_text_layout):
    /// the glyph cache for a key can be evicted while the metrics cache
    /// — which is keyed by the text — survives, so a real backend can map
    /// the key back to its text for the message. Returns `None` when the
    /// backend can't (the default, including the mock, which shares one
    /// `layout_key` across all layouts). Never on a hot path; only invoked
    /// from a `cfg(debug_assertions)` warning.
    fn debug_layout_text(&self, _layout_key: u64) -> Option<String> {
        None
    }
}

/// Outcome of [`TextBackend::debug_validate_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphValidation {
    /// Every glyph of the layout is resident in the atlas at exactly the
    /// rectangle baked into the retained quads.
    Valid,
    /// The backend no longer knows this layout key (its layout/glyph
    /// caches were cleared since the quads were baked). A retained paint
    /// that outlives a wholesale cache clear is the signature of a
    /// missing invalidation path — suspicious, but transitional frames
    /// around legitimate clears (scale-factor change) can hit it too, so
    /// callers should log loudly rather than abort.
    StaleKey,
    /// At least one glyph was evicted or now occupies a different atlas
    /// rectangle than the one baked into the retained quads. Replaying
    /// those quads draws the wrong pixels — definite corruption; callers
    /// should abort (debug builds) with a diagnostic.
    RectMismatch,
}

/// Atlas information from the text backend for GPU upload.
#[derive(Debug, Clone)]
pub struct AtlasInfo {
    /// True when the atlas pixels changed since the previous
    /// `atlas_info` call (any caller). Kept for single-consumer call
    /// sites; multi-window callers should rely on `version` instead.
    pub dirty: bool,
    pub width: u32,
    pub height: u32,
    /// Atlas pixels, populated only when the caller's `seen_version`
    /// lags `version` (the caller needs to upload). Empty otherwise to
    /// avoid a ~1 MB memcpy per clean frame.
    pub pixels: Vec<u8>,
    /// Monotonic atlas content version. Each consumer (window renderer)
    /// records the version it last uploaded and passes it back as
    /// `seen_version`; a difference means "upload `pixels` now". This
    /// replaces consume-once dirty semantics so several windows can all
    /// converge on the same atlas content.
    pub version: u64,
    /// True when glyph eviction occurred since the previous `atlas_info`
    /// call — regardless of which internal path evicted (snapshot-driven
    /// or render-driven). Callers that cache glyph output (paint caches)
    /// must invalidate, since evicted atlas space may be reused by future
    /// glyph allocations.
    pub glyphs_evicted: bool,
}

/// A mock text backend for headless testing.
/// Returns fixed-size measurements without real font rendering.
pub struct MockTextBackend {
    char_width: f32,
    line_height: f32,
}

impl MockTextBackend {
    pub fn new() -> Self {
        Self {
            char_width: 8.0,
            line_height: 16.0,
        }
    }
}

impl Default for MockTextBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBackend for MockTextBackend {
    fn layout_single_line(
        &mut self,
        text: &str,
        _style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextLayout {
        let width = text.len() as f32 * self.char_width;
        let clamped_width = match max_width {
            Some(max) => width.min(max),
            None => width,
        };
        TextLayout {
            width: clamped_width,
            height: self.line_height,
            ascent: self.line_height * 0.75,
            descent: self.line_height * 0.25,
            underline_offset: 2.0,
            underline_thickness: 1.0,
            layout_key: 0,
            line_count: 1,
            spans: Vec::new(),
            raster_scale: 1.0,
        }
    }

    fn layout_paragraph(
        &mut self,
        text: &str,
        _style: &TextStyle,
        max_width: f32,
        max_lines: Option<usize>,
    ) -> TextLayout {
        let max_chars_per_line = (max_width / self.char_width).floor() as usize;
        if max_chars_per_line == 0 {
            return TextLayout {
                width: 0.0,
                height: self.line_height,
                ascent: self.line_height * 0.75,
                descent: self.line_height * 0.25,
                underline_offset: 2.0,
                underline_thickness: 1.0,
                layout_key: 0,
                line_count: 1,
                spans: Vec::new(),
                raster_scale: 1.0,
            };
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        let mut lines: Vec<f32> = Vec::new(); // width of each line
        let mut current_line_chars: usize = 0;

        for word in &words {
            let word_len = word.len();
            let needed = if current_line_chars == 0 {
                word_len
            } else {
                current_line_chars + 1 + word_len // space + word
            };

            if needed > max_chars_per_line && current_line_chars > 0 {
                // Wrap: finish current line, start new one
                lines.push(current_line_chars as f32 * self.char_width);
                current_line_chars = word_len;
            } else {
                current_line_chars = needed;
            }
        }
        // Finish last line
        if current_line_chars > 0 || lines.is_empty() {
            lines.push(current_line_chars as f32 * self.char_width);
        }

        // Apply max_lines limit
        if let Some(max) = max_lines {
            lines.truncate(max);
        }

        let line_count = lines.len();
        let max_line_width = lines.iter().cloned().fold(0.0_f32, f32::max);

        TextLayout {
            width: max_line_width,
            height: line_count as f32 * self.line_height,
            ascent: self.line_height * 0.75,
            descent: self.line_height * 0.25,
            underline_offset: 2.0,
            underline_thickness: 1.0,
            layout_key: 0,
            line_count,
            spans: Vec::new(),
            raster_scale: 1.0,
        }
    }

    fn ensure_glyphs(&mut self, layout: &TextLayout) -> Vec<GlyphQuad> {
        // Return one fake glyph per 8px of width (matching the mock char width)
        // so that draw_text_layout tests can verify rendering happens.
        let char_count = (layout.width / 8.0).ceil() as usize;
        if char_count == 0 {
            return Vec::new();
        }
        (0..char_count)
            .map(|i| GlyphQuad {
                screen: [i as f32 * 8.0, 0.0, 8.0, layout.height],
                atlas: [0.0, 0.0, 8.0, layout.height],
                color: [0.0, 0.0, 0.0, 1.0],
                is_color: false,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_raster_scale_ladder_properties() {
        // Identity and zoom-out clamp to 1.0 (never rasterize below
        // logical size).
        assert_eq!(quantize_raster_scale(1.0), 1.0);
        assert_eq!(quantize_raster_scale(0.5), 1.0);
        assert_eq!(quantize_raster_scale(0.0), 1.0);
        assert_eq!(quantize_raster_scale(f32::NAN), 1.0);
        // Bucket values are exact powers of 1.25 (bit-stable cache keys).
        assert_eq!(quantize_raster_scale(2.0), 1.25_f32.powi(3));
        assert_eq!(quantize_raster_scale(3.0), 1.25_f32.powi(5));
        // Deep zoom clamps to the top bucket (a ladder value, so the
        // clamp preserves idempotence).
        assert_eq!(quantize_raster_scale(10.0), 1.25_f32.powi(6));
        // Idempotent: a bucket value maps to itself, so re-quantizing an
        // already-quantized accumulated scale is a no-op.
        for raw in [1.1, 1.5, 2.0, 2.7, 3.3, 5.0] {
            let q = quantize_raster_scale(raw);
            assert_eq!(quantize_raster_scale(q), q, "not idempotent at {raw}");
        }
    }

    #[test]
    fn mock_backend_measures_text() {
        let mut backend = MockTextBackend::new();
        let layout = backend.layout_single_line("Hello", &TextStyle::default(), None);
        assert_eq!(layout.width, 40.0); // 5 chars × 8.0
        assert_eq!(layout.height, 16.0);
    }

    #[test]
    fn mock_backend_respects_max_width() {
        let mut backend = MockTextBackend::new();
        let layout = backend.layout_single_line("Hello World", &TextStyle::default(), Some(50.0));
        assert!(layout.width <= 50.0);
    }

    #[test]
    fn mock_backend_empty_text() {
        let mut backend = MockTextBackend::new();
        let layout = backend.layout_single_line("", &TextStyle::default(), None);
        assert_eq!(layout.width, 0.0);
        assert!(layout.height > 0.0); // still has line height
    }

    #[test]
    fn mock_backend_ensure_glyphs_returns_fake_quads() {
        let mut backend = MockTextBackend::new();
        let layout = backend.layout_single_line("Hi", &TextStyle::default(), None);
        let glyphs = backend.ensure_glyphs(&layout);
        // "Hi" = 2 chars * 8px = 16px width → ceil(16/8) = 2 glyphs
        assert_eq!(glyphs.len(), 2);
    }

    #[test]
    fn mock_backend_single_line_count() {
        let mut backend = MockTextBackend::new();
        let layout = backend.layout_single_line("Hello", &TextStyle::default(), None);
        assert_eq!(layout.line_count, 1);
    }

    #[test]
    fn mock_backend_paragraph_wraps() {
        let mut backend = MockTextBackend::new();
        // "Hello World" = 11 chars × 8 = 88px. Max width 50px → 6 chars per line
        let layout = backend.layout_paragraph("Hello World", &TextStyle::default(), 50.0, None);
        assert_eq!(layout.line_count, 2);
        assert_eq!(layout.height, 32.0); // 2 lines × 16px
    }

    #[test]
    fn mock_backend_paragraph_max_lines() {
        let mut backend = MockTextBackend::new();
        // Multiple words that would wrap to 3+ lines, but limit to 2
        let layout = backend.layout_paragraph(
            "one two three four five",
            &TextStyle::default(),
            40.0, // 5 chars max per line
            Some(2),
        );
        assert_eq!(layout.line_count, 2);
    }

    #[test]
    fn mock_backend_paragraph_single_line_fits() {
        let mut backend = MockTextBackend::new();
        let layout = backend.layout_paragraph("Hi", &TextStyle::default(), 100.0, None);
        assert_eq!(layout.line_count, 1);
        assert_eq!(layout.width, 16.0); // 2 chars × 8
    }
}
