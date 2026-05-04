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
        self.state
            .band_snapshot
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
            // Bands paint first so the strokes drawn over them stay
            // crisp and the tooltip sits on the very top.
            paint_bands(canvas, &self.state, opacity);
            paint_all_bounds(canvas, ctx, &self.state, opacity);
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

fn paint_bands(canvas: &mut Canvas, state: &InspectorState, opacity: f32) {
    let bands = state.band_snapshot.get_ref();
    if bands.is_empty() {
        return;
    }
    // PaddingInset = warm yellow, StackGap = soft green. Translucent
    // so the underlying widget colors still show through.
    let inset_fill = Color::from_rgba(0.95, 0.85, 0.20, 0.20 * opacity);
    let gap_fill = Color::from_rgba(0.20, 0.85, 0.55, 0.20 * opacity);
    for band in bands.iter() {
        if band.bounds.width <= 0.0 || band.bounds.height <= 0.0 {
            continue;
        }
        let color = match band.kind {
            BandKind::PaddingInset => inset_fill,
            BandKind::StackGap => gap_fill,
        };
        canvas.fill_rounded_rect(band.bounds, fern_tokens::CornerRadius::ZERO, color);
    }
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

/// Per-band snapshot entry kept in `InspectorState::band_snapshot`.
/// Painted as a filled translucent rect by `HighlightLayer` to make
/// per-axis spacing visible inside Padding widgets and between
/// consecutive HStack/VStack siblings.
#[derive(Clone, Debug)]
pub(crate) struct BandEntry {
    pub bounds: Rect,
    pub kind: BandKind,
}

/// Two flavors of spacing band — different colors so the user can tell
/// inset from gap at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BandKind {
    /// Space inside a `Padding` widget that lies outside its child.
    PaddingInset,
    /// Space between consecutive `HStack`/`VStack` siblings.
    StackGap,
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

        // Update bounds snapshot + bands + hover_info when AllBounds
        // is active. Walk the user-root subtrees only — never
        // `arena.roots()` — so the inspector's own chrome (panel,
        // overlays) doesn't show up in the bounds overlay or the
        // hover tooltip.
        let mode = self.state.overlay_mode.get();
        if mode == OverlayMode::AllBounds {
            let user_roots = self.state.user_root_ids.get();
            if let Some(arena) = ctx.arena() {
                let mut snap: Vec<BoundsEntry> = Vec::new();
                let mut bands: Vec<BandEntry> = Vec::new();
                for &root in user_roots.iter() {
                    collect_bounds_and_bands(arena, root, &mut snap, &mut bands);
                }
                self.state.bounds_snapshot.set(snap);
                self.state.band_snapshot.set(bands);

                // Resolve the hovered widget into a HoverInfo only if
                // it lives inside one of the user-root subtrees —
                // hovering over the inspector panel itself shouldn't
                // pop a tooltip describing the panel's internals.
                let new_hover = self
                    .state
                    .hover_id
                    .get()
                    .filter(|id| is_in_any_user_root(arena, *id, &user_roots))
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
            // Clear snapshots + hover_info when we leave AllBounds so
            // a stale overlay or tooltip doesn't ghost.
            if !self.state.bounds_snapshot.get_ref().is_empty() {
                self.state.bounds_snapshot.set(Vec::new());
            }
            if !self.state.band_snapshot.get_ref().is_empty() {
                self.state.band_snapshot.set(Vec::new());
            }
            if self.state.hover_info.get().is_some() {
                self.state.hover_info.set(None);
            }
        }

        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Walks ancestors of `id` looking for any user_root_id. Returns
/// true when `id` (or one of its ancestors) is itself a user-root —
/// i.e. `id` lives inside the user app subtree, not the inspector
/// chrome.
fn is_in_any_user_root(arena: &WidgetArena, id: WidgetId, user_roots: &[WidgetId]) -> bool {
    if user_roots.is_empty() {
        return false;
    }
    let mut cur = Some(id);
    while let Some(c) = cur {
        if user_roots.contains(&c) {
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

fn collect_bounds_and_bands(
    arena: &WidgetArena,
    id: WidgetId,
    out: &mut Vec<BoundsEntry>,
    bands: &mut Vec<BandEntry>,
) {
    if !arena.is_active(id) {
        return;
    }
    let Some(node) = arena.get(id) else { return };
    let bounds = arena.bounds(id);
    let is_layout = is_layout_primitive(node.widget.type_name());
    out.push(BoundsEntry { bounds, is_layout });

    // Spacing bands derived from this widget's relationship to its
    // children — Padding insets and HStack/VStack gaps are the two
    // patterns that produce "visible empty space" the user wants to
    // see.
    let label = last_segment(node.widget.type_name());
    let children: Vec<WidgetId> = arena.children(id).to_vec();
    if label == "Padding" && children.len() == 1 {
        let child = children[0];
        if arena.is_active(child) {
            let inner = arena.bounds(child);
            push_padding_bands(bounds, inner, bands);
        }
    } else if (label == "HStack" || label == "VStack") && children.len() >= 2 {
        let active_children: Vec<Rect> = children
            .iter()
            .filter(|c| arena.is_active(**c))
            .map(|c| arena.bounds(*c))
            .filter(|r| r.width > 0.0 && r.height > 0.0)
            .collect();
        if active_children.len() >= 2 {
            push_stack_gap_bands(bounds, &active_children, label == "HStack", bands);
        }
    }

    for child in children {
        collect_bounds_and_bands(arena, child, out, bands);
    }
}

/// Emit up to four PaddingInset bands — top / bottom / leading /
/// trailing space between a `Padding`'s outer rect and its child's
/// inner rect. Skips zero-width sides.
fn push_padding_bands(outer: Rect, inner: Rect, bands: &mut Vec<BandEntry>) {
    let push = |bands: &mut Vec<BandEntry>, x, y, w, h| {
        if w > 0.0 && h > 0.0 {
            bands.push(BandEntry {
                bounds: Rect::new(x, y, w, h),
                kind: BandKind::PaddingInset,
            });
        }
    };
    let top_h = (inner.y - outer.y).max(0.0);
    let bottom_h = (outer.y + outer.height - (inner.y + inner.height)).max(0.0);
    let leading_w = (inner.x - outer.x).max(0.0);
    let trailing_w = (outer.x + outer.width - (inner.x + inner.width)).max(0.0);
    // Top band spans full outer width.
    push(bands, outer.x, outer.y, outer.width, top_h);
    // Bottom band spans full outer width.
    push(
        bands,
        outer.x,
        inner.y + inner.height,
        outer.width,
        bottom_h,
    );
    // Side bands cover only the inner-row slice (avoid double-tinting
    // the corners that the top/bottom bands already cover).
    push(bands, outer.x, inner.y, leading_w, inner.height);
    push(
        bands,
        inner.x + inner.width,
        inner.y,
        trailing_w,
        inner.height,
    );
}

/// Emit StackGap bands for the empty space between consecutive
/// children of an HStack (`horizontal == true`) or VStack. The gap
/// rect spans the parent's cross-axis extent and the inter-child
/// gap on the main axis.
fn push_stack_gap_bands(
    parent: Rect,
    child_bounds: &[Rect],
    horizontal: bool,
    bands: &mut Vec<BandEntry>,
) {
    // Sort by main-axis position so consecutive comparison is correct
    // even if the children were inserted out of visual order.
    let mut sorted: Vec<Rect> = child_bounds.to_vec();
    if horizontal {
        sorted.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        sorted.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
    }
    for pair in sorted.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if horizontal {
            let gap_x = a.x + a.width;
            let gap_w = b.x - gap_x;
            if gap_w > 0.0 {
                bands.push(BandEntry {
                    bounds: Rect::new(gap_x, parent.y, gap_w, parent.height),
                    kind: BandKind::StackGap,
                });
            }
        } else {
            let gap_y = a.y + a.height;
            let gap_h = b.y - gap_y;
            if gap_h > 0.0 {
                bands.push(BandEntry {
                    bounds: Rect::new(parent.x, gap_y, parent.width, gap_h),
                    kind: BandKind::StackGap,
                });
            }
        }
    }
}
