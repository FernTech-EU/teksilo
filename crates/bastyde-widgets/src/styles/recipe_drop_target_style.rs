// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `DropTargetStyle` impl driven by paint-recipe data.
//!
//! `RecipeDropTargetStyle` ships the IntUI drop-target chrome: the wrapped
//! child fills the bounds and stays fully visible, a full-bleed `RectWidget`
//! overlay strokes a reactive highlight **border** (no fill — an opaque tint
//! would hide the child), and, when a hint slot is set, a popup `Card` holding
//! the hint is centered over the child while a drag with an accepted payload
//! hovers.
//!
//! The overlay layout (a `ZStack` of child + border-rect + centered hint) never
//! inflates the stack's intrinsic size: both `RectWidget` and `Center` report
//! 0×0 for an unspecified proposal and fill an exact one, so the target sizes
//! to exactly the wrapped child. The `DropTarget` widget sets `clips_children`,
//! keeping an oversized hint card inside the zone.
//!
//! The hint is gated with [`BuildContext::visible_when`] on a derived
//! "is the drag accepted-hovering?" signal: it culls both paint **and** the
//! accessibility node when idle, so a screen reader never meets the hint
//! prompt while at rest. `Live::Polite` on the card announces it appearing.
//!
//! Apps wanting a different look (dashed border, translucent wash, glow, no
//! popup) write their own `impl DropTargetStyle` block and install it per-call
//! (`DropTarget::style(...)`) or theme-wide
//! (`theme.style_slots.drop_target = Some(Rc::new(...))`). The
//! [`DropTargetDragState::surface_role`] helper is there for styles that do
//! want a (translucent) fill.

use bastyde_core::accesskit::Live;
use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{
    DropTargetDragState, DropTargetStyle, DropTargetStyleConfig, DropTargetVariant,
};
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

use crate::card::Card;
use crate::primitives::{Center, RectWidget, ZStack};

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

impl Default for DropTargetRecipe {
    fn default() -> Self {
        Self {
            corner_radius: DROP_TARGET_CORNER_RADIUS,
            border_width_default: DROP_TARGET_BORDER_WIDTH_DEFAULT,
            border_width_prominent: DROP_TARGET_BORDER_WIDTH_PROMINENT,
            border_width_subtle: DROP_TARGET_BORDER_WIDTH_SUBTLE,
        }
    }
}

/// Default `DropTargetStyle` shipped with Bastyde.
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
        let mut zstack = ZStack::new().add_child(cfg.content_id);

        // Reactive highlight border over the child (skipped for `None`). Only a
        // stroke — no fill — so the child is never hidden. The border color
        // tracks the drag state (Idle → transparent, so nothing shows at rest).
        // It is `event_pass_through` so this decorative overlay never steals
        // pointer events from the wrapped child — otherwise wrapping interactive
        // content (a tree row's expand chevron, a button) in a `DropTarget`
        // would silently break it, since the full-bounds rect sits on top.
        if cfg.variant != DropTargetVariant::None {
            let border = cfg.drag_state.map(|s| s.border_role());
            let rect = ctx.add(
                RectWidget::new()
                    .border_color(border)
                    .border_width(border_width)
                    .corner_radius(CornerRadius::uniform(self.recipe.corner_radius))
                    .event_pass_through(true),
            );
            zstack = zstack.add_child(rect);
        }

        // Centered popup hint, shown only while an accepted payload hovers.
        if let Some(hint_id) = cfg.hint_id {
            // Derived (read-only) bool — `visible_when` accepts it and culls
            // both paint and the AT node when false.
            let hint_visible = cfg
                .drag_state
                .map(|s| *s == DropTargetDragState::HoverAccept);
            // Live::Polite on the card → announce the hint *appearing*, not
            // arbitrary changes to the wrapped child's content.
            let card = ctx.add(Card::new().content_id(hint_id).access_live(Live::Polite));
            let center = ctx.add(Center::new().child_id(card));
            ctx.visible_when(center, hint_visible);
            zstack = zstack.add_child(center);
        }

        ctx.add(zstack)
    }
}
