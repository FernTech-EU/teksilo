//! Default `BadgeStyle` impl driven by paint-recipe data.
//!
//! `RecipeBadgeStyle` ships the IntUI badge chrome: a fully-rounded
//! (`9999` radius) pill with a soft `AccentSubtle` background by
//! default, content inset by the badge padding. Pure composition —
//! no custom paint.
//!
//! Apps that want a different badge look (square tag, status-tinted
//! pill, bordered chip) write their own `impl BadgeStyle` block and
//! install it per-call (`Badge::style(...)`) or theme-wide
//! (`theme.style_slots.badge`).

use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::styles::{BadgeStyle, BadgeStyleConfig};
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, SurfaceRole};

use crate::primitives::{Padding, RectWidget, ZStack};

// IntUI design tokens for Badge. Relocated from
// `theme.components.badge` in Stage C of the group-5 styling
// migration — the recipe owns its own dimensions.
pub const BADGE_PADDING_HORIZONTAL: f32 = 6.0;
pub const BADGE_PADDING_VERTICAL: f32 = 1.0;
/// Fully-rounded pill — a large radius the renderer clamps to half the
/// shorter side.
pub const BADGE_CORNER_RADIUS: f32 = 9999.0;

/// Default `BadgeStyle` shipped with FernUI. Background defaults to
/// `SurfaceRole::AccentSubtle` when the caller sets no override.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeBadgeStyle;

impl BadgeStyle for RecipeBadgeStyle {
    fn make_body(&self, cfg: &BadgeStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let bg: ColorProp = cfg
            .background_override
            .clone()
            .unwrap_or(ColorProp::SurfaceRole(SurfaceRole::AccentSubtle));
        let bg_rect = ctx.add(
            RectWidget::new()
                .background(bg)
                .corner_radius(CornerRadius::uniform(BADGE_CORNER_RADIUS)),
        );
        let padding_id = ctx.add(
            Padding::symmetric(BADGE_PADDING_VERTICAL, BADGE_PADDING_HORIZONTAL)
                .child_id(cfg.content),
        );
        ctx.add(ZStack::new().add_child(bg_rect).add_child(padding_id))
    }
}
