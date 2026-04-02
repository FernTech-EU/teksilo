use std::collections::HashMap;

use fern_canvas::text_backend::{AtlasInfo, TextBackend, TextLayout};
use fern_canvas::GlyphQuad;
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
    /// Cached result from the last layout_single_line call.
    last_result: Option<SingleLineResult>,
    /// Display scale factor for HiDPI rasterization.
    scale_factor: f32,
    /// Layout metrics cache: avoids re-shaping text just for size measurement.
    layout_cache: HashMap<LayoutCacheKey, TextLayout>,
}

impl TypesetterBridge {
    pub fn new() -> Self {
        Self {
            typesetter: Typesetter::new(),
            default_font: None,
            next_layout_key: 1,
            last_result: None,
            scale_factor: 1.0,
            layout_cache: HashMap::new(),
        }
    }

    /// Create a bridge with the bundled default font (Noto Sans).
    pub fn new_with_default_font() -> Self {
        let mut bridge = Self::new();
        bridge.register_default_font();
        bridge
    }

    /// Register a font from raw TTF/OTF data.
    pub fn register_font(&mut self, data: &[u8]) -> FontFaceId {
        self.typesetter.register_font(data)
    }

    /// Register and set the bundled default font.
    fn register_default_font(&mut self) {
        let font_data = include_bytes!("../fonts/NotoSans-Regular.ttf");
        let face_id = self.typesetter.register_font(font_data);
        self.typesetter.set_default_font(face_id, 14.0);
        self.default_font = Some(face_id);
    }

    /// Set the default font and size.
    pub fn set_default_font(&mut self, face_id: FontFaceId, size_px: f32) {
        self.typesetter.set_default_font(face_id, size_px);
        self.default_font = Some(face_id);
    }

    /// Get atlas information for GPU upload.
    /// Calls render() internally to synchronize atlas metadata.
    pub fn atlas_info(&mut self) -> AtlasInfo {
        let frame = self.typesetter.render();
        AtlasInfo {
            dirty: frame.atlas_dirty,
            width: frame.atlas_width,
            height: frame.atlas_height,
            pixels: frame.atlas_pixels.clone(),
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
            self.layout_cache.clear(); // Scale change invalidates all cached layouts
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

        // Check cache for metrics (avoids full shaping for size_that_fits calls).
        // If last_result is already Some (previous call not yet consumed by
        // ensure_glyphs), we can return cached metrics safely.
        // If last_result is None (ensure_glyphs consumed it), we must re-shape
        // because the next ensure_glyphs call needs fresh glyph data.
        if self.last_result.is_some() {
            if let Some(cached) = self.layout_cache.get(&cache_key) {
                return cached.clone();
            }
        }

        // Full shaping — either cache miss or last_result was consumed
        let mut format = Self::to_text_format(style);
        format.font_size = format.font_size.map(|s| s * sf);

        let physical_max = max_width.map(|w| w * sf);
        let result = self.typesetter.layout_single_line(text, &format, physical_max);

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

        self.layout_cache.insert(cache_key, layout.clone());
        self.last_result = Some(result);
        layout
    }

    fn ensure_glyphs(&mut self, _layout: &TextLayout) -> Vec<GlyphQuad> {
        let result = match self.last_result.take() {
            Some(r) => r,
            None => return Vec::new(),
        };

        let inv = 1.0 / self.scale_factor;
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
            .collect()
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
}
