//! Tier-3 style protocol for `Panel`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum PanelVariant {
    /// Bare surface, no border.
    #[default]
    Plain,
    /// Sunken below the parent surface (form sections).
    Sunken,
    /// Raised with a subtle shadow.
    Raised,
    /// Highlighted accent surface (info banners, on-boarding).
    Highlighted,
}

#[derive(Clone, Debug)]
pub struct PanelStyleConfig {
    pub content: WidgetId,
    pub variant: PanelVariant,
}

pub trait PanelStyle: 'static {
    fn make_body(&self, cfg: &PanelStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedPanelStyle = Rc<dyn PanelStyle>;
