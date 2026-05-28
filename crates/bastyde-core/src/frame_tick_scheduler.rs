//! Per-frame-effect scheduler.
//!
//! Tracks widgets that have registered an arbitrary per-frame effect
//! on [`BuildContext::frame_tick`](crate::build_context::BuildContext::frame_tick)
//! and want the event loop to keep waking **only while their owner
//! widget is being painted**.
//!
//! Why this exists alongside
//! [`AnimationScheduler`](crate::animation::AnimationScheduler) and
//! [`AnimatedQuadRegistry`](crate::animated_quad::AnimatedQuadRegistry):
//!
//! - `AnimationScheduler` drives signal-based linear tweens. Pulse's
//!   sine oscillation, Cycle's discrete index advance, and similar
//!   "I just need a per-frame callback" patterns don't fit the linear-
//!   tween shape.
//! - `AnimatedQuadRegistry` drives shader-driven quad uniforms. It is
//!   paint-time GPU plumbing, not a general per-frame hook.
//! - Without this scheduler, widgets like `Pulse` and `Cycle`
//!   self-managed the chain via `frame_request_handle`, with no
//!   visibility gate — so a `Pulse` sitting in a non-selected
//!   `Switcher` branch kept the event loop pumping at full frame
//!   rate forever.
//!
//! All three schedulers consult the shared
//! [`motion_visibility`] helpers so the
//! "is my owner visible enough to keep waking?" decision is uniform.
//!
//! ## Lifecycle
//!
//! Subscription is RAII via [`FrameTickSubscription`]: a widget
//! requests one in `build()` (typically via
//! `BuildContext::subscribe_frame_tick`) and stashes the guard on
//! `self`. Dropping the guard removes the entry. On rebuild the old
//! guard drops before the new one is created, so no leak.
//!
//! ## Visibility gate
//!
//! Strict equality
//! ([`painted_this_frame`](crate::motion_visibility::painted_this_frame))
//! — same rationale as `AnimatedQuadRegistry`. The per-frame effect
//! itself does not directly dirty the owner; it mutates a signal
//! whose binding may or may not propagate. We need to be sure the
//! widget actually painted this frame before re-arming.
//!
//! ## Resume semantics
//!
//! When a subscriber's owner becomes visible again (e.g. its parent
//! `Switcher` flips the relevant `visible_when` binding to `true`),
//! the framework's existing `Relayout`-level dirty path triggers a
//! repaint independently of this scheduler. That repaint paints the
//! subscriber, stamping its `last_painted_epoch` to match the new
//! `paint_epoch`. The render-end re-arm (see
//! `should_arm_frame_tick`) then detects the visible subscriber
//! and sets `frame_tick_requested` so the next tick fires.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::arena::WidgetArena;
use crate::motion_visibility;
use crate::widget_id::WidgetId;

/// Internal table of subscriptions. Wrapped in `Rc<RefCell<...>>` so
/// per-widget RAII guards can mutate it on drop without holding a
/// `&mut WidgetTree`.
type SubscriberSet = Rc<RefCell<Vec<WidgetId>>>;

/// Scheduler for per-frame effects that should run only while their
/// owner widget is visible.
#[derive(Clone)]
pub struct FrameTickScheduler {
    subscribers: SubscriberSet,
}

impl Default for FrameTickScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FrameTickScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameTickScheduler")
            .field("subscriber_count", &self.subscribers.borrow().len())
            .finish()
    }
}

impl FrameTickScheduler {
    pub fn new() -> Self {
        Self {
            subscribers: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Register a per-frame effect for `owner`. The returned guard
    /// removes the entry on drop. Multiple subscriptions for the same
    /// owner are allowed (e.g. a composite that has both a Pulse and
    /// a Cycle child); each guard owns one slot.
    pub fn subscribe(&self, owner: WidgetId) -> FrameTickSubscription {
        self.subscribers.borrow_mut().push(owner);
        FrameTickSubscription {
            set: Rc::downgrade(&self.subscribers),
            owner,
        }
    }

    /// Number of live subscriptions (test/debug API). Stale entries
    /// — subscribers whose owner has been removed from the arena —
    /// are NOT pruned automatically; the visibility gate filters them
    /// out implicitly. Drop the guard for explicit removal.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.borrow().len()
    }

    /// Whether ANY subscriber's owner widget was painted in the most
    /// recent paint pass. Used by `WidgetTree::render` to decide
    /// whether to re-arm `frame_tick_requested` so the chain stays
    /// alive across visible frames and dies cleanly when all
    /// subscribers are hidden.
    pub fn should_arm_frame_tick(&self, arena: &WidgetArena, paint_epoch: u64) -> bool {
        self.subscribers
            .borrow()
            .iter()
            .any(|&id| motion_visibility::painted_this_frame(arena, id, paint_epoch))
    }

    /// Whether at least one subscription is registered. Used by the
    /// idle-work predicate so the event loop doesn't return early
    /// while the chain is alive but the very-first-paint hasn't
    /// happened yet (bootstrap case before `paint_epoch` advances).
    pub fn has_running(&self) -> bool {
        !self.subscribers.borrow().is_empty()
    }
}

/// RAII guard returned by [`FrameTickScheduler::subscribe`]. Removes
/// the subscription from the scheduler on drop. Safe to drop after
/// the parent tree has been torn down (the `Weak` upgrade fails
/// gracefully).
pub struct FrameTickSubscription {
    set: Weak<RefCell<Vec<WidgetId>>>,
    owner: WidgetId,
}

impl FrameTickSubscription {
    /// The widget id this subscription is associated with. Useful for
    /// debugging.
    pub fn owner(&self) -> WidgetId {
        self.owner
    }
}

impl Drop for FrameTickSubscription {
    fn drop(&mut self) {
        let Some(set) = self.set.upgrade() else {
            return;
        };
        let mut subs = set.borrow_mut();
        if let Some(pos) = subs.iter().position(|&id| id == self.owner) {
            subs.swap_remove(pos);
        }
    }
}

impl std::fmt::Debug for FrameTickSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameTickSubscription")
            .field("owner", &self.owner)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget_tree::WidgetTree;

    fn fresh_ids() -> (WidgetId, WidgetId) {
        // Allocate two distinct ids via a real tree; using
        // `slotmap`-internal constructors directly would tie tests
        // to the slotmap version.
        let mut tree = WidgetTree::new();
        let a = tree.add(crate::test_widgets::FillWidget::new());
        let b = tree.add(crate::test_widgets::FillWidget::new());
        (a, b)
    }

    #[test]
    fn subscribe_then_drop_removes() {
        let sched = FrameTickScheduler::new();
        let (id_a, id_b) = fresh_ids();
        let g_a = sched.subscribe(id_a);
        let _g_b = sched.subscribe(id_b);
        assert_eq!(sched.subscriber_count(), 2);
        drop(g_a);
        assert_eq!(sched.subscriber_count(), 1);
    }

    #[test]
    fn drop_after_scheduler_is_no_op() {
        let (id_a, _) = fresh_ids();
        let g = {
            let sched = FrameTickScheduler::new();
            sched.subscribe(id_a)
        };
        drop(g);
    }

    #[test]
    fn duplicate_owner_subscriptions_count_separately() {
        let sched = FrameTickScheduler::new();
        let (id, _) = fresh_ids();
        let g1 = sched.subscribe(id);
        let _g2 = sched.subscribe(id);
        assert_eq!(sched.subscriber_count(), 2);
        drop(g1);
        assert_eq!(sched.subscriber_count(), 1);
    }
}
