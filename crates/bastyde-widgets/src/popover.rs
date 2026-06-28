// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `Popover` — a button that opens a floating panel anchored to itself.
//!
//! `Popover` is the legacy one-type-does-everything disclosure widget: it
//! pairs a labelled [`Button`] trigger (or any custom trigger supplied via
//! `.trigger(...)`) with a themed popover surface and the full overlay
//! wiring (dormant pre-build, `activate` on open, `show_overlay`, dismiss
//! callback). For the more ergonomic generic form that works with both
//! `Button` and `IconButton` triggers see
//! [`PopoverWidget`](crate::popover_widget::PopoverWidget) /
//! [`PopoverButton`](crate::popover_widget::PopoverButton) /
//! [`PopoverIconButton`](crate::popover_widget::PopoverIconButton).
//!
//! ## Accessibility
//!
//! The trigger announces `HasPopup::Dialog` and tracks open/closed state
//! via `set_expanded`. The popover surface carries `Role::Dialog` named
//! after the trigger label.
//!
//! ```rust
//! # use bastyde_widgets::popover::Popover;
//! # use bastyde_widgets::primitives::TextWidget;
//! # use bastyde_i18n::lit;
//! let _w = Popover::new(lit!("Choose…"))
//!     .content(TextWidget::new(lit!("Pick an option")));
//! ```

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, EdgeInsets, Path, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, SurfaceRole};

use crate::button::{Button, ButtonVariant};
use crate::overlay_trigger::OverlayTrigger;
use bastyde_i18n::LocalizedString;

pub(crate) struct PopoverSurface {
    content_id: Option<WidgetId>,
    pending_content: Option<PendingChild>,
    placement: OverlayPlacement,
    show_caret: bool,
    caret_size: f32,
    /// Accessible name for the dialog node — propagated from the trigger label.
    name: String,
    /// Inset between the panel edge and the wrapped content. Defaulted
    /// per `PopoverVariant` by `RecipePopoverStyle` (16 px for
    /// Default/Tooltip, zero for Menu so menu rows reach the edge).
    content_padding: EdgeInsets,
    /// Surface fill role for the panel background + caret.
    background: SurfaceRole,
    /// Panel corner radius in logical pixels.
    corner_radius: f32,
    /// When true the surface emits no semantic node (`set_hidden`) —
    /// used by the Menu variant, where the caller (`MenuList`,
    /// `DropdownPanel`, `SuggestionListBox`) already carries the
    /// container role. Default/Tooltip surfaces emit `Role::Dialog`.
    presentational: bool,
}

impl PopoverSurface {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        content: PendingChild,
        placement: OverlayPlacement,
        show_caret: bool,
        caret_size: f32,
        name: String,
        content_padding: EdgeInsets,
        background: SurfaceRole,
        corner_radius: f32,
        presentational: bool,
    ) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
            placement,
            show_caret,
            caret_size,
            name,
            content_padding,
            background,
            corner_radius,
            presentational,
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
        layout_direction: bastyde_core::environment::LayoutDirection,
    ) -> Option<crate::shadow::AttachedSide> {
        use bastyde_core::environment::LayoutDirection;
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
    ) -> bastyde_core::widget::LayoutResponse {
        let inset_w = self.content_padding.leading + self.content_padding.trailing;
        let inset_h = self.content_padding.top + self.content_padding.bottom;
        let (caret_top, caret_bottom) = self.caret_insets();
        self.content_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width.map(|width| (width - inset_w).max(0.0)),
                        height: proposal
                            .height
                            .map(|height| (height - inset_h - caret_top - caret_bottom).max(0.0)),
                    },
                )
            })
            .map(|size| {
                Size::new(
                    size.width + inset_w,
                    size.height + inset_h + caret_top + caret_bottom,
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
        let pad = self.content_padding;
        for child in children.iter_mut() {
            child.origin = bastyde_canvas::Point::new(panel.x + pad.leading, panel.y + pad.top);
            child.size = Size::new(
                (panel.width - pad.leading - pad.trailing).max(0.0),
                (panel.height - pad.top - pad.bottom).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let panel = self.panel_bounds(bounds);
        let radius = CornerRadius::uniform(self.corner_radius);
        let fill = self.background.resolve(&ctx.theme.colors);
        crate::shadow::paint_layered_shadow(
            canvas,
            panel,
            radius,
            &ctx.theme.shape.shadow_sm,
            &ctx.theme.shape.shadow_inner_sm,
            crate::styles::recipe_popover_style::POPOVER_SHADOW_DENSITY,
            self.attached_shadow_side(ctx.layout_direction),
        );
        // The caret extends into the just-suppressed zone (between
        // panel and trigger). It's painted unshaded below — that's
        // intentional, the caret reads as part of the trigger-attach
        // region, not as a separate elevated surface.
        canvas.fill_rounded_rect(panel, radius, fill);
        canvas.stroke_rounded_rect(
            panel,
            radius,
            ctx.theme.colors.border,
            ctx.theme.shape.border_width,
        );
        if let Some(path) = self.caret_path(bounds) {
            canvas.fill_path(&path, fill);
            canvas.stroke_path(&path, ctx.theme.colors.border, ctx.theme.shape.border_width);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.presentational {
            // Menu-variant container: the caller (`MenuList`,
            // `DropdownPanel`, `SuggestionListBox`) already owns the
            // semantic role, so the surface contributes nothing.
            builder.set_hidden();
            return;
        }
        // Popover surface is modeled as a non-modal Dialog: ARIA has
        // no dedicated popover role, and Role::Dialog without
        // `set_modal` is the standard fallback for panels that float
        // over other content without blocking it. Every dialog node
        // must have an accessible name; use the trigger's label.
        builder.set_role(bastyde_core::accesskit::Role::Dialog);
        builder.set_name(&self.name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}

/// Labelled button that opens a floating popover panel. See the [module docs](self).
pub struct Popover {
    label: LocalizedString,
    variant: ButtonVariant,
    enabled: bool,
    placement: OverlayPlacement,
    dismiss: DismissBehavior,
    pending_content: Option<PendingChild>,
    pending_trigger: Option<PendingChild>,
    show_caret: bool,
    caret_size: f32,
    /// Visual variant of the popover surface. Default `Default`.
    /// Distinct from the trigger-button's `ButtonVariant` (which lives
    /// in `self.variant` for legacy reasons; we'd rename it to
    /// `trigger_variant` if breakage budget allowed).
    surface_variant: bastyde_core::styles::PopoverVariant,
    /// Per-call override for the popover surface chrome. Replaces the
    /// theme-wide `style_slots.popover` and the IntUI default
    /// `RecipePopoverStyle` for just this Popover instance.
    style_override: Option<bastyde_core::styles::SharedPopoverStyle>,
    /// When set, the popover requests focus on the given widget
    /// immediately after showing the overlay, so the user can type
    /// without clicking again. Populated by callers that embed a
    /// focusable editor in the content (e.g. filter popover).
    initial_focus_slot: Option<Rc<Cell<Option<WidgetId>>>>,
    root_child_id: Option<WidgetId>,
    /// Popover surface root produced by `make_body`. Stored so the
    /// widget can re-export it in [`Widget::children`] — linking the
    /// dormant content as a child of `Popover` is what lets
    /// `arena.hit_test_at` skip the entire popover subtree when the
    /// popover is closed (the framework prunes dormant children
    /// automatically). Without this, the content survives as an
    /// orphan root in the arena and absorbs every click that lands
    /// inside the trigger's bounds.
    content_id: Option<WidgetId>,
}

impl Popover {
    /// Construct a popover with the given trigger-button label. Supply content via
    /// [`.content(...)`](Self::content) before mounting.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            variant: ButtonVariant::Plain,
            enabled: true,
            placement: OverlayPlacement::BelowPreferred,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            pending_content: None,
            pending_trigger: None,
            show_caret: true,
            caret_size: 10.0,
            surface_variant: bastyde_core::styles::PopoverVariant::default(),
            style_override: None,
            initial_focus_slot: None,
            root_child_id: None,
            content_id: None,
        }
    }

    /// Pick the popover surface's design-language variant. Default
    /// `Default`. The active `PopoverStyle` decides what each variant
    /// means (the IntUI default ships one chrome shape across all
    /// variants and lets the inner content distinguish them; custom
    /// styles can branch on the variant for distinct surfaces).
    pub fn surface_variant(mut self, variant: bastyde_core::styles::PopoverVariant) -> Self {
        self.surface_variant = variant;
        self
    }

    /// Per-call style override for the popover surface chrome.
    /// Replaces the theme-wide default `PopoverStyle` for just this
    /// Popover instance.
    pub fn style(mut self, style: impl bastyde_core::styles::PopoverStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Set the popover body widget (required). Built as a dormant subtree during
    /// `build()` and woken when the trigger is activated.
    pub fn content(mut self, content: impl Widget + 'static) -> Self {
        self.pending_content = Some(PendingChild::Deferred(Box::new(content)));
        self
    }

    /// Set the popover body by pre-registered [`WidgetId`].
    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.pending_content = Some(PendingChild::Id(id));
        self
    }

    /// Set the [`ButtonVariant`] used for the built-in text trigger. Default `Plain`.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Enable or disable the trigger button. Default `true`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the [`OverlayPlacement`] of the popover surface. Default
    /// [`OverlayPlacement::BelowPreferred`].
    pub fn placement(mut self, placement: OverlayPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Override the dismiss gesture. Default
    /// [`DismissBehavior::EscapeOrClickOutside`].
    pub fn dismiss_behavior(mut self, dismiss: DismissBehavior) -> Self {
        self.dismiss = dismiss;
        self
    }

    /// Replace the built-in text `Button` with a custom trigger widget. The
    /// custom trigger is wrapped in overlay machinery (focusable, tap / key /
    /// AT-click open the panel) via an internal `OverlayTrigger`.
    pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self {
        self.pending_trigger = Some(PendingChild::Deferred(Box::new(trigger)));
        self
    }

    /// Set a custom trigger by pre-registered [`WidgetId`].
    pub fn trigger_id(mut self, id: WidgetId) -> Self {
        self.pending_trigger = Some(PendingChild::Id(id));
        self
    }

    /// Show or hide the pointing caret between the popover panel and the
    /// trigger. Default `true`.
    pub fn caret(mut self, show_caret: bool) -> Self {
        self.show_caret = show_caret;
        self
    }

    /// Override the caret size in logical pixels (clamped to `0`). Default `10`.
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
        // Materialize the inner content first so the style sees a
        // ready WidgetId. The content was either inline-deferred or
        // pre-registered by id; either way we land on a single id.
        let inner_content_id = match self
            .pending_content
            .take()
            .expect("Popover requires .content(...) — no content was set")
        {
            PendingChild::Id(id) => id,
            PendingChild::Deferred(w) => ctx.add_boxed(w),
        };

        // Resolve the active surface style: per-call > theme slot >
        // built-in `RecipePopoverStyle` default.
        let surface_style: bastyde_core::styles::SharedPopoverStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.popover.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipePopoverStyle));
        let surface_cfg = bastyde_core::styles::PopoverStyleConfig {
            content: inner_content_id,
            variant: self.surface_variant,
            name: label.resolve_now(),
            placement: placement.clone(),
            show_caret,
            caret_size,
        };
        let content_id = surface_style.make_body(&surface_cfg, ctx);
        ctx.set_dormant(content_id);
        self.content_id = Some(content_id);

        // Popover-is-open signal drives the trigger's `set_expanded`
        // disclosure state. Each open handler sets it to `true`
        // before showing the overlay; the `on_dismiss` callback
        // installed on every `OverlayRequest` below resets it to
        // `false` when the overlay is dismissed, regardless of
        // which dismiss path fired.
        let is_open: Signal<bool> = ctx.signal(false);
        let dismiss_callback: bastyde_core::overlay::OverlayDismissCallback = {
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
            let handlers = bastyde_core::widget_builder::HandlerSet::new()
                .focusable(true)
                .cursor(bastyde_core::widget::CursorIcon::Pointer)
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
                        if action == bastyde_core::accesskit::Action::Click && enabled {
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
            .has_popup(bastyde_core::accesskit::HasPopup::Dialog)
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
                Button::new(label)
                    .variant(style)
                    .enabled(enabled)
                    .has_popup(bastyde_core::accesskit::HasPopup::Dialog)
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
                            if action == bastyde_core::accesskit::Action::Click && enabled {
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
        // Return BOTH the trigger root AND the dormant popover content
        // as children so the framework links `content_id` under
        // `Popover` in the arena. Without this, the content survives as
        // an orphan root and `arena.hit_test_at` walks its subtree on
        // every click (descendants of an orphan root keep their
        // pre-dormant bounds and absorb hits inside the trigger). The
        // framework's layout pass skips dormant children automatically.
        vec![root_id, content_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        // Trigger fills our bounds; the popover content (active or
        // dormant) is positioned by the overlay manager via
        // `position_overlays`, not by us. Dormant children are
        // already filtered out before placements reach this fn; when
        // active, zero out the content's placement so our trigger
        // bounds don't drive a layout pass that would clobber the
        // overlay positioning.
        for child in children.iter_mut() {
            if Some(child.id) == self.content_id {
                child.size = Size::ZERO;
                continue;
            }
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        // Include both the trigger root AND the popover content so
        // `set_dormant` cascades correctly and `arena.hit_test_at`
        // prunes the content subtree when dormant.
        let mut out = Vec::new();
        if let Some(id) = self.root_child_id {
            out.push(id);
        }
        if let Some(id) = self.content_id {
            out.push(id);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn access_click_opens_popover_overlay() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Popover::new(lit!("Show popover")).content(FixedLeaf(140.0, 60.0)));
        tree.layout(SizeProposal::exact(480.0, 320.0));

        let trigger = tree.find_by_label("Show popover").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });

        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn escape_dismisses_popover_overlay() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Popover::new(lit!("Show popover")).content(FixedLeaf(140.0, 60.0)));
        tree.layout(SizeProposal::exact(480.0, 320.0));

        let trigger = tree.find_by_label("Show popover").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });
        tree.press_key(Key::Escape, bastyde_core::event::Modifiers::NONE);

        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn popover_trigger_tracks_expanded_across_dismiss_paths() {
        // Regression guard: the Popover button reports
        // set_expanded(true) while its panel is shown and
        // set_expanded(false) after it's dismissed — including
        // framework-level dismiss paths (via on_dismiss callback).
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Popover::new(lit!("Show popover")).content(FixedLeaf(140.0, 60.0)));
        tree.layout(SizeProposal::exact(480.0, 320.0));
        let trigger = tree.find_by_label("Show popover").unwrap();

        assert!(!tree.accessibility_node(trigger).is_expanded());

        // Open via Click action.
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            Popover::new(lit!("Show popover"))
                .content(FixedLeaf(140.0, 60.0))
                .trigger(FixedLeaf(128.0, 36.0)),
        );
        tree.layout(SizeProposal::exact(480.0, 320.0));

        // OverlayTrigger now routes handlers onto the trigger child;
        // a pointer click on the wrapper hit-tests into the child where
        // the handler lives.
        let trigger = tree.find_by_label("Show popover").unwrap();
        tree.click(trigger);

        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn caret_increases_popover_height_for_below_placement() {
        let mut plain_tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        plain_tree.add(
            Popover::new(lit!("Show popover"))
                .content(FixedLeaf(140.0, 60.0))
                .placement(OverlayPlacement::Below)
                .caret(false),
        );
        plain_tree.layout(SizeProposal::exact(480.0, 320.0));
        let trigger = plain_tree.find_by_label("Show popover").unwrap();
        plain_tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });
        plain_tree.layout(SizeProposal::exact(480.0, 320.0));
        let plain_bounds = plain_tree.bounds(plain_tree.overlay_manager().active_content_ids()[0]);

        let mut caret_tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        caret_tree.add(
            Popover::new(lit!("Show popover"))
                .content(FixedLeaf(140.0, 60.0))
                .placement(OverlayPlacement::Below)
                .caret_size(12.0),
        );
        caret_tree.layout(SizeProposal::exact(480.0, 320.0));
        let trigger = caret_tree.find_by_label("Show popover").unwrap();
        caret_tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });
        caret_tree.layout(SizeProposal::exact(480.0, 320.0));
        let caret_bounds = caret_tree.bounds(caret_tree.overlay_manager().active_content_ids()[0]);

        assert!(caret_bounds.height >= plain_bounds.height + 11.0);
    }

    #[test]
    #[should_panic(expected = "Popover requires .content(...)")]
    fn popover_without_content_panics_on_build() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Popover::new(lit!("Show popover")));
        tree.layout(SizeProposal::exact(480.0, 320.0));
    }
}
