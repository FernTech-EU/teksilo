// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Button`.
//!
//! See `docs/styling-system.md`. The trait is object-safe so
//! `Rc<dyn ButtonStyle>` can be stored in a theme slot or attached
//! per-call via `Button::style(...)`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use teksilo_canvas::{EdgeInsets, Size};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::styles::recipe::{BorderRecipe, FillRecipe, PerStateRecipe, ShadowRecipe, ShapeRecipe};
use crate::widget_id::WidgetId;

/// Closed enum naming the design-language variants of `Button`. Set
/// per-call via `Button::variant(ButtonVariant::Outlined)` or
/// per-app default via a `ComponentDefaults` extension.
///
/// Variants are *hints* the active [`ButtonStyle`] may honour or
/// remap. The IntUI default `RecipeButtonStyle` collapses some pairs:
/// Tinted/Outlined → Plain look, Link → Ghost, Destructive → Filled
/// (the warning lives in the dialog title, not the button).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum ButtonVariant {
    Filled,
    Tinted,
    Outlined,
    #[default]
    Plain,
    Ghost,
    Link,
    Destructive,
}

/// Inputs handed to a [`ButtonStyle::make_body`] call.
///
/// `label` is a pre-built subtree (the active style only arranges
/// chrome around it; it never builds the label itself). The four
/// boolean signals carry the live interaction state — the style can
/// `.zip` / `.map` them to derive a [`crate::styles::WidgetState`] if
/// it wants to pick between per-state recipes.
#[derive(Clone, Debug)]
pub struct ButtonStyleConfig {
    pub label: WidgetId,
    pub is_pressed: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub variant: ButtonVariant,
}

/// Style protocol for `Button`. The active style owns *all* paint and
/// layering — it receives the label subtree pre-built and arranges
/// background, border, focus ring, padding, etc. around it.
///
/// `'static` (no `Send + Sync`) because the rest of teksilo-core is
/// already single-threaded (`Signal` uses `Rc`); enforcing thread
/// safety here would be inconsistent and pay no benefit.
pub trait ButtonStyle: 'static {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId;

    /// Optional per-variant override of the label/icon text role.
    ///
    /// The `Button` picks its label color from its built-in
    /// variant→role mapping (`OnAccent` for accent-filled variants,
    /// `Primary` otherwise, `Link` for `Link`) *before* the style runs.
    /// Returning `Some(role)` here lets a design-language style redirect
    /// it — e.g. Material 3 paints text/outlined buttons in the accent
    /// color (`TextRole::Accent`) rather than `Primary`.
    ///
    /// Default `None` preserves the built-in mapping, so existing styles
    /// (and the IntUI default) are unaffected. A per-call
    /// `Button::text_role(...)` still wins over this.
    fn label_text_role(&self, _variant: ButtonVariant) -> Option<teksilo_tokens::TextRole> {
        None
    }
}

/// Shared handle for a `ButtonStyle` impl. Cheap to clone; one shared
/// `Rc` is used per theme slot and per-call override.
pub type SharedButtonStyle = Rc<dyn ButtonStyle>;

/// Tier-2 paint-recipe for one variant of `Button`. The default
/// [`crate::styles::ButtonStyle`] impl shipped in `teksilo-widgets`
/// (`RecipeButtonStyle`) holds a `HashMap<ButtonVariant, ButtonRecipe>`
/// and looks up the recipe at paint time.
///
/// Custom `ButtonStyle` impls can ignore recipes entirely (paint a
/// glassmorphism gradient, run their own canvas code, etc.); the
/// recipe layer is the *default* surface, not an obligation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ButtonRecipe {
    pub shape: ShapeRecipe,
    pub fill: PerStateRecipe<FillRecipe>,
    pub border: PerStateRecipe<BorderRecipe>,
    pub shadow: PerStateRecipe<Option<ShadowRecipe>>,
    pub padding: EdgeInsets,
    pub min_size: Size,
}
