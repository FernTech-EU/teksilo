//! Windows title bar host stub. Real implementation arrives in M4 / M5
//! (frame extension via `DwmExtendFrameIntoClientArea`, `WM_NCCALCSIZE` /
//! `WM_NCHITTEST` subclassing, `HTMAXBUTTON` for the Win11 snap layout
//! flyout). Today the constructor returns [`PlatformError::Unsupported`]
//! so the rest of the codebase compiles on Windows.

use std::sync::Arc;

use fern_canvas::{Point, Size};
use fern_core::{HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge};
use winit::window::Window;

#[allow(dead_code)]
pub struct WindowsHost {
    window: Arc<Window>,
}

impl WindowsHost {
    #[allow(dead_code)]
    pub fn new(_window: Arc<Window>) -> Result<Self, PlatformError> {
        Err(PlatformError::Unsupported)
    }
}

impl PlatformTitleBarHost for WindowsHost {
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
        Err(PlatformError::Unsupported)
    }
    fn begin_resize(&self, _edge: ResizeEdge) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
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
