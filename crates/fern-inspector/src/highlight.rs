//! Visual highlight + selected-bounds tracker for the inspector.

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::arena::WidgetArena;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

use crate::state::{InspectorState, OverlayMode};

/// `Widget::type_name()` returns the fully-qualified path. Layout
/// primitives all live under `fern_widgets::primitives::`, so a
/// substring match is the cheapest reliable classifier.
pub(crate) fn is_layout_primitive(type_name: &str) -> bool {
    type_name.contains("::primitives::")
}

/// Decorative leaf widget that paints overlay strokes for the
/// inspector. Driven by `InspectorState::overlay_mode`:
///
/// - `Off`: paints nothing.
/// - `SelectionOnly`: stroke around the selected widget's bounds.
/// - `AllBounds`: stroke every active widget; layout primitives in
///   cyan, content widgets in magenta. The selected widget gets a
///   thicker accent stroke on top.
///
/// `event_pass_through` is set on the wrapping node so this layer
/// never absorbs pointer events.
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
        let self_id = ctx.self_id();
        // Repaint when selection, mode, or opacity change.
        self.state
            .selected_bounds
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .overlay_mode
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .overlay_opacity
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        // AllBounds mode walks the arena every paint — keep us
        // dirtied on every paint epoch via a Relayout binding to
        // selected_id (cheap, ensures we paint after every layout).
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, _bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let mode = self.state.overlay_mode.get();
        if mode == OverlayMode::Off {
            return;
        }
        let opacity = self.state.overlay_opacity.get().clamp(0.0, 1.0);

        if mode == OverlayMode::AllBounds {
            // We rely on the arena that PaintContext does not carry
            // — paint walks per-widget but doesn't expose the tree.
            // Workaround: this widget is invoked during paint with
            // its own bounds; the *all-bounds* visualization can use
            // the LayoutContext-driven snapshot stored on the
            // inspector state by `BoundsTracker`. Slice 3 keeps it
            // simple by walking via the same channel: we can't reach
            // the arena from paint, so AllBounds overlay reads a
            // snapshot kept up-to-date by the tracker.
            paint_all_bounds(canvas, ctx, &self.state, opacity);
        }

        // Selection stroke (drawn over everything in AllBounds mode).
        if let Some(rect) = self.state.selected_bounds.get() {
            if rect.width > 0.0 && rect.height > 0.0 {
                let stroke = Color::from_rgba(0.13, 0.55, 1.0, 0.95 * opacity);
                canvas.stroke_rounded_rect(
                    rect,
                    fern_tokens::CornerRadius::ZERO,
                    stroke,
                    2.0,
                );
            }
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn paint_all_bounds(
    canvas: &mut Canvas,
    _ctx: &PaintContext,
    state: &InspectorState,
    opacity: f32,
) {
    // Read the snapshot from `state.bounds_snapshot` — populated by
    // `BoundsTracker` on every layout pass.
    let snapshot = state.bounds_snapshot.get_ref();
    let layout_color = Color::from_rgba(0.0, 0.7, 0.85, 0.55 * opacity);
    let content_color = Color::from_rgba(0.85, 0.0, 0.55, 0.55 * opacity);

    for entry in snapshot.iter() {
        if entry.bounds.width <= 0.0 || entry.bounds.height <= 0.0 {
            continue;
        }
        let color = if entry.is_layout {
            layout_color
        } else {
            content_color
        };
        canvas.stroke_rounded_rect(entry.bounds, fern_tokens::CornerRadius::ZERO, color, 1.0);
    }
}

/// Per-widget snapshot entry kept in `InspectorState::bounds_snapshot`.
#[derive(Clone, Debug)]
pub(crate) struct BoundsEntry {
    pub bounds: Rect,
    pub is_layout: bool,
}

/// Invisible leaf widget. On every layout pass:
/// - Mirrors `tree.bounds(selected_id)` into `selected_bounds`.
/// - When overlay mode is `AllBounds`, snapshots every active
///   widget's bounds into `bounds_snapshot` for the `HighlightLayer`
///   to draw.
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
        let self_id = ctx.self_id();
        // Re-run layout on selection or mode changes so we re-snapshot.
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .overlay_mode
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Update selected_bounds.
        let new_bounds = self
            .state
            .selected_id
            .get()
            .and_then(|id| ctx.widget_bounds(id))
            .filter(|r| r.width > 0.0 && r.height > 0.0);
        if self.state.selected_bounds.get() != new_bounds {
            self.state.selected_bounds.set(new_bounds);
        }

        // Update bounds snapshot when AllBounds is active.
        if self.state.overlay_mode.get() == OverlayMode::AllBounds {
            if let Some(arena) = ctx.arena() {
                let exclude = self.state.shell_root_id.get();
                let mut snap: Vec<BoundsEntry> = Vec::new();
                for &root in arena.roots().iter() {
                    if Some(root) == exclude {
                        continue;
                    }
                    collect_bounds(arena, root, exclude, &mut snap);
                }
                self.state.bounds_snapshot.set(snap);
            }
        } else if !self.state.bounds_snapshot.get_ref().is_empty() {
            // Clear snapshot when we leave AllBounds so a stale
            // overlay doesn't ghost.
            self.state.bounds_snapshot.set(Vec::new());
        }

        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn collect_bounds(
    arena: &WidgetArena,
    id: WidgetId,
    exclude: Option<WidgetId>,
    out: &mut Vec<BoundsEntry>,
) {
    if Some(id) == exclude || !arena.is_active(id) {
        return;
    }
    let Some(node) = arena.get(id) else { return };
    let bounds = arena.bounds(id);
    let is_layout = is_layout_primitive(node.widget.type_name());
    out.push(BoundsEntry { bounds, is_layout });
    let children: Vec<WidgetId> = arena.children(id).to_vec();
    for child in children {
        collect_bounds(arena, child, exclude, out);
    }
}
