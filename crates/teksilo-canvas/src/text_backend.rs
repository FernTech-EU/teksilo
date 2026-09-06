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

/// Reading direction of a [`TextLineSegment`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    LeftToRight,
    RightToLeft,
}

/// One character's extent along its segment's reading direction.
///
/// `position` is measured from the segment's *leading* edge — its left
/// edge in [`TextDirection::LeftToRight`], its right edge in
/// [`TextDirection::RightToLeft`] — so positions are non-decreasing and
/// widths are never negative in both directions. A character with no
/// advance of its own (a combining mark, the interior of a ligature)
/// reports its cluster's leading position and a width of `0.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharGeom {
    pub position: f32,
    pub width: f32,
}

/// How a laid-out line ends.
///
/// A screen reader distinguishes a paragraph break — which it announces,
/// and which occupies a character in the accessible text — from a soft
/// wrap, which occupies nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnd {
    /// Broken to fit the wrap width; nothing in the source separates this
    /// line from the next.
    SoftWrap,
    /// Ends with an explicit break in the source. `chars` is how many
    /// source characters the break occupies (`\n` → 1, `\r\n` → 2);
    /// `bytes` how many source bytes (likewise 1 and 2). AccessKit counts
    /// a `\r\n` as one character whose `character_lengths` entry is 2 —
    /// that is `bytes`.
    HardBreak { chars: u8, bytes: u8 },
    /// Ends because the source does.
    EndOfText,
}

/// Where a truncated line's ellipsis was drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineTruncation {
    /// Leading edge of the ellipsis, in layout-local coordinates.
    pub ellipsis_x: f32,
    /// Advance width of the ellipsis. Zero when the backend clipped
    /// rather than ellipsized.
    pub ellipsis_width: f32,
}

/// One direction-uniform stretch of a laid-out line.
///
/// A pure LTR or pure RTL line has exactly one segment; a bidirectional
/// line has one per direction change, in *logical* order.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLineSegment {
    /// Byte range in the text the geometry indexes (see
    /// [`TextGeometry::rendered_text`]).
    pub byte_range: std::ops::Range<usize>,
    /// Character range in the same text.
    pub char_range: std::ops::Range<usize>,
    pub direction: TextDirection,
    /// Bounding box `[x, y, width, height]` in layout-local coordinates.
    pub rect: [f32; 4],
    /// One entry per character of `char_range`, in logical order.
    pub characters: Vec<CharGeom>,
}

/// One laid-out visual line.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    /// Zero-based index within the layout.
    pub index: usize,
    /// Byte range of the line, including any trailing hard break.
    pub byte_range: std::ops::Range<usize>,
    /// Character range of the line, in the same text.
    pub char_range: std::ops::Range<usize>,
    /// Bounding box `[x, y, width, height]`; `y` is the top of the line
    /// box and `height` its full line height.
    pub rect: [f32; 4],
    /// Baseline y in layout-local coordinates.
    pub baseline: f32,
    /// Where a caret sits on a line with no glyphs of its own.
    pub caret_x: f32,
    /// Direction-uniform stretches in logical order.
    ///
    /// Empty means *unmeasurable* — a consumer must fall back to
    /// degenerate geometry for the whole line rather than for part of it
    /// — except on a line whose `char_range` is empty, which simply has
    /// no characters and locates its caret with `caret_x`.
    pub segments: Vec<TextLineSegment>,
    pub end: LineEnd,
    /// Set when the line was cut short.
    pub truncation: Option<LineTruncation>,
}

/// One hyperlink in a markup layout, reported against the rendered text.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLink {
    /// Byte range of the link's label in [`TextGeometry::rendered_text`]
    /// — *not* in the markup source.
    pub rendered_byte_range: std::ops::Range<usize>,
    pub url: String,
}

/// Per-line, per-character geometry for one laid-out text.
///
/// Accessibility needs what the shaper already knows one *character* at
/// a time: AccessKit's `Role::TextRun` carries `character_positions` and
/// `character_widths` so a screen reader can route a braille cell to a
/// word, a magnifier can follow the review cursor, and
/// `AXBoundsForRange` can answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextGeometry {
    /// One entry per emitted line, in visual top-to-bottom order.
    pub lines: Vec<TextLine>,
    /// How many further lines the layout produced but did not emit,
    /// because a `max_lines` cap cut them.
    pub dropped_lines: usize,
    /// Byte length of the text every range indexes.
    pub source_len: usize,
    /// The text the ranges index, when it differs from the caller's input
    /// — i.e. on the markup paths, where the syntax has been stripped.
    /// `None` on the plain-text paths, where the input *is* that text.
    pub rendered_text: Option<String>,
    /// Hyperlinks found in the markup, against the rendered text.
    pub links: Vec<TextLink>,
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
    /// Per-line, per-character geometry, when the backend produced it.
    ///
    /// Shared rather than owned: a `TextLayout` is cloned out of the
    /// backend's layout cache on every hit, and the geometry is the
    /// largest thing it carries. Behind an `Rc` a cache hit costs a
    /// refcount, and the geometry lives exactly as long as the cache
    /// entry that produced it — no second map to invalidate, no key to
    /// look up backwards.
    pub geometry: Option<std::rc::Rc<TextGeometry>>,
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
///
/// Deliberately simple, but not *degenerate*: it measures characters
/// (not bytes), treats `\n` as a hard break, wraps at word boundaries,
/// hands out a distinct `layout_key` per layout, and reports the same
/// per-line, per-character [`TextGeometry`] a real backend does. Tests
/// that assert on accessible text runs need geometry that is real
/// enough to be wrong when the code under test is wrong; a backend that
/// reported nothing would make every such test vacuous.
///
/// Every character advances by `char_width` (8 dp) except the `\r` and
/// `\n` of a line terminator, which advance by nothing.
pub struct MockTextBackend {
    char_width: f32,
    line_height: f32,
    next_key: u64,
    texts: std::collections::HashMap<u64, String>,
    cache_generation: u64,
}

/// One line the mock broke the source into.
struct MockLine {
    bytes: std::ops::Range<usize>,
    chars: std::ops::Range<usize>,
    end: LineEnd,
}

impl MockTextBackend {
    pub fn new() -> Self {
        Self {
            char_width: 8.0,
            line_height: 16.0,
            next_key: 1,
            texts: std::collections::HashMap::new(),
            cache_generation: 0,
        }
    }

    /// Forget every layout key handed out so far, as a real backend does
    /// when its layout cache is cleared. Bumps
    /// [`layout_cache_generation`](TextBackend::layout_cache_generation)
    /// so a widget holding a retained layout re-shapes instead of
    /// drawing a dangling key.
    pub fn invalidate(&mut self) {
        self.texts.clear();
        self.cache_generation += 1;
    }

    fn take_key(&mut self, text: &str) -> u64 {
        let key = self.next_key;
        self.next_key += 1;
        self.texts.insert(key, text.to_string());
        key
    }

    /// Advance width of one character. A line terminator takes no space.
    fn advance_of(&self, ch: char) -> f32 {
        if ch == '\n' || ch == '\r' {
            0.0
        } else {
            self.char_width
        }
    }

    /// Split `text` at explicit breaks. Always yields at least one line,
    /// and the ranges are contiguous and cover the whole source — a
    /// trailing `\n` therefore produces an empty final line, as a real
    /// engine's does.
    fn split_hard_lines(text: &str) -> Vec<(std::ops::Range<usize>, LineEnd)> {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut start = 0usize;
        for i in 0..bytes.len() {
            if bytes[i] == b'\n' {
                let crlf = i > start && bytes[i - 1] == b'\r';
                let n = if crlf { 2 } else { 1 };
                out.push((start..i + 1, LineEnd::HardBreak { chars: n, bytes: n }));
                start = i + 1;
            }
        }
        out.push((start..text.len(), LineEnd::EndOfText));
        out
    }

    /// Greedily wrap `range` at spaces, or mid-word when a word is wider
    /// than the line. The pieces are contiguous and cover `range`.
    fn wrap_range(
        text: &str,
        range: std::ops::Range<usize>,
        max_chars: usize,
    ) -> Vec<std::ops::Range<usize>> {
        if max_chars == 0 || range.is_empty() {
            return vec![range];
        }
        let mut pieces = Vec::new();
        let mut piece_start = range.start;
        let mut chars_in_piece = 0usize;
        let mut last_break: Option<usize> = None;
        for (offset, ch) in text[range.clone()].char_indices() {
            let at = range.start + offset;
            if chars_in_piece >= max_chars {
                let cut = match last_break {
                    Some(b) if b > piece_start && b <= at => b,
                    _ => at,
                };
                if cut > piece_start {
                    pieces.push(piece_start..cut);
                    piece_start = cut;
                    chars_in_piece = text[piece_start..at].chars().count();
                    last_break = None;
                }
            }
            chars_in_piece += 1;
            if ch == ' ' {
                last_break = Some(at + ch.len_utf8());
            }
        }
        pieces.push(piece_start..range.end);
        pieces
    }

    /// Break `text` into display lines: explicit breaks first, then word
    /// wrapping within each. A line's terminator stays on the line it
    /// ends, so the ranges remain contiguous.
    fn break_lines(text: &str, max_chars: Option<usize>) -> Vec<MockLine> {
        let mut out: Vec<MockLine> = Vec::new();
        let mut char_cursor = 0usize;
        for (range, end) in Self::split_hard_lines(text) {
            let term = match end {
                LineEnd::HardBreak { bytes, .. } => bytes as usize,
                _ => 0,
            };
            let body = range.start..range.end - term;
            let mut pieces = match max_chars {
                Some(n) => Self::wrap_range(text, body, n),
                None => vec![body],
            };
            if term > 0
                && let Some(last) = pieces.last_mut()
            {
                last.end += term;
            }
            let last_index = pieces.len() - 1;
            for (i, piece) in pieces.into_iter().enumerate() {
                let count = text[piece.clone()].chars().count();
                out.push(MockLine {
                    chars: char_cursor..char_cursor + count,
                    bytes: piece,
                    end: if i == last_index {
                        end
                    } else {
                        LineEnd::SoftWrap
                    },
                });
                char_cursor += count;
            }
        }
        out
    }

    /// Geometry for a set of broken lines stacked from the origin.
    ///
    /// `visible_chars` limits the first line to a prefix, which is how
    /// the single-line path reports a trailing ellipsis: the line still
    /// covers the whole source, but only the drawn prefix is measured.
    fn geometry_for(
        &self,
        text: &str,
        lines: &[MockLine],
        dropped_lines: usize,
        visible_chars: Option<usize>,
        rendered_text: Option<String>,
        links: Vec<TextLink>,
    ) -> TextGeometry {
        let out = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let y = index as f32 * self.line_height;
                let mut characters = Vec::new();
                let mut position = 0.0f32;
                let mut measured_bytes = line.bytes.start;
                for (offset, ch) in text[line.bytes.clone()].char_indices() {
                    if visible_chars.is_some_and(|n| characters.len() >= n) {
                        break;
                    }
                    let width = self.advance_of(ch);
                    characters.push(CharGeom { position, width });
                    position += width;
                    measured_bytes = line.bytes.start + offset + ch.len_utf8();
                }
                let segments = if characters.is_empty() {
                    Vec::new()
                } else {
                    vec![TextLineSegment {
                        byte_range: line.bytes.start..measured_bytes,
                        char_range: line.chars.start..line.chars.start + characters.len(),
                        direction: TextDirection::LeftToRight,
                        rect: [0.0, y, position, self.line_height],
                        characters,
                    }]
                };
                TextLine {
                    index,
                    byte_range: line.bytes.clone(),
                    char_range: line.chars.clone(),
                    rect: [0.0, y, position, self.line_height],
                    baseline: y + self.line_height * 0.75,
                    caret_x: 0.0,
                    segments,
                    end: line.end,
                    truncation: visible_chars.map(|_| LineTruncation {
                        ellipsis_x: position,
                        ellipsis_width: self.char_width,
                    }),
                }
            })
            .collect();
        TextGeometry {
            lines: out,
            dropped_lines,
            source_len: text.len(),
            rendered_text,
            links,
        }
    }

    /// Strip the supported markup (`[label](url)`, `**bold**`,
    /// `*italic*`) and report each link against the rendered text.
    ///
    /// An approximation of the real parser, enough for tests to assert
    /// that a markup label announces its rendered text and that its
    /// links carry rendered — not source — offsets.
    fn flatten_markup(source: &str) -> (String, Vec<TextLink>) {
        let mut rendered = String::with_capacity(source.len());
        let mut links = Vec::new();
        let bytes = source.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'['
                && let Some(close) = source[i..].find("](").map(|o| i + o)
                && let Some(end) = source[close + 2..].find(')').map(|o| close + 2 + o)
            {
                let label = &source[i + 1..close];
                let url = &source[close + 2..end];
                let start = rendered.len();
                rendered.push_str(label);
                links.push(TextLink {
                    rendered_byte_range: start..rendered.len(),
                    url: url.to_string(),
                });
                i = end + 1;
                continue;
            }
            if bytes[i] == b'*' {
                i += if source[i..].starts_with("**") { 2 } else { 1 };
                continue;
            }
            let ch = source[i..]
                .chars()
                .next()
                .expect("byte index on a boundary");
            rendered.push(ch);
            i += ch.len_utf8();
        }
        (rendered, links)
    }

    fn layout_from_lines(
        &mut self,
        source_key_text: &str,
        geometry: TextGeometry,
        width: f32,
    ) -> TextLayout {
        let line_count = geometry.lines.len().max(1);
        let key = self.take_key(source_key_text);
        TextLayout {
            width,
            height: line_count as f32 * self.line_height,
            ascent: self.line_height * 0.75,
            descent: self.line_height * 0.25,
            underline_offset: 2.0,
            underline_thickness: 1.0,
            layout_key: key,
            line_count,
            spans: Vec::new(),
            raster_scale: 1.0,
            geometry: Some(std::rc::Rc::new(geometry)),
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
        let char_count = text.chars().count();
        let width = char_count as f32 * self.char_width;
        let (clamped_width, visible) = match max_width {
            Some(max) if width > max => {
                let budget = (max - self.char_width).max(0.0);
                let kept = (budget / self.char_width).floor() as usize;
                (kept as f32 * self.char_width + self.char_width, Some(kept))
            }
            Some(max) => (width.min(max), None),
            None => (width, None),
        };
        // One display line: the single-line path never breaks, so an
        // embedded `\n` is an ordinary (zero-advance) character.
        let lines = vec![MockLine {
            bytes: 0..text.len(),
            chars: 0..char_count,
            end: LineEnd::EndOfText,
        }];
        let geometry = self.geometry_for(text, &lines, 0, visible, None, Vec::new());
        let mut layout = self.layout_from_lines(text, geometry, clamped_width);
        layout.height = self.line_height;
        layout.line_count = 1;
        layout
    }

    fn layout_paragraph(
        &mut self,
        text: &str,
        _style: &TextStyle,
        max_width: f32,
        max_lines: Option<usize>,
    ) -> TextLayout {
        let max_chars = (max_width / self.char_width).floor() as usize;
        let mut lines = Self::break_lines(text, Some(max_chars));
        let dropped = match max_lines {
            Some(n) if lines.len() > n => {
                let dropped = lines.len() - n;
                lines.truncate(n);
                dropped
            }
            _ => 0,
        };
        let geometry = self.geometry_for(text, &lines, dropped, None, None, Vec::new());
        let width = geometry
            .lines
            .iter()
            .map(|l| l.rect[2])
            .fold(0.0f32, f32::max);
        self.layout_from_lines(text, geometry, width)
    }

    fn layout_single_line_markup(
        &mut self,
        source: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextLayout {
        let (rendered, links) = Self::flatten_markup(source);
        let mut layout = self.layout_single_line(&rendered, style, max_width);
        if let Some(geometry) = layout.geometry.as_mut().and_then(std::rc::Rc::get_mut) {
            geometry.rendered_text = Some(rendered);
            geometry.links = links;
        }
        layout
    }

    fn layout_paragraph_markup(
        &mut self,
        source: &str,
        style: &TextStyle,
        max_width: f32,
        max_lines: Option<usize>,
    ) -> TextLayout {
        let (rendered, links) = Self::flatten_markup(source);
        let mut layout = self.layout_paragraph(&rendered, style, max_width, max_lines);
        if let Some(geometry) = layout.geometry.as_mut().and_then(std::rc::Rc::get_mut) {
            geometry.rendered_text = Some(rendered);
            geometry.links = links;
        }
        layout
    }

    fn ensure_glyphs(&mut self, layout: &TextLayout) -> Vec<GlyphQuad> {
        // One quad per laid-out character with an advance of its own, at
        // the position the geometry reports — so a test that reads quad
        // positions and a test that reads character positions cannot
        // disagree.
        let Some(geometry) = layout.geometry.as_ref() else {
            return Vec::new();
        };
        let mut quads = Vec::new();
        for line in &geometry.lines {
            for segment in &line.segments {
                for ch in &segment.characters {
                    if ch.width <= 0.0 {
                        continue;
                    }
                    quads.push(GlyphQuad {
                        screen: [
                            segment.rect[0] + ch.position,
                            line.rect[1],
                            ch.width,
                            self.line_height,
                        ],
                        atlas: [0.0, 0.0, ch.width, self.line_height],
                        color: [0.0, 0.0, 0.0, 1.0],
                        is_color: false,
                    });
                }
            }
        }
        quads
    }

    fn layout_cache_generation(&self) -> u64 {
        self.cache_generation
    }

    fn debug_layout_text(&self, layout_key: u64) -> Option<String> {
        self.texts.get(&layout_key).cloned()
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
