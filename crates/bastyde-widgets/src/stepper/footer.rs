//! [`StepperFooter`] — the navigation bar: Back / Skip / Help / Next / Finish
//! (+ optional Cancel). Next is gated reactively on the active step's
//! completion signal via [`Signal::flat_map`], and runs any imperative
//! `validate_on_next` before advancing.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::LocalizedString;

use super::controller::StepperController;
use super::step::{StepStatus, StepValidator};
use crate::button::{Button, ButtonVariant};
use crate::primitives::{HStack, Spacer};

type FooterAction = Rc<dyn Fn(&mut EventContext, &StepperController)>;

pub(crate) struct FooterStep {
    pub initial_status: StepStatus,
    pub complete: Option<Signal<bool>>,
    pub validate: Option<StepValidator>,
}

pub(crate) struct StepperFooter {
    controller: StepperController,
    steps: Vec<FooterStep>,
    back_label: LocalizedString,
    next_label: LocalizedString,
    finish_label: LocalizedString,
    skip_label: LocalizedString,
    help_label: Option<LocalizedString>,
    cancel_label: Option<LocalizedString>,
    finish_action: Option<FooterAction>,
    help_action: Option<FooterAction>,
    cancel_action: Option<FooterAction>,
    root_child_id: Option<WidgetId>,
}

impl StepperFooter {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        controller: StepperController,
        steps: Vec<FooterStep>,
        back_label: LocalizedString,
        next_label: LocalizedString,
        finish_label: LocalizedString,
        skip_label: LocalizedString,
        help_label: Option<LocalizedString>,
        cancel_label: Option<LocalizedString>,
        finish_action: Option<FooterAction>,
        help_action: Option<FooterAction>,
        cancel_action: Option<FooterAction>,
    ) -> Self {
        Self {
            controller,
            steps,
            back_label,
            next_label,
            finish_label,
            skip_label,
            help_label,
            cancel_label,
            finish_action,
            help_action,
            cancel_action,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for StepperFooter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepperFooter")
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl Widget for StepperFooter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if self.steps.is_empty() {
            return Vec::new();
        }
        let total = self.steps.len();
        let current = self.controller.current_step_signal();
        let version = self.controller.version_signal();

        // Reactive Next gate: follow the *active* step's completion signal.
        let completion: Vec<Signal<bool>> = self
            .steps
            .iter()
            .map(|s| s.complete.clone().unwrap_or_else(|| Signal::new(true)))
            .collect();
        let next_enabled = current.flat_map(move |i| {
            completion
                .get(*i)
                .cloned()
                .unwrap_or_else(|| Signal::new(true))
        });

        // Imperative validators, indexed by step.
        let validators: Rc<Vec<Option<StepValidator>>> =
            Rc::new(self.steps.iter().map(|s| s.validate.clone()).collect());

        let next_focus: Rc<RefCell<Option<WidgetId>>> = Rc::new(RefCell::new(None));
        let finish_focus: Rc<RefCell<Option<WidgetId>>> = Rc::new(RefCell::new(None));

        // ── Back ──────────────────────────────────────────────────────────
        let back_id = ctx.add(
            Button::new(self.back_label.clone())
                .variant(ButtonVariant::Plain)
                .on_activate_fn({
                    let controller = self.controller.clone();
                    let next_focus = next_focus.clone();
                    move |ctx| {
                        controller.back();
                        if let Some(t) = *next_focus.borrow() {
                            ctx.request_focus(t);
                        }
                    }
                }),
        );

        // ── Next ──────────────────────────────────────────────────────────
        let next_id = ctx.add(
            Button::new(self.next_label.clone())
                .variant(ButtonVariant::Filled)
                .on_activate_fn({
                    let controller = self.controller.clone();
                    let validators = validators.clone();
                    let finish_focus = finish_focus.clone();
                    let next_focus = next_focus.clone();
                    move |ctx| {
                        let i = controller.current();
                        if let Some(Some(v)) = validators.get(i) {
                            if !v() {
                                controller.set_status(i, StepStatus::Error);
                                return;
                            }
                        }
                        // Clear any prior Error and mark done before advancing
                        // (mark_active only auto-completes a step left in the
                        // Active state, not one stuck in Error).
                        controller.set_status(i, StepStatus::Complete);
                        controller.next();
                        // Move focus forward to the button that is now shown.
                        let focus = if controller.current() + 1 >= total {
                            *finish_focus.borrow()
                        } else {
                            *next_focus.borrow()
                        };
                        if let Some(t) = focus {
                            ctx.request_focus(t);
                        }
                    }
                }),
        );

        // ── Finish ────────────────────────────────────────────────────────
        let finish_id = ctx.add(
            Button::new(self.finish_label.clone())
                .variant(ButtonVariant::Filled)
                .on_activate_fn({
                    let controller = self.controller.clone();
                    let validators = validators.clone();
                    let finish_action = self.finish_action.clone();
                    move |ctx| {
                        let i = controller.current();
                        if let Some(Some(v)) = validators.get(i) {
                            if !v() {
                                controller.set_status(i, StepStatus::Error);
                                return;
                            }
                        }
                        controller.set_status(i, StepStatus::Complete);
                        if let Some(action) = &finish_action {
                            action(ctx, &controller);
                        }
                    }
                }),
        );

        // ── Skip (optional steps only) ─────────────────────────────────────
        let skip_id = ctx.add(
            Button::new(self.skip_label.clone())
                .variant(ButtonVariant::Ghost)
                .on_activate_fn({
                    let controller = self.controller.clone();
                    let next_focus = next_focus.clone();
                    move |ctx| {
                        controller.skip();
                        if let Some(t) = *next_focus.borrow() {
                            ctx.request_focus(t);
                        }
                    }
                }),
        );

        *next_focus.borrow_mut() = Some(next_id);
        *finish_focus.borrow_mut() = Some(finish_id);

        // Visibility / enablement.
        ctx.visible_when(back_id, {
            let c = self.controller.clone();
            version.zip(&current).map(move |_| c.can_back())
        });
        ctx.visible_when(next_id, current.map(move |i| *i + 1 < total));
        ctx.visible_when(finish_id, current.map(move |i| *i + 1 >= total));
        ctx.enabled_when(next_id, next_enabled.clone());
        ctx.enabled_when(finish_id, next_enabled);
        let optional_flags: Vec<bool> = self
            .steps
            .iter()
            .map(|s| s.initial_status == StepStatus::Optional)
            .collect();
        ctx.visible_when(
            skip_id,
            current.map(move |i| optional_flags.get(*i).copied().unwrap_or(false)),
        );

        // ── Optional Help / Cancel ─────────────────────────────────────────
        let help_id = self.help_label.clone().map(|label| {
            ctx.add(
                Button::new(label)
                    .variant(ButtonVariant::Plain)
                    .on_activate_fn({
                        let controller = self.controller.clone();
                        let help_action = self.help_action.clone();
                        move |ctx| {
                            if let Some(action) = &help_action {
                                action(ctx, &controller);
                            }
                        }
                    }),
            )
        });
        let cancel_id = self.cancel_label.clone().map(|label| {
            ctx.add(
                Button::new(label)
                    .variant(ButtonVariant::Ghost)
                    .on_activate_fn({
                        let controller = self.controller.clone();
                        let cancel_action = self.cancel_action.clone();
                        move |ctx| {
                            if let Some(action) = &cancel_action {
                                action(ctx, &controller);
                            }
                        }
                    }),
            )
        });

        // Layout: [Back] ──spacer── [Help] [Cancel] [Skip] [Next] [Finish]
        let mut row = HStack::new().spacing(12.0).add_child(back_id).child(Spacer::new());
        if let Some(id) = help_id {
            row = row.add_child(id);
        }
        if let Some(id) = cancel_id {
            row = row.add_child(id);
        }
        row = row.add_child(skip_id).add_child(next_id).add_child(finish_id);

        let root = ctx.add(row);
        self.root_child_id = Some(root);
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
        builder.set_role(bastyde_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
