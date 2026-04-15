//! Per-widget rich-text layout engine.
//!
//! `RichTextEngine` is a thin driver that runs a `text_typeset::Typesetter`
//! through the layout / render / hit-test cycle needed by
//! `RichTextEditor`. It never owns its own `Typesetter` — every engine
//! shares the one inside a `SharedTypesetter`, so all rich-text widgets
//! and all single-line labels emit glyphs into **the same atlas**
//! fern-render uploads to the GPU. The flow-layout state stored on the
//! `Typesetter` is transient: each widget re-layouts its document in
//! its `paint()` pass, before calling `render()`.
//!
//! HiDPI correctness is handled entirely upstream by
//! [`text_typeset::Typesetter::set_scale_factor`]: layout stays in
//! logical pixels, glyph rasterization happens at `font_size *
//! scale_factor`. No per-widget pre-scaling on the fern-text side.

use std::cell::RefCell;
use std::rc::Rc;

use text_document::{FlowSnapshot, TextDocument};
use text_typeset::{CursorDisplay, FontFaceId, HitTestResult, RenderFrame};

use crate::font_registrar::{EmbeddedInterRegistrar, FontRegistrar};
use crate::shared_typesetter::SharedTypesetter;
use crate::typesetter_bridge::TypesetterBridge;

/// Wrap mode chosen at construction; forwarded to the underlying Typesetter
/// via `set_content_width_auto()` or `set_content_width(INFINITY)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    /// Text reflows at the viewport width. No horizontal scroll needed.
    Word,
    /// Text does not wrap; horizontal scrolling exposes long lines.
    None,
}

impl Default for WrapMode {
    fn default() -> Self {
        WrapMode::Word
    }
}

pub struct RichTextEngine {
    shared: Rc<RefCell<TypesetterBridge>>,
    default_face: Option<FontFaceId>,
    wrap_mode: WrapMode,
    has_full_layout: bool,
}

impl std::fmt::Debug for RichTextEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichTextEngine")
            .field("wrap_mode", &self.wrap_mode)
            .field("has_full_layout", &self.has_full_layout)
            .finish_non_exhaustive()
    }
}

impl RichTextEngine {
    /// Build an engine that shares an existing `SharedTypesetter`. The
    /// engine does **not** re-register fonts — the caller is expected
    /// to have populated the shared typesetter already (e.g. via
    /// `SharedTypesetter::new_with_default_font`).
    pub fn from_shared(shared: SharedTypesetter) -> Self {
        Self {
            shared: shared.bridge().clone(),
            default_face: None,
            wrap_mode: WrapMode::Word,
            has_full_layout: false,
        }
    }

    /// Construct a standalone engine with a private `SharedTypesetter`,
    /// registering fonts from `registrar`. Used by tests and by
    /// isolated headless runs that have no `SharedTypesetter` reachable
    /// via `app_state`. The private atlas will **not** be uploaded by
    /// fern-render, so this constructor is not suitable for windowed
    /// rendering — use [`from_shared`](Self::from_shared) in that case.
    pub fn private_with_registrar(registrar: &dyn FontRegistrar) -> Self {
        let bridge = Rc::new(RefCell::new(TypesetterBridge::new()));
        let default_face = {
            let mut b = bridge.borrow_mut();
            registrar.register(b.typesetter_mut())
        };
        Self {
            shared: bridge,
            default_face,
            wrap_mode: WrapMode::Word,
            has_full_layout: false,
        }
    }

    /// Convenience: private engine with the embedded Inter registrar.
    pub fn private_default() -> Self {
        Self::private_with_registrar(&EmbeddedInterRegistrar::new())
    }

    // --- Configuration ---------------------------------------------------

    pub fn set_wrap_mode(&mut self, mode: WrapMode) {
        self.wrap_mode = mode;
        let mut b = self.shared.borrow_mut();
        match mode {
            WrapMode::Word => b.typesetter_mut().set_content_width_auto(),
            WrapMode::None => b.typesetter_mut().set_content_width(f32::INFINITY),
        }
    }

    pub fn wrap_mode(&self) -> WrapMode {
        self.wrap_mode
    }

    /// Set the user-facing zoom factor (1.0 = normal). Forwarded
    /// directly to the typesetter's own `set_zoom` (post-layout
    /// display transform — glyph rasterization size is unaffected;
    /// HiDPI crispness is preserved by `set_scale_factor`).
    pub fn set_zoom(&mut self, zoom: f32) {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .set_zoom(zoom);
    }

    pub fn zoom(&self) -> f32 {
        self.shared.borrow().typesetter_zoom_readonly()
    }

    /// Current HiDPI display scale factor, read from the shared
    /// typesetter. Exposed for diagnostics only — widgets never
    /// need to consume this value; the typesetter handles the
    /// pre-scale internally.
    pub fn display_scale_factor(&self) -> f32 {
        self.shared.borrow().display_scale_factor()
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .set_viewport(width, height);
    }

    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .set_scroll_offset(offset);
    }

    pub fn set_selection_color(&mut self, color: [f32; 4]) {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .set_selection_color(color);
    }

    pub fn set_cursor_color(&mut self, color: [f32; 4]) {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .set_cursor_color(color);
    }

    pub fn set_text_color(&mut self, color: [f32; 4]) {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .set_text_color(color);
    }

    pub fn default_face(&self) -> Option<FontFaceId> {
        self.default_face
    }

    pub fn layout_width(&self) -> f32 {
        self.shared.borrow().typesetter_layout_width_readonly()
    }

    pub fn content_height(&self) -> f32 {
        self.shared.borrow().typesetter_content_height_readonly()
    }

    pub fn max_content_width(&self) -> f32 {
        self.shared
            .borrow()
            .typesetter_max_content_width_readonly()
    }

    pub fn has_full_layout(&self) -> bool {
        self.has_full_layout
    }

    // --- Layout ----------------------------------------------------------

    pub fn layout_full(&mut self, flow: &FlowSnapshot) {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .layout_full(flow);
        self.has_full_layout = true;
    }

    /// Incremental relayout of a single block. Falls back to `layout_full`
    /// if no full layout has happened yet.
    pub fn relayout_block_snapshot(
        &mut self,
        doc: &TextDocument,
        block_position: usize,
    ) -> Result<usize, String> {
        if !self.has_full_layout {
            let flow = doc.snapshot_flow();
            self.layout_full(&flow);
            return Ok(0);
        }
        let snap = doc
            .snapshot_block_at_position(block_position)
            .ok_or_else(|| "no block at position".to_string())?;
        let block_id = snap.block_id;
        let params = text_typeset::bridge::convert_block(&snap);
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .relayout_block(&params);
        Ok(block_id)
    }

    // --- Rendering -------------------------------------------------------

    /// Render the current layout state and run `f` with the resulting
    /// frame. The closure pattern keeps the bridge borrow alive just
    /// long enough for the caller to consume the frame without holding
    /// it across other bridge calls.
    pub fn with_render_frame<R>(&mut self, f: impl FnOnce(&RenderFrame) -> R) -> R {
        let mut bridge = self.shared.borrow_mut();
        let frame = bridge.typesetter_mut().render();
        f(frame)
    }

    pub fn with_render_block_only<R>(
        &mut self,
        block_id: usize,
        f: impl FnOnce(&RenderFrame) -> R,
    ) -> R {
        let mut bridge = self.shared.borrow_mut();
        let frame = bridge.typesetter_mut().render_block_only(block_id);
        f(frame)
    }

    pub fn with_render_cursor_only<R>(&mut self, f: impl FnOnce(&RenderFrame) -> R) -> R {
        let mut bridge = self.shared.borrow_mut();
        let frame = bridge.typesetter_mut().render_cursor_only();
        f(frame)
    }

    pub fn set_cursor(&mut self, cursor: &CursorDisplay) {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .set_cursor(cursor);
    }

    // --- Hit testing / caret geometry ------------------------------------

    pub fn hit_test(&self, x: f32, y: f32) -> Option<HitTestResult> {
        self.shared.borrow_mut().typesetter_mut().hit_test(x, y)
    }

    pub fn caret_rect(&self, position: usize) -> [f32; 4] {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .caret_rect(position)
    }

    /// Passthrough to `Typesetter::character_geometry`. Returns
    /// per-character `(position, width)` for a character range
    /// within a laid-out block. Used by the rich text editor's
    /// accessibility pass to populate AccessKit `character_positions`
    /// / `character_widths` on `Role::TextRun` children.
    pub fn character_geometry(
        &self,
        block_id: usize,
        char_start: usize,
        char_end: usize,
    ) -> Vec<text_typeset::CharacterGeometry> {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .character_geometry(block_id, char_start, char_end)
    }

    pub fn ensure_caret_visible(&mut self) -> Option<f32> {
        self.shared
            .borrow_mut()
            .typesetter_mut()
            .ensure_caret_visible()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use text_document::TextDocument;

    #[test]
    fn private_engine_constructs_with_embedded_inter() {
        let engine = RichTextEngine::private_default();
        assert!(engine.default_face().is_some());
        assert!(!engine.has_full_layout());
    }

    #[test]
    fn private_engine_lays_out_plain_text_document() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        engine.set_wrap_mode(WrapMode::Word);

        let doc = TextDocument::new();
        doc.set_plain_text("Hello, world!\nSecond line.").unwrap();

        let flow = doc.snapshot_flow();
        engine.layout_full(&flow);

        assert!(engine.has_full_layout());
        assert!(engine.content_height() > 0.0);

        let glyph_count = engine.with_render_frame(|frame| frame.glyphs.len());
        assert!(
            glyph_count > 0,
            "layout should produce glyph quads ({} glyphs)",
            glyph_count
        );
    }

    #[test]
    fn relayout_block_falls_back_to_full_on_first_call() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        let doc = TextDocument::new();
        doc.set_plain_text("Hello").unwrap();

        let result = engine.relayout_block_snapshot(&doc, 0);
        assert!(result.is_ok());
        assert!(engine.has_full_layout());
    }

    #[test]
    fn hit_test_returns_a_position_inside_layout() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        let doc = TextDocument::new();
        doc.set_plain_text("Hello world").unwrap();
        engine.layout_full(&doc.snapshot_flow());
        engine.with_render_frame(|_| {});
        let hit = engine.hit_test(5.0, 5.0);
        assert!(hit.is_some(), "hit test on laid out text must succeed");
    }

    #[test]
    fn two_engines_sharing_typesetter_both_produce_glyphs() {
        let shared = SharedTypesetter::new_with_default_font();
        let mut a = RichTextEngine::from_shared(shared.clone());
        let mut b = RichTextEngine::from_shared(shared);
        a.set_viewport(200.0, 200.0);
        b.set_viewport(200.0, 200.0);

        let doc_a = TextDocument::new();
        doc_a.set_plain_text("Alpha").unwrap();
        let doc_b = TextDocument::new();
        doc_b.set_plain_text("Beta").unwrap();

        a.layout_full(&doc_a.snapshot_flow());
        let a_glyphs = a.with_render_frame(|f| f.glyphs.len());
        assert!(a_glyphs > 0);

        b.layout_full(&doc_b.snapshot_flow());
        let b_glyphs = b.with_render_frame(|f| f.glyphs.len());
        assert!(b_glyphs > 0);
    }
}
