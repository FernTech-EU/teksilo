//! Tier-3 style protocol for `SegmentedControl`. See
//! `docs/styling-system.md`.
//!
//! Themes the segmented-control chrome: the outer rounded frame, the
//! per-segment hover tint, the selected-segment surface + border, the
//! divider geometry between segments, the focus ring, and the
//! per-segment label rendering. The `SegmentedControl` widget keeps
//! its `Role::RadioGroup` + per-segment `Role::RadioButton` semantics
//! and dispatches taps/keys; it owns no `paint()` of its own.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::focus::FocusOrigin;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct SegmentedControlStyleConfig {
    /// Resolved per-segment labels — drawn by the recipe widget.
    pub labels: Vec<String>,
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
    /// Disabled state (static).
    pub is_enabled: bool,
}

pub trait SegmentedControlStyle: 'static {
    fn make_body(
        &self,
        cfg: &SegmentedControlStyleConfig,
        ctx: &mut BuildContext,
    ) -> WidgetId;
}

pub type SharedSegmentedControlStyle = Rc<dyn SegmentedControlStyle>;
