// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`StepperController`] — a shared, cloneable handle that drives a
//! [`Stepper`](crate::stepper::Stepper) and lets app code reset / jump /
//! introspect it from the outside.
//!
//! Mirrors the `SceneModel = Rc<RefCell<…>>` pattern: cloning a controller
//! produces a second handle to the **same** state, so a toolbar button and the
//! stepper itself share one source of truth. Every mutator takes `&self`,
//! mutates the inner state, drops the borrow, then writes the reactive signals
//! — so a signal observer (the bound stepper rebuild) never re-borrows the
//! controller mid-mutation.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_core::signal::Signal;

use super::step::StepStatus;

struct StepperState {
    step_count: usize,
    statuses: Vec<StepStatus>,
    /// The statuses the stepper seeded from its `Step` list. `reset()`
    /// restores these rather than blanket `Upcoming`, so a step declared
    /// `Disabled` / `Optional` keeps that character across a reset.
    initial_statuses: Vec<StepStatus>,
    /// Per-step visibility, driven by [`super::Step::visible_when`] (or
    /// [`StepperController::set_visible`]). An invisible step is skipped by
    /// navigation and hidden from the indicator strip.
    visible: Vec<bool>,
    /// Back-stack of previously-visited step indices (most recent last),
    /// not including the current step. `back()` pops this — so it returns
    /// to where the user actually came from, even after a non-linear jump.
    visit_history: Vec<usize>,
    visited: Vec<bool>,
    skipped: Vec<bool>,
    /// `true` once [`StepperController::seed_statuses`] has run, so a stepper
    /// rebuild does not wipe accumulated progress.
    seeded: bool,
}

impl StepperState {
    /// A step is reachable when it is visible and not `Disabled` — the two
    /// ways an app takes a step out of the flow.
    fn reachable(&self, idx: usize) -> bool {
        self.visible.get(idx).copied().unwrap_or(false)
            && !matches!(self.statuses.get(idx), Some(StepStatus::Disabled))
    }

    fn next_reachable(&self, from: usize) -> Option<usize> {
        ((from + 1)..self.step_count).find(|&i| self.reachable(i))
    }

    fn first_reachable(&self) -> Option<usize> {
        (0..self.step_count).find(|&i| self.reachable(i))
    }
}

/// Shared handle controlling a [`Stepper`](crate::stepper::Stepper).
#[derive(Clone)]
pub struct StepperController {
    inner: Rc<RefCell<StepperState>>,
    /// Active step index — the stepper's `Switcher` and indicator strip bind
    /// to this.
    current: Signal<usize>,
    /// Bumped on every structural mutation; the stepper binds it at
    /// `BindingLevel::Rebuild` so external `go_to`/`set_status`/`reset`
    /// re-derive the indicator strip and footer.
    version: Signal<u64>,
}

impl StepperController {
    /// A controller for a stepper with `step_count` steps, starting at step 0.
    pub fn new(step_count: usize) -> Self {
        let mut visited = vec![false; step_count];
        if step_count > 0 {
            visited[0] = true;
        }
        Self {
            inner: Rc::new(RefCell::new(StepperState {
                step_count,
                statuses: vec![StepStatus::Upcoming; step_count],
                initial_statuses: vec![StepStatus::Upcoming; step_count],
                visible: vec![true; step_count],
                visit_history: Vec::new(),
                visited,
                skipped: vec![false; step_count],
                seeded: false,
            })),
            current: Signal::new(0),
            version: Signal::new(0),
        }
    }

    /// Seed the per-step statuses (called by the stepper from its `Step`
    /// list). Marks the active step `Active`. Idempotent: a no-op after the
    /// first call, so a stepper rebuild never wipes accumulated progress.
    pub(crate) fn seed_statuses(&self, statuses: Vec<StepStatus>) {
        {
            let mut st = self.inner.borrow_mut();
            if st.seeded {
                return;
            }
            if statuses.len() == st.step_count {
                st.initial_statuses = statuses.clone();
                st.statuses = statuses;
            }
            st.seeded = true;
        }
        let cur = self.current.get();
        self.mark_active(cur);
    }

    fn bump(&self) {
        self.version.set(self.version.get().wrapping_add(1));
    }

    /// Set `idx` to `Active` and demote any other previously-active step.
    fn mark_active(&self, idx: usize) {
        {
            let mut st = self.inner.borrow_mut();
            for (i, s) in st.statuses.iter_mut().enumerate() {
                if *s == StepStatus::Active && i != idx {
                    // A step we are leaving becomes Complete unless it was an
                    // error/skip; keep terminal states sticky.
                    *s = StepStatus::Complete;
                }
            }
            if let Some(s) = st.statuses.get_mut(idx) {
                if !matches!(*s, StepStatus::Error | StepStatus::Disabled) {
                    *s = StepStatus::Active;
                }
            }
        }
        self.bump();
    }

    /// Advance to the next **reachable** step, recording the current one on
    /// the back-stack. Invisible ([`Step::visible_when`](super::Step::visible_when))
    /// and [`StepStatus::Disabled`] steps are stepped over; a no-op when none
    /// remains.
    pub fn next(&self) {
        let cur = self.current.get();
        let dest = {
            let mut st = self.inner.borrow_mut();
            let Some(dest) = st.next_reachable(cur) else {
                return;
            };
            st.visit_history.push(cur);
            if let Some(v) = st.visited.get_mut(dest) {
                *v = true;
            }
            dest
        };
        self.current.set(dest);
        self.mark_active(dest);
    }

    /// Mark the current (optional) step skipped, then advance like
    /// [`next`](Self::next).
    pub fn skip(&self) {
        let cur = self.current.get();
        {
            let mut st = self.inner.borrow_mut();
            if let Some(sk) = st.skipped.get_mut(cur) {
                *sk = true;
            }
            if let Some(s) = st.statuses.get_mut(cur) {
                *s = StepStatus::Skipped;
            }
        }
        self.next();
    }

    /// Return to the most recently visited **reachable** step (the back-stack
    /// top). Entries that became unreachable meanwhile are popped and skipped.
    /// No-op on an empty stack.
    pub fn back(&self) {
        let dest = {
            let mut st = self.inner.borrow_mut();
            loop {
                match st.visit_history.pop() {
                    Some(i) if st.reachable(i) => break Some(i),
                    Some(_) => continue,
                    None => break None,
                }
            }
        };
        if let Some(dest) = dest {
            self.current.set(dest);
            self.mark_active(dest);
        }
    }

    /// Jump to step `idx` (non-linear), recording the current step on the
    /// back-stack so [`back`](Self::back) returns here. A no-op when `idx` is
    /// out of range or not [reachable](Self::is_reachable).
    pub fn go_to(&self, idx: usize) {
        let cur = self.current.get();
        {
            let mut st = self.inner.borrow_mut();
            if idx == cur || !st.reachable(idx) {
                return;
            }
            st.visit_history.push(cur);
            if let Some(v) = st.visited.get_mut(idx) {
                *v = true;
            }
        }
        self.current.set(idx);
        self.mark_active(idx);
    }

    /// Reset to the first reachable step: clears the back-stack, restores the
    /// statuses the stepper was declared with (so a `Disabled` / `Optional`
    /// step keeps its character), and clears visited/skipped flags. Per-step
    /// visibility is app-owned and left untouched.
    pub fn reset(&self) {
        let dest = {
            let mut st = self.inner.borrow_mut();
            let n = st.step_count;
            st.statuses = st.initial_statuses.clone();
            st.visit_history.clear();
            st.visited = vec![false; n];
            st.skipped = vec![false; n];
            let dest = st.first_reachable().unwrap_or(0);
            if let Some(v) = st.visited.get_mut(dest) {
                *v = true;
            }
            dest
        };
        self.current.set(dest);
        self.mark_active(dest);
    }

    /// Override a step's [`StepStatus`] (e.g. mark it `Error` after async
    /// validation). Setting [`StepStatus::Disabled`] takes the step out of the
    /// flow — [`next`](Self::next) / [`go_to`](Self::go_to) skip it — but does
    /// **not** move off it if it is the active step.
    pub fn set_status(&self, idx: usize, status: StepStatus) {
        {
            let mut st = self.inner.borrow_mut();
            if let Some(s) = st.statuses.get_mut(idx) {
                *s = status;
            }
        }
        self.bump();
    }

    /// Show or hide step `idx`. A hidden step is skipped by
    /// [`next`](Self::next) / [`back`](Self::back) / [`go_to`](Self::go_to)
    /// and drops out of the indicator strip — the branching-wizard shape
    /// ("this step only if you chose X") without maintaining two step lists.
    ///
    /// Usually driven declaratively by
    /// [`Step::visible_when`](super::Step::visible_when); this is the
    /// imperative twin. Hiding the *active* step does not navigate away from
    /// it — hide steps the user has not reached yet.
    pub fn set_visible(&self, idx: usize, visible: bool) {
        {
            let mut st = self.inner.borrow_mut();
            match st.visible.get_mut(idx) {
                Some(v) if *v == visible => return,
                Some(v) => *v = visible,
                None => return,
            }
        }
        self.bump();
    }

    // ── queries ────────────────────────────────────────────────────────────

    pub fn current(&self) -> usize {
        self.current.get()
    }

    pub fn status(&self, idx: usize) -> StepStatus {
        self.inner
            .borrow()
            .statuses
            .get(idx)
            .copied()
            .unwrap_or_default()
    }

    /// `true` if step `idx` has ever been the active step.
    pub fn visited(&self, idx: usize) -> bool {
        self.inner
            .borrow()
            .visited
            .get(idx)
            .copied()
            .unwrap_or(false)
    }

    /// `true` if step `idx` was skipped via [`skip`](Self::skip).
    pub fn skipped(&self, idx: usize) -> bool {
        self.inner
            .borrow()
            .skipped
            .get(idx)
            .copied()
            .unwrap_or(false)
    }

    /// `true` if step `idx` is visible (see [`set_visible`](Self::set_visible)).
    pub fn is_visible(&self, idx: usize) -> bool {
        self.inner
            .borrow()
            .visible
            .get(idx)
            .copied()
            .unwrap_or(false)
    }

    /// `true` if step `idx` participates in the flow — visible **and** not
    /// [`StepStatus::Disabled`].
    pub fn is_reachable(&self, idx: usize) -> bool {
        self.inner.borrow().reachable(idx)
    }

    /// The next reachable step after `from`, if any.
    pub fn next_reachable(&self, from: usize) -> Option<usize> {
        self.inner.borrow().next_reachable(from)
    }

    /// `true` if [`next`](Self::next) would move — i.e. the active step is not
    /// the last reachable one. The footer shows Next when this holds and
    /// Finish when it does not.
    pub fn has_next(&self) -> bool {
        let cur = self.current.get();
        self.inner.borrow().next_reachable(cur).is_some()
    }

    pub fn step_count(&self) -> usize {
        self.inner.borrow().step_count
    }

    /// `true` if there is a previously-visited, still-reachable step to
    /// return to.
    pub fn can_back(&self) -> bool {
        let st = self.inner.borrow();
        st.visit_history.iter().any(|&i| st.reachable(i))
    }

    // ── reactive surface ─────────────────────────────────────────────────────

    /// The active-step signal — the stepper's `Switcher` and indicators bind
    /// to it.
    pub fn current_step_signal(&self) -> Signal<usize> {
        self.current.clone()
    }

    /// Bumped on every structural mutation; bind at `BindingLevel::Rebuild`.
    pub fn version_signal(&self) -> Signal<u64> {
        self.version.clone()
    }
}

impl std::fmt::Debug for StepperController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepperController")
            .field("current", &self.current.get())
            .field("step_count", &self.step_count())
            .finish()
    }
}
