//! Tier-3 style protocol for `Card`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum CardVariant {
    #[default]
    Plain,
    Elevated,
    Outlined,
    Filled,
}

#[derive(Clone, Debug)]
pub struct CardStyleConfig {
    pub content: WidgetId,
    /// `Some(signal)` if the card is interactive (hover lifts elevation).
    pub is_hovered: Option<Signal<bool>>,
    pub variant: CardVariant,
}

pub trait CardStyle: 'static {
    fn make_body(&self, cfg: &CardStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedCardStyle = Rc<dyn CardStyle>;
