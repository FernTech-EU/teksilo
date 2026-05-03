//! Visual highlight + selected-bounds tracker for the inspector.

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_tokens::Color;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_id::WidgetId;

use crate::state::InspectorState;

/// Decorative leaf widget that paints a colored stroke around the
/// currently selected widget's bounds. Reads `selected_bounds` from
/// `InspectorState`. `event_pass_through` is set on the wrapping node
/// so this layer never absorbs pointer events.
///
/// Sized to fill its allotted slot (the user-root subregion); paints
/// in window-local canvas coordinates, so the stroke lands exactly at
/// the selected widget's `bounds`.
pub(crate) struct HighlightLayer {
    state: InspectorState,
}

impl HighlightLayer {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for HighlightLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HighlightLayer").finish()
    }
}

impl Widget for HighlightLayer {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Re-paint when the selection's bounds change.
        let self_id = ctx.self_id();
        self.state
            .selected_bounds
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Fill whatever slot the parent gives us.
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, _bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let Some(rect) = self.state.selected_bounds.get() else {
            return;
        };
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        // Bright, theme-agnostic stroke. 2 px so it reads clearly on
        // both light and dark surfaces.
        let stroke = Color::from_rgba(0.13, 0.55, 1.0, 0.95);
        canvas.stroke_rounded_rect(rect, fern_tokens::CornerRadius::ZERO, stroke, 2.0);
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Invisible leaf widget. On every layout pass, mirrors
/// `tree.bounds(selected_id)` into `selected_bounds`. This keeps the
/// `HighlightLayer` accurate as the selected widget moves under
/// resize / scroll / theme changes — the tracker just rides along
/// the layout cycle and writes the latest bounds.
pub(crate) struct BoundsTracker {
    state: InspectorState,
}

impl BoundsTracker {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for BoundsTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundsTracker").finish()
    }
}

impl Widget for BoundsTracker {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // We need to relayout whenever the selection changes so our
        // own `layout_response` runs and re-syncs the bounds.
        let self_id = ctx.self_id();
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Sync as a side effect of layout. `Signal::set` is a no-op
        // when the value is unchanged, so this stays cheap.
        let new_bounds = self
            .state
            .selected_id
            .get()
            .and_then(|id| ctx.widget_bounds(id))
            .filter(|r| r.width > 0.0 && r.height > 0.0);
        if self.state.selected_bounds.get() != new_bounds {
            self.state.selected_bounds.set(new_bounds);
        }
        // Zero-size: takes no space in any layout slot.
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
