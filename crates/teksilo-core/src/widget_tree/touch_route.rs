// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The tree-owned long-press route.
//!
//! A finger has no hover, so every affordance a mouse reaches by resting on it
//! needs a second way in. For two of them — a context menu and a tooltip — the
//! way in is a hold, and neither can be delivered by the widget that owns the
//! affordance:
//!
//! * a **context menu** is mounted from a factory the router walks up to
//!   (`WidgetTree::show_context_menu_for`), not from a handler, so no widget
//!   is in a position to open one from its own `on_long_press`;
//! * a **tooltip** is attached to a node the router keeps in its own table, and
//!   the anchor may be **disabled** — a disabled node is dropped from focus
//!   traversal and never reaches `dispatch_to_widget`, so it has no gesture
//!   arena and cannot recognise anything at all. Hover is today the only way to
//!   ask "why is this greyed out?", which is precisely the question a touch user
//!   has no way to ask.
//!
//! So the route is the tree's: a deadline hung off the press, resolved in the
//! same pass as the press-feedback delay and a standing hold's expiry
//! (`tick_gestures_with_ops`), reached from `advance_time` like every other
//! input deadline and folded into the one `WaitUntil` through
//! [`WidgetTree::next_input_deadline`].
//!
//! # Precedence
//!
//! Three rules, in order:
//!
//! 1. **A widget's own `on_long_press` wins.** If any *enabled* node on the
//!    press's frozen path carries one, no tree route is armed. The widget said
//!    what its hold means.
//! 2. **A hold that is a grab is spent — on the node whose grab it arms.**
//!    Where the hold is the mechanism that arms a drag — a reorderable row
//!    under a finger, or a scene view whose marquee waits one out — the hold
//!    belongs to that node's grab, and its long-press *recognition* is
//!    suppressed as well (see `WidgetTree::long_press_is_a_grab`). The rule is
//!    keyed on the node, not on the press: an ancestor whose own grab is
//!    deferred does not thereby take the hold away from the controls inside it,
//!    which for a finger — with no secondary button — would be taking away the
//!    only route they have to a context menu. [`LongPressRole::DragHandle`] is
//!    the explicit, subtree-wide form of the same claim, for a node that really
//!    does own every hold beneath it — and it applies only to a **direct**
//!    pointer, because a mouse spends no hold arming a drag *it never asked
//!    for*. A node that asks, by declaring
//!    [`DragActivation::AfterLongPress`](teksilo_tokens::DragActivation::AfterLongPress)
//!    rather than leaving it to `Auto`, does spend a mouse's hold too; see
//!    `WidgetTree::long_press_is_a_grab`.
//! 3. Otherwise the resolved [`LongPressRole`] selects the route, and
//!    [`LongPressRole::Auto`] — the default — resolves to the context menu if
//!    one is reachable, else the tooltip, else nothing.
//!
//! # What it is not
//!
//! Not a mouse path. The route is armed only for a **coarse** pointer
//! ([`PointerKind::is_coarse`](teksilo_tokens::PointerKind::is_coarse)), which
//! is touch and touch only — a pen has a barrel button and a genuine hover, so
//! giving it a hold-to-menu would take away a press it already uses, and an
//! unclassified pointer is tuned as a mouse (see `arm_touch_route` for why
//! "cannot hover" is the wrong predicate here, even though it reads like the
//! right one).

use teksilo_canvas::Point;

use crate::pointer::{EventTime, PointerId};
use crate::widget_id::WidgetId;

use super::WidgetTree;

/// How long a tooltip a hold summoned stays up.
///
/// There is no pointer to leave and no focus to move, so the two routes that
/// retire a hovered or a focused tip both do nothing here. The tip owns its own
/// expiry instead — long enough to read a sentence, short enough that it is
/// gone before it is in the way. Escape and a press anywhere else still retire
/// it sooner.
pub const TOUCH_TOOLTIP_DISMISS: std::time::Duration = std::time::Duration::from_secs(5);

/// What a hold on a node means, when the node itself does not say.
///
/// Selects the tree-owned fallback described in the
/// [module documentation](self); a widget's own `on_long_press` always takes
/// precedence over every variant of this.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LongPressRole {
    /// Resolve from what the node offers: a context menu if a factory is
    /// reachable on the path, else a tooltip if one is attached, else nothing.
    /// The default, and what every node that never mentions the property gets.
    #[default]
    Auto,
    /// No tree-owned route, and none for anything below this node either. A
    /// subtree that wants its hold left alone without having to install a
    /// handler to swallow it.
    None,
    /// The hold opens the context menu, and does not fall back to a tooltip
    /// when no factory answers.
    ContextMenu,
    /// The hold shows the tooltip, even where a context-menu factory is also
    /// reachable.
    Tooltip,
    /// The hold **is** the grab: it arms this node's drag, so it is not
    /// available for a menu or a tip, and long-press recognition is suppressed
    /// on this node and everything below it. Declare it on a node whose grab
    /// the framework's own deferral cannot see — one that takes the pointer by
    /// an explicit `capture_pointer` rather than through a drag recognizer.
    ///
    /// **Direct pointers only.** A hold is a drag-start route for a finger or a
    /// pen; a mouse latches its drag on travel and never spends one, so this
    /// variant takes nothing from it and a mouse hold inside the subtree still
    /// reaches the node's own `on_long_press` and the tree's own route.
    DragHandle,
}

/// The route a resolved hold will take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    ContextMenu,
    /// The index into `WidgetTree::tooltips` of the entry to show.
    Tooltip(usize),
}

/// A hold the tree is waiting out.
pub(super) struct PendingTouchRoute {
    pointer: PointerId,
    /// The node the press landed on. The context menu walks up from here and
    /// the announcement is checked against it.
    target: WidgetId,
    /// **Window** space, not node-local: `show_context_menu_for` places the
    /// menu against the window and a gesture's own `TapEvent` has already been
    /// localized by the time it is dispatched.
    position: Point,
    deadline: EventTime,
    route: Route,
}

impl WidgetTree {
    /// Whether a hold on `id` is already spoken for as a grab, for the pointer
    /// whose sequence is `pointer`.
    ///
    /// Two ways it can be, and they are deliberately scoped differently:
    ///
    /// * **`id` itself** is a live member whose grab was **deferred to the
    ///   long-press deadline** — which is what `DragActivation::Auto` resolves
    ///   to for a direct pointer with a pan competitor, i.e. a reorderable row
    ///   under a finger, or a scene view whose marquee waits out a hold. The
    ///   deferral *is* the mechanism, so the node whose grab it arms cannot also
    ///   fire a long press;
    /// * a node at or above `id` on the frozen path declared
    ///   [`LongPressRole::DragHandle`] **and the pointer is a direct one** (see
    ///   below).
    ///
    /// # Why only the first is per node
    ///
    /// A deferral is an *implicit* consequence of a node carrying a drag while
    /// something claims a pan. Reading it across the whole sequence made it an
    /// assertion about the node's descendants as well — so a `SceneView` with
    /// selection on took the touch long press and the touch context menu away
    /// from every heavyweight widget placed inside it, and a drag-capable
    /// ancestor inside a scroller did the same to every control beneath it. A
    /// finger has no secondary button, so the hold is the *only* route to a
    /// context menu: that is an accessibility loss, and nothing in the press
    /// said the ancestor wanted it.
    ///
    /// [`LongPressRole::DragHandle`] is the explicit form of the same claim and
    /// keeps its ancestor walk, because declaring it *is* the node saying every
    /// hold in its subtree is a grab.
    ///
    /// # Why the role walk is gated on a direct pointer
    ///
    /// The declaration says *the hold is this node's drag-start route* — and a
    /// hold is only ever a drag-start route for a **direct** pointer, which is
    /// the one kind `DragActivation::Auto` puts off to the long-press deadline.
    /// A mouse starts a drag by moving while pressed, at `drag_slop`, with no
    /// deadline in it at all: there is no grab for the declaration to protect,
    /// so the hold is still the application's.
    ///
    /// Without the gate, chaining `data_views::row_grab_surface` onto a row —
    /// which is what `.reorderable(true)` and `.exportable(..)` do — silently
    /// deleted that row's own `on_long_press` under a mouse, where nothing was
    /// competing for the hold at all. `DragHandle` was the framework's first
    /// production declaration, so the loss arrived with it.
    ///
    /// # Why the deferral branch above is *not* gated
    ///
    /// Not because a mouse can never reach it — it can — but because the two
    /// ways in are already the right shape, and the third is deliberate:
    ///
    /// * the **implicit** door is closed to a mouse by construction, not by a
    ///   gate. [`DragActivation::Auto`](teksilo_tokens::DragActivation::Auto)
    ///   is the only activation `resolve_activation` ever *synthesises* an
    ///   `AfterLongPress` from, and it does so only for a direct pointer with
    ///   an eligible pan competitor. A mouse enrols no pan claimant, so nothing
    ///   an `Auto` node declares is deferred on its sequence;
    /// * the **role** door is closed to a mouse by the `is_direct()` gate above;
    /// * an **explicitly declared**
    ///   [`DragActivation::AfterLongPress`](teksilo_tokens::DragActivation::AfterLongPress)
    ///   is open to every pointer kind, on purpose. `resolve_activation` passes
    ///   a declared activation straight through, so the grab really is deferred
    ///   to the hold under a mouse as much as under a finger — the node said
    ///   its drag starts on a hold — and the node's own long press must not
    ///   fire as well. Gating this branch on `is_direct()` would silently
    ///   demote such a declaration to `Auto` for a mouse;
    ///   `an_explicitly_deferred_grab_takes_the_hold_from_every_pointer_kind`
    ///   pins that.
    ///
    /// So "a mouse is unaffected" is a statement about the two *inferred*
    /// claims, `Auto` and `DragHandle`, and not about a node that asked for the
    /// deferral by name. No shipped widget declares one today.
    pub(super) fn long_press_is_a_grab(&self, pointer: PointerId, id: WidgetId) -> bool {
        let Some(entry) = self.pointers.get(pointer) else {
            return false;
        };
        let Some(sequence) = entry.sequence.as_ref() else {
            return false;
        };
        if sequence.has_deferred_grab_for(id) {
            return true;
        }
        if !entry.info.is_direct() {
            return false;
        }
        let path = sequence.path();
        let Some(cut) = path.iter().position(|&n| n == id) else {
            return false;
        };
        // The path runs target → root, so everything from `cut` onward is `id`
        // itself or an ancestor of it.
        path[cut..]
            .iter()
            .any(|&n| self.long_press_role_of(n) == LongPressRole::DragHandle)
    }

    fn long_press_role_of(&self, id: WidgetId) -> LongPressRole {
        self.arena
            .get(id)
            .map(|node| node.long_press_role)
            .unwrap_or_default()
    }

    /// Arm the tree-owned hold for the press that has just been opened.
    ///
    /// Called from the `PointerDown` arm once the sequence exists (the frozen
    /// path is what the resolution walks) and the press record is open.
    pub(super) fn arm_touch_route(&mut self, target: WidgetId, position: Point) {
        self.pending_touch_route = None;
        let pointer = self.current_input.pointer;
        // Touch and touch only, which is `is_coarse` and not `!hovers`: a pen
        // hovers, so it already reaches both affordances the way a mouse does
        // and has a barrel button for the menu — but an *unclassified* pointer
        // hovers no more than a finger does, and `InputTokens::profile` hands it
        // the MOUSE profile ("treated as Mouse for gesture tuning, the
        // conservative choice"). Arming a route here for it would time a hold
        // against a profile whose `long_press` was never meant to arm one.
        if !pointer.kind.is_coarse() {
            return;
        }
        // The kill switch means the framework is not handling touch at all;
        // a route armed here would be the one touch behaviour that survived it.
        if !self.effective_theme.input.touch_enabled {
            return;
        }
        let Some(path) = self
            .pointers
            .get(pointer.id)
            .and_then(|entry| entry.sequence.as_ref())
            .map(|sequence| sequence.path().to_vec())
        else {
            return;
        };
        // Rule 1: a widget said what its own hold means.
        if path.iter().any(|&id| {
            self.arena.is_enabled(id)
                && self
                    .arena
                    .get(id)
                    .is_some_and(|node| node.any_handler(|h| h.on_long_press.is_some()))
        }) {
            return;
        }
        let Some((route, owner)) = self.resolve_touch_route(&path) else {
            return;
        };
        // Rule 2: the hold is a grab — asked over the span this route actually
        // spends, the press target up to and including the node whose
        // affordance the hold would open.
        if self.hold_is_spent_on_a_grab(pointer.id, target, owner, &path) {
            return;
        }
        let profile = self.effective_theme.input.profile(pointer.kind);
        let started = self
            .pointers
            .get(pointer.id)
            .and_then(|entry| entry.sequence.as_ref())
            .map(|sequence| sequence.started_at());
        let Some(started) = started else {
            return;
        };
        self.pending_touch_route = Some(PendingTouchRoute {
            pointer: pointer.id,
            target,
            position,
            deadline: started + profile.long_press,
            route,
        });
    }

    /// Rule 3: what the innermost node that has an opinion says, else the
    /// `Auto` fallback order.
    ///
    /// Reports the route **and the node that owns it** — the one whose factory
    /// will be asked, or whose tooltip will be shown. Rule 2 needs it: a hold
    /// spends the affordance of that node, so that node is the one to ask
    /// whether the hold is already arming a grab.
    fn resolve_touch_route(&self, path: &[WidgetId]) -> Option<(Route, WidgetId)> {
        // The path runs target → root, so this walk is innermost-first: a row
        // inside a menu-owning list decides for itself.
        for &id in path {
            match self.long_press_role_of(id) {
                LongPressRole::Auto => continue,
                LongPressRole::None | LongPressRole::DragHandle => return None,
                LongPressRole::ContextMenu => return self.context_menu_route(path),
                LongPressRole::Tooltip => return self.tooltip_route(path),
            }
        }
        self.context_menu_route(path)
            .or_else(|| self.tooltip_route(path))
    }

    /// The innermost context-menu factory at or above the pressed node, and the
    /// node carrying it — which is the node `show_context_menu_for` will reach,
    /// since it walks up from the target the same way.
    fn context_menu_route(&self, path: &[WidgetId]) -> Option<(Route, WidgetId)> {
        path.iter()
            .copied()
            .find(|&id| {
                self.arena
                    .get(id)
                    .is_some_and(|node| node.context_menu_factory.is_some())
            })
            .map(|owner| (Route::ContextMenu, owner))
    }

    /// The innermost tooltip anchored at or above the pressed node — the same
    /// choice `tooltip_pointer_enter` makes for a hover, and made the same way,
    /// so a hold and a hover surface the same tip.
    fn tooltip_route(&self, path: &[WidgetId]) -> Option<(Route, WidgetId)> {
        let target = *path.first()?;
        let index = self.tooltip_index_for(target)?;
        let anchor = self.tooltips.get(index)?.anchor_id;
        Some((Route::Tooltip(index), anchor))
    }

    /// Whether the hold that would open `owner`'s affordance, for a press on
    /// `target`, is already spoken for as a grab.
    ///
    /// The span asked is `target` **up to and including `owner`** on the frozen
    /// path, plus whatever [`long_press_is_a_grab`](Self::long_press_is_a_grab)
    /// answers for `target` itself (its own deferred grab, and the
    /// [`LongPressRole::DragHandle`] walk to the root).
    ///
    /// That span is the rule stated precisely: *a hold is spent by any node
    /// between the press and the affordance it would open*. Both ends matter.
    /// Stopping at `target` would let a container whose marquee this very hold
    /// arms have its own context menu opened by the same hold — one hold, two
    /// things. Running to the root is what the sequence-wide reading did, and
    /// it took the hold away from descendants whose affordance no ancestor had
    /// any claim on: a finger has no secondary button, so that removed the only
    /// touch route to their context menus.
    ///
    /// A node *above* `owner` with a deferred grab is a third party — the hold
    /// is not opening anything of its. A container that really does own every
    /// hold beneath it says so with [`LongPressRole::DragHandle`], which is
    /// walked to the root and is deliberately the only subtree-wide door.
    ///
    /// An `owner` that is not on the frozen path — which the two finders above
    /// cannot produce, since both search it — degrades to the target alone,
    /// i.e. to arming the route. That direction is deliberate: a missing
    /// affordance is the failure this whole module exists to prevent.
    fn hold_is_spent_on_a_grab(
        &self,
        pointer: PointerId,
        target: WidgetId,
        owner: WidgetId,
        path: &[WidgetId],
    ) -> bool {
        if self.long_press_is_a_grab(pointer, target) {
            return true;
        }
        let Some(sequence) = self
            .pointers
            .get(pointer)
            .and_then(|entry| entry.sequence.as_ref())
        else {
            return false;
        };
        let cut = path.iter().position(|&n| n == owner).unwrap_or(0);
        path[..=cut]
            .iter()
            .any(|&id| sequence.has_deferred_grab_for(id))
    }

    /// Disarm the hold: the press ended, was revoked, or travelled far enough
    /// to be something else.
    pub(super) fn cancel_touch_route(&mut self, pointer: PointerId) {
        if self
            .pending_touch_route
            .as_ref()
            .is_some_and(|pending| pending.pointer == pointer)
        {
            self.pending_touch_route = None;
        }
    }

    /// A move: the hold survives only while the contact stays inside the
    /// profile's tap boundary. Past it the gesture is a pan or a drag and the
    /// route has nothing to say.
    pub(super) fn touch_route_moved(&mut self, pointer: PointerId) {
        let Some(pending) = self.pending_touch_route.as_ref() else {
            return;
        };
        if pending.pointer != pointer {
            return;
        }
        let Some(entry) = self.pointers.get(pointer) else {
            return;
        };
        let Some(sequence) = entry.sequence.as_ref() else {
            // No sequence left to measure against — the press is over.
            self.pending_touch_route = None;
            return;
        };
        let slop = self.effective_theme.input.profile(entry.info.kind).tap_slop;
        if sequence.travel() > slop {
            self.pending_touch_route = None;
        }
    }

    /// The instant the pending hold wants the loop back, if there is one.
    pub(super) fn next_touch_route_deadline(&self) -> Option<EventTime> {
        self.pending_touch_route
            .as_ref()
            .map(|pending| pending.deadline)
    }

    /// Fire a hold whose deadline has come. Runs in `tick_gestures_with_ops`
    /// beside the other three tree-owned input deadlines.
    pub(super) fn resolve_touch_routes(
        &mut self,
        now: EventTime,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let Some(pending) = self.pending_touch_route.as_ref() else {
            return;
        };
        if now < pending.deadline {
            return;
        }
        let Some(pending) = self.pending_touch_route.take() else {
            return;
        };
        // The target may have gone away under the finger.
        if !self.arena.is_active(pending.target) {
            return;
        }
        let fired = match pending.route {
            Route::ContextMenu => self.fire_context_menu_route(&pending, ops),
            Route::Tooltip(index) => self.fire_tooltip_route(&pending, index),
        };
        if fired {
            // The hold has been answered; the release must not also actuate the
            // control the menu or the tip is about.
            if let Some(sequence) = self
                .pointers
                .get_mut(pending.pointer)
                .and_then(|entry| entry.sequence.as_mut())
            {
                sequence.set_taps_cancelled();
            }
        }
    }

    fn fire_context_menu_route(
        &mut self,
        pending: &PendingTouchRoute,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        if !self.show_context_menu_for(pending.target, pending.position, ops) {
            return false;
        }
        if let Some(wording) = self.context_menu_announcement.clone() {
            self.announce_unless_widget_speaks(pending.target, wording());
        }
        true
    }

    fn fire_tooltip_route(&mut self, pending: &PendingTouchRoute, index: usize) -> bool {
        // The table may have been rebuilt since the press; re-resolve rather
        // than trusting the index across a rebuild.
        let index = match self.tooltip_index_for(pending.target) {
            Some(fresh) => fresh,
            None => index,
        };
        let Some(entry) = self.tooltips.get_mut(index) else {
            return false;
        };
        if entry.overlay_id.is_some() {
            // Already up — nothing to do, and nothing to cancel a tap for.
            return false;
        }
        // Backdate the dwell instead of duplicating the show: the hold has
        // already paid the delay, so the ordinary tooltip pass shows it inside
        // this same virtual frame, having run its content and blank-body checks
        // exactly as it would for a hover.
        let delay = entry.delay;
        entry.hover_start = Some(sub_or(self.sim_clock, delay));
        entry.real_hover_start = Some(sub_or(std::time::Instant::now(), delay));
        entry.hover_origin = Some(pending.position);
        entry.armed_by_focus = false;
        entry.armed_by_hold = true;
        let anchor = entry.anchor_id;
        self.arena.mark_needs_paint(anchor);
        true
    }
}

/// `instant - duration`, or `instant` where the platform's monotonic origin is
/// nearer than that. A tooltip whose dwell cannot be backdated ripens on the
/// next pass instead of this one — the same tip, one frame later.
fn sub_or(instant: std::time::Instant, duration: std::time::Duration) -> std::time::Instant {
    instant.checked_sub(duration).unwrap_or(instant)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use teksilo_canvas::{Point, SizeProposal};

    use super::*;
    use crate::test_widgets::{FillWidget, InsetWidget, StackWidget};
    use crate::widget::Widget;
    use crate::widget_builder::WidgetBuilder;
    use crate::widget_tree::WidgetTree;

    /// The touch profile's hold, which is what the route's deadline is.
    fn hold() -> std::time::Duration {
        teksilo_tokens::GestureProfile::TOUCH.long_press
    }

    fn menu() -> Box<dyn Widget> {
        Box::new(FillWidget::new().label("menu"))
    }

    /// A tree whose single widget owns a context-menu factory, plus the count of
    /// times that factory has been asked.
    fn tree_with_menu() -> (WidgetTree, WidgetId, Rc<Cell<usize>>) {
        let opened = Rc::new(Cell::new(0usize));
        let counter = opened.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().context_menu(move |_pos, _ctx| {
            counter.set(counter.get() + 1);
            Some(menu())
        }));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        (tree, id, opened)
    }

    #[test]
    fn a_hold_opens_the_context_menu_once_and_announces() {
        let (mut tree, id, opened) = tree_with_menu();
        let spoken = Rc::new(Cell::new(0usize));
        let counter = spoken.clone();
        tree.set_context_menu_announcement(Some(Rc::new(move || {
            counter.set(counter.get() + 1);
            "menu opened".to_string()
        })));

        let at = tree.bounds(id).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        assert_eq!(opened.get(), 0, "the factory must not run on the press");

        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(opened.get(), 1, "the hold must open the menu");
        assert_eq!(spoken.get(), 1, "and announce it once");
        assert_eq!(tree.active_overlays().len(), 1);

        // Holding on does not open a second one.
        tree.advance_input_time(hold() * 3);
        assert_eq!(opened.get(), 1, "the hold opens exactly one menu");
        assert_eq!(spoken.get(), 1);
    }

    #[test]
    fn no_wording_registered_announces_nothing() {
        let (mut tree, id, opened) = tree_with_menu();
        let at = tree.bounds(id).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(opened.get(), 1);
        assert!(
            tree.announcements_since(0).is_empty(),
            "silence is the default: the framework cannot word this itself",
        );
    }

    #[test]
    fn a_mouse_hold_opens_no_context_menu() {
        let (mut tree, id, opened) = tree_with_menu();
        let at = tree.bounds(id).center();
        tree.pointer_move(at);
        tree.pointer_down_button(at, crate::event::PointerButton::Primary);
        tree.advance_input_time(hold() * 4);
        assert_eq!(
            opened.get(),
            0,
            "a mouse has a secondary button; a held primary is not a menu request",
        );
    }

    #[test]
    fn a_pen_hold_opens_no_context_menu() {
        let (mut tree, id, opened) = tree_with_menu();
        let at = tree.bounds(id).center();
        tree.pen_down(at, 0.5, (0.0, 0.0));
        tree.advance_input_time(hold() * 4);
        assert_eq!(
            opened.get(),
            0,
            "a pen hovers and has a barrel button, so the hold stays its own",
        );
    }

    #[test]
    fn a_lift_before_the_deadline_opens_nothing() {
        let (mut tree, id, opened) = tree_with_menu();
        let at = tree.bounds(id).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() / 2);
        tree.touch_up(f, at);
        tree.advance_input_time(hold() * 2);
        assert_eq!(opened.get(), 0);
    }

    #[test]
    fn a_contact_that_travels_past_the_tap_slop_opens_nothing() {
        let (mut tree, id, opened) = tree_with_menu();
        let at = tree.bounds(id).center();
        let slop = teksilo_tokens::GestureProfile::TOUCH.tap_slop;
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.touch_move(f, Point::new(at.x, at.y + slop + 2.0));
        tree.advance_input_time(hold() * 2);
        assert_eq!(
            opened.get(),
            0,
            "that travel is a pan, not a considered hold"
        );
    }

    #[test]
    fn a_cancelled_contact_opens_nothing() {
        let (mut tree, id, opened) = tree_with_menu();
        let at = tree.bounds(id).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.touch_cancel(f, at);
        tree.advance_input_time(hold() * 2);
        assert_eq!(opened.get(), 0);
    }

    #[test]
    fn the_hold_deadline_is_folded_into_the_one_input_deadline() {
        let (mut tree, id, _opened) = tree_with_menu();
        let at = tree.bounds(id).center();
        assert!(tree.next_input_deadline().is_none());
        let f = tree.new_contact();
        tree.touch_down(f, at);
        assert!(
            tree.next_input_deadline().is_some(),
            "the loop must be asked back, or a resting finger never ripens",
        );
    }

    #[test]
    fn a_widgets_own_long_press_takes_precedence_over_the_route() {
        let opened = Rc::new(Cell::new(0usize));
        let counter = opened.clone();
        let fired = Rc::new(Cell::new(0usize));
        let own = fired.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new().on_long_press(move |_e, _c| own.set(own.get() + 1)));
        let _outer = tree.add(StackWidget::new().add_child(inner).context_menu(
            move |_pos, _ctx| {
                counter.set(counter.get() + 1);
                Some(menu())
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(fired.get(), 1, "the widget's own handler runs");
        assert_eq!(
            opened.get(),
            0,
            "and the tree route stands down: the widget said what its hold means",
        );
    }

    #[test]
    fn a_drag_handle_role_takes_the_hold() {
        let opened = Rc::new(Cell::new(0usize));
        let counter = opened.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new());
        let _outer = tree.add(
            StackWidget::new()
                .add_child(inner)
                .long_press_role(LongPressRole::DragHandle)
                .context_menu(move |_pos, _ctx| {
                    counter.set(counter.get() + 1);
                    Some(menu())
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(opened.get(), 0, "the hold is the grab, so no menu opens");
    }

    /// What `DragHandle` is *for*, and the half `a_drag_handle_role_takes_the_hold`
    /// cannot see: the role suppresses long-press **recognition**, so the node's
    /// own `on_long_press` does not fire either. That is the distinctive job —
    /// standing the tree route down is what `None` already does — and it is
    /// reached through `long_press_is_a_grab`'s role walk rather than through
    /// `resolve_touch_route`, which is why a test asserting only "no menu
    /// opened" holds with that walk deleted.
    #[test]
    fn a_drag_handle_role_suppresses_the_nodes_own_long_press() {
        let fired = Rc::new(Cell::new(0usize));
        let counter = fired.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new());
        let _outer = tree.add(
            StackWidget::new()
                .add_child(inner)
                .long_press_role(LongPressRole::DragHandle)
                .on_long_press(move |_e, _ctx| counter.set(counter.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(
            fired.get(),
            0,
            "the hold arms the grab, so the node's own long press must not also fire"
        );
    }

    /// The gate on the role walk, at the mechanism rather than through a data
    /// view: the identical hold under a **mouse** must still reach the node's
    /// own `on_long_press`.
    ///
    /// `DragHandle` says the hold is this node's drag-start route, and a hold is
    /// that only for a direct pointer — a mouse latches its drag on travel, so
    /// it spends no hold and has none to lose. This is the twin of
    /// `a_drag_handle_role_suppresses_the_nodes_own_long_press`: the two must
    /// disagree, or the gate is either absent (both silent — the regression that
    /// arrived with the framework's first production declaration) or too wide
    /// (both firing, which loses the finger rule).
    #[test]
    fn a_mouse_hold_under_a_drag_handle_role_still_fires_the_nodes_own_long_press() {
        let fired = Rc::new(Cell::new(0usize));
        let counter = fired.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new());
        let _outer = tree.add(
            StackWidget::new()
                .add_child(inner)
                .long_press_role(LongPressRole::DragHandle)
                .on_long_press(move |_e, _ctx| counter.set(counter.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        tree.pointer_move(at);
        tree.pointer_down_button(at, crate::event::PointerButton::Primary);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(
            fired.get(),
            1,
            "a mouse spends no hold arming a drag, so `DragHandle` takes nothing \
             from it and the node's own long press is still heard"
        );
    }

    /// And the tree-owned route stays declined for a mouse on its own terms, not
    /// on `DragHandle`'s: `arm_touch_route` is coarse-only, so a mouse never had
    /// a route here for the role to stand down. Pinned so that the gate above
    /// cannot be read as having opened one.
    #[test]
    fn a_mouse_hold_under_a_drag_handle_role_still_opens_no_context_menu() {
        let opened = Rc::new(Cell::new(0usize));
        let counter = opened.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new());
        let _outer = tree.add(
            StackWidget::new()
                .add_child(inner)
                .long_press_role(LongPressRole::DragHandle)
                .context_menu(move |_pos, _ctx| {
                    counter.set(counter.get() + 1);
                    Some(menu())
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        tree.pointer_move(at);
        tree.pointer_down_button(at, crate::event::PointerButton::Primary);
        tree.advance_input_time(hold() * 4);
        assert_eq!(
            opened.get(),
            0,
            "a mouse has a secondary button; a held primary is not a menu request",
        );
    }

    /// The **third** door into the deferral branch, and the one the mouse can
    /// walk through: a node that declares
    /// [`DragActivation::AfterLongPress`](teksilo_tokens::DragActivation::AfterLongPress)
    /// outright.
    ///
    /// `PointerSequence::resolve_activation` only *synthesises* that answer
    /// from `Auto`, and only for a direct pointer with an eligible pan
    /// competitor; an explicitly declared one is passed through untouched for
    /// every pointer kind. So the deferral — and with it the suppression of the
    /// deferred node's own long press — reaches a mouse here, where it never
    /// reaches one through `Auto`.
    ///
    /// That is deliberate, not a hole the `is_direct()` gate below it forgot to
    /// close: the node said its drag starts on a hold, so the hold genuinely
    /// arms its grab whatever is pressing it, and firing its `on_long_press`
    /// too would be the one hold meaning two things. Gating this branch on
    /// `is_direct()` would make an explicit declaration silently mean `Auto`
    /// under a mouse — this test is what reddens if someone does.
    #[test]
    fn an_explicitly_deferred_grab_takes_the_hold_from_every_pointer_kind() {
        /// `list` declares the deferral and owns the long press; `row` takes
        /// the press, because the deferral is armed by the ancestor walk that
        /// runs *through* a captor.
        fn tree_with_explicit_deferral() -> (WidgetTree, WidgetId, Rc<Cell<usize>>) {
            let fired = Rc::new(Cell::new(0usize));
            let counter = fired.clone();
            let mut tree = WidgetTree::new();
            let row = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
            let _list = tree.add(
                StackWidget::new()
                    .add_child(row)
                    .drag_activation(teksilo_tokens::DragActivation::AfterLongPress)
                    .on_drag(|_phase, _c| {})
                    .on_long_press(move |_e, _ctx| counter.set(counter.get() + 1)),
            );
            tree.layout(SizeProposal::exact(200.0, 50.0));
            (tree, row, fired)
        }

        let (mut tree, row, fired) = tree_with_explicit_deferral();
        let at = tree.bounds(row).center();
        tree.pointer_move(at);
        tree.pointer_down_button(at, crate::event::PointerButton::Primary);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(
            fired.get(),
            0,
            "an explicit `AfterLongPress` is not synthesised, so it is not \
             gated: the mouse's hold arms the declared grab and is spent"
        );

        let (mut tree, row, fired) = tree_with_explicit_deferral();
        let at = tree.bounds(row).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(
            fired.get(),
            0,
            "and a finger reaches the same branch, by the same declaration \
             rather than by `Auto`"
        );
    }

    /// A **pen** is direct, so the role takes its hold exactly as it takes a
    /// finger's — the gate is `is_direct()`, not `is_coarse()`. A pen rests and
    /// then moves the way a finger does, and `DragActivation::Auto` defers its
    /// grab to the same deadline, so the same claim must reach it.
    #[test]
    fn a_pen_hold_under_a_drag_handle_role_is_taken_like_a_fingers() {
        let fired = Rc::new(Cell::new(0usize));
        let counter = fired.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new());
        let _outer = tree.add(
            StackWidget::new()
                .add_child(inner)
                .long_press_role(LongPressRole::DragHandle)
                .on_long_press(move |_e, _ctx| counter.set(counter.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        tree.pen_down(at, 0.5, (0.0, 0.0));
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(
            fired.get(),
            0,
            "a pen is a direct pointer, so its hold arms the grab and is spent"
        );
    }

    /// An unclassified pointer is not a finger. `!hovers()` admits both, which
    /// is why the gate reads `is_coarse()` instead: `InputTokens::profile` hands
    /// `Unknown` the *mouse* profile, so a hold armed for it would be timed
    /// against a `long_press` that was never meant to arm one. Deleting the
    /// gate, or widening it back to `!hovers()`, reddens this.
    #[test]
    fn an_unclassified_pointer_arms_no_hold_route() {
        let opened = Rc::new(Cell::new(0usize));
        let counter = opened.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new());
        let _outer = tree.add(StackWidget::new().add_child(inner).context_menu(
            move |_pos, _ctx| {
                counter.set(counter.get() + 1);
                Some(menu())
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        let p = tree.new_contact();
        let sample = tree.direct_sample(
            p,
            teksilo_tokens::PointerKind::Unknown,
            crate::pointer::PointerPhase::Down,
            at,
            true,
        );
        tree.dispatch_pointer(sample);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(
            opened.get(),
            0,
            "an unclassified pointer is tuned as a mouse, so its hold opens nothing"
        );
    }

    /// The discriminator between the two roles that share `resolve_touch_route`'s
    /// `None | DragHandle => return None` arm. `None` says "no tree route here";
    /// it does not say "no long press here", so a node that declares one still
    /// hears its own hold. Without this, the two variants are indistinguishable
    /// to the suite and either could be deleted for the other.
    #[test]
    fn a_none_role_leaves_the_nodes_own_long_press_alone() {
        let fired = Rc::new(Cell::new(0usize));
        let counter = fired.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new());
        let _outer = tree.add(
            StackWidget::new()
                .add_child(inner)
                .long_press_role(LongPressRole::None)
                .on_long_press(move |_e, _ctx| counter.set(counter.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(
            fired.get(),
            1,
            "`None` stands the tree route down, not the widget's own handler"
        );
    }

    #[test]
    fn a_none_role_stands_the_route_down() {
        let opened = Rc::new(Cell::new(0usize));
        let counter = opened.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new().long_press_role(LongPressRole::None));
        let _outer = tree.add(StackWidget::new().add_child(inner).context_menu(
            move |_pos, _ctx| {
                counter.set(counter.get() + 1);
                Some(menu())
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(opened.get(), 0);
    }

    #[test]
    fn a_tooltip_role_wins_over_a_reachable_menu() {
        let opened = Rc::new(Cell::new(0usize));
        let counter = opened.clone();
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().long_press_role(LongPressRole::Tooltip));
        let _outer = tree.add(StackWidget::new().add_child(anchor).context_menu(
            move |_pos, _ctx| {
                counter.set(counter.get() + 1);
                Some(menu())
            },
        ));
        let tip = tree.add(FillWidget::new().label("tip text"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(500));

        let at = tree.bounds(anchor).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(opened.get(), 0, "the role named the tooltip, not the menu");
        assert!(tree.find_by_label("tip text").is_some());
    }

    // ---------------------------------------------------------------
    // The tooltip route — including the one it exists for
    // ---------------------------------------------------------------

    #[test]
    fn a_hold_shows_the_tooltip_of_a_disabled_control() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tip = tree.add(FillWidget::new().label("why it is greyed out"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(500));
        tree.enabled_when(anchor, false);

        let at = tree.bounds(anchor).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert!(
            tree.find_by_label("why it is greyed out").is_some(),
            "the disabled control's tip is hover-only with no other route; the hold is it",
        );
    }

    #[test]
    fn a_tooltip_a_hold_summoned_retires_on_its_own() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tip = tree.add(FillWidget::new().label("tip text"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(500));

        let at = tree.bounds(anchor).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        tree.touch_up(f, at);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "the tip is up after the lift"
        );

        tree.advance_input_time(TOUCH_TOOLTIP_DISMISS / 2);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "and stays up long enough to read",
        );
        tree.advance_input_time(TOUCH_TOOLTIP_DISMISS);
        assert!(
            tree.active_overlays().is_empty(),
            "nothing else would ever retire it: no pointer left it and no focus moved",
        );
    }

    #[test]
    fn a_mouse_hold_shows_no_tooltip_before_its_dwell() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tip = tree.add(FillWidget::new().label("tip text"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_secs(5));

        let at = tree.bounds(anchor).center();
        tree.pointer_move(at);
        tree.pointer_down_button(at, crate::event::PointerButton::Primary);
        tree.advance_input_time(hold() * 4);
        assert!(
            tree.find_by_label("tip text").is_none(),
            "the mouse keeps its own dwell; a held button is not a summons",
        );
    }

    // ---------------------------------------------------------------
    // The hover-owner gate on the PointerLeave dismissal
    // ---------------------------------------------------------------

    /// A contact's move must not start a `DismissBehavior::PointerLeave` grace.
    ///
    /// No mouse test can see this: a mouse **is** the hover owner, so it takes
    /// the gated branch either way. Without the gate the frame pass closes a
    /// submenu a finger has just tapped open, with no further input at all.
    #[test]
    fn a_contacts_move_does_not_start_an_overlays_pointer_leave_grace() {
        let (mut tree, content, away) = leave_grace_fixture();

        // A finger lands **on** the overlay — so this is not an outside press,
        // which would dismiss it deliberately — and then travels off it.
        let inside = tree.bounds(content).center();
        let f = tree.new_contact();
        tree.touch_down(f, inside);
        tree.touch_move(f, away);
        tree.advance_input_time(std::time::Duration::from_millis(400));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "a contact cannot leave what it never hovered",
        );
    }

    /// The other half of the gate: it withholds the grace from a contact, and
    /// from nothing else. The identical journey under a mouse — which *is* the
    /// hover owner — must still close the overlay, or the gate has taken the
    /// mouse's dismissal away with it.
    #[test]
    fn a_mouses_move_off_the_same_overlay_still_starts_the_grace() {
        let (mut tree, content, away) = leave_grace_fixture();

        let inside = tree.bounds(content).center();
        tree.pointer_move(inside);
        tree.pointer_move(away);
        tree.advance_input_time(std::time::Duration::from_millis(400));

        assert_eq!(
            tree.active_overlays().len(),
            0,
            "the pointer that can leave still leaves",
        );
    }

    /// A widget that declares its own [`LongPressRole`] from `build`, through
    /// `apply_self_handlers` — the third of the three places a handler set is
    /// folded into a node, and the one a builder-method test never reaches.
    #[derive(Debug)]
    struct SelfDeclaring(LongPressRole);

    impl Widget for SelfDeclaring {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            ctx.apply_self_handlers(
                crate::widget_builder::HandlerSet::new().long_press_role(self.0),
            );
            vec![]
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    /// The role reaches the node from `apply_self_handlers` too, not only from
    /// the builder method.
    ///
    /// The property is folded into a node at **three** sites — the two rebuild
    /// paths in `widget_tree.rs` and `WidgetArena::apply_handler_set` — and they
    /// are a hand-maintained list, so a test that only ever uses
    /// `.long_press_role(..)` on a builder leaves the third one free to be
    /// wrong. This is the case that walks through it.
    #[test]
    fn a_widget_can_declare_its_own_role_from_build() {
        let opened = Rc::new(Cell::new(0usize));
        let counter = opened.clone();
        let mut tree = WidgetTree::new();
        let inner = tree.add(SelfDeclaring(LongPressRole::None));
        let _outer = tree.add(StackWidget::new().add_child(inner).context_menu(
            move |_pos, _ctx| {
                counter.set(counter.get() + 1);
                Some(menu())
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(inner).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        assert_eq!(
            opened.get(),
            0,
            "the role the widget declared for itself stood the route down",
        );
    }

    /// A tip a hold summoned does not carry a pointer-leave dismissal, so a
    /// mouse moving afterwards does not take it away.
    ///
    /// This is what choosing the dismissal by *route* buys. The finger that
    /// summoned it has lifted; the only pointer that can still move is one that
    /// had nothing to do with it, and on a convertible there usually is one. A
    /// `PointerLeave` grace would let it close a tip it never opened, well
    /// inside the five seconds the tip was given to be read.
    #[test]
    fn a_mouse_moving_afterwards_does_not_take_away_a_tip_a_hold_summoned() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tip = tree.add(FillWidget::new().label("tip text"));
        let _root = tree.add(InsetWidget::new(60.0).set_child(anchor));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(500));

        let at = tree.bounds(anchor).center();
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        tree.touch_up(f, at);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "the tip is up after the lift"
        );

        // A mouse, elsewhere, doing nothing in particular.
        tree.pointer_move(Point::new(395.0, 5.0));
        tree.advance_input_time(std::time::Duration::from_millis(300));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "a pointer that did not open it does not close it",
        );
    }

    /// A leaf of a fixed size. `FillWidget` fills whatever it is proposed, so
    /// an overlay built on one covers the window and no point is ever outside
    /// it — which is the trap the fixture below asserts its way out of.
    #[derive(Debug)]
    struct SizedWidget(teksilo_canvas::Size);

    impl Widget for SizedWidget {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            self.0.into()
        }
    }

    /// A `PointerLeave`-dismissed overlay, its anchor, and a point far enough
    /// from both to be a leave.
    ///
    /// **The mouse case is this fixture's proof of reach**, and that is why it
    /// exists as a test rather than as a comment. An earlier version anchored
    /// the overlay on a widget that filled the window; every point was inside
    /// the anchor, `update_pointer_leave_overlays` returned early whatever it
    /// was handed, and the touch case passed with the gate deleted. Asserting
    /// the geometry does not fix that — the branch reads the overlay stack's
    /// own rectangles, not the arena's. A mouse making the identical journey
    /// and *dismissing* does: it can only have got there through the call the
    /// gate guards, so if it dismisses and the contact does not, the gate is
    /// the only thing that separated them.
    fn leave_grace_fixture() -> (WidgetTree, WidgetId, Point) {
        const WINDOW: (f32, f32) = (400.0, 300.0);
        const AWAY: Point = Point { x: 390.0, y: 10.0 };

        let mut tree = WidgetTree::new();
        let trigger = tree.add(FillWidget::new());
        // Inset, so the anchor does not cover the whole window and `AWAY` can
        // be outside it.
        let _root = tree.add(InsetWidget::new(100.0).set_child(trigger));
        let content = tree.add(SizedWidget(teksilo_canvas::Size::new(80.0, 60.0)));
        tree.layout(SizeProposal::exact(WINDOW.0, WINDOW.1));
        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor: trigger,
            placement: crate::overlay::OverlayPlacement::AtPointer(Point::new(300.0, 200.0)),
            dismiss: crate::overlay::DismissBehavior::PointerLeave {
                delay: std::time::Duration::from_millis(150),
            },
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.layout(SizeProposal::exact(WINDOW.0, WINDOW.1));

        assert_eq!(tree.active_overlays().len(), 1);
        (tree, content, AWAY)
    }
}
