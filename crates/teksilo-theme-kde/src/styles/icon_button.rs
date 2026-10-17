// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze tool button, for `IconButton`.
//!
//! An `IconButton` is Breeze's auto-raise tool button, the borderless kind a
//! toolbar holds: nothing at rest, a highlight outline under the pointer or
//! keyboard focus, the highlight at 30 % while pressed and, for a toggle
//! that is on, the button text at 12.5 % with the plain button outline. It
//! never casts a shadow. [`crate::styles::chrome`] paints it.
//!
//! ## Sizes
//!
//! Breeze sizes an auto-raise tool button as its icon plus the 6 px margin
//! on each side: 28 px around a 16 px icon and 34 px around the 22 px
//! toolbar icon. The five `IconButtonSize`s take those two and the same rule
//! at Kirigami's 32 and 48 px icon sizes, with a compact rung at the
//! density's target size ([`BreezeToolButtonSizes`]). Every one clears the
//! 24 dp conformance floor at every density, so no conformance box is built
//! around them.
//!
//! Breeze also draws a tool button with a frame, the push button's, 32 and
//! 38 px around the same icons. `IconButton` has no framed form, so those
//! two sizes ([`BreezeGeometry`](crate::geometry::BreezeGeometry)'s
//! `framed_tool_button_height` and its medium twin) have no widget to reach.
//!
//! The glyph inside is the one the `IconButton` builds at its own icon size
//! for each rung; a style does not size it.

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{IconButtonSize, IconButtonStyle, IconButtonStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_widgets::primitives::{Center, FixedSize, ZStack};

use crate::styles::chrome::{BreezeButtonChrome, BreezeButtonKind, BreezeButtonState};
use crate::styles::metrics::{BreezeToolButtonSizes, breeze_tool_button_sizes_for};

/// Breeze `IconButtonStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeIconButtonStyle;

impl BreezeIconButtonStyle {
    /// The square of `size` among `sizes`.
    pub fn square(size: IconButtonSize, sizes: &BreezeToolButtonSizes) -> f32 {
        match size {
            IconButtonSize::Compact => sizes.compact,
            IconButtonSize::Default => sizes.default,
            IconButtonSize::Toolbar => sizes.toolbar,
            IconButtonSize::Large => sizes.large,
            IconButtonSize::Hero => sizes.hero,
        }
    }
}

impl IconButtonStyle for BreezeIconButtonStyle {
    fn make_body(&self, cfg: &IconButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let sizes = breeze_tool_button_sizes_for(&ctx.theme().input);
        let square = Self::square(cfg.size, &sizes);
        let state = BreezeButtonState::derive(
            &cfg.is_disabled,
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_focused,
        );
        let show_focus_frame = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);
        let chrome = ctx.add(
            BreezeButtonChrome::new(BreezeButtonKind::Flat, state, show_focus_frame)
                .checked(cfg.is_on.clone()),
        );
        let icon = ctx.add(Center::new().child(cfg.icon));
        let stack = ctx.add(ZStack::new().child(chrome).child(icon));
        ctx.add(FixedSize::new().width(square).height(square).child(stack))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styles::metrics::breeze_tool_button_sizes;

    #[test]
    fn each_size_takes_its_rung() {
        let s = breeze_tool_button_sizes();
        let squares = [
            IconButtonSize::Compact,
            IconButtonSize::Default,
            IconButtonSize::Toolbar,
            IconButtonSize::Large,
            IconButtonSize::Hero,
        ]
        .map(|size| BreezeIconButtonStyle::square(size, &s));
        assert_eq!(squares, [24.0, 28.0, 34.0, 44.0, 60.0]);
    }
}
