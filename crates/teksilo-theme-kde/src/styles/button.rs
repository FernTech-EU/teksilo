// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze push button.
//!
//! Breeze has three buttons, and the seven `ButtonVariant`s map onto them:
//!
//! | `ButtonVariant` | Breeze button |
//! | --- | --- |
//! | `Filled` | the default button (the dialog's default action) |
//! | `Plain` (default) / `Tinted` / `Outlined` | the push button |
//! | `Destructive` | the push button |
//! | `Ghost` / `Link` | the flat button |
//!
//! Breeze has no tonal or outlined emphasis tier: its push button is
//! already outlined, so `Tinted` and `Outlined` are the push button rather
//! than an invented fourth look. It has no destructive button either: a KDE
//! application says an action is destructive in its text and icon, so
//! `Destructive` is the push button too. The default button is Breeze's
//! one emphasised button, a fill moved a fifth of the way to the highlight,
//! and only in an active window; its label stays the button text.
//! [`crate::styles::chrome`] paints all three.
//!
//! ## Labels
//!
//! Every Breeze button labels in the button text, a flat one in the window
//! text: both are the primary text role. The Button gives its `Filled` and
//! `Destructive` labels the on-accent role by default, for an accent fill
//! Breeze does not paint, so this style asks for the primary role there.
//! `Link` keeps the link colour.
//!
//! ## Metrics
//!
//! A push button is the label's line box plus 8 px above and below (the 6 px
//! button margin and the 2 px frame margin), 34 px at Plasma's default
//! font, and at least 84 px wide: Breeze's `max(text + 12, 80) + 4`. A flat
//! push button keeps those dimensions. See [`crate::styles::metrics`].

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{ButtonStyle, ButtonStyleConfig, ButtonVariant};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextRole;
use teksilo_widgets::primitives::{MinSize, Padding, ZStack};

use crate::styles::chrome::{BreezeButtonChrome, BreezeButtonKind, BreezeButtonState};
use crate::styles::metrics::breeze_button_metrics_for;

/// Breeze `ButtonStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeButtonStyle;

impl BreezeButtonStyle {
    /// The Breeze button `variant` is painted as.
    pub fn kind(variant: ButtonVariant) -> BreezeButtonKind {
        match variant {
            ButtonVariant::Filled => BreezeButtonKind::Default,
            ButtonVariant::Plain
            | ButtonVariant::Tinted
            | ButtonVariant::Outlined
            | ButtonVariant::Destructive => BreezeButtonKind::Push,
            ButtonVariant::Ghost | ButtonVariant::Link => BreezeButtonKind::Flat,
        }
    }
}

impl ButtonStyle for BreezeButtonStyle {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme();
        let m = breeze_button_metrics_for(&theme.input, &theme.typography);
        let state = BreezeButtonState::derive(
            &cfg.is_disabled,
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_focused,
        );
        // The Button's focus signal is already its `:focus-visible` one.
        let show_focus_frame = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);
        let chrome = ctx.add(BreezeButtonChrome::new(
            Self::kind(cfg.variant),
            state,
            show_focus_frame,
        ));
        let padded = ctx.add(
            Padding::new(
                m.padding_vertical,
                m.padding_horizontal,
                m.padding_vertical,
                m.padding_horizontal,
            )
            .child(cfg.label),
        );
        let stack = ctx.add(ZStack::new().child(chrome).child(padded));
        ctx.add(MinSize::new(m.min_width, m.height).child(stack))
    }

    fn label_text_role(&self, variant: ButtonVariant) -> Option<TextRole> {
        match variant {
            // The default button's fill is the button colour moved toward
            // the highlight, not the accent: its label is the button text.
            ButtonVariant::Filled | ButtonVariant::Destructive => Some(TextRole::Primary),
            // The push and flat buttons already label in the primary text,
            // and `Link` in the link colour: the Button's own mapping.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variants_map_onto_breezes_three_buttons() {
        assert_eq!(
            BreezeButtonStyle::kind(ButtonVariant::Filled),
            BreezeButtonKind::Default
        );
        for v in [
            ButtonVariant::Plain,
            ButtonVariant::Tinted,
            ButtonVariant::Outlined,
            ButtonVariant::Destructive,
        ] {
            assert_eq!(BreezeButtonStyle::kind(v), BreezeButtonKind::Push, "{v:?}");
        }
        for v in [ButtonVariant::Ghost, ButtonVariant::Link] {
            assert_eq!(BreezeButtonStyle::kind(v), BreezeButtonKind::Flat, "{v:?}");
        }
    }

    #[test]
    fn no_variant_labels_in_the_on_accent_colour() {
        let s = BreezeButtonStyle;
        assert_eq!(
            s.label_text_role(ButtonVariant::Filled),
            Some(TextRole::Primary)
        );
        assert_eq!(
            s.label_text_role(ButtonVariant::Destructive),
            Some(TextRole::Primary)
        );
        for v in [
            ButtonVariant::Plain,
            ButtonVariant::Tinted,
            ButtonVariant::Outlined,
            ButtonVariant::Ghost,
            ButtonVariant::Link,
        ] {
            assert_eq!(s.label_text_role(v), None, "{v:?}");
        }
    }
}
