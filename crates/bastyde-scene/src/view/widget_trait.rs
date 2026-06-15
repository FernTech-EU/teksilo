// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

    fn paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, ctx: &PaintContext) {
        self.paint_impl(bounds, canvas, ctx)
    }

    fn wants_post_paint(&self) -> bool {
        self.wants_post_paint_impl()
    }

    fn post_paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, ctx: &PaintContext) {
        self.post_paint_impl(bounds, canvas, ctx)
    }

    fn clips_children(&self) -> bool {
        true
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

    fn accessibility(&self, builder: &mut bastyde_core::accessibility::AccessNodeBuilder) {
        self.accessibility_impl(builder)
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
