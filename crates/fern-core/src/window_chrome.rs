//! Platform abstraction for custom window chrome (title bars).
//!
//! `PlatformTitleBarHost` is the seam between a platform-agnostic `TitleBar`
//! widget (in `fern-widgets`) and the per-OS implementations that own the
//! window handle (in `fern-platform`). The trait is intentionally `!Send +
//! !Sync`: every implementation lives on the UI thread alongside the widget
//! tree.

use std::fmt;

use fern_canvas::{Point, Rect, Size};

/// Capabilities the title bar widget needs from the windowing layer.
pub trait PlatformTitleBarHost {
    /// Logical-pixel area on the leading edge that the widget must leave
    /// blank because the OS draws there. macOS reserves space for the traffic
    /// lights; Windows / Wayland return `Size::ZERO`.
    fn reserved_leading_inset(&self) -> Size;

    /// Logical-pixel area on the trailing edge reserved by the OS. Currently
    /// always `Size::ZERO`; reserved for future use.
    fn reserved_trailing_inset(&self) -> Size;

    /// Whether the widget should render its own minimize / maximize / close
    /// buttons. `true` on Windows and Wayland; `false` on macOS where the OS
    /// draws the traffic lights.
    fn renders_custom_controls(&self) -> bool;

    /// Begin an interactive window move. Called on left-press inside a drag
    /// region. The OS takes over until the user releases the button.
    fn begin_drag(&self) -> Result<(), PlatformError>;

    /// Begin an interactive resize from the given edge. Called on left-press
    /// inside a resize border widget.
    fn begin_resize(&self, edge: ResizeEdge) -> Result<(), PlatformError>;

    /// Show the system window menu at the given client-area position. Wayland
    /// only; other platforms return `Ok(())` and do nothing.
    fn show_window_menu(&self, at: Point) -> Result<(), PlatformError>;

    fn minimize(&self);
    fn toggle_maximize(&self);
    fn close(&self);
    fn is_maximized(&self) -> bool;

    /// Publish the current physical-pixel rectangles of the title bar's
    /// interactive sub-regions. The Windows backend reads these from its
    /// `WM_NCHITTEST` handler each frame; other backends ignore them.
    ///
    /// Called from `TitleBar::paint` on every frame.
    fn update_hit_regions(&self, regions: &HitRegions);
}

/// Set of physical-pixel rectangles inside the window client area that the
/// title bar widget cares about. Coordinates are relative to the window
/// client origin (top-left).
#[derive(Debug, Default, Clone)]
pub struct HitRegions {
    pub minimize: Option<Rect>,
    pub maximize: Option<Rect>,
    pub close: Option<Rect>,
    /// One or more drag-region rectangles. Multiple rects allow non-rectangular
    /// drag surfaces (e.g. drag region split around a centred search bar).
    pub drag: Vec<Rect>,
    pub resize_borders: ResizeBorders,
}

impl HitRegions {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Physical-pixel widths of the eight resize edges. Zero means "no resize
/// border on this side". The Windows backend uses these to translate
/// `WM_NCHITTEST` cursor positions into `HTLEFT`/`HTTOPRIGHT`/etc.
#[derive(Debug, Default, Clone, Copy)]
pub struct ResizeBorders {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl ResizeBorders {
    pub const fn uniform(thickness: f32) -> Self {
        Self {
            top: thickness,
            right: thickness,
            bottom: thickness,
            left: thickness,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    Top,
    TopLeft,
    TopRight,
    Left,
    Right,
    Bottom,
    BottomLeft,
    BottomRight,
}

#[derive(Debug)]
pub enum PlatformError {
    /// The current platform / window system does not support custom chrome
    /// at all (e.g. X11) or does not support a specific operation (e.g.
    /// `begin_resize` on macOS, where winit lacks `drag_resize_window`).
    Unsupported,
    /// An OS-level call failed. The string is intended for logging, not
    /// programmatic inspection.
    Os(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "operation not supported on this platform"),
            Self::Os(msg) => write!(f, "platform error: {msg}"),
        }
    }
}

impl std::error::Error for PlatformError {}
