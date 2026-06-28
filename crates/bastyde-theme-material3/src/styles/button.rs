// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 button chrome.
//!
//! Built on the reusable [`RecipeButtonStyle`] (no custom `impl
//! ButtonStyle` needed): M3 buttons are full-pill, 40 dp tall, and the
//! seven Bastyde variants are mapped onto the M3 button family —
//!
//! | `ButtonVariant` | M3 button |
//! | --- | --- |
//! | `Filled` | Filled (primary) |
//! | `Tinted` | Filled tonal (container) |
//! | `Plain` (default) / `Outlined` | Outlined |
//! | `Ghost` / `Link` | Text |
//! | `Destructive` | Filled (error) |
//!
//! Hover/pressed/disabled use the M3-mapped `accent_*` surface roles (so
//! they stay theme-reactive); the error fill reuses `TextRole::Error`
//! (which carries the M3 error color in both schemes). Outlined/text/
//! plain labels are redirected to the accent color via
//! [`RecipeButtonStyle::label_roles`].

use std::collections::HashMap;

use bastyde_canvas::{EdgeInsets, Size};
use bastyde_core::styles::{
    BorderRecipe, ButtonRecipe, ButtonVariant, FillRecipe, PerStateRecipe, RecipeColor, ShapeRecipe,
};
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole};
use bastyde_widgets::styles::RecipeButtonStyle;

/// M3 button height (dp).
const HEIGHT: f32 = 40.0;
/// M3 horizontal padding for container buttons (dp).
const PADDING_H: f32 = 24.0;
/// M3 horizontal padding for text buttons (dp).
const PADDING_H_TEXT: f32 = 16.0;
/// M3 focus-indicator width (dp).
const FOCUS_WIDTH: f32 = 3.0;

/// Build the Material 3 `RecipeButtonStyle`.
pub fn m3_button_style() -> RecipeButtonStyle {
    let mut recipes = HashMap::new();
    recipes.insert(ButtonVariant::Filled, filled());
    recipes.insert(ButtonVariant::Destructive, destructive());
    recipes.insert(ButtonVariant::Tinted, tonal());
    // The default variant reads as an M3 outlined button (a clear,
    // medium-emphasis neutral button rather than an invisible text one).
    recipes.insert(ButtonVariant::Plain, outlined());
    recipes.insert(ButtonVariant::Outlined, outlined());
    recipes.insert(ButtonVariant::Ghost, text());
    recipes.insert(ButtonVariant::Link, text());

    // Outlined / text / default buttons read in the accent color (M3).
    // Filled & Destructive keep OnAccent, Tinted keeps Primary (good
    // contrast on the tonal container), Link keeps Link — all via the
    // Button's built-in mapping, so they are left unset here.
    //
    // Deviation: M3 specs `on_error` for the Destructive label, but there
    // is no `TextRole::OnError` (no on-error field in `ColorTokens`), so it
    // falls back to `OnAccent` (`on_primary`). In light mode both are
    // `#FFFFFF`, so this is exact; in dark mode the label is the dark
    // `on_primary` purple instead of the dark `on_error` red — a hue
    // nuance on a still-readable (AAA-contrast) dark-on-pink label. A
    // first-class `TextRole::OnError` would close the gap (see the
    // framework-gaps notes).
    let mut label_roles = HashMap::new();
    label_roles.insert(ButtonVariant::Plain, TextRole::Accent);
    label_roles.insert(ButtonVariant::Outlined, TextRole::Accent);
    label_roles.insert(ButtonVariant::Ghost, TextRole::Accent);

    RecipeButtonStyle {
        recipes,
        label_roles,
    }
}

/// Filled (primary) — high emphasis.
fn filled() -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::Pill,
        fill: PerStateRecipe {
            idle: FillRecipe::solid(SurfaceRole::Accent),
            hover: Some(FillRecipe::solid(SurfaceRole::AccentHover)),
            pressed: Some(FillRecipe::solid(SurfaceRole::AccentPressed)),
            focused: None,
            disabled: Some(FillRecipe::solid(SurfaceRole::AccentDisabled)),
        },
        border: PerStateRecipe::uniform(BorderRecipe::none()),
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(PADDING_H, 0.0),
        min_size: Size::new(0.0, HEIGHT),
    }
}

/// Filled (error) — destructive actions.
fn destructive() -> ButtonRecipe {
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
        padding: EdgeInsets::symmetric(PADDING_H, 0.0),
        min_size: Size::new(0.0, HEIGHT),
    }
}

/// Filled tonal — medium emphasis container.
fn tonal() -> ButtonRecipe {
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
        padding: EdgeInsets::symmetric(PADDING_H, 0.0),
        min_size: Size::new(0.0, HEIGHT),
    }
}

/// Outlined — low/medium emphasis with an outline.
fn outlined() -> ButtonRecipe {
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
        padding: EdgeInsets::symmetric(PADDING_H, 0.0),
        min_size: Size::new(0.0, HEIGHT),
    }
}

/// Text — lowest emphasis.
fn text() -> ButtonRecipe {
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
        padding: EdgeInsets::symmetric(PADDING_H_TEXT, 0.0),
        min_size: Size::new(0.0, HEIGHT),
    }
}
