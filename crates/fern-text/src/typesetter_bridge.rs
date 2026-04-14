use std::collections::HashMap;

use fern_canvas::GlyphQuad;
use fern_canvas::text_backend::{AtlasInfo, TextBackend, TextLayout};
use fern_tokens::TextStyle;
use text_typeset::{FontFaceId, ParagraphResult, SingleLineResult, TextFormat, Typesetter};

/// Which layout method produced the cache entry — separates the
/// single-line and paragraph caches so a single-line truncated result
/// never masquerades as a wrapped paragraph result (or vice versa).
#[derive(Clone, PartialEq, Eq, Hash)]
enum LayoutMode {
    SingleLine,
    /// `max_lines` cap expressed as `u32::MAX` when unbounded.
    Paragraph(u32),
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

/// Bridge between fern-canvas's TextBackend trait and text-typeset's Typesetter.
pub struct TypesetterBridge {
    typesetter: Typesetter,
    default_font: Option<FontFaceId>,
    next_layout_key: u64,
    /// Display scale factor for HiDPI rasterization.
    scale_factor: f32,
    /// Layout metrics cache: avoids re-shaping text just for size measurement.
    layout_cache: HashMap<LayoutCacheKey, TextLayout>,
    /// Glyph quads stored by opaque layout key so ensure_glyphs can be
    /// resolved independently for many text widgets in the same frame.
    glyph_cache: HashMap<u64, Vec<GlyphQuad>>,
    /// Whether any text work (layout_single_line/ensure_glyphs) happened
    /// since the last atlas_info() call. When false, we skip advancing
    /// the eviction generation to avoid aging out idle-but-visible glyphs.
    had_text_activity: bool,
    /// Sticky atlas-dirty flag set by rich-text widgets via
    /// `typesetter_mut()`. Necessary because text-typeset's `render()`
    /// path clears `typesetter.atlas.dirty` after copying pixels into
    /// the returned `RenderFrame`. fern-app consumes the atlas through
    /// `atlas_info()` *after* `tree.render()`, so by then the
    /// typesetter's own dirty flag is already false and the atlas
    /// would never be uploaded, leaving rich-text glyphs invisible on
    /// the GPU. This flag closes the gap: anyone who touched the
    /// typesetter mutably since the last `atlas_info()` call forces
    /// the bridge to report the atlas as dirty one more time.
    rich_text_atlas_dirty: bool,
}

impl TypesetterBridge {
    pub fn new() -> Self {
        Self {
            typesetter: Typesetter::new(),
            default_font: None,
            next_layout_key: 1,
            scale_factor: 1.0,
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
    /// same font registry so that `text-typeset`'s shaper-level
    /// `.notdef` fallback loop can cover mixed-script text without
    /// any locale awareness at the caller site.
    pub fn new_with_default_font() -> Self {
        let mut bridge = Self::new();
        bridge.register_default_font();
        bridge
    }

    /// Register a font from raw TTF/OTF data.
    pub fn register_font(&mut self, data: &[u8]) -> FontFaceId {
        self.typesetter.register_font(data)
    }

    /// Register Inter as the primary default, then register every
    /// feature-gated script-specific fallback font. The feature-gated
    /// registrations discard their `FontFaceId` because fallback
    /// eligibility only requires that a font be in the registry —
    /// `text-typeset`'s `find_fallback_font` iterates every registered
    /// font and picks the first one whose charmap covers a `.notdef`
    /// glyph's codepoint.
    fn register_default_font(&mut self) {
        // Primary default: InterVariable covers Latin, Cyrillic, Greek,
        // and Vietnamese. Used as the default font in
        // `TypographyTokens`.
        let inter_data = include_bytes!("../fonts/InterVariable.ttf");
        let face_id = self.typesetter.register_font(inter_data);
        self.typesetter.set_default_font(face_id, 14.0);
        self.default_font = Some(face_id);

        // Script-specific fallback fonts. Each bundle is feature-gated
        // so a Latin-only app can opt out via `default-features = false`.
        // The order of registration below is also the order in which
        // `find_fallback_font` consults fonts when resolving a `.notdef`,
        // so scripts that commonly co-occur with Latin (Arabic, Hebrew)
        // go first for minor cache locality.
        #[cfg(feature = "fonts-arabic")]
        {
            let data =
                include_bytes!("../fonts/NotoSansArabic-VariableFont_wdth,wght.ttf");
            let _ = self.typesetter.register_font(data);
        }
        #[cfg(feature = "fonts-hebrew")]
        {
            let data =
                include_bytes!("../fonts/NotoSansHebrew-VariableFont_wdth,wght.ttf");
            let _ = self.typesetter.register_font(data);
        }
        #[cfg(feature = "fonts-thai")]
        {
            let data =
                include_bytes!("../fonts/NotoSansThai-VariableFont_wdth,wght.ttf");
            let _ = self.typesetter.register_font(data);
        }
        #[cfg(feature = "fonts-devanagari")]
        {
            let data =
                include_bytes!("../fonts/NotoSansDevanagari-VariableFont_wdth,wght.ttf");
            let _ = self.typesetter.register_font(data);
        }
        #[cfg(feature = "fonts-cjk-sc")]
        {
            let data = include_bytes!("../fonts/NotoSansSC-VariableFont_wght.ttf");
            let _ = self.typesetter.register_font(data);
        }
        #[cfg(feature = "fonts-cjk-jp")]
        {
            let data = include_bytes!("../fonts/NotoSansJP-VariableFont_wght.ttf");
            let _ = self.typesetter.register_font(data);
        }
        #[cfg(feature = "fonts-cjk-kr")]
        {
            let data = include_bytes!("../fonts/NotoSansKR-VariableFont_wght.ttf");
            let _ = self.typesetter.register_font(data);
        }
    }

    /// Set the default font and size.
    pub fn set_default_font(&mut self, face_id: FontFaceId, size_px: f32) {
        self.typesetter.set_default_font(face_id, size_px);
        self.default_font = Some(face_id);
    }

    /// Get atlas information for GPU upload.
    /// Only advances the glyph cache generation and runs eviction when
    /// text work happened since the last call — this prevents aging out
    /// glyphs that are still visible but cached (idle app scenario).
    pub fn atlas_info(&mut self) -> AtlasInfo {
        let (dirty, width, height, pixels, glyphs_evicted) =
            self.typesetter.atlas_snapshot(self.had_text_activity);
        let rich_dirty = self.rich_text_atlas_dirty;
        self.rich_text_atlas_dirty = false;
        self.had_text_activity = false;
        AtlasInfo {
            dirty: dirty || rich_dirty,
            width,
            height,
            pixels: pixels.to_vec(),
            glyphs_evicted,
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

    /// Mutable access to the underlying `text_typeset::Typesetter`.
    ///
    /// The bridge normally restricts itself to single-line layout for
    /// labels and similar widgets. Rich-text consumers (the
    /// `fern_text::RichTextEngine` used by the `RichTextEditor`
    /// widget) need direct access to the typesetter's full-flow layout
    /// and render methods, and must share the same instance the bridge
    /// uses so glyphs end up in the same atlas fern-render uploads to
    /// the GPU. Exposed behind `#[cfg(feature = "rich-text")]` so the
    /// default feature set keeps a minimal public surface.
    #[cfg(feature = "rich-text")]
    pub fn typesetter_mut(&mut self) -> &mut Typesetter {
        self.had_text_activity = true;
        self.rich_text_atlas_dirty = true;
        &mut self.typesetter
    }

    /// Immutable read-only queries on the inner typesetter, used by
    /// `RichTextEngine` for zoom / content-size getters that must not
    /// mark the bridge dirty. Kept alongside `typesetter_mut` under
    /// the `rich-text` feature.
    #[cfg(feature = "rich-text")]
    pub fn typesetter_zoom_readonly(&self) -> f32 {
        self.typesetter.zoom()
    }

    #[cfg(feature = "rich-text")]
    pub fn typesetter_layout_width_readonly(&self) -> f32 {
        self.typesetter.layout_width()
    }

    #[cfg(feature = "rich-text")]
    pub fn typesetter_content_height_readonly(&self) -> f32 {
        self.typesetter.content_height()
    }

    #[cfg(feature = "rich-text")]
    pub fn typesetter_max_content_width_readonly(&self) -> f32 {
        self.typesetter.max_content_width()
    }

    /// Current HiDPI display scale factor as last set by
    /// `TextBackend::set_scale_factor`. The rich-text engine reads
    /// this on every `layout_full` so glyph rasterization stays
    /// crisp on HiDPI displays — the widget itself never needs to
    /// know about it, exactly like `TextWidget`'s label path where
    /// `layout_single_line` pre-multiplies the font size by
    /// `self.scale_factor` internally.
    #[cfg(feature = "rich-text")]
    pub fn display_scale_factor(&self) -> f32 {
        self.scale_factor
    }
}

impl Default for TypesetterBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBackend for TypesetterBridge {
    fn set_scale_factor(&mut self, scale_factor: f32) {
        if (self.scale_factor - scale_factor).abs() > 0.001 {
            self.scale_factor = scale_factor;
            // text-typeset now owns the pre-scale: forward the new
            // value and let it clear its own caches. Our layout /
            // glyph caches still need to be dropped because they
            // key off the scale factor (and the typesetter itself
            // has evicted every rasterized glyph, so any cached
            // atlas coords are now stale).
            self.typesetter.set_scale_factor(scale_factor);
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
        let cache_key = LayoutCacheKey::new_single_line(text, style, max_width, self.scale_factor);

        if let Some(cached) = self.layout_cache.get(&cache_key) {
            return cached.clone();
        }

        // Full shaping on cache miss. text-typeset handles the
        // scale-factor pre-multiply internally now, so the caller
        // supplies logical font sizes and gets logical metrics back.
        self.had_text_activity = true;
        let format = Self::to_text_format(style);
        let result: SingleLineResult =
            self.typesetter
                .layout_single_line(text, &format, max_width);

        let key = self.next_layout_key;
        self.next_layout_key += 1;

        let layout = TextLayout {
            width: result.width,
            height: result.height,
            ascent: result.baseline,
            descent: result.height - result.baseline,
            layout_key: key,
            line_count: 1,
        };

        self.layout_cache.insert(cache_key.clone(), layout.clone());
        self.glyph_cache.insert(
            key,
            result
                .glyphs
                .iter()
                .map(|g| GlyphQuad {
                    screen: g.screen,
                    atlas: g.atlas,
                    color: g.color,
                })
                .collect(),
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
            LayoutCacheKey::new_paragraph(text, style, max_width, max_lines, self.scale_factor);

        if let Some(cached) = self.layout_cache.get(&cache_key) {
            return cached.clone();
        }

        self.had_text_activity = true;
        let format = Self::to_text_format(style);
        let result: ParagraphResult =
            self.typesetter
                .layout_paragraph(text, &format, max_width, max_lines);

        let key = self.next_layout_key;
        self.next_layout_key += 1;

        let layout = TextLayout {
            width: result.width,
            height: result.height,
            ascent: result.baseline_first,
            descent: (result.height - result.baseline_first).max(0.0),
            layout_key: key,
            line_count: result.line_count.max(1),
        };

        self.layout_cache.insert(cache_key.clone(), layout.clone());
        self.glyph_cache.insert(
            key,
            result
                .glyphs
                .iter()
                .map(|g| GlyphQuad {
                    screen: g.screen,
                    atlas: g.atlas,
                    color: g.color,
                })
                .collect(),
        );
        layout
    }

    fn ensure_glyphs(&mut self, layout: &TextLayout) -> Vec<GlyphQuad> {
        self.glyph_cache
            .get(&layout.layout_key)
            .cloned()
            .unwrap_or_default()
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
        // A `.notdef` glyph has atlas dimensions of exactly 0.0 (the
        // rasterizer produces an empty rect for missing outlines).
        // A real shaped glyph has positive atlas width AND height.
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

    /// Regression test for a text-typeset bidi bug: Latin text embedded in
    /// an Arabic string must not be visually reversed.
    ///
    /// Before the fix, `layout_single_line` passed the whole string to
    /// rustybuzz as one run with `Direction::Auto`. rustybuzz inferred RTL
    /// from the first strong Arabic char and reversed the entire buffer,
    /// so "Alice" embedded in an Arabic string rendered as "ecilA".
    ///
    /// After the fix, the layout path splits text into UAX #9 bidi runs
    /// in visual order and shapes each run with an explicit direction.
    ///
    /// This test shapes "Alice" on its own and "مرحبا Alice" together,
    /// then asserts the Latin glyphs in the mixed string appear in the
    /// same left-to-right order as the pure-Latin layout.
    #[cfg(feature = "fonts-arabic")]
    #[test]
    fn latin_in_arabic_is_not_visually_reversed() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let style = TextStyle::default();

        // Reference: pure-Latin "Alice" in LTR order.
        let pure = bridge.layout_single_line("Alice", &style, None);
        let pure_glyphs = bridge.ensure_glyphs(&pure);
        assert_eq!(
            pure_glyphs.len(),
            5,
            "expected 5 glyphs for 'Alice', got {}",
            pure_glyphs.len()
        );
        let pure_widths: Vec<f32> = pure_glyphs.iter().map(|g| g.screen[2]).collect();

        // Arabic-first mixed string: paragraph direction is RTL, so under
        // UAX #9 the Latin embedding ends up visually to the LEFT of the
        // Arabic. Its internal order must still be LTR (A, l, i, c, e).
        let mixed = bridge.layout_single_line("مرحبا Alice", &style, None);
        let mixed_glyphs = bridge.ensure_glyphs(&mixed);
        assert!(
            mixed_glyphs.len() > pure_glyphs.len(),
            "mixed layout should contain at least the Latin glyphs plus Arabic"
        );

        // The first 5 glyphs in visual order should be the Latin cluster
        // (leftmost in RTL paragraph). Their widths must match "Alice".
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

    /// Hebrew mirrors the Arabic test — verifies that the
    /// `fonts-hebrew` default feature actually registers the Noto
    /// Sans Hebrew font and that the shaper's codepoint fallback
    /// picks it up.
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
        // They should differ (narrow is constrained)
        assert!(narrow.width < wide.width || narrow.width <= 31.0);
    }

    /// Reproduces the stale-glyph bug: after a layout pass (many layout_single_line
    /// calls without ensure_glyphs), the first paint call's ensure_glyphs should
    /// return glyphs for the correct text, not for whatever was last measured.
    #[test]
    fn ensure_glyphs_after_layout_pass_returns_correct_text() {
        let mut bridge = TypesetterBridge::new_with_default_font();
        let style = TextStyle::default();

        // Simulate layout pass: measure several texts (no ensure_glyphs calls)
        let _l1 = bridge.layout_single_line("First text", &style, None);
        let _l2 = bridge.layout_single_line("Second text", &style, None);
        let _l3 = bridge.layout_single_line("Third text is the last measured", &style, None);

        // Simulate paint for the FIRST text: layout_single_line + ensure_glyphs
        let layout = bridge.layout_single_line("First text", &style, None);
        let glyphs = bridge.ensure_glyphs(&layout);

        // "First text" has 10 characters → should produce ~10 glyphs
        // "Third text is the last measured" has 31 chars → ~31 glyphs
        // If we get ~31 glyphs, the bug is present (stale last_result)
        assert!(!glyphs.is_empty(), "should produce glyphs for 'First text'");
        assert!(
            glyphs.len() <= 15,
            "got {} glyphs — expected ~10 for 'First text', not ~31 for the stale last_result",
            glyphs.len()
        );
    }
}
