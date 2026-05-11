//! Default `PopoverStyle` impl driven by paint-recipe data.
//!
//! `RecipePopoverStyle` constructs the IntUI [`PopoverSurface`]
//! (`crates/fern-widgets/src/popover.rs`) — an elevated panel with
//! `surface_main` background, accent-aware shadow whose attached side
//! is suppressed (so the panel reads as connected to its trigger),
//! and an optional directional caret that points at the trigger.
//!
//! Apps wanting a different chrome (frosted-glass popover, brutalist
//! flat panel, custom caret) write their own `impl PopoverStyle`
//! block and install per-call (`Popover::style(...)`) or theme-wide
//! (`theme.style_slots.popover = Some(Rc::new(MyPopover))`).

use fern_core::build_context::BuildContext;
use fern_core::styles::{PopoverStyle, PopoverStyleConfig};
use fern_core::widget::PendingChild;
use fern_core::widget_id::WidgetId;

use crate::popover::PopoverSurface;

/// Default `PopoverStyle` shipped with FernUI. Reads dimensions from
/// `theme.shape.radius_popup`, `theme.shape.shadow_sm`, and
/// `theme.components.popover.shadow_density` at paint time.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipePopoverStyle;

impl PopoverStyle for RecipePopoverStyle {
    fn make_body(&self, cfg: &PopoverStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // The variant is currently a hint only — the IntUI default
        // ships one chrome shape across Default/Menu/Tooltip and lets
        // the inner content (MenuList rows, tooltip text, etc.)
        // distinguish them. Step 7 + custom-chrome work can branch on
        // `cfg.variant` here without touching the trait surface.
        let _ = cfg.variant;

        ctx.add(PopoverSurface::new(
            PendingChild::Id(cfg.content),
            cfg.placement.clone(),
            cfg.show_caret,
            cfg.caret_size,
            cfg.name.clone(),
        ))
    }
}
