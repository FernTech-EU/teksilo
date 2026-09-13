// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default [`GridViewStyle`] impl — the stock IntUI grid decoration.
//!
//! Every method falls through to the trait's default recipe (focus ring =
//! `BorderRole::Focused` 1.5 px inset 1 px; marquee = translucent
//! `Focused`; insertion bar = `BorderRole::Accent` 2 px; pinned header =
//! `SurfaceRole::Raised`). Apps wanting a different look write their own
//! `impl GridViewStyle` block and install it per-call (`GridView::style(...)`)
//! or theme-wide (`theme.style_slots.grid_view = Some(Rc::new(...))`).

use teksilo_core::styles::GridViewStyle;
use teksilo_tokens::InputTokens;

/// The stock grid decoration style. Unit struct — all chrome comes from the
/// trait defaults.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeGridViewStyle;

impl RecipeGridViewStyle {
    /// This style resolved against a density's [`InputTokens`], as
    /// `RecipeGridViewStyle::for_tokens(&ctx.theme().input)` at the widget's own
    /// build site.
    ///
    /// The style is a unit struct: every dimension it draws on comes from
    /// the [`GridViewStyle`] trait defaults, none of which is a target or a
    /// gap, so the projection is the identity at every density.
    pub fn for_tokens(_tokens: &InputTokens) -> Self {
        Self
    }
}

impl GridViewStyle for RecipeGridViewStyle {}
