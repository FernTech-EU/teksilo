// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Content that changes off the UI thread: the registry of what is attached
//! to the tree's widgets, and the pre-pass that takes the requests in.
//!
//! A producer on any thread stores its change and requests through a
//! [`RepaintTrigger`]; the request raises a flag on the widget's
//! `NodeWakeState` and, on the clear-to-set edge, wakes the window through
//! the tree's redraw waker (see [`teksilo_canvas::wake`]). The window's next
//! frame takes the flags in:
//!
//! - **repaint**: taken by the paint walker right before the widget's
//!   `paint()`. The pre-pass only marks a widget whose paint is cached, so a
//!   widget that is not painted (dormant, clipped out) keeps the flag raised
//!   and absorbs a burst of requests in one wake.
//! - **relayout**: taken by the pre-pass for an active widget, which marks
//!   it and its ancestors for layout, and by `WidgetArena::activate` for one
//!   being woken, so a request made while it slept is not lost.
//! - **pull**: taken by the pre-pass for every attached widget, active or
//!   not, which runs the widget's pull hook and applies what it returns.
//!
//! A relayout and a pull wake the window with a state wake, which reaches
//! a window that draws nothing. The event loop then asks the tree
//! ([`WidgetTree::off_thread_needs_frame`]) whether a widget shown in the
//! last frame has one pending: if so the window draws a frame, which takes it
//! in and repaints; if not, it runs a frame that draws nothing.
//!
//! A widget's wake state outlives its rebuilds: a request raised before or
//! during one is still pending after it, and its new `build()` attaching a
//! trigger it had already attached changes nothing. A rebuild releases only
//! the triggers its new `build()` no longer attaches; the widget's
//! destruction releases the rest.

mod repaint_trigger;

use std::collections::HashMap;
use std::sync::Arc;

use teksilo_canvas::wake::RedrawWaker;

pub(crate) use repaint_trigger::NodeWakeState;
pub use repaint_trigger::{PullOutcome, RepaintTrigger, RepaintTriggerStats};

use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

/// A widget's pull hook: runs on the UI thread in the pre-pass after a
/// `RepaintTrigger::request_pull`.
pub(crate) type PullHook = Box<dyn FnMut() -> PullOutcome>;

/// What a tree's widgets have attached, and the waker their requests wake.
#[derive(Default)]
pub(crate) struct OffThreadRegistry {
    waker: Option<Arc<dyn RedrawWaker>>,
    triggers: HashMap<WidgetId, TriggerNode>,
}

struct TriggerNode {
    state: Arc<NodeWakeState>,
    /// The triggers attached, so a second attach of one is a no-op.
    triggers: Vec<RepaintTrigger>,
    hook: Option<PullHook>,
    /// During a rebuild, the triggers the previous build attached that the
    /// new one has not attached again yet.
    previous: Option<Vec<RepaintTrigger>>,
}

impl TriggerNode {
    fn new(waker: Option<Arc<dyn RedrawWaker>>) -> Self {
        Self {
            // The waker is set before the state is published to any trigger.
            state: Arc::new(NodeWakeState::new(waker)),
            triggers: Vec::new(),
            hook: None,
            previous: None,
        }
    }
}

impl OffThreadRegistry {
    pub(crate) fn waker(&self) -> Option<&Arc<dyn RedrawWaker>> {
        self.waker.as_ref()
    }

    /// Store the waker and give it to every attached widget, which wakes it
    /// for whatever was requested while there was none.
    pub(crate) fn set_waker(&mut self, waker: Option<Arc<dyn RedrawWaker>>) {
        for node in self.triggers.values() {
            node.state.set_waker(waker.clone());
        }
        self.waker = waker;
    }

    /// Release everything attached to `id`, on its destruction.
    pub(crate) fn cancel_by_widget(&mut self, id: WidgetId) {
        if let Some(node) = self.triggers.remove(&id) {
            node.state.detach();
        }
    }

    /// Widget `id` is about to be rebuilt: its new `build()` attaches its
    /// triggers and sets its hook again. Its wake state, and whatever is
    /// pending on it, stays.
    pub(crate) fn begin_rebuild(&mut self, id: WidgetId) {
        if let Some(node) = self.triggers.get_mut(&id) {
            node.previous = Some(std::mem::take(&mut node.triggers));
            node.hook = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn trigger_node_count(&self) -> usize {
        self.triggers.len()
    }
}

impl Drop for OffThreadRegistry {
    fn drop(&mut self) {
        for node in self.triggers.values() {
            node.state.detach();
        }
    }
}

impl WidgetTree {
    /// Attach `trigger` to widget `id`. Register-then-read: the widget's wake
    /// state is published to the trigger before the widget's first paint
    /// after this build, so a request either lands before that paint reads
    /// the state or wakes the window. Idempotent per (widget, trigger).
    pub(crate) fn attach_repaint_trigger(&mut self, id: WidgetId, trigger: &RepaintTrigger) {
        let waker = self.off_thread.waker.clone();
        let node = self
            .off_thread
            .triggers
            .entry(id)
            .or_insert_with(|| TriggerNode::new(waker));
        if node.triggers.iter().any(|t| t.ptr_eq(trigger)) {
            return;
        }
        let state = node.state.clone();
        node.triggers.push(trigger.clone());
        if let Some(arena_node) = self.arena.get_mut(id) {
            debug_assert!(
                arena_node.dirty.needs_paint,
                "a widget attaching in build() is about to paint, which is the read"
            );
            arena_node.repaint_wake = Some(state.clone());
        }
        // Attached by the build before this rebuild: still published to the
        // trigger, so there is nothing to register.
        if let Some(previous) = node.previous.as_mut()
            && let Some(at) = previous.iter().position(|t| t.ptr_eq(trigger))
        {
            previous.remove(at);
            return;
        }
        trigger.attach_state(&state);
    }

    /// Widget `id`'s rebuild ran its `build()`: release the triggers it no
    /// longer attached, and the widget's wake state if it attached nothing
    /// and set no hook.
    pub(crate) fn finish_off_thread_rebuild(&mut self, id: WidgetId) {
        let Some(node) = self.off_thread.triggers.get_mut(&id) else {
            return;
        };
        for stale in node.previous.take().unwrap_or_default() {
            stale.detach_state(&node.state);
        }
        if node.triggers.is_empty() && node.hook.is_none() {
            self.off_thread.cancel_by_widget(id);
            if let Some(arena_node) = self.arena.get_mut(id) {
                arena_node.repaint_wake = None;
            }
        }
    }

    /// Set widget `id`'s pull hook, replacing the one an earlier build set.
    pub(crate) fn set_trigger_pull_hook(&mut self, id: WidgetId, hook: PullHook) {
        let waker = self.off_thread.waker.clone();
        let node = self
            .off_thread
            .triggers
            .entry(id)
            .or_insert_with(|| TriggerNode::new(waker));
        let state = node.state.clone();
        node.hook = Some(hook);
        if let Some(arena_node) = self.arena.get_mut(id) {
            arena_node.repaint_wake = Some(state);
        }
    }

    /// How many widgets have a `RepaintTrigger` attached.
    pub fn repaint_trigger_count(&self) -> usize {
        self.off_thread
            .triggers
            .values()
            .filter(|node| !node.triggers.is_empty())
            .count()
    }

    /// The layout pre-pass: take in what attached triggers requested since
    /// the last frame. Collects first, then marks, so no arena borrow is
    /// held while a pull hook runs.
    pub(crate) fn poll_off_thread(&mut self) {
        if self.off_thread.triggers.is_empty() {
            return;
        }
        let mut orphans = Vec::new();
        let mut relayout = Vec::new();
        let mut repaint = Vec::new();
        let mut pulled = Vec::new();
        for (&id, node) in &mut self.off_thread.triggers {
            let alive = self.arena.get(id).is_some_and(|arena_node| {
                arena_node
                    .repaint_wake
                    .as_ref()
                    .is_some_and(|state| Arc::ptr_eq(state, &node.state))
            });
            if !alive {
                orphans.push(id);
                continue;
            }
            if node.state.take_pull() {
                let outcome = match node.hook.as_mut() {
                    Some(hook) => hook(),
                    None => PullOutcome::Repaint,
                };
                pulled.push((id, outcome));
            }
            if !self.arena.is_active(id) {
                // Activation takes a pending relayout; the walker, a repaint.
                continue;
            }
            if node.state.take_relayout() {
                relayout.push(id);
            }
            // Only a widget whose paint is cached needs marking: the walker
            // paints any other. Peeked, not loaded: a producer whose raise
            // follows this look must see the frame being delivered (see
            // `teksilo_canvas::wake`, "Deciding before taking").
            let cached = self
                .arena
                .get(id)
                .is_some_and(|arena_node| arena_node.cached_paint.is_some());
            if cached && node.state.repaint_pending() {
                repaint.push(id);
            }
        }
        for id in orphans {
            self.off_thread.cancel_by_widget(id);
        }
        for (id, outcome) in pulled {
            self.apply_pull_outcome(id, outcome);
        }
        for id in relayout {
            self.arena.mark_needs_layout(id);
            self.arena.mark_ancestors_need_layout(id);
        }
        for id in repaint {
            self.arena.mark_needs_paint(id);
        }
    }

    /// Apply what a pull hook returned for widget `id`.
    fn apply_pull_outcome(&mut self, id: WidgetId, outcome: PullOutcome) {
        let active = self.arena.is_active(id);
        let shown = self.trigger_widget_shown(id);
        match outcome {
            PullOutcome::Unchanged => {}
            PullOutcome::Repaint if shown => self.arena.mark_needs_paint(id),
            // Not shown: drop what it painted, so it repaints whenever it is
            // painted again, without asking its window for a frame now.
            PullOutcome::Repaint => {
                if let Some(node) = self.arena.get_mut(id) {
                    node.cached_paint = None;
                    node.cached_post_paint = None;
                }
            }
            PullOutcome::Relayout if active => {
                self.arena.mark_needs_layout(id);
                self.arena.mark_ancestors_need_layout(id);
            }
            PullOutcome::Relayout => {
                if let Some(node) = self.off_thread.triggers.get(&id) {
                    node.state.keep_relayout_for_activation();
                }
            }
        }
    }

    /// Whether attached widget `id` showed in the last frame: active, painted
    /// in it, and not made transparent by an opacity of its own (the walker
    /// stamps a widget painted before it skips one whose opacity is below
    /// what can be seen).
    fn trigger_widget_shown(&self, id: WidgetId) -> bool {
        self.arena.is_active(id)
            && crate::motion_visibility::painted_this_frame(&self.arena, id, self.paint_epoch)
            && self.arena.get(id).is_some_and(|node| {
                node.opacity_prop
                    .as_ref()
                    .is_none_or(|opacity| opacity.get() >= 1.0 / 512.0)
            })
    }

    /// Whether a widget shown in the last frame has a relayout or a pull
    /// requested off the UI thread pending. The event loop asks it on a state
    /// wake: a window that draws then draws a frame, which takes the request
    /// in and repaints; otherwise it runs a frame that draws nothing.
    #[doc(hidden)]
    pub fn off_thread_needs_frame(&self) -> bool {
        self.off_thread
            .triggers
            .iter()
            .any(|(&id, node)| node.state.state_pending() && self.trigger_widget_shown(id))
    }
}
