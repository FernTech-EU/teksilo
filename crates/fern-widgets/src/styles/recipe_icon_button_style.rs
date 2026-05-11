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
//! (`IconButton::style(...)`) or theme-wide (step 8's
//! `ComponentStyles.icon_button = Rc::new(MyIconButton)`).

use fern_tokens::CornerRadius;
use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::signal::Signal;
use fern_core::styles::{
    IconButtonSize, IconButtonStyle, IconButtonStyleConfig,
};
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, SurfaceRole};

use crate::primitives::{Center, FixedSize, RectWidget, ZStack};

/// Default `IconButtonStyle` shipped with FernUI. Reads its dimensions
/// from `theme.components.icon_button` and its surface roles from the
/// active theme's role resolver (so theme-swap repaints for free).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeIconButtonStyle;

impl IconButtonStyle for RecipeIconButtonStyle {
    fn make_body(&self, cfg: &IconButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let ib_style = ctx.theme().components.icon_button;
        let focus_ring_width = ctx.theme().shape.focus_ring_width;
        let corner_radius = ib_style.corner_radius;
        let button_dim = resolve_size(cfg.size, &ib_style);

        // Background — `Selected` flavor when `is_on == true`, plain
        // flat treatment otherwise. Pressed always wins (the press
        // flash overrides Selected).
        let bg_role: ColorProp = match cfg.is_on.clone() {
            Some(on) => bistate_bg_role(&cfg.is_pressed, &cfg.is_hovered, &cfg.is_disabled, &on)
                .into(),
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

fn resolve_size(size: IconButtonSize, style: &fern_tokens::IconButtonStyle) -> f32 {
    match size {
        IconButtonSize::Compact => style.size_compact,
        IconButtonSize::Default => style.size_default,
        IconButtonSize::Toolbar => style.size_toolbar,
        IconButtonSize::Large => style.size_large,
        IconButtonSize::Hero => style.size_hero,
    }
}
