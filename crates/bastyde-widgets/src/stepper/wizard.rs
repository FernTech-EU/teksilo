// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`Wizard`] — a thin modal launcher around [`Stepper`].
//!
//! Renders as a button (or a custom `.trigger(...)` widget) that opens a modal
//! containing a `Stepper` built from the same [`Step`]s. The modal's Cancel and
//! a wrapped Finish both dismiss it.

use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use bastyde_core::signal::Prop;
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, lit};

use super::Stepper;
use super::controller::StepperController;
use super::step::Step;
use crate::button::{Button, ButtonVariant};
use crate::dialog::ModalContainer;
use crate::overlay_trigger::OverlayTrigger;

const DEFAULT_WIZARD_WIDTH: u32 = 640;
const DEFAULT_WIZARD_HEIGHT: u32 = 460;

type FinishAction = Rc<dyn Fn(&mut EventContext, &StepperController)>;

/// The shared, cloneable modal recipe captured by the trigger handlers.
struct WizardSpec {
    title: LocalizedString,
    steps: Vec<Step>,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    size: (u32, u32),
    non_linear: bool,
    back_label: LocalizedString,
    next_label: LocalizedString,
    finish_label: LocalizedString,
    skip_label: LocalizedString,
    cancel_label: LocalizedString,
    finish_action: Option<FinishAction>,
}

fn present_wizard(spec: &Rc<WizardSpec>, ctx: &mut EventContext) {
    if spec.steps.is_empty() {
        return;
    }
    let spec = spec.clone();
    let presentation = spec.presentation;
    let close_behavior = spec.close_behavior;
    let (w, h) = spec.size;
    let title = spec.title.resolve_now();
    ctx.present_modal(
        ModalRequest::deferred(move |tree| {
            let finish = spec.finish_action.clone();
            let stepper = Stepper::new()
                .steps(spec.steps.clone())
                .non_linear(spec.non_linear)
                .back_label(spec.back_label.clone())
                .next_label(spec.next_label.clone())
                .finish_label(spec.finish_label.clone())
                .skip_label(spec.skip_label.clone())
                .cancel(spec.cancel_label.clone(), |ctx, _ctrl| ctx.dismiss_modal())
                .on_finish(move |ctx, ctrl| {
                    if let Some(action) = &finish {
                        action(ctx, ctrl);
                    }
                    ctx.dismiss_modal();
                });
            tree.add(ModalContainer::boxed(Box::new(stepper)))
        })
        .presentation(presentation)
        .close_behavior(close_behavior)
        .title(title)
        .size(w, h),
    );
}

/// A button (or custom trigger) that opens a modal [`Stepper`].
///
/// `Wizard::new(label)` renders as a `Filled` [`Button`] whose tap opens a
/// full-screen modal containing a [`Stepper`] built from the same [`Step`]s.
/// The modal's auto-injected Cancel button and the wrapped Finish both dismiss
/// it. Override the trigger with [`trigger`](Self::trigger) to use any widget
/// instead of the default button.
pub struct Wizard {
    label: LocalizedString,
    variant: ButtonVariant,
    /// Enabled state, static or reactive; forwarded to the arena at build
    /// time.
    enabled: Prop<bool>,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    size: (u32, u32),
    non_linear: bool,
    steps: Vec<Step>,
    back_label: LocalizedString,
    next_label: LocalizedString,
    finish_label: LocalizedString,
    skip_label: LocalizedString,
    cancel_label: LocalizedString,
    finish_action: Option<FinishAction>,
    pending_trigger: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl Wizard {
    /// Create a wizard trigger button with the given label. The label is also
    /// used as the modal title.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Filled,
            enabled: Prop::Static(true),
            presentation: ModalPresentation::Auto,
            close_behavior: ModalCloseBehavior::Manual,
            size: (DEFAULT_WIZARD_WIDTH, DEFAULT_WIZARD_HEIGHT),
            non_linear: false,
            steps: Vec::new(),
            back_label: lit!("Back"),
            next_label: lit!("Next"),
            finish_label: lit!("Finish"),
            skip_label: lit!("Skip"),
            cancel_label: lit!("Cancel"),
            finish_action: None,
            pending_trigger: None,
            root_child_id: None,
        }
    }

    /// Append a single [`Step`] to the wizard.
    pub fn step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }
    /// Append multiple [`Step`]s from an iterator.
    pub fn steps(mut self, steps: impl IntoIterator<Item = Step>) -> Self {
        self.steps.extend(steps);
        self
    }
    /// Set the visual variant of the trigger button (default `Filled`).
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }
    /// Enable or disable the trigger button, statically or reactively.
    /// When disabled, tapping or pressing the trigger is a no-op.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }
    /// Allow jumping between steps by clicking their indicators (the
    /// markers become `Role::Tab`). Default: linear.
    pub fn non_linear(mut self, non_linear: bool) -> Self {
        self.non_linear = non_linear;
        self
    }
    /// Control how the modal is presented (auto, sheet, full-screen, …).
    pub fn presentation(mut self, presentation: ModalPresentation) -> Self {
        self.presentation = presentation;
        self
    }
    /// Control how the modal is dismissed (manual, click-outside, …).
    pub fn close_behavior(mut self, close_behavior: ModalCloseBehavior) -> Self {
        self.close_behavior = close_behavior;
        self
    }
    /// Set the preferred modal size in logical pixels. Default 640 × 460.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.size = (width, height);
        self
    }
    /// Override the "Back" button label inside the modal. Default: "Back".
    pub fn back_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.back_label = label.into();
        self
    }
    /// Override the "Next" button label inside the modal. Default: "Next".
    pub fn next_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.next_label = label.into();
        self
    }
    /// Override the "Finish" button label inside the modal. Default: "Finish".
    pub fn finish_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.finish_label = label.into();
        self
    }
    /// Override the "Skip" button label inside the modal. Default: "Skip".
    pub fn skip_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.skip_label = label.into();
        self
    }
    /// Override the "Cancel" button label inside the modal. Default: "Cancel".
    pub fn cancel_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.cancel_label = label.into();
        self
    }
    pub fn on_finish(
        mut self,
        action: impl Fn(&mut EventContext, &StepperController) + 'static,
    ) -> Self {
        self.finish_action = Some(Rc::new(action));
        self
    }
    pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self {
        self.pending_trigger = Some(Box::new(trigger));
        self
    }

    fn spec(&self) -> Rc<WizardSpec> {
        Rc::new(WizardSpec {
            title: self.label.clone(),
            steps: self.steps.clone(),
            presentation: self.presentation,
            close_behavior: self.close_behavior,
            size: self.size,
            non_linear: self.non_linear,
            back_label: self.back_label.clone(),
            next_label: self.next_label.clone(),
            finish_label: self.finish_label.clone(),
            skip_label: self.skip_label.clone(),
            cancel_label: self.cancel_label.clone(),
            finish_action: self.finish_action.clone(),
        })
    }
}

impl std::fmt::Debug for Wizard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wizard")
            .field("label", &self.label)
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl Widget for Wizard {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let enabled = self.enabled.clone();
        let spec = self.spec();

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            let handlers = HandlerSet::new()
                .focusable(true)
                .cursor(CursorIcon::Pointer)
                .on_tap({
                    let spec = spec.clone();
                    let enabled = enabled.clone();
                    move |_pos, ctx| {
                        if enabled.get() {
                            present_wizard(&spec, ctx);
                        }
                    }
                })
                .on_key({
                    let spec = spec.clone();
                    let enabled = enabled.clone();
                    move |event, ctx| match event {
                        WidgetEvent::KeyUp {
                            key: Key::Enter | Key::Space,
                            ..
                        } if enabled.get() => {
                            present_wizard(&spec, ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                })
                .on_access_action({
                    let spec = spec.clone();
                    let enabled = enabled.clone();
                    move |action, ctx| {
                        if action == bastyde_core::accesskit::Action::Click && enabled.get() {
                            present_wizard(&spec, ctx);
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                });
            ctx.add(
                OverlayTrigger::new(trigger, handlers)
                    .enabled(self.enabled.clone())
                    .name(self.label.clone()),
            )
        } else {
            ctx.add(
                Button::new(self.label.clone())
                    .variant(self.variant)
                    .enabled(enabled.clone())
                    .on_activate_fn(move |ctx| {
                        if enabled.get() {
                            present_wizard(&spec, ctx);
                        }
                    }),
            )
        };

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(140.0, 40.0))
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
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
