// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`TouchTarget`] — the last resort of the three hit-targeting mechanisms:
//! the one that actually moves things.

use teksilo_canvas::{EdgeInsets, Point, Rect, Size, SizeProposal};
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{InputTokens, PointerKind, TargetDensity};

/// Give an undersized control a conforming touch target, growing the layout
/// around it when nothing cheaper will do — and **only at
/// [`TargetDensity::Touch`]**.
///
/// # When you need this, and when you do not
///
/// Teksilo has three ways to make a target reachable, and they are ordered by
/// how much they disturb:
///
/// 1. **`Widget::hit_outset`** — a thin grip claims the space around it.
///    Hit-only; nothing moves. This is what a splitter gutter or a column
///    resize strip uses.
/// 2. **The miss-only slop pass** — an isolated small control catches a near
///    miss. Hit-only; nothing moves. This is what a radio dot or a chart mark
///    uses.
/// 3. **`TouchTarget`** — this. The control genuinely needs *room*, because the
///    thing beside it is also a target and there is no space to borrow. It
///    changes layout, so siblings reflow.
///
/// Reach for (3) only when (1) and (2) cannot work: when a control sits in a
/// tight row of other controls, so widening its hit area would take presses
/// from its neighbours rather than from empty space. Everything else the
/// density sweep already handles by projecting the recipes.
///
/// # Why Touch only
///
/// At `Compact` and `Comfortable` this wrapper is the **identity**: it reports
/// its child's own response, unchanged, and adds no hit outset. Compact is the
/// density every existing layout was designed at and every layout golden was
/// recorded at, and `Comfortable` is served by the recipes' own density
/// projection, which raises a control's *own* dimensions rather than padding
/// around it. `Touch` is the ladder where a 24 dp control still falls 20 dp
/// short of the target and no recipe can close the gap from inside.
///
/// ```ignore
/// // A 16 dp close affordance in a dense tab strip: at Touch it is given a
/// // 44 dp slot and centred in it; at Compact nothing changes at all.
/// TouchTarget::new().child(close_button)
/// ```
///
/// # `reserve_space`
///
/// * `reserve_space(true)` (**the default**) — the slot reports at least
///   `size` on both axes and centres the child in it. Siblings reflow.
/// * `reserve_space(false)` — the slot reports the child's own size and
///   declares a [`Widget::hit_outset`] that brings the *hit* area up to `size`
///   instead. Nothing moves; the target overlaps whatever is beside it. Use it
///   when the row has slack in one direction but you cannot spend it.
pub struct TouchTarget {
    size: Option<f32>,
    reserve_space: bool,
    child: Option<WidgetId>,
    pending: Option<Box<dyn Widget>>,
}

impl TouchTarget {
    /// A new wrapper at the density's own `target_size`. Attach content with
    /// [`child`](Self::child) or [`child`](Self::child).
    pub fn new() -> Self {
        Self {
            size: None,
            reserve_space: true,
            child: None,
            pending: None,
        }
    }

    /// Override the target size, in dp. Defaults to the density's
    /// `InputTokens::target_size` (44 dp at Touch).
    pub fn size(mut self, dp: f32) -> Self {
        self.size = Some(dp);
        self
    }

    /// Whether the slot takes the room it needs (`true`, the default) or widens
    /// only the hit area (`false`). See the type docs.
    pub fn reserve_space(mut self, reserve: bool) -> Self {
        self.reserve_space = reserve;
        self
    }

    /// Wrap an inline widget.
    pub fn child(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self {
        match teksilo_core::IntoTeksiChild::into_pending(widget) {
            teksilo_core::PendingChild::Id(id) => {
                self.child = Some(id);
                self
            }
            teksilo_core::PendingChild::Deferred(w) => {
                self.pending = Some(w);
                self
            }
        }
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

    /// The target this wrapper aims for under `tokens`, or `None` when it is
    /// inert (any density but `Touch`).
    fn target(&self, tokens: &InputTokens) -> Option<f32> {
        if tokens.density != TargetDensity::Touch {
            return None;
        }
        let size = self.size.unwrap_or(tokens.target_size);
        (size.is_finite() && size > 0.0).then_some(size)
    }
}

impl Default for TouchTarget {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TouchTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TouchTarget")
            .field("size", &self.size)
            .field("reserve_space", &self.reserve_space)
            .finish()
    }
}

impl Widget for TouchTarget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending.take() {
            self.child = Some(ctx.add_boxed(pending));
        }
        self.child.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let child = self
            .child
            .and_then(|id| ctx.child_layout_response(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into());
        // Inert at every density but Touch, and inert whenever the caller asked
        // for hit-only widening: forward the child's FULL response — grow
        // weight, shrink weight and compression floor — so wrapping a
        // shrinkable child does not silently make it rigid.
        let Some(target) = self.target(&ctx.theme.input) else {
            return child;
        };
        if !self.reserve_space {
            return child;
        }
        let size = Size::new(child.size.width.max(target), child.size.height.max(target));
        LayoutResponse {
            size,
            flex: child.flex,
            min: Size::new(child.min.width.max(target), child.min.height.max(target)),
            shrink: child.shrink,
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            // The child keeps its own size and is centred in whatever slot the
            // wrapper was given; it is the SLOT that grew, never the control.
            let natural = self
                .child
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or(bounds.size());
            let size = Size::new(
                natural.width.min(bounds.width),
                natural.height.min(bounds.height),
            );
            child.origin = Point::new(
                bounds.x + (bounds.width - size.width) / 2.0,
                bounds.y + (bounds.height - size.height) / 2.0,
            );
            child.size = size;
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn hit_outset(&self, kind: PointerKind, tokens: &InputTokens) -> EdgeInsets {
        // The `reserve_space(false)` half of the contract: no layout moves, so
        // the shortfall is made up between the pointer and the arena instead.
        if self.reserve_space || !kind.is_direct() {
            return EdgeInsets::ZERO;
        }
        match self.target(tokens) {
            Some(target) => EdgeInsets::uniform(target / 2.0),
            None => EdgeInsets::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{FixedSize, HStack, Shrinkable};
    use teksilo_core::widget_builder::WidgetBuilder;
    use teksilo_core::widget_tree::WidgetTree;

    fn tree_at(density: TargetDensity) -> WidgetTree {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_input_density(density);
        tree
    }

    /// Compact renders identically: the wrapper reports the child's size and
    /// places it exactly where an unwrapped child would sit.
    #[test]
    fn compact_and_comfortable_are_the_identity() {
        for density in [TargetDensity::Compact, TargetDensity::Comfortable] {
            let mut tree = tree_at(density);
            let inner = tree.add(FixedSize::new().width(16.0).height(16.0));
            let slot = tree.add(TouchTarget::new().child(inner));
            tree.layout(SizeProposal::unspecified());
            assert_eq!(
                tree.bounds(slot).size(),
                Size::new(16.0, 16.0),
                "{density:?} must not grow the slot"
            );
            assert_eq!(tree.bounds(inner).size(), Size::new(16.0, 16.0));
        }
    }

    /// At Touch the slot reaches the target and the child is centred in it —
    /// the control itself never grows.
    #[test]
    fn touch_gives_the_slot_the_target_and_centres_the_child() {
        let mut tree = tree_at(TargetDensity::Touch);
        let inner = tree.add(FixedSize::new().width(16.0).height(16.0));
        let slot = tree.add(TouchTarget::new().child(inner));
        tree.layout(SizeProposal::unspecified());
        assert_eq!(tree.bounds(slot).size(), Size::new(44.0, 44.0));
        assert_eq!(tree.bounds(inner).size(), Size::new(16.0, 16.0));
        assert_eq!(tree.bounds(inner).center(), tree.bounds(slot).center());
    }

    /// An explicit size overrides the density's own.
    #[test]
    fn an_explicit_size_wins_over_the_density() {
        let mut tree = tree_at(TargetDensity::Touch);
        let inner = tree.add(FixedSize::new().width(16.0).height(16.0));
        let slot = tree.add(TouchTarget::new().size(48.0).child(inner));
        tree.layout(SizeProposal::unspecified());
        assert_eq!(tree.bounds(slot).size(), Size::new(48.0, 48.0));
    }

    /// A control already at or beyond the target is left alone.
    #[test]
    fn a_conforming_child_is_untouched() {
        let mut tree = tree_at(TargetDensity::Touch);
        let inner = tree.add(FixedSize::new().width(60.0).height(50.0));
        let slot = tree.add(TouchTarget::new().child(inner));
        tree.layout(SizeProposal::unspecified());
        assert_eq!(tree.bounds(slot).size(), Size::new(60.0, 50.0));
    }

    /// `reserve_space(false)` moves nothing and widens the hit area instead.
    #[test]
    fn reserve_space_false_widens_the_hit_area_without_moving_anything() {
        let mut tree = tree_at(TargetDensity::Touch);
        let inner = tree.add(FixedSize::new().width(16.0).height(16.0));
        let slot = tree.add(
            TouchTarget::new()
                .reserve_space(false)
                .child(inner)
                .on_tap(|_e, _c| {}),
        );
        tree.layout(SizeProposal::unspecified());
        assert_eq!(
            tree.bounds(slot).size(),
            Size::new(16.0, 16.0),
            "nothing may move"
        );
        let finger = teksilo_core::pointer::PointerInfo::touch(
            teksilo_core::pointer::PointerId::MOUSE,
            teksilo_core::pointer::EventTime::ZERO,
        );
        // 22 dp of outset on every edge: a press 10 dp past the child's edge
        // still reaches it, and a mouse press does not.
        assert_eq!(
            tree.hit_test_for(Point::new(26.0, 8.0), &finger),
            Some(slot)
        );
        assert_ne!(tree.hit_test(Point::new(26.0, 8.0)), Some(slot));
    }

    /// Layout-transparency for the FULL response: wrapping a shrinkable child
    /// must not make it rigid — the `DeadZone` lesson.
    #[test]
    fn the_wrapper_forwards_its_child_shrink_weight() {
        let mut tree = tree_at(TargetDensity::Compact);
        let slot = tree.add(
            TouchTarget::new().child(
                Shrinkable::new()
                    .min_width(20.0)
                    .child(FixedSize::new().width(100.0).height(20.0)),
            ),
        );
        let rigid = tree.add(FixedSize::new().width(100.0).height(20.0));
        tree.add(HStack::new().child(rigid).child(slot));
        tree.layout(SizeProposal::exact(120.0, 20.0));
        let w = tree.bounds(slot).width;
        assert!(
            w < 100.0,
            "the TouchTarget must forward the child's shrink weight (width was {w})"
        );
    }
}
