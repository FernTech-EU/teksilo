// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shrinkable — a layout modifier that allows its child to compress under an over-constraint.
//!
//! By default every widget is rigid: when a stack runs out of main-axis room, rigid
//! children keep their wanted size and overflow the bounds. `Shrinkable` opts a child
//! into the over-constraint distribution: the stack divides any deficit across all
//! shrinkable children proportional to their [`shrink`](Shrinkable::shrink) weight,
//! never below the [`min_width`](Shrinkable::min_width) / [`min_height`](Shrinkable::min_height)
//! floor set here.
//!
//! `Shrinkable` is the shrink counterpart to
//! [`Expand`](crate::primitives::Expand): while `Expand` claims leftover slack
//! (grow), `Shrinkable` absorbs excess pressure (shrink). The two are independent
//! — a child can both grow on surplus and shrink on deficit by wrapping with
//! `Shrinkable` and setting a non-zero `flex` on the inner widget.
//!
//! ## When to use
//!
//! - A long text label that should ellipsize before a rigid icon/badge loses space.
//! - A thumbnail image column that may compress while a fixed sidebar stays at full width.
//! - "Compress A before B": give A `Shrinkable`, leave B rigid (`shrink = 0`).
//!
//! ```rust
//! # use bastyde_widgets::primitives::{HStack, Shrinkable, TextWidget};
//! # use bastyde_i18n::lit;
//! // The label shrinks as far as 48 dp; the button stays rigid.
//! let _row = HStack::new()
//!     .child(Shrinkable::new().min_width(48.0)
//!         .child(TextWidget::new(lit!("A long label that may compress")).single_line()))
//!     .child(TextWidget::new(lit!("Rigid")));
//! ```

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// Layout modifier that lets its child be **compressed** when a stack is
/// over-constrained — the shrink counterpart to [`Expand`](crate::primitives::Expand).
///
/// By default widgets do not shrink: when an `HStack`/`VStack` runs out of
/// room, rigid children keep their wanted size and overflow. Wrap a child in
/// `Shrinkable` to opt it into compression: the parent distributes any deficit
/// across shrinkable children proportional to their shrink weight, never below
/// the floor set here.
///
/// ```rust
/// # use bastyde_widgets::primitives::{HStack, Shrinkable, TextWidget, IconWidget};
/// # use bastyde_i18n::lit;
/// # let long_label = TextWidget::new(lit!("A very long label that may need to shrink"));
/// # let icon = IconWidget::chevron_right(16.0);
/// // The label gives up space before the (rigid) icon when the row is narrow:
/// let _w = HStack::new()
///     .child(Shrinkable::new().min_width(40.0).child(long_label))
///     .child(icon); // rigid — never shrinks
/// ```
///
/// `Shrinkable` preserves its child's grow weight (`flex`) and cross size, so a
/// child can both grow on surplus and shrink on a deficit. It forwards the
/// parent's proposal to the child unchanged; when the stack compresses it, the
/// child is re-laid-out at the smaller size (so e.g. a wrapped-text child
/// re-wraps and reports its taller height via the height-for-width pass).
///
/// **Floor caveat.** The default floor is `0` on both axes, which lets the
/// child shrink to nothing. Set [`min_width`](Self::min_width) /
/// [`min_height`](Self::min_height) to a sensible minimum — the caller owns
/// this choice (unlike the stock height-stable widgets, which report their own
/// natural floor).
#[derive(Debug)]
pub struct Shrinkable {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    shrink: f32,
    min_width: f32,
    min_height: f32,
}

impl Shrinkable {
    /// A shrinkable wrapper with shrink weight `1.0` and a zero floor.
    pub fn new() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            shrink: 1.0,
            min_width: 0.0,
            min_height: 0.0,
        }
    }

    /// Set the shrink weight (relative share of an over-constraint deficit this
    /// child absorbs). Clamped to `>= 0`; `0` makes the child rigid again.
    pub fn shrink(mut self, weight: f32) -> Self {
        self.shrink = weight.max(0.0);
        self
    }

    /// Set the minimum width the child may be compressed to.
    pub fn min_width(mut self, min: f32) -> Self {
        self.min_width = min.max(0.0);
        self
    }

    /// Set the minimum height the child may be compressed to.
    pub fn min_height(mut self, min: f32) -> Self {
        self.min_height = min.max(0.0);
        self
    }

    /// Wrap an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Wrap a pre-registered child by id.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }
}

impl Default for Shrinkable {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Shrinkable {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            let id = match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            self.child_id = Some(id);
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let Some(child) = self.child_id else {
            return proposal.resolve(0.0, 0.0).into();
        };
        let r = ctx
            .child_layout_response(child, proposal)
            .unwrap_or(LayoutResponse::ZERO);
        let min = Size::new(
            self.min_width.min(r.size.width),
            self.min_height.min(r.size.height),
        );
        // Preserve the child's grow weight; add this wrapper's shrink + floor.
        LayoutResponse::flexible(r.size, r.flex)
            .with_shrink(self.shrink)
            .with_min(min)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Single child fills our (possibly compressed) bounds; the driver then
        // re-lays it out at this exact size.
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::hstack::HStack;
    use bastyde_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(&self, _p: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn shrinkable_wrapper_compresses_its_child_down_to_floor() {
        let mut tree = WidgetTree::new();
        // 200-wide child wrapped to allow shrink to a 50 floor, plus a rigid
        // 60 sibling. Bounds 100 → deficit 160; the wrapped child absorbs it
        // down to its 50 floor (residual overflow), the sibling stays 60.
        let big = tree.add(FixedLeaf(200.0, 20.0));
        let wrapped = tree.add(Shrinkable::new().min_width(50.0).child_id(big));
        let rigid = tree.add(FixedLeaf(60.0, 20.0));
        let _stack = tree.add(HStack::new().add_child(wrapped).add_child(rigid));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        assert!(
            (tree.bounds(wrapped).width - 50.0).abs() < 0.01,
            "wrapped width = {}",
            tree.bounds(wrapped).width
        );
        assert!(
            (tree.bounds(rigid).width - 60.0).abs() < 0.01,
            "rigid width = {}",
            tree.bounds(rigid).width
        );
        // The child fills the compressed wrapper bounds.
        assert!((tree.bounds(big).width - 50.0).abs() < 0.01);
    }

    #[test]
    fn shrinkable_does_not_shrink_when_there_is_room() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(80.0, 20.0));
        let wrapped = tree.add(Shrinkable::new().min_width(20.0).child_id(child));
        let _stack = tree.add(HStack::new().add_child(wrapped));
        tree.layout(SizeProposal::exact(300.0, 40.0));
        // Plenty of room → keeps its natural width (no growth, no shrink).
        assert!((tree.bounds(wrapped).width - 80.0).abs() < 0.01);
    }
}
