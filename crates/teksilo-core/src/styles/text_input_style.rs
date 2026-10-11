// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `TextInput`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum TextInputVariant {
    /// IntUI default: 1 dp bordered rectangle.
    #[default]
    Outlined,
    /// Filled background, no border (Material-3-ish).
    Filled,
    /// Just a baseline underline.
    Underline,
    /// No chrome at all — for embedded fields where the parent
    /// surface IS the chrome (search fields, ComboBox text part).
    Bare,
}

/// Validation outcome state, separate from interaction state. Drives
/// border-color tinting and the per-state recipe lookup independently
/// of focus/hover.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum TextInputValidationLevel {
    #[default]
    None,
    Info,
    Warning,
    Error,
    /// Brief accent tint after a successful auto-correction.
    Corrected,
}

#[derive(Clone, Debug)]
pub struct TextInputStyleConfig {
    /// The actual editable area subtree (caret, glyph layout, IME).
    /// The style wraps it with border / fill / focus-ring chrome.
    pub editor: WidgetId,
    pub is_focused: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub validation: Signal<TextInputValidationLevel>,
    pub variant: TextInputVariant,
    /// The minimum height the widget resolved for the field, in logical
    /// pixels, when it differs from the style's own: an app's explicit
    /// `TextInput::min_height`, or the density's target size when a text
    /// style sizes the field from its line. `None` — the default — leaves the
    /// style its own control height.
    ///
    /// A style that floors its chrome with a minimum height should use this in
    /// its place when it is set; one that ignores it keeps its own floor, and
    /// the field can then be no shorter than that.
    pub min_height: Option<f32>,
}

pub trait TextInputStyle: 'static {
    fn make_body(&self, cfg: &TextInputStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedTextInputStyle = Rc<dyn TextInputStyle>;
