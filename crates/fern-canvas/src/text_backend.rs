use fern_tokens::TextStyle;

use crate::render_frame::GlyphQuad;

/// Result of measuring text.
#[derive(Debug, Clone)]
pub struct TextLayout {
    pub width: f32,
    pub height: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Opaque key for the backend to identify the cached layout.
    pub layout_key: u64,
    /// Number of lines (1 for single-line, ≥1 for paragraph).
    pub line_count: usize,
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
            line_count: 1,
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
                layout_key: 0,
                line_count: 1,
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
            layout_key: 0,
            line_count,
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
