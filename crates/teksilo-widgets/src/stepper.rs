// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`Stepper`] — a modern, embeddable step-flow widget (Material/Ant/Flutter
//! "stepper"), and [`Wizard`], a thin modal launcher built on it.
//!
//! A stepper shows a **visible step-indicator strip** above (or beside) a
//! content area driven by a [`Switcher`], with a
//! footer of Back / Skip / Help / Next / Finish controls. It supports linear
//! and **non-linear** (clickable) navigation, optional + skippable steps, per
//! step validation gating, a generic chrome slot, and a
//! [`StepperController`] handle for programmatic reset / jump / introspection.
//!
//! # Data flow
//!
//! The application owns its form state as `Signal`s. A step's content factory
//! captures clones of those signals (write side); [`Step::complete_when`]
//! derives the Next gate from the same signals; and
//! [`Stepper::on_finish`] reads them back — plus the [`StepperController`] for
//! per-step introspection (`visited` / `skipped`) — to branch on the choices
//! made. There is no `QVariant` field registry: plain shared signals are the
//! cross-step channel.
//!
//! ```ignore
//! #[derive(Clone)]
//! struct Form { name: Signal<String>, plan: Signal<Plan> }
//! let form = Form { name: Signal::new(String::new()), plan: Signal::new(Plan::Free) };
//!
//! Stepper::new()
//!     .step(Step::new(lit!("Account"))
//!         .content({ let f = form.clone(); move || TextInput::new().text(f.name.clone()) })
//!         .complete_when(form.name.map(|n| !n.is_empty())))
//!     .step(Step::new(lit!("Plan"))
//!         .content({ let f = form.clone(); move || plan_picker(f.plan.clone()) }))
//!     .on_finish({ let f = form.clone(); move |_ctx, ctrl| {
//!         match f.plan.get() { Plan::Free => {/* … */} Plan::Pro => {/* … */} }
//!         let _ = ctrl.skipped(1);
//!     }});
//! ```

mod content_pane;
mod controller;
mod footer;
mod indicator;
mod indicator_strip;
mod step;
mod wizard;

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LocalizedString, lit};

use crate::primitives::{Divider, Expand, HStack, Switcher, VStack};

pub use controller::StepperController;
pub use step::{Step, StepStatus};
pub use wizard::Wizard;

use content_pane::StepPane;
use footer::{FooterStep, StepperFooter};
use indicator::DEFAULT_CIRCLE_SIZE;
use indicator_strip::{IndicatorStrip, StepMeta};

/// Indicator-strip orientation for a [`Stepper`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepperOrientation {
    /// Markers in a row, content below (default).
    #[default]
    Horizontal,
    /// Markers in a column on the leading side, content beside.
    Vertical,
}

/// Where the optional chrome slot (banner / sidebar) sits relative to the
/// stepper body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromePosition {
    /// Leading column (left in LTR). Forced to `Top` in vertical orientation.
    #[default]
    Leading,
    /// Banner above the stepper body.
    Top,
}

type StepperFinish = Rc<dyn Fn(&mut EventContext, &StepperController)>;

/// An embeddable multi-step flow widget. See the [module docs](self) for the
/// data-flow pattern and a usage example.
pub struct Stepper {
    steps: Vec<Step>,
    controller: Option<StepperController>,
    orientation: StepperOrientation,
    non_linear: bool,
    circle_size: f32,
    chrome: Option<Box<dyn Widget>>,
    chrome_position: ChromePosition,
    back_label: LocalizedString,
    next_label: LocalizedString,
    finish_label: LocalizedString,
    skip_label: LocalizedString,
    help_label: Option<LocalizedString>,
    help_action: Option<StepperFinish>,
    cancel_label: Option<LocalizedString>,
    cancel_action: Option<StepperFinish>,
    finish_action: Option<StepperFinish>,
    root_child_id: Option<WidgetId>,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl Default for Stepper {
    fn default() -> Self {
        Self::new()
    }
}

impl Stepper {
    /// Create an empty `Stepper`. Append steps with [`step`](Self::step) or
    /// [`steps`](Self::steps) and provide a finish callback with
    /// [`on_finish`](Self::on_finish).
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            controller: None,
            orientation: StepperOrientation::Horizontal,
            non_linear: false,
            circle_size: DEFAULT_CIRCLE_SIZE,
            chrome: None,
            chrome_position: ChromePosition::Leading,
            back_label: lit!("Back"),
            next_label: lit!("Next"),
            finish_label: lit!("Finish"),
            skip_label: lit!("Skip"),
            help_label: None,
            help_action: None,
            cancel_label: None,
            cancel_action: None,
            finish_action: None,
            root_child_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Append a single [`Step`] definition.
    pub fn step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }

    /// Append multiple [`Step`] definitions from an iterator.
    pub fn steps(mut self, steps: impl IntoIterator<Item = Step>) -> Self {
        self.steps.extend(steps);
        self
    }

    /// Drive the stepper with an externally-held controller (for programmatic
    /// reset / jump / introspection). If omitted, the stepper creates its own.
    pub fn controller(mut self, controller: StepperController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Set the indicator-strip orientation (horizontal or vertical).
    pub fn orientation(mut self, orientation: StepperOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Shorthand for `.orientation(StepperOrientation::Vertical)`.
    pub fn vertical(mut self) -> Self {
        self.orientation = StepperOrientation::Vertical;
        self
    }

    /// Allow jumping between steps by clicking their indicators (the markers
    /// become `Role::Tab`). Linear (default) markers are `Role::ListItem`.
    pub fn non_linear(mut self, non_linear: bool) -> Self {
        self.non_linear = non_linear;
        self
    }

    /// Override the marker circle diameter (logical px).
    pub fn circle_size(mut self, size: f32) -> Self {
        self.circle_size = size;
        self
    }

    /// A generic chrome widget (banner / sidebar) — the modern replacement for
    /// QWizard's watermark pixmap.
    pub fn chrome(mut self, chrome: impl Widget + 'static) -> Self {
        self.chrome = Some(Box::new(chrome));
        self
    }

    /// Choose where the optional chrome widget sits relative to the stepper
    /// body. Forced to [`ChromePosition::Top`] when
    /// [`orientation`](Self::orientation) is `Vertical`.
    pub fn chrome_position(mut self, position: ChromePosition) -> Self {
        self.chrome_position = position;
        self
    }

    /// Override the "Back" button label. Default: "Back".
    pub fn back_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.back_label = label.into();
        self
    }
    /// Override the "Next" button label. Default: "Next".
    pub fn next_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.next_label = label.into();
        self
    }
    /// Override the "Finish" button label. Default: "Finish".
    pub fn finish_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.finish_label = label.into();
        self
    }
    /// Override the "Skip" button label. Default: "Skip".
    pub fn skip_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.skip_label = label.into();
        self
    }

    /// Add a Help button + callback to the footer.
    pub fn help(
        mut self,
        label: impl Into<LocalizedString>,
        action: impl Fn(&mut EventContext, &StepperController) + 'static,
    ) -> Self {
        self.help_label = Some(label.into());
        self.help_action = Some(Rc::new(action));
        self
    }

    /// Add a Cancel button + callback to the footer.
    pub fn cancel(
        mut self,
        label: impl Into<LocalizedString>,
        action: impl Fn(&mut EventContext, &StepperController) + 'static,
    ) -> Self {
        self.cancel_label = Some(label.into());
        self.cancel_action = Some(Rc::new(action));
        self
    }

    /// Called when Finish is activated on the last step. Receives the event
    /// context and the controller (for `skipped` / `visited` introspection);
    /// read collected values from the form signals your steps wrote.
    pub fn on_finish(
        mut self,
        action: impl Fn(&mut EventContext, &StepperController) + 'static,
    ) -> Self {
        self.finish_action = Some(Rc::new(action));
        self
    }

    /// Attach a plain single-line tooltip to this stepper. Clears any
    /// previously set rich or composite tooltip.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip identified by a registry key. Clears any
    /// previously set plain or composite tooltip.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip with inline content. Clears any previously set
    /// plain or composite tooltip.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip (arbitrary widget body). Clears any
    /// previously set plain or rich tooltip.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl std::fmt::Debug for Stepper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stepper")
            .field("steps", &self.steps.len())
            .field("orientation", &self.orientation)
            .field("non_linear", &self.non_linear)
            .finish()
    }
}

impl Widget for Stepper {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if self.steps.is_empty() {
            self.root_child_id = None;
            return Vec::new();
        }

        // Reuse a persisted controller across rebuilds so navigation state
        // survives an ancestor-triggered rebuild. `seed_statuses` is
        // idempotent, so re-seeding here is harmless.
        let controller = self
            .controller
            .get_or_insert_with(|| StepperController::new(self.steps.len()))
            .clone();
        controller.seed_statuses(self.steps.iter().map(|s| s.initial_status).collect());

        let panel_ids: Rc<RefCell<Vec<WidgetId>>> = Rc::new(RefCell::new(Vec::new()));
        let indicator_ids: Rc<RefCell<Vec<WidgetId>>> = Rc::new(RefCell::new(Vec::new()));

        // Pre-mount every step pane so `panel_ids` is complete on the first
        // build (required for the indicators' `controls` and the panes'
        // `labelled_by`). The content factory runs eagerly here.
        let mut switcher = Switcher::new(controller.current_step_signal())
            .capture_child_ids_into(panel_ids.clone());
        for step in &self.steps {
            let factory = step.content_factory.as_ref().unwrap_or_else(|| {
                panic!(
                    "Step \"{}\" requires .content(...) — no content factory was set",
                    step.title.resolve_now()
                )
            });
            let pane = StepPane::new(
                step.title.clone(),
                factory(),
                panel_ids.clone(),
                indicator_ids.clone(),
            );
            let pane_id = ctx.add(pane);
            switcher = switcher.child_id(pane_id);
        }
        let switcher_id = ctx.add(switcher);

        let metas: Vec<StepMeta> = self
            .steps
            .iter()
            .map(|s| StepMeta {
                title: s.title.clone(),
                supporting_text: s.supporting_text.clone(),
            })
            .collect();
        let strip_id = ctx.add(IndicatorStrip::new(
            metas,
            controller.clone(),
            self.orientation,
            self.non_linear,
            self.circle_size,
            indicator_ids.clone(),
            panel_ids.clone(),
        ));

        let footer_steps: Vec<FooterStep> = self
            .steps
            .iter()
            .map(|s| FooterStep {
                initial_status: s.initial_status,
                complete: s.complete.clone(),
                validate: s.validate.clone(),
            })
            .collect();
        let footer_id = ctx.add(StepperFooter::new(
            controller.clone(),
            footer_steps,
            self.back_label.clone(),
            self.next_label.clone(),
            self.finish_label.clone(),
            self.skip_label.clone(),
            self.help_label.clone(),
            self.cancel_label.clone(),
            self.finish_action.clone(),
            self.help_action.clone(),
            self.cancel_action.clone(),
        ));

        let content = ctx.add(Expand::new().child_id(switcher_id));

        let body = match self.orientation {
            StepperOrientation::Horizontal => ctx.add(
                VStack::new()
                    .spacing(12.0)
                    .add_child(strip_id)
                    .child(Divider::new())
                    .add_child(content)
                    .child(Divider::new())
                    .add_child(footer_id),
            ),
            StepperOrientation::Vertical => {
                let right = ctx.add(
                    VStack::new()
                        .spacing(12.0)
                        .add_child(content)
                        .child(Divider::new())
                        .add_child(footer_id),
                );
                ctx.add(
                    HStack::new()
                        .spacing(20.0)
                        .add_child(strip_id)
                        .child(Expand::new().child_id(right)),
                )
            }
        };

        // Chrome slot. Vertical orientation forces the banner on top to avoid a
        // cramped three-column layout.
        let root = if let Some(chrome) = self.chrome.take() {
            let chrome_id = ctx.add_boxed(chrome);
            let on_top = matches!(self.chrome_position, ChromePosition::Top)
                || matches!(self.orientation, StepperOrientation::Vertical);
            if on_top {
                ctx.add(
                    VStack::new()
                        .spacing(12.0)
                        .add_child(chrome_id)
                        .child(Expand::new().child_id(body)),
                )
            } else {
                ctx.add(
                    HStack::new()
                        .spacing(16.0)
                        .add_child(chrome_id)
                        .child(Expand::new().child_id(body)),
                )
            }
        } else {
            body
        };

        self.root_child_id = Some(root);

        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root, tooltip_id, delay);
        }

        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Group);
        builder.set_name(teksilo_i18n::tr_widget!(a11y_stepper_content_name()).resolve_now());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
