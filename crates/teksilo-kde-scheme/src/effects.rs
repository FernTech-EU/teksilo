// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Disabled and Inactive state effects: how a scheme's stored colours
//! become the colours of a disabled control or of a window without focus.
//!
//! Implemented from sections c.2, c.5 and e of Teksilo's specification
//! (`engineering/docs/kde-breeze-spec.md`).
//!
//! A state's effects ([`StateEffect`]) are three optional transformations:
//! an *intensity* change (shade, darken or lighten the luma), a *colour*
//! change (desaturate, fade toward a colour, or tint toward it) and, for
//! foregrounds only, a *contrast* change (fade or tint toward the
//! background the colour is drawn on). They run in this order:
//!
//! - a foreground or a decoration gets the contrast change first, against
//!   the set's background as it was before any effect, then the intensity
//!   change, then the colour change ([`foreground_chain`]);
//! - a stored background gets the intensity change, then the colour change
//!   ([`background_chain`]).
//!
//! The Active state never has effects. When a state's effects are disabled
//! (`Enable=false`), every colour passes through untouched, with no colour
//! operation run on it, so it is not even re-quantised.
//!
//! [`KdeColorScheme::state_colors`] also chooses where each (state, set)
//! takes its colours from: a set's `[Inactive]` subgroup replaces it for the
//! Inactive state, and the Selection set of an inactive window is, by
//! default, the Window set with both backgrounds tinted toward the
//! selection colour ([`tint_toward_selection`]). The six computed
//! backgrounds and the palette are derived from the result in
//! [`crate::palette`].

use crate::color::KdeColor;
use crate::math::{darken, lighten, mix, shade, tint};
use crate::scheme::{
    ColorEffect, ColorRole, ColorSet, ColorState, ContrastEffect, IntensityEffect, KdeColorScheme,
    SetColors, StateEffect,
};

/// How far the backgrounds of an inactive window's selection are tinted
/// toward the selection colour when `ChangeSelectionColor` is on.
pub const SELECTION_TINT_AMOUNT: f64 = 0.4;

/// The effects a stored background gets: the intensity change, then the
/// colour change on its result.
///
/// - intensity: shade adds `intensity_amount` to the luma (chroma amount 0),
///   darken and lighten move the luma by `intensity_amount` toward black or
///   white (chroma factor 1);
/// - colour: desaturate scales the chroma by `1 - color_amount` (luma
///   amount 0), fade mixes toward `color` by `color_amount`, tint tints
///   toward it by `color_amount`.
///
/// A disabled effect returns `color` unchanged.
pub fn background_chain(color: KdeColor, effect: &StateEffect) -> KdeColor {
    if !effect.enabled {
        return color;
    }
    let intensified = match effect.intensity_effect {
        IntensityEffect::None => color,
        IntensityEffect::Shade => shade(color, effect.intensity_amount, 0.0),
        IntensityEffect::Darken => darken(color, effect.intensity_amount, 1.0),
        IntensityEffect::Lighten => lighten(color, effect.intensity_amount, 1.0),
    };
    match effect.color_effect {
        ColorEffect::None => intensified,
        ColorEffect::Desaturate => darken(intensified, 0.0, 1.0 - effect.color_amount),
        ColorEffect::Fade => mix(intensified, effect.color, effect.color_amount),
        ColorEffect::Tint => tint(intensified, effect.color, effect.color_amount),
    }
}

/// The effects a foreground or a decoration drawn on `background` gets: the
/// contrast change toward `background` (fade mixes, tint tints, each by
/// `contrast_amount`), then [`background_chain`] on its result.
///
/// A disabled effect returns `color` unchanged.
pub fn foreground_chain(color: KdeColor, background: KdeColor, effect: &StateEffect) -> KdeColor {
    if !effect.enabled {
        return color;
    }
    let contrasted = match effect.contrast_effect {
        ContrastEffect::None => color,
        ContrastEffect::Fade => mix(color, background, effect.contrast_amount),
        ContrastEffect::Tint => tint(color, background, effect.contrast_amount),
    };
    background_chain(contrasted, effect)
}

/// A set's twelve colours after `effect`.
///
/// The eight foregrounds and two decorations each get [`foreground_chain`]
/// against the set's `BackgroundNormal` as given (before any effect); the
/// two backgrounds each get [`background_chain`]. A disabled effect returns
/// the set unchanged.
pub fn apply_state_effect(colors: &SetColors, effect: &StateEffect) -> SetColors {
    if !effect.enabled {
        return *colors;
    }
    let background = colors.background_normal;
    let mut out = *colors;
    for role in ColorRole::ALL {
        let color = colors.get(role);
        let changed = match role {
            ColorRole::BackgroundNormal | ColorRole::BackgroundAlternate => {
                background_chain(color, effect)
            }
            _ => foreground_chain(color, background, effect),
        };
        out.set(role, changed);
    }
    out
}

/// The Selection set of an inactive window when `ChangeSelectionColor` is
/// on: `window` with its two backgrounds tinted toward
/// `selection_background` by [`SELECTION_TINT_AMOUNT`], its foregrounds and
/// decorations kept.
pub fn tint_toward_selection(window: &SetColors, selection_background: KdeColor) -> SetColors {
    let mut out = *window;
    out.background_normal = tint(
        window.background_normal,
        selection_background,
        SELECTION_TINT_AMOUNT,
    );
    out.background_alternate = tint(
        window.background_alternate,
        selection_background,
        SELECTION_TINT_AMOUNT,
    );
    out
}

impl KdeColorScheme {
    /// The twelve colours of `set` in `state`, after the state's effects
    /// and before the computed backgrounds.
    ///
    /// - Active: the stored set, unchanged.
    /// - Inactive: the set's `[Inactive]` subgroup when it has one, else the
    ///   stored set, then the Inactive effects. The Selection set with
    ///   [`KdeColorScheme::change_selection_color`] on is instead the Window
    ///   set (its `[Inactive]` subgroup when present) passed through
    ///   [`tint_toward_selection`] with the stored Selection
    ///   `BackgroundNormal`, then the Inactive effects.
    /// - Disabled: the stored set, the Selection set being the stored
    ///   Window set (with no tint), then the Disabled effects.
    pub fn state_colors(&self, state: ColorState, set: ColorSet) -> SetColors {
        match state {
            ColorState::Active => *self.set(set),
            ColorState::Inactive => {
                let source = if set == ColorSet::Selection && self.change_selection_color {
                    let window = self
                        .inactive_set(ColorSet::Window)
                        .unwrap_or_else(|| self.set(ColorSet::Window));
                    tint_toward_selection(window, self.set(ColorSet::Selection).background_normal)
                } else {
                    *self.inactive_set(set).unwrap_or_else(|| self.set(set))
                };
                apply_state_effect(&source, &self.inactive_effect)
            }
            ColorState::Disabled => {
                let source = if set == ColorSet::Selection {
                    self.set(ColorSet::Window)
                } else {
                    self.set(set)
                };
                apply_state_effect(source, &self.disabled_effect)
            }
        }
    }
}
