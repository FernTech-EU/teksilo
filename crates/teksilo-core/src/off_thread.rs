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
//!
//! A live image attaches a `LiveImageSource` instead
//! ([`BuildContext::attach_live_image`](crate::build_context::BuildContext::attach_live_image)).
//! Its pixels never pass through the tree: a commit raises a flag the
//! window's renderer takes, and wakes the window, which replays its cached
//! frame while the renderer uploads. Its size and status reach layout here:
//! the pre-pass takes every attachment's geometry flag, visible or not, then
//! reads each source's size and status once and writes them into the
//! attachment's [`LiveImageSignals`], which the widget binds. A rebuild
//! detaches a widget's live images and its new `build()` attaches them again;
//! the textures, which the renderer keys by source, are kept.

mod live_image;
mod repaint_trigger;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use teksilo_canvas::ImageGeometry;
use teksilo_canvas::live_image::{LiveImageConsumer, LiveImageId, LiveImageSource, LiveImageStats};
use teksilo_canvas::wake::RedrawWaker;

pub(crate) use live_image::AttachmentEntry;
pub use live_image::{LiveImageAttachment, LiveImageSignals};
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
    /// Each widget's live-image attachments, in the order it attached them.
    live: HashMap<WidgetId, Vec<Rc<AttachmentEntry>>>,
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
        for entry in self.live.values().flatten() {
            entry.consumer.set_waker(waker.clone());
        }
        self.waker = waker;
    }

    /// Release everything attached to `id`, on its destruction.
    pub(crate) fn cancel_by_widget(&mut self, id: WidgetId) {
        self.release_triggers(id);
        self.detach_live(id);
    }

    /// Release `id`'s wake state, which its triggers and pull hook share,
    /// and nothing else: its live images are attached apart.
    fn release_triggers(&mut self, id: WidgetId) {
        if let Some(node) = self.triggers.remove(&id) {
            node.state.detach();
        }
    }

    /// Widget `id` is about to be rebuilt: its new `build()` attaches its
    /// triggers and sets its hook again. Its wake state, and whatever is
    /// pending on it, stays. Its live images are detached: the new `build()`
    /// attaches them again, and records their current size and status as it
    /// does.
    pub(crate) fn begin_rebuild(&mut self, id: WidgetId) {
        if let Some(node) = self.triggers.get_mut(&id) {
            node.previous = Some(std::mem::take(&mut node.triggers));
            node.hook = None;
        }
        self.detach_live(id);
    }

    fn detach_live(&mut self, id: WidgetId) {
        for entry in self.live.remove(&id).unwrap_or_default() {
            entry.consumer.detach();
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
        for entry in self.live.values().flatten() {
            entry.consumer.detach();
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
    /// longer attached, and the widget's wake state if it attached none and
    /// set no hook. The live images that `build()` attached stay: they are
    /// the widget's, not its triggers'.
    pub(crate) fn finish_off_thread_rebuild(&mut self, id: WidgetId) {
        let Some(node) = self.off_thread.triggers.get_mut(&id) else {
            return;
        };
        for stale in node.previous.take().unwrap_or_default() {
            stale.detach_state(&node.state);
        }
        if node.triggers.is_empty() && node.hook.is_none() {
            self.off_thread.release_triggers(id);
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

    /// Attach `source` to widget `id`, in this tree's window, writing its
    /// size and status into `signals`. Register-then-read: the source sees
    /// the attachment before its size and status are read, so a change made
    /// meanwhile is either in what is written or wakes the window.
    pub(crate) fn attach_live_image(
        &mut self,
        id: WidgetId,
        source: &LiveImageSource,
        signals: &LiveImageSignals,
    ) -> LiveImageAttachment {
        let consumer = source.attach(self.off_thread.waker.clone());
        let meta = consumer.layout_meta();
        signals.frame_size.set_if_changed(meta.size);
        signals.status.set_if_changed(meta.status);
        let entry = Rc::new(AttachmentEntry::new(consumer, signals.clone(), id));
        self.off_thread
            .live
            .entry(id)
            .or_default()
            .push(entry.clone());
        LiveImageAttachment { entry }
    }

    /// How many live-image attachments this tree holds.
    pub fn live_image_attachment_count(&self) -> usize {
        self.off_thread.live.values().map(Vec::len).sum()
    }

    /// The consumer of the live image widget `id` shows: the first it
    /// attached. Found by widget id, so a `WidgetBuilder` wrapper around the
    /// widget does not hide it.
    pub fn live_image_consumer(&self, id: WidgetId) -> Option<LiveImageConsumer> {
        self.first_live(id).map(|e| e.consumer.clone())
    }

    /// The counters of the live image widget `id` shows: its source's and
    /// its attachment's.
    pub fn live_image_stats(&self, id: WidgetId) -> Option<LiveImageStats> {
        self.first_live(id).map(|e| e.consumer.stats())
    }

    /// The placement the live image widget `id` recorded at its last layout.
    pub fn live_image_geometry(&self, id: WidgetId) -> Option<teksilo_canvas::ImageGeometry> {
        self.first_live(id).and_then(|e| e.geometry())
    }

    /// Every live-image attachment of this tree, with its widget: what a
    /// screenshot matches a frame's live quads against.
    pub fn live_image_attachments(
        &self,
    ) -> impl Iterator<Item = (WidgetId, LiveImageConsumer)> + '_ {
        self.off_thread
            .live
            .iter()
            .flat_map(|(&id, entries)| entries.iter().map(move |e| (id, e.consumer.clone())))
    }

    fn first_live(&self, id: WidgetId) -> Option<&Rc<AttachmentEntry>> {
        self.off_thread
            .live
            .get(&id)
            .and_then(|entries| entries.first())
    }

    /// The layout pre-pass: take in what changed off the UI thread since the
    /// last frame, triggers first, then live images.
    pub(crate) fn poll_off_thread(&mut self) {
        self.poll_triggers();
        self.poll_live_images();
    }

    /// The live-image half: for every attachment, visible or not, take its
    /// geometry flag, then read its source's size and status once per
    /// source, record them for the paint that follows, and write the
    /// attachment's Signals where they changed. Two phases: everything is
    /// collected before a Signal is set, so no registry borrow is held while
    /// a Signal's observers run.
    fn poll_live_images(&mut self) {
        if self.off_thread.live.is_empty() {
            return;
        }
        let mut orphans = Vec::new();
        let mut resnap = Vec::new();
        let mut by_source: Vec<(LiveImageId, Vec<Rc<AttachmentEntry>>)> = Vec::new();
        for (&id, entries) in &self.off_thread.live {
            if self.arena.get(id).is_none() {
                orphans.push(id);
                continue;
            }
            // Snapped under a transform an ancestor has changed since: a
            // transform scope repaints and lays nothing out, so the widget is
            // laid out again here to snap anew. Between two transforms that
            // neither snaps under, its placement is the same.
            if self.arena.is_active(id)
                && entries.iter().any(|entry| {
                    entry.snapped_under().is_some_and(|then| {
                        let now = self.arena.effective_transform(id);
                        now != then
                            && (ImageGeometry::snaps_under(then) || ImageGeometry::snaps_under(now))
                    })
                })
            {
                resnap.push(id);
            }
            for entry in entries {
                let source = entry.consumer.source().id();
                match by_source.iter_mut().find(|(other, _)| *other == source) {
                    Some((_, group)) => group.push(entry.clone()),
                    None => by_source.push((source, vec![entry.clone()])),
                }
            }
        }
        for id in orphans {
            self.off_thread.cancel_by_widget(id);
        }
        for id in resnap {
            self.arena.mark_needs_layout(id);
            self.arena.mark_ancestors_need_layout(id);
        }
        let mut writes = Vec::new();
        for (_, group) in by_source {
            for entry in &group {
                let _ = entry.consumer.take_geometry();
            }
            // Read after every flag of the source is taken: a change made
            // after this read raised a flag again and wakes the window.
            let meta = group[0].consumer.source().meta();
            for entry in group {
                entry.consumer.record_layout_meta(meta);
                writes.push((entry.signals.clone(), meta));
            }
        }
        for (signals, meta) in writes {
            signals.frame_size.set_if_changed(meta.size);
            signals.status.set_if_changed(meta.status);
        }
    }

    /// The trigger half of the pre-pass: take in what attached triggers
    /// requested since the last frame. Collects first, then marks, so no
    /// arena borrow is held while a pull hook runs.
    fn poll_triggers(&mut self) {
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
        let shown = self.off_thread_widget_shown(id);
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
    fn off_thread_widget_shown(&self, id: WidgetId) -> bool {
        self.arena.is_active(id)
            && crate::motion_visibility::painted_this_frame(&self.arena, id, self.paint_epoch)
            && self.arena.get(id).is_some_and(|node| {
                node.opacity_prop
                    .as_ref()
                    .is_none_or(|opacity| opacity.get() >= 1.0 / 512.0)
            })
    }

    /// Whether a widget shown in the last frame has a relayout or a pull
    /// requested off the UI thread pending, or a live image whose size or
    /// status changed. The event loop asks it on a state wake: a window that
    /// draws then draws a frame, which takes the change in and repaints;
    /// otherwise it runs a frame that draws nothing, which keeps layout's
    /// view of the change current.
    #[doc(hidden)]
    pub fn off_thread_needs_frame(&self) -> bool {
        self.off_thread
            .triggers
            .iter()
            .any(|(&id, node)| node.state.state_pending() && self.off_thread_widget_shown(id))
            || self.off_thread.live.iter().any(|(&id, entries)| {
                entries.iter().any(|e| e.consumer.geometry_pending())
                    && self.off_thread_widget_shown(id)
            })
    }
}
