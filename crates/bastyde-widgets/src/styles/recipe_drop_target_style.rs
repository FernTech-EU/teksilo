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

/// Default `DropTargetStyle` shipped with Bastyde.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeDropTargetStyle;

impl DropTargetStyle for RecipeDropTargetStyle {
    fn make_body(&self, cfg: &DropTargetStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let border_width = match cfg.variant {
            DropTargetVariant::Default => DROP_TARGET_BORDER_WIDTH_DEFAULT,
            DropTargetVariant::Prominent => DROP_TARGET_BORDER_WIDTH_PROMINENT,
            DropTargetVariant::Subtle => DROP_TARGET_BORDER_WIDTH_SUBTLE,
            DropTargetVariant::None => 0.0,
        };

        // The wrapped child fills the bounds and is always visible.
        let mut zstack = ZStack::new().add_child(cfg.content_id);

        // Reactive highlight border over the child (skipped for `None`). Only a
        // stroke — no fill — so the child is never hidden. The border color
        // tracks the drag state (Idle → transparent, so nothing shows at rest).
        if cfg.variant != DropTargetVariant::None {
            let border = cfg.drag_state.map(|s| s.border_role());
            let rect = ctx.add(
                RectWidget::new()
                    .border_color(border)
                    .border_width(border_width)
                    .corner_radius(CornerRadius::uniform(DROP_TARGET_CORNER_RADIUS)),
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
