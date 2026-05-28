//! Default `PopoverStyle` impl driven by paint-recipe data.
//!
//! `RecipePopoverStyle` constructs the IntUI `PopoverSurface`
//! (`crates/bastyde-widgets/src/popover.rs`) — an elevated panel with
//! `surface_main` background, accent-aware shadow whose attached side
//! is suppressed (so the panel reads as connected to its trigger),
//! and an optional directional caret that points at the trigger.
//!
//! Apps wanting a different chrome (frosted-glass popover, brutalist
//! flat panel, custom caret) write their own `impl PopoverStyle`
//! block and install per-call (`Popover::style(...)`) or theme-wide
//! (`theme.style_slots.popover = Some(Rc::new(MyPopover))`).

use bastyde_canvas::EdgeInsets;
use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{PopoverStyle, PopoverStyleConfig, PopoverVariant};
use bastyde_core::widget::PendingChild;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::SurfaceRole;

use crate::popover::PopoverSurface;

// IntUI design tokens for Popover. The recipe and surface own their
// own dimensions.
//
// `POPOVER_PADDING` is the content inset for Default/Tooltip-flavoured
// popovers. The `Menu` variant uses zero content padding so menu rows
// (which carry their own row padding) reach the surface edge.
pub const POPOVER_PADDING: f32 = 16.0;
pub const POPOVER_CORNER_RADIUS: f32 = 8.0;
pub const POPOVER_BORDER_WIDTH: f32 = 1.0;
/// Corner radius for the `Menu`-flavoured popup surface (menu lists,
/// combo-box dropdowns, search-field suggestion panels).
pub const MENU_POPUP_CORNER_RADIUS: f32 = 8.0;
/// 0..=1 multiplier on `shape.shadow_inner_sm.color.a` at paint time.
pub const POPOVER_SHADOW_DENSITY: f32 = 0.5;

/// Default `PopoverStyle` shipped with Bastyde. The `Default` and
/// `Tooltip` variants produce the elevated `surface_main` panel with
/// 16 px content padding; the `Menu` variant produces a `surface_raised`
/// panel with zero content padding (so menu rows reach the edge) and a
/// presentational a11y node (the caller owns the container role).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipePopoverStyle;

impl PopoverStyle for RecipePopoverStyle {
    fn make_body(&self, cfg: &PopoverStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let (content_padding, background, corner_radius, presentational) = match cfg.variant {
            PopoverVariant::Menu => (
                EdgeInsets::ZERO,
                SurfaceRole::Raised,
                MENU_POPUP_CORNER_RADIUS,
                true,
            ),
            PopoverVariant::Default | PopoverVariant::Tooltip => (
                EdgeInsets::uniform(POPOVER_PADDING),
                SurfaceRole::Main,
                POPOVER_CORNER_RADIUS,
                false,
            ),
        };

        ctx.add(PopoverSurface::new(
            PendingChild::Id(cfg.content),
            cfg.placement.clone(),
            cfg.show_caret,
            cfg.caret_size,
            cfg.name.clone(),
            content_padding,
            background,
            corner_radius,
            presentational,
        ))
    }
}
