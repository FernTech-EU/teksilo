//! Wayland title bar host. Pure delegation to winit's `Window` API: the
//! compositor handles drag, resize, and the system window menu via the
//! standard xdg-shell protocol, so we don't need any FFI of our own.

use std::sync::Arc;

use fern_canvas::{Point, Size};
use fern_core::{HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge, TitleBarHostCallbacks};
use winit::dpi::LogicalPosition;
use winit::window::{ResizeDirection, Window};

pub struct WaylandHost {
    window: Arc<Window>,
}

impl WaylandHost {
    pub fn new(
        window: Arc<Window>,
        _callbacks: TitleBarHostCallbacks,
    ) -> Result<Self, PlatformError> {
        // `callbacks` used to carry a `request_close` closure before
        // the trait trim — close now flows through
        // `WindowState::close` on the widget-tree side, so the
        // Wayland backend no longer needs it. Parameter kept in the
        // signature so the factory in `title_bar_host.rs` can keep
        // its uniform construction form.
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

    fn needs_custom_resize_handles(&self) -> bool {
        // Wayland with `with_decorations(false)` leaves all frame painting
        // and edge resizing to the client — we install a WindowFrame overlay.
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
