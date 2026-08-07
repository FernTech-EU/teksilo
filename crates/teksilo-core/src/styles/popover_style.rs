// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Popover`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::overlay::OverlayPlacement;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum PopoverVariant {
    /// Generic popover (combo-box dropdown, color picker, etc.).
    #[default]
    Default,
    /// Menu popover (rounded, slightly-elevated). Distinct from
    /// `MenuItemStyle` which paints the rows; this draws the
    /// surrounding container.
    Menu,
    /// Tooltip-flavored container (dark surface in IntUI even in
    /// light theme).
    Tooltip,
}

#[derive(Clone, Debug)]
pub struct PopoverStyleConfig {
    /// Pre-built content subtree that the popover frame wraps.
    pub content: WidgetId,
    pub variant: PopoverVariant,
    /// Accessible name for the popover dialog node — typically the
    /// trigger label.
    pub name: String,
    /// Overlay placement (drives caret direction + which side of the
    /// shadow to suppress so the panel reads as attached to its
    /// trigger).
    pub placement: OverlayPlacement,
    /// Whether to paint a directional caret pointing at the trigger.
    pub show_caret: bool,
    /// Caret half-extent in logical pixels (the apex protrudes by
    /// this amount). Honoured only when `show_caret == true` AND the
    /// placement is one of the cardinal Above/Below directions.
    pub caret_size: f32,
}

pub trait PopoverStyle: 'static {
    fn make_body(&self, cfg: &PopoverStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedPopoverStyle = Rc<dyn PopoverStyle>;
