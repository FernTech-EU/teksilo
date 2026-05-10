//! Tier-3 style protocol for `IconButton`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Five sizes calibrated to the IntelliJ Int UI scale. Apps usually
/// pick one per call; the active style maps each to a (square_size,
/// icon_size) pair from its recipe.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum IconButtonSize {
    Compact,
    #[default]
    Default,
    Toolbar,
    Large,
    Hero,
}

#[derive(Clone, Debug)]
pub struct IconButtonStyleConfig {
    /// Pre-built icon subtree.
    pub icon: WidgetId,
    pub is_pressed: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub size: IconButtonSize,
}

pub trait IconButtonStyle: 'static {
    fn make_body(&self, cfg: &IconButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedIconButtonStyle = Rc<dyn IconButtonStyle>;
