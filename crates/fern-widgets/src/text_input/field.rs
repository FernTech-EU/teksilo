//! TextInputField — inner leaf widget for text editing.
//!
//! Handles text rendering, caret, selection, keyboard/mouse dispatch,
//! and accessibility (`Role::TextInput`). Created internally by the
//! public [`TextInput`](super::TextInput) composite; not exported.

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::EventResponse;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_text::text_document::SelectionType;
use fern_text::{CursorDisplay, RichTextEngine, SharedTypesetter};

use crate::menu_item::MenuItem;
use crate::menu_list::{MenuList, MenuSeparator};
use crate::rich_text::paint::{PaintParams, paint_frame};

use super::state::{SharedState, TextInputState, sync_cursor_signals};
use super::{keyboard, mouse};

/// Caret blink half-period (same as RichTextEditor).
const CARET_BLINK_INTERVAL: f32 = 0.5;

/// Debounce window for coalesced signal emission.
const DEBOUNCE_WINDOW_SECS: f32 = 0.150;

/// Horizontal scroll margin in pixels. The caret stays at least this
/// far from the left/right edge of the viewport.
const SCROLL_MARGIN: f32 = 4.0;

pub(crate) struct TextInputField {
    pub state: SharedState,
    /// Fixed height for the text area (from TextFieldStyle, minus borders).
    pub text_height: f32,
    /// Interaction state signal owned by the outer TextInput composite.
    /// Updated here on focus gain/loss to drive the FocusRing and border color.
    pub interaction: Signal<crate::button::InteractionState>,
}

impl std::fmt::Debug for TextInputField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputField")
            .field("text_height", &self.text_height)
            .finish_non_exhaustive()
    }
}

impl Widget for TextInputField {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Swap the private engine for one sharing the app's SharedTypesetter
        // so glyphs land in the atlas fern-render uploads to the GPU.
        if let Some(shared) = ctx.app_state::<SharedTypesetter>() {
            let mut st = self.state.borrow_mut();
            let mut engine = RichTextEngine::from_shared(shared.clone());
            engine.set_wrap_mode(fern_text::WrapMode::None);
            st.engine = engine;
            st.needs_full_layout = true;
        }

        // Apply theme colors AFTER the engine swap — setting them on the
        // old private engine is lost when the SharedTypesetter engine
        // replaces it above.
        {
            let theme = ctx.theme();
            let colors = &theme.colors;
            let mut st = self.state.borrow_mut();
            st.engine.set_text_color(colors.text_primary.to_array());
            st.engine.set_cursor_color(colors.text_primary.to_array());
            st.engine.set_selection_color(colors.selection_bg_active.to_array());
        }

        // Bind caret_visible for repaint.
        {
            let st = self.state.borrow();
            let caret_visible = st.caret_visible.clone();
            drop(st);
            let self_id = ctx.self_id();
            caret_visible.bind_to(
                self_id,
                ctx.binding_registry(),
                fern_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // Bind text_signal at AccessibilityOnly so screen readers see edits.
        {
            let st = self.state.borrow();
            let text_signal = st.text_signal.clone();
            drop(st);
            let self_id = ctx.self_id();
            text_signal.bind_to(
                self_id,
                ctx.binding_registry(),
                fern_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // Stash frame infrastructure handles and self_id.
        {
            let mut st = self.state.borrow_mut();
            st.frame_request = Some(ctx.frame_request_handle());
            st.frame_wake_at = Some(ctx.wake_at_handle());
            st.field_widget_id = Some(ctx.self_id());
        }

        ctx.request_frame();

        // Frame-tick effect: simplified frame loop.
        //
        // IMPORTANT: The mutable borrow must be dropped BEFORE setting
        // text_signal. Setting text_signal fires observers synchronously,
        // which chain into the ext→internal sync effect that borrows the
        // same state. Holding borrow_mut across signal.set() would panic.
        {
            let state = self.state.clone();
            let tick_signal = ctx.frame_tick();
            ctx.effect(&tick_signal, move |delta| {
                let (more, pending_text) = {
                    let mut st = state.borrow_mut();
                    let more = tick(&mut st, *delta);
                    st.has_selection.set(st.cursor.has_selection());
                    let pending = st.deferred_text_update.take();
                    (more, pending)
                };
                // State borrow is now dropped — safe to set external-facing signals.
                if let Some(text) = pending_text {
                    let st = state.borrow();
                    if st.text_signal.get() != text {
                        st.text_signal.set(text);
                    }
                }
                if more {
                    let st = state.borrow();
                    if let Some(handle) = &st.frame_request {
                        handle.set(true);
                    }
                }
            });
        }

        // Pre-build context menu (dormant until right-click).
        let context_menu_id = build_context_menu(ctx, &self.state);
        self.state.borrow_mut().context_menu_id = Some(context_menu_id);

        // Attach handlers.
        // Track hover state to infer focus origin (same pattern as Slider).
        let hovered = std::rc::Rc::new(std::cell::Cell::new(false));
        let hovered_for_focus = hovered.clone();
        let hovered_for_hover = hovered.clone();

        let handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Text)
            .on_hover(move |entered, _ctx| {
                hovered_for_hover.set(entered);
            })
            .on_focus({
                let state = self.state.clone();
                let interaction = self.interaction.clone();
                move |gained, ctx| {
                    // Update the composite's interaction signal so the
                    // FocusRing and border color react. Focus ring shows
                    // on both keyboard and pointer focus (Int UI behavior).
                    use crate::button::InteractionState;
                    interaction.set(if gained {
                        InteractionState::Focused
                    } else {
                        InteractionState::Idle
                    });

                    let mut st = state.borrow_mut();
                    st.has_focus = gained;
                    if gained {
                        st.blink_last_toggle = Some(std::time::Instant::now());
                        st.caret_visible.set(true);
                        // Int UI: select all on keyboard focus (not pointer click).
                        let is_keyboard = !hovered_for_focus.get();
                        drop(st);
                        if is_keyboard {
                            let st = state.borrow();
                            st.cursor.select(SelectionType::Document);
                            drop(st);
                            sync_cursor_signals(&state);
                        }
                    } else {
                        // Focus lost: clear selection, reset scroll to show
                        // beginning (Int UI spec), hide caret.
                        st.cursor.clear_selection();
                        st.scroll_x = 0.0;
                        st.caret_visible.set(false);
                        st.drag_state = super::state::DragState::Idle;
                    }
                    ctx.request_frame();
                }
            })
            .on_pointer_event({
                let state = self.state.clone();
                move |event, ctx| mouse::handle_pointer_event(&state, event, ctx)
            })
            .on_key({
                let state = self.state.clone();
                move |event, ctx| keyboard::handle_key(&state, event, ctx)
            })
            .on_double_tap({
                let state = self.state.clone();
                move |pos, ctx| mouse::handle_double_tap(&state, pos, ctx)
            })
            .on_triple_tap({
                let state = self.state.clone();
                move |pos, ctx| mouse::handle_triple_tap(&state, pos, ctx)
            })
            .on_access_action_request({
                let state = self.state.clone();
                move |action, _target_node, data, ctx| {
                    handle_access_action(&state, action, data, ctx)
                }
            });

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        let w = proposal.width.unwrap_or(200.0).max(0.0);
        // Fixed height derived from TextFieldStyle minus border.
        let h = self.text_height.max(0.0);
        Size::new(w, h)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let mut st = self.state.borrow_mut();
        st.viewport_width = bounds.width;
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let mut st = self.state.borrow_mut();

        st.viewport_origin = Point::new(bounds.x, bounds.y);
        let viewport_changed = (st.viewport_width - bounds.width).abs() > 0.5;
        if viewport_changed {
            st.viewport_width = bounds.width;
            st.needs_full_layout = true;
        }

        // Set engine viewport to a very large width (no wrap).
        // Vertical viewport = bounds height.
        st.engine.set_viewport(10_000.0, bounds.height);

        if st.needs_full_layout || !st.engine.has_full_layout() {
            let flow = st.document.snapshot_flow();
            st.engine.layout_full(&flow);
            st.needs_full_layout = false;
            st.content_dirty = true;
        }

        // Update cursor display.
        let caret_on = st.caret_visible.get() && st.has_focus;
        let cursor_display = CursorDisplay {
            position: st.cursor.position(),
            anchor: st.cursor.anchor(),
            visible: caret_on,
            selected_cells: Vec::new(),
        };
        st.engine.set_cursor(&cursor_display);

        // Ensure caret is visible (horizontal scroll).
        ensure_caret_visible_h(&mut st);

        let scroll_x = st.scroll_x;

        // Clip to bounds and paint with scroll offset.
        canvas.set_clip(bounds);

        let state_ref: &mut TextInputState = &mut *st;
        let TextInputState {
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
                    origin: Point::new(bounds.x - scroll_x, bounds.y),
                    document,
                    image_cache,
                    draw_caret: caret_on,
                },
            );
        });

        canvas.clear_clip();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use fern_core::accesskit::{Action, Role};

        let st = self.state.borrow();

        builder.set_role(Role::TextInput);

        let text = st.document.to_plain_text().unwrap_or_default();
        if !text.is_empty() {
            builder.set_value(&text);
        }

        if !st.placeholder.is_empty() {
            builder.inner_mut().set_placeholder(st.placeholder.clone());
        }

        if st.read_only {
            builder.set_read_only();
        }

        // Cursor / selection.
        let pos = st.cursor.position();
        let anchor = st.cursor.anchor();
        builder.set_text_selection_on_self(anchor, pos);

        // Character-level metadata for screen reader navigation.
        let char_lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
        builder.inner_mut().set_character_lengths(char_lengths);

        // Word boundaries.
        let word_starts = compute_word_starts(&text);
        if !word_starts.is_empty() {
            builder.inner_mut().set_word_starts(word_starts);
        }

        builder.add_action(Action::Focus);
        if !st.read_only {
            builder.add_action(Action::SetValue);
        }
        builder.add_action(Action::SetTextSelection);
    }
}

/// Adjust `scroll_x` so the caret stays within the visible viewport.
fn ensure_caret_visible_h(st: &mut TextInputState) {
    if !st.engine.has_full_layout() || st.viewport_width <= 0.0 {
        return;
    }
    let pos = st.cursor.position();
    // Ask the engine for the caret rectangle. Returns [x, y, w, h]
    // in document space (no scroll offset applied).
    let caret = st.engine.caret_rect(pos);
    let caret_x = caret[0];
    let caret_w = caret[2].max(1.0);
    let vw = st.viewport_width;

    if caret_x - st.scroll_x < SCROLL_MARGIN {
        // Caret is left of viewport.
        st.scroll_x = (caret_x - SCROLL_MARGIN).max(0.0);
    } else if caret_x + caret_w - st.scroll_x > vw - SCROLL_MARGIN {
        // Caret is right of viewport.
        st.scroll_x = caret_x + caret_w - vw + SCROLL_MARGIN;
    }
}

/// Simplified frame-loop tick for single-line text input.
fn tick(state: &mut TextInputState, delta: f32) -> bool {
    // Step 1: flush pending chars.
    if !state.pending_chars.is_empty() {
        let batch = std::mem::take(&mut state.pending_chars);
        let _ = state.cursor.insert_text(&batch);
        state.pending_text_changed = true;
    }

    // Step 2: drain document events.
    let had_events = state.drain_events();

    // Step 3: caret blink (wall-clock driven).
    let blinking_active = state.has_focus;
    if blinking_active {
        let now = std::time::Instant::now();
        let interval = std::time::Duration::from_secs_f32(CARET_BLINK_INTERVAL);
        match state.blink_last_toggle {
            None => {
                state.blink_last_toggle = Some(now);
            }
            Some(last) if now.saturating_duration_since(last) >= interval => {
                state.blink_last_toggle = Some(now);
                let was = state.caret_visible.get();
                state.caret_visible.set(!was);
            }
            _ => {}
        }
        // Schedule wake-up for next blink toggle.
        if let (Some(last), Some(wake)) = (state.blink_last_toggle, &state.frame_wake_at) {
            let next = last + interval;
            let merged = match wake.get() {
                Some(existing) if existing <= next => existing,
                _ => next,
            };
            wake.set(Some(merged));
        }
    } else {
        state.blink_last_toggle = None;
        if state.caret_visible.get() {
            state.caret_visible.set(false);
        }
    }

    // Step 4: layout if dirty.
    if state.needs_full_layout && state.viewport_width > 0.0 {
        let flow = state.document.snapshot_flow();
        state.engine.layout_full(&flow);
        state.needs_full_layout = false;
        state.content_dirty = true;
    }

    // Step 5: defer text_signal update. The actual set() happens
    // AFTER the mutable borrow is dropped in the frame-tick effect,
    // to avoid a RefCell double-borrow when signal observers chain.
    if state.pending_text_changed {
        let new_text = state.document.to_plain_text().unwrap_or_default();
        if state.text_signal.get() != new_text {
            state.deferred_text_update = Some(new_text);
        }
    }

    // Step 6: debounce drain for undo/redo signals.
    state.debounce_timer += delta;
    let debounce_ready = state.debounce_timer >= DEBOUNCE_WINDOW_SECS;
    if debounce_ready {
        if state.pending_text_changed {
            state.pending_text_changed = false;
        }
        if let Some((cu, cr)) = state.pending_undo_redo.take() {
            if state.can_undo.get() != cu {
                state.can_undo.set(cu);
            }
            if state.can_redo.get() != cr {
                state.can_redo.set(cr);
            }
        }
        state.debounce_timer = 0.0;
    }
    let debounce_work = state.pending_text_changed || state.pending_undo_redo.is_some();

    had_events || debounce_work
}

/// Handle AccessKit actions (SetValue, SetTextSelection).
fn handle_access_action(
    state: &SharedState,
    action: fern_core::accesskit::Action,
    data: Option<fern_core::accesskit::ActionData>,
    ctx: &mut EventContext,
) -> EventResponse {
    use fern_core::accesskit::{Action, ActionData};

    match (action, data) {
        (Action::SetTextSelection, Some(ActionData::SetTextSelection(sel))) => {
            let st = state.borrow();
            // For a single-node TextInput, both anchor and focus reference
            // the widget's own node. character_index is the char-based offset.
            st.cursor.set_position(sel.anchor.character_index, fern_text::text_document::MoveMode::MoveAnchor);
            st.cursor.set_position(sel.focus.character_index, fern_text::text_document::MoveMode::KeepAnchor);
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        (Action::SetValue, Some(ActionData::Value(value))) => {
            let st = state.borrow();
            st.cursor.select(SelectionType::Document);
            let _ = st.cursor.insert_text(value.as_ref());
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        (Action::Focus, _) => {
            if let Some(id) = state.borrow().field_widget_id {
                ctx.request_focus(id);
            }
            EventResponse::Handled
        }
        _ => EventResponse::Ignored,
    }
}

/// Compute word-start character indices for AccessKit.
///
/// AccessKit `word_starts` is a sorted array of character indices (into the
/// `character_lengths` array) where each word begins. A "character" here is
/// defined by `character_lengths` (one entry per `char`). Trailing
/// whitespace belongs to the preceding word; leading whitespace is its own
/// word (per the AccessKit spec).
fn compute_word_starts(text: &str) -> Vec<u8> {
    let mut starts = Vec::new();
    let mut in_word = false;
    for (char_index, ch) in text.chars().enumerate() {
        let is_word_char = ch.is_alphanumeric() || ch == '_';
        if is_word_char && !in_word {
            if let Ok(idx) = u8::try_from(char_index) {
                starts.push(idx);
            }
            // char_index > 255: stop emitting (u8 can't represent it).
            // Screen reader word nav past character 255 won't work, but
            // single-line inputs rarely reach that length.
        }
        in_word = is_word_char;
    }
    starts
}

/// Pre-build the right-click context menu as a dormant widget.
fn build_context_menu(ctx: &mut BuildContext, state: &SharedState) -> WidgetId {
    let state_cut = state.clone();
    let state_copy = state.clone();
    let state_paste = state.clone();
    let state_select_all = state.clone();

    // TODO: i18n — use tr!("edit-cut") etc. once i18n keys are defined.
    // TODO: enabled states should be signal-driven for live updates.
    // For now they are evaluated at menu-open time by rebuilding.
    let menu = MenuList::new()
        .item(
            MenuItem::new_literal("Cut")
                .shortcut_label("Ctrl+X")
                .on_activate_fn(move |ctx| {
                    {
                        let mut st = state_cut.borrow_mut();
                        keyboard::clipboard_cut(&mut st, ctx);
                    }
                    sync_cursor_signals(&state_cut);
                    ctx.request_frame();
                    ctx.dismiss_all_overlays();
                }),
        )
        .item(
            MenuItem::new_literal("Copy")
                .shortcut_label("Ctrl+C")
                .on_activate_fn(move |ctx| {
                    {
                        let mut st = state_copy.borrow_mut();
                        keyboard::clipboard_copy(&mut st, ctx);
                    }
                    ctx.dismiss_all_overlays();
                }),
        )
        .item(
            MenuItem::new_literal("Paste")
                .shortcut_label("Ctrl+V")
                .on_activate_fn(move |ctx| {
                    {
                        let mut st = state_paste.borrow_mut();
                        keyboard::clipboard_paste(&mut st, ctx);
                    }
                    sync_cursor_signals(&state_paste);
                    ctx.request_frame();
                    ctx.dismiss_all_overlays();
                }),
        )
        .item(MenuSeparator)
        .item(
            MenuItem::new_literal("Select All")
                .shortcut_label("Ctrl+A")
                .on_activate_fn(move |ctx| {
                    {
                        let st = state_select_all.borrow();
                        st.cursor.select(SelectionType::Document);
                    }
                    sync_cursor_signals(&state_select_all);
                    ctx.request_frame();
                    ctx.dismiss_all_overlays();
                }),
        );

    let menu_id = ctx.add(menu);
    ctx.set_dormant(menu_id);
    menu_id
}
