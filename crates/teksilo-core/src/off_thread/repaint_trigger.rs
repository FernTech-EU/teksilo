// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`RepaintTrigger`]: a widget whose content changes off the UI thread
//! repaints, relayouts or takes the change in, and only its window wakes.

use std::sync::{Arc, Weak};

use teksilo_canvas::sync::{AtomicBool, AtomicU32, AtomicU64, Mutex, Ordering};
use teksilo_canvas::wake::{RedrawWaker, WakeFlag, WakeKind};

/// Marks the widgets it is attached to for repaint, relayout or a pull
/// from any thread, and wakes only their windows.
///
/// For content that changes off the UI thread: a terminal's child printing,
/// a background task's progress, a status a service publishes. The producer
/// stores the new state where the widget will read it, *then* requests; the
/// widget reads it in the frame the request wakes. Requests coalesce: one
/// wake per window until the widget has consumed the request.
///
/// - [`request_repaint`](Self::request_repaint): only what the widget paints
///   changed. Its window redraws, and the widget alone repaints.
/// - [`request_relayout`](Self::request_relayout): its size or what layout
///   reads changed. It and its ancestors relayout, also in a window that
///   draws nothing.
/// - [`request_pull`](Self::request_pull): the widget has data to take
///   in on the UI thread whether or not it is shown, through the hook it
///   registered with `BuildContext::on_trigger_pull`. A widget that is not
///   shown takes it in without its window drawing a frame.
///
/// A relayout or a pull reaches a window that draws nothing (minimised,
/// covered, its frames withheld by the compositor), whose event loop then
/// runs the layout half of a frame for it; a repaint waits there for the
/// window to draw again.
///
/// Attach it in every `build()` with `BuildContext::attach_repaint_trigger`;
/// a rebuild or the widget's destruction releases the attachment, and the
/// tree's drop releases the rest. One trigger may be attached to several
/// widgets, in several windows. `Send + Sync`; a clone shares the trigger.
///
/// Do not request from `paint()` or a frame-tick effect: the widget would
/// repaint at display rate. Inside an event handler, the widget's own
/// invalidation (`EventContext::request_repaint`) is cheaper.
#[derive(Clone, Default)]
pub struct RepaintTrigger {
    shared: Arc<TriggerShared>,
}

/// What a [`RepaintTrigger`] did. Monotonic counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RepaintTriggerStats {
    /// Requests of every kind.
    pub requests: u64,
    /// Window wakes issued.
    pub wakes: u64,
    /// Requests that found at least one attached widget and woke nobody:
    /// merged into a wake already on its way.
    pub wakes_coalesced: u64,
    /// Paints that consumed a pending repaint.
    pub repaints: u64,
    /// Layout passes or activations that consumed a pending relayout.
    pub relayouts: u64,
    /// Pull hooks run for a pending pull.
    pub pulls: u64,
    /// Widgets it is attached to.
    pub attachments: u32,
}

/// What a widget's pull hook (`BuildContext::on_trigger_pull`) did with
/// the data it took in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PullOutcome {
    /// Nothing the widget shows changed.
    Unchanged,
    /// What it paints changed. A widget that is shown repaints in this frame;
    /// one that is not repaints when it is shown again, and its window draws
    /// nothing for it now.
    Repaint,
    /// Its size or what layout reads changed: it and its ancestors relayout,
    /// and it repaints.
    Relayout,
}

#[derive(Default)]
struct TriggerShared {
    /// The widgets attached, by their wake state. A leaf lock, released
    /// before anything else is locked or any waker runs.
    nodes: Mutex<Vec<Weak<NodeWakeState>>>,
    counters: Arc<TriggerCounters>,
}

#[derive(Default)]
pub(crate) struct TriggerCounters {
    requests: AtomicU64,
    wakes: AtomicU64,
    wakes_coalesced: AtomicU64,
    repaints: AtomicU64,
    relayouts: AtomicU64,
    pulls: AtomicU64,
    attachments: AtomicU32,
}

#[derive(Clone, Copy)]
enum Request {
    Repaint,
    Relayout,
    Pull,
}

impl RepaintTrigger {
    /// A trigger attached to nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Only what the attached widgets paint changed. Each window wakes at
    /// most once until the widget's next paint. A widget that is not painted
    /// (dormant, clipped out) keeps the request for when it is, and costs one
    /// wake until then, however many requests follow.
    pub fn request_repaint(&self) {
        self.request(Request::Repaint);
    }

    /// The attached widgets' size or what layout reads changed: they and
    /// their ancestors relayout, and repaint. Each window wakes at most once
    /// until the widget's next layout pass, and the wake reaches a window
    /// that draws nothing.
    pub fn request_relayout(&self) {
        self.request(Request::Relayout);
    }

    /// The attached widgets have data to take in on the UI thread, shown or
    /// not: their pull hook (`BuildContext::on_trigger_pull`) runs in
    /// their window's next frame. The window draws that frame when a widget
    /// with a pull pending was shown in its last one, and otherwise runs
    /// one that draws nothing. Each window wakes at most once until the hook
    /// has run. A widget with no hook is repainted as by
    /// [`request_repaint`](Self::request_repaint).
    pub fn request_pull(&self) {
        self.request(Request::Pull);
    }

    /// Whether the trigger is attached to at least one live widget.
    pub fn is_attached(&self) -> bool {
        self.shared.counters.attachments.load(Ordering::Relaxed) > 0
    }

    /// What the trigger did so far.
    pub fn stats(&self) -> RepaintTriggerStats {
        let c = &self.shared.counters;
        RepaintTriggerStats {
            requests: c.requests.load(Ordering::Relaxed),
            wakes: c.wakes.load(Ordering::Relaxed),
            wakes_coalesced: c.wakes_coalesced.load(Ordering::Relaxed),
            repaints: c.repaints.load(Ordering::Relaxed),
            relayouts: c.relayouts.load(Ordering::Relaxed),
            pulls: c.pulls.load(Ordering::Relaxed),
            attachments: c.attachments.load(Ordering::Relaxed),
        }
    }

    /// Whether `self` and `other` are the same trigger.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    fn request(&self, request: Request) {
        let counters = &self.shared.counters;
        counters.requests.fetch_add(1, Ordering::Relaxed);
        // Under the list lock only long enough to copy it out: no waker runs
        // and no other lock is taken while it is held.
        let live: Vec<Arc<NodeWakeState>> = {
            let mut nodes = self
                .shared
                .nodes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            nodes.retain(|weak| {
                weak.upgrade()
                    .is_some_and(|state| !state.detached.load(Ordering::Acquire))
            });
            nodes.iter().filter_map(Weak::upgrade).collect()
        };
        let mut wakers: Vec<(Arc<dyn RedrawWaker>, WakeKind)> = Vec::new();
        let mut raised_any = false;
        for state in &live {
            let flag = match request {
                Request::Repaint => &state.repaint,
                Request::Relayout => &state.relayout,
                Request::Pull => &state.pull,
            };
            if !flag.raise() {
                continue;
            }
            raised_any = true;
            let Some(waker) = state.waker() else {
                continue;
            };
            // A relayout and a pull keep state current, so they must
            // reach a window that draws nothing. Whether one draws a frame
            // is the event loop's to decide, which knows what the window
            // showed and whether it draws: a guess made here, from another
            // thread, can be stale in a way that strands the request.
            let kind = match request {
                Request::Repaint => WakeKind::Draw,
                Request::Relayout | Request::Pull => WakeKind::Layout,
            };
            // Each window once per request, with the larger kind.
            let data = Arc::as_ptr(&waker) as *const ();
            match wakers
                .iter_mut()
                .find(|(other, _)| Arc::as_ptr(other) as *const () == data)
            {
                Some((_, other_kind)) => *other_kind = (*other_kind).max(kind),
                None => wakers.push((waker, kind)),
            }
        }
        for (waker, kind) in wakers {
            waker.wake(kind);
            counters.wakes.fetch_add(1, Ordering::Relaxed);
        }
        if !live.is_empty() && !raised_any {
            counters.wakes_coalesced.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Detach from `state` only, leaving its other triggers and whatever is
    /// pending on it: a rebuild whose new `build()` no longer attaches this
    /// trigger. UI thread.
    pub(crate) fn detach_state(&self, state: &Arc<NodeWakeState>) {
        self.shared
            .nodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|weak| !std::ptr::eq(weak.as_ptr(), Arc::as_ptr(state)));
        if state.remove_sink(&self.shared.counters) {
            self.shared
                .counters
                .attachments
                .fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Attach to `state`: the producer side of register-then-read. The state
    /// is published to the list under its lock, after which a request sees
    /// it.
    pub(crate) fn attach_state(&self, state: &Arc<NodeWakeState>) {
        state.add_sink(self.shared.counters.clone());
        let mut nodes = self
            .shared
            .nodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Prune here too: a trigger attached on every rebuild and never
        // requested would otherwise grow its list without bound.
        nodes.retain(|weak| {
            weak.upgrade()
                .is_some_and(|other| !other.detached.load(Ordering::Acquire))
        });
        nodes.push(Arc::downgrade(state));
        drop(nodes);
        self.shared
            .counters
            .attachments
            .fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn node_list_len(&self) -> usize {
        self.shared
            .nodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

impl std::fmt::Debug for RepaintTrigger {
    /// Counters only: never takes the list lock, so it is safe to print from
    /// anywhere, while a request is in progress included.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.stats();
        write!(
            f,
            "RepaintTrigger(attachments {}, requests {}, wakes {})",
            s.attachments, s.requests, s.wakes
        )
    }
}

/// One per attached widget, shared by every trigger attached to it.
pub(crate) struct NodeWakeState {
    repaint: WakeFlag,
    relayout: WakeFlag,
    pull: WakeFlag,
    detached: AtomicBool,
    /// The window's waker. A leaf lock.
    waker: Mutex<Option<Arc<dyn RedrawWaker>>>,
    /// The counters of every trigger attached here. UI thread only.
    sinks: Mutex<Vec<Arc<TriggerCounters>>>,
}

impl NodeWakeState {
    pub(crate) fn new(waker: Option<Arc<dyn RedrawWaker>>) -> Self {
        Self {
            repaint: WakeFlag::new(),
            relayout: WakeFlag::new(),
            pull: WakeFlag::new(),
            detached: AtomicBool::new(false),
            waker: Mutex::new(waker),
            sinks: Mutex::new(Vec::new()),
        }
    }

    fn waker(&self) -> Option<Arc<dyn RedrawWaker>> {
        self.waker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Install the window's waker, then wake it for whatever was requested
    /// while there was none: such a request raised its flag, found no waker,
    /// and woke nobody.
    pub(crate) fn set_waker(&self, waker: Option<Arc<dyn RedrawWaker>>) {
        *self
            .waker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = waker.clone();
        let Some(waker) = waker else {
            return;
        };
        // Once, with the larger kind owed: a relayout or a pull must reach
        // a window that draws nothing; a repaint need not.
        if self.relayout.is_raised() || self.pull.is_raised() {
            waker.wake(WakeKind::Layout);
        } else if self.repaint.is_raised() {
            waker.wake(WakeKind::Draw);
        }
    }

    fn add_sink(&self, counters: Arc<TriggerCounters>) {
        self.sinks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(counters);
    }

    /// Drop the counters of one trigger; whether they were here.
    fn remove_sink(&self, counters: &Arc<TriggerCounters>) -> bool {
        let mut sinks = self
            .sinks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = sinks.len();
        sinks.retain(|sink| !Arc::ptr_eq(sink, counters));
        sinks.len() != before
    }

    fn for_each_sink(&self, f: impl Fn(&TriggerCounters)) {
        for sink in self
            .sinks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            f(sink);
        }
    }

    /// The paint walker, right before the widget's `paint()`: the repaint
    /// requested so far is served by it.
    pub(crate) fn consume_repaint(&self) {
        if self.repaint.take() {
            self.for_each_sink(|c| {
                c.repaints.fetch_add(1, Ordering::Relaxed);
            });
        }
    }

    /// The pre-pass, or an activation: take a pending relayout.
    pub(crate) fn take_relayout(&self) -> bool {
        let taken = self.relayout.take();
        if taken {
            self.for_each_sink(|c| {
                c.relayouts.fetch_add(1, Ordering::Relaxed);
            });
        }
        taken
    }

    /// Leave a relayout for the widget's activation, without waking.
    pub(crate) fn keep_relayout_for_activation(&self) {
        self.relayout.park();
    }

    /// The pre-pass: take a pending pull, counting the hook run for it.
    pub(crate) fn take_pull(&self) -> bool {
        let taken = self.pull.take();
        if taken {
            self.for_each_sink(|c| {
                c.pulls.fetch_add(1, Ordering::Relaxed);
            });
        }
        taken
    }

    /// The pre-pass, deciding whether to mark the widget for paint before
    /// the walker takes the request: an RMW, see `WakeFlag::peek`.
    pub(crate) fn repaint_pending(&self) -> bool {
        self.repaint.peek()
    }

    /// Whether a relayout or a pull is pending. Advisory: the UI thread
    /// choosing a frame or a frame that draws nothing, either of which takes
    /// it in.
    pub(crate) fn state_pending(&self) -> bool {
        self.relayout.is_raised() || self.pull.is_raised()
    }

    /// Release the attachment: no request reaches it again, and its window's
    /// waker is let go here, on the UI thread. Idempotent.
    pub(crate) fn detach(&self) {
        if self.detached.swap(true, Ordering::AcqRel) {
            return;
        }
        let waker = self
            .waker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(waker);
        self.for_each_sink(|c| {
            c.attachments.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

#[cfg(all(test, not(teksilo_loom)))]
mod tests;

#[cfg(all(test, teksilo_loom))]
mod loom_tests;
