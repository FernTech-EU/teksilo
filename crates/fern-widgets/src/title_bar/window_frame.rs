//! A borderless-window frame: 4 invisible resize strips arranged around
//! a single inner content widget, with absolute positioning.
//!
//! `WindowFrame` is the canonical way to wrap a `TitleBar` + body for an
//! undecorated Wayland window. Composing the same shape with nested
//! `HStack` / `VStack` only works if every wrapper between the strips
//! and the content is a spacer — `HStack`/`VStack` only fill their main
//! axis when they have spacer children, otherwise they collapse to the
//! sum of their children's intrinsic sizes. That makes the nested
//! approach fragile (one missing `Expand::fills_stack` and the frame
//! collapses to ~12 pixels). Doing the layout ourselves with absolute
//! coordinates sidesteps the whole issue.
//!
//! Layout:
//!
//! ```text
//! ┌────────────────────────────────┐
//! │      top resize strip          │
//! ├──┬──────────────────────────┬──┤
//! │  │                          │  │
//! │L │        content           │R │
//! │  │                          │  │
//! ├──┴──────────────────────────┴──┤
//! │     bottom resize strip        │
//! └────────────────────────────────┘
//! ```
//!
//! The four strips are M2-final 4-edge resize: there are no diagonal
//! corner widgets yet (fern-core's `CursorIcon` has no diagonal resize
//! variant). Corners belong to the top and bottom strips.

use std::rc::Rc;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::widget::{
    LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_id::WidgetId;
use fern_core::{PlatformTitleBarHost, ResizeEdge};

use super::resize_strip::ResizeStrip;

pub struct WindowFrame {
    host: Rc<dyn PlatformTitleBarHost>,
    thickness: f32,
    pending_content: Option<PendingChild>,
    content_id: Option<WidgetId>,
    /// Order: [top, bottom, left, right]
    strip_ids: [Option<WidgetId>; 4],
    /// Order: [top_left, top_right, bottom_left, bottom_right]
    corner_ids: [Option<WidgetId>; 4],
}

impl std::fmt::Debug for WindowFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowFrame")
            .field("thickness", &self.thickness)
            .field("has_content", &self.pending_content.is_some())
            .finish_non_exhaustive()
    }
}

impl WindowFrame {
    pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self {
        Self {
            host,
            thickness: 6.0,
            pending_content: None,
            content_id: None,
            strip_ids: [None; 4],
            corner_ids: [None; 4],
        }
    }

    /// Logical-pixel thickness of each resize strip. Default: 6.
    pub fn thickness(mut self, t: f32) -> Self {
        self.thickness = t;
        self
    }

    /// Set the inner content widget — typically a `VStack` containing a
    /// `TitleBar` and the application body.
    pub fn content(mut self, w: impl Widget + 'static) -> Self {
        self.pending_content = Some(PendingChild::Deferred(Box::new(w)));
        self
    }

    pub fn content_boxed(mut self, w: Box<dyn Widget>) -> Self {
        self.pending_content = Some(PendingChild::Deferred(w));
        self
    }

    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.pending_content = Some(PendingChild::Id(id));
        self
    }
}

impl Widget for WindowFrame {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<WidgetId> {
        // Resolve the optional content child first so it sits at index 0
        // in the children list — `place_children` relies on the order
        // matching
        // `[content, top, bottom, left, right, top_left, top_right, bottom_left, bottom_right]`.
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }

        self.strip_ids[0] = Some(ctx.add(ResizeStrip::horizontal(
            self.host.clone(),
            ResizeEdge::Top,
            self.thickness,
        )));
        self.strip_ids[1] = Some(ctx.add(ResizeStrip::horizontal(
            self.host.clone(),
            ResizeEdge::Bottom,
            self.thickness,
        )));
        self.strip_ids[2] = Some(ctx.add(ResizeStrip::vertical(
            self.host.clone(),
            ResizeEdge::Left,
            self.thickness,
        )));
        self.strip_ids[3] = Some(ctx.add(ResizeStrip::vertical(
            self.host.clone(),
            ResizeEdge::Right,
            self.thickness,
        )));

        // Corners — added AFTER the edges so that fern-core's hit-test
        // (children walked in reverse order — see `hit_test_recursive`
        // in `event_dispatch_impl.rs`) checks the corners first. In
        // practice we also place them at non-overlapping positions, but
        // walking last also guarantees priority under future layout
        // refactors.
        self.corner_ids[0] = Some(ctx.add(ResizeStrip::corner(
            self.host.clone(),
            ResizeEdge::TopLeft,
            self.thickness,
        )));
        self.corner_ids[1] = Some(ctx.add(ResizeStrip::corner(
            self.host.clone(),
            ResizeEdge::TopRight,
            self.thickness,
        )));
        self.corner_ids[2] = Some(ctx.add(ResizeStrip::corner(
            self.host.clone(),
            ResizeEdge::BottomLeft,
            self.thickness,
        )));
        self.corner_ids[3] = Some(ctx.add(ResizeStrip::corner(
            self.host.clone(),
            ResizeEdge::BottomRight,
            self.thickness,
        )));

        let mut ids = Vec::with_capacity(9);
        if let Some(c) = self.content_id {
            ids.push(c);
        }
        for s in &self.strip_ids {
            if let Some(s) = s {
                ids.push(*s);
            }
        }
        for c in &self.corner_ids {
            if let Some(c) = c {
                ids.push(*c);
            }
        }
        ids
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // Always claim every pixel offered. The frame is meant to wrap a
        // window's full client area — anything smaller would leave bare
        // space at the edges.
        Size::new(
            proposal.width.unwrap_or(0.0),
            proposal.height.unwrap_or(0.0),
        )
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let t = self.thickness;
        let inner_w = (bounds.width - 2.0 * t).max(0.0);
        let inner_h = (bounds.height - 2.0 * t).max(0.0);

        // Children are in the same order they were added in build():
        // index 0 (if content present) → content
        // then [top, bottom, left, right] edges
        // then [top_left, top_right, bottom_left, bottom_right] corners
        let mut i = 0;

        if self.content_id.is_some() {
            children[i].origin = Point::new(bounds.x + t, bounds.y + t);
            children[i].size = Size::new(inner_w, inner_h);
            i += 1;
        }

        // Edges — shortened by `t` on both ends so the 4 corner cells
        // own the corner squares.
        // Top: between the two top corners.
        children[i].origin = Point::new(bounds.x + t, bounds.y);
        children[i].size = Size::new(inner_w, t);
        i += 1;

        // Bottom: between the two bottom corners.
        children[i].origin = Point::new(bounds.x + t, bounds.bottom() - t);
        children[i].size = Size::new(inner_w, t);
        i += 1;

        // Left: between the two left corners.
        children[i].origin = Point::new(bounds.x, bounds.y + t);
        children[i].size = Size::new(t, inner_h);
        i += 1;

        // Right: between the two right corners.
        children[i].origin = Point::new(bounds.right() - t, bounds.y + t);
        children[i].size = Size::new(t, inner_h);
        i += 1;

        // Corners — square `t × t` cells at each window corner.
        // Top-left.
        children[i].origin = bounds.origin();
        children[i].size = Size::new(t, t);
        i += 1;

        // Top-right.
        children[i].origin = Point::new(bounds.right() - t, bounds.y);
        children[i].size = Size::new(t, t);
        i += 1;

        // Bottom-left.
        children[i].origin = Point::new(bounds.x, bounds.bottom() - t);
        children[i].size = Size::new(t, t);
        i += 1;

        // Bottom-right.
        children[i].origin = Point::new(bounds.right() - t, bounds.bottom() - t);
        children[i].size = Size::new(t, t);
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {
        // Fully transparent — the inner content paints its own background.
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids = Vec::with_capacity(9);
        if let Some(c) = self.content_id {
            ids.push(c);
        }
        for s in &self.strip_ids {
            if let Some(s) = s {
                ids.push(*s);
            }
        }
        for c in &self.corner_ids {
            if let Some(c) = c {
                ids.push(*c);
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::Point;
    use fern_core::widget_tree::WidgetTree;
    use fern_core::{HitRegions, PlatformError};
    use std::cell::Cell;

    #[derive(Default)]
    struct TestHost {
        last_resize_edge: Cell<Option<ResizeEdge>>,
    }

    impl PlatformTitleBarHost for TestHost {
        fn reserved_leading_inset(&self) -> Size {
            Size::ZERO
        }
        fn reserved_trailing_inset(&self) -> Size {
            Size::ZERO
        }
        fn renders_custom_controls(&self) -> bool {
            true
        }
        fn begin_drag(&self) -> Result<(), PlatformError> {
            Ok(())
        }
        fn begin_resize(&self, edge: ResizeEdge) -> Result<(), PlatformError> {
            self.last_resize_edge.set(Some(edge));
            Ok(())
        }
        fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
            Ok(())
        }
        fn minimize(&self) {}
        fn toggle_maximize(&self) {}
        fn close(&self) {}
        fn is_maximized(&self) -> bool {
            false
        }
        fn update_hit_regions(&self, _regions: &HitRegions) {}
    }

    #[derive(Debug)]
    struct ContentLeaf;
    impl Widget for ContentLeaf {
        fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(
                proposal.width.unwrap_or(0.0),
                proposal.height.unwrap_or(0.0),
            )
        }
    }

    #[test]
    fn frame_inset_content_by_thickness_on_all_sides() {
        let host: Rc<dyn PlatformTitleBarHost> = Rc::new(TestHost::default());
        let mut tree = WidgetTree::new();
        let frame = tree.add(
            WindowFrame::new(host)
                .thickness(6.0)
                .content(ContentLeaf),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));

        // The frame itself fills the window.
        let f = tree.bounds(frame);
        assert!((f.width - 900.0).abs() < 0.01);
        assert!((f.height - 600.0).abs() < 0.01);

        // Walk to the content child (index 0, added before the strips).
        let kids = tree.children(frame);
        let content = kids[0];
        let cb = tree.bounds(content);
        assert!((cb.x - 6.0).abs() < 0.01, "content x = {}", cb.x);
        assert!((cb.y - 6.0).abs() < 0.01, "content y = {}", cb.y);
        assert!((cb.width - 888.0).abs() < 0.01, "content w = {}", cb.width);
        assert!(
            (cb.height - 588.0).abs() < 0.01,
            "content h = {}",
            cb.height
        );
    }

    #[test]
    fn clicking_top_strip_calls_begin_resize_top() {
        let host = Rc::new(TestHost::default());
        let mut tree = WidgetTree::new();
        let _frame = tree.add(
            WindowFrame::new(host.clone() as Rc<dyn PlatformTitleBarHost>)
                .thickness(6.0)
                .content(ContentLeaf),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));

        // Click in the top 6 pixels.
        tree.pointer_move(Point::new(450.0, 3.0));
        tree.pointer_down_button(
            Point::new(450.0, 3.0),
            fern_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(450.0, 3.0),
            fern_core::event::PointerButton::Primary,
        );

        assert_eq!(host.last_resize_edge.get(), Some(ResizeEdge::Top));
    }

    #[test]
    fn clicking_top_left_corner_calls_begin_resize_top_left() {
        let host = Rc::new(TestHost::default());
        let mut tree = WidgetTree::new();
        let _frame = tree.add(
            WindowFrame::new(host.clone() as Rc<dyn PlatformTitleBarHost>)
                .thickness(6.0)
                .content(ContentLeaf),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));

        // Click inside the 6x6 top-left corner.
        tree.pointer_move(Point::new(2.0, 2.0));
        tree.pointer_down_button(
            Point::new(2.0, 2.0),
            fern_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(2.0, 2.0),
            fern_core::event::PointerButton::Primary,
        );

        assert_eq!(host.last_resize_edge.get(), Some(ResizeEdge::TopLeft));
    }

    #[test]
    fn clicking_bottom_right_corner_calls_begin_resize_bottom_right() {
        let host = Rc::new(TestHost::default());
        let mut tree = WidgetTree::new();
        let _frame = tree.add(
            WindowFrame::new(host.clone() as Rc<dyn PlatformTitleBarHost>)
                .thickness(6.0)
                .content(ContentLeaf),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));

        // Click inside the 6x6 bottom-right corner: x in [894, 900),
        // y in [594, 600).
        let p = Point::new(897.0, 597.0);
        tree.pointer_move(p);
        tree.pointer_down_button(p, fern_core::event::PointerButton::Primary);
        tree.pointer_up_button(p, fern_core::event::PointerButton::Primary);

        assert_eq!(host.last_resize_edge.get(), Some(ResizeEdge::BottomRight));
    }

    #[test]
    fn clicking_in_content_area_does_not_resize() {
        let host = Rc::new(TestHost::default());
        let mut tree = WidgetTree::new();
        let _frame = tree.add(
            WindowFrame::new(host.clone() as Rc<dyn PlatformTitleBarHost>)
                .thickness(6.0)
                .content(ContentLeaf),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));

        // Click in the middle of the content area.
        tree.pointer_move(Point::new(450.0, 300.0));
        tree.pointer_down_button(
            Point::new(450.0, 300.0),
            fern_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(450.0, 300.0),
            fern_core::event::PointerButton::Primary,
        );

        assert_eq!(
            host.last_resize_edge.get(),
            None,
            "interior clicks must not trigger resize"
        );
    }
}
