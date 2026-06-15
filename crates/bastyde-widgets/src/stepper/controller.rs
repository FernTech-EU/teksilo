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

use bastyde_core::signal::Signal;

use super::step::StepStatus;

struct StepperState {
    step_count: usize,
    statuses: Vec<StepStatus>,
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

    /// Advance to the next step (clamped), recording the current one on the
    /// back-stack.
    pub fn next(&self) {
        let cur = self.current.get();
        let dest = {
            let mut st = self.inner.borrow_mut();
            if cur + 1 >= st.step_count {
                return;
            }
            let dest = cur + 1;
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

    /// Return to the most recently visited step (the back-stack top). No-op
    /// on an empty stack.
    pub fn back(&self) {
        let dest = {
            let mut st = self.inner.borrow_mut();
            st.visit_history.pop()
        };
        if let Some(dest) = dest {
            self.current.set(dest);
            self.mark_active(dest);
        }
    }

    /// Jump to step `idx` (non-linear), recording the current step on the
    /// back-stack so [`back`](Self::back) returns here.
    pub fn go_to(&self, idx: usize) {
        let cur = self.current.get();
        {
            let mut st = self.inner.borrow_mut();
            if idx >= st.step_count || idx == cur {
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

    /// Reset to step 0: clears the back-stack, all statuses to `Upcoming`
    /// (then step 0 `Active`), and visited/skipped flags.
    pub fn reset(&self) {
        {
            let mut st = self.inner.borrow_mut();
            let n = st.step_count;
            st.statuses = vec![StepStatus::Upcoming; n];
            st.visit_history.clear();
            st.visited = vec![false; n];
            if n > 0 {
                st.visited[0] = true;
            }
            st.skipped = vec![false; n];
        }
        self.current.set(0);
        self.mark_active(0);
    }

    /// Override a step's [`StepStatus`] (e.g. mark it `Error` after async
    /// validation).
    pub fn set_status(&self, idx: usize, status: StepStatus) {
        {
            let mut st = self.inner.borrow_mut();
            if let Some(s) = st.statuses.get_mut(idx) {
                *s = status;
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

    pub fn step_count(&self) -> usize {
        self.inner.borrow().step_count
    }

    /// `true` if there is a previously-visited step to return to.
    pub fn can_back(&self) -> bool {
        !self.inner.borrow().visit_history.is_empty()
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
