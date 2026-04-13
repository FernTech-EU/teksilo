//! A thin invisible widget that forwards a window resize gesture to the
//! platform host when the user presses the primary button inside it. Used
//! to build a 6-px resize frame around a borderless window on Wayland.
//!
//! This is the frame complement to [`crate::title_bar::DragRegion`]: drag
//! moves the window, resize strips drag the window edges. On platforms
//! that don't expose `Window::drag_resize_window` (notably winit's macOS
//! backend), [`PlatformTitleBarHost::begin_resize`] returns
//! `PlatformError::Unsupported` and the strip becomes a silent no-op —
//! macOS handles edge resize via its own native chrome.

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::widget::{
    CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_core::{PlatformTitleBarHost, ResizeEdge};

/// A single edge of a resize frame. Construct one per side and lay them
/// out around your content (HStack of left + content + right inside a
/// VStack of top + middle + bottom is the conventional shape — see the
/// title bar demo for an example).
pub struct ResizeStrip {
    host: Rc<dyn PlatformTitleBarHost>,
    edge: ResizeEdge,
    width: f32,
    height: f32,
}

impl std::fmt::Debug for ResizeStrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResizeStrip")
            .field("edge", &self.edge)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl ResizeStrip {
    /// Build a horizontal (top / bottom) strip of the given height. The
    /// width is unconstrained — the strip claims whatever its parent
    /// container offers, so it can stretch across the full window width.
    pub fn horizontal(host: Rc<dyn PlatformTitleBarHost>, edge: ResizeEdge, thickness: f32) -> Self {
        debug_assert!(matches!(edge, ResizeEdge::Top | ResizeEdge::Bottom));
        Self {
            host,
            edge,
            width: 0.0,
            height: thickness,
        }
    }

    /// Build a vertical (left / right) strip of the given width. The
    /// height is unconstrained.
    pub fn vertical(host: Rc<dyn PlatformTitleBarHost>, edge: ResizeEdge, thickness: f32) -> Self {
        debug_assert!(matches!(edge, ResizeEdge::Left | ResizeEdge::Right));
        Self {
            host,
            edge,
            width: thickness,
            height: 0.0,
        }
    }

    /// Build a square corner cell of the given size. The corner handles a
    /// diagonal resize gesture (e.g. `TopLeft` does NW/SE resize). Should
    /// be placed *on top of* the edge strips at the four corners so the
    /// framework's hit-test routes the click to the corner rather than
    /// the adjacent edge.
    pub fn corner(host: Rc<dyn PlatformTitleBarHost>, edge: ResizeEdge, size: f32) -> Self {
        debug_assert!(matches!(
            edge,
            ResizeEdge::TopLeft
                | ResizeEdge::TopRight
                | ResizeEdge::BottomLeft
                | ResizeEdge::BottomRight
        ));
        Self {
            host,
            edge,
            width: size,
            height: size,
        }
    }
}

fn cursor_for_edge(edge: ResizeEdge) -> CursorIcon {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorIcon::RowResize,
        ResizeEdge::Left | ResizeEdge::Right => CursorIcon::ColResize,
        // NW/SE diagonal — corners on the top-left and bottom-right.
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorIcon::NwseResize,
        // NE/SW diagonal — corners on the top-right and bottom-left.
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorIcon::NeswResize,
    }
}

impl Widget for ResizeStrip {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<WidgetId> {
        let host = self.host.clone();
        let edge = self.edge;

        let handlers = HandlerSet::new()
            .cursor(cursor_for_edge(edge))
            .on_pointer_event(move |evt, _ctx| {
                if let WidgetEvent::PointerDown {
                    button: PointerButton::Primary,
                    ..
                } = evt
                {
                    let _ = host.begin_resize(edge);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // Horizontal strips: claim full proposed width, fixed height.
        // Vertical strips: claim full proposed height, fixed width.
        let w = if self.width > 0.0 {
            self.width
        } else {
            proposal.width.unwrap_or(0.0)
        };
        let h = if self.height > 0.0 {
            self.height
        } else {
            proposal.height.unwrap_or(0.0)
        };
        Size::new(w, h)
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {
        // Invisible.
    }

    fn is_spacer(&self) -> bool {
        // For a horizontal (top/bottom) strip in a VStack, claiming
        // "spacer on the cross axis" doesn't matter — VStack uses the
        // child's reported width directly. We mark non-spacer so the
        // strip's reported width equals the parent's offered width and
        // it stretches naturally across the row/column.
        false
    }
}
