use std::collections::HashMap;

use fern_canvas::GlyphQuad;
use fern_canvas::text_backend::{
    AtlasInfo, TextBackend, TextLayout, TextLayoutSpan, TextSpanKind,
};
use fern_tokens::TextStyle;
use text_typeset::{
    DocumentFlow, FontFaceId, InlineMarkup, LaidOutSpanKind, ParagraphResult, SingleLineResult,
    TextFontService, TextFormat,
};
use text_typeset::atlas::cache::GlyphCacheKey;

/// Which layout method produced the cache entry — separates the
/// single-line and paragraph caches so a single-line truncated result
/// never masquerades as a wrapped paragraph result (or vice versa).
#[derive(Clone, PartialEq, Eq, Hash)]
enum LayoutMode {
    SingleLine,
    /// `max_lines` cap expressed as `u32::MAX` when unbounded.
    Paragraph(u32),
    SingleLineMarkup,
    ParagraphMarkup(u32),
}

/// Cache key for text layout results.
#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutCacheKey {
    text: String,
    font_family: String,
    font_size_bits: u32, // f32 as bits for Hash/Eq
    font_weight: u32,
    max_width_bits: Option<u32>,
    mode: LayoutMode,
}

impl LayoutCacheKey {
    fn new_single_line(
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        scale_factor: f32,
    ) -> Self {
        let scaled_size = style.size * scale_factor;
        Self {
            text: text.to_string(),
            font_family: style.family.clone(),
            font_size_bits: scaled_size.to_bits(),
            font_weight: style.weight.0 as u32,
            max_width_bits: max_width.map(|w| (w * scale_factor).to_bits()),
            mode: LayoutMode::SingleLine,
        }
    }

    fn new_paragraph(
        text: &str,
        style: &TextStyle,
        max_width: f32,
        max_lines: Option<usize>,
        scale_factor: f32,
    ) -> Self {
        let scaled_size = style.size * scale_factor;
        let cap = max_lines.map(|n| n.min(u32::MAX as usize) as u32).unwrap_or(u32::MAX);
        Self {
            text: text.to_string(),
            font_family: style.family.clone(),
            font_size_bits: scaled_size.to_bits(),
            font_weight: style.weight.0 as u32,
            max_width_bits: Some((max_width * scale_factor).to_bits()),
            mode: LayoutMode::Paragraph(cap),
        }
    }
}

// Only cache the metrics — glyphs are re-generated on demand during paint.

/// Bridge between fern-canvas's `TextBackend` trait and text-typeset.
///
/// Holds a shared [`TextFontService`] (font registry + glyph atlas +
/// shaper cache) plus a dedicated [`DocumentFlow`] used only for the
/// single-line / paragraph label API exposed via `TextBackend`. Rich
/// text widgets that need a full-document flow keep their OWN
/// `DocumentFlow` on the `RichTextEngine` side; they reach through
/// this bridge to grab a mutable borrow of the shared service when
/// they layout and render, so every widget's glyphs land in the same
/// GPU atlas.
pub struct TypesetterBridge {
    service: TextFontService,
    /// Dedicated flow used by the `TextBackend` label path
    /// (`layout_single_line` / `layout_paragraph` / their markup
    /// variants). Labels do not need a persistent flow-layout state,
    /// so this single flow is sufficient — every `layout_*` call
    /// reshapes from scratch through the shared service's atlas.
    label_flow: DocumentFlow,
    default_font: Option<FontFaceId>,
    next_layout_key: u64,
    /// Layout metrics cache: avoids re-shaping text just for size
    /// measurement.
    layout_cache: HashMap<LayoutCacheKey, TextLayout>,
    /// Glyph quads + per-glyph cache keys stored by opaque layout key
    /// so ensure_glyphs can be resolved independently for many text
    /// widgets in the same frame. The cache keys are used to touch
    /// glyphs in text-typeset's internal `GlyphCache` so they aren't
    /// evicted while still being rendered via paint-cache hits.
    glyph_cache: HashMap<u64, (Vec<GlyphQuad>, Vec<GlyphCacheKey>)>,
    /// Whether any text work (`layout_single_line`/`ensure_glyphs`)
    /// happened since the last `atlas_info()` call. When false we
    /// skip advancing the eviction generation to avoid aging out
    /// idle-but-visible glyphs.
    had_text_activity: bool,
    /// Sticky atlas-dirty flag set by rich-text widgets via
    /// `service_mut()`. text-typeset's `render()` path clears
    /// `atlas.dirty` after copying pixels into the returned
    /// `RenderFrame`. fern-app consumes the atlas through
    /// `atlas_info()` *after* `tree.render()`, so by then the
    /// typesetter's own dirty flag is already false and the atlas
    /// would never be uploaded, leaving rich-text glyphs invisible
    /// on the GPU. This flag closes the gap: anyone who took a
    /// mutable borrow of the service since the last `atlas_info()`
    /// call forces the bridge to report the atlas as dirty one
    /// more time.
    rich_text_atlas_dirty: bool,
}

impl TypesetterBridge {
    pub fn new() -> Self {
        Self {
            service: TextFontService::new(),
            label_flow: DocumentFlow::new(),
            default_font: None,
            next_layout_key: 1,
            layout_cache: HashMap::new(),
            glyph_cache: HashMap::new(),
            had_text_activity: false,
            rich_text_atlas_dirty: false,
        }
    }

    /// Create a bridge with the bundled default font (Inter) plus
    /// every script-specific fallback font whose Cargo feature is
    /// enabled (`fonts-arabic`, `fonts-hebrew`, …).
    ///
    /// Inter is the primary font — it's what the default `TextStyle`
    /// asks for via `TypographyTokens::default().family = "Inter"` —
    /// and covers Latin, Cyrillic, Greek, and Vietnamese. The
    /// additional Noto Sans variable fonts are registered into the
    /// same font registry so that text-typeset's shaper-level
    /// `.notdef` fallback loop can cover mixed-script text without
    /// any locale awareness at the caller site.
    pub fn new_with_default_font() -> Self {
        let mut bridge = Self::new();
        bridge.register_default_font();
        bridge
    }

    /// Register a font from raw TTF/OTF data. Forwards to the
    /// shared [`TextFontService`].
    pub fn register_font(&mut self, data: &[u8]) -> FontFaceId {
        self.service.register_font(data)
    }

    /// Register Inter as the primary default, then register every
    /// feature-gated script-specific fallback font. The
    /// feature-gated registrations discard their `FontFaceId`
    /// because fallback eligibility only requires that a font be in
    /// the registry — text-typeset's `find_fallback_font` iterates
    /// every registered font and picks the first one whose charmap
    /// covers a `.notdef` glyph's codepoint.
    fn register_default_font(&mut self) {
        let inter_data = include_bytes!("../fonts/InterVariable.ttf");
        let face_id = self.service.register_font(inter_data);
        self.service.set_default_font(face_id, 14.0);
        self.default_font = Some(face_id);

        #[cfg(feature = "fonts-arabic")]
        {
            let data =
                include_bytes!("../fonts/NotoSansArabic-VariableFont_wdth,wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-hebrew")]
        {
            let data =
                include_bytes!("../fonts/NotoSansHebrew-VariableFont_wdth,wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-thai")]
        {
            let data =
                include_bytes!("../fonts/NotoSansThai-VariableFont_wdth,wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-devanagari")]
        {
            let data =
                include_bytes!("../fonts/NotoSansDevanagari-VariableFont_wdth,wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-cjk-sc")]
        {
            let data = include_bytes!("../fonts/NotoSansSC-VariableFont_wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-cjk-jp")]
        {
            let data = include_bytes!("../fonts/NotoSansJP-VariableFont_wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-cjk-kr")]
        {
            let data = include_bytes!("../fonts/NotoSansKR-VariableFont_wght.ttf");
            let _ = self.service.register_font(data);
        }

        // Optional runtime color-emoji fallback. Reads the platform's
        // installed emoji font at startup when `system-emoji` is on.
        // Silent on miss so headless / embedded targets aren't noisy.
        #[cfg(feature = "system-emoji")]
        {
            let _ = self.register_system_emoji_font();
        }
    }

    /// Try to register the platform's default color-emoji font as a
    /// fallback. Returns the registered [`FontFaceId`] on success, or
    /// `None` if no emoji font was found at a known system path.
    ///
    /// Called automatically by [`new_with_default_font`](Self::new_with_default_font).
    /// Exposed publicly so apps that construct a bare bridge via
    /// [`new`](Self::new) can still opt in without replicating the
    /// per-OS path list.
    ///
    /// Requires the `system-emoji` feature.
    #[cfg(feature = "system-emoji")]
    pub fn register_system_emoji_font(&mut self) -> Option<FontFaceId> {
        crate::system_emoji::load_system_emoji_data()
            .map(|data| self.service.register_font(&data))
    }

    /// Set the default font and size. Forwards to the shared
    /// [`TextFontService`].
    pub fn set_default_font(&mut self, face_id: FontFaceId, size_px: f32) {
        self.service.set_default_font(face_id, size_px);
        self.default_font = Some(face_id);
    }

    /// Get atlas information for GPU upload.
    ///
    /// Only advances the glyph cache generation and runs eviction
    /// when text work happened since the last call — this prevents
    /// aging out glyphs that are still visible but cached (idle
    /// app scenario).
    pub fn atlas_info(&mut self) -> AtlasInfo {
        let snapshot = self.service.atlas_snapshot(self.had_text_activity);
        let rich_dirty = self.rich_text_atlas_dirty;
        self.rich_text_atlas_dirty = false;
        self.had_text_activity = false;
        let dirty = snapshot.dirty || rich_dirty;
        // Only copy the atlas pixels when the renderer is actually
        // going to upload them. Callers consult `dirty` first and
        // skip `upload_atlas` otherwise — so a clean-atlas frame no
        // longer pays for a ~1 MB memcpy (512×512×4 or larger).
        // Profiling the animations example showed this clone
        // dominating self-time at ~6 % during shader-driven
        // animations, purely because it ran at the ~30 Hz frame
        // rate even though nothing in the atlas had changed.
        let pixels = if dirty {
            snapshot.pixels.to_vec()
        } else {
            Vec::new()
        };
        AtlasInfo {
            dirty,
            width: snapshot.width,
            height: snapshot.height,
            pixels,
            glyphs_evicted: snapshot.glyphs_evicted,
        }
    }

    /// Convert a fern-tokens TextStyle to a text-typeset TextFormat.
    fn to_text_format(style: &TextStyle) -> TextFormat {
        TextFormat {
            font_family: Some(style.family.clone()),
            font_weight: Some(style.weight.0 as u32),
            font_bold: None,
            font_italic: None,
            font_size: Some(style.size),
            color: None,
        }
    }

    /// Invalidate the layout cache (e.g. on scale factor change).
    pub fn invalidate_cache(&mut self) {
        self.layout_cache.clear();
        self.glyph_cache.clear();
    }

    /// Borrow the underlying [`TextFontService`] immutably.
    ///
    /// Rich-text widgets read this to shape and lay out against
    /// the shared font registry without taking a mutable borrow.
    pub fn service(&self) -> &TextFontService {
        &self.service
    }

    /// Borrow the underlying [`TextFontService`] mutably.
    ///
    /// Rich-text widgets (`fern_text::RichTextEngine` driving the
    /// `RichTextEditor` widget) keep their own `DocumentFlow` and
    /// call `flow.layout_full(&bridge.service, ...)` +
    /// `flow.render(&mut bridge.service, ...)` through this
    /// accessor, so every widget's glyphs land in the same GPU
    /// atlas. Marks the atlas as dirty for the next
    /// [`atlas_info`](Self::atlas_info) call.
    ///
    /// Exposed behind `#[cfg(feature = "rich-text")]` so the
    /// default feature set keeps a minimal public surface.
    #[cfg(feature = "rich-text")]
    pub fn service_mut(&mut self) -> &mut TextFontService {
        self.had_text_activity = true;
        self.rich_text_atlas_dirty = true;
        &mut self.service
    }

    /// Current HiDPI display scale factor as last set by
    /// [`TextBackend::set_scale_factor`]. Reads through to the
    /// shared [`TextFontService`].
    #[cfg(feature = "rich-text")]
    pub fn display_scale_factor(&self) -> f32 {
        self.service.scale_factor()
    }

    /// Line height (in logical pixels) of the registry's default
    /// font + size — `ascent + descent + leading`. Useful for
    /// widgets that need to size against an intrinsic line height
    /// before any content has been laid out (e.g.
    /// `RichTextEditor::min_lines` / `max_lines`).
    ///
    /// Returns `0.0` if no default font is registered. Does not
    /// apply any per-block `line_height_multiplier`.
    pub fn default_line_height(&self) -> f32 {
        self.service.default_line_height()
    }

    /// Line height (in logical pixels) for a specific [`TextStyle`].
    /// Same calculation as [`default_line_height`](Self::default_line_height)
    /// but resolves the explicit family / weight / size first.
    ///
    /// Returns `0.0` when the style cannot be resolved.
    pub fn measure_line_height(&self, style: &TextStyle) -> f32 {
        self.service.measure_line_height(&Self::to_text_format(style))
    }
}

impl Default for TypesetterBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBackend for TypesetterBridge {
    fn set_scale_factor(&mut self, scale_factor: f32) {
        if (self.service.scale_factor() - scale_factor).abs() > 0.001 {
            // Push the new factor to the shared service. The
            // service clears its glyph cache and atlas in place
            // and bumps its `scale_generation`. Our own layout +
            // glyph caches still need to drop because they key
            // on the scale factor.
            self.service.set_scale_factor(scale_factor);
            self.layout_cache.clear();
            self.glyph_cache.clear();
        }
    }

    fn layout_single_line(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextLayout {
        let cache_key = LayoutCacheKey::new_single_line(text, style, max_width, self.service.scale_factor());

        if let Some(cached) = self.layout_cache.get(&cache_key) {
            return cached.clone();
        }

        self.had_text_activity = true;
        let format = Self::to_text_format(style);
        let result: SingleLineResult = self.label_flow.layout_single_line(
            &mut self.service,
            text,
            &format,
            max_width,
        );

        let key = self.next_layout_key;
        self.next_layout_key += 1;

        let layout = TextLayout {
            width: result.width,
            height: result.height,
            ascent: result.baseline,
            descent: result.height - result.baseline,
            underline_offset: result.underline_offset,
            underline_thickness: result.underline_thickness,
            layout_key: key,
            line_count: 1,
            spans: Vec::new(),
        };

        self.layout_cache.insert(cache_key.clone(), layout.clone());
        self.glyph_cache.insert(
            key,
            (
                result
                    .glyphs
                    .iter()
                    .map(|g| GlyphQuad {
                        screen: g.screen,
                        atlas: g.atlas,
                        color: g.color,
                        is_color: g.is_color,
                    })
                    .collect(),
                result.glyph_keys.clone(),
            ),
        );
        layout
    }

    fn layout_paragraph(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: f32,
        max_lines: Option<usize>,
    ) -> TextLayout {
        let cache_key =
            LayoutCacheKey::new_paragraph(text, style, max_width, max_lines, self.service.scale_factor());

        if let Some(cached) = self.layout_cache.get(&cache_key) {
            return cached.clone();
        }

        self.had_text_activity = true;
        let format = Self::to_text_format(style);
        let result: ParagraphResult = self.label_flow.layout_paragraph(
            &mut self.service,
            text,
            &format,
            max_width,
            max_lines,
        );

        let key = self.next_layout_key;
        self.next_layout_key += 1;

        let layout = TextLayout {
            width: result.width,
            height: result.height,
            ascent: result.baseline_first,
            descent: (result.height - result.baseline_first).max(0.0),
            underline_offset: result.underline_offset,
            underline_thickness: result.underline_thickness,
            layout_key: key,
            line_count: result.line_count.max(1),
            spans: Vec::new(),
        };

        self.layout_cache.insert(cache_key.clone(), layout.clone());
        self.glyph_cache.insert(
            key,
            (
                result
                    .glyphs
                    .iter()
                    .map(|g| GlyphQuad {
                        screen: g.screen,
                        atlas: g.atlas,
                        color: g.color,
                        is_color: g.is_color,
                    })
                    .collect(),
                result.glyph_keys.clone(),
            ),
        );
        layout
    }

    fn layout_single_line_markup(
        &mut self,
        source: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextLayout {
        let cache_key = LayoutCacheKey {
            text: source.to_string(),
            font_family: style.family.clone(),
            font_size_bits: (style.size * self.service.scale_factor()).to_bits(),
            font_weight: style.weight.0 as u32,
            max_width_bits: max_width.map(|w| (w * self.service.scale_factor()).to_bits()),
            mode: LayoutMode::SingleLineMarkup,
        };
        if let Some(cached) = self.layout_cache.get(&cache_key) {
            return cached.clone();
        }

        self.had_text_activity = true;
        let format = Self::to_text_format(style);
        let tt_markup = InlineMarkup::parse(source);
        let result: SingleLineResult = self.label_flow.layout_single_line_markup(
            &mut self.service,
            &tt_markup,
            &format,
            max_width,
        );

        let key = self.next_layout_key;
        self.next_layout_key += 1;

        let layout = TextLayout {
            width: result.width,
            height: result.height,
            ascent: result.baseline,
            descent: (result.height - result.baseline).max(0.0),
            underline_offset: result.underline_offset,
            underline_thickness: result.underline_thickness,
            layout_key: key,
            line_count: 1,
            spans: result
                .spans
                .iter()
                .map(|s| TextLayoutSpan {
                    kind: match &s.kind {
                        LaidOutSpanKind::Text => TextSpanKind::Text,
                        LaidOutSpanKind::Link { url } => TextSpanKind::Link { url: url.clone() },
                    },
                    line_index: s.line_index,
                    rect: s.rect,
                    byte_range: s.byte_range.clone(),
                })
                .collect(),
        };

        self.layout_cache.insert(cache_key, layout.clone());
        self.glyph_cache.insert(
            key,
            (
                result
                    .glyphs
                    .iter()
                    .map(|g| GlyphQuad {
                        screen: g.screen,
                        atlas: g.atlas,
                        color: g.color,
                        is_color: g.is_color,
                    })
                    .collect(),
                result.glyph_keys.clone(),
            ),
        );
        layout
    }

    fn layout_paragraph_markup(
        &mut self,
        source: &str,
        style: &TextStyle,
        max_width: f32,
        max_lines: Option<usize>,
    ) -> TextLayout {
        let cap = max_lines
            .map(|n| n.min(u32::MAX as usize) as u32)
            .unwrap_or(u32::MAX);
        let cache_key = LayoutCacheKey {
            text: source.to_string(),
            font_family: style.family.clone(),
            font_size_bits: (style.size * self.service.scale_factor()).to_bits(),
            font_weight: style.weight.0 as u32,
            max_width_bits: Some((max_width * self.service.scale_factor()).to_bits()),
            mode: LayoutMode::ParagraphMarkup(cap),
        };
        if let Some(cached) = self.layout_cache.get(&cache_key) {
            return cached.clone();
        }

        self.had_text_activity = true;
        let format = Self::to_text_format(style);
        let tt_markup = InlineMarkup::parse(source);
        let result: ParagraphResult = self.label_flow.layout_paragraph_markup(
            &mut self.service,
            &tt_markup,
            &format,
            max_width,
            max_lines,
        );

        let key = self.next_layout_key;
        self.next_layout_key += 1;

        let layout = TextLayout {
            width: result.width,
            height: result.height,
            ascent: result.baseline_first,
            descent: (result.height - result.baseline_first).max(0.0),
            underline_offset: result.underline_offset,
            underline_thickness: result.underline_thickness,
            layout_key: key,
            line_count: result.line_count.max(1),
            spans: result
                .spans
                .iter()
                .map(|s| TextLayoutSpan {
                    kind: match &s.kind {
                        LaidOutSpanKind::Text => TextSpanKind::Text,
                        LaidOutSpanKind::Link { url } => TextSpanKind::Link { url: url.clone() },
                    },
                    line_index: s.line_index,
                    rect: s.rect,
                    byte_range: s.byte_range.clone(),
                })
                .collect(),
        };

        self.layout_cache.insert(cache_key, layout.clone());
        self.glyph_cache.insert(
            key,
            (
                result
                    .glyphs
                    .iter()
                    .map(|g| GlyphQuad {
                        screen: g.screen,
                        atlas: g.atlas,
                        color: g.color,
                        is_color: g.is_color,
                    })
                    .collect(),
                result.glyph_keys.clone(),
            ),
        );
        layout
    }

    fn ensure_glyphs(&mut self, layout: &TextLayout) -> Vec<GlyphQuad> {
        if let Some((quads, keys)) = self.glyph_cache.get(&layout.layout_key) {
            // Touch the glyphs in text-typeset's internal cache so they
            // aren't evicted while still being rendered via paint-cache
            // hits (where layout_single_line is not called and the
            // internal glyph_cache.get() that refreshes last_used is
            // never reached).
            //
            // Note: do NOT set `had_text_activity` here — touch_glyphs
            // directly updates the glyph timestamps, which is sufficient
            // to protect them when eviction does run (triggered by real
            // text work on other frames). Setting the flag here would
            // advance the atlas generation every frame, defeating the
            // idle-protection mechanism.
            self.service.touch_glyphs(keys);
            quads.clone()
        } else {
            Vec::new()
        }
    }

    fn touch_layout(&mut self, layout_key: u64) {
        // Fast path used by the widget-tree renderer when a widget's
        // `cached_paint` is reused. The cached frame references atlas
        // positions by baked-in UVs, so if the underlying glyphs age
        // out and their slots get reused by newer glyphs, the cached
        // quads render the wrong pixels. Refreshing timestamps here
        // mirrors what `ensure_glyphs` does on a cache hit, but without
        // reconstructing or cloning the quad list.
        if let Some((_, keys)) = self.glyph_cache.get(&layout_key) {
            self.service.touch_glyphs(keys);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_single_line_returns_nonzero_size() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let layout = bridge.layout_single_line("Hello", &TextStyle::default(), None);
        assert!(layout.width > 0.0, "width was {}", layout.width);
        assert!(layout.height > 0.0, "height was {}", layout.height);
    }

    #[test]
    fn ensure_glyphs_returns_quads() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let layout = bridge.layout_single_line("Hi", &TextStyle::default(), None);
        let glyphs = bridge.ensure_glyphs(&layout);
        assert!(
            glyphs.len() >= 2,
            "expected >= 2 glyphs, got {}",
            glyphs.len()
        );
    }

    #[test]
    fn empty_text_returns_zero_width() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let layout = bridge.layout_single_line("", &TextStyle::default(), None);
        assert_eq!(layout.width, 0.0);
    }

    #[test]
    fn longer_text_is_wider() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let short = bridge.layout_single_line("Hi", &TextStyle::default(), None);
        let long = bridge.layout_single_line("Hello World", &TextStyle::default(), None);
        assert!(long.width > short.width);
    }

    #[test]
    fn ascent_and_descent_are_positive() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let layout = bridge.layout_single_line("Test", &TextStyle::default(), None);
        assert!(layout.ascent > 0.0);
        assert!(layout.descent > 0.0);
        assert!((layout.ascent + layout.descent - layout.height).abs() < 0.01);
    }

    /// Arabic text renders with visible glyphs — regression test for
    /// the default-font gap that the font-coverage plan fixes. Before
    /// the fix, `register_default_font` loaded only Inter, which has
    /// no Arabic glyph coverage, so every shaped codepoint produced a
    /// `.notdef` with a zero-size atlas rect. After the fix,
    /// `fonts-arabic` (default) registers Noto Sans Arabic as a
    /// fallback font, and text-typeset's codepoint-based fallback
    /// loop picks it up automatically.
    ///
    /// The test shapes an Arabic greeting and asserts (a) the total
    /// advance is positive and (b) at least one glyph in the layout
    /// rasterizes to a non-zero atlas rect, proving a real glyph was
    /// found (not an invisible `.notdef`).
    #[cfg(feature = "fonts-arabic")]
    #[test]
    fn arabic_text_renders_with_visible_glyphs() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let layout =
            bridge.layout_single_line("مرحبا", &TextStyle::default(), None);
        assert!(
            layout.width > 0.0,
            "Arabic text should produce a non-zero advance, got {}",
            layout.width
        );
        let glyphs = bridge.ensure_glyphs(&layout);
        assert!(
            !glyphs.is_empty(),
            "Arabic text should produce at least one glyph"
        );
        let visible = glyphs
            .iter()
            .any(|g| g.atlas[2] > 0.0 && g.atlas[3] > 0.0);
        assert!(
            visible,
            "no Arabic glyph rasterized to a visible atlas rect — \
             is `fonts-arabic` enabled and the Noto Sans Arabic font \
             registered via `register_default_font`?"
        );
    }

    /// Regression test for a text-typeset bidi bug: Latin text
    /// embedded in an Arabic string must not be visually reversed.
    #[cfg(feature = "fonts-arabic")]
    #[test]
    fn latin_in_arabic_is_not_visually_reversed() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let style = TextStyle::default();

        let pure = bridge.layout_single_line("Alice", &style, None);
        let pure_glyphs = bridge.ensure_glyphs(&pure);
        assert_eq!(
            pure_glyphs.len(),
            5,
            "expected 5 glyphs for 'Alice', got {}",
            pure_glyphs.len()
        );
        let pure_widths: Vec<f32> = pure_glyphs.iter().map(|g| g.screen[2]).collect();

        let mixed = bridge.layout_single_line("مرحبا Alice", &style, None);
        let mixed_glyphs = bridge.ensure_glyphs(&mixed);
        assert!(
            mixed_glyphs.len() > pure_glyphs.len(),
            "mixed layout should contain at least the Latin glyphs plus Arabic"
        );

        for (i, (pw, mg)) in pure_widths.iter().zip(mixed_glyphs.iter()).take(5).enumerate() {
            let mw = mg.screen[2];
            assert!(
                (pw - mw).abs() < 0.5,
                "Latin glyph {} width mismatch: pure={:.2}, mixed={:.2} \
                 (Latin cluster in RTL paragraph is reversed — text-typeset bidi bug)",
                i,
                pw,
                mw,
            );
        }
    }

    #[cfg(feature = "fonts-hebrew")]
    #[test]
    fn hebrew_text_renders_with_visible_glyphs() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let layout =
            bridge.layout_single_line("שלום", &TextStyle::default(), None);
        assert!(layout.width > 0.0);
        let glyphs = bridge.ensure_glyphs(&layout);
        let visible = glyphs
            .iter()
            .any(|g| g.atlas[2] > 0.0 && g.atlas[3] > 0.0);
        assert!(
            visible,
            "no Hebrew glyph rasterized to a visible atlas rect — \
             is `fonts-hebrew` enabled and the Noto Sans Hebrew font \
             registered via `register_default_font`?"
        );
    }

    #[test]
    fn max_width_truncates() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let unconstrained = bridge.layout_single_line(
            "A very long text that should be truncated",
            &TextStyle::default(),
            None,
        );
        let constrained = bridge.layout_single_line(
            "A very long text that should be truncated",
            &TextStyle::default(),
            Some(50.0),
        );
        assert!(constrained.width <= 51.0);
        assert!(constrained.width < unconstrained.width);
    }

    #[test]
    fn cache_hit_returns_same_result() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let first = bridge.layout_single_line("Hello", &TextStyle::default(), None);
        let second = bridge.layout_single_line("Hello", &TextStyle::default(), None);
        assert!((first.width - second.width).abs() < 0.001);
        assert!((first.height - second.height).abs() < 0.001);
    }

    #[test]
    fn different_max_width_is_separate_cache_entry() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let wide = bridge.layout_single_line("Hello World", &TextStyle::default(), None);
        let narrow = bridge.layout_single_line("Hello World", &TextStyle::default(), Some(30.0));
        assert!(narrow.width < wide.width || narrow.width <= 31.0);
    }

    /// Reproduces the stale-glyph bug: after a layout pass (many
    /// layout_single_line calls without ensure_glyphs), the first
    /// paint call's ensure_glyphs should return glyphs for the
    /// correct text, not for whatever was last measured.
    #[test]
    fn ensure_glyphs_after_layout_pass_returns_correct_text() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let style = TextStyle::default();

        let _l1 = bridge.layout_single_line("First text", &style, None);
        let _l2 = bridge.layout_single_line("Second text", &style, None);
        let _l3 = bridge.layout_single_line("Third text is the last measured", &style, None);

        let layout = bridge.layout_single_line("First text", &style, None);
        let glyphs = bridge.ensure_glyphs(&layout);

        assert!(!glyphs.is_empty(), "should produce glyphs for 'First text'");
        assert!(
            glyphs.len() <= 15,
            "got {} glyphs — expected ~10 for 'First text', not ~31 for the stale last_result",
            glyphs.len()
        );
    }
}
