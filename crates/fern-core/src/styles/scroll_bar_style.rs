//! Tier-3 style protocol for `ScrollBar`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum ScrollBarOrientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum ScrollBarVariant {
    /// Translucent overlay scrollbar that thickens on hover (IntUI
    /// default).
    #[default]
    Overlay,
    /// Always-visible plain scrollbar (legacy / accessibility setting).
    Plain,
}

#[derive(Clone, Debug)]
pub struct ScrollBarStyleConfig {
    /// Track range as a fraction of the visible area: `0.0..=1.0`.
    pub thumb_start: Signal<f32>,
    pub thumb_end: Signal<f32>,
    pub is_hovered: Signal<bool>,
    pub is_dragging: Signal<bool>,
    pub orientation: ScrollBarOrientation,
    pub variant: ScrollBarVariant,
}

pub trait ScrollBarStyle: 'static {
    fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedScrollBarStyle = Rc<dyn ScrollBarStyle>;
