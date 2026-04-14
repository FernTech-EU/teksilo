use std::collections::HashMap;

use fern_canvas::GlyphQuad;
use fern_canvas::text_backend::{AtlasInfo, TextBackend, TextLayout};
use fern_tokens::TextStyle;
use text_typeset::{FontFaceId, SingleLineResult, TextFormat, Typesetter};

/// Cache key for text layout results.
#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutCacheKey {
    text: String,
    font_family: String,
    font_size_bits: u32, // f32 as bits for Hash/Eq
    font_weight: u32,
    max_width_bits: Option<u32>,
}

impl LayoutCacheKey {
    fn new(text: &str, style: &TextStyle, max_width: Option<f32>, scale_factor: f32) -> Self {
        let scaled_size = style.size * scale_factor;
        Self {
            text: text.to_string(),
            font_family: style.family.clone(),
            font_size_bits: scaled_size.to_bits(),
            font_weight: style.weight.0 as u32,
            max_width_bits: max_width.map(|w| (w * scale_factor).to_bits()),
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
        self.had_text_activity = false;
        AtlasInfo {
            dirty,
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
        let sf = self.scale_factor;
        let cache_key = LayoutCacheKey::new(text, style, max_width, sf);

        if let Some(cached) = self.layout_cache.get(&cache_key) {
            return cached.clone();
        }

        // Full shaping on cache miss.
        self.had_text_activity = true;
        let mut format = Self::to_text_format(style);
        format.font_size = format.font_size.map(|s| s * sf);

        let physical_max = max_width.map(|w| w * sf);
        let result: SingleLineResult =
            self.typesetter
                .layout_single_line(text, &format, physical_max);

        let key = self.next_layout_key;
        self.next_layout_key += 1;

        let inv = 1.0 / sf;
        let layout = TextLayout {
            width: result.width * inv,
            height: result.height * inv,
            ascent: result.baseline * inv,
            descent: (result.height - result.baseline) * inv,
            layout_key: key,
            line_count: 1,
        };

        self.layout_cache.insert(cache_key.clone(), layout.clone());
        let inv = 1.0 / sf;
        self.glyph_cache.insert(
            key,
            result
                .glyphs
                .iter()
                .map(|g| GlyphQuad {
                    screen: [
                        g.screen[0] * inv,
                        g.screen[1] * inv,
                        g.screen[2] * inv,
                        g.screen[3] * inv,
                    ],
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
