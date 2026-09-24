// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

/// Linear interpolation between two points, `t` in `0.0..=1.0`.
///
/// Every multi-sample helper in this module walks its path with this, so the
/// intermediate positions of a drag, a fling and a pinch are produced by one
/// rule and a test that counts samples can reason about where each one landed.
fn lerp_point(from: Point, to: Point, t: f32) -> Point {
    Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
}

impl WidgetTree {
    /// The content id of the tooltip anchored at `widget` or anywhere inside
    /// it.
    ///
    /// The attach helpers keep the content id to themselves, so a test that
    /// needs to drive a tooltip's own surface (promote it, focus into it) has
    /// no other way to name it. Matching the whole subtree, not just the id,
    /// is what makes this work for composing controls: `Button` keeps focus on
    /// its outer node but attaches its tooltip to an inner body root.
    pub fn tooltip_content_within(&self, widget: WidgetId) -> Option<WidgetId> {
        self.tooltips
            .iter()
            .find(|e| self.is_descendant_of(e.anchor_id, widget))
            .map(|e| e.content_id)
    }

    /// Whether that tooltip has been promoted.
    ///
    /// Promotion is the line between an informational tip and a panel the user
    /// asked for: it decides the AT role, the dismiss behaviour, and whether
    /// the surface takes a Tab stop.
    pub fn tooltip_is_sticky_within(&self, widget: WidgetId) -> bool {
        self.tooltips
            .iter()
            .any(|e| self.is_descendant_of(e.anchor_id, widget) && e.is_sticky)
    }

    /// Simulate a click at the center of a widget.
    pub fn click(&mut self, id: WidgetId) {
        self.synthesise_tap(id);
    }

    /// Synthesise a primary-button tap at the center of `id`'s
    /// resolved bounds. The OS hands the click off to the widget tree
    /// even though the click never went through the normal hit-test
    /// path. Used by the Windows custom-title-bar backend when
    /// `WM_NCHITTEST` reported `HTMINBUTTON`/`HTMAXBUTTON`/`HTCLOSE`
    /// for an area covering a `ControlButton` — the OS treated the
    /// area as non-client and `WM_LBUTTONDOWN`/`UP` never fired in
    /// widget land, so we re-issue a synthetic primary-button down
    /// + up on the right widget.
    ///
    /// Equivalent semantics to [`Self::click`]; named differently so
    /// production call sites read clearly.
    ///
    /// The tap runs on a standalone dispatch, so a handler it reaches
    /// cannot use the multi-window API. Call
    /// [`synthesise_tap_with_ops`](Self::synthesise_tap_with_ops) from
    /// anywhere that already holds a real
    /// [`WindowOps`](crate::window::WindowOps) sink.
    pub fn synthesise_tap(&mut self, id: WidgetId) {
        let mut noop = crate::window::NoopWindowOps;
        self.synthesise_tap_with_ops(id, &mut noop);
    }

    /// [`synthesise_tap`](Self::synthesise_tap), dispatched over the
    /// caller's app-level [`WindowOps`](crate::window::WindowOps) sink.
    ///
    /// A synthetic tap is a *nested* dispatch, and everything the tapped
    /// widget does happens inside it — including the intent it sends and
    /// the action that intent resolves to. Dispatching it standalone
    /// therefore hands that action a context with no window sink:
    /// `ctx.open_window` panics, and `find_window` / `focus_window` /
    /// `close_window_by_id` silently do nothing. That is how keyboard
    /// activation in a menu (Enter, Space, a mnemonic, type-ahead — all
    /// four route through `EventContext::synthetic_click`) lost the
    /// multi-window API that the same row reached fine by mouse.
    pub fn synthesise_tap_with_ops(
        &mut self,
        id: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let center = self.arena.bounds(id).center();
        self.dispatch_event_with_ops(
            WidgetEvent::pointer_down(center, PointerButton::Primary, Modifiers::NONE),
            &mut *ops,
        );
        self.dispatch_event_with_ops(
            WidgetEvent::pointer_up(center, PointerButton::Primary, Modifiers::NONE),
            &mut *ops,
        );
    }

    /// Simulate pointer movement to a position.
    pub fn pointer_move(&mut self, position: Point) {
        self.dispatch_event(WidgetEvent::pointer_move(position));
    }

    /// Simulate a key press (down + up), carrying the text the platform
    /// attaches to the key ([`Key::to_text`]).
    ///
    /// That text is not decoration: Escape arrives as U+001B, and a widget
    /// that inspects `text` behaves differently with it than without. This
    /// helper used to send `text: None` for every key, so a whole class of
    /// bug was invisible to every test in the workspace — a field that
    /// swallowed Escape passed the suite while failing in the user's hands.
    pub fn press_key(&mut self, key: Key, modifiers: Modifiers) {
        self.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: key.to_text().map(str::to_string),
        });
        self.dispatch_event(WidgetEvent::KeyUp { key, modifiers });
    }

    /// Simulate typing text into the focused widget.
    pub fn type_text(&mut self, _widget: WidgetId, text: &str) {
        for ch in text.chars() {
            self.dispatch_event(WidgetEvent::KeyDown {
                key: Key::Character(ch),
                modifiers: Modifiers::NONE,
                text: Some(ch.to_string()),
            });
        }
    }

    /// Simulate a pointer down at a specific position with a specific button.
    pub fn pointer_down_button(&mut self, position: Point, button: PointerButton) {
        self.dispatch_event(WidgetEvent::pointer_down(position, button, Modifiers::NONE));
    }

    /// Simulate a pointer up at a specific position with a specific button.
    pub fn pointer_up_button(&mut self, position: Point, button: PointerButton) {
        self.dispatch_event(WidgetEvent::pointer_up(position, button, Modifiers::NONE));
    }

    /// Simulate a drag from one position to another.
    pub fn drag(&mut self, from: Point, to: Point) {
        self.dispatch_event(WidgetEvent::pointer_down(
            from,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        self.dispatch_event(WidgetEvent::pointer_move(to));
        self.dispatch_event(WidgetEvent::pointer_up(
            to,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
    }

    /// Get bounds of a child by index.
    pub fn child_bounds(&self, parent: WidgetId, index: usize) -> Rect {
        let children = self.children(parent);
        self.bounds(children[index])
    }

    /// Get a child widget ID by index.
    pub fn child_widget(&self, parent: WidgetId, index: usize) -> WidgetId {
        self.children(parent)[index]
    }

    /// Advance this tree's clock by `duration`, and run everything that clock
    /// drives.
    ///
    /// **The one door.** One call moves, to one virtual now: the simulated
    /// clock, the input timeline, the gesture arenas (today: the long-press
    /// hold), the press-feedback delays, every live fling, the animation
    /// scheduler, the frame tick, the overlay manager's clock, tooltip dwell,
    /// delayed overlays, the pointer-leave grace and overlay auto-dismissal —
    /// then drains the signal, rebuild and visibility changes any of that
    /// produced. A caller never has to advance a second thing to keep one of
    /// those in step with another.
    ///
    /// It is not, however, the door to *everything* that is timed; the list
    /// below is the current boundary, and it is the list that has to grow when
    /// a subsystem is brought onto this clock.
    ///
    /// While this runs, time is **taken over**: the input timeline and the
    /// animation clock both read the simulated clock and nothing else. A long
    /// press fires because the caller advanced the hold and never because the
    /// caller itself took that long; two samples dispatched without an
    /// intervening advance are stamped the same instant rather than however far
    /// apart the machine happened to run them; and an animation ages by exactly
    /// what was advanced. A headless test wants that to persist, and it does. A
    /// host sharing the tree with a real event loop — the debug automation
    /// bridge — must give time back when the operation ends, or the window it
    /// is attached to never measures another gesture and never advances another
    /// animation frame: see [`resume_real_time`](Self::resume_real_time).
    ///
    /// What it does **not** move:
    ///
    /// - The shader-driven
    ///   [`AnimatedQuadRegistry`](crate::animated_quad::AnimatedQuadRegistry).
    ///   It is ticked from `render()` and has no simulated door at all.
    /// - A deferred member's `eligible_at` on a
    ///   [`PointerSequence`](crate::gesture::PointerSequence). Not an
    ///   oversight: eligibility is never stored, it is re-derived against the
    ///   timestamp of whatever sample is being arbitrated, so there is no
    ///   transition to perform at that instant and a press that sat still past
    ///   its `long_press` is already eligible on its very next move. See
    ///   [`PointerSequence::next_hold_deadline`](crate::gesture::PointerSequence::next_hold_deadline).
    ///   A hold's `max_hold`, by contrast, *is* a stored transition and is
    ///   moved — by the gesture pass in (3).
    /// - Any clock a widget owns itself. A widget that reads the wall clock
    ///   directly rather than taking its deadline from the tree is outside this
    ///   door by construction, and there are several in `teksilo-widgets`.
    ///
    /// Dispatched over a no-op window sink; call
    /// [`advance_time_with_ops`](Self::advance_time_with_ops) from anywhere
    /// that holds a real one.
    pub fn advance_time(&mut self, duration: std::time::Duration) {
        let mut noop = crate::window::NoopWindowOps;
        self.advance_time_with_ops(duration, &mut noop);
    }

    /// [`advance_time`](Self::advance_time), over the caller's
    /// [`WindowOps`](crate::window::WindowOps) sink.
    ///
    /// A tick is a dispatch: a long press recognized here runs its handler,
    /// and that handler may open a window. Standalone,
    /// [`NoopWindowOps`](crate::window::NoopWindowOps) panics on
    /// `open_window` — the same trap `synthesise_tap_with_ops` exists for.
    pub fn advance_time_with_ops(
        &mut self,
        duration: std::time::Duration,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // (0) Take the tree off the wall clock *before* anything reads a
        // deadline, so this whole call is measured on one axis.
        self.enter_simulated_mode();

        // (1) Promote before the clock moves. An `animate_to` armed while the
        // clock read T must start at T; stamping it after the clock reached
        // T + d starts it d late and the caller's very next assertion is off
        // by exactly the duration they just advanced.
        self.process_pending_animations_at(self.sim_clock);

        // (2) The clock itself. A clock that has to be told (a `ManualClock`)
        // is moved here; an anchored one is read off `sim_clock` by
        // `input_now`. The overlay manager's mirror must be updated before any
        // pass below can dismiss, because `OverlayManager::dismiss` stamps the
        // fade's simulated start from it.
        self.sim_clock += duration;
        self.input_clock().advance(duration);
        self.overlay_manager.set_sim_clock(self.sim_clock);

        // (3) The input layer, in the order the real event loop uses: flings,
        // then press-feedback delays, then the gesture arenas. `tick_gestures`
        // owns all three — giving the fling pump its own call site here would
        // pump every live coast twice per advance.
        self.tick_gestures_with_ops(self.sim_clock, &mut *ops);

        // (4) The frame tick, and only if one was asked for: an unrequested
        // advance must not fire the per-frame observers. The delta is the
        // duration advanced, not a reading of `last_frame_time` — nothing was
        // rendered, and `last_frame_time` is the *render* pacing reference.
        if self.take_frame_tick_request() {
            let delta = duration.as_secs_f32().clamp(0.0, 0.1);
            self.frame_tick.set(delta);
        }

        // (5) Animations, at the new now and after the promotion in (1), so an
        // animation armed before this call has aged by exactly `duration`.
        self.animation_scheduler
            .tick(self.sim_clock, &self.arena, self.paint_epoch);

        // (6) The overlay and tooltip passes, in the order they depend on:
        // a dwell that ripens can show a tooltip, a delayed overlay that
        // matures can show a surface, and the dismissal passes below must see
        // both within this same virtual frame.
        self.process_tooltips();
        self.process_delayed_overlays();
        self.process_pointer_leave_overlays();
        self.process_auto_dismiss_overlays();
        self.process_overlay_fade_dismissals_sim();

        // (7) Last, so a signal written by a long-press handler, a coasting
        // fling's chained scroll or an overlay dismissal is flushed inside the
        // virtual frame that produced it rather than a frame later.
        self.process_state_changes(&mut *ops);
    }

    /// [`advance_time`](Self::advance_time), under the name the input side
    /// reads better by.
    ///
    /// An alias, not a second timeline: there is one clock, and moving the
    /// input axis is moving it.
    pub fn advance_input_time(&mut self, duration: std::time::Duration) {
        self.advance_time(duration);
    }

    /// Get the current simulated clock value.
    pub fn simulated_now(&self) -> std::time::Instant {
        self.sim_clock
    }

    /// Total number of live tooltip attachments, dead ones included.
    ///
    /// Distinct from `pending_tooltip_count`, which only counts entries with a
    /// running dwell. This is the raw table size — the number that must stay
    /// flat across rebuilds, since `attach_tooltip*` is called from `build()`
    /// and the table is scanned on every pointer move, every layout pass and
    /// once per widget in the accessibility walk.
    pub fn tooltip_entry_count(&self) -> usize {
        self.tooltips.len()
    }

    /// Every node inside `root` (inclusive) that Tab traversal would stop on:
    /// focusable, and not suppressed by a `tab_stop` flag on itself or any
    /// ancestor.
    /// Every widget the arena still holds — active, dormant and orphaned alike.
    ///
    /// The number a leak test must assert on. `active_widget_count` walks the
    /// tree from its roots and so cannot see the failure mode that matters
    /// here: a node kept alive in the arena with nothing pointing at it. A
    /// parentless orphan (tooltip content is `ctx.add`ed, hence parentless by
    /// construction) is invisible to every other count in this file, and to the
    /// accessibility tree, while still paying for itself in the arena's slotmap
    /// forever.
    /// Every node inside `root` (inclusive) that Tab traversal would stop on:
    /// focusable, and not suppressed by a `tab_stop` flag on itself or any
    /// ancestor.
    ///
    /// Pressing Tab and watching focus cannot answer this for a view that
    /// claims the key for its own navigation — `TableView` moves a cell cursor
    /// on Tab, so focus never moves and the traversal graph underneath stays
    /// invisible. A data view should expose exactly one stop however many rows
    /// are realized; more than one means a control inside a row has leaked
    /// into the Tab order, where its presence would track the scroll position.
    ///
    /// Membership matches the real collector
    /// ([`collect_scope_entries`](crate::widget_tree::WidgetTree)) exactly: a
    /// dormant node and a disabled subtree are both skipped, because Tab
    /// traversal returns at each. The two differ only in *shape* — the real
    /// collector groups a `traversal_scope` subtree so it can order it
    /// independently, and this returns one flat list in tree order — which is
    /// what a membership assertion wants.
    ///
    /// The guards are load-bearing rather than cosmetic. Without them this
    /// reports stops the traversal never visits, and a test asserting that a
    /// culled or collapsed subtree left the Tab ring passes or fails for a
    /// reason unrelated to the mechanism it is pinning.
    pub fn tab_stops_within(&self, root: WidgetId) -> Vec<WidgetId> {
        let mut out = Vec::new();
        self.collect_tab_stops_within(root, &mut out);
        out
    }

    fn collect_tab_stops_within(&self, id: WidgetId, out: &mut Vec<WidgetId>) {
        // Dormant: `collect_scope_entries` returns here, so the whole subtree
        // is off the traversal graph — a `Switcher`'s hidden branch, a closed
        // popover, a `visible_when` gate that went false.
        if !self.arena.is_active(id) {
            return;
        }
        let Some(node) = self.arena.get(id) else {
            return;
        };
        // Disabled: likewise a whole-subtree stop in the real collector.
        if node
            .enabled_state
            .as_ref()
            .map(|s| !s.get())
            .unwrap_or(false)
        {
            return;
        }
        if self.is_node_focusable(node) && self.tab_stop_effective(id) {
            out.push(id);
        }
        for &child in self.arena.children(id) {
            self.collect_tab_stops_within(child, out);
        }
    }

    pub fn widget_count(&self) -> usize {
        self.arena.len()
    }

    /// Tear down a widget and everything it owns — its subtree, its tooltip,
    /// and the parentless content it built with
    /// [`add_detached`](crate::build_context::BuildContext::add_detached).
    ///
    /// The application-facing door is `BuildContext::destroy_subtree`; this is
    /// the same call for tests that hold the tree directly.
    pub fn destroy_subtree_for_testing(&mut self, id: WidgetId) {
        self.destroy_subtree(id);
    }

    /// Panic unless every trace of a pointer interaction is gone.
    ///
    /// The one assertion a touch test ends with. A leak here is not a cosmetic
    /// untidiness: a surviving capture redelivers every later move to a widget
    /// nobody is pointing at, a surviving sequence lets a stale competitor win
    /// the *next* press, and a live recognizer entry starts the next contact
    /// mid-gesture. All three are silent until something much later
    /// misbehaves, which is why this is checked rather than reasoned about.
    ///
    /// A **hovering** pointer resting in the table is not a leak: a mouse that
    /// has been seen once keeps its entry for the life of the tree, and that
    /// entry is what every singular accessor reads. What must not survive is a
    /// pointer still *contacting* the surface, a capture, a sequence, or a
    /// gesture arena still following a contact.
    ///
    /// One thing the design lists is still absent: the touch-motion layer's own
    /// state — live pans, coasts, the window's pinch and the palm watches. The
    /// framework press *is* checked, at the bottom of this function.
    pub fn assert_no_leaked_pointer_state(&self) {
        let mut leaks: Vec<String> = Vec::new();
        for entry in self.pointers.iter() {
            let id = entry.info.id;
            if entry.is_contacting() {
                leaks.push(format!(
                    "{id:?} ({:?}) is still contacting the surface",
                    entry.info.kind
                ));
            }
            if let Some(captor) = entry.captured_by {
                leaks.push(format!("{id:?} still captures {captor:?}"));
            }
            if let Some(sequence) = entry.sequence.as_ref() {
                leaks.push(format!(
                    "{id:?} still has a sequence ({} member(s), winner {:?})",
                    sequence.members().len(),
                    sequence.winner()
                ));
            }
        }
        for &owner in &self.gesture_owners {
            if self
                .arena
                .get(owner)
                .and_then(|node| node.handlers.gesture_arena.as_ref())
                .is_some_and(|set| set.is_live())
            {
                leaks.push(format!(
                    "{owner:?} has a gesture arena still following a contact"
                ));
            }
        }
        // The framework press. Every exit — a release, a cancel, a peer claim —
        // goes through `end_press`, so a surviving record means one of them was
        // missed and some node is painted as held by a pointer that is gone.
        for id in self.arena.active_ids_iter() {
            if let Some(pointer) = self.pressed_by(id) {
                leaks.push(format!("{id:?} is still pressed by {pointer:?}"));
            }
        }
        assert!(
            leaks.is_empty(),
            "pointer state leaked after the interaction:\n  - {}",
            leaks.join("\n  - ")
        );
    }

    // ---------------------------------------------------------------
    // A21 — driving touch and pen from a test
    // ---------------------------------------------------------------
    //
    // Every helper below builds a `PointerSample` in exactly the shape
    // `teksilo-platform`'s translator builds one (`event_translation.rs`:
    // a contact holds `ButtonMask::PRIMARY` while it is down and reports
    // `Some(PointerButton::Primary)` on the two phases that change a
    // button; a stylus adds its axes) and pushes it through
    // `dispatch_pointer`, the one ingress door. Nothing here fabricates a
    // `WidgetEvent`: a helper that stepped around the router would test
    // the helper rather than the framework, and the hit-test-by-kind, the
    // sequence, the pan session, the palm watch and the pinch feed all
    // hang off that door.
    //
    // Every one of them puts the tree on the **simulated clock** first, and
    // then stamps its sample from [`input_now`](Self::input_now). Both halves
    // are load-bearing: a tree still on the wall clock stamps two consecutive
    // samples microseconds apart, so a `touch_drag` that means "travel 200 dp,
    // no time passes" would instead describe a flick at some thousands of dp
    // per second and hand off to a coast — differently on every machine. Once
    // simulated, the interval between two samples is exactly what
    // [`advance_input_time`](Self::advance_input_time) put there and nothing
    // else, which is what the rest of P14 is for.

    /// Mint a fresh contact identity, the way the platform layer does.
    ///
    /// A backend reuses its own contact ids the moment a finger lifts, so
    /// the allocator mints a `PointerId` per press; this is that call with
    /// a per-process os id, and it `end`s the mapping immediately so the
    /// allocator's live table does not grow across a test run.
    pub fn new_contact(&self) -> crate::pointer::PointerId {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_OS_ID: AtomicU64 = AtomicU64::new(1);
        let device = crate::pointer::BackendDeviceKey::new(0x7E57);
        let os_id = NEXT_OS_ID.fetch_add(1, Ordering::Relaxed);
        let alloc = crate::pointer::PointerIdAllocator::global();
        let id = alloc.begin(device, os_id);
        alloc.end(device, os_id);
        id
    }

    /// One direct-pointer sample, stamped on this tree's input timeline.
    ///
    /// `pub(super)` so a sibling module's tests can dispatch a contact of a kind
    /// the A21 helpers do not name — `touch_down` and `pen_down` cover the two
    /// kinds an application sees, and a gate that must refuse
    /// [`PointerKind::Unknown`](teksilo_tokens::PointerKind::Unknown) can only be
    /// tested by asking for one.
    pub(super) fn direct_sample(
        &self,
        id: crate::pointer::PointerId,
        kind: teksilo_tokens::PointerKind,
        phase: crate::pointer::PointerPhase,
        at: Point,
        down: bool,
    ) -> crate::pointer::PointerSample {
        use crate::pointer::PointerPhase;

        let mut pointer = crate::pointer::PointerInfo::touch(id, self.input_now());
        pointer.kind = kind;
        pointer.buttons = if down {
            crate::event::ButtonMask::PRIMARY
        } else {
            crate::event::ButtonMask::NONE
        };
        crate::pointer::PointerSample {
            pointer,
            phase,
            position: at,
            // The translator reports a button only where one changed.
            button: match phase {
                PointerPhase::Down | PointerPhase::Up => Some(PointerButton::Primary),
                PointerPhase::Move | PointerPhase::Cancel => None,
            },
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// A finger lands at `at`.
    pub fn touch_down(&mut self, pointer: crate::pointer::PointerId, at: Point) {
        self.enter_simulated_mode();
        let sample = self.direct_sample(
            pointer,
            teksilo_tokens::PointerKind::Touch,
            crate::pointer::PointerPhase::Down,
            at,
            true,
        );
        self.dispatch_pointer(sample);
    }

    /// That finger moves to `at`, still down.
    pub fn touch_move(&mut self, pointer: crate::pointer::PointerId, at: Point) {
        self.enter_simulated_mode();
        let sample = self.direct_sample(
            pointer,
            teksilo_tokens::PointerKind::Touch,
            crate::pointer::PointerPhase::Move,
            at,
            true,
        );
        self.dispatch_pointer(sample);
    }

    /// That finger lifts at `at`.
    pub fn touch_up(&mut self, pointer: crate::pointer::PointerId, at: Point) {
        self.enter_simulated_mode();
        let sample = self.direct_sample(
            pointer,
            teksilo_tokens::PointerKind::Touch,
            crate::pointer::PointerPhase::Up,
            at,
            false,
        );
        self.dispatch_pointer(sample);
    }

    /// The system revokes that finger (a `wl_touch.cancel`, a compositor
    /// grab). Not an [`touch_up`](Self::touch_up): the end position carries
    /// no meaning and no tap is completed.
    pub fn touch_cancel(&mut self, pointer: crate::pointer::PointerId, at: Point) {
        self.enter_simulated_mode();
        let sample = self.direct_sample(
            pointer,
            teksilo_tokens::PointerKind::Touch,
            crate::pointer::PointerPhase::Cancel,
            at,
            false,
        );
        self.dispatch_pointer(sample);
    }

    /// The live stylus's identity, minting one if the pen has not been seen.
    ///
    /// A stylus is singular and it *hovers*, so its table entry outlives a
    /// lift the way a mouse's does — which is exactly what lets the pen
    /// helpers take no id and still address one continuous session.
    fn pen_id(&mut self) -> crate::pointer::PointerId {
        self.pointers
            .iter()
            .find(|e| matches!(e.info.kind, teksilo_tokens::PointerKind::Pen(_)))
            .map(|e| e.info.id)
            .unwrap_or_else(|| self.new_contact())
    }

    /// One stylus sample: the direct-pointer shape plus the axes a digitizer
    /// reports.
    fn pen_sample(
        &mut self,
        phase: crate::pointer::PointerPhase,
        at: Point,
        pressure: Option<f32>,
        tilt: Option<(f32, f32)>,
        down: bool,
    ) -> crate::pointer::PointerSample {
        self.enter_simulated_mode();
        let id = self.pen_id();
        let mut sample = self.direct_sample(
            id,
            teksilo_tokens::PointerKind::Pen(teksilo_tokens::PenKind::default()),
            phase,
            at,
            down,
        );
        sample.pointer.axes.pressure = pressure;
        sample.pointer.axes.tilt = tilt;
        sample
    }

    /// The stylus tip touches down at `at`.
    ///
    /// `pressure` is normalised `0.0..=1.0`; `tilt` is `(tilt_x, tilt_y)` in
    /// degrees. Both are the axes a real digitizer reports, so a surface that
    /// reads [`PointerInfo::effective_pressure`](crate::pointer::PointerInfo::effective_pressure)
    /// sees what it would see from hardware.
    pub fn pen_down(&mut self, at: Point, pressure: f32, tilt: (f32, f32)) {
        let sample = self.pen_sample(
            crate::pointer::PointerPhase::Down,
            at,
            Some(pressure),
            Some(tilt),
            true,
        );
        self.dispatch_pointer(sample);
    }

    /// The stylus draws to `at`, still on the surface.
    pub fn pen_move(&mut self, at: Point, pressure: f32, tilt: (f32, f32)) {
        let sample = self.pen_sample(
            crate::pointer::PointerPhase::Move,
            at,
            Some(pressure),
            Some(tilt),
            true,
        );
        self.dispatch_pointer(sample);
    }

    /// The stylus lifts off at `at`. It stays in proximity — a pen hovers,
    /// so its entry survives the lift and the next `pen_move` continues the
    /// same session.
    pub fn pen_up(&mut self, at: Point, pressure: f32, tilt: (f32, f32)) {
        let sample = self.pen_sample(
            crate::pointer::PointerPhase::Up,
            at,
            Some(pressure),
            Some(tilt),
            false,
        );
        self.dispatch_pointer(sample);
    }

    /// The stylus moves in proximity without touching: no tip pressure, no
    /// button. The one direct-pointer hover in the framework.
    pub fn pen_hover(&mut self, at: Point) {
        let sample = self.pen_sample(
            crate::pointer::PointerPhase::Move,
            at,
            Some(0.0),
            None,
            false,
        );
        self.dispatch_pointer(sample);
    }

    /// A complete press-and-release at `at` by the named device, and the
    /// identity it used.
    ///
    /// The mouse arm is [`PointerId::MOUSE`](crate::pointer::PointerId::MOUSE)
    /// and the legacy `PointerDown`/`PointerUp` pair, so
    /// `tap_with(PointerKind::Mouse, ..)` is the pre-touch-programme click
    /// with a position rather than a widget id.
    pub fn tap_with(
        &mut self,
        kind: teksilo_tokens::PointerKind,
        at: Point,
    ) -> crate::pointer::PointerId {
        match kind {
            teksilo_tokens::PointerKind::Touch => {
                let id = self.new_contact();
                self.touch_down(id, at);
                self.touch_up(id, at);
                id
            }
            teksilo_tokens::PointerKind::Pen(_) => {
                self.pen_down(at, 0.5, (0.0, 0.0));
                let id = self.pen_id();
                self.pen_up(at, 0.0, (0.0, 0.0));
                id
            }
            _ => {
                self.enter_simulated_mode();
                self.pointer_down_button(at, PointerButton::Primary);
                self.pointer_up_button(at, PointerButton::Primary);
                crate::pointer::PointerId::MOUSE
            }
        }
    }

    /// Press at `at`, hold for exactly the kind's `long_press`, release.
    ///
    /// The hold comes from the active profile rather than a constant written
    /// here, and it is advanced *exactly* — the recognizer fires at
    /// `>= hold`, so a helper that added a safety margin would stop the
    /// threshold itself from ever being asserted.
    pub fn long_press_at(
        &mut self,
        kind: teksilo_tokens::PointerKind,
        at: Point,
    ) -> crate::pointer::PointerId {
        let hold = self.effective_theme.input.profile(kind).long_press;
        let id = match kind {
            teksilo_tokens::PointerKind::Touch => {
                let id = self.new_contact();
                self.touch_down(id, at);
                id
            }
            teksilo_tokens::PointerKind::Pen(_) => {
                self.pen_down(at, 0.5, (0.0, 0.0));
                self.pen_id()
            }
            _ => {
                self.enter_simulated_mode();
                self.pointer_down_button(at, PointerButton::Primary);
                crate::pointer::PointerId::MOUSE
            }
        };
        self.advance_input_time(hold);
        match kind {
            teksilo_tokens::PointerKind::Touch => self.touch_up(id, at),
            teksilo_tokens::PointerKind::Pen(_) => self.pen_up(at, 0.0, (0.0, 0.0)),
            _ => self.pointer_up_button(at, PointerButton::Primary),
        }
        id
    }

    /// One finger from `from` to `to` in `steps` evenly spaced moves, then a
    /// lift. Returns the contact's identity, so the caller can ask
    /// [`sequence_winner`](Self::sequence_winner) about it.
    ///
    /// The clock does **not** move: this is a drag, and a drag is decided by
    /// distance. Use [`fling`](Self::fling) when the speed is the point.
    pub fn touch_drag(
        &mut self,
        from: Point,
        to: Point,
        steps: usize,
    ) -> crate::pointer::PointerId {
        let id = self.new_contact();
        self.touch_down(id, from);
        let steps = steps.max(1);
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            self.touch_move(id, lerp_point(from, to, t));
        }
        self.touch_up(id, to);
        id
    }

    /// One finger from `from` to `to` over `over` of simulated time, released
    /// while still moving — the shape a coast is handed off from.
    ///
    /// Sampled at [`FLING_SAMPLE_INTERVAL`](Self::FLING_SAMPLE_INTERVAL) so
    /// the velocity tracker sees gaps under its `STOP_GAP` and at least its
    /// `MIN_SAMPLE_SIZE` of them; a flick described by two far-apart samples
    /// yields no velocity at all and would silently never fling.
    pub fn fling(
        &mut self,
        from: Point,
        to: Point,
        over: std::time::Duration,
    ) -> crate::pointer::PointerId {
        let interval = Self::FLING_SAMPLE_INTERVAL;
        let steps = (over.as_secs_f64() / interval.as_secs_f64()).ceil() as usize;
        let steps = steps.max(crate::kinetic::MIN_SAMPLE_SIZE);
        let per_step = over / steps as u32;

        let id = self.new_contact();
        self.touch_down(id, from);
        for step in 1..=steps {
            self.advance_input_time(per_step);
            let t = step as f32 / steps as f32;
            self.touch_move(id, lerp_point(from, to, t));
        }
        self.touch_up(id, to);
        id
    }

    /// The cadence [`fling`](Self::fling) samples at: one 60 Hz frame, which
    /// is under the velocity tracker's `STOP_GAP` and therefore never splits
    /// a flick into two unrelated runs.
    pub const FLING_SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_micros(16_667);

    /// Two fingers, from `a0`/`b0` to `a1`/`b1` in `steps` moves, then both
    /// lift. Returns their identities in the order they landed.
    ///
    /// Both contacts are down before either moves, which is what a pinch
    /// needs: the recognizer's reference span is the distance between the two
    /// landings.
    pub fn pinch(
        &mut self,
        a0: Point,
        b0: Point,
        a1: Point,
        b1: Point,
        steps: usize,
    ) -> (crate::pointer::PointerId, crate::pointer::PointerId) {
        let a = self.new_contact();
        let b = self.new_contact();
        self.touch_down(a, a0);
        self.touch_down(b, b0);
        let steps = steps.max(1);
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            self.touch_move(a, lerp_point(a0, a1, t));
            self.touch_move(b, lerp_point(b0, b1, t));
        }
        self.touch_up(a, a1);
        self.touch_up(b, b1);
        (a, b)
    }

    /// Switch the active [`TargetDensity`](teksilo_tokens::TargetDensity).
    ///
    /// The name A21 gives [`set_input_density`](Self::set_input_density); an
    /// alias, because density is one setting and there is one door to it.
    pub fn set_density(&mut self, density: teksilo_tokens::TargetDensity) {
        self.set_input_density(density);
    }

    /// The [`TouchAction`](crate::pointer::touch_action::TouchAction) in force
    /// at `id`: the intersection of every declaration from the root down to
    /// it.
    ///
    /// This is what a press landing on `id` would *freeze*. Distinct from
    /// [`sequence_touch_action`](Self::sequence_touch_action), which reports
    /// what a press already in flight froze — the two differ the moment a
    /// widget changes its declaration mid-press, which is the whole reason
    /// the value is frozen.
    pub fn touch_action_for(&self, id: WidgetId) -> crate::pointer::touch_action::TouchAction {
        self.effective_touch_action(id)
    }

    /// Mark a widget as needing repaint.
    pub fn mark_needs_paint(&mut self, id: WidgetId) {
        self.arena.mark_needs_paint(id);
    }

    /// Set a widget subtree as dormant.
    ///
    /// Goes through the tree's cancel-aware parking door, so a pointer working
    /// inside the subtree is cancelled rather than stranded on a widget the
    /// dispatcher will no longer reach.
    pub fn set_dormant(&mut self, id: WidgetId) {
        self.park_subtree(id);
        self.arena.mark_ancestors_need_layout(id);
        self.cached_frame = None;
        self.a11y_dirty = true;
    }

    /// Activate a dormant widget subtree.
    pub fn activate(&mut self, id: WidgetId) {
        self.arena.activate(id);
        self.arena.mark_ancestors_need_layout(id);
        self.cached_frame = None;
        self.a11y_dirty = true;
    }

    /// Invalidate all per-widget paint caches (paint AND post-paint) and
    /// the assembled frame cache. Forces every widget to repaint on the
    /// next `render()` call. Used by the glyph-atlas eviction recovery:
    /// after an eviction, any retained frame may hold quads whose atlas
    /// UVs now point at recycled slots.
    pub fn invalidate_all_paints(&mut self) {
        for id in self.arena.active_ids() {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_paint = true;
                node.cached_paint = None;
                node.cached_post_paint = None;
            }
        }
        self.cached_frame = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::Signal;
    use crate::test_widgets::{FillWidget, InsetWidget, StackWidget};
    use crate::widget_builder::WidgetBuilder;

    #[test]
    fn child_bounds_helper() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new());
        let parent = tree.add(InsetWidget::new(5.0).set_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let child_bounds = tree.child_bounds(parent, 0);
        assert_eq!(child_bounds.x, 5.0);
    }

    #[test]
    fn signal_get_set_and_derived() {
        let text = Signal::new(String::new());
        let is_empty = text.map(|value| value.is_empty());
        assert!(is_empty.get());
        text.set("hello".to_string());
        assert!(!is_empty.get());
    }

    #[test]
    fn advance_time_updates_simulated_clock() {
        let mut tree = WidgetTree::new();
        let start = tree.simulated_now();

        tree.advance_time(std::time::Duration::from_millis(500));
        let end = tree.simulated_now();

        assert_eq!(
            end.duration_since(start),
            std::time::Duration::from_millis(500)
        );
    }

    #[test]
    fn animate_to_interpolates_over_time() {
        let mut tree = WidgetTree::new();
        let owner = tree.add(FillWidget::new());
        let signal = Signal::<f32>::new_animated(0.0);
        tree.register_animated_signal(&signal, owner);

        signal.animate_to(
            100.0,
            std::time::Duration::from_millis(200),
            teksilo_tokens::Easing::Linear,
        );

        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (signal.get() - 50.0).abs() < 2.0,
            "at 50%: {}",
            signal.get()
        );

        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (signal.get() - 100.0).abs() < 0.1,
            "at 100%: {}",
            signal.get()
        );

        assert!(!tree.has_active_animations());
    }

    #[test]
    fn animate_to_with_easing() {
        let mut tree = WidgetTree::new();
        let owner = tree.add(FillWidget::new());
        let signal = Signal::<f32>::new_animated(0.0);
        tree.register_animated_signal(&signal, owner);

        signal.animate_to(
            100.0,
            std::time::Duration::from_millis(200),
            teksilo_tokens::Easing::EaseIn,
        );

        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (signal.get() - 25.0).abs() < 2.0,
            "ease-in at 50%: {}",
            signal.get()
        );
    }

    #[test]
    fn animate_to_replaces_in_flight() {
        let mut tree = WidgetTree::new();
        let owner = tree.add(FillWidget::new());
        let signal = Signal::<f32>::new_animated(0.0);
        tree.register_animated_signal(&signal, owner);

        signal.animate_to(
            100.0,
            std::time::Duration::from_millis(200),
            teksilo_tokens::Easing::Linear,
        );
        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!((signal.get() - 50.0).abs() < 2.0);

        signal.animate_to(
            0.0,
            std::time::Duration::from_millis(100),
            teksilo_tokens::Easing::Linear,
        );
        tree.tick_animations(std::time::Duration::from_millis(50));
        assert!(
            (signal.get() - 25.0).abs() < 3.0,
            "mid-replace: {}",
            signal.get()
        );

        tree.tick_animations(std::time::Duration::from_millis(50));
        assert!(
            (signal.get() - 0.0).abs() < 0.5,
            "end-replace: {}",
            signal.get()
        );
    }

    #[test]
    fn animation_marks_widgets_dirty() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        let signal = Signal::<f32>::new_animated(100.0);
        tree.register_animated_signal(&signal, widget);

        signal.bind_to(
            widget,
            tree.binding_registry(),
            crate::binding::BindingLevel::Relayout,
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        signal.animate_to(
            0.0,
            std::time::Duration::from_millis(100),
            teksilo_tokens::Easing::Linear,
        );

        tree.tick_animations(std::time::Duration::from_millis(50));
        assert!(tree.needs_redraw());
    }

    // -----------------------------------------------------------------
    // A21 — the touch / pen helpers
    // -----------------------------------------------------------------

    /// A finger holds `ButtonMask::PRIMARY` for as long as it is down.
    ///
    /// Normative, not cosmetic: every `accept_buttons` recognizer in the
    /// framework gates on `PRIMARY`, so a helper that reported an empty mask
    /// would make tap, drag, long-press and multi-tap invisible to a contact —
    /// and every touch test in the workspace would then be testing a device the
    /// platform layer does not produce (`event_translation.rs` sets the same
    /// mask).
    #[test]
    fn a_touch_helper_reports_the_primary_button_while_it_is_down() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Vec<(crate::event::ButtonMask, bool)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_pointer_event(move |_event, ctx| {
            let p = ctx.pointer();
            log.borrow_mut().push((p.buttons, p.kind.is_coarse()));
            crate::event::EventResponse::Ignored
        }));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let finger = tree.new_contact();
        let at = Point::new(50.0, 50.0);
        tree.touch_down(finger, at);
        tree.touch_move(finger, Point::new(60.0, 50.0));
        tree.touch_up(finger, Point::new(60.0, 50.0));

        let seen = seen.borrow();
        assert!(
            seen.iter().all(|(_, coarse)| *coarse),
            "all three are a finger"
        );
        assert_eq!(
            seen.iter().map(|(b, _)| *b).collect::<Vec<_>>(),
            vec![
                crate::event::ButtonMask::PRIMARY,
                crate::event::ButtonMask::PRIMARY,
                crate::event::ButtonMask::NONE,
            ],
            "down and move hold PRIMARY; the lift reports none"
        );
        tree.assert_no_leaked_pointer_state();
    }

    /// The stylus helpers carry the axes a digitizer reports, and a hover
    /// carries neither a button nor tip pressure.
    #[test]
    fn the_pen_helpers_carry_pressure_and_tilt_and_hover_carries_neither() {
        use std::cell::RefCell;
        use std::rc::Rc;

        type Sample = (
            Option<f32>,
            Option<(f32, f32)>,
            crate::event::ButtonMask,
            f32,
        );
        let seen: Rc<RefCell<Vec<Sample>>> = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_pointer_event(move |_event, ctx| {
            let p = ctx.pointer();
            log.borrow_mut().push((
                p.axes.pressure,
                p.axes.tilt,
                p.buttons,
                p.effective_pressure(),
            ));
            crate::event::EventResponse::Ignored
        }));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.pen_hover(Point::new(40.0, 40.0));
        tree.pen_down(Point::new(50.0, 50.0), 0.75, (12.0, -30.0));
        tree.pen_up(Point::new(50.0, 50.0), 0.0, (12.0, -30.0));

        let seen = seen.borrow();
        assert_eq!(
            seen[0],
            (Some(0.0), None, crate::event::ButtonMask::NONE, 0.0),
            "a hover reports no tilt, no button and no tip pressure"
        );
        assert_eq!(
            seen[1],
            (
                Some(0.75),
                Some((12.0, -30.0)),
                crate::event::ButtonMask::PRIMARY,
                0.75
            ),
            "the tip's pressure and tilt reach the handler"
        );
        assert_eq!(
            seen[2].2,
            crate::event::ButtonMask::NONE,
            "the lift holds nothing"
        );
        tree.assert_no_leaked_pointer_state();
    }

    /// A pen keeps one identity across a lift: it hovers, so its entry outlives
    /// the tip leaving the surface and the helpers address one session.
    #[test]
    fn the_pen_helpers_address_one_session_across_a_lift() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.pen_down(Point::new(50.0, 50.0), 0.5, (0.0, 0.0));
        let first = tree
            .live_pointers()
            .find(|p| matches!(p.kind, teksilo_tokens::PointerKind::Pen(_)))
            .map(|p| p.id)
            .expect("the pen was admitted");
        tree.pen_up(Point::new(50.0, 50.0), 0.0, (0.0, 0.0));
        tree.pen_hover(Point::new(60.0, 50.0));
        let second = tree
            .live_pointers()
            .find(|p| matches!(p.kind, teksilo_tokens::PointerKind::Pen(_)))
            .map(|p| p.id)
            .expect("the pen is still in proximity");
        assert_eq!(first, second, "one stylus, one identity");
    }

    /// A test scrollable: the vertical `scroll_container` claim — kinetic, as
    /// `ScrollArea`'s is, since a claim that is not kinetic never hands off to
    /// a coast — plus the `on_scroll` contract `teksilo-widgets` implements:
    /// absorb and answer `Handled`.
    fn flingable(offset: crate::signal::Signal<f32>) -> impl Widget + 'static {
        FillWidget::new()
            .scroll_container(crate::pointer::touch_action::PanAxes::Y)
            .on_scroll(move |event, _ctx| {
                let crate::event::WidgetEvent::Scroll { delta, .. } = event else {
                    return crate::event::EventResponse::Ignored;
                };
                let dy = match *delta {
                    crate::event::ScrollDelta::Pixels { y, .. } => y,
                    crate::event::ScrollDelta::Lines { y, .. } => y * 20.0,
                };
                offset.set((offset.get() + dy).clamp(0.0, 10_000.0));
                crate::event::EventResponse::Handled
            })
    }

    /// `fling` hands off to a coast and `touch_drag` over the same path does
    /// not.
    ///
    /// The pair is the assertion: both travel the same distance, and only the
    /// one that spends simulated time between its samples produces a velocity.
    /// A `fling` helper that forgot to advance the clock would still pan the
    /// scroller, so asserting the scroll alone would not notice.
    #[test]
    fn fling_coasts_where_the_same_drag_does_not() {
        let offset = crate::signal::Signal::new(0.0_f32);
        let mut tree = WidgetTree::new();
        let scroller = tree.add(flingable(offset.clone()));
        tree.layout(SizeProposal::exact(200.0, 400.0));

        tree.touch_drag(Point::new(100.0, 300.0), Point::new(100.0, 100.0), 8);
        assert!(offset.get() > 0.0, "the drag scrolled: {}", offset.get());
        assert!(
            !tree.is_flinging(scroller),
            "…but a drag with no time between its samples has no velocity"
        );
        tree.assert_no_leaked_pointer_state();

        let offset = crate::signal::Signal::new(0.0_f32);
        let mut tree = WidgetTree::new();
        let scroller = tree.add(flingable(offset.clone()));
        tree.layout(SizeProposal::exact(200.0, 400.0));

        tree.fling(
            Point::new(100.0, 300.0),
            Point::new(100.0, 100.0),
            std::time::Duration::from_millis(50),
        );
        assert!(
            tree.is_flinging(scroller),
            "200 dp in 50 ms is a flick and hands off to a coast"
        );
        let at_release = offset.get();
        tree.advance_time(std::time::Duration::from_millis(100));
        assert!(
            offset.get() > at_release,
            "and the one clock moves it: {at_release} -> {}",
            offset.get()
        );
    }

    /// `pinch` produces a real two-contact pinch stream through the single
    /// ingress.
    #[test]
    fn pinch_drives_a_two_contact_pinch() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let phases: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let log = phases.clone();
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_pinch(move |phase, _ctx| {
            log.borrow_mut().push(match phase {
                crate::gesture::PinchPhase::Started { .. } => "started",
                crate::gesture::PinchPhase::Changed { .. } => "changed",
                crate::gesture::PinchPhase::Ended { .. } => "ended",
                crate::gesture::PinchPhase::Cancelled { .. } => "cancelled",
            });
        }));
        tree.layout(SizeProposal::exact(400.0, 400.0));

        tree.pinch(
            Point::new(180.0, 200.0),
            Point::new(220.0, 200.0),
            Point::new(100.0, 200.0),
            Point::new(300.0, 200.0),
            6,
        );

        let phases = phases.borrow();
        assert!(
            phases.contains(&"started"),
            "the spread started a pinch: {phases:?}"
        );
        assert!(
            phases.contains(&"changed"),
            "…and reported its changes: {phases:?}"
        );
        tree.assert_no_leaked_pointer_state();
    }

    /// `long_press_at` holds for exactly the profile's `long_press` — not a
    /// millisecond more.
    ///
    /// The recognizer fires at `>= hold`, so holding for exactly it is what
    /// makes the threshold itself observable: a helper that padded the wait
    /// would pass with the hold set to anything shorter.
    #[test]
    fn long_press_at_holds_for_exactly_the_profiles_hold() {
        use std::cell::Cell;
        use std::rc::Rc;

        for kind in [
            teksilo_tokens::PointerKind::Mouse,
            teksilo_tokens::PointerKind::Touch,
            teksilo_tokens::PointerKind::Pen(teksilo_tokens::PenKind::Pen),
        ] {
            let fired = Rc::new(Cell::new(0));
            let f = fired.clone();
            let mut tree = WidgetTree::new();
            tree.add(FillWidget::new().on_long_press(move |_e, _c| f.set(f.get() + 1)));
            tree.layout(SizeProposal::exact(100.0, 100.0));

            let before = tree.simulated_now();
            tree.long_press_at(kind, Point::new(50.0, 50.0));
            assert_eq!(fired.get(), 1, "{kind:?} held long enough, once");
            assert_eq!(
                tree.simulated_now().duration_since(before),
                tree.effective_theme.input.profile(kind).long_press,
                "{kind:?}: the helper advanced exactly the profile's hold"
            );
            tree.assert_no_leaked_pointer_state();
        }
    }

    /// `tap_with` completes a tap for every device.
    #[test]
    fn tap_with_taps_for_every_device() {
        use std::cell::Cell;
        use std::rc::Rc;

        for kind in [
            teksilo_tokens::PointerKind::Mouse,
            teksilo_tokens::PointerKind::Touch,
            teksilo_tokens::PointerKind::Pen(teksilo_tokens::PenKind::Pen),
        ] {
            let taps = Rc::new(Cell::new(0));
            let t = taps.clone();
            let mut tree = WidgetTree::new();
            tree.add(FillWidget::new().on_tap(move |_e, _c| t.set(t.get() + 1)));
            tree.layout(SizeProposal::exact(100.0, 100.0));
            tree.tap_with(kind, Point::new(50.0, 50.0));
            assert_eq!(taps.get(), 1, "{kind:?} tapped once");
            tree.assert_no_leaked_pointer_state();
        }
    }

    /// `touch_action_for` reports the **declaration** in force at a node — the
    /// root-to-target intersection — which is a different question from
    /// `sequence_touch_action`'s "what did this press freeze".
    ///
    /// The two differ the moment a widget changes its declaration mid-press,
    /// which is the whole reason the value is frozen at all.
    #[test]
    fn touch_action_for_reads_the_declaration_and_the_sequence_reads_the_freeze() {
        use crate::pointer::touch_action::TouchAction;

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        let outer = tree.add(
            StackWidget::new()
                .child(leaf)
                .touch_action(TouchAction::PAN_Y),
        );
        tree.layout(SizeProposal::exact(100.0, 100.0));

        assert_eq!(tree.touch_action_for(outer), TouchAction::PAN_Y);
        assert_eq!(
            tree.touch_action_for(leaf),
            TouchAction::PAN_Y,
            "the fold runs root to target"
        );

        let finger = tree.new_contact();
        tree.touch_down(finger, Point::new(50.0, 50.0));
        assert_eq!(tree.sequence_touch_action(finger), TouchAction::PAN_Y);

        // The declaration changes under the live press.
        tree.arena
            .get_mut(outer)
            .expect("the node is live")
            .touch_action = TouchAction::NONE;
        assert_eq!(
            tree.touch_action_for(leaf),
            TouchAction::NONE,
            "the declaration moved"
        );
        assert_eq!(
            tree.sequence_touch_action(finger),
            TouchAction::PAN_Y,
            "…and the press keeps what it froze"
        );
        tree.touch_up(finger, Point::new(50.0, 50.0));
        tree.assert_no_leaked_pointer_state();
    }

    /// `set_density` is the one density door under A21's name for it.
    #[test]
    fn set_density_is_set_input_density() {
        let mut a = WidgetTree::new();
        let mut b = WidgetTree::new();
        a.set_density(teksilo_tokens::TargetDensity::Touch);
        b.set_input_density(teksilo_tokens::TargetDensity::Touch);
        assert_eq!(a.theme().input, b.theme().input);
        assert_eq!(
            a.theme().input.density,
            teksilo_tokens::TargetDensity::Touch
        );
    }
}
