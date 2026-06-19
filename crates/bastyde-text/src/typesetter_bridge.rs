// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::collections::HashMap;

use bastyde_canvas::GlyphQuad;
use bastyde_canvas::text_backend::{
    AtlasInfo, GlyphValidation, TextBackend, TextLayout, TextLayoutSpan, TextSpanKind,
};
use bastyde_tokens::TextStyle;
use text_typeset::atlas::cache::GlyphCacheKey;
use text_typeset::{
    DocumentFlow, FontFaceId, InlineMarkup, LaidOutSpanKind, ParagraphResult, SingleLineResult,
    TextFontService, TextFormat,
};

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
    /// Ambient raster scale at layout time (f32 bits). Metrics are
    /// identical across raster scales, but each entry's `layout_key`
    /// indexes glyph quads baked at a specific bitmap density — entries
    /// at different scales must not alias.
    raster_scale_bits: u32,
}

impl LayoutCacheKey {
    fn new_single_line(
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        scale_factor: f32,
        raster_scale: f32,
    ) -> Self {
        let scaled_size = style.size * scale_factor;
        Self {
            text: text.to_string(),
            font_family: style.family.clone(),
            font_size_bits: scaled_size.to_bits(),
            font_weight: style.weight.0 as u32,
            max_width_bits: max_width.map(|w| (w * scale_factor).to_bits()),
            mode: LayoutMode::SingleLine,
            raster_scale_bits: raster_scale.to_bits(),
        }
    }

    fn new_paragraph(
        text: &str,
        style: &TextStyle,
        max_width: f32,
        max_lines: Option<usize>,
        scale_factor: f32,
        raster_scale: f32,
    ) -> Self {
        let scaled_size = style.size * scale_factor;
        let cap = max_lines
            .map(|n| n.min(u32::MAX as usize) as u32)
            .unwrap_or(u32::MAX);
        Self {
            text: text.to_string(),
            font_family: style.family.clone(),
            font_size_bits: scaled_size.to_bits(),
            font_weight: style.weight.0 as u32,
            max_width_bits: Some((max_width * scale_factor).to_bits()),
            mode: LayoutMode::Paragraph(cap),
            raster_scale_bits: raster_scale.to_bits(),
        }
    }
}

// Only cache the metrics — glyphs are re-generated on demand during paint.

/// Bridge between bastyde-canvas's `TextBackend` trait and text-typeset.
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
    /// `RenderFrame`. bastyde-app consumes the atlas through
    /// `atlas_info()` *after* `tree.render()`, so by then the
    /// typesetter's own dirty flag is already false and the atlas
    /// would never be uploaded, leaving rich-text glyphs invisible
    /// on the GPU. This flag closes the gap: anyone who took a
    /// mutable borrow of the service since the last `atlas_info()`
    /// call forces the bridge to report the atlas as dirty one
    /// more time.
    rich_text_atlas_dirty: bool,
    /// `TextFontService::eviction_epoch()` as of the previous
    /// `atlas_info()` call. Comparing against the live epoch reports
    /// evictions from EVERY internal path — the snapshot-driven scan,
    /// the scan at the start of every rich-text `render()`
    /// (`build_render_frame`), and the wholesale reset on scale-factor
    /// change — not just the snapshot's own `glyphs_evicted` flag.
    last_seen_eviction_epoch: u64,
    /// Monotonic atlas content version, bumped whenever `atlas_info()`
    /// observes changed pixels. Each window renderer records the version
    /// it last uploaded; see [`AtlasInfo::version`].
    atlas_version: u64,
    /// Ambient raster scale set by the paint walker for the widget
    /// currently painting (see [`TextBackend::set_raster_scale`]).
    /// Flows into every `label_flow.layout_*` call and into the layout
    /// cache key — flipping it costs nothing, entries at different
    /// scales coexist and age out via the normal touch/LRU machinery.
    raster_scale: f32,
    /// Debug-only `layout_key -> source text` side map for diagnostics.
    ///
    /// Unlike `layout_cache` / `glyph_cache`, this is **not** cleared on a
    /// scale-factor change — the scale reset is the dominant trigger of
    /// the evicted-layout warning (a widget retains a `TextLayout` from
    /// before the reset, then paints it after both caches were wiped), so
    /// recovering the text for the warning requires a mapping that
    /// outlives the wipe. Bounded by [`DEBUG_TEXT_MAP_CAP`]; never read on
    /// a hot path (only [`debug_layout_text`](TextBackend::debug_layout_text)).
    #[cfg(debug_assertions)]
    debug_text_by_key: HashMap<u64, String>,
}

/// Cap on the debug `layout_key -> text` map. When exceeded the map is
/// cleared wholesale (oldest text is then simply unavailable to the
/// warning — it degrades to "no text", never to wrong text). Generous
/// enough that a normal session never trips it.
#[cfg(debug_assertions)]
const DEBUG_TEXT_MAP_CAP: usize = 8192;

impl TypesetterBridge {
    pub fn new() -> Self {
        let service = TextFontService::new();
        let last_seen_eviction_epoch = service.eviction_epoch();
        Self {
            service,
            label_flow: DocumentFlow::new(),
            default_font: None,
            next_layout_key: 1,
            layout_cache: HashMap::new(),
            glyph_cache: HashMap::new(),
            had_text_activity: false,
            rich_text_atlas_dirty: false,
            last_seen_eviction_epoch,
            atlas_version: 1,
            raster_scale: 1.0,
            #[cfg(debug_assertions)]
            debug_text_by_key: HashMap::new(),
        }
    }

    /// Record a `layout_key -> text` association for the debug
    /// evicted-layout warning. No-op in release. Bounded by
    /// [`DEBUG_TEXT_MAP_CAP`].
    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    #[inline]
    fn debug_remember_text(&mut self, key: u64, text: &str) {
        #[cfg(debug_assertions)]
        {
            if self.debug_text_by_key.len() >= DEBUG_TEXT_MAP_CAP {
                self.debug_text_by_key.clear();
            }
            self.debug_text_by_key.insert(key, text.to_string());
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
            let data = include_bytes!("../fonts/NotoSansArabic-VariableFont_wdth,wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-hebrew")]
        {
            let data = include_bytes!("../fonts/NotoSansHebrew-VariableFont_wdth,wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-thai")]
        {
            let data = include_bytes!("../fonts/NotoSansThai-VariableFont_wdth,wght.ttf");
            let _ = self.service.register_font(data);
        }
        #[cfg(feature = "fonts-devanagari")]
        {
            let data = include_bytes!("../fonts/NotoSansDevanagari-VariableFont_wdth,wght.ttf");
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
        // `register_font_shared` keeps the mmap handle alive inside
        // the font registry without copying the bytes to the heap —
        // the kernel pages glyph tables in on demand.
        crate::system_emoji::load_system_emoji_data()
            .map(|data| self.service.register_font_shared(data))
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
    ///
    /// `seen_version` is the [`AtlasInfo::version`] the caller last
    /// uploaded (0 for a fresh consumer). Pixels are cloned into the
    /// result only when the caller is behind, so several windows can
    /// each pull the current atlas at their own redraw without the
    /// first caller consuming it for everyone — and a clean-atlas
    /// frame still skips the ~1 MB memcpy (profiling the animations
    /// example showed an unconditional clone dominating self-time at
    /// ~6 % during shader-driven animations).
    ///
    /// `glyphs_evicted` is derived from
    /// [`TextFontService::eviction_epoch`], so it reports evictions
    /// from every path — the snapshot scan below, the scan inside
    /// every rich-text `render()`, and scale-factor resets — not just
    /// the snapshot's own flag. On `true`, the caller must invalidate
    /// every retained paint cache in every window.
    pub fn atlas_info(&mut self, seen_version: u64) -> AtlasInfo {
        let (snapshot_dirty, width, height) = {
            let snapshot = self.service.atlas_snapshot(self.had_text_activity);
            (snapshot.dirty, snapshot.width, snapshot.height)
        };
        let rich_dirty = self.rich_text_atlas_dirty;
        self.rich_text_atlas_dirty = false;
        self.had_text_activity = false;
        let dirty = snapshot_dirty || rich_dirty;
        if dirty {
            self.atlas_version = self.atlas_version.wrapping_add(1);
        }
        let pixels = if seen_version != self.atlas_version {
            self.service.atlas_pixels().to_vec()
        } else {
            Vec::new()
        };

        let epoch = self.service.eviction_epoch();
        let glyphs_evicted = epoch != self.last_seen_eviction_epoch;
        self.last_seen_eviction_epoch = epoch;

        AtlasInfo {
            dirty,
            width,
            height,
            pixels,
            version: self.atlas_version,
            glyphs_evicted,
        }
    }

    /// Current atlas content version (see [`AtlasInfo::version`]),
    /// without consuming any pending dirty/eviction state. Used when a
    /// consumer primes itself outside the per-frame `atlas_info` flow —
    /// e.g. a freshly created window uploading the current atlas pixels
    /// directly — and needs the matching version stamp.
    pub fn atlas_version(&self) -> u64 {
        self.atlas_version
    }

    /// Convert a bastyde-tokens TextStyle to a text-typeset TextFormat.
    fn to_text_format(style: &TextStyle) -> TextFormat {
        TextFormat {
            font_family: Some(style.family.clone()),
            font_weight: Some(style.weight.0 as u32),
            font_bold: None,
            font_italic: None,
            font_size: Some(style.size),
            color: None,
            ..Default::default()
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
    /// Rich-text widgets (`bastyde_text::RichTextEngine` driving the
    /// `RichTextEditor` widget) keep their own `DocumentFlow` and
    /// call `flow.layout_full(&bridge.service, ...)` +
    /// `flow.render(&mut bridge.service, ...)` through this
    /// accessor, so every widget's glyphs land in the same GPU
    /// atlas. Marks the atlas as dirty for the next
    /// [`atlas_info`](Self::atlas_info) call.
    pub fn service_mut(&mut self) -> &mut TextFontService {
        self.had_text_activity = true;
        self.rich_text_atlas_dirty = true;
        &mut self.service
    }

    /// Current HiDPI display scale factor as last set by
    /// [`TextBackend::set_scale_factor`]. Reads through to the
    /// shared [`TextFontService`].
    pub fn display_scale_factor(&self) -> f32 {
        self.service.scale_factor()
    }

    /// Ambient raster scale as last set by
    /// [`TextBackend::set_raster_scale`] (the paint walker's
    /// accumulated transform scale for the widget currently
    /// painting). Rich-text widgets sync this into their own
    /// `DocumentFlow` (`flow.set_raster_scale`) before `render`, so
    /// document glyphs densify under a scene zoom exactly like the
    /// label path.
    pub fn ambient_raster_scale(&self) -> f32 {
        self.raster_scale
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
        self.service
            .measure_line_height(&Self::to_text_format(style))
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

    fn set_raster_scale(&mut self, raster_scale: f32) {
        // No cache clearing: the layout cache keys on the raster scale,
        // so entries at different scales coexist. Stale-scale glyph
        // entries age out of text-typeset's atlas via the normal LRU.
        self.raster_scale = if raster_scale > 0.0 {
            raster_scale
        } else {
            1.0
        };
    }

    fn raster_scale(&self) -> f32 {
        self.raster_scale
    }

    fn layout_single_line(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextLayout {
        let cache_key = LayoutCacheKey::new_single_line(
            text,
            style,
            max_width,
            self.service.scale_factor(),
            self.raster_scale,
        );

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
            self.raster_scale,
        );

        let key = self.next_layout_key;
        self.next_layout_key += 1;
        self.debug_remember_text(key, text);

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
            raster_scale: self.raster_scale,
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
        let cache_key = LayoutCacheKey::new_paragraph(
            text,
            style,
            max_width,
            max_lines,
            self.service.scale_factor(),
            self.raster_scale,
        );

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
            self.raster_scale,
        );

        let key = self.next_layout_key;
        self.next_layout_key += 1;
        self.debug_remember_text(key, text);

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
            raster_scale: self.raster_scale,
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
            raster_scale_bits: self.raster_scale.to_bits(),
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
            self.raster_scale,
        );

        let key = self.next_layout_key;
        self.next_layout_key += 1;
        self.debug_remember_text(key, source);

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
            raster_scale: self.raster_scale,
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
            raster_scale_bits: self.raster_scale.to_bits(),
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
            self.raster_scale,
        );

        let key = self.next_layout_key;
        self.next_layout_key += 1;
        self.debug_remember_text(key, source);

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
            raster_scale: self.raster_scale,
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

    fn debug_layout_text(&self, layout_key: u64) -> Option<String> {
        #[cfg(debug_assertions)]
        {
            return self.debug_text_by_key.get(&layout_key).cloned();
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = layout_key;
            None
        }
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

    fn glyph_epoch(&self) -> u64 {
        self.service.eviction_epoch()
    }

    fn debug_validate_layout(&self, layout_key: u64) -> GlyphValidation {
        let Some((quads, keys)) = self.glyph_cache.get(&layout_key) else {
            // The bridge's caches were cleared (eviction recovery or
            // scale-factor change) after these quads were baked. A
            // retained paint replaying them outlived an invalidation
            // that should have reached it.
            return GlyphValidation::StaleKey;
        };
        // `quads` and `keys` are parallel arrays: every layout_* path
        // stores `result.glyphs` mapped 1:1 alongside
        // `result.glyph_keys`, both produced by the same shaping pass.
        debug_assert_eq!(quads.len(), keys.len());
        for (quad, key) in quads.iter().zip(keys.iter()) {
            // Zero-size quads (whitespace / .notdef placeholders) have
            // no atlas residency requirement.
            if quad.atlas[2] <= 0.0 || quad.atlas[3] <= 0.0 {
                continue;
            }
            let Some(rect) = self.service.peek_glyph_rect(key) else {
                return GlyphValidation::RectMismatch;
            };
            let baked = [
                quad.atlas[0] as u32,
                quad.atlas[1] as u32,
                quad.atlas[2] as u32,
                quad.atlas[3] as u32,
            ];
            if rect != baked {
                return GlyphValidation::RectMismatch;
            }
        }
        GlyphValidation::Valid
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
    fn set_raster_scale_produces_separate_cache_entries_and_layout_keys() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let style = TextStyle::default();

        let at_one = bridge.layout_single_line("Hello", &style, None);
        bridge.set_raster_scale(2.0);
        let at_two = bridge.layout_single_line("Hello", &style, None);

        // Metrics are raster-scale-independent…
        assert!((at_one.width - at_two.width).abs() < 1e-3);
        assert!((at_one.height - at_two.height).abs() < 1e-3);
        // …but each scale owns its own cache entry / glyph set.
        assert_ne!(at_one.layout_key, at_two.layout_key);
        assert_eq!(at_one.raster_scale, 1.0);
        assert_eq!(at_two.raster_scale, 2.0);

        let q1 = bridge.ensure_glyphs(&at_one);
        let q2 = bridge.ensure_glyphs(&at_two);
        assert_eq!(q1.len(), q2.len());
        assert!(!q1.is_empty());
        // The 2x entry samples a denser bitmap: atlas rects grow, screen
        // rects stay logical.
        let (a1, a2) = (&q1[0], &q2[0]);
        assert!(
            a2.atlas[2] > a1.atlas[2] * 1.5,
            "atlas w should roughly double: {} -> {}",
            a1.atlas[2],
            a2.atlas[2]
        );
        assert!((a1.screen[2] - a2.screen[2]).abs() <= 1.01);
    }

    #[test]
    fn raster_scale_back_to_one_hits_original_cache() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let style = TextStyle::default();

        let first = bridge.layout_single_line("Hello", &style, None);
        bridge.set_raster_scale(2.0);
        let scaled = bridge.layout_single_line("Hello", &style, None);
        bridge.set_raster_scale(1.0);
        let back = bridge.layout_single_line("Hello", &style, None);

        // Returning to 1.0 must hit the original entry (no cache churn —
        // set_raster_scale clears nothing).
        assert_eq!(first.layout_key, back.layout_key);
        assert_ne!(first.layout_key, scaled.layout_key);
    }

    /// Minimal BlockLayoutParams for driving a rich-text `DocumentFlow`
    /// through the bridge's shared service, the way `RichTextEngine` does.
    fn make_test_block(id: usize, text: &str) -> text_typeset::layout::block::BlockLayoutParams {
        use text_typeset::layout::block::{BlockLayoutParams, FragmentParams};
        BlockLayoutParams {
            block_id: id,
            position: 0,
            text: text.to_string(),
            fragments: vec![FragmentParams {
                text: text.to_string(),
                offset: 0,
                length: text.len(),
                font_family: None,
                font_weight: None,
                font_bold: None,
                font_italic: None,
                font_point_size: None,
                underline_style: text_typeset::UnderlineStyle::None,
                overline: false,
                strikeout: false,
                is_link: false,
                letter_spacing: 0.0,
                word_spacing: 0.0,
                foreground_color: None,
                underline_color: None,
                background_color: None,
                anchor_href: None,
                tooltip: None,
                vertical_alignment: text_typeset::VerticalAlignment::Normal,
                image_name: None,
                image_width: 0.0,
                image_height: 0.0,
                features: Vec::new(),
            }],
            alignment: text_typeset::layout::paragraph::Alignment::Left,
            top_margin: 0.0,
            bottom_margin: 0.0,
            left_margin: 0.0,
            right_margin: 0.0,
            text_indent: 0.0,
            list_marker: String::new(),
            list_indent: 0.0,
            tab_positions: vec![],
            line_height_multiplier: None,
            non_breakable_lines: false,
            hyphenation: None,
            checkbox: None,
            background_color: None,
        }
    }

    /// Regression test for the corruption root cause: evictions that
    /// happen inside `build_render_frame` (every rich-text `render()` —
    /// i.e. every keystroke in any text field) used to bump only the
    /// service's eviction epoch; `atlas_info` passed through just the
    /// snapshot's own `glyphs_evicted` flag, so the app-level recovery
    /// (invalidate every retained paint) never fired and stale glyph
    /// UVs stayed on screen until a forced repaint.
    #[test]
    fn atlas_info_reports_render_path_evictions() {
        let mut bridge = TypesetterBridge::new_with_default_font();

        // A label whose glyphs live in the shared atlas…
        let layout = bridge.layout_single_line("Hello", &TextStyle::default(), None);
        let _ = bridge.ensure_glyphs(&layout);
        // …with the baseline atlas state consumed.
        let baseline = bridge.atlas_info(0);
        assert!(
            !baseline.glyphs_evicted,
            "nothing can have been evicted yet"
        );

        // Drive a rich-text DocumentFlow through the shared service,
        // exactly like RichTextEngine, far past the LRU idle window
        // (120 generations) + scan cadence (60). The label's glyphs are
        // never touched, so build_render_frame evicts them mid-loop.
        let mut flow = DocumentFlow::new();
        flow.set_viewport(800.0, 600.0);
        flow.layout_blocks(bridge.service_mut(), vec![make_test_block(1, "zzzz")]);
        for _ in 0..250 {
            let _ = flow.render(bridge.service_mut());
        }

        let info = bridge.atlas_info(baseline.version);
        assert!(
            info.glyphs_evicted,
            "evictions performed inside build_render_frame (rich-text render path) \
             must surface through AtlasInfo::glyphs_evicted"
        );
    }

    #[test]
    fn atlas_info_version_semantics() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let layout = bridge.layout_single_line("Hi", &TextStyle::default(), None);
        let _ = bridge.ensure_glyphs(&layout);

        // First consumer, never uploaded: gets pixels + a version stamp.
        let first = bridge.atlas_info(0);
        assert!(first.version > 0);
        assert!(
            !first.pixels.is_empty(),
            "a consumer at version 0 must receive the atlas pixels"
        );

        // Same consumer, up to date, no new text work: no pixels.
        let second = bridge.atlas_info(first.version);
        assert_eq!(second.version, first.version, "clean frame must not bump");
        assert!(
            second.pixels.is_empty(),
            "an up-to-date consumer must not pay the pixel memcpy"
        );

        // A second window that never uploaded still gets pixels — the
        // first consumer must not have consumed them for everyone
        // (the old consume-once dirty-flag bug).
        let lagging = bridge.atlas_info(0);
        assert_eq!(lagging.version, first.version);
        assert!(
            !lagging.pixels.is_empty(),
            "a lagging consumer must receive pixels even after another \
             consumer already pulled this version"
        );

        // New rasterization dirties the atlas: version bumps, pixels flow.
        let layout2 = bridge.layout_single_line("WXYZ", &TextStyle::default(), None);
        let _ = bridge.ensure_glyphs(&layout2);
        let third = bridge.atlas_info(first.version);
        assert!(third.version > first.version);
        assert!(!third.pixels.is_empty());
    }

    #[test]
    fn debug_validate_layout_catches_stale_rect() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let layout = bridge.layout_single_line("Hi", &TextStyle::default(), None);

        // Untouched layout: every glyph resident at its baked rect.
        assert_eq!(
            bridge.debug_validate_layout(layout.layout_key),
            GlyphValidation::Valid
        );

        // Move one resident glyph's atlas rect underneath the baked
        // quads (what slot reuse after eviction does).
        let key = {
            let (quads, keys) = bridge
                .glyph_cache
                .get(&layout.layout_key)
                .expect("layout entry exists");
            quads
                .iter()
                .zip(keys.iter())
                .find(|(q, _)| q.atlas[2] > 0.0 && q.atlas[3] > 0.0)
                .map(|(_, k)| *k)
                .expect("at least one glyph with atlas residency")
        };
        assert!(
            bridge
                .service_mut()
                .debug_set_glyph_rect(&key, [499, 499, 1, 1])
        );
        assert_eq!(
            bridge.debug_validate_layout(layout.layout_key),
            GlyphValidation::RectMismatch,
            "a moved atlas rect under baked quads is definite corruption"
        );

        // After a wholesale cache clear, the key is unknown — the
        // retained-frame-outlived-a-clear signature.
        bridge.invalidate_cache();
        assert_eq!(
            bridge.debug_validate_layout(layout.layout_key),
            GlyphValidation::StaleKey
        );
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
    /// the default-font gap that a future font-coverage fix addresses. Before
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
        let layout = bridge.layout_single_line("مرحبا", &TextStyle::default(), None);
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
        let visible = glyphs.iter().any(|g| g.atlas[2] > 0.0 && g.atlas[3] > 0.0);
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

        for (i, (pw, mg)) in pure_widths
            .iter()
            .zip(mixed_glyphs.iter())
            .take(5)
            .enumerate()
        {
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
        let layout = bridge.layout_single_line("שלום", &TextStyle::default(), None);
        assert!(layout.width > 0.0);
        let glyphs = bridge.ensure_glyphs(&layout);
        let visible = glyphs.iter().any(|g| g.atlas[2] > 0.0 && g.atlas[3] > 0.0);
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
