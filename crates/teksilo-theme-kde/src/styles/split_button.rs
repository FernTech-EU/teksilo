// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze frame of a `SplitButton`.
//!
//! A `SplitButton` is a push button with a menu, and its frame is the Breeze
//! push button's ([`crate::styles::chrome`]): the same fill, outline,
//! highlight under the pointer or keyboard focus, press and 1 px shadow,
//! around the widget's own row (the action, a divider, the menu chevron). It
//! is as tall as a push button, 34 px at Plasma's default font, and at least
//! as wide, 84 px ([`crate::styles::metrics`]).
//!
//! | `ButtonVariant` | Frame |
//! | --- | --- |
//! | `Plain` (default) / `Tinted` / `Outlined` | the push button |
//! | `Ghost` / `Link` | the flat button |
//! | `Filled` / `Destructive` | the push button filled with the accent |
//!
//! ## The filled variants
//!
//! A `SplitButton` labels its `Filled` and `Destructive` variants in the
//! on-accent text and gives its style no say over the label. Breeze has no
//! accent-filled button, and its default button's light fill under the
//! on-accent text would be unreadable in a dark scheme (black on Breeze
//! Dark's 45,70,85), so those two variants are framed in the accent itself,
//! with its hover, press and disabled shades, which the on-accent text is
//! lifted to read on (D1 of the colour mapping). This is the one place the
//! Breeze styles paint an accent fill (`[unpublished]`). A `Button`'s
//! `Filled` variant is the Breeze default button instead, because its style
//! chooses the label.

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{ButtonVariant, SplitButtonStyle, SplitButtonStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_widgets::primitives::{MinSize, ZStack};

use crate::styles::chrome::{BreezeButtonChrome, BreezeButtonKind, BreezeButtonState};
use crate::styles::metrics::breeze_button_metrics_for;

/// Breeze `SplitButtonStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeSplitButtonStyle;

impl BreezeSplitButtonStyle {
    /// The Breeze button a split button of `variant` is framed as.
    pub fn kind(variant: ButtonVariant) -> BreezeButtonKind {
        match variant {
            ButtonVariant::Plain | ButtonVariant::Tinted | ButtonVariant::Outlined => {
                BreezeButtonKind::Push
            }
            ButtonVariant::Ghost | ButtonVariant::Link => BreezeButtonKind::Flat,
            ButtonVariant::Filled | ButtonVariant::Destructive => BreezeButtonKind::Accent,
        }
    }
}

impl SplitButtonStyle for BreezeSplitButtonStyle {
    fn make_body(&self, cfg: &SplitButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme();
        let m = breeze_button_metrics_for(&theme.input, &theme.typography);
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
        let chrome = ctx.add(BreezeButtonChrome::new(
            Self::kind(cfg.variant),
            state,
            show_focus_frame,
        ));
        let stack = ctx.add(ZStack::new().child(chrome).child(cfg.content));
        ctx.add(MinSize::new(m.min_width, m.height).child(stack))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variants_map_onto_the_frames() {
        for v in [
            ButtonVariant::Plain,
            ButtonVariant::Tinted,
            ButtonVariant::Outlined,
        ] {
            assert_eq!(BreezeSplitButtonStyle::kind(v), BreezeButtonKind::Push);
        }
        for v in [ButtonVariant::Ghost, ButtonVariant::Link] {
            assert_eq!(BreezeSplitButtonStyle::kind(v), BreezeButtonKind::Flat);
        }
        for v in [ButtonVariant::Filled, ButtonVariant::Destructive] {
            assert_eq!(BreezeSplitButtonStyle::kind(v), BreezeButtonKind::Accent);
        }
    }

    #[test]
    fn the_on_accent_label_reads_on_every_accent_frame() {
        use teksilo_tokens::contrast::composite_contrast;
        for t in [crate::light(), crate::dark()] {
            let c = &t.colors;
            for fill in [c.accent, c.accent_hover, c.accent_pressed] {
                assert!(composite_contrast(c.text_on_accent, fill) >= 4.5);
            }
        }
    }
}
