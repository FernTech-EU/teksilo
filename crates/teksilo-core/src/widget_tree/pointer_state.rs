// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer / hover / capture / gesture-owner state: the per-node probes
//! the router consults, the ancestor drag-observer bookkeeping, and the
//! gesture-recognizer tick that drives them.

use super::*;
use crate::pointer::touch_action::{Axis, PanClaim, TouchAction};

impl WidgetTree {
    /// This tree's input clock — the one source of
    /// [`EventTime`](crate::pointer::EventTime)s for everything the pointer
    /// path does.
    ///
    /// A [`MonotonicClock`](crate::pointer::clock::MonotonicClock) anchored at
    /// the tree epoch by default. The epoch is the same `Instant`
    /// [`simulated_now`](Self::simulated_now) starts at, so the input timeline
    /// and the simulated animation timeline are one axis rather than two.
    pub fn input_clock(&self) -> std::rc::Rc<dyn crate::pointer::clock::InputClock> {
        self.input_clock.clone()
    }

    /// Replace the input clock.
    ///
    /// A headless test installs a
    /// [`ManualClock`](crate::pointer::clock::ManualClock) here so gesture
    /// deadlines fire exactly when it says, with no sleeping and no dependence
    /// on how long the test itself took.
    pub fn set_input_clock(&mut self, clock: std::rc::Rc<dyn crate::pointer::clock::InputClock>) {
        self.input_clock = clock;
    }

    /// The current time on this tree's input timeline.
    pub fn input_now(&self) -> crate::pointer::EventTime {
        self.input_clock.now()
    }

    /// Whether `id` carries a drag or swipe handler (hence gets a drag/swipe
    /// recognizer once its arena is built).
    fn widget_has_drag(&self, id: WidgetId) -> bool {
        self.arena
            .get(id)
            .map(|n| n.any_handler(|h| h.on_drag.is_some() || h.on_swipe.is_some()))
            .unwrap_or(false)
    }

    /// Whether `id` is a gesture dead-zone boundary — a press inside its
    /// subtree must not arm a drag/swipe on any ancestor above it. See
    /// [`WidgetNode::gesture_dead_zone`](crate::arena::WidgetNode::gesture_dead_zone).
    fn is_gesture_dead_zone(&self, id: WidgetId) -> bool {
        self.arena
            .get(id)
            .map(|n| n.gesture_dead_zone)
            .unwrap_or(false)
    }

    /// Whether `id` is a keyboard-capture surface — while focused it
    /// receives every `KeyDown` raw, bypassing shortcut resolution. See
    /// [`WidgetNode::keyboard_capture`](crate::arena::WidgetNode::keyboard_capture).
    pub(super) fn is_keyboard_capture(&self, id: WidgetId) -> bool {
        self.arena
            .get(id)
            .map(|n| n.keyboard_capture)
            .unwrap_or(false)
    }

    /// On `PointerDown`, when a descendant has captured the pointer for a
    /// non-drag gesture (a tap / long-press), arm every strict ancestor that
    /// carries a drag/swipe recognizer so an ancestor drag can still begin
    /// once the pointer moves past threshold — the tap-vs-drag disambiguation
    /// across the hit-path. Without this a descendant `on_tap` permanently
    /// shadows an ancestor `on_drag` (the bubble stops + capture routes every
    /// move to the descendant alone).
    ///
    /// Skipped when the captured widget can itself drag: the innermost drag
    /// owns the gesture, so no ancestor observation.
    pub(super) fn arm_drag_observers(
        &mut self,
        captured: WidgetId,
        down_event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        self.drag_observers.clear();
        if self.widget_has_drag(captured) {
            return;
        }
        // The press is inside a gesture dead zone (the captured control *is* the
        // dead zone) → arm no ancestor drag at all.
        if self.is_gesture_dead_zone(captured) {
            return;
        }
        let mut observers = Vec::new();
        let mut current = self.arena.parent(captured);
        while let Some(id) = current {
            // A dead-zone boundary stops the walk: ancestors AT or ABOVE it are
            // never armed, so a control inside the dead zone can never start the
            // ancestor's drag (the robust fix for "clicking a header button +
            // a few px of jitter drags the whole panel").
            if self.is_gesture_dead_zone(id) {
                break;
            }
            if self.widget_has_drag(id) {
                // Build the arena (the bubble never reached this ancestor) and
                // feed it the press so its DragRecognizer records the origin.
                {
                    let WidgetTree {
                        arena,
                        gesture_owners,
                        ..
                    } = self;
                    if let Some(node) = arena.get_mut(id) {
                        Self::ensure_gesture_arena(node, id, gesture_owners);
                    }
                }
                self.observe_drag_on_ancestor(id, down_event, ops);
                observers.push(id);
            }
            current = self.arena.parent(id);
        }
        self.drag_observers = observers;
    }

    /// The pointer sequence ended (a tap / plain release) WITHOUT the armed
    /// ancestor drag latching. Feed the terminating `Up` to each armed ancestor
    /// so its `DragRecognizer` clears the press origin it recorded when it was
    /// armed on `PointerDown` — otherwise a later *hover* move would cross the
    /// drag threshold and start a phantom drag. This matters because the press
    /// was captured by an interactive descendant (e.g. a card's read-only
    /// `RichTextEditor`), so the ancestor's own arena never saw this `Up` on
    /// its own and its recognizer would stay armed indefinitely. Also discards
    /// the observer list.
    pub(super) fn release_drag_observers(
        &mut self,
        up_event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if self.drag_observers.is_empty() {
            return;
        }
        let observers = std::mem::take(&mut self.drag_observers);
        for id in &observers {
            // An `Up` while the recognizer is not mid-drag resolves it to
            // `Failed` and clears `down_position` — no gesture is produced, so
            // this only tidies recognizer state.
            self.observe_drag_on_ancestor(*id, up_event, ops);
        }
    }

    /// On a captured `PointerMove`, feed the move to each armed ancestor drag
    /// observer (innermost first). If one latches a drag, it has already called
    /// `start_drag` (so `active_drag` now owns the pointer) — stop observing.
    pub(super) fn advance_drag_observers(
        &mut self,
        move_event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if self.drag_observers.is_empty() {
            return;
        }
        let observers = std::mem::take(&mut self.drag_observers);
        for id in &observers {
            let recognized = self.observe_drag_on_ancestor(*id, move_event, ops);
            if recognized || self.active_drag.is_some() {
                // A drag latched on this ancestor — it now owns the pointer.
                return;
            }
        }
        // No drag yet — keep observing on the next move.
        self.drag_observers = observers;
    }

    /// Feed one raw pointer event to `id`'s gesture arena WITHOUT firing its
    /// `on_pointer_event` or taking the implicit capture (the descendant
    /// already holds it). Returns `true` if the arena recognized a gesture
    /// (a drag/swipe latched), in which case it is dispatched so the
    /// `on_drag` handler's `start_drag` runs and `active_drag` takes over.
    fn observe_drag_on_ancestor(
        &mut self,
        id: WidgetId,
        event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        let localized = self.localize_event(id, event);
        let event = localized.as_ref().unwrap_or(event);
        let raw = match event {
            WidgetEvent::PointerDown {
                position,
                button,
                modifiers,
            } => crate::gesture::RawPointerEvent::Down {
                position: *position,
                button: *button,
                modifiers: *modifiers,
            },
            WidgetEvent::PointerMove { position } => crate::gesture::RawPointerEvent::Move {
                position: *position,
            },
            WidgetEvent::PointerUp {
                position,
                button,
                modifiers,
            } => crate::gesture::RawPointerEvent::Up {
                position: *position,
                button: *button,
                modifiers: *modifiers,
            },
            _ => return false,
        };
        let mut ctx = self.make_event_context(&mut *ops);
        let WidgetTree { arena, .. } = self;
        let recognized = if let Some(node) = arena.get_mut(id) {
            if let Some(arena_ref) = node.handlers.gesture_arena.as_mut() {
                if let Some(gesture) = arena_ref.process(&raw) {
                    Self::dispatch_recognized_gesture(node, gesture, &mut ctx);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        self.collect_from_ctx(ctx, id);
        recognized
    }

    /// The clock a newly promoted animation must be stamped with: the same one
    /// the scheduler will later be ticked against.
    ///
    /// Normally the wall clock. But once [`tick_animations`](Self::tick_animations)
    /// has driven this tree, the scheduler is *only* ever ticked at
    /// [`Self::sim_clock`] — so an animation stamped `Instant::now()` is measured
    /// against a clock that may never reach its start. A headless test
    /// interleaving `layout()` (which promotes) with `tick_animations()` (which
    /// ticks) advances the two clocks independently: simulated time by whatever
    /// the test asks for, real time by however long the test actually takes. The
    /// moment real time overtakes simulated time, every animation armed from then
    /// on has a start in the scheduler's future and its progress **freezes** —
    /// not slowly, completely, and no number of further ticks recovers it.
    ///
    /// That made animated layout tests fail as a function of machine load rather
    /// than of behaviour: green run alone or on a couple of threads, red once the
    /// runner filled the cores and each test's wall-clock time stretched past the
    /// simulated time it was asking for. The overlay manager already keeps its
    /// real and simulated timestamps apart for this reason; animations now agree
    /// on one clock the same way.
    pub(super) fn animation_clock(&self) -> std::time::Instant {
        if self.sim_driven {
            self.sim_clock
        } else {
            std::time::Instant::now()
        }
    }

    /// Advance time-driven gesture recognizers (currently only
    /// [`crate::gesture::LongPressRecognizer`]) across every widget that
    /// has a gesture arena. Must be called by the event loop on each
    /// wake-up; otherwise long-press will never fire during an idle hold.
    ///
    /// When a recognizer transitions to `Recognized`, the corresponding
    /// handler on the owning widget is invoked with a fresh
    /// [`EventContext`], and any commands / overlay requests it emits are
    /// collected through the normal post-event path.
    pub fn tick_gestures(&mut self, now: std::time::Instant) {
        let mut noop = crate::window::NoopWindowOps;
        self.tick_gestures_with_ops(now, &mut noop);
    }

    /// App-facing variant of [`tick_gestures`](Self::tick_gestures)
    /// that accepts a real [`WindowOps`](crate::window::WindowOps)
    /// sink so gesture-recognized handlers can call the multi-window
    /// API synchronously.
    pub fn tick_gestures_with_ops(
        &mut self,
        now: std::time::Instant,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Snapshot the gesture-owners set into the reusable scratch.
        // Previously this iterated every active widget; in practice
        // only a tiny fraction carry a gesture arena, so visiting the
        // rest was pure overhead.
        // `mem::take` lets the loop borrow `&mut self` for
        // `make_event_context` etc. without conflicting with the
        // scratch buffer; we put the storage back at the end.
        let mut ids = std::mem::take(&mut self.active_ids_scratch);
        ids.clear();
        ids.extend(
            self.gesture_owners
                .iter()
                .copied()
                .filter(|id| self.arena.is_active(*id)),
        );
        for &id in &ids {
            let gesture = match self.arena.get_mut(id) {
                Some(node) => node
                    .handlers
                    .gesture_arena
                    .as_mut()
                    .and_then(|arena| arena.tick(now)),
                None => None,
            };
            let Some(gesture) = gesture else { continue };

            let mut ctx = self.make_event_context(&mut *ops);
            if let Some(node) = self.arena.get_mut(id) {
                Self::dispatch_recognized_gesture(node, gesture, &mut ctx);
            }
            self.collect_from_ctx(ctx, id);
            self.arena.mark_needs_paint(id);
        }
        self.active_ids_scratch = ids;
    }

    /// Earliest wall-clock deadline at which any active gesture arena
    /// needs [`WidgetTree::tick_gestures`] called — typically a pending
    /// long-press timeout. Returns `None` when no recognizer is waiting.
    pub fn next_gesture_deadline(&self) -> Option<std::time::Instant> {
        // Iterate just the widgets that actually carry a gesture arena.
        // `filter` for `is_active` skips dormant entries that may still
        // be in the set after a hide-without-detach.
        self.gesture_owners
            .iter()
            .copied()
            .filter(|id| self.arena.is_active(*id))
            .filter_map(|id| self.arena.get(id))
            .filter_map(|node| node.handlers.gesture_arena.as_ref())
            .filter_map(|arena| arena.next_deadline())
            .min()
    }

    /// The [`TouchAction`] permitted for `target`: every node's own
    /// declaration from the root down to `target` (inclusive), intersected.
    /// An ancestor's `NONE` wins no matter what a descendant declares —
    /// intersection is absorbing at `NONE` (see
    /// `crate::pointer::touch_action`), so this needs no early exit to get
    /// that right; it just folds the whole chain.
    ///
    /// One of the two path folds the arbitration package consumes. **Not
    /// called from any dispatch path yet.**
    #[allow(dead_code)] // plumbing for the P08 arbitration package; exercised by tests today
    pub(crate) fn effective_touch_action(&self, target: WidgetId) -> TouchAction {
        let mut chain = Vec::new();
        let mut current = Some(target);
        while let Some(id) = current {
            chain.push(id);
            current = self.arena.parent(id);
        }
        // `chain` is target..=root (innermost first); fold root-to-target so
        // the read matches the CSS `touch-action` model this mirrors — an
        // ancestor's declaration is applied before a descendant's narrows it
        // further. `intersect` is commutative and associative, so the fold
        // order can never change the *answer*, only which step "loses" reads
        // as the natural one.
        chain
            .iter()
            .rev()
            .map(|&id| {
                self.arena
                    .get(id)
                    .map(|n| n.touch_action)
                    .unwrap_or(TouchAction::AUTO)
            })
            .fold(TouchAction::AUTO, TouchAction::intersect)
    }

    /// Every [`PanClaim`] from `target` up to the root, **innermost first**
    /// — the order a boundary pan chains along (a nested scrollable hits its
    /// edge and hands off to its container), so it is normative. A claimant
    /// is excluded entirely — never narrowed — when `allowed` forbids any
    /// axis it declares.
    ///
    /// The second of the two path folds the arbitration package consumes.
    /// **Not called from any dispatch path yet.**
    #[allow(dead_code)] // plumbing for the P08 arbitration package; exercised by tests today
    pub(crate) fn pan_candidates(
        &self,
        target: WidgetId,
        allowed: TouchAction,
    ) -> Vec<(WidgetId, PanClaim)> {
        let mut result = Vec::new();
        let mut current = Some(target);
        while let Some(id) = current {
            if let Some(claim) = self.arena.get(id).and_then(|n| n.pan_claim) {
                let x_ok = !claim.axes.contains(Axis::X) || allowed.allows_pan_x();
                let y_ok = !claim.axes.contains(Axis::Y) || allowed.allows_pan_y();
                if x_ok && y_ok {
                    result.push((id, claim));
                }
            }
            current = self.arena.parent(id);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;

    #[test]
    fn destroy_subtree_clears_dangling_pointer_capture() {
        use crate::event::{EventResponse, Modifiers, PointerButton};
        use crate::test_widgets::StackWidget;

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_pointer_event(|event, ctx| {
            if matches!(event, WidgetEvent::PointerDown { .. }) {
                ctx.capture_pointer();
            }
            EventResponse::Ignored
        }));
        let parent = tree.add(StackWidget::new().add_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // A press inside the child captures the pointer to it.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            tree.pointer_captured_by,
            Some(child),
            "PointerDown handler should have captured the pointer"
        );

        // Tearing down the capturing subtree (e.g. mid-gesture rebuild) must
        // release the capture eagerly rather than leaving a dangling id that
        // swallows every later Move/Up until the next layout pass heals it.
        tree.destroy_subtree(parent);
        assert_eq!(
            tree.pointer_captured_by, None,
            "destroy_subtree must clear a capture anchored at a destroyed widget"
        );
    }
}

/// The two path folds `effective_touch_action` / `pan_candidates` declare
/// for the arbitration package (P08) — pure plumbing, exercised here in
/// isolation since nothing dispatches through them yet.
#[cfg(test)]
mod touch_action_tests {
    use super::*;
    use crate::pointer::touch_action::PanAxes;
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;

    /// A 20-deep chain, root to target, where one mid-level node declares
    /// `PAN_Y` and another declares `PINCH_ZOOM`. Neither permission is
    /// shared by the other, so the intersection collapses to `NONE` — the
    /// clearest possible demonstration that the fold really intersects the
    /// *whole* chain rather than reading only the nearest declaration.
    #[test]
    fn effective_touch_action_intersects_the_whole_root_to_target_chain() {
        let mut tree = WidgetTree::new();
        let mut chain = vec![tree.add(FillWidget::new())]; // depth 0: the root
        for depth in 1..20usize {
            let parent = *chain.last().expect("root was pushed");
            let id = if depth == 5 {
                tree.add_child(parent, FillWidget::new().touch_action(TouchAction::PAN_Y))
            } else if depth == 12 {
                tree.add_child(
                    parent,
                    FillWidget::new().touch_action(TouchAction::PINCH_ZOOM),
                )
            } else {
                tree.add_child(parent, FillWidget::new())
            };
            chain.push(id);
        }
        assert_eq!(chain.len(), 20, "the path must be 20 nodes deep");
        let target = *chain.last().expect("chain is non-empty");
        tree.layout(SizeProposal::exact(50.0, 50.0));

        assert_eq!(
            tree.effective_touch_action(target),
            TouchAction::NONE,
            "PAN_Y at depth 5 and PINCH_ZOOM at depth 12 share no permission"
        );

        // Every node above depth 5 (inclusive) is untouched: the plain
        // `AUTO` prefix intersects down to exactly `PAN_Y`.
        assert_eq!(tree.effective_touch_action(chain[5]), TouchAction::PAN_Y);
    }

    /// `pan_candidates` walks target-to-root (innermost first) and drops a
    /// claimant whose declared axes the allowed action forbids, rather than
    /// narrowing it.
    #[test]
    fn pan_candidates_orders_innermost_first_and_filters_by_allowed_axes() {
        let mut tree = WidgetTree::new();
        // root claims X; an unclaimed node in between; target (innermost)
        // claims Y.
        let root = tree.add(FillWidget::new().scroll_container(PanAxes::X));
        let mid = tree.add_child(root, FillWidget::new());
        let target = tree.add_child(mid, FillWidget::new().scroll_container(PanAxes::Y));
        tree.layout(SizeProposal::exact(50.0, 50.0));

        let x_claim = PanClaim {
            axes: PanAxes::X,
            devices: teksilo_tokens::PointerKindMask::DIRECT,
            kinetic: true,
        };
        let y_claim = PanClaim {
            axes: PanAxes::Y,
            devices: teksilo_tokens::PointerKindMask::DIRECT,
            kinetic: true,
        };

        // Both axes allowed: both claims survive, innermost (target) first.
        assert_eq!(
            tree.pan_candidates(target, TouchAction::PAN),
            vec![(target, y_claim), (root, x_claim)]
        );

        // Only PAN_X allowed: target's Y-axis claim is forbidden and
        // excluded outright; root's X-axis claim still survives.
        assert_eq!(
            tree.pan_candidates(target, TouchAction::PAN_X),
            vec![(root, x_claim)]
        );

        // Only PAN_Y allowed: the reverse — root's claim is excluded,
        // target's survives.
        assert_eq!(
            tree.pan_candidates(target, TouchAction::PAN_Y),
            vec![(target, y_claim)]
        );
    }
}

/// The one-clock rule: the input timeline and the tree's simulated clock are
/// one axis, seeded from one epoch.
#[cfg(test)]
mod clock_tests {
    use super::*;
    use crate::pointer::EventTime;
    use crate::pointer::clock::ManualClock;

    /// The whole point of taking the epoch as a parameter rather than
    /// capturing it: `EventTime::ZERO` and the tree's simulated clock name the
    /// *same* instant, so one `advance_time` moves gesture deadlines and
    /// animations against the same origin.
    ///
    /// If this ever fails, the two timelines have drifted apart and a test that
    /// advances one has silently stopped advancing the other.
    #[test]
    fn the_input_clock_shares_the_trees_epoch() {
        let tree = WidgetTree::new();
        assert_eq!(
            tree.input_clock().epoch(),
            Some(tree.simulated_now()),
            "the input clock must be anchored at the instant sim_clock starts from"
        );
    }

    /// …and stays one axis as simulated time moves: the offset between the
    /// simulated clock and the epoch is exactly what was advanced.
    #[test]
    fn advancing_simulated_time_moves_along_the_input_axis() {
        let mut tree = WidgetTree::new();
        let epoch = tree
            .input_clock()
            .epoch()
            .expect("the default input clock is monotonic");

        tree.advance_time(std::time::Duration::from_millis(400));
        assert_eq!(
            tree.simulated_now().duration_since(epoch),
            std::time::Duration::from_millis(400)
        );

        tree.advance_time(std::time::Duration::from_millis(100));
        assert_eq!(
            tree.simulated_now().duration_since(epoch),
            std::time::Duration::from_millis(500)
        );
    }

    /// A test can take the input timeline over entirely.
    #[test]
    fn a_manual_clock_replaces_the_default() {
        let mut tree = WidgetTree::new();
        let manual = std::rc::Rc::new(ManualClock::new(EventTime::from_millis(30)));
        tree.set_input_clock(manual.clone());

        assert_eq!(tree.input_now(), EventTime::from_millis(30));
        manual.advance(std::time::Duration::from_millis(70));
        assert_eq!(tree.input_now(), EventTime::from_millis(100));
        // Reading does not move it — a manual clock is only moved by its owner.
        assert_eq!(tree.input_now(), EventTime::from_millis(100));
    }

    /// The default clock actually reads the wall clock, so a real window's
    /// gestures advance without anyone ticking anything.
    #[test]
    fn the_default_clock_is_monotonic() {
        let tree = WidgetTree::new();
        let a = tree.input_now();
        let b = tree.input_now();
        assert!(b >= a);
    }
}
