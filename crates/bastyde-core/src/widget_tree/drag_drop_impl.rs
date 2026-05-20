use super::*;

impl WidgetTree {
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

    // --- External (OS) drag-and-drop -----------------------------------
    //
    // OS drops (files / text / URLs dragged from another application or the
    // file manager) reuse the *entire* internal drag pipeline. Rather than a
    // parallel set of handlers, an external drag synthesises a `DragSession`
    // carrying a `DragPayload::external(...)` and then drives the same
    // `handle_drag_move` / `handle_drag_drop` / `cancel_active_drag` paths, so
    // any widget with `on_drag_hover` / `on_drag_leave` / `on_drop` works for
    // both internal and external drags. Widgets distinguish the source via
    // `payload.is_external()` / `payload.files()` etc.
    //
    // Differences from internal drags: there is no in-app source widget
    // (`source_widget = None`), no pointer capture (the OS owns the pointer
    // during its drag loop), and no in-tree preview overlay (the OS renders
    // its own drag image).

    /// Begin an external drag session at `position` carrying OS-delivered
    /// `data`. Establishes the initial hover target and feedback immediately.
    pub fn begin_external_drag(
        &mut self,
        position: bastyde_canvas::Point,
        data: crate::drag_payload::ExternalDropData,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Defensively clear any stale session (e.g. a re-entered drag that
        // never delivered a matching leave). cancel_active_drag fires
        // on_drag_leave on the previous target first.
        if self.active_drag.is_some() {
            self.cancel_active_drag(&mut *ops);
        }
        self.active_drag = Some(crate::drag_state::DragSession {
            payload: crate::drag_payload::DragPayload::external(data),
            source_widget: None,
            is_external: true,
            current_position: position,
            current_target: None,
            feedback: crate::drag_state::DropFeedback::NoFeedback,
            preview_content_id: None,
            preview_overlay_id: None,
        });
        // No pointer capture, no Grabbing cursor — the OS owns the drag image
        // and cursor during an external drag.
        self.handle_drag_move(position, &mut *ops);
    }

    /// Update an in-flight external drag as the OS reports pointer motion.
    /// No-op unless an external session is active.
    pub fn update_external_drag(
        &mut self,
        position: bastyde_canvas::Point,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if self.active_drag.as_ref().is_some_and(|d| d.is_external) {
            self.handle_drag_move(position, &mut *ops);
        }
    }

    /// Complete an external drag with a drop at `position`, firing `on_drop`
    /// on the target. `data` is the authoritative payload read at drop time;
    /// if non-empty it replaces the session payload (some backends only have
    /// the full data at drop, not at enter). No-op unless an external session
    /// is active.
    pub fn end_external_drag(
        &mut self,
        position: bastyde_canvas::Point,
        data: crate::drag_payload::ExternalDropData,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if !self.active_drag.as_ref().is_some_and(|d| d.is_external) {
            return;
        }
        if !data.is_empty()
            && let Some(drag) = self.active_drag.as_mut()
        {
            drag.payload = crate::drag_payload::DragPayload::external(data);
        }
        self.handle_drag_drop(position, &mut *ops);
    }

    /// Cancel an in-flight external drag (the pointer left the window or the
    /// OS aborted the operation) without dropping. No-op unless an external
    /// session is active.
    pub fn cancel_external_drag(&mut self, ops: &mut dyn crate::window::WindowOps) {
        if self.active_drag.as_ref().is_some_and(|d| d.is_external) {
            self.cancel_active_drag(&mut *ops);
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
        let local = bastyde_canvas::Point::new(position.x - bounds.x, position.y - bounds.y);
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
    pub(super) fn handle_drag_move(
        &mut self,
        position: bastyde_canvas::Point,
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
                bastyde_canvas::Point::new(position.x - target_bounds.x, position.y - target_bounds.y);
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
    pub(super) fn handle_drag_drop(
        &mut self,
        position: bastyde_canvas::Point,
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
                bastyde_canvas::Point::new(position.x - target_bounds.x, position.y - target_bounds.y);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget::CursorIcon;
    use crate::widget_builder::WidgetBuilder;

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
        assert_eq!(drag.source_widget, Some(source));
        assert!(!drag.is_external);
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
        // bastyde-app applies the tree's cursor to the winit window after
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
                bastyde_canvas::Size::new(50.0, 20.0).into()
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

    // --- External (OS) drag-and-drop -----------------------------------

    #[test]
    fn external_drop_delivers_files_and_marks_external() {
        use crate::drag_payload::ExternalDropData;
        use std::cell::RefCell;
        use std::path::PathBuf;
        use std::rc::Rc;

        let dropped_files: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let was_external = Rc::new(std::cell::Cell::new(false));
        let df = dropped_files.clone();
        let we = was_external.clone();

        let mut tree = WidgetTree::new();
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(|payload, _pos, _ctx| {
                    // External file drags are accepted with a highlight.
                    if payload.is_external() && !payload.files().is_empty() {
                        crate::drag_state::DropFeedback::HighlightRect {
                            rect: bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0),
                            color: bastyde_tokens::Color::WHITE,
                        }
                    } else {
                        crate::drag_state::DropFeedback::NoFeedback
                    }
                })
                .on_drop(move |payload, _pos, _ctx| {
                    we.set(payload.is_external());
                    *df.borrow_mut() = payload.files().to_vec();
                    true
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut noop = crate::window::NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.png")],
            ..Default::default()
        };
        tree.begin_external_drag(Point::new(100.0, 50.0), data, &mut noop);
        assert!(tree.active_drag.is_some());
        assert!(tree.active_drag.as_ref().unwrap().is_external);

        tree.update_external_drag(Point::new(110.0, 55.0), &mut noop);
        // Pass the same files again at drop — exercises the payload-refresh path.
        let drop_data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.png")],
            ..Default::default()
        };
        tree.end_external_drag(Point::new(110.0, 55.0), drop_data, &mut noop);

        assert!(tree.active_drag.is_none(), "external drag must clear on drop");
        assert!(was_external.get(), "payload should report external origin");
        assert_eq!(
            *dropped_files.borrow(),
            vec![PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.png")],
        );
    }

    #[test]
    fn external_drop_passes_local_coordinates() {
        use crate::drag_payload::ExternalDropData;
        use crate::test_widgets::InsetWidget;
        use std::cell::Cell;
        use std::path::PathBuf;
        use std::rc::Rc;

        let drop_local = Rc::new(Cell::new(Point::new(-1.0, -1.0)));
        let d = drop_local.clone();

        let mut tree = WidgetTree::new();
        // Inset 40 → target origin at (40, 40).
        let target = tree.add(
            FillWidget::new()
                .on_drag_hover(|_p, _pos, _ctx| crate::drag_state::DropFeedback::HighlightRect {
                    rect: bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0),
                    color: bastyde_tokens::Color::WHITE,
                })
                .on_drop(move |_payload, pos, _ctx| {
                    d.set(pos);
                    true
                }),
        );
        let _wrapper = tree.add(InsetWidget::new(40.0).set_child(target));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut noop = crate::window::NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/x")],
            ..Default::default()
        };
        // Drop at tree (110, 55) → target-local (70, 15).
        tree.begin_external_drag(Point::new(110.0, 55.0), data, &mut noop);
        tree.end_external_drag(Point::new(110.0, 55.0), ExternalDropData::default(), &mut noop);

        let drp = drop_local.get();
        assert!(
            (drp.x - 70.0).abs() < 0.01 && (drp.y - 15.0).abs() < 0.01,
            "external on_drop should receive local coords, got {:?}",
            drp,
        );
    }

    #[test]
    fn cancel_external_drag_clears_session_and_fires_leave() {
        use crate::drag_payload::ExternalDropData;
        use std::cell::Cell;
        use std::path::PathBuf;
        use std::rc::Rc;

        let left = Rc::new(Cell::new(0_u32));
        let l = left.clone();

        let mut tree = WidgetTree::new();
        let _target = tree.add(
            FillWidget::new()
                .on_drag_hover(|_p, _pos, _ctx| crate::drag_state::DropFeedback::HighlightRect {
                    rect: bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0),
                    color: bastyde_tokens::Color::WHITE,
                })
                .on_drag_leave(move |_ctx| l.set(l.get() + 1))
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut noop = crate::window::NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/x")],
            ..Default::default()
        };
        tree.begin_external_drag(Point::new(100.0, 50.0), data, &mut noop);
        assert!(tree.active_drag.is_some());

        tree.cancel_external_drag(&mut noop);
        assert!(tree.active_drag.is_none(), "cancel must clear the session");
        assert_eq!(left.get(), 1, "cancel must fire on_drag_leave on the target");
    }

    #[test]
    fn external_drag_helpers_noop_without_session() {
        // update/end/cancel are no-ops when no external session is active.
        let mut tree = WidgetTree::new();
        let _t = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let mut noop = crate::window::NoopWindowOps;
        tree.update_external_drag(Point::new(10.0, 10.0), &mut noop);
        tree.end_external_drag(
            Point::new(10.0, 10.0),
            crate::drag_payload::ExternalDropData::default(),
            &mut noop,
        );
        tree.cancel_external_drag(&mut noop);
        assert!(tree.active_drag.is_none());
    }
}
