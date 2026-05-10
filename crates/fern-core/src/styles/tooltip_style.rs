//! Tier-3 style protocol for `Tooltip`. See `docs/styling-system.md`.
//!
//! Distinct from [`crate::styles::PopoverStyle`] (which paints the
//! generic popover surface): this is the tooltip-flavored chrome
//! shipped by all three tooltip tiers (plain / rich / composite).

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct TooltipStyleConfig {
    /// Pre-built body subtree (text widget for plain; structured
    /// content for rich / composite).
    pub content: WidgetId,
}

pub trait TooltipStyle: 'static {
    fn make_body(&self, cfg: &TooltipStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedTooltipStyle = Rc<dyn TooltipStyle>;
