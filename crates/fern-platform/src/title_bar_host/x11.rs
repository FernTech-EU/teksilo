//! X11 fallback. The X11 server protocol cannot give us the equivalent of
//! Windows' DWM extended frame or Wayland's `xdg_toplevel.move` without
//! either reimplementing window-manager hints by hand (out of scope) or
//! shipping a full IPC integration with each WM. So we don't try.
//!
//! [`X11Host`] exists only to make the rest of the codebase compile cleanly
//! on every Linux configuration; the factory in `title_bar_host.rs` returns
//! [`PlatformError::Unsupported`] before ever constructing one.

use std::sync::Arc;

use fern_canvas::{Point, Size};
use fern_core::{HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge};
use winit::window::Window;

#[allow(dead_code)]
pub struct X11Host {
    window: Arc<Window>,
}

impl X11Host {
    #[allow(dead_code)]
    pub fn new(_window: Arc<Window>) -> Result<Self, PlatformError> {
        Err(PlatformError::Unsupported)
    }
}

impl PlatformTitleBarHost for X11Host {
    fn reserved_leading_inset(&self) -> Size {
        Size::ZERO
    }
    fn reserved_trailing_inset(&self) -> Size {
        Size::ZERO
    }
    fn renders_custom_controls(&self) -> bool {
        false
    }
    fn begin_drag(&self) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn begin_resize(&self, _edge: ResizeEdge) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn minimize(&self) {}
    fn toggle_maximize(&self) {}
    fn close(&self) {}
    fn is_maximized(&self) -> bool {
        false
    }
    fn update_hit_regions(&self, _regions: &HitRegions) {}
}
