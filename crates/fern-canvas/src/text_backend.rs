use fern_tokens::TextStyle;

use crate::render_frame::GlyphQuad;

/// Result of measuring a single line of text.
#[derive(Debug, Clone)]
pub struct TextLayout {
    pub width: f32,
    pub height: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Opaque key for the backend to identify the cached layout.
    pub layout_key: u64,
}

/// Trait for text layout and glyph rasterization backends.
/// Implemented by fern-text (wrapping text-typeset) for real rendering,
/// and by a mock for headless tests.
pub trait TextBackend {
    /// Set the display scale factor (e.g. 2.0 for HiDPI/Retina).
    /// Implementations should rasterize glyphs at `font_size * scale_factor`
    /// while returning metrics in logical pixels.
    fn set_scale_factor(&mut self, _scale_factor: f32) {}

    /// Measure and layout a single line of text.
    fn layout_single_line(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextLayout;

    /// Produce GPU-ready glyph quads for a previously laid-out text.
    /// The quads are positioned relative to (0, 0); the caller offsets them.
    fn ensure_glyphs(&mut self, layout: &TextLayout) -> Vec<GlyphQuad>;
}

/// Atlas information from the text backend for GPU upload.
#[derive(Debug, Clone)]
pub struct AtlasInfo {
    pub dirty: bool,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
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
            layout_key: 0,
        }
    }

    fn ensure_glyphs(&mut self, _layout: &TextLayout) -> Vec<GlyphQuad> {
        // Mock returns empty glyphs — no atlas needed for headless tests
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn mock_backend_ensure_glyphs_returns_empty() {
        let mut backend = MockTextBackend::new();
        let layout = backend.layout_single_line("Hi", &TextStyle::default(), None);
        let glyphs = backend.ensure_glyphs(&layout);
        assert!(glyphs.is_empty());
    }
}
