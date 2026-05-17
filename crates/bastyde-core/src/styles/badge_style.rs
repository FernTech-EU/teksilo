//! Tier-3 style protocol for `Badge`. See `docs/styling-system.md`.
//!
//! Themes the pill-shaped label chrome — the rounded background fill
//! and the content padding inset. `Badge` builds the label
//! `TextWidget` (with the resolved text color) itself; `BadgeStyle`
//! only owns the surrounding pill.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::color_prop::ColorProp;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct BadgeStyleConfig {
    /// Pre-built label subtree the pill wraps.
    pub content: WidgetId,
    /// Caller override for the pill background — `None` means "use the
    /// recipe default" (`SurfaceRole::AccentSubtle`). Custom styles may
    /// ignore it.
    pub background_override: Option<ColorProp>,
}

pub trait BadgeStyle: 'static {
    fn make_body(&self, cfg: &BadgeStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedBadgeStyle = Rc<dyn BadgeStyle>;
