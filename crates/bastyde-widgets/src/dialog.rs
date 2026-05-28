use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use bastyde_core::overlay::{OverlayDismissCallback, OverlayId};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{DialogStyleConfig, SharedDialogStyle};
use bastyde_core::widget::{EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_builder::{HandlerSet, WidgetBuilder};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::button::{Button, ButtonVariant};
use crate::overlay_trigger::OverlayTrigger;
use crate::primitives::{Divider, TextWidget, VStack};

type DialogFactory = std::rc::Rc<dyn Fn() -> Box<dyn Widget>>;

/// Modal container surface — pulls all dimensions from `DialogStyle` by
/// default. Per-instance overrides are still possible via `padding()` and
/// `min_width()`; `None` means "use the theme value".
pub struct ModalContainer {
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn Widget>>,
    padding_override: Option<f32>,
    min_width_override: Option<f32>,
    /// Explicit accessible title for the dialog. Set via `.title(...)`
    /// — typically the same string the inner `DialogContent` uses as
    /// its visual title. When `None`, `accessibility()` falls back to
    /// the generic i18n `a11y_dialog_name` string so there's always
    /// a non-empty name for screen readers.
    title: Option<String>,
    /// Per-call override for the modal panel chrome. Replaces the
    /// theme-wide `style_slots.dialog` and the IntUI default
    /// `RecipeDialogStyle` for just this container.
    style_override: Option<SharedDialogStyle>,
    /// Build state — the `DialogStyle::make_panel` root.
    root_child_id: Option<WidgetId>,
}

impl ModalContainer {
    pub fn new(content: impl Widget + 'static) -> Self {
        Self::boxed(Box::new(content))
    }

    pub(crate) fn boxed(content: Box<dyn Widget>) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
            padding_override: None,
            min_width_override: None,
            title: None,
            style_override: None,
            root_child_id: None,
        }
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding_override = Some(padding.max(0.0));
        self
    }

    pub fn min_width(mut self, min_width: f32) -> Self {
        self.min_width_override = Some(min_width.max(0.0));
        self
    }

    /// Per-call style override for the modal panel chrome. Replaces the
    /// theme-wide default `DialogStyle` for just this container.
    pub fn style(mut self, style: impl bastyde_core::styles::DialogStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Accessible title for the dialog. Screen readers announce this
    /// as the dialog's name. Should match the inner `DialogContent`'s
    /// visible title string.
    pub fn title(mut self, title: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = title.into();
        self.title = Some(ls.resolve_now());
        self
    }
}

impl std::fmt::Debug for ModalContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModalContainer")
            .field("padding_override", &self.padding_override)
            .field("min_width_override", &self.min_width_override)
            .finish()
    }
}

impl Widget for ModalContainer {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(content) = self.pending_content.take() {
            // If the caller didn't set an explicit `.title(...)`,
            // ask the content widget for a suggested title — e.g.
            // `DialogContent::accessible_title_hint` returns its
            // own visible title. This lets dialogs announce their
            // real name without forcing callers to duplicate the
            // string at both the content and the container level.
            if self.title.is_none()
                && let Some(hint) = content.accessible_title_hint()
            {
                self.title = Some(hint);
            }
            self.content_id = Some(ctx.add_boxed(content));
        }

        // The panel chrome (rounded surface + border + content
        // padding) is owned by the active `DialogStyle`; the modal
        // mounting / dismissal pipeline stays on this widget.
        let content_id = self
            .content_id
            .expect("ModalContainer requires content — none was set");
        let style: SharedDialogStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.dialog.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeDialogStyle));
        let cfg = DialogStyleConfig {
            content: content_id,
            has_scrim: true,
            padding_override: self.padding_override,
            min_width_override: self.min_width_override,
        };
        let root_id = style.make_panel(&cfg, ctx);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(240.0, 120.0))
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
        builder.set_role(bastyde_core::accesskit::Role::Dialog);
        let name = self
            .title
            .clone()
            .unwrap_or_else(|| bastyde_i18n::tr_widget!(a11y_dialog_name()).resolve_now());
        builder.set_name(name);
        // ModalContainer is always modal — it's the one path that goes
        // through `ModalRequest` / `ModalPresentation`. A dialog that
        // doesn't block outside interaction would use `Popover` instead.
        builder.set_modal();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Full-viewport dimming scrim painted behind a [`ModalContainer`].
///
/// Mounted by the modal-presentation pipeline (bastyde-app) as a separate
/// `OverlayPlacement::FullViewport` overlay pushed BEFORE the centered
/// modal overlay so it z-orders below the panel. The chrome itself is
/// delegated to the active [`DialogStyle::make_scrim`]; clicking the
/// scrim dismisses the linked modal when the modal's
/// [`ModalCloseBehavior`] permits click-outside dismissal.
///
/// The dismissal cascade is wired via
/// [`OverlayManager::set_parent_overlay`] AFTER both overlays are
/// pushed — the scrim's `parent_overlay` is set to the modal's id, so
/// any dismiss of the modal cascades through `dismiss_immediate` and
/// also dismisses the scrim. The scrim's own `dismiss` behavior is
/// `Manual` — it never dismisses itself directly.
pub struct ModalScrim {
    style_override: Option<SharedDialogStyle>,
    /// Filled in by the framework AFTER the modal overlay is pushed
    /// — the scrim is mounted FIRST (so it z-orders below the modal),
    /// so the modal's `OverlayId` isn't yet known at build time. The
    /// scrim's on-tap closure reads through this `Cell` at click time
    /// rather than capturing a value that doesn't exist yet.
    dismiss_target: Rc<Cell<Option<OverlayId>>>,
    /// Whether clicking the scrim should dismiss `dismiss_target`.
    /// Reflects the modal's [`ModalCloseBehavior`]: `true` for
    /// `ClickOutside` and `EscapeOrClickOutside`; `false` for
    /// `EscapeKey` and `Manual` (clicks on the dim are absorbed but
    /// do not dismiss).
    click_to_dismiss: bool,
    root_child_id: Option<WidgetId>,
}

impl ModalScrim {
    pub fn new() -> Self {
        Self {
            style_override: None,
            dismiss_target: Rc::new(Cell::new(None)),
            click_to_dismiss: false,
            root_child_id: None,
        }
    }

    /// Per-call style override for the scrim chrome. Replaces the
    /// theme-wide default `DialogStyle` for just this scrim.
    pub fn style(mut self, style: impl bastyde_core::styles::DialogStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Handle to the modal-overlay id the scrim dismisses on click.
    /// The framework fills this AFTER the modal is pushed (see the
    /// in-tree modal pipeline in `bastyde-app`).
    pub fn dismiss_target(mut self, target: Rc<Cell<Option<OverlayId>>>) -> Self {
        self.dismiss_target = target;
        self
    }

    /// Enable click-to-dismiss on the scrim. Should mirror whether the
    /// modal's [`ModalCloseBehavior`] permits click-outside dismissal.
    pub fn click_to_dismiss(mut self, enabled: bool) -> Self {
        self.click_to_dismiss = enabled;
        self
    }
}

impl Default for ModalScrim {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ModalScrim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModalScrim")
            .field("click_to_dismiss", &self.click_to_dismiss)
            .finish()
    }
}

impl Widget for ModalScrim {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let style: SharedDialogStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.dialog.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeDialogStyle));
        let chrome_id = style.make_scrim(ctx);

        if self.click_to_dismiss {
            let target = self.dismiss_target.clone();
            let handlers = HandlerSet::new().on_tap(move |_event, ctx| {
                if let Some(modal_id) = target.get() {
                    ctx.dismiss_overlay(modal_id);
                }
            });
            ctx.apply_self_handlers(handlers);
        }

        self.root_child_id = Some(chrome_id);
        vec![chrome_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // The scrim's actual size is determined by
        // `OverlayPlacement::FullViewport` in `position_overlays`,
        // which overrides the intrinsic size to the full viewport. We
        // still report the child's wanted size so the proposal flows
        // correctly when the framework probes the intrinsic size.
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
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Hidden from the AT: the modal panel above carries the
        // `Role::Dialog` node with the accessible name.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn queue_dialog_request(
    ctx: &mut EventContext,
    factory: &DialogFactory,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    title: &str,
    on_dismiss: Option<OverlayDismissCallback>,
) {
    let factory = factory.clone();
    let mut request = ModalRequest::deferred(move |tree| {
        let content = (factory.as_ref())();
        tree.add(ModalContainer::boxed(content))
    })
    .presentation(presentation)
    .close_behavior(close_behavior)
    .title(title)
    .size(460, 260);
    if let Some(cb) = on_dismiss {
        request = request.on_dismiss(cb);
    }
    ctx.present_modal(request);
}

pub struct DialogContent {
    title: Option<bastyde_i18n::LocalizedString>,
    supporting_text: Option<bastyde_i18n::LocalizedString>,
    pending_body: Option<PendingChild>,
    pending_footer: Option<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl DialogContent {
    pub fn new() -> Self {
        Self {
            title: None,
            supporting_text: None,
            pending_body: None,
            pending_footer: None,
            root_child_id: None,
        }
    }

    pub fn title(mut self, title: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn supporting_text(mut self, text: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        self.supporting_text = Some(text.into());
        self
    }

    pub fn body(mut self, body: impl Widget + 'static) -> Self {
        self.pending_body = Some(PendingChild::Deferred(Box::new(body)));
        self
    }

    pub fn body_id(mut self, id: WidgetId) -> Self {
        self.pending_body = Some(PendingChild::Id(id));
        self
    }

    pub fn footer(mut self, footer: impl Widget + 'static) -> Self {
        self.pending_footer = Some(PendingChild::Deferred(Box::new(footer)));
        self
    }

    pub fn footer_id(mut self, id: WidgetId) -> Self {
        self.pending_footer = Some(PendingChild::Id(id));
        self
    }
}

impl Default for DialogContent {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DialogContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogContent")
            .field("title", &self.title)
            .field("supporting_text", &self.supporting_text)
            .finish()
    }
}

impl Widget for DialogContent {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut stack = VStack::new().spacing(16.0);

        if self.title.is_some() || self.supporting_text.is_some() {
            let mut header = VStack::new().spacing(8.0);
            if let Some(title) = self.title.clone() {
                header = header.child(
                    TextWidget::new(title)
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary)
                        .single_line(),
                );
            }
            if let Some(text) = self.supporting_text.clone() {
                header = header.child(
                    TextWidget::new(text)
                        .style(TextStyleRole::Body)
                        .color(TextRole::Secondary),
                );
            }
            let header_id = ctx.add(header);
            stack = stack.add_child(header_id);
        }

        if let Some(body) = self.pending_body.take() {
            let body_id = match body {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            stack = stack.add_child(body_id);
        }

        if let Some(footer) = self.pending_footer.take() {
            let divider_id = ctx.add(Divider::new());
            let footer_id = match footer {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            stack = stack.add_child(divider_id).add_child(footer_id);
        }

        let root = ctx.add(stack);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    /// Expose the visible title to an enclosing `ModalContainer`
    /// (or any other shell) so it can use it as its own accessible
    /// name without the caller having to thread the same string
    /// through twice.
    fn accessible_title_hint(&self) -> Option<String> {
        self.title.as_ref().map(|t| t.resolve_now())
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

pub struct Dialog {
    label: bastyde_i18n::LocalizedString,
    variant: ButtonVariant,
    enabled: bool,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    content_factory: Option<DialogFactory>,
    pending_trigger: Option<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl Dialog {
    pub fn new(label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Filled,
            enabled: true,
            presentation: ModalPresentation::Auto,
            close_behavior: ModalCloseBehavior::EscapeOrClickOutside,
            content_factory: None,
            pending_trigger: None,
            root_child_id: None,
        }
    }

    pub fn content<W, F>(mut self, factory: F) -> Self
    where
        W: Widget + 'static,
        F: Fn() -> W + 'static,
    {
        self.content_factory = Some(std::rc::Rc::new(move || {
            Box::new(factory()) as Box<dyn Widget>
        }));
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

    pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self {
        self.pending_trigger = Some(PendingChild::Deferred(Box::new(trigger)));
        self
    }

    pub fn trigger_id(mut self, id: WidgetId) -> Self {
        self.pending_trigger = Some(PendingChild::Id(id));
        self
    }
}

impl std::fmt::Debug for Dialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dialog")
            .field("label", &self.label)
            .field("style", &self.variant)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for Dialog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let label = self.label.clone();
        let enabled = self.enabled;
        let close_behavior = self.close_behavior;
        let presentation = self.presentation;
        let style = self.variant;
        let content_factory = self
            .content_factory
            .clone()
            .expect("Dialog requires .content(...) — no content factory was set");

        // Track whether the modal is currently open so the trigger can set
        // aria-expanded correctly. The dismiss callback resets it to false
        // regardless of which close path fires (Escape, click-outside, explicit
        // ctx.dismiss_modal()). Only in-tree presentations fire this callback.
        let is_open: Signal<bool> = ctx.signal(false);
        let dismiss_callback: OverlayDismissCallback = {
            let is_open = is_open.clone();
            std::rc::Rc::new(move || {
                is_open.set(false);
            })
        };

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            let tap_open = is_open.clone();
            let tap_dismiss = dismiss_callback.clone();
            let key_open = is_open.clone();
            let key_dismiss = dismiss_callback.clone();
            let action_open = is_open.clone();
            let action_dismiss = dismiss_callback.clone();
            let handlers = bastyde_core::widget_builder::HandlerSet::new()
                .focusable(true)
                .cursor(bastyde_core::widget::CursorIcon::Pointer)
                .on_tap({
                    let label = label.clone();
                    let content_factory = content_factory.clone();
                    move |_pos, ctx| {
                        if !enabled {
                            return;
                        }
                        tap_open.set(true);
                        queue_dialog_request(
                            ctx,
                            &content_factory,
                            presentation,
                            close_behavior,
                            &label.resolve_now(),
                            Some(tap_dismiss.clone()),
                        );
                    }
                })
                .on_key({
                    let label = label.clone();
                    let content_factory = content_factory.clone();
                    move |event, ctx| match event {
                        WidgetEvent::KeyUp {
                            key: Key::Enter | Key::Space,
                            ..
                        } if enabled => {
                            key_open.set(true);
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label.resolve_now(),
                                Some(key_dismiss.clone()),
                            );
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                })
                .on_access_action({
                    let label = label.clone();
                    let content_factory = content_factory.clone();
                    move |action, ctx| {
                        if action == bastyde_core::accesskit::Action::Click && enabled {
                            action_open.set(true);
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label.resolve_now(),
                                Some(action_dismiss.clone()),
                            );
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                });
            let overlay_trigger = match trigger {
                PendingChild::Id(id) => OverlayTrigger::from_id(id, handlers),
                PendingChild::Deferred(widget) => OverlayTrigger::new(widget, handlers),
            }
            .name(label)
            .has_popup(bastyde_core::accesskit::HasPopup::Dialog)
            .expanded_when(is_open.clone());
            ctx.add(overlay_trigger)
        } else {
            let tap_open = is_open.clone();
            let tap_dismiss = dismiss_callback.clone();
            let key_open = is_open.clone();
            let key_dismiss = dismiss_callback.clone();
            let action_open = is_open.clone();
            let action_dismiss = dismiss_callback.clone();
            ctx.add(
                Button::new(label)
                    .variant(style)
                    .enabled(enabled)
                    .has_popup(bastyde_core::accesskit::HasPopup::Dialog)
                    .expanded_when(is_open.clone())
                    .on_tap({
                        let label = self.label.clone();
                        let content_factory = content_factory.clone();
                        move |_pos, ctx| {
                            if !enabled {
                                return;
                            }
                            tap_open.set(true);
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label.resolve_now(),
                                Some(tap_dismiss.clone()),
                            );
                        }
                    })
                    .on_key({
                        let label = self.label.clone();
                        let content_factory = content_factory.clone();
                        move |event, ctx| match event {
                            WidgetEvent::KeyUp {
                                key: Key::Enter | Key::Space,
                                ..
                            } if enabled => {
                                key_open.set(true);
                                queue_dialog_request(
                                    ctx,
                                    &content_factory,
                                    presentation,
                                    close_behavior,
                                    &label.resolve_now(),
                                    Some(key_dismiss.clone()),
                                );
                                EventResponse::Handled
                            }
                            _ => EventResponse::Ignored,
                        }
                    })
                    .on_access_action({
                        let label = self.label.clone();
                        let content_factory = content_factory.clone();
                        move |action, ctx| {
                            if action == bastyde_core::accesskit::Action::Click && enabled {
                                action_open.set(true);
                                queue_dialog_request(
                                    ctx,
                                    &content_factory,
                                    presentation,
                                    close_behavior,
                                    &label.resolve_now(),
                                    Some(action_dismiss.clone()),
                                );
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
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
    ) -> bastyde_core::widget::LayoutResponse {
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

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::Size;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_core::{ModalContent, ModalPresentation};
    use bastyde_i18n::lit;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn access_click_opens_centered_dialog_overlay() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Dialog::new(lit!("Open dialog")).content(|| FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.presentation, ModalPresentation::Auto);
        assert_eq!(
            requests[0].request.close_behavior,
            ModalCloseBehavior::EscapeOrClickOutside,
        );
        assert!(matches!(
            requests[0].request.content,
            ModalContent::Deferred(_)
        ));
    }

    #[test]
    fn dialog_surface_exposes_dialog_role() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Dialog::new(lit!("Open dialog")).content(|| FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });

        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        let content_id = match request.content {
            ModalContent::Deferred(builder) => builder(&mut tree),
            ModalContent::ExistingWidget(_) => {
                unreachable!("dialog now always uses deferred content")
            }
        };
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let dialog = tree
            .find_by_role(bastyde_core::accesskit::Role::Dialog)
            .unwrap();
        let info = tree.accessibility_node(dialog);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Dialog);
        assert!(tree.bounds(content_id).width > 0.0);
    }

    #[test]
    fn modal_container_inherits_title_from_dialog_content() {
        // When a ModalContainer wraps a DialogContent and the
        // caller didn't set an explicit title on the container,
        // the title should propagate automatically via
        // `Widget::accessible_title_hint`.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let container = tree.add(ModalContainer::new(
            DialogContent::new()
                .title(lit!("Delete file?"))
                .body(FixedLeaf(100.0, 40.0)),
        ));
        tree.layout(SizeProposal::exact(600.0, 400.0));
        let info = tree.accessibility_node(container);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Dialog);
        assert_eq!(info.name(), Some("Delete file?"));
    }

    #[test]
    fn modal_container_explicit_title_wins_over_hint() {
        // An explicit `.title(...)` on ModalContainer takes
        // precedence over whatever the content suggests.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let container = tree.add(
            ModalContainer::new(
                DialogContent::new()
                    .title(lit!("Inner title"))
                    .body(FixedLeaf(100.0, 40.0)),
            )
            .title(lit!("Outer title")),
        );
        tree.layout(SizeProposal::exact(600.0, 400.0));
        let info = tree.accessibility_node(container);
        assert_eq!(info.name(), Some("Outer title"));
    }

    #[test]
    fn modal_container_preserves_shell_sizing_defaults() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let container = tree.add(ModalContainer::new(FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        // DialogStyle defaults: 24 dp content_padding, 280 dp min_width.
        // Content 220×120 + 48 padding = 268×168, clamped to 280×168.
        let bounds = tree.bounds(container);
        assert!((bounds.width - 280.0).abs() < 0.01);
        assert!((bounds.height - 168.0).abs() < 0.01);
    }

    #[test]
    fn modal_container_custom_padding_changes_layout() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let container = tree.add(
            ModalContainer::new(FixedLeaf(220.0, 120.0))
                .padding(12.0)
                .min_width(200.0),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        let bounds = tree.bounds(container);
        assert!((bounds.width - 244.0).abs() < 0.01);
        assert!((bounds.height - 144.0).abs() < 0.01);
    }

    #[test]
    fn custom_trigger_opens_dialog_overlay() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            Dialog::new(lit!("Open dialog"))
                .content(|| FixedLeaf(220.0, 120.0))
                .trigger(FixedLeaf(140.0, 40.0)),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // The OverlayTrigger now routes its handlers onto the trigger
        // child (so real `Button` triggers, which install their own
        // gesture arena, can't consume the tap before the opener
        // fires). Clicking the wrapper hit-tests into the child, which
        // is where the handler lives.
        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.click(trigger);

        assert_eq!(tree.drain_pending_modal_requests().len(), 1);
    }

    #[test]
    fn dialog_content_helper_builds_dialog_sections() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Dialog::new(lit!("Open dialog")).content(|| {
            DialogContent::new()
                .title(lit!("Review Changes"))
                .supporting_text(lit!("Confirm the staged updates before continuing."))
                .body(FixedLeaf(220.0, 120.0))
                .footer(Button::new(lit!("Close")))
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });

        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        match request.content {
            ModalContent::Deferred(builder) => {
                builder(&mut tree);
            }
            ModalContent::ExistingWidget(_) => {
                unreachable!("dialog now always uses deferred content")
            }
        }
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert!(tree.find_by_label("Review Changes").is_some());
        assert!(tree.find_by_label("Close").is_some());
    }

    #[test]
    fn dialog_presentation_can_be_overridden() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            Dialog::new(lit!("Open dialog"))
                .content(|| FixedLeaf(220.0, 120.0))
                .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.presentation, ModalPresentation::InTree);
    }

    #[test]
    fn dialog_close_behavior_can_be_overridden() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            Dialog::new(lit!("Open dialog"))
                .content(|| FixedLeaf(220.0, 120.0))
                .close_behavior(ModalCloseBehavior::Manual),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].request.close_behavior,
            ModalCloseBehavior::Manual
        );
    }

    #[test]
    #[should_panic(expected = "Dialog requires .content(...)")]
    fn dialog_without_content_panics_on_build() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Dialog::new(lit!("Open dialog")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
    }
}
