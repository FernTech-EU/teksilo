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

/// The stock grid decoration style. Unit struct — all chrome comes from the
/// trait defaults.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeGridViewStyle;

impl GridViewStyle for RecipeGridViewStyle {}
