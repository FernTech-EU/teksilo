// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Snackbar — a transient, button-triggered floating notification surface.
//!
//! A `Snackbar` pairs a trigger (a `Button` by default, or any custom
//! widget via `.trigger(...)`) with a dormant content surface. Activating
//! the trigger presents the surface as an `OverlayPlacement::BottomCenter`
//! overlay and dismisses it automatically after a configurable timeout
//! (default: 4 s). The surface stays until dismissed when `.persistent()`
//! is set. Only one snackbar can be shown at a time — presenting a second
//! one dismisses the first.
//!
//! For richer, stackable, severity-aware notifications see the
//! [`Toast`](crate::toast::Toast) system, which also maintains a
//! persistent `NotificationArchiveModel`.
//!
//! ## Accessibility
//!
//! The content surface exposes `Role::Alert` with `Live::Polite` so
//! screen readers announce the notification without interrupting the user.
//! Supply `.announcement(...)` to give the alert a descriptive name
//! instead of the generic "notification" fallback.
//!
//! ```ignore
//! use bastyde_widgets::{Snackbar};
//! use bastyde_i18n::lit;
//! use bastyde_widgets::primitives::TextWidget;
//! use bastyde_tokens::TextRole;
//!
//! // In build():
//! ctx.add(
//!     Snackbar::new(lit!("Undo"))
//!         .content(TextWidget::new(lit!("File deleted.")).color(TextRole::TooltipText))
//!         .announcement(lit!("File deleted."))
//!         .auto_dismiss_after(std::time::Duration::from_secs(5)),
//! );
//! ```

use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::styles::{SharedSnackbarStyle, SnackbarStyleConfig};
use bastyde_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use crate::button::{Button, ButtonVariant};
use crate::overlay_trigger::OverlayTrigger;
use bastyde_i18n::LocalizedString;

const DEFAULT_AUTO_DISMISS: Duration = Duration::from_secs(4);

fn present_snackbar(
    ctx: &mut bastyde_core::widget::EventContext,
    anchor: WidgetId,
    content_id: WidgetId,
    dismiss: DismissBehavior,
    auto_dismiss_after: Option<Duration>,
    fade_duration: Option<Duration>,
) {
    ctx.dismiss_all_except_hosts();
    ctx.activate(content_id);
    let request = OverlayRequest {
        content_id,
        anchor,
        placement: OverlayPlacement::BottomCenter,
        dismiss,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: None,
        fade_duration,
    };
    if let Some(duration) = auto_dismiss_after {
        ctx.show_overlay_for(request, duration);
    } else {
        ctx.show_overlay(request);
    }
}

struct SnackbarSurface {
    content_id: Option<WidgetId>,
    pending_content: Option<PendingChild>,
    /// Optional explicit SR announcement string. When set,
    /// `accessibility()` uses it as the Alert's accessible name
    /// so screen readers read out the caller-provided message
    /// the moment the snackbar appears. Falls back to the
    /// generic `a11y_snackbar_name` when unset.
    announcement: Option<LocalizedString>,
    /// Per-call override for the snackbar surface chrome.
    style_override: Option<SharedSnackbarStyle>,
    /// Build state — the `SnackbarStyle::make_body` root.
    root_child_id: Option<WidgetId>,
}

impl SnackbarSurface {
    fn new(content: PendingChild) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
            announcement: None,
            style_override: None,
            root_child_id: None,
        }
    }

    fn with_announcement(mut self, text: Option<LocalizedString>) -> Self {
        self.announcement = text;
        self
    }

    fn with_style(mut self, style: Option<SharedSnackbarStyle>) -> Self {
        self.style_override = style;
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
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // The surface chrome (dark `tooltip_bg` panel + border + padding
        // inset) is owned by the active `SnackbarStyle`; this widget
        // keeps its `Role::Alert` / `Live::Polite` accessibility node.
        let content_id = self
            .content_id
            .expect("SnackbarSurface requires content — none was set");
        let style: SharedSnackbarStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.snackbar.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeSnackbarStyle::default()));
        let root_id = style.make_body(
            &SnackbarStyleConfig {
                content: content_id,
            },
            ctx,
        );
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
            .unwrap_or_else(|| proposal.resolve(220.0, 44.0))
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
        // Role::Alert + Live::Polite mirrors the ARIA pattern for
        // transient notifications: screen readers announce the
        // contents when the snackbar appears, without interrupting
        // the user's current action. The accessible name is the
        // caller-supplied announcement when present, otherwise
        // the generic fallback. Child widgets still contribute
        // their own nodes for full context.
        builder.set_role(bastyde_core::accesskit::Role::Alert);
        builder.set_live(bastyde_core::accesskit::Live::Polite);
        let name = self
            .announcement
            .as_ref()
            .map(|a| a.resolve_now())
            .unwrap_or_else(|| bastyde_i18n::tr_widget!(a11y_snackbar_name()).resolve_now());
        builder.set_name(name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// A button-triggered transient notification surface.
///
/// Call `.content(...)` to supply the notification body, then add the
/// widget to the tree. The trigger label is shown as a `Button` (or a
/// custom widget via `.trigger(...)`); activating it presents the
/// content surface at the bottom center of the window.
pub struct Snackbar {
    label: LocalizedString,
    variant: ButtonVariant,
    enabled: bool,
    dismiss: DismissBehavior,
    auto_dismiss_after: Option<Duration>,
    pending_content: Option<PendingChild>,
    pending_trigger: Option<PendingChild>,
    /// Optional explicit announcement string threaded through to
    /// the `SnackbarSurface`'s a11y node. When set, screen readers
    /// read this as the Alert's name when the snackbar appears.
    announcement: Option<LocalizedString>,
    /// Per-call override for the snackbar surface chrome.
    style_override: Option<SharedSnackbarStyle>,
    root_child_id: Option<WidgetId>,
}

impl Snackbar {
    /// Create a snackbar whose default trigger button shows `label`.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            variant: ButtonVariant::Plain,
            enabled: true,
            dismiss: DismissBehavior::ClickOutside,
            auto_dismiss_after: Some(DEFAULT_AUTO_DISMISS),
            pending_content: None,
            pending_trigger: None,
            announcement: None,
            style_override: None,
            root_child_id: None,
        }
    }

    /// Per-call style override for the snackbar surface chrome.
    /// Replaces the theme-wide default `SnackbarStyle` for just this
    /// instance.
    pub fn style(mut self, style: impl bastyde_core::styles::SnackbarStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// The snackbar body — the message (and optional inline action)
    /// shown on the floating surface.
    ///
    /// The default surface is the high-contrast (dark) `tooltip_bg`,
    /// the same one tooltips use, and it stays dark in light theme.
    /// So any `TextWidget` you pass here must set
    /// `.color(TextRole::TooltipText)` (and actions can use
    /// `TooltipText` / `TooltipShortcut`) — the default `TextRole::Primary`
    /// is dark and renders nearly invisible on the dark surface in light
    /// theme. If you install a light-surface `SnackbarStyle`, color the
    /// content to match that instead.
    pub fn content(mut self, content: impl Widget + 'static) -> Self {
        self.pending_content = Some(PendingChild::Deferred(Box::new(content)));
        self
    }

    /// Supply the notification body by `WidgetId` (already added to the
    /// tree). Mutually exclusive with `.content(...)`.
    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.pending_content = Some(PendingChild::Id(id));
        self
    }

    /// Override the default trigger [`ButtonVariant`] (default: `Plain`).
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the initial enabled state of the trigger.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Override the overlay dismiss behavior (default: `ClickOutside`).
    pub fn dismiss_behavior(mut self, dismiss: DismissBehavior) -> Self {
        self.dismiss = dismiss;
        self
    }

    /// Set the auto-dismiss timeout. The overlay is removed after this
    /// duration without user interaction (default: 4 s).
    pub fn auto_dismiss_after(mut self, duration: Duration) -> Self {
        self.auto_dismiss_after = Some(duration);
        self
    }

    /// Keep the snackbar visible until explicitly dismissed; disables
    /// the auto-dismiss timeout.
    pub fn persistent(mut self) -> Self {
        self.auto_dismiss_after = None;
        self
    }

    /// Replace the default `Button` trigger with a custom widget. The
    /// widget is wired for tap, keyboard (Enter/Space), and AT Click
    /// activation automatically.
    pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self {
        self.pending_trigger = Some(PendingChild::Deferred(Box::new(trigger)));
        self
    }

    /// Supply the custom trigger by `WidgetId` (already added to the tree).
    pub fn trigger_id(mut self, id: WidgetId) -> Self {
        self.pending_trigger = Some(PendingChild::Id(id));
        self
    }

    /// Screen-reader announcement string — used as the Alert's
    /// accessible name when the snackbar appears. Without this
    /// the surface falls back to the generic `a11y_snackbar_name`
    /// i18n string, which says "notification" but can't describe
    /// the specific message. Set this whenever the snackbar
    /// conveys information the user needs to hear (errors,
    /// confirmations, status changes).
    pub fn announcement(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.announcement = Some(ls);
        self
    }
}

impl std::fmt::Debug for Snackbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snackbar")
            .field("label", &self.label)
            .field("style", &self.variant)
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
        let style = self.variant;
        // Captured at build time so the present-snackbar handlers
        // don't need a theme lookup at fire-time. `duration_normal`
        // matches the snackbar's typical "notification slide"
        // recommendation in MotionTokens.
        let fade_duration = if ctx.prefers_reduced_motion() {
            None
        } else {
            Some(ctx.theme().motion.duration_normal)
        };
        let content_id = ctx.add(
            SnackbarSurface::new(
                self.pending_content
                    .take()
                    .expect("Snackbar requires .content(...) — no content was set"),
            )
            .with_announcement(self.announcement.clone())
            .with_style(self.style_override.clone()),
        );
        ctx.set_dormant(content_id);

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            // A custom trigger is an arbitrary widget with no built-in
            // activation, so we wire pointer / keyboard / AT activation
            // by hand. (The default-Button branch below delegates all
            // three to `Button::on_activate_fn`.)
            let open_on_tap = {
                let dismiss = dismiss.clone();
                move |_event: &bastyde_core::TapEvent,
                      ctx: &mut bastyde_core::widget::EventContext| {
                    if !enabled {
                        return;
                    }
                    present_snackbar(
                        ctx,
                        self_id,
                        content_id,
                        dismiss.clone(),
                        auto_dismiss_after,
                        fade_duration,
                    );
                }
            };
            let handlers = bastyde_core::widget_builder::HandlerSet::new()
                .focusable(true)
                .cursor(bastyde_core::widget::CursorIcon::Pointer)
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
                                fade_duration,
                            );
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                })
                .on_access_action({
                    move |action, ctx| {
                        if action == bastyde_core::accesskit::Action::Click && enabled {
                            present_snackbar(
                                ctx,
                                self_id,
                                content_id,
                                dismiss.clone(),
                                auto_dismiss_after,
                                fade_duration,
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
            // `Button::on_activate_fn` already fires on pointer tap,
            // Space/Enter (with the matched-KeyDown guard), and AccessKit
            // Click — so one handler covers all three activation paths.
            ctx.add(
                Button::new(label)
                    .variant(style)
                    .enabled(enabled)
                    .on_activate_fn(move |ctx| {
                        present_snackbar(
                            ctx,
                            self_id,
                            content_id,
                            dismiss.clone(),
                            auto_dismiss_after,
                            fade_duration,
                        );
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
        // The outer Snackbar widget is just a layout shell around the
        // focusable trigger (Button or OverlayTrigger). Hiding it from
        // the platform a11y tree prevents a dead GenericContainer node
        // from sitting between the trigger and its ancestors.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
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
    fn access_click_opens_bottom_center_snackbar() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Snackbar::new(lit!("Show snackbar")).content(FixedLeaf(220.0, 40.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Show snackbar").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert_eq!(tree.active_overlays().len(), 1);
        let content_id = tree.overlay_manager().active_content_ids()[0];
        let bounds = tree.bounds(content_id);
        let expected_x = (800.0 - bounds.width) / 2.0;
        assert!((bounds.x - expected_x).abs() < 1.0);
        assert!((bounds.y + bounds.height - (600.0 - 24.0)).abs() < 1.0);
    }

    #[test]
    fn default_button_keyboard_activation_opens_snackbar() {
        // The default-Button branch delegates all activation to
        // `Button::on_activate_fn`, so a matched KeyDown + KeyUp pair on
        // the focused trigger must present the snackbar — and inherits
        // Button's lone-KeyUp guard for free.
        use bastyde_core::event::Modifiers;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Snackbar::new(lit!("Show snackbar")).content(FixedLeaf(220.0, 40.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Show snackbar").unwrap();
        tree.focus(trigger);

        // A lone KeyUp (no matching KeyDown) must not activate.
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert!(tree.active_overlays().is_empty());

        // A matched KeyDown + KeyUp pair presents the snackbar.
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn custom_trigger_opens_snackbar() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            Snackbar::new(lit!("Show snackbar"))
                .content(FixedLeaf(180.0, 36.0))
                .trigger(FixedLeaf(132.0, 36.0)),
        );
        tree.layout(SizeProposal::exact(640.0, 480.0));

        // OverlayTrigger now routes handlers onto the trigger child;
        // a pointer click on the wrapper hit-tests into the child where
        // the handler lives.
        let trigger = tree.find_by_label("Show snackbar").unwrap();
        tree.click(trigger);

        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn snackbar_auto_dismisses_after_duration() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            Snackbar::new(lit!("Show snackbar"))
                .content(FixedLeaf(220.0, 40.0))
                .auto_dismiss_after(Duration::from_millis(300)),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Show snackbar").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });
        assert_eq!(tree.active_overlays().len(), 1);

        tree.advance_time(Duration::from_millis(200));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.advance_time(Duration::from_millis(150));
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    #[should_panic(expected = "Snackbar requires .content(...)")]
    fn snackbar_without_content_panics_on_build() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Snackbar::new(lit!("Show snackbar")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
    }
}
