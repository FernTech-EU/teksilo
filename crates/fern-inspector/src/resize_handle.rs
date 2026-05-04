//! Top-edge resize handle for the inspector panel.
//!
//! A 6-pixel-tall horizontal strip sitting between the user-root area
//! and the panel switcher. While dragging, every `PointerMove` resizes
//! `state.panel_height` so the handle's top edge tracks the cursor
//! exactly — see [`HighlightLayer`] for the math used to keep the
//! widget-local frame stable under live layout.

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::widget::{CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, CornerRadius};

use crate::state::{InspectorState, MAX_PANEL_HEIGHT, MIN_PANEL_HEIGHT};

/// Visual height of the resize strip in logical pixels.
pub(crate) const HANDLE_HEIGHT: f32 = 6.0;

pub(crate) struct ResizeHandle {
    state: InspectorState,
}

impl ResizeHandle {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for ResizeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResizeHandle").finish()
    }
}

impl Widget for ResizeHandle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(false)
            .cursor(CursorIcon::RowResize)
            .on_pointer_event(move |event, ctx| match event {
                WidgetEvent::PointerDown {
                    position,
                    button: PointerButton::Primary,
                    ..
                } => {
                    // `position` is window-local. Snapshot both the
                    // anchor window-y and the panel's height at the
                    // press moment — every PointerMove computes a
                    // *total* delta against these, never deltas
                    // against the live (already-updated) height.
                    state
                        .panel_drag_anchor
                        .set(Some((position.y, state.panel_height.get())));
                    ctx.capture_pointer();
                    EventResponse::Handled
                }
                WidgetEvent::PointerMove { position } => {
                    if let Some((anchor_y, start_h)) = state.panel_drag_anchor.get() {
                        // Cursor moved UP from anchor → grow the
                        // panel by that amount; cursor moved DOWN →
                        // shrink. Total height is always derived from
                        // `start_h`, so the handle's top edge tracks
                        // the cursor 1:1.
                        let new_height = (start_h + (anchor_y - position.y))
                            .clamp(MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT);
                        if (new_height - state.panel_height.get()).abs() > f32::EPSILON {
                            state.panel_height.set(new_height);
                        }
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
                WidgetEvent::PointerUp { .. } => {
                    if state.panel_drag_anchor.get().is_some() {
                        state.panel_drag_anchor.set(None);
                        ctx.release_pointer();
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
                _ => EventResponse::Ignored,
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, HANDLE_HEIGHT).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Faint divider line so the user can see (and target) the
        // handle. Uses the theme's border color for consistency with
        // the panel below.
        let color = BorderRole::Default.resolve(&ctx.theme.colors);
        let stripe = Rect::new(
            bounds.x,
            bounds.y + (bounds.height * 0.5 - 0.5),
            bounds.width,
            1.0,
        );
        canvas.fill_rounded_rect(stripe, CornerRadius::ZERO, color);
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
