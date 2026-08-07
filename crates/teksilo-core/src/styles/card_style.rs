// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Card`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::color_prop::ColorProp;
use crate::signal::{Prop, Signal};
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum CardVariant {
    Plain,
    #[default]
    Elevated,
    Outlined,
    Filled,
}

#[derive(Clone, Debug)]
pub struct CardStyleConfig {
    /// Pre-built content subtree wrapped by the card's chrome.
    pub content: WidgetId,
    /// `Some(signal)` if the card is interactive (hover lifts elevation).
    pub is_hovered: Option<Signal<bool>>,
    pub variant: CardVariant,
    /// Caller overrides — when `Some`, the style should use these
    /// values instead of the variant's defaults. Custom styles may
    /// ignore them.
    pub background_override: Option<ColorProp>,
    pub corner_radius_override: Option<Prop<f32>>,
    pub padding_override: Option<Prop<f32>>,
    /// Optional caller-supplied shadow override (replaces variant's
    /// default shadow).
    pub shadow_override: Option<teksilo_tokens::Shadow>,
}

pub trait CardStyle: 'static {
    fn make_body(&self, cfg: &CardStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedCardStyle = Rc<dyn CardStyle>;
