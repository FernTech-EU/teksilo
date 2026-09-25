// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for [`TextInputField`]: build (the
//! text engine, pointer and keyboard handlers, focus, IME and the context
//! menu), layout, placement, paint and accessibility.

use super::*;
impl Widget for TextInputField {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Tell the framework this widget edits text.
        //
        // What it buys: an application may take `Ctrl+Z`, `Ctrl+C` and friends
        // for itself — a single Undo command over the whole app has to — and
        // registered shortcuts resolve before any widget sees the raw key. This
        // is how the host can tell that the caret is *here*, and either drive
        // this surface or step aside so it keeps its own keys. Without it, an
        // application that routes those chords silently breaks every text
        // widget it does not personally know about. See
        // `teksilo_core::text_surface`.
        ctx.register_text_surface(std::rc::Rc::new(self.handle()));
        // Resolve the interaction signal (external override wins).
        if let Some(signal) = self.external_interaction.take() {
            self.interaction = signal;
        }

        // Resolve the mask placeholder character. Caller override wins;
        // otherwise pull from the recipe constant. The theme snapshot
        // is still captured for downstream typography reads below.
        let theme_snapshot = ctx.theme_signal().get();
        let mask_placeholder_char = self
            .mask_placeholder_override
            .unwrap_or(crate::styles::recipe_text_input_style::TEXT_FIELD_MASK_PLACEHOLDER_CHAR);

        // Auto-derive placeholder from mask when none was explicitly
        // set: an empty masked field paints `__/__/____` rather than
        // a blank surface, giving the user a self-documenting template.
        if self.placeholder.is_empty()
            && let Some(ref m) = self.mask
        {
            self.placeholder = m.empty_template(mask_placeholder_char);
        }

        // Cache mask-aware natural width. When a mask is set, the
        // visual content envelope is the FILLED template — every
        // editable position holding its widest plausible glyph
        // (`0` for digits, `M` for letters, etc.) and every fixed
        // position holding its literal. Measuring the empty
        // (`__/__/____`) template instead would shortchange the
        // field by the difference between an underscore and a real
        // glyph: ~2 dp per digit slot for `0`, ~5 dp per letter
        // slot for `M`, which adds up to a multi-character shortfall
        // for date / 12h time fields. We want the natural width to
        // hold the fully-typed value without overflow.
        //
        // Without a mask the 200 dp fallback (set in `new()`) stays.
        if let Some(ref m) = self.mask {
            // Measure the worst-case glyph row PLUS one extra `M` of
            // safety: one for caret breathing room past the last
            // position, plus a defensive cushion for any per-glyph
            // measurement variance between our heuristic fallback
            // and the real glyph shaper. Without this safety char,
            // dates were observed to clip the trailing 2 characters
            // and 12h time fields clipped the AM/PM letters.
            let mut widest = worst_case_template(m);
            widest.push('M');
            let style = &theme_snapshot.typography.body;
            let measured = measure_width_px(ctx, &widest, style);
            let slack = style.size;
            self.natural_width = measured + slack;
        }

        // Compose the user's char_filter with the mask's class filter.
        // The mask doesn't know the cursor position here (this is a
        // pre-position filter), so it accepts any char that fits *any*
        // editable position class — a permissive gate that catches
        // gross mismatches (typing "a" into a digits-only mask) without
        // requiring per-keystroke position tracking. Per-position
        // gating happens at commit time via the validator.
        if let Some(ref mask) = self.mask {
            let mask_for_filter = mask.clone();
            let user_filter = self.char_filter.take();
            let combined: CharFilter = Rc::new(move |c: char| {
                // Always allow fixed-separator characters (they're
                // legitimate input even if user types them — the
                // formatter consumes them).
                let in_mask_class = mask_for_filter.positions().any(|p| match p {
                    MaskPosition::Editable { class, .. } => class.accepts(c),
                    MaskPosition::Fixed(sep) => *sep == c,
                });
                if !in_mask_class {
                    return false;
                }
                match user_filter.as_ref() {
                    Some(f) => f(c),
                    None => true,
                }
            });
            self.char_filter = Some(combined);
        }

        // Build the shared state from the configured builder values.
        let mut on_submit = self.on_submit.take().map(Rc::new);
        let mut on_blur = self.on_blur.take().map(Rc::new);

        // Wrap commit callbacks with the validator pipeline. The
        // wrapping closure: snapshots the bound text, runs the
        // validator, applies the outcome (writes feedback, mutates
        // text on `Corrected`), then chains the user's callback so
        // composites can react to the now-updated state.
        if let Some(validator) = self.validator.clone() {
            let bound_text = self.text.clone();
            let feedback = self.feedback.clone();
            let prev_on_blur = on_blur.take();
            on_blur = Some(Rc::new(Box::new({
                let validator = validator.clone();
                let feedback = feedback.clone();
                let bound_text = bound_text.clone();
                move |evt_ctx: &mut EventContext| {
                    run_validator_and_apply(&validator, &bound_text, &feedback);
                    if let Some(cb) = prev_on_blur.as_ref() {
                        cb(evt_ctx);
                    }
                }
            }) as CommandFactory));
            let prev_on_submit = on_submit.take();
            on_submit = Some(Rc::new(Box::new({
                let validator = validator.clone();
                let feedback = feedback.clone();
                let bound_text = bound_text.clone();
                move |evt_ctx: &mut EventContext| {
                    run_validator_and_apply(&validator, &bound_text, &feedback);
                    if let Some(cb) = prev_on_submit.as_ref() {
                        cb(evt_ctx);
                    }
                }
            }) as CommandFactory));
        }

        let initial_text = self.text.get();
        // `read_only_effective` snapshots the build-time state so the
        // shared TextInputState's read-only mode is set once. Disabled
        // is now arena-driven and propagates per-paint via
        // `effective_enabled`; the framework gates pointer events on
        // `arena.is_enabled` before they reach this field's handlers. The
        // shared state's read_only stays a separate, document-level
        // concept (allows selection / no edits).
        let read_only_effective = self.read_only || !self.enabled.get();

        let initial_suffix = self.suffix.get();
        let shared_state = TextInputState::new(TextInputConfig {
            initial_text,
            max_length: self.max_length,
            read_only: read_only_effective,
            on_submit,
            on_access_set_value: self.on_access_set_value.clone(),
            on_blur,
            char_filter: self.char_filter.take(),
            placeholder: self.placeholder.clone(),
            suffix: initial_suffix,
            secure: self.secure,
            echo_mode: self.echo_mode,
            echo_char: self.echo_char,
            revealed: self.revealed.clone(),
            at_reveal_policy: self.at_reveal_policy,
            allow_copy: self.allow_copy,
            focus_signal: self.focus_signal.clone(),
        });
        self.state = Some(shared_state.clone());
        // Late-populate the slot so `caret_setter()` closures captured
        // before build can now reach the inner state. Idempotent on
        // rebuild — overwrites the slot with the freshly created
        // SharedState.
        *self.state_slot.borrow_mut() = Some(shared_state.clone());

        // Reset feedback to Pristine whenever the user types — prior
        // Invalid / Corrected announcements should clear as soon as
        // the user starts editing again so they don't shout stale
        // errors at someone trying to fix them.
        {
            let feedback = self.feedback.clone();
            ctx.effect(&self.text, move |_| {
                if !matches!(feedback.get(), ValidationFeedback::Pristine) {
                    feedback.set(ValidationFeedback::Pristine);
                }
            });
        }

        // Mirror the inner state's `cursor_position` onto the field's
        // public `caret_position` so callers of `caret_position()` see
        // live caret updates. The state's signal is keyed by the
        // shared state's identity (created in `TextInputState::new`),
        // not by the field's; this effect bridges the two.
        {
            let inner = shared_state.borrow().cursor_position.clone();
            let outer = self.caret_position.clone();
            outer.set(inner.get());
            ctx.effect(&inner, move |pos| {
                if outer.get() != *pos {
                    outer.set(*pos);
                }
            });
        }

        // Bind feedback at AccessibilityOnly so the field's AT node
        // refreshes its `set_invalid` state when feedback changes.
        {
            let self_id = ctx.self_id();
            self.feedback.bind_to(
                self_id,
                ctx.binding_registry(),
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // Combobox wiring: a moved highlight in the list this field drives must
        // re-walk the AT tree so the new `active_descendant` is announced.
        // AccessibilityOnly — nothing about this field's own pixels changed.
        for sig in [self.active_descendant.as_ref(), self.controls.as_ref()]
            .into_iter()
            .flatten()
        {
            sig.bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // Secure fields: flipping the reveal toggle must repaint AND
        // refresh AT. `RepaintOnly` dirties this node for the render
        // walker so `paint()` runs and re-lays-out the masked/unmasked
        // glyphs via the `needs_full_layout` flag the effect below sets
        // — without it the flag is set but nothing calls `paint()`, so
        // the visual only updates on the next unrelated repaint
        // (hover / focus). This mirrors how `text_signal` is bound for
        // edits. The parallel `AccessibilityOnly` bind swaps the AT
        // role/value (PasswordInput ↔ TextInput under SwapRole); it lives
        // in its own bucket and does not imply repaint, so both are
        // required.
        if self.secure
            && let Some(revealed) = self.revealed.clone()
        {
            let id = ctx.self_id();
            let reg = ctx.binding_registry();
            revealed.bind_to(id, reg, teksilo_core::binding::BindingLevel::RepaintOnly);
            revealed.bind_to(
                id,
                reg,
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            );
            // The mask a hiding walk withheld (see `AtPublished`) is
            // published by the walk this bump asks for.
            shared_state.borrow().at_republish.bind_to(
                id,
                reg,
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        let text_signal = shared_state.borrow().text_signal.clone();

        // Sync external text signal → internal state. A programmatic
        // update on the bound signal rewrites the document; the
        // caret ends up at the end of the inserted text (cursor
        // behavior is documented in
        // `text_document::TextCursor::insert_text`).
        //
        // `insert_text` only enqueues a `ContentsChanged` document
        // event — `tick()` drains it on the next frame and propagates
        // the new text to `text_signal`. Frames are demand-driven, so
        // we ping `frame_request` here to guarantee a tick runs even
        // when the external writer (e.g. an HSV-canvas drag feeding a
        // spinner / hex bridge) is the only thing changing on screen.
        // Without it, the document stays in sync with the bound signal
        // but the visible glyphs lag until something else (focus, a
        // keystroke, an animation frame) wakes the loop.
        {
            let ext = self.text.clone();
            let state_for_sync = shared_state.clone();
            let touch_for_sync = self.touch.clone();
            ctx.effect(&ext, move |new_text| {
                let st = state_for_sync.borrow();
                let current = st.document.to_plain_text().unwrap_or_default();
                if current != *new_text {
                    st.cursor.select(SelectionType::Document);
                    let _ = st.cursor.insert_text(new_text);
                    if let Some(handle) = &st.frame_request {
                        handle.set(true);
                    }
                    drop(st);
                    // The content the affordances marked is gone. This effect has
                    // no `EventContext`, which is why retirement is the
                    // controller publishing empty geometry rather than an overlay
                    // dismissal — see `FieldTouch::dismiss`.
                    touch_for_sync.dismiss();
                }
            });
        }

        // Sync internal text signal → external. Every edit that
        // reaches `text_signal` also updates the caller-owned
        // signal, so observers bound to it see every keystroke
        // (after the debounce in `tick`).
        {
            let ext = self.text.clone();
            ctx.effect(&text_signal, move |new_text| {
                if ext.get() != *new_text {
                    ext.set(new_text.clone());
                }
            });
        }

        // Secure reveal toggle: flipping the bound `revealed` signal
        // swaps the laid-out glyphs wholesale (bullets ↔ plaintext), so
        // mark the layout dirty and ping the frame loop to re-lay-out.
        if self.secure
            && let Some(revealed) = self.revealed.clone()
        {
            let state_for_reveal = shared_state.clone();
            ctx.effect(&revealed, move |_| {
                let mut st = state_for_reveal.borrow_mut();
                st.needs_full_layout = true;
                if let Some(handle) = &st.frame_request {
                    handle.set(true);
                }
            });
        }

        // Swap the private engine for one sharing the app's
        // `SharedTypesetter` so glyphs land in the atlas
        // teksilo-render uploads to the GPU. When no typesetter is
        // installed (headless tests), the pre-built private
        // engine stays in place.
        if let Some(shared) = ctx.app_state::<SharedTypesetter>() {
            let mut st = self.state().borrow_mut();
            let mut engine = RichTextEngine::from_shared(shared.clone());
            engine.set_wrap_mode(teksilo_text::WrapMode::None);
            st.engine = engine;
            st.needs_full_layout = true;
        }

        // Apply theme colors to the (possibly freshly swapped-in) engine.
        // Setting them before the swap would be lost. The rich-text
        // engine stores colors in GPU-ready form, so we register an
        // effect on the theme signal that re-applies the palette on
        // every theme switch instead of capturing a single snapshot.
        //
        // The text / caret / suffix *foreground* colours are deliberately
        // NOT set here — `paint` owns them, because they depend on the
        // effective enabled state as well as the theme (see the resolve
        // block there). Selection is theme + window-active only, so it
        // stays on this effect path.
        let theme_signal = ctx.theme_signal();
        // The selection colour is also window-active-aware. Rather than one
        // effect on a derived `theme.zip(window_active)`, the theme effect
        // reads the live window-active value via `.get()`, and the separate
        // window-active effect (below, near the frame handles) re-applies the
        // selection colour reading the live theme. Between them, a change to
        // either axis re-applies correctly.
        {
            let theme = theme_signal.get();
            let colors = &theme.colors;
            let mut st = self.state().borrow_mut();
            let tint = field_selection_color(colors, ctx.window_active(), st.has_focus);
            st.selection_tint = tint;
            st.engine.set_selection_color(tint);
        }
        {
            let state = self.state().clone();
            let wa_signal = ctx.window_active_signal();
            ctx.effect(&theme_signal, move |theme| {
                let colors = &theme.colors;
                let mut st = state.borrow_mut();
                let tint = field_selection_color(colors, wa_signal.get(), st.has_focus);
                st.selection_tint = tint;
                st.engine.set_selection_color(tint);
            });
        }

        // Suffix engine: second independent `RichTextEngine` used
        // to paint the non-editable trailing string (Qt's
        // `QSpinBox` `suffix`). Shares the app's typesetter when
        // available so glyphs land in the same atlas as the main
        // document; falls back to a private engine under headless
        // tests.
        //
        // `suffix_width` is cached on `TextInputState` and drives
        // both the effective text viewport (so the scroll logic
        // keeps the caret visible without sliding text behind the
        // suffix) and the suffix paint origin at the right edge
        // of the field. When the suffix is bound to a signal, a
        // reactive effect below re-lays the engine out each time
        // the signal fires.
        let text_area_height = self.text_height.unwrap_or(DEFAULT_TEXT_HEIGHT).max(1.0);
        let needs_suffix_engine = matches!(self.suffix, Prop::Bound(_)) || {
            let st = self.state().borrow();
            !st.suffix.is_empty()
        };
        if needs_suffix_engine {
            let mut suffix_engine = if let Some(shared) = ctx.app_state::<SharedTypesetter>() {
                RichTextEngine::from_shared(shared.clone())
            } else {
                RichTextEngine::private_default()
            };
            suffix_engine.set_wrap_mode(teksilo_text::WrapMode::None);
            {
                let theme = theme_signal.get();
                let secondary = theme.colors.text_secondary.to_array();
                suffix_engine.set_text_color(secondary);
                suffix_engine.set_cursor_color(secondary);
                suffix_engine.set_selection_color([0.0, 0.0, 0.0, 0.0]);
            }
            suffix_engine.set_viewport(10_000.0, text_area_height);

            {
                let mut st = self.state().borrow_mut();
                st.suffix_engine = Some(suffix_engine);
            }
            // Initial layout from the current suffix value.
            let initial = self.state().borrow().suffix.clone();
            relayout_suffix(self.state(), &initial);
        }

        // Reactive suffix: observe the signal and re-lay out on
        // every change. `Relayout` dirty-tracking ensures the
        // surrounding layout sees the new `suffix_width` and the
        // text viewport narrows/widens accordingly.
        if let Prop::Bound(signal) = &self.suffix {
            let self_id = ctx.self_id();
            signal.bind_to(
                self_id,
                ctx.binding_registry(),
                teksilo_core::binding::BindingLevel::Relayout,
            );
            let state_for_effect = self.state().clone();
            ctx.effect(signal, move |new_text| {
                relayout_suffix(&state_for_effect, new_text);
            });
        }

        // Bind caret_visible for repaint.
        {
            let st = self.state().borrow();
            let caret_visible = st.caret_visible.clone();
            drop(st);
            let self_id = ctx.self_id();
            caret_visible.bind_to(
                self_id,
                ctx.binding_registry(),
                teksilo_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // Bind text_signal at RepaintOnly AND AccessibilityOnly.
        //
        // RepaintOnly: when the text changes by any route — local
        // typing, IME, clipboard paste, the ext→internal sync
        // effect firing because a composite parent (SpinBox etc.)
        // drove the bound signal — the field must redraw. During
        // typing the caret-blink signal already keeps the widget
        // repainting, which used to mask a missing repaint trigger
        // on programmatic text changes to an unfocused field. With
        // the explicit bind, no path depends on blink.
        //
        // AccessibilityOnly: screen readers see edits as soon as
        // the text signal updates, independent of whether a paint
        // happens this frame.
        {
            let st = self.state().borrow();
            let text_signal = st.text_signal.clone();
            drop(st);
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            text_signal.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::RepaintOnly,
            );
            text_signal.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // Bind the caret and the selection anchor at AccessibilityOnly.
        //
        // A caret-only move (an arrow, Home, End, Shift+arrow, Ctrl+A, a click)
        // edits nothing, so `text_signal` stays put; without these bindings
        // the node was never re-walked and every reader was told the caret
        // and selection of the last edit. `has_selection` is derived from the
        // two and needs no binding of its own. Same pairing as
        // `RichTextEditorBody`.
        {
            let st = self.state().borrow();
            let signals = [st.cursor_position.clone(), st.cursor_anchor.clone()];
            drop(st);
            for signal in &signals {
                signal.bind_to(
                    ctx.self_id(),
                    ctx.binding_registry(),
                    teksilo_core::binding::BindingLevel::AccessibilityOnly,
                );
            }
        }

        // Stash frame infrastructure handles and self_id.
        {
            let mut st = self.state().borrow_mut();
            st.frame_request = Some(ctx.frame_request_handle());
            st.frame_wake_at = Some(ctx.wake_at_handle());
            st.field_widget_id = Some(ctx.self_id());
        }

        self.mount_touch_selection(ctx);

        // Same dormancy discipline as `RichTextEditor`: a field parked in a
        // non-selected `Switcher` / `visible_when(false)` branch must not
        // keep the event loop awake (caret `wake_at`, frame-tick work,
        // window-active re-arm). See that widget's build for the full story.
        let activation = ctx.activation_signal(ctx.self_id());
        if activation.get() {
            ctx.request_frame();
        }

        {
            let state = self.state().clone();
            let interaction = self.interaction.clone();
            ctx.effect(&activation, move |&active| {
                if active {
                    // **Re-activated** — re-arm the frame loop. The dormant branch
                    // below does not re-arm `frame_request` (a parked surface has
                    // nothing to paint) and the frame-tick effect is skipped
                    // entirely while dormant, so nothing restarts the tick on the
                    // way back. Same defect and same fix as `RichTextEditor` /
                    // `CodeEditor`: the in-tree modal path builds content, parks it
                    // dormant, mounts it, activates it and *then* moves focus in
                    // (`present_in_tree_modal_request`), so without this a field in
                    // a dialog draws no caret at all.
                    let st = state.borrow();
                    if let Some(handle) = &st.frame_request {
                        handle.set(true);
                    }
                    return;
                }
                let mut st = state.borrow_mut();
                if st.has_focus {
                    st.has_focus = false;
                    st.focus_signal.set(false);
                    // Mirror the on_focus(false) interaction write so a
                    // Focused chrome style doesn't stick on a parked field.
                    interaction.set(InteractionState::Idle);
                }
                if st.caret_visible.get() {
                    st.caret_visible.set(false);
                }
                st.blink.reset();
            });
        }

        // Frame-tick effect: flushes pending chars, drains document
        // events, drives the caret blink, and debounces undo/redo
        // state changes.
        //
        // IMPORTANT: the mutable borrow must be dropped BEFORE
        // setting `text_signal`. Setting it fires observers
        // synchronously, which chain into the ext→internal sync
        // effect that borrows the same state. Holding `borrow_mut`
        // across `signal.set()` would panic.
        {
            let state = self.state().clone();
            let active = activation.clone();
            let tick_signal = ctx.frame_tick();
            ctx.effect(&tick_signal, move |delta| {
                if !active.get() {
                    return;
                }
                let (more, pending_text) = {
                    let mut st = state.borrow_mut();
                    let more = tick(&mut st, *delta);
                    let pending = st.deferred_text_update.take();
                    (more, pending)
                };
                if let Some(text) = pending_text {
                    let st = state.borrow();
                    if st.text_signal.get() != text {
                        st.text_signal.set(text);
                    }
                }
                // An edit applied by `tick` (typed characters, a paste, a
                // programmatic rewrite, undo) moves the caret without passing
                // through a key handler's `sync_cursor_signals`. Publish it
                // here, or the signals keep the pre-edit caret, and the next
                // arrow key that lands on that stale value changes no signal
                // and reaches no reader.
                {
                    let st = state.borrow();
                    publish_cursor_signals(&st);
                    // The walk that revealed or hid a secure field published
                    // no runs; ask for the one that publishes its new text.
                    if st.at_published.get() == AtPublished::Withheld {
                        st.at_republish.set(st.at_republish.get().wrapping_add(1));
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

        // Window-active effect — mirror the tree's window-active state onto the
        // field state so the frame loop (no context) can gate the caret, and
        // re-apply the window-aware selection colour (reading the live theme,
        // since `ctx.effect` can't observe a derived theme×active signal). The
        // loop may not tick while the window is inactive (animation scheduler
        // parked), so on deactivation hide the caret synchronously here and
        // request a frame so it reaches a paint pass — only while this field
        // is itself active (a dormant field must not re-arm the loop).
        {
            let state = self.state().clone();
            let active = activation.clone();
            let wa_signal = ctx.window_active_signal();
            let theme_for_sel = theme_signal.clone();
            let touch_for_window = self.touch.clone();
            ctx.effect(&wa_signal, move |&window_active| {
                let mut st = state.borrow_mut();
                st.window_active = window_active;
                let theme = theme_for_sel.get();
                let tint = field_selection_color(&theme.colors, window_active, st.has_focus);
                st.selection_tint = tint;
                st.engine.set_selection_color(tint);
                if window_active {
                    // Reactivated: show the caret immediately if still focused
                    // (restart the blink phase), rather than waiting one interval.
                    if st.has_focus && !st.caret_visible.get() {
                        st.caret_visible.set(true);
                    }
                    st.blink.reset();
                } else {
                    // Deactivated: hide the caret synchronously (the frame loop
                    // may not tick while the window is inactive).
                    if st.caret_visible.get() {
                        st.caret_visible.set(false);
                    }
                    st.blink.reset();
                    // …and retire the touch affordances with it. Handles over an
                    // inactive window's text are as wrong as a caret in it.
                    touch_for_window.dismiss();
                }
                if active.get()
                    && let Some(handle) = &st.frame_request
                {
                    handle.set(true);
                }
            });
        }

        // Forward the enabled state into the arena. Disabled state no
        // longer seeded into the interaction signal — the framework's
        // arena enabled-state is the single source of truth (events
        // gated, leaves resolve Disabled role).
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        // Attach handlers. Focus-origin inference mirrors the
        // `Slider` pattern: hover cached, focus event checks hover
        // to distinguish keyboard vs pointer origin for the
        // select-all-on-keyboard-focus rule.
        let hovered = std::rc::Rc::new(std::cell::Cell::new(false));
        let hovered_for_focus = hovered.clone();
        let hovered_for_hover = hovered.clone();

        let state_for_focus = self.state().clone();
        let touch_for_focus = self.touch.clone();
        let interaction_for_focus = self.interaction.clone();
        // The selection band's tint depends on focus, so the focus handler has
        // to re-apply it — and needs the live theme to do so.
        let theme_for_focus = theme_signal.clone();
        let state_for_pointer = self.state().clone();
        let state_for_hold = self.state().clone();
        let touch_for_pointer = self.touch.clone();
        let touch_for_hold = self.touch.clone();
        let state_for_key = self.state().clone();
        let touch_for_key = self.touch.clone();
        let state_for_double = self.state().clone();
        let state_for_triple = self.state().clone();
        let touch_for_double = self.touch.clone();
        let touch_for_triple = self.touch.clone();
        let state_for_access = self.state().clone();
        let touch_for_access = self.touch.clone();
        let state_for_menu = self.state().clone();

        let handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Text)
            // Secure fields declare `ImePurpose::Password` so the platform
            // suppresses the IME's learning dictionary / candidate history —
            // the OS input method stays enabled (a password must still be
            // composable in a non-Latin script), and masking the preedit is
            // this widget's own job. Read by the platform IME layer at
            // focus-change time; an unset descriptor (the node default) is
            // what means "no OS IME".
            .ime_input(if self.secure {
                teksilo_core::ime::ImeContext::password()
            } else {
                teksilo_core::ime::ImeContext::text()
            })
            .on_hover(move |entered, _ctx| {
                hovered_for_hover.set(entered);
            })
            .on_focus(move |gained, ctx| {
                interaction_for_focus.set(if gained {
                    InteractionState::Focused
                } else {
                    InteractionState::Idle
                });

                let mut st = state_for_focus.borrow_mut();
                st.has_focus = gained;
                st.focus_signal.set(gained);
                // Re-tint the selection band: `has_focus` is half of what
                // decides it, so losing focus inside an active window has to
                // re-apply just as losing the window does.
                let sel_theme = theme_for_focus.get();
                let tint = field_selection_color(&sel_theme.colors, st.window_active, gained);
                st.selection_tint = tint;
                st.engine.set_selection_color(tint);
                // RevealWhileTyping shows plaintext while focused and
                // re-masks on blur — both transitions need a relayout.
                if st.secure && st.echo_mode == EchoMode::RevealWhileTyping {
                    st.needs_full_layout = true;
                }
                let mut blur_callback: Option<Rc<CommandFactory>> = None;
                if gained {
                    st.blink.restart();
                    st.caret_visible.set(true);
                    let is_keyboard = !hovered_for_focus.get();
                    drop(st);
                    if is_keyboard {
                        let st = state_for_focus.borrow();
                        st.cursor.select(SelectionType::Document);
                        drop(st);
                        sync_cursor_signals(&state_for_focus);
                    }
                    // Seed the OS IME candidate area at the caret so the
                    // first composition appears in the right place.
                    keyboard::report_ime_cursor_area(&state_for_focus, ctx);
                } else {
                    // Preserve `cursor`'s selection across focus loss
                    // — clearing it here breaks the right-click
                    // context menu path (the framework focuses the
                    // newly-mounted menu, which dispatches `FocusLost`
                    // here, and `Cut` / `Copy` invoked from the menu
                    // afterwards find an empty selection). A Win32
                    // edit control does the same: the default (no
                    // `ES_NOHIDESEL`) hides the highlight on blur but
                    // `EM_GETSEL` still returns the range, so the
                    // menu invoked afterwards still has something to
                    // act on. The *painting* is the part that stops —
                    // see `field_selection_color`, which returns a
                    // transparent band for an unfocused field.
                    st.scroll_x = 0.0;
                    st.caret_visible.set(false);
                    st.drag_state = state::DragState::Idle;
                    // Drop the IME-area dedup cache. The OS candidate area is a
                    // single per-window resource a sibling field may re-point
                    // while we are unfocused; clearing this forces the next
                    // focus-gain report to re-seed it instead of being deduped.
                    st.last_ime_area = None;
                    blur_callback = st.on_blur.clone();
                    drop(st);
                    // The band is exempt from outside-press dismissal, so the
                    // host owns this.
                    //
                    // Measured, and worth saying: deleting this line reddens
                    // nothing, because the framework retires an overlay when
                    // focus leaves the widget it is anchored on
                    // (`dismiss_overlays_left_by_focus`), and the affordance
                    // overlays are anchored on this field — so their `on_dismiss`
                    // callbacks reach the same conclusion. That rule walks *one*
                    // overlay's parent chain, though, and a field inside a dialog
                    // anchors two sibling overlays rather than a cascade, which
                    // is not a shape the host can verify from here. The call is
                    // what makes the answer the host's own.
                    touch_for_focus.dismiss();
                    // Abandon any in-progress composition on blur — remove
                    // the tentative preedit text from the document.
                    keyboard::clear_ime_preedit(&state_for_focus);
                    sync_cursor_signals(&state_for_focus);
                }
                if let Some(cb) = blur_callback {
                    cb(ctx);
                }
                ctx.request_frame();
            })
            .on_pointer_event(move |event, ctx| {
                mouse::handle_pointer_event(&state_for_pointer, &touch_for_pointer, event, ctx)
            })
            // A hold selects the word under the finger. Attaching this
            // **withdraws** the tree-owned long-press route (`touch_route`
            // rule 1: a widget's own `on_long_press` wins), which is what used
            // to open this field's context menu for a coarse pointer — the
            // selection toolbar `FieldTouch::raise` puts up is its replacement,
            // and offers the same commands.
            .on_long_press(move |event, ctx| {
                mouse::handle_long_press(&state_for_hold, &touch_for_hold, event, ctx);
            })
            .on_key(move |event, ctx| {
                let response = keyboard::handle_key(&state_for_key, event, ctx);
                // A keystroke moves the caret and edits the text, neither of
                // which the controller made — so the handles it published are
                // pointing at where the text used to be. `refresh` is a no-op
                // until something has been raised.
                touch_for_key.refresh(ctx, touch::ToolbarIntent::Hide);
                response
            })
            .on_double_tap(move |event, ctx| {
                mouse::handle_double_tap(&state_for_double, event.position, ctx);
                // A finger can double-tap too, and the selection it just made is
                // one the controller did not make.
                if event.pointer.kind.is_direct() {
                    touch_for_double.raise(ctx, touch::ToolbarIntent::Show);
                }
            })
            .on_triple_tap(move |event, ctx| {
                mouse::handle_triple_tap(&state_for_triple, event.position, ctx);
                if event.pointer.kind.is_direct() {
                    touch_for_triple.raise(ctx, touch::ToolbarIntent::Show);
                }
            })
            .on_access_action_request(move |action, _target_node, data, ctx| {
                let response = handle_access_action(&state_for_access, action, data, ctx);
                touch_for_access.refresh(ctx, touch::ToolbarIntent::Keep);
                response
            })
            // Right-click context menu — built fresh per click so the
            // enabled state of each item reflects the live selection /
            // clipboard state at the moment the menu opens. The framework
            // handles overlay placement, focus restoration, and dismissal.
            .context_menu(move |position, ctx| {
                let _ = ctx;
                // Framework gates pointer events on `arena.is_enabled`
                // before reaching this closure — a disabled field
                // never receives the right-click that would open the
                // context menu.
                // Reposition the caret to the click position when the
                // click lands outside the existing selection — the
                // platform convention for "right-click then Cut /
                // Copy / Paste at the new caret".
                mouse::reposition_caret_for_context_menu(&state_for_menu, position);
                Some(build_context_menu_widget(&state_for_menu))
            });

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Default unwrap is the cached natural width (mask-aware when
        // a mask is set; 200 dp fallback otherwise). Composing widgets
        // that wrap us in a constraint pass `Some(width)` and we use
        // that; the natural width is what surfaces in unconstrained
        // intrinsic queries (ZStack measurement with `unspecified()`,
        // etc.) so the chain reports a sensible content size.
        //
        // The cached `natural_width` / `text_height` are 1.0-scale baselines;
        // multiply by `ctx.text_scale` so the field box grows with the global
        // accessibility text scale (the engine grows the glyphs to match — see
        // `paint`). A caller-supplied width constraint is honored as-is.
        let scale = ctx.text_scale;
        let w = proposal
            .width
            .unwrap_or(self.natural_width * scale)
            .max(0.0);
        let h = (self.text_height.unwrap_or(DEFAULT_TEXT_HEIGHT) * scale).max(0.0);
        Size::new(w, h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Layout runs before paint, so this is the authoritative point to adopt
        // the field's viewport. `sync_viewport` welds the width write to the
        // `needs_full_layout` flag it also serves as the detector for (see its
        // docs); paint calls it again as an idempotent echo.
        if let Some(state) = self.state.as_ref() {
            state.borrow_mut().sync_viewport(bounds);
        }

        if let Some(backend) = ctx.text_backend {
            self.retain_text_geometry(
                bounds,
                &ctx.theme.typography.body,
                backend,
                base_text_direction(ctx.layout_direction),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let mut st = state.borrow_mut();

        // Grow the shaped text with the global accessibility scale. Must run
        // before the relayout block below so the larger glyphs are shaped this
        // frame; no-op when the scale is unchanged.
        st.apply_font_scale(ctx.text_scale);
        // Idempotent echo — `place_children` already adopted these exact bounds
        // during layout, so this is normally a no-op.
        st.sync_viewport(bounds);

        // Resolve the glyph / caret / suffix colours against the *effective*
        // enabled state, exactly as `TextWidget` and `RectWidget` resolve a
        // `ColorProp` at paint time. `paint` is the single writer of these:
        // the field shapes through a `RichTextEngine`, which takes raw GPU
        // colours and so never passes through `ColorProp::resolve` — the
        // disabled substitution that greys every role-driven leaf for free
        // cannot reach it. Doing it here (rather than as a build-time effect
        // on `effective_enabled_signal`) is also the only correct option:
        // that signal is *derived* whenever an ancestor binds `enabled`, and
        // `Signal::observe` panics on derived signals. Cheap — the engine
        // stores the colour and the render-frame builder reads it, so there
        // is no relayout and no reshaping.
        let text_color = if ctx.effective_enabled {
            ctx.theme.colors.text_primary
        } else {
            ctx.theme.colors.text_disabled
        };
        st.engine.set_text_color(text_color.to_array());
        st.engine.set_cursor_color(text_color.to_array());

        let suffix_width = st.suffix_width;
        let text_viewport_width = (bounds.width - suffix_width).max(0.0);

        st.engine.set_viewport(10_000.0, bounds.height);

        if st.needs_full_layout || !st.engine.has_full_layout() {
            st.layout_full_masked();
            st.needs_full_layout = false;
            st.content_dirty = true;
        }

        // Suppress the caret in an inactive window for every paint — the
        // authoritative gate, covering the frame between a window-active flip
        // and the build-time effect running.
        let caret_on = st.caret_visible.get() && st.has_focus && st.window_active;
        // `NoEcho` while masked lays out an *empty* source, so the real
        // document cursor (which may sit past 0) must not be handed to
        // the engine — pin the displayed caret/selection to the start.
        // The real `cursor` still tracks the true position for editing.
        let hide_all = st.echo_mode == EchoMode::NoEcho && st.should_mask();
        let (disp_pos, disp_anchor) = if hide_all {
            (0, 0)
        } else {
            (st.cursor.position(), st.cursor.anchor())
        };
        // Single-line input has no wrap → affinity is moot; the
        // default Downstream matches pre-affinity behavior.
        let cursor_display = CursorDisplay {
            position: disp_pos,
            anchor: disp_anchor,
            affinity: CursorAffinity::Downstream,
            visible: caret_on,
            selected_cells: Vec::new(),
        };
        st.engine.set_cursor(&cursor_display);

        ensure_caret_visible_h(&mut st, text_viewport_width, &ctx.theme.input);

        let scroll_x = st.scroll_x;

        let text_clip = Rect::new(bounds.x, bounds.y, text_viewport_width, bounds.height);
        canvas.set_clip(text_clip);

        {
            let state_ref: &mut TextInputState = &mut st;
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
                        // No inline images on this surface, so none can be missing.
                        image_resolver: None,
                        selection: None,
                        selection_color: [0.0; 4],
                        selected_image_out: None,
                        resize_preview: None,
                        draw_caret: caret_on,
                    },
                );
            });
        }

        // IME preedit underline: a thin line under the composing range so
        // the user sees the text is tentative. Single line → one segment;
        // on a secure field it sits under the masked bullets. Drawn inside
        // the text clip so it never spills past the viewport.
        if let Some(range) = st.ime_preedit_range.clone()
            && st.engine.has_full_layout()
            && range.start < range.end
        {
            let start_c = st
                .engine
                .caret_rect(range.start, CursorAffinity::Downstream);
            let end_c = st.engine.caret_rect(range.end, CursorAffinity::Downstream);
            let x0 = bounds.x - scroll_x + start_c[0];
            let x1 = bounds.x - scroll_x + end_c[0];
            let y = bounds.y + start_c[1] + start_c[3] - 1.0;
            canvas.draw_line(
                Point::new(x0, y),
                Point::new(x1, y),
                ctx.theme.colors.text_primary,
                teksilo_canvas::StrokeStyle::solid(1.0),
            );
        }

        canvas.clear_clip();

        if suffix_width > 0.0
            && let Some(suffix_engine) = st.suffix_engine.as_mut()
        {
            // The suffix dims with the value it annotates — a crisp " %"
            // beside greyed-out digits reads as a rendering bug.
            let suffix_color = if ctx.effective_enabled {
                ctx.theme.colors.text_secondary
            } else {
                ctx.theme.colors.text_disabled
            };
            suffix_engine.set_text_color(suffix_color.to_array());
            let suffix_clip = Rect::new(
                bounds.x + text_viewport_width,
                bounds.y,
                suffix_width,
                bounds.height,
            );
            canvas.set_clip(suffix_clip);
            let suffix_origin = Point::new(bounds.x + text_viewport_width, bounds.y);
            suffix_engine.with_render_frame(|frame| {
                paint_suffix_glyphs(canvas, frame, suffix_origin);
            });
            canvas.clear_clip();
        }

        // Re-measure for the accessibility pass. `place_children` is the
        // authority on the width, but an edit dirties the field at
        // `RepaintOnly` / `AccessibilityOnly` and never relayouts — so
        // without this echo every keystroke would leave the runs describing
        // the text as it was before it. Same shape as the `sync_viewport`
        // echo above; the backend caches by (text, style), so a frame that
        // only blinks the caret pays a lookup.
        drop(st);
        if let Some(backend) = canvas.text_backend() {
            self.retain_text_geometry(
                bounds,
                &ctx.theme.typography.body,
                backend,
                base_text_direction(ctx.layout_direction),
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use teksilo_core::accesskit::{Action, Role};

        let Some(state) = self.state.as_ref() else {
            return;
        };
        let st = state.borrow();

        let text = st.document.to_plain_text().unwrap_or_default();

        // AT-protection tracks the *explicit* reveal toggle only — not
        // the visual `RevealWhileTyping` focus-reveal (a sighted-only
        // convenience that a screen reader shouldn't surface as
        // plaintext, and that has no AT-dirty trigger on focus). The
        // reveal signal is bound at AccessibilityOnly in `build`, so the
        // role/value swap reaches AT when it flips. `Role::PasswordInput`
        // is the sole mechanism telling AT not to speak the value —
        // accesskit has no separate `protected` flag.
        let explicitly_revealed = st.revealed.as_ref().is_some_and(|s| s.get());
        let protected = st.secure
            && match st.at_reveal_policy {
                AtRevealPolicy::AlwaysProtected => true,
                AtRevealPolicy::SwapRole => !explicitly_revealed,
            };

        // What a reader may read. A protected field shows it what a sighted
        // user sees: the mask, one echo character per character of the secret
        // (nothing at all under `NoEcho`), and never the plaintext.
        let hide_all = protected && st.echo_mode == EchoMode::NoEcho;
        // Revealing or hiding a secure field: publish no runs this walk, so
        // the adapter has no plaintext to report as inserted or deleted (see
        // `AtPublished`), and have the frame tick ask for the walk that
        // publishes the new text.
        let withhold = st.secure
            && matches!(
                (st.at_published.get(), protected),
                (AtPublished::Plaintext, true) | (AtPublished::Mask, false)
            );
        if !builder.emits_no_children() {
            st.at_published.set(if withhold {
                AtPublished::Withheld
            } else if protected {
                AtPublished::Mask
            } else {
                AtPublished::Plaintext
            });
            if withhold && let Some(frame) = &st.frame_request {
                frame.set(true);
            }
        }
        let at_text = if !protected {
            text
        } else if hide_all {
            String::new()
        } else {
            st.echo_char.to_string().repeat(text.chars().count())
        };

        if protected {
            builder.set_role(Role::PasswordInput);
        } else {
            // Plain field, or a revealed field under `SwapRole`: report
            // as a text input exposing the real value, mirroring the web
            // `type=password ↔ type=text` swap. The specialised role from
            // `input_purpose` (WCAG 1.3.5) applies here; `Role::TextInput` is
            // the `Normal` default.
            builder.set_role(self.input_purpose.to_role());
        }
        // Keep the value on the input node so the focus announcement is
        // unchanged: accesskit resolves `value()` from `data().value()`
        // first, falling back to the TextRun text only when unset.
        if !at_text.is_empty() {
            builder.set_value(&at_text);
        }

        // Expose the content as child `Role::TextRun`s, NOT as
        // `character_lengths` on the input node itself. accesskit_consumer's
        // `supports_text_ranges()` is false for a childless input that only
        // hosts character data on its own node, so the macOS adapter never
        // fires `AXSelectedTextChanged` (VoiceOver reads the value once on
        // focus but never echoes characters/words while typing) and AT-SPI
        // publishes no Text interface. Runs are emitted even for an empty
        // field so `supports_text_ranges()` is already true before the first
        // keystroke (the change-diff's *old* node must support ranges too for
        // the notification to fire). A masked field needs them as much: Orca
        // 46.1 speaks no key in password text (`default.py`,
        // `presentKeyboardEvent`) and echoes a keystroke there only from the
        // text-inserted event (`script_utilities.py`,
        // `isEchoableTextInsertionEvent`), which the mask's runs raise.
        let retained = self.retained.borrow();
        let source = match retained.as_ref() {
            // A retained measurement describes the text it was taken of.
            // The document can move on between two layouts, so compare
            // rather than trust — a run whose ranges index a text that no
            // longer exists is worse than one with no extents.
            Some(placed) if placed.text == at_text => match placed.geometry.as_deref() {
                // The measurement covers the whole line; the field shows a
                // window onto it, so slide the rects back by the scroll
                // offset to land in the field's own space. `build`
                // translates them into window space from there.
                Some(geometry) => TextRunSource::from_geometry(
                    &placed.text,
                    geometry,
                    Point::new(-st.scroll_x, 0.0),
                    0,
                )
                .with_base_direction(placed.base_direction),
                None => TextRunSource::flat(&placed.text, 0)
                    .with_fallback_rect(Rect::new(
                        0.0,
                        0.0,
                        placed.bounds.width,
                        placed.bounds.height,
                    ))
                    .with_base_direction(placed.base_direction),
            },
            // Never placed, painted without a measuring backend, or one
            // edit ahead of the last measurement.
            _ => TextRunSource::flat(&at_text, 0),
        };
        let emission = if withhold {
            TextRunEmission::default()
        } else {
            push_text_runs(builder, None, &source)
        };

        // While composing (IME preedit active), expose the composition
        // as a selection so screen readers / braille track the tentative
        // text — the composing characters are already in `value`. Falls
        // back to the live cursor/selection when not composing. A protected
        // field reports its caret only: the mask has one character per
        // character, so the offsets are the same, and under `NoEcho` the
        // caret stays at the start of a text that is always empty, as the
        // painted caret does. `position()` / `anchor()` are character
        // indices (text-document is char-space), which the emission maps
        // onto the run that holds them — a caret past 255 characters is
        // in the second run, at its own offset.
        let (anchor, pos) = match st.ime_preedit_range.clone() {
            _ if hide_all => (0, 0),
            Some(range) if !protected => (range.start, range.end),
            _ => (st.cursor.anchor(), st.cursor.position()),
        };
        if let (Some(anchor), Some(focus)) =
            (emission.position_of(anchor), emission.position_of(pos))
        {
            builder.set_text_selection_to(anchor, focus);
        }

        if !st.placeholder.is_empty() {
            builder.set_placeholder(st.placeholder.clone());
        }

        if st.read_only {
            builder.set_read_only();
        }

        builder.add_action(Action::Focus);
        if !st.read_only {
            builder.add_action(Action::SetValue);
            builder.add_action(Action::ReplaceSelectedText);
        }
        // Only meaningful when the caret model is exposed to AT.
        if !hide_all {
            builder.add_action(Action::SetTextSelection);
        }

        // Validation feedback → accesskit `aria-invalid`. Surface
        // `Invalid` as `Invalid::True`; `Corrected` doesn't carry an
        // invalid marker (the data is now valid) but the composite's
        // Live region announces the correction. The framework's
        // AccessNodeBuilder doesn't yet wrap `set_invalid`, so reach
        // through `inner_mut()` which is the documented escape hatch.
        if self.feedback.get().is_invalid() {
            builder
                .inner_mut()
                .set_invalid(teksilo_core::accesskit::Invalid::True);
        }

        // ARIA combobox wiring. This node is the one that actually holds
        // keyboard focus, which is why the relation is published here and not
        // on whichever composite owns the list — AT follows the *focused*
        // node's active descendant.
        if let Some(listbox) = self.controls.as_ref().and_then(|s| s.get()) {
            builder.push_controlled(teksilo_core::accessibility::widget_id_to_node_id(listbox));
        }
        if let Some(active) = self.active_descendant.as_ref().and_then(|s| s.get()) {
            builder
                .inner_mut()
                .set_active_descendant(teksilo_core::accessibility::widget_id_to_node_id(active));
        }
    }
}
