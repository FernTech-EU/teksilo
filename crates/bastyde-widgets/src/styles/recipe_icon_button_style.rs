// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `IconButtonStyle` impl driven by paint-recipe data.
//!
//! `RecipeIconButtonStyle` ships the IntUI flat-square treatment:
//! transparent at rest, surface tint on hover / pressed, accent border
//! on focus, optional `Selected` surface tint when a `toggled` signal
//! is bound.
//!
//! Apps that want a different treatment (Material 3 elevated icon
//! button, glassmorphism, brutalist square frame) write their own
//! `impl IconButtonStyle` block and install it per-call
//! (`IconButton::style(...)`) or theme-wide
//! (`theme.style_slots.icon_button = Some(Rc::new(MyIconButton))`).

use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{IconButtonSize, IconButtonStyle, IconButtonStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;
use bastyde_tokens::{BorderRole, SurfaceRole};

use crate::primitives::{Center, FixedSize, RectWidget, ZStack};

// IntUI design tokens for IconButton. The recipe owns its own dimensions.
// Sizes follow the IntelliJ IntUI scale (Compact < Default < Toolbar
// < Large < Hero).
pub const ICON_BUTTON_SIZE_COMPACT: f32 = 22.0;
pub const ICON_BUTTON_SIZE_DEFAULT: f32 = 24.0;
pub const ICON_BUTTON_SIZE_TOOLBAR: f32 = 30.0;
pub const ICON_BUTTON_SIZE_LARGE: f32 = 40.0;
pub const ICON_BUTTON_SIZE_HERO: f32 = 50.0;
pub const ICON_BUTTON_ICON_SIZE: f32 = 16.0;
pub const ICON_BUTTON_ICON_SIZE_TOOLBAR: f32 = 18.0;
pub const ICON_BUTTON_ICON_SIZE_LARGE: f32 = 24.0;
pub const ICON_BUTTON_ICON_SIZE_HERO: f32 = 32.0;
pub const ICON_BUTTON_CORNER_RADIUS: f32 = 8.0;

/// Default `IconButtonStyle` shipped with Bastyde. Surface roles come
/// from the active theme's role resolver (so theme-swap repaints for
/// free).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeIconButtonStyle;

impl IconButtonStyle for RecipeIconButtonStyle {
    fn make_body(&self, cfg: &IconButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let focus_ring_width = ctx.theme().shape.focus_ring_width;
        let corner_radius = ICON_BUTTON_CORNER_RADIUS;
        let button_dim = resolve_size(cfg.size);

        // Background — `Selected` flavor when `is_on == true`, plain
        // flat treatment otherwise. Pressed always wins (the press
        // flash overrides Selected).
        let bg_role: ColorProp = match cfg.is_on.clone() {
            Some(on) => {
                bistate_bg_role(&cfg.is_pressed, &cfg.is_hovered, &cfg.is_disabled, &on).into()
            }
            None => plain_bg_role(&cfg.is_pressed, &cfg.is_hovered, &cfg.is_disabled).into(),
        };

        // Border — focus uses accent border at `focus_ring_width`,
        // every other state is transparent + 0 dp. The button's own
        // border IS the focus indicator (Int UI convention).
        let border_role: ColorProp = cfg
            .is_focused
            .map(|focused| {
                if *focused {
                    BorderRole::Focused
                } else {
                    BorderRole::Transparent
                }
            })
            .into();
        let border_width = cfg
            .is_focused
            .map(move |focused| if *focused { focus_ring_width } else { 0.0 });

        let bg_id = ctx.add(
            RectWidget::new()
                .bind_background(bg_role)
                .bind_border_color(border_role)
                .bind_border_width(border_width)
                .corner_radius(CornerRadius::uniform(corner_radius)),
        );

        let centered_id = ctx.add(Center::new().child_id(cfg.icon));
        let zstack_id = ctx.add(ZStack::new().add_child(bg_id).add_child(centered_id));
        ctx.add(
            FixedSize::new()
                .bind_width(button_dim)
                .bind_height(button_dim)
                .child_id(zstack_id),
        )
    }
}

fn plain_bg_role(
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_disabled: &Signal<bool>,
) -> Signal<SurfaceRole> {
    is_pressed
        .zip3(is_hovered, is_disabled)
        .map(|(pressed, hovered, disabled)| {
            // Int UI icon buttons DO have a distinct pressed (mouse-down)
            // state — unlike regular buttons. The shared helper now feeds
            // Pressed on pointer-down, so this renders on mouse-down too,
            // not just keyboard activation.
            if *disabled {
                SurfaceRole::Transparent
            } else if *pressed {
                SurfaceRole::Pressed
            } else if *hovered {
                SurfaceRole::Hover
            } else {
                SurfaceRole::Transparent
            }
        })
}

fn bistate_bg_role(
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_disabled: &Signal<bool>,
    is_on: &Signal<bool>,
) -> Signal<SurfaceRole> {
    let combined = is_pressed.zip3(is_hovered, is_disabled);
    combined
        .zip(is_on)
        .map(|((pressed, hovered, disabled), on)| {
            if *disabled {
                SurfaceRole::Transparent
            } else if *on {
                if *pressed {
                    SurfaceRole::Pressed
                } else {
                    SurfaceRole::Selected
                }
            } else if *pressed {
                SurfaceRole::Pressed
            } else if *hovered {
                SurfaceRole::Hover
            } else {
                SurfaceRole::Transparent
            }
        })
}

fn resolve_size(size: IconButtonSize) -> f32 {
    match size {
        IconButtonSize::Compact => ICON_BUTTON_SIZE_COMPACT,
        IconButtonSize::Default => ICON_BUTTON_SIZE_DEFAULT,
        IconButtonSize::Toolbar => ICON_BUTTON_SIZE_TOOLBAR,
        IconButtonSize::Large => ICON_BUTTON_SIZE_LARGE,
        IconButtonSize::Hero => ICON_BUTTON_SIZE_HERO,
    }
}
