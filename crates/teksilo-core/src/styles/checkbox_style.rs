// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Checkbox`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum CheckboxVariant {
    /// IntUI default: rounded square with a check glyph.
    #[default]
    Square,
    /// Softer rounded variant.
    Rounded,
    /// Circular checkmark (Material-3 style).
    Circle,
}

/// Tristate checkbox state. Mirrors `teksilo_data::CheckState` but lives
/// in teksilo-core so the style trait stays inside the foundation crate.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum CheckboxState {
    #[default]
    Unchecked,
    Checked,
    Indeterminate,
}

#[derive(Clone, Debug)]
pub struct CheckboxStyleConfig {
    pub state: Signal<CheckboxState>,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub variant: CheckboxVariant,
}

pub trait CheckboxStyle: 'static {
    fn make_body(&self, cfg: &CheckboxStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedCheckboxStyle = Rc<dyn CheckboxStyle>;
