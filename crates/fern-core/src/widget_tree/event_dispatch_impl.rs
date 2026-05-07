use super::*;

use crate::gesture::{GestureArena, GestureEvent, RawPointerEvent};

/// Fire an `EventResponse`-returning handler from BOTH the external
/// and own slots (in that order). Returns `Handled` if either did,
/// `Ignored` otherwise. `None` slots are skipped.
fn fire_event_handler_both(
    external: &mut Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    own: &mut Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let r1 = external
        .as_mut()
        .map(|h| h(event, ctx))
        .unwrap_or(EventResponse::Ignored);
    let r2 = own
        .as_mut()
        .map(|h| h(event, ctx))
        .unwrap_or(EventResponse::Ignored);
    if r1 == EventResponse::Handled || r2 == EventResponse::Handled {
        EventResponse::Handled
    } else {
        EventResponse::Ignored
    }
}

impl WidgetTree {
    /// Dispatch an event into the widget tree.
    ///
    /// Routing rules (architecture Section 7.1):
    /// - Pointer events -> hit testing against layout tree
    /// - Keyboard/IME events -> focused widget
    /// - AccessKit actions -> target widget directly
    /// - Scroll events -> hit testing (scroll target under pointer)
    ///
    /// Dispatch an event with the caller-supplied app-level
    /// [`WindowOps`](crate::window::WindowOps) sink. `fern-app` calls
    /// this variant; handlers can reach the multi-window API
    /// synchronously (`open_window` creates the winit window inside
    /// the same call before returning).
    pub fn dispatch_event_with_ops(
        &mut self,
        event: WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        self.dispatch_event_impl(event, ops)
    }

    /// Dispatch an event on a standalone tree (tests, headless
    /// scenarios). Handler code that calls `ctx.open_window(...)`
    /// from within this dispatch will panic — by design. See
    /// [`dispatch_event_with_ops`](Self::dispatch_event_with_ops)
    /// for the app-facing variant.
    pub fn dispatch_event(&mut self, event: WidgetEvent) {
        let mut noop = crate::window::NoopWindowOps;
        self.dispatch_event_impl(event, &mut noop);
    }

    fn dispatch_event_impl(&mut self, event: WidgetEvent, ops: &mut dyn crate::window::WindowOps) {
        if let WidgetEvent::KeyDown {
            key: Key::ArrowLeft,
            ..
        } = &event
            && self.overlay_manager.len() > 1
        {
            if let Some((_id, content_ids, focus_restore)) = self.overlay_manager.dismiss_top() {
                self.dormant_dismissed_content(&content_ids, &mut *ops);
                if let Some(restore_id) = focus_restore
                    && self.arena.is_active(restore_id)
                {
                    self.focus_ops(restore_id, &mut *ops);
                }
            }
            return;
        }

        if let WidgetEvent::KeyDown {
            key: Key::Escape, ..
        } = &event
            && !self.overlay_manager.is_empty()
            && let Some((_id, content_ids, focus_restore)) =
                self.overlay_manager.try_dismiss_top_on_escape()
        {
            self.dormant_dismissed_content(&content_ids, &mut *ops);
            if let Some(restore_id) = focus_restore
                && self.arena.is_active(restore_id)
            {
                self.focus_ops(restore_id, &mut *ops);
            }
            return;
        }

        if let WidgetEvent::PointerDown {
            position, button, ..
        } = &event
        {
            let (dismissed, focus_restore) = self.overlay_manager.handle_click_outside(*position);
            if !dismissed.is_empty() {
                self.dormant_dismissed_content(&dismissed, &mut *ops);
                if let Some(restore_id) = focus_restore
                    && self.arena.is_active(restore_id)
                {
                    self.focus_ops(restore_id, &mut *ops);
                }
                if *button != PointerButton::Secondary {
                    return;
                }
            }
        }

        // Key-capture mode: if a callback is armed (via
        // `WidgetTree::begin_key_capture`), the next KeyDown bypasses
        // shortcut resolution entirely and runs the callback with
        // mutable access to the registry AND an `EventContext` so
        // rebind handlers can also emit commands, send intents,
        // dismiss overlays, etc. The capture is one-shot; its slot
        // is emptied before the callback runs so a re-entrant
        // `begin_key_capture` call from inside the callback arms a
        // fresh session (rather than competing with the in-flight
        // one).
        if let WidgetEvent::KeyDown { key, modifiers, .. } = &event
            && let Some(callback) = self.take_key_capture()
        {
            let keystroke = crate::shortcut::KeyStroke::new(*key, *modifiers);
            let mut cap_ctx = self.make_event_context(&mut *ops);
            callback(keystroke, self.shortcut_registry_mut(), &mut cap_ctx);
            // Route side effects of the callback through the
            // focused widget (or an arbitrary root if no focus).
            let anchor = self.focused.or_else(|| self.arena.roots().first().copied());
            if let Some(anchor_id) = anchor {
                self.collect_from_ctx(cap_ctx, anchor_id);
                self.drain_pending_intents(&mut *ops);
            }
            return;
        }

        // Shortcut → intent → action dispatch. A KeyDown whose chord
        // matches a registered enabled `Shortcut` whose scope contains
        // the focused widget is consumed here: the shortcut's
        // `on_activate` runs (producing an `Intent`), its ctx side
        // effects are collected, and the intent walks source-widget →
        // root firing any matching `Action`. Otherwise the focused
        // widget sees the raw KeyDown below.
        //
        // Two-phase: the registry is inspected first (immutable read)
        // to resolve `id / scope / propagate_when_disabled`. Only if
        // scope matches the current focus do we take a mutable borrow
        // to invoke `on_activate` — this way a scope mismatch cannot
        // drop side effects the closure put into its ctx, because
        // the closure never runs.
        if let WidgetEvent::KeyDown { key, modifiers, .. } = &event {
            let keystroke = crate::shortcut::KeyStroke::new(*key, *modifiers);
            let lookup = self
                .shortcut_registry
                .find_by_keystroke(keystroke)
                .map(|eff| {
                    (
                        eff.shortcut.id,
                        eff.shortcut.scope,
                        eff.shortcut.propagate_when_disabled,
                    )
                });
            if let Some((id, scope, propagate_when_disabled)) = lookup {
                let anchor = match scope {
                    // Global shortcuts fire regardless of focus. If no
                    // widget is currently focused, anchor the intent
                    // walk at an arbitrary root so actions registered
                    // at the top of the tree still see the intent.
                    crate::shortcut::ShortcutScope::Global => {
                        self.focused.or_else(|| self.arena.roots().first().copied())
                    }
                    crate::shortcut::ShortcutScope::Scoped(scope_id) => {
                        self.focused.filter(|f| self.is_descendant_of(*f, scope_id))
                    }
                };
                if let Some(anchor_id) = anchor {
                    let mut act_ctx = self.make_event_context(&mut *ops);
                    if let Some(intent) =
                        self.shortcut_registry
                            .invoke_on_activate(id, keystroke, &mut act_ctx)
                    {
                        self.collect_from_ctx(act_ctx, anchor_id);
                        // Phase 5.2: tag shortcut origin so analytics can
                        // distinguish keyboard-driven activations from
                        // button / menu / programmatic ones.
                        let intent = intent.with_source(crate::telemetry::IntentSource::Shortcut);
                        self.enqueue_intent(anchor_id, intent, propagate_when_disabled);
                        self.drain_pending_intents(&mut *ops);
                        return;
                    }
                }
                // Scope mismatch — fall through to normal KeyDown
                // dispatch. `on_activate` was never called, so nothing
                // to clean up.
            }
        }

        // --- Active drag session handling ---
        if self.active_drag.is_some() {
            match &event {
                WidgetEvent::PointerMove { position } => {
                    self.handle_drag_move(*position, &mut *ops);
                    return;
                }
                WidgetEvent::PointerUp { position, .. } => {
                    self.handle_drag_drop(*position, &mut *ops);
                    return;
                }
                WidgetEvent::KeyDown {
                    key: Key::Escape, ..
                } => {
                    self.cancel_active_drag(&mut *ops);
                    return;
                }
                WidgetEvent::Scroll { .. } => {
                    // Route the wheel to the current drop target so users
                    // can scroll the list/tree beneath the drag. Then
                    // synthesise a hover at the stationary pointer so
                    // feedback, drop-index math and the preview overlay
                    // all reflect the new scroll offset.
                    let target_and_pos = self
                        .active_drag
                        .as_ref()
                        .and_then(|d| d.current_target.map(|t| (t, d.current_position)));
                    if let Some((target, _pos)) = target_and_pos {
                        self.dispatch_to_widget(target, &event, &mut *ops);
                    }
                    if let Some((_, pos)) = target_and_pos
                        && self.active_drag.is_some()
                    {
                        self.handle_drag_move(pos, &mut *ops);
                    }
                    return;
                }
                _ => {}
            }
        }

        match &event {
            WidgetEvent::PointerMove { position } => {
                if let Some(captured) = self.pointer_captured_by {
                    self.dispatch_to_widget(
                        captured,
                        &WidgetEvent::PointerMove {
                            position: *position,
                        },
                        &mut *ops,
                    );
                } else {
                    self.handle_pointer_move(*position, &mut *ops);
                }
                self.update_pointer_leave_overlays(*position, &mut *ops);
            }
            WidgetEvent::PointerDown {
                position, button, ..
            } => {
                if let Some(target) = self.hit_test(*position) {
                    if *button == PointerButton::Secondary
                        && self.show_context_menu_for(target, *position, &mut *ops)
                    {
                        return;
                    }
                    if let Some(focusable) = self.find_focusable_at_or_above(target) {
                        self.focus_with_origin_ops(
                            focusable,
                            crate::focus::FocusOrigin::Pointer,
                            &mut *ops,
                        );
                    }
                    self.dispatch_to_widget(target, &event, &mut *ops);
                }
            }
            WidgetEvent::PointerUp { position, .. } => {
                if let Some(captured) = self.pointer_captured_by {
                    self.dispatch_to_widget(captured, &event, &mut *ops);
                    self.pointer_captured_by = None;
                } else if let Some(target) = self.hit_test(*position) {
                    self.dispatch_to_widget(target, &event, &mut *ops);
                }
            }
            WidgetEvent::Scroll { .. } => {
                if let Some(target) = self.hovered.or(self.focused) {
                    self.dispatch_to_widget(target, &event, &mut *ops);
                }
            }
            WidgetEvent::KeyDown { key, modifiers, .. } => {
                if *key == Key::Tab {
                    // Dispatch Tab to the focused widget first so
                    // ancestors (e.g. an open overlay that wants to
                    // close instead of moving focus out through its
                    // content) get a chance to intercept. Fall back to
                    // built-in focus cycling only when no handler
                    // returns `EventResponse::Handled`.
                    let handled = self
                        .focused
                        .map(|focused| {
                            self.dispatch_to_widget_returning_handled(focused, &event, &mut *ops)
                        })
                        .unwrap_or(false);
                    if !handled {
                        self.cycle_focus(modifiers.shift(), &mut *ops);
                    }
                } else if let Some(focused) = self.focused {
                    self.dispatch_to_widget(focused, &event, &mut *ops);
                }
            }
            WidgetEvent::KeyUp { .. }
            | WidgetEvent::ImeComposition { .. }
            | WidgetEvent::ImeCommit { .. } => {
                if let Some(focused) = self.focused {
                    self.dispatch_to_widget(focused, &event, &mut *ops);
                }
            }
            WidgetEvent::AccessAction { target, action, .. } => {
                if *action == accesskit::Action::Focus {
                    if let Some(id) = target.filter(|id| self.arena.is_active(*id)) {
                        self.focus_with_origin_ops(
                            id,
                            crate::focus::FocusOrigin::Programmatic,
                            &mut *ops,
                        );
                    }
                } else {
                    let dispatch_target = target
                        .filter(|id| self.arena.is_active(*id))
                        .or(self.focused);
                    if let Some(id) = dispatch_target {
                        self.dispatch_to_widget(id, &event, &mut *ops);
                    }
                }
            }
            WidgetEvent::Gesture { .. } => {
                if let Some(target) = self.hovered.or(self.focused) {
                    self.dispatch_to_widget(target, &event, &mut *ops);
                }
            }
            WidgetEvent::ScrollIntoView { .. }
            | WidgetEvent::PointerEnter
            | WidgetEvent::PointerLeave
            | WidgetEvent::FocusGained { .. }
            | WidgetEvent::FocusLost => {}
        }
        // Any intents queued by handlers via `ctx.send_intent(...)`
        // are dispatched after the raw event has been handled but
        // before commands are flushed, so commands emitted from
        // action handlers land on the same tick.
        self.drain_pending_intents(&mut *ops);
    }

    fn show_context_menu_for(
        &mut self,
        target: WidgetId,
        position: Point,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        // Walks up the parent chain calling each factory in turn. A
        // factory returning `Some(menu)` claims the click and mounts;
        // a factory returning `None` declines and the walk continues.
        // No factory anywhere on the chain → fall through to whatever
        // the caller does with the unconsumed PointerDown.
        let mut ctx = self.make_event_context(&mut *ops);
        let mut walker = Some(target);
        let menu_decision: Option<(WidgetId, Box<dyn Widget>)> = loop {
            // Walk to the next ancestor (including `walker` itself)
            // that owns a factory.
            let owner_id = {
                let mut probe = walker;
                loop {
                    match probe {
                        None => break None,
                        Some(id) => {
                            if self
                                .arena
                                .get(id)
                                .is_some_and(|node| node.context_menu_factory.is_some())
                            {
                                break Some(id);
                            }
                            probe = self.arena.get(id).and_then(|node| node.parent);
                        }
                    }
                }
            };
            let Some(owner_id) = owner_id else {
                break None;
            };
            // Invoke the factory with the click position and a real
            // EventContext. The factory is `Fn` (not FnMut), so we
            // can call it through an immutable borrow on the node.
            // `ctx` is a local — its `&mut WindowOps` lifetime is
            // disjoint from `self.arena`, so the immutable arena
            // borrow doesn't conflict with the mutable ctx borrow.
            let outcome: Option<Box<dyn Widget>> = {
                let node = self
                    .arena
                    .get(owner_id)
                    .expect("owner_id from active arena walk");
                let factory = node
                    .context_menu_factory
                    .as_ref()
                    .expect("owner_id only set when factory present");
                factory(position, &mut ctx)
            };
            match outcome {
                Some(menu) => break Some((owner_id, menu)),
                None => {
                    // Decline → keep walking up from the parent.
                    walker = self.arena.get(owner_id).and_then(|n| n.parent);
                }
            }
        };

        // Drain ctx side effects regardless of whether a menu showed —
        // a factory that returns `None` may still have queued intents,
        // updated signals, or requested a frame.
        let drain_anchor = menu_decision
            .as_ref()
            .map(|(id, _)| *id)
            .or_else(|| self.arena.roots().first().copied())
            .unwrap_or(target);
        self.collect_from_ctx(ctx, drain_anchor);

        let Some((owner_id, menu_widget)) = menu_decision else {
            return false;
        };

        let dismissed = self.overlay_manager.dismiss_all();
        self.dormant_dismissed_content(&dismissed, &mut *ops);

        let content_id = self.add_boxed(menu_widget);
        let prev_focus = self.focused;
        self.overlay_manager.show(crate::overlay::OverlayRequest {
            content_id,
            anchor: owner_id,
            placement: crate::overlay::OverlayPlacement::AtPointer(position),
            dismiss: crate::overlay::DismissBehavior::EscapeOrClickOutside,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        if let Some(focus_id) = prev_focus {
            self.overlay_manager.set_top_focus_restore(focus_id);
        }
        self.focus_ops(content_id, &mut *ops);
        // Flush intents the factory queued so they take effect on the
        // same dispatch tick as the menu mount. The caller's
        // PointerDown handler returns after we return `true`, skipping
        // its own drain — fire ours here.
        self.drain_pending_intents(&mut *ops);
        true
    }

    fn handle_pointer_move(&mut self, position: Point, ops: &mut dyn crate::window::WindowOps) {
        self.last_pointer_position = Some(position);
        let target = self.hit_test(position);

        if target != self.hovered {
            let previously_hovered = self.hovered;
            if let Some(old) = self.hovered {
                self.dispatch_to_widget(old, &WidgetEvent::PointerLeave, &mut *ops);
                self.tooltip_pointer_leave(old, &mut *ops);
            }
            if let Some(new) = target {
                self.dispatch_to_widget(new, &WidgetEvent::PointerEnter, &mut *ops);
                self.tooltip_pointer_enter(new);
            }
            self.set_hovered(target);
            self.update_hover_within_signals(previously_hovered, target);
        }

        if let Some(target) = target {
            self.dispatch_to_widget(target, &WidgetEvent::PointerMove { position }, &mut *ops);
        }
    }

    pub(super) fn dispatch_to_widget(
        &mut self,
        target: WidgetId,
        event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        self.dispatch_to_widget_returning_handled(target, event, ops);
    }

    /// Same as [`dispatch_to_widget`] but returns `true` when any
    /// preview or bubble handler consumed the event. Used for keyboard
    /// events the framework wants to consume by default (Tab focus
    /// navigation): callers can dispatch first, then fall back to
    /// built-in behavior only when no widget claimed it.
    pub(super) fn dispatch_to_widget_returning_handled(
        &mut self,
        target: WidgetId,
        event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        if !self.arena.is_enabled(target) {
            return false;
        }

        let mut ancestors = Vec::new();
        let mut current = self.arena.parent(target);
        while let Some(id) = current {
            ancestors.push(id);
            current = self.arena.parent(id);
        }
        ancestors.reverse();

        for &id in &ancestors {
            let mut ctx = self.make_event_context(&mut *ops);
            let response = if let Some(node) = self.arena.get_mut(id) {
                Self::try_handler_preview(node, event, &mut ctx).unwrap_or(EventResponse::Ignored)
            } else {
                EventResponse::Ignored
            };
            self.collect_from_ctx(ctx, id);
            if response == EventResponse::Handled {
                self.arena.mark_needs_paint(id);
                return true;
            }
        }

        let needs_layout_on_handle = matches!(
            event,
            WidgetEvent::Scroll { .. } | WidgetEvent::ScrollIntoView { .. }
        );
        let mut current = Some(target);
        let mut is_target = true;
        while let Some(id) = current {
            let mut ctx = self.make_event_context(&mut *ops);
            let response = if let Some(node) = self.arena.get_mut(id) {
                Self::try_handler_bubble(node, event, &mut ctx, is_target)
                    .unwrap_or(EventResponse::Ignored)
            } else {
                EventResponse::Ignored
            };
            self.collect_from_ctx(ctx, id);
            if response == EventResponse::Handled {
                if needs_layout_on_handle {
                    self.arena.mark_needs_layout(id);
                } else {
                    self.arena.mark_needs_paint(id);
                }
                return true;
            }
            is_target = false;
            current = self.arena.parent(id);
        }
        false
    }

    pub(super) fn dispatch_to_widget_direct(
        &mut self,
        target: WidgetId,
        event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if !self.arena.is_enabled(target) {
            return;
        }

        let mut ctx = self.make_event_context(&mut *ops);
        let response = if let Some(node) = self.arena.get_mut(target) {
            Self::try_handler_bubble(node, event, &mut ctx, true).unwrap_or(EventResponse::Ignored)
        } else {
            EventResponse::Ignored
        };
        self.collect_from_ctx(ctx, target);

        if response == EventResponse::Handled {
            self.arena.mark_needs_paint(target);
        }
    }

    fn try_handler_preview(
        node: &mut crate::arena::WidgetNode,
        event: &WidgetEvent,
        ctx: &mut EventContext,
    ) -> Option<EventResponse> {
        match event {
            // Key + IME events fire `on_key_preview` on each strict
            // ancestor of the focused widget (root → parent-of-target).
            // Mirrors how `on_pointer_event` previews on the pointer
            // side; the focused widget itself does NOT see its own
            // `on_key_preview` (the dispatch loop builds an ancestors
            // list that excludes the target, so this is enforced by
            // the caller, not here).
            WidgetEvent::KeyDown { .. }
            | WidgetEvent::KeyUp { .. }
            | WidgetEvent::ImeComposition { .. }
            | WidgetEvent::ImeCommit { .. } => {
                let has = node.external_handlers.on_key_preview.is_some()
                    || node.handlers.on_key_preview.is_some();
                if !has {
                    return None;
                }
                Some(fire_event_handler_both(
                    &mut node.external_handlers.on_key_preview,
                    &mut node.handlers.on_key_preview,
                    event,
                    ctx,
                ))
            }
            _ => {
                let has = node.external_handlers.on_pointer_event.is_some()
                    || node.handlers.on_pointer_event.is_some();
                if !has {
                    return None;
                }
                Some(fire_event_handler_both(
                    &mut node.external_handlers.on_pointer_event,
                    &mut node.handlers.on_pointer_event,
                    event,
                    ctx,
                ))
            }
        }
    }

    /// `fire_on_pointer_event` gates the pre-gesture `on_pointer_event`
    /// intercept. Set it to `true` for the bubble target (the widget the
    /// event was dispatched at) and `false` for every ancestor, because
    /// ancestors already fired their `on_pointer_event` during the
    /// preview pass — firing it again in bubble was the source of
    /// double-toggle / double-select bugs when a wrapper widget (e.g.
    /// `ListItemWrapper`) held the handler and a child leaf was the hit
    /// target.
    fn try_handler_bubble(
        node: &mut crate::arena::WidgetNode,
        event: &WidgetEvent,
        ctx: &mut EventContext,
        fire_on_pointer_event: bool,
    ) -> Option<EventResponse> {
        match event {
            WidgetEvent::PointerEnter => {
                if let Some(cursor) = node.node_cursor {
                    ctx.set_cursor(cursor);
                }
                let mut fired = false;
                if let Some(h) = node.external_handlers.on_hover.as_mut() {
                    h(true, ctx);
                    fired = true;
                }
                if let Some(h) = node.handlers.on_hover.as_mut() {
                    h(true, ctx);
                    fired = true;
                }
                if fired {
                    Some(EventResponse::Handled)
                } else {
                    node.node_cursor.map(|_| EventResponse::Handled)
                }
            }
            WidgetEvent::PointerLeave => {
                if node.node_cursor.is_some() {
                    ctx.set_cursor(crate::widget::CursorIcon::Default);
                }
                let mut fired = false;
                if let Some(h) = node.external_handlers.on_hover.as_mut() {
                    h(false, ctx);
                    fired = true;
                }
                if let Some(h) = node.handlers.on_hover.as_mut() {
                    h(false, ctx);
                    fired = true;
                }
                if fired {
                    Some(EventResponse::Handled)
                } else {
                    node.node_cursor.map(|_| EventResponse::Handled)
                }
            }
            WidgetEvent::FocusGained { .. } => {
                let mut fired = false;
                if let Some(h) = node.external_handlers.on_focus.as_mut() {
                    h(true, ctx);
                    fired = true;
                }
                if let Some(h) = node.handlers.on_focus.as_mut() {
                    h(true, ctx);
                    fired = true;
                }
                fired.then_some(EventResponse::Handled)
            }
            WidgetEvent::FocusLost => {
                let mut fired = false;
                if let Some(h) = node.external_handlers.on_focus.as_mut() {
                    h(false, ctx);
                    fired = true;
                }
                if let Some(h) = node.handlers.on_focus.as_mut() {
                    h(false, ctx);
                    fired = true;
                }
                fired.then_some(EventResponse::Handled)
            }
            WidgetEvent::KeyDown { .. }
            | WidgetEvent::KeyUp { .. }
            | WidgetEvent::ImeComposition { .. }
            | WidgetEvent::ImeCommit { .. } => {
                if node.external_handlers.on_key.is_some() || node.handlers.on_key.is_some() {
                    Some(fire_event_handler_both(
                        &mut node.external_handlers.on_key,
                        &mut node.handlers.on_key,
                        event,
                        ctx,
                    ))
                } else {
                    None
                }
            }
            WidgetEvent::Scroll { .. } | WidgetEvent::ScrollIntoView { .. } => {
                if node.external_handlers.on_scroll.is_some() || node.handlers.on_scroll.is_some() {
                    Some(fire_event_handler_both(
                        &mut node.external_handlers.on_scroll,
                        &mut node.handlers.on_scroll,
                        event,
                        ctx,
                    ))
                } else {
                    None
                }
            }
            WidgetEvent::AccessAction {
                action,
                target_node,
                data,
                ..
            } => {
                // Prefer the full-payload handler when the widget has
                // opted in; it's the one that receives `target_node` and
                // `data`. Within each payload variant, fire BOTH external
                // and own handlers — Button (own) and Dialog (external)
                // layered together rely on both firing for a single
                // accesskit click.
                // Phase 5.2 — assistive-tech action paths run under
                // the `Accessibility` source label. Restored after
                // the inner if/else.
                let saved_a11y_source = ctx
                    .current_source
                    .replace(crate::telemetry::IntentSource::Accessibility);
                let request_is_set = node.handlers.on_access_action_request.is_some()
                    || node.external_handlers.on_access_action_request.is_some();
                let user_handled = if request_is_set {
                    let r1 = node
                        .external_handlers
                        .on_access_action_request
                        .as_mut()
                        .map(|h| h(*action, *target_node, data.clone(), ctx))
                        .unwrap_or(EventResponse::Ignored);
                    let r2 = node
                        .handlers
                        .on_access_action_request
                        .as_mut()
                        .map(|h| h(*action, *target_node, data.clone(), ctx))
                        .unwrap_or(EventResponse::Ignored);
                    Some(
                        if r1 == EventResponse::Handled || r2 == EventResponse::Handled {
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        },
                    )
                } else if node.handlers.on_access_action.is_some()
                    || node.external_handlers.on_access_action.is_some()
                {
                    let r1 = node
                        .external_handlers
                        .on_access_action
                        .as_mut()
                        .map(|h| h(*action, ctx))
                        .unwrap_or(EventResponse::Ignored);
                    let r2 = node
                        .handlers
                        .on_access_action
                        .as_mut()
                        .map(|h| h(*action, ctx))
                        .unwrap_or(EventResponse::Ignored);
                    Some(
                        if r1 == EventResponse::Handled || r2 == EventResponse::Handled {
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        },
                    )
                } else {
                    None
                };

                // Builder-level access_action / access_custom_action
                // callbacks. These layer on top of any user-installed
                // on_access_action / on_access_action_request — both
                // fire for the same dispatched event. Drives the
                // SwiftUI `.accessibilityAction(...)` parity.
                let mut override_handled = false;
                if let Some(ov) = node.access_overrides.as_deref_mut() {
                    if matches!(action, accesskit::Action::CustomAction) {
                        if let Some(accesskit::ActionData::CustomAction(idx)) = data
                            && let Some((_, cb)) = ov.custom_actions.get_mut(*idx as usize)
                        {
                            cb(ctx);
                            override_handled = true;
                        }
                    } else {
                        for (a, cb) in ov.actions.iter_mut() {
                            if *a == *action {
                                cb(ctx);
                                override_handled = true;
                            }
                        }
                    }
                }

                ctx.current_source = saved_a11y_source;
                match (user_handled, override_handled) {
                    (Some(EventResponse::Handled), _) | (_, true) => Some(EventResponse::Handled),
                    (Some(EventResponse::Ignored), false) => Some(EventResponse::Ignored),
                    (None, false) => None,
                }
            }
            WidgetEvent::Gesture { gesture } => {
                // Pre-recognized gestures from the platform (OS trackpad
                // pinch/rotation, double-tap, …) bypass the gesture arena
                // and go straight to the matching handler. See §10.
                let matched = matches!(
                    gesture,
                    GestureEvent::PinchStarted { .. }
                        | GestureEvent::PinchChanged { .. }
                        | GestureEvent::PinchEnded
                        | GestureEvent::Swipe { .. }
                        | GestureEvent::DoubleTap { .. }
                        | GestureEvent::TripleTap { .. }
                ) && {
                    let has_handler = match gesture {
                        GestureEvent::PinchStarted { .. }
                        | GestureEvent::PinchChanged { .. }
                        | GestureEvent::PinchEnded => node.any_handler(|h| h.on_pinch.is_some()),
                        GestureEvent::Swipe { .. } => node.any_handler(|h| h.on_swipe.is_some()),
                        GestureEvent::DoubleTap { .. } => {
                            node.any_handler(|h| h.on_double_tap.is_some())
                        }
                        GestureEvent::TripleTap { .. } => {
                            node.any_handler(|h| h.on_triple_tap.is_some())
                        }
                        _ => false,
                    };
                    if has_handler {
                        Self::dispatch_recognized_gesture(node, *gesture, ctx);
                    }
                    has_handler
                };
                if matched {
                    Some(EventResponse::Handled)
                } else {
                    None
                }
            }
            WidgetEvent::PointerDown {
                position,
                button,
                modifiers,
            } => {
                // Raw pointer handler runs first so widgets can intercept
                // events that the gesture recognizers won't catch (e.g.
                // right-click → context menu). If it returns Handled the
                // gesture arena is skipped; otherwise we fall through.
                // Only fire for the target — ancestors already fired
                // on_pointer_event during the preview pass.
                if fire_on_pointer_event {
                    let r = fire_event_handler_both(
                        &mut node.external_handlers.on_pointer_event,
                        &mut node.handlers.on_pointer_event,
                        event,
                        ctx,
                    );
                    if r == EventResponse::Handled {
                        return Some(EventResponse::Handled);
                    }
                }
                Self::ensure_gesture_arena(node);
                if let Some(arena) = node.handlers.gesture_arena.as_mut() {
                    // Implicit capture for the Down..Up sequence so that
                    // moves leaving the widget bounds still reach the
                    // arena. Without this, a drag that starts inside the
                    // widget but crosses its edge before the recognizer
                    // latches would be hit-tested to another widget and
                    // the press-origin arena would never see a `Move`.
                    // Released unconditionally by the `PointerUp` branch
                    // in `dispatch_event`.
                    ctx.capture_pointer();
                    let result = arena.process(&RawPointerEvent::Down {
                        position: *position,
                        button: *button,
                        modifiers: *modifiers,
                    });
                    if let Some(gesture) = result {
                        Self::dispatch_recognized_gesture(node, gesture, ctx);
                    }
                    return Some(EventResponse::Handled);
                }
                None
            }
            WidgetEvent::PointerUp {
                position,
                button,
                modifiers,
            } => {
                if fire_on_pointer_event {
                    let r = fire_event_handler_both(
                        &mut node.external_handlers.on_pointer_event,
                        &mut node.handlers.on_pointer_event,
                        event,
                        ctx,
                    );
                    if r == EventResponse::Handled {
                        return Some(EventResponse::Handled);
                    }
                }
                if let Some(arena) = node.handlers.gesture_arena.as_mut() {
                    let result = arena.process(&RawPointerEvent::Up {
                        position: *position,
                        button: *button,
                        modifiers: *modifiers,
                    });
                    if let Some(gesture) = result {
                        Self::dispatch_recognized_gesture(node, gesture, ctx);
                    }
                    return Some(EventResponse::Handled);
                }
                None
            }
            WidgetEvent::PointerMove { position } => {
                if fire_on_pointer_event {
                    let r = fire_event_handler_both(
                        &mut node.external_handlers.on_pointer_event,
                        &mut node.handlers.on_pointer_event,
                        event,
                        ctx,
                    );
                    if r == EventResponse::Handled {
                        return Some(EventResponse::Handled);
                    }
                }
                if let Some(arena) = node.handlers.gesture_arena.as_mut() {
                    let result = arena.process(&RawPointerEvent::Move {
                        position: *position,
                    });
                    if let Some(gesture) = result {
                        Self::dispatch_recognized_gesture(node, gesture, ctx);
                        // A recognized gesture (DragStarted / DragMoved / …)
                        // almost always changes visible state — return
                        // `Handled` so the bubble loop marks this widget
                        // `needs_paint`, which in turn makes
                        // `WidgetTree::needs_redraw()` return true and
                        // triggers a `request_redraw` for the next frame.
                        // Without this, state updates via bound signals are
                        // only observed on the *next* layout/render pass,
                        // which in turn is never scheduled because
                        // `fern-app::update_control_flow` only wakes up when
                        // `needs_redraw()` is true.
                        return Some(EventResponse::Handled);
                    }
                    return Some(EventResponse::Ignored);
                }
                None
            }
        }
    }

    /// Lazily install a gesture arena populated with whichever recognizers
    /// the widget's handler set actually needs. Without this, a widget
    /// that wires `on_drag` or `on_double_tap` (but not `on_tap`) would
    /// never get a gesture arena and the handlers would never fire.
    ///
    /// Checks BOTH handler buckets (own + external) so a recognizer gets
    /// installed whether the handler was attached via
    /// `apply_self_handlers` or via the `WidgetBuilder` chain.
    pub(crate) fn ensure_gesture_arena(node: &mut crate::arena::WidgetNode) {
        if node.handlers.gesture_arena.is_some() {
            return;
        }
        let has_tap = node.any_handler(|h| h.on_tap.is_some());
        let has_double_tap = node.any_handler(|h| h.on_double_tap.is_some());
        let has_triple_tap = node.any_handler(|h| h.on_triple_tap.is_some());
        let has_drag = node.any_handler(|h| h.on_drag.is_some());
        let has_long_press = node.any_handler(|h| h.on_long_press.is_some());
        let has_swipe = node.any_handler(|h| h.on_swipe.is_some());

        if !(has_tap || has_double_tap || has_triple_tap || has_drag || has_long_press || has_swipe)
        {
            return;
        }

        // Read per-handler button-mask overrides from BOTH buckets,
        // preferring the own (`handlers`) bucket. Falls back to the
        // recognizer's own default (`ButtonMask::PRIMARY`) when neither
        // bucket sets a mask.
        let tap_buttons = node
            .handlers
            .tap_buttons
            .or(node.external_handlers.tap_buttons);
        let double_tap_buttons = node
            .handlers
            .double_tap_buttons
            .or(node.external_handlers.double_tap_buttons);
        let triple_tap_buttons = node
            .handlers
            .triple_tap_buttons
            .or(node.external_handlers.triple_tap_buttons);
        let long_press_buttons = node
            .handlers
            .long_press_buttons
            .or(node.external_handlers.long_press_buttons);

        let mut arena = GestureArena::new();
        // Important: install `TapRecognizer` ONLY when the widget actually
        // wired `on_tap` AND no multi-tap recognizer is in the arena. A
        // parallel `TapRecognizer` would let `Tap` win on the first up
        // (it returns `Recognized` while `DoubleTap` / `TripleTap` return
        // `Pending`), and the arena's reset loop would wipe the multi-tap
        // state. Multi-tap recognizers opt out of that reset via
        // `resets_on_peer_recognition = false`, so once we install a
        // multi-tap recognizer, we intentionally skip `TapRecognizer` —
        // callers that need click-1 behaviour under a multi-tap widget
        // use `on_pointer_event::PointerDown` (which fires before the
        // gesture arena and runs regardless of multi-tap state).
        if has_tap && !(has_double_tap || has_triple_tap) {
            let mut rec = crate::gesture::TapRecognizer::new();
            if let Some(mask) = tap_buttons {
                rec = rec.accept_buttons(mask);
            }
            arena.add(rec);
        }
        if has_double_tap {
            let mut rec = crate::gesture::DoubleTapRecognizer::new();
            if let Some(mask) = double_tap_buttons {
                rec = rec.accept_buttons(mask);
            }
            arena.add(rec);
        }
        if has_triple_tap {
            let mut rec = crate::gesture::TripleTapRecognizer::new();
            if let Some(mask) = triple_tap_buttons {
                rec = rec.accept_buttons(mask);
            }
            arena.add(rec);
        }
        if has_drag {
            arena.add(crate::gesture::DragRecognizer::new().threshold(5.0));
        }
        if has_long_press {
            let mut rec = crate::gesture::LongPressRecognizer::new();
            if let Some(mask) = long_press_buttons {
                rec = rec.accept_buttons(mask);
            }
            arena.add(rec);
        }
        if has_swipe {
            arena.add(crate::gesture::SwipeRecognizer::new());
        }
        node.handlers.gesture_arena = Some(arena);
    }

    /// Route a gesture recognized by the arena (or the OS pinch/rotate
    /// stream) to the matching handler on the node.
    pub(crate) fn dispatch_recognized_gesture(
        node: &mut crate::arena::WidgetNode,
        gesture: GestureEvent,
        ctx: &mut EventContext,
    ) {
        use crate::gesture::{DragPhase, PinchPhase};
        // Phase 5.2 — every gesture handler invocation runs under a
        // `Handler` source label. Any `ctx.send_intent(...)` issued
        // from inside a tap / double-tap / drag / etc. handler
        // emits with `IntentSource::Handler`. The label is restored
        // at the bottom of this fn so nested dispatch doesn't
        // pollute the wrong bucket.
        let saved_source = ctx
            .current_source
            .replace(crate::telemetry::IntentSource::Handler);
        // For every gesture handler, fire BOTH the external and own slot
        // in that order so a widget that wired an on_tap via the
        // WidgetBuilder AND via apply_self_handlers sees both callbacks —
        // and more importantly, so widgets that rely on one bucket don't
        // miss the gesture when the other is empty.
        match gesture {
            GestureEvent::Tap(event) => {
                if let Some(h) = node.external_handlers.on_tap.as_mut() {
                    h(&event, ctx);
                }
                if let Some(h) = node.handlers.on_tap.as_mut() {
                    h(&event, ctx);
                }
            }
            GestureEvent::DoubleTap(event) => {
                if let Some(h) = node.external_handlers.on_double_tap.as_mut() {
                    h(&event, ctx);
                }
                if let Some(h) = node.handlers.on_double_tap.as_mut() {
                    h(&event, ctx);
                }
            }
            GestureEvent::TripleTap(event) => {
                if let Some(h) = node.external_handlers.on_triple_tap.as_mut() {
                    h(&event, ctx);
                }
                if let Some(h) = node.handlers.on_triple_tap.as_mut() {
                    h(&event, ctx);
                }
            }
            GestureEvent::LongPress(event) => {
                if let Some(h) = node.external_handlers.on_long_press.as_mut() {
                    h(&event, ctx);
                }
                if let Some(h) = node.handlers.on_long_press.as_mut() {
                    h(&event, ctx);
                }
            }
            GestureEvent::DragStarted { position, button } => {
                // Auto-capture the pointer for the duration of the drag so
                // the widget keeps receiving `Moved` / `Ended` even when
                // the cursor leaves its bounds. Released on `DragEnded`.
                ctx.capture_pointer();
                let phase = DragPhase::Started { position, button };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::DragMoved { position, delta } => {
                let phase = DragPhase::Moved { position, delta };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::DragEnded { position } => {
                let phase = DragPhase::Ended { position };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                ctx.release_pointer();
            }
            GestureEvent::Swipe {
                direction,
                velocity,
            } => {
                if let Some(h) = node.external_handlers.on_swipe.as_mut() {
                    h(direction, velocity, ctx);
                }
                if let Some(h) = node.handlers.on_swipe.as_mut() {
                    h(direction, velocity, ctx);
                }
            }
            GestureEvent::PinchStarted { center } => {
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(PinchPhase::Started { center }, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(PinchPhase::Started { center }, ctx);
                }
            }
            GestureEvent::PinchChanged {
                center,
                scale,
                rotation,
            } => {
                let phase = PinchPhase::Changed {
                    center,
                    scale,
                    rotation,
                };
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::PinchEnded => {
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(PinchPhase::Ended, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(PinchPhase::Ended, ctx);
                }
            }
        }
        // Phase 5.2 restore — see the matching `replace` at the
        // top of this function.
        ctx.current_source = saved_source;
    }

    pub(super) fn collect_from_ctx<'ops>(
        &mut self,
        mut ctx: EventContext<'ops>,
        source_widget: WidgetId,
    ) {
        // Take the ops handle out of ctx up front so we can freely
        // reborrow it inside the method without fighting the 'ops
        // lifetime propagation when other fields of `ctx` are moved.
        // When no ops is set (standalone trees / tests), fall back to
        // a stack NoopWindowOps.
        let local_ops = ctx.window_ops.take();
        let mut noop = crate::window::NoopWindowOps;
        let ops: &mut dyn crate::window::WindowOps = match local_ops {
            Some(o) => o,
            None => &mut noop,
        };
        if ctx.frame_requested {
            self.request_frame();
        }
        if let Some(cursor) = ctx.cursor_request {
            self.current_cursor = cursor;
        }
        // Intents queued through `ctx.send_intent` are anchored at
        // the originating widget. Programmatic sends default to
        // `propagate_when_disabled = true` — there is no shortcut to
        // consult, and propagation is the safe, least-surprising
        // default.
        for intent in ctx.pending_intents {
            self.enqueue_intent(source_widget, intent, true);
        }
        // Key capture: process cancel before arm, matching the
        // handler's call order (the handler sets `cancel_key_capture`
        // when it calls `ctx.cancel_key_capture()`, and separately
        // stores `pending_key_capture` when it calls
        // `ctx.begin_key_capture(...)`). If the handler did both,
        // arm wins (whichever was called last on the ctx has
        // already overwritten the other field's effect via the
        // setter logic).
        if ctx.cancel_key_capture {
            self.cancel_key_capture();
        }
        if let Some(slot) = ctx.pending_key_capture {
            self.key_capture = Some(slot);
        }
        // Registry mutations queued by settings-UI buttons.
        for mutation in ctx.pending_shortcut_mutations {
            match mutation {
                crate::widget::ShortcutMutation::RebindPrimary { id, keystroke } => {
                    self.shortcut_registry.rebind_primary(id, keystroke);
                }
                crate::widget::ShortcutMutation::RebindSecondary { id, keystroke } => {
                    self.shortcut_registry.rebind_secondary(id, keystroke);
                }
                crate::widget::ShortcutMutation::ClearOverride { id } => {
                    self.shortcut_registry.clear_override(&id);
                }
            }
        }
        if ctx.close_window_requested {
            self.close_window_requested = true;
        }
        self.pending_modal_requests
            .extend(ctx.modal_requests.into_iter().map(|request| {
                crate::modal::QueuedModalRequest {
                    source_widget,
                    request,
                }
            }));
        if ctx.dismiss_modal && !self.dismiss_modal_for_source(source_widget, &mut *ops) {
            self.pending_modal_dismissal = true;
        }
        for callback in ctx.idle_callbacks {
            self.idle_queue.push_boxed(callback);
        }
        if ctx.dismiss_all_overlays {
            let dismissed = self.overlay_manager.dismiss_all();
            self.dormant_dismissed_content(&dismissed, &mut *ops);
        } else if ctx.dismiss_all_except_hosts {
            self.dismiss_all_overlays_except_hosts(&mut *ops);
        } else if ctx.dismiss_self_overlay_chain {
            self.dismiss_self_overlay_chain_for_source(source_widget, &mut *ops);
        } else if ctx.dismiss_top {
            if let Some((_id, content_ids, focus_restore)) = self.overlay_manager.dismiss_top() {
                self.dormant_dismissed_content(&content_ids, &mut *ops);
                if let Some(restore_id) = focus_restore
                    && self.arena.is_active(restore_id)
                {
                    self.focus_ops(restore_id, &mut *ops);
                }
            }
        } else {
            for id in ctx.overlay_dismissals {
                let dismissed = self.overlay_manager.dismiss(id);
                self.dormant_dismissed_content(&dismissed, &mut *ops);
            }
        }
        for preserve_content in ctx.dismiss_descendant_overlays {
            self.dismiss_child_overlays_for_source(source_widget, preserve_content, &mut *ops);
        }
        self.apply_tree_mutations(&ctx.tree_mutations);
        for mut req in ctx.overlay_requests {
            if req.parent_overlay.is_none() {
                req.parent_overlay = self.overlay_ancestor_for_widget(source_widget);
            }
            if self
                .overlay_manager
                .find_by_content(req.content_id)
                .is_some()
            {
                continue;
            }
            let current_focus = self.focused;
            self.overlay_manager.show(req);
            if let Some(focus_id) = current_focus {
                self.overlay_manager.set_top_focus_restore(focus_id);
            }
        }
        for (mut req, duration) in ctx.timed_overlay_requests {
            if req.parent_overlay.is_none() {
                req.parent_overlay = self.overlay_ancestor_for_widget(source_widget);
            }
            if self
                .overlay_manager
                .find_by_content(req.content_id)
                .is_some()
            {
                continue;
            }
            let current_focus = self.focused;
            let overlay_id = self.overlay_manager.show_for(req, duration);
            self.overlay_manager
                .set_shown_at_sim(overlay_id, self.sim_clock);
            if let Some(focus_id) = current_focus {
                self.overlay_manager.set_top_focus_restore(focus_id);
            }
        }
        if let Some(capture) = ctx.pointer_capture {
            if capture {
                self.pointer_captured_by = Some(source_widget);
            } else {
                self.pointer_captured_by = None;
            }
        }
        for (mut request, delay, focus_target) in ctx.delayed_overlay_requests {
            if request.parent_overlay.is_none() {
                request.parent_overlay = self.overlay_ancestor_for_widget(source_widget);
            }
            if self
                .overlay_manager
                .find_by_content(request.content_id)
                .is_some()
            {
                continue;
            }
            let content_id = request.content_id;
            self.pending_delayed_overlays
                .retain(|pending| pending.request.content_id != content_id);
            self.pending_delayed_overlays.push(PendingDelayedOverlay {
                request,
                delay,
                focus_target,
                real_requested_at: std::time::Instant::now(),
                sim_requested_at: self.sim_clock,
            });
            self.arena.mark_needs_paint(source_widget);
        }
        for content_id in ctx.cancel_delayed_overlays {
            self.pending_delayed_overlays
                .retain(|pending| pending.request.content_id != content_id);
        }
        for id in ctx.repaint_requests {
            self.arena.mark_needs_paint(id);
        }
        for id in ctx.synthetic_clicks {
            self.click(id);
        }
        if let Some(&id) = ctx.focus_requests.last() {
            // If the requested widget is itself not focusable (e.g. a
            // composite like `TextInput` whose focus-handling lives on
            // an inner leaf), walk into the subtree and land on the
            // first focusable descendant in document order. This makes
            // `ctx.request_focus(some_composite)` Do The Right Thing
            // without every caller having to reach into private inner
            // ids. `first_focusable_descendant` returns the node itself
            // when it's focusable, so the usual leaf-target case is
            // still a no-op lookup.
            let target = self.first_focusable_descendant(id).unwrap_or(id);
            self.focus_ops(target, &mut *ops);
        }

        // --- Drag and drop ---
        if let Some((source_widget, payload, preview_widget)) = ctx.drag_start_request {
            let (preview_content_id, preview_overlay_id) = if let Some(preview) = preview_widget {
                // `add_boxed` — NOT `arena.insert` — runs the widget's
                // `build()` so composite previews (our `DragPreview`
                // wrapper in fern-widgets, or anything a user supplies)
                // actually instantiate their child subtree. Plain
                // `arena.insert` stops at the root node, leaves build
                // un-fired, and the overlay renders an empty widget.
                let content_id = self.add_boxed(preview);
                let overlay_id = self.overlay_manager.show(crate::overlay::OverlayRequest {
                    content_id,
                    anchor: source_widget,
                    placement: crate::overlay::OverlayPlacement::AtPointer(
                        fern_canvas::Point::ZERO,
                    ),
                    dismiss: crate::overlay::DismissBehavior::Manual,
                    layer: crate::overlay::OverlayLayer::InTree,
                    parent_overlay: None,
                    on_dismiss: None,
                    fade_duration: None,
                });
                // Force the next layout pass to run `position_overlays`
                // and `set_content_bounds` — otherwise the preview sits
                // at its initial (0, 0) placement forever.
                self.arena.mark_needs_layout(content_id);
                (Some(content_id), Some(overlay_id))
            } else {
                (None, None)
            };
            self.active_drag = Some(crate::drag_state::DragSession {
                payload,
                source_widget,
                current_position: fern_canvas::Point::ZERO,
                current_target: None,
                feedback: crate::drag_state::DropFeedback::NoFeedback,
                preview_content_id,
                preview_overlay_id,
            });
            self.pointer_captured_by = Some(source_widget);
            // Grabbing-hand cursor while the drag is in flight. Reset on
            // drop / cancel / source-destroyed below.
            self.current_cursor = crate::widget::CursorIcon::Grabbing;
        }
        if ctx.cancel_drag {
            self.cancel_active_drag(&mut *ops);
        }

        // --- Environment changes (architecture §9.5) ---
        if let Some(theme) = ctx.theme_request {
            self.set_theme(theme);
        }
        if let Some(locale) = ctx.locale_request {
            // Stored, not applied: the app layer must route this through
            // `WindowManager::set_locale` so the `I18nManager`'s active
            // locale and direction stay in sync. Applying via
            // `WidgetTree::set_locale` alone would leave `tr!` bindings
            // reading the old translations.
            self.pending_locale_request = Some(locale);
        }
    }

    // --- Drag and drop helpers ---

    /// Clean up drag preview overlay (if any).
    pub(super) fn cleanup_drag_preview(&mut self) {
        if let Some(ref drag) = self.active_drag {
            if let Some(overlay_id) = drag.preview_overlay_id {
                self.overlay_manager.dismiss(overlay_id);
            }
            if let Some(content_id) = drag.preview_content_id {
                self.arena.destroy(content_id);
            }
        }
    }

    /// Cancel the active drag session: fire `on_drag_leave` on the current
    /// target (if any), dismiss the preview overlay, clear the session and
    /// release pointer capture. Used by Escape, explicit cancel requests,
    /// and the source-destroyed salvage in `revalidate_interaction_state`.
    pub(super) fn cancel_active_drag(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let prev_target = self.active_drag.as_ref().and_then(|d| d.current_target);
        self.cleanup_drag_preview();
        self.active_drag = None;
        self.pointer_captured_by = None;
        self.current_cursor = crate::widget::CursorIcon::Default;
        if let Some(prev) = prev_target {
            self.fire_on_drag_leave(prev, &mut *ops);
        }
    }

    /// Fire `on_drag_tick` on the current drop target (if any). Runs once
    /// per layout pass while a drag session is active. The handler
    /// receives the pointer position in the target's local coordinates.
    /// Fires from both external and own handler buckets.
    pub(super) fn process_drag_tick(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let Some((target_id, position)) = self
            .active_drag
            .as_ref()
            .and_then(|d| d.current_target.map(|t| (t, d.current_position)))
        else {
            return;
        };
        if !self.arena.is_active(target_id) {
            return;
        }
        let bounds = self.arena.bounds(target_id);
        let local = fern_canvas::Point::new(position.x - bounds.x, position.y - bounds.y);
        let (mut ext_handler, mut own_handler) = match self.arena.get_mut(target_id) {
            Some(node) => (
                node.external_handlers.on_drag_tick.take(),
                node.handlers.on_drag_tick.take(),
            ),
            None => return,
        };
        if ext_handler.is_none() && own_handler.is_none() {
            return;
        }
        let mut ctx = self.make_event_context(&mut *ops);
        if let Some(h) = ext_handler.as_mut() {
            h(local, &mut ctx);
        }
        if let Some(h) = own_handler.as_mut() {
            h(local, &mut ctx);
        }
        if let Some(node) = self.arena.get_mut(target_id) {
            node.external_handlers.on_drag_tick = ext_handler;
            node.handlers.on_drag_tick = own_handler;
        }
        self.collect_from_ctx(ctx, target_id);
        // If the tick handler scrolled content, the pointer is now over a
        // different item — refresh the hover pipeline with the same
        // pointer position so feedback reflects the new content offset.
        if self.active_drag.is_some() {
            self.handle_drag_move(position, &mut *ops);
        }
    }

    /// Fire `on_drag_leave` on the given widget (if it has one), mark it
    /// needs_paint, and process any commands the handler emitted. Used
    /// whenever a drop target stops being the current target — whether
    /// because the pointer moved elsewhere, the drop completed, or the
    /// drag was cancelled. Fires from both external and own buckets.
    pub(super) fn fire_on_drag_leave(
        &mut self,
        target_id: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if !self.arena.is_active(target_id) {
            return;
        }
        let (mut ext_handler, mut own_handler) = match self.arena.get_mut(target_id) {
            Some(node) => (
                node.external_handlers.on_drag_leave.take(),
                node.handlers.on_drag_leave.take(),
            ),
            None => return,
        };
        if ext_handler.is_none() && own_handler.is_none() {
            // Still mark for repaint so any visual artefacts the
            // framework owns (feedback lines, highlights) clear.
            self.arena.mark_needs_paint(target_id);
            return;
        }
        let mut ctx = self.make_event_context(&mut *ops);
        if let Some(h) = ext_handler.as_mut() {
            h(&mut ctx);
        }
        if let Some(h) = own_handler.as_mut() {
            h(&mut ctx);
        }
        if let Some(node) = self.arena.get_mut(target_id) {
            node.external_handlers.on_drag_leave = ext_handler;
            node.handlers.on_drag_leave = own_handler;
        }
        self.collect_from_ctx(ctx, target_id);
        self.arena.mark_needs_paint(target_id);
    }

    /// Update the drag session on pointer move: find the drop target under the
    /// pointer and call its `on_drag_hover` handler.
    fn handle_drag_move(
        &mut self,
        position: fern_canvas::Point,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Update position on the session
        if let Some(ref mut drag) = self.active_drag {
            drag.current_position = position;
        }

        // Update preview overlay placement. `update_placement` only
        // stores the new enum — the actual overlay bounds are recomputed
        // by `position_overlays` which runs inside `WidgetTree::layout()`
        // behind a `needs_layout` gate. Mark the content widget dirty so
        // the next layout pass actually re-positions the preview instead
        // of leaving it pinned at (0, 0).
        let preview_content = self
            .active_drag
            .as_ref()
            .and_then(|d| Some((d.preview_overlay_id?, d.preview_content_id?)));
        if let Some((overlay_id, content_id)) = preview_content {
            self.overlay_manager.update_placement(
                overlay_id,
                crate::overlay::OverlayPlacement::AtPointer(position),
            );
            self.arena.mark_needs_layout(content_id);
        }

        // Hit-test to find the widget under the pointer, excluding the drag
        // preview overlay and its content widget so they don't block hit-testing
        // of actual drop targets.
        let exclude_overlay = self.active_drag.as_ref().and_then(|d| d.preview_overlay_id);
        let exclude_widget = self.active_drag.as_ref().and_then(|d| d.preview_content_id);
        let target =
            self.hit_test_excluding_overlay_and_widget(position, exclude_overlay, exclude_widget);

        // Walk up from hit target to find a widget with on_drag_hover
        let drop_target = target.and_then(|t| self.find_drop_target_at_or_above(t));

        // Detect target change BEFORE firing new handlers so we can fire
        // on_drag_leave on the outgoing target first.
        let prev_target = self.active_drag.as_ref().and_then(|d| d.current_target);
        if prev_target != drop_target {
            if let Some(ref mut drag) = self.active_drag {
                drag.feedback = crate::drag_state::DropFeedback::NoFeedback;
                drag.current_target = drop_target;
            }
            if let Some(prev) = prev_target {
                self.fire_on_drag_leave(prev, &mut *ops);
            }
        }

        // Call on_drag_hover on the target if it has one. Pointer position
        // is passed in TARGET-LOCAL coordinates — same coordinate system as
        // the handler's own `bounds`, so insertion-line / drop-index math
        // doesn't have to know where it sits in the window.
        //
        // on_drag_hover is a "decision" handler (returns DropFeedback).
        // When both buckets are set, own takes precedence — the widget's
        // own feedback reflects its internal view of acceptance.
        if let Some(target_id) = drop_target {
            let target_bounds = self.arena.bounds(target_id);
            let local =
                fern_canvas::Point::new(position.x - target_bounds.x, position.y - target_bounds.y);
            let (mut ext_handler, mut own_handler) = match self.arena.get_mut(target_id) {
                Some(node) => (
                    node.external_handlers.on_drag_hover.take(),
                    node.handlers.on_drag_hover.take(),
                ),
                None => return,
            };
            if ext_handler.is_none() && own_handler.is_none() {
                return;
            }
            let mut ctx = self.make_event_context(&mut *ops);
            if let Some(ref drag) = self.active_drag {
                let mut feedback = crate::drag_state::DropFeedback::NoFeedback;
                if let Some(h) = ext_handler.as_mut() {
                    feedback = h(&drag.payload, local, &mut ctx);
                }
                if let Some(h) = own_handler.as_mut() {
                    feedback = h(&drag.payload, local, &mut ctx);
                }
                if let Some(node) = self.arena.get_mut(target_id) {
                    node.external_handlers.on_drag_hover = ext_handler;
                    node.handlers.on_drag_hover = own_handler;
                }
                if let Some(ref mut drag) = self.active_drag {
                    drag.feedback = feedback;
                }
                self.collect_from_ctx(ctx, target_id);
                self.arena.mark_needs_paint(target_id);
            } else if let Some(node) = self.arena.get_mut(target_id) {
                node.external_handlers.on_drag_hover = ext_handler;
                node.handlers.on_drag_hover = own_handler;
            }
        }
    }

    /// Complete the drag: fire `on_drop` on the target widget and end the session.
    fn handle_drag_drop(
        &mut self,
        position: fern_canvas::Point,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Clean up preview overlay
        self.cleanup_drag_preview();

        // Take the drag session
        let drag = match self.active_drag.take() {
            Some(d) => d,
            None => return,
        };
        self.pointer_captured_by = None;
        self.current_cursor = crate::widget::CursorIcon::Default;

        // Fire on_drag_leave on the session's current target before on_drop
        // runs — widgets own their feedback state and must be given a
        // chance to clear it regardless of whether the drop is accepted.
        if let Some(prev) = drag.current_target {
            self.fire_on_drag_leave(prev, &mut *ops);
        }

        // Hit-test to find drop target
        let target = self.hit_test(position);
        let drop_target = target.and_then(|t| self.find_drop_target_at_or_above(t));

        // on_drop is a "decision" handler (returns bool). Prefer own over
        // external: the widget's own drop semantics trump any external
        // listener. If the own bucket doesn't have it, fall back to
        // external. Fires exactly once, not both.
        if let Some(target_id) = drop_target {
            let target_bounds = self.arena.bounds(target_id);
            let local =
                fern_canvas::Point::new(position.x - target_bounds.x, position.y - target_bounds.y);
            let (taken_own, taken_ext) = match self.arena.get_mut(target_id) {
                Some(node) => {
                    let own = node.handlers.on_drop.take();
                    let ext = if own.is_none() {
                        node.external_handlers.on_drop.take()
                    } else {
                        None
                    };
                    (own, ext)
                }
                None => (None, None),
            };
            let picked = if let Some(h) = taken_own {
                Some((h, /*is_own=*/ true))
            } else {
                taken_ext.map(|h| (h, /*is_own=*/ false))
            };
            if let Some((mut handler, is_own)) = picked {
                let mut ctx = self.make_event_context(&mut *ops);
                let _accepted = handler(drag.payload, local, &mut ctx);
                if let Some(node) = self.arena.get_mut(target_id) {
                    if is_own {
                        node.handlers.on_drop = Some(handler);
                    } else {
                        node.external_handlers.on_drop = Some(handler);
                    }
                }
                self.collect_from_ctx(ctx, target_id);
                self.arena.mark_needs_paint(target_id);
            }
        }
        // Drop was not accepted — payload is dropped (Rust Drop)
    }

    /// Walk up from a widget to find the nearest ancestor (or self) with a
    /// drop handler (`on_drop` or `on_drag_hover`) in either bucket.
    fn find_drop_target_at_or_above(&self, start: WidgetId) -> Option<WidgetId> {
        let mut current = Some(start);
        while let Some(id) = current {
            if let Some(node) = self.arena.get(id)
                && node.any_handler(|h| h.on_drop.is_some() || h.on_drag_hover.is_some())
            {
                return Some(id);
            }
            current = self.arena.parent(id);
        }
        None
    }

    fn apply_tree_mutations(&mut self, mutations: &[crate::widget::TreeMutation]) {
        use crate::widget::TreeMutation;

        for mutation in mutations {
            match mutation {
                TreeMutation::SetDormant(id) => self.arena.set_dormant(*id),
                TreeMutation::Activate(id) => self.arena.activate(*id),
                TreeMutation::Destroy(id) => self.arena.destroy(*id),
            }
        }
    }

    pub fn hit_test(&self, point: Point) -> Option<WidgetId> {
        self.hit_test_excluding_overlay_and_widget(point, None, None)
    }

    /// Hit-test at a point, excluding a specific overlay and widget from consideration.
    /// Used during drag-and-drop to exclude the preview overlay and its content widget,
    /// so they don't block hit-testing of the actual drop targets underneath.
    pub fn hit_test_excluding_overlay_and_widget(
        &self,
        point: Point,
        exclude_overlay: Option<crate::overlay::OverlayId>,
        exclude_widget: Option<WidgetId>,
    ) -> Option<WidgetId> {
        if let Some(overlay_id) = self.overlay_manager.hit_test(point) {
            if Some(overlay_id) == exclude_overlay {
                // Skip this excluded overlay, fall through to widget tree
            } else if let Some(overlay) = self.overlay_manager.overlay(overlay_id) {
                return self.hit_test_recursive_excluding(
                    overlay.content_id,
                    point,
                    exclude_widget,
                );
            }
        }

        if self.overlay_manager.topmost_centered().is_some() {
            return None;
        }

        // Delegates to WidgetArena::hit_test_at, which honors
        // event_pass_through and clips_children correctly.
        self.arena.hit_test_at(point, exclude_widget)
    }

    fn hit_test_recursive_excluding(
        &self,
        id: WidgetId,
        point: Point,
        exclude: Option<WidgetId>,
    ) -> Option<WidgetId> {
        // Subtree hit-test from a specific root — used by the overlay
        // path. Delegates to a tiny wrapper around the arena's recursion
        // by walking from `id` only.
        if !self.arena.is_active(id) || Some(id) == exclude {
            return None;
        }
        // If this node carries a `set_transform` scope, the render walker
        // pushes that transform onto its stack around this node's own paint
        // AND its subtree. Hit-testing must mirror that composition: the
        // input point arrives in this node's parent-effective space; apply
        // this node's transform inverse once, then both the node's own
        // bounds test and the recursion into children operate in the new
        // local space. Identity transforms (and missing transform_prop) are
        // skipped so the hot path stays scalar.
        let local_point = match self
            .arena
            .get(id)
            .and_then(|n| n.transform_prop.as_ref())
            .map(|p| p.get())
            .filter(|t| !t.is_identity())
        {
            Some(t) => match t.inverse() {
                Some(inv) => inv.apply_point(point),
                // A degenerate transform (collapsed axis) hides the entire
                // subtree visually; mirror that for hit-testing.
                None => return None,
            },
            None => point,
        };
        let bounds = self.arena.bounds(id);
        if !bounds.contains(local_point) {
            return None;
        }
        let pass_through = self
            .arena
            .get(id)
            .map(|n| n.event_pass_through)
            .unwrap_or(false);
        let children = self.arena.children(id).to_vec();
        for &child in children.iter().rev() {
            if let Some(hit) = self.hit_test_recursive_excluding(child, local_point, exclude) {
                return Some(hit);
            }
        }
        if pass_through {
            return None;
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget::CursorIcon;
    use crate::widget_builder::WidgetBuilder;

    #[test]
    fn pointer_enter_leave_synthesized() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.hovered, Some(widget));
        tree.pointer_move(Point::new(200.0, 200.0));
        assert_eq!(tree.hovered, None);
    }

    #[test]
    fn pointer_hover_updates_current_cursor() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().cursor(CursorIcon::ColResize));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.current_cursor(), CursorIcon::ColResize);

        tree.pointer_move(Point::new(200.0, 200.0));
        assert_eq!(tree.current_cursor(), CursorIcon::Default);
    }

    #[test]
    fn click_dispatches_to_widget() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.click(widget);
    }

    #[test]
    fn disabled_ancestor_blocks_event_to_descendant() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let flag = tapped.clone();
        let enabled = Signal::new(true);

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_tap(move |_pos, _ctx| {
            flag.set(true);
        }));
        let parent = tree.add(StackWidget::new().add_child(child));
        tree.enabled_when(parent, enabled.clone());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        enabled.set(false);
        tree.click(child);
        assert!(
            !tapped.get(),
            "disabled ancestor should block descendant tap"
        );

        enabled.set(true);
        tree.click(child);
        assert!(tapped.get(), "re-enabling should restore dispatch");
    }

    #[test]
    fn dormant_widget_not_hit_tested() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.hovered, Some(widget));

        tree.set_dormant(widget);
        tree.pointer_move(Point::new(200.0, 200.0));
        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.hovered, None);
    }

    // NOTE: legacy `shortcut_intercepts_before_widget` test removed with
    // the ShortcutMap dispatch path. The new shortcut→intent interception
    // lands in step 3 on top of `ShortcutRegistry` + `Action`.

    #[test]
    fn scroll_event_dispatched_to_hovered() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: crate::event::ScrollDelta::Lines { x: 0.0, y: 1.0 },
            modifiers: Default::default(),
        });
    }

    #[test]
    fn ime_event_dispatched_to_focused() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        tree.dispatch_event(WidgetEvent::ImeComposition {
            text: "あ".to_string(),
            cursor: None,
        });
        tree.dispatch_event(WidgetEvent::ImeCommit {
            text: "あ".to_string(),
        });
    }

    // ── on_key_preview ──────────────────────────────────────────

    #[test]
    fn key_preview_consumes_before_focused_on_key() {
        // root → mid → leaf (focused). Root consumes Enter via
        // on_key_preview; the leaf's on_key must NOT fire.
        use crate::event::EventResponse;
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let leaf_fired = Rc::new(Cell::new(false));
        let leaf_flag = leaf_fired.clone();
        let preview_fired = Rc::new(Cell::new(false));
        let preview_flag = preview_fired.clone();

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable().on_key(move |event, _c| {
            // Only count KeyDown so the trailing KeyUp from
            // press_key doesn't trigger us spuriously.
            if matches!(event, WidgetEvent::KeyDown { .. }) {
                leaf_flag.set(true);
            }
            EventResponse::Handled
        }));
        let mid = tree.add(StackWidget::new().add_child(leaf));
        let _root =
            tree.add(StackWidget::new().add_child(mid).on_key_preview(
                move |event, _c| match event {
                    WidgetEvent::KeyDown {
                        key: Key::Enter, ..
                    } => {
                        preview_flag.set(true);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                },
            ));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(leaf);
        tree.press_key(Key::Enter, Modifiers::NONE);

        assert!(
            preview_fired.get(),
            "ancestor on_key_preview must fire for KeyDown on a focused descendant"
        );
        assert!(
            !leaf_fired.get(),
            "consuming the event in preview must prevent the focused widget's on_key from running"
        );
    }

    #[test]
    fn key_preview_falls_through_when_returning_ignored() {
        // Same shape; this time the preview returns Ignored, so
        // the leaf's on_key must still fire.
        use crate::event::EventResponse;
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let leaf_fired = Rc::new(Cell::new(false));
        let leaf_flag = leaf_fired.clone();
        let preview_fired = Rc::new(Cell::new(false));
        let preview_flag = preview_fired.clone();

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable().on_key(move |_e, _c| {
            leaf_flag.set(true);
            EventResponse::Handled
        }));
        let mid = tree.add(StackWidget::new().add_child(leaf));
        let _root = tree.add(StackWidget::new().add_child(mid).on_key_preview(
            move |_event, _c| {
                preview_flag.set(true);
                EventResponse::Ignored
            },
        ));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(leaf);
        tree.press_key(Key::Enter, Modifiers::NONE);

        assert!(preview_fired.get(), "preview must always be invoked");
        assert!(
            leaf_fired.get(),
            "preview returning Ignored must not block the focused widget's on_key"
        );
    }

    #[test]
    fn key_preview_excludes_focused_target_itself() {
        // Strict-ancestors-only: the focused widget's own
        // on_key_preview must NOT fire — the preview pass walks
        // strict ancestors only.
        use crate::event::EventResponse;
        use std::cell::Cell;
        use std::rc::Rc;

        let preview_on_target = Rc::new(Cell::new(false));
        let pf = preview_on_target.clone();

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable().on_key_preview(move |_e, _c| {
            pf.set(true);
            EventResponse::Handled
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(leaf);
        tree.press_key(Key::Enter, Modifiers::NONE);

        assert!(
            !preview_on_target.get(),
            "the focused widget itself must not see its own on_key_preview"
        );
    }

    #[test]
    fn key_preview_root_to_target_order() {
        // Two ancestors with on_key_preview attached. The outer
        // (root-side) one must fire first; the closer one (still
        // ancestor of the focused leaf) fires second.
        use crate::event::EventResponse;
        use crate::test_widgets::StackWidget;
        use std::cell::RefCell;
        use std::rc::Rc;

        let order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let outer_log = order.clone();
        let inner_log = order.clone();

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable());
        let inner = tree.add(StackWidget::new().add_child(leaf).on_key_preview(
            move |event, _c| {
                if matches!(event, WidgetEvent::KeyDown { .. }) {
                    inner_log.borrow_mut().push("inner");
                }
                EventResponse::Ignored
            },
        ));
        let _outer = tree.add(StackWidget::new().add_child(inner).on_key_preview(
            move |event, _c| {
                if matches!(event, WidgetEvent::KeyDown { .. }) {
                    outer_log.borrow_mut().push("outer");
                }
                EventResponse::Ignored
            },
        ));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(leaf);
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });

        assert_eq!(
            *order.borrow(),
            vec!["outer", "inner"],
            "preview must walk root → parent-of-target"
        );
    }

    #[test]
    fn gesture_tap_recognized_on_click() {
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let tapped_flag = tapped.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_tap(move |_pos, _ctx| {
            tapped_flag.set(true);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.click(widget);
        assert!(tapped.get());
    }

    #[test]
    fn gesture_drag_recognized_on_drag() {
        use crate::gesture::DragPhase;
        use std::cell::Cell;
        use std::rc::Rc;

        let drag_started = Rc::new(Cell::new(false));
        let drag_ended = Rc::new(Cell::new(false));
        let start_flag = drag_started.clone();
        let end_flag = drag_ended.clone();

        let mut tree = WidgetTree::new();
        let _widget = tree.add(FillWidget::new().on_drag(move |phase, _ctx| match phase {
            DragPhase::Started { .. } => start_flag.set(true),
            DragPhase::Ended { .. } => end_flag.set(true),
            _ => {}
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.drag(Point::new(50.0, 25.0), Point::new(80.0, 25.0));

        assert!(drag_started.get());
        assert!(drag_ended.get());
    }

    #[test]
    fn gesture_handler_called_on_tap() {
        use std::cell::Cell;
        use std::rc::Rc;

        let handler_called = Rc::new(Cell::new(false));
        let handler_flag = handler_called.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_tap(move |_pos, _ctx| {
            handler_flag.set(true);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.click(widget);
        assert!(handler_called.get());
    }

    #[test]
    fn on_swipe_fires_from_platform_gesture_event() {
        use crate::gesture::{GestureEvent, SwipeDirection};
        use std::cell::Cell;
        use std::rc::Rc;

        let observed: Rc<Cell<Option<(SwipeDirection, i32)>>> = Rc::new(Cell::new(None));
        let flag = observed.clone();

        let mut tree = WidgetTree::new();
        tree.add(
            FillWidget::new().on_swipe(move |direction, velocity, _ctx| {
                flag.set(Some((direction, velocity as i32)));
            }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: GestureEvent::Swipe {
                direction: SwipeDirection::Left,
                velocity: 450.0,
            },
        });

        let got = observed.get();
        assert!(matches!(got, Some((SwipeDirection::Left, 450))));
    }

    #[test]
    fn on_pinch_fires_from_platform_gesture_event() {
        use crate::gesture::{GestureEvent, PinchPhase};
        use std::cell::Cell;
        use std::rc::Rc;

        let started = Rc::new(Cell::new(false));
        let scale_seen = Rc::new(Cell::new(0.0_f32));
        let ended = Rc::new(Cell::new(false));
        let started_flag = started.clone();
        let scale_flag = scale_seen.clone();
        let ended_flag = ended.clone();

        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_pinch(move |phase, _ctx| match phase {
            PinchPhase::Started { .. } => started_flag.set(true),
            PinchPhase::Changed { scale, .. } => scale_flag.set(scale),
            PinchPhase::Ended => ended_flag.set(true),
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: GestureEvent::PinchStarted {
                center: Point::new(50.0, 25.0),
            },
        });
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: GestureEvent::PinchChanged {
                center: Point::new(50.0, 25.0),
                scale: 1.5,
                rotation: 0.0,
            },
        });
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: GestureEvent::PinchEnded,
        });

        assert!(started.get());
        assert!((scale_seen.get() - 1.5).abs() < 0.001);
        assert!(ended.get());
    }

    #[test]
    fn drag_auto_captures_pointer_until_ended() {
        use crate::gesture::DragPhase;
        use std::cell::Cell;
        use std::rc::Rc;

        let started = Rc::new(Cell::new(false));
        let moved = Rc::new(Cell::new(0));
        let ended = Rc::new(Cell::new(false));
        let started_flag = started.clone();
        let moved_flag = moved.clone();
        let ended_flag = ended.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_drag(move |phase, _ctx| match phase {
            DragPhase::Started { .. } => started_flag.set(true),
            DragPhase::Moved { .. } => moved_flag.set(moved_flag.get() + 1),
            DragPhase::Ended { .. } => ended_flag.set(true),
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Press inside, move past the 5px threshold while still inside —
        // DragRecognizer emits DragStarted, and auto-capture kicks in.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(70.0, 25.0),
        });
        assert!(started.get(), "DragStarted must fire");

        // Move the pointer well outside the widget bounds. Without
        // auto-capture this event would hit-test to another widget and
        // the scrollbar would never see it.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(500.0, 500.0),
        });
        assert!(
            moved.get() >= 1,
            "Move outside bounds must still reach drag handler"
        );

        // Release outside bounds — must still fire DragEnded on the
        // original widget, and pointer capture must be released.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(500.0, 500.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert!(ended.get(), "DragEnded must fire on the original widget");
        assert_eq!(
            tree.pointer_captured_by, None,
            "pointer capture must be released after DragEnded"
        );

        // Sanity: the widget we instantiated is the one we hooked.
        let _ = widget;
    }

    #[test]
    fn on_long_press_fires_from_tick_gestures() {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::time::{Duration, Instant};

        let pressed = Rc::new(Cell::new(false));
        let pressed_flag = pressed.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_long_press(move |_pos, _ctx| {
            pressed_flag.set(true);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let center = tree.bounds(widget).center();
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Before the timeout, tick does nothing.
        tree.tick_gestures(Instant::now());
        assert!(!pressed.get());

        // After the configured 500ms, tick fires the handler.
        tree.tick_gestures(Instant::now() + Duration::from_millis(600));
        assert!(pressed.get());

        // After firing there is no remaining deadline.
        assert!(tree.next_gesture_deadline().is_none());
    }

    #[test]
    fn multiple_recognizers_on_same_widget() {
        use crate::gesture::DragPhase;
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let dragged = Rc::new(Cell::new(false));
        let tapped_flag = tapped.clone();
        let dragged_flag = dragged.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(
            FillWidget::new()
                .on_tap(move |_pos, _ctx| {
                    tapped_flag.set(true);
                })
                .on_drag(move |phase, _ctx| {
                    if matches!(phase, DragPhase::Started { .. }) {
                        dragged_flag.set(true);
                    }
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.click(widget);
        assert!(tapped.get());
        assert!(!dragged.get());

        tapped.set(false);
        dragged.set(false);

        tree.drag(Point::new(50.0, 25.0), Point::new(80.0, 25.0));
        assert!(dragged.get());
    }

    #[test]
    fn access_action_routes_to_target_widget() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(a);
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: accesskit::Action::Click,
            target: Some(b),
            target_node: crate::accessibility::widget_id_to_node_id(b),
            data: None,
        });
    }

    #[test]
    fn access_action_falls_back_to_focused() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(widget);
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: accesskit::Action::Focus,
            target: None,
            target_node: crate::accessibility::root_node_id(),
            data: None,
        });
    }

    // NOTE: legacy `scoped_shortcut_fires_when_focused_in_subtree` test
    // removed along with the ShortcutMap dispatch path. Scope-aware
    // dispatch returns in step 3 on the new ShortcutRegistry.

    // --- Drag and Drop tests ---

    #[test]
    fn start_drag_creates_session() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new().on_tap({
            move |_pos, ctx: &mut crate::widget::EventContext| {
                ctx.start_drag(
                    ctx.focus_requests.first().copied().unwrap_or_default(),
                    crate::drag_payload::DragPayload::typed(42_u32),
                );
            }
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Manually start a drag via EventContext
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(42_u32));
        tree.collect_from_ctx(ctx, source);

        assert!(tree.active_drag.is_some());
        let drag = tree.active_drag.as_ref().unwrap();
        assert_eq!(drag.source_widget, source);
        assert!(drag.payload.has_typed::<u32>());
    }

    #[test]
    fn drag_move_updates_position() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Start a drag session
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed("hello"));
        tree.collect_from_ctx(ctx, source);
        assert!(tree.active_drag.is_some());

        // Move the pointer
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(50.0, 30.0),
        });

        let drag = tree.active_drag.as_ref().unwrap();
        assert!((drag.current_position.x - 50.0).abs() < 0.01);
        assert!((drag.current_position.y - 30.0).abs() < 0.01);
    }

    #[test]
    fn escape_cancels_drag() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(99_i32));
        tree.collect_from_ctx(ctx, source);
        assert!(tree.active_drag.is_some());

        tree.press_key(Key::Escape, Modifiers::NONE);
        assert!(tree.active_drag.is_none(), "drag should be cancelled");
    }

    #[test]
    fn drop_on_target_fires_handler() {
        use std::cell::Cell;
        use std::rc::Rc;

        let dropped = Rc::new(Cell::new(false));
        let dropped_value = Rc::new(Cell::new(0_u32));
        let d = dropped.clone();
        let dv = dropped_value.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        // Target occupies right half (100..200, 0..100)
        let _target = tree.add(FillWidget::new().on_drop(move |mut payload, _pos, _ctx| {
            d.set(true);
            if let Some(val) = payload.take_typed::<u32>() {
                dv.set(val);
            }
            true
        }));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Start drag from source
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(42_u32));
        tree.collect_from_ctx(ctx, source);

        // Drop at a position over the target
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(150.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(tree.active_drag.is_none(), "drag session should be cleared");
        assert!(dropped.get(), "on_drop should have been called");
        assert_eq!(dropped_value.get(), 42);
    }

    #[test]
    fn drop_on_no_target_cancels() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(42_u32));
        tree.collect_from_ctx(ctx, source);

        // Drop outside any widget
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(999.0, 999.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(tree.active_drag.is_none(), "drag session should be cleared");
    }

    #[test]
    fn drag_hover_calls_on_drag_hover() {
        use std::cell::Cell;
        use std::rc::Rc;

        let hover_count = Rc::new(Cell::new(0));
        let hc = hover_count.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(move |_payload, _pos, _ctx| {
                    hc.set(hc.get() + 1);
                    crate::drag_state::DropFeedback::InsertionLine {
                        y: 50.0,
                        width: 200.0,
                    }
                })
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Start drag
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed("test"));
        tree.collect_from_ctx(ctx, source);

        // Move over the target
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(150.0, 50.0),
        });

        assert!(
            hover_count.get() > 0,
            "on_drag_hover should have been called"
        );
    }

    #[test]
    fn drop_outside_window_cancels() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(42_u32));
        tree.collect_from_ctx(ctx, source);
        assert!(tree.active_drag.is_some());

        // PointerUp far outside any widget
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(-100.0, -100.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(tree.active_drag.is_none(), "drag should be cleared");
    }

    #[test]
    fn drop_target_rejects_wrong_type() {
        use std::cell::Cell;
        use std::rc::Rc;

        let accepted = Rc::new(Cell::new(false));
        let a = accepted.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        // Target only accepts String payloads
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(|payload, _pos, _ctx| {
                    if payload.has_typed::<String>() {
                        crate::drag_state::DropFeedback::InsertionLine {
                            y: 0.0,
                            width: 100.0,
                        }
                    } else {
                        crate::drag_state::DropFeedback::NoFeedback
                    }
                })
                .on_drop(move |payload, _pos, _ctx| {
                    if payload.has_typed::<String>() {
                        a.set(true);
                        true
                    } else {
                        false
                    }
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Drag a u32 (not String)
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(42_u32));
        tree.collect_from_ctx(ctx, source);

        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(150.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(!accepted.get(), "on_drop should reject wrong payload type");
    }

    #[test]
    fn inter_widget_drop_transfers_payload() {
        use std::cell::Cell;
        use std::rc::Rc;

        let received_value = Rc::new(Cell::new(0_u32));
        let rv = received_value.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(|_payload, _pos, _ctx| {
                    crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 100.0,
                    }
                })
                .on_drop(move |mut payload, _pos, _ctx| {
                    if let Some(val) = payload.take_typed::<u32>() {
                        rv.set(val);
                        true
                    } else {
                        false
                    }
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Start drag from source with typed payload
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(777_u32));
        tree.collect_from_ctx(ctx, source);

        // Drop on target
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(150.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert_eq!(
            received_value.get(),
            777,
            "Target should receive the typed payload from source"
        );
    }

    #[test]
    fn drop_on_child_walks_up_to_ancestor_drop_target() {
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        // Parent container with `on_drop`; child has no drop handler. The
        // framework should walk up from the hit target to find the parent.
        let parent_fired = Rc::new(Cell::new(false));
        let pf = parent_fired.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let child = tree.add(FillWidget::new());
        let _parent = tree.add(StackWidget::new().add_child(child).on_drop(
            move |_payload, _pos, _ctx| {
                pf.set(true);
                true
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Start a drag.
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(1_u8));
        tree.collect_from_ctx(ctx, source);

        // Drop at the child's center. Hit test lands on the child; drop
        // should bubble up to the parent StackWidget.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(
            parent_fired.get(),
            "Parent's on_drop should fire via ancestor walk"
        );
    }

    #[test]
    fn drag_preview_overlay_created_and_dismissed() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let overlay_count_before = tree.overlay_manager().len();

        // Start drag with a preview widget.
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag_with_preview(
            source,
            crate::drag_payload::DragPayload::typed(0_u32),
            Box::new(FillWidget::new()),
        );
        tree.collect_from_ctx(ctx, source);

        assert!(tree.active_drag.is_some(), "drag session should be active");
        assert!(
            tree.active_drag
                .as_ref()
                .unwrap()
                .preview_overlay_id
                .is_some(),
            "preview overlay id should be recorded"
        );
        assert_eq!(
            tree.overlay_manager().len(),
            overlay_count_before + 1,
            "overlay count should increase by one for the preview"
        );

        // Drop outside any target — cleanup should remove the overlay.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(999.0, 999.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(tree.active_drag.is_none(), "drag session should be cleared");
        assert_eq!(
            tree.overlay_manager().len(),
            overlay_count_before,
            "preview overlay should be dismissed on drop"
        );
    }

    #[test]
    fn drag_preview_follows_pointer_position() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag_with_preview(
            source,
            crate::drag_payload::DragPayload::typed("p"),
            Box::new(FillWidget::new()),
        );
        tree.collect_from_ctx(ctx, source);

        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(73.0, 41.0),
        });

        let drag = tree.active_drag.as_ref().expect("active drag");
        assert!(
            (drag.current_position.x - 73.0).abs() < 0.01
                && (drag.current_position.y - 41.0).abs() < 0.01,
            "drag session position should track the pointer"
        );

        let overlay_id = drag.preview_overlay_id.expect("preview overlay");
        let overlay = tree
            .overlay_manager()
            .overlay(overlay_id)
            .expect("overlay looked up by id");
        match &overlay.placement {
            crate::overlay::OverlayPlacement::AtPointer(p) => {
                assert!(
                    (p.x - 73.0).abs() < 0.01 && (p.y - 41.0).abs() < 0.01,
                    "preview overlay placement should follow pointer"
                );
            }
            other => panic!("expected AtPointer placement, got {:?}", other),
        }
    }

    #[test]
    fn escape_during_hover_dismisses_preview() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(|_payload, _pos, _ctx| {
                    crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 100.0,
                    }
                })
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let overlay_count_before = tree.overlay_manager().len();

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag_with_preview(
            source,
            crate::drag_payload::DragPayload::typed(0_u32),
            Box::new(FillWidget::new()),
        );
        tree.collect_from_ctx(ctx, source);

        // Move over the target to establish feedback.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(150.0, 50.0),
        });

        assert!(tree.active_drag.is_some());
        assert_eq!(tree.overlay_manager().len(), overlay_count_before + 1);

        // Escape cancels: session cleared AND preview overlay dismissed.
        tree.press_key(Key::Escape, Modifiers::NONE);

        assert!(tree.active_drag.is_none(), "drag must be cancelled");
        assert_eq!(
            tree.overlay_manager().len(),
            overlay_count_before,
            "preview overlay must be dismissed after Escape"
        );
    }

    #[test]
    fn active_drag_blocks_on_tap_on_other_widgets() {
        use std::cell::Cell;
        use std::rc::Rc;

        // While a drag is in progress, PointerMove and PointerUp must go
        // through the drag pipeline (handle_drag_move / handle_drag_drop) —
        // NOT be dispatched to the hovered widget. A widget with `on_tap` in
        // the drop location should not receive it.
        let tap_fired = Rc::new(Cell::new(false));
        let tf = tap_fired.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _other = tree.add(FillWidget::new().on_tap(move |_pos, _ctx| {
            tf.set(true);
        }));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);

        // Move over and release on the `on_tap` widget. Normally this would
        // synthesize a Tap gesture — but an active drag short-circuits.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(150.0, 50.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(150.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(
            !tap_fired.get(),
            "on_tap must not fire during an active drag"
        );
    }

    // --- on_drag_leave lifecycle ---------------------------------------

    #[test]
    fn on_drag_leave_fires_when_pointer_leaves_target_bounds() {
        // Single drop target wrapped in an InsetWidget so its bounds do
        // NOT fill the viewport — the pointer can be "inside the scene
        // but outside the target" so a target-change (target → None) is
        // reachable without destroying widgets. That is the main
        // semantic we want `on_drag_leave` to cover.
        use crate::test_widgets::InsetWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let leave = Rc::new(Cell::new(0_u32));
        let l = leave.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let target = tree.add(
            FillWidget::new()
                .on_drag_hover(
                    |_p, _pos, _ctx| crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 10.0,
                    },
                )
                .on_drag_leave(move |_ctx| l.set(l.get() + 1))
                .on_drop(|_, _, _| true),
        );
        let _wrapper = tree.add(InsetWidget::new(40.0).set_child(target));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);

        // Pointer inside the inset (where the target lives).
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        assert_eq!(leave.get(), 0, "no leave yet — target just became active");

        // Pointer in the inset area, outside the target's bounds — the
        // only hit is the InsetWidget which has no drag handlers, so
        // drop_target becomes None. Target changed → leave fires on the
        // old target.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(10.0, 10.0),
        });
        assert_eq!(
            leave.get(),
            1,
            "on_drag_leave fires when pointer exits the target's bounds"
        );

        // Moving back in shouldn't fire again.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        assert_eq!(
            leave.get(),
            1,
            "leave fires at most once per leave transition"
        );

        // Leaving again fires a second time.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(10.0, 10.0),
        });
        assert_eq!(leave.get(), 2);
    }

    #[test]
    fn on_drag_leave_fires_on_drop() {
        use std::cell::Cell;
        use std::rc::Rc;

        let leave = Rc::new(Cell::new(0_u32));
        let l = leave.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(
                    |_p, _pos, _ctx| crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 10.0,
                    },
                )
                .on_drag_leave(move |_ctx| l.set(l.get() + 1))
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert_eq!(leave.get(), 1, "on_drag_leave fires exactly once on drop");
    }

    #[test]
    fn on_drag_leave_fires_on_escape_cancel() {
        use std::cell::Cell;
        use std::rc::Rc;

        let leave = Rc::new(Cell::new(0_u32));
        let l = leave.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(
                    |_p, _pos, _ctx| crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 10.0,
                    },
                )
                .on_drag_leave(move |_ctx| l.set(l.get() + 1))
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        tree.press_key(Key::Escape, Modifiers::NONE);

        assert_eq!(
            leave.get(),
            1,
            "Escape cancel must fire on_drag_leave on the current target"
        );
    }

    #[test]
    fn on_drag_leave_fires_when_source_destroyed_mid_drag() {
        use std::cell::Cell;
        use std::rc::Rc;

        let leave = Rc::new(Cell::new(0_u32));
        let l = leave.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(
                    |_p, _pos, _ctx| crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 10.0,
                    },
                )
                .on_drag_leave(move |_ctx| l.set(l.get() + 1))
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });

        tree.arena.destroy(source);
        // revalidate_interaction_state runs on the next process_pending_rebuilds
        // — drive it by a no-op layout call.
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert!(
            tree.active_drag.is_none(),
            "active drag should have been cancelled"
        );
        assert_eq!(
            leave.get(),
            1,
            "on_drag_leave fires on the drop target when the source is torn down"
        );
    }

    #[test]
    fn on_drag_tick_fires_per_layout_pass() {
        use std::cell::Cell;
        use std::rc::Rc;

        let ticks = Rc::new(Cell::new(0_u32));
        let t = ticks.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(
                    |_p, _pos, _ctx| crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 10.0,
                    },
                )
                .on_drag_tick(move |_pos, _ctx| t.set(t.get() + 1))
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);
        // Move over the target so it becomes the current drop target.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        assert_eq!(ticks.get(), 0, "tick shouldn't have fired yet");

        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(ticks.get(), 1);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(ticks.get(), 3);

        // End the drag; ticks stop.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        let after_drop = ticks.get();
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(
            ticks.get(),
            after_drop,
            "on_drag_tick must not fire after drag ends"
        );
    }

    #[test]
    fn on_drag_hover_and_on_drop_receive_widget_local_coordinates() {
        // Regression for "drop indicator is always 2 items below the
        // cursor": `on_drag_hover` and `on_drop` must receive the
        // pointer in the target's local coordinates, not tree coords.
        // Otherwise a widget placed below a header computes insertion
        // indices against an absolute Y and the line renders offset by
        // the header's height divided by row height.
        use crate::test_widgets::InsetWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let hover_local = Rc::new(Cell::new(Point::new(-1.0, -1.0)));
        let drop_local = Rc::new(Cell::new(Point::new(-1.0, -1.0)));
        let h = hover_local.clone();
        let d = drop_local.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        // Inset 40 pushes the drop target to (40, 40) in tree coords.
        let target = tree.add(
            FillWidget::new()
                .on_drag_hover(move |_p, pos, _ctx| {
                    h.set(pos);
                    crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 10.0,
                    }
                })
                .on_drop(move |_payload, pos, _ctx| {
                    d.set(pos);
                    true
                }),
        );
        let _wrapper = tree.add(InsetWidget::new(40.0).set_child(target));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);

        // Move pointer to (100, 60) in tree coords — inside the inset
        // target whose origin is (40, 40). Local position should be
        // (60, 20).
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 60.0),
        });
        let hov = hover_local.get();
        assert!(
            (hov.x - 60.0).abs() < 0.01 && (hov.y - 20.0).abs() < 0.01,
            "on_drag_hover should receive local coords, got {:?}",
            hov,
        );

        // Drop at (110, 55) tree coords → local (70, 15).
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(110.0, 55.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        let drp = drop_local.get();
        assert!(
            (drp.x - 70.0).abs() < 0.01 && (drp.y - 15.0).abs() < 0.01,
            "on_drop should receive local coords, got {:?}",
            drp,
        );
    }

    #[test]
    fn active_drag_sets_grabbing_cursor() {
        // Starting a drag with a preview must switch the tree's cursor
        // to `Grabbing`; dropping or cancelling must reset to `Default`.
        // fern-app applies the tree's cursor to the winit window after
        // each pointer event, so this is what the user actually sees.
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.current_cursor(), CursorIcon::Default);

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag_with_preview(
            source,
            crate::drag_payload::DragPayload::typed(0_u32),
            Box::new(FillWidget::new()),
        );
        tree.collect_from_ctx(ctx, source);
        assert_eq!(tree.current_cursor(), CursorIcon::Grabbing);

        // Drop somewhere.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(tree.current_cursor(), CursorIcon::Default);
    }

    #[test]
    fn escape_cancel_resets_cursor() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag_with_preview(
            source,
            crate::drag_payload::DragPayload::typed(0_u32),
            Box::new(FillWidget::new()),
        );
        tree.collect_from_ctx(ctx, source);
        assert_eq!(tree.current_cursor(), CursorIcon::Grabbing);

        tree.press_key(Key::Escape, Modifiers::NONE);
        assert_eq!(tree.current_cursor(), CursorIcon::Default);
    }

    #[test]
    fn drag_preview_composite_gets_built() {
        // Regression — composite preview widgets must have their `build()`
        // called after `start_drag_with_preview`. A plain `arena.insert`
        // inserts the node but never runs build, leaving the preview tree
        // empty (no children, zero area of useful content) and the overlay
        // invisible. The fix routes through `add_boxed` so build fires.
        use std::cell::Cell;
        use std::rc::Rc;

        let built = Rc::new(Cell::new(false));
        let b = built.clone();

        #[derive(Debug)]
        struct CheckingWidget {
            built: Rc<Cell<bool>>,
        }
        impl Widget for CheckingWidget {
            fn build(&mut self, _ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
                self.built.set(true);
                Vec::new()
            }
            fn layout_response(
                &self,
                _: SizeProposal,
                _: &crate::widget::LayoutContext,
            ) -> crate::widget::LayoutResponse {
                fern_canvas::Size::new(50.0, 20.0).into()
            }
        }

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag_with_preview(
            source,
            crate::drag_payload::DragPayload::typed(0_u32),
            Box::new(CheckingWidget { built: b }),
        );
        tree.collect_from_ctx(ctx, source);

        assert!(built.get(), "preview's build() must fire on drag start");
    }

    #[test]
    fn preview_placement_drives_layout_needs() {
        // Regression for "preview stays at (0, 0)": each pointer move
        // during drag updates the overlay placement via
        // `update_placement`, but the overlay's bounds are only
        // recomputed by `position_overlays` inside `layout()` — which
        // early-returns when nothing is `needs_layout`. Verify the
        // drag path marks the preview content dirty so layout actually
        // runs.
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag_with_preview(
            source,
            crate::drag_payload::DragPayload::typed(0_u32),
            Box::new(FillWidget::new()),
        );
        tree.collect_from_ctx(ctx, source);

        // Right after drag start, the preview content should need layout
        // so the first layout pass positions it.
        assert!(
            tree.needs_layout(),
            "drag start must mark preview content for layout"
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert!(
            !tree.needs_layout(),
            "layout should have cleared dirty flag"
        );

        // A subsequent PointerMove must remark the preview so its
        // overlay bounds get repositioned on the next layout pass.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(75.0, 120.0),
        });
        assert!(
            tree.needs_layout(),
            "PointerMove during drag must mark preview for layout"
        );
    }

    #[test]
    fn scroll_during_drag_routes_to_drop_target() {
        use std::cell::Cell;
        use std::rc::Rc;

        let scroll_count = Rc::new(Cell::new(0_u32));
        let sc = scroll_count.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(
                    |_p, _pos, _ctx| crate::drag_state::DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 10.0,
                    },
                )
                .on_scroll(move |event, _ctx| match event {
                    WidgetEvent::Scroll { .. } => {
                        sc.set(sc.get() + 1);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                })
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);
        // Make target the current drop target.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });

        // A wheel event during drag should reach the drop target (not the
        // stale hover from before the drag started).
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: crate::event::ScrollDelta::Pixels { x: 0.0, y: 40.0 },
            modifiers: Default::default(),
        });
        assert_eq!(
            scroll_count.get(),
            1,
            "Scroll during drag must route to the current drop target"
        );
    }

    // --- Intent / Action dispatch (step 3) ------------------------------

    #[test]
    fn shortcut_fires_matching_action_on_source_widget() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use std::cell::Cell;
        use std::rc::Rc;

        let fired = Rc::new(Cell::new(false));
        let fired_flag = fired.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable());
        tree.push_action(
            widget,
            Action::new("app.save").on_invoke(move |_intent, _ctx| {
                fired_flag.set(true);
            }),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(fired.get(), "matching action must fire on KeyDown");
    }

    #[test]
    fn global_shortcut_fires_without_focused_widget() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use std::cell::Cell;
        use std::rc::Rc;

        // Regression: a global shortcut must fire even when no widget
        // is focused. A root-registered action should still receive
        // the intent (anchored at the root as a fallback).
        let fired = Rc::new(Cell::new(false));
        let fired_flag = fired.clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(FillWidget::new());
        tree.push_action(
            root,
            Action::new("app.save").on_invoke(move |_intent, _ctx| {
                fired_flag.set(true);
            }),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        // Deliberately no focus() call.

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(
            fired.get(),
            "global shortcut must fire without a focused widget"
        );
    }

    #[test]
    fn global_shortcut_fires_after_focused_widget_destroyed() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use std::cell::Cell;
        use std::rc::Rc;

        // Regression: if the focused widget is destroyed (e.g. during a
        // rebuild after a settings-panel rebind), focus must be cleared
        // so the next global shortcut falls through to the root-anchor
        // path instead of dispatching from a stale, destroyed id.
        let fired = Rc::new(Cell::new(false));
        let fired_flag = fired.clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(FillWidget::new());
        let focusable = tree.add_child(root, FillWidget::new().focusable());
        tree.push_action(
            root,
            Action::new("app.save").on_invoke(move |_intent, _ctx| {
                fired_flag.set(true);
            }),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(focusable);
        assert_eq!(tree.focused(), Some(focusable));

        // Destroy the focused subtree (simulates a rebuild that drops
        // the currently-focused Rebind button).
        tree.destroy_subtree(focusable);
        assert_eq!(tree.focused(), None, "focus must clear when destroyed");

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(
            fired.get(),
            "global shortcut must still fire after the focused widget is destroyed"
        );
    }

    #[test]
    fn scoped_shortcut_matches_only_when_focus_in_scope() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut, ShortcutScope};
        use std::cell::Cell;
        use std::rc::Rc;

        let fired = Rc::new(Cell::new(0));
        let fired_flag = fired.clone();

        let mut tree = WidgetTree::new();
        let scope_root = tree.add(FillWidget::new().focusable());
        let inside = tree.add_child(scope_root, FillWidget::new().focusable());
        let outside = tree.add(FillWidget::new().focusable());

        tree.push_action(
            scope_root,
            Action::new("editor.find").on_invoke(move |_i, _c| {
                fired_flag.set(fired_flag.get() + 1);
            }),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("editor.find")
                .primary(KeyStroke::ctrl(Key::F))
                .scope(ShortcutScope::Scoped(scope_root))
                .build(),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Focus outside the scope: the shortcut does NOT activate.
        tree.focus(outside);
        tree.press_key(Key::F, Modifiers::CTRL);
        assert_eq!(
            fired.get(),
            0,
            "scoped shortcut must not fire outside scope"
        );

        // Focus inside the scope: it fires.
        tree.focus(inside);
        tree.press_key(Key::F, Modifiers::CTRL);
        assert_eq!(
            fired.get(),
            1,
            "scoped shortcut must fire when focus in scope"
        );
    }

    #[test]
    fn propagated_action_lets_ancestor_handle() {
        use crate::action::Action;
        use crate::intent::IntentResponse;
        use crate::shortcut::{KeyStroke, Shortcut};
        use std::cell::Cell;
        use std::rc::Rc;

        let inner_seen = Rc::new(Cell::new(false));
        let outer_seen = Rc::new(Cell::new(false));
        let inner_flag = inner_seen.clone();
        let outer_flag = outer_seen.clone();

        let mut tree = WidgetTree::new();
        let outer = tree.add(FillWidget::new().focusable());
        let inner = tree.add_child(outer, FillWidget::new().focusable());

        // Inner observes then propagates; outer consumes.
        tree.push_action(
            inner,
            Action::new("app.save").on_invoke_with_response(move |_i, _c| {
                inner_flag.set(true);
                IntentResponse::Propagated
            }),
        );
        tree.push_action(
            outer,
            Action::new("app.save").on_invoke(move |_i, _c| {
                outer_flag.set(true);
            }),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(inner);

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(inner_seen.get(), "inner action observed the intent");
        assert!(outer_seen.get(), "outer action reached after Propagated");
    }

    #[test]
    fn handled_action_stops_propagation() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use std::cell::Cell;
        use std::rc::Rc;

        let inner_seen = Rc::new(Cell::new(false));
        let outer_seen = Rc::new(Cell::new(false));
        let inner_flag = inner_seen.clone();
        let outer_flag = outer_seen.clone();

        let mut tree = WidgetTree::new();
        let outer = tree.add(FillWidget::new().focusable());
        let inner = tree.add_child(outer, FillWidget::new().focusable());

        tree.push_action(
            inner,
            Action::new("app.save").on_invoke(move |_i, _c| {
                inner_flag.set(true);
            }),
        );
        tree.push_action(
            outer,
            Action::new("app.save").on_invoke(move |_i, _c| {
                outer_flag.set(true);
            }),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(inner);

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(inner_seen.get());
        assert!(!outer_seen.get(), "Handled at inner must stop propagation");
    }

    #[test]
    fn disabled_action_propagates_by_default() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use crate::signal::Signal;
        use std::cell::Cell;
        use std::rc::Rc;

        let inner_seen = Rc::new(Cell::new(false));
        let outer_seen = Rc::new(Cell::new(false));
        let inner_flag = inner_seen.clone();
        let outer_flag = outer_seen.clone();

        let mut tree = WidgetTree::new();
        let outer = tree.add(FillWidget::new().focusable());
        let inner = tree.add_child(outer, FillWidget::new().focusable());

        let enabled = Signal::new(false);
        tree.push_action(
            inner,
            Action::new("app.save")
                .enabled_when(enabled.clone())
                .on_invoke(move |_i, _c| {
                    inner_flag.set(true);
                }),
        );
        tree.push_action(
            outer,
            Action::new("app.save").on_invoke(move |_i, _c| {
                outer_flag.set(true);
            }),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(inner);

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(!inner_seen.get(), "disabled inner must not run");
        assert!(
            outer_seen.get(),
            "intent must propagate past disabled inner"
        );
    }

    #[test]
    fn disabled_action_with_non_propagating_shortcut_consumes() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use crate::signal::Signal;
        use std::cell::Cell;
        use std::rc::Rc;

        let inner_seen = Rc::new(Cell::new(false));
        let outer_seen = Rc::new(Cell::new(false));
        let inner_flag = inner_seen.clone();
        let outer_flag = outer_seen.clone();

        let mut tree = WidgetTree::new();
        let outer = tree.add(FillWidget::new().focusable());
        let inner = tree.add_child(outer, FillWidget::new().focusable());

        let enabled = Signal::new(false);
        tree.push_action(
            inner,
            Action::new("app.save")
                .enabled_when(enabled.clone())
                .on_invoke(move |_i, _c| {
                    inner_flag.set(true);
                }),
        );
        tree.push_action(
            outer,
            Action::new("app.save").on_invoke(move |_i, _c| {
                outer_flag.set(true);
            }),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .propagate_when_disabled(false)
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(inner);

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(!inner_seen.get(), "disabled inner still does not run");
        assert!(
            !outer_seen.get(),
            "intent must NOT propagate when shortcut disallows it"
        );
    }

    #[test]
    fn send_intent_from_handler_reaches_ancestor_action() {
        use crate::action::Action;
        use crate::intent::Intent;
        use std::cell::Cell;
        use std::rc::Rc;

        let save_seen = Rc::new(Cell::new(false));
        let save_flag = save_seen.clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(FillWidget::new());
        let button = tree.add_child(
            root,
            FillWidget::new().on_tap(|_pos, ctx| {
                ctx.send_intent(Intent::new("app.save"));
            }),
        );
        tree.push_action(
            root,
            Action::new("app.save").on_invoke(move |_i, _c| {
                save_flag.set(true);
            }),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.click(button);
        assert!(
            save_seen.get(),
            "ctx.send_intent must reach ancestor action"
        );
    }

    #[test]
    fn widget_type_histogram_counts_distinct_types() {
        // Phase 5.3: the histogram surfaces concrete widget types
        // by std::any::type_name_of_val. Widgets become active
        // after the first layout pass, so we run that before
        // checking the histogram.
        let mut tree = WidgetTree::new();
        let _ = tree.add(FillWidget::new());
        let _ = tree.add(FillWidget::new());
        let _ = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let histogram = tree.widget_type_histogram();
        let total: u32 = histogram.values().sum();
        assert!(
            total >= 3,
            "expected at least 3 active widgets, got {total}: {histogram:?}"
        );
        let fillwidget_entries: u32 = histogram
            .iter()
            .filter(|(k, _)| k.contains("FillWidget"))
            .map(|(_, v)| *v)
            .sum();
        assert!(
            fillwidget_entries >= 3,
            "expected ≥3 FillWidget instances; histogram = {histogram:?}"
        );
        assert_eq!(tree.active_widget_count() as u32, total);
    }

    #[test]
    fn intent_source_tagged_handler_for_tap_activation() {
        // Phase 5.2: a tap-driven `ctx.send_intent` must surface as
        // `IntentSource::Handler` to ancestor actions, not the
        // `Programmatic` default of `Intent::new`.
        use crate::action::Action;
        use crate::intent::Intent;
        use crate::telemetry::IntentSource;
        use std::cell::Cell;
        use std::rc::Rc;
        let captured = Rc::new(Cell::new(IntentSource::Unknown));
        let captured_for_action = captured.clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(FillWidget::new());
        let button = tree.add_child(
            root,
            FillWidget::new().on_tap(|_pos, ctx| {
                ctx.send_intent(Intent::new("app.save"));
            }),
        );
        tree.push_action(
            root,
            Action::new("app.save").on_invoke(move |intent, _c| {
                captured_for_action.set(intent.source);
            }),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.click(button);
        assert_eq!(
            captured.get(),
            IntentSource::Handler,
            "tap-driven intent must tag IntentSource::Handler"
        );
    }

    #[test]
    fn intent_source_programmatic_when_no_handler_active() {
        use crate::intent::Intent;
        use crate::telemetry::IntentSource;
        let intent = Intent::new("app.demo");
        assert_eq!(intent.source, IntentSource::Programmatic);

        // ctx.send_intent without a handler scope keeps it Programmatic.
        let mut ctx = EventContext::new();
        ctx.send_intent(Intent::new("app.demo"));
        let queued = ctx.pending_intents.first().expect("intent queued");
        assert_eq!(queued.source, IntentSource::Programmatic);
    }

    #[test]
    fn with_intent_source_overrides_for_managed_widgets() {
        use crate::intent::Intent;
        use crate::telemetry::IntentSource;
        let mut ctx = EventContext::new();
        ctx.with_intent_source(IntentSource::Menu, |ctx| {
            ctx.send_intent(Intent::new("app.demo"));
        });
        let queued = ctx.pending_intents.first().expect("intent queued");
        assert_eq!(
            queued.source,
            IntentSource::Menu,
            "with_intent_source(Menu) must tag the dispatched intent"
        );

        // After the closure returns, current_source is restored —
        // a follow-up send_intent without a wrapping closure goes
        // back to the default (no override).
        ctx.send_intent(Intent::new("app.next"));
        let next = ctx.pending_intents.last().expect("second intent");
        assert_eq!(next.source, IntentSource::Programmatic);
    }

    #[test]
    fn disabled_shortcut_falls_through_to_focused_widget() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use crate::signal::Signal;
        use std::cell::Cell;
        use std::rc::Rc;

        let action_fired = Rc::new(Cell::new(false));
        let on_key_fired = Rc::new(Cell::new(false));
        let af = action_fired.clone();
        let kf = on_key_fired.clone();

        let enabled = Signal::new(false);

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable().on_key(move |event, _ctx| {
            if matches!(
                event,
                WidgetEvent::KeyDown {
                    key: Key::S,
                    modifiers,
                    ..
                } if modifiers.ctrl()
            ) {
                kf.set(true);
                return EventResponse::Handled;
            }
            EventResponse::Ignored
        }));
        tree.push_action(
            widget,
            Action::new("app.save").on_invoke(move |_i, _c| af.set(true)),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .enabled_when(enabled.clone())
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        // Disabled: keystroke falls through to on_key.
        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(
            !action_fired.get(),
            "disabled shortcut must not invoke its action"
        );
        assert!(
            on_key_fired.get(),
            "disabled shortcut must let KeyDown reach the focused widget"
        );

        // Re-enable → action fires, on_key does not.
        on_key_fired.set(false);
        enabled.set(true);
        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(action_fired.get(), "re-enabled shortcut must dispatch");
        assert!(
            !on_key_fired.get(),
            "enabled shortcut must consume the KeyDown"
        );
    }

    #[test]
    fn scope_mismatch_does_not_invoke_on_activate() {
        use crate::intent::Intent;
        use crate::shortcut::{KeyStroke, Shortcut, ShortcutScope};
        use std::cell::Cell;
        use std::rc::Rc;

        // Regression: before the find/invoke split, `on_activate` ran
        // even when the focused widget was outside the shortcut's
        // scope, and any side effects on its ctx were silently
        // dropped. The closure must now only run when the scope
        // check has already passed.
        let activated = Rc::new(Cell::new(false));
        let activated_flag = activated.clone();

        let mut tree = WidgetTree::new();
        let scope_root = tree.add(FillWidget::new().focusable());
        let outside = tree.add(FillWidget::new().focusable());

        tree.shortcut_registry_mut().register(
            Shortcut::new("editor.find")
                .primary(KeyStroke::ctrl(Key::F))
                .scope(ShortcutScope::Scoped(scope_root))
                .on_activate(move |_ks, _ctx| {
                    activated_flag.set(true);
                    Intent::new("editor.find")
                })
                .build(),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(outside);

        tree.press_key(Key::F, Modifiers::CTRL);
        assert!(
            !activated.get(),
            "on_activate must not run when focus is outside the shortcut's scope"
        );
    }

    #[test]
    fn key_capture_runs_callback_and_bypasses_registry() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use std::cell::Cell;
        use std::rc::Rc;

        let action_fired = Rc::new(Cell::new(false));
        let af = action_fired.clone();
        let captured = Rc::new(Cell::new(None));
        let cf = captured.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable());
        tree.push_action(
            widget,
            Action::new("app.save").on_invoke(move |_i, _c| af.set(true)),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        let handle = tree.begin_key_capture(move |ks, _reg, _ctx| cf.set(Some(ks)));
        assert!(tree.is_capturing_keys());

        tree.press_key(Key::S, Modifiers::CTRL);
        assert_eq!(
            captured.get(),
            Some(KeyStroke::ctrl(Key::S)),
            "capture callback must receive the chord"
        );
        assert!(
            !action_fired.get(),
            "shortcut action must not fire while capture is armed"
        );
        assert!(
            !tree.is_capturing_keys(),
            "capture is one-shot; next KeyDown flows normally"
        );
        drop(handle);
    }

    #[test]
    fn key_capture_can_rebind_through_registry() {
        use crate::shortcut::{KeyStroke, Shortcut};

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable());
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        // Arm capture: whatever chord comes next, rebind app.save to it.
        let _h = tree.begin_key_capture(|ks, reg, _ctx| {
            reg.rebind_primary("app.save", Some(ks));
        });

        tree.press_key(Key::B, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(
            tree.shortcut_registry()
                .effective("app.save")
                .unwrap()
                .primary,
            Some(KeyStroke::ctrl_shift(Key::B))
        );
    }

    #[test]
    fn dropping_capture_handle_cancels_capture() {
        use crate::shortcut::{KeyStroke, Shortcut};
        use std::cell::Cell;
        use std::rc::Rc;

        let action_fired = Rc::new(Cell::new(false));
        let af = action_fired.clone();
        let capture_fired = Rc::new(Cell::new(false));
        let cf = capture_fired.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable());
        tree.push_action(
            widget,
            crate::action::Action::new("app.save").on_invoke(move |_i, _c| af.set(true)),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        // Arm capture in a scope, then drop the handle before any key
        // is pressed. The next KeyDown must fall through to the normal
        // shortcut path, firing the action — not the cancelled capture.
        {
            let _h = tree.begin_key_capture(move |_ks, _reg, _ctx| cf.set(true));
            assert!(tree.is_capturing_keys());
            // `_h` drops here → cancel.
        }
        assert!(
            !tree.is_capturing_keys(),
            "dropping the handle must cancel the capture"
        );

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(!capture_fired.get(), "cancelled capture must not fire");
        assert!(
            action_fired.get(),
            "shortcut action runs after capture was cancelled"
        );
    }

    #[test]
    fn second_begin_key_capture_does_not_racecancel_first() {
        use std::cell::Cell;
        use std::rc::Rc;

        let first = Rc::new(Cell::new(false));
        let second = Rc::new(Cell::new(false));
        let f = first.clone();
        let s = second.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        // Arm #1 then replace with #2. #1's handle is later dropped,
        // which would have cancelled the active capture under the old
        // `Option<Box<FnOnce>>` design — CaptureHandle now ties each
        // session to its own slot, so the drop only clears #1's
        // (orphaned) slot, not #2.
        let h1 = tree.begin_key_capture(move |_ks, _reg, _ctx| f.set(true));
        let _h2 = tree.begin_key_capture(move |_ks, _reg, _ctx| s.set(true));
        drop(h1);

        assert!(
            tree.is_capturing_keys(),
            "dropping the older handle must not cancel the active capture"
        );
        tree.press_key(Key::K, Modifiers::CTRL);
        assert!(!first.get());
        assert!(second.get(), "newest capture wins");
    }

    #[test]
    fn capture_callback_can_send_intent() {
        use crate::action::Action;
        use crate::intent::Intent;

        use std::cell::Cell;
        use std::rc::Rc;

        let ran = Rc::new(Cell::new(false));
        let flag = ran.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable());
        tree.push_action(
            widget,
            Action::new("app.save").on_invoke(move |_i, _c| flag.set(true)),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        let _h = tree.begin_key_capture(|_ks, _reg, ctx| {
            ctx.send_intent(Intent::new("app.save"));
        });
        tree.press_key(Key::X, Modifiers::CTRL);
        assert!(
            ran.get(),
            "intent queued from capture callback must dispatch"
        );
    }

    #[test]
    fn binding_registry_does_not_accumulate_across_rebuilds() {
        use crate::binding::BindingLevel;
        use crate::signal::Signal;

        #[derive(Debug)]
        struct BoundLeaf {
            tick: Signal<u64>,
        }
        impl crate::widget::Widget for BoundLeaf {
            fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
                self.tick.bind_to(
                    ctx.self_id(),
                    ctx.binding_registry(),
                    BindingLevel::Relayout,
                );
                Vec::new()
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &crate::widget::LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(10.0, 10.0).into()
            }
        }

        let mut tree = WidgetTree::new();
        let tick = Signal::new(0_u64);
        let widget = tree.add(BoundLeaf { tick: tick.clone() });
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let after_first_build = tree.binding_registry().len();
        assert!(after_first_build >= 1);

        // Force rebuild a handful of times and verify the binding
        // count does not keep growing. Pre-fix: each rebuild pushed
        // a new entry for the same (widget, signal) pair.
        for _ in 0..5 {
            tree.arena.mark_needs_rebuild(widget);
            tree.layout(SizeProposal::exact(200.0, 200.0));
        }
        assert_eq!(
            tree.binding_registry().len(),
            after_first_build,
            "bindings must be cleared on rebuild"
        );

        tree.destroy_subtree(widget);
        assert_eq!(
            tree.binding_registry().len(),
            0,
            "bindings must be cleared on destroy"
        );
        // Silence unused-variable warning for the signal.
        let _ = tick;
    }

    #[test]
    fn clear_shortcut_override_via_event_context_restores_default() {
        use crate::shortcut::{KeyStroke, Shortcut};

        let mut tree = WidgetTree::new();
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        tree.shortcut_registry_mut()
            .rebind_primary("app.save", Some(KeyStroke::alt(Key::S)));

        let source = tree.add(FillWidget::new());
        let mut ctx = EventContext::new();
        ctx.clear_shortcut_override("app.save");
        tree.collect_from_ctx(ctx, source);

        assert_eq!(
            tree.shortcut_registry()
                .effective("app.save")
                .unwrap()
                .primary,
            Some(KeyStroke::ctrl(Key::S))
        );
    }

    #[test]
    fn rebind_shortcut_primary_via_event_context() {
        use crate::shortcut::{KeyStroke, Shortcut};

        let mut tree = WidgetTree::new();
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        let source = tree.add(FillWidget::new());

        let mut ctx = EventContext::new();
        ctx.rebind_shortcut_primary("app.save", Some(KeyStroke::alt(Key::S)));
        tree.collect_from_ctx(ctx, source);

        assert_eq!(
            tree.shortcut_registry()
                .effective("app.save")
                .unwrap()
                .primary,
            Some(KeyStroke::alt(Key::S))
        );
    }

    #[test]
    fn unregister_all_for_owner_called_on_destroy() {
        use crate::shortcut::{KeyStroke, Shortcut};

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        let widget_owner = widget;
        tree.shortcut_registry_mut().register_owned(
            Shortcut::new("scoped.thing")
                .primary(KeyStroke::ctrl(Key::K))
                .build(),
            widget_owner,
        );
        assert!(
            tree.shortcut_registry()
                .get_default("scoped.thing")
                .is_some()
        );

        tree.destroy_subtree(widget);
        assert!(
            tree.shortcut_registry()
                .get_default("scoped.thing")
                .is_none(),
            "destroying the owner must unregister its shortcut"
        );
    }

    // --- Transform-aware hit-testing -------------------------------------
    //
    // `set_transform` scopes are paint-only: the renderer pushes the
    // transform around the subtree, so the visually-displayed area is
    // shifted relative to `arena.bounds(id)`. Hit-testing must inverse-
    // transform the screen-space input point as it descends through each
    // transform scope so that a click on the visually-rendered area lands
    // on the correct widget. Pre-fix, screen-space `bounds.contains(point)`
    // returned the *pre-transform* widget for in-bounds-pre-transform
    // points and missed the visually-shifted hit area entirely.

    #[test]
    fn hit_test_through_translate_scope() {
        use crate::test_widgets::StackWidget;
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new());
        let parent = tree.add(StackWidget::new().add_child(child));
        // Visually shift the entire subtree right by 100px.
        tree.set_transform(parent, fern_canvas::Transform2D::translate(100.0, 0.0));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // (50, 25) is inside the *pre-transform* bounds but the widget is
        // visually painted at x=100..200; a click at (50, 25) lands on
        // empty space.
        assert_eq!(
            tree.hit_test(Point::new(50.0, 25.0)),
            None,
            "pre-transform area is not visually populated and must not hit"
        );
        // (150, 25) is inside the visually-rendered area (post-translate).
        assert_eq!(
            tree.hit_test(Point::new(150.0, 25.0)),
            Some(child),
            "visually-rendered area must hit the child"
        );
        // Off everything.
        assert_eq!(tree.hit_test(Point::new(250.0, 25.0)), None);
    }

    #[test]
    fn hit_test_through_scale_scope() {
        use crate::test_widgets::StackWidget;
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new());
        let parent = tree.add(StackWidget::new().add_child(child));
        // Halve the visual size: pre-transform bounds (0,0,100,50) →
        // visually (0,0,50,25).
        tree.set_transform(parent, fern_canvas::Transform2D::scale(0.5, 0.5));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Inside the visual area.
        assert_eq!(tree.hit_test(Point::new(25.0, 12.0)), Some(child));
        // Outside the visual area but inside the pre-transform bounds.
        // Without the fix this would (incorrectly) hit the child.
        assert_eq!(
            tree.hit_test(Point::new(75.0, 25.0)),
            None,
            "scaled-out region must not hit"
        );
    }

    #[test]
    fn hit_test_through_nested_transforms_compose() {
        use crate::test_widgets::StackWidget;
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let inner = tree.add(StackWidget::new().add_child(leaf));
        let outer = tree.add(StackWidget::new().add_child(inner));
        // Outer translates by (100, 0); inner additionally scales by 2.
        // Effective at leaf = scale(2,2).then(translate(100,0)) — the
        // renderer composes deepest-first (see `effective_transform`).
        tree.set_transform(outer, fern_canvas::Transform2D::translate(100.0, 0.0));
        tree.set_transform(inner, fern_canvas::Transform2D::scale(2.0, 2.0));
        tree.layout(SizeProposal::exact(50.0, 25.0));

        // Leaf-local (0, 0) → scale → (0, 0) → translate → (100, 0).
        // Leaf-local (50, 25) → scale → (100, 50) → translate → (200, 50).
        // So the visual hit area is x in [100, 200], y in [0, 50].
        assert_eq!(tree.hit_test(Point::new(150.0, 25.0)), Some(leaf));
        assert_eq!(tree.hit_test(Point::new(50.0, 25.0)), None);
        assert_eq!(tree.hit_test(Point::new(250.0, 25.0)), None);
    }

    #[test]
    fn hit_test_identity_transform_unchanged() {
        // Sanity: an identity transform must not perturb the existing
        // hit-test behavior. Guards against accidental over-application
        // of inversion on the hot path.
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.set_transform(widget, fern_canvas::Transform2D::IDENTITY);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert_eq!(tree.hit_test(Point::new(50.0, 25.0)), Some(widget));
    }

    #[test]
    fn arena_effective_transform_composes_ancestors() {
        // `arena.effective_transform(id)` must equal the renderer's
        // transform-stack top by the time it begins painting `id` —
        // i.e. mapping `id`'s pre-transform local point to screen space.
        // The renderer's `PushTransform` handler composes as
        // `device_t.then(prev_top)` (see `fern-render/src/renderer.rs`),
        // so the *innermost* transform applies first to a local point.
        // For ancestors [outer, inner] both with transforms, this means
        // effective = inner.then(outer), NOT outer.then(inner).
        // fern-scene relies on this to project scene-coord bounds to
        // screen space when emitting AT nodes for view-transformed items.
        use crate::test_widgets::StackWidget;
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let inner = tree.add(StackWidget::new().add_child(leaf));
        let outer = tree.add(StackWidget::new().add_child(inner));
        tree.set_transform(outer, fern_canvas::Transform2D::translate(100.0, 0.0));
        tree.set_transform(inner, fern_canvas::Transform2D::scale(2.0, 2.0));
        tree.layout(SizeProposal::exact(50.0, 25.0));

        let eff = tree.arena.effective_transform(leaf);
        let expected = fern_canvas::Transform2D::scale(2.0, 2.0)
            .then(&fern_canvas::Transform2D::translate(100.0, 0.0));
        for (a, b) in eff.m.iter().zip(expected.m.iter()) {
            assert!(
                (a - b).abs() < 1e-5,
                "effective_transform mismatch: got {:?}, want {:?}",
                eff.m,
                expected.m
            );
        }

        // Concrete-point check that pins the composition order without
        // relying on matrix equality alone: a leaf-local point at the
        // bounds origin (0, 0) should land at screen (100, 0) — scale
        // first (still (0,0)), then translate by 100 in x. With the
        // wrong composition order it would land at (200, 0).
        let screen_origin = eff.apply_point(Point::new(0.0, 0.0));
        assert!((screen_origin.x - 100.0).abs() < 1e-5);
        assert!((screen_origin.y - 0.0).abs() < 1e-5);
        // Far corner: leaf-local (50, 25) → scale → (100, 50) → translate
        // by 100 in x → (200, 50).
        let screen_corner = eff.apply_point(Point::new(50.0, 25.0));
        assert!((screen_corner.x - 200.0).abs() < 1e-5);
        assert!((screen_corner.y - 50.0).abs() < 1e-5);
    }

    // ─── Context-menu factory: position, ctx, None fall-through ─────────

    /// A throwaway content widget the factory mounts. We never paint
    /// it — the test only checks that it lands in the overlay manager.
    #[derive(Debug)]
    struct StubMenu;
    impl crate::widget::Widget for StubMenu {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            fern_canvas::Size::new(100.0, 40.0).into()
        }
    }

    #[test]
    fn context_menu_factory_receives_click_position() {
        use crate::event::{Modifiers, PointerButton};
        use std::cell::Cell;
        use std::rc::Rc;

        let captured_position = Rc::new(Cell::new(None::<Point>));
        let cap = captured_position.clone();
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().context_menu(move |pos, _ctx| {
            cap.set(Some(pos));
            Some(Box::new(StubMenu) as Box<dyn crate::widget::Widget>)
        }));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let click = Point::new(73.0, 42.0);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: click,
            button: PointerButton::Secondary,
            modifiers: Modifiers::NONE,
        });

        let got = captured_position.get();
        assert_eq!(
            got,
            Some(click),
            "factory must receive the click position; got {:?}",
            got
        );
        let _ = widget;
    }

    #[test]
    fn context_menu_factory_returning_none_falls_through_to_parent() {
        use crate::event::{Modifiers, PointerButton};
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        // Outer factory always returns Some(StubMenu); inner factory
        // returns None. Right-click should walk past the inner and
        // mount the outer's menu.
        let outer_called = Rc::new(Cell::new(0_u32));
        let outer_flag = outer_called.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new().context_menu(|_pos, _ctx| None));
        let _outer = tree.add(StackWidget::new().add_child(inner).context_menu(
            move |_pos, _ctx| {
                outer_flag.set(outer_flag.get() + 1);
                Some(Box::new(StubMenu) as Box<dyn crate::widget::Widget>)
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Secondary,
            modifiers: Modifiers::NONE,
        });

        assert_eq!(
            outer_called.get(),
            1,
            "inner returning None must fall through to the outer factory"
        );
    }

    #[test]
    fn context_menu_factory_none_throughout_chain_does_not_show_overlay() {
        use crate::event::{Modifiers, PointerButton};

        // Single factory returning None → no overlay shown, no panic.
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().context_menu(|_pos, _ctx| None));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let overlay_count_before = tree.overlay_manager.len();
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Secondary,
            modifiers: Modifiers::NONE,
        });
        let overlay_count_after = tree.overlay_manager.len();
        assert_eq!(
            overlay_count_before, overlay_count_after,
            "a factory returning None must not mount any overlay"
        );
    }
}
