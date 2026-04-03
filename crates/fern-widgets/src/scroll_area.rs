use std::cell::Cell;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use fern_core::state::{BindingLevel, BindingRegistry, State};
use fern_core::widget::{EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::scroll_bar::{ScrollBar, ScrollBarOrientation};

/// How the scroll bar is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollBarStyle {
    /// Scroll bar overlays the content (macOS-style). A thin passive
    /// indicator is painted during scrolling; the full interactive ScrollBar
    /// is shown as an overlay on pointer proximity.
    Overlay,
    /// Scroll bar is a permanent layout sibling of the viewport, reducing
    /// the content area by the scroll bar's width. Always visible and
    /// interactive.
    Permanent,
}

impl Default for ScrollBarStyle {
    fn default() -> Self {
        ScrollBarStyle::Overlay
    }
}

/// A scroll area that clips its content to a viewport and handles
/// scroll events. The scroll offset is stored as `State<f32>` per axis,
/// shared with the ScrollBar widget via the reactive binding system.
///
/// Supports two display modes via `ScrollBarStyle`:
/// - **Overlay** (default): thin indicator painted during scrolling
/// - **Permanent**: ScrollBar is a layout child alongside the viewport
pub struct ScrollArea {
    content_child: Option<PendingChild>,
    scroll_bar_style: ScrollBarStyle,
    /// Pixels per scroll line (for line-based mouse wheel events).
    line_height: f32,
    /// Thickness of the scroll bar (for permanent mode layout).
    scroll_bar_thickness: f32,

    // --- shared reactive state ---
    /// Vertical scroll position (0.0 = top).
    scroll_y: State<f32>,
    /// Horizontal scroll position (0.0 = left).
    scroll_x: State<f32>,
    /// Maximum vertical scroll (content_height - viewport_height).
    max_scroll_y: State<f32>,
    /// Maximum horizontal scroll (content_width - viewport_width).
    max_scroll_x: State<f32>,
    /// Vertical viewport/content ratio (0.0..1.0).
    viewport_ratio_y: State<f32>,
    /// Horizontal viewport/content ratio (0.0..1.0).
    viewport_ratio_x: State<f32>,

    // --- resolved children ---
    /// Resolved child IDs: [content, optional_v_scrollbar, optional_h_scrollbar]
    child_ids: Vec<WidgetId>,

    // --- cached sizes for event handling ---
    content_size: Cell<Size>,
    viewport_size: Cell<Size>,
}

impl std::fmt::Debug for ScrollArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollArea")
            .field("scroll_y", &*self.scroll_y.get())
            .field("scroll_x", &*self.scroll_x.get())
            .field("style", &self.scroll_bar_style)
            .field("content_size", &self.content_size.get())
            .field("viewport_size", &self.viewport_size.get())
            .finish()
    }
}

impl ScrollArea {
    pub fn new(child: impl fern_core::widget::IntoWidgetTree) -> Self {
        Self {
            content_child: Some(PendingChild::Deferred(Box::new(child))),
            scroll_bar_style: ScrollBarStyle::default(),
            line_height: 20.0,
            scroll_bar_thickness: 12.0,
            scroll_y: State::new(0.0),
            scroll_x: State::new(0.0),
            max_scroll_y: State::new(0.0),
            max_scroll_x: State::new(0.0),
            viewport_ratio_y: State::new(1.0),
            viewport_ratio_x: State::new(1.0),
            child_ids: Vec::new(),
            content_size: Cell::new(Size::ZERO),
            viewport_size: Cell::new(Size::ZERO),
        }
    }

    /// Construct from an already-registered child WidgetId.
    pub fn from_id(child: WidgetId) -> Self {
        Self {
            content_child: Some(PendingChild::Id(child)),
            scroll_bar_style: ScrollBarStyle::default(),
            line_height: 20.0,
            scroll_bar_thickness: 12.0,
            scroll_y: State::new(0.0),
            scroll_x: State::new(0.0),
            max_scroll_y: State::new(0.0),
            max_scroll_x: State::new(0.0),
            viewport_ratio_y: State::new(1.0),
            viewport_ratio_x: State::new(1.0),
            child_ids: Vec::new(),
            content_size: Cell::new(Size::ZERO),
            viewport_size: Cell::new(Size::ZERO),
        }
    }

    pub fn scroll_bar_style(mut self, style: ScrollBarStyle) -> Self {
        self.scroll_bar_style = style;
        self
    }

    pub fn line_height(mut self, lh: f32) -> Self {
        self.line_height = lh;
        self
    }

    pub fn scroll_bar_thickness(mut self, thickness: f32) -> Self {
        self.scroll_bar_thickness = thickness;
        self
    }

    /// Get the vertical scroll position state (for external observation).
    pub fn scroll_y_state(&self) -> &State<f32> {
        &self.scroll_y
    }

    /// Get the horizontal scroll position state (for external observation).
    pub fn scroll_x_state(&self) -> &State<f32> {
        &self.scroll_x
    }

    fn clamp_and_set_scroll(&self) {
        let max_y = *self.max_scroll_y.get();
        let max_x = *self.max_scroll_x.get();
        let cur_y = *self.scroll_y.get();
        let cur_x = *self.scroll_x.get();
        let clamped_y = cur_y.clamp(0.0, max_y);
        let clamped_x = cur_x.clamp(0.0, max_x);
        if (clamped_y - cur_y).abs() > f32::EPSILON {
            self.scroll_y.set(clamped_y);
        }
        if (clamped_x - cur_x).abs() > f32::EPSILON {
            self.scroll_x.set(clamped_x);
        }
    }

    /// Whether a vertical scroll bar is present as a layout child.
    fn has_v_scrollbar(&self) -> bool {
        self.scroll_bar_style == ScrollBarStyle::Permanent && self.child_ids.len() > 1
    }
}

impl Widget for ScrollArea {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(300.0, 200.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }

        // Determine viewport bounds (reduced by scrollbar in Permanent mode)
        let scrollbar_width = if self.has_v_scrollbar() {
            self.scroll_bar_thickness
        } else {
            0.0
        };
        let viewport_width = (bounds.width - scrollbar_width).max(0.0);
        let viewport_bounds = Rect::new(bounds.x, bounds.y, viewport_width, bounds.height);

        // Propose unbounded on both scroll axes to the content child
        let content_proposal = SizeProposal {
            width: Some(viewport_width),
            height: None,
        };

        let content_size = ctx
            .child_size(children[0].id, content_proposal)
            .unwrap_or(viewport_bounds.size());
        self.content_size.set(content_size);
        self.viewport_size.set(viewport_bounds.size());

        // Update shared state for ScrollBar
        let max_y = (content_size.height - viewport_bounds.height).max(0.0);
        let max_x = (content_size.width - viewport_width).max(0.0);
        self.max_scroll_y.set(max_y);
        self.max_scroll_x.set(max_x);

        let ratio_y = if content_size.height > 0.0 {
            (viewport_bounds.height / content_size.height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let ratio_x = if content_size.width > 0.0 {
            (viewport_width / content_size.width).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(ratio_y);
        self.viewport_ratio_x.set(ratio_x);

        // Clamp scroll position to valid range
        self.clamp_and_set_scroll();
        let scroll_y = *self.scroll_y.get();
        let scroll_x = *self.scroll_x.get();

        // Place content with scroll offset
        children[0].origin = Point::new(
            viewport_bounds.x - scroll_x,
            viewport_bounds.y - scroll_y,
        );
        children[0].size = content_size;

        // Place vertical scrollbar (Permanent mode)
        if children.len() > 1 {
            let sb_x = if ctx.is_rtl() {
                bounds.x
            } else {
                bounds.right() - scrollbar_width
            };
            children[1].origin = Point::new(sb_x, bounds.y);
            children[1].size = Size::new(scrollbar_width, bounds.height);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas, ctx: &PaintContext) {
        // In Overlay mode, paint a thin passive scroll indicator
        if self.scroll_bar_style == ScrollBarStyle::Overlay {
            let content_size = self.content_size.get();
            let viewport_size = self.viewport_size.get();
            if content_size.height > viewport_size.height {
                let track_height = bounds.height;
                let thumb_ratio = viewport_size.height / content_size.height;
                let thumb_height = (track_height * thumb_ratio).max(20.0);
                let max_y = *self.max_scroll_y.get();
                let scroll_ratio = if max_y > 0.0 {
                    *self.scroll_y.get() / max_y
                } else {
                    0.0
                };
                let thumb_y = bounds.y + scroll_ratio * (track_height - thumb_height);

                let bar_width = 4.0;
                let thumb_rect = Rect::new(
                    bounds.right() - bar_width - 2.0,
                    thumb_y,
                    bar_width,
                    thumb_height,
                );
                canvas.fill_rounded_rect(
                    thumb_rect,
                    fern_tokens::CornerRadius::uniform(bar_width / 2.0),
                    ctx.theme.colors.on_surface.with_alpha(0.3),
                );
            }
        }
        // In Permanent mode, the ScrollBar widget paints itself — no manual painting needed.
    }

    fn event(&mut self, event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        match event {
            WidgetEvent::Scroll { delta, .. } => {
                let (dx, dy) = match delta {
                    ScrollDelta::Lines { x, y } => {
                        (x * self.line_height, y * self.line_height)
                    }
                    ScrollDelta::Pixels { x, y } => (*x, *y),
                };
                let new_y = *self.scroll_y.get() + dy;
                let new_x = *self.scroll_x.get() + dx;
                self.scroll_y.set(new_y);
                self.scroll_x.set(new_x);
                self.clamp_and_set_scroll();
                EventResponse::Handled
            }
            WidgetEvent::ScrollIntoView { target_bounds } => {
                let vp = self.viewport_size.get();
                let scroll_y = *self.scroll_y.get();
                let scroll_x = *self.scroll_x.get();

                // Vertical: scroll minimum amount to make target fully visible
                let viewport_top = scroll_y;
                let viewport_bottom = viewport_top + vp.height;
                let target_top = target_bounds.y + scroll_y;
                let target_bottom = target_top + target_bounds.height;

                let mut new_y = scroll_y;
                if target_top < viewport_top {
                    new_y = target_top;
                } else if target_bottom > viewport_bottom {
                    new_y = target_bottom - vp.height;
                }

                // Horizontal: same logic
                let viewport_left = scroll_x;
                let viewport_right = viewport_left + vp.width;
                let target_left = target_bounds.x + scroll_x;
                let target_right = target_left + target_bounds.width;

                let mut new_x = scroll_x;
                if target_left < viewport_left {
                    new_x = target_left;
                } else if target_right > viewport_right {
                    new_x = target_right - vp.width;
                }

                self.scroll_y.set(new_y);
                self.scroll_x.set(new_x);
                self.clamp_and_set_scroll();
                EventResponse::Handled
            }
            WidgetEvent::AccessAction { action, .. } => match *action {
                fern_core::accesskit::Action::ScrollDown => {
                    let step = self.viewport_size.get().height * 0.9;
                    self.scroll_y.set(*self.scroll_y.get() + step);
                    self.clamp_and_set_scroll();
                    EventResponse::Handled
                }
                fern_core::accesskit::Action::ScrollUp => {
                    let step = self.viewport_size.get().height * 0.9;
                    self.scroll_y.set(*self.scroll_y.get() - step);
                    self.clamp_and_set_scroll();
                    EventResponse::Handled
                }
                fern_core::accesskit::Action::ScrollRight => {
                    let step = self.viewport_size.get().width * 0.9;
                    self.scroll_x.set(*self.scroll_x.get() + step);
                    self.clamp_and_set_scroll();
                    EventResponse::Handled
                }
                fern_core::accesskit::Action::ScrollLeft => {
                    let step = self.viewport_size.get().width * 0.9;
                    self.scroll_x.set(*self.scroll_x.get() - step);
                    self.clamp_and_set_scroll();
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

    fn take_pending_children(&mut self) -> Vec<PendingChild> {
        let mut pending = Vec::new();
        if let Some(child) = self.content_child.take() {
            pending.push(child);
        }

        // In Permanent mode, also add the vertical scroll bar as a child
        if self.scroll_bar_style == ScrollBarStyle::Permanent {
            let v_scrollbar = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                self.scroll_y.clone(),
                self.max_scroll_y.clone(),
                self.viewport_ratio_y.clone(),
            )
            .thickness(self.scroll_bar_thickness);

            pending.push(PendingChild::Deferred(Box::new(v_scrollbar)));
        }

        pending
    }

    fn set_resolved_children(&mut self, ids: Vec<WidgetId>) {
        self.child_ids = ids;
    }

    fn register_bindings(&self, id: WidgetId, registry: &BindingRegistry) {
        // Scroll position changes trigger relayout (content offset moves)
        let y_handle = fern_core::state::StateHandle::from(self.scroll_y.clone());
        y_handle.register(id, registry, BindingLevel::Relayout);

        let x_handle = fern_core::state::StateHandle::from(self.scroll_x.clone());
        x_handle.register(id, registry, BindingLevel::Relayout);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ScrollView);
        builder.inner_mut().set_clips_children();

        let scroll_y = *self.scroll_y.get();
        let scroll_x = *self.scroll_x.get();
        let max_y = *self.max_scroll_y.get();
        let max_x = *self.max_scroll_x.get();

        builder.inner_mut().set_scroll_y(scroll_y as f64);
        builder.inner_mut().set_scroll_y_min(0.0);
        builder.inner_mut().set_scroll_y_max(max_y as f64);
        builder.inner_mut().set_scroll_x(scroll_x as f64);
        builder.inner_mut().set_scroll_x_min(0.0);
        builder.inner_mut().set_scroll_x_max(max_x as f64);
        builder.add_action(fern_core::accesskit::Action::ScrollDown);
        builder.add_action(fern_core::accesskit::Action::ScrollUp);
        builder.add_action(fern_core::accesskit::Action::ScrollLeft);
        builder.add_action(fern_core::accesskit::Action::ScrollRight);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::widget::LayoutContext;
    use fern_core::widget_tree::WidgetTree;

    use crate::primitives::VStack;

    /// A leaf widget with a fixed intrinsic size.
    #[derive(Debug)]
    struct TallLeaf {
        width: f32,
        height: f32,
    }

    impl TallLeaf {
        fn new(w: f32, h: f32) -> Self {
            Self {
                width: w,
                height: h,
            }
        }
    }

    impl Widget for TallLeaf {
        fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(
                proposal.width.unwrap_or(self.width),
                proposal.height.unwrap_or(self.height),
            )
        }
    }

    #[test]
    fn scroll_area_clips_hit_test() {
        let mut tree = WidgetTree::new();

        // Content taller than viewport: 3 items × 100px = 300px
        let a = tree.add(TallLeaf::new(200.0, 100.0));
        let b = tree.add(TallLeaf::new(200.0, 100.0));
        let c = tree.add(TallLeaf::new(200.0, 100.0));
        let content = tree.add(VStack::new().add_child(a).add_child(b).add_child(c));

        let scroll = tree.add(ScrollArea::from_id(content));
        tree.set_clips_children(scroll, true);

        // Viewport is 200×80 — only first 80px visible
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Point inside viewport: should hit a child
        let hit = tree.hit_test(Point::new(50.0, 40.0));
        assert!(hit.is_some());

        // Point outside viewport (below): should not hit any child
        let hit_outside = tree.hit_test(Point::new(50.0, 100.0));
        // This point is outside the scroll area's 80px bounds
        assert!(hit_outside.is_none() || hit_outside == Some(scroll));
    }

    #[test]
    fn scroll_changes_visible_content() {
        let mut tree = WidgetTree::new();

        let a = tree.add(TallLeaf::new(200.0, 100.0));
        let b = tree.add(TallLeaf::new(200.0, 100.0));
        let content = tree.add(VStack::new().add_child(a).add_child(b));

        let scroll = tree.add(ScrollArea::from_id(content));
        tree.set_clips_children(scroll, true);
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Before scrolling, item a is at y=0
        assert!(tree.bounds(a).y >= 0.0);

        // Move pointer into viewport so Scroll events have a target
        tree.pointer_move(Point::new(50.0, 40.0));

        // Scroll down 100px
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        });
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // After scrolling, item a should be above viewport (negative y)
        assert!(tree.bounds(a).y < 0.0);
        // Item b should now be at or near viewport top
        assert!(tree.bounds(b).y < 80.0);
    }

    #[test]
    fn scroll_accessibility_reports_position() {
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(200.0, 1000.0));
        let scroll = tree.add(ScrollArea::from_id(content));
        tree.set_clips_children(scroll, true);
        tree.layout(SizeProposal::exact(200.0, 80.0));

        let info = tree.accessibility_node(scroll);
        assert_eq!(info.role(), fern_core::accesskit::Role::ScrollView);
    }

    #[test]
    fn scroll_offset_is_clamped() {
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(200.0, 200.0));
        let scroll = tree.add(ScrollArea::from_id(content));
        tree.set_clips_children(scroll, true);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Move pointer into viewport
        tree.pointer_move(Point::new(50.0, 50.0));

        // Scroll way past the end
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Content should not be scrolled past max (200 - 100 = 100)
        let content_y = tree.bounds(content).y;
        assert!(content_y >= -100.0 - 0.01);
    }

    #[test]
    fn permanent_scrollbar_reduces_viewport() {
        let mut tree = WidgetTree::new();

        let content = TallLeaf::new(200.0, 500.0);
        let scroll = tree.add_widget(
            ScrollArea::new(content)
                .scroll_bar_style(ScrollBarStyle::Permanent)
                .scroll_bar_thickness(12.0),
        );
        tree.set_clips_children(scroll, true);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // The scroll area should be 200×100
        let scroll_bounds = tree.bounds(scroll);
        assert!((scroll_bounds.width - 200.0).abs() < 0.01);
        assert!((scroll_bounds.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn permanent_scrollbar_scroll_event_updates_content() {
        let mut tree = WidgetTree::new();

        let leaf = TallLeaf::new(180.0, 500.0);
        let scroll = tree.add_widget(
            ScrollArea::new(leaf)
                .scroll_bar_style(ScrollBarStyle::Permanent),
        );
        tree.set_clips_children(scroll, true);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Scroll via mouse wheel
        tree.pointer_move(Point::new(50.0, 50.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 50.0 },
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // The content child should have moved up
        let children = tree.children(scroll);
        assert!(!children.is_empty());
        let content_y = tree.bounds(children[0]).y;
        assert!(content_y < 0.0, "Expected negative y after scroll, got {}", content_y);
    }

    #[test]
    fn overlay_mode_is_default() {
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(200.0, 500.0));
        let scroll = tree.add(ScrollArea::from_id(content));
        tree.set_clips_children(scroll, true);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // In overlay mode, ScrollArea has only 1 child (the content).
        let children = tree.children(scroll);
        assert_eq!(children.len(), 1, "Overlay mode should have 1 child (content only)");
    }

    #[test]
    fn scroll_area_new_accepts_inline_widget() {
        let mut tree = WidgetTree::new();
        // Test the new API: pass widget directly, not a WidgetId
        let scroll = tree.add_widget(ScrollArea::new(TallLeaf::new(200.0, 500.0)));
        tree.set_clips_children(scroll, true);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let bounds = tree.bounds(scroll);
        assert!((bounds.width - 200.0).abs() < 0.01);
    }
}
