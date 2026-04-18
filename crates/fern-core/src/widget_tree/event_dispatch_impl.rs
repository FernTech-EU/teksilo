use super::*;

use crate::gesture::{GestureArena, GestureEvent, RawPointerEvent};

impl WidgetTree {
    /// Dispatch an event into the widget tree.
    ///
    /// Routing rules (architecture Section 7.1):
    /// - Pointer events -> hit testing against layout tree
    /// - Keyboard/IME events -> focused widget
    /// - AccessKit actions -> target widget directly
    /// - Scroll events -> hit testing (scroll target under pointer)
    pub fn dispatch_event(&mut self, event: WidgetEvent) {
        if let WidgetEvent::KeyDown {
            key: Key::ArrowLeft,
            ..
        } = &event
            && self.overlay_manager.len() > 1
        {
            if let Some((_id, content_ids, focus_restore)) = self.overlay_manager.dismiss_top() {
                self.dormant_dismissed_content(&content_ids);
                if let Some(restore_id) = focus_restore {
                    if self.arena.is_active(restore_id) {
                        self.focus(restore_id);
                    }
                }
            }
            return;
        }

        if let WidgetEvent::KeyDown {
            key: Key::Escape, ..
        } = &event
            && !self.overlay_manager.is_empty()
        {
            if let Some((_id, content_ids, focus_restore)) =
                self.overlay_manager.try_dismiss_top_on_escape()
            {
                self.dormant_dismissed_content(&content_ids);
                if let Some(restore_id) = focus_restore {
                    if self.arena.is_active(restore_id) {
                        self.focus(restore_id);
                    }
                }
                return;
            }
        }

        if let WidgetEvent::PointerDown {
            position, button, ..
        } = &event
        {
            let dismissed = self.overlay_manager.handle_click_outside(*position);
            if !dismissed.is_empty() {
                self.dormant_dismissed_content(&dismissed);
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
        if let WidgetEvent::KeyDown { key, modifiers, .. } = &event {
            if let Some(callback) = self.take_key_capture() {
                let keystroke = crate::shortcut::KeyStroke::new(*key, *modifiers);
                let mut cap_ctx =
                    EventContext::new().with_app_context(self.app_context.clone());
                callback(keystroke, self.shortcut_registry_mut(), &mut cap_ctx);
                // Route side effects of the callback through the
                // focused widget (or an arbitrary root if no focus).
                let anchor = self.focused.or_else(|| self.arena.roots().first().copied());
                if let Some(anchor_id) = anchor {
                    self.collect_from_ctx(cap_ctx, anchor_id);
                    self.drain_pending_intents();
                }
                return;
            }
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
            let lookup = self.shortcut_registry.find_by_keystroke(keystroke).map(
                |eff| {
                    (
                        eff.shortcut.id,
                        eff.shortcut.scope,
                        eff.shortcut.propagate_when_disabled,
                    )
                },
            );
            if let Some((id, scope, propagate_when_disabled)) = lookup {
                let anchor = match scope {
                    // Global shortcuts fire regardless of focus. If no
                    // widget is currently focused, anchor the intent
                    // walk at an arbitrary root so actions registered
                    // at the top of the tree still see the intent.
                    crate::shortcut::ShortcutScope::Global => self
                        .focused
                        .or_else(|| self.arena.roots().first().copied()),
                    crate::shortcut::ShortcutScope::Scoped(scope_id) => self
                        .focused
                        .filter(|f| self.is_descendant_of(*f, scope_id)),
                };
                if let Some(anchor_id) = anchor {
                    let mut act_ctx =
                        EventContext::new().with_app_context(self.app_context.clone());
                    if let Some(intent) = self.shortcut_registry.invoke_on_activate(
                        id,
                        keystroke,
                        &mut act_ctx,
                    ) {
                        self.collect_from_ctx(act_ctx, anchor_id);
                        self.enqueue_intent(anchor_id, intent, propagate_when_disabled);
                        self.drain_pending_intents();
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
                    self.handle_drag_move(*position);
                    return;
                }
                WidgetEvent::PointerUp { position, .. } => {
                    self.handle_drag_drop(*position);
                    return;
                }
                WidgetEvent::KeyDown {
                    key: Key::Escape, ..
                } => {
                    self.cleanup_drag_preview();
                    self.active_drag = None;
                    self.pointer_captured_by = None;
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
                    );
                } else {
                    self.handle_pointer_move(*position);
                }
                self.update_pointer_leave_overlays(*position);
            }
            WidgetEvent::PointerDown {
                position, button, ..
            } => {
                if let Some(target) = self.hit_test(*position) {
                    if *button == PointerButton::Secondary
                        && self.show_context_menu_for(target, *position)
                    {
                        return;
                    }
                    if let Some(focusable) = self.find_focusable_at_or_above(target) {
                        self.focus_with_origin(focusable, crate::focus::FocusOrigin::Pointer);
                    }
                    self.dispatch_to_widget(target, &event);
                }
            }
            WidgetEvent::PointerUp { position, .. } => {
                if let Some(captured) = self.pointer_captured_by {
                    self.dispatch_to_widget(captured, &event);
                    self.pointer_captured_by = None;
                } else if let Some(target) = self.hit_test(*position) {
                    self.dispatch_to_widget(target, &event);
                }
            }
            WidgetEvent::Scroll { .. } => {
                if let Some(target) = self.hovered.or(self.focused) {
                    self.dispatch_to_widget(target, &event);
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
                            self.dispatch_to_widget_returning_handled(focused, &event)
                        })
                        .unwrap_or(false);
                    if !handled {
                        self.cycle_focus(modifiers.shift());
                    }
                } else if let Some(focused) = self.focused {
                    self.dispatch_to_widget(focused, &event);
                }
            }
            WidgetEvent::KeyUp { .. }
            | WidgetEvent::ImeComposition { .. }
            | WidgetEvent::ImeCommit { .. } => {
                if let Some(focused) = self.focused {
                    self.dispatch_to_widget(focused, &event);
                }
            }
            WidgetEvent::AccessAction { target, action, .. } => {
                if *action == accesskit::Action::Focus {
                    if let Some(id) = target.filter(|id| self.arena.is_active(*id)) {
                        self.focus_with_origin(id, crate::focus::FocusOrigin::Programmatic);
                    }
                } else {
                    let dispatch_target = target
                        .filter(|id| self.arena.is_active(*id))
                        .or(self.focused);
                    if let Some(id) = dispatch_target {
                        self.dispatch_to_widget(id, &event);
                    }
                }
            }
            WidgetEvent::Gesture { .. } => {
                if let Some(target) = self.hovered.or(self.focused) {
                    self.dispatch_to_widget(target, &event);
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
        self.drain_pending_intents();
    }

    fn show_context_menu_for(&mut self, target: WidgetId, position: Point) -> bool {
        let mut current = Some(target);
        let factory_owner = loop {
            match current {
                None => break None,
                Some(id) => {
                    if self
                        .arena
                        .get(id)
                        .is_some_and(|node| node.context_menu_factory.is_some())
                    {
                        break Some(id);
                    }
                    current = self.arena.get(id).and_then(|node| node.parent);
                }
            }
        };

        let Some(owner_id) = factory_owner else {
            return false;
        };

        let dismissed = self.overlay_manager.dismiss_all();
        self.dormant_dismissed_content(&dismissed);

        let menu_widget = {
            let node = self.arena.get(owner_id).unwrap();
            let factory = node.context_menu_factory.as_ref().unwrap();
            factory()
        };

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
        });
        if let Some(focus_id) = prev_focus {
            self.overlay_manager.set_top_focus_restore(focus_id);
        }
        self.focus(content_id);
        true
    }

    fn handle_pointer_move(&mut self, position: Point) {
        let target = self.hit_test(position);

        if target != self.hovered {
            if let Some(old) = self.hovered {
                self.dispatch_to_widget(old, &WidgetEvent::PointerLeave);
                self.tooltip_pointer_leave(old);
            }
            if let Some(new) = target {
                self.dispatch_to_widget(new, &WidgetEvent::PointerEnter);
                self.tooltip_pointer_enter(new);
            }
            self.hovered = target;
        }

        if let Some(target) = target {
            self.dispatch_to_widget(target, &WidgetEvent::PointerMove { position });
        }
    }

    pub(super) fn dispatch_to_widget(&mut self, target: WidgetId, event: &WidgetEvent) {
        self.dispatch_to_widget_returning_handled(target, event);
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
            let mut ctx = EventContext::new().with_app_context(self.app_context.clone());
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
        while let Some(id) = current {
            let mut ctx = EventContext::new().with_app_context(self.app_context.clone());
            let response = if let Some(node) = self.arena.get_mut(id) {
                Self::try_handler_bubble(node, event, &mut ctx).unwrap_or(EventResponse::Ignored)
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
            current = self.arena.parent(id);
        }
        false
    }

    pub(super) fn dispatch_to_widget_direct(&mut self, target: WidgetId, event: &WidgetEvent) {
        if !self.arena.is_enabled(target) {
            return;
        }

        let mut ctx = EventContext::new().with_app_context(self.app_context.clone());
        let response = if let Some(node) = self.arena.get_mut(target) {
            Self::try_handler_bubble(node, event, &mut ctx).unwrap_or(EventResponse::Ignored)
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
            WidgetEvent::KeyDown { .. } | WidgetEvent::KeyUp { .. } => None,
            _ => node
                .handlers
                .on_pointer_event
                .as_mut()
                .map(|handler| handler(event, ctx)),
        }
    }

    fn try_handler_bubble(
        node: &mut crate::arena::WidgetNode,
        event: &WidgetEvent,
        ctx: &mut EventContext,
    ) -> Option<EventResponse> {
        match event {
            WidgetEvent::PointerEnter => {
                if let Some(cursor) = node.node_cursor {
                    ctx.set_cursor(cursor);
                }
                node.handlers
                    .on_hover
                    .as_mut()
                    .map(|handler| {
                        handler(true, ctx);
                        EventResponse::Handled
                    })
                    .or_else(|| node.node_cursor.map(|_| EventResponse::Handled))
            }
            WidgetEvent::PointerLeave => {
                if node.node_cursor.is_some() {
                    ctx.set_cursor(crate::widget::CursorIcon::Default);
                }
                node.handlers
                    .on_hover
                    .as_mut()
                    .map(|handler| {
                        handler(false, ctx);
                        EventResponse::Handled
                    })
                    .or_else(|| node.node_cursor.map(|_| EventResponse::Handled))
            }
            WidgetEvent::FocusGained { .. } => node.handlers.on_focus.as_mut().map(|handler| {
                handler(true, ctx);
                EventResponse::Handled
            }),
            WidgetEvent::FocusLost => node.handlers.on_focus.as_mut().map(|handler| {
                handler(false, ctx);
                EventResponse::Handled
            }),
            WidgetEvent::KeyDown { .. } | WidgetEvent::KeyUp { .. } => node
                .handlers
                .on_key
                .as_mut()
                .map(|handler| handler(event, ctx)),
            WidgetEvent::Scroll { .. } | WidgetEvent::ScrollIntoView { .. } => node
                .handlers
                .on_scroll
                .as_mut()
                .map(|handler| handler(event, ctx)),
            WidgetEvent::AccessAction {
                action,
                target_node,
                data,
                ..
            } => {
                // Prefer the full-payload handler when the widget
                // has opted in; it's the one that receives
                // `target_node` and `data` so it can honour
                // `SetTextSelection` / `SetValue` / `SetScrollOffset`
                // correctly. Fall back to the bare handler
                // otherwise — most widgets only care about the
                // action type.
                if let Some(handler) = node.handlers.on_access_action_request.as_mut() {
                    Some(handler(*action, *target_node, data.clone(), ctx))
                } else {
                    node.handlers
                        .on_access_action
                        .as_mut()
                        .map(|handler| handler(*action, ctx))
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
                        | GestureEvent::PinchEnded => node.handlers.on_pinch.is_some(),
                        GestureEvent::Swipe { .. } => node.handlers.on_swipe.is_some(),
                        GestureEvent::DoubleTap { .. } => node.handlers.on_double_tap.is_some(),
                        GestureEvent::TripleTap { .. } => node.handlers.on_triple_tap.is_some(),
                        _ => false,
                    };
                    if has_handler {
                        Self::dispatch_recognized_gesture(node, gesture.clone(), ctx);
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
                position, button, ..
            } => {
                // Raw pointer handler runs first so widgets can intercept
                // events that the gesture recognizers won't catch (e.g.
                // right-click → context menu). If it returns Handled the
                // gesture arena is skipped; otherwise we fall through.
                if let Some(handler) = node.handlers.on_pointer_event.as_mut()
                    && handler(event, ctx) == EventResponse::Handled
                {
                    return Some(EventResponse::Handled);
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
                    });
                    if let Some(gesture) = result {
                        Self::dispatch_recognized_gesture(node, gesture, ctx);
                    }
                    return Some(EventResponse::Handled);
                }
                None
            }
            WidgetEvent::PointerUp {
                position, button, ..
            } => {
                if let Some(handler) = node.handlers.on_pointer_event.as_mut()
                    && handler(event, ctx) == EventResponse::Handled
                {
                    return Some(EventResponse::Handled);
                }
                if let Some(arena) = node.handlers.gesture_arena.as_mut() {
                    let result = arena.process(&RawPointerEvent::Up {
                        position: *position,
                        button: *button,
                    });
                    if let Some(gesture) = result {
                        Self::dispatch_recognized_gesture(node, gesture, ctx);
                    }
                    return Some(EventResponse::Handled);
                }
                None
            }
            WidgetEvent::PointerMove { position } => {
                if let Some(handler) = node.handlers.on_pointer_event.as_mut()
                    && handler(event, ctx) == EventResponse::Handled
                {
                    return Some(EventResponse::Handled);
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
            _ => None,
        }
    }

    /// Lazily install a gesture arena populated with whichever recognizers
    /// the widget's handler set actually needs. Without this, a widget
    /// that wires `on_drag` or `on_double_tap` (but not `on_tap`) would
    /// never get a gesture arena and the handlers would never fire.
    pub(crate) fn ensure_gesture_arena(node: &mut crate::arena::WidgetNode) {
        if node.handlers.gesture_arena.is_some() {
            return;
        }
        let has_tap = node.handlers.on_tap.is_some();
        let has_double_tap = node.handlers.on_double_tap.is_some();
        let has_triple_tap = node.handlers.on_triple_tap.is_some();
        let has_drag = node.handlers.on_drag.is_some();
        let has_long_press = node.handlers.on_long_press.is_some();
        let has_swipe = node.handlers.on_swipe.is_some();

        if !(has_tap
            || has_double_tap
            || has_triple_tap
            || has_drag
            || has_long_press
            || has_swipe)
        {
            return;
        }

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
            arena.add(crate::gesture::TapRecognizer::new());
        }
        if has_double_tap {
            arena.add(crate::gesture::DoubleTapRecognizer::new());
        }
        if has_triple_tap {
            arena.add(crate::gesture::TripleTapRecognizer::new());
        }
        if has_drag {
            arena.add(crate::gesture::DragRecognizer::new().threshold(5.0));
        }
        if has_long_press {
            arena.add(crate::gesture::LongPressRecognizer::new());
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
        match gesture {
            GestureEvent::Tap { position } => {
                if let Some(handler) = node.handlers.on_tap.as_mut() {
                    handler(position, ctx);
                }
            }
            GestureEvent::DoubleTap { position } => {
                if let Some(handler) = node.handlers.on_double_tap.as_mut() {
                    handler(position, ctx);
                }
            }
            GestureEvent::TripleTap { position } => {
                if let Some(handler) = node.handlers.on_triple_tap.as_mut() {
                    handler(position, ctx);
                }
            }
            GestureEvent::LongPress { position } => {
                if let Some(handler) = node.handlers.on_long_press.as_mut() {
                    handler(position, ctx);
                }
            }
            GestureEvent::DragStarted { position, button } => {
                // Auto-capture the pointer for the duration of the drag so
                // the widget keeps receiving `Moved` / `Ended` even when
                // the cursor leaves its bounds. Released on `DragEnded`.
                ctx.capture_pointer();
                if let Some(handler) = node.handlers.on_drag.as_mut() {
                    handler(DragPhase::Started { position, button }, ctx);
                }
            }
            GestureEvent::DragMoved { position, delta } => {
                if let Some(handler) = node.handlers.on_drag.as_mut() {
                    handler(DragPhase::Moved { position, delta }, ctx);
                }
            }
            GestureEvent::DragEnded { position } => {
                if let Some(handler) = node.handlers.on_drag.as_mut() {
                    handler(DragPhase::Ended { position }, ctx);
                }
                ctx.release_pointer();
            }
            GestureEvent::Swipe {
                direction,
                velocity,
            } => {
                if let Some(handler) = node.handlers.on_swipe.as_mut() {
                    handler(direction, velocity, ctx);
                }
            }
            GestureEvent::PinchStarted { center } => {
                if let Some(handler) = node.handlers.on_pinch.as_mut() {
                    handler(PinchPhase::Started { center }, ctx);
                }
            }
            GestureEvent::PinchChanged {
                center,
                scale,
                rotation,
            } => {
                if let Some(handler) = node.handlers.on_pinch.as_mut() {
                    handler(
                        PinchPhase::Changed {
                            center,
                            scale,
                            rotation,
                        },
                        ctx,
                    );
                }
            }
            GestureEvent::PinchEnded => {
                if let Some(handler) = node.handlers.on_pinch.as_mut() {
                    handler(PinchPhase::Ended, ctx);
                }
            }
        }
    }

    pub(super) fn collect_from_ctx(&mut self, ctx: EventContext, source_widget: WidgetId) {
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
        if ctx.dismiss_modal && !self.dismiss_modal_for_source(source_widget) {
            self.pending_modal_dismissal = true;
        }
        for callback in ctx.idle_callbacks {
            self.idle_queue.push_boxed(callback);
        }
        if ctx.dismiss_all_overlays {
            let dismissed = self.overlay_manager.dismiss_all();
            self.dormant_dismissed_content(&dismissed);
        } else if ctx.dismiss_top {
            if let Some((_id, content_ids, focus_restore)) = self.overlay_manager.dismiss_top() {
                self.dormant_dismissed_content(&content_ids);
                if let Some(restore_id) = focus_restore {
                    if self.arena.is_active(restore_id) {
                        self.focus(restore_id);
                    }
                }
            }
        } else {
            for id in ctx.overlay_dismissals {
                let dismissed = self.overlay_manager.dismiss(id);
                self.dormant_dismissed_content(&dismissed);
            }
        }
        for preserve_content in ctx.dismiss_descendant_overlays {
            self.dismiss_child_overlays_for_source(source_widget, preserve_content);
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
            self.focus(target);
        }

        // --- Drag and drop ---
        if let Some((source_widget, payload, preview_widget)) = ctx.drag_start_request {
            let (preview_content_id, preview_overlay_id) = if let Some(preview) = preview_widget {
                let content_id = self.arena.insert(preview);
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
                });
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
        }
        if ctx.cancel_drag {
            self.cleanup_drag_preview();
            self.active_drag = None;
            self.pointer_captured_by = None;
        }

        // --- Environment changes (architecture §9.5) ---
        if let Some(theme) = ctx.theme_request {
            self.set_theme(theme);
        }
        if let Some(locale) = ctx.locale_request {
            self.set_locale(locale);
        }
    }

    // --- Drag and drop helpers ---

    /// Clean up drag preview overlay (if any).
    fn cleanup_drag_preview(&mut self) {
        if let Some(ref drag) = self.active_drag {
            if let Some(overlay_id) = drag.preview_overlay_id {
                self.overlay_manager.dismiss(overlay_id);
            }
            if let Some(content_id) = drag.preview_content_id {
                self.arena.destroy(content_id);
            }
        }
    }

    /// Update the drag session on pointer move: find the drop target under the
    /// pointer and call its `on_drag_hover` handler.
    fn handle_drag_move(&mut self, position: fern_canvas::Point) {
        // Update position on the session
        if let Some(ref mut drag) = self.active_drag {
            drag.current_position = position;
        }

        // Update preview overlay position
        if let Some(ref drag) = self.active_drag {
            if let Some(overlay_id) = drag.preview_overlay_id {
                self.overlay_manager.update_placement(
                    overlay_id,
                    crate::overlay::OverlayPlacement::AtPointer(position),
                );
            }
        }

        // Hit-test to find the widget under the pointer
        let target = self.hit_test(position);

        // Walk up from hit target to find a widget with on_drag_hover
        let drop_target = target.and_then(|t| self.find_drop_target_at_or_above(t));

        // Update current target on the session
        if let Some(ref mut drag) = self.active_drag {
            let prev_target = drag.current_target;
            drag.current_target = drop_target;

            // If target changed, reset feedback
            if prev_target != drop_target {
                drag.feedback = crate::drag_state::DropFeedback::NoFeedback;
            }
        }

        // Call on_drag_hover on the target if it has one
        if let Some(target_id) = drop_target {
            // We need to temporarily take the drag payload reference for the callback.
            // Since on_drag_hover takes &DragPayload (not owned), we can borrow from the session.
            // But we also need &mut for the handler. Use take_widget pattern.
            if let Some(node) = self.arena.get_mut(target_id) {
                if let Some(mut handler) = node.handlers.on_drag_hover.take() {
                    // Temporarily read position and create a minimal event context
                    let mut ctx = crate::widget::EventContext::new()
                        .with_app_context(self.app_context.clone());

                    // We need access to the payload — borrow from active_drag
                    if let Some(ref drag) = self.active_drag {
                        let feedback = handler(&drag.payload, position, &mut ctx);
                        // Put handler back
                        if let Some(node) = self.arena.get_mut(target_id) {
                            node.handlers.on_drag_hover = Some(handler);
                        }
                        // Store feedback
                        if let Some(ref mut drag) = self.active_drag {
                            drag.feedback = feedback;
                        }
                        // Process any commands emitted
                        self.collect_from_ctx(ctx, target_id);
                    } else {
                        // Put handler back even if drag ended
                        if let Some(node) = self.arena.get_mut(target_id) {
                            node.handlers.on_drag_hover = Some(handler);
                        }
                    }
                }
            }
        }
    }

    /// Complete the drag: fire `on_drop` on the target widget and end the session.
    fn handle_drag_drop(&mut self, position: fern_canvas::Point) {
        // Clean up preview overlay
        self.cleanup_drag_preview();

        // Take the drag session
        let drag = match self.active_drag.take() {
            Some(d) => d,
            None => return,
        };
        self.pointer_captured_by = None;

        // Hit-test to find drop target
        let target = self.hit_test(position);
        let drop_target = target.and_then(|t| self.find_drop_target_at_or_above(t));

        if let Some(target_id) = drop_target {
            if let Some(node) = self.arena.get_mut(target_id) {
                if let Some(mut handler) = node.handlers.on_drop.take() {
                    let mut ctx = crate::widget::EventContext::new()
                        .with_app_context(self.app_context.clone());
                    let _accepted = handler(drag.payload, position, &mut ctx);
                    // Put handler back
                    if let Some(node) = self.arena.get_mut(target_id) {
                        node.handlers.on_drop = Some(handler);
                    }
                    self.collect_from_ctx(ctx, target_id);
                    return;
                }
            }
        }
        // Drop was not accepted — payload is dropped (Rust Drop)
    }

    /// Walk up from a widget to find the nearest ancestor (or self) with a
    /// drop handler (`on_drop` or `on_drag_hover`).
    fn find_drop_target_at_or_above(&self, start: WidgetId) -> Option<WidgetId> {
        let mut current = Some(start);
        while let Some(id) = current {
            if let Some(node) = self.arena.get(id) {
                if node.handlers.on_drop.is_some() || node.handlers.on_drag_hover.is_some() {
                    return Some(id);
                }
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
        if let Some(overlay_id) = self.overlay_manager.hit_test(point)
            && let Some(overlay) = self.overlay_manager.overlay(overlay_id)
        {
            return self.hit_test_recursive(overlay.content_id, point);
        }

        if self.overlay_manager.topmost_centered().is_some() {
            return None;
        }

        let roots = self.arena.roots();
        for &root in roots.iter().rev() {
            if let Some(hit) = self.hit_test_recursive(root, point) {
                return Some(hit);
            }
        }
        None
    }

    fn hit_test_recursive(&self, id: WidgetId, point: Point) -> Option<WidgetId> {
        if !self.arena.is_active(id) {
            return None;
        }
        let bounds = self.arena.bounds(id);
        if !bounds.contains(point) {
            return None;
        }
        let children = self.arena.children(id).to_vec();
        for &child in children.iter().rev() {
            if let Some(hit) = self.hit_test_recursive(child, point) {
                return Some(hit);
            }
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
        assert!(!tapped.get(), "disabled ancestor should block descendant tap");

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
        let widget = tree.add(FillWidget::new().on_drag(move |phase, _ctx| match phase {
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
        tree.add(FillWidget::new().on_swipe(move |direction, velocity, _ctx| {
            flag.set(Some((direction, velocity as i32)));
        }));
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
        tree.add(
            FillWidget::new().on_pinch(move |phase, _ctx| match phase {
                PinchPhase::Started { .. } => started_flag.set(true),
                PinchPhase::Changed { scale, .. } => scale_flag.set(scale),
                PinchPhase::Ended => ended_flag.set(true),
            }),
        );
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
        let widget = tree.add(
            FillWidget::new().on_drag(move |phase, _ctx| match phase {
                DragPhase::Started { .. } => started_flag.set(true),
                DragPhase::Moved { .. } => moved_flag.set(moved_flag.get() + 1),
                DragPhase::Ended { .. } => ended_flag.set(true),
            }),
        );
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
        assert!(moved.get() >= 1, "Move outside bounds must still reach drag handler");

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
        assert_eq!(fired.get(), 0, "scoped shortcut must not fire outside scope");

        // Focus inside the scope: it fires.
        tree.focus(inside);
        tree.press_key(Key::F, Modifiers::CTRL);
        assert_eq!(fired.get(), 1, "scoped shortcut must fire when focus in scope");
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
        assert!(outer_seen.get(), "intent must propagate past disabled inner");
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
        assert!(save_seen.get(), "ctx.send_intent must reach ancestor action");
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
        let widget = tree.add(
            FillWidget::new()
                .focusable()
                .on_key(move |event, _ctx| {
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
                }),
        );
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
            crate::action::Action::new("app.save")
                .on_invoke(move |_i, _c| af.set(true)),
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
        use crate::shortcut::KeyStroke;
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
        use crate::shortcut::KeyStroke;
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
            fn build(
                &mut self,
                ctx: &mut crate::build_context::BuildContext,
            ) -> Vec<WidgetId> {
                self.tick.bind_to(
                    ctx.self_id(),
                    ctx.binding_registry(),
                    BindingLevel::Relayout,
                );
                Vec::new()
            }
            fn size_that_fits(
                &self,
                proposal: SizeProposal,
                _ctx: &crate::widget::LayoutContext,
            ) -> fern_canvas::Size {
                proposal.resolve(10.0, 10.0)
            }
        }

        let mut tree = WidgetTree::new();
        let tick = Signal::new(0_u64);
        let widget = tree.add(BoundLeaf {
            tick: tick.clone(),
        });
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
        assert!(tree.shortcut_registry().get_default("scoped.thing").is_some());

        tree.destroy_subtree(widget);
        assert!(
            tree.shortcut_registry().get_default("scoped.thing").is_none(),
            "destroying the owner must unregister its shortcut"
        );
    }
}
