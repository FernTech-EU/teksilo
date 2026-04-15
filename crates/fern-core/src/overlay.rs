//! Overlay system for tooltips, dropdown menus, context menus, and popovers.
//!
//! Overlays render outside the normal layout hierarchy. They float above the
//! main content, positioned relative to an anchor widget or the pointer.
//! The `OverlayManager` coordinates creation, positioning, stacking, dismissal,
//! event routing, and accessibility.

use std::time::Duration;

use fern_canvas::{Point, Rect, Size, Vec2};

use crate::widget_id::WidgetId;

/// Unique identifier for an active overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayId(u64);

impl OverlayId {
    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }
}

/// How an overlay is positioned relative to its anchor.
#[derive(Debug, Clone)]
pub enum OverlayPlacement {
    /// Below the anchor, leading-edge aligned (dropdown).
    Below,
    /// Above the anchor (fallback when no space below).
    Above,
    /// To the trailing side of the anchor (submenu).
    TrailingEdge,
    /// At the pointer position (context menu).
    AtPointer(Point),
    /// Near the anchor with a preferred alignment and offset (tooltip).
    NearAnchor { offset: Vec2 },
    /// Centered within the viewport (dialog).
    Centered,
    /// Bottom-centered within the viewport (snackbar/toast).
    BottomCenter,
    /// Below the anchor if space allows, otherwise above (combo box dropdown).
    /// The viewport height is supplied by `position_overlays()` at layout time.
    BelowPreferred,
}

/// When an overlay is dismissed.
#[derive(Debug, Clone)]
pub enum DismissBehavior {
    /// Dismiss when the user clicks outside the overlay.
    ClickOutside,
    /// Dismiss when the user presses Escape.
    EscapeKey,
    /// Dismiss on either Escape or an outside click.
    EscapeOrClickOutside,
    /// Dismiss when the pointer leaves both anchor and overlay.
    PointerLeave { delay: Duration },
    /// Dismiss only via explicit API call.
    Manual,
}

/// Where the overlay renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayLayer {
    /// Rendered within the application window's wgpu surface.
    InTree,
    /// Rendered in a separate native OS window.
    NativePopup,
    /// Framework decides based on content size.
    Auto,
}

/// A request to show an overlay.
pub struct OverlayRequest {
    /// The root widget of the overlay content.
    pub content_id: WidgetId,
    /// The widget this overlay is anchored to.
    pub anchor: WidgetId,
    /// Positioning relative to the anchor.
    pub placement: OverlayPlacement,
    /// How the overlay is dismissed.
    pub dismiss: DismissBehavior,
    /// Rendering layer.
    pub layer: OverlayLayer,
    /// Parent overlay (for submenu cascading).
    pub parent_overlay: Option<OverlayId>,
}

/// An active overlay in the stack.
#[derive(Debug)]
pub(crate) struct ActiveOverlay {
    pub id: OverlayId,
    pub content_id: WidgetId,
    pub anchor: WidgetId,
    pub placement: OverlayPlacement,
    pub dismiss: DismissBehavior,
    #[allow(dead_code)] // Part of V2 overlay API, used for z-ordering
    pub layer: OverlayLayer,
    pub parent_overlay: Option<OverlayId>,
    /// Computed bounds after positioning.
    pub bounds: Rect,
    /// Widget that had focus before this overlay was shown.
    /// Used to restore focus when the overlay is dismissed.
    pub focus_restore: Option<WidgetId>,
    /// When pointer-leave dismissal started (real time).
    pub pointer_leave_started_real: Option<std::time::Instant>,
    /// When pointer-leave dismissal started (simulated time).
    pub pointer_leave_started_sim: Option<std::time::Instant>,
    /// Dismiss automatically after this duration, if set.
    pub auto_dismiss_after: Option<Duration>,
    /// When the overlay was shown (real time).
    pub shown_at_real: std::time::Instant,
    /// When the overlay was shown (simulated time).
    pub shown_at_sim: std::time::Instant,
}

/// Manages the overlay stack — creation, positioning, dismissal, cascading.
pub struct OverlayManager {
    pub(crate) stack: Vec<ActiveOverlay>,
    next_id: u64,
}

impl OverlayManager {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            next_id: 1,
        }
    }

    /// Show a new overlay. Returns the OverlayId.
    pub fn show(&mut self, request: OverlayRequest) -> OverlayId {
        self.show_with_auto_dismiss(request, None)
    }

    /// Show a new overlay that dismisses automatically after `duration`.
    pub fn show_for(&mut self, request: OverlayRequest, duration: Duration) -> OverlayId {
        self.show_with_auto_dismiss(request, Some(duration))
    }

    fn show_with_auto_dismiss(
        &mut self,
        request: OverlayRequest,
        auto_dismiss_after: Option<Duration>,
    ) -> OverlayId {
        let id = OverlayId::new(self.next_id);
        self.next_id += 1;
        let now = std::time::Instant::now();

        let overlay = ActiveOverlay {
            id,
            content_id: request.content_id,
            anchor: request.anchor,
            placement: request.placement,
            dismiss: request.dismiss,
            layer: request.layer,
            parent_overlay: request.parent_overlay,
            bounds: Rect::ZERO,
            focus_restore: None,
            pointer_leave_started_real: None,
            pointer_leave_started_sim: None,
            auto_dismiss_after,
            shown_at_real: now,
            shown_at_sim: now,
        };
        self.stack.push(overlay);
        id
    }

    pub fn next_auto_dismiss_deadline(&self) -> Option<std::time::Instant> {
        self.stack
            .iter()
            .filter_map(|overlay| {
                overlay
                    .auto_dismiss_after
                    .map(|delay| overlay.shown_at_real + delay)
            })
            .min()
    }

    pub(crate) fn set_shown_at_sim(&mut self, id: OverlayId, shown_at_sim: std::time::Instant) {
        if let Some(overlay) = self.stack.iter_mut().find(|overlay| overlay.id == id) {
            overlay.shown_at_sim = shown_at_sim;
        }
    }

    pub(crate) fn is_descendant_of(&self, child: OverlayId, ancestor: OverlayId) -> bool {
        let mut current = self
            .stack
            .iter()
            .find(|overlay| overlay.id == child)
            .and_then(|overlay| overlay.parent_overlay);

        while let Some(parent) = current {
            if parent == ancestor {
                return true;
            }
            current = self
                .stack
                .iter()
                .find(|overlay| overlay.id == parent)
                .and_then(|overlay| overlay.parent_overlay);
        }

        false
    }

    pub(crate) fn overlay(&self, id: OverlayId) -> Option<&ActiveOverlay> {
        self.stack.iter().find(|overlay| overlay.id == id)
    }

    pub(crate) fn topmost_centered(&self) -> Option<&ActiveOverlay> {
        self.stack
            .iter()
            .rev()
            .find(|overlay| matches!(overlay.placement, OverlayPlacement::Centered))
    }

    /// Dismiss an overlay and all its children (cascade), returning the
    /// dismissed content widget IDs and the overlay's focus_restore target.
    pub fn dismiss_with_focus_restore(
        &mut self,
        id: OverlayId,
    ) -> (Vec<WidgetId>, Option<WidgetId>) {
        let focus_restore = self
            .stack
            .iter()
            .find(|overlay| overlay.id == id)
            .and_then(|overlay| overlay.focus_restore);
        let dismissed = self.dismiss(id);
        (dismissed, focus_restore)
    }

    /// Dismiss all descendant overlays of `parent`, optionally preserving the
    /// subtree rooted at `preserve`.
    pub fn dismiss_descendants_of(
        &mut self,
        parent: OverlayId,
        preserve: Option<OverlayId>,
    ) -> (Vec<WidgetId>, Option<WidgetId>) {
        let mut to_dismiss = Vec::new();

        for overlay in &self.stack {
            if !self.is_descendant_of(overlay.id, parent) {
                continue;
            }
            if preserve
                .is_some_and(|keep| overlay.id == keep || self.is_descendant_of(overlay.id, keep))
            {
                continue;
            }
            to_dismiss.push(overlay.id);
        }

        if to_dismiss.is_empty() {
            return (Vec::new(), None);
        }

        let focus_restore = self
            .stack
            .iter()
            .rev()
            .find(|overlay| to_dismiss.contains(&overlay.id))
            .and_then(|overlay| overlay.focus_restore);

        let dismissed_content: Vec<WidgetId> = self
            .stack
            .iter()
            .filter(|overlay| to_dismiss.contains(&overlay.id))
            .map(|overlay| overlay.content_id)
            .collect();
        self.stack
            .retain(|overlay| !to_dismiss.contains(&overlay.id));

        (dismissed_content, focus_restore)
    }

    /// Update the placement of an existing overlay.
    pub fn update_placement(&mut self, id: OverlayId, placement: OverlayPlacement) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id) {
            overlay.placement = placement;
        }
    }

    /// Dismiss an overlay and all its children (cascade).
    /// Returns the content widget IDs of all dismissed overlays.
    pub fn dismiss(&mut self, id: OverlayId) -> Vec<WidgetId> {
        // Collect IDs to dismiss: the target + all descendants
        let mut to_dismiss = vec![id];
        let mut i = 0;
        while i < to_dismiss.len() {
            let parent = to_dismiss[i];
            for overlay in &self.stack {
                if overlay.parent_overlay == Some(parent) && !to_dismiss.contains(&overlay.id) {
                    to_dismiss.push(overlay.id);
                }
            }
            i += 1;
        }
        let dismissed_content: Vec<WidgetId> = self
            .stack
            .iter()
            .filter(|o| to_dismiss.contains(&o.id))
            .map(|o| o.content_id)
            .collect();
        self.stack.retain(|o| !to_dismiss.contains(&o.id));
        dismissed_content
    }

    /// Dismiss the topmost overlay unconditionally (e.g., ArrowLeft for submenu cascading).
    /// Returns the overlay ID, content widget IDs, and focus_restore target.
    pub fn dismiss_top(&mut self) -> Option<(OverlayId, Vec<WidgetId>, Option<WidgetId>)> {
        if let Some(overlay) = self.stack.last() {
            let id = overlay.id;
            let focus_restore = overlay.focus_restore;
            let content_ids = self.dismiss(id);
            Some((id, content_ids, focus_restore))
        } else {
            None
        }
    }

    /// Try to dismiss the topmost overlay on Escape, respecting `DismissBehavior`.
    /// Only dismisses if the overlay's behavior includes Escape dismissal.
    /// Returns the overlay ID, content widget IDs, and focus_restore target, or `None`
    /// if the topmost overlay does not allow Escape dismissal.
    pub fn try_dismiss_top_on_escape(
        &mut self,
    ) -> Option<(OverlayId, Vec<WidgetId>, Option<WidgetId>)> {
        let dominated_by_escape = self.stack.last().is_some_and(|o| {
            matches!(
                o.dismiss,
                DismissBehavior::EscapeKey | DismissBehavior::EscapeOrClickOutside
            )
        });
        if dominated_by_escape {
            self.dismiss_top()
        } else {
            None
        }
    }

    /// Set the focus_restore target for the topmost overlay.
    pub fn set_top_focus_restore(&mut self, focus_restore: WidgetId) {
        if let Some(overlay) = self.stack.last_mut() {
            overlay.focus_restore = Some(focus_restore);
        }
    }

    /// Dismiss all overlays.
    /// Returns the content widget IDs of all dismissed overlays.
    pub fn dismiss_all(&mut self) -> Vec<WidgetId> {
        let content_ids: Vec<WidgetId> = self.stack.iter().map(|o| o.content_id).collect();
        self.stack.clear();
        content_ids
    }

    /// Whether there are any active overlays.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Number of active overlays.
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Get all active overlay content widget IDs (for rendering).
    pub fn active_content_ids(&self) -> Vec<WidgetId> {
        self.stack.iter().map(|o| o.content_id).collect()
    }

    /// Get all active overlay IDs (for testing/querying).
    pub fn active_ids(&self) -> Vec<OverlayId> {
        self.stack.iter().map(|o| o.id).collect()
    }

    /// Get the anchor widget for an overlay.
    pub fn anchor_for(&self, id: OverlayId) -> Option<WidgetId> {
        self.stack.iter().find(|o| o.id == id).map(|o| o.anchor)
    }

    /// Get the topmost overlay.
    #[allow(dead_code)] // V2 API: used for overlay z-ordering and focus management
    pub(crate) fn topmost(&self) -> Option<&ActiveOverlay> {
        self.stack.last()
    }

    /// Check if a point hits any overlay (topmost first).
    /// Returns the overlay ID if hit, None if the point is outside all overlays.
    pub fn hit_test(&self, point: Point) -> Option<OverlayId> {
        for overlay in self.stack.iter().rev() {
            if overlay.bounds.contains(point) {
                return Some(overlay.id);
            }
        }
        None
    }

    /// Handle a click-outside event: if the click is outside all overlays
    /// with ClickOutside dismiss behavior, dismiss them.
    /// Returns the content widget IDs of dismissed overlays (empty if none).
    pub fn handle_click_outside(&mut self, point: Point) -> Vec<WidgetId> {
        if self.stack.is_empty() {
            return Vec::new();
        }

        // Check if the point is inside any overlay
        if self.hit_test(point).is_some() {
            return Vec::new();
        }

        // Dismiss all overlays that should close on an outside click.
        let to_dismiss: Vec<OverlayId> = self
            .stack
            .iter()
            .filter(|o| {
                matches!(
                    o.dismiss,
                    DismissBehavior::ClickOutside
                        | DismissBehavior::EscapeOrClickOutside
                        | DismissBehavior::PointerLeave { .. }
                )
            })
            .map(|o| o.id)
            .collect();

        if to_dismiss.is_empty() {
            return Vec::new();
        }

        let mut all_dismissed = Vec::new();
        for id in to_dismiss {
            all_dismissed.extend(self.dismiss(id));
        }
        all_dismissed
    }

    /// Compute overlay positions based on anchor bounds.
    /// Called after layout to position overlays correctly.
    /// `viewport` is (width, height) used for clamping overlays to the visible area.
    pub fn position_overlays(
        &mut self,
        anchor_bounds_fn: impl Fn(WidgetId) -> Rect,
        viewport: (f32, f32),
    ) {
        let (vw, vh) = viewport;
        for overlay in &mut self.stack {
            let anchor = anchor_bounds_fn(overlay.anchor);
            let content_size = overlay.bounds.size(); // Will be set from content layout

            overlay.bounds = match &overlay.placement {
                OverlayPlacement::Below => Rect::new(
                    anchor.x,
                    anchor.y + anchor.height + 4.0,
                    content_size.width.max(anchor.width),
                    content_size.height,
                ),
                OverlayPlacement::Above => Rect::new(
                    anchor.x,
                    anchor.y - content_size.height - 4.0,
                    content_size.width.max(anchor.width),
                    content_size.height,
                ),
                OverlayPlacement::TrailingEdge => {
                    let x_right = anchor.x + anchor.width + 2.0;
                    let fits_right = x_right + content_size.width <= vw;
                    let x = if fits_right {
                        x_right
                    } else {
                        // Fallback: open to the leading edge
                        anchor.x - content_size.width - 2.0
                    };
                    let y = anchor.y.min(vh - content_size.height).max(0.0);
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::AtPointer(point) => {
                    // Clamp to viewport so menus don't overflow off-screen
                    let x = point.x.min(vw - content_size.width).max(0.0);
                    let y = if point.y + content_size.height <= vh {
                        point.y
                    } else {
                        // Not enough space below pointer — open above
                        (point.y - content_size.height).max(0.0)
                    };
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::NearAnchor { offset } => Rect::new(
                    anchor.x + offset.x,
                    anchor.y + anchor.height + offset.y + 4.0,
                    content_size.width,
                    content_size.height,
                ),
                OverlayPlacement::Centered => Rect::new(
                    ((vw - content_size.width) / 2.0).max(0.0),
                    ((vh - content_size.height) / 2.0).max(0.0),
                    content_size.width.min(vw),
                    content_size.height.min(vh),
                ),
                OverlayPlacement::BottomCenter => Rect::new(
                    ((vw - content_size.width) / 2.0).max(0.0),
                    (vh - content_size.height - 24.0).max(0.0),
                    content_size.width.min(vw),
                    content_size.height.min(vh),
                ),
                OverlayPlacement::BelowPreferred => {
                    let below_y = anchor.y + anchor.height + 4.0;
                    let fits_below = below_y + content_size.height <= vh;
                    let y = if fits_below {
                        below_y
                    } else {
                        anchor.y - content_size.height - 4.0
                    };
                    // Clamp horizontally
                    let x = anchor.x.min(vw - content_size.width).max(0.0);
                    Rect::new(
                        x,
                        y,
                        content_size.width.max(anchor.width),
                        content_size.height,
                    )
                }
            };
        }
    }

    /// Set the content bounds for an overlay (after its content has been laid out).
    pub fn set_content_bounds(&mut self, id: OverlayId, size: Size) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id) {
            overlay.bounds = Rect::new(overlay.bounds.x, overlay.bounds.y, size.width, size.height);
        }
    }

    /// Get overlay by content widget ID (for routing events to the correct overlay).
    pub fn find_by_content(&self, content_id: WidgetId) -> Option<OverlayId> {
        self.stack
            .iter()
            .find(|o| o.content_id == content_id)
            .map(|o| o.id)
    }

    /// Change the dismiss behavior of an active overlay in place.
    ///
    /// Used by rich tooltips that promote from "ephemeral hover" to
    /// "sticky panel" after a dwell timer: at t=2s the tooltip calls
    /// this to swap `PointerLeave` for `EscapeOrClickOutside`, so the
    /// overlay stops vanishing the moment the pointer leaves the
    /// anchor. Also cancels any in-flight pointer-leave countdown.
    pub fn set_dismiss(&mut self, id: OverlayId, behavior: DismissBehavior) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id) {
            overlay.dismiss = behavior;
            overlay.pointer_leave_started_real = None;
            overlay.pointer_leave_started_sim = None;
        }
    }
}

impl Default for OverlayManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OverlayManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayManager")
            .field("active_count", &self.stack.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::KeyData;

    fn fake_id(n: u64) -> WidgetId {
        KeyData::from_ffi(n).into()
    }

    #[test]
    fn show_and_dismiss() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });
        assert_eq!(mgr.len(), 1);

        mgr.dismiss(id);
        assert!(mgr.is_empty());
    }

    #[test]
    fn cascade_dismissal() {
        let mut mgr = OverlayManager::new();
        let parent = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });
        let _child = mgr.show(OverlayRequest {
            content_id: fake_id(11),
            anchor: fake_id(10),
            placement: OverlayPlacement::TrailingEdge,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: Some(parent),
        });
        assert_eq!(mgr.len(), 2);

        // Dismissing parent cascades to child
        mgr.dismiss(parent);
        assert!(mgr.is_empty());
    }

    #[test]
    fn dismiss_top() {
        let mut mgr = OverlayManager::new();
        let _a = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });
        let b = mgr.show(OverlayRequest {
            content_id: fake_id(11),
            anchor: fake_id(2),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        let dismissed = mgr.dismiss_top();
        assert_eq!(dismissed.map(|(id, _, _)| id), Some(b));
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn click_outside_dismisses() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        // Set overlay bounds
        let id = mgr.active_ids()[0];
        mgr.set_content_bounds(id, Size::new(100.0, 50.0));

        // Click inside — no dismiss
        assert!(mgr.handle_click_outside(Point::new(50.0, 25.0)).is_empty());
        assert_eq!(mgr.len(), 1);

        // Click outside — dismissed
        assert!(
            !mgr.handle_click_outside(Point::new(500.0, 500.0))
                .is_empty()
        );
        assert!(mgr.is_empty());
    }

    #[test]
    fn manual_dismiss_ignores_click_outside() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        assert!(
            mgr.handle_click_outside(Point::new(500.0, 500.0))
                .is_empty()
        );
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn escape_dismisses_escape_or_click_outside() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        let dismissed = mgr.try_dismiss_top_on_escape();
        assert_eq!(dismissed.map(|(oid, _, _)| oid), Some(id));
        assert!(mgr.is_empty());
    }

    #[test]
    fn escape_dismisses_escape_key_only() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeKey,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        // Escape should dismiss
        let dismissed = mgr.try_dismiss_top_on_escape();
        assert_eq!(dismissed.map(|(oid, _, _)| oid), Some(id));
        assert!(mgr.is_empty());
    }

    #[test]
    fn escape_does_not_dismiss_click_outside_only() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        assert!(mgr.try_dismiss_top_on_escape().is_none());
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn escape_does_not_dismiss_manual() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        assert!(mgr.try_dismiss_top_on_escape().is_none());
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn click_outside_dismisses_escape_or_click_outside() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        let id = mgr.active_ids()[0];
        mgr.set_content_bounds(id, Size::new(100.0, 50.0));

        assert!(
            !mgr.handle_click_outside(Point::new(500.0, 500.0))
                .is_empty()
        );
        assert!(mgr.is_empty());
    }

    #[test]
    fn click_outside_does_not_dismiss_escape_key_only() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeKey,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        assert!(
            mgr.handle_click_outside(Point::new(500.0, 500.0))
                .is_empty()
        );
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn active_content_ids() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });
        mgr.show(OverlayRequest {
            content_id: fake_id(20),
            anchor: fake_id(2),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        let ids = mgr.active_content_ids();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], fake_id(10));
        assert_eq!(ids[1], fake_id(20));
    }

    #[test]
    fn hit_test_topmost_first() {
        let mut mgr = OverlayManager::new();
        let a = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });
        let b = mgr.show(OverlayRequest {
            content_id: fake_id(11),
            anchor: fake_id(2),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        // Both overlays at origin with same bounds
        mgr.set_content_bounds(a, Size::new(100.0, 50.0));
        mgr.set_content_bounds(b, Size::new(100.0, 50.0));

        // Hit test should find topmost (b)
        assert_eq!(mgr.hit_test(Point::new(50.0, 25.0)), Some(b));
    }

    #[test]
    fn centered_placement_uses_viewport_center() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        mgr.set_content_bounds(id, Size::new(240.0, 120.0));
        mgr.position_overlays(|_| Rect::new(0.0, 0.0, 10.0, 10.0), (800.0, 600.0));

        let bounds = mgr
            .stack
            .iter()
            .find(|overlay| overlay.id == id)
            .unwrap()
            .bounds;
        assert!((bounds.x - 280.0).abs() < 0.01);
        assert!((bounds.y - 240.0).abs() < 0.01);
    }

    #[test]
    fn bottom_center_placement_uses_viewport_bottom_margin() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::BottomCenter,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
        });

        mgr.set_content_bounds(id, Size::new(240.0, 64.0));
        mgr.position_overlays(|_| Rect::new(0.0, 0.0, 10.0, 10.0), (800.0, 600.0));

        let bounds = mgr
            .stack
            .iter()
            .find(|overlay| overlay.id == id)
            .unwrap()
            .bounds;
        assert!((bounds.x - 280.0).abs() < 0.01);
        assert!((bounds.y - 512.0).abs() < 0.01);
    }
}
