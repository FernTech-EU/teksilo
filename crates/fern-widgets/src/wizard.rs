use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::button::{Button, ButtonVariant};
use crate::dialog::ModalContainer;
use crate::overlay_trigger::OverlayTrigger;
use crate::primitives::{Divider, HStack, Spacer, Switcher, TextWidget, VStack};
use fern_tokens::{TextRole, TextStyleRole};

const DEFAULT_WIZARD_WIDTH: u32 = 640;
const DEFAULT_WIZARD_HEIGHT: u32 = 420;
const DEFAULT_BACK_LABEL: &str = "Back";
const DEFAULT_CANCEL_LABEL: &str = "Cancel";
const DEFAULT_NEXT_LABEL: &str = "Next";
const DEFAULT_FINISH_LABEL: &str = "Finish";

type WizardStepFactory = Rc<dyn Fn() -> Box<dyn Widget>>;
type WizardAction = Rc<dyn Fn(&mut EventContext)>;

#[derive(Clone)]
struct WizardStepInfo {
    title: String,
    supporting_text: Option<String>,
}

#[derive(Clone)]
pub struct WizardStep {
    title: String,
    supporting_text: Option<String>,
    content_factory: Option<WizardStepFactory>,
}

impl WizardStep {
    pub fn new(title: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = title.into();
        Self {
            title: ls.resolve_now(),
            supporting_text: None,
            content_factory: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw title in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(title: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(title))
    }

    pub fn content<W, F>(mut self, factory: F) -> Self
    where
        W: Widget + 'static,
        F: Fn() -> W + 'static,
    {
        self.content_factory = Some(Rc::new(move || Box::new(factory()) as Box<dyn Widget>));
        self
    }

    pub fn supporting_text(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.supporting_text = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `supporting_text(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn supporting_text_literal(mut self, text: impl Into<String>) -> Self {
        self.supporting_text = Some(text.into());
        self
    }

    fn info(&self) -> WizardStepInfo {
        WizardStepInfo {
            title: self.title.clone(),
            supporting_text: self.supporting_text.clone(),
        }
    }
}

impl std::fmt::Debug for WizardStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WizardStep")
            .field("title", &self.title)
            .field("supporting_text", &self.supporting_text)
            .finish()
    }
}

fn queue_wizard_request(
    ctx: &mut EventContext,
    label: &str,
    steps: &[WizardStep],
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    size: (u32, u32),
    back_label: &str,
    cancel_label: &str,
    next_label: &str,
    finish_label: &str,
    finish_action: Option<&WizardAction>,
) {
    if steps.is_empty() {
        return;
    }

    let steps = steps.to_vec();
    let back_label = back_label.to_string();
    let cancel_label = cancel_label.to_string();
    let next_label = next_label.to_string();
    let finish_label = finish_label.to_string();
    let finish_action = finish_action.cloned();

    ctx.present_modal(
        ModalRequest::deferred(move |tree| {
            tree.add(ModalContainer::boxed(Box::new(WizardFlow::new(
                steps,
                back_label,
                cancel_label,
                next_label,
                finish_label,
                finish_action,
            ))))
        })
        .presentation(presentation)
        .close_behavior(close_behavior)
        .title(label)
        .size(size.0, size.1),
    );
}

struct WizardHeader {
    steps: Rc<Vec<WizardStepInfo>>,
    current_step: Signal<usize>,
    root_child_id: Option<WidgetId>,
}

impl WizardHeader {
    fn new(steps: Rc<Vec<WizardStepInfo>>, current_step: Signal<usize>) -> Self {
        Self {
            steps,
            current_step,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for WizardHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WizardHeader")
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl Widget for WizardHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let steps = self.steps.clone();
        let current_step = self.current_step.clone();

        let progress = current_step.map({
            let steps = steps.clone();
            move |index| format!("Step {} of {}", *index + 1, steps.len())
        });
        let title = current_step.map({
            let steps = steps.clone();
            move |index| {
                steps
                    .get(*index)
                    .map(|step| step.title.clone())
                    .unwrap_or_default()
            }
        });
        let supporting_text = current_step.map({
            let steps = steps.clone();
            move |index| {
                steps
                    .get(*index)
                    .and_then(|step| step.supporting_text.clone())
                    .unwrap_or_default()
            }
        });
        let show_supporting = current_step.map({
            let steps = steps.clone();
            move |index| {
                steps
                    .get(*index)
                    .and_then(|step| step.supporting_text.as_ref())
                    .is_some_and(|text| !text.is_empty())
            }
        });

        let progress_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(progress)
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary)
                .single_line(),
        );
        let title_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(title)
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary)
                .single_line(),
        );
        // `supporting_text` wraps naturally — it's the caller's
        // explanatory paragraph.
        let supporting_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(supporting_text)
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary),
        );
        ctx.visible_when(supporting_id, show_supporting);

        let root = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(progress_id)
                .add_child(title_id)
                .add_child(supporting_id),
        );
        self.root_child_id = Some(root);

        let self_id = ctx.self_id();
        self.current_step.bind_to(
            self_id,
            ctx.binding_registry(),
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
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
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
        builder.set_name(fern_i18n::tr_widget!(a11y_wizard_progress_name()).resolve_now());
        let total = self.steps.len();
        if total > 0 {
            let current = self.current_step.get().min(total.saturating_sub(1));
            builder.inner_mut().set_position_in_set(current + 1);
            builder.inner_mut().set_size_of_set(total);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

struct WizardFooter {
    current_step: Signal<usize>,
    total_steps: usize,
    back_label: String,
    cancel_label: String,
    next_label: String,
    finish_label: String,
    finish_action: Option<WizardAction>,
    root_child_id: Option<WidgetId>,
    back_button_id: Option<WidgetId>,
    next_button_id: Option<WidgetId>,
    finish_button_id: Option<WidgetId>,
}

impl WizardFooter {
    fn new(
        current_step: Signal<usize>,
        total_steps: usize,
        back_label: String,
        cancel_label: String,
        next_label: String,
        finish_label: String,
        finish_action: Option<WizardAction>,
    ) -> Self {
        Self {
            current_step,
            total_steps,
            back_label,
            cancel_label,
            next_label,
            finish_label,
            finish_action,
            root_child_id: None,
            back_button_id: None,
            next_button_id: None,
            finish_button_id: None,
        }
    }
}

impl std::fmt::Debug for WizardFooter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WizardFooter")
            .field("total_steps", &self.total_steps)
            .finish()
    }
}

impl Widget for WizardFooter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if self.total_steps == 0 {
            return Vec::new();
        }

        if self.root_child_id.is_none() {
            let current_step = self.current_step.clone();
            let total_steps = self.total_steps;
            let next_focus_id = Rc::new(RefCell::new(None));
            let finish_focus_id = Rc::new(RefCell::new(None));

            let back_id = ctx.add(
                Button::new_literal(self.back_label.clone())
                    .variant(ButtonVariant::Plain)
                    .on_activate_fn({
                        let current_step = current_step.clone();
                        let next_focus_id = next_focus_id.clone();
                        move |ctx| {
                            let index = current_step.get();
                            if index == 0 {
                                return;
                            }
                            current_step.set(index - 1);
                            if let Some(target) = *next_focus_id.borrow() {
                                ctx.request_focus(target);
                            }
                        }
                    }),
            );

            let cancel_id = ctx.add(
                Button::new_literal(self.cancel_label.clone())
                    .variant(ButtonVariant::Ghost)
                    .on_activate_fn(|ctx| ctx.dismiss_modal()),
            );

            let next_id = ctx.add(
                Button::new_literal(self.next_label.clone())
                    .variant(ButtonVariant::Filled)
                    .on_activate_fn({
                        let current_step = current_step.clone();
                        let next_focus_id = next_focus_id.clone();
                        let finish_focus_id = finish_focus_id.clone();
                        move |ctx| {
                            let index = current_step.get();
                            if index + 1 >= total_steps {
                                return;
                            }
                            let next_index = index + 1;
                            current_step.set(next_index);
                            let focus_target = if next_index + 1 >= total_steps {
                                *finish_focus_id.borrow()
                            } else {
                                *next_focus_id.borrow()
                            };
                            if let Some(target) = focus_target {
                                ctx.request_focus(target);
                            }
                        }
                    }),
            );

            let finish_action = self.finish_action.clone();
            let finish_id = ctx.add(
                Button::new_literal(self.finish_label.clone())
                    .variant(ButtonVariant::Filled)
                    .on_activate_fn(move |ctx| {
                        if let Some(action) = &finish_action {
                            action(ctx);
                        }
                        ctx.dismiss_modal();
                    }),
            );

            *next_focus_id.borrow_mut() = Some(next_id);
            *finish_focus_id.borrow_mut() = Some(finish_id);

            ctx.visible_when(back_id, current_step.map(move |index| *index > 0));
            ctx.visible_when(
                next_id,
                current_step.map(move |index| *index + 1 < total_steps),
            );
            ctx.visible_when(
                finish_id,
                current_step.map(move |index| *index + 1 >= total_steps),
            );

            let root = ctx.add(
                HStack::new()
                    .spacing(12.0)
                    .add_child(back_id)
                    .child(Spacer::new())
                    .add_child(cancel_id)
                    .add_child(next_id)
                    .add_child(finish_id),
            );

            self.back_button_id = Some(back_id);
            self.next_button_id = Some(next_id);
            self.finish_button_id = Some(finish_id);
            self.root_child_id = Some(root);
        }

        self.root_child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
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
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

struct WizardFlow {
    steps: Vec<WizardStep>,
    current_step: Signal<usize>,
    back_label: String,
    cancel_label: String,
    next_label: String,
    finish_label: String,
    finish_action: Option<WizardAction>,
    root_child_id: Option<WidgetId>,
}

impl WizardFlow {
    fn new(
        steps: Vec<WizardStep>,
        back_label: String,
        cancel_label: String,
        next_label: String,
        finish_label: String,
        finish_action: Option<WizardAction>,
    ) -> Self {
        Self {
            steps,
            current_step: Signal::new(0),
            back_label,
            cancel_label,
            next_label,
            finish_label,
            finish_action,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for WizardFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WizardFlow")
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl Widget for WizardFlow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let step_info = Rc::new(self.steps.iter().map(WizardStep::info).collect::<Vec<_>>());

        let header_id = ctx.add(WizardHeader::new(step_info, self.current_step.clone()));

        let mut switcher = Switcher::new(self.current_step.clone());
        for step in &self.steps {
            let factory = step.content_factory.as_ref().unwrap_or_else(|| {
                panic!(
                    "WizardStep \"{}\" requires .content(...) — no content factory was set",
                    step.title
                )
            });
            switcher = switcher.child_boxed(factory());
        }
        let switcher_id = ctx.add(switcher);

        let footer_id = ctx.add(WizardFooter::new(
            self.current_step.clone(),
            self.steps.len(),
            self.back_label.clone(),
            self.cancel_label.clone(),
            self.next_label.clone(),
            self.finish_label.clone(),
            self.finish_action.clone(),
        ));

        let root = ctx.add(
            VStack::new()
                .spacing(16.0)
                .add_child(header_id)
                .child(Divider::new())
                .add_child(switcher_id)
                .child(Divider::new())
                .add_child(footer_id),
        );

        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
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
        builder.set_role(fern_core::accesskit::Role::Region);
        builder.set_name(fern_i18n::tr_widget!(a11y_wizard_content_name()).resolve_now());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

pub struct Wizard {
    label: String,
    variant: ButtonVariant,
    enabled: bool,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    size: (u32, u32),
    steps: Vec<WizardStep>,
    back_label: String,
    cancel_label: String,
    next_label: String,
    finish_label: String,
    finish_action: Option<WizardAction>,
    pending_trigger: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl Wizard {
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            variant: ButtonVariant::Filled,
            enabled: true,
            presentation: ModalPresentation::Auto,
            close_behavior: ModalCloseBehavior::Manual,
            size: (DEFAULT_WIZARD_WIDTH, DEFAULT_WIZARD_HEIGHT),
            steps: Vec::new(),
            back_label: DEFAULT_BACK_LABEL.to_string(),
            cancel_label: DEFAULT_CANCEL_LABEL.to_string(),
            next_label: DEFAULT_NEXT_LABEL.to_string(),
            finish_label: DEFAULT_FINISH_LABEL.to_string(),
            finish_action: None,
            pending_trigger: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
    }

    pub fn step(mut self, step: WizardStep) -> Self {
        self.steps.push(step);
        self
    }

    pub fn steps(mut self, steps: impl IntoIterator<Item = WizardStep>) -> Self {
        self.steps.extend(steps);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn presentation(mut self, presentation: ModalPresentation) -> Self {
        self.presentation = presentation;
        self
    }

    pub fn close_behavior(mut self, close_behavior: ModalCloseBehavior) -> Self {
        self.close_behavior = close_behavior;
        self
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.size = (width, height);
        self
    }

    pub fn back_label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.back_label = ls.resolve_now();
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `back_label(...)`.
    #[doc(hidden)]
    pub fn back_label_literal(mut self, label: impl Into<String>) -> Self {
        self.back_label = label.into();
        self
    }

    pub fn cancel_label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.cancel_label = ls.resolve_now();
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `cancel_label(...)`.
    #[doc(hidden)]
    pub fn cancel_label_literal(mut self, label: impl Into<String>) -> Self {
        self.cancel_label = label.into();
        self
    }

    pub fn next_label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.next_label = ls.resolve_now();
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `next_label(...)`.
    #[doc(hidden)]
    pub fn next_label_literal(mut self, label: impl Into<String>) -> Self {
        self.next_label = label.into();
        self
    }

    pub fn finish_label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.finish_label = ls.resolve_now();
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `finish_label(...)`.
    #[doc(hidden)]
    pub fn finish_label_literal(mut self, label: impl Into<String>) -> Self {
        self.finish_label = label.into();
        self
    }

    pub fn on_finish(mut self, action: impl Fn(&mut EventContext) + 'static) -> Self {
        self.finish_action = Some(Rc::new(action));
        self
    }

    pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self {
        self.pending_trigger = Some(Box::new(trigger));
        self
    }
}

impl std::fmt::Debug for Wizard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wizard")
            .field("label", &self.label)
            .field("style", &self.variant)
            .field("enabled", &self.enabled)
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl Widget for Wizard {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let enabled = self.enabled;
        let style = self.variant;
        let presentation = self.presentation;
        let close_behavior = self.close_behavior;
        let size = self.size;
        let steps = self.steps.clone();
        let back_label = self.back_label.clone();
        let cancel_label = self.cancel_label.clone();
        let next_label = self.next_label.clone();
        let finish_label = self.finish_label.clone();
        let finish_action = self.finish_action.clone();

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            ctx.add(
                OverlayTrigger::new(
                    trigger,
                    fern_core::widget_builder::HandlerSet::new()
                        .focusable(true)
                        .cursor(fern_core::widget::CursorIcon::Pointer)
                        .on_tap({
                            let label = self.label.clone();
                            let steps = steps.clone();
                            let back_label = back_label.clone();
                            let cancel_label = cancel_label.clone();
                            let next_label = next_label.clone();
                            let finish_label = finish_label.clone();
                            let finish_action = finish_action.clone();
                            move |_pos, ctx| {
                                if !enabled {
                                    return;
                                }
                                queue_wizard_request(
                                    ctx,
                                    &label,
                                    &steps,
                                    presentation,
                                    close_behavior,
                                    size,
                                    &back_label,
                                    &cancel_label,
                                    &next_label,
                                    &finish_label,
                                    finish_action.as_ref(),
                                );
                            }
                        })
                        .on_key({
                            let label = self.label.clone();
                            let steps = steps.clone();
                            let back_label = back_label.clone();
                            let cancel_label = cancel_label.clone();
                            let next_label = next_label.clone();
                            let finish_label = finish_label.clone();
                            let finish_action = finish_action.clone();
                            move |event, ctx| match event {
                                WidgetEvent::KeyUp {
                                    key: Key::Enter | Key::Space,
                                    ..
                                } if enabled => {
                                    queue_wizard_request(
                                        ctx,
                                        &label,
                                        &steps,
                                        presentation,
                                        close_behavior,
                                        size,
                                        &back_label,
                                        &cancel_label,
                                        &next_label,
                                        &finish_label,
                                        finish_action.as_ref(),
                                    );
                                    EventResponse::Handled
                                }
                                _ => EventResponse::Ignored,
                            }
                        })
                        .on_access_action({
                            let label = self.label.clone();
                            let steps = steps.clone();
                            let back_label = back_label.clone();
                            let cancel_label = cancel_label.clone();
                            let next_label = next_label.clone();
                            let finish_label = finish_label.clone();
                            let finish_action = finish_action.clone();
                            move |action, ctx| {
                                if action == fern_core::accesskit::Action::Click && enabled {
                                    queue_wizard_request(
                                        ctx,
                                        &label,
                                        &steps,
                                        presentation,
                                        close_behavior,
                                        size,
                                        &back_label,
                                        &cancel_label,
                                        &next_label,
                                        &finish_label,
                                        finish_action.as_ref(),
                                    );
                                    EventResponse::Handled
                                } else {
                                    EventResponse::Ignored
                                }
                            }
                        }),
                )
                .name(self.label.clone()),
            )
        } else {
            ctx.add(
                Button::new_literal(self.label.clone())
                    .variant(style)
                    .enabled(enabled)
                    .on_activate_fn({
                        let label = self.label.clone();
                        let steps = steps.clone();
                        let back_label = back_label.clone();
                        let cancel_label = cancel_label.clone();
                        let next_label = next_label.clone();
                        let finish_label = finish_label.clone();
                        let finish_action = finish_action.clone();
                        move |ctx| {
                            if !enabled {
                                return;
                            }
                            queue_wizard_request(
                                ctx,
                                &label,
                                &steps,
                                presentation,
                                close_behavior,
                                size,
                                &back_label,
                                &cancel_label,
                                &next_label,
                                &finish_label,
                                finish_action.as_ref(),
                            );
                        }
                    }),
            )
        };

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
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
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::Size;
    use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
    use fern_core::widget_tree::WidgetTree;
    use fern_core::{ModalContent, ModalPresentation};

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn wizard_queues_modal_request() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(
            Wizard::new_literal("Open wizard")
                .step(WizardStep::new_literal("Details").content(|| FixedLeaf(220.0, 120.0))),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open wizard").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.presentation, ModalPresentation::Auto);
        assert_eq!(
            requests[0].request.close_behavior,
            ModalCloseBehavior::Manual
        );
        assert!(matches!(
            requests[0].request.content,
            ModalContent::Deferred(_)
        ));
    }

    /// Regression: the catalog used `.trigger(Button::new(...))` — the
    /// inner Button installs its own gesture arena (Button always wires
    /// `on_tap` for InteractionState tracking), and the tap was
    /// consumed there before the OverlayTrigger ancestor's on_tap
    /// could fire, so the modal was never queued. The fix routes
    /// OverlayTrigger's handlers into the child's external bucket via
    /// `apply_handlers`, so both fire on the recognized Tap.
    #[test]
    fn wizard_with_button_trigger_queues_modal_on_tap() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(
            Wizard::new_literal("Open wizard")
                .trigger(Button::new_literal("Launch"))
                .step(WizardStep::new_literal("Details").content(|| FixedLeaf(220.0, 120.0))),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Launch").unwrap();
        tree.click(trigger);

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(
            requests.len(),
            1,
            "wizard with custom Button trigger must queue a modal on tap"
        );
    }

    #[test]
    fn wizard_navigation_updates_step_and_finish_dismisses_modal() {
        let finished = Rc::new(RefCell::new(false));
        let finished_flag = finished.clone();

        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let trigger = tree.add(Button::new_literal("Anchor"));
        tree.add(
            Wizard::new_literal("Launch")
                .step(
                    WizardStep::new_literal("Account")
                        .content(|| Button::new_literal("Account field"))
                        .supporting_text_literal("Enter the account details before continuing."),
                )
                .step(
                    WizardStep::new_literal("Review")
                        .content(|| Button::new_literal("Review field")),
                )
                .finish_label_literal("Create")
                .on_finish(move |_ctx| {
                    *finished_flag.borrow_mut() = true;
                }),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let launch = tree.find_by_label("Launch").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(launch),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });

        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        let content_id = match request.content {
            ModalContent::Deferred(builder) => builder(&mut tree),
            ModalContent::ExistingWidget(_) => unreachable!("wizard uses deferred modal content"),
        };

        tree.show_overlay(OverlayRequest {
            content_id,
            anchor: trigger,
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert!(tree.find_by_label("Step 1 of 2").is_some());

        let next = tree.find_by_label("Next").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(next),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert!(tree.find_by_label("Step 2 of 2").is_some());

        let finish = tree.find_by_label("Create").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(finish),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert!(*finished.borrow());
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    #[should_panic(expected = "requires .content(...)")]
    fn wizard_step_without_content_panics_on_modal_build() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Wizard::new_literal("Open wizard").step(WizardStep::new_literal("Details")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open wizard").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });

        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        match request.content {
            ModalContent::Deferred(builder) => {
                builder(&mut tree);
            }
            ModalContent::ExistingWidget(_) => unreachable!("wizard uses deferred modal content"),
        }
    }
}
