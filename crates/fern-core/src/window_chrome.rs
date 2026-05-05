//! Platform abstraction for custom window chrome (title bars).
//!
//! `PlatformTitleBarHost` is the seam between a platform-agnostic `TitleBar`
//! widget (in `fern-widgets`) and the per-OS implementations that own the
//! window handle (in `fern-platform`). The trait is intentionally `!Send +
//! !Sync`: every implementation lives on the UI thread alongside the widget
//! tree.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use fern_canvas::{Point, Rect, Size};

use crate::widget_id::WidgetId;
use crate::window::FernWindowId;

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

    /// Whether the application should install a [`WindowFrame`]-style overlay
    /// with invisible edge / corner resize strips. `true` on Windows and
    /// Wayland where the client draws the entire frame; `false` on macOS
    /// where the native `NSWindow` frame still services edge resize.
    fn needs_custom_resize_handles(&self) -> bool;

    /// Begin an interactive window move. Called on left-press inside a drag
    /// region. The OS takes over until the user releases the button.
    fn begin_drag(&self) -> Result<(), PlatformError>;

    /// Begin an interactive resize from the given edge. Called on left-press
    /// inside a resize border widget.
    fn begin_resize(&self, edge: ResizeEdge) -> Result<(), PlatformError>;

    /// Show the system window menu at the given client-area position. Wayland
    /// only; other platforms return `Ok(())` and do nothing.
    fn show_window_menu(&self, at: Point) -> Result<(), PlatformError>;

    /// Publish the current rectangles of the title bar's interactive
    /// sub-regions. The widget tree publishes them in **logical**
    /// pixels; backends that need physical pixels (Windows) convert
    /// internally. Wayland and macOS ignore the payload.
    ///
    /// Called once per frame from
    /// [`Widget::after_paint`](crate::widget::Widget::after_paint) on
    /// the [`crate::Widget`]-implementing title bar root.
    fn update_hit_regions(&self, regions: &HitRegions);

    /// Resolve a control-button target back to the `WidgetId` of the
    /// `ControlButton` that the widget tree last reported for it.
    /// Used by the Windows backend's synthetic-tap forwarding when
    /// `WM_NCLBUTTONUP` fires on `HTMINBUTTON`/`HTMAXBUTTON`/`HTCLOSE`
    /// — the OS owns the click area, so the proc looks up the
    /// matching widget id and the app routes a synthetic tap into it.
    ///
    /// Default: `None`. Backends that don't intercept non-client
    /// button presses (Wayland, macOS) have no synthetic-tap path.
    fn title_bar_widget_id(&self, _target: ControlTarget) -> Option<WidgetId> {
        None
    }

    /// Inject a synthetic hover entered/leave event for the given
    /// control button. Used by the Windows backend's `WM_NCMOUSEMOVE`
    /// / `WM_NCMOUSELEAVE` path: the OS handles non-client hover, so
    /// widget-side hover events never fire over button rects.
    ///
    /// Default: no-op. The Windows host stores the matching
    /// `Signal<bool>` (registered by `WindowControls` at build time)
    /// and writes it; macOS / Wayland never produce these so the
    /// no-op suffices.
    fn set_button_hover(&self, _target: ControlTarget, _entered: bool) {}

    /// Register the per-button hover signal that the host writes
    /// when the OS reports a non-client hover for the matching
    /// `target`. Called by `WindowControls` at build time for each
    /// of the three buttons.
    ///
    /// Default: no-op. macOS / Wayland never need this — they get
    /// hover events through the widget tree's pointer pipeline.
    fn register_hover_signal(&self, _target: ControlTarget, _signal: crate::signal::Signal<bool>) {}
}

/// Target a synthetic title-bar tap or hover at a specific button.
/// The Windows backend posts these as part of
/// [`TitleBarSyntheticEvent`] / [`TitleBarHoverEvent`] payloads; the
/// fern-app dispatcher then looks up the matching widget id via
/// [`PlatformTitleBarHost::title_bar_widget_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlTarget {
    Minimize,
    Maximize,
    Close,
}

/// Synthetic primary-button tap on a custom title bar's control
/// button. Posted by the Windows backend's wndproc subclass on
/// `WM_NCLBUTTONUP` over `HTMINBUTTON` / `HTMAXBUTTON` / `HTCLOSE` —
/// the OS owned the click area (returned non-`HTCLIENT` from
/// `WM_NCHITTEST`) so widget land never saw it. The fern-app
/// dispatcher resolves the right `ControlButton` via
/// [`PlatformTitleBarHost::title_bar_widget_id`] and calls
/// `WidgetTree::synthesise_tap` on it. Wayland and macOS never
/// produce these.
#[derive(Debug, Clone, Copy)]
pub struct TitleBarSyntheticEvent {
    pub fern_id: FernWindowId,
    pub target: ControlTarget,
}

/// Hover entered/leave for a custom title-bar control button. Posted
/// by the Windows backend's `WM_NCMOUSEMOVE` / `WM_NCMOUSELEAVE`
/// handlers for the same reason as [`TitleBarSyntheticEvent`]: the
/// OS owns hover events over non-client areas.
#[derive(Debug, Clone, Copy)]
pub struct TitleBarHoverEvent {
    pub fern_id: FernWindowId,
    pub target: ControlTarget,
    pub entered: bool,
}

/// Callbacks the window manager hands to a platform host at
/// construction time. Hosts invoke these for operations that must go
/// through the event loop:
///
/// - `request_close`: winit 0.30 has no synchronous
///   `Window::request_close`, so the host posts a `CloseWindowRequest`
///   and `fern-app` routes it to `WindowManager::queue_close`.
/// - `post_external`: the Windows backend forwards `WM_NCLBUTTONUP` /
///   `WM_NCMOUSEMOVE` over its custom title-bar buttons as
///   `TitleBarSyntheticEvent` / `TitleBarHoverEvent` payloads through
///   the same `AppEvent::External` arm. The closure abstracts the
///   posting mechanism (a winit `EventLoopProxy` in production) so
///   fern-core stays winit-free.
/// - `fern_id`: the host's window id, copied into the synthetic
///   payloads so the dispatcher knows which window to address.
#[derive(Clone)]
pub struct TitleBarHostCallbacks {
    pub request_close: Rc<dyn Fn()>,
    /// Post a `Box<dyn Any + Send>` payload back to the application
    /// event loop. Currently used by the Windows backend for
    /// `TitleBarSyntheticEvent` and `TitleBarHoverEvent`. Wayland and
    /// macOS construct hosts that never call this.
    pub post_external: Rc<dyn Fn(Box<dyn Any + Send>)>,
    /// fern-side window id. The Windows backend stamps this into
    /// every synthetic payload it posts so the app dispatcher can
    /// route into the right `WidgetTree`.
    pub fern_id: FernWindowId,
}

impl TitleBarHostCallbacks {
    /// Callbacks that do nothing. Useful for tests and for platform stubs
    /// that never construct a host (e.g. X11).
    pub fn noop() -> Self {
        Self {
            request_close: Rc::new(|| {}),
            post_external: Rc::new(|_| {}),
            fern_id: FernWindowId::new(0),
        }
    }
}

impl fmt::Debug for TitleBarHostCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TitleBarHostCallbacks")
            .finish_non_exhaustive()
    }
}

/// Set of rectangles inside the window client area that the title bar
/// widget cares about. The widget tree publishes them in **logical**
/// pixels (its native coordinate system); platform backends that need
/// physical pixels (Windows) convert internally before storing.
/// Coordinates are relative to the window client origin (top-left).
#[derive(Debug, Default, Clone)]
pub struct HitRegions {
    pub minimize: Option<Rect>,
    pub maximize: Option<Rect>,
    pub close: Option<Rect>,
    /// Widget id of the minimize button, when one is present in the
    /// tree. Companions to the rect above; the Windows backend uses
    /// these to route `WM_NCLBUTTONUP` on `HTMINBUTTON` back into the
    /// widget tree as a synthetic tap.
    pub minimize_id: Option<WidgetId>,
    pub maximize_id: Option<WidgetId>,
    pub close_id: Option<WidgetId>,
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

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// The current platform / window system does not support custom chrome
    /// at all (e.g. X11) or does not support a specific operation (e.g.
    /// `begin_resize` on macOS, where winit lacks `drag_resize_window`).
    #[error("operation not supported on this platform")]
    Unsupported,
    /// An OS-level call failed. The string is intended for logging, not
    /// programmatic inspection.
    #[error("platform error: {0}")]
    Os(String),
}
