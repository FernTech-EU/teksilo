// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MinSize — a layout modifier that ensures a child reaches a minimum width and/or height.
//!
//! The child's reported size is clamped upward so it never falls below the
//! configured minimum on each constrained axis. The minimum is also forwarded
//! as part of the clamped proposal so that wrap-aware children (e.g. a
//! multi-line `TextWidget`) measure against the constraint they will actually
//! be placed into. Axes with no minimum set are passed through unchanged.
//!
//! `MinSize` propagates the child's `flex` and `shrink` weights so that a
//! `Spacer` or `Expand` inside `MinSize` still participates in stack
//! slack-distribution; the child's own compression floor is composed with
//! the `MinSize` floor.
//!
//! For the inverse operation (capping a maximum size) see [`MaxSize`](super::MaxSize).
//!
//! # Hit targets: take the floor from the theme
//!
//! `MinSize` is the mechanism for a minimum hit box, but the *number* belongs to
//! the active density rather than to the call site. Route it through
//! [`density_min_size`](teksilo_core::styles::density::density_min_size) (or
//! `dp(.., TargetRole::Target, ..)`), which raises the named axes to
//! `theme.input.target_size` — 24 dp at `Compact`, 32 at `Comfortable`, 44 at
//! `Touch`, and 48 under the Material 3 preset. That is what every shipped
//! recipe does; see `docs/density-and-targets.md`.
//!
//! ```rust
//! # use teksilo_widgets::primitives::{MinSize, icon_widget::IconWidget};
//! # use teksilo_core::styles::density::density_min_size;
//! # use teksilo_tokens::{InputTokens, TargetAxes, TargetDensity};
//! # let tokens = InputTokens::for_density(TargetDensity::Touch);
//! # let base = teksilo_canvas::Size::new(20.0, 20.0);
//! // The density decides the floor; the call site only says "both axes".
//! let min = density_min_size(base, TargetAxes::BOTH, &tokens);
//! let _tap_target = MinSize::new(min.width, min.height)
//!     .child(IconWidget::checkmark(20.0));
//! ```
//!
//! A hard-coded literal is right only where the number is a *design* dimension
//! that must not move with density. Where it is a target, name its bar: 24 dp is
//! WCAG 2.2 SC 2.5.8 *Target Size (Minimum)*, level AA — the floor
//! `min_target_conformance` holds at every density; 44 dp is Apple's HIG minimum
//! and SC 2.5.5 *Target Size (Enhanced)*, level AAA, which is what the `Touch`
//! ladder aims for. 44 dp is never the AA figure.

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::signal::Prop;
use teksilo_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

/// Layout modifier that enforces a minimum width and/or height on a single child widget.
///
/// Constraints can be static or bound to a reactive `Signal<f32>` for dynamic resizing.
#[derive(Debug)]
pub struct MinSize {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    min_width: Option<Prop<f32>>,
    min_height: Option<Prop<f32>>,
}

impl MinSize {
    /// Enforce a minimum on both axes: the child's width will be at least `width` and its height at least `height`.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: Some(Prop::Static(width)),
            min_height: Some(Prop::Static(height)),
        }
    }

    /// Enforce a minimum only on the width axis; the height axis is unconstrained by this modifier.
    pub fn width(width: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: Some(Prop::Static(width)),
            min_height: None,
        }
    }

    /// Enforce a minimum only on the height axis; the width axis is unconstrained by this modifier.
    pub fn height(height: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: None,
            min_height: Some(Prop::Static(height)),
        }
    }

    /// Bind min width to a reactive state.
    pub fn min_width(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.min_width = Some(state.into());
        self
    }

    /// Bind min height to a reactive state.
    pub fn min_height(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.min_height = Some(state.into());
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self {
        self.pending_child = Some(teksilo_core::IntoTeksiChild::into_pending(widget));
        self
    }
    /// Attach `widget` when it is `Some`, and do nothing when it is `None`.
    ///
    /// The conditional-child form. `teksu!`'s `if` without an `else` lowers to
    /// this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
    /// adds no arena node, so nothing is laid out, painted, or published to the
    /// accessibility tree, and a stack applies no spacing around it.
    pub fn child_opt(self, widget: Option<impl teksilo_core::IntoTeksiChild>) -> Self {
        match widget {
            Some(w) => self.child(w),
            None => self,
        }
    }
}

impl Widget for MinSize {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(ref w) = self.min_width {
            w.register_if_bound(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::Relayout,
            );
        }
        if let Some(ref h) = self.min_height {
            h.register_if_bound(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::Relayout,
            );
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let min_w = self.min_width.as_ref().map(|r| r.get());
        let min_h = self.min_height.as_ref().map(|r| r.get());

        // Clamp the proposal upward to the minimums before forwarding,
        // so wrap-aware children (TextWidget, etc.) measure against the
        // actual constraint they will be placed into. Mirrors MaxSize's
        // approach of clamping the proposal before forwarding.
        let clamped_proposal = SizeProposal {
            width: match (proposal.width, min_w) {
                (Some(w), Some(min)) => Some(w.max(min)),
                (None, Some(min)) => Some(min),
                (w, None) => w,
            },
            height: match (proposal.height, min_h) {
                (Some(h), Some(min)) => Some(h.max(min)),
                (None, Some(min)) => Some(min),
                (h, None) => h,
            },
        };

        // Use child_layout_response to capture flex so a Spacer (or any
        // other flex child) inside MinSize is still seen as flex by the
        // parent stack. Without this, MinSize(Spacer) would report flex=0
        // and break the parent HStack's slack distribution.
        let child_response = self
            .child_id
            .and_then(|id| ctx.child_layout_response(id, clamped_proposal));
        let child_size = child_response
            .as_ref()
            .map(|r| r.size)
            .unwrap_or(Size::ZERO);
        let child_flex = child_response.map(|r| r.flex).unwrap_or(0.0);
        let child_shrink = child_response.map(|r| r.shrink).unwrap_or(0.0);
        let child_min = child_response.map(|r| r.min).unwrap_or(Size::ZERO);

        let w = match min_w {
            Some(min) => child_size.width.max(min),
            None => child_size.width,
        };
        let h = match min_h {
            Some(min) => child_size.height.max(min),
            None => child_size.height,
        };

        // Compose the compression floor: a shrinkable child may still shrink,
        // but never below MinSize's own minimum nor the child's own floor.
        // Clamp componentwise to the wanted size so `min <= size` holds.
        let floor_w = child_min.width.max(min_w.unwrap_or(0.0)).min(w);
        let floor_h = child_min.height.max(min_h.unwrap_or(0.0)).min(h);
        teksilo_core::widget::LayoutResponse::flexible(Size::new(w, h), child_flex)
            .with_shrink(child_shrink)
            .with_min(Size::new(floor_w, floor_h))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {}

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::signal::Signal;
    use teksilo_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn clamps_small_child_to_minimum() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(MinSize::new(48.0, 48.0).child(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(min);
        assert!((mb.width - 48.0).abs() < 0.01);
        assert!((mb.height - 48.0).abs() < 0.01);
    }

    #[test]
    fn large_child_is_not_clamped() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(100.0, 80.0));
        let min = tree.add(MinSize::new(48.0, 48.0).child(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(min);
        assert!((mb.width - 100.0).abs() < 0.01);
        assert!((mb.height - 80.0).abs() < 0.01);
    }

    #[test]
    fn min_width_only() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(MinSize::width(48.0).child(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(min);
        assert!((mb.width - 48.0).abs() < 0.01);
        assert!((mb.height - 10.0).abs() < 0.01);
    }

    #[test]
    fn min_width_dynamic() {
        let min_w = Signal::new(48.0_f32);
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(MinSize::width(0.0).min_width(min_w.clone()).child(child));
        tree.layout(SizeProposal::unspecified());
        assert!((tree.bounds(min).width - 48.0).abs() < 0.01);

        min_w.set(80.0);
        tree.layout(SizeProposal::unspecified());
        assert!((tree.bounds(min).width - 80.0).abs() < 0.01);
    }

    /// A leaf that simulates wrapping text: it has 120 logical px of
    /// content. When the proposal width is >= 120 it reports 120×20
    /// (single line). When narrower, it wraps: width = proposal,
    /// height = ceil(120 / proposal) * 20.
    #[derive(Debug)]
    struct WrappingLeaf;
    impl Widget for WrappingLeaf {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            let content_width = 120.0_f32;
            let line_height = 20.0_f32;
            let w = proposal.width.unwrap_or(content_width).min(content_width);
            let lines = (content_width / w.max(1.0)).ceil();
            Size::new(w, lines * line_height).into()
        }
    }

    #[test]
    fn child_receives_clamped_proposal() {
        // A VStack with unspecified width queries the MinSize for its
        // intrinsic size. MinSize (min_width=100) should forward 100px
        // to the wrapping child (not leave width unspecified or too
        // narrow), yielding the correct wrapped height.
        use crate::primitives::vstack::VStack;

        let mut tree = WidgetTree::new();
        let child = tree.add(WrappingLeaf);
        let min = tree.add(MinSize::width(100.0).child(child));
        let _stack = tree.add(VStack::new().child(min));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        let mb = tree.bounds(min);
        assert!(
            (mb.width - 100.0).abs() < 0.01,
            "width should be 100, got {}",
            mb.width
        );
        // At 100px width: ceil(120/100) = 2 lines → 40px height
        assert!(
            (mb.height - 40.0).abs() < 0.01,
            "height should be 40 (2 lines at 100px), got {}",
            mb.height
        );
    }

    #[test]
    fn unspecified_proposal_gets_clamped_to_minimum() {
        // When the parent proposes no width at all, MinSize should
        // forward the minimum as the proposal so the child measures
        // against the constraint it will actually be placed into.
        let mut tree = WidgetTree::new();
        let child = tree.add(WrappingLeaf);
        let min = tree.add(MinSize::width(80.0).child(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(min);
        assert!(
            (mb.width - 80.0).abs() < 0.01,
            "width should be 80, got {}",
            mb.width
        );
        // At 80px width: ceil(120/80) = 2 lines → 40px
        assert!(
            (mb.height - 40.0).abs() < 0.01,
            "height should be 40 (2 lines at 80px), got {}",
            mb.height
        );
    }
}
