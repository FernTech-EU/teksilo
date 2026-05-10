//! Tier-3 style protocol for `ComboBox`. See `docs/styling-system.md`.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct ComboBoxStyleConfig {
    /// Pre-built selected-item display subtree.
    pub selected_label: WidgetId,
    pub is_open: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
}

pub trait ComboBoxStyle: 'static {
    fn make_body(&self, cfg: &ComboBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedComboBoxStyle = Rc<dyn ComboBoxStyle>;
