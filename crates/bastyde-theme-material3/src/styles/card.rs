// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 card.
//!
//! M3 cards use a 12 dp corner radius (vs IntUI's 8 dp). The M3 surface
//! tones and the softened M3 elevation already come from the M3 color and
//! shape tokens, so the only thing left is the radius — and rather than
//! re-implement the card frame, this delegates to the tested
//! [`RecipeCardStyle`] and injects M3's 12 dp radius whenever the caller
//! hasn't set an explicit `corner_radius` override.

use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Prop;
use bastyde_core::styles::{CardStyle, CardStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_widgets::styles::RecipeCardStyle;

/// M3 medium corner radius for cards (dp).
const M3_CARD_RADIUS: f32 = 12.0;

/// Material 3 card `CardStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct M3CardStyle;

impl CardStyle for M3CardStyle {
    fn make_body(&self, cfg: &CardStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let mut cfg = cfg.clone();
        if cfg.corner_radius_override.is_none() {
            cfg.corner_radius_override = Some(Prop::Static(M3_CARD_RADIUS));
        }
        RecipeCardStyle.make_body(&cfg, ctx)
    }
}
