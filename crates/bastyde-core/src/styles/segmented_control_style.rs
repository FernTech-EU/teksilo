// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `SegmentedControl`. See
//! `docs/styling-system.md`.
//!
//! Themes the segmented-control chrome *behind* the segments: the outer
//! rounded frame, the per-segment hover tint, the selected-segment
//! surface + border, the focus ring. Labels and icons are composed
//! widgets owned by the `SegmentedControl` (so they stay locale- and
//! theme-reactive); the chrome paints no text or icons. The widget keeps
//! its `Role::RadioGroup` + per-segment `Role::RadioButton` semantics and
//! dispatches taps/keys; it owns no `paint()` of its own.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::focus::FocusOrigin;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct SegmentedControlStyleConfig {
    /// Number of segments — the chrome divides its inner width evenly to
    /// place the per-segment hover / selected backgrounds. Labels/icons
    /// are composed widgets the chrome does not draw.
    pub segment_count: usize,
    /// Current selection.
    pub selected: Signal<usize>,
    /// `Some(index)` while the pointer is over a segment. The recipe
    /// reads this to paint the hover tint behind the non-selected
    /// segment under the pointer.
    pub hovered_segment: Signal<Option<usize>>,
    /// Current focus origin (`None` when unfocused). Any focus
    /// triggers the accent-tinted selected-segment appearance; only
    /// keyboard focus paints the outer focus ring envelope.
    pub focus_origin: Signal<Option<FocusOrigin>>,
    /// Reactive — re-emits when arena `enabled_state` flips for this
    /// widget (or any ancestor). Chrome implementations should
    /// subscribe at `BindingLevel::RepaintOnly` so they re-paint on
    /// flip.
    pub is_enabled: Signal<bool>,
}

pub trait SegmentedControlStyle: 'static {
    fn make_body(&self, cfg: &SegmentedControlStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedSegmentedControlStyle = Rc<dyn SegmentedControlStyle>;
