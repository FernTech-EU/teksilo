use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

use crate::button::{Button, ButtonStyle};

const SURFACE_PADDING: f32 = 16.0;

struct PopoverSurface {
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn Widget>>,
}

impl PopoverSurface {
    fn new(content: Box<dyn Widget>) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
        }
    }
}

impl std::fmt::Debug for PopoverSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopoverSurface").finish()
    }
}

impl Widget for PopoverSurface {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(content) = self.pending_content.take() {
            self.content_id = Some(ctx.add_boxed(content));
        }
        self.children()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let inset = SURFACE_PADDING * 2.0;
        self.content_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width.map(|width| (width - inset).max(0.0)),
                        height: proposal.height.map(|height| (height - inset).max(0.0)),
                    },
                )
            })
            .map(|size| Size::new(size.width + inset, size.height + inset))
            .unwrap_or_else(|| proposal.resolve(200.0, 80.0))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x + SURFACE_PADDING, bounds.y + SURFACE_PADDING);
            child.size = Size::new(
                (bounds.width - SURFACE_PADDING * 2.0).max(0.0),
                (bounds.height - SURFACE_PADDING * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(ctx.theme.shape.radius_md);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.surface);
        canvas.stroke_rounded_rect(
            bounds,
            radius,
            ctx.theme.colors.border,
            ctx.theme.shape.border_width,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}

pub struct Popover {
    label: String,
    style: ButtonStyle,
    enabled: bool,
    placement: OverlayPlacement,
    dismiss: DismissBehavior,
    pending_content: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl Popover {
    pub fn new(label: impl Into<String>, content: impl Widget + 'static) -> Self {
        Self {
            label: label.into(),
            style: ButtonStyle::Outlined,
            enabled: true,
            placement: OverlayPlacement::BelowPreferred,
            dismiss: DismissBehavior::ClickOutside,
            pending_content: Some(Box::new(content)),
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

    pub fn placement(mut self, placement: OverlayPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn dismiss_behavior(mut self, dismiss: DismissBehavior) -> Self {
        self.dismiss = dismiss;
        self
    }
}

impl std::fmt::Debug for Popover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Popover")
            .field("label", &self.label)
            .field("style", &self.style)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for Popover {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let label = self.label.clone();
        let enabled = self.enabled;
        let placement = self.placement.clone();
        let dismiss = self.dismiss.clone();
        let style = self.style;
        let content_id = ctx.add(PopoverSurface::new(
            self.pending_content
                .take()
                .expect("Popover built without content"),
        ));
        ctx.set_dormant(content_id);

        let trigger = Button::new(label)
            .style(style)
            .enabled(enabled)
            .on_tap({
                let placement = placement.clone();
                let dismiss = dismiss.clone();
                move |ctx| {
                    if !enabled {
                        return;
                    }
                    ctx.dismiss_all_overlays();
                    ctx.activate(content_id);
                    ctx.show_overlay(OverlayRequest {
                        content_id,
                        anchor: self_id,
                        placement: placement.clone(),
                        dismiss: dismiss.clone(),
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                    });
                }
            })
            .on_key({
                let placement = placement.clone();
                let dismiss = dismiss.clone();
                move |event, ctx| match event {
                    WidgetEvent::KeyUp {
                        key: Key::Enter | Key::Space,
                        ..
                    } if enabled => {
                        ctx.dismiss_all_overlays();
                        ctx.activate(content_id);
                        ctx.show_overlay(OverlayRequest {
                            content_id,
                            anchor: self_id,
                            placement: placement.clone(),
                            dismiss: dismiss.clone(),
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                        });
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            })
            .on_access_action({
                move |action, ctx| {
                    if action == fern_core::accesskit::Action::Click && enabled {
                        ctx.dismiss_all_overlays();
                        ctx.activate(content_id);
                        ctx.show_overlay(OverlayRequest {
                            content_id,
                            anchor: self_id,
                            placement: placement.clone(),
                            dismiss: dismiss.clone(),
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                        });
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            });

        let root_id = ctx.add(trigger);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(120.0, 40.0))
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
    fn access_click_opens_popover_overlay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Popover::new("Show popover", FixedLeaf(140.0, 60.0)));
        tree.layout(SizeProposal::exact(480.0, 320.0));

        let trigger = tree.find_by_label("Show popover").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
        });

        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn escape_dismisses_popover_overlay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Popover::new("Show popover", FixedLeaf(140.0, 60.0)));
        tree.layout(SizeProposal::exact(480.0, 320.0));

        let trigger = tree.find_by_label("Show popover").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
        });
        tree.press_key(Key::Escape, fern_core::event::Modifiers::NONE);

        assert!(tree.active_overlays().is_empty());
    }
}