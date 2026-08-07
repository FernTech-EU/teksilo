// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_canvas::{Rect, Size};

use crate::arena::WidgetArena;
use crate::widget_id::WidgetId;

/// Context available during painting.
pub struct PaintContext<'a> {
    pub theme: &'a crate::styles::Theme,
    /// Accumulated (quantized) scale of the transform scopes enclosing
    /// this widget — `1.0` outside any scale transform, the zoom-derived
    /// raster ladder value inside a `SceneView` / `Scale` wrapper. This
    /// is the ambient text raster scale the walker has already set on
    /// the shared `TextBackend`; widgets normally don't need it (text
    /// drawn via `Canvas::draw_text` / `draw_paragraph` picks it up
    /// automatically), but resolution-dependent custom paint can read
    /// it to densify its own raster content. NOT the HiDPI device scale
    /// — that lives on the renderer/text-service.
    pub scale_factor: f32,
    /// Combined user×OS text-scale factor (`1.0` = 100 %) — the *logical*
    /// accessibility magnification, distinct from the raster `scale_factor`
    /// above. Widgets that paint text via `Theme.typography` already scale
    /// through the effective theme; this is for paint paths that size text
    /// from another source (e.g. a scene `TextItem` that opts in). `1.0` when
    /// no scale is active.
    pub text_scale: f32,
    /// Active layout direction. Used by widgets that have to resolve
    /// Leading/Trailing semantics into geometric Left/Right at paint
    /// time (e.g. attached-side shadow suppression on a popover that
    /// opened off the trailing edge of its anchor).
    pub layout_direction: crate::environment::LayoutDirection,
    /// Whether the widget being painted is effectively enabled —
    /// `false` iff this node or any ancestor has its arena-level
    /// `enabled_state` resolved to `false`. Computed once per node by
    /// the paint walker (start `true` at root, AND with each node's
    /// `enabled_state` value as the walker descends).
    ///
    /// Leaf widgets that paint role-derived colors
    /// ([`crate::color_prop::ColorProp::TextRole`] and the dynamic
    /// variants) consult this to substitute `TextRole::Disabled`
    /// automatically — the single hook that makes any descendant of a
    /// disabled subtree dim without the composite parent doing
    /// per-color bookkeeping. Static and bound color props are not
    /// substituted (caller's literal wins).
    pub effective_enabled: bool,
    // TODO: Wire from platform accessibility settings (winit doesn't expose these yet)
    pub prefers_high_contrast: bool,
    pub prefers_reduced_motion: bool,
    pub prefers_large_text: bool,
    /// Whether the host window is currently active (`focused AND not
    /// occluded`). Widgets that change appearance when the window loses focus
    /// read this directly in `paint()` — the selection band in
    /// `TableView`/`TreeTableView` desaturates, text engines swap their
    /// selection colour, custom paint can dim. A window-active flip triggers a
    /// global repaint ([`WidgetArena::mark_all_needs_paint_only`]), so no
    /// per-widget signal binding is required to keep a paint-time read correct.
    /// `true` in headless test contexts.
    pub window_active: bool,
    /// The accumulated clip rectangle this widget is painted within — the
    /// intersection of every `clips_children` ancestor's bounds (a `ScrollArea`
    /// viewport, a `MaxSize`, …), in the same screen space as the widget's own
    /// `bounds`. `None` when no ancestor clips (the widget can paint anywhere).
    ///
    /// The paint walker already computes this to skip fully-offscreen subtrees;
    /// surfacing it lets a widget that is laid out larger than its visible slot
    /// — an editor at full document height inside an outer `ScrollArea`
    /// ("dubious mode") — window its own expensive work to `clip ∩ bounds`
    /// instead of processing the whole document. Correct under arbitrary
    /// nesting, since it is the intersection of *all* clipping ancestors.
    pub clip_bounds: Option<Rect>,
}

/// Read-only view of the widget tree's geometry passed to
/// [`Widget::after_paint`](super::Widget::after_paint). Wraps a borrow of
/// the arena so a parent widget can read the layout-resolved bounds of
/// any descendant (typically by ids it memoised during `build`) once
/// their paint pass has committed.
///
/// The view exposes only immutable queries; constructing one is
/// crate-internal so the contract stays narrow.
pub struct WidgetTreeView<'a> {
    arena: &'a WidgetArena,
}

impl<'a> WidgetTreeView<'a> {
    pub(crate) fn new(arena: &'a WidgetArena) -> Self {
        Self { arena }
    }

    /// Logical-pixel bounds of `id` after the most recent layout pass.
    /// Returns `Rect::ZERO` for unknown ids.
    pub fn bounds(&self, id: WidgetId) -> Rect {
        self.arena.bounds(id)
    }

    /// Direct arena children of `id`, in order.
    pub fn children(&self, id: WidgetId) -> &[WidgetId] {
        self.arena.children(id)
    }

    /// Whether `id` is a gesture dead-zone boundary — see
    /// [`WidgetNode::gesture_dead_zone`](crate::arena::WidgetNode::gesture_dead_zone)
    /// and the `DeadZone` wrapper widget. Lets a parent classify its own
    /// subtree from `after_paint`: `TitleBar` uses it to carve interactive
    /// controls out of the OS caption region it publishes.
    pub fn is_gesture_dead_zone(&self, id: WidgetId) -> bool {
        self.arena
            .get(id)
            .map(|n| n.gesture_dead_zone)
            .unwrap_or(false)
    }

    /// Whether `id` is active — i.e. not parked dormant by a `Switcher` or a
    /// `visible_when` gate. A dormant node's [`bounds`](Self::bounds) are
    /// stale, so callers walking a subtree must skip it.
    pub fn is_active(&self, id: WidgetId) -> bool {
        self.arena.is_active(id)
    }
}

/// Placement of a child widget during layout.
#[derive(Debug, Clone, Copy)]
pub struct WidgetPlacement {
    pub id: WidgetId,
    pub origin: teksilo_canvas::Point,
    pub size: Size,
}
