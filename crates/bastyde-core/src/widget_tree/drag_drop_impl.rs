// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

// App-global stash for the typed payload of an in-flight app-originated OS
// drag. When an in-app drag escalates to a native OS drag at the window
// boundary, the OS only carries the serialized MIME bytes — the typed
// `Box<dyn Any>` fast-path value would be lost. We park the whole `DragPayload`
// here for the lifetime of the OS drag so that if the drag wanders back over
// **any** window of this app (the source window or another one), that window
// can recover the original typed payload and present it as a normal internal
// drag. Single-threaded GUI ⇒ a thread-local is the process-wide registry, and
// it is only ever touched on the main thread (escalation and all
// `*_external_drag` / `handle_os_drag_ended` routing run there; the Wayland
// dispatch thread only `post_external`s).
//
// **Liveness gate.** Recovery is gated on the `live` flag, not on payload
// presence. `live` is set true by `outbound_begin` (escalation) and cleared by
// `outbound_end` (the terminal `DragEnded`, or a source-window close). This is
// what keeps a leaked / stale payload from misclaiming a *later* genuine
// external drag from another application: `outbound_take_if_live` only hands
// the payload back while `live`, and `outbound_restash` is a no-op once the
// drag has ended — so even a racing re-stash (cross-window drop-on-nothing)
// can't resurrect a finished drag.
struct OutboundStash {
    /// True while an app-originated OS drag is in flight.
    live: bool,
    /// The parked typed payload, present whenever no window currently holds it
    /// as a re-entered session.
    payload: Option<crate::drag_payload::DragPayload>,
}

thread_local! {
    static OUTBOUND: std::cell::RefCell<OutboundStash> =
        const { std::cell::RefCell::new(OutboundStash { live: false, payload: None }) };
}

/// Begin an outbound drag: mark live and park the typed payload.
fn outbound_begin(payload: crate::drag_payload::DragPayload) {
    OUTBOUND.with(|s| {
        let mut s = s.borrow_mut();
        s.live = true;
        s.payload = Some(payload);
    });
}

/// Recover the parked payload **only while the drag is live**. Leaves `live`
/// set (a window now holds the payload as a re-entered session).
fn outbound_take_if_live() -> Option<crate::drag_payload::DragPayload> {
    OUTBOUND.with(|s| {
        let mut s = s.borrow_mut();
        if s.live { s.payload.take() } else { None }
    })
}

/// Return a re-entered payload to the stash so another window can recover it —
/// but only if the drag is still live (a racing terminal event may have ended
/// it first, in which case the payload is dropped).
fn outbound_restash(payload: crate::drag_payload::DragPayload) {
    OUTBOUND.with(|s| {
        let mut s = s.borrow_mut();
        if s.live {
            s.payload = Some(payload);
        }
    });
}

/// Whether a payload is currently parked. Test/diagnostic helper.
#[cfg(test)]
fn has_outbound_typed() -> bool {
    OUTBOUND.with(|s| s.borrow().payload.is_some())
}

/// End the outbound drag: clear the live flag and drop any parked payload.
/// Idempotent. After this, no window can recover the payload.
fn outbound_end() {
    OUTBOUND.with(|s| {
        let mut s = s.borrow_mut();
        s.live = false;
        s.payload = None;
    });
}

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
        // The source widget, so a cancelled in-app drag still notifies its
        // originator via `on_drag_ended(Cancelled)`. External drags carry no
        // source (`None`), so they never fire it.
        let source = self.active_drag.as_ref().and_then(|d| d.source_widget);
        self.cleanup_drag_preview();
        self.active_drag = None;
        self.pointer_captured_by = None;
        self.current_cursor = crate::widget::CursorIcon::Default;
        if let Some(prev) = prev_target {
            self.fire_on_drag_leave(prev, &mut *ops);
        }
        if let Some(src) = source {
            self.fire_on_drag_ended(src, crate::drag_payload::DropOutcome::Cancelled, &mut *ops);
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

        // Is this our own app's in-flight OS drag wandering (back) over a
        // window? A non-empty global stash means an app-originated OS drag is
        // live (only one OS drag exists at a time), so recover the original
        // typed payload and present it as a normal *internal* drag. In-app
        // targets then see the typed value — this is what enables a drag to
        // round-trip out and back, and drag-and-drop between two windows of the
        // same app. The terminal `on_drag_ended` is owned by the source window
        // (via `DragEnded`), so this re-entered session carries no
        // `source_widget` and never fires it on drop.
        if let Some(mut payload) = outbound_take_if_live() {
            // Also expose the file/text/URI view derived from the carried MIME,
            // so the re-entered drag satisfies external-style targets (DropZone)
            // in addition to typed in-app targets.
            payload.enrich_external_from_mime();
            self.active_drag = Some(crate::drag_state::DragSession {
                payload,
                source_widget: None,
                is_external: false,
                current_position: position,
                current_target: None,
                feedback: crate::drag_state::DropFeedback::NoFeedback,
                preview_content_id: None,
                preview_overlay_id: None,
            });
            self.os_drag_reentered = true;
            self.handle_drag_move(position, &mut *ops);
            return;
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
        // Drives both a genuine external drag and our own re-entered OS drag
        // (now an internal session). `handle_drag_move` re-stashes and re-exits
        // if a re-entered drag leaves the window again.
        if self.active_drag.as_ref().is_some_and(|d| d.is_external) || self.os_drag_reentered {
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
        // Our own OS drag dropped inside an app window: complete it as an
        // internal drop with the recovered typed payload. The re-entered
        // session has no `source_widget`, so `handle_drag_drop` fires `on_drop`
        // on the target but not `on_drag_ended` — the source window fires that
        // once when the OS posts the terminal `DragEnded`. Clear the global
        // stash so that trailing event treats the drag as finished.
        if self.os_drag_reentered {
            self.os_drag_reentered = false;
            self.handle_drag_drop(position, &mut *ops);
            outbound_end();
            return;
        }
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
        // A re-entered OS drag leaving the window again must NOT cancel the
        // whole drag (the OS drag is still live) — re-stash the typed payload
        // for the next window it enters and tear down this internal session
        // without a terminal `on_drag_ended`.
        if self.os_drag_reentered {
            self.reexit_outbound(&mut *ops);
        } else if self.active_drag.as_ref().is_some_and(|d| d.is_external) {
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

    /// Fire `on_drag_ended` on a drag's **source** widget with the final
    /// outcome (in-app drop, OS export, or cancel). Mirrors
    /// [`Self::fire_on_drag_leave`]'s take/restore-handler discipline.
    pub(super) fn fire_on_drag_ended(
        &mut self,
        source_id: WidgetId,
        outcome: crate::drag_payload::DropOutcome,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if !self.arena.is_active(source_id) {
            return;
        }
        // Handlers attached at the widget's creation site live in the
        // `external_handlers` bucket; those installed from the widget's own
        // `build()` live in `handlers`. Fire whichever is present (both, if
        // both) — same dual-bucket discipline as `fire_on_drag_leave`.
        let (mut ext_handler, mut own_handler) = match self.arena.get_mut(source_id) {
            Some(node) => (
                node.external_handlers.on_drag_ended.take(),
                node.handlers.on_drag_ended.take(),
            ),
            None => return,
        };
        if ext_handler.is_none() && own_handler.is_none() {
            return;
        }
        let mut ctx = self.make_event_context(&mut *ops);
        if let Some(h) = ext_handler.as_mut() {
            h(outcome, &mut ctx);
        }
        if let Some(h) = own_handler.as_mut() {
            h(outcome, &mut ctx);
        }
        if let Some(node) = self.arena.get_mut(source_id) {
            node.external_handlers.on_drag_ended = ext_handler;
            node.handlers.on_drag_ended = own_handler;
        }
        self.collect_from_ctx(ctx, source_id);
    }

    /// Window content size (logical px) from the last layout proposal, if both
    /// axes were exact. Used to detect when an in-app drag leaves the window.
    fn window_content_size(&self) -> Option<(f32, f32)> {
        Some((self.last_proposal.width?, self.last_proposal.height?))
    }

    /// Whether `position` is outside this window's content rect. Unknown bounds
    /// (non-exact proposal) ⇒ never treated as outside.
    fn is_outside_window(&self, position: bastyde_canvas::Point) -> bool {
        match self.window_content_size() {
            Some((w, h)) => {
                position.x < 0.0 || position.y < 0.0 || position.x > w || position.y > h
            }
            None => false,
        }
    }

    /// When an **internal** drag whose payload is OS-exportable leaves the
    /// window bounds, hand it to the platform as a native OS drag. Returns
    /// `true` if it consumed the move (escalated, or re-exited a re-entered
    /// drag); `false` when escalation does not apply, leaving the caller to
    /// continue the normal in-app flow.
    fn try_escalate_to_os_drag(
        &mut self,
        position: bastyde_canvas::Point,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        // Already handed off to the OS and currently re-entered into this
        // window: leaving again must NOT start a second OS drag. Re-stash the
        // typed payload (so the next window can recover it) and tear the
        // internal session down without a terminal `on_drag_ended`.
        if self.os_drag_reentered {
            if self.is_outside_window(position) {
                self.reexit_outbound(&mut *ops);
                return true;
            }
            return false;
        }

        // Only a plain internal drag with an exportable payload escalates.
        let data = match self.active_drag.as_ref() {
            Some(d)
                if !d.is_external && d.source_widget.is_some() && d.payload.is_os_exportable() =>
            {
                d.payload.to_outbound()
            }
            _ => return false,
        };
        if !self.is_outside_window(position) {
            return false;
        }

        // Ask the platform to start a native OS drag. If it can't (no backend
        // / test sink), leave the in-app session intact — current
        // behavior: the drag can still come back into the window.
        if !ops.begin_os_drag(data, None) {
            return false;
        }

        // Escalated: the OS owns the drag now. Take the in-app session and park
        // its full (typed) payload in the app-global stash for the OS drag's
        // lifetime, so any window the drag re-enters can recover it. Remember
        // the source so the eventual `DragEnded` notifies it.
        let prev_target = self.active_drag.as_ref().and_then(|d| d.current_target);
        self.cleanup_drag_preview();
        let drag = self
            .active_drag
            .take()
            .expect("active_drag present (matched above)");
        self.pointer_captured_by = None;
        self.current_cursor = crate::widget::CursorIcon::Default;
        self.outbound_drag_source = drag.source_widget;
        outbound_begin(drag.payload);
        if let Some(prev) = prev_target {
            self.fire_on_drag_leave(prev, &mut *ops);
        }
        true
    }

    /// A re-entered OS drag left this window again: return the typed payload to
    /// the app-global stash and tear down the internal session, *without* a
    /// terminal `on_drag_ended` (the OS drag is still in flight).
    fn reexit_outbound(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let prev_target = self.active_drag.as_ref().and_then(|d| d.current_target);
        self.cleanup_drag_preview();
        if let Some(drag) = self.active_drag.take() {
            outbound_restash(drag.payload);
        }
        self.os_drag_reentered = false;
        self.pointer_captured_by = None;
        self.current_cursor = crate::widget::CursorIcon::Default;
        if let Some(prev) = prev_target {
            self.fire_on_drag_leave(prev, &mut *ops);
        }
    }

    /// Resolve an OS (outbound) drag at its terminal event. Clears the global
    /// typed-payload stash and fires `on_drag_ended(outcome)` once on the
    /// source widget (set only on the window that started the drag). Routed
    /// here by `bastyde-app` when the platform backend reports `DragEnded`.
    pub fn handle_os_drag_ended(
        &mut self,
        outcome: crate::drag_payload::DropOutcome,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // The OS guarantees this terminal event; end the stash so a later drag
        // from another app can't be mistaken for ours.
        outbound_end();
        self.os_drag_reentered = false;
        if let Some(source) = self.outbound_drag_source.take() {
            self.fire_on_drag_ended(source, outcome, &mut *ops);
        }
    }

    /// Abort any outbound OS drag this tree participates in, used when the
    /// window is closing. If this tree is the drag *source*, the whole drag is
    /// ending (the source object dies with the window) — end the stash so a
    /// later genuine external drag can't be mistaken for ours. If instead this
    /// is a non-source window currently holding the re-entered payload, hand it
    /// back to the stash so another window can still recover it. No
    /// `on_drag_ended` fires (the window and its handlers are being torn down).
    pub fn abort_outbound_drag(&mut self) {
        if self.outbound_drag_source.take().is_some() {
            outbound_end();
        } else if self.os_drag_reentered
            && let Some(drag) = self.active_drag.take()
        {
            outbound_restash(drag.payload);
        }
        self.os_drag_reentered = false;
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

        // If an internal, OS-exportable drag has left the window, hand it to
        // the OS as a native drag and stop the in-app pipeline.
        if self.try_escalate_to_os_drag(position, &mut *ops) {
            return;
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

        // Drop-target bubbling: walk up from the hit target through successive
        // drop targets, firing each one's `on_drag_hover`, and stop at the first
        // that ENGAGES (returns a non-`NoFeedback` response). A target that
        // returns `NoFeedback` does not accept this payload, so the drag bubbles
        // to the next drop target above it — letting a reorderable view behind a
        // per-row `DropTarget` still receive the drag. Pointer position is passed
        // to each handler in TARGET-LOCAL coordinates.
        let mut candidate = target.and_then(|t| self.find_drop_target_at_or_above(t));
        let mut engaged: Option<WidgetId> = None;
        let mut engaged_feedback = crate::drag_state::DropFeedback::NoFeedback;
        let mut bubbled_past: Vec<WidgetId> = Vec::new();
        while let Some(cand) = candidate {
            let fb = self.fire_on_drag_hover(cand, position, &mut *ops);
            if fb.is_engaged() {
                engaged = Some(cand);
                engaged_feedback = fb;
                break;
            }
            bubbled_past.push(cand);
            candidate = self.next_drop_target_above(cand);
        }

        // Resolve the tracked target and clear stray hover state:
        // - If an ancestor ENGAGED, every rejecting target we passed is
        //   transparent (the drag is accepted above) — clear them all so none
        //   leaves a stuck "forbidden" border.
        // - If NOTHING engaged, the drag is genuinely rejected: the DEEPEST drop
        //   target keeps its own reject affordance and becomes the tracked target
        //   (cleared when the drag moves off); clear only the ancestors above it.
        let (new_target, new_feedback) = if engaged.is_some() {
            for cand in &bubbled_past {
                self.fire_on_drag_leave(*cand, &mut *ops);
            }
            (engaged, engaged_feedback)
        } else if let Some((&deepest, rest)) = bubbled_past.split_first() {
            for cand in rest {
                self.fire_on_drag_leave(*cand, &mut *ops);
            }
            (Some(deepest), crate::drag_state::DropFeedback::NoFeedback)
        } else {
            (None, crate::drag_state::DropFeedback::NoFeedback)
        };

        // Fire `on_drag_leave` on the previously-tracked target when it changes.
        // Skip targets already cleared by the per-frame rejecter cleanup above:
        // a target that rejected this frame while an ancestor engaged is in
        // `bubbled_past` and has already had its `on_drag_leave` fired, so
        // re-firing here would deliver two leaves for one pointer move.
        let prev_target = self.active_drag.as_ref().and_then(|d| d.current_target);
        if prev_target != new_target
            && let Some(prev) = prev_target
            && !bubbled_past.contains(&prev)
        {
            self.fire_on_drag_leave(prev, &mut *ops);
        }
        if let Some(ref mut drag) = self.active_drag {
            drag.current_target = new_target;
            drag.feedback = new_feedback;
        }
    }

    /// Fire `on_drag_hover` on a single drop target and return its response.
    /// A drop target that has an `on_drop` handler but no `on_drag_hover`
    /// engages optimistically (`Accept`, no visual) so it can still receive the
    /// drop; `on_drop` makes the final decision on release.
    fn fire_on_drag_hover(
        &mut self,
        target_id: WidgetId,
        position: bastyde_canvas::Point,
        ops: &mut dyn crate::window::WindowOps,
    ) -> crate::drag_state::DropFeedback {
        use crate::drag_state::DropFeedback;
        let target_bounds = self.arena.bounds(target_id);
        let local =
            bastyde_canvas::Point::new(position.x - target_bounds.x, position.y - target_bounds.y);
        let (mut ext_handler, mut own_handler, has_on_drop) = match self.arena.get_mut(target_id) {
            Some(node) => {
                let has_on_drop = node.any_handler(|h| h.on_drop.is_some());
                let ext = node.external_handlers.on_drag_hover.take();
                let own = node.handlers.on_drag_hover.take();
                (ext, own, has_on_drop)
            }
            None => return DropFeedback::NoFeedback,
        };
        // Drop-only target (no hover handler): engage optimistically.
        if ext_handler.is_none() && own_handler.is_none() {
            return if has_on_drop {
                DropFeedback::Accept
            } else {
                DropFeedback::NoFeedback
            };
        }
        let mut feedback = DropFeedback::NoFeedback;
        if self.active_drag.is_some() {
            let mut ctx = self.make_event_context(&mut *ops);
            if let Some(ref drag) = self.active_drag {
                if let Some(h) = ext_handler.as_mut() {
                    feedback = h(&drag.payload, local, &mut ctx);
                }
                if let Some(h) = own_handler.as_mut() {
                    feedback = h(&drag.payload, local, &mut ctx);
                }
            }
            if let Some(node) = self.arena.get_mut(target_id) {
                node.external_handlers.on_drag_hover = ext_handler;
                node.handlers.on_drag_hover = own_handler;
            }
            self.collect_from_ctx(ctx, target_id);
            self.arena.mark_needs_paint(target_id);
        } else if let Some(node) = self.arena.get_mut(target_id) {
            node.external_handlers.on_drag_hover = ext_handler;
            node.handlers.on_drag_hover = own_handler;
        }
        feedback
    }

    /// The next drop target strictly above `id` (its nearest ancestor with a
    /// drop handler) — used to bubble a drag past a non-accepting target.
    fn next_drop_target_above(&self, id: WidgetId) -> Option<WidgetId> {
        let parent = self.arena.parent(id)?;
        self.find_drop_target_at_or_above(parent)
    }

    /// Complete the drag: fire `on_drop` on the target widget and end the session.
    pub(super) fn handle_drag_drop(
        &mut self,
        position: bastyde_canvas::Point,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Clean up preview overlay
        self.cleanup_drag_preview();

        if self.active_drag.is_none() {
            return;
        }

        // Determine the drop target while the session is still live. Normally
        // it's the target the last hover ENGAGED (drop-target bubbling already
        // chose it). For a drop with no prior hover (a quick drag, or a
        // programmatic `start_drag` + release), re-run the bubbling engagement at
        // the drop position so the drop still lands — and bubbles past a
        // non-accepting per-row target exactly as a hover would.
        // Ignore a `current_target` whose widget was destroyed since the last
        // hover (a rebuild tore it down mid-drag) — otherwise the drop resolves
        // to a dead arena id and is silently lost. Fall through to the
        // re-hit-test below so the drop still lands on whatever is live now.
        let mut drop_target = self
            .active_drag
            .as_ref()
            .and_then(|d| d.current_target)
            .filter(|&t| self.arena.is_active(t));
        if drop_target.is_none() {
            let hit = self.hit_test(position);
            let mut candidate = hit.and_then(|t| self.find_drop_target_at_or_above(t));
            while let Some(cand) = candidate {
                if self
                    .fire_on_drag_hover(cand, position, &mut *ops)
                    .is_engaged()
                {
                    drop_target = Some(cand);
                    break;
                }
                // Clear the bubbled-past target's hover state so it doesn't stay
                // highlighted after the drag ends.
                self.fire_on_drag_leave(cand, &mut *ops);
                candidate = self.next_drop_target_above(cand);
            }
        }

        // Take the drag session
        let drag = match self.active_drag.take() {
            Some(d) => d,
            None => return,
        };
        self.pointer_captured_by = None;
        self.current_cursor = crate::widget::CursorIcon::Default;
        // Source widget so an in-app drop notifies its originator via
        // `on_drag_ended`. External drags carry no source.
        let source = drag.source_widget;
        // Default: landed on nothing ⇒ cancelled. Set to `InApp { accepted }`
        // when a drop handler actually runs.
        let mut outcome = crate::drag_payload::DropOutcome::Cancelled;

        // Fire on_drag_leave on the engaged target before on_drop runs — widgets
        // own their feedback state and must be given a chance to clear it
        // regardless of whether the drop is accepted.
        if let Some(prev) = drop_target {
            self.fire_on_drag_leave(prev, &mut *ops);
        }

        // on_drop is a "decision" handler (returns bool). Prefer own over
        // external: the widget's own drop semantics trump any external
        // listener. If the own bucket doesn't have it, fall back to
        // external. Fires exactly once, not both.
        if let Some(target_id) = drop_target {
            let target_bounds = self.arena.bounds(target_id);
            let local = bastyde_canvas::Point::new(
                position.x - target_bounds.x,
                position.y - target_bounds.y,
            );
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
                let accepted = handler(drag.payload, local, &mut ctx);
                outcome = crate::drag_payload::DropOutcome::InApp { accepted };
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
        // Notify the source the drag it started has ended (in-app drops only;
        // external drags carry no source). Payload was moved into the handler
        // above, or dropped (Rust Drop) if unaccepted.
        if let Some(src) = source {
            self.fire_on_drag_ended(src, outcome, &mut *ops);
        }
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
    use crate::test_widgets::{FillWidget, StackWidget};
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
    fn drop_falls_through_a_destroyed_current_target() {
        // A rebuild that destroys the hovered drop target mid-drag (e.g. a
        // docking side disabled while dragging over its rail) must not leave a
        // stale `current_target` that swallows the drop into a dead arena id.
        // The drop should re-hit-test and land on the live target beneath.
        use std::cell::Cell;
        use std::rc::Rc;

        let bg_dropped = Rc::new(Cell::new(false));
        let bg_sink = bg_dropped.clone();

        let mut tree = WidgetTree::new();
        // Children stack (topmost = last added). Source at the bottom (just the
        // drag origin), then the background drop target, then the foreground
        // drop target on top.
        let source = tree.add(FillWidget::new());
        let _bg = tree.add(FillWidget::new().on_drop(move |_p, _pos, _ctx| {
            bg_sink.set(true);
            true
        }));
        // Foreground drop target on top — engages on hover so it becomes the
        // drag's `current_target`.
        let fg = tree.add(
            FillWidget::new()
                .on_drag_hover(|_payload, _pos, _ctx| {
                    crate::drag_state::DropFeedback::InsertionLine {
                        y: 50.0,
                        width: 200.0,
                    }
                })
                .on_drop(|_, _, _| true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(7_u32));
        tree.collect_from_ctx(ctx, source);

        // Hover over the foreground target → it becomes `current_target`.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        assert_eq!(
            tree.active_drag.as_ref().unwrap().current_target,
            Some(fg),
            "fg engaged as the current drop target"
        );

        // Tear the foreground target down mid-drag.
        tree.arena.destroy(fg);
        assert!(!tree.arena.is_active(fg));

        // Drop where fg used to be → must fall through to the live bg, not
        // vanish into the destroyed fg id.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(tree.active_drag.is_none(), "drag session cleared");
        assert!(
            bg_dropped.get(),
            "drop landed on the live background target, not the destroyed one"
        );
    }

    #[test]
    fn drag_arming_walks_to_an_ancestor_without_a_dead_zone() {
        // Baseline: pressing a button inside a draggable ancestor arms the
        // ancestor's drag recognizer (so a press-drag can start the ancestor
        // drag — the cross-widget tap/drag disambiguation).
        let mut tree = WidgetTree::new();
        let button = tree.add(FillWidget::new().on_tap(|_e, _ctx| {}));
        let inner = tree.add(StackWidget::new().add_child(button));
        let ancestor = tree.add(
            StackWidget::new()
                .add_child(inner)
                .on_drag(|_phase, _ctx| {}),
        );
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let b = tree.bounds(button);
        tree.pointer_down_button(
            Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0),
            PointerButton::Primary,
        );
        assert_eq!(
            tree.drag_observers,
            vec![ancestor],
            "the draggable ancestor is armed when the button press is not in a dead zone"
        );
        tree.pointer_up_button(
            Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0),
            PointerButton::Primary,
        );
    }

    #[test]
    fn gesture_dead_zone_blocks_ancestor_drag_arming() {
        // The fix: a `gesture_dead_zone` boundary between the button and the
        // draggable ancestor stops the arming walk — the ancestor is NEVER
        // armed, so no amount of pointer jitter while clicking the button can
        // start the ancestor's drag (capture-release-proof, unlike a
        // recognizer-shadowing absorber).
        use crate::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new();
        let button = tree.add(FillWidget::new().on_tap(|_e, _ctx| {}));
        let dead_zone = tree.add(StackWidget::new().add_child(button).gesture_dead_zone(true));
        let _ancestor = tree.add(
            StackWidget::new()
                .add_child(dead_zone)
                .on_drag(|_phase, _ctx| {}),
        );
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let b = tree.bounds(button);
        tree.pointer_down_button(
            Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0),
            PointerButton::Primary,
        );
        assert!(
            tree.drag_observers.is_empty(),
            "a dead zone blocks the draggable ancestor from being armed"
        );
        tree.pointer_up_button(
            Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0),
            PointerButton::Primary,
        );
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

    /// Regression: a rejecting per-row target nested under an engaging ancestor
    /// must receive exactly ONE `on_drag_leave` for the pointer move that flips
    /// the ancestor from idle to engaged — not two. The per-frame rejecter
    /// cleanup (it's in `bubbled_past`) and the tracked-target-change cleanup
    /// (it was last frame's `current_target`) used to fire independently.
    #[test]
    fn rejecter_under_engaging_ancestor_leaves_once() {
        use std::cell::Cell;
        use std::rc::Rc;

        let leaves = Rc::new(Cell::new(0));
        let lv = leaves.clone();
        // The ancestor only engages once we flip this between the two moves,
        // reproducing "frame 1 nothing engages, frame 2 the ancestor does".
        let engage = Rc::new(Cell::new(false));
        let eg = engage.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());

        // Deepest target: always rejects (NoFeedback), counts its leaves.
        let child = tree.add(
            FillWidget::new()
                .on_drag_hover(|_payload, _pos, _ctx| crate::drag_state::DropFeedback::NoFeedback)
                .on_drag_leave(move |_ctx| lv.set(lv.get() + 1)),
        );
        // Ancestor container wrapping the child: engages conditionally.
        let _ancestor = tree.add(StackWidget::new().add_child(child).on_drag_hover(
            move |_payload, _pos, _ctx| {
                if eg.get() {
                    crate::drag_state::DropFeedback::Accept
                } else {
                    crate::drag_state::DropFeedback::NoFeedback
                }
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(7_u32));
        tree.collect_from_ctx(ctx, source);

        // Frame 1: nothing engages → child becomes the tracked (rejecting) target.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        assert_eq!(
            tree.active_drag.as_ref().unwrap().current_target,
            Some(child)
        );
        assert_eq!(leaves.get(), 0, "no leave yet — child is freshly tracked");

        // Frame 2: ancestor engages while child still rejects.
        engage.set(true);
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(101.0, 50.0),
        });

        assert_eq!(
            leaves.get(),
            1,
            "child must receive exactly one on_drag_leave, not two"
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
    fn drop_bubbles_past_a_rejecting_child_to_ancestor() {
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        // A child drop target that REJECTS this payload (its `on_drag_hover`
        // returns `NoFeedback` and `on_drop` returns `false`) must NOT swallow
        // the drag — it bubbles to the accepting parent. This is the
        // per-row-`DropTarget`-over-a-reorderable-view case.
        let child_drop = Rc::new(Cell::new(false));
        let parent_drop = Rc::new(Cell::new(false));
        let cd = child_drop.clone();
        let pd = parent_drop.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let child = tree.add(
            FillWidget::new()
                .on_drag_hover(|_p, _pos, _ctx| crate::drag_state::DropFeedback::NoFeedback)
                .on_drop(move |_p, _pos, _ctx| {
                    cd.set(true);
                    false // reject → the framework should bubble past
                }),
        );
        let _parent = tree.add(StackWidget::new().add_child(child).on_drop(
            move |_p, _pos, _ctx| {
                pd.set(true);
                true
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(7_u8));
        tree.collect_from_ctx(ctx, source);

        // Hover over the child (its on_drag_hover runs → NoFeedback → bubble),
        // then release there.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 50.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(parent_drop.get(), "drop bubbles to the accepting ancestor");
        assert!(
            !child_drop.get(),
            "the rejecting child must not receive the drop"
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

        assert!(
            tree.active_drag.is_none(),
            "external drag must clear on drop"
        );
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
                .on_drag_hover(
                    |_p, _pos, _ctx| crate::drag_state::DropFeedback::HighlightRect {
                        rect: bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0),
                        color: bastyde_tokens::Color::WHITE,
                    },
                )
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
        tree.end_external_drag(
            Point::new(110.0, 55.0),
            ExternalDropData::default(),
            &mut noop,
        );

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
                .on_drag_hover(
                    |_p, _pos, _ctx| crate::drag_state::DropFeedback::HighlightRect {
                        rect: bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0),
                        color: bastyde_tokens::Color::WHITE,
                    },
                )
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
        assert_eq!(
            left.get(),
            1,
            "cancel must fire on_drag_leave on the target"
        );
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

    // --- Outbound (app → OS) escalation + unified on_drag_ended ----------

    /// `WindowOps` sink that records `begin_os_drag` calls and reports a
    /// configurable success, standing in for the platform backend.
    struct RecordingWindowOps {
        started: std::rc::Rc<std::cell::RefCell<Vec<crate::drag_payload::OutboundDragData>>>,
        succeed: bool,
        cancels: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl RecordingWindowOps {
        fn new(succeed: bool) -> Self {
            Self {
                started: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
                succeed,
                cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
            }
        }
    }
    impl crate::window::WindowOps for RecordingWindowOps {
        fn open_window(
            &mut self,
            _c: crate::window::WindowConfig,
        ) -> crate::window::BastydeWindowId {
            panic!("not used in these tests")
        }
        fn find_window(&self, _s: &str) -> Option<crate::window::BastydeWindowId> {
            None
        }
        fn window_state(
            &self,
            _id: crate::window::BastydeWindowId,
        ) -> Option<crate::window::WindowState> {
            None
        }
        fn windows(&self) -> Vec<crate::window::WindowState> {
            Vec::new()
        }
        fn focus_window(&mut self, _id: crate::window::BastydeWindowId) {}
        fn close_window_by_id(&mut self, _id: crate::window::BastydeWindowId) {}
        fn begin_os_drag(
            &mut self,
            data: crate::drag_payload::OutboundDragData,
            _image: Option<crate::drag_payload::DragImageData>,
        ) -> bool {
            self.started.borrow_mut().push(data);
            self.succeed
        }
        fn cancel_os_drag(&mut self) {
            self.cancels.set(self.cancels.get() + 1);
        }
    }

    fn exportable_payload() -> crate::drag_payload::DragPayload {
        crate::drag_payload::DragPayload::typed(7_u32).with_mime("text/plain", b"hi".to_vec())
    }

    #[test]
    fn internal_exportable_drag_escalates_when_leaving_window() {
        let started = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ops = RecordingWindowOps {
            started: started.clone(),
            succeed: true,
            cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
        };

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, exportable_payload());
        tree.collect_from_ctx(ctx, source);
        assert!(tree.active_drag.is_some());

        // Inside the window: no escalation.
        tree.handle_drag_move(Point::new(100.0, 50.0), &mut ops);
        assert!(started.borrow().is_empty());
        assert!(tree.active_drag.is_some());

        // Pointer leaves the window: escalate.
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops);
        assert_eq!(started.borrow().len(), 1, "begin_os_drag called once");
        assert!(
            started.borrow()[0].mime.contains_key("text/plain"),
            "outbound data carries the payload's mime"
        );
        assert!(tree.active_drag.is_none(), "in-app session torn down");
        assert_eq!(tree.outbound_drag_source, Some(source));
    }

    #[test]
    fn os_drag_ended_fires_source_on_drag_ended() {
        use crate::drag_payload::DropOutcome;
        use std::cell::Cell;
        use std::rc::Rc;

        let outcome = Rc::new(Cell::new(None));
        let o = outcome.clone();

        let started = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ops = RecordingWindowOps {
            started,
            succeed: true,
            cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
        };

        let mut tree = WidgetTree::new();
        let source =
            tree.add(FillWidget::new().on_drag_ended(move |outcome, _ctx| o.set(Some(outcome))));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, exportable_payload());
        tree.collect_from_ctx(ctx, source);
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops); // escalate
        assert_eq!(tree.outbound_drag_source, Some(source));

        tree.handle_os_drag_ended(DropOutcome::OsMove, &mut ops);
        assert_eq!(outcome.get(), Some(DropOutcome::OsMove));
        assert!(
            tree.outbound_drag_source.is_none(),
            "cleared after delivery"
        );
    }

    #[test]
    fn escape_during_an_escalated_drag_asks_the_platform_to_cancel() {
        // The in-app session is gone once the platform accepts the hand-off,
        // so this cannot ride the `active_drag` Escape path. Routing it through
        // `WindowOps` (rather than special-casing it in a backend's own event
        // loop) is what makes it observable here at all.
        let mut ops = RecordingWindowOps::new(true);
        let cancels = ops.cancels.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, exportable_payload());
        tree.collect_from_ctx(ctx, source);
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops); // escalate
        assert_eq!(tree.outbound_drag_source, Some(source));

        tree.dispatch_event_with_ops(
            crate::event::WidgetEvent::KeyDown {
                key: crate::event::Key::Escape,
                modifiers: crate::event::Modifiers::NONE,
                text: None,
            },
            &mut ops,
        );
        assert_eq!(cancels.get(), 1, "the platform must be asked to cancel");
        assert_eq!(
            tree.outbound_drag_source,
            Some(source),
            "the session stays until the backend reports its terminal outcome — \
             tearing it down here would drop the source's on_drag_ended"
        );
    }

    #[test]
    fn escape_without_an_os_drag_does_not_touch_the_platform() {
        let mut ops = RecordingWindowOps::new(true);
        let cancels = ops.cancels.clone();

        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.dispatch_event_with_ops(
            crate::event::WidgetEvent::KeyDown {
                key: crate::event::Key::Escape,
                modifiers: crate::event::Modifiers::NONE,
                text: None,
            },
            &mut ops,
        );
        assert_eq!(cancels.get(), 0);
    }

    #[test]
    fn no_backend_keeps_session_active_on_leave() {
        // begin_os_drag returns false (no outbound backend): the in-app
        // drag stays active so the user can drag back in — current behavior.
        let started = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ops = RecordingWindowOps {
            started: started.clone(),
            succeed: false,
            cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
        };

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, exportable_payload());
        tree.collect_from_ctx(ctx, source);
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops);

        assert_eq!(started.borrow().len(), 1, "escalation was attempted");
        assert!(tree.active_drag.is_some(), "session kept (no backend)");
        assert!(tree.outbound_drag_source.is_none());
    }

    #[test]
    fn non_exportable_drag_does_not_escalate() {
        let started = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ops = RecordingWindowOps {
            started: started.clone(),
            succeed: true,
            cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
        };

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Plain typed payload, no mime ⇒ not OS-exportable.
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(1_u32));
        tree.collect_from_ctx(ctx, source);
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops);

        assert!(started.borrow().is_empty(), "no escalation attempt");
        assert!(tree.active_drag.is_some(), "session unaffected");
    }

    #[test]
    fn in_app_drop_fires_source_on_drag_ended_with_accepted() {
        use crate::drag_payload::DropOutcome;
        use std::cell::Cell;
        use std::rc::Rc;

        let outcome = Rc::new(Cell::new(None));
        let o = outcome.clone();

        let mut tree = WidgetTree::new();
        let source =
            tree.add(FillWidget::new().on_drag_ended(move |outcome, _ctx| o.set(Some(outcome))));
        let _target = tree.add(FillWidget::new().on_drop(|_, _, _| true));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(42_u32));
        tree.collect_from_ctx(ctx, source);

        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(150.0, 50.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert_eq!(outcome.get(), Some(DropOutcome::InApp { accepted: true }));
    }

    #[test]
    fn escape_fires_source_on_drag_ended_cancelled() {
        use crate::drag_payload::DropOutcome;
        use std::cell::Cell;
        use std::rc::Rc;

        let outcome = Rc::new(Cell::new(None));
        let o = outcome.clone();

        let mut tree = WidgetTree::new();
        let source =
            tree.add(FillWidget::new().on_drag_ended(move |outcome, _ctx| o.set(Some(outcome))));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(0_u32));
        tree.collect_from_ctx(ctx, source);

        tree.press_key(Key::Escape, Modifiers::NONE);
        assert_eq!(outcome.get(), Some(DropOutcome::Cancelled));
    }

    /// Drag out (escalate to OS), then the OS drag re-enters the same window
    /// and drops on an in-app target: the original *typed* payload is
    /// recovered (not lost to the file/text round-trip), and the source's
    /// `on_drag_ended` fires exactly once with the OS outcome.
    #[test]
    fn os_drag_reentry_recovers_typed_payload_for_in_app_drop() {
        use crate::drag_payload::{DragPayload, DropOutcome};
        use std::cell::Cell;
        use std::rc::Rc;

        let started = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ops = RecordingWindowOps {
            started,
            succeed: true,
            cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
        };

        let got_typed = Rc::new(Cell::new(0_u32));
        let ended = Rc::new(Cell::new(0_u32));
        let last_outcome = Rc::new(Cell::new(None));
        let g = got_typed.clone();
        let e = ended.clone();
        let lo = last_outcome.clone();

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new().on_drag_ended(move |outcome, _ctx| {
            e.set(e.get() + 1);
            lo.set(Some(outcome));
        }));
        let _target =
            tree.add(
                FillWidget::new().on_drop(move |mut p, _, _| match p.take_typed::<u32>() {
                    Some(v) => {
                        g.set(v);
                        true
                    }
                    None => false,
                }),
            );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Internal drag with a typed value AND an exportable MIME rep.
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(
            source,
            DragPayload::typed(123_u32).with_mime("text/plain", b"x".to_vec()),
        );
        tree.collect_from_ctx(ctx, source);

        // Leave the window → escalate to OS drag (typed payload stashed).
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops);
        assert!(tree.active_drag.is_none());
        assert!(
            super::has_outbound_typed(),
            "typed payload stashed globally"
        );

        // OS drag re-enters → restored as an internal session with the typed
        // value (not an external file/text drop).
        tree.begin_external_drag(
            Point::new(100.0, 50.0),
            crate::drag_payload::ExternalDropData::default(),
            &mut ops,
        );
        let d = tree.active_drag.as_ref().expect("re-entered session");
        assert!(!d.is_external, "re-entry is an internal session");
        assert!(d.payload.has_typed::<u32>(), "typed payload recovered");
        assert_eq!(
            d.payload.text(),
            Some("x"),
            "external view enriched from MIME so DropZone-style targets also accept"
        );
        assert!(!super::has_outbound_typed(), "stash taken by the re-entry");

        // Drop inside on the target → on_drop receives the typed value.
        tree.end_external_drag(
            Point::new(150.0, 50.0),
            crate::drag_payload::ExternalDropData::default(),
            &mut ops,
        );
        assert_eq!(got_typed.get(), 123, "target received the typed payload");
        assert_eq!(
            ended.get(),
            0,
            "source on_drag_ended not fired by the drop itself"
        );

        // OS posts the terminal event on the source window → exactly one
        // on_drag_ended with the OS outcome.
        tree.handle_os_drag_ended(DropOutcome::OsCopy, &mut ops);
        assert_eq!(ended.get(), 1, "on_drag_ended fired exactly once");
        assert_eq!(last_outcome.get(), Some(DropOutcome::OsCopy));
    }

    /// The same recovery works across two windows of the same app: window A
    /// starts the drag, the OS drag enters window B, and B's target receives
    /// the original typed payload.
    #[test]
    fn os_drag_reentry_recovers_typed_payload_across_windows() {
        use crate::drag_payload::{DragPayload, DropOutcome};
        use std::cell::Cell;
        use std::rc::Rc;

        let started = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ops = RecordingWindowOps {
            started,
            succeed: true,
            cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
        };

        // Window A: starts and escalates.
        let mut tree_a = WidgetTree::new();
        let src = tree_a.add(FillWidget::new());
        tree_a.layout(SizeProposal::exact(200.0, 100.0));
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(
            src,
            DragPayload::typed(77_u32).with_mime("text/plain", b"x".to_vec()),
        );
        tree_a.collect_from_ctx(ctx, src);
        tree_a.handle_drag_move(Point::new(-5.0, 50.0), &mut ops);
        assert!(super::has_outbound_typed());
        assert_eq!(tree_a.outbound_drag_source, Some(src));

        // Window B (separate tree, same thread ⇒ same global stash): the OS
        // drag enters and drops on B's target, which gets the typed value.
        let got = Rc::new(Cell::new(0_u32));
        let g = got.clone();
        let mut tree_b = WidgetTree::new();
        let _t = tree_b.add(FillWidget::new().on_drop(
            move |mut p, _, _| match p.take_typed::<u32>() {
                Some(v) => {
                    g.set(v);
                    true
                }
                None => false,
            },
        ));
        tree_b.layout(SizeProposal::exact(200.0, 100.0));

        tree_b.begin_external_drag(
            Point::new(50.0, 50.0),
            crate::drag_payload::ExternalDropData::default(),
            &mut ops,
        );
        assert!(
            tree_b
                .active_drag
                .as_ref()
                .is_some_and(|d| d.payload.has_typed::<u32>()),
            "window B recovered the typed payload"
        );
        tree_b.end_external_drag(
            Point::new(50.0, 50.0),
            crate::drag_payload::ExternalDropData::default(),
            &mut ops,
        );
        assert_eq!(
            got.get(),
            77,
            "window B's target received the typed payload"
        );

        // Source window A reports the terminal outcome.
        tree_a.handle_os_drag_ended(DropOutcome::OsCopy, &mut ops);
    }

    /// A re-entered OS drag that leaves the window again re-stashes the typed
    /// payload (does not start a second OS drag, does not fire on_drag_ended),
    /// so a later window can still recover it.
    #[test]
    fn os_drag_reexit_restashes_payload() {
        use crate::drag_payload::DragPayload;
        use std::rc::Rc;

        let started = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ops = RecordingWindowOps {
            started: started.clone(),
            succeed: true,
            cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
        };

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(
            source,
            DragPayload::typed(9_u32).with_mime("text/plain", b"x".to_vec()),
        );
        tree.collect_from_ctx(ctx, source);

        // Escalate, then re-enter, then leave again.
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops);
        assert_eq!(started.borrow().len(), 1, "OS drag started once");
        tree.begin_external_drag(
            Point::new(100.0, 50.0),
            crate::drag_payload::ExternalDropData::default(),
            &mut ops,
        );
        assert!(tree.os_drag_reentered);
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops); // leave again

        assert!(!tree.os_drag_reentered, "re-exited");
        assert!(tree.active_drag.is_none(), "session torn down on re-exit");
        assert!(super::has_outbound_typed(), "payload re-stashed");
        assert_eq!(
            started.borrow().len(),
            1,
            "no second OS drag started on re-exit"
        );
    }

    /// Closing the source window mid-OS-drag clears the global stash, so a
    /// later genuine external drag from another app is NOT misrecovered as the
    /// stale typed payload. (Regression for the CRITICAL stash-leak finding.)
    #[test]
    fn source_window_close_clears_stash_no_hijack() {
        use crate::drag_payload::{DragPayload, ExternalDropData};
        use std::path::PathBuf;
        use std::rc::Rc;

        let started = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ops = RecordingWindowOps {
            started,
            succeed: true,
            cancels: std::rc::Rc::new(std::cell::Cell::new(0)),
        };

        let mut tree = WidgetTree::new();
        let source = tree.add(FillWidget::new());
        let _target = tree.add(FillWidget::new().on_drop(|_, _, _| true));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(
            source,
            DragPayload::typed(5_u32).with_mime("text/plain", b"x".to_vec()),
        );
        tree.collect_from_ctx(ctx, source);
        tree.handle_drag_move(Point::new(-5.0, 50.0), &mut ops); // escalate
        assert!(super::has_outbound_typed());
        assert_eq!(tree.outbound_drag_source, Some(source));

        // Window closes mid-drag.
        tree.abort_outbound_drag();
        assert!(
            !super::has_outbound_typed(),
            "stash cleared when the source window closes"
        );
        assert!(tree.outbound_drag_source.is_none());

        // A later real external drag (another app) must present as external,
        // NOT recover the stale typed payload.
        tree.begin_external_drag(
            Point::new(50.0, 50.0),
            ExternalDropData {
                files: vec![PathBuf::from("/tmp/real")],
                ..Default::default()
            },
            &mut ops,
        );
        let d = tree.active_drag.as_ref().expect("external session");
        assert!(
            d.is_external,
            "stale stash did not hijack the new external drag"
        );
        assert!(
            !d.payload.has_typed::<u32>(),
            "no stale typed payload leaked in"
        );
        assert_eq!(d.payload.files(), &[PathBuf::from("/tmp/real")]);
    }

    /// A re-stash that races in *after* the drag's terminal event must not
    /// resurrect a finished drag (cross-window drop-on-nothing race). Tests the
    /// liveness gate directly. (Regression for the HIGH race finding.)
    #[test]
    fn restash_after_drag_ended_is_noop() {
        use crate::drag_payload::DragPayload;

        super::outbound_begin(DragPayload::typed(1_u32).with_mime("text/plain", b"x".to_vec()));
        assert!(super::has_outbound_typed());
        // A window re-entered and took the payload.
        let held = super::outbound_take_if_live().expect("payload taken while live");
        assert!(!super::has_outbound_typed());
        // The source window's terminal DragEnded ends the drag first.
        super::outbound_end();
        // The other window's late re-stash must be dropped, not resurrected.
        super::outbound_restash(held);
        assert!(
            !super::has_outbound_typed(),
            "ended drag is not resurrected by a racing re-stash"
        );
        // And a take after end yields nothing.
        assert!(super::outbound_take_if_live().is_none());
    }
}
