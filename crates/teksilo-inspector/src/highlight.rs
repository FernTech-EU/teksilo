// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Visual highlight + selected-bounds tracker for the inspector.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::arena::WidgetArena;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::Color;

use crate::state::{InspectorState, OverlayMode};

/// `Widget::type_name()` returns the fully-qualified path. Layout
/// primitives all live under `teksilo_widgets::primitives::`, so a
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
        self.state.selected_bounds.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.state
            .overlay_mode
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state.overlay_opacity.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .hover_info
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state.band_snapshot.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.state.overflow_overlay.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.state.overflow_snapshot.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let opacity = self.state.overlay_opacity.get().clamp(0.0, 1.0);

        // The overflow overlay is independent of `overlay_mode` — it paints
        // whenever enabled, even with the inspector panel closed (mode == Off).
        if self.state.overflow_overlay.get() {
            paint_overflow(canvas, &self.state, opacity);
        }

        let mode = self.state.overlay_mode.get();
        if mode == OverlayMode::Off {
            return;
        }

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
        if let Some(rect) = self.state.selected_bounds.get()
            && rect.width > 0.0
            && rect.height > 0.0
        {
            let stroke = Color::from_rgba(0.13, 0.55, 1.0, 0.95 * opacity);
            canvas.stroke_rounded_rect(rect, teksilo_tokens::CornerRadius::ZERO, stroke, 2.0);
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
    canvas.fill_rounded_rect(bg_rect, teksilo_tokens::CornerRadius::uniform(3.0), bg);
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
        canvas.fill_rounded_rect(band.bounds, teksilo_tokens::CornerRadius::ZERO, color);
    }
}

/// Band width, and the gap between bands, in the hazard pattern (px).
const PITCH: f32 = 10.0;

/// Tallest slice of an overflow strip that a single hazard band may span (px).
///
/// A 45° band drawn across a region of height `c` has a **bounding box** of
/// roughly `(c + PITCH) x c` — it slides one pixel sideways for every pixel down.
/// `teksilo-render`'s `PathAtlas` rasterizes a path by its bounding box and only
/// *composites* it through the clip, so `set_clip` does nothing to shrink the
/// bitmap: a band drawn across a tall strip in one piece is a quadratic raster.
///
/// That is not theoretical. A scene overflowing a narrowed window produced a
/// 7563px-tall strip, so each band became a single 7573x7563 path — a 229 MB
/// rasterization the atlas (max 4096px) could never store, so it was rebuilt and
/// discarded *every frame*, wedging the UI thread at 100% CPU for as long as the
/// overflow was on screen.
///
/// Slicing the strip caps every emitted path at about `(CHUNK + PITCH)²` px
/// regardless of how large the overflowing region is, which is the property that
/// makes this overlay safe to leave on.
const CHUNK: f32 = 64.0;

/// Paint Flutter-style yellow/black hazard stripes over each overflow strip,
/// with a bright red border, so over-constrained layouts are impossible to
/// miss in debug builds.
///
/// The stripes are emitted in bounded-height slices (see [`CHUNK`]) rather than
/// as full-height bands, because the path rasterizer sizes its bitmap from the
/// path's bounding box, not from the clip.
fn paint_overflow(canvas: &mut Canvas, state: &InspectorState, opacity: f32) {
    let strips = state.overflow_snapshot.get_ref();
    if strips.is_empty() {
        return;
    }
    let yellow = Color::from_rgba(0.98, 0.80, 0.10, 0.55 * opacity);
    let black = Color::from_rgba(0.0, 0.0, 0.0, 0.55 * opacity);
    let border = Color::from_rgba(0.95, 0.15, 0.10, 0.95 * opacity);

    for strip in strips.iter() {
        if strip.width <= 0.0 || strip.height <= 0.0 {
            continue;
        }
        // Clip so the slanted bands stay inside the strip, then lay a yellow
        // base and 45° black diagonals (band width = gap width = PITCH).
        canvas.set_clip(*strip);
        canvas.fill_rect(*strip, yellow);
        paint_hazard_bands(canvas, *strip, black);
        canvas.clear_clip();

        canvas.stroke_rounded_rect(*strip, teksilo_tokens::CornerRadius::ZERO, border, 1.5);
    }
}

/// The 45° black diagonals of one hazard strip, emitted in [`CHUNK`]-tall slices.
///
/// Every band is a parallelogram that descends one slice only, so its bounding box
/// stays bounded by `CHUNK` no matter how tall `strip` is. The bands are anchored to
/// a single global lattice — a stripe is the locus where `x - y` falls in a fixed
/// residue window — so consecutive slices line up and the diagonals read as
/// continuous across the seams rather than resetting at each one.
fn paint_hazard_bands(canvas: &mut Canvas, strip: Rect, color: Color) {
    use teksilo_canvas::{Path, Point};

    let lattice = 2.0 * PITCH;
    let (y0, y1) = (strip.y, strip.bottom());

    let mut cy0 = y0;
    while cy0 < y1 {
        let cy1 = (cy0 + CHUNK).min(y1);
        let c = cy1 - cy0;

        // A band whose left edge is at `u` on this slice's top spans x in
        // [u, u + PITCH + c] by the time it reaches the bottom, so it can touch the
        // strip only for u in [strip.x - PITCH - c, strip.right()].
        let first = strip.x - PITCH - c;
        // Snap that start onto the global lattice, phase-shifted by how far this
        // slice has slid down the diagonal — this is what keeps the seams invisible.
        let anchor = strip.x - PITCH + (cy0 - y0);
        let k = ((first - anchor) / lattice).floor();
        let mut u = anchor + k * lattice;

        let end = strip.right() + PITCH;
        while u < end {
            let mut p = Path::new();
            p.move_to(Point::new(u, cy0));
            p.line_to(Point::new(u + PITCH, cy0));
            p.line_to(Point::new(u + PITCH + c, cy1));
            p.line_to(Point::new(u + c, cy1));
            p.close();
            canvas.fill_path(&p, color);
            u += lattice;
        }
        cy0 = cy1;
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
        canvas.stroke_rounded_rect(entry.bounds, teksilo_tokens::CornerRadius::ZERO, color, 1.0);
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
        // Re-run the overflow walk when the toggle flips.
        self.state.overflow_overlay.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        Vec::new()
    }

    /// `BoundsTracker` opts out of the per-pass layout cache defensively. Its
    /// whole-tree snapshot into signals IS its purpose (not a by-product of
    /// sizing), so we keep it running on every query rather than relying on the
    /// idempotency the cache assumes.
    fn cacheable_layout(&self) -> bool {
        false
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
                        let type_label = last_segment(node.widget.type_name()).to_string();
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

        // Overflow detection runs INDEPENDENT of `overlay_mode` (it is on by
        // default and works with the panel closed). Walk the user-root
        // subtrees and collect the regions where a distributing container's
        // children spill past its bounds.
        if self.state.overflow_overlay.get() {
            if let Some(arena) = ctx.arena() {
                let mut overflow: Vec<Rect> = Vec::new();
                for &root in self.state.user_root_ids.get().iter() {
                    collect_overflow(arena, root, &mut overflow);
                }
                let changed = *self.state.overflow_snapshot.get_ref() != overflow;
                if changed {
                    self.state.overflow_snapshot.set(overflow);
                }
            }
        } else if !self.state.overflow_snapshot.get_ref().is_empty() {
            self.state.overflow_snapshot.set(Vec::new());
        }

        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Recursively collect the **overhang strips** where a distributing
/// container's children spill past its bounds. Only `HStack` / `VStack` /
/// `Grid` / `FormLayout` are considered (so intentional overlap in `ZStack`,
/// scene content, and overlays never false-positive), and containers that clip
/// their children (`ScrollArea`, `MaxSize`) are skipped (their overflow is
/// expected and clipped away).
fn collect_overflow(arena: &WidgetArena, id: WidgetId, out: &mut Vec<Rect>) {
    if !arena.is_active(id) {
        return;
    }
    let Some(node) = arena.get(id) else { return };
    let label = last_segment(node.widget.type_name());
    let is_distributor = matches!(label, "HStack" | "VStack" | "Grid" | "FormLayout");
    let children: Vec<WidgetId> = arena.children(id).to_vec();

    if is_distributor && !node.clips_children && !children.is_empty() {
        let parent = arena.bounds(id);
        // Union of active children's bounds.
        let (mut ux0, mut uy0) = (f32::INFINITY, f32::INFINITY);
        let (mut ux1, mut uy1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut any = false;
        for &c in &children {
            if !arena.is_active(c) {
                continue;
            }
            let b = arena.bounds(c);
            if b.width <= 0.0 || b.height <= 0.0 {
                continue;
            }
            ux0 = ux0.min(b.x);
            uy0 = uy0.min(b.y);
            ux1 = ux1.max(b.right());
            uy1 = uy1.max(b.bottom());
            any = true;
        }
        if any {
            const EPS: f32 = 0.5;
            if ux1 > parent.right() + EPS {
                out.push(Rect::new(
                    parent.right(),
                    parent.y,
                    ux1 - parent.right(),
                    parent.height,
                ));
            }
            if ux0 < parent.x - EPS {
                out.push(Rect::new(ux0, parent.y, parent.x - ux0, parent.height));
            }
            if uy1 > parent.bottom() + EPS {
                out.push(Rect::new(
                    parent.x,
                    parent.bottom(),
                    parent.width,
                    uy1 - parent.bottom(),
                ));
            }
            if uy0 < parent.y - EPS {
                out.push(Rect::new(parent.x, uy0, parent.width, parent.y - uy0));
            }
        }
    }

    for c in children {
        collect_overflow(arena, c, out);
    }
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

#[cfg(test)]
mod overflow_tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_widgets::HStack;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            teksilo_canvas::Size::new(self.0, self.1).into()
        }
    }

    /// Runs `collect_overflow` from `root` during layout (where `ctx.arena()`
    /// is available, as `BoundsTracker` does) and stashes the result.
    #[derive(Debug)]
    struct OverflowProbe {
        root: WidgetId,
        out: Rc<RefCell<Vec<Rect>>>,
    }
    impl Widget for OverflowProbe {
        fn layout_response(&self, p: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
            if let Some(arena) = ctx.arena() {
                let mut v = Vec::new();
                collect_overflow(arena, self.root, &mut v);
                *self.out.borrow_mut() = v;
            }
            p.resolve(0.0, 0.0).into()
        }
        fn cacheable_layout(&self) -> bool {
            false
        }
    }

    // The HStack is a root laid out at `exact(box_w)`, so the driver commits
    // its bounds to `box_w` (a real constraint) while a rigid child keeps its
    // natural width. The probe is a second root, laid out after the HStack in
    // the same pass, so it sees the HStack's committed bounds.
    fn run(child_w: f32, box_w: f32) -> Vec<Rect> {
        let out = Rc::new(RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FixedLeaf(child_w, 20.0));
        let hstack = tree.add(HStack::new().add_child(leaf));
        let _probe = tree.add(OverflowProbe {
            root: hstack,
            out: out.clone(),
        });
        tree.layout(SizeProposal::exact(box_w, 40.0));
        out.borrow().clone()
    }

    #[test]
    fn over_constrained_hstack_is_flagged() {
        // 200px rigid child in a 100px box → 100px trailing overhang.
        let strips = run(200.0, 100.0);
        assert_eq!(
            strips.len(),
            1,
            "expected one overhang strip, got {strips:?}"
        );
        assert!(
            (strips[0].width - 100.0).abs() < 1.0,
            "overhang width should be ~100, got {}",
            strips[0].width
        );
    }

    #[test]
    fn fitting_hstack_is_not_flagged() {
        // 50px child in a 100px box → no overflow.
        let strips = run(50.0, 100.0);
        assert!(
            strips.is_empty(),
            "fitting layout should not flag, got {strips:?}"
        );
    }
}

#[cfg(test)]
mod hazard_stripe_tests {
    use super::*;
    use teksilo_canvas::Canvas;

    /// Every hazard band emitted for `strip`, as `(width, height)` of its bounding box.
    fn band_bounds(strip: Rect) -> Vec<(f32, f32)> {
        let mut canvas = Canvas::new();
        paint_hazard_bands(&mut canvas, strip, Color::from_rgba(0.0, 0.0, 0.0, 1.0));
        canvas
            .into_render_frame()
            .paths
            .iter()
            .map(|p| (p.bounds[2], p.bounds[3]))
            .collect()
    }

    /// The freeze, pinned.
    ///
    /// A scene overflowing a narrowed window produced a 7563px-tall strip. Each hazard
    /// band used to span the whole strip in one piece, so its *bounding box* — which is
    /// what the path rasterizer sizes its bitmap from, `set_clip` notwithstanding —
    /// became 7573x7563: a 229 MB rasterization, too big for the 4096px atlas to ever
    /// store, and therefore rebuilt and thrown away on every single frame. The UI thread
    /// sat at 100% CPU and never came back.
    ///
    /// No band may be bigger than one slice, however tall the overflow is.
    #[test]
    fn a_very_tall_overflow_emits_no_oversized_band() {
        let strip = Rect::new(48.0, 118.0, 40.0, 7563.0);
        let bands = band_bounds(strip);

        assert!(!bands.is_empty(), "a tall strip must still be striped");

        let cap = CHUNK + PITCH + 1.0; // +1 for the ceil() the rasterizer applies
        for (w, h) in &bands {
            assert!(
                *w <= cap && *h <= cap,
                "a hazard band is {w}x{h}, over the {cap} cap — a band's bounding box \
                 must stay bounded by CHUNK no matter how tall the strip is, or the \
                 path atlas re-rasterizes hundreds of MB every frame"
            );
        }

        // And it must stay bounded by the atlas's own limit, which is the property
        // that actually prevents the freeze.
        let worst = bands.iter().fold(0.0_f32, |m, (w, h)| m.max(w.max(*h)));
        assert!(
            worst < 4096.0,
            "worst band dimension {worst} would not fit the 4096px path atlas"
        );
    }

    /// Slicing the strip must not make the diagonals restart at every seam: the bands
    /// are anchored to one global lattice, so a stripe leaving the bottom of one slice
    /// enters the top of the next at exactly the same offset. Without this the overlay
    /// reads as a stack of disconnected chevrons.
    #[test]
    fn the_diagonals_stay_continuous_across_slice_seams() {
        // Two slices' worth of height, and a strip wide enough to hold whole bands.
        let strip = Rect::new(0.0, 0.0, 200.0, CHUNK * 2.0);
        let mut canvas = Canvas::new();
        paint_hazard_bands(&mut canvas, strip, Color::from_rgba(0.0, 0.0, 0.0, 1.0));
        let frame = canvas.into_render_frame();

        // A stripe is the locus where (x - y) sits in a fixed residue window modulo
        // 2*PITCH. Take each band's top-left vertex and check every one shares the
        // same residue — that is exactly what "the diagonals line up" means, and it
        // holds across the seam only because each slice is phase-shifted by its depth.
        let lattice = 2.0 * PITCH;
        let residues: Vec<f32> = frame
            .paths
            .iter()
            .map(|p| {
                // bounds = [x, y, w, h]; the top-left vertex of a band is (x, y)
                // for bands fully inside, so use the bounds origin.
                let r = (p.bounds[0] - p.bounds[1]).rem_euclid(lattice);
                (r * 100.0).round() / 100.0
            })
            .collect();

        assert!(!residues.is_empty(), "expected bands");
        let first = residues[0];
        for r in &residues {
            let d = (r - first).abs().min(lattice - (r - first).abs());
            assert!(
                d < 0.01,
                "band phases diverge ({first} vs {r}) — the slices are not anchored to \
                 one lattice, so the diagonals break at the seam"
            );
        }
    }

    /// A short strip is unaffected — the slicing is transparent below one CHUNK.
    #[test]
    fn a_short_overflow_is_still_striped() {
        let strip = Rect::new(0.0, 0.0, 100.0, 20.0);
        let bands = band_bounds(strip);
        assert!(!bands.is_empty(), "a short strip must still be striped");
        for (w, h) in &bands {
            assert!(*h <= 20.0 + 1.0, "band height {h} exceeds the strip's 20px");
            assert!(
                *w <= CHUNK + PITCH + 1.0,
                "band width {w} unexpectedly large"
            );
        }
    }
}
