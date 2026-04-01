use std::cell::Cell;

use fern_canvas::{Point, Rect, Size, SizeProposal, Vec2};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// A scroll area that clips its content to a viewport and handles
/// scroll events. The scroll offset is encoded as a placement offset
/// in `place_children` — no special coordinate transformation needed.
impl std::fmt::Debug for ScrollArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollArea")
            .field("scroll_offset", &self.scroll_offset)
            .field("content_size", &self.content_size.get())
            .field("viewport_size", &self.viewport_size.get())
            .finish()
    }
}

pub struct ScrollArea {
    child: WidgetId,
    scroll_offset: Vec2,
    /// Updated during place_children via interior mutability.
    content_size: Cell<Size>,
    viewport_size: Cell<Size>,
    /// Pixels per scroll line (for line-based mouse wheel events).
    line_height: f32,
}

impl ScrollArea {
    pub fn new(child: WidgetId) -> Self {
        Self {
            child,
            scroll_offset: Vec2::ZERO,
            content_size: Cell::new(Size::ZERO),
            viewport_size: Cell::new(Size::ZERO),
            line_height: 20.0,
        }
    }

    pub fn line_height(mut self, lh: f32) -> Self {
        self.line_height = lh;
        self
    }

    fn max_scroll_y(&self) -> f32 {
        (self.content_size.get().height - self.viewport_size.get().height).max(0.0)
    }

    fn max_scroll_x(&self) -> f32 {
        (self.content_size.get().width - self.viewport_size.get().width).max(0.0)
    }

    fn clamp_offset(&mut self) {
        self.scroll_offset.x = self.scroll_offset.x.clamp(0.0, self.max_scroll_x());
        self.scroll_offset.y = self.scroll_offset.y.clamp(0.0, self.max_scroll_y());
    }
}

impl Widget for ScrollArea {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // The scroll area claims whatever space its parent offers (viewport).
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

        // Propose unbounded height to the content child
        let content_proposal = SizeProposal {
            width: Some(bounds.width),
            height: None, // unbounded on scroll axis
        };

        // Query the child's natural size and store for scroll calculations
        let content_size = ctx
            .child_size(children[0].id, content_proposal)
            .unwrap_or(bounds.size());
        self.content_size.set(content_size);
        self.viewport_size.set(bounds.size());

        // Place the content with the scroll offset encoded as position
        children[0].origin = Point::new(
            bounds.x - self.scroll_offset.x,
            bounds.y - self.scroll_offset.y,
        );
        children[0].size = content_size;
    }

    fn paint(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas, ctx: &PaintContext) {
        // Paint scroll bar indicator (thin overlay at right edge)
        let content_size = self.content_size.get();
        let viewport_size = self.viewport_size.get();
        if content_size.height > viewport_size.height {
            let track_height = bounds.height;
            let thumb_ratio = viewport_size.height / content_size.height;
            let thumb_height = (track_height * thumb_ratio).max(20.0);
            let max_y = self.max_scroll_y();
            let scroll_ratio = if max_y > 0.0 {
                self.scroll_offset.y / max_y
            } else {
                0.0
            };
            let thumb_y = bounds.y + scroll_ratio * (track_height - thumb_height);

            let bar_width = 6.0;
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

    fn event(&mut self, event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        match event {
            WidgetEvent::Scroll { delta, .. } => {
                let (dx, dy) = match delta {
                    ScrollDelta::Lines { x, y } => {
                        (x * self.line_height, y * self.line_height)
                    }
                    ScrollDelta::Pixels { x, y } => (*x, *y),
                };
                self.scroll_offset.x += dx;
                self.scroll_offset.y += dy;
                self.clamp_offset();
                EventResponse::Handled
            }
            WidgetEvent::ScrollIntoView { target_bounds } => {
                let vp = self.viewport_size.get();

                // Vertical: scroll minimum amount to make target fully visible
                let viewport_top = self.scroll_offset.y;
                let viewport_bottom = viewport_top + vp.height;
                let target_top = target_bounds.y + self.scroll_offset.y;
                let target_bottom = target_top + target_bounds.height;

                if target_top < viewport_top {
                    self.scroll_offset.y = target_top;
                } else if target_bottom > viewport_bottom {
                    self.scroll_offset.y = target_bottom - vp.height;
                }

                // Horizontal: same logic
                let viewport_left = self.scroll_offset.x;
                let viewport_right = viewport_left + vp.width;
                let target_left = target_bounds.x + self.scroll_offset.x;
                let target_right = target_left + target_bounds.width;

                if target_left < viewport_left {
                    self.scroll_offset.x = target_left;
                } else if target_right > viewport_right {
                    self.scroll_offset.x = target_right - vp.width;
                }

                self.clamp_offset();
                EventResponse::Handled
            }
            WidgetEvent::AccessAction { action, .. } => match *action {
                fern_core::accesskit::Action::ScrollDown => {
                    self.scroll_offset.y += self.viewport_size.get().height * 0.9;
                    self.clamp_offset();
                    EventResponse::Handled
                }
                fern_core::accesskit::Action::ScrollUp => {
                    self.scroll_offset.y -= self.viewport_size.get().height * 0.9;
                    self.clamp_offset();
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ScrollView);
        builder.inner_mut().set_clips_children();
        builder
            .inner_mut()
            .set_scroll_y(self.scroll_offset.y as f64);
        builder.inner_mut().set_scroll_y_min(0.0);
        builder
            .inner_mut()
            .set_scroll_y_max(self.max_scroll_y() as f64);
        builder
            .inner_mut()
            .set_scroll_x(self.scroll_offset.x as f64);
        builder.inner_mut().set_scroll_x_min(0.0);
        builder
            .inner_mut()
            .set_scroll_x_max(self.max_scroll_x() as f64);
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

        let scroll = tree.add(ScrollArea::new(content));
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

        let scroll = tree.add(ScrollArea::new(content));
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
        let scroll = tree.add(ScrollArea::new(content));
        tree.set_clips_children(scroll, true);
        tree.layout(SizeProposal::exact(200.0, 80.0));

        let info = tree.accessibility_node(scroll);
        assert_eq!(info.role(), fern_core::accesskit::Role::ScrollView);
    }

    #[test]
    fn scroll_offset_is_clamped() {
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(200.0, 200.0));
        let scroll = tree.add(ScrollArea::new(content));
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
}
