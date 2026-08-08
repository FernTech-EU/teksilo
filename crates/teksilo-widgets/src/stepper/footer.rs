// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`StepperFooter`] — the navigation bar: Back / Skip / Help / Next / Finish
//! (+ optional Cancel). Next is gated reactively on the active step's
//! completion signal via [`Signal::flat_map`]; the Next / Finish semantics
//! themselves live in [`StepNav`] so the Enter key runs the same code path.

use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;

use super::controller::StepperController;
use super::nav::StepNav;
use crate::button::{Button, ButtonVariant};
use crate::primitives::{HStack, Spacer};

pub(crate) type FooterAction = Rc<dyn Fn(&mut EventContext, &StepperController)>;

pub(crate) struct StepperFooter {
    nav: Rc<StepNav>,
    /// Per-step `Optional` flags — the only step data the footer still needs
    /// directly (the Skip button's visibility).
    optional_flags: Vec<bool>,
    back_label: LocalizedString,
    next_label: LocalizedString,
    finish_label: LocalizedString,
    skip_label: LocalizedString,
    help_label: Option<LocalizedString>,
    cancel_label: Option<LocalizedString>,
    help_action: Option<FooterAction>,
    cancel_action: Option<FooterAction>,
    root_child_id: Option<WidgetId>,
}

impl StepperFooter {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        nav: Rc<StepNav>,
        optional_flags: Vec<bool>,
        back_label: LocalizedString,
        next_label: LocalizedString,
        finish_label: LocalizedString,
        skip_label: LocalizedString,
        help_label: Option<LocalizedString>,
        cancel_label: Option<LocalizedString>,
        help_action: Option<FooterAction>,
        cancel_action: Option<FooterAction>,
    ) -> Self {
        Self {
            nav,
            optional_flags,
            back_label,
            next_label,
            finish_label,
            skip_label,
            help_label,
            cancel_label,
            help_action,
            cancel_action,
            root_child_id: None,
        }
    }

    fn controller(&self) -> StepperController {
        self.nav.controller().clone()
    }
}

impl std::fmt::Debug for StepperFooter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepperFooter")
            .field("steps", &self.optional_flags.len())
            .finish()
    }
}

impl Widget for StepperFooter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if self.optional_flags.is_empty() {
            return Vec::new();
        }
        let controller = self.controller();
        let current = controller.current_step_signal();
        let version = controller.version_signal();

        // Reactive Next gate: follow the *active* step's completion signal.
        let completion = self.nav.completion_signals();
        let next_enabled = current.flat_map(move |i| {
            completion
                .get(*i)
                .cloned()
                .unwrap_or_else(|| Signal::new(true))
        });

        // ── Back ──────────────────────────────────────────────────────────
        let back_id = ctx.add(
            Button::new(self.back_label.clone())
                .variant(ButtonVariant::Plain)
                .on_activate_fn({
                    let nav = self.nav.clone();
                    move |ctx| {
                        nav.controller().back();
                        nav.focus_primary(ctx);
                    }
                }),
        );

        // ── Next ──────────────────────────────────────────────────────────
        let next_id = ctx.add(
            Button::new(self.next_label.clone())
                .variant(ButtonVariant::Filled)
                .on_activate_fn({
                    let nav = self.nav.clone();
                    move |ctx| {
                        nav.advance(ctx);
                    }
                }),
        );

        // ── Finish ────────────────────────────────────────────────────────
        let finish_id = ctx.add(
            Button::new(self.finish_label.clone())
                .variant(ButtonVariant::Filled)
                .on_activate_fn({
                    let nav = self.nav.clone();
                    move |ctx| {
                        nav.finish(ctx);
                    }
                }),
        );

        // ── Skip (optional steps only) ─────────────────────────────────────
        let skip_id = ctx.add(
            Button::new(self.skip_label.clone())
                .variant(ButtonVariant::Ghost)
                .on_activate_fn({
                    let nav = self.nav.clone();
                    move |ctx| {
                        nav.controller().skip();
                        nav.focus_primary(ctx);
                    }
                }),
        );

        self.nav.set_focus_targets(next_id, finish_id);

        // Visibility / enablement. Next vs Finish follows *reachability*, not
        // `index + 1 == len`: a trailing run of hidden / disabled steps means
        // the active step is the last one, so it must show Finish.
        ctx.visible_when(back_id, {
            let c = controller.clone();
            version.zip(&current).map(move |_| c.can_back())
        });
        ctx.visible_when(next_id, {
            let c = controller.clone();
            version.zip(&current).map(move |_| c.has_next())
        });
        ctx.visible_when(finish_id, {
            let c = controller.clone();
            version.zip(&current).map(move |_| !c.has_next())
        });
        ctx.enabled_when(next_id, next_enabled.clone());
        ctx.enabled_when(finish_id, next_enabled);
        let optional_flags = self.optional_flags.clone();
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
                        let controller = controller.clone();
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
                        let controller = controller.clone();
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
        let mut row = HStack::new()
            .spacing(12.0)
            .add_child(back_id)
            .child(Spacer::new());
        if let Some(id) = help_id {
            row = row.add_child(id);
        }
        if let Some(id) = cancel_id {
            row = row.add_child(id);
        }
        row = row
            .add_child(skip_id)
            .add_child(next_id)
            .add_child(finish_id);

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
        builder.set_role(teksilo_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
