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

    /// Restrict render culling to the content-space band `[top, top + height]`
    /// instead of the viewport-derived window, without moving glyph positions or
    /// hit-testing. `None` restores the default. See
    /// [`text_typeset::DocumentFlow::set_render_window`].
    pub fn set_render_window(&mut self, window: Option<(f32, f32)>) {
        self.flow.set_render_window(window);
    }

    /// The active render window, if any.
    pub fn render_window(&self) -> Option<(f32, f32)> {
        self.flow.render_window()
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

    /// Show several carets at once (multi-caret editing).
    ///
    /// Replaces the whole cursor set, so pass every caret each time — a lone
    /// primary caret is `set_cursor`, which is the same thing with one entry.
    pub fn set_cursors(&mut self, cursors: &[CursorDisplay]) {
        self.flow.set_cursors(cursors);
    }

    // --- Streaming buffers (log / console views) --------------------------

    /// Append one block to the tail of the existing layout.
    ///
    /// The incremental alternative to re-laying-out after content grows: a
    /// full layout is O(N) in the whole document, so appending one line to a
    /// 100 000-line buffer costs over a second, while this stays flat at the
    /// cost of shaping the one new line. A view tailing output needs it; an
    /// editor does not.
    ///
    /// Returns [`ScaleDirty`](text_typeset::RelayoutError::ScaleDirty) if the
    /// layout was shaped at a different HiDPI scale than the service now
    /// reports — appending at the new scale would leave the flow permanently
    /// mixed-scale. Re-run [`layout_full`](Self::layout_full) first.
    ///
    /// Honours [`set_typography_defaults`](Self::set_typography_defaults) on
    /// the block's unset fields, exactly as `layout_full` does — otherwise a
    /// view that laid out its first screen with `layout_full` and grew with
    /// this would shape the two halves in different fonts.
    pub fn append_block(
        &mut self,
        params: &text_typeset::layout::block::BlockLayoutParams,
    ) -> Result<(), text_typeset::RelayoutError> {
        let filled = self.fill_defaults(params);
        let mut bridge = self.shared.borrow_mut();
        self.flow.add_block(bridge.service_mut(), &filled)
    }

    /// Apply this engine's typography defaults to a block's unset fields,
    /// borrowing the caller's params untouched when no default could apply.
    fn fill_defaults<'p>(
        &self,
        params: &'p text_typeset::layout::block::BlockLayoutParams,
    ) -> std::borrow::Cow<'p, text_typeset::layout::block::BlockLayoutParams> {
        if typography_defaults::needs_params_fill(&self.typography_defaults) {
            let mut owned = params.clone();
            typography_defaults::apply_to_block_params(&mut owned, &self.typography_defaults);
            std::borrow::Cow::Owned(owned)
        } else {
            std::borrow::Cow::Borrowed(params)
        }
    }

    /// Drop the first `n` blocks, returning how many were removed.
    ///
    /// The eviction half of a bounded streaming buffer. Survivors keep their
    /// absolute `y` and `content_height` is unchanged, so nothing below moves
    /// and the viewport stays where the user put it — the vacated band at the
    /// top simply becomes empty.
    pub fn remove_leading(&mut self, n: usize) -> usize {
        self.flow.remove_leading(n)
    }

    /// Shape only `window` of a much larger uniform-row document, placing each
    /// row at `y = index * row_height`.
    ///
    /// Where [`append_block`](Self::append_block) makes *growing* a buffer
    /// cheap, this makes *holding* a large one cheap: a resident shaped line
    /// costs ~6.5 KB, so a fully laid-out 100 000-line buffer costs ~623 MB
    /// against ~1 MB for a viewport-sized window. Rendering already culls to
    /// the viewport, so shaping the remainder only ever cost memory.
    ///
    /// Correct only for genuinely uniform rows — one row = one unwrapped
    /// visual line of exactly `row_height`, one font size, no per-row margins
    /// (log/console output, monospaced code). Prose must use
    /// [`layout_full`](Self::layout_full). `window` must be sorted ascending by
    /// index. Both are checked in debug builds.
    ///
    /// Drops any paint overlay, like a full layout does — re-apply highlight
    /// spans after re-windowing.
    ///
    /// Honours [`set_typography_defaults`](Self::set_typography_defaults) on
    /// each row's unset fields, exactly as `layout_full` does. This is not
    /// cosmetic here: a `line_height` default applied to some rows and not
    /// others would break the uniform-row invariant this method's arithmetic
    /// placement depends on.
    pub fn layout_window(
        &mut self,
        window: &[(usize, text_typeset::layout::block::BlockLayoutParams)],
        total_rows: usize,
        row_height: f32,
    ) {
        let filled: std::borrow::Cow<[(usize, text_typeset::layout::block::BlockLayoutParams)]> =
            if typography_defaults::needs_params_fill(&self.typography_defaults) {
                let mut owned = window.to_vec();
                for (_, params) in &mut owned {
                    typography_defaults::apply_to_block_params(params, &self.typography_defaults);
                }
                std::borrow::Cow::Owned(owned)
            } else {
                std::borrow::Cow::Borrowed(window)
            };
        let mut bridge = self.shared.borrow_mut();
        self.flow
            .layout_window(bridge.service_mut(), &filled, total_rows, row_height);
    }

    /// Declare the document's total extent without shaping anything.
    ///
    /// Keeps the scrollbar honest when the row count changes outside the shaped
    /// window — a line appended while the user is scrolled away from the tail.
    /// Only meaningful for a flow driven by [`layout_window`](Self::layout_window).
    pub fn set_uniform_extent(&mut self, total_rows: usize, row_height: f32) {
        self.flow.set_uniform_extent(total_rows, row_height);
    }

    /// Shape a window of a large uniform-row document from document block
    /// snapshots, optionally tinting each row's text.
    ///
    /// The document-driven counterpart of [`layout_window`](Self::layout_window):
    /// where that takes already-built `BlockLayoutParams`, this takes the
    /// `(row index, block snapshot, optional whole-line colour)` a streaming log
    /// or console view has on hand — a caller two crates up cannot build the
    /// params itself (they need this flow's bridge options), so it hands over
    /// snapshots and lets the engine convert them exactly as
    /// [`layout_full`](Self::layout_full) would, applying this engine's
    /// typography defaults so the two paths shape a given block identically.
    ///
    /// The tint sets every fragment's foreground colour — the whole-line
    /// severity colour a log wants (an error line red); `None` leaves the row in
    /// its document colours. Per-run colouring is out of scope here (use the
    /// document's own highlight sessions). `rows` must be sorted ascending by
    /// index. Like every windowed layout this reports `content_height` as
    /// `total_rows * row_height`, so the scrollbar spans the whole document
    /// even though only the window is shaped.
    pub fn layout_window_from_snapshots(
        &mut self,
        rows: &[(usize, text_document::BlockSnapshot, Option<[f32; 4]>)],
        total_rows: usize,
        row_height: f32,
    ) {
        let fill = typography_defaults::needs_params_fill(&self.typography_defaults);
        let window: Vec<(usize, text_typeset::layout::block::BlockLayoutParams)> = rows
            .iter()
            .map(|(idx, snap, tint)| {
                let mut params = self.flow.block_params_for(snap);
                if fill {
                    typography_defaults::apply_to_block_params(
                        &mut params,
                        &self.typography_defaults,
                    );
                }
                if let Some(color) = tint {
                    for fragment in &mut params.fragments {
                        fragment.foreground_color = Some(*color);
                    }
                }
                (*idx, params)
            })
            .collect();
        let mut bridge = self.shared.borrow_mut();
        self.flow
            .layout_window(bridge.service_mut(), &window, total_rows, row_height);
    }

    /// Visual position and height of a laid-out block.
    ///
    /// Answers only for *resident* blocks: under
    /// [`layout_window`](Self::layout_window) everything outside the window is
    /// unshaped and returns `None`, so derive off-window geometry
    /// arithmetically from the row height instead.
    pub fn block_visual_info(&self, block_id: usize) -> Option<text_typeset::BlockVisualInfo> {
        self.flow.block_visual_info(block_id)
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

    /// Reading direction of the text at `position` — the direction of
    /// the bidi run the caret sits in, not the paragraph's.
    ///
    /// Arrow keys need this: stepping one character *forward logically*
    /// moves the caret visually left inside right-to-left text, so a
    /// handler that maps ArrowRight straight to "next character" sends
    /// the caret backwards on screen.
    pub fn direction_at(&self, position: usize) -> text_typeset::TextDirection {
        self.flow.direction_at(position)
    }

    /// Base direction of the paragraph containing `position`.
    ///
    /// Home and End want this rather than [`Self::direction_at`]: they
    /// move to the *logical* ends of the line, and which visual edge
    /// those sit on is a property of the paragraph.
    pub fn paragraph_direction_at(&self, position: usize) -> text_typeset::TextDirection {
        self.flow.paragraph_direction_at(position)
    }

    /// Document positions of the logical start and end of the visual
    /// line containing `position` — what Home and End move to.
    ///
    /// Logical, not visual: in a right-to-left paragraph the start is
    /// drawn on the right. `affinity` disambiguates a soft-wrap
    /// boundary, where one position ends one line and starts the next.
    pub fn visual_line_range_at(
        &self,
        position: usize,
        affinity: text_typeset::CursorAffinity,
    ) -> Option<(usize, usize)> {
        self.flow.visual_line_range_at(position, affinity)
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

    // --- Streaming passthroughs -------------------------------------

    /// One row of a streaming buffer, as a log view builds them: a single
    /// unwrapped line with no margins, which is the uniform-row invariant
    /// `layout_window` relies on.
    fn row(block_id: usize, text: &str) -> text_typeset::layout::block::BlockLayoutParams {
        use text_typeset::layout::block::{BlockLayoutParams, FragmentParams};
        use text_typeset::layout::paragraph::Alignment;
        use text_typeset::{UnderlineStyle, VerticalAlignment};

        BlockLayoutParams {
            base_direction: Default::default(),
            block_id,
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
                underline_style: UnderlineStyle::None,
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
                vertical_alignment: VerticalAlignment::Normal,
                image_name: None,
                image_width: 0.0,
                image_height: 0.0,
                features: Vec::new(),
            }],
            alignment: Alignment::Left,
            // No margins, no wrapping: one row is exactly one visual line,
            // which is what `layout_window`'s arithmetic placement requires.
            top_margin: 0.0,
            bottom_margin: 0.0,
            left_margin: 0.0,
            right_margin: 0.0,
            text_indent: 0.0,
            list_marker: String::new(),
            list_indent: 0.0,
            tab_positions: Vec::new(),
            line_height_multiplier: None,
            non_breakable_lines: true,
            hyphenation: None,
            checkbox: None,
            background_color: None,
        }
    }

    /// A streaming view sets a default font once on the engine and then grows
    /// itself with `append_block`. If the append path ignored the default that
    /// `layout_full` honours, the view's first screen and everything appended
    /// after it would render in two different fonts, with nothing reporting it.
    #[test]
    fn append_block_honours_the_engine_typography_defaults() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        engine.set_typography_defaults(EditorTypographyDefaults {
            font_family: Some("Streaming Face".to_string()),
            line_height: 1.5,
            ..EditorTypographyDefaults::default()
        });

        // The row a log view builds: family unset, so the engine's default
        // must fill it — exactly as it fills a `layout_full` snapshot.
        let params = row(1, "streamed line");
        assert!(params.fragments[0].font_family.is_none());
        assert!(params.line_height_multiplier.is_none());

        let filled = engine.fill_defaults(&params);
        assert_eq!(
            filled.fragments[0].font_family.as_deref(),
            Some("Streaming Face"),
            "an appended row must inherit the engine's default family, or the \
             view renders in two fonts"
        );
        assert_eq!(
            filled.line_height_multiplier,
            Some(1.5),
            "an appended row must inherit the default line height — a row that \
             is a different height than the windowed rows breaks the uniform-row \
             invariant that layout_window's placement arithmetic depends on"
        );
    }

    /// A caller that set the family explicitly outranks the engine default —
    /// filling is for *unset* fields, matching `apply_to_block`'s contract.
    #[test]
    fn append_block_does_not_override_an_explicit_font() {
        let mut engine = RichTextEngine::private_default();
        engine.set_typography_defaults(EditorTypographyDefaults {
            font_family: Some("Default Face".to_string()),
            ..EditorTypographyDefaults::default()
        });

        let mut params = row(1, "explicit");
        params.fragments[0].font_family = Some("Chosen Face".to_string());

        let filled = engine.fill_defaults(&params);
        assert_eq!(
            filled.fragments[0].font_family.as_deref(),
            Some("Chosen Face"),
            "an explicitly-set family must win over the engine default"
        );
    }

    /// With no defaults set the params are passed through untouched — the
    /// streaming path must not pay a clone per appended line for nothing.
    #[test]
    fn append_block_does_not_clone_when_no_default_applies() {
        let engine = RichTextEngine::private_default();
        let params = row(1, "untouched");
        assert!(
            matches!(engine.fill_defaults(&params), std::borrow::Cow::Borrowed(_)),
            "a default-free engine must borrow the caller's params, not clone \
             them once per appended line"
        );
    }

    /// The whole point of the append path: growing the buffer must not re-shape
    /// what is already in it, so the flow keeps every earlier row.
    #[test]
    fn append_block_extends_the_layout_without_rebuilding_it() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        let doc = TextDocument::new();
        doc.set_plain_text("first").unwrap();
        engine.layout_full(&doc.snapshot_flow());
        let height_before = engine.content_height();

        engine.append_block(&row(999, "streamed line")).unwrap();

        assert!(
            engine.content_height() > height_before,
            "the appended row must add height"
        );
        assert!(
            engine.block_visual_info(999).is_some(),
            "the appended row must be laid out and locatable"
        );
    }

    /// Appending at a scale the layout was not shaped at would leave the flow
    /// permanently mixed-scale, and — worse — stamping it as freshly laid out
    /// would clear the caller's own staleness signal.
    #[test]
    fn append_block_refuses_a_stale_scale() {
        let shared = SharedTypesetter::new_with_default_font();
        let mut engine = RichTextEngine::from_shared(shared.clone());
        engine.set_viewport(400.0, 300.0);
        let doc = TextDocument::new();
        doc.set_plain_text("first").unwrap();
        engine.layout_full(&doc.snapshot_flow());

        shared.set_scale_factor(2.0);

        assert!(
            matches!(
                engine.append_block(&row(999, "streamed")),
                Err(text_typeset::RelayoutError::ScaleDirty)
            ),
            "appending against a stale scale must be refused, not silently mixed"
        );
    }

    /// Windowing is what keeps a large buffer affordable: only the window is
    /// shaped, yet the flow still spans the whole document so the scrollbar
    /// stays honest.
    #[test]
    fn layout_window_shapes_only_the_window_but_spans_the_document() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);

        // Learn the true row height by shaping one row, as a caller must:
        // `layout_window` asserts the height it is told matches what the row
        // actually lays out to, so guessing it is not an option.
        engine.append_block(&row(1, "probe")).unwrap();
        let row_height = engine.block_visual_info(1).expect("probe row").height;

        let window: Vec<_> = (500..510).map(|i| (i, row(i + 1, "line"))).collect();
        engine.layout_window(&window, 100_000, row_height);

        assert!(
            engine.block_visual_info(501).is_some(),
            "a row inside the window must be laid out"
        );
        assert!(
            engine.block_visual_info(1).is_none(),
            "a row outside the window must not be resident"
        );
        assert!(
            (engine.content_height() - 100_000.0 * row_height).abs() < 1.0,
            "content_height must span all 100k rows, not just the window"
        );
    }

    /// The append/evict cycle a capped log view actually runs, composed through
    /// this wrapper rather than the engine underneath it.
    #[test]
    fn append_and_evict_compose_into_a_bounded_buffer() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        let doc = TextDocument::new();
        doc.set_plain_text("line 0").unwrap();
        engine.layout_full(&doc.snapshot_flow());

        for i in 1..20 {
            engine.append_block(&row(1000 + i, "line")).unwrap();
        }
        let evicted = engine.remove_leading(5);

        assert_eq!(evicted, 5, "eviction must report what it actually removed");
        assert!(
            engine.block_visual_info(1001).is_none(),
            "an evicted row must be gone"
        );
        assert!(
            engine.block_visual_info(1019).is_some(),
            "a surviving row must remain laid out"
        );
    }

    /// The document-driven windowing a `LogView` uses: hand the engine block
    /// snapshots for the visible rows and it shapes the window, spanning the
    /// whole document for the scrollbar. A caller two crates up can't build the
    /// params, so it passes what it has — snapshots.
    #[test]
    fn layout_window_from_snapshots_shapes_the_window_from_document_blocks() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);

        // A document standing in for a long log; window three of its blocks.
        let doc = TextDocument::new();
        doc.set_plain_text("alpha\nbeta\ngamma\ndelta\nepsilon")
            .unwrap();

        // Learn the row height by shaping one row (as the widget must).
        engine.append_block(&row(1, "probe")).unwrap();
        let row_height = engine.block_visual_info(1).expect("probe row").height;

        let rows: Vec<(usize, text_document::BlockSnapshot, Option<[f32; 4]>)> = (0..3)
            .map(|i| {
                let blk = doc.block_by_number(i).expect("block");
                (i, blk.snapshot(), None)
            })
            .collect();
        engine.layout_window_from_snapshots(&rows, 5, row_height);

        assert!(
            (engine.content_height() - 5.0 * row_height).abs() < 1.0,
            "content_height must span the whole document, not just the window"
        );
        // The probe row was outside the window, so it must not survive.
        assert!(
            engine.block_visual_info(1).is_none(),
            "windowing drops rows outside the window"
        );
    }

    /// A per-row tint must reach the shaped glyphs: the whole-line severity
    /// colour a log paints an error line with.
    #[test]
    fn layout_window_from_snapshots_tints_a_row() {
        let shared = SharedTypesetter::new_with_default_font();
        let mut engine = RichTextEngine::from_shared(shared);
        engine.set_viewport(400.0, 300.0);
        let doc = TextDocument::new();
        doc.set_plain_text("ERROR boom").unwrap();

        engine.append_block(&row(1, "probe")).unwrap();
        let row_height = engine.block_visual_info(1).expect("probe row").height;

        let red = [1.0, 0.0, 0.0, 1.0];
        let blk = doc.block_by_number(0).expect("block");
        engine.layout_window_from_snapshots(&[(0, blk.snapshot(), Some(red))], 1, row_height);

        // Every glyph of the tinted row must carry the override colour.
        let all_red = engine.with_render_frame(|frame| {
            !frame.glyphs.is_empty() && frame.glyphs.iter().all(|g| g.color == red)
        });
        assert!(all_red, "the tint must colour every glyph of the row");
    }

    /// Multi-caret rendering: every caret must be shown, not just the primary.
    #[test]
    fn set_cursors_renders_every_caret() {
        let mut engine = RichTextEngine::private_default();
        engine.set_viewport(400.0, 300.0);
        let doc = TextDocument::new();
        doc.set_plain_text("hello world").unwrap();
        engine.layout_full(&doc.snapshot_flow());

        let caret = |p: usize| CursorDisplay {
            position: p,
            anchor: p,
            affinity: text_typeset::CursorAffinity::Downstream,
            visible: true,
            selected_cells: Vec::new(),
        };
        engine.set_cursors(&[caret(0), caret(3), caret(6)]);

        let carets = engine.with_render_frame(|frame| {
            frame
                .decorations
                .iter()
                .filter(|d| matches!(d.kind, text_typeset::DecorationKind::Cursor))
                .count()
        });
        assert_eq!(carets, 3, "all three carets must render");
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
