// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `ComboBox`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Tier-1 design-language variant for a combo box trigger. Mirrors the
/// `TextInputVariant` shape because both controls live in the same
/// "form-field" visual family — apps that ship a Material 3 theme
/// typically want filled triggers for both, etc.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum ComboBoxVariant {
    /// IntUI default: 1 dp bordered rectangle with a vertical divider
    /// before the chevron.
    #[default]
    Outlined,
    /// Filled background, no border (Material 3 dropdown style).
    Filled,
    /// Just a baseline underline under the trigger.
    Underline,
    /// No chrome at all — for embedded combo boxes where the parent
    /// surface IS the chrome (table cells, inline pickers).
    Plain,
}

#[derive(Clone, Debug)]
pub struct ComboBoxStyleConfig {
    /// Pre-built selected-item display subtree.
    pub selected_label: WidgetId,
    pub is_open: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub variant: ComboBoxVariant,
}

pub trait ComboBoxStyle: 'static {
    fn make_body(&self, cfg: &ComboBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedComboBoxStyle = Rc<dyn ComboBoxStyle>;
