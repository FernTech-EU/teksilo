// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 button chrome.
//!
//! Built on the reusable [`RecipeButtonStyle`] (no custom `impl
//! ButtonStyle` needed): M3 buttons are full-pill, 40 dp tall, and the
//! seven Teksilo variants are mapped onto the M3 button family —
//!
//! | `ButtonVariant` | M3 button |
//! | --- | --- |
//! | `Filled` | Filled (primary) |
//! | `Tinted` | Filled tonal (container) |
//! | `Plain` (default) / `Outlined` | Outlined |
//! | `Ghost` / `Link` | Text |
//! | `Destructive` | Filled (error) |
//!
//! The filled button's hover/pressed use exact M3 state layers
//! ([`FillRecipe::state_layer`] — an 8 % / 12 % on-primary overlay over
//! the primary fill); the error fill reuses `TextRole::Error` (which
//! carries the M3 error color in both schemes). Outlined/text/plain
//! labels are redirected to the accent color, and the Destructive label
//! to `TextRole::OnError`, via [`RecipeButtonStyle::label_roles`].

use std::collections::HashMap;

use teksilo_canvas::{EdgeInsets, Size};
use teksilo_core::styles::density::{density_min_size, spacing};
use teksilo_core::styles::{
    BorderRecipe, ButtonRecipe, ButtonVariant, FillRecipe, PerStateRecipe, RecipeColor, ShapeRecipe,
};
use teksilo_tokens::{BorderRole, InputTokens, SurfaceRole, TargetAxes, TextRole};
use teksilo_widgets::styles::RecipeButtonStyle;

/// M3 button height (dp).
const HEIGHT: f32 = 40.0;

/// [`HEIGHT`] raised to the density's `target_size`. Under Material 3 that is
/// **48 dp** at Touch rather than the generic ladder's 44 (see
/// [`crate::input_tokens`]); the identity at Compact and Comfortable, both of
/// which sit below M3's own 40 dp button.
fn height(tokens: &InputTokens) -> f32 {
    density_min_size(Size::new(0.0, HEIGHT), TargetAxes::HEIGHT, tokens).height
}
/// M3 horizontal padding for container buttons (dp).
const PADDING_H: f32 = 24.0;
/// M3 horizontal padding for text buttons (dp).
const PADDING_H_TEXT: f32 = 16.0;
/// M3 focus-indicator width (dp).
const FOCUS_WIDTH: f32 = 3.0;

/// Build the Material 3 `RecipeButtonStyle`.
pub fn m3_button_style(tokens: &InputTokens) -> RecipeButtonStyle {
    let mut recipes = HashMap::new();
    recipes.insert(ButtonVariant::Filled, filled(tokens));
    recipes.insert(ButtonVariant::Destructive, destructive(tokens));
    recipes.insert(ButtonVariant::Tinted, tonal(tokens));
    // The default variant reads as an M3 outlined button (a clear,
    // medium-emphasis neutral button rather than an invisible text one).
    recipes.insert(ButtonVariant::Plain, outlined(tokens));
    recipes.insert(ButtonVariant::Outlined, outlined(tokens));
    recipes.insert(ButtonVariant::Ghost, text(tokens));
    recipes.insert(ButtonVariant::Link, text(tokens));

    // Outlined / text / default buttons read in the accent color (M3);
    // Destructive reads in `OnError` (the M3 on-error color, now a
    // first-class role). Filled keeps OnAccent, Tinted keeps Primary
    // (good contrast on the tonal container), Link keeps Link — all via
    // the Button's built-in mapping, so they are left unset here.
    let mut label_roles = HashMap::new();
    label_roles.insert(ButtonVariant::Plain, TextRole::Accent);
    label_roles.insert(ButtonVariant::Outlined, TextRole::Accent);
    label_roles.insert(ButtonVariant::Ghost, TextRole::Accent);
    label_roles.insert(ButtonVariant::Destructive, TextRole::OnError);

    RecipeButtonStyle {
        recipes,
        label_roles,
    }
}

/// Filled (primary) — high emphasis.
fn filled(tokens: &InputTokens) -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::Pill,
        fill: PerStateRecipe {
            idle: FillRecipe::solid(SurfaceRole::Accent),
            // Exact M3 state layers: an 8 % / 12 % on-primary overlay
            // composited over the primary fill at paint time.
            hover: Some(FillRecipe::state_layer(
                SurfaceRole::Accent,
                TextRole::OnAccent,
                0.08,
            )),
            pressed: Some(FillRecipe::state_layer(
                SurfaceRole::Accent,
                TextRole::OnAccent,
                0.12,
            )),
            focused: None,
            disabled: Some(FillRecipe::solid(SurfaceRole::AccentDisabled)),
        },
        border: PerStateRecipe::uniform(BorderRecipe::none()),
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(spacing(PADDING_H, tokens), 0.0),
        min_size: Size::new(0.0, height(tokens)),
    }
}

/// Filled (error) — destructive actions.
fn destructive(tokens: &InputTokens) -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::Pill,
        fill: PerStateRecipe {
            // `TextRole::Error` carries the M3 error color (#B3261E /
            // #F2B8B5) in both schemes, so it doubles as a reactive fill.
            idle: FillRecipe::solid(RecipeColor::Text(TextRole::Error)),
            hover: None,
            pressed: None,
            focused: None,
            disabled: Some(FillRecipe::solid(SurfaceRole::AccentDisabled)),
        },
        border: PerStateRecipe::uniform(BorderRecipe::none()),
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(spacing(PADDING_H, tokens), 0.0),
        min_size: Size::new(0.0, height(tokens)),
    }
}

/// Filled tonal — medium emphasis container.
fn tonal(tokens: &InputTokens) -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::Pill,
        fill: PerStateRecipe {
            // `AccentSubtle` maps to the M3 primary container.
            idle: FillRecipe::solid(SurfaceRole::AccentSubtle),
            // No neutral hover here — a grey state layer would clobber the
            // tonal container; the container itself signals the affordance.
            hover: None,
            pressed: None,
            focused: None,
            disabled: Some(FillRecipe::solid(SurfaceRole::AccentDisabled)),
        },
        border: PerStateRecipe::uniform(BorderRecipe::none()),
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(spacing(PADDING_H, tokens), 0.0),
        min_size: Size::new(0.0, height(tokens)),
    }
}

/// Outlined — low/medium emphasis with an outline.
fn outlined(tokens: &InputTokens) -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::Pill,
        fill: PerStateRecipe {
            idle: FillRecipe::solid(SurfaceRole::Transparent),
            // Neutral hover layer (M3 state-layer approximation).
            hover: Some(FillRecipe::solid(SurfaceRole::Hover)),
            pressed: None,
            focused: None,
            disabled: None,
        },
        border: PerStateRecipe {
            idle: BorderRecipe::solid(1.0, RecipeColor::Border(BorderRole::Strong)),
            hover: None,
            pressed: None,
            focused: Some(BorderRecipe::solid(
                FOCUS_WIDTH,
                RecipeColor::Border(BorderRole::Focused),
            )),
            disabled: None,
        },
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(spacing(PADDING_H, tokens), 0.0),
        min_size: Size::new(0.0, height(tokens)),
    }
}

/// Text — lowest emphasis.
fn text(tokens: &InputTokens) -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::Pill,
        fill: PerStateRecipe {
            idle: FillRecipe::solid(SurfaceRole::Transparent),
            hover: Some(FillRecipe::solid(SurfaceRole::Hover)),
            pressed: None,
            focused: None,
            disabled: None,
        },
        border: PerStateRecipe {
            idle: BorderRecipe::none(),
            hover: None,
            pressed: None,
            focused: Some(BorderRecipe::solid(
                FOCUS_WIDTH,
                RecipeColor::Border(BorderRole::Focused),
            )),
            disabled: None,
        },
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(spacing(PADDING_H_TEXT, tokens), 0.0),
        min_size: Size::new(0.0, height(tokens)),
    }
}
