// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`StepNav`] — the Next / Finish semantics shared by the footer buttons and
//! the Enter-key shortcut, plus [`FinishOutcome`]: the contract that lets an
//! `on_finish` callback *refuse* to complete the flow.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::EventContext;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;

use super::controller::StepperController;
use super::step::{StepStatus, StepValidator};

/// What an [`on_finish`](super::Stepper::on_finish) callback decided.
///
/// `Finish` is the mirror of [`Step::validate_on_next`](super::Step::validate_on_next):
/// the last step gets to say "no". Returning [`Rejected`](Self::Rejected)
/// keeps the stepper on the last step, marks it [`StepStatus::Error`], and — in
/// a [`Wizard`](super::Wizard) — leaves the modal open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishOutcome {
    /// The flow completed. The last step is marked `Complete`; a `Wizard`
    /// modal dismisses itself.
    Finished,
    /// The finish attempt failed (disk full, name taken, server refused). The
    /// stepper stays put and marks the step `Error`; a `Wizard` modal stays
    /// open so the user can correct the input and retry.
    Rejected,
}

/// Return-type bridge for [`Stepper::on_finish`](super::Stepper::on_finish) /
/// [`Wizard::on_finish`](super::Wizard::on_finish) callbacks, so the same
/// setter accepts a callback that cannot fail and one that can.
///
/// | callback returns | outcome |
/// | --- | --- |
/// | `()` | `Finished` (finishing always succeeds) |
/// | `bool` | `true` → `Finished`, `false` → `Rejected` |
/// | `Result<T, E>` | `Ok` → `Finished`, `Err` → `Rejected` |
/// | [`FinishOutcome`] | itself |
pub trait IntoFinishOutcome {
    fn into_finish_outcome(self) -> FinishOutcome;
}

impl IntoFinishOutcome for () {
    fn into_finish_outcome(self) -> FinishOutcome {
        FinishOutcome::Finished
    }
}

impl IntoFinishOutcome for bool {
    fn into_finish_outcome(self) -> FinishOutcome {
        if self {
            FinishOutcome::Finished
        } else {
            FinishOutcome::Rejected
        }
    }
}

impl IntoFinishOutcome for FinishOutcome {
    fn into_finish_outcome(self) -> FinishOutcome {
        self
    }
}

impl<T, E> IntoFinishOutcome for Result<T, E> {
    fn into_finish_outcome(self) -> FinishOutcome {
        match self {
            Ok(_) => FinishOutcome::Finished,
            Err(_) => FinishOutcome::Rejected,
        }
    }
}

pub(crate) type FinishAction = Rc<dyn Fn(&mut EventContext, &StepperController) -> FinishOutcome>;

/// The per-step gates + actions the footer and the Enter shortcut both need.
///
/// Built once by [`Stepper::build`](super::Stepper) and shared as an `Rc`, so
/// pressing Enter on a step form and clicking Next run the *same* code path
/// (validators, completion gate, status transitions, focus hand-off).
pub(crate) struct StepNav {
    controller: StepperController,
    validators: Vec<Option<StepValidator>>,
    completion: Vec<Option<Prop<bool>>>,
    finish_action: Option<FinishAction>,
    /// Filled in by the footer's build; the Enter path reuses them so focus
    /// follows the flow instead of being stranded on the dormant pane.
    next_focus: RefCell<Option<WidgetId>>,
    finish_focus: RefCell<Option<WidgetId>>,
    /// Step titles, in declaration order, for the step announcement.
    titles: Vec<LocalizedString>,
    /// The step panes, read at call time (re-captured on a rebuild).
    panel_ids: RefCell<Option<Rc<RefCell<Vec<WidgetId>>>>>,
    /// The enabled state of each pane's first control, found when the pane
    /// was built (`None`: no control). A dormant pane cannot be asked when
    /// the handler runs, and there is one pending focus request, so the
    /// choice between the pane and the primary button is made from this.
    pane_first_enabled: RefCell<Vec<Option<Signal<bool>>>>,
}

impl StepNav {
    pub(crate) fn new(
        controller: StepperController,
        validators: Vec<Option<StepValidator>>,
        completion: Vec<Option<Prop<bool>>>,
        finish_action: Option<FinishAction>,
        titles: Vec<LocalizedString>,
    ) -> Self {
        Self {
            controller,
            validators,
            completion,
            finish_action,
            next_focus: RefCell::new(None),
            finish_focus: RefCell::new(None),
            titles,
            panel_ids: RefCell::new(None),
            pane_first_enabled: RefCell::new(Vec::new()),
        }
    }

    /// Hand over the step panes and the enabled state of each one's first
    /// control.
    pub(crate) fn set_panes(
        &self,
        panel_ids: Rc<RefCell<Vec<WidgetId>>>,
        first_enabled: Vec<Option<Signal<bool>>>,
    ) {
        *self.panel_ids.borrow_mut() = Some(panel_ids);
        *self.pane_first_enabled.borrow_mut() = first_enabled;
    }

    pub(crate) fn controller(&self) -> &StepperController {
        &self.controller
    }

    pub(crate) fn set_focus_targets(&self, next: WidgetId, finish: WidgetId) {
        *self.next_focus.borrow_mut() = Some(next);
        *self.finish_focus.borrow_mut() = Some(finish);
    }

    /// The step's reactive completion gate — `true` when nothing blocks Next
    /// (a step without `complete_when` is always open).
    pub(crate) fn gate_open(&self, idx: usize) -> bool {
        self.completion
            .get(idx)
            .and_then(|c| c.as_ref())
            .map(|p| p.get())
            .unwrap_or(true)
    }

    /// Per-step completion signals, in declaration order — the footer wires
    /// them into one `flat_map` that follows the *active* step's gate.
    pub(crate) fn completion_signals(&self) -> Vec<Signal<bool>> {
        self.completion
            .iter()
            .map(|c| {
                c.as_ref()
                    .map(|p| p.as_signal())
                    .unwrap_or_else(|| Signal::new(true))
            })
            .collect()
    }

    /// Run the imperative `validate_on_next` hook; `true` when it passes or
    /// there is none.
    fn validates(&self, idx: usize) -> bool {
        match self.validators.get(idx) {
            Some(Some(v)) => v(),
            _ => true,
        }
    }

    /// After the controller moved from step `from`: put focus in the new step
    /// and say which step it is.
    ///
    /// Exactly one focus request is made. Focus goes to the new page's first
    /// control when that control is enabled; otherwise (a summary page, or a
    /// page whose controls are disabled) to the primary button, Next while a
    /// step remains and Finish on the last one. Either request waits for the
    /// layout that wakes its target, so a button the move has just hidden is
    /// never the target and focus is not left on the control that went away.
    ///
    /// The sentence "Step 2 of 3, Details" is announced afterwards: the
    /// announcer holds it out of the update that moves focus, so the reader
    /// says the field first and the step right after. Nothing happens when
    /// the step did not change.
    ///
    /// Only the first control is checked, by its enabled state as of the last
    /// layout pass. A first control hidden at the moment of the move is not
    /// noticed: the request into the page lands on the next control, or finds
    /// nothing when there is none.
    pub(crate) fn focus_step(&self, ctx: &mut EventContext, from: usize) {
        let to = self.controller.current();
        if to == from {
            return;
        }
        let pane = self
            .panel_ids
            .borrow()
            .as_ref()
            .and_then(|ids| ids.borrow().get(to).copied());
        let control_enabled = self
            .pane_first_enabled
            .borrow()
            .get(to)
            .and_then(|s| s.as_ref().map(|s| s.get()))
            .unwrap_or(false);
        match pane {
            Some(pane) if control_enabled => ctx.request_focus_into(pane),
            _ => {
                let target = if self.controller.has_next() {
                    *self.next_focus.borrow()
                } else {
                    *self.finish_focus.borrow()
                };
                if let Some(t) = target {
                    ctx.request_focus(t);
                }
            }
        }
        if let (Some(n), Some(title)) = (self.controller.visible_position(to), self.titles.get(to))
        {
            ctx.announce(
                teksilo_i18n::tr_widget!(a11y_stepper_step_announcement(
                    n = n as i64,
                    total = self.controller.visible_count() as i64,
                    title = title.resolve_now()
                ))
                .resolve_now(),
            );
        }
    }

    /// Next: gate → validator → advance. `false` when it refused to move.
    pub(crate) fn advance(&self, ctx: &mut EventContext) -> bool {
        let i = self.controller.current();
        if !self.gate_open(i) {
            return false;
        }
        if !self.validates(i) {
            self.controller.set_status(i, StepStatus::Error);
            return false;
        }
        // Clear any prior Error and mark done before advancing (`mark_active`
        // only auto-completes a step left in the Active state, not one stuck
        // in Error).
        self.controller.set_status(i, StepStatus::Complete);
        self.controller.next();
        self.focus_step(ctx, i);
        true
    }

    /// Finish: gate → validator → `on_finish`. The callback's
    /// [`FinishOutcome`] decides whether the last step lands on `Complete` or
    /// `Error`; a `Rejected` finish leaves the stepper exactly where it was.
    pub(crate) fn finish(&self, ctx: &mut EventContext) -> FinishOutcome {
        let i = self.controller.current();
        if !self.gate_open(i) {
            return FinishOutcome::Rejected;
        }
        if !self.validates(i) {
            self.controller.set_status(i, StepStatus::Error);
            return FinishOutcome::Rejected;
        }
        let outcome = match &self.finish_action {
            Some(action) => action(ctx, &self.controller),
            None => FinishOutcome::Finished,
        };
        match outcome {
            FinishOutcome::Finished => self.controller.set_status(i, StepStatus::Complete),
            FinishOutcome::Rejected => self.controller.set_status(i, StepStatus::Error),
        }
        outcome
    }

    /// Whatever the footer's primary button would do right now — Next while a
    /// reachable step remains, Finish on the last one. Drives the Enter key.
    pub(crate) fn activate_primary(&self, ctx: &mut EventContext) -> bool {
        if self.controller.has_next() {
            self.advance(ctx)
        } else {
            self.finish(ctx) == FinishOutcome::Finished
        }
    }
}

impl std::fmt::Debug for StepNav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepNav")
            .field("steps", &self.validators.len())
            .finish()
    }
}
