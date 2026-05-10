//! Tier-3 style protocol for `Toggle`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Toggle visual archetype. Most styles compose Switch; the others
/// give designers explicit alternates without forking the widget.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum ToggleVariant {
    /// Capsule track + circular knob that slides on (the IntUI default
    /// and most other 2026-era look-and-feels).
    #[default]
    Switch,
    /// Pill-shaped track without a sliding knob — a colored / muted
    /// fill toggles instead.
    Pill,
    /// Square track with a square knob (brutalist / 8-bit aesthetic).
    Square,
    /// Inset toggle — the entire control is a flat surface that
    /// recesses on activation.
    Inset,
}

#[derive(Clone, Debug)]
pub struct ToggleStyleConfig {
    /// Whether the toggle is currently on. The style usually animates
    /// the knob position from this signal directly via
    /// [`Signal::animate_to`](crate::signal::Signal).
    pub is_on: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub variant: ToggleVariant,
}

pub trait ToggleStyle: 'static {
    fn make_body(&self, cfg: &ToggleStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedToggleStyle = Rc<dyn ToggleStyle>;
