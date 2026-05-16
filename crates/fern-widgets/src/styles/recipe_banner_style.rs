//! Default `BannerStyle` impl driven by paint-recipe data.
//!
//! `RecipeBannerStyle` ships the IntUI banner chrome: a per-severity
//! status-tinted surface (`StatusInfo` / `StatusSuccess` /
//! `StatusWarning` / `StatusError`) with rounded corners, the content
//! inset by the banner padding, and the leading severity glyph placed
//! to the leading edge of the message/action content.
//!
//! The `SeverityGlyph` itself is built by the `Banner` widget — it is
//! a functional renderer (it draws domain data, the info/warn/error
//! mark), not chrome (principle 6). Apps that want a different banner
//! look (full-bleed strip, bordered callout, icon-free) write their
//! own `impl BannerStyle` block.

use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::styles::{BannerStyle, BannerStyleConfig};
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, VAlignment};

use crate::primitives::{Expand, HStack, Padding, RectWidget, ZStack};

// IntUI design tokens for Banner. Relocated from
// `theme.components.banner` in Stage B of the group-5 styling
// migration — the recipe owns its own dimensions. `BANNER_GLYPH_SIZE`
// and `BANNER_TITLE_DESCRIPTION_GAP` are consumed by the `Banner`
// widget (it builds the glyph + text column); the rest are consumed
// here.
pub const BANNER_PADDING_HORIZONTAL: f32 = 12.0;
pub const BANNER_PADDING_VERTICAL: f32 = 10.0;
pub const BANNER_CORNER_RADIUS: f32 = 8.0;
/// Diameter of the leading severity glyph (info / success / error
/// circle, warning triangle).
pub const BANNER_GLYPH_SIZE: f32 = 16.0;
/// Horizontal gap between glyph, text column, action widget, and
/// dismiss button.
pub const BANNER_CONTENT_GAP: f32 = 10.0;
/// Vertical gap between the title and the optional description text
/// inside the body column.
pub const BANNER_TITLE_DESCRIPTION_GAP: f32 = 2.0;

/// Default `BannerStyle` shipped with FernUI. Surface tint comes from
/// the per-severity `SurfaceRole` (no border — the status surface
/// tokens already encode contrast with the page background).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeBannerStyle;

impl BannerStyle for RecipeBannerStyle {
    fn make_body(&self, cfg: &BannerStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let radius = CornerRadius::uniform(BANNER_CORNER_RADIUS);

        // Background panel — status surface tint, no border.
        let bg = ctx.add(
            RectWidget::new()
                .background(ColorProp::SurfaceRole(cfg.severity.surface()))
                .corner_radius(radius),
        );

        // Row layout: [glyph] [content (expands to fill)].
        let content = ctx.add(Expand::horizontal().child_id(cfg.content));
        let row = ctx.add(
            HStack::new()
                .spacing(BANNER_CONTENT_GAP)
                .alignment(VAlignment::Center)
                .add_child(cfg.leading_glyph)
                .add_child(content),
        );
        let padded = ctx.add(
            Padding::symmetric(BANNER_PADDING_VERTICAL, BANNER_PADDING_HORIZONTAL).child_id(row),
        );

        ctx.add(ZStack::new().add_child(bg).add_child(padded))
    }
}
