// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A borderless-window frame: an invisible overlay of resize strips and
//! corner cells along the four edges of a single content widget.
//!
//! `WindowFrame` is the canonical way to wrap a `TitleBar` + body for an
//! undecorated Wayland window. The content child fills the entire window
//! bounds — there is *no* visible padding — and the resize strips +
//! corners sit on top of the content along the edges. teksilo-core's
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
//! window boundary — with a *mouse*. A finger reaches the same edge
//! through each strip's [`Widget::hit_outset`], which widens the band to
//! the density's target size without moving a pixel of layout; see
//! [`resize_strip`](super::resize_strip).
//!
//! ## Telling the OS the same number
//!
//! On a platform where the window manager answers the resize hit test itself
//! — Windows, through `WM_NCHITTEST` — the frame's own strips never see the
//! press, so the two layers have to agree about how wide the band is or a
//! finger lands in the gap between them. The frame therefore publishes its
//! **coarse** band (the widened one, in logical pixels) as
//! [`HitRegions::resize_borders`] every frame, and the backend applies it to
//! coarse messages only, never shrinking the band a mouse gets.
//!
//! That channel has one publisher per window — [`TitleBar`](crate::TitleBar)
//! aggregates the drag region and the control buttons into one snapshot from
//! its own `after_paint`. The frame does not compete with it: the snapshot it
//! publishes carries a non-zero `resize_borders` and nothing else, which is the
//! documented shape a backend reads as a *band update* rather than a
//! replacement. `after_paint` is post-order, so wrapping the title bar (the
//! canonical shape — `WindowFrame::content(VStack { TitleBar, body })`) puts
//! the band update after the aggregate snapshot every frame.
//!
//! Wrapping the frame in a `WidgetBuilder` method is safe:
//! `WidgetWithHandlers` forwards `wants_after_paint` / `after_paint` along with
//! the rest of the trait, so `WindowFrame::new(host).content(..).on_tap(..)`
//! still publishes. It did not always — the wrapper's forwarding list was
//! incomplete, and an unforwarded hook silences a publish with no diagnostic —
//! so the list is now exhaustive and lint-guarded at its own impl.

use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::styles::density::dp;
use teksilo_core::widget::{
    LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement, WidgetTreeView,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::{HitRegions, PlatformTitleBarHost, ResizeBorders, ResizeEdge};
use teksilo_tokens::{InputTokens, TargetRole};

use super::resize_strip::ResizeStrip;

/// Invisible overlay of resize strips and corner cells that gives a borderless window
/// draggable edges. The content child fills the full client area with no visible inset;
/// the strips are hit-test-only overlays along the outer `thickness` pixels.
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

/// Logical-pixel thickness of each resize strip, and the frame's default.
///
/// The same 6 dp gutter the `Splitter` and the dock resize handle use. It is
/// **fixed at every density**: the strip is a hit-test-only overlay drawn over
/// the window's own edge, so widening it would eat into the content rather
/// than into empty space. A coarse pointer reaches it through
/// `Widget::hit_outset` (24 dp, 44 at Touch) over an unchanged 6 dp visual —
/// see `docs/density-inventory.md` and the touch design's Constants table.
pub const WINDOW_FRAME_RESIZE_THICKNESS: f32 = 6.0;

impl WindowFrame {
    /// Create a frame bound to the given platform host. Use [`thickness`](WindowFrame::thickness)
    /// and [`content`](WindowFrame::content) to configure it before adding to the tree.
    pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self {
        Self {
            host,
            thickness: WINDOW_FRAME_RESIZE_THICKNESS,
            pending_content: None,
            content_id: None,
            strip_ids: [None; 4],
            corner_ids: [None; 4],
        }
    }

    /// Logical-pixel thickness of each resize strip. Default:
    /// [`WINDOW_FRAME_RESIZE_THICKNESS`].
    pub fn thickness(mut self, t: f32) -> Self {
        self.thickness = t;
        self
    }

    /// Set the inner content widget — typically a `VStack` containing a
    /// `TitleBar` and the application body.
    pub fn content(mut self, w: impl teksilo_core::IntoTeksiChild) -> Self {
        self.pending_content = Some(teksilo_core::IntoTeksiChild::into_pending(w));
        self
    }

    /// Set the inner content widget from an already-boxed value. Prefer [`content`](WindowFrame::content)
    /// for unboxed widgets; use this variant when the concrete type is not known at the call site.
    pub fn content_boxed(mut self, w: Box<dyn Widget>) -> Self {
        self.pending_content = Some(PendingChild::Deferred(w));
        self
    }

    /// The band a **coarse** pointer actually catches on each edge, in logical
    /// pixels: the strip's painted thickness widened to the density's target
    /// size by [`Widget::hit_outset`] (24 dp Compact, 44 dp Touch).
    ///
    /// This is what the frame publishes as [`HitRegions::resize_borders`] so a
    /// backend that answers the resize hit test itself can use the same number.
    /// It is deliberately *not* the mouse band: a mouse keeps the painted
    /// thickness, and a backend must never shrink its own metric to this.
    ///
    /// Uniform across the four edges — every strip is built at the same
    /// thickness — so a caller reading one field reads them all.
    pub fn coarse_resize_borders(&self, tokens: &InputTokens) -> ResizeBorders {
        ResizeBorders::uniform(dp(self.thickness, TargetRole::Target, tokens))
    }
}

impl Widget for WindowFrame {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
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

        // Corners — added AFTER the edges so that teksilo-core's hit-test
        // (children walked in reverse order — see `hit_test_recursive`
        // in `arena.rs`) checks the corners first. In
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
    ) -> teksilo_core::widget::LayoutResponse {
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

    fn paint(&self, _bounds: Rect, _canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        // Fully transparent — the inner content paints its own background.
    }

    fn wants_after_paint(&self) -> bool {
        // Publish the coarse resize band so a backend that owns the non-client
        // hit test agrees with what the strips will actually catch. See the
        // module docs for why this cannot collide with `TitleBar`'s aggregate
        // snapshot.
        true
    }

    fn after_paint(&self, _view: &WidgetTreeView<'_>, ctx: &PaintContext) {
        let regions = HitRegions {
            resize_borders: self.coarse_resize_borders(&ctx.theme.input),
            ..HitRegions::default()
        };
        self.host.update_hit_regions(&regions);
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
    use std::cell::Cell;
    use teksilo_canvas::Point;
    use teksilo_core::Signal;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_core::{HitRegions, PlatformError};

    struct TestHost {
        last_resize_edge: Cell<Option<ResizeEdge>>,
        resize_calls: Cell<u32>,
        /// Last payload handed to `update_hit_regions` — what a real backend
        /// would store and answer its non-client hit test from.
        last_regions: std::cell::RefCell<Option<HitRegions>>,
        is_max: Signal<bool>,
    }

    impl Default for TestHost {
        fn default() -> Self {
            Self {
                last_resize_edge: Cell::new(None),
                resize_calls: Cell::new(0),
                last_regions: std::cell::RefCell::new(None),
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
            self.resize_calls.set(self.resize_calls.get() + 1);
            Ok(())
        }
        fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
            Ok(())
        }
        fn update_hit_regions(&self, regions: &HitRegions) {
            *self.last_regions.borrow_mut() = Some(regions.clone());
        }
    }

    #[derive(Debug)]
    struct ContentLeaf;
    impl Widget for ContentLeaf {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
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
            teksilo_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(450.0, 3.0),
            teksilo_core::event::PointerButton::Primary,
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
            teksilo_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(2.0, 2.0),
            teksilo_core::event::PointerButton::Primary,
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
        tree.pointer_down_button(p, teksilo_core::event::PointerButton::Primary);
        tree.pointer_up_button(p, teksilo_core::event::PointerButton::Primary);

        assert_eq!(host.last_resize_edge.get(), Some(ResizeEdge::BottomRight));
    }

    // ---------------------------------------------------------------
    // The coarse grab band
    // ---------------------------------------------------------------

    /// The content is **tappable** in these fixtures on purpose: over inert
    /// content the miss-only slop pass re-attributes a nearby press to a strip
    /// by itself, so a hit test written over an inert leaf would pass with
    /// `hit_outset` deleted. Beating a target that owns the press is what the
    /// outset is for.
    fn frame_over_tappable_content(
        host: Rc<TestHost>,
        density: teksilo_tokens::TargetDensity,
    ) -> WidgetTree {
        use teksilo_core::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light().with_density(density))
            .with_text_backend(Rc::new(std::cell::RefCell::new(
                teksilo_canvas::MockTextBackend::new(),
            )));
        let _frame = tree.add(
            WindowFrame::new(host as Rc<dyn PlatformTitleBarHost>)
                .thickness(6.0)
                .content(ContentLeaf.on_tap(|_, _| {})),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));
        tree
    }

    fn finger(raw: u64, primary: bool) -> teksilo_core::pointer::PointerInfo {
        use teksilo_core::pointer::{BackendDeviceKey, EventTime, PointerIdAllocator, PointerInfo};
        let id = PointerIdAllocator::global().begin(BackendDeviceKey::new(0x50F2), raw);
        let mut info = PointerInfo::touch(id, EventTime::ZERO);
        info.primary = primary;
        info
    }

    fn contact(
        pointer: teksilo_core::pointer::PointerInfo,
        phase: teksilo_core::pointer::PointerPhase,
        at: Point,
    ) -> teksilo_core::pointer::PointerSample {
        teksilo_core::pointer::PointerSample {
            pointer,
            phase,
            position: at,
            button: None,
            modifiers: teksilo_core::event::Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// A finger reaches the top edge from 14 dp inside the content — the band
    /// the OS-side publication advertises — and the press really does start a
    /// resize, not merely hit-test to the strip.
    #[test]
    fn a_finger_grabs_the_top_edge_from_inside_the_content() {
        let host = Rc::new(TestHost::default());
        let mut tree =
            frame_over_tappable_content(host.clone(), teksilo_tokens::TargetDensity::Compact);
        let at = Point::new(450.0, 14.0);
        let f = finger(31, true);
        tree.dispatch_pointer(contact(f, teksilo_core::pointer::PointerPhase::Down, at));
        assert_eq!(host.last_resize_edge.get(), Some(ResizeEdge::Top));
    }

    /// The same point with a mouse is content, exactly as it was before the
    /// grab existed: the band is direct-pointer-only.
    #[test]
    fn a_mouse_below_the_strip_is_still_content() {
        let host = Rc::new(TestHost::default());
        let mut tree =
            frame_over_tappable_content(host.clone(), teksilo_tokens::TargetDensity::Compact);
        let at = Point::new(450.0, 14.0);
        tree.pointer_move(at);
        tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
        tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
        assert_eq!(host.last_resize_edge.get(), None);
        assert_eq!(host.resize_calls.get(), 0);
    }

    /// Touch density widens the band to 44 dp without moving a pixel: the
    /// strips are still 6 dp and the content still fills the window.
    #[test]
    fn the_grab_moves_no_layout_at_touch_density() {
        let host = Rc::new(TestHost::default());
        let mut tree =
            frame_over_tappable_content(host.clone(), teksilo_tokens::TargetDensity::Touch);
        let frame = tree.roots()[0];
        let kids = tree.children(frame);
        let content = tree.bounds(kids[0]);
        assert_eq!(content, Rect::new(0.0, 0.0, 900.0, 600.0));
        // kids[1] is the top strip — 6 dp tall, unchanged.
        assert_eq!(tree.bounds(kids[1]).height, 6.0);

        // …and a finger reaches 30 dp in, which Compact would not have allowed.
        let at = Point::new(450.0, 30.0);
        let f = finger(32, true);
        tree.dispatch_pointer(contact(f, teksilo_core::pointer::PointerPhase::Down, at));
        assert_eq!(host.last_resize_edge.get(), Some(ResizeEdge::Top));
    }

    /// One interactive resize per gesture: a second contact must not ask the
    /// compositor for a second resize of the same window against a live one.
    #[test]
    fn a_second_contact_does_not_start_a_second_resize() {
        let host = Rc::new(TestHost::default());
        let mut tree =
            frame_over_tappable_content(host.clone(), teksilo_tokens::TargetDensity::Compact);
        let first = finger(33, true);
        tree.dispatch_pointer(contact(
            first,
            teksilo_core::pointer::PointerPhase::Down,
            Point::new(450.0, 3.0),
        ));
        assert_eq!(host.resize_calls.get(), 1);

        let second = finger(34, false);
        tree.dispatch_pointer(contact(
            second,
            teksilo_core::pointer::PointerPhase::Down,
            Point::new(300.0, 3.0),
        ));
        assert_eq!(
            host.resize_calls.get(),
            1,
            "the second contact must not start a second resize"
        );
    }

    /// The band the frame publishes is the one a finger actually catches, so a
    /// backend answering the non-client hit test agrees with the widget layer.
    #[test]
    fn the_frame_publishes_the_coarse_band_it_will_catch() {
        for (density, expected) in [
            (teksilo_tokens::TargetDensity::Compact, 24.0_f32),
            (teksilo_tokens::TargetDensity::Comfortable, 32.0),
            (teksilo_tokens::TargetDensity::Touch, 44.0),
        ] {
            let host = Rc::new(TestHost::default());
            let mut tree = frame_over_tappable_content(host.clone(), density);
            let _ = tree.render();
            let regions = host
                .last_regions
                .borrow()
                .clone()
                .expect("the frame publishes every frame");
            assert_eq!(regions.resize_borders.top, expected, "{density:?}");
            assert_eq!(regions.resize_borders.left, expected, "{density:?}");
            // …and nothing else, so a backend reads it as a band update rather
            // than as a replacement for the title bar's own snapshot.
            assert!(regions.drag.is_empty());
            assert!(regions.minimize.is_none());
        }
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
            teksilo_core::event::PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(450.0, 300.0),
            teksilo_core::event::PointerButton::Primary,
        );

        assert_eq!(
            host.last_resize_edge.get(),
            None,
            "interior clicks must not trigger resize"
        );
    }
}
