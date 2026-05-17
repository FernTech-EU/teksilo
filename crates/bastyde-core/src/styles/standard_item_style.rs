//! Tier-3 style protocol for `StandardListItem` / `StandardTreeItem`.
//! See `docs/styling-system.md`.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct StandardItemStyleConfig {
    /// Pre-built row content — typically an HStack of `[checkbox?]
    /// [leading?] [center?] [label / VStack { label, subtitle row }]
    /// [Spacer] [trailing?]` composed by the host
    /// `StandardListItem` / `StandardTreeItem`. The style is
    /// responsible for the chrome (selection background, corner
    /// radius, padding) but not for row-internal layout — the
    /// per-slot composition is StandardItem-specific (subtitle has
    /// its own sub-row, the checkbox carries `labels_hidden` AT
    /// metadata, etc.) and would force every custom style to
    /// reimplement it.
    pub content: WidgetId,
    pub is_selected: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
}

pub trait StandardItemStyle: 'static {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedStandardItemStyle = Rc<dyn StandardItemStyle>;
