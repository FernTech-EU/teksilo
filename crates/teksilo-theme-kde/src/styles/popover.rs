// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze popup panel: the frame around a menu, a combo box's list, a
//! search field's suggestions and a popover.
//!
//! The frame is [`crate::styles::popup`]'s: the frame background in the
//! frame outline at radius 4.5, over Breeze's two-layer window shadow. A
//! menu's frame has no padding of its own: `MenuList` puts its rows 4 px
//! inside it, Breeze's menu margin, and the highlight of a row fills the row
//! up to that margin. Any other popup's content lies 4 px inside the frame,
//! the same margin, scaled by the density's spacing. The `Tooltip` variant
//! is drawn in the tooltip's colours.
//!
//! ## Where this departs from Breeze
//!
//! - **No caret.** A Breeze popup has no arrow pointing at what opened it,
//!   so the frame ignores [`PopoverStyleConfig::show_caret`] and lies flush
//!   against the placement the overlay gives it.

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{PopoverStyle, PopoverStyleConfig, PopoverVariant};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::InputTokens;

use crate::styles::metrics::POPUP_PADDING;
use crate::styles::popup::{BreezePopupFrame, BreezePopupKind, PopupNode};
use teksilo_core::styles::density::spacing;

/// Breeze `PopoverStyle`.
///
/// [`Default`] is the Compact density; the preset installs
/// [`BreezePopoverStyle::for_tokens`] of the theme it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezePopoverStyle {
    /// `[M]` The space between the frame and the content of a popup that is
    /// not a menu.
    pub padding: f32,
}

impl BreezePopoverStyle {
    /// The style at the density of `tokens`.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            padding: spacing(POPUP_PADDING, tokens),
        }
    }
}

impl Default for BreezePopoverStyle {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

impl PopoverStyle for BreezePopoverStyle {
    fn make_body(&self, cfg: &PopoverStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let (kind, padding, node) = match cfg.variant {
            // The menu's owner carries the container role, and its rows
            // their own margin.
            PopoverVariant::Menu => (BreezePopupKind::Panel, 0.0, PopupNode::Container),
            PopoverVariant::Default => (
                BreezePopupKind::Panel,
                self.padding,
                PopupNode::Dialog(cfg.name.clone()),
            ),
            PopoverVariant::Tooltip => (
                BreezePopupKind::Tooltip,
                self.padding,
                PopupNode::Dialog(cfg.name.clone()),
            ),
        };
        ctx.add(BreezePopupFrame::new(
            cfg.content,
            kind,
            padding,
            None,
            node,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    #[test]
    fn the_padding_is_the_menu_margin_and_grows_with_the_density() {
        assert_eq!(BreezePopoverStyle::default().padding, 4.0);
        let touch = BreezePopoverStyle::for_tokens(&InputTokens::for_density(TargetDensity::Touch));
        assert!((touch.padding - 5.2).abs() < 1e-4);
    }
}
