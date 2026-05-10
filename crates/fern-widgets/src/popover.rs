use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

use crate::button::{Button, ButtonVariant};
use crate::overlay_trigger::OverlayTrigger;

const SURFACE_PADDING: f32 = 16.0;

struct PopoverSurface {
    content_id: Option<WidgetId>,
    pending_content: Option<PendingChild>,
    placement: OverlayPlacement,
    show_caret: bool,
    caret_size: f32,
    /// Accessible name for the dialog node — propagated from the trigger label.
    name: String,
}

impl PopoverSurface {
    fn new(
        content: PendingChild,
        placement: OverlayPlacement,
        show_caret: bool,
        caret_size: f32,
        name: String,
    ) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
            placement,
            show_caret,
            caret_size,
            name,
        }
    }

    /// Which side of the panel rect is attached to the trigger and
    /// should suppress shadow drawing. Derived from `placement` plus
    /// the active layout direction (resolved at paint time):
    /// - `Below*` / `NearAnchor` → anchor sits above ⇒ Top.
    /// - `Above` → anchor sits below ⇒ Bottom.
    /// - `TrailingEdge` → anchor sits on the leading side ⇒ Left in
    ///   LTR, Right in RTL.
    /// - Anything else (Centered, AtPointer, BottomCenter) → not
    ///   visually attached ⇒ no suppression.
    fn attached_shadow_side(
        &self,
        layout_direction: fern_core::environment::LayoutDirection,
    ) -> Option<crate::shadow::AttachedSide> {
        use fern_core::environment::LayoutDirection;
        match self.placement {
            OverlayPlacement::Below
            | OverlayPlacement::BelowPreferred
            | OverlayPlacement::NearAnchor { .. } => Some(crate::shadow::AttachedSide::Top),
            OverlayPlacement::Above => Some(crate::shadow::AttachedSide::Bottom),
            OverlayPlacement::TrailingEdge => match layout_direction {
                LayoutDirection::LeftToRight => Some(crate::shadow::AttachedSide::Left),
                LayoutDirection::RightToLeft => Some(crate::shadow::AttachedSide::Right),
            },
            _ => None,
        }
    }

    fn caret_insets(&self) -> (f32, f32) {
        if !self.show_caret {
            return (0.0, 0.0);
        }

        match self.placement {
            OverlayPlacement::Below
            | OverlayPlacement::BelowPreferred
            | OverlayPlacement::NearAnchor { .. } => (self.caret_size, 0.0),
            OverlayPlacement::Above => (0.0, self.caret_size),
            _ => (0.0, 0.0),
        }
    }

    fn panel_bounds(&self, bounds: Rect) -> Rect {
        let (top, bottom) = self.caret_insets();
        Rect::new(
            bounds.x,
            bounds.y + top,
            bounds.width,
            (bounds.height - top - bottom).max(0.0),
        )
    }

    fn caret_path(&self, bounds: Rect) -> Option<Path> {
        if !self.show_caret {
            return None;
        }

        let panel = self.panel_bounds(bounds);
        let center_x = panel.x + panel.width.min(56.0) / 2.0 + 18.0;
        let half = self.caret_size;
        let mut path = Path::new();

        match self.placement {
            OverlayPlacement::Below
            | OverlayPlacement::BelowPreferred
            | OverlayPlacement::NearAnchor { .. } => {
                path.move_to(Point::new(center_x - half, panel.y));
                path.line_to(Point::new(center_x, bounds.y));
                path.line_to(Point::new(center_x + half, panel.y));
                path.close();
                Some(path)
            }
            OverlayPlacement::Above => {
                let bottom = panel.bottom();
                path.move_to(Point::new(center_x - half, bottom));
                path.line_to(Point::new(center_x, bottom + self.caret_size));
                path.line_to(Point::new(center_x + half, bottom));
                path.close();
                Some(path)
            }
            _ => None,
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
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.children()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let inset = SURFACE_PADDING * 2.0;
        let (caret_top, caret_bottom) = self.caret_insets();
        self.content_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width.map(|width| (width - inset).max(0.0)),
                        height: proposal
                            .height
                            .map(|height| (height - inset - caret_top - caret_bottom).max(0.0)),
                    },
                )
            })
            .map(|size| {
                Size::new(
                    size.width + inset,
                    size.height + inset + caret_top + caret_bottom,
                )
            })
            .unwrap_or_else(|| proposal.resolve(200.0, 80.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let panel = self.panel_bounds(bounds);
        for child in children.iter_mut() {
            child.origin =
                fern_canvas::Point::new(panel.x + SURFACE_PADDING, panel.y + SURFACE_PADDING);
            child.size = Size::new(
                (panel.width - SURFACE_PADDING * 2.0).max(0.0),
                (panel.height - SURFACE_PADDING * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let panel = self.panel_bounds(bounds);
        let radius = CornerRadius::uniform(ctx.theme.shape.radius_popup);
        crate::shadow::paint_layered_shadow(
            canvas,
            panel,
            radius,
            &ctx.theme.shape.shadow_sm,
            &ctx.theme.shape.shadow_inner_sm,
            ctx.theme.components.popover.shadow_density,
            self.attached_shadow_side(ctx.layout_direction),
        );
        // The caret extends into the just-suppressed zone (between
        // panel and trigger). It's painted unshaded below — that's
        // intentional, the caret reads as part of the trigger-attach
        // region, not as a separate elevated surface.
        canvas.fill_rounded_rect(panel, radius, ctx.theme.colors.surface_main);
        canvas.stroke_rounded_rect(
            panel,
            radius,
            ctx.theme.colors.border,
            ctx.theme.shape.border_width,
        );
        if let Some(path) = self.caret_path(bounds) {
            canvas.fill_path(&path, ctx.theme.colors.surface_main);
            canvas.stroke_path(&path, ctx.theme.colors.border, ctx.theme.shape.border_width);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Popover surface is modeled as a non-modal Dialog: ARIA has
        // no dedicated popover role, and Role::Dialog without
        // `set_modal` is the standard fallback for panels that float
        // over other content without blocking it. Every dialog node
        // must have an accessible name; use the trigger's label.
        builder.set_role(fern_core::accesskit::Role::Dialog);
        builder.set_name(&self.name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}

pub struct Popover {
    label: String,
    variant: ButtonVariant,
    enabled: bool,
    placement: OverlayPlacement,
    dismiss: DismissBehavior,
    pending_content: Option<PendingChild>,
    pending_trigger: Option<PendingChild>,
    show_caret: bool,
    caret_size: f32,
    /// When set, the popover requests focus on the given widget
    /// immediately after showing the overlay, so the user can type
    /// without clicking again. Populated by callers that embed a
    /// focusable editor in the content (e.g. filter popover).
    initial_focus_slot: Option<Rc<Cell<Option<WidgetId>>>>,
    root_child_id: Option<WidgetId>,
}

impl Popover {
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            variant: ButtonVariant::Plain,
            enabled: true,
            placement: OverlayPlacement::BelowPreferred,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            pending_content: None,
            pending_trigger: None,
            show_caret: true,
            caret_size: 10.0,
            initial_focus_slot: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
    }

    pub fn content(mut self, content: impl Widget + 'static) -> Self {
        self.pending_content = Some(PendingChild::Deferred(Box::new(content)));
        self
    }

    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.pending_content = Some(PendingChild::Id(id));
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

    pub fn placement(mut self, placement: OverlayPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn dismiss_behavior(mut self, dismiss: DismissBehavior) -> Self {
        self.dismiss = dismiss;
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

    pub fn caret(mut self, show_caret: bool) -> Self {
        self.show_caret = show_caret;
        self
    }

    pub fn caret_size(mut self, caret_size: f32) -> Self {
        self.caret_size = caret_size.max(0.0);
        self
    }

    /// Request focus on a specific widget immediately after the popover
    /// opens. The slot is written by the content widget during `build()`
    /// (same pattern as `ComboBox`'s search-input slot).
    pub fn focus_on_show(mut self, slot: Rc<Cell<Option<WidgetId>>>) -> Self {
        self.initial_focus_slot = Some(slot);
        self
    }
}

impl std::fmt::Debug for Popover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Popover")
            .field("label", &self.label)
            .field("style", &self.variant)
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
        let show_caret = self.show_caret;
        let caret_size = self.caret_size;
        let style = self.variant;
        let initial_focus_slot = self.initial_focus_slot.take();
        // Captured at build time so the open handlers don't need a
        // theme lookup at fire-time. `duration_fast` matches the
        // MotionTokens recommendation for popup fade.
        let fade_duration = if ctx.prefers_reduced_motion() {
            None
        } else {
            Some(ctx.theme().motion.duration_fast)
        };
        let content_id = ctx.add(PopoverSurface::new(
            self.pending_content
                .take()
                .expect("Popover requires .content(...) — no content was set"),
            placement.clone(),
            show_caret,
            caret_size,
            label.clone(),
        ));
        ctx.set_dormant(content_id);

        // Popover-is-open signal drives the trigger's `set_expanded`
        // disclosure state. Each open handler sets it to `true`
        // before showing the overlay; the `on_dismiss` callback
        // installed on every `OverlayRequest` below resets it to
        // `false` when the overlay is dismissed, regardless of
        // which dismiss path fired.
        let is_open: Signal<bool> = ctx.signal(false);
        let dismiss_callback: fern_core::overlay::OverlayDismissCallback = {
            let is_open = is_open.clone();
            Rc::new(move || {
                is_open.set(false);
            })
        };

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            let tap_open = is_open.clone();
            let tap_dismiss = dismiss_callback.clone();
            let tap_focus = initial_focus_slot.clone();
            let key_open = is_open.clone();
            let key_dismiss = dismiss_callback.clone();
            let key_focus = initial_focus_slot.clone();
            let action_open = is_open.clone();
            let action_dismiss = dismiss_callback.clone();
            let action_focus = initial_focus_slot.clone();
            let handlers = fern_core::widget_builder::HandlerSet::new()
                .focusable(true)
                .cursor(fern_core::widget::CursorIcon::Pointer)
                .on_tap({
                    let placement = placement.clone();
                    let dismiss = dismiss.clone();
                    move |_pos, ctx| {
                        if !enabled {
                            return;
                        }
                        ctx.dismiss_all_except_hosts();
                        ctx.activate(content_id);
                        tap_open.set(true);
                        ctx.show_overlay(OverlayRequest {
                            content_id,
                            anchor: self_id,
                            placement: placement.clone(),
                            dismiss: dismiss.clone(),
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                            on_dismiss: Some(tap_dismiss.clone()),
                            fade_duration,
                        });
                        if let Some(id) = tap_focus.as_ref().and_then(|s| s.get()) {
                            ctx.request_focus(id);
                        }
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
                            ctx.dismiss_all_except_hosts();
                            ctx.activate(content_id);
                            key_open.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id,
                                anchor: self_id,
                                placement: placement.clone(),
                                dismiss: dismiss.clone(),
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: Some(key_dismiss.clone()),
                                fade_duration,
                            });
                            if let Some(id) = key_focus.as_ref().and_then(|s| s.get()) {
                                ctx.request_focus(id);
                            }
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                })
                .on_access_action({
                    move |action, ctx| {
                        if action == fern_core::accesskit::Action::Click && enabled {
                            ctx.dismiss_all_except_hosts();
                            ctx.activate(content_id);
                            action_open.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id,
                                anchor: self_id,
                                placement: placement.clone(),
                                dismiss: dismiss.clone(),
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: Some(action_dismiss.clone()),
                                fade_duration,
                            });
                            if let Some(id) = action_focus.as_ref().and_then(|s| s.get()) {
                                ctx.request_focus(id);
                            }
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
            .has_popup(fern_core::accesskit::HasPopup::Dialog)
            .expanded_when(is_open.clone());
            ctx.add(overlay_trigger)
        } else {
            let tap_open = is_open.clone();
            let tap_dismiss = dismiss_callback.clone();
            let tap_focus = initial_focus_slot.clone();
            let key_open = is_open.clone();
            let key_dismiss = dismiss_callback.clone();
            let key_focus = initial_focus_slot.clone();
            let action_open = is_open.clone();
            let action_dismiss = dismiss_callback.clone();
            let action_focus = initial_focus_slot;
            ctx.add(
                Button::new_literal(label)
                    .variant(style)
                    .enabled(enabled)
                    .has_popup(fern_core::accesskit::HasPopup::Dialog)
                    .expanded_when(is_open.clone())
                    .on_tap({
                        let placement = placement.clone();
                        let dismiss = dismiss.clone();
                        move |_pos, ctx| {
                            if !enabled {
                                return;
                            }
                            ctx.dismiss_all_except_hosts();
                            ctx.activate(content_id);
                            tap_open.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id,
                                anchor: self_id,
                                placement: placement.clone(),
                                dismiss: dismiss.clone(),
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: Some(tap_dismiss.clone()),
                                fade_duration,
                            });
                            if let Some(id) = tap_focus.as_ref().and_then(|s| s.get()) {
                                ctx.request_focus(id);
                            }
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
                                ctx.dismiss_all_except_hosts();
                                ctx.activate(content_id);
                                key_open.set(true);
                                ctx.show_overlay(OverlayRequest {
                                    content_id,
                                    anchor: self_id,
                                    placement: placement.clone(),
                                    dismiss: dismiss.clone(),
                                    layer: OverlayLayer::InTree,
                                    parent_overlay: None,
                                    on_dismiss: Some(key_dismiss.clone()),
                                    fade_duration,
                                });
                                if let Some(id) = key_focus.as_ref().and_then(|s| s.get()) {
                                    ctx.request_focus(id);
                                }
                                EventResponse::Handled
                            }
                            _ => EventResponse::Ignored,
                        }
                    })
                    .on_access_action({
                        move |action, ctx| {
                            if action == fern_core::accesskit::Action::Click && enabled {
                                ctx.dismiss_all_except_hosts();
                                ctx.activate(content_id);
                                action_open.set(true);
                                ctx.show_overlay(OverlayRequest {
                                    content_id,
                                    anchor: self_id,
                                    placement: placement.clone(),
                                    dismiss: dismiss.clone(),
                                    layer: OverlayLayer::InTree,
                                    parent_overlay: None,
                                    on_dismiss: Some(action_dismiss.clone()),
                                    fade_duration,
                                });
                                if let Some(id) = action_focus.as_ref().and_then(|s| s.get()) {
                                    ctx.request_focus(id);
                                }
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
    ) -> fern_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(120.0, 40.0))
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
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

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
    fn access_click_opens_popover_overlay() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Popover::new_literal("Show popover").content(FixedLeaf(140.0, 60.0)));
        tree.layout(SizeProposal::exact(480.0, 320.0));

        let trigger = tree.find_by_label("Show popover").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });

        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn escape_dismisses_popover_overlay() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Popover::new_literal("Show popover").content(FixedLeaf(140.0, 60.0)));
        tree.layout(SizeProposal::exact(480.0, 320.0));

        let trigger = tree.find_by_label("Show popover").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        tree.press_key(Key::Escape, fern_core::event::Modifiers::NONE);

        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn popover_trigger_tracks_expanded_across_dismiss_paths() {
        // Regression guard: the Popover button reports
        // set_expanded(true) while its panel is shown and
        // set_expanded(false) after it's dismissed — including
        // framework-level dismiss paths (via on_dismiss callback).
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Popover::new_literal("Show popover").content(FixedLeaf(140.0, 60.0)));
        tree.layout(SizeProposal::exact(480.0, 320.0));
        let trigger = tree.find_by_label("Show popover").unwrap();

        assert!(!tree.accessibility_node(trigger).is_expanded());

        // Open via Click action.
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        tree.layout(SizeProposal::exact(480.0, 320.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(
            tree.accessibility_node(trigger).is_expanded(),
            "open popover should report expanded=true"
        );

        // Dismiss via the framework path (bypasses trigger handlers).
        let overlay_id = tree
            .active_overlays()
            .first()
            .copied()
            .expect("popover overlay active");
        tree.dismiss_overlay(overlay_id);
        tree.layout(SizeProposal::exact(480.0, 320.0));
        assert!(
            !tree.accessibility_node(trigger).is_expanded(),
            "framework dismiss must reset popover expanded=false"
        );
    }

    #[test]
    fn custom_trigger_opens_popover_overlay() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(
            Popover::new_literal("Show popover")
                .content(FixedLeaf(140.0, 60.0))
                .trigger(FixedLeaf(128.0, 36.0)),
        );
        tree.layout(SizeProposal::exact(480.0, 320.0));

        let trigger = tree.find_by_label("Show popover").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });

        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn caret_increases_popover_height_for_below_placement() {
        let mut plain_tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        plain_tree.add(
            Popover::new_literal("Show popover")
                .content(FixedLeaf(140.0, 60.0))
                .placement(OverlayPlacement::Below)
                .caret(false),
        );
        plain_tree.layout(SizeProposal::exact(480.0, 320.0));
        let trigger = plain_tree.find_by_label("Show popover").unwrap();
        plain_tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        plain_tree.layout(SizeProposal::exact(480.0, 320.0));
        let plain_bounds = plain_tree.bounds(plain_tree.overlay_manager().active_content_ids()[0]);

        let mut caret_tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        caret_tree.add(
            Popover::new_literal("Show popover")
                .content(FixedLeaf(140.0, 60.0))
                .placement(OverlayPlacement::Below)
                .caret_size(12.0),
        );
        caret_tree.layout(SizeProposal::exact(480.0, 320.0));
        let trigger = caret_tree.find_by_label("Show popover").unwrap();
        caret_tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        caret_tree.layout(SizeProposal::exact(480.0, 320.0));
        let caret_bounds = caret_tree.bounds(caret_tree.overlay_manager().active_content_ids()[0]);

        assert!(caret_bounds.height >= plain_bounds.height + 11.0);
    }

    #[test]
    #[should_panic(expected = "Popover requires .content(...)")]
    fn popover_without_content_panics_on_build() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Popover::new_literal("Show popover"));
        tree.layout(SizeProposal::exact(480.0, 320.0));
    }
}
