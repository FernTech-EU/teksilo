use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

use crate::button::{Button, ButtonStyle};
use crate::overlay_trigger::OverlayTrigger;
use crate::primitives::{Divider, TextWidget, VStack};

const DEFAULT_MODAL_PADDING: f32 = 24.0;
const DEFAULT_MODAL_MIN_WIDTH: f32 = 320.0;

type DialogFactory = std::rc::Rc<dyn Fn() -> Box<dyn Widget>>;

pub struct ModalContainer {
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn Widget>>,
    padding: f32,
    min_width: f32,
}

impl ModalContainer {
    pub fn new(content: impl Widget + 'static) -> Self {
        Self::boxed(Box::new(content))
    }

    pub(crate) fn boxed(content: Box<dyn Widget>) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
            padding: DEFAULT_MODAL_PADDING,
            min_width: DEFAULT_MODAL_MIN_WIDTH,
        }
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding.max(0.0);
        self
    }

    pub fn min_width(mut self, min_width: f32) -> Self {
        self.min_width = min_width.max(0.0);
        self
    }
}

impl std::fmt::Debug for ModalContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModalContainer")
            .field("padding", &self.padding)
            .field("min_width", &self.min_width)
            .finish()
    }
}

impl Widget for ModalContainer {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(content) = self.pending_content.take() {
            self.content_id = Some(ctx.add_boxed(content));
        }
        self.children()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let inset = self.padding * 2.0;
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

        Size::new(
            (content.width + inset).max(self.min_width),
            content.height + inset,
        )
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin =
                fern_canvas::Point::new(bounds.x + self.padding, bounds.y + self.padding);
            child.size = Size::new(
                (bounds.width - self.padding * 2.0).max(0.0),
                (bounds.height - self.padding * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(ctx.theme.shape.radius_md);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.surface);
        canvas.stroke_rounded_rect(
            bounds,
            radius,
            ctx.theme.colors.border_strong,
            ctx.theme.shape.border_width,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Dialog);
        builder.set_name("Dialog");
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
    pending_body: Option<Box<dyn Widget>>,
    pending_footer: Option<Box<dyn Widget>>,
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

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn supporting_text(mut self, text: impl Into<String>) -> Self {
        self.supporting_text = Some(text.into());
        self
    }

    pub fn body(mut self, body: impl Widget + 'static) -> Self {
        self.pending_body = Some(Box::new(body));
        self
    }

    pub fn footer(mut self, footer: impl Widget + 'static) -> Self {
        self.pending_footer = Some(Box::new(footer));
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
        let theme = ctx.theme().clone();
        let mut stack = VStack::new().spacing(16.0);

        if self.title.is_some() || self.supporting_text.is_some() {
            let mut header = VStack::new().spacing(8.0);
            if let Some(title) = self.title.clone() {
                header = header.child(
                    TextWidget::new(title)
                        .style(theme.typography.heading_3.clone())
                        .color(theme.colors.on_surface),
                );
            }
            if let Some(text) = self.supporting_text.clone() {
                header = header.child(
                    TextWidget::new(text)
                        .style(theme.typography.body.clone())
                        .color(theme.colors.on_surface_secondary),
                );
            }
            let header_id = ctx.add(header);
            stack = stack.add_child(header_id);
        }

        if let Some(body) = self.pending_body.take() {
            let body_id = ctx.add_boxed(body);
            stack = stack.add_child(body_id);
        }

        if let Some(footer) = self.pending_footer.take() {
            let divider_id = ctx.add(Divider::new());
            let footer_id = ctx.add_boxed(footer);
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

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

pub struct Dialog {
    label: String,
    style: ButtonStyle,
    enabled: bool,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    content_factory: DialogFactory,
    pending_trigger: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl Dialog {
    pub fn new<W, F>(label: impl Into<String>, factory: F) -> Self
    where
        W: Widget + 'static,
        F: Fn() -> W + 'static,
    {
        Self {
            label: label.into(),
            style: ButtonStyle::Filled,
            enabled: true,
            presentation: ModalPresentation::Auto,
            close_behavior: ModalCloseBehavior::EscapeOrClickOutside,
            content_factory: std::rc::Rc::new(move || {
                Box::new(factory()) as Box<dyn Widget>
            }),
            pending_trigger: None,
            root_child_id: None,
        }
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
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
        self.pending_trigger = Some(Box::new(trigger));
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
        let content_factory = self.content_factory.clone();

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            ctx.add(
                OverlayTrigger::new(
                    trigger,
                    fern_core::widget_builder::HandlerSet::new()
                        .focusable(true)
                        .cursor(fern_core::widget::CursorIcon::Pointer)
                        .on_tap({
                            let label = label.clone();
                            let content_factory = content_factory.clone();
                            move |ctx| {
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
                        }),
                )
                .name(label),
            )
        } else {
            ctx.add(
                Button::new(label)
                    .style(style)
                    .enabled(enabled)
                    .on_tap({
                        let label = self.label.clone();
                        let content_factory = content_factory.clone();
                        move |ctx| {
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
        tree.add(Dialog::new("Open dialog", || FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
        });

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
        tree.add(Dialog::new("Open dialog", || FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
        });

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
    fn modal_container_preserves_shell_sizing_defaults() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let container = tree.add(ModalContainer::new(FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        let bounds = tree.bounds(container);
        assert!((bounds.width - 320.0).abs() < 0.01);
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
            Dialog::new("Open dialog", || FixedLeaf(220.0, 120.0)).trigger(FixedLeaf(140.0, 40.0)),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
        });

        assert_eq!(tree.drain_pending_modal_requests().len(), 1);
    }

    #[test]
    fn dialog_content_helper_builds_dialog_sections() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Dialog::new(
            "Open dialog",
            || {
                DialogContent::new()
                    .title("Review Changes")
                    .supporting_text("Confirm the staged updates before continuing.")
                    .body(FixedLeaf(220.0, 120.0))
                    .footer(Button::new("Close"))
            },
        ));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
        });

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
            Dialog::new("Open dialog", || FixedLeaf(220.0, 120.0))
                .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.presentation, ModalPresentation::InTree);
    }

    #[test]
    fn dialog_close_behavior_can_be_overridden() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            Dialog::new("Open dialog", || FixedLeaf(220.0, 120.0))
                .close_behavior(ModalCloseBehavior::Manual),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.close_behavior, ModalCloseBehavior::Manual);
    }
}
