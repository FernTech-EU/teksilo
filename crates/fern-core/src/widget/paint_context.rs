use fern_canvas::{Rect, Size};

use crate::arena::WidgetArena;
use crate::widget_id::WidgetId;

/// Context available during painting.
pub struct PaintContext<'a> {
    pub theme: &'a crate::styles::Theme,
    pub scale_factor: f32,
    /// Active layout direction. Used by widgets that have to resolve
    /// Leading/Trailing semantics into geometric Left/Right at paint
    /// time (e.g. attached-side shadow suppression on a popover that
    /// opened off the trailing edge of its anchor).
    pub layout_direction: crate::environment::LayoutDirection,
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
    pub origin: fern_canvas::Point,
    pub size: Size,
}
