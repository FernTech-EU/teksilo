//! `RichTextEditor` — the main widget type.
//!
//! M8a implements the `read_only()` preset and the shared infrastructure
//! that the future `editor()` preset (M8b) will reuse. The widget
//! subscribes to `TextDocument::on_change` so multiple editors can share
//! a document like QTextEdit views — see gap 10 of the plan.

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, PointerButton, ScrollDelta, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_text::{FontRegistrar, RichTextEngine, SharedTypesetter, WrapMode};
use fern_tokens::Color;
use fern_text::text_document::{MoveMode, MoveOperation, SelectionType, TextDocument};

use super::frame_loop;
use super::hit_test;
use super::paint::{paint_frame, PaintParams};
use super::policy::{CaretPolicy, EditCommandKind, PolicyBundle, EDITOR_PRESET, READ_ONLY_PRESET};
use super::state::{EditorState, SharedState};
use fern_text::text_document::TextFormat;

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
        Some(hit_test::classify(&hit, selection))
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

    /// Select the entire document.
    pub fn select_all(&self) {
        {
            let st = self.state.borrow();
            st.cursor.select(SelectionType::Document);
        }
        sync_cursor_signals(&self.state);
    }

    /// Clear any current selection.
    pub fn deselect(&self) {
        {
            let st = self.state.borrow();
            st.cursor.clear_selection();
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

        // Stash the tree's frame-request handle on the state so the
        // frame-tick effect can self-chain (caret blink, drag
        // auto-scroll) without mutable access to the tree.
        {
            let mut st = self.state.borrow_mut();
            st.frame_request = Some(ctx.frame_request_handle());
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

        // Attach handlers: pointer for click dispatch, key for nav
        // (read-only preset still wants arrow-key + Home/End + Ctrl+C
        // + Ctrl+A), focus for caret state, scroll for wheel pans.
        let mut handlers = HandlerSet::new();
        handlers = handlers
            .focusable(true)
            .cursor(CursorIcon::Text)
            .on_focus({
                let state = self.state.clone();
                move |gained, ctx| {
                    let mut st = state.borrow_mut();
                    st.has_focus = gained;
                    if gained {
                        // Reset the blink phase to "now" so the
                        // caret pops on immediately and the first
                        // off-toggle happens exactly one interval
                        // later.
                        st.blink_last_toggle = Some(std::time::Instant::now());
                        st.caret_visible.set(true);
                    }
                    drop(st);
                    ctx.request_frame();
                }
            })
            .on_pointer_event({
                let state = self.state.clone();
                move |event, ctx| on_pointer_event(&state, event, ctx)
            })
            .on_scroll({
                let state = self.state.clone();
                move |event, ctx| on_scroll(&state, event, ctx)
            })
            .on_key({
                let state = self.state.clone();
                move |event, ctx| on_key(&state, event, ctx)
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
        // Scroll bar children layer on in M8b.
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

        // First-frame guard + viewport-change guard: (re)run the full
        // layout so the render call produces glyphs sized for the
        // current bounds.
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
        use super::policy::AccessibilityRole;
        use fern_core::accesskit::{Action, Role};
        let st = self.state.borrow();
        let role = match st.policy.access_role {
            AccessibilityRole::Editor => Role::MultilineTextInput,
            AccessibilityRole::Document => Role::Document,
        };
        builder.set_role(role);
        if st.policy.is_read_only() {
            builder.set_read_only();
        }
        if let Ok(text) = st.document.to_plain_text() {
            builder.set_value(text);
        }
        builder.add_action(Action::ScrollIntoView);
        builder.add_action(Action::SetTextSelection);
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
/// next signal propagation.
fn sync_cursor_signals(state: &SharedState) {
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

fn on_pointer_event(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    match event {
        WidgetEvent::PointerDown {
            position,
            button,
            modifiers: _,
        } => {
            if *button != PointerButton::Primary {
                // Secondary / middle are for the application's own
                // context menu; let them bubble.
                return EventResponse::Ignored;
            }
            let st = state.borrow();
            // Pointer position arrives in window-local coordinates;
            // subtract the widget origin (recorded by `paint()`) to
            // get the widget-local point text-typeset's `hit_test`
            // expects. scroll offset and zoom are applied internally
            // by the typesetter.
            let local = Point::new(
                position.x - st.viewport_origin.x,
                position.y - st.viewport_origin.y,
            );
            let hit = hit_test::hit_test_at(&st.engine, local, 0.0, 0.0);
            drop(st);
            let Some(hit) = hit else {
                return EventResponse::Ignored;
            };
            match &hit.region {
                fern_text::HitRegion::Link { href: _ }
                | fern_text::HitRegion::Image { name: _ } => {
                    // Link / image click: M8b will emit the typed
                    // command. For now just flag the request.
                    ctx.request_frame();
                    EventResponse::Handled
                }
                _ => {
                    // Place the cursor so the selection anchor tracks
                    // the click; read-only preset still allows
                    // click-drag selection for Copy/Cut later.
                    let st = state.borrow();
                    st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
                    drop(st);
                    sync_cursor_signals(state);
                    ctx.request_frame();
                    EventResponse::Handled
                }
            }
        }
        WidgetEvent::PointerMove { position: _ } => EventResponse::Ignored,
        WidgetEvent::PointerUp { .. } => EventResponse::Handled,
        _ => EventResponse::Ignored,
    }
}

fn on_scroll(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    if let WidgetEvent::Scroll { delta } = event {
        // Match `ScrollArea`'s sign convention: `delta.y` is the
        // scroll distance in document pixels per unit of wheel /
        // trackpad movement, already oriented so that positive
        // means "scroll content up" (i.e. increase scroll_y). For
        // line-based events the line_height multiplier is 16 px to
        // match ScrollArea's default.
        let (dx, dy) = match delta {
            ScrollDelta::Lines { x, y } => (*x * 16.0, *y * 16.0),
            ScrollDelta::Pixels { x, y } => (*x, *y),
        };
        let st = state.borrow();
        let new_y = (st.scroll_y.get() + dy).clamp(0.0, st.max_scroll_y.get());
        let new_x = (st.scroll_x.get() + dx).clamp(0.0, st.max_scroll_x.get());
        st.scroll_y.set(new_y);
        st.scroll_x.set(new_x);
        drop(st);
        ctx.request_frame();
        return EventResponse::Handled;
    }
    EventResponse::Ignored
}

/// Kind of key action taken by `on_key`, used to decide whether to
/// clear the sticky preferred-X afterwards.
#[derive(Copy, Clone, PartialEq, Eq)]
enum KeyAction {
    /// The key caused horizontal motion, a selection change, or
    /// something else that invalidates the preferred column.
    ClearPreferredX,
    /// Vertical motion (Up/Down/PageUp/PageDown): the sticky column
    /// must be preserved so repeated vertical presses land on the
    /// same visual column.
    KeepPreferredX,
    /// The key was not handled.
    Unhandled,
}

fn on_key(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    // IME commit — one string per composition, already a finalized
    // grapheme cluster. Treated identically to a KeyDown with printable
    // text: batched into `pending_chars`, flushed next frame.
    if let WidgetEvent::ImeCommit { text } = event {
        return push_pending_chars(state, ctx, text);
    }

    let WidgetEvent::KeyDown { key, modifiers, text, .. } = event else {
        return EventResponse::Ignored;
    };

    let shift = modifiers.shift();
    let ctrl = modifiers.ctrl() || modifiers.super_key();
    let mode = if shift {
        MoveMode::KeepAnchor
    } else {
        MoveMode::MoveAnchor
    };

    // `TextCursor::clone()` creates an **independent** cursor with
    // its own position/anchor data (see the Clone impl in
    // text-document/.../cursor.rs). Cloning and mutating the clone
    // leaves `state.cursor` untouched. We must therefore operate on
    // `state.cursor` directly through a short-lived borrow and drop
    // the state before calling `sync_cursor_signals` so the signal
    // observers see the post-move value.
    let action: KeyAction = {
        let mut st = state.borrow_mut();
        let filter = st.policy.command_filter;
        match key {
            Key::ArrowLeft if filter.accepts(EditCommandKind::MoveLeft) => {
                let op = if ctrl {
                    MoveOperation::WordLeft
                } else {
                    MoveOperation::Left
                };
                st.cursor.move_position(op, mode, 1);
                KeyAction::ClearPreferredX
            }
            Key::ArrowRight if filter.accepts(EditCommandKind::MoveRight) => {
                let op = if ctrl {
                    MoveOperation::WordRight
                } else {
                    MoveOperation::Right
                };
                st.cursor.move_position(op, mode, 1);
                KeyAction::ClearPreferredX
            }
            Key::ArrowUp if filter.accepts(EditCommandKind::MoveUp) => {
                move_cursor_vertical(&mut st, -1, mode);
                KeyAction::KeepPreferredX
            }
            Key::ArrowDown if filter.accepts(EditCommandKind::MoveDown) => {
                move_cursor_vertical(&mut st, 1, mode);
                KeyAction::KeepPreferredX
            }
            Key::PageUp if filter.accepts(EditCommandKind::PageUp) => {
                move_cursor_page(&mut st, -1, mode);
                KeyAction::KeepPreferredX
            }
            Key::PageDown if filter.accepts(EditCommandKind::PageDown) => {
                move_cursor_page(&mut st, 1, mode);
                KeyAction::KeepPreferredX
            }
            Key::Home if filter.accepts(EditCommandKind::MoveHome) => {
                if ctrl {
                    st.cursor.move_position(MoveOperation::Start, mode, 1);
                } else {
                    move_cursor_to_line_edge(&mut st, LineEdge::Start, mode);
                }
                KeyAction::ClearPreferredX
            }
            Key::End if filter.accepts(EditCommandKind::MoveEnd) => {
                if ctrl {
                    st.cursor.move_position(MoveOperation::End, mode, 1);
                } else {
                    // Use the typesetter to find end-of-visual-line
                    // rather than text-document's EndOfBlock. Two
                    // wins: (a) a second End press from an already-
                    // at-end cursor is a no-op, avoiding the
                    // block-advance bug where `get_block_at_position`
                    // returns the *next* block when queried at a
                    // boundary; (b) wrapped blocks stop at the wrap
                    // point, which is the standard editor behaviour.
                    move_cursor_to_line_edge(&mut st, LineEdge::End, mode);
                }
                KeyAction::ClearPreferredX
            }
            Key::A if ctrl && filter.accepts(EditCommandKind::SelectAll) => {
                st.cursor.select(SelectionType::Document);
                KeyAction::ClearPreferredX
            }
            Key::C if ctrl && filter.accepts(EditCommandKind::Copy) => {
                // Copy is a no-op here until Phase B wires
                // `clipboard::copy` through `EventContext::app_state`.
                // The selection remains readable via
                // `RichTextEditor::selected_text()`.
                KeyAction::ClearPreferredX
            }
            // --- Editor-preset mutating commands ---
            Key::Backspace if filter.accepts(EditCommandKind::DeletePrev) => {
                if ctrl {
                    // Ctrl+Backspace = delete word to the left.
                    // Select the word, then delete the selection —
                    // matches godot rich_text_edit.rs:580 (there is
                    // no dedicated delete-word API on TextCursor).
                    if !st.cursor.has_selection() {
                        st.cursor.move_position(MoveOperation::WordLeft, MoveMode::KeepAnchor, 1);
                    }
                    let _ = st.cursor.remove_selected_text();
                } else if st.cursor.has_selection() {
                    let _ = st.cursor.remove_selected_text();
                } else {
                    let _ = st.cursor.delete_previous_char();
                }
                KeyAction::ClearPreferredX
            }
            Key::Delete if filter.accepts(EditCommandKind::DeleteNext) => {
                if ctrl {
                    if !st.cursor.has_selection() {
                        st.cursor.move_position(MoveOperation::WordRight, MoveMode::KeepAnchor, 1);
                    }
                    let _ = st.cursor.remove_selected_text();
                } else if st.cursor.has_selection() {
                    let _ = st.cursor.remove_selected_text();
                } else {
                    let _ = st.cursor.delete_char();
                }
                KeyAction::ClearPreferredX
            }
            Key::Enter if filter.accepts(EditCommandKind::InsertBlock) => {
                // Phase A: always insert a new block. Phase B will
                // add table-cell-aware navigation (Enter inside a
                // table cell navigates to the next row).
                let _ = st.cursor.insert_block();
                KeyAction::ClearPreferredX
            }
            Key::B if ctrl && filter.accepts(EditCommandKind::ToggleBold) => {
                toggle_char_format(&mut st, FormatBit::Bold);
                KeyAction::ClearPreferredX
            }
            Key::I if ctrl && filter.accepts(EditCommandKind::ToggleItalic) => {
                toggle_char_format(&mut st, FormatBit::Italic);
                KeyAction::ClearPreferredX
            }
            Key::U if ctrl && filter.accepts(EditCommandKind::ToggleUnderline) => {
                toggle_char_format(&mut st, FormatBit::Underline);
                KeyAction::ClearPreferredX
            }
            Key::Z if ctrl && !shift && filter.accepts(EditCommandKind::Undo) => {
                let _ = st.document.undo();
                KeyAction::ClearPreferredX
            }
            Key::Y if ctrl && filter.accepts(EditCommandKind::Redo) => {
                let _ = st.document.redo();
                KeyAction::ClearPreferredX
            }
            Key::Z if ctrl && shift && filter.accepts(EditCommandKind::Redo) => {
                let _ = st.document.redo();
                KeyAction::ClearPreferredX
            }
            _ => {
                // Printable character fallback: winit populates
                // `KeyDown::text` with the character produced by the
                // key (post-layout mapping, so Shift / dead keys /
                // layout translations are already applied). Only
                // accepted under `All` filter.
                if let Some(t) = text.as_deref() {
                    if filter.accepts(EditCommandKind::InsertChar) {
                        let clean: String = t
                            .chars()
                            .filter(|c| !c.is_control())
                            .collect();
                        if !clean.is_empty() {
                            st.pending_chars.push_str(&clean);
                            // Fall through the outer match arm's
                            // post-processing — we want preferred_x
                            // cleared and a frame request.
                            KeyAction::ClearPreferredX
                        } else {
                            KeyAction::Unhandled
                        }
                    } else {
                        KeyAction::Unhandled
                    }
                } else {
                    KeyAction::Unhandled
                }
            }
        }
    };

    match action {
        KeyAction::Unhandled => EventResponse::Ignored,
        KeyAction::ClearPreferredX => {
            {
                let mut st = state.borrow_mut();
                st.preferred_x = None;
            }
            ensure_caret_visible(state);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        KeyAction::KeepPreferredX => {
            ensure_caret_visible(state);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
    }
}

/// Which `TextFormat` bit a Ctrl+B/I/U toggle flips.
#[derive(Copy, Clone)]
enum FormatBit {
    Bold,
    Italic,
    Underline,
}

/// Toggle a single character-format bit at the caret, mirroring the
/// godot reference (rich_text_edit.rs:2089-2117): the decision to
/// turn a format on or off is read from the current caret format
/// (`char_format()`), not from a selection-wide consensus. If the
/// caret sits in bold text the toggle turns bold off for the whole
/// selection (or, with no selection, for subsequent inserts at
/// the caret position); if the caret sits in plain text with a
/// mixed-bold selection, Ctrl+B bolds the whole selection.
///
/// **Read-position subtlety**: `TextCursor::char_format()` reads
/// the inline element at `position()`. After a select-all the
/// caret sits at the *end* of the selection, which may be past the
/// last character (an empty "virtual" element with default format).
/// To get a meaningful read for the toggle decision we use
/// `selection_start()` when a selection is active — that position
/// is always the actual first character of the selected range.
fn toggle_char_format(st: &mut EditorState, bit: FormatBit) {
    let probe = st.document.cursor();
    if st.cursor.has_selection() {
        let start = st.cursor.selection_start();
        probe.set_position(start, fern_text::text_document::MoveMode::MoveAnchor);
    } else {
        probe.set_position(
            st.cursor.position(),
            fern_text::text_document::MoveMode::MoveAnchor,
        );
    }
    let current = probe.char_format().unwrap_or_default();
    let new_value = !match bit {
        FormatBit::Bold => current.font_bold.unwrap_or(false),
        FormatBit::Italic => current.font_italic.unwrap_or(false),
        FormatBit::Underline => current.font_underline.unwrap_or(false),
    };
    let fmt = match bit {
        FormatBit::Bold => TextFormat {
            font_bold: Some(new_value),
            ..Default::default()
        },
        FormatBit::Italic => TextFormat {
            font_italic: Some(new_value),
            ..Default::default()
        },
        FormatBit::Underline => TextFormat {
            font_underline: Some(new_value),
            ..Default::default()
        },
    };
    let _ = st.cursor.merge_char_format(&fmt);
    st.pending_format_changed = true;
}

/// Shared helper for printable-character ingestion: push the text
/// into `pending_chars`, clear sticky `preferred_x`, request a frame.
/// Reused by the IME commit path.
fn push_pending_chars(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
) -> EventResponse {
    if text.is_empty() {
        return EventResponse::Ignored;
    }
    let filter = state.borrow().policy.command_filter;
    if !filter.accepts(EditCommandKind::InsertChar) {
        return EventResponse::Ignored;
    }
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    if clean.is_empty() {
        return EventResponse::Ignored;
    }
    {
        let mut st = state.borrow_mut();
        st.pending_chars.push_str(&clean);
        st.preferred_x = None;
    }
    ctx.request_frame();
    EventResponse::Handled
}

/// Ask the typesetter to compute a scroll offset that keeps the
/// current caret inside the viewport, and write that offset into
/// the widget's `scroll_y` signal. Called only from keyboard
/// handlers (after arrow/page nav), never from the frame loop —
/// otherwise wheel scrolls that move the viewport away from the
/// caret would be undone on the next tick.
fn ensure_caret_visible(state: &SharedState) {
    let mut st = state.borrow_mut();
    if !st.engine.has_full_layout() {
        return;
    }
    // Forward the current wheel-driven scroll so ensure_caret_visible
    // computes the correction relative to where the viewport actually
    // is, not where it was at last paint.
    let current = st.scroll_y.get();
    st.engine.set_scroll_offset(current);
    if let Some(new_off) = st.engine.ensure_caret_visible() {
        st.scroll_y.set(new_off);
    }
}

#[derive(Copy, Clone)]
enum LineEdge {
    Start,
    End,
}

/// Move the cursor to the start or end of the current visual line
/// using the typesetter's `hit_test`. Solves two bugs at once:
///  * A second End press after landing at line end is a no-op,
///    avoiding text-document's block-boundary ambiguity where
///    `get_block_at_position(block_end_pos)` returns the next block.
///  * Wrapped blocks stop at the wrap point (the standard editor
///    Home/End semantics).
fn move_cursor_to_line_edge(st: &mut EditorState, edge: LineEdge, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos);
    let line_y = caret[1] + caret[3] * 0.5;
    // Probe far outside the viewport horizontally; the typesetter
    // clamps the hit to the actual line extent and returns a valid
    // position at either edge.
    let probe_x = match edge {
        LineEdge::Start => -1.0e6,
        LineEdge::End => 1.0e6,
    };
    if let Some(hit) = st.engine.hit_test(probe_x, line_y) {
        if hit.position != pos {
            st.cursor.set_position(hit.position, mode);
        }
    }
}

/// Move the cursor up or down by one visual line, using the
/// typesetter's layout and caret_rect for the source position and
/// `hit_test` at the target Y to find the position on the next
/// line. Uses a sticky `preferred_x` so repeated vertical presses
/// stay on the same visual column even across short lines.
///
/// Called from `on_key` with `state.borrow_mut()` already held.
fn move_cursor_vertical(st: &mut EditorState, direction: i32, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos);
    let line_height = caret[3].max(16.0);
    let center_y = caret[1] + caret[3] * 0.5;

    let x = st.preferred_x.unwrap_or(caret[0]);
    if st.preferred_x.is_none() {
        st.preferred_x = Some(caret[0]);
    }

    let target_y = center_y + (direction as f32) * line_height;
    if target_y < 0.0 || target_y > st.engine.content_height() {
        return;
    }

    if let Some(hit) = st.engine.hit_test(x, target_y) {
        if hit.position != pos {
            st.cursor.set_position(hit.position, mode);
        }
    }
}

/// Move the cursor up or down by roughly one viewport page, and
/// scroll so the caret stays visible. Like `move_cursor_vertical`,
/// uses a sticky preferred X.
fn move_cursor_page(st: &mut EditorState, direction: i32, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    let viewport_h = st.viewport_height;
    if viewport_h <= 0.0 {
        return;
    }
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos);
    let line_height = caret[3].max(16.0);
    let center_y = caret[1] + caret[3] * 0.5;

    let x = st.preferred_x.unwrap_or(caret[0]);
    if st.preferred_x.is_none() {
        st.preferred_x = Some(caret[0]);
    }

    // Move by one viewport minus one line so the reader keeps a
    // line of visual context across the page jump.
    let page_step = (viewport_h - line_height).max(line_height);
    let target_y = (center_y + (direction as f32) * page_step)
        .clamp(0.0, st.engine.content_height());

    if let Some(hit) = st.engine.hit_test(x, target_y) {
        if hit.position != pos {
            st.cursor.set_position(hit.position, mode);
        }
    }

    // Scroll so the new caret position is visible. We do a simple
    // viewport-height step on the scroll signal and let the frame
    // loop's `ensure_caret_visible` path clamp it.
    let new_scroll = (st.scroll_y.get() + (direction as f32) * page_step)
        .clamp(0.0, st.max_scroll_y.get());
    st.scroll_y.set(new_scroll);
}
