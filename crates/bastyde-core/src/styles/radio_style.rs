//! Tier-3 style protocol for `RadioButton`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum RadioVariant {
    /// IntUI default: filled inner dot inside a ring.
    #[default]
    Circle,
    /// Square with an inner check glyph (rare; some accessibility kits
    /// prefer it for distinguishability from the round Toggle).
    Square,
    /// Rounded square — softer than Square but distinct from Circle.
    Rounded,
}

#[derive(Clone, Debug)]
pub struct RadioStyleConfig {
    pub is_selected: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub variant: RadioVariant,
}

pub trait RadioStyle: 'static {
    fn make_body(&self, cfg: &RadioStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedRadioStyle = Rc<dyn RadioStyle>;
