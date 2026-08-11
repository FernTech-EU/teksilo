// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent button.
//!
//! A WinUI button is a 4 dp rounded rectangle filled with
//! `ControlFillColorDefault`, outlined by a 1 dp hairline, and finished
//! with the **elevation edge** that makes it read as a physical, raised
//! surface: a slightly heavier stroke along the bottom in the light theme
//! (a cast shadow) and along the top in the dark theme (a catch-light).
//! Pressing it flattens the edge away. That single detail is what most
//! distinguishes a Fluent button from a Material 3 pill or an IntUI
//! rectangle, and [`crate::styles::chrome`] paints it.
//!
//! ## Variant mapping
//!
//! | `ButtonVariant` | WinUI button |
//! | --- | --- |
//! | `Filled` | `AccentButtonStyle` |
//! | `Destructive` | `AccentButtonStyle`, recoloured critical |
//! | `Plain` (default) / `Tinted` / `Outlined` | `DefaultButtonStyle` |
//! | `Ghost` | `SubtleButtonStyle` |
//! | `Link` | `HyperlinkButtonStyle` |
//!
//! Fluent has no tonal button, so `Tinted` folds into the standard one
//! rather than inventing a fourth emphasis level. `Outlined` folds in too:
//! the standard Fluent button *is* the outlined one.
//!
//! ## Metrics
//!
//! `ButtonPadding` is `11,5,11,6` — asymmetric by one pixel at the bottom,
//! which is what optically centres a 14/20 label against its cap height.
//! WinUI sets no `MinHeight` on the button; the familiar 32 dp is what
//! that padding plus the Body line box adds up to. Teksilo resolves the
//! label's height itself, so the same 32 dp is applied here as a floor
//! rather than left to emerge — it keeps a short label from producing a
//! shorter button than the field beside it.

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{ButtonStyle, ButtonStyleConfig, ButtonVariant};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextRole;
use teksilo_widgets::primitives::{MinSize, Padding, ZStack};

use crate::shape::FLUENT_CONTROL_CORNER_RADIUS;
use crate::styles::chrome::{FluentControlChrome, FluentState, FluentSurfaceKind};

/// `ButtonPadding` leading / trailing (dp).
const PADDING_H: f32 = 11.0;
/// `ButtonPadding` top (dp).
const PADDING_TOP: f32 = 5.0;
/// `ButtonPadding` bottom (dp) — one more than the top; see the module doc.
const PADDING_BOTTOM: f32 = 6.0;
/// The standard Fluent control height (dp).
const MIN_HEIGHT: f32 = 32.0;

/// Fluent `ButtonStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentButtonStyle;

impl FluentButtonStyle {
    fn surface_kind(variant: ButtonVariant) -> FluentSurfaceKind {
        match variant {
            ButtonVariant::Filled => FluentSurfaceKind::Accent,
            ButtonVariant::Destructive => FluentSurfaceKind::Critical,
            ButtonVariant::Ghost | ButtonVariant::Link => FluentSurfaceKind::Subtle,
            ButtonVariant::Plain | ButtonVariant::Tinted | ButtonVariant::Outlined => {
                FluentSurfaceKind::Standard
            }
        }
    }
}

impl ButtonStyle for FluentButtonStyle {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let kind = Self::surface_kind(cfg.variant);
        let state = FluentState::derive(&cfg.is_disabled, &cfg.is_pressed, &cfg.is_hovered);
        // The Button surface exposes no `:focus-visible` signal, so the ring
        // follows plain focus — the same trade the IntUI recipe makes.
        let show_ring = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);

        let chrome = ctx.add(FluentControlChrome::new(
            kind,
            FLUENT_CONTROL_CORNER_RADIUS,
            state,
            show_ring,
        ));

        let padded = ctx.add(
            Padding::new(PADDING_TOP, PADDING_H, PADDING_BOTTOM, PADDING_H).child_id(cfg.label),
        );
        let stack = ctx.add(ZStack::new().add_child(chrome).add_child(padded));
        ctx.add(MinSize::new(0.0, MIN_HEIGHT).child_id(stack))
    }

    fn label_text_role(&self, variant: ButtonVariant) -> Option<TextRole> {
        match variant {
            // `TextOnAccentFillColorPrimary` — white on the light theme's
            // darkened accent, black on the dark theme's lightened one.
            ButtonVariant::Filled => Some(TextRole::OnAccent),
            // The critical fill flips the same way (deep red in light, pale
            // pink in dark), so its label needs the on-error role, not
            // on-accent.
            ButtonVariant::Destructive => Some(TextRole::OnError),
            // A `HyperlinkButton` reads in `AccentTextFillColorPrimary`;
            // everything else keeps `TextFillColorPrimary`. Both are the
            // Button's own built-in mapping, so they are left alone.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_variants_map_to_the_accent_surface() {
        assert_eq!(
            FluentButtonStyle::surface_kind(ButtonVariant::Filled),
            FluentSurfaceKind::Accent
        );
        assert_eq!(
            FluentButtonStyle::surface_kind(ButtonVariant::Destructive),
            FluentSurfaceKind::Critical
        );
    }

    #[test]
    fn tinted_and_outlined_fold_into_the_standard_button() {
        // Fluent has exactly one neutral button; folding rather than
        // inventing a tonal tier is deliberate.
        for v in [
            ButtonVariant::Plain,
            ButtonVariant::Tinted,
            ButtonVariant::Outlined,
        ] {
            assert_eq!(
                FluentButtonStyle::surface_kind(v),
                FluentSurfaceKind::Standard
            );
        }
    }

    #[test]
    fn ghost_and_link_are_subtle() {
        for v in [ButtonVariant::Ghost, ButtonVariant::Link] {
            assert_eq!(
                FluentButtonStyle::surface_kind(v),
                FluentSurfaceKind::Subtle
            );
        }
    }

    #[test]
    fn destructive_label_is_on_error_not_on_accent() {
        let s = FluentButtonStyle;
        assert_eq!(
            s.label_text_role(ButtonVariant::Destructive),
            Some(TextRole::OnError)
        );
        assert_eq!(
            s.label_text_role(ButtonVariant::Filled),
            Some(TextRole::OnAccent)
        );
    }

    #[test]
    fn neutral_variants_keep_the_buttons_own_label_mapping() {
        let s = FluentButtonStyle;
        for v in [
            ButtonVariant::Plain,
            ButtonVariant::Tinted,
            ButtonVariant::Outlined,
            ButtonVariant::Ghost,
            ButtonVariant::Link,
        ] {
            assert_eq!(s.label_text_role(v), None);
        }
    }
}
