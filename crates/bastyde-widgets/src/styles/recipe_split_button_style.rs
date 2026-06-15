// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `SplitButtonStyle` impl for `SplitButton`.
//!
//! Owns the shared frame chrome (background fill, border, corner radius,
//! overall min size) and wraps the pre-built interactive `content` row
//! handed in via [`SplitButtonStyleConfig`]. Mirrors `RecipeButtonStyle`:
//! the widget keeps text-colour resolution and event wiring; the style only
//! frames the content.
//!
//! Background / border roles are resolved from the live interaction state
//! exactly as the widget did before the Tier-3 migration, so the default
//! render is unchanged. Disabled appearance is handled by the leaves' own
//! paint (`effective_enabled`), so the frame is intentionally *not* dimmed
//! here — `cfg.is_disabled` is available for custom styles that want to.

use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{SplitButtonStyle, SplitButtonStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::button::{ButtonVariant, InteractionState};
use crate::primitives::{MinSize, RectWidget, ZStack};
use crate::split_button::{
    SPLIT_BUTTON_BORDER_WIDTH, SPLIT_BUTTON_CHEVRON_WIDTH, SPLIT_BUTTON_CORNER_RADIUS,
    SPLIT_BUTTON_DIVIDER_WIDTH, SPLIT_BUTTON_HEIGHT, SPLIT_BUTTON_MIN_WIDTH, SplitButtonFamily,
    classify,
};

/// IntUI default `SplitButtonStyle`. Resolves the frame background / border
/// from the variant × interaction state. Apps retheme by installing a custom
/// impl per-call (`SplitButton::style(...)`) or theme-wide
/// (`theme.style_slots.split_button = Some(Rc::new(...))`).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSplitButtonStyle;

impl SplitButtonStyle for RecipeSplitButtonStyle {
    fn make_body(&self, cfg: &SplitButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let variant = cfg.variant;

        // Collapse the live interaction bools back into the single state the
        // role tables key on. Priority mirrors the widget's `interaction`
        // enum (which carries exactly one of these at a time): pressed >
        // focused > hovered > idle. `is_disabled` is deliberately not folded
        // in — the frame doesn't dim on disable (leaves handle that).
        let state: Signal<InteractionState> = cfg
            .is_pressed
            .zip3(&cfg.is_focused, &cfg.is_hovered)
            .map(|(pressed, focused, hovered)| {
                if *pressed {
                    InteractionState::Pressed
                } else if *focused {
                    InteractionState::Focused
                } else if *hovered {
                    InteractionState::Hovered
                } else {
                    InteractionState::Idle
                }
            });

        let bg_role = state.map(move |s| resolve_bg_role(variant, *s));
        let border_role = state.map(move |s| resolve_border_role(variant, *s));

        let normal_bw = SPLIT_BUTTON_BORDER_WIDTH;
        let focus_bw = ctx.theme().shape.focus_ring_width;
        let border_width =
            state.map(move |s| resolve_border_width(variant, *s, normal_bw, focus_bw));

        // Shared frame (single RectWidget behind the content row).
        let bg_id = ctx.add(
            RectWidget::new()
                .bind_background(bg_role)
                .bind_border_color(border_role)
                .bind_border_width(border_width)
                .corner_radius(CornerRadius::uniform(SPLIT_BUTTON_CORNER_RADIUS)),
        );

        let frame_id = ctx.add(ZStack::new().add_child(bg_id).add_child(cfg.content));

        // Enforce the overall minimum: main min_width + divider + chevron.
        let total_min_width =
            SPLIT_BUTTON_MIN_WIDTH + SPLIT_BUTTON_DIVIDER_WIDTH + SPLIT_BUTTON_CHEVRON_WIDTH;
        ctx.add(MinSize::new(total_min_width, SPLIT_BUTTON_HEIGHT).child_id(frame_id))
    }
}

// --- Color resolution (variant × state × theme) ---
//
// Mirrors `Button::resolve_bg` / `resolve_border` so a Button and a
// SplitButton with the same variant look identical. The `classify` bucketing
// is shared with the widget's `resolve_text_role` (it lives in `split_button`
// so text and frame stay in lockstep).

fn resolve_bg_role(variant: ButtonVariant, state: InteractionState) -> SurfaceRole {
    match (classify(variant), state) {
        (SplitButtonFamily::FilledLike, InteractionState::Disabled) => SurfaceRole::AccentDisabled,
        (SplitButtonFamily::FilledLike, InteractionState::Pressed) => SurfaceRole::AccentPressed,
        (SplitButtonFamily::FilledLike, InteractionState::Hovered) => SurfaceRole::AccentHover,
        (SplitButtonFamily::FilledLike, _) => SurfaceRole::Accent,

        (SplitButtonFamily::PlainLike, InteractionState::Pressed) => SurfaceRole::Pressed,
        (SplitButtonFamily::PlainLike, InteractionState::Hovered) => SurfaceRole::Hover,
        (SplitButtonFamily::PlainLike, _) => SurfaceRole::Main,

        (SplitButtonFamily::GhostLike, InteractionState::Pressed) => SurfaceRole::Pressed,
        (SplitButtonFamily::GhostLike, InteractionState::Hovered) => SurfaceRole::Hover,
        (SplitButtonFamily::GhostLike, _) => SurfaceRole::Transparent,
    }
}

fn resolve_border_role(variant: ButtonVariant, state: InteractionState) -> BorderRole {
    if state == InteractionState::Focused {
        return BorderRole::Focused;
    }
    match classify(variant) {
        SplitButtonFamily::FilledLike | SplitButtonFamily::GhostLike => BorderRole::Transparent,
        SplitButtonFamily::PlainLike => match state {
            InteractionState::Hovered | InteractionState::Pressed => BorderRole::Strong,
            _ => BorderRole::Default,
        },
    }
}

/// Border width for the SplitButton frame: thickens to the theme's
/// `focus_ring_width` on focus, rests at the variant's normal width otherwise.
fn resolve_border_width(
    variant: ButtonVariant,
    state: InteractionState,
    normal_bw: f32,
    focus_bw: f32,
) -> f32 {
    if state == InteractionState::Focused {
        return focus_bw;
    }
    match classify(variant) {
        SplitButtonFamily::FilledLike | SplitButtonFamily::GhostLike => 0.0,
        SplitButtonFamily::PlainLike => normal_bw,
    }
}
