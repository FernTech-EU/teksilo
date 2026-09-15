// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `DropTargetStyle` impl driven by paint-recipe data.
//!
//! `RecipeDropTargetStyle` ships the IntUI drop-target chrome: the wrapped
//! child fills the bounds and stays fully visible; a full-bleed `RectWidget`
//! strokes a reactive rounded **whole-bounds** border (error on reject, accent
//! on a `Center` accept — no fill, an opaque tint would hide the child); a
//! `DropRegionOverlay` paints the active **side** zone's highlight (an edge
//! strip → translucent fill + accent frame) and hosts the per-region hint cards;
//! and each hint is a popup `Card` centered within its region's rect, shown only
//! while that zone is the active accepted-hover.
//!
//! The overlay layout (a `ZStack` of child + reject-rect + region-overlay) never
//! inflates the stack's intrinsic size: `RectWidget` and `DropRegionOverlay`
//! report 0×0 for an unspecified proposal and fill an exact one, so the target
//! sizes to exactly the wrapped child. The `DropTarget` widget sets
//! `clips_children`, keeping an oversized hint card inside its zone.
//!
//! Each hint is gated with [`BuildContext::visible_when`] on a derived
//! "is *this* region the active accepted-hover?" signal: it culls both paint
//! **and** the accessibility node when its zone isn't active, so a screen reader
//! never meets an inactive zone's prompt. `Live::Polite` on the card announces
//! it appearing.
//!
//! The decorative chrome (the reject-border `RectWidget` and, when the target
//! declares no hints, the `DropRegionOverlay`) is **hidden from the AT tree** —
//! a hint-less multi-zone target (e.g. a docking pane) adds no empty container
//! per drop target.
//!
//! Apps wanting a different look (dashed border, translucent wash, glow, no
//! popup) write their own `impl DropTargetStyle` block and install it per-call
//! (`DropTarget::style(...)`) or theme-wide
//! (`theme.style_slots.drop_target = Some(Rc::new(...))`). The
//! [`DropTargetDragState::surface_role`] helper is there for styles that do
//! want a (translucent) fill.

use teksilo_core::accesskit::Live;
use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{
    DropRegion, DropTargetDragState, DropTargetStyle, DropTargetStyleConfig, DropTargetVariant,
};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, CornerRadius, InputTokens};

use crate::card::Card;
use crate::drop_target::overlay::DropRegionOverlay;
use crate::primitives::{RectWidget, ZStack};

/// Corner radius of the overlay's rounded border.
pub const DROP_TARGET_CORNER_RADIUS: f32 = 8.0;
/// Border thickness for the `Default` variant.
pub const DROP_TARGET_BORDER_WIDTH_DEFAULT: f32 = 2.0;
/// Border thickness for the `Prominent` variant.
pub const DROP_TARGET_BORDER_WIDTH_PROMINENT: f32 = 3.0;
/// Border thickness for the `Subtle` variant.
pub const DROP_TARGET_BORDER_WIDTH_SUBTLE: f32 = 1.0;

/// Configurable dimensions for [`RecipeDropTargetStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropTargetRecipe {
    /// Corner radius of the overlay's rounded border.
    pub corner_radius: f32,
    /// Border thickness for the `Default` variant.
    pub border_width_default: f32,
    /// Border thickness for the `Prominent` variant.
    pub border_width_prominent: f32,
    /// Border thickness for the `Subtle` variant.
    pub border_width_subtle: f32,
}

impl DropTargetRecipe {
    /// This recipe's dimensions resolved against a density's [`InputTokens`].
    ///
    /// [`Default`] is `for_tokens(&InputTokens::default())` — the Compact
    /// ladder — so the shipped values below are the Compact column by
    /// construction and cannot drift from it.
    ///
    /// Every dimension this recipe carries is a decoration (a corner radius, a
    /// hairline, a glyph metric), so the parameter is unused: the density
    /// ladder never moves any of them. It is taken all the same, so every
    /// recipe is constructed the same way at its widget's build site.
    pub fn for_tokens(_tokens: &InputTokens) -> Self {
        Self {
            corner_radius: DROP_TARGET_CORNER_RADIUS,
            border_width_default: DROP_TARGET_BORDER_WIDTH_DEFAULT,
            border_width_prominent: DROP_TARGET_BORDER_WIDTH_PROMINENT,
            border_width_subtle: DROP_TARGET_BORDER_WIDTH_SUBTLE,
        }
    }
}

impl Default for DropTargetRecipe {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

/// Default `DropTargetStyle` shipped with Teksilo.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeDropTargetStyle {
    /// Tunable dimensions for this style instance.
    pub recipe: DropTargetRecipe,
}

impl RecipeDropTargetStyle {
    /// Create a style with custom recipe dimensions.
    pub fn new(recipe: DropTargetRecipe) -> Self {
        Self { recipe }
    }

    /// This style with every dimension resolved against a density's
    /// [`InputTokens`], as `RecipeDropTargetStyle::for_tokens(&ctx.theme().input)` at
    /// the widget's own build site.
    ///
    /// [`Default`] is the `TargetDensity::Compact` projection, so a Compact
    /// tree gets exactly the values this module documents.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            recipe: DropTargetRecipe::for_tokens(tokens),
        }
    }
}

impl DropTargetStyle for RecipeDropTargetStyle {
    fn make_body(&self, cfg: &DropTargetStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let border_width = match cfg.variant {
            DropTargetVariant::Default => self.recipe.border_width_default,
            DropTargetVariant::Prominent => self.recipe.border_width_prominent,
            DropTargetVariant::Subtle => self.recipe.border_width_subtle,
            DropTargetVariant::None => 0.0,
        };

        // The wrapped child fills the bounds and is always visible.
        let mut zstack = ZStack::new().child(cfg.content_id);

        // Full-bounds rounded border for the whole-bounds states: a reject error
        // border, and the `Center` accept border (so single-zone accept keeps its
        // rounded corners — the overlay paints only the *side* zones). Side-zone
        // accept and idle leave it transparent. Only a stroke — no fill — so the
        // child is never hidden, and `event_pass_through` so this decorative
        // overlay never steals pointer events from the wrapped content. Skipped
        // for `None`.
        if cfg.variant != DropTargetVariant::None {
            let border = cfg
                .drag_state
                .zip(&cfg.active_region)
                .map(|(s, r)| match s {
                    DropTargetDragState::HoverReject => BorderRole::Error,
                    DropTargetDragState::HoverAccept if *r == Some(DropRegion::Center) => {
                        BorderRole::Accent
                    }
                    _ => BorderRole::Transparent,
                });
            let rect = ctx.add(
                RectWidget::new()
                    .border_color(border)
                    .border_width(border_width)
                    .corner_radius(CornerRadius::uniform(self.recipe.corner_radius))
                    .event_pass_through(true)
                    // Decorative highlight border — keep it out of the AT tree.
                    .access_hidden(true),
            );
            zstack = zstack.child(rect);
        }

        // Per-region hint cards, each shown only while *its* region is the
        // active accepted-hover. `visible_when` culls both paint and the AT
        // node when a region isn't active; `Live::Polite` announces the hint
        // *appearing*. The cards are hosted (and placed inside their region
        // rect) by the `DropRegionOverlay` below — so we pass their ids on.
        let mut hint_cards: Vec<(DropRegion, WidgetId)> = Vec::new();
        for &(region, hint_id) in &cfg.region_hints {
            let card = ctx.add(Card::new().content(hint_id).access_live(Live::Polite));
            let visible = cfg.active_region.map(move |r| *r == Some(region));
            ctx.visible_when(card, visible);
            hint_cards.push((region, card));
        }

        // The reactive zone highlight + hint host. It paints the active region's
        // affordance (frame over the child — `event_pass_through`, so it never
        // steals pointer events from the wrapped interactive content) and places
        // each hint card centered within its zone. Skipped entirely only when
        // there's nothing for it to do (no border and no hints).
        if border_width > 0.0 || !hint_cards.is_empty() {
            let overlay = ctx.add(DropRegionOverlay::new(
                cfg.active_region.clone(),
                cfg.size_factor,
                // The same floor the target's own hit test applies, from the
                // same token, so the painted zone and the dropping zone are one
                // rectangle.
                ctx.theme().input.target_size,
                border_width,
                hint_cards,
            ));
            zstack = zstack.child(overlay);
        }

        ctx.add(zstack)
    }
}
