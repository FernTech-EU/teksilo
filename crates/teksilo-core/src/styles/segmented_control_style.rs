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
//!
//! The chrome cannot derive segment rectangles by dividing its bounds:
//! once a control can overflow, the number of visible segments and their
//! widths are decided per layout pass by the widget's overflow plan. The
//! widget therefore publishes the resolved geometry through
//! [`SegmentSlots`], and the chrome paints from that.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::Rect;

use crate::build_context::BuildContext;
use crate::focus::FocusOrigin;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Resolved, control-local geometry for one layout pass.
///
/// A *slot* is a position on the strip. A *segment* is an entry in the
/// control's live segment list. The two coincide until the control
/// overflows, after which [`order`](Self::order) maps between them —
/// the last slot may hold a promoted segment from anywhere in the list.
#[derive(Debug, Clone, Default)]
pub struct SegmentSlotGeometry {
    /// The stroked outer frame, inside the focus-ring envelope.
    pub frame: Rect,
    /// One rectangle per visible slot, in visual (reading) order.
    pub segments: Vec<Rect>,
    /// `order[slot]` is the live segment index drawn in that slot.
    /// Same length as [`segments`](Self::segments).
    pub order: Vec<usize>,
    /// The overflow trigger's rectangle, when the control is
    /// overflowing. **Paint-only** — the trigger is a real widget whose
    /// bounds come from the layout pass; never hit-test against this.
    pub overflow: Option<Rect>,
}

/// Shared handle to the geometry the widget publishes each layout pass
/// and the chrome reads at paint time.
///
/// The widget's `place_children` always runs before its children paint,
/// so a chrome reading this during `paint` always sees the current pass's
/// values. Before the first layout it is empty, and the chrome must treat
/// that as "nothing to draw" rather than as an error.
#[derive(Debug, Clone, Default)]
pub struct SegmentSlots(Rc<RefCell<SegmentSlotGeometry>>);

impl SegmentSlots {
    /// An empty handle, as produced before the first layout pass.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the published geometry. Called from the widget's
    /// `place_children`.
    pub fn publish(&self, geometry: SegmentSlotGeometry) {
        *self.0.borrow_mut() = geometry;
    }

    /// Read the published geometry.
    pub fn with<R>(&self, f: impl FnOnce(&SegmentSlotGeometry) -> R) -> R {
        f(&self.0.borrow())
    }

    /// The slot a live segment currently occupies, if it is on the strip.
    /// Returns `None` for a segment that overflowed into the menu.
    pub fn slot_of(&self, segment: usize) -> Option<usize> {
        self.0.borrow().order.iter().position(|&s| s == segment)
    }

    /// The rectangle a live segment currently occupies, if any.
    pub fn rect_of(&self, segment: usize) -> Option<Rect> {
        let inner = self.0.borrow();
        let slot = inner.order.iter().position(|&s| s == segment)?;
        inner.segments.get(slot).copied()
    }

    /// Number of visible slots.
    pub fn len(&self) -> usize {
        self.0.borrow().segments.len()
    }

    /// Whether the strip currently shows no segments at all.
    pub fn is_empty(&self) -> bool {
        self.0.borrow().segments.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct SegmentedControlStyleConfig {
    /// Resolved slot geometry for the current layout pass, published by
    /// the widget. Replaces the old `segment_count` + divide-by-`n`
    /// derivation, which cannot express overflow or non-uniform widths.
    pub slots: SegmentSlots,
    /// Current selection, as an index into the **live** segment list.
    /// Resolve it to a slot with [`SegmentSlots::slot_of`].
    pub selected: Signal<usize>,
    /// `Some(live segment index)` while the pointer is over a segment.
    /// The recipe reads this to paint the hover tint behind the
    /// non-selected segment under the pointer. A segment that overflowed
    /// while hovered simply resolves to no slot, so a stale value paints
    /// nothing.
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
