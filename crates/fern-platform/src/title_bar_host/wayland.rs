//! Wayland title bar host. Pure delegation to winit's `Window` API: the
//! compositor handles drag, resize, and the system window menu via the
//! standard xdg-shell protocol, so we don't need any FFI of our own.

use std::sync::Arc;

use fern_canvas::{Point, Size};
use fern_core::{HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge};
use winit::dpi::LogicalPosition;
use winit::window::{ResizeDirection, Window};

pub struct WaylandHost {
    window: Arc<Window>,
}

impl WaylandHost {
    pub fn new(window: Arc<Window>) -> Result<Self, PlatformError> {
        Ok(Self { window })
    }
}

impl PlatformTitleBarHost for WaylandHost {
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
        self.window
            .drag_window()
            .map_err(|e| PlatformError::Os(e.to_string()))
    }

    fn begin_resize(&self, edge: ResizeEdge) -> Result<(), PlatformError> {
        self.window
            .drag_resize_window(edge_to_direction(edge))
            .map_err(|e| PlatformError::Os(e.to_string()))
    }

    fn show_window_menu(&self, at: Point) -> Result<(), PlatformError> {
        self.window
            .show_window_menu(LogicalPosition::new(at.x as f64, at.y as f64));
        Ok(())
    }

    fn minimize(&self) {
        self.window.set_minimized(true);
    }

    fn toggle_maximize(&self) {
        self.window.set_maximized(!self.window.is_maximized());
    }

    fn close(&self) {
        // winit 0.30 has no direct "close window" call; the application
        // observes `WindowEvent::CloseRequested` and tears down the
        // `PlatformWindow`. The closest equivalent is to drop the window,
        // which we cannot do from behind a shared `Arc`. Instead, the
        // application is expected to wire its title-bar close button
        // through `TitleBar::close_action`, whose closure receives an
        // `EventContext` and can call `EventContext::close_window`
        // directly (or dispatch an `Intent` whose root `Action` does).
        // Until that wiring exists we leave this as a no-op.
    }

    fn is_maximized(&self) -> bool {
        self.window.is_maximized()
    }

    fn update_hit_regions(&self, _regions: &HitRegions) {
        // Wayland doesn't need hit regions: the widget tree handles every
        // pointer event itself, and `begin_drag` / `begin_resize` are
        // initiated explicitly from the widget's pointer handlers.
    }
}

fn edge_to_direction(edge: ResizeEdge) -> ResizeDirection {
    match edge {
        ResizeEdge::Top => ResizeDirection::North,
        ResizeEdge::TopRight => ResizeDirection::NorthEast,
        ResizeEdge::Right => ResizeDirection::East,
        ResizeEdge::BottomRight => ResizeDirection::SouthEast,
        ResizeEdge::Bottom => ResizeDirection::South,
        ResizeEdge::BottomLeft => ResizeDirection::SouthWest,
        ResizeEdge::Left => ResizeDirection::West,
        ResizeEdge::TopLeft => ResizeDirection::NorthWest,
    }
}
