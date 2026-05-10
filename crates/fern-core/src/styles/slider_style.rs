//! Tier-3 style protocol for `Slider`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum SliderVariant {
    #[default]
    Continuous,
    /// Snaps to discrete tick positions; the style typically paints
    /// the ticks above/below the track.
    Discrete,
    /// Two thumbs: the value is a `(low, high)` range. Here for
    /// completeness; the IntUI default impl does NOT yet wire range
    /// behaviour — apps that need range sliders write a custom
    /// impl.
    Range,
}

#[derive(Clone, Debug)]
pub struct SliderStyleConfig {
    /// Normalized `0.0..=1.0` thumb position.
    pub value_normalized: Signal<f32>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    /// `Some(n)` ⇒ Discrete with `n` ticks; `None` ⇒ Continuous.
    pub tick_count: Option<u32>,
    pub variant: SliderVariant,
}

pub trait SliderStyle: 'static {
    fn make_body(&self, cfg: &SliderStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedSliderStyle = Rc<dyn SliderStyle>;
