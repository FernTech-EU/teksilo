use std::time::Duration;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

use crate::button::{Button, ButtonVariant};
use crate::overlay_trigger::OverlayTrigger;

const DEFAULT_AUTO_DISMISS: Duration = Duration::from_secs(4);

fn present_snackbar(
    ctx: &mut fern_core::widget::EventContext,
    anchor: WidgetId,
    content_id: WidgetId,
    dismiss: DismissBehavior,
    auto_dismiss_after: Option<Duration>,
) {
    ctx.dismiss_all_overlays();
    ctx.activate(content_id);
    let request = OverlayRequest {
        content_id,
        anchor,
        placement: OverlayPlacement::BottomCenter,
        dismiss,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: None,
    };
    if let Some(duration) = auto_dismiss_after {
        ctx.show_overlay_for(request, duration);
    } else {
        ctx.show_overlay(request);
    }
}

struct SnackbarSurface {
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn Widget>>,
    /// Optional explicit SR announcement string. When set,
    /// `accessibility()` uses it as the Alert's accessible name
    /// so screen readers read out the caller-provided message
    /// the moment the snackbar appears. Falls back to the
    /// generic `a11y_snackbar_name` when unset.
    announcement: Option<String>,
}

impl SnackbarSurface {
    fn new(content: Box<dyn Widget>) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
            announcement: None,
        }
    }

    fn with_announcement(mut self, text: Option<String>) -> Self {
        self.announcement = text;
        self
    }
}

impl std::fmt::Debug for SnackbarSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnackbarSurface").finish()
    }
}

impl Widget for SnackbarSurface {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(content) = self.pending_content.take() {
            self.content_id = Some(ctx.add_boxed(content));
        }
        self.children()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let style = ctx.theme.components.notification;
        let inset_x = style.padding_horizontal * 2.0;
        let inset_y = style.padding_vertical * 2.0;
        let content = self
            .content_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width.map(|width| (width - inset_x).max(0.0)),
                        height: proposal.height.map(|height| (height - inset_y).max(0.0)),
                    },
                )
            })
            .unwrap_or_else(|| proposal.resolve(220.0, 44.0));

        Size::new(content.width + inset_x, content.height + inset_y)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let style = ctx.theme.components.notification;
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(
                bounds.x + style.padding_horizontal,
                bounds.y + style.padding_vertical,
            );
            child.size = Size::new(
                (bounds.width - style.padding_horizontal * 2.0).max(0.0),
                (bounds.height - style.padding_vertical * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let style = ctx.theme.components.notification;
        let radius = CornerRadius::uniform(style.corner_radius);
        // Notifications use the (dark) tooltip surface for high-contrast popups.
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_bg);
        canvas.stroke_rounded_rect(
            bounds,
            radius,
            ctx.theme.colors.tooltip_border,
            ctx.theme.shape.border_width,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Role::Alert + Live::Polite mirrors the ARIA pattern for
        // transient notifications: screen readers announce the
        // contents when the snackbar appears, without interrupting
        // the user's current action. The accessible name is the
        // caller-supplied announcement when present, otherwise
        // the generic fallback. Child widgets still contribute
        // their own nodes for full context.
        builder.set_role(fern_core::accesskit::Role::Alert);
        builder.set_live(fern_core::accesskit::Live::Polite);
        let name = self
            .announcement
            .clone()
            .unwrap_or_else(|| fern_i18n::tr_widget!(a11y_snackbar_name()).resolve_now());
        builder.set_name(name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}

pub struct Snackbar {
    label: String,
    style: ButtonVariant,
    enabled: bool,
    dismiss: DismissBehavior,
    auto_dismiss_after: Option<Duration>,
    pending_content: Option<Box<dyn Widget>>,
    pending_trigger: Option<Box<dyn Widget>>,
    /// Optional explicit announcement string threaded through to
    /// the `SnackbarSurface`'s a11y node. When set, screen readers
    /// read this as the Alert's name when the snackbar appears.
    announcement: Option<String>,
    root_child_id: Option<WidgetId>,
}

impl Snackbar {
    pub fn new(
        label: impl Into<fern_i18n::LocalizedString>,
        content: impl Widget + 'static,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            style: ButtonVariant::Regular,
            enabled: true,
            dismiss: DismissBehavior::ClickOutside,
            auto_dismiss_after: Some(DEFAULT_AUTO_DISMISS),
            pending_content: Some(Box::new(content)),
            pending_trigger: None,
            announcement: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>, content: impl Widget + 'static) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label), content)
    }

    pub fn style(mut self, style: ButtonVariant) -> Self {
        self.style = style;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn dismiss_behavior(mut self, dismiss: DismissBehavior) -> Self {
        self.dismiss = dismiss;
        self
    }

    pub fn auto_dismiss_after(mut self, duration: Duration) -> Self {
        self.auto_dismiss_after = Some(duration);
        self
    }

    pub fn persistent(mut self) -> Self {
        self.auto_dismiss_after = None;
        self
    }

    pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self {
        self.pending_trigger = Some(Box::new(trigger));
        self
    }

    /// Screen-reader announcement string — used as the Alert's
    /// accessible name when the snackbar appears. Without this
    /// the surface falls back to the generic `a11y_snackbar_name`
    /// i18n string, which says "notification" but can't describe
    /// the specific message. Set this whenever the snackbar
    /// conveys information the user needs to hear (errors,
    /// confirmations, status changes).
    pub fn announcement(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.announcement = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `announcement(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn announcement_literal(mut self, text: impl Into<String>) -> Self {
        self.announcement = Some(text.into());
        self
    }
}

impl std::fmt::Debug for Snackbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snackbar")
            .field("label", &self.label)
            .field("style", &self.style)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for Snackbar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let label = self.label.clone();
        let enabled = self.enabled;
        let dismiss = self.dismiss.clone();
        let auto_dismiss_after = self.auto_dismiss_after;
        let style = self.style;
        let content_id = ctx.add(
            SnackbarSurface::new(
                self.pending_content
                    .take()
                    .expect("Snackbar built without content"),
            )
            .with_announcement(self.announcement.clone()),
        );
        ctx.set_dormant(content_id);

        let open_on_tap = {
            let dismiss = dismiss.clone();
            move |_pos: fern_canvas::Point, ctx: &mut fern_core::widget::EventContext| {
                if !enabled {
                    return;
                }
                present_snackbar(
                    ctx,
                    self_id,
                    content_id,
                    dismiss.clone(),
                    auto_dismiss_after,
                );
            }
        };

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            ctx.add(
                OverlayTrigger::new(
                    trigger,
                    fern_core::widget_builder::HandlerSet::new()
                        .focusable(true)
                        .cursor(fern_core::widget::CursorIcon::Pointer)
                        .on_tap(open_on_tap)
                        .on_key({
                            let dismiss = dismiss.clone();
                            move |event, ctx| match event {
                                WidgetEvent::KeyUp {
                                    key: Key::Enter | Key::Space,
                                    ..
                                } if enabled => {
                                    present_snackbar(
                                        ctx,
                                        self_id,
                                        content_id,
                                        dismiss.clone(),
                                        auto_dismiss_after,
                                    );
                                    EventResponse::Handled
                                }
                                _ => EventResponse::Ignored,
                            }
                        })
                        .on_access_action({
                            move |action, ctx| {
                                if action == fern_core::accesskit::Action::Click && enabled {
                                    present_snackbar(
                                        ctx,
                                        self_id,
                                        content_id,
                                        dismiss.clone(),
                                        auto_dismiss_after,
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
                Button::new_literal(label)
                    .style(style)
                    .enabled(enabled)
                    .on_tap(open_on_tap)
                    .on_key({
                        let dismiss = dismiss.clone();
                        move |event, ctx| match event {
                            WidgetEvent::KeyUp {
                                key: Key::Enter | Key::Space,
                                ..
                            } if enabled => {
                                present_snackbar(
                                    ctx,
                                    self_id,
                                    content_id,
                                    dismiss.clone(),
                                    auto_dismiss_after,
                                );
                                EventResponse::Handled
                            }
                            _ => EventResponse::Ignored,
                        }
                    })
                    .on_access_action({
                        move |action, ctx| {
                            if action == fern_core::accesskit::Action::Click && enabled {
                                present_snackbar(
                                    ctx,
                                    self_id,
                                    content_id,
                                    dismiss.clone(),
                                    auto_dismiss_after,
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
    fn access_click_opens_bottom_center_snackbar() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Snackbar::new_literal("Show snackbar", FixedLeaf(220.0, 40.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Show snackbar").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert_eq!(tree.active_overlays().len(), 1);
        let content_id = tree.overlay_manager().active_content_ids()[0];
        let bounds = tree.bounds(content_id);
        let expected_x = (800.0 - bounds.width) / 2.0;
        assert!((bounds.x - expected_x).abs() < 1.0);
        assert!((bounds.y + bounds.height - (600.0 - 24.0)).abs() < 1.0);
    }

    #[test]
    fn custom_trigger_opens_snackbar() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            Snackbar::new_literal("Show snackbar", FixedLeaf(180.0, 36.0)).trigger(FixedLeaf(132.0, 36.0)),
        );
        tree.layout(SizeProposal::exact(640.0, 480.0));

        let trigger = tree.find_by_label("Show snackbar").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });

        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn snackbar_auto_dismisses_after_duration() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            Snackbar::new_literal("Show snackbar", FixedLeaf(220.0, 40.0))
                .auto_dismiss_after(Duration::from_millis(300)),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Show snackbar").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(trigger), target_node: fern_core::accessibility::root_node_id(), data: None });
        assert_eq!(tree.active_overlays().len(), 1);

        tree.advance_time(Duration::from_millis(200));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.advance_time(Duration::from_millis(150));
        assert!(tree.active_overlays().is_empty());
    }
}
