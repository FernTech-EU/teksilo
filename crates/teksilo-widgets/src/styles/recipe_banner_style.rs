// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::styles::{BannerStyle, BannerStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, VAlignment};

use crate::primitives::{Expand, HStack, Padding, RectWidget, ZStack};

// IntUI design tokens for Banner. The recipe owns its own dimensions.
// `BANNER_GLYPH_SIZE` and `BANNER_TITLE_DESCRIPTION_GAP` are consumed
// by the `Banner` widget (it builds the glyph + text column); the rest
// are consumed here.
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

/// Dimension recipe for [`RecipeBannerStyle`]. All fields default to the
/// corresponding `BANNER_*` constants so callers can override individual
/// measurements without touching the rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BannerRecipe {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub glyph_size: f32,
    pub content_gap: f32,
    pub title_description_gap: f32,
}

impl Default for BannerRecipe {
    fn default() -> Self {
        Self {
            padding_horizontal: BANNER_PADDING_HORIZONTAL,
            padding_vertical: BANNER_PADDING_VERTICAL,
            corner_radius: BANNER_CORNER_RADIUS,
            glyph_size: BANNER_GLYPH_SIZE,
            content_gap: BANNER_CONTENT_GAP,
            title_description_gap: BANNER_TITLE_DESCRIPTION_GAP,
        }
    }
}

/// Default `BannerStyle` shipped with Teksilo. Surface tint comes from
/// the per-severity `SurfaceRole` (no border — the status surface
/// tokens already encode contrast with the page background).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeBannerStyle {
    pub recipe: BannerRecipe,
}

impl RecipeBannerStyle {
    pub fn new(recipe: BannerRecipe) -> Self {
        Self { recipe }
    }
}

impl BannerStyle for RecipeBannerStyle {
    fn make_body(&self, cfg: &BannerStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let radius = CornerRadius::uniform(self.recipe.corner_radius);

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
                .spacing(self.recipe.content_gap)
                .alignment(VAlignment::Center)
                .add_child(cfg.leading_glyph)
                .add_child(content),
        );
        let padded = ctx.add(
            Padding::symmetric(self.recipe.padding_vertical, self.recipe.padding_horizontal)
                .child_id(row),
        );

        ctx.add(ZStack::new().add_child(bg).add_child(padded))
    }
}
