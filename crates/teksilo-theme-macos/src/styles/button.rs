// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS push button.
//!
//! Two buttons, really. The ordinary one is a 6 dp-rounded **bezel** — a
//! faintly graded face, a hairline, and a shadow half a point below it —
//! and the *default* one is a flat accent fill with a white label. AppKit
//! gives the default button no gradient and no shadow: the accent already
//! separates it from the window, and doubling the cues would make it shout.
//! Pressing either darkens the face and drops the shadow, so the control
//! settles into the surface rather than lighting up.
//!
//! That is the whole vocabulary, and it is a different one from both
//! siblings: Fluent reads its elevation from a single heavier edge on one
//! side, Material 3 from a tonal fill and a ripple. See
//! [`crate::styles::chrome`].
//!
//! ## Variant mapping
//!
//! | `ButtonVariant` | AppKit button |
//! | --- | --- |
//! | `Filled` | the *default* button (`keyEquivalent == "\r"`) |
//! | `Destructive` | the default button, recoloured `systemRed` |
//! | `Plain` (default) / `Tinted` / `Outlined` | `NSBezelStyle.rounded` |
//! | `Ghost` | a borderless / toolbar button |
//! | `Link` | `NSButton` with a link title |
//!
//! macOS has no tonal button, so `Tinted` folds into the bezel rather than
//! inventing a fourth emphasis level, and `Outlined` folds in too: the
//! ordinary macOS button *is* the outlined one.
//!
//! ## Metrics
//!
//! A regular-size push button is 22 dp tall with roughly 10 dp of gutter.
//! Apple publishes neither — control heights live only in Interface
//! Builder's size-class metrics — so both are measured. What matters more
//! than the exact figure is the *relationship*: 22 dp against Fluent's 32
//! is why a macOS window fits more, and a preset that quietly adopted 32
//! would read as Fluent with rounder corners.
//!
//! No minimum **width** is imposed. AppKit does give dialog buttons one
//! (an OK button is never narrow), but it is a property of the alert
//! layout rather than of the control, and applying it here would inflate
//! every toolbar button in the app.

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{ButtonStyle, ButtonStyleConfig, ButtonVariant};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextRole;
use teksilo_widgets::primitives::{MinSize, Padding, ZStack};

use crate::shape::{MACOS_CONTROL_CORNER_RADIUS, MACOS_CONTROL_HEIGHT};
use crate::styles::chrome::{MacOsControlChrome, MacOsState, MacOsSurfaceKind};

/// Horizontal gutter (dp).
const PADDING_H: f32 = 10.0;
/// Vertical gutter (dp) — what turns a 16 dp Body line box into a 22 dp
/// control.
const PADDING_V: f32 = 3.0;

// The gutters have to add up to the control height around a Body line box,
// or a button stops lining up with the field beside it.
const _: () = assert!(PADDING_V * 2.0 + 16.0 == MACOS_CONTROL_HEIGHT);
// …and the whole control stays denser than its 32 dp Fluent counterpart.
const _: () = assert!(MACOS_CONTROL_HEIGHT < 32.0);

/// macOS `ButtonStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsButtonStyle;

impl MacOsButtonStyle {
    fn surface_kind(variant: ButtonVariant) -> MacOsSurfaceKind {
        match variant {
            ButtonVariant::Filled => MacOsSurfaceKind::Accent,
            ButtonVariant::Destructive => MacOsSurfaceKind::Destructive,
            ButtonVariant::Ghost | ButtonVariant::Link => MacOsSurfaceKind::Borderless,
            ButtonVariant::Plain | ButtonVariant::Tinted | ButtonVariant::Outlined => {
                MacOsSurfaceKind::Bezel
            }
        }
    }
}

impl ButtonStyle for MacOsButtonStyle {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let kind = Self::surface_kind(cfg.variant);
        let state = MacOsState::derive(&cfg.is_disabled, &cfg.is_pressed, &cfg.is_hovered);
        // The Button surface exposes no `:focus-visible` signal, so the
        // ring follows plain focus — the same trade the IntUI recipe and
        // the Fluent style both make.
        let show_ring = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);

        let chrome = ctx.add(MacOsControlChrome::new(
            kind,
            MACOS_CONTROL_CORNER_RADIUS,
            state,
            show_ring,
        ));

        let padded =
            ctx.add(Padding::new(PADDING_V, PADDING_H, PADDING_V, PADDING_H).child_id(cfg.label));
        let stack = ctx.add(ZStack::new().add_child(chrome).add_child(padded));
        ctx.add(MinSize::new(0.0, MACOS_CONTROL_HEIGHT).child_id(stack))
    }

    fn label_text_role(&self, variant: ButtonVariant) -> Option<TextRole> {
        match variant {
            // `alternateSelectedControlTextColor` — white on the darkened
            // accent fill in both appearances.
            ButtonVariant::Filled => Some(TextRole::OnAccent),
            // The critical fill flips between a deep red (Aqua) and a pale
            // one (Dark Aqua), so its label needs the on-error role, which
            // is resolved by contrast, not the on-accent one.
            ButtonVariant::Destructive => Some(TextRole::OnError),
            // A bezelled or borderless button reads in `labelColor`, and a
            // link button in `linkColor`. Both are the Button's own
            // built-in mapping, so they are left alone.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_button_is_accent_filled() {
        assert_eq!(
            MacOsButtonStyle::surface_kind(ButtonVariant::Filled),
            MacOsSurfaceKind::Accent
        );
        assert_eq!(
            MacOsButtonStyle::surface_kind(ButtonVariant::Destructive),
            MacOsSurfaceKind::Destructive
        );
    }

    #[test]
    fn tinted_and_outlined_fold_into_the_one_bezelled_button() {
        // macOS has exactly one neutral button; folding rather than
        // inventing a tonal tier is deliberate.
        for v in [
            ButtonVariant::Plain,
            ButtonVariant::Tinted,
            ButtonVariant::Outlined,
        ] {
            assert_eq!(
                MacOsButtonStyle::surface_kind(v),
                MacOsSurfaceKind::Bezel,
                "{v:?} should be the ordinary bezelled button"
            );
        }
    }

    #[test]
    fn ghost_and_link_are_borderless() {
        for v in [ButtonVariant::Ghost, ButtonVariant::Link] {
            assert_eq!(
                MacOsButtonStyle::surface_kind(v),
                MacOsSurfaceKind::Borderless
            );
        }
    }

    #[test]
    fn only_the_filled_variants_carry_a_bezel_free_flat_fill() {
        // The accent-filled default button must NOT get the bezel
        // treatment — doubling the elevation cues is the classic way to
        // make a macOS default button look wrong.
        assert!(!MacOsButtonStyle::surface_kind(ButtonVariant::Filled).is_bezelled());
        assert!(MacOsButtonStyle::surface_kind(ButtonVariant::Plain).is_bezelled());
    }

    #[test]
    fn destructive_label_is_on_error_not_on_accent() {
        let s = MacOsButtonStyle;
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
        let s = MacOsButtonStyle;
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

    #[test]
    fn the_gutters_are_the_measured_values() {
        // That they sum to the control height, and that the control stays
        // denser than Fluent's, are compile-time invariants above.
        assert_eq!(PADDING_H, 10.0);
        assert_eq!(PADDING_V, 3.0);
    }
}
