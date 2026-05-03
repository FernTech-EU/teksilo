//! Picker tool — captures a click and resolves it to a `WidgetId` on
//! the next layout pass. See `InspectorState::picker_mode` and
//! `InspectorState::pending_pick_point`.

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_tokens::Color;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;

use crate::state::InspectorState;

/// Transparent leaf widget that covers the user-root subregion when
/// picker mode is active. Captures the next pointer-down and stashes
/// the position into `pending_pick_point`. The actual hit-test
/// (mapping point → widget id) is performed by `PickResolver` on the
/// following layout pass, where it can read the arena via
/// `LayoutContext`.
///
/// Mounted only when `picker_mode == true` (see `InspectorShell`).
/// While mounted it is **not** `event_pass_through` — that's the whole
/// point: it intercepts the click so the user's app doesn't react to
/// it.
pub(crate) struct PickerOverlay {
    state: InspectorState,
}

impl PickerOverlay {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for PickerOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickerOverlay").finish()
    }
}

impl Widget for PickerOverlay {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Capture primary pointer-down anywhere within our slot.
        // `PickResolver` resolves the point → widget id on the next
        // layout pass and turns picker mode off.
        let state_for_handler = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(false)
            .on_pointer_event(move |event, _ctx| match event {
                WidgetEvent::PointerDown {
                    position,
                    button: PointerButton::Primary,
                    ..
                } => {
                    state_for_handler.pending_pick_point.set(Some(*position));
                    EventResponse::Handled
                }
                // Eat all other pointer events while picking so the
                // user's widgets don't get confusing partial input.
                _ => EventResponse::Handled,
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        // Faint tint so the user sees that "pick mode" is on.
        let tint = Color::from_rgba(0.13, 0.55, 1.0, 0.05);
        canvas.fill_rounded_rect(bounds, fern_tokens::CornerRadius::ZERO, tint);
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Invisible leaf widget. On every layout pass, if
/// `pending_pick_point` is set, hit-tests it against the arena via
/// `LayoutContext::widget_at_point` (excluding the inspector shell's
/// own subtree so the picker never picks itself). Updates
/// `selected_id`, clears `pending_pick_point`, and turns off
/// `picker_mode` once a pointer-down was resolved.
pub(crate) struct PickResolver {
    state: InspectorState,
}

impl PickResolver {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for PickResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickResolver").finish()
    }
}

impl Widget for PickResolver {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Re-run layout when the pending point changes.
        let self_id = ctx.self_id();
        self.state.pending_pick_point.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        if let Some(point) = self.state.pending_pick_point.get() {
            // Multi-window: try every shell exclusion in turn. The
            // hit-test API takes a single exclude id, so we walk all
            // shells looking for a non-shell hit. With one window this
            // collapses to the previous behavior.
            let excludes = self.state.shell_root_ids.get();
            let hit = if excludes.is_empty() {
                ctx.widget_at_point(point, None)
            } else {
                excludes
                    .iter()
                    .find_map(|&shell| ctx.widget_at_point(point, Some(shell)))
            };
            if let Some(id) = hit {
                self.state.selected_id.set(Some(id));
            }
            self.state.pending_pick_point.set(None);
            // Always exit picker mode after a pointer event resolved
            // — even if the hit-test missed (e.g. point landed inside
            // the excluded subtree). The user can re-enter pick mode
            // from the toolbar.
            if self.state.picker_mode.get() {
                self.state.picker_mode.set(false);
            }
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
