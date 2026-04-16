//! Rich text editor widget. Feature-gated behind the `rich-text` feature.
//!
//! See [`§27.10` of the architecture doc](../../../../../docs/fern-ui-architecture.md)
//! for the design rationale. This crate ships `RichTextEditor` with two
//! construction presets — M8a provides [`RichTextEditor::read_only`]
//! (view documents, select/copy, click links). M8b will add
//! [`RichTextEditor::editor`] (full editing).
//!
//! The widget owns its own `fern_text::RichTextEngine` (per-widget
//! typesetter — see gap 5 of the plan), subscribes to document events
//! via `on_change` so multiple editors can share a `TextDocument` like
//! QTextEdit views, and drives its own scroll bars outside of
//! `ScrollArea` to break the wrap/scrollbar circular dependency of
//! §27.10.5.
//!
//! Constructors: [`RichTextEditor::read_only`] (hidden caret, filter
//! rejects mutations, accessibility role `Document`) and
//! [`RichTextEditor::editor`] (blinking caret, full command filter,
//! role `MultilineTextInput`, `SetValue` action declared). Both
//! widgets subscribe to `TextDocument::on_change` independently so
//! any number of editors / viewers can share a document and observe
//! each other's edits — see gap 10 of the plan.
//!
//! This file owns the struct, its builder methods and signal
//! accessors, `Widget` trait impl (`build` / `size_that_fits` /
//! `place_children` / `paint` / `accessibility`), and the shared
//! `sync_cursor_signals` helper used by both `keyboard` and `mouse`
//! dispatch modules. Key / pointer / gesture handlers live in
//! [`keyboard`] and [`mouse`]; the frame-tick loop lives in
//! [`frame_loop`]; clipboard actions in [`clipboard`].

mod clipboard;
mod frame_loop;
mod hit_test;
pub(crate) mod image_cache;
mod keyboard;
mod mouse;
pub(crate) mod paint;
mod policy;
mod state;

#[cfg(test)]
mod tests;

pub use hit_test::ContextTarget;
pub use policy::{
    AccessibilityRole, CaretPolicy, ClipboardPolicy, CommandFilter, EditCommandKind,
    PolicyBundle, EDITOR_PRESET, READ_ONLY_PRESET,
};

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_text::text_document::{SelectionType, TextDocument, TextFormat};
use fern_text::{FontRegistrar, RichTextEngine, SharedTypesetter, WrapMode};
use fern_tokens::Color;

use self::paint::{PaintParams, paint_frame};
use self::state::{EditorState, SharedState};

/// Scrollbar visibility policy, applied independently per axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPolicy {
    /// Visible only when the corresponding `max_scroll_axis > 0`.
    Auto,
    /// Always visible (reserves gutter width even when content fits).
    AlwaysOn,
    /// Never rendered. Useful when embedding the editor in an outer
    /// scroll container, or in tests.
    AlwaysOff,
}

impl Default for ScrollPolicy {
    fn default() -> Self {
        Self::Auto
    }
}

/// The main rich text widget. Construct via [`RichTextEditor::read_only`]
/// (M8a) or [`RichTextEditor::editor`] (M8b, currently stubbed as
/// `unimplemented!`).
pub struct RichTextEditor {
    state: SharedState,
    v_scroll_policy: ScrollPolicy,
    h_scroll_policy: ScrollPolicy,
}

impl std::fmt::Debug for RichTextEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichTextEditor")
            .field("policy", &self.state.borrow().policy)
            .finish_non_exhaustive()
    }
}

impl RichTextEditor {
    /// Construct a read-only rich text viewer bound to `document`. The
    /// document can also back an editable `RichTextEditor::editor` in
    /// another part of the UI — both widgets receive document events
    /// independently via `on_change` subscriptions.
    pub fn read_only(document: TextDocument) -> Self {
        Self::construct(document, READ_ONLY_PRESET)
    }

    /// Construct an editable rich text editor bound to `document`.
    /// Uses the full editor preset: every command accepted, caret
    /// blinks, `MultilineTextInput` accessibility role, full clipboard
    /// support. Multiple editors on the same document share live edits
    /// via per-widget `on_change` subscriptions — see §27.10.1 of the
    /// architecture doc.
    pub fn editor(document: TextDocument) -> Self {
        Self::construct(document, EDITOR_PRESET)
    }

    fn construct(document: TextDocument, policy: PolicyBundle) -> Self {
        // Start with a private engine. `build()` swaps it for one that
        // shares the application's `SharedTypesetter` when one is
        // reachable via `ctx.app_state`, so rendered glyphs land in
        // the atlas that fern-render actually uploads to the GPU.
        // Outside a windowed fern-app (headless tests) the private
        // engine is correct: no renderer is ever invoked.
        let mut engine = RichTextEngine::private_default();
        engine.set_wrap_mode(WrapMode::Word);
        let state = EditorState::new(document, engine, policy, WrapMode::Word);
        Self {
            state,
            v_scroll_policy: ScrollPolicy::Auto,
            h_scroll_policy: ScrollPolicy::Auto,
        }
    }

    // --- Builder methods ------------------------------------------------

    pub fn wrap_mode(self, mode: WrapMode) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.wrap_mode = mode;
            st.engine.set_wrap_mode(mode);
            st.needs_full_layout = true;
        }
        self
    }

    pub fn zoom(self, zoom: f32) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.engine.set_zoom(zoom);
            st.needs_full_layout = true;
        }
        self
    }

    pub fn selection_color(self, color: Color) -> Self {
        self.state
            .borrow_mut()
            .engine
            .set_selection_color(color.to_array());
        self
    }

    pub fn caret_color(self, color: Color) -> Self {
        self.state
            .borrow_mut()
            .engine
            .set_cursor_color(color.to_array());
        self
    }

    pub fn text_color(self, color: Color) -> Self {
        self.state
            .borrow_mut()
            .engine
            .set_text_color(color.to_array());
        self
    }

    pub fn v_scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.v_scroll_policy = policy;
        self
    }

    pub fn h_scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.h_scroll_policy = policy;
        self
    }

    pub fn scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.v_scroll_policy = policy;
        self.h_scroll_policy = policy;
        self
    }

    /// Install a custom font registrar for the fallback private
    /// engine. Only has effect when the editor is built outside a
    /// windowed fern-app — once `build()` sees a `SharedTypesetter`
    /// in `app_state`, the private engine is replaced with one that
    /// shares the app's typesetter and this registrar is ignored.
    pub fn font_registrar(self, registrar: &dyn FontRegistrar) -> Self {
        {
            let mut st = self.state.borrow_mut();
            let mut engine = RichTextEngine::private_with_registrar(registrar);
            engine.set_wrap_mode(st.wrap_mode);
            st.engine = engine;
            st.needs_full_layout = true;
        }
        self
    }

    // --- Observable signals ---------------------------------------------

    pub fn document_version(&self) -> Signal<u64> {
        self.state.borrow().document_version.clone()
    }

    /// Current cursor position in the document, in character units.
    /// Exposed for tests and for applications that need to mirror the
    /// caret position externally (status bar, outline panel, etc.).
    pub fn cursor_position(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    /// Current selection anchor (equal to `cursor_position` when there
    /// is no selection).
    pub fn cursor_anchor(&self) -> usize {
        self.state.borrow().cursor.anchor()
    }

    /// Reactive cursor position signal. Observers fire whenever the
    /// cursor moves (arrow keys, click, Home/End, …). Useful for
    /// status bars and tests.
    pub fn cursor_position_signal(&self) -> Signal<usize> {
        self.state.borrow().cursor_position.clone()
    }

    /// Reactive selection anchor signal.
    pub fn cursor_anchor_signal(&self) -> Signal<usize> {
        self.state.borrow().cursor_anchor.clone()
    }

    pub fn has_selection(&self) -> Signal<bool> {
        self.state.borrow().has_selection.clone()
    }

    /// Reactive undo-availability signal, suitable for toolbar button
    /// enable-state. Updated through the frame loop's debounce drain
    /// so toolbars don't flicker during rapid editing.
    pub fn can_undo(&self) -> Signal<bool> {
        self.state.borrow().can_undo.clone()
    }

    /// Reactive redo-availability signal.
    pub fn can_redo(&self) -> Signal<bool> {
        self.state.borrow().can_redo.clone()
    }

    /// Read the current character format at the widget's caret.
    /// Used by toolbars that mirror bold/italic/underline state, and
    /// by tests — `TextDocument::cursor()` creates a fresh cursor
    /// each call, so reading format through the document would miss
    /// the widget's internal caret position.
    pub fn caret_char_format(&self) -> TextFormat {
        self.state
            .borrow()
            .cursor
            .char_format()
            .unwrap_or_default()
    }

    /// Clone the internal shared state handle for test observation.
    /// Tests take this before `tree.add(editor)` moves the widget
    /// into the arena, so they can read the widget's live cursor,
    /// signal state, and debounce fields through the very same
    /// `Rc<RefCell<EditorState>>` that the arena-stored editor is
    /// mutating.
    #[cfg(test)]
    #[doc(hidden)]
    pub(crate) fn state_handle(&self) -> SharedState {
        self.state.clone()
    }

    pub fn scroll_y(&self) -> Signal<f32> {
        self.state.borrow().scroll_y.clone()
    }

    pub fn scroll_x(&self) -> Signal<f32> {
        self.state.borrow().scroll_x.clone()
    }

    // --- Context-menu support (external menus) --------------------------

    /// Classify what is under `point` in the widget's local coordinates
    /// (origin at the widget's top-left, scroll offset and zoom handled
    /// internally by the typesetter), for applications building an
    /// external context menu. Returns `None` if the point does not
    /// land on any hit region.
    pub fn context_target_at(&self, point: Point) -> Option<hit_test::ContextTarget> {
        let st = self.state.borrow();
        let hit = hit_test::hit_test_at(&st.engine, point, 0.0, 0.0)?;
        let selection = Some((st.cursor.anchor(), st.cursor.position()));
        Some(hit_test::classify(&hit, selection, &st.document))
    }

    // --- Selection helpers (allowed under both presets) -----------------

    /// Currently selected text, or an empty string if nothing is selected.
    pub fn selected_text(&self) -> String {
        self.state
            .borrow()
            .cursor
            .selected_text()
            .unwrap_or_default()
    }

    /// Select the entire document programmatically. Equivalent to
    /// the final step of the Ctrl+A ladder; resets the ladder state
    /// so a subsequent Ctrl+A starts fresh at level 1.
    pub fn select_all(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.cursor.select(SelectionType::Document);
            st.select_all_level = 0;
            st.select_all_anchor_cell = None;
        }
        sync_cursor_signals(&self.state);
    }

    /// Clear any current selection.
    pub fn deselect(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.cursor.clear_selection();
            st.select_all_level = 0;
            st.select_all_anchor_cell = None;
        }
        sync_cursor_signals(&self.state);
    }
}

impl Widget for RichTextEditor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Swap the private fallback engine for one that shares the
        // application's `SharedTypesetter` so rendered glyphs end up
        // in the atlas fern-render uploads to the GPU. Headless tests
        // without a `SharedTypesetter` in app_state keep the private
        // engine untouched.
        if let Some(shared) = ctx.app_state::<SharedTypesetter>() {
            let mut st = self.state.borrow_mut();
            let wrap = st.wrap_mode;
            let mut engine = RichTextEngine::from_shared(shared.clone());
            engine.set_wrap_mode(wrap);
            st.engine = engine;
            st.needs_full_layout = true;
        }

        // Bind `caret_visible` to the framework's repaint tracker so
        // that every toggle in the frame-tick effect marks this
        // widget `needs_paint`. Without this the cached paint frame
        // is reused on subsequent redraws and the caret never
        // visibly changes state even though the Signal flips.
        // Skipped for `CaretPolicy::Hidden` — no caret means no
        // repaint reason and we save per-frame work on pure viewers.
        {
            let st = self.state.borrow();
            let caret_policy = st.policy.caret_policy;
            let caret_visible = st.caret_visible.clone();
            drop(st);
            if caret_policy != CaretPolicy::Hidden {
                let self_id = ctx.self_id();
                caret_visible.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    fern_core::binding::BindingLevel::RepaintOnly,
                );
            }
        }

        // Bind document_version at `BindingLevel::AccessibilityOnly`
        // so any text or format edit (which bumps document_version
        // inside `drain_events`) automatically flips the tree's
        // `a11y_dirty` flag during `process_state_changes`. Without
        // this binding, screen readers only see updated text when
        // an unrelated event (focus change, window resize) happens
        // to mark the a11y tree dirty. See the RichTextEditor
        // accessibility plan for details.
        {
            let st = self.state.borrow();
            let document_version = st.document_version.clone();
            drop(st);
            let self_id = ctx.self_id();
            document_version.bind_to(
                self_id,
                ctx.binding_registry(),
                fern_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // Stash the tree's frame-request handle on the state so the
        // frame-tick effect can self-chain (caret blink, drag
        // auto-scroll) without mutable access to the tree.
        {
            let mut st = self.state.borrow_mut();
            st.frame_request = Some(ctx.frame_request_handle());
            st.frame_wake_at = Some(ctx.wake_at_handle());
        }

        // Ask for one frame so the initial layout / paint runs through
        // the tick path and populates max_scroll / content metrics.
        ctx.request_frame();

        // Frame-tick effect: runs only on frames the tree was asked
        // to pump. `frame_loop::tick` returns `true` while there's
        // more work pending (document events draining, caret blink
        // active) — we re-arm the tree's frame-request flag so the
        // next layout pass runs the effect again.
        {
            let state = self.state.clone();
            let tick_signal = ctx.frame_tick();
            ctx.effect(&tick_signal, move |delta| {
                let mut st = state.borrow_mut();
                let more = frame_loop::tick(&mut st, *delta);
                st.has_selection.set(st.cursor.has_selection());
                if more {
                    if let Some(handle) = &st.frame_request {
                        handle.set(true);
                    }
                }
                drop(st);
            });
        }

        // Attach handlers:
        // * `on_pointer_event`: PointerDown for caret placement and
        //   drag-select start, PointerMove for drag extension and
        //   auto-scroll velocity, PointerUp for drag teardown. Returns
        //   `Ignored` on Down/Up so the gesture arena also processes
        //   the event and the double/triple tap recognizers see every
        //   press. Handled on Move during an active drag.
        // * `on_scroll`: mouse wheel / trackpad.
        // * `on_key`: arrow navigation, Home/End (line + document),
        //   PageUp/PageDown, Enter, Backspace, Delete, Ctrl+Backspace
        //   / Ctrl+Delete word deletion, Ctrl+B/I/U formatting,
        //   Ctrl+Z/Y/Shift+Z undo/redo, Ctrl+C/X/V clipboard,
        //   Ctrl+A with table-aware escalation ladder, printable
        //   characters into `pending_chars` for frame-start batch
        //   insertion, IME commit.
        // * `on_double_tap` / `on_triple_tap`: word / paragraph
        //   selection via cooperative gesture recognizers in
        //   `fern-core::gesture`. The single-click caret placement
        //   is handled by `on_pointer_event::PointerDown` above
        //   because mouse-down semantics demand immediate response,
        //   which `on_tap` (fires on release) would violate.
        // * `on_focus`: mirror `has_focus` onto the editor state so
        //   `paint()` and `frame_loop::tick` can gate the caret.
        let mut handlers = HandlerSet::new();
        handlers = handlers
            .focusable(true)
            .cursor(CursorIcon::Text)
            .on_focus({
                let state = self.state.clone();
                move |gained, ctx| {
                    let mut st = state.borrow_mut();
                    st.has_focus = gained;
                    if gained && matches!(st.policy.caret_policy, CaretPolicy::Blinking) {
                        // Reset the blink phase to "now" so the caret
                        // pops on immediately and the first off-toggle
                        // happens exactly one interval later. Skipped
                        // for `Hidden` — no caret means no blink, and
                        // we avoid a spurious `caret_visible` signal
                        // update on focus gain.
                        st.blink_last_toggle = Some(std::time::Instant::now());
                        st.caret_visible.set(true);
                    }
                    drop(st);
                    ctx.request_frame();
                }
            })
            .on_pointer_event({
                let state = self.state.clone();
                move |event, ctx| self::mouse::handle_pointer_event(&state, event, ctx)
            })
            .on_scroll({
                let state = self.state.clone();
                move |event, ctx| self::mouse::handle_scroll(&state, event, ctx)
            })
            .on_key({
                let state = self.state.clone();
                move |event, ctx| self::keyboard::handle_key(&state, event, ctx)
            })
            .on_double_tap({
                let state = self.state.clone();
                move |pos, ctx| self::mouse::handle_double_tap(&state, pos, ctx)
            })
            .on_triple_tap({
                let state = self.state.clone();
                move |pos, ctx| self::mouse::handle_triple_tap(&state, pos, ctx)
            })
            .on_access_action_request({
                let state = self.state.clone();
                move |action, target_node, data, ctx| {
                    handle_access_action_request(&state, action, target_node, data, ctx)
                }
            });

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // Greedy: consume the full proposal. Intrinsic sizing for
        // `ScrollPolicy::AlwaysOff` is an M8b refinement.
        let w = proposal.width.unwrap_or(200.0).max(0.0);
        let h = proposal.height.unwrap_or(100.0).max(0.0);
        Size::new(w, h)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Record the viewport for the frame loop to read next tick.
        // The rich text editor is currently a leaf — scroll bar
        // siblings (§27.10.5's "scrollbars outside ScrollArea" trick)
        // are a future addition. For now `paint()` is the only
        // place `viewport_width / height` get reliably refreshed
        // (since leaf widgets may not call `place_children`), and
        // this path is the fallback when a parent does call us.
        let mut st = self.state.borrow_mut();
        st.viewport_width = bounds.width;
        st.viewport_height = bounds.height;
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let mut st = self.state.borrow_mut();

        // The engine reads the HiDPI display scale factor from the
        // shared `TypesetterBridge` on every `layout_full`, exactly
        // like `TextWidget` does internally. No widget-side plumbing
        // — this is a render-pipeline concern, invisible to the
        // widget author.

        // Bug-fix: `place_children` is only called when a widget has
        // children, so for the M8a leaf editor the viewport was never
        // recorded on the state. Pull it from the paint bounds now,
        // flag a relayout if it changed, and update the typesetter.
        // Also record the widget's origin in window space — event
        // handlers (click / hit-test) subtract this from the incoming
        // pointer position to obtain widget-local coordinates.
        st.viewport_origin = Point::new(bounds.x, bounds.y);
        let viewport_changed = (st.viewport_width - bounds.width).abs() > 0.5
            || (st.viewport_height - bounds.height).abs() > 0.5;
        if viewport_changed {
            st.viewport_width = bounds.width;
            st.viewport_height = bounds.height;
            st.engine.set_viewport(bounds.width, bounds.height);
            st.needs_full_layout = true;
        }

        // First-frame guard + viewport-change guard: (re)run the
        // full layout so the render call produces glyphs sized
        // for the current bounds. With per-widget `DocumentFlow`
        // state inside the engine, `has_full_layout()` only
        // reports `false` when this widget has never laid out
        // or when the shared service's HiDPI scale factor has
        // changed since the last layout — there is no
        // cross-widget trampling left to guard against.
        if st.needs_full_layout || !st.engine.has_full_layout() {
            let flow = st.document.snapshot_flow();
            st.engine.layout_full(&flow);
            st.needs_full_layout = false;
            st.content_dirty = true;
        }

        // Update the cursor display every paint so selection
        // highlights follow the caret without needing a frame tick.
        let caret_on_now = match st.policy.caret_policy {
            CaretPolicy::Hidden => false,
            CaretPolicy::StaticVisible => st.has_focus,
            CaretPolicy::Blinking => st.caret_visible.get() && st.has_focus,
        };
        let cursor_display = fern_text::CursorDisplay {
            position: st.cursor.position(),
            anchor: st.cursor.anchor(),
            visible: caret_on_now,
            selected_cells: Vec::new(),
        };
        st.engine.set_cursor(&cursor_display);

        // Forward the widget's scroll state to the typesetter so
        // viewport culling knows where the visible window is. text-
        // typeset's `render()` only emits glyphs whose flow Y falls
        // inside `[scroll_offset, scroll_offset + viewport_height]`,
        // and the emitted screen coordinates already have
        // `scroll_offset` subtracted — so the paint walker doesn't
        // apply any further offset beyond the widget origin.
        let scroll_y_logical = st.scroll_y.get();
        st.engine.set_scroll_offset(scroll_y_logical);

        // Clip to bounds so overflowing glyphs don't bleed into siblings.
        canvas.set_clip(bounds);

        // Split-borrow the state fields so the paint walker can hold
        // `&engine.with_render_frame(...)`, `&document`, and
        // `&mut image_cache` simultaneously.
        let state_ref: &mut EditorState = &mut *st;
        let EditorState {
            ref mut engine,
            ref document,
            ref mut image_cache,
            ..
        } = *state_ref;
        engine.with_render_frame(|frame| {
            paint_frame(
                canvas,
                PaintParams {
                    frame,
                    origin: Point::new(bounds.x, bounds.y),
                    document,
                    image_cache,
                    draw_caret: caret_on_now,
                },
            );
        });

        canvas.clear_clip();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use self::policy::AccessibilityRole;
        use self::state::SyntheticElementRef;
        use fern_core::accesskit::{Action, NodeId, Role};
        use fern_text::text_document::{FlowElementSnapshot, FragmentContent};

        let st = self.state.borrow();

        let role = match st.policy.access_role {
            AccessibilityRole::Editor => Role::MultilineTextInput,
            AccessibilityRole::Document => Role::Document,
        };
        builder.set_role(role);
        if st.policy.is_read_only() {
            builder.set_read_only();
        }

        // Walk the cached flow snapshot (or rebuild it if the last
        // edit cleared the cache). For each block we emit a
        // Role::Paragraph child (or Role::Heading when the block's
        // heading_level is set), then for each text fragment we
        // emit a Role::TextRun child carrying value,
        // character_lengths, word_starts, and per-character
        // geometry from text-typeset. Widget-local
        // synthetic_to_element map is populated so the on-access
        // handler can convert AccessKit TextSelection back into
        // document-absolute cursor positions.
        let snap = {
            let mut cache = st.accessibility_flow_snapshot.borrow_mut();
            if cache.is_none() {
                *cache = Some(st.document.snapshot_flow());
            }
            cache.as_ref().cloned()
        };

        let user_pos = st.cursor.position();
        let user_anchor = st.cursor.anchor();
        let mut caret_pair: Option<(NodeId, usize)> = None;
        let mut anchor_pair: Option<(NodeId, usize)> = None;
        let mut syn_map: std::collections::HashMap<NodeId, SyntheticElementRef> =
            std::collections::HashMap::new();

        if let Some(snap) = snap {
            for elem in &snap.elements {
                if let FlowElementSnapshot::Block(block) = elem {
                    let para_id = builder.push_paragraph_child(block.block_id as u64);
                    if let Some(level) = block.block_format.heading_level {
                        builder.set_paragraph_as_heading(para_id, level);
                    }
                    for frag in &block.fragments {
                        if let FragmentContent::Text {
                            text,
                            offset,
                            length,
                            element_id,
                            word_starts,
                            ..
                        } = frag
                        {
                            // character_lengths: UTF-8 byte length of each char.
                            // AccessKit indexes by char, each entry is byte count.
                            let char_lengths: Vec<u8> =
                                text.chars().map(|c| c.len_utf8() as u8).collect();

                            // Per-character geometry from text-typeset. char_start
                            // / char_end are block-relative character offsets
                            // (matches LayoutLine::char_range's coordinate space).
                            let char_start = *offset;
                            let char_end = char_start + *length;
                            let geom = st
                                .engine
                                .character_geometry(block.block_id, char_start, char_end);
                            let char_positions: Vec<f32> =
                                geom.iter().map(|g| g.position).collect();
                            let char_widths: Vec<f32> =
                                geom.iter().map(|g| g.width).collect();

                            let node_id = builder.push_text_run_child(
                                para_id,
                                *element_id,
                                *offset,
                                text.clone(),
                                char_lengths,
                                Some(word_starts.clone()),
                                if char_positions.is_empty() {
                                    None
                                } else {
                                    Some(char_positions)
                                },
                                if char_widths.is_empty() {
                                    None
                                } else {
                                    Some(char_widths)
                                },
                            );

                            // Remember where this run lives in the document so
                            // the on-access handler can resolve
                            // SetTextSelection(TextRun NodeId, char_index).
                            let absolute_start = block.position + *offset;
                            syn_map.insert(
                                node_id,
                                SyntheticElementRef {
                                    element_id: *element_id,
                                    absolute_start,
                                    text: text.clone(),
                                },
                            );

                            // Resolve user cursor / anchor to this run if they
                            // fall within its absolute character range
                            // [absolute_start, absolute_start + length].
                            let absolute_end = absolute_start + *length;
                            if user_pos >= absolute_start && user_pos <= absolute_end {
                                let char_idx = char_index_in_text(
                                    text,
                                    user_pos - absolute_start,
                                );
                                caret_pair = Some((node_id, char_idx));
                            }
                            if user_anchor >= absolute_start && user_anchor <= absolute_end {
                                let char_idx = char_index_in_text(
                                    text,
                                    user_anchor - absolute_start,
                                );
                                anchor_pair = Some((node_id, char_idx));
                            }
                        }
                    }
                }
            }
        }

        // Attach the text selection on the editor itself, referencing
        // the appropriate TextRun children. If we couldn't resolve
        // either endpoint (empty document, cursor in no fragment),
        // fall back to a self-targeted selection so screen readers
        // still see *something*.
        if let (Some(a), Some(c)) = (anchor_pair, caret_pair) {
            builder.set_text_selection_to(a, c);
        } else {
            builder.set_text_selection_on_self(user_anchor, user_pos);
        }

        *st.synthetic_to_element.borrow_mut() = syn_map;

        builder.add_action(Action::ScrollIntoView);
        builder.add_action(Action::SetTextSelection);
        if matches!(st.policy.access_role, AccessibilityRole::Editor) {
            builder.add_action(Action::SetValue);
        }
    }

    fn clips_children(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Event handlers — take `&SharedState` so they can be boxed into handler
// closures without borrowing `self`.
// ---------------------------------------------------------------------------

/// Push the current cursor position / anchor / selection flag into
/// the state's reactive signals. Called after every cursor mutation
/// so external observers (status bars, tests) see the change on the
/// next signal propagation. Exported to `keyboard` and `mouse`
/// because every event handler ends with a signal publish.
pub(super) fn sync_cursor_signals(state: &SharedState) {
    let st = state.borrow();
    let pos = st.cursor.position();
    let anc = st.cursor.anchor();
    let has_sel = st.cursor.has_selection();
    let pos_sig = st.cursor_position.clone();
    let anc_sig = st.cursor_anchor.clone();
    let sel_sig = st.has_selection.clone();
    drop(st);
    pos_sig.set(pos);
    anc_sig.set(anc);
    sel_sig.set(has_sel);
}

/// Dispatch an AccessKit `ActionRequest` payload for the rich text
/// editor. Handles `SetTextSelection` (screen-reader-initiated
/// caret moves), `SetValue` (programmatic text replacement), and
/// `ScrollIntoView` (scroll so the caret is visible).
fn handle_access_action_request(
    state: &SharedState,
    action: fern_core::accesskit::Action,
    _target_node: fern_core::accesskit::NodeId,
    data: Option<fern_core::accesskit::ActionData>,
    ctx: &mut fern_core::widget::EventContext,
) -> fern_core::event::EventResponse {
    use self::policy::EditCommandKind;
    use fern_core::accesskit::{Action, ActionData};
    use fern_core::event::EventResponse;
    use fern_text::text_document::{MoveMode, SelectionType};

    match (action, data) {
        (Action::SetTextSelection, Some(ActionData::SetTextSelection(sel))) => {
            let filter = state.borrow().policy.command_filter;
            // Screen-reader-initiated caret moves are "navigation",
            // filtered under the same rule as arrow keys.
            if !filter.accepts(EditCommandKind::MoveLeft) {
                return EventResponse::Ignored;
            }
            let resolve = |pos: fern_core::accesskit::TextPosition| -> Option<usize> {
                let st = state.borrow();
                let map = st.synthetic_to_element.borrow();
                let er = map.get(&pos.node)?.clone();
                // Convert character_index (char units within the run)
                // to a byte offset within the run's text, then add
                // absolute_start to get the document position.
                let byte_off = er
                    .text
                    .char_indices()
                    .nth(pos.character_index)
                    .map(|(i, _)| i)
                    .unwrap_or(er.text.len());
                Some(er.absolute_start + byte_off)
            };
            if let (Some(a), Some(f)) = (resolve(sel.anchor), resolve(sel.focus)) {
                let st = state.borrow();
                st.cursor.set_position(a, MoveMode::MoveAnchor);
                st.cursor.set_position(f, MoveMode::KeepAnchor);
                drop(st);
                sync_cursor_signals(state);
                ctx.request_frame();
                EventResponse::Handled
            } else {
                EventResponse::Ignored
            }
        }
        (Action::SetValue, Some(ActionData::Value(value))) => {
            let filter = state.borrow().policy.command_filter;
            if !filter.accepts(EditCommandKind::InsertChar) {
                return EventResponse::Ignored;
            }
            let st = state.borrow();
            st.cursor.select(SelectionType::Document);
            let _ = st.cursor.insert_text(value.as_ref());
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        (Action::ScrollIntoView, _) => {
            let mut st = state.borrow_mut();
            if let Some(new_y) = st.engine.ensure_caret_visible() {
                st.scroll_y.set(new_y);
            }
            drop(st);
            ctx.request_frame();
            EventResponse::Handled
        }
        _ => EventResponse::Ignored,
    }
}

/// Convert an intra-fragment byte offset into a character index.
/// Used by `accessibility()` to map the user's document-absolute
/// cursor position into AccessKit's `TextPosition.character_index`
/// (which indexes into the target TextRun's `character_lengths`,
/// i.e., one entry per Rust `char`).
fn char_index_in_text(text: &str, byte_offset: usize) -> usize {
    // Walk char_indices until we pass byte_offset; the count at
    // that point is the character index. Fall back to the char
    // count when byte_offset >= text.len().
    if byte_offset >= text.len() {
        return text.chars().count();
    }
    let mut count = 0usize;
    for (i, _) in text.char_indices() {
        if i >= byte_offset {
            return count;
        }
        count += 1;
    }
    count
}
