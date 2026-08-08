// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`Step`] — one page of a [`Stepper`](crate::stepper::Stepper), plus the
//! per-step [`StepStatus`] state model (Material/Ant/Flutter-style).

use std::rc::Rc;

use teksilo_core::widget::Widget;
use teksilo_i18n::LocalizedString;

/// Lifecycle state of a single step, surfaced in the indicator strip and
/// (for the active step) as `aria-current="step"`.
///
/// Mirrors the modern stepper status model (Ant `wait/process/finish/error`,
/// Flutter `StepState`): `Upcoming` = not yet reached, `Active` = currently
/// shown, `Complete` = validated, `Error` = failed validation, `Disabled` =
/// unreachable, `Optional` = reachable but skippable, `Skipped` = an optional
/// step the user bypassed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepStatus {
    #[default]
    Upcoming,
    Active,
    Complete,
    Error,
    Disabled,
    Optional,
    Skipped,
}

impl StepStatus {
    /// `true` for `Optional` — the only status that surfaces a Skip button.
    pub fn is_optional(self) -> bool {
        matches!(self, StepStatus::Optional)
    }
}

pub(crate) type StepContentFactory = Rc<dyn Fn() -> Box<dyn Widget>>;
pub(crate) type StepValidator = Rc<dyn Fn() -> bool>;

/// One page in a [`Stepper`](crate::stepper::Stepper).
///
/// A step carries a localized `title`, optional `supporting_text`, a content
/// factory (the body shown when the step is active), and an optional
/// completion gate. The recommended data-flow pattern: the application owns
/// its form state as `Signal`s, the content factory binds widgets to those
/// signals (write side), and [`complete_when`](Self::complete_when) derives
/// the Next gate from the same signals.
#[derive(Clone)]
pub struct Step {
    pub(crate) title: LocalizedString,
    pub(crate) supporting_text: Option<LocalizedString>,
    pub(crate) content_factory: Option<StepContentFactory>,
    pub(crate) initial_status: StepStatus,
    /// Reactive completion gate — when `Some`, the Next button binds its
    /// enabled state to this signal while this step is active.
    pub(crate) complete: Option<teksilo_core::signal::Prop<bool>>,
    /// Imperative fallback — checked on the Next click; if it returns
    /// `false`, navigation does not advance.
    pub(crate) validate: Option<StepValidator>,
    /// Reactive visibility gate — when `Some(false)`, the step drops out of
    /// the flow (navigation skips it, the indicator strip hides it).
    pub(crate) visible: Option<teksilo_core::signal::Prop<bool>>,
}

impl Step {
    pub fn new(title: impl Into<LocalizedString>) -> Self {
        Self {
            title: title.into(),
            supporting_text: None,
            content_factory: None,
            initial_status: StepStatus::Upcoming,
            complete: None,
            validate: None,
            visible: None,
        }
    }

    /// The body shown while this step is active. The factory may capture
    /// clones of the application's form `Signal`s to read/write step input.
    pub fn content<W, F>(mut self, factory: F) -> Self
    where
        W: Widget + 'static,
        F: Fn() -> W + 'static,
    {
        self.content_factory = Some(Rc::new(move || Box::new(factory()) as Box<dyn Widget>));
        self
    }

    /// The body shown while this step is active, as a **boxed** widget — the
    /// escape hatch for a body whose concrete type varies at runtime.
    ///
    /// [`content`](Self::content) is generic over one `W: Widget`, and
    /// `Box<dyn Widget>` does not itself implement `Widget`, so a step whose
    /// body branches on app state cannot be expressed as a single `content`
    /// factory. Box each branch instead of duplicating the surrounding
    /// builder:
    ///
    /// ```ignore
    /// Step::new(lit!("Details")).content_boxed({
    ///     let purpose = purpose.clone();
    ///     move || -> Box<dyn Widget> {
    ///         match purpose.get() {
    ///             Purpose::Novel => Box::new(novel_form()),
    ///             Purpose::Import => Box::new(import_form()),
    ///         }
    ///     }
    /// })
    /// ```
    pub fn content_boxed(mut self, factory: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.content_factory = Some(Rc::new(factory));
        self
    }

    /// A pre-boxed content factory (used by the `Wizard` bridge).
    #[allow(dead_code)]
    pub(crate) fn content_factory_rc(mut self, factory: StepContentFactory) -> Self {
        self.content_factory = Some(factory);
        self
    }

    /// Secondary line under the title in the header / indicator.
    pub fn supporting_text(mut self, text: impl Into<LocalizedString>) -> Self {
        self.supporting_text = Some(text.into());
        self
    }

    /// Set the step's initial [`StepStatus`].
    pub fn status(mut self, status: StepStatus) -> Self {
        self.initial_status = status;
        self
    }

    /// Mark the step optional (reachable but skippable — surfaces a Skip
    /// button while active). Equivalent to `.status(StepStatus::Optional)`.
    pub fn optional(mut self, optional: bool) -> Self {
        if optional {
            self.initial_status = StepStatus::Optional;
        } else if self.initial_status == StepStatus::Optional {
            self.initial_status = StepStatus::Upcoming;
        }
        self
    }

    /// Reactive Next gate: while this step is active, Next is enabled iff
    /// `signal` is `true`. Derive it from the same form signals the step's
    /// content writes — e.g. `name.map(|n| !n.is_empty())`.
    pub fn complete_when(mut self, signal: impl Into<teksilo_core::signal::Prop<bool>>) -> Self {
        self.complete = Some(signal.into());
        self
    }

    /// Imperative validation fallback: checked on the Next click. Returning
    /// `false` blocks navigation. Prefer [`complete_when`](Self::complete_when)
    /// where a reactive signal is available.
    pub fn validate_on_next(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.validate = Some(Rc::new(f));
        self
    }

    /// Reactive visibility: while `visible` is `false` this step drops out of
    /// the flow — Next / Back / indicator clicks skip it, and its marker is
    /// hidden from the indicator strip (and from AT).
    ///
    /// This is how a **branching** wizard is expressed: declare every step
    /// once and gate the conditional ones on the choice that selects them,
    /// instead of maintaining one step list per branch.
    ///
    /// ```ignore
    /// let purpose = Signal::new(Purpose::Novel);
    /// Stepper::new()
    ///     .step(Step::new(lit!("Purpose")).content(|| purpose_picker()))
    ///     .step(Step::new(lit!("Import source"))
    ///         .visible_when(purpose.map(|p| *p == Purpose::Import))
    ///         .content(|| import_form()))
    /// ```
    ///
    /// Hiding the step the user is *currently on* does not navigate away from
    /// it — gate steps ahead of the choice, not the one making it.
    pub fn visible_when(mut self, visible: impl Into<teksilo_core::signal::Prop<bool>>) -> Self {
        self.visible = Some(visible.into());
        self
    }
}

impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Step")
            .field("title", &self.title)
            .field("supporting_text", &self.supporting_text)
            .field("initial_status", &self.initial_status)
            .field("has_content", &self.content_factory.is_some())
            .field("has_complete_gate", &self.complete.is_some())
            .finish()
    }
}
