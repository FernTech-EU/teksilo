use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use fern_core::widget::{
    EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, TextRole};

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

    /// Accessible title for the dialog. Screen readers announce this
    /// as the dialog's name. Should match the inner `DialogContent`'s
    /// visible title string.
    pub fn title(mut self, title: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = title.into();
        self.title = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `title(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn title_literal(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    fn resolved_padding(&self, theme: &fern_tokens::Theme) -> f32 {
        self.padding_override
            .unwrap_or(theme.components.dialog.content_padding)
    }

    fn resolved_min_width(&self, theme: &fern_tokens::Theme) -> f32 {
        self.min_width_override
            .unwrap_or(theme.components.dialog.min_width)
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
            if self.title.is_none() {
                if let Some(hint) = content.accessible_title_hint() {
                    self.title = Some(hint);
                }
            }
            self.content_id = Some(ctx.add_boxed(content));
        }
        self.children()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let pad = self.resolved_padding(ctx.theme);
        let min_w = self.resolved_min_width(ctx.theme);
        let inset = pad * 2.0;
        let content = self
            .content_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width.map(|width| (width - inset).max(0.0)),
                        height: proposal.height.map(|height| (height - inset).max(0.0)),
                    },
                )
            })
            .unwrap_or_else(|| proposal.resolve(240.0, 120.0));

        Size::new((content.width + inset).max(min_w), content.height + inset)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let pad = self.resolved_padding(ctx.theme);
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x + pad, bounds.y + pad);
            child.size = Size::new(
                (bounds.width - pad * 2.0).max(0.0),
                (bounds.height - pad * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(ctx.theme.components.dialog.corner_radius);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.surface_main);
        canvas.stroke_rounded_rect(
            bounds,
            radius,
            ctx.theme.colors.border_strong,
            ctx.theme.shape.border_width,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Dialog);
        let name = self
            .title
            .clone()
            .unwrap_or_else(|| fern_i18n::tr_widget!(a11y_dialog_name()).resolve_now());
        builder.set_name(name);
        // ModalContainer is always modal — it's the one path that goes
        // through `ModalRequest` / `ModalPresentation`. A dialog that
        // doesn't block outside interaction would use `Popover` instead.
        builder.set_modal();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}

fn queue_dialog_request(
    ctx: &mut EventContext,
    factory: &DialogFactory,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    title: &str,
) {
    let factory = factory.clone();
    ctx.present_modal(
        ModalRequest::deferred(move |tree| {
            let content = (factory.as_ref())();
            tree.add(ModalContainer::boxed(content))
        })
        .presentation(presentation)
        .close_behavior(close_behavior)
        .title(title)
        .size(460, 260),
    );
}

pub struct DialogContent {
    title: Option<String>,
    supporting_text: Option<String>,
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

    pub fn title(mut self, title: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = title.into();
        self.title = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `title(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn title_literal(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
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
        let theme_signal = ctx.theme_signal();
        let snapshot = theme_signal.get();
        let typography_body_bold = snapshot.typography.body_bold.clone();
        let typography_body = snapshot.typography.body.clone();
        let mut stack = VStack::new().spacing(16.0);

        if self.title.is_some() || self.supporting_text.is_some() {
            let mut header = VStack::new().spacing(8.0);
            if let Some(title) = self.title.clone() {
                header = header.child(
                    TextWidget::new_literal(title)
                        .style(typography_body_bold)
                        .color(TextRole::Primary)
                        .single_line(),
                );
            }
            if let Some(text) = self.supporting_text.clone() {
                header = header.child(
                    TextWidget::new_literal(text)
                        .style(typography_body)
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

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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

    /// Expose the visible title to an enclosing `ModalContainer`
    /// (or any other shell) so it can use it as its own accessible
    /// name without the caller having to thread the same string
    /// through twice.
    fn accessible_title_hint(&self) -> Option<String> {
        self.title.clone()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

pub struct Dialog {
    label: String,
    style: ButtonVariant,
    enabled: bool,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    content_factory: Option<DialogFactory>,
    pending_trigger: Option<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl Dialog {
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            style: ButtonVariant::Default,
            enabled: true,
            presentation: ModalPresentation::Auto,
            close_behavior: ModalCloseBehavior::EscapeOrClickOutside,
            content_factory: None,
            pending_trigger: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
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

    pub fn style(mut self, style: ButtonVariant) -> Self {
        self.style = style;
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
            .field("style", &self.style)
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
        let style = self.style;
        let content_factory = self
            .content_factory
            .clone()
            .expect("Dialog requires .content(...) — no content factory was set");

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            let handlers = fern_core::widget_builder::HandlerSet::new()
                .focusable(true)
                .cursor(fern_core::widget::CursorIcon::Pointer)
                .on_tap({
                    let label = label.clone();
                    let content_factory = content_factory.clone();
                    move |_pos, ctx| {
                        if !enabled {
                            return;
                        }
                        queue_dialog_request(
                            ctx,
                            &content_factory,
                            presentation,
                            close_behavior,
                            &label,
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
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label,
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
                        if action == fern_core::accesskit::Action::Click && enabled {
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label,
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
            .name(label);
            ctx.add(overlay_trigger)
        } else {
            ctx.add(
                Button::new_literal(label)
                    .style(style)
                    .enabled(enabled)
                    .on_tap({
                        let label = self.label.clone();
                        let content_factory = content_factory.clone();
                        move |_pos, ctx| {
                            if !enabled {
                                return;
                            }
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label,
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
                                queue_dialog_request(
                                    ctx,
                                    &content_factory,
                                    presentation,
                                    close_behavior,
                                    &label,
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
                            if action == fern_core::accesskit::Action::Click && enabled {
                                queue_dialog_request(
                                    ctx,
                                    &content_factory,
                                    presentation,
                                    close_behavior,
                                    &label,
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

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(140.0, 40.0))
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
    use fern_core::{ModalContent, ModalPresentation};
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn access_click_opens_centered_dialog_overlay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Dialog::new_literal("Open dialog").content(|| FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.presentation, ModalPresentation::Auto);
        assert_eq!(
            requests[0].request.close_behavior,
            ModalCloseBehavior::EscapeOrClickOutside,
        );
        assert!(matches!(requests[0].request.content, ModalContent::Deferred(_)));
    }

    #[test]
    fn dialog_surface_exposes_dialog_role() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Dialog::new_literal("Open dialog").content(|| FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });

        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        let content_id = match request.content {
            ModalContent::Deferred(builder) => builder(&mut tree),
            ModalContent::ExistingWidget(_) => unreachable!("dialog now always uses deferred content"),
        };
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let dialog = tree
            .find_by_role(fern_core::accesskit::Role::Dialog)
            .unwrap();
        let info = tree.accessibility_node(dialog);
        assert_eq!(info.role(), fern_core::accesskit::Role::Dialog);
        assert!(tree.bounds(content_id).width > 0.0);
    }

    #[test]
    fn modal_container_inherits_title_from_dialog_content() {
        // When a ModalContainer wraps a DialogContent and the
        // caller didn't set an explicit title on the container,
        // the title should propagate automatically via
        // `Widget::accessible_title_hint`.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let container = tree.add(ModalContainer::new(
            DialogContent::new()
                .title_literal("Delete file?")
                .body(FixedLeaf(100.0, 40.0)),
        ));
        tree.layout(SizeProposal::exact(600.0, 400.0));
        let info = tree.accessibility_node(container);
        assert_eq!(info.role(), fern_core::accesskit::Role::Dialog);
        assert_eq!(info.name(), Some("Delete file?"));
    }

    #[test]
    fn modal_container_explicit_title_wins_over_hint() {
        // An explicit `.title(...)` on ModalContainer takes
        // precedence over whatever the content suggests.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let container = tree.add(
            ModalContainer::new(
                DialogContent::new()
                    .title_literal("Inner title")
                    .body(FixedLeaf(100.0, 40.0)),
            )
            .title_literal("Outer title"),
        );
        tree.layout(SizeProposal::exact(600.0, 400.0));
        let info = tree.accessibility_node(container);
        assert_eq!(info.name(), Some("Outer title"));
    }

    #[test]
    fn modal_container_preserves_shell_sizing_defaults() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            Dialog::new_literal("Open dialog")
                .content(|| FixedLeaf(220.0, 120.0))
                .trigger(FixedLeaf(140.0, 40.0)),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });

        assert_eq!(tree.drain_pending_modal_requests().len(), 1);
    }

    #[test]
    fn dialog_content_helper_builds_dialog_sections() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Dialog::new_literal("Open dialog").content(|| {
            DialogContent::new()
                .title_literal("Review Changes")
                .supporting_text_literal("Confirm the staged updates before continuing.")
                .body(FixedLeaf(220.0, 120.0))
                .footer(Button::new_literal("Close"))
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });

        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        match request.content {
            ModalContent::Deferred(builder) => {
                builder(&mut tree);
            }
            ModalContent::ExistingWidget(_) => unreachable!("dialog now always uses deferred content"),
        }
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert!(tree.find_by_label("Review Changes").is_some());
        assert!(tree.find_by_label("Close").is_some());
    }

    #[test]
    fn dialog_presentation_can_be_overridden() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            Dialog::new_literal("Open dialog")
                .content(|| FixedLeaf(220.0, 120.0))
                .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.presentation, ModalPresentation::InTree);
    }

    #[test]
    fn dialog_close_behavior_can_be_overridden() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            Dialog::new_literal("Open dialog")
                .content(|| FixedLeaf(220.0, 120.0))
                .close_behavior(ModalCloseBehavior::Manual),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.close_behavior, ModalCloseBehavior::Manual);
    }

    #[test]
    #[should_panic(expected = "Dialog requires .content(...)")]
    fn dialog_without_content_panics_on_build() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Dialog::new_literal("Open dialog"));
        tree.layout(SizeProposal::exact(800.0, 600.0));
    }
}
