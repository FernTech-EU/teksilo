//! Tier-3 style protocol for `StandardListItem` / `StandardTreeItem`.
//! See `docs/styling-system.md`.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct StandardItemStyleConfig {
    pub label: WidgetId,
    /// Optional leading slot (checkbox, icon, drag handle).
    pub leading: Option<WidgetId>,
    /// Optional trailing slot (badge, chevron, action button).
    pub trailing: Option<WidgetId>,
    /// Optional secondary line beneath the label.
    pub subtitle: Option<WidgetId>,
    pub is_selected: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    /// Tree-row indent depth in logical pixels. `0.0` for flat lists.
    pub indent: f32,
}

pub trait StandardItemStyle: 'static {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedStandardItemStyle = Rc<dyn StandardItemStyle>;
