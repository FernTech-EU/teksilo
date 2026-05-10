//! Tier-3 style protocol for `Popover`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum PopoverVariant {
    /// Generic popover (combo-box dropdown, color picker, etc.).
    #[default]
    Default,
    /// Menu popover (rounded, slightly-elevated). Distinct from
    /// [`MenuItemStyle`] which paints the rows; this draws the
    /// surrounding container.
    Menu,
    /// Tooltip-flavored container (dark surface in IntUI even in
    /// light theme).
    Tooltip,
}

#[derive(Clone, Debug)]
pub struct PopoverStyleConfig {
    pub content: WidgetId,
    pub variant: PopoverVariant,
}

pub trait PopoverStyle: 'static {
    fn make_body(&self, cfg: &PopoverStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedPopoverStyle = Rc<dyn PopoverStyle>;
