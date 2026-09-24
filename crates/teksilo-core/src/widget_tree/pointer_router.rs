// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer, keyboard and accessibility event routing: the dispatch
//! pipeline, hit-testing, the preview/bubble handler passes and the
//! context-menu entry points.

use super::*;

use crate::gesture::{GestureEvent, RawPointerEvent, TapEvent};

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

/// A dispatch deferred because another was in flight.
///
/// Carries everything needed to replay it faithfully: the event, and the input
/// snapshot that says which pointer produced it. `ops` is not carried — the
/// drain runs inside the same top-level call, so the caller's sink is still in
/// hand.
pub(super) enum QueuedDispatch {
    /// A nested `dispatch_*` call, replayed verbatim — event *and* input
    /// snapshot — once the outer sample completes.
    ///
    /// The snapshot is boxed: it carries the packet's coalesced positions, so
    /// it is by some way the largest thing either variant holds, and a queue
    /// entry that is mostly padding would be paid for on the cancel path too.
    Event {
        event: WidgetEvent,
        snapshot: Box<crate::pointer::InputSnapshot>,
    },
    /// A revocation raised through
    /// [`WidgetTree::cancel_pointer`](crate::WidgetTree::cancel_pointer).
    ///
    /// It rides this queue rather than one of its own so that a cancel and the
    /// dispatch that provoked it cannot be reordered relative to each other:
    /// one queue is one order. It carries the reason rather than a
    /// pre-built `PointerCancel`, because the event's position and
    /// `PointerInfo` must be read from the table at *drain* time — by then the
    /// pointer may have moved, or ceased to exist, and a snapshot taken at
    /// queue time would describe a state the widget is no longer in.
    Cancel {
        pointer: crate::pointer::PointerId,
        reason: crate::pointer::CancelReason,
        /// Who to tell, when the producer knows better than the table does.
        ///
        /// The funnel normally addresses the cancel to whoever holds the
        /// capture. A producer that has *already* taken the capture back as
        /// part of its own teardown — the OS-drag escalation hands the pointer
        /// to the platform before it raises the cancel — would leave the
        /// funnel with nobody to tell, so it names the widget itself.
        recipient: Option<WidgetId>,
    },
}

/// The window-logical position a pointer event happened at, if it carries one.
///
/// The pointer table is fed from here: every event with a position refreshes
/// its pointer's entry, and everything else (keys, IME, focus, AT actions)
/// leaves the table alone.
fn pointer_event_position(event: &WidgetEvent) -> Option<Point> {
    match event {
        WidgetEvent::PointerDown { position, .. }
        | WidgetEvent::PointerUp { position, .. }
        | WidgetEvent::PointerMove { position, .. } => Some(*position),
        // Deliberately not `PointerCancel`. A revocation must never *create*
        // a pointer: admitting one here would resurrect a contact the funnel
        // is in the middle of forgetting, and leave its entry behind for good.
        _ => None,
    }
}

/// What the bubble pass is allowed to run on one node.
///
/// Two independent gates, kept together because they answer the same
/// question — "how much of this node takes part in *this* dispatch".
#[derive(Copy, Clone)]
struct BubbleGates {
    /// Gates the pre-gesture `on_pointer_event` intercept. `true` for the
    /// bubble target (the widget the event was dispatched at) and `false`
    /// for every ancestor, because ancestors already fired their
    /// `on_pointer_event` during the preview pass — firing it again in
    /// bubble was the source of double-toggle / double-select bugs when a
    /// wrapper widget (e.g. `ListItemWrapper`) held the handler and a child
    /// leaf was the hit target.
    fire_on_pointer_event: bool,
    /// This node lost the pointer's arbitration, so its **recognizers** stay
    /// out of the event and it bubbles on as if it carried none. Its own
    /// handlers still run: losing an arbitration is not the same as being
    /// removed from the tree. See `WidgetTree::sequence_blocks_arena`.
    arena_blocked: bool,
}

impl WidgetTree {
    /// Hops from `focus` up to `scope_id` (0 when equal), or `None` when
    /// `scope_id` is not an ancestor-or-self of `focus`. Fewer hops means
    /// the scope sits closer to focus — i.e. a more specific binding.
    fn scope_distance_from_focus(&self, focus: WidgetId, scope_id: WidgetId) -> Option<usize> {
        let mut hops = 0usize;
        let mut current = Some(focus);
        while let Some(c) = current {
            if c == scope_id {
                return Some(hops);
            }
            current = self.arena.parent(c);
            hops += 1;
        }
        None
    }

    /// From every same-chord shortcut candidate, choose the one whose
    /// scope applies to the current focus, preferring the most specific
    /// scope: a `Scoped` binding whose subtree contains focus beats a
    /// `Global` one, and among nested applicable scopes the one closest
    /// to focus (fewest hops) wins. Equal-specificity ties keep the
    /// deterministic `(category, id)` order `candidates` arrives in (the
    /// first such candidate wins). Returns `None` when no candidate
    /// applies — every match is a scoped binding outside the focused
    /// subtree — so the caller falls through to normal KeyDown dispatch.
    fn select_shortcut_for_focus(
        &self,
        candidates: &[(&'static str, crate::shortcut::ShortcutScope, bool)],
    ) -> Option<(&'static str, crate::shortcut::ShortcutScope, bool)> {
        use crate::shortcut::ShortcutScope;
        let mut best: Option<(usize, (&'static str, ShortcutScope, bool))> = None;
        for &(id, scope, propagate) in candidates {
            // Specificity score, higher = more specific. Global is the
            // least-specific fallback (0); any applicable scoped binding
            // outranks it (`usize::MAX - hops`, so fewer hops = deeper
            // scope = higher score). Tree depth is tiny, so no overflow.
            let score = match scope {
                ShortcutScope::Global => Some(0usize),
                ShortcutScope::Scoped(scope_id) => self
                    .focused
                    .and_then(|f| self.scope_distance_from_focus(f, scope_id))
                    .map(|hops| usize::MAX - hops),
            };
            let Some(score) = score else { continue };
            // Strictly-greater keeps the first candidate on a tie, so the
            // existing `(category, id)` precedence holds within a scope.
            if best.as_ref().is_none_or(|(b, _)| score > *b) {
                best = Some((score, (id, scope, propagate)));
            }
        }
        best.map(|(_, c)| c)
    }

    /// Dispatch an event into the widget tree.
    ///
    /// Routing rules:
    /// - Pointer events -> hit testing against layout tree
    /// - Keyboard/IME events -> focused widget
    /// - AccessKit actions -> target widget directly
    /// - Scroll events -> hit testing (scroll target under pointer)
    ///
    /// Dispatch an event with the caller-supplied app-level
    /// [`WindowOps`](crate::window::WindowOps) sink. `teksilo-app` calls
    /// this variant; handlers can reach the multi-window API
    /// synchronously (`open_window` creates the winit window inside
    /// the same call before returning).
    pub fn dispatch_event_with_ops(
        &mut self,
        event: WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // A legacy event names no pointer, so it is the mouse at the epoch —
        // which is exactly what it has always meant.
        let mut snapshot = crate::pointer::InputSnapshot::from_event(&event);
        // A legacy `WidgetEvent` carries no timestamp of its own, so stamp it
        // from the tree clock. Without this the gesture layer would see every
        // hand-built event at the epoch and no interval — a double tap, a long
        // press and a swipe would all be undecidable.
        if snapshot.pointer.time == crate::pointer::EventTime::ZERO {
            snapshot.pointer.time = self.input_now();
        }
        self.dispatch_with_input_snapshot(event, snapshot, ops)
    }

    /// Dispatch an event on a standalone tree (tests, headless
    /// scenarios). Handler code that calls `ctx.open_window(...)`
    /// from within this dispatch will panic — by design. See
    /// [`dispatch_event_with_ops`](Self::dispatch_event_with_ops)
    /// for the app-facing variant.
    pub fn dispatch_event(&mut self, event: WidgetEvent) {
        let mut noop = crate::window::NoopWindowOps;
        self.dispatch_event_with_ops(event, &mut noop);
    }

    // The three ingress doors

    /// Deliver one pointer sample.
    ///
    /// This and [`dispatch_scroll`](Self::dispatch_scroll) are the real input
    /// doors: a backend produces [`PointerSample`](crate::pointer::PointerSample)s
    /// and [`ScrollSample`](crate::pointer::ScrollSample)s, and everything
    /// Teksilo knows about *who* is pointing — identity, kind, pressure,
    /// timestamp, coalesced history — reaches the tree through them.
    ///
    /// For now a sample is **lowered** onto the legacy `WidgetEvent` it
    /// describes and takes the existing route, so a mouse behaves bit for bit
    /// as it did before the doors existed. What changes here is only that the
    /// door exists and that the sample's
    /// [`PointerInfo`](crate::pointer::PointerInfo) is visible to handlers
    /// through [`EventContext::pointer`](crate::widget::EventContext::pointer).
    pub fn dispatch_pointer(&mut self, sample: crate::pointer::PointerSample) {
        let mut noop = crate::window::NoopWindowOps;
        self.dispatch_pointer_with_ops(sample, &mut noop);
    }

    /// [`dispatch_pointer`](Self::dispatch_pointer) with the caller's
    /// app-level [`WindowOps`](crate::window::WindowOps) sink, so handlers can
    /// reach the multi-window API synchronously.
    pub fn dispatch_pointer_with_ops(
        &mut self,
        sample: crate::pointer::PointerSample,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        use crate::pointer::PointerPhase;

        crate::trace_input!(
            Samples,
            "{:?} {:?} at {:?} buttons={:?} t={:?}",
            sample.phase,
            sample.pointer.id,
            sample.position,
            sample.pointer.buttons,
            sample.pointer.time
        );

        // The button a Down/Up is *about*. A direct-pointer contact reports no
        // button at all, and the widget layer has always been told
        // `Primary` for a press — that is what a tap is.
        let button = sample
            .button
            .unwrap_or(crate::event::PointerButton::Primary);
        let event = match sample.phase {
            PointerPhase::Down => WidgetEvent::PointerDown {
                position: sample.position,
                button,
                modifiers: sample.modifiers,
                pointer: sample.pointer,
            },
            PointerPhase::Move => WidgetEvent::PointerMove {
                position: sample.position,
                modifiers: sample.modifiers,
                pointer: sample.pointer,
            },
            PointerPhase::Up => WidgetEvent::PointerUp {
                position: sample.position,
                button,
                modifiers: sample.modifiers,
                pointer: sample.pointer,
            },
            // The platform revoked the contact (a `wl_touch.cancel`, a
            // `WM_POINTERCAPTURECHANGED`, a compositor grab). It takes the
            // cancel funnel directly rather than being lowered onto an event:
            // the funnel owns the teardown order, and lowering would first
            // *admit* the pointer the sample is revoking.
            PointerPhase::Cancel => {
                // The dismissal this contact had armed goes with it. The funnel
                // below returns early for a pointer with nothing revocable, and
                // a suppressed arming `Down` leaves exactly that shape — no
                // sequence, no capture — so the abort cannot ride on it.
                self.overlay_manager.abort_dismiss(sample.pointer.id);
                self.cancel_pointer(
                    sample.pointer.id,
                    crate::pointer::CancelReason::Platform,
                    ops,
                );
                // A revoked contact ceases to exist, exactly as a lifted one
                // does — the `ends_pointer` rule below, which this arm returns
                // before reaching. The funnel ends the entry itself *when it
                // runs*; for a pointer that had no press to revoke it returns
                // first, and the entry would outlive the finger. Idempotent, so
                // the ordinary path is unaffected.
                if !sample.pointer.kind.hovers() {
                    self.pointers.end(sample.pointer.id);
                }
                return;
            }
        };

        // Admit the pointer before anything is dispatched. A palm, or an
        // eleventh simultaneous contact, is refused here and produces no event
        // at all — the alternative (evicting a live contact to make room) turns
        // a pinch into a fling, and letting a resting palm through turns a hand
        // on a tablet into a stream of taps.
        if !self.pointers.would_admit(&sample.pointer) {
            return;
        }
        // A contact ceases to exist when it lifts; a hovering-capable pointer
        // does not — a mouse that releases a button is still there, still
        // hovering, and its entry is what every legacy singular accessor reads.
        let ends_pointer = matches!(sample.phase, PointerPhase::Up | PointerPhase::Cancel)
            && !sample.pointer.kind.hovers();
        let pointer_id = sample.pointer.id;

        let snapshot = crate::pointer::InputSnapshot::from_pointer_sample(&sample);
        self.dispatch_with_input_snapshot(event, snapshot, ops);

        if ends_pointer {
            self.pointers.end(pointer_id);
        }
    }

    /// Deliver one scroll sample.
    ///
    /// Routing follows [`ScrollSample::position`](crate::pointer::ScrollSample::position):
    /// `Some` hit-tests it, `None` falls back to the hovered (else focused)
    /// widget, which is what every scroll did before. A mouse wheel carries no
    /// position, so this is a no-op for a mouse; a pan synthesised from a
    /// direct pointer must carry one, because a contact never writes hover.
    pub fn dispatch_scroll(&mut self, sample: crate::pointer::ScrollSample) {
        let mut noop = crate::window::NoopWindowOps;
        self.dispatch_scroll_with_ops(sample, &mut noop);
    }

    /// [`dispatch_scroll`](Self::dispatch_scroll) with the caller's app-level
    /// [`WindowOps`](crate::window::WindowOps) sink.
    pub fn dispatch_scroll_with_ops(
        &mut self,
        sample: crate::pointer::ScrollSample,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        crate::trace_input!(
            Samples,
            "scroll {:?} {:?}/{:?} at {:?}",
            sample.delta,
            sample.phase,
            sample.source,
            sample.position
        );

        let event = WidgetEvent::Scroll {
            delta: sample.delta,
            modifiers: sample.modifiers,
            window_position: sample.position,
            phase: sample.phase,
            pointer: sample.pointer,
        };
        let snapshot = crate::pointer::InputSnapshot::from_scroll_sample(&sample);
        self.dispatch_with_input_snapshot(event, snapshot, ops);
    }

    /// The pointer left the window.
    ///
    /// The third ingress door, and the only one that carries no sample: the OS
    /// says the cursor crossed the window boundary and nothing else. It exists
    /// because hover is otherwise cleared *only* by a move that lands
    /// elsewhere — so a mouse that leaves through an edge would leave the last
    /// widget hovered for as long as it stays away, with its hover chrome
    /// painted, its `hover_within` signal true and its tooltip still counting
    /// down.
    ///
    /// Clears hover the way a move to an empty spot does: a `PointerLeave` to
    /// the hovered widget, the tooltip dwell cancelled, the `hover_within`
    /// chain updated. It touches nothing else — no pointer is cancelled, no
    /// capture released, no table entry ended. A mouse that leaves the window
    /// is still a mouse, and a *captured* pointer is deliberately exempt: a
    /// drag whose pointer wanders off the window keeps its target, which is
    /// what makes a drag past the edge (and the OS-drag escalation built on
    /// it) work at all.
    ///
    /// There is no matching `pointer_entered_window`, and that is not an
    /// omission: the enter carries no position either, and a position only
    /// ever arrives with a `CursorMoved` — which re-arms hover through the
    /// ordinary path. A door that could only say "somewhere" would have
    /// nothing to hit-test.
    pub fn pointer_left_window(&mut self, ops: &mut dyn crate::window::WindowOps) {
        // The pointer that owns hover is the one that just left; if it holds a
        // capture, the interaction it is in the middle of outranks the
        // boundary crossing.
        if self
            .pointers
            .hover_owner_id()
            .and_then(|id| self.captured_by(id))
            .is_some()
        {
            return;
        }
        let Some(hovered) = self.hovered_id() else {
            return;
        };
        // The hover owner is the pointer that just left — read from the table
        // because this door carries no sample of its own.
        let leave = WidgetEvent::PointerLeave {
            pointer: self.hover_transition_pointer(),
        };
        self.dispatch_to_widget(hovered, &leave, &mut *ops);
        self.tooltip_pointer_leave(hovered, &mut *ops);
        self.set_hovered(None);
        self.update_hover_within_signals(Some(hovered), None);
    }

    /// The pointer a hover transition is credited to.
    ///
    /// [`PointerEnter`](WidgetEvent::PointerEnter) /
    /// [`PointerLeave`](WidgetEvent::PointerLeave) are hover-owner events by
    /// construction — a contact never writes hover — so the answer is the hover
    /// owner's own [`PointerInfo`](crate::pointer::PointerInfo), read from the
    /// table rather than from the in-flight sample: a transition can be raised
    /// by something that is not a pointer sample at all (a relayout that moves
    /// a widget out from under the cursor, the window-leave door), and a
    /// hover-incapable pointer must never be credited with hover.
    ///
    /// Falls back to a mouse at the current tree time when the table has no
    /// hover owner, which is the state a synthesized `dispatch_event` leaves it
    /// in. Either way this is the tree's own view rather than a producer's — see
    /// the rule stated on
    /// [`handle_pointer_move`](Self::handle_pointer_move).
    pub(super) fn hover_transition_pointer(&self) -> crate::pointer::PointerInfo {
        self.pointers
            .hover_owner()
            .map(|entry| entry.info)
            .unwrap_or_else(|| crate::pointer::PointerInfo::mouse(self.input_now()))
    }

    /// Run one dispatch with `snapshot` installed as the tree's view of the
    /// in-flight sample, restoring the previous value afterwards.
    ///
    /// Save-and-restore rather than reset-to-default so a nested dispatch (a
    /// synthetic click queued by a handler, a scroll-into-view walk) leaves the
    /// outer sample's snapshot intact for the rest of the outer dispatch.
    fn dispatch_with_input_snapshot(
        &mut self,
        event: WidgetEvent,
        snapshot: crate::pointer::InputSnapshot,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // A dispatch reached from inside a dispatch — a handler's synthetic
        // click, an assistive-technology action re-entering the door — is
        // **queued**, not run inline. Running it inline would let it unwind the
        // pointer state the outer sample is still standing on: the outer
        // handler would return to a tree whose hover, capture and table entries
        // had all moved under it. Queued, the outer dispatch finishes on the
        // state it started with and the nested one replays immediately
        // afterwards, so from a caller's side nothing changed — the queue is
        // empty again before the top-level call returns.
        self.pending_dispatch.push_back(QueuedDispatch::Event {
            event,
            snapshot: Box::new(snapshot),
        });
        if self.dispatch_depth > 0 {
            return;
        }
        self.drain_pending_dispatch(&mut *ops);
    }

    /// Run everything the queue holds, in order, each at depth zero.
    ///
    /// `pop_front` in a loop rather than `drain`: a replayed dispatch — or a
    /// cancel's own `PointerCancel` handler — may queue another entry, and
    /// each must in turn run at depth zero.
    pub(super) fn drain_pending_dispatch(&mut self, ops: &mut dyn crate::window::WindowOps) {
        while let Some(queued) = self.pending_dispatch.pop_front() {
            match queued {
                QueuedDispatch::Event { event, snapshot } => {
                    self.run_one_dispatch(event, *snapshot, &mut *ops);
                }
                QueuedDispatch::Cancel {
                    pointer,
                    reason,
                    recipient,
                } => {
                    self.run_one_cancel(pointer, reason, recipient, &mut *ops);
                }
            }
        }
    }

    /// One dispatch at depth zero, with `snapshot` installed as the tree's view
    /// of the in-flight sample and the previous value restored afterwards.
    ///
    /// Save-and-restore rather than reset-to-default: the restore is what stops
    /// one queued dispatch's snapshot standing as the tree's view of the world
    /// once it has returned, while the drain replays the next entry.
    fn run_one_dispatch(
        &mut self,
        event: WidgetEvent,
        snapshot: crate::pointer::InputSnapshot,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let previous = std::mem::replace(&mut self.current_input, snapshot);
        self.dispatch_depth += 1;
        self.dispatch_event_impl(event, ops);
        self.dispatch_depth -= 1;
        self.current_input = previous;
    }

    fn dispatch_event_impl(&mut self, event: WidgetEvent, ops: &mut dyn crate::window::WindowOps) {
        // Admit the pointer this event belongs to into the table before
        // anything routes. A legacy `WidgetEvent` names no pointer, so
        // `current_input` reports the mouse and this creates (or refreshes) the
        // one mouse entry — which is what every singular accessor then reads,
        // so a mouse-only tree behaves exactly as it did before the table.
        if let Some(position) = pointer_event_position(&event) {
            let is_down = matches!(event, WidgetEvent::PointerDown { .. });
            let is_move = matches!(event, WidgetEvent::PointerMove { .. });
            if !self.admit_current_pointer(position, is_down, is_move) {
                return;
            }
            // A hovering-capable pointer takes the hover-owner role by pointing:
            // the later sample wins, and whoever held it is sent a leave. A
            // contact is refused the role outright — it has no hover to give.
            if self.current_input.pointer.kind.hovers() {
                self.claim_hover_owner_for_current(&mut *ops);
            }
        }

        // Track input modality for `:focus-visible`: keyboard input reveals
        // focus rings, pointer input hides them. Updated at the dispatch root so
        // every handler (and the next paint) observes the current modality.
        match &event {
            WidgetEvent::KeyDown { .. } if !self.focus_visible.get() => {
                self.focus_visible.set(true);
            }
            WidgetEvent::PointerDown { .. } if self.focus_visible.get() => {
                self.focus_visible.set(false);
            }
            _ => {}
        }

        // The "back toward the parent overlay" key closes the top nested
        // overlay (e.g. an open submenu over its parent menu). It is the
        // inline-start arrow: ArrowLeft under LTR, ArrowRight under RTL.
        // Without the RTL flip, ArrowLeft would navigate *into* a submenu
        // in RTL menus yet still dismiss it here.
        let overlay_back_key = match self.layout_direction {
            crate::environment::LayoutDirection::RightToLeft => Key::ArrowRight,
            crate::environment::LayoutDirection::LeftToRight => Key::ArrowLeft,
        };
        if let WidgetEvent::KeyDown { key, .. } = &event
            && *key == overlay_back_key
        {
            // Count menu-level (non-host) overlays. A revealed collapsible
            // `MenuBar` is itself a *host* overlay (Role::MenuBar), so a
            // single open top-level menu sitting over it must NOT be treated
            // as a nested submenu — otherwise the back key would close the
            // menu instead of letting the menubar navigate to the previous
            // one. Only when ≥2 non-host overlays are stacked (a submenu over
            // its parent menu) does the back key dismiss the top overlay.
            //
            // **Menus only**, which is what the band says. Every mounted text
            // editor keeps one full-viewport affordance host alive in the
            // [`TextAffordance`](crate::overlay::OverlayBand::TextAffordance)
            // band for its selection handles, so counting bands alike made two
            // editors on one page read as a menu cascade: the back key then
            // tore down an affordance host and returned, and ArrowLeft stopped
            // reaching *any* editor in that window for as long as a second one
            // was mounted. A text affordance is not a cascade level, the same
            // reason `OverlayBand::dismissed_by_outside_press` already excludes
            // it from press dismissal.
            //
            // `dismiss_top` below stays correct because the stack is
            // band-ordered (`OverlayManager::show_with_auto_dismiss` inserts,
            // it does not push): a `Standard` overlay always sits above every
            // `TextAffordance` one, so whenever this count exceeds one the top
            // of the stack is the menu this key means.
            let nested_menu_overlays = {
                let ids: Vec<_> = self
                    .overlay_manager
                    .stack
                    .iter()
                    .filter(|o| o.band == crate::overlay::OverlayBand::Standard)
                    .map(|o| o.id)
                    .collect();
                ids.into_iter()
                    .filter(|&id| !self.overlay_is_host_surface(id))
                    .count()
            };
            // The back key only navigates *menu* cascades; it must never close a
            // dialog / alert / modal that happens to sit on top. Each modal is a
            // scrim+panel overlay pair and the (non-host) scrims inflate the count
            // above, so also require the *topmost* overlay to be back-navigable —
            // i.e. a non-host (menu) surface — before dismissing it.
            let top_id = self.overlay_manager.stack.last().map(|o| o.id);
            let top_is_back_navigable = top_id.is_some_and(|id| !self.overlay_is_host_surface(id));
            if nested_menu_overlays > 1 && top_is_back_navigable {
                if let Some((_id, content_ids, focus_restore)) = self
                    .overlay_manager
                    .dismiss_top_because(crate::overlay::DismissReason::Escape)
                {
                    self.dormant_dismissed_content(&content_ids, &mut *ops);
                    if let Some(restore_id) = focus_restore
                        && self.arena.is_active(restore_id)
                    {
                        self.focus_ops(restore_id, &mut *ops);
                    }
                }
                return;
            }
        }

        // Escape retires any shown tooltip first, and does **not** stop there —
        // see `tooltip_escape_pressed`. Ordered before the stack walk below so
        // that walk can no longer pick a tooltip as the thing to dismiss, which
        // is what used to spend the key on a tip nobody was reading while the
        // editor / menu / dialog the user meant stayed open.
        if let WidgetEvent::KeyDown {
            key: Key::Escape, ..
        } = &event
        {
            self.tooltip_escape_pressed();
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

        // Outside-press overlay dismissal. Two shapes, chosen by the pointer:
        // an indirect one dismisses on the press and falls through, exactly as
        // it always has; a direct one *arms* on the press and commits on the
        // release. See `arm_outside_press_dismissal`.
        match &event {
            WidgetEvent::PointerDown {
                position, button, ..
            } => {
                if self.arm_outside_press_dismissal(*position, *button, &mut *ops) {
                    return;
                }
            }
            WidgetEvent::PointerUp { position, .. } => {
                if self.commit_outside_press_dismissal(*position, &mut *ops) {
                    return;
                }
            }
            WidgetEvent::PointerCancel { pointer, .. } => {
                // A revoked press dismisses nothing. The arming Down was never
                // delivered beneath either, so the whole gesture leaves no
                // trace — which is the point of deferring to the release.
                self.overlay_manager.abort_dismiss(pointer.id);
            }
            _ => {}
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

        // Keyboard-capture surfaces (a terminal, a game viewport) opt out
        // of shortcut resolution entirely while focused: they want every
        // keystroke delivered raw so a host-app `Ctrl+C` shortcut can't
        // steal the SIGINT the child process needs. The Escape / overlay
        // back-navigation handled above still runs first, so an open
        // overlay is still dismissable. Only a KeyDown is affected; KeyUp
        // and IME already bypass the shortcut path.
        let focus_captures_keys = matches!(&event, WidgetEvent::KeyDown { .. })
            && self.focused.is_some_and(|f| self.is_keyboard_capture(f));

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
        if !focus_captures_keys && let WidgetEvent::KeyDown { key, modifiers, .. } = &event {
            let keystroke = crate::shortcut::KeyStroke::new(*key, *modifiers);
            // Gather every same-chord candidate (owned fields) before any
            // mutable borrow of the registry, then pick the one whose scope
            // actually applies to the current focus. `find_by_keystroke`
            // alone yields only the first by `(category, id)` order, which
            // can be a `Scoped` binding outside focus shadowing an
            // applicable `Global` one — or a `Global` binding that should
            // yield to an in-focus `Scoped` one. Selection needs focus +
            // the tree, so it happens here, not in the registry.
            let candidates: Vec<(&'static str, crate::shortcut::ShortcutScope, bool)> = self
                .shortcut_registry
                .matches_by_keystroke(keystroke)
                .map(|eff| {
                    (
                        eff.shortcut.id,
                        eff.shortcut.scope,
                        eff.shortcut.propagate_when_disabled,
                    )
                })
                .collect();
            let lookup = self.select_shortcut_for_focus(&candidates);
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
                        // Tag shortcut origin so analytics can
                        // distinguish keyboard-driven activations from
                        // button / menu / programmatic ones.
                        let intent = intent.with_source(crate::telemetry::IntentSource::Shortcut);
                        self.enqueue_intent(anchor_id, intent, propagate_when_disabled);
                        self.drain_pending_intents(&mut *ops);
                        return;
                    }
                }
                // Chosen candidate had no anchor after all (e.g. a Global
                // match while nothing is focused and the tree has no
                // roots) — fall through to normal KeyDown dispatch.
                // `on_activate` was never called, so nothing to clean up.
            }
            // `lookup` is `None` when every same-chord candidate was a
            // scoped binding outside the focused subtree — fall through.
        }

        // Escape during an OS drag we escalated. There is no `active_drag`
        // any more — `try_escalate_to_os_drag` took it when the platform
        // accepted the hand-off — so this cannot live in the block below, but
        // it is the same user gesture and belongs on the same path rather than
        // being special-cased in the event loop of whichever backend needs it.
        if self.outbound_drag_source.is_some()
            && let WidgetEvent::KeyDown {
                key: Key::Escape, ..
            } = &event
        {
            ops.cancel_os_drag();
            // Deliberately no `return`: the backend answers asynchronously with
            // a terminal `DragEnded`, which is what actually tears the session
            // down via `handle_os_drag_ended`. Swallowing the key here would
            // also stop Escape from closing whatever else is open.
        }

        // --- Active drag session handling ---
        if self.active_drag.is_some() {
            match &event {
                WidgetEvent::PointerMove { position, .. } => {
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

        // A keyboard route to the context menu, reserved at the dispatcher so
        // every widget with a `.context_menu(..)` gets one without opting in.
        //
        // It has to be here rather than in a widget, and it cannot be a
        // `Shortcut`: shortcut resolution runs above this point, so a global
        // binding would fire while the user was typing in a modal. Sitting
        // below it means an application that deliberately binds Shift+F10 to
        // something else still wins.
        if let WidgetEvent::KeyDown { key, modifiers, .. } = &event
            && is_context_menu_chord(*key, *modifiers)
            && self.open_context_menu_from_keyboard(&mut *ops)
        {
            return;
        }

        match &event {
            WidgetEvent::PointerMove { position, .. } => {
                // Every sample: re-check the sequence's members against the
                // arena, and record where the pointer now is so each threshold
                // reads one number.
                self.note_sequence_position(*position);
                self.revalidate_sequence(&mut *ops);
                // Timers before positional thresholds, and before the move
                // reaches any recognizer — see `tick_sequence_timers`.
                self.tick_sequence_timers();
                // The multi-contact and palm layers see every sample, decided
                // or not: a pinch is arbitrated by contact count rather than by
                // the press arbitration, and the palm watch has to know whether
                // this contact ever moved.
                self.note_palm_sample(*position);
                self.feed_pinch(super::pan_arbiter::PinchFeed::Move, *position, &mut *ops);
                if let Some(captured) = self.current_pointer_capture() {
                    self.dispatch_to_widget(captured, &event, &mut *ops);
                    // Advance the arbitration so an ancestor drag can still
                    // begin while a descendant tap holds the capture. Once a
                    // drag latches, `active_drag` takes over and the capture
                    // branch above is bypassed.
                    if self.active_drag.is_none() {
                        self.advance_sequence(&event, &mut *ops);
                    }
                } else {
                    self.handle_pointer_move(&event, *position, &mut *ops);
                    // No capture: for a mouse there is nothing enrolled (a
                    // gesture member is only enrolled *through* a capture), so
                    // this is a no-op. A contact panning from empty space has
                    // its pan claimants here.
                    if self.active_drag.is_none() {
                        self.advance_sequence(&event, &mut *ops);
                    }
                }
                // After the arbitration, so a claim taken on *this* sample
                // already delivers its own movement rather than waiting a frame.
                self.advance_pan(*position, &mut *ops);
                // …and after that, so the press visual answers to a claim taken
                // on this very sample rather than surviving it by one move.
                self.update_press(*position);
                // A tree-owned hold survives only while the contact holds
                // still; past the tap boundary this is a pan or a drag.
                self.touch_route_moved(self.current_pointer_id());
                // Hover-owner-only, exactly as `handle_pointer_move` is. The
                // `DismissBehavior::PointerLeave` grace is a *hover* dismissal:
                // it asks "has the pointer left this overlay and its trigger",
                // a question only a pointer that hovers is entitled to answer.
                // Ungated, any contact's move anywhere started the 150 ms grace
                // on every such overlay — so a submenu a finger had just tapped
                // open was closed by the frame pass with no further input, and
                // no mouse test could see it because a mouse *is* the hover
                // owner.
                if self.pointers.hover_owner_id() == Some(self.current_pointer_id()) {
                    self.update_pointer_leave_overlays(*position, &mut *ops);
                }
            }
            WidgetEvent::PointerDown {
                position, button, ..
            } => {
                // A new press is a new interaction: whatever took the previous
                // one away has nothing to say about this one.
                let pressed = self.current_pointer_id();
                self.cancelled_pointers.retain(|p| *p != pressed);
                // The user has acted — a tooltip that has not yet appeared is
                // now answering a question nobody is asking any more, and one
                // already up is covering the thing being clicked. Cancel the
                // pending dwell and retire any shown non-sticky tip, the way
                // Windows and GTK both do. Runs before hit-testing so it fires
                // even for a press that lands on nothing.
                self.tooltip_pointer_press(Some(*position));
                // Routed for the pointer that is actually pressing: a finger
                // gets its grip outsets and its miss-only slop, a mouse gets the
                // exact test it has always had.
                let pressing = self.current_input.pointer;
                if let Some(target) = self.hit_test_for(*position, &pressing) {
                    if *button == PointerButton::Secondary
                        && self.show_context_menu_for(target, *position, &mut *ops)
                    {
                        return;
                    }
                    // Open the arbitration BEFORE any handler runs: the frozen
                    // `TouchAction` has to be readable from `ctx.touch_action()`
                    // inside the press handler, and an explicit
                    // `capture_pointer()` made there needs a sequence to enrol
                    // into.
                    self.begin_sequence(target, *position);
                    // Straight after the sequence, so the frozen `TouchAction`
                    // and the enrolled pan members are already in hand — and so
                    // a press on a coasting list catches it before anything
                    // else runs.
                    let modifiers = match &event {
                        WidgetEvent::PointerDown { modifiers, .. } => *modifiers,
                        _ => crate::event::Modifiers::NONE,
                    };
                    self.begin_pan(target, *position, modifiers);
                    self.begin_palm_watch(*position);
                    self.feed_pinch(super::pan_arbiter::PinchFeed::Down, *position, &mut *ops);
                    // Open the press record before the dispatch, so a handler
                    // asking `ctx.press_pending()` on its own `PointerDown`
                    // gets the answer the router already knows.
                    let focusable = self.find_focusable_at_or_above(target);
                    self.begin_press(focusable);
                    // An indirect pointer focuses on press, as it always has.
                    // A direct one waits for the release: a finger that lands
                    // on a control and slides away has not chosen it, and
                    // moving focus at touch-down would leave the ring — and
                    // the caret — on a control the user never activated. See
                    // `focus_on_release`.
                    if !pressing.kind.is_direct()
                        && let Some(focusable) = focusable
                    {
                        self.focus_with_origin_ops(
                            focusable,
                            crate::focus::FocusOrigin::Pointer(pressing.kind),
                            &mut *ops,
                        );
                    }
                    self.dispatch_to_widget(target, &event, &mut *ops);
                    // Enrol the competitors that only become knowable once the
                    // press has been dispatched: the drag-capable ancestors of
                    // whoever took the capture (tap-vs-drag across the hit
                    // path).
                    if self.active_drag.is_none() {
                        self.enrol_sequence_members(&event, &mut *ops);
                    }
                    // The arena has now claimed the press, so the node whose
                    // visual this record drives is known — and whether the
                    // button that opened it is one that node can act on.
                    self.adopt_press_owner(*button);
                    // Last: the tree-owned hold. It has to see the enrolment
                    // (a deferred grab spends the hold) and it has to see the
                    // handlers a press-time build may have installed, so it is
                    // resolved after both. See `super::touch_route`.
                    self.arm_touch_route(target, *position);
                }
            }
            WidgetEvent::PointerUp { position, .. } => {
                // A `PointerCancel` is terminal. If this pointer's press was
                // revoked, the `Up` that follows completes nothing — the widget
                // has already been told to let go, and delivering the release
                // would hand it back an interaction the system took away. Drop
                // it, and forget the cancel: the pointer is free again.
                let released = self.current_pointer_id();
                // The hold is over whatever else this release does, and it is
                // cleared before any of it so a handler that runs below cannot
                // see a route that will never fire.
                self.cancel_touch_route(released);
                if let Some(index) = self.cancelled_pointers.iter().position(|p| *p == released) {
                    self.cancelled_pointers.swap_remove(index);
                    crate::trace_input!(
                        Samples,
                        "swallowing the Up for {released:?}: its press was cancelled"
                    );
                    return;
                }
                // The palm verdict, before anything else acts on the release:
                // a contact the heuristic rejects must fire no tap at all, and
                // the only way to guarantee that is to take the cancel funnel
                // instead of the release path. Judged on the `Up` and never
                // earlier — a contact is not revoked while the user might still
                // be doing something with it.
                self.feed_pinch(super::pan_arbiter::PinchFeed::Up, *position, &mut *ops);
                if self.take_palm_verdict(released) {
                    crate::trace_input!(
                        Samples,
                        "{released:?} released as a palm: large, and it never moved"
                    );
                    self.cancel_pointer(
                        released,
                        crate::pointer::CancelReason::PalmRejected,
                        &mut *ops,
                    );
                    return;
                }
                // A pan hands its release velocity to the fling driver here,
                // and closes its session either way.
                self.end_pan(*position, &mut *ops);
                // The pointer sequence ends here — the release sweep feeds the
                // `Up` to every member still following the press so its
                // recognizer clears the press origin it recorded. Without this,
                // a press that an interactive descendant captured (a card's
                // editor, a row's button) leaves the ancestor's DragRecognizer
                // armed, and the next hover move starts a phantom drag.
                self.note_sequence_position(*position);
                self.end_sequence(&event, &mut *ops);
                // A direct pointer's focus lands here, before the release is
                // dispatched, so a handler activating on the `Up` runs with the
                // focus its own press earned.
                self.focus_on_release(*position, &mut *ops);
                if let Some(captured) = self.current_pointer_capture() {
                    self.dispatch_to_widget(captured, &event, &mut *ops);
                    // Per pointer: this Up releases *this* pointer's capture and
                    // leaves every other contact's alone.
                    self.set_current_pointer_capture(None);
                } else {
                    let releasing = self.current_input.pointer;
                    if let Some(target) = self.hit_test_for(*position, &releasing) {
                        self.dispatch_to_widget(target, &event, &mut *ops);
                    }
                }
                // The press is over. Any arena still following this contact saw
                // its `Down` but not its `Up` — see `release_arenas_following`.
                let released = self.current_pointer_id();
                self.release_arenas_following(released);
                // The visual goes with it. After the dispatch, so a release
                // handler reading `ctx.is_pressed()` still sees the press it is
                // completing.
                self.end_press(released);
            }
            WidgetEvent::PointerCancel {
                reason, pointer, ..
            } => {
                // A hand-built `PointerCancel` — a caller reaching the legacy
                // door with one, a test — means the same thing a producer does,
                // so it takes the same funnel rather than a second teardown of
                // its own. Queued behind this dispatch, like every cancel.
                let (reason, pointer) = (*reason, pointer.id);
                self.cancel_pointer(pointer, reason, &mut *ops);
            }
            WidgetEvent::Scroll {
                window_position: position,
                ..
            } => {
                use super::pan_arbiter::ScrollDelivery;
                match ScrollDelivery::for_source(self.current_input.scroll_source) {
                    // A synthesised pan (and the coast that follows it) walks
                    // the pan claimants and nothing else — see
                    // `widget_tree::pan_arbiter` for why the generic bubble
                    // would be wrong here.
                    ScrollDelivery::ClaimantChain => {
                        let position = *position;
                        self.route_scroll_along_chain(&event, position, &mut *ops);
                    }
                    // A positioned scroll routes by hit test; a positionless one
                    // keeps the historical hover-then-focus fallback. A mouse wheel
                    // is positionless, so this is a no-op for it — the change
                    // exists for a pan synthesised from a direct pointer, which
                    // never writes hover and would otherwise route nowhere.
                    ScrollDelivery::Bubble => {
                        let scrolling = self.current_input.pointer;
                        let target = match position {
                            Some(p) => self.hit_test_for(*p, &scrolling),
                            None => self.hovered_id().or(self.focused),
                        };
                        if let Some(target) = target {
                            self.dispatch_to_widget(target, &event, &mut *ops);
                        }
                    }
                }
            }
            WidgetEvent::KeyDown { key, modifiers, .. } => {
                if *key == Key::Tab {
                    // Ctrl+Tab / Ctrl+Shift+Tab always leave a keyboard-capture
                    // surface (WCAG 2.1.2). A capture node exists precisely to
                    // swallow every keystroke — a terminal encodes Tab as `\t`
                    // and Shift+Tab as CSI Z — so the ordinary "dispatch first,
                    // cycle only when unhandled" rule below can never move focus
                    // out of one. Reserving this one chord at the dispatcher, not
                    // in each capture widget, is what makes the escape a property
                    // of `keyboard_capture` itself rather than a promise every
                    // future capture-surface author has to remember to keep.
                    //
                    // Literal `ctrl()`, not `command()`: Ctrl+Tab is Ctrl+Tab on
                    // macOS too — ⌘⇥ is the application switcher and never
                    // reaches an app at all. Same reading as `TableView`'s
                    // cell-grid escape and `RichTextEditor`'s `tab_escape`.
                    let captured_focus = self
                        .focused
                        .is_some_and(|focused| self.is_keyboard_capture(focused));
                    if captured_focus && modifiers.ctrl() {
                        self.cycle_focus(modifiers.shift(), &mut *ops);
                        return;
                    }
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
                // An AT action (e.g. VoiceOver's VO+Space → `Action::Click`)
                // always names the node it targets — the element under the
                // assistive-technology cursor. It must be delivered to THAT
                // node, never to whatever happens to hold keyboard focus.
                // Falling back to `self.focused` would make VO+Space fire the
                // focused control instead of the cursored one, and would mask
                // a stale/inactive target by silently activating something
                // else. If the target is missing or no longer active, drop the
                // action rather than redirecting it.
                if let Some(id) = target.filter(|id| self.arena.is_active(*id)) {
                    if *action == accesskit::Action::Focus {
                        // Land where the keys go. A composite publishes one AT
                        // node on a root that is not itself focusable (a
                        // `ComboBox` or a `DateEdit` keeps focus on an inner
                        // leaf), and `ctx.request_focus` has always walked
                        // into the subtree for exactly that reason. The AT path
                        // must too: focusing the root parks `self.focused` on a
                        // node that takes no keystrokes, and because
                        // `on_key_preview` fires only on *strict* ancestors of
                        // the focused node, it also disarms the composite's own
                        // stepping keys. `first_focusable_descendant` returns the
                        // node itself when it is focusable, so every leaf control
                        // is unchanged.
                        //
                        // The walk is gated on the node actually offering
                        // `Action::Focus`, which is what makes the sentence
                        // above true of composites and only of them. Walking
                        // from *any* non-focusable node meant an AT `Focus` on a
                        // `Panel`, a `GroupBox`, a landmark or a label moved the
                        // keyboard onto the first control inside it — a node the
                        // assistive technology could have named itself and did
                        // not — and reported success. A node that offers no
                        // `Focus` now reports the action unhandled instead,
                        // which is the honest answer.
                        if self.advertises_focus_action(id) {
                            let target = self.first_focusable_descendant(id).unwrap_or(id);
                            // An assistive move, not a scripted one: the user is
                            // navigating, so the focus ring appears exactly as it
                            // would for a Tab. `Programmatic` — what this used to
                            // pass — declares no modality and left a screen-reader
                            // user with an invisible focus after any click.
                            self.focus_with_origin_ops(
                                target,
                                crate::focus::FocusOrigin::Accessibility,
                                &mut *ops,
                            );
                            // Focus is serviced here rather than by the widget,
                            // so "handled" means the focus actually landed.
                            self.access_action_handled = self.focused == Some(target);
                        } else {
                            self.access_action_handled = false;
                        }
                    } else if *action == accesskit::Action::ShowContextMenu {
                        // A "show context menu" AT action — a screen reader's
                        // menu key, or an automation `right_click` /
                        // `invoke_action(node, "show_context_menu")` — first
                        // offers itself to the node's own `on_access_action`
                        // handlers. If none consume it, fall through to the very
                        // same machinery a Secondary `PointerDown` drives, so a
                        // widget's `.context_menu(..)` factory opens without the
                        // caller having to synthesise a right-click. The AT
                        // action carries no point, so anchor the menu at the
                        // node's centre. Without this, the AT action was a silent
                        // no-op for every widget that wires its menu through the
                        // factory (i.e. all of them) — see `show_context_menu_for`.
                        // Handled = the widget consumed it, or the factory
                        // fallback actually opened a menu. A node with neither
                        // reports unhandled rather than a silent success.
                        self.access_action_handled =
                            if self.dispatch_to_widget_returning_handled(id, &event, &mut *ops) {
                                true
                            } else {
                                let position = self.arena.bounds(id).center();
                                self.show_context_menu_for(id, position, &mut *ops)
                            };
                    } else {
                        self.access_action_handled =
                            self.dispatch_to_widget_returning_handled(id, &event, &mut *ops);
                    }
                }
            }
            WidgetEvent::Gesture { .. } => {
                if let Some(target) = self.hovered_id().or(self.focused) {
                    self.dispatch_to_widget(target, &event, &mut *ops);
                }
            }
            WidgetEvent::ScrollIntoView { .. }
            | WidgetEvent::PointerEnter { .. }
            | WidgetEvent::PointerLeave { .. }
            | WidgetEvent::FocusGained { .. }
            | WidgetEvent::FocusLost => {}
        }
        // Any intents queued by handlers via `ctx.send_intent(...)`
        // are dispatched after the raw event has been handled but
        // before commands are flushed, so commands emitted from
        // action handlers land on the same tick.
        self.drain_pending_intents(&mut *ops);
    }

    /// Open the context menu the keyboard just asked for, and report whether
    /// one appeared.
    ///
    /// Targets the focused widget, or whatever its
    /// [`context_menu_key_target`](crate::widget::Widget::context_menu_key_target)
    /// nominates instead — for a data view, the selected row. Anchors the menu
    /// at the target's own bounds rather than at the last pointer position,
    /// which may be anywhere on screen or nowhere at all.
    ///
    /// Returns `false` when nothing on the ancestor chain owns a factory, so
    /// the key falls through to normal dispatch and a widget that wants to
    /// handle it itself still can.
    fn open_context_menu_from_keyboard(&mut self, ops: &mut dyn crate::window::WindowOps) -> bool {
        let Some(focused) = self.focused else {
            return false;
        };
        let target = self
            .arena
            .get(focused)
            .and_then(|node| node.widget.context_menu_key_target())
            .filter(|id| self.arena.is_active(*id))
            .unwrap_or(focused);

        // The menu belongs where the thing it is about is. A keyboard user has
        // no pointer position, and the stale one is worse than useless: it
        // would put the menu over an unrelated part of the window.
        let bounds = self.bounds(target);
        let anchor = Point {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height / 2.0,
        };
        self.show_context_menu_for(target, anchor, ops)
    }

    pub(super) fn show_context_menu_for(
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

        // Clear stale transient overlays (other menus / popovers) before mounting
        // the new menu, but KEEP any overlay that *contains* the right-clicked
        // widget — otherwise a right-click inside a modal editor would tear down
        // the modal it lives in (dismiss_all did exactly that). The context menu
        // then mounts on top of its host overlay.
        let keep: std::collections::HashSet<WidgetId> = self
            .overlay_manager
            .stack
            .iter()
            .map(|o| o.content_id)
            .filter(|&content_id| self.is_descendant_of(owner_id, content_id))
            .collect();
        let dismissed = self.overlay_manager.dismiss_except(&keep);
        self.dormant_dismissed_content(&dismissed, &mut *ops);

        let content_id = self.add_boxed(menu_widget);
        let prev_focus = self.focused;
        // One branch, every menu: a coarse pointer gets a placement that keeps
        // clear of its own contact patch, everything else the historical
        // `AtPointer`. A point-anchored panel that puts its own corner under
        // the finger is the defect this exists to fix, and it is invisible from
        // a mouse — which is why the decision lives in
        // `OverlayPlacement::at_pointer_for` rather than at each call site.
        let placement =
            crate::overlay::OverlayPlacement::at_pointer_for(position, &self.current_input.pointer);
        self.overlay_manager.show(crate::overlay::OverlayRequest {
            content_id,
            anchor: owner_id,
            placement,
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

    // -----------------------------------------------------------------
    // Outside-press overlay dismissal
    // -----------------------------------------------------------------

    /// Handle a press that lands outside one or more dismissable overlays.
    ///
    /// Returns `true` when the press is consumed and must not reach the tree.
    ///
    /// **An indirect pointer is unchanged.** It dismisses on the press and
    /// falls through, so one click still closes a menu and actuates the control
    /// beneath — deliberate behaviour for a cursor, which names one pixel that
    /// the user could see the whole time they were aiming at it.
    ///
    /// **A direct pointer arms instead.** A finger covers what it is about to
    /// actuate: the menu is the only thing the user was looking at, and the
    /// control underneath is one they never saw. So the `Down` is withheld from
    /// the tree, the dismissal waits for the release, and a press that is
    /// cancelled — or slid onto the very overlay it would have closed — leaves
    /// nothing behind at all. Both the arming `Down` and the committing `Up`
    /// are consumed, so nothing beneath ever sees half a press.
    fn arm_outside_press_dismissal(
        &mut self,
        position: Point,
        button: PointerButton,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        self.prune_stale_dismiss_arms();
        let pressing = self.current_input.pointer;
        let busy = self.busy_press_points(pressing.id);

        if pressing.kind.is_direct() {
            return self
                .overlay_manager
                .arm_dismiss(pressing.id, position, &busy)
                .suppress_beneath;
        }

        let (dismissed, focus_restore, toggle_anchors) =
            self.overlay_manager.dismiss_outside_press(position, &busy);
        if dismissed.is_empty() {
            return false;
        }
        self.dormant_dismissed_content(&dismissed, &mut *ops);
        if let Some(restore_id) = focus_restore
            && self.arena.is_active(restore_id)
        {
            self.focus_ops(restore_id, &mut *ops);
        }
        // The press dismissed one or more overlays. By default it
        // now ALSO falls through to the widget under the cursor,
        // so a single click both closes the menu/popover and
        // activates the control beneath — the behaviour a
        // secondary press already had. The one case still
        // swallowed: a primary press on the anchor of a
        // click-opened overlay, because the anchor's own tap
        // handler would otherwise reopen the overlay this very
        // press just dismissed (click-the-trigger-to-close).
        button == PointerButton::Primary
            && toggle_anchors.iter().any(|&anchor| {
                self.arena.is_active(anchor) && self.arena.bounds(anchor).contains(position)
            })
    }

    /// Complete a direct pointer's armed dismissal on its release.
    ///
    /// Returns `true` when this pointer held an arm — in which case the `Up` is
    /// consumed whether or not anything actually closed. Nothing beneath saw
    /// the `Down`, so delivering the `Up` alone would hand a widget the second
    /// half of a press it never started.
    fn commit_outside_press_dismissal(
        &mut self,
        position: Point,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        let pointer = self.current_pointer_id();
        if !self.overlay_manager.has_armed_dismiss(pointer) {
            return false;
        }
        let (dismissed, focus_restore, _anchors) =
            self.overlay_manager.commit_dismiss(pointer, position);
        if !dismissed.is_empty() {
            self.dormant_dismissed_content(&dismissed, &mut *ops);
            if let Some(restore_id) = focus_restore
                && self.arena.is_active(restore_id)
            {
                self.focus_ops(restore_id, &mut *ops);
            }
        }
        true
    }

    /// Where every *other* live pointer is holding a press.
    ///
    /// A press is only "outside" relative to the overlays nobody else is
    /// working in — see
    /// [`OverlayManager::dismiss_outside_press`](crate::overlay::OverlayManager::dismiss_outside_press).
    /// Liveness is judged with
    /// [`press_is_revocable`](Self::press_is_revocable), the predicate the
    /// cancel funnel already uses, and for the same reason: a pointer with
    /// neither a sequence nor a capture has no interaction that could be taken
    /// away, so it has none to protect either. That also exempts a pointer
    /// inside its terminal `Up` — its sequence is still installed but already
    /// terminating — which is what lets a menu item's own handler close its
    /// menu while a second contact rests elsewhere.
    ///
    /// The *press* position, not the current one: a contact that grabbed a
    /// menu and dragged past its edge is still manipulating that menu.
    fn busy_press_points(&self, exclude: crate::pointer::PointerId) -> Vec<Point> {
        self.pointers
            .iter()
            .filter(|entry| entry.info.id != exclude)
            .filter(|entry| self.press_is_revocable(entry.info.id))
            .map(|entry| entry.down_position)
            .collect()
    }

    /// Drop arms whose contact is gone.
    ///
    /// An arm is normally retired by its own `Up` or `Cancel`. A contact that
    /// disappears without either — a table sweep, a window losing its input —
    /// would otherwise leave one behind, and a later contact minted with the
    /// same id would inherit a dismissal it never asked for.
    fn prune_stale_dismiss_arms(&mut self) {
        let stale: Vec<crate::pointer::PointerId> = self
            .overlay_manager
            .armed_pointers()
            .into_iter()
            .filter(|id| self.pointers.get(*id).is_none())
            .collect();
        for id in stale {
            self.overlay_manager.abort_dismiss(id);
        }
    }

    // -----------------------------------------------------------------
    // Press state
    // -----------------------------------------------------------------

    /// Open the press record for the contact being dispatched.
    ///
    /// Called from the `PointerDown` arm before the press is dispatched, with
    /// the focusable the press landed on (which a direct pointer will hold
    /// until its release). The node whose visual the record drives is not
    /// known yet — the arena claims the press during the dispatch — so
    /// [`adopt_press_owner`](Self::adopt_press_owner) finishes the record
    /// afterwards.
    ///
    /// The press-feedback delay applies **only inside a pan claimant**: a
    /// finger resting on a list row must not flash the row before the pan has
    /// been ruled out, while a button that nothing can scroll has no ambiguity
    /// to wait out and highlights at once. `begin_pan` has already decided
    /// whether this press is inside one — a session exists only when a claimant
    /// along the hit path accepts this pointer kind — so this reads its answer
    /// rather than re-deriving it.
    fn begin_press(&mut self, focusable: Option<WidgetId>) {
        let pointer = self.current_pointer_id();
        let now = self.sequence_now();
        let delay = self
            .pan_session_open(pointer)
            .then(|| self.current_profile().press_feedback_delay)
            .filter(|d| !d.is_zero());
        self.presses.press(pointer, focusable, now, delay);
    }

    /// Record the node whose gesture arena took this press, and publish its
    /// signal.
    ///
    /// The owner is the sequence's `pressed_owner` — the node holding the
    /// pointer capture once the `Down` has been dispatched, which is exactly
    /// the node whose `on_tap` would fire. A press no arena took owns no
    /// visual, and its record stays for the focus deferral alone.
    ///
    /// So does a press on a **button the owner cannot act on**. A press visual
    /// says "release here and this control acts", so it has to answer to the
    /// same buttons the activation does: the router opens a record for every
    /// button, but only [`press_buttons`](Self::press_buttons) — the union of
    /// the owner's own click-style [`ButtonMask`](crate::event::ButtonMask)s,
    /// `PRIMARY` unless the widget widened it — decides which of them may light
    /// the control up. Without the gate a middle-click, or a right-click on a
    /// node with no context menu, would raise a press that can never complete;
    /// with it, the visual and the activation agree by construction, exactly as
    /// they do at the tap boundary where one predicate fails the tap, fires
    /// `cancel_taps` and clears the visual.
    ///
    /// The record itself is untouched either way, so the focus a direct
    /// pointer deferred to its release is still there to be assigned.
    fn adopt_press_owner(&mut self, button: PointerButton) {
        let pointer = self.current_pointer_id();
        let Some(owner) = self.current_sequence().and_then(|s| s.pressed_owner()) else {
            return;
        };
        if !self.press_buttons(owner).contains(button) {
            crate::trace_input!(
                Samples,
                "no press visual for {owner:?}: {button:?} is not one of its accepted buttons"
            );
            return;
        }
        self.presses.set_owner(pointer, owner);
        self.publish_pressed(owner);
    }

    /// Re-evaluate the press being dispatched against `position`.
    ///
    /// Three ways a press visual changes without a release:
    ///
    /// * the pointer left the press's [`TapBoundary`](crate::gesture::TapBoundary)
    ///   — the same predicate that fails the tap and fires `cancel_taps`, so
    ///   the visual and the activation are abandoned together;
    /// * it came back inside, which restores the visual: WCAG 2.2 SC 2.5.2's
    ///   abort gesture is reversible right up to the release;
    /// * the arbitration decided for somebody else — a pan claimant, an
    ///   ancestor drag — and the pressed control has lost the press without
    ///   ever seeing a release.
    fn update_press(&mut self, position: Point) {
        let pointer = self.current_pointer_id();
        if self.presses.get(pointer).is_none() {
            return;
        }
        // A peer claim ends the press outright: the node was never told, and
        // leaving its visual up would advertise an interaction it has lost.
        let claimed_elsewhere = self.current_sequence().is_some_and(|sequence| {
            sequence
                .winner()
                .is_some_and(|winner| Some(winner) != sequence.pressed_owner())
        });
        if claimed_elsewhere {
            self.end_press(pointer);
            return;
        }
        let profile = self.current_profile();
        let origin = self.current_sequence().map(|s| s.press_origin());
        let owner = self.presses.get(pointer).and_then(|p| p.owner);
        let inside = match (origin, owner) {
            (Some(origin), Some(owner)) => {
                let bounds = self
                    .arena
                    .is_active(owner)
                    .then(|| self.arena.bounds(owner));
                !crate::gesture::TapBoundary::for_pointer(&self.current_input.pointer, &profile)
                    .left(origin, position, bounds, &profile)
            }
            // No sequence to measure from, or no owner to measure against:
            // there is no visual either way, so the flag is moot.
            _ => true,
        };
        let Some(press) = self.presses.get_mut(pointer) else {
            return;
        };
        if press.inside == inside {
            return;
        }
        press.inside = inside;
        if let Some(owner) = owner {
            self.publish_pressed(owner);
        }
    }

    /// Close the press held by `pointer` and republish the node it drove.
    ///
    /// The one exit: a release, a cancel, and a peer claim all come through
    /// here, so a node can never be left painted as pressed by a path that
    /// forgot to clear it.
    pub(crate) fn end_press(&mut self, pointer: crate::pointer::PointerId) {
        if let Some(owner) = self.presses.release(pointer) {
            self.publish_pressed(owner);
        }
    }

    /// Resolve every elapsed press-feedback delay and publish the visuals that
    /// just appeared. Driven by the same tick that advances the long press.
    pub(crate) fn resolve_press_delays(&mut self, now: crate::pointer::EventTime) {
        if self.presses.is_empty() {
            return;
        }
        for id in self.presses.resolve_delays(now) {
            self.publish_pressed(id);
            self.arena.mark_needs_paint(id);
        }
    }

    /// The earliest instant a pending press wants the event loop back, so a
    /// finger that lands and does not move still gets its highlight.
    pub(crate) fn next_press_deadline(&self) -> Option<std::time::Instant> {
        self.presses.next_deadline().map(|t| self.instant_for(t))
    }

    /// Assign the focus a direct pointer's press deferred, if the release
    /// earned it.
    ///
    /// The guard is "the release landed on the same focusable as the press".
    /// A finger that presses a button, slides onto its neighbour and lifts has
    /// activated nothing and must move focus nowhere — the same rule the tap
    /// recognizer applies to activation, applied to focus so the two cannot
    /// disagree. A press that found no focusable defers nothing.
    ///
    /// A no-op for an indirect pointer, which focused at press.
    fn focus_on_release(&mut self, position: Point, ops: &mut dyn crate::window::WindowOps) {
        let releasing = self.current_input.pointer;
        if !releasing.kind.is_direct() {
            return;
        }
        let pointer = self.current_pointer_id();
        let Some(pressed) = self.presses.get(pointer).and_then(|p| p.focusable) else {
            return;
        };
        let released_on = self
            .hit_test_for(position, &releasing)
            .and_then(|target| self.find_focusable_at_or_above(target));
        if released_on == Some(pressed) {
            self.focus_with_origin_ops(
                pressed,
                crate::focus::FocusOrigin::Pointer(releasing.kind),
                &mut *ops,
            );
        }
    }

    /// The hover walk for one move: hit-test, run the enter/leave transitions,
    /// then deliver `event` to whatever the pointer is now over.
    ///
    /// `event` is the `PointerMove` being dispatched and is forwarded
    /// **verbatim** — its `modifiers` (a Shift or Ctrl pressed mid-drag) and its
    /// `pointer` are the producer's own, not a copy assembled here. `position`
    /// is that event's position, passed separately only because every step below
    /// needs it.
    ///
    /// Which is the rule for the whole router: a producer's event reaches the
    /// widget carrying what the producer said, while an event the *tree*
    /// synthesizes — the enter/leave pair below — carries the tree's own view of
    /// who is pointing, read from the pointer table. The two differ only for a
    /// hand-built legacy event, which names no time and is stamped from the tree
    /// clock into the snapshot [`EventContext::pointer`] reports (see
    /// [`dispatch_event_with_ops`](Self::dispatch_event_with_ops)) without that
    /// stamp being written back onto the event.
    ///
    /// [`EventContext::pointer`]: crate::widget::EventContext::pointer
    fn handle_pointer_move(
        &mut self,
        event: &WidgetEvent,
        position: Point,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // A contact routes its move by hit test but writes **no hover**: a
        // finger has no hover state, so a second finger arriving beside a
        // hovering mouse must leave enter/leave, the cursor, tooltip dwell and
        // every `on_hover` handler exactly where they were.
        let moving = self.current_input.pointer;
        if self.pointers.hover_owner_id() != Some(self.current_pointer_id()) {
            if let Some(target) = self.hit_test_for(position, &moving) {
                self.dispatch_to_widget(target, event, &mut *ops);
            }
            return;
        }
        let target = self.hit_test_for(position, &moving);

        if target != self.hovered_id() {
            // Past the gate above, so the mover *is* the hover owner.
            let hover = self.hover_transition_pointer();
            let previously_hovered = self.hovered_id();
            if let Some(old) = previously_hovered {
                self.dispatch_to_widget(
                    old,
                    &WidgetEvent::PointerLeave { pointer: hover },
                    &mut *ops,
                );
                self.tooltip_pointer_leave(old, &mut *ops);
            }
            if let Some(new) = target {
                self.dispatch_to_widget(
                    new,
                    &WidgetEvent::PointerEnter { pointer: hover },
                    &mut *ops,
                );
                self.tooltip_pointer_enter(new);
            }
            self.set_hovered(target);
            self.update_hover_within_signals(previously_hovered, target);
        } else if let Some(target) = target {
            // Same hover target — restart pending tooltip timers if the
            // pointer is still moving beyond the stationary slop.
            self.tooltip_pointer_moved(target, position);
        }

        if let Some(target) = target {
            self.dispatch_to_widget(target, event, &mut *ops);
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

    /// Rebuild `event` with any pointer position converted into `id`'s
    /// **widget-local** space. Returns `None` for events that carry no
    /// position, so the caller keeps the original event.
    ///
    /// This is the single point where the framework localizes pointer
    /// coordinates. It runs once per node in both the preview and bubble
    /// passes, and because both `on_pointer_event` and the gesture arena
    /// read the position out of `event`, localizing it here makes
    /// `on_tap` / `on_double_tap` / `on_long_press` / `on_drag` and
    /// `on_pointer_event` all receive widget-local coordinates uniformly.
    /// See [`WidgetArena::local_pointer_position`].
    pub(super) fn localize_event(&self, id: WidgetId, event: &WidgetEvent) -> Option<WidgetEvent> {
        match event {
            WidgetEvent::PointerDown {
                position,
                button,
                modifiers,
                pointer,
            } => Some(WidgetEvent::PointerDown {
                position: self.arena.local_pointer_position(id, *position),
                button: *button,
                modifiers: *modifiers,
                pointer: *pointer,
            }),
            WidgetEvent::PointerUp {
                position,
                button,
                modifiers,
                pointer,
            } => Some(WidgetEvent::PointerUp {
                position: self.arena.local_pointer_position(id, *position),
                button: *button,
                modifiers: *modifiers,
                pointer: *pointer,
            }),
            WidgetEvent::PointerMove {
                position,
                modifiers,
                pointer,
            } => Some(WidgetEvent::PointerMove {
                position: self.arena.local_pointer_position(id, *position),
                modifiers: *modifiers,
                pointer: *pointer,
            }),
            WidgetEvent::Gesture { gesture } => Some(WidgetEvent::Gesture {
                gesture: self.localize_gesture(id, gesture),
            }),
            // Deliberately no `Scroll` or `PointerCancel` arm. Both name their
            // position `window_position` precisely because it stays in window
            // space — see the fields' own docs for why routing and velocity
            // need it there — and adding an arm here would silently change the
            // frame every reader of those two fields works in.
            _ => None,
        }
    }

    /// Convert every position / center field of a pre-recognized
    /// [`GestureEvent`] into `id`'s widget-local space (`DragMoved.delta`
    /// is relative and left untouched). Used for the platform gesture
    /// path; arena-recognized gestures are already local because the
    /// `RawPointerEvent` feeding the arena was localized by
    /// [`Self::localize_event`].
    fn localize_gesture(&self, id: WidgetId, gesture: &GestureEvent) -> GestureEvent {
        let loc = |p: teksilo_canvas::Point| self.arena.local_pointer_position(id, p);
        let tap = |t: &TapEvent| {
            TapEvent::new(loc(t.position), t.button, t.modifiers).with_pointer(t.pointer)
        };
        match gesture {
            GestureEvent::Tap(t) => GestureEvent::Tap(tap(t)),
            GestureEvent::DoubleTap(t) => GestureEvent::DoubleTap(tap(t)),
            GestureEvent::TripleTap(t) => GestureEvent::TripleTap(tap(t)),
            GestureEvent::LongPress(t) => GestureEvent::LongPress(tap(t)),
            GestureEvent::DragStarted {
                position,
                button,
                pointer,
            } => GestureEvent::DragStarted {
                position: loc(*position),
                button: *button,
                pointer: *pointer,
            },
            GestureEvent::DragMoved {
                position,
                delta,
                pointer,
            } => GestureEvent::DragMoved {
                position: loc(*position),
                delta: *delta,
                pointer: *pointer,
            },
            GestureEvent::DragEnded { position, pointer } => GestureEvent::DragEnded {
                position: loc(*position),
                pointer: *pointer,
            },
            GestureEvent::DragCancelled {
                position,
                pointer,
                reason,
            } => GestureEvent::DragCancelled {
                position: loc(*position),
                pointer: *pointer,
                reason: *reason,
            },
            GestureEvent::PinchStarted { center } => GestureEvent::PinchStarted {
                center: loc(*center),
            },
            GestureEvent::PinchChanged {
                center,
                scale,
                rotation,
            } => GestureEvent::PinchChanged {
                center: loc(*center),
                scale: *scale,
                rotation: *rotation,
            },
            GestureEvent::PinchEnded => GestureEvent::PinchEnded,
            GestureEvent::PinchCancelled { reason } => {
                GestureEvent::PinchCancelled { reason: *reason }
            }
            GestureEvent::Swipe {
                direction,
                velocity,
            } => GestureEvent::Swipe {
                direction: *direction,
                velocity: *velocity,
            },
        }
    }

    /// Same as `dispatch_to_widget` but returns `true` when any
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

        // For a pointer press, find the innermost tap-owning node at-or-above
        // the hit target (a chevron / checkbox / inline button). A row or
        // container that selects on press consults
        // `ctx.press_claimed_by_interactive_child()` to skip selecting when this
        // owner is a strict descendant of it — the press belongs to the inner
        // control, not the row. Tap-like handlers only; drag/swipe are excluded
        // so a draggable row still selects itself on press.
        //
        // **`on_tap` / `on_long_press` only — never `on_double_tap` alone.**
        // The question this answers is "does a descendant own *this press*",
        // and a widget that wired only a multi-tap handler does not: the first
        // click of a double-click is not its business. Counting it meant a
        // table cell could not carry double-click-to-edit without also
        // silently stopping its row from selecting on a plain click — while
        // every file manager selects a row on the first click of the
        // double-click that opens it. A node that wants the press still has
        // `on_tap` (a real `Button`, a checkbox), and those are unaffected.
        let tap_owner: Option<WidgetId> = if matches!(
            event,
            WidgetEvent::PointerDown { .. } | WidgetEvent::PointerUp { .. }
        ) {
            let mut owner = None;
            let mut cur = Some(target);
            while let Some(id) = cur {
                if self.arena.get(id).is_some_and(|n| {
                    n.any_handler(|h| h.on_tap.is_some() || h.on_long_press.is_some())
                }) {
                    owner = Some(id);
                    break;
                }
                cur = self.arena.parent(id);
            }
            owner
        } else {
            None
        };

        for &id in &ancestors {
            let mut ctx = self
                .make_event_context(&mut *ops)
                .with_dispatch_node(id)
                .with_dispatch_target(target);
            ctx.press_claimed_by_interactive_child =
                tap_owner.is_some_and(|owner| owner != id && self.is_descendant_of(owner, id));
            // Convert any pointer position into this node's widget-local
            // space before its handlers see it (see `localize_event`).
            let localized = self.localize_event(id, event);
            let event = localized.as_ref().unwrap_or(event);
            let response = if let Some(node) = self.arena.get_mut(id) {
                Self::try_handler_preview(node, event, &mut ctx).unwrap_or(EventResponse::Ignored)
            } else {
                EventResponse::Ignored
            };
            self.collect_from_ctx(ctx, id);
            if response == EventResponse::Handled {
                self.arena.mark_needs_paint(id);
                // Step 1 of the decision procedure: the raw-preview pass runs
                // FIRST and keeps its root-first order, and the first `Handled`
                // claims the press. Deliberately not folded into the
                // innermost-first member order — `rich_text/mouse.rs` documents
                // relying on an outer wrapper seeing a press before an inner
                // one, and reordering it would silently invert a precedence
                // real widgets depend on.
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    self.note_preview_claim(id);
                }
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
            let mut ctx = self
                .make_event_context(&mut *ops)
                .with_dispatch_node(id)
                .with_dispatch_target(target);
            ctx.press_claimed_by_interactive_child =
                tap_owner.is_some_and(|owner| owner != id && self.is_descendant_of(owner, id));
            // Convert any pointer position into this node's widget-local
            // space before its handlers (and its gesture arena) see it.
            let localized = self.localize_event(id, event);
            let gesture_cx = self.recognizer_context(id);
            // A member that lost the arbitration keeps its handlers and loses
            // only its recognizers — see `sequence_blocks_arena`.
            let arena_blocked = self.sequence_blocks_arena(id);
            let WidgetTree {
                arena,
                gesture_owners,
                ..
            } = self;
            let event = localized.as_ref().unwrap_or(event);
            let response = if let Some(node) = arena.get_mut(id) {
                Self::try_handler_bubble(
                    node,
                    event,
                    &mut ctx,
                    BubbleGates {
                        fire_on_pointer_event: is_target,
                        arena_blocked,
                    },
                    id,
                    gesture_owners,
                    gesture_cx,
                )
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
                self.note_pointer_acceptance(id, event);
                // **Hover transitions are notifications, and every ancestor is
                // entitled to one.** Stopping the bubble here left a container
                // stuck hovered whenever the pointer left it *through* an
                // interactive child: the child's own `on_hover` handled the
                // `PointerLeave`, the bubble stopped, and the row went on believing
                // the pointer was still over it. A search result whose controls
                // appear on hover then kept them after the pointer had gone.
                //
                // The preview pass already refuses to let an ancestor swallow a
                // descendant's Enter/Leave; this is that rule in the other
                // direction, and it is what makes a container's hover mean "the
                // pointer is somewhere inside me" rather than "the pointer is on my
                // own background". Every other event still stops at its handler,
                // which is what makes handling one mean anything.
                if !matches!(
                    event,
                    WidgetEvent::PointerEnter { .. } | WidgetEvent::PointerLeave { .. }
                ) {
                    return true;
                }
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
        self.dispatch_to_widget_direct_returning_handled(target, event, ops);
    }

    /// [`dispatch_to_widget_direct`](Self::dispatch_to_widget_direct), reporting
    /// whether the node consumed the event.
    ///
    /// The claimant chain needs the answer: `Handled` means the container
    /// absorbed some of the delta and the walk stops, `Ignored` means it is at
    /// a boundary and the same whole event goes to the next container outward.
    /// Addressed rather than bubbled, which is exactly what a claimant chain is
    /// — a list of named recipients, like the one the cancel funnel delivers
    /// to.
    pub(super) fn dispatch_to_widget_direct_returning_handled(
        &mut self,
        target: WidgetId,
        event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        if !self.arena.is_enabled(target) {
            return false;
        }

        let mut ctx = self
            .make_event_context(&mut *ops)
            .with_dispatch_node(target)
            .with_dispatch_target(target);
        let gesture_cx = self.recognizer_context(target);
        let arena_blocked = self.sequence_blocks_arena(target);
        let WidgetTree {
            arena,
            gesture_owners,
            ..
        } = self;
        let response = if let Some(node) = arena.get_mut(target) {
            Self::try_handler_bubble(
                node,
                event,
                &mut ctx,
                BubbleGates {
                    fire_on_pointer_event: true,
                    arena_blocked,
                },
                target,
                gesture_owners,
                gesture_cx,
            )
            .unwrap_or(EventResponse::Ignored)
        } else {
            EventResponse::Ignored
        };
        self.collect_from_ctx(ctx, target);

        if response == EventResponse::Handled {
            // A scroll changes geometry, so it earns a layout pass rather than
            // a repaint — the same distinction the bubble path makes.
            if matches!(
                event,
                WidgetEvent::Scroll { .. } | WidgetEvent::ScrollIntoView { .. }
            ) {
                self.arena.mark_needs_layout(target);
            } else {
                self.arena.mark_needs_paint(target);
            }
            self.note_pointer_acceptance(target, event);
        }
        response == EventResponse::Handled
    }

    /// Remember that `target` answered `Handled` to one of the current
    /// pointer's positional events.
    ///
    /// Read only by the cancel funnel, as the recipient of last resort when a
    /// revoked pointer holds no capture. Restricted to the three positional
    /// phases: a key, an accessibility action or a focus change is not "an
    /// event from this pointer", and letting one of those set the anchor would
    /// address the cancel to a widget the pointer never touched.
    pub(super) fn note_pointer_acceptance(&mut self, target: WidgetId, event: &WidgetEvent) {
        if !matches!(
            event,
            WidgetEvent::PointerDown { .. }
                | WidgetEvent::PointerMove { .. }
                | WidgetEvent::PointerUp { .. }
        ) {
            return;
        }
        let pointer = self.current_pointer_id();
        if let Some(entry) = self.pointers.get_mut(pointer) {
            entry.last_accepted = Some(target);
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
            // `PointerEnter` / `PointerLeave` are per-node hover transitions
            // synthesized by `handle_pointer_move`, not part of the raw pointer
            // stream. Running them through the ancestor preview pass would let
            // a drag-detecting ancestor whose `on_pointer_event` returns
            // `Handled` silently swallow a descendant's hover (its cursor and
            // `on_hover` would never fire). They are delivered to their target
            // directly via the bubble pass (where Enter/Leave fire `on_hover`),
            // so they have no business in preview. `PointerMove`/`Down`/`Up`
            // and `Scroll` still preview through the catch-all below — the
            // tab-bar wheel-remap (`tab_widget/bar.rs`) and the split-view /
            // rich-text drag guards depend on that.
            WidgetEvent::PointerEnter { .. } | WidgetEvent::PointerLeave { .. } => None,
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

    fn try_handler_bubble(
        node: &mut crate::arena::WidgetNode,
        event: &WidgetEvent,
        ctx: &mut EventContext,
        gates: BubbleGates,
        node_id: WidgetId,
        gesture_owners: &mut std::collections::HashSet<WidgetId>,
        gesture_cx: crate::gesture::RecognizerContext<'_>,
    ) -> Option<EventResponse> {
        let BubbleGates {
            fire_on_pointer_event,
            arena_blocked,
        } = gates;
        match event {
            WidgetEvent::PointerEnter { .. } => {
                if let Some(cursor) = node.node_cursor {
                    // The declared channel, not `set_cursor`: a handler
                    // overriding the cursor in the same dispatch must not
                    // erase the tree's record of what the node asked for —
                    // that record is what `release_cursor` hands back to.
                    ctx.declared_cursor_request = Some(cursor);
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
            WidgetEvent::PointerLeave { .. } => {
                if node.node_cursor.is_some() {
                    ctx.declared_cursor_request = Some(crate::widget::CursorIcon::Default);
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
                // Every installed slot fires — both payload shapes, and
                // within each shape both the external (app-installed
                // `.on_access_action*`) and the widget's own. Button (own)
                // and Dialog (external) layered together rely on that for a
                // single accesskit click.
                //
                // The two shapes are layered, not alternatives, because they
                // have different owners: `on_access_action_request` is what a
                // widget reaches for when it needs `target_node` or `data`
                // (`Slider`, `SpinBox`, `TextInputField`, `CodeEditor`,
                // `TabBar`), while `.on_access_action(..)` is the app's
                // builder-level hook. Preferring the payload shape when it was
                // set therefore did not choose between two handlers for the
                // same job — it silently disabled the app's handler on exactly
                // the widgets that had migrated, with nothing at the call site
                // to say so.
                //
                // Assistive-tech action paths run under the `Accessibility`
                // source label. Restored after the block.
                let saved_a11y_source = ctx
                    .current_source
                    .replace(crate::telemetry::IntentSource::Accessibility);
                let mut any_slot = false;
                let mut any_handled = false;
                if let Some(h) = node.external_handlers.on_access_action_request.as_mut() {
                    any_slot = true;
                    any_handled |=
                        h(*action, *target_node, data.clone(), ctx) == EventResponse::Handled;
                }
                if let Some(h) = node.handlers.on_access_action_request.as_mut() {
                    any_slot = true;
                    any_handled |=
                        h(*action, *target_node, data.clone(), ctx) == EventResponse::Handled;
                }
                if let Some(h) = node.external_handlers.on_access_action.as_mut() {
                    any_slot = true;
                    any_handled |= h(*action, ctx) == EventResponse::Handled;
                }
                if let Some(h) = node.handlers.on_access_action.as_mut() {
                    any_slot = true;
                    any_handled |= h(*action, ctx) == EventResponse::Handled;
                }
                let user_handled = any_slot.then_some(if any_handled {
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                });

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
                ..
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
                if arena_blocked {
                    // This node lost the arbitration for the press: its
                    // recognizers stay out of it, and the event goes on
                    // bubbling as if the node carried none.
                    return None;
                }
                Self::ensure_gesture_arena(node, node_id, gesture_owners);
                if let Some(arena) = node.handlers.gesture_arena.as_mut() {
                    let cx = gesture_cx;
                    // Implicit capture for the Down..Up sequence so that
                    // moves leaving the widget bounds still reach the
                    // arena. Without this, a drag that starts inside the
                    // widget but crosses its edge before the recognizer
                    // latches would be hit-tested to another widget and
                    // the press-origin arena would never see a `Move`.
                    // Released unconditionally by the `PointerUp` branch
                    // in `dispatch_event`.
                    //
                    // **Implicit**: this is plumbing, not a claim. Routing it
                    // through the public `capture_pointer` would enrol every
                    // arena-bearing node as a `RawDrag` competitor and decide
                    // every mouse sequence at press. See
                    // `EventContext::capture_pointer_implicit`.
                    ctx.capture_pointer_implicit();
                    let result = arena.process(
                        &RawPointerEvent::Down {
                            position: *position,
                            button: *button,
                            modifiers: *modifiers,
                            pointer: cx.pointer,
                            time: cx.now,
                        },
                        &cx,
                    );
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
                ..
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
                if arena_blocked {
                    return None;
                }
                if let Some(arena) = node.handlers.gesture_arena.as_mut() {
                    let cx = gesture_cx;
                    let result = arena.process(
                        &RawPointerEvent::Up {
                            position: *position,
                            button: *button,
                            modifiers: *modifiers,
                            pointer: cx.pointer,
                            time: cx.now,
                        },
                        &cx,
                    );
                    if let Some(gesture) = result {
                        Self::dispatch_recognized_gesture(node, gesture, ctx);
                    }
                    return Some(EventResponse::Handled);
                }
                None
            }
            WidgetEvent::PointerMove { position, .. } => {
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
                if arena_blocked {
                    return None;
                }
                if let Some(arena) = node.handlers.gesture_arena.as_mut() {
                    let cx = gesture_cx;
                    let result = arena.process(
                        &RawPointerEvent::Move {
                            position: *position,
                            pointer: cx.pointer,
                            time: cx.now,
                        },
                        &cx,
                    );
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
                        // `teksilo-app::update_control_flow` only wakes up when
                        // `needs_redraw()` is true.
                        return Some(EventResponse::Handled);
                    }
                    return Some(EventResponse::Ignored);
                }
                None
            }
            WidgetEvent::PointerCancel {
                reason, pointer, ..
            } => {
                // Two hooks, and the dedicated one always runs. `on_pointer_cancel`
                // is a notification, not a route: a widget releasing what its
                // press latched has nothing to consume, and letting it report
                // `Handled` would make releasing state look like claiming the
                // event. The raw `on_pointer_event` hook keeps its ordinary
                // consuming semantics for widgets that drive the whole pointer
                // stream themselves.
                for slot in [
                    &mut node.external_handlers.on_pointer_cancel,
                    &mut node.handlers.on_pointer_cancel,
                ] {
                    if let Some(handler) = slot.as_mut() {
                        handler(pointer, *reason, ctx);
                    }
                }
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
                None
            }
        }
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
        // Declared first, handler second — the order the two used to occur in
        // when both wrote the same slot, so a handler that speaks during an
        // enter still outranks the node it entered.
        if let Some(declared) = ctx.declared_cursor_request {
            self.node_declared_cursor = declared;
            self.current_cursor = declared;
        }
        match ctx.cursor_request {
            Some(crate::widget::CursorRequest::Set(cursor)) => {
                self.current_cursor = cursor;
            }
            // A withdrawal restores the node-declared cursor rather than
            // resetting to `Default`: the handler is stepping back, not
            // claiming the cursor is nothing.
            Some(crate::widget::CursorRequest::Release) => {
                self.current_cursor = self.node_declared_cursor;
            }
            None => {}
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
        if ctx.force_close_requested {
            self.force_close_requested = true;
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
        match ctx.dismiss_scope {
            Some(crate::widget::DismissScope::All) => {
                let dismissed = self.overlay_manager.dismiss_all();
                self.dormant_dismissed_content(&dismissed, &mut *ops);
            }
            Some(crate::widget::DismissScope::AllExceptHosts) => {
                self.dismiss_all_overlays_except_hosts(&mut *ops);
            }
            Some(crate::widget::DismissScope::SelfChain) => {
                self.dismiss_self_overlay_chain_for_source(source_widget, &mut *ops);
            }
            Some(crate::widget::DismissScope::Top) => {
                if let Some((_id, content_ids, focus_restore)) = self.overlay_manager.dismiss_top()
                {
                    self.dormant_dismissed_content(&content_ids, &mut *ops);
                    if let Some(restore_id) = focus_restore
                        && self.arena.is_active(restore_id)
                    {
                        self.focus_ops(restore_id, &mut *ops);
                    }
                }
            }
            None => {
                for id in ctx.overlay_dismissals {
                    let dismissed = self.overlay_manager.dismiss(id);
                    self.dormant_dismissed_content(&dismissed, &mut *ops);
                }
            }
        }
        // Content-keyed dismissals (`dismiss_overlay_by_content`). Drained
        // unconditionally — independent of `dismiss_scope` and of the
        // pending delayed-overlay list — so a handler can retract a shown
        // reusable overlay it identifies only by content. Resolving the
        // id here (not at call time) is what lets the caller skip
        // tracking the `OverlayId`.
        for content_id in ctx.overlay_content_dismissals {
            if let Some(overlay_id) = self.overlay_manager.find_by_content(content_id) {
                let dismissed = self.overlay_manager.dismiss(overlay_id);
                self.dormant_dismissed_content(&dismissed, &mut *ops);
            }
        }
        // Apply pause/resume queue (ToastHost hover-pause). Drained
        // here so the handler-side `ctx.pause_overlay_auto_dismiss(id)`
        // is order-independent with `dismiss_overlay(id)` and the
        // scope-based dismissals: pause/resume on an overlay that
        // was concurrently dismissed is silently dropped (the find
        // inside the OverlayManager methods misses on the gone id).
        for (id, pause) in ctx.overlay_pause_requests {
            if pause {
                self.overlay_manager.pause_auto_dismiss(id);
            } else {
                self.overlay_manager.resume_auto_dismiss(id);
            }
        }
        for preserve_content in ctx.dismiss_descendant_overlays {
            self.dismiss_child_overlays_for_source(source_widget, preserve_content, &mut *ops);
        }
        let deferred_row_activations =
            self.apply_tree_mutations(std::mem::take(&mut ctx.tree_mutations));
        if ctx.request_a11y_update {
            self.a11y_dirty = true;
        }
        if let Some(visible) = ctx.soft_keyboard_request.take() {
            self.request_soft_keyboard(visible);
        }
        // Handed to the tree's own live regions, which schedule the two
        // accessibility syncs each message needs. See `crate::announcer`.
        for (message, politeness) in std::mem::take(&mut ctx.announcements) {
            self.announce_with(message, politeness);
        }
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
            // Overlay show changes the AT tree shape — mirror the
            // `WidgetTree::show_overlay` path. The dismissal sibling
            // (`dismiss_overlay_with_ops`) already flips this.
            self.a11y_dirty = true;
            if let Some(focus_id) = current_focus {
                self.overlay_manager.set_top_focus_restore(focus_id);
            }
        }
        for (mut req, band) in ctx.overlay_band_requests {
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
            let content_id = req.content_id;
            self.overlay_manager.show_in_band(req, band);
            self.arena.activate(content_id);
            self.a11y_dirty = true;
            // Deliberately no `set_top_focus_restore`: the text-affordance band
            // never takes focus from the anchor, so there is nothing to give
            // back when it goes.
        }
        // After the shows, so a handler may raise an overlay and place it in
        // the same dispatch.
        for (content_id, placement) in ctx.overlay_placement_updates {
            if let Some(overlay_id) = self.overlay_manager.find_by_content(content_id) {
                self.overlay_manager.update_placement(overlay_id, placement);
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
            self.a11y_dirty = true;
            if let Some(focus_id) = current_focus {
                self.overlay_manager.set_top_focus_restore(focus_id);
            }
        }
        for (mut req, progress, duration) in ctx.reveal_overlay_requests {
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
            let content_id = req.content_id;
            let current_focus = self.focused;
            let overlay_id = self.overlay_manager.show(req);
            self.overlay_manager
                .set_shown_at_sim(overlay_id, self.sim_clock);
            self.a11y_dirty = true;
            if let Some(focus_id) = current_focus {
                self.overlay_manager.set_top_focus_restore(focus_id);
            }
            // Drive the caller's progress signal 0 → 1, and register it
            // as the overlay's fade-state signal so every dismiss path
            // tweens it 1 → 0 and defers removal until it completes — the
            // same deferral machinery as `with_fade`, minus `set_opacity`
            // (the caller owns how `progress` paints).
            self.register_animated_signal(&progress, content_id);
            let _ = progress.try_animate_with_options(crate::animation::AnimationRequest {
                target: 1.0,
                duration,
                easing: teksilo_tokens::Easing::EaseOut,
                frame_interval: None,
                looping: false,
                epsilon: 0.0,
                max_duration: None,
            });
            self.overlay_manager
                .attach_fade(overlay_id, progress, duration);
        }
        if let Some((pointer, capture)) = ctx.pointer_capture {
            // Per pointer, and by default the pointer whose sample the handler
            // was serving — so a mouse call site means exactly what it meant
            // before, and two contacts on two widgets hold two captures.
            let named = pointer;
            let pointer = pointer.unwrap_or_else(|| self.current_pointer_id());
            self.set_pointer_capture(pointer, capture.then_some(source_widget));
            // An explicit `capture_pointer()` from a handler is an arbitration
            // act; the arena's and the drag pipeline's own captures are
            // plumbing and route through `capture_pointer_implicit`.
            if capture && ctx.explicit_capture && named.is_none() {
                self.note_explicit_capture(source_widget);
            }
        }
        if let Some(activation) = ctx.drag_activation_override.take() {
            // A press handler chose this node's drag activation for this press.
            // Onto the sequence, where it dies with the press — see
            // `EventContext::set_drag_activation`.
            self.note_drag_activation_override(source_widget, activation);
        }
        if ctx.recognized_owning_gesture {
            // A drag or a swipe recognized on this node owns the rest of the
            // press, however the recognizer was reached.
            self.note_gesture_recognized(source_widget);
        }
        if !ctx.gesture_acts.is_empty() {
            let acts = std::mem::take(&mut ctx.gesture_acts);
            self.apply_gesture_acts(&acts, source_widget);
        }
        if let Some(reason) = ctx.cancel_pointer_request {
            let pointer = self.current_pointer_id();
            self.cancel_pointer(pointer, reason, &mut *ops);
        }
        for (mut request, delay, focus_target, replace_siblings) in ctx.delayed_overlay_requests {
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
                replace_siblings,
                real_requested_at: std::time::Instant::now(),
                sim_requested_at: self.sim_clock,
            });
            self.arena.mark_needs_paint(source_widget);
        }
        for content_id in ctx.cancel_delayed_overlays {
            self.pending_delayed_overlays
                .retain(|pending| pending.request.content_id != content_id);
        }
        // Apex = the last sample that was still over the anchor, which
        // for the intended caller (the anchor's own hover-leave) is the
        // point the diagonal starts from. Ops are applied per node as
        // its handler returns, so this lands before the next widget's
        // hover-enter and before the move's own pointer-leave
        // bookkeeping.
        for content_id in ctx.safe_region_arm_requests {
            if let Some(apex) = self
                .previous_pointer_position
                .or_else(|| self.hover_owner_position())
            {
                self.overlay_manager.arm_safe_region(
                    content_id,
                    apex,
                    std::time::Instant::now(),
                    self.sim_clock,
                );
            }
        }
        for id in ctx.repaint_requests {
            self.arena.mark_needs_paint(id);
        }
        for id in ctx.synthetic_clicks {
            // Over the caller's ops, never a standalone dispatch: the
            // tapped widget's own handler runs inside this nested
            // dispatch, so a standalone one would deny it the
            // multi-window API this dispatch already has in hand.
            self.synthesise_tap_with_ops(id, &mut *ops);
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
        if let Some(&id) = ctx.focus_into_requests.last() {
            // "Focus into" semantics: land on the first focusable descendant
            // and — unlike `focus_requests` above — do NOT fall back to the
            // container itself. A region with no focusable content (and not
            // focusable in its own right) leaves focus untouched rather than
            // trapping it on a non-interactive node. Drives Enter-on-a-tab →
            // into the tab panel.
            if let Some(target) = self.first_focusable_descendant(id) {
                self.focus_ops(target, &mut *ops);
            }
        }

        // Rect-based "scroll this into view" requests (`ctx.ensure_visible`).
        // Walk outward from the widget whose handler queued the request and
        // reveal the rect inside every enclosing scroll container. Run after
        // focus so that if the same handler also moved focus, both follows
        // settle against the same (pre-relayout) bounds; each dispatch is
        // gated on the container not already showing the rect, so ordering is
        // harmless. The source widget itself is excluded from the walk — it
        // owns revealing an interior rect inside its own viewport.
        for req in ctx.scroll_into_view_requests {
            self.scroll_rect_into_view(
                // Whoever the rect belongs to — the source widget unless the caller
                // named another. See `EventContext::ensure_visible_from`.
                req.from.unwrap_or(source_widget),
                req.rect,
                req.margin,
                req.align,
                req.motion,
                &mut *ops,
            );
        }
        // Id-based `ctx.ensure_widget_visible`: resolve to the target's current
        // absolute bounds and walk *its* ancestors (skip if it was destroyed
        // before the drain). Walking from the target — not `source_widget` —
        // means the request reveals that widget wherever it sits, even when the
        // handler runs on a different node (a group's roving-key handler
        // revealing the child tile it just selected).
        for (id, margin) in ctx.scroll_widget_into_view_requests {
            if self.arena.get(id).is_some() {
                let bounds = self.arena.bounds(id);
                self.scroll_rect_into_view(
                    id,
                    bounds,
                    margin,
                    crate::event::ScrollAlign::Minimal,
                    crate::event::ScrollMotion::Instant,
                    &mut *ops,
                );
            }
        }

        // Keyboard-highlight tooltip: surface the highlighted (menu) item's
        // tooltip immediately and dismiss the previously-highlighted one. Keyed
        // on the item id, NOT real focus (which stays on the menu panel for key
        // handling). Only the last request per handler is honoured.
        if let Some(&id) = ctx.highlight_tooltip_requests.last() {
            self.show_highlight_tooltip(id, &mut *ops);
        }

        // --- Drag and drop ---
        if let Some((source_widget, payload, preview_widget)) = ctx.drag_start_request {
            let (preview_content_id, preview_overlay_id) = if let Some(preview) = preview_widget {
                // `add_boxed` — NOT `arena.insert` — runs the widget's
                // `build()` so composite previews (our `DragPreview`
                // wrapper in teksilo-widgets, or anything a user supplies)
                // actually instantiate their child subtree. Plain
                // `arena.insert` stops at the root node, leaves build
                // un-fired, and the overlay renders an empty widget.
                let content_id = self.add_boxed(preview);
                let overlay_id = self.overlay_manager.show(crate::overlay::OverlayRequest {
                    content_id,
                    anchor: source_widget,
                    placement: crate::overlay::OverlayPlacement::AtPointer(
                        teksilo_canvas::Point::ZERO,
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
                // The pointer that armed the drag. `start_drag` is always
                // reached from a handler serving a real sample, which is the
                // only place this is knowable — from here on the drag runs
                // through layout ticks and platform threads that have no
                // sample of their own. See `DragSession::pointer`.
                pointer: self.current_input.pointer,
                source_widget: Some(source_widget),
                is_external: false,
                current_position: teksilo_canvas::Point::ZERO,
                current_target: None,
                feedback: crate::drag_state::DropFeedback::NoFeedback,
                preview_content_id,
                preview_overlay_id,
            });
            self.set_current_pointer_capture(Some(source_widget));
            // Grabbing-hand cursor while the drag is in flight. Reset on
            // drop / cancel / source-destroyed below.
            self.current_cursor = crate::widget::CursorIcon::Grabbing;
        }
        if ctx.cancel_drag {
            self.cancel_active_drag(&mut *ops);
        }

        // --- Environment changes (architecture §9.5) ---
        if let Some(theme) = ctx.theme_request {
            // Stored, not applied: the app layer routes this through
            // `WindowManager::set_theme` so every window re-themes, matching
            // the app-wide `set_locale` path below. Applying
            // `WidgetTree::set_theme` inline would re-theme only the
            // originating window.
            self.pending_theme_request = Some(theme);
        }
        if ctx.follow_system_request {
            // Stored, not applied: the app layer switches to
            // `ThemeMode::Native` and recomputes the theme from the current
            // OS colours, fanning it to every window.
            self.pending_follow_system_request = true;
        }
        if let Some(locale) = ctx.locale_request {
            // Stored, not applied: the app layer must route this through
            // `WindowManager::set_locale` so the `I18nManager`'s active
            // locale and direction stay in sync. Applying via
            // `WidgetTree::set_locale` alone would leave `tr!` bindings
            // reading the old translations.
            self.pending_locale_request = Some(locale);
        }
        if let Some(scale) = ctx.text_scale_request {
            // Stored, not applied: the app layer routes this through
            // `WindowManager::set_text_scale` so every window re-scales its
            // text. Applying `WidgetTree::set_user_text_scale` inline would
            // grow only the originating window.
            self.pending_text_scale_request = Some(scale);
        }
        // Last, and with a context of their own: `Space` on a data view's
        // focused row runs the row's published toggle, and a checkbox's toggle
        // fires the app's `on_change`, which may send an intent or open a
        // window. Running them here rather than inside the mutation drain is
        // what gives them an `EventContext`; the drain resolved which action to
        // run against the live tree and handed it back.
        if !deferred_row_activations.is_empty() {
            self.run_with_event_context(&mut *ops, move |ctx| {
                for action in deferred_row_activations {
                    action(ctx);
                }
            });
        }
    }

    /// Returns the row activations it resolved but could not run: they need an
    /// [`EventContext`], and this method has no `ops` to build one from. The
    /// caller runs them once the drain is finished, the way
    /// [`WidgetTree::run_mount_actions`](crate::WidgetTree::run_mount_actions)
    /// does.
    #[must_use]
    fn apply_tree_mutations(
        &mut self,
        mutations: Vec<crate::widget::TreeMutation>,
    ) -> Vec<std::rc::Rc<dyn Fn(&mut crate::widget::EventContext)>> {
        let mut deferred_row_activations = Vec::new();
        use crate::binding::BindingLevel;
        use crate::widget::TreeMutation;

        for mutation in mutations {
            match mutation {
                TreeMutation::SetDormant(id) => {
                    self.park_subtree(id);
                }
                TreeMutation::Activate(id) => self.arena.activate(id),
                TreeMutation::Destroy(id) => {
                    // Route through `destroy_subtree`, NOT the bare
                    // `arena.destroy`: the latter only unlinks nodes from
                    // the slotmap and leaks everything the widget owned —
                    // animation-scheduler entries (which hold strong
                    // `Signal<f32>` clones, so the widget keeps animating
                    // after it's gone), animated-quad slots, event-source
                    // subscriptions, registered shortcuts, bindings, and
                    // gesture ownership — and leaves `focused`/`hovered`
                    // dangling at a removed id. This mirrors the build-time
                    // `BuildContext::destroy_subtree`, including dismissing
                    // any overlay that still references the subtree so the
                    // manager doesn't retain a stale content reference.
                    if let Some(overlay_id) = self.overlay_manager().find_by_content(id) {
                        self.dismiss_overlay(overlay_id);
                    }
                    self.destroy_subtree(id);
                }
                // Build now, not next frame: the same handler is about to show
                // an overlay over this node and move focus into it, and both
                // read the subtree. See `EventContext::materialize_now`.
                TreeMutation::MaterializeNow(id) => {
                    if self.arena.get(id).is_some() {
                        self.rebuild_single_widget(id);
                    }
                }
                TreeMutation::RowSpaceActivate { row, fallback } => {
                    // Resolve against the *live* tree: a data view rebuilds its
                    // rows as they realize, so the row that was focused when
                    // the key arrived may have been rebuilt since.
                    //
                    // Resolve here, run later. The action carries an
                    // `EventContext` so a row's checkbox fires its `on_change`
                    // on this path exactly as it does under the pointer; this
                    // method has no `ops` to build one from, so the caller runs
                    // it after the drain.
                    deferred_row_activations.push(self.keyboard_toggle_in(row).unwrap_or(fallback));
                }
                TreeMutation::WithWidgetMut { id, dirty, apply } => {
                    // Run the typed mutation while `&mut arena` is live, then
                    // drop the borrow before dirty-marking (the `mark_*` calls
                    // re-borrow the arena). Only dirty-mark a live node so we
                    // never call `mark_ancestors_need_layout` on a destroyed id.
                    let existed = if let Some(any) =
                        self.arena.get_mut(id).and_then(|n| n.widget.as_any_mut())
                    {
                        apply(any);
                        true
                    } else {
                        false
                    };
                    if existed {
                        match dirty {
                            BindingLevel::RepaintOnly => self.arena.mark_needs_paint(id),
                            BindingLevel::SubtreeRepaint => self.arena.mark_subtree_needs_paint(id),
                            BindingLevel::Relayout => {
                                self.arena.mark_needs_layout(id);
                                self.arena.mark_ancestors_need_layout(id);
                            }
                            BindingLevel::Rebuild => {
                                self.arena.mark_needs_rebuild(id);
                                self.arena.mark_ancestors_need_layout(id);
                            }
                            BindingLevel::AccessibilityOnly => self.a11y_dirty = true,
                        }
                    }
                }
            }
        }
        deferred_row_activations
    }

    /// Hit-test at a point for the **mouse, exactly** — the meaning this door
    /// has always had, and keeps.
    ///
    /// A mouse cursor's hot-spot is exact, so neither hit-targeting mechanism
    /// applies to it: the outset pre-pass sees zero insets and the miss-only
    /// slop pass short-circuits on a zero radius. A caller that holds a pointer
    /// should use [`hit_test_for`](Self::hit_test_for) instead, which is the
    /// same test for a mouse and the widened one for a finger or a stylus.
    pub fn hit_test(&self, point: Point) -> Option<WidgetId> {
        self.hit_test_excluding_overlay_and_widget(point, None, None)
    }

    /// Hit-test at a point on behalf of a named pointer.
    ///
    /// Runs the exact pass with that pointer's `Widget::hit_outset`, then — only
    /// if the exact pass found nothing eligible — the miss-only slop pass. For
    /// [`PointerKind::Mouse`](teksilo_tokens::PointerKind::Mouse) this is
    /// exactly [`hit_test`](Self::hit_test).
    ///
    /// Candidates are restricted to the **topmost overlay layer the exact pass
    /// entered**: a press inside an open menu can be re-attributed to a menu
    /// row, never to a control on the page behind it.
    pub fn hit_test_for(
        &self,
        point: Point,
        pointer: &crate::pointer::PointerInfo,
    ) -> Option<WidgetId> {
        self.hit_test_for_excluding(point, pointer, None, None)
    }

    /// [`hit_test_for`](Self::hit_test_for) with the drag-and-drop exclusions of
    /// [`hit_test_excluding_overlay_and_widget`](Self::hit_test_excluding_overlay_and_widget).
    pub fn hit_test_for_excluding(
        &self,
        point: Point,
        pointer: &crate::pointer::PointerInfo,
        exclude_overlay: Option<crate::overlay::OverlayId>,
        exclude_widget: Option<WidgetId>,
    ) -> Option<WidgetId> {
        let surfaces = self.text_surfaces();
        let read_only = |id: WidgetId| surfaces.is_read_only(id);
        let hit =
            crate::pointer::hit_slop::HitContext::new(pointer.kind, &self.effective_theme.input)
                .direction(self.layout_direction)
                .read_only_probe(&read_only);
        self.hit_test_with(point, exclude_overlay, exclude_widget, &hit)
    }

    /// Hit-test at a point, excluding a specific overlay and widget from consideration.
    /// Used during drag-and-drop to exclude the preview overlay and its content widget,
    /// so they don't block hit-testing of the actual drop targets underneath.
    ///
    /// **Mouse, exact** — the pointer-aware twin is
    /// [`hit_test_for_excluding`](Self::hit_test_for_excluding).
    pub fn hit_test_excluding_overlay_and_widget(
        &self,
        point: Point,
        exclude_overlay: Option<crate::overlay::OverlayId>,
        exclude_widget: Option<WidgetId>,
    ) -> Option<WidgetId> {
        self.hit_test_with(
            point,
            exclude_overlay,
            exclude_widget,
            &crate::pointer::hit_slop::HitContext::mouse(),
        )
    }

    /// The one hit-test body: overlay first, then the arena, under whichever
    /// [`HitContext`](crate::pointer::hit_slop::HitContext) the caller built.
    pub(crate) fn hit_test_with(
        &self,
        point: Point,
        exclude_overlay: Option<crate::overlay::OverlayId>,
        exclude_widget: Option<WidgetId>,
        hit: &crate::pointer::hit_slop::HitContext<'_>,
    ) -> Option<WidgetId> {
        if let Some(overlay_id) = self.overlay_manager.hit_test(point) {
            if Some(overlay_id) == exclude_overlay {
                // Skip this excluded overlay, fall through to widget tree
            } else if let Some(overlay) = self.overlay_manager.overlay(overlay_id) {
                // Scoped to the overlay's content: this is what restricts the
                // slop pass's candidates to the topmost layer the exact pass
                // entered.
                let content_id = overlay.content_id;
                if let Some(found) =
                    self.arena
                        .hit_test_in_subtree_with_slop(content_id, point, exclude_widget, hit)
                {
                    return Some(found);
                }
                // The overlay was chosen by its **bounds** and its content
                // claimed nothing at this point. For every ordinary overlay
                // that is the end of the search — answering `None` is what
                // keeps the slop pass inside the one layer the exact pass
                // entered, so a near-miss on a menu row cannot be beaten by a
                // control behind the menu.
                //
                // An overlay whose content root declares `event_pass_through`
                // is the exception, because that flag already means "what I did
                // not claim belongs to whatever is behind me": the tree's
                // walker honours it for an ordinary node *after* its children
                // miss, and an overlay root is not an exception to it. Without
                // this, a viewport-sized pass-through layer — the placement the
                // text-affordance band was written for — stops the surface
                // under it taking presses at all. The widening is confined to
                // that flag: an overlay that does not set it still returns
                // here, so no ordinary overlay's candidate set changes.
                if !self
                    .arena
                    .get(content_id)
                    .is_some_and(|node| node.event_pass_through)
                {
                    return None;
                }
            }
        }

        if self.overlay_manager.topmost_centered().is_some() {
            return None;
        }

        // Delegates to WidgetArena::hit_test_at_with_slop, which honors
        // event_pass_through and clips_children correctly.
        self.arena.hit_test_at_with_slop(point, exclude_widget, hit)
    }
}

/// Whether this keystroke is one of the chords that ask for a context menu.
///
/// Three routes, because no single one exists on every platform:
///
/// * **The dedicated key.** `VK_APPS` on Windows, `keysyms::Menu` on X11 and
///   Wayland. `winit-0.30.13`'s AppKit backend references
///   `NamedKey::ContextMenu` zero times, so macOS never produces it.
/// * **Shift+F10.** The convention Windows, GTK and Qt all honour, and the one
///   thing a Windows or Linux keyboard without a Menu key can still reach.
/// * **Ctrl+Shift+M on macOS.** Neither of the above is available there: Mac
///   keyboards have no Menu key, and F10 is a media key under the default
///   "Use F1, F2 etc. as standard function keys = off" setting, so Shift+F10
///   may never arrive as F10 at all. Kept off the other platforms, where
///   Ctrl+Shift+M is a plausible application binding.
///
/// Modifiers are matched exactly. Shift+F10 with Ctrl held is a different
/// gesture and must reach the application unchanged.
fn is_context_menu_chord(key: Key, modifiers: Modifiers) -> bool {
    match key {
        Key::ContextMenu => modifiers == Modifiers::NONE,
        Key::F10 => modifiers == Modifiers::SHIFT,
        #[cfg(target_os = "macos")]
        Key::M => modifiers == Modifiers::CTRL | Modifiers::SHIFT,
        _ => false,
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
        assert_eq!(tree.hovered(), Some(widget));
        tree.pointer_move(Point::new(200.0, 200.0));
        assert_eq!(tree.hovered(), None);
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

    /// A handler's `set_cursor` outlives its dispatch, so a handler that
    /// re-decides the cursor on every move needs a way to stop deciding.
    ///
    /// Going quiet does not do it: the cursor moves only when something writes
    /// to it, and the node-declared cursor is written on `PointerEnter` /
    /// `PointerLeave` alone — so while the pointer stays inside one node there
    /// is no second writer, and the handler's last word simply stands. That is
    /// what `release_cursor` withdraws, and it withdraws *to the node's own
    /// declaration*, not to `Default`.
    #[test]
    fn release_cursor_hands_the_cursor_back_to_the_node_that_declared_it() {
        // Overrides itself to `Crosshair` on the left third of the widget and
        // withdraws everywhere else — the shape of any handler that arbitrates
        // its own affordance against the node it sits on.
        let mut tree = WidgetTree::new();
        tree.add(
            FillWidget::new()
                .cursor(CursorIcon::ColResize)
                .on_pointer_event(|event, ctx| {
                    if let WidgetEvent::PointerMove { position, .. } = event {
                        if position.x < 30.0 {
                            ctx.set_cursor(CursorIcon::Crosshair);
                        } else {
                            ctx.release_cursor();
                        }
                    }
                    EventResponse::Ignored
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(10.0, 25.0));
        assert_eq!(
            tree.current_cursor(),
            CursorIcon::Crosshair,
            "the handler outranks the node it is attached to",
        );
        tree.pointer_move(Point::new(60.0, 25.0));
        assert_eq!(
            tree.current_cursor(),
            CursorIcon::ColResize,
            "and hands it back to the node, not to Default — no enter/leave \
             fires on a move within one node, so nothing else could",
        );
        tree.pointer_move(Point::new(10.0, 25.0));
        assert_eq!(
            tree.current_cursor(),
            CursorIcon::Crosshair,
            "and can take it again"
        );
        tree.pointer_move(Point::new(200.0, 200.0));
        assert_eq!(
            tree.current_cursor(),
            CursorIcon::Default,
            "leaving the node clears both the override and the declaration",
        );
    }

    /// The withdrawal is a no-op for a handler that never spoke, and resolves
    /// to `Default` when the chain declares nothing — so a handler may call it
    /// unconditionally without having to remember whether it once set a cursor.
    #[test]
    fn release_cursor_is_a_no_op_with_nothing_to_undo() {
        let mut tree = WidgetTree::new();
        tree.add(
            FillWidget::new()
                .cursor(CursorIcon::ColResize)
                .on_pointer_event(|event, ctx| {
                    if matches!(event, WidgetEvent::PointerMove { .. }) {
                        ctx.release_cursor();
                    }
                    EventResponse::Ignored
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.current_cursor(), CursorIcon::ColResize);

        // Same handler over a node that declares nothing.
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_pointer_event(|event, ctx| {
            if let WidgetEvent::PointerMove { position, .. } = event {
                if position.x < 30.0 {
                    ctx.set_cursor(CursorIcon::Crosshair);
                } else {
                    ctx.release_cursor();
                }
            }
            EventResponse::Ignored
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.pointer_move(Point::new(10.0, 25.0));
        assert_eq!(tree.current_cursor(), CursorIcon::Crosshair);
        tree.pointer_move(Point::new(60.0, 25.0));
        assert_eq!(
            tree.current_cursor(),
            CursorIcon::Default,
            "nothing declared a cursor for this chain, so the hand-back \
             resolves to Default",
        );
    }

    /// A handler that speaks during the `PointerEnter` itself still outranks
    /// the node it entered — and the node's declaration is remembered anyway,
    /// so the hand-back has somewhere to go.
    ///
    /// The two used to share one slot, where the handler's later write erased
    /// the declaration outright; they are separate channels now precisely so
    /// this case keeps both.
    #[test]
    fn an_on_hover_override_does_not_erase_the_declaration_it_outranks() {
        let mut tree = WidgetTree::new();
        tree.add(
            FillWidget::new()
                .cursor(CursorIcon::ColResize)
                .on_hover(|entered, ctx| {
                    if entered {
                        ctx.set_cursor(CursorIcon::Crosshair);
                    }
                })
                .on_pointer_event(|event, ctx| {
                    if let WidgetEvent::PointerMove { position, .. } = event
                        && position.x >= 30.0
                    {
                        ctx.release_cursor();
                    }
                    EventResponse::Ignored
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(10.0, 25.0));
        assert_eq!(
            tree.current_cursor(),
            CursorIcon::Crosshair,
            "on_hover runs after the node cursor is applied, so it wins",
        );
        tree.pointer_move(Point::new(60.0, 25.0));
        assert_eq!(
            tree.current_cursor(),
            CursorIcon::ColResize,
            "and the node's declaration survived being overridden",
        );
    }

    // A leaf that opts into typed introspection, so `with_widget_mut` /
    // `widget_as_any(_mut)` can reach it (the default `as_any_mut` is `None`).
    #[derive(Debug)]
    struct Bumpable {
        value: i32,
    }

    impl crate::widget::Widget for Bumpable {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
        fn as_any(&self) -> Option<&dyn std::any::Any> {
            Some(self)
        }
        fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
            Some(self)
        }
    }

    #[test]
    fn with_widget_mut_applies_and_dirty_marks() {
        let mut tree = WidgetTree::new();
        let id = tree.add(Bumpable { value: 0 });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let mut ctx = EventContext::new();
        ctx.with_widget_mut::<Bumpable>(id, crate::binding::BindingLevel::Relayout, |b| {
            b.value = 42;
        });
        tree.collect_from_ctx(ctx, id);

        let value = tree
            .widget_as_any(id)
            .and_then(|a| a.downcast_ref::<Bumpable>())
            .map(|b| b.value);
        assert_eq!(
            value,
            Some(42),
            "the deferred closure must mutate the live widget"
        );
        assert!(
            tree.needs_layout(),
            "Relayout dirty level must mark the tree for relayout"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "not the requested type")]
    fn with_widget_mut_wrong_type_panics_in_debug() {
        struct Other;
        let mut tree = WidgetTree::new();
        let id = tree.add(Bumpable { value: 0 });
        let mut ctx = EventContext::new();
        ctx.with_widget_mut::<Other>(
            id,
            crate::binding::BindingLevel::RepaintOnly,
            |_o: &mut Other| {},
        );
        // Bumpable opts into as_any_mut, so the closure runs and the
        // wrong-type downcast trips the debug_assert.
        tree.collect_from_ctx(ctx, id);
    }

    #[test]
    fn with_widget_mut_closure_may_fire_observed_signals() {
        // Reentrancy guard. The closure runs inside `apply_tree_mutations`
        // while the target arena node is mutably borrowed. If it fires a
        // `Signal` whose observer sets *another* signal — the exact
        // `SceneView` shape (`item_change_signal` → bump `reconcile_dirty`) —
        // nothing may double-borrow the arena. The arena borrow is scoped to
        // the closure call and dropped before dirty-marking; signal/observer
        // work touches the binding registry, not the arena.
        use crate::signal::Signal;
        let mut tree = WidgetTree::new();
        let id = tree.add(Bumpable { value: 0 });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let trigger = Signal::new(0_u64);
        let echo = Signal::new(0_u64);
        let echo_for_obs = echo.clone();
        let _obs = trigger.observe(move |v| echo_for_obs.set(*v));

        let trigger_in = trigger.clone();
        let mut ctx = EventContext::new();
        ctx.with_widget_mut::<Bumpable>(id, crate::binding::BindingLevel::RepaintOnly, move |b| {
            b.value = 7;
            // Fires `_obs` synchronously, mid-deferred-apply.
            trigger_in.set(99);
        });
        tree.collect_from_ctx(ctx, id); // must not panic / double-borrow

        assert_eq!(
            echo.get(),
            99,
            "the observer ran during the deferred mutation"
        );
        let value = tree
            .widget_as_any(id)
            .and_then(|a| a.downcast_ref::<Bumpable>())
            .map(|b| b.value);
        assert_eq!(value, Some(7));
    }

    #[test]
    fn request_accessibility_update_forces_rewalk() {
        let mut tree = WidgetTree::new();
        let id = tree.add(Bumpable { value: 0 });
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let _ = tree.sync_accessibility(); // populate cache, clears a11y_dirty
        assert!(
            !tree.a11y_dirty,
            "sync_accessibility should clear the dirty flag"
        );

        let mut ctx = EventContext::new();
        ctx.request_accessibility_update();
        tree.collect_from_ctx(ctx, id);
        assert!(
            tree.a11y_dirty,
            "request_accessibility_update must force an AT re-walk"
        );
    }

    #[test]
    fn rebuild_dirties_accessibility_tree() {
        // Regression for audit Blocker G1: every `BindingLevel::Rebuild`
        // consumer (ListView / TreeView / TableView / ComboBox / Calendar /
        // DockingLayout / ...) tears down and re-creates its subtree on an
        // ordinary model change, allocating fresh WidgetIds and changing the
        // AccessKit tree shape. That pass must dirty the cached AT snapshot,
        // or screen readers keep reading the pre-mutation tree indefinitely.
        let mut tree = WidgetTree::new();
        let id = tree.add(Bumpable { value: 0 });
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let _ = tree.sync_accessibility(); // populate cache, clears a11y_dirty
        assert!(
            !tree.a11y_dirty,
            "sync_accessibility should clear the dirty flag"
        );

        // Marking for rebuild is exactly what a Rebuild-level binding does;
        // the following layout pass drains pending rebuilds.
        tree.arena_mark_needs_rebuild_for_testing(id);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert!(
            tree.a11y_dirty,
            "a rebuild must dirty the AT tree so the next sync re-walks"
        );
    }

    #[test]
    fn bound_access_label_change_dirties_accessibility_tree() {
        use crate::signal::Signal;
        use crate::test_widgets::FillWidget;
        use crate::widget_builder::WidgetBuilder;

        // Regression for audit G15: a reactive `.access_label(signal)` (and
        // likewise description / value) must register at AccessibilityOnly so
        // changing the signal re-walks the AT tree and re-resolves the
        // announced name. Previously only `access_hidden` was registered, so
        // label / description / value updates were invisible to screen readers.
        let label = Signal::new("first".to_string());
        let mut tree = WidgetTree::new();
        let _id = tree.add(FillWidget::new().access_label(label.clone()));
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let _ = tree.sync_accessibility(); // populate cache, clears a11y_dirty
        assert!(!tree.a11y_dirty, "sync_accessibility should clear the flag");

        label.set("second".to_string());
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert!(
            tree.a11y_dirty,
            "changing a bound access_label must dirty the AT tree"
        );
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
        let parent = tree.add(StackWidget::new().child(child));
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
    fn pointer_positions_are_widget_local_at_nonzero_origin() {
        use crate::event::{Modifiers, PointerButton};
        use crate::test_widgets::InsetWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        // A 20px inset places the child at window origin (20, 20).
        let tap_pos: Rc<Cell<Option<Point>>> = Rc::new(Cell::new(None));
        let down_pos: Rc<Cell<Option<Point>>> = Rc::new(Cell::new(None));
        let drag_pos: Rc<Cell<Option<Point>>> = Rc::new(Cell::new(None));
        let (tp, dp, gp) = (tap_pos.clone(), down_pos.clone(), drag_pos.clone());

        let mut tree = WidgetTree::new();
        let child = tree.add(
            FillWidget::new()
                .on_tap(move |ev, _ctx| tp.set(Some(ev.position)))
                .on_pointer_event(move |ev, _ctx| {
                    if let WidgetEvent::PointerDown { position, .. } = ev {
                        dp.set(Some(*position));
                    }
                    crate::event::EventResponse::Ignored
                })
                .on_drag(move |phase, _ctx| {
                    use crate::gesture::DragPhase;
                    match phase {
                        DragPhase::Started { position, .. }
                        | DragPhase::Moved { position, .. }
                        | DragPhase::Ended { position, .. } => gp.set(Some(position)),
                        _ => {}
                    }
                }),
        );
        let inset = tree.add(InsetWidget::new(20.0).set_child(child));
        let _ = inset;
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert_eq!(tree.bounds(child).origin(), Point::new(20.0, 20.0));

        // A tap at window (50, 40) must reach the handler as local (30, 20).
        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(50.0, 40.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert_eq!(
            down_pos.get(),
            Some(Point::new(30.0, 20.0)),
            "on_pointer_event PointerDown must be widget-local"
        );
        tree.dispatch_event(WidgetEvent::pointer_up(
            Point::new(50.0, 40.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert_eq!(
            tap_pos.get(),
            Some(Point::new(30.0, 20.0)),
            "on_tap position must be widget-local"
        );

        // A drag (down then a move past the recognizer threshold) must
        // also deliver widget-local coordinates.
        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(50.0, 40.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        // First move crosses the recognizer threshold (DragStarted);
        // the second reports a known DragMoved position.
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(65.0, 55.0)));
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(90.0, 70.0)));
        assert_eq!(
            drag_pos.get(),
            Some(Point::new(70.0, 50.0)),
            "on_drag position must be widget-local"
        );
    }

    #[test]
    fn dormant_widget_not_hit_tested() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.hovered(), Some(widget));

        tree.set_dormant(widget);
        tree.pointer_move(Point::new(200.0, 200.0));
        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.hovered(), None);
    }

    #[test]
    fn ancestor_pointer_handler_does_not_suppress_descendant_hover() {
        use crate::event::EventResponse;
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        // The child reports its own hover transitions via `on_hover`.
        let hovered = Rc::new(Cell::new(false));
        let h = hovered.clone();

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_hover(move |entered, _ctx| h.set(entered)));
        // An ancestor whose `on_pointer_event` greedily claims everything it
        // previews — exactly the "drag-detecting ancestor" footgun. Before the
        // fix it consumed the descendant's `PointerEnter`/`Leave` in the
        // preview pass and the child's hover never fired.
        tree.add(
            StackWidget::new()
                .child(child)
                .on_pointer_event(|_event, _ctx| EventResponse::Handled),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        assert!(
            hovered.get(),
            "a greedy ancestor on_pointer_event must NOT swallow the child's PointerEnter"
        );

        tree.pointer_move(Point::new(500.0, 500.0));
        assert!(
            !hovered.get(),
            "PointerLeave must likewise reach the child despite the ancestor"
        );
    }

    /// **The other direction: a child must not swallow its ancestor's hover.**
    ///
    /// A row that reveals controls on hover puts interactive children inside
    /// itself, and the pointer leaves the row *through* one of them. The child's
    /// own `on_hover` used to handle the `PointerLeave` and stop the bubble there,
    /// so the row went on believing the pointer was still over it and kept its
    /// controls showing after the pointer had gone.
    #[test]
    fn a_child_hover_handler_does_not_swallow_its_ancestors() {
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let (row, button) = (Rc::new(Cell::new(false)), Rc::new(Cell::new(false)));
        let (r, b) = (row.clone(), button.clone());

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_hover(move |entered, _ctx| b.set(entered)));
        tree.add(
            StackWidget::new()
                .child(child)
                .on_hover(move |entered, _ctx| r.set(entered)),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        assert!(button.get(), "the child is hovered");
        assert!(row.get(), "and so is the row it is inside");

        tree.pointer_move(Point::new(500.0, 500.0));
        assert!(!button.get(), "the child heard the leave");
        assert!(
            !row.get(),
            "and so did the row — a container is not still hovered because the \
             pointer left it through a button"
        );
    }

    // NOTE: legacy `shortcut_intercepts_before_widget` test removed with
    // the ShortcutMap dispatch path. The new shortcut→intent interception
    // is built on top of `ShortcutRegistry` + `Action`.

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
        let mid = tree.add(StackWidget::new().child(leaf));
        let _root = tree.add(
            StackWidget::new()
                .child(mid)
                .on_key_preview(move |event, _c| match event {
                    WidgetEvent::KeyDown {
                        key: Key::Enter, ..
                    } => {
                        preview_flag.set(true);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }),
        );

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
        let mid = tree.add(StackWidget::new().child(leaf));
        let _root = tree.add(
            StackWidget::new()
                .child(mid)
                .on_key_preview(move |_event, _c| {
                    preview_flag.set(true);
                    EventResponse::Ignored
                }),
        );

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
        let inner = tree.add(
            StackWidget::new()
                .child(leaf)
                .on_key_preview(move |event, _c| {
                    if matches!(event, WidgetEvent::KeyDown { .. }) {
                        inner_log.borrow_mut().push("inner");
                    }
                    EventResponse::Ignored
                }),
        );
        let _outer = tree.add(
            StackWidget::new()
                .child(inner)
                .on_key_preview(move |event, _c| {
                    if matches!(event, WidgetEvent::KeyDown { .. }) {
                        outer_log.borrow_mut().push("outer");
                    }
                    EventResponse::Ignored
                }),
        );

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
    fn access_action_routes_to_cursored_target_not_focus() {
        // VoiceOver's VO+Space targets the node under the AT cursor (`b`),
        // even when keyboard focus is on a different control (`a`). The action
        // must fire on `b`, never get redirected to the focused `a`.
        use crate::signal::Signal;
        let a_fired = Signal::new(false);
        let b_fired = Signal::new(false);
        let a_cb = a_fired.clone();
        let b_cb = b_fired.clone();

        let mut tree = WidgetTree::new();
        let a = tree.add(
            FillWidget::new().access_action(accesskit::Action::Click, move |_ctx| a_cb.set(true)),
        );
        let b = tree.add(
            FillWidget::new().access_action(accesskit::Action::Click, move |_ctx| b_cb.set(true)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(a);
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: accesskit::Action::Click,
            target: Some(b),
            target_node: crate::accessibility::widget_id_to_node_id(b),
            data: None,
        });

        assert!(b_fired.get(), "the cursored target must receive the action");
        assert!(
            !a_fired.get(),
            "the keyboard-focused widget must NOT receive an action targeting another node"
        );
    }

    #[test]
    fn access_action_without_target_is_dropped_not_redirected_to_focus() {
        // An action with no (or an inactive) target must be dropped — never
        // silently re-routed to whatever holds keyboard focus.
        use crate::signal::Signal;
        let fired = Signal::new(false);
        let cb = fired.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(
            FillWidget::new().access_action(accesskit::Action::Click, move |_ctx| cb.set(true)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(widget);
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: accesskit::Action::Click,
            target: None,
            target_node: crate::accessibility::root_node_id(),
            data: None,
        });

        assert!(
            !fired.get(),
            "a target-less action must not be redirected to the focused widget"
        );
    }

    // NOTE: legacy `scoped_shortcut_fires_when_focused_in_subtree` test
    // removed along with the ShortcutMap dispatch path. Scope-aware
    // dispatch is handled by the new ShortcutRegistry.

    // --- Intent / Action dispatch ------------------------------

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
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        tree.press_key(Key::S, Modifiers::COMMAND);
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
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        // Deliberately no focus() call.

        tree.press_key(Key::S, Modifiers::COMMAND);
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
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(focusable);
        assert_eq!(tree.focused(), Some(focusable));

        // Destroy the focused subtree (simulates a rebuild that drops
        // the currently-focused Rebind button).
        tree.destroy_subtree(focusable);
        assert_eq!(tree.focused(), None, "focus must clear when destroyed");

        tree.press_key(Key::S, Modifiers::COMMAND);
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
                .primary(KeyStroke::command(Key::F))
                .scope(ShortcutScope::Scoped(scope_root))
                .build(),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Focus outside the scope: the shortcut does NOT activate.
        tree.focus(outside);
        tree.press_key(Key::F, Modifiers::COMMAND);
        assert_eq!(
            fired.get(),
            0,
            "scoped shortcut must not fire outside scope"
        );

        // Focus inside the scope: it fires.
        tree.focus(inside);
        tree.press_key(Key::F, Modifiers::COMMAND);
        assert_eq!(
            fired.get(),
            1,
            "scoped shortcut must fire when focus in scope"
        );
    }

    #[test]
    fn same_chord_scoped_first_falls_back_to_global_when_focus_outside() {
        // Defect 1: a Scoped binding that sorts first by id must NOT
        // shadow the slot when focus is outside its subtree — the
        // applicable Global binding fires instead. (`editor.saveBlock`
        // < `zzz.global.save`, so the scoped one wins the id-order race
        // that `find_by_keystroke` used to settle on.)
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut, ShortcutScope};
        use std::cell::Cell;
        use std::rc::Rc;

        let scoped_fired = Rc::new(Cell::new(0));
        let global_fired = Rc::new(Cell::new(0));
        let sf = scoped_fired.clone();
        let gf = global_fired.clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(FillWidget::new());
        let editor = tree.add_child(root, FillWidget::new().focusable());
        let _editor_inner = tree.add_child(editor, FillWidget::new().focusable());
        let sidebar = tree.add_child(root, FillWidget::new().focusable());

        tree.push_action(
            editor,
            Action::new("editor.saveBlock").on_invoke(move |_i, _c| sf.set(sf.get() + 1)),
        );
        tree.push_action(
            root,
            Action::new("zzz.global.save").on_invoke(move |_i, _c| gf.set(gf.get() + 1)),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("editor.saveBlock")
                .primary(KeyStroke::command(Key::S))
                .scope(ShortcutScope::Scoped(editor))
                .build(),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("zzz.global.save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(sidebar);
        tree.press_key(Key::S, Modifiers::COMMAND);

        assert_eq!(global_fired.get(), 1, "applicable global must fire");
        assert_eq!(
            scoped_fired.get(),
            0,
            "inapplicable scoped binding must not eat the chord"
        );
    }

    #[test]
    fn same_chord_global_first_yields_to_scoped_when_focus_inside() {
        // Defect 2: a Global binding that sorts first by id must yield to
        // an in-focus Scoped binding (most-specific-scope wins), then
        // reclaim the chord once focus leaves the scope. (`app.save` <
        // `editor.saveBlock`, so the global one wins id order.)
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut, ShortcutScope};
        use std::cell::Cell;
        use std::rc::Rc;

        let scoped_fired = Rc::new(Cell::new(0));
        let global_fired = Rc::new(Cell::new(0));
        let sf = scoped_fired.clone();
        let gf = global_fired.clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(FillWidget::new());
        let editor = tree.add_child(root, FillWidget::new().focusable());
        let editor_inner = tree.add_child(editor, FillWidget::new().focusable());
        let sidebar = tree.add_child(root, FillWidget::new().focusable());

        tree.push_action(
            editor,
            Action::new("editor.saveBlock").on_invoke(move |_i, _c| sf.set(sf.get() + 1)),
        );
        tree.push_action(
            root,
            Action::new("app.save").on_invoke(move |_i, _c| gf.set(gf.get() + 1)),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("editor.saveBlock")
                .primary(KeyStroke::command(Key::S))
                .scope(ShortcutScope::Scoped(editor))
                .build(),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Focus inside the editor: the scoped binding wins over global.
        tree.focus(editor_inner);
        tree.press_key(Key::S, Modifiers::COMMAND);
        assert_eq!(
            scoped_fired.get(),
            1,
            "in-focus scoped must win over global"
        );
        assert_eq!(
            global_fired.get(),
            0,
            "global must yield to the scoped binding"
        );

        // Focus outside the editor: global reclaims the chord.
        tree.focus(sidebar);
        tree.press_key(Key::S, Modifiers::COMMAND);
        assert_eq!(scoped_fired.get(), 1, "scoped stays put outside its scope");
        assert_eq!(
            global_fired.get(),
            1,
            "global fires when focus leaves the scope"
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
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(inner);

        tree.press_key(Key::S, Modifiers::COMMAND);
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
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(inner);

        tree.press_key(Key::S, Modifiers::COMMAND);
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
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(inner);

        tree.press_key(Key::S, Modifiers::COMMAND);
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
                .primary(KeyStroke::command(Key::S))
                .propagate_when_disabled(false)
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(inner);

        tree.press_key(Key::S, Modifiers::COMMAND);
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
        // The histogram surfaces concrete widget types
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
        // A tap-driven `ctx.send_intent` must surface as
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
                } if modifiers.command()
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
                .primary(KeyStroke::command(Key::S))
                .enabled_when(enabled.clone())
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        // Disabled: keystroke falls through to on_key.
        tree.press_key(Key::S, Modifiers::COMMAND);
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
        tree.press_key(Key::S, Modifiers::COMMAND);
        assert!(action_fired.get(), "re-enabled shortcut must dispatch");
        assert!(
            !on_key_fired.get(),
            "enabled shortcut must consume the KeyDown"
        );
    }

    #[test]
    fn keyboard_capture_bypasses_shortcut() {
        use crate::action::Action;
        use crate::shortcut::{KeyStroke, Shortcut};
        use std::cell::Cell;
        use std::rc::Rc;

        // A focused keyboard-capture surface (e.g. a terminal) must receive
        // the accelerator chord itself (⌘S on macOS, Ctrl+S elsewhere), even
        // though an ENABLED global shortcut binds it — the whole point of
        // GAP 1. A non-capturing widget must yield to the shortcut (the
        // control case).
        fn run(capture: bool) -> (bool, bool) {
            let action_fired = Rc::new(Cell::new(false));
            let on_key_fired = Rc::new(Cell::new(false));
            let af = action_fired.clone();
            let kf = on_key_fired.clone();

            let mut tree = WidgetTree::new();
            let widget = tree.add(
                FillWidget::new()
                    .focusable()
                    .keyboard_capture(capture)
                    .on_key(move |event, _ctx| {
                        if matches!(
                            event,
                            WidgetEvent::KeyDown { key: Key::S, modifiers, .. } if modifiers.command()
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
                    .primary(KeyStroke::command(Key::S))
                    .build(),
            );

            tree.layout(SizeProposal::exact(100.0, 50.0));
            tree.focus(widget);
            tree.press_key(Key::S, Modifiers::COMMAND);
            (action_fired.get(), on_key_fired.get())
        }

        // Capture on: the shortcut is bypassed, the widget sees the key.
        let (action, on_key) = run(true);
        assert!(
            !action,
            "keyboard_capture must suppress the shortcut action"
        );
        assert!(on_key, "keyboard_capture must deliver the raw KeyDown");

        // Capture off (control): the shortcut consumes the key.
        let (action, on_key) = run(false);
        assert!(action, "without capture the shortcut must fire");
        assert!(!on_key, "without capture the widget must not see the key");
    }

    #[test]
    fn ctrl_tab_always_escapes_a_keyboard_capture_surface() {
        use std::cell::Cell;
        use std::rc::Rc;

        // WCAG 2.1.2. A capture surface answers `Handled` to every key —
        // that is what it is for — so the "cycle focus only when the focused
        // widget did not handle Tab" rule can never get focus out of one.
        // Ctrl+Tab / Ctrl+Shift+Tab are therefore reserved by the dispatcher
        // and never reach the widget at all.
        let saw_key = Rc::new(Cell::new(false));
        let sk = saw_key.clone();

        let mut tree = WidgetTree::new();
        let capture = tree.add(
            FillWidget::new()
                .focusable()
                .keyboard_capture(true)
                // The greediest possible handler: everything is consumed.
                .on_key(move |_event, _ctx| {
                    sk.set(true);
                    EventResponse::Handled
                }),
        );
        let neighbour = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Plain Tab stays inside: the widget consumed it (a terminal writes
        // it to the child as `\t`).
        tree.focus(capture);
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert!(saw_key.get(), "plain Tab must reach the capture surface");
        assert_eq!(
            tree.focused(),
            Some(capture),
            "plain Tab must not move focus off a capture surface"
        );

        // Ctrl+Tab escapes forward, without the widget ever seeing it.
        saw_key.set(false);
        tree.press_key(Key::Tab, Modifiers::CTRL);
        assert!(
            !saw_key.get(),
            "Ctrl+Tab is reserved and must not reach the capture surface"
        );
        assert_eq!(
            tree.focused(),
            Some(neighbour),
            "Ctrl+Tab must move focus out of a capture surface"
        );

        // And backwards.
        tree.focus(capture);
        tree.press_key(Key::Tab, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(
            tree.focused(),
            Some(neighbour),
            "Ctrl+Shift+Tab must move focus out of a capture surface"
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
                .primary(KeyStroke::command(Key::F))
                .scope(ShortcutScope::Scoped(scope_root))
                .on_activate(move |_ks, _ctx| {
                    activated_flag.set(true);
                    Intent::new("editor.find")
                })
                .build(),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(outside);

        tree.press_key(Key::F, Modifiers::COMMAND);
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
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        let handle = tree.begin_key_capture(move |ks, _reg, _ctx| cf.set(Some(ks)));
        assert!(tree.is_capturing_keys());

        tree.press_key(Key::S, Modifiers::COMMAND);
        assert_eq!(
            captured.get(),
            Some(KeyStroke::command(Key::S)),
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
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        // Arm capture: whatever chord comes next, rebind app.save to it.
        let _h = tree.begin_key_capture(|ks, reg, _ctx| {
            reg.rebind_primary("app.save", Some(ks));
        });

        tree.press_key(Key::B, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(
            tree.shortcut_registry()
                .effective("app.save")
                .unwrap()
                .primary,
            Some(KeyStroke::command_shift(Key::B))
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
                .primary(KeyStroke::command(Key::S))
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

        tree.press_key(Key::S, Modifiers::COMMAND);
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
        tree.press_key(Key::K, Modifiers::COMMAND);
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
        tree.press_key(Key::X, Modifiers::COMMAND);
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
    fn ctx_destroy_cancels_animations_and_bindings_via_deferred_path() {
        // Regression: `EventContext::destroy` queues
        // `TreeMutation::Destroy`, which used to be applied with the
        // bare `arena.destroy` — unlinking the node but leaking the
        // animation-scheduler entry (it holds a strong `Signal<f32>`
        // clone, so the widget kept animating after destruction) and
        // the widget's bindings. It must route through
        // `destroy_subtree` like every other destroy path does.
        use crate::binding::BindingLevel;
        use crate::signal::Signal;
        use std::time::{Duration, Instant};
        use teksilo_tokens::Easing;

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
        let widget = tree.add(BoundLeaf {
            tick: Signal::new(0_u64),
        });
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert!(tree.binding_registry().len() >= 1);

        // Seed an animation owned by the widget — exactly the strong
        // `Signal<f32>` clone the scheduler outlives the widget with.
        let anim = Signal::<f32>::new_animated(0.0);
        tree.animation_scheduler.animate(
            &anim,
            widget,
            1.0,
            Duration::from_secs(10),
            Easing::Linear,
            Instant::now(),
        );
        assert_eq!(tree.animation_scheduler.active_count(), 1);

        // Destroy via the deferred handler-time path.
        let mut noop = crate::window::NoopWindowOps;
        tree.run_with_event_context(&mut noop, |ctx| ctx.destroy(widget));

        assert_eq!(
            tree.animation_scheduler.active_count(),
            0,
            "ctx.destroy must cancel animations owned by the destroyed widget"
        );
        assert_eq!(
            tree.binding_registry().len(),
            0,
            "ctx.destroy must unregister the destroyed widget's bindings"
        );
        assert!(
            tree.arena.get(widget).is_none(),
            "node must be removed from the arena"
        );
    }

    #[test]
    fn clear_shortcut_override_via_event_context_restores_default() {
        use crate::shortcut::{KeyStroke, Shortcut};

        let mut tree = WidgetTree::new();
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
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
            Some(KeyStroke::command(Key::S))
        );
    }

    #[test]
    fn rebind_shortcut_primary_via_event_context() {
        use crate::shortcut::{KeyStroke, Shortcut};

        let mut tree = WidgetTree::new();
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
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
                .primary(KeyStroke::command(Key::K))
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

    /// A global action fires for an intent dispatched from a widget in a
    /// completely unrelated subtree — proving it is a position-independent
    /// fallback (the menu-bar-vs-content case).
    #[test]
    fn global_action_reached_from_unrelated_source() {
        use crate::action::Action;
        use crate::intent::Intent;
        use std::cell::Cell;
        use std::rc::Rc;

        #[derive(Debug)]
        struct Registrar(Rc<Cell<bool>>);
        impl crate::widget::Widget for Registrar {
            fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
                let flag = self.0.clone();
                ctx.register_action_global(
                    Action::new("test.global").on_invoke(move |_i, _c| flag.set(true)),
                );
                vec![]
            }
            fn layout_response(
                &self,
                _p: teksilo_canvas::SizeProposal,
                _c: &crate::widget::LayoutContext,
            ) -> crate::widget::LayoutResponse {
                teksilo_canvas::Size::new(0.0, 0.0).into()
            }
        }

        let mut tree = WidgetTree::new();
        let fired = Rc::new(Cell::new(false));
        let registrar = tree.add(Registrar(fired.clone()));
        let source = tree.add(FillWidget::new()); // unrelated sibling root
        let mut ops = crate::window::NoopWindowOps;

        tree.dispatch_intent(source, Intent::new("test.global"), true, &mut ops);
        assert!(
            fired.get(),
            "global action must fire from an unrelated source"
        );

        // And it is torn down with its owner.
        fired.set(false);
        tree.destroy_subtree(registrar);
        tree.dispatch_intent(source, Intent::new("test.global"), true, &mut ops);
        assert!(
            !fired.get(),
            "destroying the owner must remove its global action"
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
        let parent = tree.add(StackWidget::new().child(child));
        // Visually shift the entire subtree right by 100px.
        tree.set_transform(parent, teksilo_canvas::Transform2D::translate(100.0, 0.0));
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
        let parent = tree.add(StackWidget::new().child(child));
        // Halve the visual size: pre-transform bounds (0,0,100,50) →
        // visually (0,0,50,25).
        tree.set_transform(parent, teksilo_canvas::Transform2D::scale(0.5, 0.5));
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
        let inner = tree.add(StackWidget::new().child(leaf));
        let outer = tree.add(StackWidget::new().child(inner));
        // Outer translates by (100, 0); inner additionally scales by 2.
        // Effective at leaf = scale(2,2).then(translate(100,0)) — the
        // renderer composes deepest-first (see `effective_transform`).
        tree.set_transform(outer, teksilo_canvas::Transform2D::translate(100.0, 0.0));
        tree.set_transform(inner, teksilo_canvas::Transform2D::scale(2.0, 2.0));
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
        tree.set_transform(widget, teksilo_canvas::Transform2D::IDENTITY);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert_eq!(tree.hit_test(Point::new(50.0, 25.0)), Some(widget));
    }

    #[test]
    fn arena_effective_transform_composes_ancestors() {
        // `arena.effective_transform(id)` must equal the renderer's
        // transform-stack top by the time it begins painting `id` —
        // i.e. mapping `id`'s pre-transform local point to screen space.
        // The renderer's `PushTransform` handler composes as
        // `device_t.then(prev_top)` (see `teksilo-render/src/renderer.rs`),
        // so the *innermost* transform applies first to a local point.
        // For ancestors [outer, inner] both with transforms, this means
        // effective = inner.then(outer), NOT outer.then(inner).
        // teksilo-scene relies on this to project scene-coord bounds to
        // screen space when emitting AT nodes for view-transformed items.
        use crate::test_widgets::StackWidget;
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let inner = tree.add(StackWidget::new().child(leaf));
        let outer = tree.add(StackWidget::new().child(inner));
        tree.set_transform(outer, teksilo_canvas::Transform2D::translate(100.0, 0.0));
        tree.set_transform(inner, teksilo_canvas::Transform2D::scale(2.0, 2.0));
        tree.layout(SizeProposal::exact(50.0, 25.0));

        let eff = tree.arena.effective_transform(leaf);
        let expected = teksilo_canvas::Transform2D::scale(2.0, 2.0)
            .then(&teksilo_canvas::Transform2D::translate(100.0, 0.0));
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
            teksilo_canvas::Size::new(100.0, 40.0).into()
        }
    }

    // The keyboard route to a context menu.
    //
    // Until this existed there was none at all: no `Key::ContextMenu`, no
    // Shift+F10, and `Action::ShowContextMenu` appears in zero of the three
    // AccessKit adapters, so the assistive-technology route is dead on every
    // platform too. A menu reachable only by right-click is a menu a keyboard
    // user does not have.

    /// A widget that hands the keyboard a different target than itself, the way
    /// every data view does: the container has focus, the row is what the menu
    /// is about.
    #[derive(Debug)]
    struct NominatingWidget {
        row: std::cell::Cell<Option<WidgetId>>,
    }

    impl crate::widget::Widget for NominatingWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(50.0, 20.0).into()
        }

        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            ctx.apply_self_handlers(crate::widget_builder::HandlerSet::new().focusable(true));
            Vec::new()
        }

        fn context_menu_key_target(&self) -> Option<WidgetId> {
            self.row.get()
        }
    }

    fn press(tree: &mut WidgetTree, key: Key, modifiers: Modifiers) {
        tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
    }

    #[test]
    fn the_context_menu_key_opens_the_focused_widget_menu() {
        use std::cell::Cell;
        use std::rc::Rc;

        let opened = Rc::new(Cell::new(false));
        let flag = opened.clone();
        let mut tree = WidgetTree::new();
        let widget = tree.add(
            FillWidget::new()
                .focusable()
                .context_menu(move |_pos, _ctx| {
                    flag.set(true);
                    Some(Box::new(StubMenu) as Box<dyn crate::widget::Widget>)
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(widget);

        press(&mut tree, Key::ContextMenu, Modifiers::NONE);
        assert!(opened.get(), "the dedicated Menu key must open the menu");
    }

    /// The chord every Windows and Linux keyboard can reach, including the many
    /// that have no dedicated Menu key at all.
    #[test]
    fn shift_f10_opens_the_focused_widget_menu() {
        use std::cell::Cell;
        use std::rc::Rc;

        let opened = Rc::new(Cell::new(false));
        let flag = opened.clone();
        let mut tree = WidgetTree::new();
        let widget = tree.add(
            FillWidget::new()
                .focusable()
                .context_menu(move |_pos, _ctx| {
                    flag.set(true);
                    Some(Box::new(StubMenu) as Box<dyn crate::widget::Widget>)
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(widget);

        press(&mut tree, Key::F10, Modifiers::SHIFT);
        assert!(opened.get(), "Shift+F10 must open the menu");
    }

    /// Modifiers are matched exactly. Ctrl+Shift+F10 is a different gesture and
    /// belongs to the application.
    #[test]
    fn a_near_miss_chord_is_not_a_context_menu_request() {
        use std::cell::Cell;
        use std::rc::Rc;

        let opened = Rc::new(Cell::new(false));
        let flag = opened.clone();
        let mut tree = WidgetTree::new();
        let widget = tree.add(
            FillWidget::new()
                .focusable()
                .context_menu(move |_pos, _ctx| {
                    flag.set(true);
                    Some(Box::new(StubMenu) as Box<dyn crate::widget::Widget>)
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(widget);

        press(&mut tree, Key::F10, Modifiers::SHIFT | Modifiers::CTRL);
        press(&mut tree, Key::F10, Modifiers::NONE);
        assert!(!opened.get(), "only Shift+F10 exactly asks for a menu");
    }

    /// The correction the design needed. A data view is focusable and its rows
    /// are not, so "the focused widget" is the list, and the menu a user asked
    /// for on row 4 would have been the list's own.
    #[test]
    fn the_keyboard_target_can_be_a_row_rather_than_the_focused_container() {
        use std::cell::Cell;
        use std::rc::Rc;

        let menu_owner = Rc::new(Cell::new(None::<&'static str>));

        let row_flag = menu_owner.clone();
        let container_flag = menu_owner.clone();

        let mut tree = WidgetTree::new();
        let row = tree.add(FillWidget::new().context_menu(move |_pos, _ctx| {
            row_flag.set(Some("row"));
            Some(Box::new(StubMenu) as Box<dyn crate::widget::Widget>)
        }));
        let container = tree.add(
            crate::test_widgets::StackWidget::new()
                .child(row)
                .context_menu(move |_pos, _ctx| {
                    container_flag.set(Some("container"));
                    Some(Box::new(StubMenu) as Box<dyn crate::widget::Widget>)
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // The container is focused, and nominates the row.
        let nominator = tree.add(NominatingWidget {
            row: std::cell::Cell::new(Some(row)),
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(nominator);
        let _ = container;

        press(&mut tree, Key::ContextMenu, Modifiers::NONE);
        assert_eq!(
            menu_owner.get(),
            Some("row"),
            "the nominated row's factory must be the one that runs"
        );
    }

    /// Nothing on the chain owns a factory, so the framework must not swallow
    /// the key: a widget that wants to handle Shift+F10 itself still can.
    #[test]
    fn the_chord_falls_through_when_there_is_no_menu_to_show() {
        use std::cell::Cell;
        use std::rc::Rc;

        let saw_key = Rc::new(Cell::new(false));
        let flag = saw_key.clone();
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable().on_key(move |ev, _ctx| {
            if matches!(ev, WidgetEvent::KeyDown { key: Key::F10, .. }) {
                flag.set(true);
            }
            crate::event::EventResponse::Ignored
        }));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(widget);

        press(&mut tree, Key::F10, Modifiers::SHIFT);
        assert!(
            saw_key.get(),
            "with no factory anywhere, the key must reach the widget"
        );
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
        tree.dispatch_event(WidgetEvent::pointer_down(
            click,
            PointerButton::Secondary,
            Modifiers::NONE,
        ));

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
        let _outer = tree.add(
            StackWidget::new()
                .child(inner)
                .context_menu(move |_pos, _ctx| {
                    outer_flag.set(outer_flag.get() + 1);
                    Some(Box::new(StubMenu) as Box<dyn crate::widget::Widget>)
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(50.0, 25.0),
            PointerButton::Secondary,
            Modifiers::NONE,
        ));

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
        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(50.0, 25.0),
            PointerButton::Secondary,
            Modifiers::NONE,
        ));
        let overlay_count_after = tree.overlay_manager.len();
        assert_eq!(
            overlay_count_before, overlay_count_after,
            "a factory returning None must not mount any overlay"
        );
    }

    // ---- Reconcile-on-rebuild (`preserves_children_on_rebuild`) ----------
    //
    // These pin the contract that the preserve path RECONCILES: it keeps the
    // children a rebuild re-attaches (and any subtree re-parented into the new
    // tree) while reaping the ones it drops — so memoizing widgets are both
    // stateful and leak-free. Regression guard for the orphan-leak the old
    // "preserve = destroy nothing" behaviour caused.

    /// `build()` mints a fresh child every time and returns only it, abandoning
    /// the previous one. Used to prove dropped children are reaped, not leaked.
    #[derive(Debug)]
    struct FreshChildHost {
        preserve: bool,
    }
    impl Widget for FreshChildHost {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            vec![ctx.add(FillWidget::new())]
        }
        fn layout_response(
            &self,
            p: SizeProposal,
            _c: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
        fn preserves_children_on_rebuild(&self) -> bool {
            self.preserve
        }
    }

    #[test]
    fn reconcile_reaps_dropped_children_no_leak() {
        // preserve=false (destroy-all) and preserve=true (reconcile) must BOTH
        // keep the arena bounded when a rebuild drops its old child. Before the
        // reconcile fix, preserve=true grew the arena (and the active set) by
        // one stranded orphan per rebuild.
        for preserve in [false, true] {
            let mut tree = WidgetTree::new();
            let host = tree.add(FreshChildHost { preserve });
            tree.layout(SizeProposal::exact(100.0, 100.0));
            let total0 = tree.arena.len();
            let active0 = tree.active_widget_count();
            for _ in 0..5 {
                tree.arena_mark_needs_rebuild_for_testing(host);
                tree.layout(SizeProposal::exact(100.0, 100.0));
            }
            assert_eq!(
                tree.arena.len(),
                total0,
                "preserve={preserve}: dropped children must be reaped, not leaked"
            );
            assert_eq!(
                tree.active_widget_count(),
                active0,
                "preserve={preserve}: no stranded still-active orphans"
            );
        }
    }

    /// `build()` mints one **detached** node every time — the shape of every
    /// pre-built popup in the widget crate (a dropdown, a calendar, a
    /// tooltip's cascade children): parked dormant, shown later through an
    /// overlay, and deliberately not a child, since activation and paint both
    /// descend through `children`.
    #[derive(Debug)]
    struct DetachedContentHost {
        preserve: bool,
    }
    impl Widget for DetachedContentHost {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            let popup = ctx.add_detached(FillWidget::new());
            ctx.set_dormant(popup);
            vec![ctx.add(FillWidget::new())]
        }
        fn layout_response(
            &self,
            p: SizeProposal,
            _c: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
        fn preserves_children_on_rebuild(&self) -> bool {
            self.preserve
        }
    }

    #[test]
    fn rebuilding_reaps_detached_content_no_leak() {
        // A parentless node is reachable from no walk at all — not the child
        // teardown, not the accessibility tree, not `active_widget_count`. Held
        // by a bare `ctx.add` it simply accumulated: one stranded popup per
        // rebuild, for the lifetime of the process. `add_detached` records the
        // ownership edge that makes it reapable.
        for preserve in [false, true] {
            let mut tree = WidgetTree::new();
            let host = tree.add(DetachedContentHost { preserve });
            tree.layout(SizeProposal::exact(100.0, 100.0));
            let total0 = tree.arena.len();
            for _ in 0..5 {
                tree.arena_mark_needs_rebuild_for_testing(host);
                tree.layout(SizeProposal::exact(100.0, 100.0));
            }
            assert_eq!(
                tree.arena.len(),
                total0,
                "preserve={preserve}: the previous build's detached content must be reaped"
            );
        }
    }

    #[test]
    fn destroying_a_host_reaps_its_detached_content() {
        let mut tree = WidgetTree::new();
        let outer = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let empty = tree.arena.len();

        let host = tree.add_child(outer, DetachedContentHost { preserve: false });
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert!(tree.arena.len() > empty);

        tree.destroy_subtree(host);
        assert_eq!(
            tree.arena.len(),
            empty,
            "the popup must die with the widget that built it"
        );
    }

    /// Memoizes one child and re-attaches the same id every build.
    #[derive(Debug)]
    struct StableChildHost {
        child: Option<WidgetId>,
        probe: std::rc::Rc<std::cell::Cell<Option<WidgetId>>>,
    }
    impl Widget for StableChildHost {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            let id = match self.child {
                Some(id) => id,
                None => {
                    let id = ctx.add(FillWidget::new());
                    self.child = Some(id);
                    self.probe.set(Some(id));
                    id
                }
            };
            vec![id]
        }
        fn layout_response(
            &self,
            p: SizeProposal,
            _c: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
        fn preserves_children_on_rebuild(&self) -> bool {
            true
        }
    }

    #[test]
    fn reconcile_preserves_reattached_child() {
        let probe = std::rc::Rc::new(std::cell::Cell::new(None));
        let mut tree = WidgetTree::new();
        let host = tree.add(StableChildHost {
            child: None,
            probe: probe.clone(),
        });
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let child = probe.get().expect("child mounted");
        let total0 = tree.arena.len();
        for _ in 0..5 {
            tree.arena_mark_needs_rebuild_for_testing(host);
            tree.layout(SizeProposal::exact(100.0, 100.0));
        }
        assert!(
            tree.arena.is_active(child),
            "the re-attached child must survive every rebuild"
        );
        assert_eq!(tree.arena.len(), total0, "no growth — same child reused");
    }

    /// Re-homes a node returned from its `build()` under itself.
    #[derive(Debug)]
    struct Wrapper {
        child: WidgetId,
    }
    impl Widget for Wrapper {
        fn build(&mut self, _ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            vec![self.child]
        }
        fn layout_response(
            &self,
            p: SizeProposal,
            _c: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
    }

    /// Memoizes a body, then wraps it in a FRESH `Wrapper` each build —
    /// re-parenting the body out of the previous (now dropped) wrapper. This is
    /// the TabWidget / CompositeTooltip pattern in miniature.
    #[derive(Debug)]
    struct ReparentHost {
        body: Option<WidgetId>,
        probe: std::rc::Rc<std::cell::Cell<Option<WidgetId>>>,
    }
    impl Widget for ReparentHost {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            let body = match self.body {
                Some(id) => id,
                None => {
                    let id = ctx.add(FillWidget::new());
                    self.body = Some(id);
                    self.probe.set(Some(id));
                    id
                }
            };
            vec![ctx.add(Wrapper { child: body })]
        }
        fn layout_response(
            &self,
            p: SizeProposal,
            _c: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
        fn preserves_children_on_rebuild(&self) -> bool {
            true
        }
    }

    #[test]
    fn reconcile_spares_reparented_survivor() {
        // The memoized body is re-parented into a fresh wrapper each rebuild;
        // the old wrapper is dropped. The body must survive (it is re-homed),
        // and the old wrappers must be reaped (no leak). This is the exact
        // failure that destroyed TabWidget's static panel before the fix: the
        // parent-authoritative recursion + single-node arena removal spare the
        // re-homed body while still reaping the dropped wrapper subtree.
        let probe = std::rc::Rc::new(std::cell::Cell::new(None));
        let mut tree = WidgetTree::new();
        let host = tree.add(ReparentHost {
            body: None,
            probe: probe.clone(),
        });
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let body = probe.get().expect("body mounted");
        let total0 = tree.arena.len();
        for _ in 0..5 {
            tree.arena_mark_needs_rebuild_for_testing(host);
            tree.layout(SizeProposal::exact(100.0, 100.0));
        }
        assert!(
            tree.arena.is_active(body),
            "the re-parented body must survive — it was moved into the new tree, \
             not swept with the dropped wrapper"
        );
        assert_eq!(
            tree.arena.len(),
            total0,
            "dropped wrappers reaped — no per-rebuild leak"
        );
    }

    // -----------------------------------------------------------------
    // EventContext::ensure_visible / ensure_widget_visible — the
    // rect/id-based outer-scroll chase drained in `collect_from_ctx`.
    // -----------------------------------------------------------------

    /// A `clips_children` container that places its single child at a fixed
    /// vertical offset — used to give a child arena bounds *outside* the
    /// container's viewport so the id-based `ensure_widget_visible` walk has a
    /// reason to dispatch `ScrollIntoView`.
    #[derive(Debug)]
    struct BelowContainer {
        child: Option<WidgetId>,
        offset: f32,
    }

    impl crate::widget::Widget for BelowContainer {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &crate::widget::LayoutContext,
        ) {
            for c in children.iter_mut() {
                c.origin = Point::new(bounds.x, bounds.y + self.offset);
                c.size = bounds.size();
            }
        }
        fn children(&self) -> Vec<WidgetId> {
            self.child.into_iter().collect()
        }
    }

    /// A `clips_children` container that records the `ScrollIntoView` it
    /// receives, so a test can assert what the framework dispatched to it.
    fn recording_scroll_container(
        tree: &mut WidgetTree,
        child: WidgetId,
        recorded: std::rc::Rc<std::cell::Cell<Option<Rect>>>,
    ) -> WidgetId {
        use crate::test_widgets::StackWidget;
        tree.add(
            StackWidget::new()
                .child(child)
                .on_scroll(move |ev, _ctx| match ev {
                    WidgetEvent::ScrollIntoView { target_bounds, .. } => {
                        recorded.set(Some(*target_bounds));
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                })
                .clips_children(true),
        )
    }

    #[test]
    fn ensure_visible_dispatches_scroll_into_view_to_clipping_ancestor() {
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<Rect>>> = Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let _container = recording_scroll_container(&mut tree, actor, recorded.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // A rect well below the 100px viewport — the container must be asked to
        // reveal it.
        let target = Rect::new(10.0, 500.0, 20.0, 15.0);
        let mut ctx = EventContext::new();
        ctx.ensure_visible(target);
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            recorded.get(),
            Some(target),
            "ensure_visible(rect) must dispatch ScrollIntoView with the exact rect \
             to the clips_children ancestor"
        );
    }

    #[test]
    fn ensure_visible_is_noop_when_rect_already_visible() {
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<Rect>>> = Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let _container = recording_scroll_container(&mut tree, actor, recorded.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Fully inside the viewport → the ancestor already shows it, so no
        // ScrollIntoView is dispatched.
        let mut ctx = EventContext::new();
        ctx.ensure_visible(Rect::new(10.0, 10.0, 20.0, 15.0));
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            recorded.get(),
            None,
            "a rect already inside the viewport must not trigger a scroll"
        );
    }

    #[test]
    fn ensure_visible_margin_forces_scroll_near_edge() {
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<Rect>>> = Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let _container = recording_scroll_container(&mut tree, actor, recorded.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Rect at y=95..99 is visible at margin 0, but with a 10px margin its
        // padded bottom (109) spills past the 100px viewport → scroll.
        let rect = Rect::new(10.0, 95.0, 20.0, 4.0);
        let mut ctx = EventContext::new();
        ctx.ensure_visible_with_margin(rect, 10.0);
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            recorded.get(),
            Some(rect),
            "the margin must widen the visibility test so a near-edge rect scrolls"
        );
    }

    /// A `clips_children` container that records the alignment and motion of the
    /// `ScrollIntoView` it receives.
    fn recording_align_container(
        tree: &mut WidgetTree,
        child: WidgetId,
        recorded: std::rc::Rc<
            std::cell::Cell<Option<(crate::event::ScrollAlign, crate::event::ScrollMotion)>>,
        >,
    ) -> WidgetId {
        use crate::test_widgets::StackWidget;
        tree.add(
            StackWidget::new()
                .child(child)
                .on_scroll(move |ev, _ctx| match ev {
                    WidgetEvent::ScrollIntoView { align, motion, .. } => {
                        recorded.set(Some((*align, *motion)));
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                })
                .clips_children(true),
        )
    }

    #[test]
    fn ensure_visible_aligned_scrolls_even_when_already_visible() {
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<Rect>>> = Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let _container = recording_scroll_container(&mut tree, actor, recorded.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Comfortably inside the viewport — a *minimal* reveal would decline
        // (see `ensure_visible_is_noop_when_rect_already_visible`). A pin must
        // still fire: re-asserting unconditionally is the whole difference
        // between "keep it on screen" and "hold it at this height".
        let target = Rect::new(10.0, 10.0, 20.0, 15.0);
        let mut ctx = EventContext::new();
        ctx.ensure_visible_aligned(target, 0.5, crate::event::ScrollMotion::Instant);
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            recorded.get(),
            Some(target),
            "an aligned reveal must dispatch even when the rect is already visible"
        );
    }

    #[test]
    fn ensure_visible_aligned_forwards_fraction_and_motion() {
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<(crate::event::ScrollAlign, crate::event::ScrollMotion)>>> =
            Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let _container = recording_align_container(&mut tree, actor, recorded.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let mut ctx = EventContext::new();
        ctx.ensure_visible_aligned(
            Rect::new(10.0, 10.0, 20.0, 15.0),
            0.25,
            crate::event::ScrollMotion::Smooth,
        );
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            recorded.get(),
            Some((
                crate::event::ScrollAlign::Fraction(0.25),
                crate::event::ScrollMotion::Smooth
            )),
            "the container must receive the requested fraction and motion verbatim"
        );
    }

    #[test]
    fn ensure_visible_aligned_clamps_the_fraction() {
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<(crate::event::ScrollAlign, crate::event::ScrollMotion)>>> =
            Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let _container = recording_align_container(&mut tree, actor, recorded.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let mut ctx = EventContext::new();
        ctx.ensure_visible_aligned(
            Rect::new(10.0, 10.0, 20.0, 15.0),
            4.2,
            crate::event::ScrollMotion::Instant,
        );
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            recorded.get().map(|(a, _)| a),
            Some(crate::event::ScrollAlign::Fraction(1.0)),
            "an out-of-range fraction must clamp rather than aim the pin off-screen"
        );
    }

    #[test]
    fn plain_ensure_visible_requests_minimal_alignment() {
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<(crate::event::ScrollAlign, crate::event::ScrollMotion)>>> =
            Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let _container = recording_align_container(&mut tree, actor, recorded.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let mut ctx = EventContext::new();
        ctx.ensure_visible(Rect::new(10.0, 500.0, 20.0, 15.0));
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            recorded.get(),
            Some((
                crate::event::ScrollAlign::Minimal,
                crate::event::ScrollMotion::Instant
            )),
            "the pre-existing reveal API must keep its exact semantics"
        );
    }

    #[test]
    fn only_the_innermost_container_aligns() {
        use std::cell::Cell;
        use std::rc::Rc;
        let inner_rec: Rc<Cell<Option<(crate::event::ScrollAlign, crate::event::ScrollMotion)>>> =
            Rc::new(Cell::new(None));
        let outer_rec: Rc<Cell<Option<(crate::event::ScrollAlign, crate::event::ScrollMotion)>>> =
            Rc::new(Cell::new(None));

        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let inner = recording_align_container(&mut tree, actor, inner_rec.clone());
        let _outer = recording_align_container(&mut tree, inner, outer_rec.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Off-screen, so the outer container is asked too (a `Minimal` request
        // is gated on visibility).
        let mut ctx = EventContext::new();
        ctx.ensure_visible_aligned(
            Rect::new(10.0, 500.0, 20.0, 15.0),
            0.5,
            crate::event::ScrollMotion::Instant,
        );
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            inner_rec.get().map(|(a, _)| a),
            Some(crate::event::ScrollAlign::Fraction(0.5)),
            "the innermost clipping ancestor owns the pin"
        );
        assert_eq!(
            outer_rec.get().map(|(a, _)| a),
            Some(crate::event::ScrollAlign::Minimal),
            "an outer container must only bring the inner viewport into view — a \
             fraction names a height in one viewport, not in every ancestor's"
        );
    }

    #[test]
    fn a_clipper_that_does_not_scroll_passes_the_pin_on() {
        // The shape of every capped text column: the editor inside a `MaxSize`
        // (which clips once a cap is set, and has no `on_scroll`) inside the
        // page's scroll area. The wrapper cannot act on a `ScrollIntoView`, so
        // it must not be the one the pin is spent on. Otherwise the scroll
        // area is handed a plain reveal and typewriter scrolling silently
        // becomes ordinary caret-following.
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;
        let outer_rec: Rc<Cell<Option<(crate::event::ScrollAlign, crate::event::ScrollMotion)>>> =
            Rc::new(Cell::new(None));

        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let clipper = tree.add(StackWidget::new().child(actor).clips_children_on(true));
        let _scroller = recording_align_container(&mut tree, clipper, outer_rec.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Inside the viewport, so only a pin (never a minimal reveal) reaches
        // the scroller at all.
        let mut ctx = EventContext::new();
        ctx.ensure_visible_aligned(
            Rect::new(10.0, 10.0, 20.0, 15.0),
            0.5,
            crate::event::ScrollMotion::Instant,
        );
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            outer_rec.get().map(|(a, _)| a),
            Some(crate::event::ScrollAlign::Fraction(0.5)),
            "a clipping wrapper with no scroll handler must pass the pin through \
             to the scroll container above it"
        );
    }

    #[test]
    fn a_scroller_that_declines_to_move_still_holds_the_pin() {
        // What the scroller does with the pin is its own business: a `SceneView`
        // already holding the target where asked answers `Ignored`, since it
        // moved nothing. That is not an invitation for the next container out
        // to align a rectangle inside a viewport it does not own.
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;
        let inner_rec: Rc<Cell<Option<crate::event::ScrollAlign>>> = Rc::new(Cell::new(None));
        let outer_rec: Rc<Cell<Option<(crate::event::ScrollAlign, crate::event::ScrollMotion)>>> =
            Rc::new(Cell::new(None));

        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let rec = inner_rec.clone();
        let inner = tree.add(
            StackWidget::new()
                .child(actor)
                .on_scroll(move |ev, _ctx| match ev {
                    WidgetEvent::ScrollIntoView { align, .. } => {
                        rec.set(Some(*align));
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                })
                .clips_children(true),
        );
        let _outer = recording_align_container(&mut tree, inner, outer_rec.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Off-screen, so the outer container is asked too.
        let mut ctx = EventContext::new();
        ctx.ensure_visible_aligned(
            Rect::new(10.0, 500.0, 20.0, 15.0),
            0.5,
            crate::event::ScrollMotion::Instant,
        );
        tree.collect_from_ctx(ctx, actor);

        assert_eq!(
            inner_rec.get(),
            Some(crate::event::ScrollAlign::Fraction(0.5)),
            "the innermost scroller is offered the pin"
        );
        assert_eq!(
            outer_rec.get().map(|(a, _)| a),
            Some(crate::event::ScrollAlign::Minimal),
            "declining to move must not pass the pin outward"
        );
    }

    #[test]
    fn ensure_widget_visible_uses_target_arena_bounds() {
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<Rect>>> = Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        // Target lives 500px below the container's top — off the viewport.
        let target = tree.add(FillWidget::new());
        let rec = recorded.clone();
        let container = tree.add(
            BelowContainer {
                child: Some(target),
                offset: 500.0,
            }
            .on_scroll(move |ev, _ctx| match ev {
                WidgetEvent::ScrollIntoView { target_bounds, .. } => {
                    rec.set(Some(*target_bounds));
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            })
            .clips_children(true),
        );
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let expected = tree.bounds(target);
        assert!(
            expected.y > 100.0,
            "fixture sanity: the target must sit below the viewport (y={})",
            expected.y
        );

        // The source widget is irrelevant for the id-based walk — it starts
        // from the *target's* parent — so pass the container itself.
        let mut ctx = EventContext::new();
        ctx.ensure_widget_visible(target);
        tree.collect_from_ctx(ctx, container);

        assert_eq!(
            recorded.get(),
            Some(expected),
            "ensure_widget_visible(id) must dispatch ScrollIntoView with the \
             target's current arena bounds"
        );
    }

    #[test]
    fn ensure_widget_visible_ignores_missing_widget() {
        // A never-mounted id must neither panic nor dispatch a spurious scroll.
        use std::cell::Cell;
        use std::rc::Rc;
        let recorded: Rc<Cell<Option<Rect>>> = Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let actor = tree.add(FillWidget::new());
        let _container = recording_scroll_container(&mut tree, actor, recorded.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let mut ctx = EventContext::new();
        ctx.ensure_widget_visible(WidgetId::default());
        tree.collect_from_ctx(ctx, actor); // must not panic

        assert_eq!(
            recorded.get(),
            None,
            "an unmounted id must not trigger a scroll"
        );
    }

    #[test]
    fn context_menu_inside_a_modal_keeps_the_modal() {
        // Regression: right-clicking a widget that lives inside an open modal must
        // open its context menu WITHOUT tearing down the modal. `show_context_menu_for`
        // used to `dismiss_all()`, which closed the very overlay hosting the editor.
        use crate::event::{Modifiers, PointerButton, WidgetEvent};
        use crate::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
        use crate::test_widgets::{FillWidget, StackWidget};

        let mut tree = WidgetTree::new();
        // A container standing in for the modal's content subtree, with the editor
        // (a right-clickable widget) inside it.
        let modal_content = tree.add(StackWidget::new());
        let _editor = tree.add_child(
            modal_content,
            FillWidget::new()
                .context_menu(|_pos, _ctx| Some(Box::new(FillWidget::new()) as Box<dyn Widget>)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let modal = tree.overlay_manager.show(OverlayRequest {
            content_id: modal_content,
            anchor: modal_content,
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::EscapeKey,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        // Give the overlay real bounds so the right-click hit-tests inside it.
        tree.overlay_manager
            .stack
            .iter_mut()
            .find(|o| o.id == modal)
            .unwrap()
            .bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        assert_eq!(tree.overlay_manager.len(), 1);

        // Right-click the editor inside the modal.
        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(50.0, 25.0),
            PointerButton::Secondary,
            Modifiers::NONE,
        ));

        assert!(
            tree.overlay_manager.active_ids().contains(&modal),
            "the modal must survive opening a context menu inside it"
        );
        assert_eq!(
            tree.overlay_manager.len(),
            2,
            "the context menu should now be open on top of the surviving modal"
        );
    }
}

/// The stage-1 input ingress: the two sample doors, the scroll routing rule,
/// and the guarantee that a mouse still behaves exactly as it did.
#[cfg(test)]
mod input_ingress_tests {
    use super::*;
    use crate::event::{EventResponse, Modifiers, PointerButton, ScrollDelta};
    use crate::pointer::{
        EventTime, PointerId, PointerInfo, PointerPhase, PointerSample, ScrollPhase, ScrollSample,
        ScrollSource,
    };
    use crate::test_widgets::FillWidget;
    use crate::widget::{LayoutContext, WidgetPlacement};
    use crate::widget_builder::WidgetBuilder;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A container that lays children out side by side across its bounds, so a
    /// hit test at a given x picks a specific child.
    ///
    /// Local rather than shared: the common `StackWidget` deliberately stacks
    /// its children at one origin, which is the opposite of what a routing test
    /// needs.
    #[derive(Debug)]
    struct RowWidget {
        children: Vec<WidgetId>,
    }

    impl crate::widget::Widget for RowWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            let n = children.len().max(1) as f32;
            let w = bounds.width / n;
            for (i, child) in children.iter_mut().enumerate() {
                child.origin = Point::new(bounds.x + w * i as f32, bounds.y);
                child.size = teksilo_canvas::Size::new(w, bounds.height);
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            self.children.clone()
        }
    }

    /// Everything a widget observes of a pointer interaction, as text, so two
    /// runs can be compared for exact equality rather than field by field.
    fn record(drive: impl FnOnce(&mut WidgetTree)) -> Vec<String> {
        let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        let mut tree = WidgetTree::new();
        let pointer_log = log.clone();
        let hover_log = log.clone();
        tree.add(
            FillWidget::new()
                .on_pointer_event(move |event, _ctx| {
                    pointer_log.borrow_mut().push(format!("{event:?}"));
                    EventResponse::Ignored
                })
                .on_hover(move |entered, _ctx| {
                    hover_log.borrow_mut().push(format!("hover({entered})"));
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 100.0));

        drive(&mut tree);
        log.borrow().clone()
    }

    /// The load-bearing compatibility claim of this package: lowering a mouse
    /// `PointerSample` produces the *same* `WidgetEvent` stream, in the same
    /// order, as writing the events out by hand. If this ever diverges, the
    /// door has started meaning something different from the events it lowers
    /// to.
    #[test]
    fn a_mouse_sample_reproduces_todays_event_stream() {
        let inside = Point::new(40.0, 40.0);
        let moved = Point::new(60.0, 55.0);
        let outside = Point::new(400.0, 400.0);

        let legacy = record(|tree| {
            tree.dispatch_event(WidgetEvent::pointer_move(inside));
            tree.dispatch_event(WidgetEvent::pointer_down(
                inside,
                PointerButton::Primary,
                Modifiers::NONE,
            ));
            tree.dispatch_event(WidgetEvent::pointer_move(moved));
            tree.dispatch_event(WidgetEvent::pointer_up(
                moved,
                PointerButton::Primary,
                Modifiers::NONE,
            ));
            tree.dispatch_event(WidgetEvent::pointer_move(outside));
        });

        let sampled = record(|tree| {
            let t = EventTime::ZERO;
            tree.dispatch_pointer(PointerSample::mouse(PointerPhase::Move, inside, t));
            tree.dispatch_pointer(
                PointerSample::mouse(PointerPhase::Down, inside, t)
                    .with_button(PointerButton::Primary),
            );
            tree.dispatch_pointer(PointerSample::mouse(PointerPhase::Move, moved, t));
            tree.dispatch_pointer(
                PointerSample::mouse(PointerPhase::Up, moved, t)
                    .with_button(PointerButton::Primary),
            );
            tree.dispatch_pointer(PointerSample::mouse(PointerPhase::Move, outside, t));
        });

        assert_eq!(legacy, sampled);
        assert!(
            legacy.contains(&"hover(true)".to_string())
                && legacy.contains(&"hover(false)".to_string()),
            "the fixture must actually exercise enter and leave: {legacy:?}"
        );
    }

    /// A press with no button — what a bare direct-pointer contact reports —
    /// still reads as the primary press, because that is what a tap has always
    /// been.
    #[test]
    fn a_buttonless_press_lowers_to_primary() {
        let events = record(|tree| {
            tree.dispatch_pointer(PointerSample::mouse(
                PointerPhase::Down,
                Point::new(20.0, 20.0),
                EventTime::ZERO,
            ));
        });
        assert!(
            events.iter().any(|e| e.contains("button: Primary")),
            "{events:?}"
        );
    }

    // --- scroll routing --------------------------------------------------

    /// Two leaves side by side across a 200-wide tree: `top` owns x < 100,
    /// `bottom` owns x >= 100.
    fn scroll_fixture() -> (WidgetTree, Rc<RefCell<Vec<&'static str>>>) {
        let hits: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();

        let leading_hits = hits.clone();
        let trailing_hits = hits.clone();
        let leading = tree.add(FillWidget::new().on_scroll(move |_event, _ctx| {
            leading_hits.borrow_mut().push("leading");
            EventResponse::Handled
        }));
        let trailing = tree.add(FillWidget::new().on_scroll(move |_event, _ctx| {
            trailing_hits.borrow_mut().push("trailing");
            EventResponse::Handled
        }));
        tree.add(RowWidget {
            children: vec![leading, trailing],
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));
        (tree, hits)
    }

    fn notch() -> ScrollDelta {
        ScrollDelta::Lines { x: 0.0, y: -1.0 }
    }

    /// The mouse path: a wheel notch carries no position, so it routes by
    /// hover exactly as it always has. This is the no-op claim.
    #[test]
    fn a_positionless_scroll_routes_by_hover() {
        let (mut tree, hits) = scroll_fixture();

        tree.pointer_move(Point::new(150.0, 50.0)); // hover the trailing leaf
        tree.dispatch_event(WidgetEvent::scroll(notch(), Modifiers::NONE));
        assert_eq!(*hits.borrow(), vec!["trailing"]);

        tree.pointer_move(Point::new(50.0, 50.0)); // hover the leading leaf
        tree.dispatch_scroll(ScrollSample::wheel(
            notch(),
            Modifiers::NONE,
            EventTime::ZERO,
        ));
        assert_eq!(*hits.borrow(), vec!["trailing", "leading"]);
    }

    /// A positioned scroll routes by hit test, *against* the hover. This is
    /// the only thing that makes a synthesised touch pan routable at all: a
    /// contact never writes hover, so hover would send the pan to whatever the
    /// mouse last touched — or nowhere.
    #[test]
    fn a_positioned_scroll_routes_by_hit_test() {
        let (mut tree, hits) = scroll_fixture();
        tree.pointer_move(Point::new(150.0, 50.0)); // hover the TRAILING leaf

        tree.dispatch_event(WidgetEvent::scroll_at(
            notch(),
            Modifiers::NONE,
            Point::new(50.0, 50.0), // …but scroll over the LEADING one
        ));
        assert_eq!(*hits.borrow(), vec!["leading"]);

        tree.dispatch_scroll(
            ScrollSample::wheel(notch(), Modifiers::NONE, EventTime::ZERO)
                .at(Point::new(150.0, 50.0)),
        );
        assert_eq!(*hits.borrow(), vec!["leading", "trailing"]);
    }

    /// With nothing hovered and nothing focused a positionless scroll goes
    /// nowhere — the pre-existing behaviour, pinned rather than left
    /// incidental.
    #[test]
    fn a_positionless_scroll_with_no_hover_goes_nowhere() {
        let (mut tree, hits) = scroll_fixture();
        tree.dispatch_event(WidgetEvent::scroll(notch(), Modifiers::NONE));
        assert!(hits.borrow().is_empty());
    }

    // --- the per-dispatch snapshot ---------------------------------------

    /// A handler can ask which pointer it is serving, and what phase and
    /// source a scroll had.
    #[test]
    fn a_handler_sees_the_sample_it_is_serving() {
        let seen: Rc<RefCell<Vec<(ScrollPhase, ScrollSource, Option<Point>)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();

        let mut tree = WidgetTree::new();
        // A `ScrollSource::TouchPan` sample is delivered along the pan
        // claimants and nowhere else (see `widget_tree::pan_arbiter`), so the
        // fixture has to be a pan surface for the sample below to reach it at
        // all. Nothing about what this test *asserts* changes — only that the
        // widget it asserts against is now the kind of widget a touch pan is
        // addressed to.
        tree.add(
            FillWidget::new()
                .scroll_container(crate::pointer::touch_action::PanAxes::BOTH)
                .on_scroll(move |_event, ctx| {
                    sink.borrow_mut().push((
                        ctx.scroll_phase(),
                        ctx.scroll_source(),
                        ctx.pointer_position(),
                    ));
                    EventResponse::Handled
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.dispatch_scroll(ScrollSample {
            delta: notch(),
            position: Some(Point::new(50.0, 50.0)),
            phase: ScrollPhase::Momentum,
            source: ScrollSource::TouchPan,
            pointer: PointerInfo::mouse(EventTime::from_millis(12)),
            modifiers: Modifiers::NONE,
        });

        assert_eq!(
            *seen.borrow(),
            vec![(
                ScrollPhase::Momentum,
                ScrollSource::TouchPan,
                Some(Point::new(50.0, 50.0))
            )]
        );
    }

    /// Outside a pointer dispatch a handler sees the default mouse — the same
    /// answer every such handler got before pointers were distinguishable.
    #[test]
    fn a_legacy_event_reports_the_mouse_at_the_epoch() {
        let seen: Rc<RefCell<Option<(PointerId, ScrollPhase)>>> = Rc::new(RefCell::new(None));
        let sink = seen.clone();

        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_pointer_event(move |_event, ctx| {
            *sink.borrow_mut() = Some((ctx.pointer().id, ctx.scroll_phase()));
            EventResponse::Ignored
        }));
        tree.layout(SizeProposal::exact(100.0, 100.0));
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(50.0, 50.0)));

        assert_eq!(
            *seen.borrow(),
            Some((PointerId::MOUSE, ScrollPhase::Discrete))
        );
    }

    /// The snapshot is saved and restored around a dispatch, so a nested one
    /// (a synthetic click, a scroll-into-view walk) does not strand the outer
    /// sample's view of the world.
    #[test]
    fn the_snapshot_is_restored_after_a_dispatch() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let before = tree.current_input.clone();
        tree.dispatch_scroll(
            ScrollSample::wheel(notch(), Modifiers::NONE, EventTime::ZERO)
                .at(Point::new(10.0, 10.0)),
        );
        assert_eq!(tree.current_input, before);
    }
}

#[cfg(test)]
mod press_and_focus_tests {
    //! The framework press, focus-on-release for direct pointers, and the one
    //! `focus_visible` signal — driven through the real ingress doors against
    //! real trees.

    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use teksilo_canvas::{Point, SizeProposal};

    use crate::WidgetId;
    use crate::event::{EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
    use crate::focus::FocusOrigin;
    use crate::pointer::clock::ManualClock;
    use crate::pointer::touch_action::PanClaim;
    use crate::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };
    use crate::test_widgets::{FillWidget, StackWidget};
    use crate::widget_builder::WidgetBuilder;
    use crate::widget_tree::WidgetTree;

    // -----------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------

    /// A fresh contact identity, minted through the real allocator.
    fn contact_id() -> PointerId {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        PointerIdAllocator::global().begin(
            BackendDeviceKey::new(0x0B11),
            NEXT.fetch_add(1, Ordering::Relaxed),
        )
    }

    fn contact(id: PointerId, phase: PointerPhase, at: Point, t: EventTime) -> PointerSample {
        PointerSample {
            pointer: PointerInfo::touch(id, t),
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// A tree on a clock the test drives, so every deadline in these tests is
    /// virtual.
    fn tree_on_a_clock() -> (WidgetTree, Rc<ManualClock>) {
        let mut tree = WidgetTree::new();
        let clock = Rc::new(ManualClock::new(EventTime::ZERO));
        tree.set_input_clock(clock.clone());
        (tree, clock)
    }

    /// A tappable leaf with a framework press signal installed on it, plus the
    /// signal itself.
    ///
    /// `on_tap` is what gives the node a gesture arena, which is what makes it
    /// the press owner — the same thing a `Button` gets from its own tap
    /// handler.
    fn tappable(tree: &mut WidgetTree) -> (WidgetId, crate::signal::Signal<bool>) {
        let id = tree.add(FillWidget::new().focusable().on_tap(|_e, _c| {}));
        let signal = tree.pressed_signal(id);
        (id, signal)
    }

    // -----------------------------------------------------------------
    // The enum
    // -----------------------------------------------------------------

    /// `FocusOrigin` now names the device behind a pointer focus, and the two
    /// accessors that replaced `== FocusOrigin::Pointer` answer for every arm.
    #[test]
    fn a_pointer_origin_names_its_device() {
        use teksilo_tokens::{PenKind, PointerKind};

        assert!(FocusOrigin::Pointer(PointerKind::Touch).is_pointer());
        assert!(FocusOrigin::Pointer(PointerKind::Pen(PenKind::Pen)).is_pointer());
        assert!(FocusOrigin::POINTER.is_pointer());
        assert!(!FocusOrigin::Keyboard.is_pointer());
        assert!(!FocusOrigin::Programmatic.is_pointer());
        assert!(!FocusOrigin::Accessibility.is_pointer());

        assert_eq!(
            FocusOrigin::Pointer(PointerKind::Touch).pointer_kind(),
            Some(PointerKind::Touch),
        );
        assert_eq!(FocusOrigin::Keyboard.pointer_kind(), None);
        assert_eq!(
            FocusOrigin::POINTER.pointer_kind(),
            Some(PointerKind::Unknown),
            "a widget deriving its own origin says so rather than naming a device it never saw",
        );
    }

    /// The `:focus-visible` verdict, per origin. `Programmatic` abstains — a
    /// scripted focus declares no modality, so the ring stays where the user's
    /// last real interaction left it.
    #[test]
    fn only_a_real_navigation_declares_a_modality() {
        use teksilo_tokens::PointerKind;

        assert_eq!(FocusOrigin::Keyboard.focus_visible(), Some(true));
        assert_eq!(FocusOrigin::Accessibility.focus_visible(), Some(true));
        assert_eq!(
            FocusOrigin::Pointer(PointerKind::Touch).focus_visible(),
            Some(false),
        );
        assert_eq!(FocusOrigin::Programmatic.focus_visible(), None);
    }

    /// The three Tier-3 configs still carry `Signal<Option<FocusOrigin>>`, and
    /// a style reading one still gets what it was written against.
    #[test]
    fn the_tier_three_configs_are_unchanged() {
        let origin: crate::signal::Signal<Option<FocusOrigin>> =
            crate::signal::Signal::new(Some(FocusOrigin::Keyboard));
        let slider: crate::signal::Signal<Option<FocusOrigin>> = origin.clone();
        let splitter: crate::signal::Signal<Option<FocusOrigin>> = origin.clone();
        let segmented: crate::signal::Signal<Option<FocusOrigin>> = origin.clone();
        for field in [slider, splitter, segmented] {
            assert_eq!(field.get(), Some(FocusOrigin::Keyboard));
        }
        origin.set(Some(FocusOrigin::POINTER));
        assert_ne!(
            origin.get(),
            Some(FocusOrigin::Keyboard),
            "the `== Some(Keyboard)` test every consumer makes still discriminates",
        );
    }

    // -----------------------------------------------------------------
    // focus-visible per kind
    // -----------------------------------------------------------------

    /// Keyboard focus, then a touch tap, leaves no ring.
    ///
    /// The behaviour predates this package; what is new is that the focus the
    /// tap installs lands on the **release**, so this pins that moving the
    /// assignment did not leave a keyboard ring standing over a control the
    /// finger just took.
    #[test]
    fn a_touch_tap_after_keyboard_focus_leaves_no_ring() {
        let (mut tree, _clock) = tree_on_a_clock();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let root = tree.add(SideBySide {
            children: vec![a, b],
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = root;

        let visible = tree.focus_visible_signal();
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));
        assert!(visible.get(), "keyboard navigation reveals the ring");

        let id = contact_id();
        let at = tree.bounds(b).center();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, at, EventTime::ZERO));
        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Up,
            at,
            EventTime::from_millis(30),
        ));

        assert_eq!(tree.focused(), Some(b), "the release moved focus");
        assert!(!visible.get(), "and the ring did not come with it");
        assert_eq!(
            tree.focus_origin(),
            Some(FocusOrigin::Pointer(teksilo_tokens::PointerKind::Touch)),
        );
    }

    /// An assistive `Action::Focus` reveals the ring. The user is navigating —
    /// they are simply not doing it with a key — and this used to route through
    /// `Programmatic`, which declares nothing and left a screen-reader user
    /// with an invisible focus after any click.
    #[test]
    fn an_assistive_focus_reveals_the_ring() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let root = tree.add(SideBySide {
            children: vec![a, b],
        });
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let _ = root;
        let visible = tree.focus_visible_signal();

        tree.click(a);
        assert_eq!(tree.focused(), Some(a));
        assert!(!visible.get(), "the click hid it");

        tree.dispatch_event(WidgetEvent::AccessAction {
            target: Some(b),
            action: accesskit::Action::Focus,
            target_node: crate::accessibility::widget_id_to_node_id(b),
            data: None,
        });
        assert_eq!(tree.focused(), Some(b));
        assert!(visible.get(), "an assistive move is a navigation");
        assert_eq!(tree.focus_origin(), Some(FocusOrigin::Accessibility));
    }

    /// A programmatic focus abstains: it declares no modality, so the ring
    /// stays exactly where the last real interaction left it. This is what
    /// `Button` / `Checkbox`'s pre-existing focus-ring tests pin, and what
    /// browsers do for `element.focus()`.
    #[test]
    fn a_programmatic_focus_leaves_the_modality_alone() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let root = tree.add(SideBySide {
            children: vec![a, b],
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = root;
        let visible = tree.focus_visible_signal();

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));
        assert!(visible.get());
        tree.focus(b);
        assert!(
            visible.get(),
            "a scripted focus does not hide a keyboard ring"
        );

        tree.click(a);
        assert!(!visible.get());
        tree.focus(b);
        assert!(
            !visible.get(),
            "and does not reveal one after a click either",
        );
    }

    // -----------------------------------------------------------------
    // Activation on release
    // -----------------------------------------------------------------

    /// A mouse focuses on press, exactly as it always has.
    #[test]
    fn a_mouse_still_focuses_on_press() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let center = tree.bounds(w).center();

        tree.dispatch_event(WidgetEvent::pointer_down(
            center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert_eq!(
            tree.focused(),
            Some(w),
            "focus lands on the press for an indirect pointer",
        );
    }

    /// A finger's focus waits for the release.
    #[test]
    fn a_finger_focuses_on_release() {
        let (mut tree, _clock) = tree_on_a_clock();
        let w = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let center = tree.bounds(w).center();

        let id = contact_id();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, center, EventTime::ZERO));
        assert_eq!(
            tree.focused(),
            None,
            "a finger that has only landed has chosen nothing yet",
        );

        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Up,
            center,
            EventTime::from_millis(40),
        ));
        assert_eq!(tree.focused(), Some(w));
        assert_eq!(
            tree.focus_origin().and_then(FocusOrigin::pointer_kind),
            Some(teksilo_tokens::PointerKind::Touch),
            "the origin names the device that delivered it",
        );
    }

    /// …and only when the release lands back on the same focusable. A finger
    /// that presses one control, slides onto its neighbour and lifts has
    /// activated nothing and must move focus nowhere.
    #[test]
    fn a_release_on_a_different_focusable_moves_no_focus() {
        let (mut tree, _clock) = tree_on_a_clock();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let root = tree.add(SideBySide {
            children: vec![a, b],
        });
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let _ = root;

        let on_a = tree.bounds(a).center();
        let on_b = tree.bounds(b).center();
        let id = contact_id();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, on_a, EventTime::ZERO));
        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Move,
            on_b,
            EventTime::from_millis(20),
        ));
        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Up,
            on_b,
            EventTime::from_millis(40),
        ));

        assert_eq!(
            tree.focused(),
            None,
            "the guard is `the release landed on the same focusable as the press`",
        );
    }

    // -----------------------------------------------------------------
    // The press visual
    // -----------------------------------------------------------------

    /// A mouse press lights the visual at once and the release clears it —
    /// the pre-touch rule, and no press-feedback delay anywhere near it.
    #[test]
    fn a_mouse_press_visual_is_unchanged() {
        let mut tree = WidgetTree::new();
        let (w, pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let center = tree.bounds(w).center();

        assert!(!pressed.get(), "not pressed at rest");
        tree.dispatch_event(WidgetEvent::pointer_down(
            center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert!(pressed.get(), "an indirect pointer lights up on the press");
        assert!(
            !tree.press_pending(w),
            "and never waits: a mouse opens no pan session, so there is no \
             ambiguity to wait out",
        );
        assert_eq!(tree.pressed_by(w), Some(PointerId::MOUSE));

        tree.dispatch_event(WidgetEvent::pointer_up(
            center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert!(!pressed.get(), "the release clears it");
        assert_eq!(tree.pressed_by(w), None);
    }

    /// A slide off the target clears the visual, and sliding back on restores
    /// it. WCAG 2.2 SC 2.5.2's abort gesture, and reversible right up to the
    /// release.
    #[test]
    fn a_slide_off_clears_the_visual_and_re_entry_restores_it() {
        let (mut tree, _clock) = tree_on_a_clock();
        let (w, pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(100.0, 200.0));
        let inside = tree.bounds(w).center();
        let outside = Point::new(inside.x, inside.y + 400.0);

        let id = contact_id();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, inside, EventTime::ZERO));
        assert!(
            pressed.get(),
            "nothing here claims a pan, so no delay applies"
        );

        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Move,
            outside,
            EventTime::from_millis(20),
        ));
        assert!(!pressed.get(), "the press has left its target");
        assert!(!tree.press_is_inside(w));
        assert_eq!(
            tree.pressed_by(w),
            Some(id),
            "the contact still holds the press — it is the *visual* that is off",
        );

        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Move,
            inside,
            EventTime::from_millis(40),
        ));
        assert!(pressed.get(), "sliding back on restores it");

        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Up,
            inside,
            EventTime::from_millis(60),
        ));
        assert!(!pressed.get());
    }

    /// A second contact cannot clear the first's visual. Its own release
    /// removes only the press it owns.
    #[test]
    fn a_second_contact_cannot_clear_the_first_visual() {
        let (mut tree, _clock) = tree_on_a_clock();
        let (w, pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let bounds = tree.bounds(w);
        let first_at = Point::new(bounds.x + 20.0, bounds.y + 25.0);
        let second_at = Point::new(bounds.x + 70.0, bounds.y + 25.0);

        let first = contact_id();
        let second = contact_id();
        tree.dispatch_pointer(contact(
            first,
            PointerPhase::Down,
            first_at,
            EventTime::ZERO,
        ));
        assert!(pressed.get());
        assert_eq!(tree.pressed_by(w), Some(first));

        tree.dispatch_pointer(contact(
            second,
            PointerPhase::Down,
            second_at,
            EventTime::from_millis(10),
        ));
        assert_eq!(
            tree.pressed_by(w),
            Some(first),
            "under `MultiContact::First` the second contact is terminated before \
             it reaches the arena at all",
        );

        tree.dispatch_pointer(contact(
            second,
            PointerPhase::Up,
            second_at,
            EventTime::from_millis(20),
        ));
        assert!(
            pressed.get(),
            "so the second contact's release cannot clear a visual it never owned",
        );
        assert_eq!(tree.pressed_by(w), Some(first));

        tree.dispatch_pointer(contact(
            first,
            PointerPhase::Up,
            first_at,
            EventTime::from_millis(30),
        ));
        assert!(!pressed.get(), "the owner's release does clear it");
    }

    /// A cancel clears the visual. The node is never sent an `Up` to clear it
    /// from, so nothing else could.
    #[test]
    fn a_cancel_clears_the_visual() {
        let (mut tree, _clock) = tree_on_a_clock();
        let (w, pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let center = tree.bounds(w).center();

        let id = contact_id();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, center, EventTime::ZERO));
        assert!(pressed.get());

        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Cancel,
            center,
            EventTime::from_millis(20),
        ));
        assert!(!pressed.get(), "a press that was taken away is not painted");
        assert_eq!(tree.pressed_by(w), None);
    }

    /// A peer winning the arbitration clears the visual. The pressed control is
    /// never told; only the router knows it has lost the press.
    #[test]
    fn a_pan_claim_clears_the_visual() {
        let (mut tree, clock) = tree_on_a_clock();
        let (row, pressed) = tappable(&mut tree);
        let list = tree.add(
            StackWidget::new()
                .child(row)
                .pan_claim(PanClaim::vertical())
                .on_scroll(|_e, _c| EventResponse::Handled),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let _ = list;
        let start = Point::new(100.0, 200.0);

        let id = contact_id();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, start, EventTime::ZERO));
        // Inside a claimant, so the visual waits.
        assert!(tree.press_pending(row));
        clock.set(EventTime::from_millis(100));
        tree.tick_gestures(std::time::Instant::now());
        assert!(pressed.get(), "…and appears once the delay has elapsed");

        // Past the touch profile's 36 dp pan slop: the list claims.
        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Move,
            Point::new(100.0, 260.0),
            EventTime::from_millis(120),
        ));
        assert!(
            !pressed.get(),
            "the row lost the press to the list and must stop advertising it",
        );
        assert_eq!(tree.pressed_by(row), None);
    }

    // -----------------------------------------------------------------
    // Which button may raise the visual
    // -----------------------------------------------------------------

    /// The visual answers to the same buttons the activation does. A node
    /// carrying a plain `on_tap` accepts `PRIMARY` and nothing else, so a
    /// middle, back or forward press must light nothing up — the press it
    /// would be advertising can never complete.
    #[test]
    fn a_button_the_control_cannot_act_on_raises_no_visual() {
        let taps = Rc::new(Cell::new(0u32));
        let count = taps.clone();
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().focusable().on_tap(move |_e, _c| {
            count.set(count.get() + 1);
        }));
        let pressed = tree.pressed_signal(w);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let center = tree.bounds(w).center();

        for button in [
            PointerButton::Middle,
            PointerButton::Back,
            PointerButton::Forward,
        ] {
            tree.dispatch_event(WidgetEvent::pointer_down(center, button, Modifiers::NONE));
            assert!(
                !pressed.get(),
                "{button:?} cannot activate an `on_tap` node, so it must not light one up",
            );
            assert_eq!(tree.pressed_by(w), None, "and owns no visual to clear");
            tree.dispatch_event(WidgetEvent::pointer_up(center, button, Modifiers::NONE));
            assert_eq!(
                taps.get(),
                0,
                "the tap recognizer refuses {button:?} too — that is the point",
            );
        }

        // …and the button it does act on is untouched.
        tree.dispatch_event(WidgetEvent::pointer_down(
            center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert!(pressed.get(), "a primary press lights up as it always has");
        assert_eq!(tree.pressed_by(w), Some(PointerId::MOUSE));
        tree.dispatch_event(WidgetEvent::pointer_up(
            center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert!(!pressed.get());
        assert_eq!(taps.get(), 1);
    }

    /// The case a real user hits: a right-click on a control with no context
    /// menu. The secondary arm finds nothing to open and falls through to the
    /// ordinary press path, which must still raise nothing.
    #[test]
    fn a_secondary_press_with_no_context_menu_raises_no_visual() {
        let mut tree = WidgetTree::new();
        let (w, pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let center = tree.bounds(w).center();

        tree.dispatch_event(WidgetEvent::pointer_down(
            center,
            PointerButton::Secondary,
            Modifiers::NONE,
        ));
        assert!(
            !pressed.get(),
            "nothing opened, and nothing may look pressed either",
        );
        assert_eq!(tree.pressed_by(w), None);
        assert!(!tree.press_is_inside(w));
    }

    /// A widget that widened its own mask keeps the visual on the buttons it
    /// widened to: the gate reads the node's declared acceptance, it does not
    /// hardcode `PRIMARY`.
    #[test]
    fn a_widened_mask_widens_the_visual_with_it() {
        use crate::event::ButtonMask;

        let mut tree = WidgetTree::new();
        let w = tree.add(
            FillWidget::new()
                .on_tap(|_e, _c| {})
                .accept_tap_buttons(ButtonMask::PRIMARY | ButtonMask::MIDDLE),
        );
        let pressed = tree.pressed_signal(w);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let center = tree.bounds(w).center();

        tree.dispatch_event(WidgetEvent::pointer_down(
            center,
            PointerButton::Middle,
            Modifiers::NONE,
        ));
        assert!(pressed.get(), "this node really does act on a middle-click");

        tree.dispatch_event(WidgetEvent::pointer_up(
            center,
            PointerButton::Middle,
            Modifiers::NONE,
        ));
        assert!(!pressed.get());
    }

    /// A press that raises no visual still records the focus a direct pointer
    /// defers to its release. A stylus barrel button reports `Secondary`, and
    /// a pen that presses a control and lifts on it has chosen that control
    /// whether or not the button lit it up — the record exists for the focus,
    /// not only for the visual.
    #[test]
    fn a_press_that_raises_no_visual_still_defers_its_focus() {
        use teksilo_tokens::{PenKind, PointerKind};

        let (mut tree, _clock) = tree_on_a_clock();
        let (w, pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let center = tree.bounds(w).center();

        let id = contact_id();
        let barrel = |phase, t| PointerSample {
            pointer: PointerInfo {
                kind: PointerKind::Pen(PenKind::Pen),
                ..PointerInfo::touch(id, t)
            },
            phase,
            position: center,
            button: Some(PointerButton::Secondary),
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        };

        tree.dispatch_pointer(barrel(PointerPhase::Down, EventTime::ZERO));
        assert!(!pressed.get(), "the barrel button activates nothing here");
        assert_eq!(tree.pressed_by(w), None);
        assert_eq!(
            tree.focused(),
            None,
            "a direct pointer has chosen nothing until it lifts",
        );

        tree.dispatch_pointer(barrel(PointerPhase::Up, EventTime::from_millis(40)));
        assert_eq!(
            tree.focused(),
            Some(w),
            "the deferral is not button-gated: the release landed where the press did",
        );
        assert_eq!(
            tree.focus_origin().and_then(FocusOrigin::pointer_kind),
            Some(PointerKind::Pen(PenKind::Pen)),
        );
    }

    // -----------------------------------------------------------------
    // The press-feedback delay
    // -----------------------------------------------------------------

    /// The delay applies only inside a pan claimant — a control nothing can
    /// scroll out from under has no ambiguity to wait out.
    #[test]
    fn the_feedback_delay_applies_only_inside_a_claimant() {
        // Outside a claimant: immediate.
        let (mut tree, _clock) = tree_on_a_clock();
        let (w, pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let id = contact_id();
        let at = tree.bounds(w).center();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, at, EventTime::ZERO));
        assert!(
            !tree.press_pending(w),
            "no claimant above it, so nothing to rule out",
        );
        assert!(pressed.get());

        // Inside one: withheld, then released by the delay.
        let (mut tree, clock) = tree_on_a_clock();
        let (row, pressed) = tappable(&mut tree);
        let list = tree.add(
            StackWidget::new()
                .child(row)
                .pan_claim(PanClaim::vertical())
                .on_scroll(|_e, _c| EventResponse::Handled),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let _ = list;
        let id = contact_id();
        let at = Point::new(100.0, 200.0);
        tree.dispatch_pointer(contact(id, PointerPhase::Down, at, EventTime::ZERO));
        assert!(tree.press_pending(row), "a finger might be about to scroll");
        assert!(!pressed.get(), "so the row does not flash");
        assert!(
            tree.press_is_inside(row),
            "the press is real; only its visual is being withheld",
        );

        clock.set(EventTime::from_millis(99));
        tree.tick_gestures(std::time::Instant::now());
        assert!(!pressed.get(), "99 ms is under the 100 ms delay");

        clock.set(EventTime::from_millis(100));
        tree.tick_gestures(std::time::Instant::now());
        assert!(pressed.get(), "and 100 ms is it");
        assert!(!tree.press_pending(row));
    }

    /// A mouse pressing inside the very same claimant waits for nothing: an
    /// indirect pointer opens no pan session, so the delay never reaches it.
    #[test]
    fn a_mouse_inside_a_claimant_never_waits() {
        let mut tree = WidgetTree::new();
        let (row, pressed) = tappable(&mut tree);
        let list = tree.add(
            StackWidget::new()
                .child(row)
                .pan_claim(PanClaim::vertical())
                .on_scroll(|_e, _c| EventResponse::Handled),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let _ = list;

        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(100.0, 200.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert!(!tree.press_pending(row));
        assert!(pressed.get(), "Compact with a mouse is exactly as it was");
    }

    // -----------------------------------------------------------------
    // The EventContext queries
    // -----------------------------------------------------------------

    /// The three queries `teksilo-widgets`' `common/interaction.rs` consumes,
    /// read from inside a real handler.
    #[test]
    fn a_handler_can_read_the_press_it_is_inside() {
        let (mut tree, _clock) = tree_on_a_clock();
        let seen: Rc<RefCell<Vec<(bool, bool, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let row = tree.add(FillWidget::new().on_tap(|_e, _c| {}).on_pointer_event(
            move |event, ctx| {
                if matches!(event, WidgetEvent::PointerUp { .. }) {
                    log.borrow_mut().push((
                        ctx.is_pressed(),
                        ctx.press_is_inside(),
                        ctx.press_pending(),
                    ));
                }
                EventResponse::Ignored
            },
        ));
        let list = tree.add(
            StackWidget::new()
                .child(row)
                .pan_claim(PanClaim::vertical())
                .on_scroll(|_e, _c| EventResponse::Handled),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let _ = list;

        let id = contact_id();
        let at = Point::new(100.0, 200.0);
        tree.dispatch_pointer(contact(id, PointerPhase::Down, at, EventTime::ZERO));
        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Up,
            at,
            EventTime::from_millis(30),
        ));

        assert_eq!(
            seen.borrow().as_slice(),
            &[(false, true, true)],
            "released inside the claimant before the delay elapsed: inside, \
             pending, and therefore not yet showing",
        );
    }

    /// A handler outside any press reads three falses rather than a panic or a
    /// stale answer.
    #[test]
    fn a_handler_outside_a_press_reads_nothing() {
        let mut tree = WidgetTree::new();
        let asked = Rc::new(Cell::new(false));
        let flag = asked.clone();
        let w = tree.add(FillWidget::new().focusable().on_key(move |_e, ctx| {
            flag.set(ctx.is_pressed() || ctx.press_is_inside() || ctx.press_pending());
            EventResponse::Ignored
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(w);
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert!(!asked.get());
    }

    /// A completed interaction leaves no press behind, and the leak detector
    /// says so — it now reads the press table, which it could not before this
    /// package landed one.
    #[test]
    fn a_completed_press_leaves_the_detector_clean() {
        let (mut tree, _clock) = tree_on_a_clock();
        let (w, pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let at = tree.bounds(w).center();

        let id = contact_id();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, at, EventTime::ZERO));
        assert!(pressed.get());
        tree.dispatch_pointer(contact(
            id,
            PointerPhase::Up,
            at,
            EventTime::from_millis(30),
        ));
        tree.assert_no_leaked_pointer_state();
    }

    /// …and it would have caught the opposite. Driven by holding the press open
    /// rather than by faking state, so the assertion is about the real exit
    /// path.
    #[test]
    fn the_detector_reports_a_press_still_held() {
        let (mut tree, _clock) = tree_on_a_clock();
        let (w, _pressed) = tappable(&mut tree);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let at = tree.bounds(w).center();

        let id = contact_id();
        tree.dispatch_pointer(contact(id, PointerPhase::Down, at, EventTime::ZERO));
        let held = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.assert_no_leaked_pointer_state()
        }));
        let message = *held
            .expect_err("a live press is leaked state")
            .downcast::<String>()
            .expect("the detector panics with a message");
        assert!(
            message.contains("is still pressed by"),
            "the press is named in the report, got: {message}",
        );
    }

    /// A container that lays its children out side by side, so a hit test at a
    /// given x picks a specific child. `StackWidget` deliberately stacks at one
    /// origin, which is the opposite of what a release-elsewhere test needs.
    #[derive(Debug)]
    struct SideBySide {
        children: Vec<WidgetId>,
    }

    impl crate::widget::Widget for SideBySide {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn place_children(
            &self,
            bounds: teksilo_canvas::Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &crate::widget::LayoutContext,
        ) {
            let n = children.len().max(1) as f32;
            let w = bounds.width / n;
            for (i, child) in children.iter_mut().enumerate() {
                child.origin = Point::new(bounds.x + w * i as f32, bounds.y);
                child.size = teksilo_canvas::Size::new(w, bounds.height);
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            self.children.clone()
        }
    }
}

#[cfg(test)]
mod overlay_release_dismissal_tests {
    //! Outside-press overlay dismissal, driven through the real ingress door.
    //!
    //! The defect: `handle_click_outside` ran on the `PointerDown` and then
    //! *fell through*, so one press both closed a menu and actuated whatever
    //! the menu was covering. With a cursor that is defensible — the user aimed
    //! at a pixel they could see the whole time. With a finger it is not: the
    //! menu is the only thing they were looking at, and the control underneath
    //! is one they never saw.

    use std::cell::Cell;
    use std::rc::Rc;

    use teksilo_canvas::{Point, Rect, Size, SizeProposal};

    use crate::WidgetId;
    use crate::event::{Modifiers, PointerButton, WidgetEvent};
    use crate::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
    use crate::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };
    use crate::test_widgets::FillWidget;
    use crate::widget::{LayoutContext, LayoutResponse, Widget};
    use crate::widget_builder::WidgetBuilder;
    use crate::widget_tree::WidgetTree;

    // -----------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------

    /// A leaf with an intrinsic size, so an overlay hung off it gets real
    /// bounds out of `position_overlays` instead of a zero rect.
    #[derive(Debug)]
    struct Panel(Size);

    impl Widget for Panel {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            self.0.into()
        }
    }

    /// A root that puts its first child over the whole window and its second in
    /// a 10 dp corner.
    ///
    /// Two bare roots would both be laid out at the window's full size — the
    /// trigger would then contain every press point, and the pre-existing
    /// "a press on a click-opened overlay's own anchor is consumed" rule would
    /// swallow the very presses these tests are about.
    #[derive(Debug)]
    struct PageAndTrigger {
        children: Vec<WidgetId>,
    }

    impl Widget for PageAndTrigger {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            if let Some(page) = children.get_mut(0) {
                page.origin = bounds.origin();
                page.size = bounds.size();
            }
            if let Some(trigger) = children.get_mut(1) {
                trigger.origin = bounds.origin();
                trigger.size = Size::new(10.0, 10.0);
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            self.children.clone()
        }
    }

    fn contact_id(n: u64) -> PointerId {
        PointerIdAllocator::global().begin(BackendDeviceKey::new(0x0FA2), n)
    }

    fn touch(id: PointerId, phase: PointerPhase, at: Point) -> PointerSample {
        PointerSample {
            pointer: PointerInfo::touch(id, EventTime::ZERO),
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// The scene every test below shares: a tappable page filling the window,
    /// and a menu overlay floating over part of it at (100, 100, 200, 200).
    ///
    /// The menu's content is a separate root, so a press outside it lands on
    /// the page and a press inside it lands on the menu — which is exactly the
    /// arrangement the dismissal rule is about.
    fn page_with_a_menu() -> (WidgetTree, WidgetId, Rc<Cell<u32>>) {
        let mut tree = WidgetTree::new();
        let taps = Rc::new(Cell::new(0u32));
        let counter = taps.clone();
        let page = tree.add(
            FillWidget::new()
                .focusable()
                .on_tap(move |_e, _c| counter.set(counter.get() + 1)),
        );
        // A small trigger in the corner, well clear of both press points — see
        // `PageAndTrigger` for why it cannot simply be a second root.
        let trigger = tree.add(Panel(Size::new(10.0, 10.0)));
        let _root = tree.add(PageAndTrigger {
            children: vec![page, trigger],
        });
        let menu = tree.add(Panel(Size::new(200.0, 200.0)));
        tree.show_overlay(OverlayRequest {
            content_id: menu,
            anchor: trigger,
            placement: OverlayPlacement::AtPointer(Point::new(100.0, 100.0)),
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(tree.active_overlays().len(), 1, "the menu is open");
        (tree, page, taps)
    }

    fn outside() -> Point {
        Point::new(600.0, 500.0)
    }

    fn inside_menu() -> Point {
        Point::new(150.0, 150.0)
    }

    // -----------------------------------------------------------------
    // The direct-pointer contract
    // -----------------------------------------------------------------

    /// The arming press reaches nothing: no press record, no capture, and the
    /// menu is still up because the decision belongs to the release.
    #[test]
    fn the_suppressed_down_leaves_nothing_pressed_or_captured_beneath() {
        let (mut tree, page, taps) = page_with_a_menu();
        let finger = contact_id(1);
        tree.dispatch_pointer(touch(finger, PointerPhase::Down, outside()));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "the press does not close the menu; the release does"
        );
        assert_eq!(tree.pressed_by(page), None, "nothing beneath is pressed");
        let entry = tree.pointers.get(finger).expect("the contact is live");
        assert_eq!(entry.captured_by, None, "nothing beneath captured it");
        assert!(entry.sequence.is_none(), "no arbitration was opened");
        assert_eq!(taps.get(), 0);
    }

    /// …and the release closes the menu without actuating what it covered.
    #[test]
    fn a_touch_tap_outside_a_menu_closes_it_and_actuates_nothing() {
        let (mut tree, page, taps) = page_with_a_menu();
        let finger = contact_id(2);
        tree.dispatch_pointer(touch(finger, PointerPhase::Down, outside()));
        tree.dispatch_pointer(touch(finger, PointerPhase::Up, outside()));

        assert!(tree.active_overlays().is_empty(), "the menu closed");
        assert_eq!(taps.get(), 0, "the page beneath was never tapped");
        assert_eq!(tree.pressed_by(page), None);
        tree.assert_no_leaked_pointer_state();
    }

    /// A press that never completes delivers nothing at all — neither the
    /// dismissal it armed nor the press it withheld.
    #[test]
    fn a_cancelled_press_aborts_the_arm() {
        let (mut tree, page, taps) = page_with_a_menu();
        let finger = contact_id(3);
        tree.dispatch_pointer(touch(finger, PointerPhase::Down, outside()));
        tree.dispatch_pointer(touch(finger, PointerPhase::Cancel, outside()));

        assert_eq!(tree.active_overlays().len(), 1, "the menu survives");
        assert_eq!(taps.get(), 0);
        assert_eq!(tree.pressed_by(page), None);
        tree.assert_no_leaked_pointer_state();
    }

    /// Land beside the menu, drag onto it, lift there. The finger changed its
    /// mind: the menu stays, and the page under the arming press was never
    /// touched either.
    #[test]
    fn a_press_slid_onto_the_menu_dismisses_nothing() {
        let (mut tree, _page, taps) = page_with_a_menu();
        let finger = contact_id(4);
        tree.dispatch_pointer(touch(finger, PointerPhase::Down, outside()));
        tree.dispatch_pointer(touch(finger, PointerPhase::Move, inside_menu()));
        tree.dispatch_pointer(touch(finger, PointerPhase::Up, inside_menu()));

        assert_eq!(tree.active_overlays().len(), 1, "the menu survives");
        assert_eq!(taps.get(), 0);
        tree.assert_no_leaked_pointer_state();
    }

    /// A finger working *inside* the menu is an interaction, and a second one
    /// landing on the page is not a reason to take it away mid-flight.
    #[test]
    fn a_second_contact_cannot_dismiss_what_the_first_is_manipulating() {
        let (mut tree, _page, _taps) = page_with_a_menu();
        let first = contact_id(5);
        let second = contact_id(6);

        tree.dispatch_pointer(touch(first, PointerPhase::Down, inside_menu()));
        assert!(
            tree.pointers
                .get(first)
                .is_some_and(|e| e.sequence.is_some()),
            "the first contact holds a live press inside the menu"
        );

        tree.dispatch_pointer(touch(second, PointerPhase::Down, outside()));
        tree.dispatch_pointer(touch(second, PointerPhase::Up, outside()));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "the menu the first finger is holding must not close under it"
        );

        // Once the first contact's press is over there is nothing left to
        // revoke, so it protects nothing and the same tap closes the menu.
        tree.dispatch_pointer(touch(first, PointerPhase::Up, inside_menu()));
        let third = contact_id(7);
        tree.dispatch_pointer(touch(third, PointerPhase::Down, outside()));
        tree.dispatch_pointer(touch(third, PointerPhase::Up, outside()));
        assert!(tree.active_overlays().is_empty());
        tree.assert_no_leaked_pointer_state();
    }

    /// A contact that is merely *holding an arm* has no sequence and no
    /// capture, so `press_is_revocable` says it has no press — and it must not
    /// block a second contact's dismissal the way a real press does.
    #[test]
    fn an_arm_is_not_itself_a_press_that_blocks_another_contact() {
        let (mut tree, _page, _taps) = page_with_a_menu();
        let first = contact_id(8);
        let second = contact_id(9);

        tree.dispatch_pointer(touch(first, PointerPhase::Down, outside()));
        assert!(
            tree.overlay_manager().has_armed_dismiss(first),
            "the first contact armed"
        );
        // The first contact is live but holds nothing revocable.
        assert!(tree.busy_press_points(second).is_empty());

        tree.dispatch_pointer(touch(second, PointerPhase::Down, outside()));
        assert!(tree.overlay_manager().has_armed_dismiss(second));
        tree.dispatch_pointer(touch(second, PointerPhase::Up, outside()));
        assert!(tree.active_overlays().is_empty());

        // The first contact's arm now names an overlay that is gone; its own
        // release must be a quiet no-op rather than a panic.
        tree.dispatch_pointer(touch(first, PointerPhase::Up, outside()));
        tree.assert_no_leaked_pointer_state();
    }

    /// A tap *inside* the menu is not an outside press, so nothing is armed and
    /// the menu's own content handles the press exactly as before.
    #[test]
    fn a_touch_inside_the_menu_arms_nothing() {
        let (mut tree, _page, _taps) = page_with_a_menu();
        let finger = contact_id(10);
        tree.dispatch_pointer(touch(finger, PointerPhase::Down, inside_menu()));
        assert!(!tree.overlay_manager().has_armed_dismiss(finger));
        tree.dispatch_pointer(touch(finger, PointerPhase::Up, inside_menu()));
        assert_eq!(tree.active_overlays().len(), 1);
        tree.assert_no_leaked_pointer_state();
    }

    /// A contact whose press was never opened — it hit nothing, or its `Down`
    /// was suppressed by an arm — still ceases to exist when the platform
    /// revokes it.
    ///
    /// The cancel funnel returns early for a pointer with nothing revocable and
    /// so never reaches the step that drops the table entry. Before the release
    /// dismissal that shape was rare (a press on bare background); the arm makes
    /// it the ordinary case, so the ingress door applies the same
    /// contact-ceases-to-exist rule it applies to an `Up`.
    #[test]
    fn a_contact_with_no_press_still_ends_when_the_platform_revokes_it() {
        let mut tree = WidgetTree::new();
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let finger = contact_id(99);
        tree.dispatch_pointer(touch(finger, PointerPhase::Down, outside()));
        tree.dispatch_pointer(touch(finger, PointerPhase::Cancel, outside()));
        tree.assert_no_leaked_pointer_state();
    }

    // -----------------------------------------------------------------
    // The mouse, unchanged
    // -----------------------------------------------------------------

    /// The press dismisses and falls through, exactly as before: one click both
    /// closes the menu and actuates the control beneath.
    #[test]
    fn a_mouse_click_outside_a_menu_dismisses_on_the_press_and_falls_through() {
        let (mut tree, _page, taps) = page_with_a_menu();
        tree.dispatch_event(WidgetEvent::pointer_down(
            outside(),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert!(
            tree.active_overlays().is_empty(),
            "the mouse still dismisses on the press"
        );
        assert!(
            tree.pointers
                .get(PointerId::MOUSE)
                .is_some_and(|e| e.sequence.is_some()),
            "and the press still reaches the page beneath"
        );
        tree.dispatch_event(WidgetEvent::pointer_up(
            outside(),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert_eq!(taps.get(), 1, "the control beneath activated");
        assert!(
            !tree.overlay_manager().has_armed_dismiss(PointerId::MOUSE),
            "a mouse never arms"
        );
        tree.assert_no_leaked_pointer_state();
    }

    // -----------------------------------------------------------------
    // Contact avoidance
    // -----------------------------------------------------------------

    /// A context menu raised by a finger keeps clear of the contact patch; the
    /// same menu raised by a mouse lands on the pixel, as it always has.
    #[test]
    fn a_context_menu_avoids_the_contact_that_opened_it() {
        fn menu_bounds(pointer: PointerInfo, at: Point) -> Rect {
            let mut tree = WidgetTree::new();
            let page = tree.add(
                FillWidget::new()
                    .focusable()
                    .context_menu(|_p, _ctx| Some(Box::new(Panel(Size::new(200.0, 160.0))))),
            );
            let _ = page;
            tree.layout(SizeProposal::exact(800.0, 600.0));
            tree.dispatch_pointer(PointerSample {
                pointer,
                phase: PointerPhase::Down,
                position: at,
                button: Some(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                coalesced: Vec::new(),
            });
            tree.layout(SizeProposal::exact(800.0, 600.0));
            let id = *tree
                .active_overlays()
                .first()
                .expect("the context menu opened");
            tree.overlay_manager().bounds_for(id).expect("bounds")
        }

        let at = Point::new(400.0, 300.0);
        let finger = menu_bounds(PointerInfo::touch(contact_id(11), EventTime::ZERO), at);
        let contact = crate::overlay::rect_centred_on(at, crate::overlay::ASSUMED_CONTACT_PATCH);
        assert!(
            finger.x < contact.x && finger.right() <= contact.x,
            "a touch menu clears the contact patch: {finger:?} vs {contact:?}"
        );

        let mouse = menu_bounds(PointerInfo::mouse(EventTime::ZERO), at);
        assert_eq!(
            (mouse.x, mouse.y),
            (at.x, at.y),
            "a mouse menu still opens with its corner on the pointer"
        );
    }
}

/// An overlay is chosen by its **bounds**, and what happens when its content
/// then claims nothing at the point.
///
/// Two answers, and the flag on the content root is what picks between them.
/// An ordinary overlay ends the search inside itself — that is what keeps the
/// miss-only slop pass confined to the one layer the exact pass entered, so a
/// near-miss on a menu row can never be re-attributed to a control on the page
/// behind the menu. An overlay whose content root declares `event_pass_through`
/// is the exception: the flag already means "what I did not claim belongs to
/// whatever is behind me", and an overlay root is not an exception to it.
///
/// This is not a hypothetical. The touch text affordances were specified as a
/// viewport-sized pass-through layer, and under the first answer applied to
/// both, mounting one made the editor beneath stop taking presses entirely —
/// measured, as `hit_test` returning `None` at a point inside the field.
#[cfg(test)]
mod pass_through_overlay_tests {
    use teksilo_canvas::{Point, Rect, SizeProposal};

    use crate::WidgetId;
    use crate::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
    use crate::test_widgets::FillWidget;
    use crate::widget::{LayoutContext, LayoutResponse, Widget};
    use crate::widget_builder::WidgetBuilder;
    use crate::widget_tree::WidgetTree;

    /// The affordance layer's shape: fills whatever it is given, and puts its
    /// one child — a selection handle — on a fixed rectangle inside it.
    #[derive(Debug)]
    struct HandleLayer {
        handle: WidgetId,
        at: Rect,
    }

    impl Widget for HandleLayer {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn place_children(
            &self,
            _bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            if let Some(child) = children.get_mut(0) {
                child.origin = self.at.origin();
                child.size = self.at.size();
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            vec![self.handle]
        }
    }

    const HANDLE: Rect = Rect {
        x: 10.0,
        y: 10.0,
        width: 20.0,
        height: 20.0,
    };

    /// Which flag the overlay's content root carries.
    #[derive(Clone, Copy, PartialEq)]
    enum RootFlag {
        /// The affordance layer's: not a target itself, children are.
        PassThrough,
        /// Neither the root nor its children are targets. Stronger, and
        /// deliberately **not** what the fall-through is gated on.
        HitTransparent,
    }

    /// A field under a viewport-sized overlay carrying one handle.
    ///
    /// Both flags make the subtree answer `None` for a point no handle covers,
    /// which is the state the fall-through decides — so the two arms differ in
    /// nothing but the flag the router reads.
    fn field_under_a_layer(flag: RootFlag) -> (WidgetTree, WidgetId, WidgetId) {
        let mut tree = WidgetTree::new();
        let field = tree.add(FillWidget::new());
        let handle = tree.add(FillWidget::new());
        let layer = HandleLayer { handle, at: HANDLE };
        let layer = match flag {
            RootFlag::PassThrough => tree.add(layer.event_pass_through(true)),
            RootFlag::HitTransparent => tree.add(layer.hit_transparent(true)),
        };
        tree.show_overlay(OverlayRequest {
            content_id: layer,
            anchor: field,
            placement: OverlayPlacement::FullViewport,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.layout(SizeProposal::exact(400.0, 200.0));
        assert_eq!(tree.active_overlays().len(), 1, "the layer is up");
        (tree, field, handle)
    }

    /// The door: the surface under a pass-through layer goes on taking presses.
    #[test]
    fn a_pass_through_layer_hands_back_what_it_did_not_claim() {
        let (tree, field, handle) = field_under_a_layer(RootFlag::PassThrough);

        assert_eq!(
            tree.hit_test(Point::new(15.0, 15.0)),
            Some(handle),
            "the layer must still win the point its own child covers"
        );
        assert_eq!(
            tree.hit_test(Point::new(200.0, 100.0)),
            Some(field),
            "a press the layer did not claim must reach the surface beneath it"
        );
    }

    /// `event_pass_through` removes a node from **hit-testing**, not from the
    /// **bubble path** of a descendant that was hit.
    ///
    /// The distinction is what lets a host mount its affordances at the full
    /// viewport and still answer the one press neither the layer nor the surface
    /// beneath can: the pass-through root is skipped when a point belongs to
    /// nobody in its subtree, and is still told about a press that landed on one
    /// of its children. The single-line text stack's `AffordanceHost` is exactly
    /// this — a cursor's click on a selection handle, which the handle refuses and
    /// the editor never sees.
    #[test]
    fn a_pass_through_root_still_hears_a_press_that_landed_on_its_child() {
        use std::cell::Cell;
        use std::rc::Rc;

        let seen = Rc::new(Cell::new(0u32));
        let counter = seen.clone();
        let mut tree = WidgetTree::new();
        let field = tree.add(FillWidget::new());
        let handle = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        let layer = tree.add(
            HandleLayer { handle, at: HANDLE }
                .event_pass_through(true)
                .on_pointer_event(move |_event, _ctx| {
                    counter.set(counter.get() + 1);
                    crate::event::EventResponse::Ignored
                }),
        );
        tree.show_overlay(OverlayRequest {
            content_id: layer,
            anchor: field,
            placement: OverlayPlacement::FullViewport,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.pointer_down_button(Point::new(15.0, 15.0), crate::event::PointerButton::Primary);
        assert!(
            seen.get() > 0,
            "a pass-through root heard nothing about a press on its own child"
        );

        let after_child = seen.get();
        tree.pointer_up_button(Point::new(15.0, 15.0), crate::event::PointerButton::Primary);
        // …and a press it did not contain a target for never arrives, because it
        // was never the hit.
        tree.pointer_down_button(
            Point::new(200.0, 100.0),
            crate::event::PointerButton::Primary,
        );
        assert_eq!(
            seen.get(),
            after_child + 1,
            "the press-and-lift on the child accounts for the count, and the \
             press on the surface beneath added nothing"
        );
    }

    /// …and the widening stops at that one flag.
    ///
    /// `hit_transparent` is the discriminating case, and the only one available:
    /// it is the other way for an overlay's content to claim nothing at a point,
    /// so it is the fixture in which the gate — rather than the subtree's own
    /// answer — is what decides. A gate that read "the subtree claimed nothing"
    /// alone would fall through here too, and with it past every overlay whose
    /// content happens to miss, which is what confines the miss-only slop pass to
    /// the layer the exact pass entered.
    ///
    /// The exclusion is deliberate rather than an oversight: the one overlay that
    /// must be seen past regardless is the drag preview, and the hit-test already
    /// takes an explicit `exclude_overlay` for it.
    #[test]
    fn a_hit_transparent_root_does_not_get_the_fall_through() {
        let (tree, field, _handle) = field_under_a_layer(RootFlag::HitTransparent);

        // Nothing in the overlay is a target, not even the handle.
        assert_eq!(
            tree.hit_test(Point::new(15.0, 15.0)),
            None,
            "hit_transparent must exclude the subtree, or this fixture is not \
             testing the gate"
        );
        assert_eq!(
            tree.hit_test(Point::new(200.0, 100.0)),
            None,
            "the fall-through reached past an overlay that did not ask for it"
        );
        let _ = field;
    }
}
