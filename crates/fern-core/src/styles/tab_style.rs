//! Tier-3 style protocol for `TabBar`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum TabBarOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug)]
pub struct TabStyleConfig {
    pub label: WidgetId,
    pub leading: Option<WidgetId>,
    pub trailing: Option<WidgetId>,
    pub is_active: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub orientation: TabBarOrientation,
}

pub trait TabStyle: 'static {
    fn make_body(&self, cfg: &TabStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedTabStyle = Rc<dyn TabStyle>;
