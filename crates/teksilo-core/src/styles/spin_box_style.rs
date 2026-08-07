// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `SpinBox`. See `docs/styling-system.md`.
//!
//! `SpinBox` composes a themed `TextInputField` with an optional pair
//! of step-up / step-down buttons (each themed via `IconButtonStyle`
//! at construction time inside `StepButton`). The remaining picker-
//! specific chrome is the *arrangement*: where the field sits, where
//! the buttons sit, and the focus-aware border that frames them as
//! one control.
//!
//! `make_body` receives the pre-built field plus optional step buttons
//! and returns the visual core (the bordered surface holding both).
//! The widget keeps responsibility for sizing policy (`width_chars` /
//! `width_pixels`), keyboard / wheel handlers, and the `Role::SpinButton`
//! accessibility.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Step-button visibility / placement. Moved up from
/// `teksilo_widgets::spin_box::ButtonLayout` so the trait config can carry
/// it without forcing the recipe to depend on the widget crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonLayout {
    /// Up arrow on top, down arrow below, stacked vertically to the
    /// right of the field. Default, matches Qt and WinUI.
    #[default]
    Stacked,
    /// No visible step buttons. Useful for read-only displays and
    /// for SpinBoxes driven entirely by keyboard / wheel.
    Hidden,
}

pub struct SpinBoxStyleConfig {
    /// Pre-built `TextInputField` subtree (already padded to the
    /// caller's policy).
    pub field: WidgetId,
    /// Pre-built up-step icon button. `None` when `layout == Hidden`.
    pub step_up: Option<WidgetId>,
    /// Pre-built down-step icon button. `None` when `layout == Hidden`.
    pub step_down: Option<WidgetId>,
    /// Layout selector — `Stacked` puts the buttons in a divided
    /// vertical column to the trailing side of the field; `Hidden`
    /// drops the divider and button column entirely.
    pub layout: ButtonLayout,
    /// Reactive focus signal — drives the border colour (focused →
    /// accent, otherwise default).
    pub is_focused: Signal<bool>,
    /// Reactive disabled signal — the AND of the SpinBox's own `enabled`
    /// prop and every ancestor's, via
    /// `BuildContext::effective_enabled_signal`. Drives the inert grey
    /// fill / outline. A SpinBox frames a `TextInputField` in *neutral*
    /// roles (`SurfaceRole::Content`, `BorderRole::Default`), and the
    /// disabled-role substitution in `ColorProp::resolve` only rewrites
    /// the *accent* family — so unlike an accent-filled Button, this
    /// control has to dim itself explicitly.
    pub is_disabled: Signal<bool>,
}

pub trait SpinBoxStyle: 'static {
    fn make_body(&self, cfg: &SpinBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedSpinBoxStyle = Rc<dyn SpinBoxStyle>;
