// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for the touch text-selection chrome — the selection
//! handles and the magnifier that `crate::text_touch` raises.
//!
//! One of the few Tier-3 traits that returns **pure-data recipes only**, with
//! no `make_*(cfg, ctx) -> WidgetId` — the others are
//! [`ChartStyle`](crate::styles::ChartStyle) and
//! [`GridViewStyle`](crate::styles::GridViewStyle), and for the same reason:
//! the chrome is single-node batched paint. Here that is a disc on a stem and a
//! framed lens whose interior is a replay of the host's own text layer. A
//! composed subtree would buy nothing, while forcing the affordance widgets —
//! which live in `teksilo-core` — to depend on `teksilo-widgets`.
//!
//! The shipped default, `RecipeTextSelectionStyle`, therefore lives in
//! `teksilo-widgets` (`styles/recipe_text_selection_style.rs`); this crate
//! holds the trait, the recipes and the `Rc<dyn TextSelectionStyle>` slot type.
//!
//! # Density
//!
//! A recipe is built once per density with `for_tokens(&InputTokens)`. The two
//! dimensions are classified differently on purpose:
//!
//! * the **hit extent** is a [`TargetRole::Target`](teksilo_tokens::TargetRole)
//!   and routes through [`dp`](crate::styles::density::dp), so it can only grow
//!   with density;
//! * the **painted diameter** is a
//!   [`Decoration`](teksilo_tokens::TargetRole::Decoration) and is the same at
//!   every density. It is chrome already sized to a fingertip, and target
//!   conformance is measured on the hit rectangle, not on the paint — the rule
//!   `docs/density-and-targets.md` states for any affordance whose hit area is
//!   widened rather than its ink.

use std::rc::Rc;

use crate::styles::{RecipeColor, Theme};

/// Painted geometry and colours of one selection handle.
///
/// `hit_size` is the extent of the handle's own node — a square centred on the
/// anchor point — and `diameter` is the disc drawn inside it. The disc is
/// always centred in the hit square, so a caller that changes one without the
/// other still gets a centred handle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSelectionHandleRecipe {
    /// Diameter of the painted disc, in dp.
    pub diameter: f32,
    /// Extent of the square hit rectangle, in dp. Never below
    /// `InputTokens::min_target_conformance`.
    pub hit_size: f32,
    /// Width of the stem drawn from the disc to the caret it marks, in dp.
    /// Zero draws no stem.
    pub stem_width: f32,
    /// Fill of the disc and the stem.
    pub fill: RecipeColor,
    /// Ring drawn around the disc so it stays visible over text of its own
    /// colour. Zero `outline_width` draws none.
    pub outline: RecipeColor,
    /// Width of that ring, in dp.
    pub outline_width: f32,
}

/// Painted geometry and colours of the magnifier lens.
///
/// The lens interior is not painted by the style: it is a replay of the host's
/// own text layer under a transform and a clip (see
/// [`crate::text_touch::magnifier`]). The style owns only the frame around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMagnifierRecipe {
    /// Half the lens's width, in dp — the lens is `2 × radius` wide.
    pub radius: f32,
    /// Half the lens's height, in dp.
    pub half_height: f32,
    /// Magnification applied to the replayed content.
    pub scale: f32,
    /// How far above the contact point the lens's bottom edge sits, in dp.
    pub rise: f32,
    /// Corner radius of the painted frame, in dp.
    ///
    /// The **clip** is rectangular — see [`crate::text_touch::magnifier`] for
    /// why, and for what shows in the corners because of it.
    pub corner_radius: f32,
    /// Fill painted behind the replayed content, so glyphs from the layer
    /// beneath do not show through.
    pub background: RecipeColor,
    /// Frame drawn around the lens.
    pub border: RecipeColor,
    /// Width of that frame, in dp.
    pub border_width: f32,
}

/// Tier-3 protocol for the touch text-selection chrome. See the module docs.
pub trait TextSelectionStyle: 'static {
    /// Geometry and colours of a selection handle.
    fn handle(&self, theme: &Theme) -> TextSelectionHandleRecipe;
    /// Geometry and colours of the magnifier frame.
    fn magnifier(&self, theme: &Theme) -> TextMagnifierRecipe;
}

/// Shared handle type stored in
/// [`ComponentStyleSlots::text_selection`](crate::styles::ComponentStyleSlots).
pub type SharedTextSelectionStyle = Rc<dyn TextSelectionStyle>;
