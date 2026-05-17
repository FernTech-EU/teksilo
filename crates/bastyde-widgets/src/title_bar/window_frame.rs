//! A borderless-window frame: an invisible overlay of resize strips and
//! corner cells along the four edges of a single content widget.
//!
//! `WindowFrame` is the canonical way to wrap a `TitleBar` + body for an
//! undecorated Wayland window. The content child fills the entire window
//! bounds — there is *no* visible padding — and the resize strips +
//! corners sit on top of the content along the edges. bastyde-core's
//! `hit_test_recursive` walks children in reverse insertion order, so
//! the strips and corners (added after content) get first crack at any
//! click that lands within `thickness` pixels of an edge; clicks
//! anywhere else fall through to the content.
//!
//! Layout (with `thickness = t`):
//!
//! ```text
//! ┌─top─edge───────────────────────┐  ← top strip overlays content (0, 0, w, t)
//! │TL│                          │TR│  ← corners overlay the strip ends
//! │──│                          │──│
//! │L │       content (full)     │R │  ← content fills (0, 0, w, h)
//! │──│                          │──│
//! │BL│                          │BR│
//! └─bottom─edge────────────────────┘
//! ```
//!
//! `t` defaults to 6 logical pixels but is configurable via
//! [`WindowFrame::thickness`]. With a small thickness the frame is
//! visually undetectable; the cursor only changes (and the resize
//! gesture only triggers) when the pointer is within `t` pixels of the
//! window boundary.

use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::{PlatformTitleBarHost, ResizeEdge};

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
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
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

        // Corners — added AFTER the edges so that bastyde-core's hit-test
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
        for s in self.strip_ids.iter().flatten() {
            ids.push(*s);
        }
        for c in self.corner_ids.iter().flatten() {
            ids.push(*c);
        }
        ids
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Always claim every pixel offered. The frame is meant to wrap a
        // window's full client area — anything smaller would leave bare
        // space at the edges.
        Size::new(
            proposal.width.unwrap_or(0.0),
            proposal.height.unwrap_or(0.0),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let t = self.thickness;

        // Children are in insertion order:
        //   index 0 (if content present) → content
        //   then [top, bottom, left, right] edges
        //   then [top_left, top_right, bottom_left, bottom_right] corners
        //
        // Hit-test walks `.iter().rev()`, so corners are checked first,
        // then edges, then content — exactly the priority we want.
        let mut i = 0;

        if self.content_id.is_some() {
            // Content fills the FULL window — no inset, no visible
            // padding. The strips overlay it along the edges.
            children[i].origin = bounds.origin();
            children[i].size = bounds.size();
            i += 1;
        }

        // Edges — full-length strips overlaying the outer `t` pixels of
        // the content. They overlap the corners by `t × t`, but the
        // corner cells (added after) win the hit-test in those regions.
        // Top.
        children[i].origin = bounds.origin();
        children[i].size = Size::new(bounds.width, t);
        i += 1;

        // Bottom.
        children[i].origin = Point::new(bounds.x, bounds.bottom() - t);
        children[i].size = Size::new(bounds.width, t);
        i += 1;

        // Left.
        children[i].origin = bounds.origin();
        children[i].size = Size::new(t, bounds.height);
        i += 1;

        // Right.
        children[i].origin = Point::new(bounds.right() - t, bounds.y);
        children[i].size = Size::new(t, bounds.height);
        i += 1;

        // Corners — `t × t` squares at the four window corners.
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

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {
        // Fully transparent — the inner content paints its own background.
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids = Vec::with_capacity(9);
        if let Some(c) = self.content_id {
            ids.push(c);
        }
        for s in self.strip_ids.iter().flatten() {
            ids.push(*s);
        }
        for c in self.corner_ids.iter().flatten() {
            ids.push(*c);
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::Point;
    use bastyde_core::Signal;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_core::{HitRegions, PlatformError};
    use std::cell::Cell;

    struct TestHost {
        last_resize_edge: Cell<Option<ResizeEdge>>,
        is_max: Signal<bool>,
    }

    impl Default for TestHost {
        fn default() -> Self {
            Self {
                last_resize_edge: Cell::new(None),
                is_max: Signal::new(false),
            }
        }
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
        fn needs_custom_resize_handles(&self) -> bool {
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
        fn update_hit_regions(&self, _regions: &HitRegions) {}
    }

    #[derive(Debug)]
    struct ContentLeaf;
    impl Widget for ContentLeaf {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(
                proposal.width.unwrap_or(0.0),
                proposal.height.unwrap_or(0.0),
            )
            .into()
        }
    }

    #[test]
    fn frame_content_fills_full_window_no_visible_padding() {
        let host: Rc<dyn PlatformTitleBarHost> = Rc::new(TestHost::default());
        let mut tree = WidgetTree::new();
        let frame = tree.add(WindowFrame::new(host).thickness(6.0).content(ContentLeaf));
        tree.layout(SizeProposal::exact(900.0, 600.0));

        // The frame itself fills the window.
        let f = tree.bounds(frame);
        assert!((f.width - 900.0).abs() < 0.01);
        assert!((f.height - 600.0).abs() < 0.01);

        // Content is the FULL window — the resize frame is a hit-test
        // overlay only, no visible inset.
        let kids = tree.children(frame);
        let content = kids[0];
        let cb = tree.bounds(content);
        assert!((cb.x - 0.0).abs() < 0.01, "content x = {}", cb.x);
        assert!((cb.y - 0.0).abs() < 0.01, "content y = {}", cb.y);
        assert!((cb.width - 900.0).abs() < 0.01, "content w = {}", cb.width);
        assert!(
            (cb.height - 600.0).abs() < 0.01,
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
            bastyde_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(450.0, 3.0),
            bastyde_core::event::PointerButton::Primary,
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
            bastyde_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(2.0, 2.0),
            bastyde_core::event::PointerButton::Primary,
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
        tree.pointer_down_button(p, bastyde_core::event::PointerButton::Primary);
        tree.pointer_up_button(p, bastyde_core::event::PointerButton::Primary);

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
            bastyde_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(450.0, 300.0),
            bastyde_core::event::PointerButton::Primary,
        );

        assert_eq!(
            host.last_resize_edge.get(),
            None,
            "interior clicks must not trigger resize"
        );
    }
}
