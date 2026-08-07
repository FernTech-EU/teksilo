// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

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
    pub fn synthesise_tap(&mut self, id: WidgetId) {
        let center = self.arena.bounds(id).center();
        self.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        self.dispatch_event(WidgetEvent::PointerUp {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    /// Simulate pointer movement to a position.
    pub fn pointer_move(&mut self, position: Point) {
        self.dispatch_event(WidgetEvent::PointerMove { position });
    }

    /// Simulate a key press (down + up).
    pub fn press_key(&mut self, key: Key, modifiers: Modifiers) {
        self.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
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
        self.dispatch_event(WidgetEvent::PointerDown {
            position,
            button,
            modifiers: Modifiers::NONE,
        });
    }

    /// Simulate a pointer up at a specific position with a specific button.
    pub fn pointer_up_button(&mut self, position: Point, button: PointerButton) {
        self.dispatch_event(WidgetEvent::PointerUp {
            position,
            button,
            modifiers: Modifiers::NONE,
        });
    }

    /// Simulate a drag from one position to another.
    pub fn drag(&mut self, from: Point, to: Point) {
        self.dispatch_event(WidgetEvent::PointerDown {
            position: from,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        self.dispatch_event(WidgetEvent::PointerMove { position: to });
        self.dispatch_event(WidgetEvent::PointerUp {
            position: to,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    /// The draggable ancestors armed by the current pointer press — the
    /// observable state of the cross-widget tap-vs-drag disambiguation (see
    /// `arm_drag_observers`).
    ///
    /// Empty when the press landed inside a
    /// [`gesture_dead_zone`](crate::arena::WidgetNode::gesture_dead_zone), or when
    /// the pressed widget carries its own drag (the innermost drag owns the
    /// gesture). Exposed so an **app** built on teksilo can assert the same thing
    /// this crate's own `gesture_dead_zone_blocks_ancestor_drag_arming` asserts —
    /// that a press on an interactive control inside a draggable container cannot
    /// start the container's drag. Read-only; test support.
    pub fn armed_drag_observers(&self) -> &[WidgetId] {
        &self.drag_observers
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

    /// Advance the simulated clock by the given duration.
    /// Triggers time-dependent behavior such as long-press gesture recognition
    /// and tooltip timers. Enables deterministic testing without real delays.
    pub fn advance_time(&mut self, duration: std::time::Duration) {
        self.sim_clock += duration;
        // Mirror the new sim_clock onto the overlay manager so any
        // dismiss triggered by the process_* steps below stamps its
        // sim-time start in lockstep with real time.
        self.overlay_manager.set_sim_clock(self.sim_clock);
        self.process_tooltips();
        self.process_delayed_overlays();
        self.process_pointer_leave_overlays();
        self.process_auto_dismiss_overlays();
        self.process_overlay_fade_dismissals_sim();
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

    /// Every widget the arena still holds — active, dormant and orphaned alike.
    ///
    /// The number a leak test must assert on. `active_widget_count` walks the
    /// tree from its roots and so cannot see the failure mode that matters
    /// here: a node kept alive in the arena with nothing pointing at it. A
    /// parentless orphan (tooltip content is `ctx.add`ed, hence parentless by
    /// construction) is invisible to every other count in this file, and to the
    /// accessibility tree, while still paying for itself in the arena's slotmap
    /// forever.
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

    /// Mark a widget as needing repaint.
    pub fn mark_needs_paint(&mut self, id: WidgetId) {
        self.arena.mark_needs_paint(id);
    }

    /// Set a widget subtree as dormant.
    pub fn set_dormant(&mut self, id: WidgetId) {
        self.arena.set_dormant(id);
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
    use crate::test_widgets::{FillWidget, InsetWidget};

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
}
