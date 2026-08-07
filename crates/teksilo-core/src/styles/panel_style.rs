// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Panel`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::color_prop::ColorProp;
use crate::signal::Prop;
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
    /// Pre-built content subtree wrapped by the panel's chrome.
    pub content: WidgetId,
    pub variant: PanelVariant,
    /// Caller overrides — when `Some`, the style should use these
    /// values instead of the variant's defaults. Custom styles may
    /// ignore them, in which case the override is silently dropped.
    pub background_override: Option<ColorProp>,
    pub border_color_override: Option<ColorProp>,
    pub border_width_override: Option<Prop<f32>>,
    pub corner_radius_override: Option<Prop<f32>>,
    pub padding_override: Option<Prop<f32>>,
}

pub trait PanelStyle: 'static {
    fn make_body(&self, cfg: &PanelStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedPanelStyle = Rc<dyn PanelStyle>;
