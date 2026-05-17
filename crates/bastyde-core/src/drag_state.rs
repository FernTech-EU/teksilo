//! Drag session state and drop feedback types.
//!
//! A `DragSession` is created when a widget calls `EventContext::start_drag()`
//! and lives on the `WidgetTree` until the drag completes or is cancelled.

use bastyde_canvas::{Point, Rect};
use bastyde_tokens::Color;

use crate::drag_payload::DragPayload;
use crate::widget_id::WidgetId;

/// Visual feedback rendered by a drop target during a drag hover.
#[derive(Debug, Clone)]
pub enum DropFeedback {
    /// A horizontal line at the given Y coordinate, spanning the given width.
    /// Used for insertion between list items.
    InsertionLine { y: f32, width: f32 },
    /// A highlighted rectangle. Used for container/folder drops.
    HighlightRect { rect: Rect, color: Color },
    /// No feedback (payload not accepted by this target).
    NoFeedback,
}

/// Active drag-and-drop session state, stored on the `WidgetTree`.
pub(crate) struct DragSession {
    /// The data being dragged.
    pub payload: DragPayload,
    /// The widget that initiated the drag.
    pub source_widget: WidgetId,
    /// Current pointer position during drag.
    pub current_position: Point,
    /// The widget currently under the pointer that accepts this payload, if any.
    pub current_target: Option<WidgetId>,
    /// Visual feedback from the current drop target.
    pub feedback: DropFeedback,
    /// Widget ID of the preview overlay content (if any).
    pub preview_content_id: Option<WidgetId>,
    /// Overlay ID for the preview (if any).
    pub preview_overlay_id: Option<crate::overlay::OverlayId>,
}

impl std::fmt::Debug for DragSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragSession")
            .field("source_widget", &self.source_widget)
            .field("current_position", &self.current_position)
            .field("current_target", &self.current_target)
            .field("feedback", &self.feedback)
            .finish()
    }
}
