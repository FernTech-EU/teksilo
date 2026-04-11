use super::*;

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

        if let WidgetEvent::KeyDown { key, modifiers, .. } = &event
            && let Some(cmd) = self.shortcut_map_lookup(*key, *modifiers)
        {
            self.pending_commands.push(cmd);
            self.flush_commands();
            return;
        }

        if let Some(raw) = to_raw_pointer_event(&event) {
            let target = if self.pointer_captured_by.is_some() {
                self.pointer_captured_by
            } else {
                match &event {
                    WidgetEvent::PointerDown { position, .. }
                    | WidgetEvent::PointerUp { position, .. } => self.hit_test(*position),
                    WidgetEvent::PointerMove { position } => self.hit_test(*position),
                    _ => None,
                }
            };
            if let Some(target_id) = target {
                self.feed_gesture_recognizers(target_id, &raw);
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
                    self.cycle_focus(modifiers.shift());
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
        self.flush_commands();
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
            dismiss: crate::overlay::DismissBehavior::ClickOutside,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
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
        if !self.arena.is_enabled(target) {
            return;
        }

        let mut ancestors = Vec::new();
        let mut current = self.arena.parent(target);
        while let Some(id) = current {
            ancestors.push(id);
            current = self.arena.parent(id);
        }
        ancestors.reverse();

        for &id in &ancestors {
            let mut ctx = EventContext::new();
            let response = if let Some(node) = self.arena.get_mut(id) {
                Self::try_handler_preview(node, event, &mut ctx).unwrap_or(EventResponse::Ignored)
            } else {
                EventResponse::Ignored
            };
            self.collect_from_ctx(ctx, id);
            if response == EventResponse::Handled {
                self.arena.mark_needs_paint(id);
                return;
            }
        }

        let needs_layout_on_handle = matches!(
            event,
            WidgetEvent::Scroll { .. } | WidgetEvent::ScrollIntoView { .. }
        );
        let mut current = Some(target);
        while let Some(id) = current {
            let mut ctx = EventContext::new();
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
                break;
            }
            current = self.arena.parent(id);
        }
    }

    pub(super) fn dispatch_to_widget_direct(&mut self, target: WidgetId, event: &WidgetEvent) {
        if !self.arena.is_enabled(target) {
            return;
        }

        let mut ctx = EventContext::new();
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
            WidgetEvent::AccessAction { action, .. } => node
                .handlers
                .on_access_action
                .as_mut()
                .map(|handler| handler(*action, ctx)),
            WidgetEvent::PointerDown {
                position, button, ..
            } => {
                if node.handlers.on_tap.is_some() {
                    let arena = node.handlers.gesture_arena.get_or_insert_with(|| {
                        let mut arena = GestureArena::new();
                        arena.add(crate::gesture::TapRecognizer::new());
                        arena
                    });
                    arena.process(&RawPointerEvent::Down {
                        position: *position,
                        button: *button,
                    });
                    return Some(EventResponse::Handled);
                }
                node.handlers
                    .on_pointer_event
                    .as_mut()
                    .map(|handler| handler(event, ctx))
            }
            WidgetEvent::PointerUp {
                position, button, ..
            } => {
                if let Some(ref mut arena) = node.handlers.gesture_arena {
                    let result = arena.process(&RawPointerEvent::Up {
                        position: *position,
                        button: *button,
                    });
                    if matches!(result, Some(GestureEvent::Tap { .. })) {
                        if let Some(ref mut handler) = node.handlers.on_tap {
                            handler(ctx);
                        }
                    }
                    return Some(EventResponse::Handled);
                }
                node.handlers
                    .on_pointer_event
                    .as_mut()
                    .map(|handler| handler(event, ctx))
            }
            WidgetEvent::PointerMove { position } => {
                if let Some(ref mut arena) = node.handlers.gesture_arena {
                    arena.process(&RawPointerEvent::Move {
                        position: *position,
                    });
                    return Some(EventResponse::Ignored);
                }
                node.handlers
                    .on_pointer_event
                    .as_mut()
                    .map(|handler| handler(event, ctx))
            }
            _ => None,
        }
    }

    fn collect_from_ctx(&mut self, ctx: EventContext, source_widget: WidgetId) {
        if let Some(cursor) = ctx.cursor_request {
            self.current_cursor = cursor;
        }
        self.pending_commands.extend(ctx.commands);
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
            self.focus(id);
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
                    let mut ctx = crate::widget::EventContext::new();

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
                    let mut ctx = crate::widget::EventContext::new();
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

    fn feed_gesture_recognizers(&mut self, target: WidgetId, raw: &RawPointerEvent) {
        let mut chain = vec![target];
        let mut current = self.arena.parent(target);
        while let Some(id) = current {
            chain.push(id);
            current = self.arena.parent(id);
        }

        for id in chain {
            if let Some(node) = self.arena.get_mut(id)
                && let Some(binding) = &mut node.gesture_binding
                && let Some(gesture) = binding.arena.process(raw)
            {
                let mut ctx = EventContext::new();
                (binding.handler)(gesture.clone(), &mut ctx);
                self.collect_from_ctx(ctx, id);
                self.dispatch_to_widget(id, &WidgetEvent::Gesture { gesture });
                return;
            }
        }
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

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Save,
    }

    impl AppCommand for TestCmd {}

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

    #[test]
    fn shortcut_intercepts_before_widget() {
        use crate::shortcut::{Shortcut, ShortcutMap};
        use std::cell::Cell;
        use std::rc::Rc;

        let save_called = Rc::new(Cell::new(false));
        let save_flag = save_called.clone();

        let shortcuts = ShortcutMap::new().bind(Shortcut::ctrl(Key::S), TestCmd::Save);

        let mut tree = WidgetTree::new().with_shortcuts(shortcuts);
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Save {
                save_flag.set(true);
            }
        });

        let widget = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(save_called.get());
    }

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
        use crate::gesture::TapRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let tapped_flag = tapped.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(widget, TapRecognizer::new(), move |gesture, _ctx| {
            if matches!(gesture, crate::gesture::GestureEvent::Tap { .. }) {
                tapped_flag.set(true);
            }
        });

        tree.click(widget);
        assert!(tapped.get());
    }

    #[test]
    fn gesture_drag_recognized_on_drag() {
        use crate::gesture::DragRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let drag_started = Rc::new(Cell::new(false));
        let drag_ended = Rc::new(Cell::new(false));
        let start_flag = drag_started.clone();
        let end_flag = drag_ended.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(
            widget,
            DragRecognizer::new().threshold(5.0),
            move |gesture, _ctx| match gesture {
                crate::gesture::GestureEvent::DragStarted { .. } => start_flag.set(true),
                crate::gesture::GestureEvent::DragEnded { .. } => end_flag.set(true),
                _ => {}
            },
        );

        tree.drag(Point::new(50.0, 25.0), Point::new(80.0, 25.0));

        assert!(drag_started.get());
        assert!(drag_ended.get());
    }

    #[test]
    fn gesture_handler_can_emit_commands() {
        use crate::gesture::TapRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let cmd_received = Rc::new(Cell::new(false));
        let received_flag = cmd_received.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(widget, TapRecognizer::new(), |_gesture, ctx| {
            ctx.emit(TestCmd::Save);
        });

        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Save {
                received_flag.set(true);
            }
        });

        tree.click(widget);
        assert!(cmd_received.get());
    }

    #[test]
    fn gesture_handler_called_on_tap() {
        use crate::gesture::TapRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let handler_called = Rc::new(Cell::new(false));
        let handler_flag = handler_called.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(widget, TapRecognizer::new(), move |_, _| {
            handler_flag.set(true);
        });

        tree.click(widget);
        assert!(handler_called.get());
    }

    #[test]
    fn multiple_recognizers_on_same_widget() {
        use crate::gesture::{DragRecognizer, TapRecognizer};
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let dragged = Rc::new(Cell::new(false));
        let tapped_flag = tapped.clone();
        let dragged_flag = dragged.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(widget, TapRecognizer::new(), move |gesture, _ctx| {
            if matches!(gesture, crate::gesture::GestureEvent::Tap { .. }) {
                tapped_flag.set(true);
            }
        });
        tree.attach_gesture(
            widget,
            DragRecognizer::new().threshold(5.0),
            move |gesture, _ctx| {
                if matches!(gesture, crate::gesture::GestureEvent::DragStarted { .. }) {
                    dragged_flag.set(true);
                }
            },
        );

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
        });
    }

    #[test]
    fn scoped_shortcut_fires_when_focused_in_subtree() {
        use std::cell::Cell;
        use std::rc::Rc;

        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Cmd {
            GlobalAction,
        }

        impl crate::app_command::AppCommand for Cmd {}

        let fired = Rc::new(Cell::new(None));
        let fired_flag = fired.clone();

        let shortcuts = crate::shortcut::ShortcutMap::new()
            .bind(crate::shortcut::Shortcut::ctrl(Key::Z), Cmd::GlobalAction);

        let mut tree = WidgetTree::new().with_shortcuts(shortcuts);
        tree.on_command(move |cmd: &Cmd| {
            fired_flag.set(Some(*cmd));
        });

        let parent = tree.add(FillWidget::new());
        let child = tree.add_child(parent, FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(child);
        tree.press_key(Key::Z, Modifiers::CTRL);
        assert_eq!(fired.get(), Some(Cmd::GlobalAction));
    }

    // --- Drag and Drop tests ---

    #[test]
    fn start_drag_creates_session() {
        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new().on_tap({
            move |ctx: &mut crate::widget::EventContext| {
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
        use std::cell::Cell;
        use std::rc::Rc;

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Register a command handler that panics if called
        let cmd_fired = Rc::new(Cell::new(false));
        let cf = cmd_fired.clone();
        tree.on_command(move |_cmd: &TestCmd| cf.set(true));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(99_i32));
        tree.collect_from_ctx(ctx, source);
        assert!(tree.active_drag.is_some());

        tree.press_key(Key::Escape, Modifiers::NONE);
        assert!(tree.active_drag.is_none(), "drag should be cancelled");
        assert!(!cmd_fired.get(), "Escape should not emit any command");
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
}
