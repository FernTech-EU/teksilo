// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-widget rich-text layout engine.
//!
//! `RichTextEngine` is a thin driver that runs a
//! [`text_typeset::DocumentFlow`] through the layout / render /
//! hit-test cycle needed by `RichTextEditor`. It owns its own
//! `DocumentFlow` — every engine has an independent viewport, zoom,
//! scroll offset, caret, wrap mode, and flow layout — and borrows a
//! shared [`TextFontService`] (via [`SharedTypesetter`]) at layout
//! and render time so glyphs from every widget land in the same
//! GPU atlas.
//!
//! This split means two widgets viewing the same `TextDocument`
//! with different viewports never cross-contaminate each other's
//! state. The shared side is strictly the expensive-to-build /
//! expensive-to-share part (font registry, glyph atlas, shaper
//! cache); everything per-widget stays on the engine.
//!
//! HiDPI correctness is handled upstream by
//! [`TextFontService::set_scale_factor`]: layout stays in logical
//! pixels, glyph rasterization happens at `font_size * scale_factor`.
//! No per-widget pre-scaling on the bastyde-text side.
//!
//! [`TextFontService`]: text_typeset::TextFontService
//! [`TextFontService::set_scale_factor`]: text_typeset::TextFontService::set_scale_factor

use std::cell::RefCell;
use std::rc::Rc;

use text_document::{FlowSnapshot, TextDocument};
use text_typeset::{CursorDisplay, DocumentFlow, FontFaceId, HitTestResult, RenderFrame};

use crate::font_registrar::{EmbeddedInterRegistrar, FontRegistrar};
use crate::shared_typesetter::SharedTypesetter;
use crate::typesetter_bridge::TypesetterBridge;
use crate::typography_defaults::{self, EditorTypographyDefaults};

/// Wrap mode chosen at construction; forwarded to the owned
/// [`DocumentFlow`] via `set_content_width_auto()` or
/// `set_content_width(INFINITY)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapMode {
    /// Text reflows at the viewport width. No horizontal scroll
    /// needed.
    #[default]
    Word,
    /// Text does not wrap; horizontal scrolling exposes long lines.
    None,
}

pub struct RichTextEngine {
    shared: Rc<RefCell<TypesetterBridge>>,
    /// Per-widget flow state. Every layout and render call borrows
    /// the shared font service through `shared` but mutates this
    /// flow directly, so two engines on the same bridge keep
    /// independent viewports / zooms / scroll offsets / cursors.
    flow: DocumentFlow,
    default_face: Option<FontFaceId>,
    wrap_mode: WrapMode,
    /// Non-destructive default typography filled onto the layout snapshot for
    /// runs / blocks with no explicit override. Never touches the document.
    typography_defaults: EditorTypographyDefaults,
}

impl std::fmt::Debug for RichTextEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichTextEngine")
            .field("wrap_mode", &self.wrap_mode)
            .field("has_layout", &self.flow.has_layout())
            .finish_non_exhaustive()
    }
}

impl RichTextEngine {
    /// Build an engine that shares an existing [`SharedTypesetter`].
    ///
    /// Does **not** register any fonts on the shared service — the
    /// caller is expected to have populated it already (e.g. via
    /// `SharedTypesetter::new_with_default_font`). Each engine
    /// gets its own independent `DocumentFlow`.
    pub fn from_shared(shared: SharedTypesetter) -> Self {
        let mut engine = Self {
            shared: shared.bridge().clone(),
            flow: DocumentFlow::new(),
            default_face: None,
            wrap_mode: WrapMode::Word,
            typography_defaults: EditorTypographyDefaults::default(),
        };
        engine.flow.set_content_width_auto();
        engine
    }

    /// Construct a standalone engine with a private bridge,
    /// registering fonts from `registrar`.
    ///
    /// Used by tests and by isolated headless runs that have no
    /// `SharedTypesetter` reachable via `app_state`. The private
    /// atlas will **not** be uploaded by bastyde-render, so this
    /// constructor is not suitable for windowed rendering — use
    /// [`from_shared`](Self::from_shared) in that case.
    pub fn private_with_registrar(registrar: &dyn FontRegistrar) -> Self {
        let bridge = Rc::new(RefCell::new(TypesetterBridge::new()));
        let default_face = {
            let mut b = bridge.borrow_mut();
            registrar.register_on_service(b.service_mut())
        };
        let mut engine = Self {
            shared: bridge,
            flow: DocumentFlow::new(),
            default_face,
            wrap_mode: WrapMode::Word,
            typography_defaults: EditorTypographyDefaults::default(),
        };
        engine.flow.set_content_width_auto();
        engine
    }

    /// Convenience: private engine with the embedded Inter
    /// registrar.
    pub fn private_default() -> Self {
        Self::private_with_registrar(&EmbeddedInterRegistrar::new())
    }

    // --- Configuration ---------------------------------------------------

    pub fn set_wrap_mode(&mut self, mode: WrapMode) {
        self.wrap_mode = mode;
        match mode {
            WrapMode::Word => self.flow.set_content_width_auto(),
            WrapMode::None => self.flow.set_content_width(f32::INFINITY),
        }
    }

    pub fn wrap_mode(&self) -> WrapMode {
        self.wrap_mode
    }

    /// Set the user-facing zoom factor (1.0 = normal). Forwarded
    /// to this engine's own `DocumentFlow` — pure display
    /// transform, glyph rasterization size is unaffected, HiDPI
    /// crispness comes from the service's scale factor instead.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.flow.set_zoom(zoom);
    }

    pub fn zoom(&self) -> f32 {
        self.flow.zoom()
    }

    /// Set the logical font-scale factor (`1.0` = none). Unlike
    /// [`set_zoom`](Self::set_zoom) (a display transform that leaves font
    /// metrics untouched), this multiplies the resolved logical font size
    /// *before* shaping, so the text genuinely grows and re-wraps — the
    /// per-engine mechanism behind an app-wide "grow all text" accessibility
    /// setting. Takes effect on the next `layout_full`.
    pub fn set_font_scale(&mut self, font_scale: f32) {
        self.flow.set_font_scale(font_scale);
    }

    /// Current logical font-scale factor.
    pub fn font_scale(&self) -> f32 {
        self.flow.font_scale()
    }

    /// Set the non-destructive default typography (font family / line height /
    /// first-line indent) filled onto runs / blocks with no explicit override at
    /// layout time. Never mutates the bound document (no undo entry, no
    /// `modified`). Takes effect on the next `layout_full` /
    /// `relayout_block_snapshot`; the caller forces a relayout.
    pub fn set_typography_defaults(&mut self, defaults: EditorTypographyDefaults) {
        self.typography_defaults = defaults;
    }

    /// The current default typography (see [`set_typography_defaults`](Self::set_typography_defaults)).
    pub fn typography_defaults(&self) -> &EditorTypographyDefaults {
        &self.typography_defaults
    }

    /// Current HiDPI display scale factor, read from the shared
    /// bridge. Exposed for diagnostics only — widgets never need
    /// to consume this value; the service handles the pre-scale
    /// internally.
    pub fn display_scale_factor(&self) -> f32 {
        self.shared.borrow().display_scale_factor()
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.flow.set_viewport(width, height);
    }

    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.flow.set_scroll_offset(offset);
    }

    pub fn set_selection_color(&mut self, color: [f32; 4]) {
        self.flow.set_selection_color(color);
    }

    pub fn set_cursor_color(&mut self, color: [f32; 4]) {
        self.flow.set_cursor_color(color);
    }

    pub fn set_text_color(&mut self, color: [f32; 4]) {
        self.flow.set_text_color(color);
    }

    /// Set the background painted behind fenced code blocks when the
    /// block carries no explicit `background_color`. Wired from the
    /// active theme's `editor_code_block_bg` by the host widget so
    /// dark / light swaps reach the cards.
    pub fn set_code_block_background(&mut self, color: [f32; 4]) {
        self.flow.set_code_block_background(color);
    }

    /// Auto-hyphenate justified blocks that don't set `hyphenate`
    /// explicitly. Enable on prose / rich-text surfaces (paired with
    /// justified alignment); leave off for single-line / label widgets.
    /// Default `false`.
    pub fn set_hyphenate_justified(&mut self, enabled: bool) {
        self.flow.set_hyphenate_justified(enabled);
    }

    /// Set the foreground used for monospaced runs (inline `code`,
    /// fenced code blocks) that carry no explicit `foreground_color`.
    /// `None` falls back to the engine's `text_color`. Wired from
    /// `editor_code_block_fg` alongside the background setter.
    pub fn set_code_block_foreground(&mut self, color: Option<[f32; 4]>) {
        self.flow.set_code_block_foreground(color);
    }

    /// Set the echo / masking character for secure (password) fields.
    ///
    /// When `Some(c)`, every character is replaced with `c` before
    /// shaping on the next `layout_full` / `relayout_block_snapshot` —
    /// the real text never reaches the shaper or the glyph atlas, only
    /// the echo character does. `None` (default) renders verbatim. One
    /// echo char is emitted per source `char`, so caret / selection /
    /// hit-test (all char-indexed) stay aligned with the host document.
    ///
    /// After flipping the masking state the caller should force a full
    /// relayout (the engine reports `has_full_layout()` independently of
    /// the echo char), since the laid-out glyphs change wholesale.
    pub fn set_echo_char(&mut self, echo: Option<char>) {
        self.flow.set_echo_char(echo);
    }

    /// Current echo / masking character, if any.
    pub fn echo_char(&self) -> Option<char> {
        self.flow.echo_char()
    }

    pub fn default_face(&self) -> Option<FontFaceId> {
        self.default_face
    }

    pub fn layout_width(&self) -> f32 {
        self.flow.layout_width()
    }

    pub fn content_height(&self) -> f32 {
        self.flow.content_height()
    }

    /// Line height (in logical pixels) of the shared service's
    /// default font + size. Useful for widgets that need to size
    /// themselves against an intrinsic line height before any
    /// content has been laid out (`RichTextEditor::min_lines` /
    /// `max_lines`). Returns `0.0` if no default font is registered
    /// on the shared service.
    pub fn default_line_height(&self) -> f32 {
        self.shared.borrow().default_line_height()
    }

    pub fn max_content_width(&self) -> f32 {
        self.flow.max_content_width()
    }

    /// Whether this engine has a valid full layout installed.
    ///
    /// Returns `false` when the engine has never run `layout_full`,
    /// or when the shared service's HiDPI scale factor has changed
    /// since the last layout (in which case shaped advances are
    /// stale and the caller must re-run `layout_full`).
    pub fn has_full_layout(&self) -> bool {
        if !self.flow.has_layout() {
            return false;
        }
        let bridge = self.shared.borrow();
        !self.flow.layout_dirty_for_scale(bridge.service())
    }

    // --- Layout ----------------------------------------------------------

    pub fn layout_full(&mut self, flow: &FlowSnapshot) {
        // Fill any per-editor default typography onto a disposable copy of the
        // snapshot (never the live document). When no defaults are set this is a
        // zero-cost borrow of the caller's snapshot.
        let filled: std::borrow::Cow<FlowSnapshot> =
            if typography_defaults::needs_snapshot_fill(&self.typography_defaults) {
                let mut owned = flow.clone();
                typography_defaults::apply_to_flow(&mut owned, &self.typography_defaults);
                std::borrow::Cow::Owned(owned)
            } else {
                std::borrow::Cow::Borrowed(flow)
            };
        let flow: &FlowSnapshot = &filled;
        {
            let bridge = self.shared.borrow();
            self.flow.layout_full(bridge.service(), flow);
        }
        // A paint-only highlighter ships its spans separately from the shaped
        // `fragments`; apply them as a post-shape recolor (no extra reshape).
        // A full layout produces base-colored blocks, so this is only needed
        // when there ARE spans.
        let spans = text_typeset::bridge::collect_paint_spans(flow);
        if !spans.is_empty() {
            self.flow.apply_paint_spans_for(spans);
        }
    }

    /// Recolor the cached layout from the snapshot's paint-only highlight
    /// overlay WITHOUT reshaping or reflowing. The editor's fast path for a
    /// `HighlightPaintChanged` event. Call a render afterward to refresh the
    /// frame. An empty overlay (highlighter removed) resets blocks to base.
    pub fn apply_paint_highlights(&mut self, flow: &FlowSnapshot) {
        let spans = text_typeset::bridge::collect_paint_spans(flow);
        self.flow.apply_paint_spans_for(spans);
    }

    /// Incremental relayout of a single block. Falls back to
    /// `layout_full` when no valid full layout is installed for
    /// this engine — either we've never run one, or the HiDPI
    /// scale factor changed and the old advances are stale.
    ///
    /// The structural `has_full_layout()` guard above makes the
    /// underlying [`DocumentFlow::relayout_block`] error variants
    /// unreachable from this entry point; the call site below
    /// `.expect`s them as a soundness assertion — any failure
    /// there would mean the two `DocumentFlow` invariant checks
    /// and `RichTextEngine::has_full_layout` disagree, which is
    /// a bug in one of them.
    ///
    /// `mask` selects which highlight sessions this view renders: an empty mask pulls a clean
    /// snapshot (no highlights), a full mask every session, a narrow one a chosen set — so two
    /// panes over one document can relayout the same block with different find highlighting.
    /// The (already-masked) paint overlay is re-applied unconditionally; an empty span set
    /// simply clears any prior overlay.
    pub fn relayout_block_snapshot(
        &mut self,
        doc: &TextDocument,
        block_position: usize,
        mask: &text_document::HighlightMask,
    ) -> Result<usize, String> {
        if !self.has_full_layout() {
            self.layout_full(&doc.snapshot_flow_masked(mask));
            return Ok(0);
        }
        let mut snap = doc
            .snapshot_block_at_position_masked(block_position, mask)
            .ok_or_else(|| "no block at position".to_string())?;
        // Fill per-editor default typography onto the (already-detached) block
        // snapshot before it reaches the typesetter — same non-destructive path
        // as `layout_full`.
        if typography_defaults::needs_snapshot_fill(&self.typography_defaults) {
            typography_defaults::apply_to_block(&mut snap, &self.typography_defaults);
        }
        let block_id = snap.block_id;
        let opts = text_typeset::bridge::BridgeOptions {
            code_block_background: self.flow.code_block_background(),
            code_block_foreground: self.flow.code_block_foreground(),
            echo_char: self.flow.echo_char(),
            hyphenate_justified: self.flow.hyphenate_justified(),
        };
        let params = text_typeset::bridge::convert_block_with(&snap, &opts);
        {
            let bridge = self.shared.borrow();
            self.flow.relayout_block(bridge.service(), &params).expect(
                "relayout_block invariant violated: has_full_layout() should already \
                 guarantee has_layout() && !layout_dirty_for_scale()",
            );
        }
        // Re-apply the paint overlay for just this block on top of its freshly-reshaped base.
        // The snapshot is already masked, so its paint spans are exactly what this view should
        // show — and applying an EMPTY set (a mask that hides everything, or a session whose
        // ranges just cleared) clears any prior overlay, which is the desired bare look.
        let spans = text_typeset::bridge::convert_paint_spans(&snap);
        self.flow.apply_block_paint_spans(block_id, &spans);
        Ok(block_id)
    }

    // --- Rendering -------------------------------------------------------

    /// Render the current layout and hand the resulting
    /// [`RenderFrame`] to the closure. The closure pattern keeps
    /// the bridge borrow alive only long enough for the caller to
    /// consume the frame without holding it across other engine
    /// calls.
    pub fn with_render_frame<R>(&mut self, f: impl FnOnce(&RenderFrame) -> R) -> R {
        let mut bridge = self.shared.borrow_mut();
        // Follow the walker-set ambient raster scale (scene zoom) so
        // document glyphs densify like the label path. Layout is
        // unaffected; the flow falls back to a full render when the
        // scale changed since the last frame.
        self.flow.set_raster_scale(bridge.ambient_raster_scale());
        let frame = self.flow.render(bridge.service_mut());
        f(frame)
    }

    pub fn with_render_block_only<R>(
        &mut self,
        block_id: usize,
        f: impl FnOnce(&RenderFrame) -> R,
    ) -> R {
        let mut bridge = self.shared.borrow_mut();
        self.flow.set_raster_scale(bridge.ambient_raster_scale());
        let frame = self.flow.render_block_only(bridge.service_mut(), block_id);
        f(frame)
    }

    pub fn with_render_cursor_only<R>(&mut self, f: impl FnOnce(&RenderFrame) -> R) -> R {
        let mut bridge = self.shared.borrow_mut();
        let frame = self.flow.render_cursor_only(bridge.service_mut());
        f(frame)
    }

    pub fn set_cursor(&mut self, cursor: &CursorDisplay) {
        self.flow.set_cursor(cursor);
    }

    // --- Hit testing / caret geometry ------------------------------------

    pub fn hit_test(&self, x: f32, y: f32) -> Option<HitTestResult> {
        self.flow.hit_test(x, y)
    }

    /// Screen-space caret rectangle at a document position with the
    /// given affinity. Affinity only changes the result at soft-wrap
    /// boundaries; at every other position the two affinities return
    /// the same rect. See [`text_typeset::CursorAffinity`].
    pub fn caret_rect(&self, position: usize, affinity: text_typeset::CursorAffinity) -> [f32; 4] {
        self.flow.caret_rect(position, affinity)
    }

    /// Per-character `(position, width)` for a character range
    /// within a laid-out block. Used by the rich text editor's
    /// accessibility pass to populate AccessKit
    /// `character_positions` / `character_widths` on
    /// `Role::TextRun` children.
    pub fn character_geometry(
        &self,
        block_id: usize,
        char_start: usize,
        char_end: usize,
    ) -> Vec<text_typeset::CharacterGeometry> {
        self.flow.character_geometry(block_id, char_start, char_end)
    }

    pub fn ensure_caret_visible(&mut self) -> Option<f32> {
        self.flow.ensure_caret_visible()
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
    fn font_scale_forwards_and_grows_content_height() {
        let layout_at = |font_scale: f32| {
            let mut engine = RichTextEngine::private_default();
            engine.set_viewport(400.0, 300.0);
            engine.set_wrap_mode(WrapMode::Word);
            engine.set_font_scale(font_scale);
            assert_eq!(engine.font_scale(), font_scale);
            let doc = TextDocument::new();
            doc.set_plain_text("Hello, world!\nSecond line.").unwrap();
            let flow = doc.snapshot_flow();
            engine.layout_full(&flow);
            engine.content_height()
        };
        let h1 = layout_at(1.0);
        let h2 = layout_at(2.0);
        assert!(
            (h2 - h1 * 2.0).abs() < h1 * 0.1,
            "2x font scale should ~double content height: {h1} vs {h2}"
        );
    }

    #[test]
    fn typography_default_line_height_grows_content_height() {
        let layout_at = |line_height: f32| {
            let mut engine = RichTextEngine::private_default();
            engine.set_viewport(400.0, 300.0);
            engine.set_wrap_mode(WrapMode::Word);
            engine.set_typography_defaults(EditorTypographyDefaults {
                line_height,
                ..Default::default()
            });
            let doc = TextDocument::new();
            doc.set_plain_text("Hello, world!\nSecond line.").unwrap();
            engine.layout_full(&doc.snapshot_flow());
            engine.content_height()
        };
        // Default (no fill) and an explicit 1.0 fill must match to the pixel.
        assert!(
            (layout_at(1.0) - {
                let mut engine = RichTextEngine::private_default();
                engine.set_viewport(400.0, 300.0);
                engine.set_wrap_mode(WrapMode::Word);
                let doc = TextDocument::new();
                doc.set_plain_text("Hello, world!\nSecond line.").unwrap();
                engine.layout_full(&doc.snapshot_flow());
                engine.content_height()
            })
            .abs()
                < 0.01
        );
        let h1 = layout_at(1.0);
        let h2 = layout_at(2.0);
        assert!(
            h2 > h1 * 1.5,
            "2x default line-height should markedly grow content height: {h1} vs {h2}"
        );
    }

    #[test]
    fn typography_default_indent_offsets_first_line_but_never_mutates_document() {
        use text_typeset::CursorAffinity::Downstream;
        let caret_x = |indent: f32| {
            let mut engine = RichTextEngine::private_default();
            engine.set_viewport(600.0, 300.0);
            engine.set_wrap_mode(WrapMode::None);
            engine.set_typography_defaults(EditorTypographyDefaults {
                first_line_indent: indent,
                ..Default::default()
            });
            let doc = TextDocument::new();
            doc.set_plain_text("Hello").unwrap();
            engine.layout_full(&doc.snapshot_flow());
            engine.with_render_frame(|_| {});
            // The live document keeps its unset block format — the fill only
            // touched the disposable snapshot.
            let snap = doc.snapshot_block_at_position(0).unwrap();
            assert_eq!(snap.block_format.text_indent, None);
            assert!(!doc.can_undo());
            assert!(!doc.is_modified());
            engine.caret_rect(0, Downstream)[0]
        };
        let x0 = caret_x(0.0);
        let x40 = caret_x(40.0);
        assert!(
            x40 - x0 > 30.0,
            "40px default indent should push the first-line caret right: {x0} vs {x40}"
        );
    }

    #[test]
    fn caret_height_is_independent_of_line_height_default() {
        use text_typeset::CursorAffinity::Downstream;
        let caret_h = |line_height: f32| {
            let mut engine = RichTextEngine::private_default();
            engine.set_viewport(400.0, 300.0);
            engine.set_wrap_mode(WrapMode::Word);
            engine.set_typography_defaults(EditorTypographyDefaults {
                line_height,
                ..Default::default()
            });
            let doc = TextDocument::new();
            doc.set_plain_text("Hello world").unwrap();
            engine.layout_full(&doc.snapshot_flow());
            engine.with_render_frame(|_| {});
            engine.caret_rect(1, Downstream)[3]
        };
        // The caret tracks the glyph box, so doubling the default line-height
        // leaves the caret height essentially unchanged (before the fix it would
        // have doubled, overshooting far past the text).
        let h1 = caret_h(1.0);
        let h2 = caret_h(2.0);
        assert!(h1 > 0.0);
        assert!(
            (h1 - h2).abs() < h1 * 0.15,
            "caret height must not scale with line-height: {h1} vs {h2}"
        );
    }

    #[test]
    fn relayout_block_falls_back_to_full_on_first_call() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        let doc = TextDocument::new();
        doc.set_plain_text("Hello").unwrap();

        let result = engine.relayout_block_snapshot(&doc, 0, &text_document::HighlightMask::all());
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

    /// Two engines sharing one bridge keep independent viewports
    /// and flow layouts. This is the property the text-typeset
    /// split was designed to enforce: A running `layout_full`
    /// leaves B's flow state untouched.
    #[test]
    fn two_engines_sharing_bridge_each_keep_independent_flows() {
        let shared = SharedTypesetter::new_with_default_font();
        let mut a = RichTextEngine::from_shared(shared.clone());
        let mut b = RichTextEngine::from_shared(shared);
        a.set_viewport(200.0, 200.0);
        b.set_viewport(400.0, 400.0);

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

        // A's flow is still intact after B laid out; rendering
        // A again produces the same glyph count without a fresh
        // `layout_full`.
        let a_glyphs_again = a.with_render_frame(|f| f.glyphs.len());
        assert_eq!(a_glyphs, a_glyphs_again);
    }

    /// Regression test for HiDPI invalidation plumbing.
    ///
    /// When the shared [`SharedTypesetter`] receives a new scale
    /// factor, its backing [`text_typeset::TextFontService`] clears
    /// the atlas and glyph cache in place and bumps a monotonic
    /// `scale_generation` counter. Every `RichTextEngine` sharing
    /// that service must then report itself as "not laid out" so
    /// the widget paint path re-runs `layout_full` before
    /// rendering; otherwise rendered glyphs would be shaped
    /// advances at the old ppem rasterized against a fresh atlas.
    ///
    /// This test walks the whole chain:
    ///
    /// 1. Build a service and an engine, lay out a document.
    /// 2. Assert `has_full_layout == true`.
    /// 3. Bump the service's scale factor via `SharedTypesetter`.
    /// 4. Assert `has_full_layout == false` — the engine picks
    ///    up the service's generation mismatch through
    ///    `DocumentFlow::layout_dirty_for_scale`.
    /// 5. Re-run `layout_full`.
    /// 6. Assert `has_full_layout == true` again, and rendering
    ///    produces glyphs.
    #[test]
    fn scale_factor_change_invalidates_engine_layout() {
        let shared = SharedTypesetter::new_with_default_font();
        let mut engine = RichTextEngine::from_shared(shared.clone());
        engine.set_viewport(400.0, 300.0);

        let doc = TextDocument::new();
        doc.set_plain_text("Hello world").unwrap();
        engine.layout_full(&doc.snapshot_flow());
        assert!(
            engine.has_full_layout(),
            "engine should report a valid layout right after layout_full"
        );

        // Kick the HiDPI path on the shared service.
        shared.set_scale_factor(2.0);

        assert!(
            !engine.has_full_layout(),
            "scale_factor change must invalidate the engine's layout via \
             DocumentFlow::layout_dirty_for_scale"
        );

        // The widget paint path's `has_full_layout` guard would
        // re-run `layout_full` at this point. Do the same here.
        engine.layout_full(&doc.snapshot_flow());
        assert!(engine.has_full_layout());

        let glyph_count = engine.with_render_frame(|f| f.glyphs.len());
        assert!(
            glyph_count > 0,
            "post-relayout render must produce glyphs (got {})",
            glyph_count
        );
    }

    /// Secure-field masking: with an echo char set, every source
    /// character lays out as one uniform-width bullet glyph, and the
    /// real text never influences the geometry. This is the property
    /// the password field relies on for correct caret / selection /
    /// hit-test over masked content.
    #[test]
    fn echo_char_renders_uniform_width_bullets() {
        use text_typeset::CursorAffinity::Downstream;

        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        engine.set_wrap_mode(WrapMode::None);

        let doc = TextDocument::new();
        // 'W' is wide, 'i'/'l' are narrow — non-uniform unmasked.
        doc.set_plain_text("Wil").unwrap();

        // Unmasked sanity: 'W' is wider than 'i'.
        engine.layout_full(&doc.snapshot_flow());
        engine.with_render_frame(|_| {});
        let p1 = engine.caret_rect(1, Downstream)[0];
        let p2 = engine.caret_rect(2, Downstream)[0];
        let w_advance = p1;
        let i_advance = p2 - p1;
        assert!(
            (w_advance - i_advance).abs() > 0.5,
            "sanity: 'W' ({w_advance}) should be wider than 'i' ({i_advance}) unmasked"
        );

        // Masked: three bullets, all the same advance.
        engine.set_echo_char(Some('•'));
        engine.layout_full(&doc.snapshot_flow());
        let masked_glyphs = engine.with_render_frame(|f| f.glyphs.len());
        assert_eq!(
            masked_glyphs, 3,
            "exactly one bullet glyph per source char (got {masked_glyphs})"
        );
        let m1 = engine.caret_rect(1, Downstream)[0];
        let m2 = engine.caret_rect(2, Downstream)[0];
        let m3 = engine.caret_rect(3, Downstream)[0];
        assert!(m1 > 0.0, "first bullet must advance from origin");
        assert!(
            ((m2 - m1) - m1).abs() < 0.5 && ((m3 - m2) - m1).abs() < 0.5,
            "bullets must be uniform width: advances {m1}, {}, {}",
            m2 - m1,
            m3 - m2
        );

        // Clearing the echo char restores verbatim layout.
        engine.set_echo_char(None);
        engine.layout_full(&doc.snapshot_flow());
        let restored = engine.caret_rect(1, Downstream)[0];
        assert!(
            (restored - w_advance).abs() < 0.5,
            "clearing echo char must restore the original 'W' advance"
        );
    }

    /// Multi-byte source characters (here, an accented `é` = 2 UTF-8
    /// bytes) must still map one-bullet-per-char. This guards the
    /// byte→char cluster conversion: the masked block text is uniform
    /// 3-byte bullets, so the caret index `char_count` stays valid.
    #[test]
    fn echo_char_handles_multibyte_source_chars() {
        use text_typeset::CursorAffinity::Downstream;

        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        engine.set_wrap_mode(WrapMode::None);

        let doc = TextDocument::new();
        doc.set_plain_text("café").unwrap(); // 4 chars, 5 bytes
        let char_count = "café".chars().count();
        assert_eq!(char_count, 4);

        engine.set_echo_char(Some('•'));
        engine.layout_full(&doc.snapshot_flow());
        let masked_glyphs = engine.with_render_frame(|f| f.glyphs.len());
        assert_eq!(masked_glyphs, char_count, "one bullet per char incl. 'é'");

        // Caret at the last char index is valid and to the right of 0.
        let end = engine.caret_rect(char_count, Downstream)[0];
        let start = engine.caret_rect(0, Downstream)[0];
        assert!(
            end > start,
            "end caret ({end}) must be right of start ({start})"
        );
    }

    /// Companion to the test above — verifies the `relayout_block`
    /// fast path also respects the scale-factor invalidation. A
    /// caller that skips the `has_full_layout` check and goes
    /// straight to `relayout_block_snapshot` must still see the
    /// method fall back to a full layout when the service's scale
    /// generation has moved on.
    #[test]
    fn scale_factor_change_forces_relayout_block_to_fall_back() {
        let shared = SharedTypesetter::new_with_default_font();
        let mut engine = RichTextEngine::from_shared(shared.clone());
        engine.set_viewport(400.0, 300.0);

        let doc = TextDocument::new();
        doc.set_plain_text("Hello world").unwrap();
        engine.layout_full(&doc.snapshot_flow());

        shared.set_scale_factor(2.0);
        assert!(!engine.has_full_layout());

        // `relayout_block_snapshot` detects the stale layout and
        // falls back to `layout_full` — no panic, no partial
        // update, no error. After the call the engine is back to
        // a consistent state.
        let result = engine.relayout_block_snapshot(&doc, 0, &text_document::HighlightMask::all());
        assert!(
            result.is_ok(),
            "relayout_block_snapshot must fall back to layout_full on scale dirty"
        );
        assert!(engine.has_full_layout());
    }
}
