// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag session state and drop feedback types.
//!
//! A `DragSession` is created when a widget calls `EventContext::start_drag()`
//! and lives on the `WidgetTree` until the drag completes or is cancelled.

use teksilo_canvas::{Point, Rect};
use teksilo_tokens::Color;

use crate::drag_payload::DragPayload;
use crate::widget_id::WidgetId;

/// A drop target's response to a drag hovering over it.
///
/// The variant decides **hover bubbling**: a target that returns
/// [`NoFeedback`](Self::NoFeedback) does **not** engage the payload, so the
/// drag passes through to the next drop target up the tree. Any *engaging*
/// variant ([`Accept`](Self::Accept), [`InsertionLine`](Self::InsertionLine),
/// [`HighlightRect`](Self::HighlightRect)) stops the bubble and **settles the
/// drop target** for the rest of the drag.
///
/// On release, `on_drop` runs once on that settled target; its returned `bool`
/// reports acceptance to the source via `DropOutcome::InApp { accepted }` — it
/// does **not** re-bubble to another target when it returns `false` (the target
/// was already chosen during the hover phase).
#[derive(Debug, Clone)]
pub enum DropFeedback {
    /// A horizontal line at the given Y coordinate, spanning the given width.
    /// Used for insertion between list items. Engages the target.
    InsertionLine { y: f32, width: f32 },
    /// A highlighted rectangle. Used for container/folder drops. Engages the
    /// target.
    HighlightRect { rect: Rect, color: Color },
    /// Accepted, but the target draws its own visual (signal-driven, e.g.
    /// `DropTarget`'s `is_targeted` border) — the framework renders nothing
    /// extra. Engages the target (stops bubbling), distinct from `NoFeedback`.
    Accept,
    /// The payload is not accepted by this target — the drag bubbles to the
    /// next drop target up the tree (hover phase). The framework renders
    /// nothing.
    NoFeedback,
}

impl DropFeedback {
    /// Whether this response engages the payload (stops drop-target bubbling).
    /// Everything except [`NoFeedback`](Self::NoFeedback) engages.
    pub fn is_engaged(&self) -> bool {
        !matches!(self, DropFeedback::NoFeedback)
    }
}

/// Active drag-and-drop session state, stored on the `WidgetTree`.
pub(crate) struct DragSession {
    /// The data being dragged.
    pub payload: DragPayload,
    /// The widget that initiated the drag. `None` for external (OS) drags,
    /// which have no in-app source widget.
    pub source_widget: Option<WidgetId>,
    /// Whether this session was started by an external (OS) drag.
    pub is_external: bool,
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
            .field("is_external", &self.is_external)
            .field("current_position", &self.current_position)
            .field("current_target", &self.current_target)
            .field("feedback", &self.feedback)
            .finish()
    }
}
