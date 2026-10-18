// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze tooltip.
//!
//! The frame is [`crate::styles::popup`]'s in the tooltip's colours: the
//! tooltip background in a 1 px outline moved toward the tooltip text by the
//! frame contrast, at radius 4.5, over Breeze's two-layer window shadow. The
//! text lies 4 px inside the frame on every side, Breeze's 3 px tooltip frame
//! and the pixel Qt adds, scaled by the density's spacing, so a one-line
//! tooltip of Plasma's default font is 26 px tall, as Qt measures it. The
//! text wraps at 320 px ([`TOOLTIP_MAX_WIDTH`]).
//!
//! The text itself is the tooltip widget's: a plain `TooltipWidget` writes
//! it in the small text style, where Breeze uses the body font. And
//! `TextWidget` does not use `TextStyle::line_height`: the text backend lays
//! a single line out at the face's own natural height (Inter at 11 px:
//! 13.31 px; Noto Sans at 11 px: 14.98 px), so a one-line tooltip is that
//! height plus 2 x 4 px, 21.31 px with the fixed presets and 22.98 px in
//! system mode, not the 26 px of Plasma. That is a property of the framework
//! shared by every preset; matching Plasma would take the tooltip text in
//! the body style or `TextWidget` honouring `line_height`, both outside this
//! crate (the crate's known limitations).

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::density::spacing;
use teksilo_core::styles::{TooltipStyle, TooltipStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::InputTokens;

use crate::geometry::BreezeGeometry;
use crate::styles::metrics::TOOLTIP_MAX_WIDTH;
use crate::styles::popup::{BreezePopupFrame, BreezePopupKind, PopupNode};

/// Breeze `TooltipStyle`.
///
/// [`Default`] is the Compact density; the preset installs
/// [`BreezeTooltipStyle::for_tokens`] of the theme it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeTooltipStyle {
    /// `[measured-qt]` The space between the frame and the text on every
    /// side.
    pub padding: f32,
    /// `[unpublished]` The widest the frame lays its content out.
    pub max_width: f32,
}

impl BreezeTooltipStyle {
    /// The style at the density of `tokens`.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            padding: spacing(BreezeGeometry::breeze_default().tooltip_padding, tokens),
            max_width: TOOLTIP_MAX_WIDTH,
        }
    }
}

impl Default for BreezeTooltipStyle {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

impl TooltipStyle for BreezeTooltipStyle {
    fn make_body(&self, cfg: &TooltipStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(BreezePopupFrame::new(
            cfg.content,
            BreezePopupKind::Tooltip,
            self.padding,
            Some(self.max_width),
            // The tooltip widget emits the tooltip node.
            PopupNode::Hidden,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    #[test]
    fn a_one_line_tooltip_is_breezes_twenty_six_pixels() {
        let s = BreezeTooltipStyle::default();
        assert_eq!(s.padding, 4.0);
        assert_eq!(s.max_width, 320.0);
        let g = BreezeGeometry::breeze_default();
        assert_eq!(g.line_height + 2.0 * s.padding, 26.0);
        assert_eq!(g.tooltip_height, 26.0);
        let touch = BreezeTooltipStyle::for_tokens(&InputTokens::for_density(TargetDensity::Touch));
        assert!((touch.padding - 5.2).abs() < 1e-4);
        assert_eq!(touch.max_width, 320.0);
    }
}
