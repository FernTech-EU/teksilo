//! Tier-3 style protocol for `MenuItem`. See `docs/styling-system.md`.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct MenuItemStyleConfig {
    pub label: WidgetId,
    /// Optional leading slot (icon, checkmark, radio dot).
    pub leading: Option<WidgetId>,
    /// Optional trailing slot (shortcut chip, submenu chevron).
    pub trailing: Option<WidgetId>,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    /// Bound to keyboard-arrow navigation within the parent menu.
    pub is_highlighted: Signal<bool>,
}

pub trait MenuItemStyle: 'static {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedMenuItemStyle = Rc<dyn MenuItemStyle>;
