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

    /// Reject a heavyweight child for a point an `Over`-band press claimant
    /// covers, so the arena's verdict matches the one this view's own dispatch
    /// gives.
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
    /// The rule is [`claims_press`](crate::claims_press), not "is painted": a
    /// decorative halo drawn over an embedded note never vetoes, so clicking it
    /// leaves focus in the note and leaves explore-by-touch announcing the
    /// note — which is the behaviour the crate's per-item AT tree exists to
    /// protect.
    fn accepts_child_hit(&self, _child: WidgetId, point: teksilo_canvas::Point) -> bool {
        !self.over_press_claimant_covers(point)
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    fn wants_descendant_redirects(&self) -> bool {
        true
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
