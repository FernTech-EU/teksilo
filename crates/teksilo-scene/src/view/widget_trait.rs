// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `Widget` trait implementation for [`SceneView`].
//!
//! Each method is a thin delegation to its `*_impl` counterpart in the
//! appropriate impl-split module (`build_impl`, `layout_impl`, `paint_impl`,
//! `a11y_impl`). Notable flags: `clips_children` is `true` (the scene clips
//! at the viewport boundary), `preserves_children_on_rebuild` is `true`
//! (heavyweight widgets survive scene rebuild and are only destroyed when their
//! item is removed), and `wants_descendant_redirects` is `true` (lightweight
//! item AT nodes are synthetic — the a11y walker reparents them into the
//! scene's virtual accessibility tree rather than as normal widget children).

use super::*;

impl Widget for SceneView {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.build_impl(ctx)
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.layout_response_impl(proposal, ctx)
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.place_children_impl(bounds, proposal, children, ctx)
    }

    fn paint(&self, bounds: Rect, canvas: &mut teksilo_canvas::Canvas, ctx: &PaintContext) {
        self.paint_impl(bounds, canvas, ctx)
    }

    fn wants_post_paint(&self) -> bool {
        self.wants_post_paint_impl()
    }

    fn post_paint(&self, bounds: Rect, canvas: &mut teksilo_canvas::Canvas, ctx: &PaintContext) {
        self.post_paint_impl(bounds, canvas, ctx)
    }

    fn clips_children(&self) -> bool {
        true
    }

    /// Reject a heavyweight child for a point a press claimant **painted above
    /// that child** covers, so the arena's verdict matches the one this view's
    /// own dispatch gives.
    ///
    /// `point` is in scene coordinates: the view is a `content_transform` node,
    /// so the arena has already mapped the pointer through the inverse view
    /// transform before offering it to children.
    ///
    /// Without this, the two pickers answer independently and only the *tap*
    /// can be papered over: press feedback, focus-on-release, the touch hold
    /// route (hence touch tooltips and context menus), the cursor and the
    /// view's own drag recognizer all resolve from the arena's target, so five
    /// of the six would stay on the card while the sixth went to the item.
    ///
    /// **Above that child**, not "in the `Over` band": the question is asked
    /// per child and answered by comparing whole
    /// [`PaintKey`]s — so an
    /// [`Interleaved`](crate::SceneLayer::Interleaved) claimant vetoes the
    /// cards it is painted over and leaves the ones painted over *it* alone.
    /// The `Over` band is above every card, so for it this is the rule it
    /// always was.
    ///
    /// A child with no key — anything this view did not place, and the
    /// paint-only nodes, which never reach here because they are
    /// `event_pass_through` — is treated as sitting at the bottom of the widget
    /// rank, which is the permissive pre-existing answer.
    ///
    /// The rule is [`claims_press`](crate::claims_press), not "is painted": a
    /// decorative halo drawn over an embedded note never vetoes, so clicking it
    /// leaves focus in the note and leaves explore-by-touch announcing the
    /// note — which is the behaviour the crate's per-item AT tree exists to
    /// protect.
    ///
    /// # The order of the two questions is the cost of the feature
    ///
    /// The claimant is resolved **first** and the child's key looked up only
    /// against an actual answer. Both halves of that matter, and getting either
    /// backwards has already cost real time here:
    ///
    /// * the claimant query is memoised for the whole hit walk, so the arena's
    ///   once-per-child call costs one snapshot scan for the walk rather than
    ///   one per child — which is only true because the memoised question is
    ///   floor-free (see `press_claimant_above` in `view.rs`);
    /// * `SceneView::child_paint_key` is a `RefCell` borrow
    ///   and a `HashMap` probe. Passed as an *argument* it is evaluated before
    ///   the claimant check can decline it, so a scene with no press claimant at
    ///   all — the case the `over_claimants` short-circuit exists to keep free —
    ///   still paid for one on every child it was asked about. Asked in this
    ///   order it pays for none.
    ///
    /// The arena's walk stops at the first child it can descend into, so the
    /// per-child cost only multiplies when the veto is *rejecting* — which is
    /// exactly the scene the feature is for, and why the claimant query has to
    /// answer from a memo rather than a scan.
    fn accepts_child_hit(&self, child: WidgetId, point: teksilo_canvas::Point) -> bool {
        match self.topmost_press_claimant(point) {
            None => true,
            // Looked up only against an actual claimant — see the second
            // bullet above. Binding it to a local first would evaluate it for
            // every child of every scene, including the ones with no claimant
            // at all.
            Some(top) => top < self.child_paint_key(child),
        }
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    /// A scene holds far more than it shows, and the half it does not show
    /// should not be *reachable*: a card 90 000 px away is a Tab stop between
    /// two visible ones and a node assistive tech is offered. Collapsing its
    /// size to zero answers neither — a zero-size widget is still alive.
    ///
    /// So `place_children` is handed every card, parked ones included, and
    /// reads back `WidgetPlacement::dormant`. See `place_children_impl` for
    /// the two regions that decide it, and `SceneView::retention_margin`.
    fn culls_children(&self) -> bool {
        true
    }

    fn wants_descendant_redirects(&self) -> bool {
        true
    }

    /// The cards this view offers assistive technology — a subset of its arena
    /// children, in the same order.
    ///
    /// Being alive and being enumerated are separate questions here (see
    /// `place_children_impl`), and this is the hook that keeps the second one
    /// answerable at all. The framework walker uses this list for BOTH the
    /// child push and the recursion, so a card left out of it is neither named
    /// by a parent nor emitted as a node: no orphan, no dangling child,
    /// nothing downstream to clean up. Suppressing through the redirect hook
    /// instead would leave the node emitted and unreferenced, which the
    /// consumer rejects.
    ///
    /// `None` — the framework's "use the arena's children" — until the first
    /// `place_children`, because before that nothing has been decided.
    ///
    /// A card in this state is still a Tab stop. That is the honest reading of
    /// the two knobs it sits between: it is alive because
    /// [`retention_margin`](SceneView::retention_margin) said to keep it warm,
    /// and unlisted because
    /// [`a11y_off_screen_mode`](SceneView::a11y_off_screen_mode) said not to
    /// offer it. Landing focus on it pins it, and the pass that lands the focus
    /// publishes it.
    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        self.at_children.borrow().clone()
    }

    fn a11y_redirect_descendant(
        &self,
        self_id: WidgetId,
        descendant: WidgetId,
    ) -> Option<accesskit::NodeId> {
        self.a11y_redirect_descendant_impl(self_id, descendant)
    }

    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        self.accessibility_impl(builder)
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
