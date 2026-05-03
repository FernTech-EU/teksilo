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
        // Repaint when selection, mode, opacity, or hover change.
        self.state
            .selected_bounds
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .overlay_mode
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .overlay_opacity
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .hover_info
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let mode = self.state.overlay_mode.get();
        if mode == OverlayMode::Off {
            return;
        }
        let opacity = self.state.overlay_opacity.get().clamp(0.0, 1.0);

        if mode == OverlayMode::AllBounds {
            paint_all_bounds(canvas, ctx, &self.state, opacity);
            // Cursor-following tooltip — only in AllBounds mode where
            // the user is actively inspecting layout. Painted after
            // the bounds strokes so it sits on top.
            if let Some(info) = self.state.hover_info.get() {
                paint_hover_tooltip(canvas, ctx, &info, bounds, opacity);
            }
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

/// Paint the AllBounds-mode hover tooltip near `info.bounds`.
///
/// Default placement is just above the hovered widget, left-aligned
/// to its left edge. Flips below if it would clip the top of the
/// inspector layer's `bounds`, and shifts left if it would clip the
/// right edge. Width is estimated from the label string — Canvas does
/// not expose text measurement, so 6.5 px / char is a conservative
/// average for the body font.
fn paint_hover_tooltip(
    canvas: &mut Canvas,
    ctx: &PaintContext,
    info: &HoverInfo,
    layer_bounds: Rect,
    opacity: f32,
) {
    let theme = ctx.theme;
    let label = format_hover_label(info);

    const PADDING_X: f32 = 6.0;
    const PADDING_Y: f32 = 3.0;
    const TEXT_HEIGHT: f32 = 14.0;
    const CHAR_WIDTH: f32 = 6.5;
    const GAP: f32 = 4.0;

    let tooltip_width = (label.chars().count() as f32) * CHAR_WIDTH + PADDING_X * 2.0;
    let tooltip_height = TEXT_HEIGHT + PADDING_Y * 2.0;

    let mut x = info.bounds.x;
    let mut y = info.bounds.y - tooltip_height - GAP;

    // Edge-flip: place below the widget if above would clip the layer.
    if y < layer_bounds.y {
        y = info.bounds.y + info.bounds.height + GAP;
    }
    // Right-clip: shift left so the tooltip stays inside the layer.
    let right_edge = layer_bounds.x + layer_bounds.width;
    if x + tooltip_width > right_edge {
        x = (right_edge - tooltip_width).max(layer_bounds.x);
    }

    let bg = if info.is_layout {
        Color::from_rgba(0.0, 0.4, 0.55, 0.92 * opacity)
    } else {
        Color::from_rgba(0.55, 0.0, 0.4, 0.92 * opacity)
    };
    let fg = Color::from_rgba(1.0, 1.0, 1.0, opacity);

    let bg_rect = Rect::new(x, y, tooltip_width, tooltip_height);
    canvas.fill_rounded_rect(bg_rect, fern_tokens::CornerRadius::uniform(3.0), bg);
    canvas.draw_text(
        &label,
        Rect::new(
            x + PADDING_X,
            y + PADDING_Y,
            tooltip_width - PADDING_X * 2.0,
            TEXT_HEIGHT,
        ),
        &theme.typography.body,
        fg,
    );
}

fn format_hover_label(info: &HoverInfo) -> String {
    // Round to whole logical pixels — sub-pixel widths add noise when
    // scanning many widgets at once.
    let w = info.bounds.width.round() as i32;
    let h = info.bounds.height.round() as i32;
    format!("{} · {}×{}", info.type_label, w, h)
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

/// Resolved hover descriptor used by the AllBounds tooltip. Populated
/// by `BoundsTracker` from `state.hover_id`; consumed by
/// `HighlightLayer::paint`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HoverInfo {
    pub bounds: Rect,
    pub type_label: String,
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
        // Re-run layout on selection, mode, or hover changes so we
        // re-snapshot. Hover changes also invalidate `hover_info`.
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .overlay_mode
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .hover_id
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

        // Update bounds snapshot + hover_info when AllBounds is active.
        let mode = self.state.overlay_mode.get();
        if mode == OverlayMode::AllBounds {
            let exclude = self.state.shell_root_id.get();
            if let Some(arena) = ctx.arena() {
                let mut snap: Vec<BoundsEntry> = Vec::new();
                for &root in arena.roots().iter() {
                    if Some(root) == exclude {
                        continue;
                    }
                    collect_bounds(arena, root, exclude, &mut snap);
                }
                self.state.bounds_snapshot.set(snap);

                // Resolve the hovered widget into a HoverInfo if it
                // exists and is not within the inspector's own subtree.
                let new_hover = self
                    .state
                    .hover_id
                    .get()
                    .filter(|id| !is_in_subtree(arena, *id, exclude))
                    .and_then(|id| {
                        let node = arena.get(id)?;
                        let bounds = arena.bounds(id);
                        if bounds.width <= 0.0 || bounds.height <= 0.0 {
                            return None;
                        }
                        let type_label =
                            last_segment(node.widget.type_name()).to_string();
                        let is_layout = is_layout_primitive(node.widget.type_name());
                        Some(HoverInfo {
                            bounds,
                            type_label,
                            is_layout,
                        })
                    });
                if self.state.hover_info.get() != new_hover {
                    self.state.hover_info.set(new_hover);
                }
            }
        } else {
            // Clear snapshot + hover_info when we leave AllBounds so
            // a stale overlay or tooltip doesn't ghost.
            if !self.state.bounds_snapshot.get_ref().is_empty() {
                self.state.bounds_snapshot.set(Vec::new());
            }
            if self.state.hover_info.get().is_some() {
                self.state.hover_info.set(None);
            }
        }

        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Walks ancestors of `id` looking for `exclude`. Used to skip the
/// inspector's own subtree when resolving the hovered widget.
fn is_in_subtree(arena: &WidgetArena, id: WidgetId, exclude: Option<WidgetId>) -> bool {
    let Some(target) = exclude else {
        return false;
    };
    let mut cur = Some(id);
    while let Some(c) = cur {
        if c == target {
            return true;
        }
        cur = arena.parent(c);
    }
    false
}

/// Last `::`-separated segment of a fully-qualified Rust type name.
/// Strips generics so `Switcher<...>` shows the bare segment. Mirrors
/// the helper in `tabs::mod` (kept local to highlight.rs to avoid a
/// cross-module dep on the tabs module).
fn last_segment(s: &str) -> &str {
    let bare = s.split_once('<').map(|(a, _)| a).unwrap_or(s);
    bare.rsplit_once("::").map(|(_, t)| t).unwrap_or(bare)
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
