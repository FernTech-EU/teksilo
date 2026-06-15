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
}

pub trait TextInputStyle: 'static {
    fn make_body(&self, cfg: &TextInputStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedTextInputStyle = Rc<dyn TextInputStyle>;
