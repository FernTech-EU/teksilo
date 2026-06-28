// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use bastyde_canvas::{Rect, Size};

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
}

/// Placement of a child widget during layout.
#[derive(Debug, Clone, Copy)]
pub struct WidgetPlacement {
    pub id: WidgetId,
    pub origin: bastyde_canvas::Point,
    pub size: Size,
}
