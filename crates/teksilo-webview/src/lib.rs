// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `teksilo-webview` — an embeddable [`WebView`] widget for Teksilo.
//!
//! An engine shows its page in one of two ways, and its backend says which
//! ([`WebViewRendering`]):
//!
//! - **A native subview** (WKWebView, WebView2 and WebKitGTK, through wry): the
//!   engine owns its rendering and lives as an OS view *on top of* the wgpu
//!   pass. The widget keeps it positioned, clipped and hidden; the two
//!   sections below are about this case.
//! - **A surface** (Servo): the engine renders offscreen into a picture the
//!   widget draws inside the pass, like any other content. Clipping, overlays
//!   and dormancy then need no help from the engine, and the widget forwards
//!   pointer, scroll and keyboard input to it ([`WebViewInputEvent`]).
//!
//! Either way the crate mirrors the established platform-backend pattern: a
//! swappable [`WebViewBackend`] creates an engine-specific [`WebViewHandle`],
//! and a per-app [`WebViewRegistry`] (installed in app-state) routes JS→Rust /
//! lifecycle events back to the widget.
//!
//! ```rust
//! # use teksilo_core::signal::Signal;
//! use teksilo_webview::WebView;
//!
//! # let title_signal: Signal<String> = Signal::new(String::new());
//! # let loading_signal: Signal<bool> = Signal::new(false);
//! let _wv = WebView::new()
//!     .url("https://example.com")
//!     .title_signal(title_signal.clone())
//!     .loading_signal(loading_signal.clone())
//!     .on_message(|msg, _ctx| println!("JS said: {msg}"));
//! ```
//!
//! # The Switcher / dormancy caveat
//!
//! Because a native subview lives *outside* the wgpu pass, "not painted"
//! does NOT mean "hidden" for a `WebView`. When a [`Switcher`] /
//! `TabWidget` / `visible_when` gate parks the widget dormant, the framework
//! simply stops painting it — but the native subview keeps floating over the
//! output. `WebView` closes this gap by bridging the framework's per-node
//! **activation signal** (`BuildContext::activation_signal`) to the engine's
//! `set_visible`: tab-away → `set_visible(false)`, tab-back → `set_visible
//! (true)`. This is the one place a widget must explicitly mirror framework
//! visibility onto an OS resource, and it is wired automatically here.
//!
//! # Who owns the pointer over the page
//!
//! A native subview is above the wgpu pass for *input* as well as for pixels:
//! the OS routes a press over its rectangle to the engine, and Teksilo is not
//! told. [`WebViewInput`] is the declaration of which side owns that
//! rectangle, and it decides four things at once — see the enum's docs.
//!
//! [`Switcher`]: https://docs.rs/teksilo-widgets

mod backend;

#[path = "styles/recipe_web_view_style.rs"]
mod recipe_web_view_style;

#[cfg(feature = "wry-backend")]
mod wry_backend;
#[cfg(feature = "wry-backend")]
pub use wry_backend::WryBackend;

#[cfg(feature = "servo-backend")]
mod servo_backend;
#[cfg(feature = "servo-backend")]
pub use servo_backend::ServoBackend;

pub use backend::{
    ConsoleLevel, MEMORY_SURFACE_COLOUR, MemoryWebViewBackend, MemoryWebViewRecords,
    NoopWebViewBackend, WebSource, WebViewAttributes, WebViewBackend, WebViewEditCommand,
    WebViewEvent, WebViewEventPayload, WebViewHandle, WebViewId, WebViewInputEvent, WebViewOp,
    WebViewPumpPayload, WebViewRegistry, WebViewRendering, WebViewTouchPhase, memory_registry,
    memory_surface_registry,
};
pub use recipe_web_view_style::RecipeWebViewStyle;

// Re-export the Tier-3 style surface (the trait lives in teksilo-core so the
// core slot bag can name it, same as every other themable widget).
pub use teksilo_core::styles::{
    SharedWebViewStyle, WebViewStyle, WebViewStyleConfig, WebViewVisualState,
};

/// Whether the process is running under a Wayland session — the signal for
/// choosing the Servo backend (wry's WebKitGTK does X11 reparenting only).
///
/// Mirrors winit's backend selection: an explicit `WINIT_UNIX_BACKEND=wayland|x11`
/// wins (so XWayland forced to X11 correctly reports `false`, where wry works);
/// otherwise a non-empty `WAYLAND_DISPLAY` means Wayland. Always `false` off
/// Linux. Apps that drive engine selection themselves (`install_web_view(...)`)
/// can use this to pick a backend.
pub fn is_wayland() -> bool {
    match std::env::var("WINIT_UNIX_BACKEND") {
        Ok(b) if b.eq_ignore_ascii_case("wayland") => return true,
        Ok(b) if b.eq_ignore_ascii_case("x11") => return false,
        _ => {}
    }
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

/// Pump pending GTK / GLib main-loop events.
///
/// wry's Linux engine (WebKitGTK) lives on the GLib main loop. When the webview
/// is embedded in a winit app (which does not run GTK's loop), the host must
/// pump it each event-loop turn or the page never lays out, paints, or runs
/// timers. Call this from `TeksiloAppBuilder::on_loop_tick` with a poll source
/// held high while any `WebView` is alive.
///
/// No-op off Linux, or without the `wry-backend` engine. Safe to call
/// unconditionally — before `gtk::init()` it does nothing.
#[cfg(all(target_os = "linux", feature = "wry-backend"))]
pub fn pump_gtk_events() {
    if gtk::is_initialized() {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }
}

/// No-op stub on platforms / builds where wry's GTK loop isn't in play.
#[cfg(not(all(target_os = "linux", feature = "wry-backend")))]
pub fn pump_gtk_events() {}

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::live_image::{LiveImageDraw, LiveImageSource, LivePixelFormat};
use teksilo_canvas::{
    Canvas, ImageGeometry, ImageOrientation, Point, Rect, SizeProposal, Transform2D,
};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::Role;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
    WidgetTreeView,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{LiveImageAttachment, LiveImageSignals};

type MessageCallback = Rc<RefCell<dyn FnMut(String, &mut EventContext)>>;
type TitleCallback = Rc<RefCell<dyn FnMut(String, &mut EventContext)>>;
type NavigationCallback = Rc<RefCell<dyn FnMut(NavigationInfo, &mut EventContext)>>;
type PageLoadCallback = Rc<RefCell<dyn FnMut(PageLoadState, &mut EventContext)>>;
type DownloadStartCallback = Rc<RefCell<dyn FnMut(DownloadStart, &mut EventContext)>>;
type DownloadFinishCallback = Rc<RefCell<dyn FnMut(DownloadOutcome, &mut EventContext)>>;

/// Page-load lifecycle phase, passed to [`WebView::on_page_load`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLoadState {
    /// The page began loading resources.
    Started,
    /// The page finished loading.
    Finished,
}

/// A navigation the page initiated, passed to [`WebView::on_navigation`].
///
/// This is an **observer**, not a veto: wry decides navigation synchronously,
/// but Teksilo delivers backend events on a later event-loop tick (events are
/// posted, not delivered inline), so a true pre-navigation veto cannot be
/// surfaced through this callback. `can_cancel` is therefore always `false` on
/// the current backends — the field exists for forward compatibility. Use the
/// callback for URL-bar sync and logging.
#[derive(Debug, Clone)]
pub struct NavigationInfo {
    /// The URL being navigated to.
    pub url: String,
    /// Whether the backend supports vetoing this navigation (always `false`
    /// today — see the type docs).
    pub can_cancel: bool,
}

/// A download the page started, passed to [`WebView::on_download_started`].
///
/// Observational: the destination path is the engine's default and cannot be
/// redirected from the callback (the decision is asynchronous). Use it to drive
/// progress UI / toasts.
#[derive(Debug, Clone)]
pub struct DownloadStart {
    /// Source URL of the download.
    pub url: String,
    /// The engine's chosen destination path.
    pub suggested_path: std::path::PathBuf,
}

/// A finished (or failed) download, passed to [`WebView::on_download_finished`].
#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    /// Where the file was written.
    pub path: std::path::PathBuf,
    /// Whether the download completed successfully.
    pub success: bool,
}

/// Who owns pointer input over the page's rectangle.
///
/// The engine's subview sits above the wgpu surface, so this is not a
/// preference the toolkit can enforce on its own: in [`Native`](Self::Native)
/// the OS hands a press over that rectangle to the engine and Teksilo never
/// sees it, and in [`Transparent`](Self::Transparent) the engine has to be
/// asked to stop taking it ([`WebViewHandle::set_input_passthrough`]) — which
/// not every engine can do.
///
/// A surface engine (see [`WebViewRendering::Surface`]) receives only the
/// input the widget forwards to it, so the declaration is fully Teksilo's to
/// honour there: in `Native` the widget forwards every event over the page and
/// answers it, in `Transparent` it forwards the event and lets it bubble too.
///
/// What Teksilo does on its own side follows from the declaration:
///
/// | | `Native` | `Transparent` |
/// |---|---|---|
/// | `touch_action` over the region | `NONE` | unset (`AUTO`) |
/// | miss-only slop / grip outsets | off (`no_hit_slop`) | as any other widget |
/// | a pointer event that does reach the node | answered, and the pointer's live sequence revoked | declined, so it bubbles |
/// | the engine is asked to pass input through | no | yes |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebViewInput {
    /// The page owns its rectangle: links, form fields, its own scrolling and
    /// its own long-press menus. The default, and what a browser-shaped view
    /// wants.
    ///
    /// Teksilo therefore claims nothing over the region. `touch_action(NONE)`
    /// stops a pan, a pinch or a tree-owned hold forming on the hit path (the
    /// page scrolls itself; an enclosing `ScrollArea` must not also move under
    /// the finger), and `no_hit_slop` makes the painted rectangle the exact
    /// contract in both directions — no neighbouring control may claim a press
    /// that landed on the page, and the page claims none that missed it.
    #[default]
    Native,
    /// Teksilo owns the rectangle; the page is a display surface.
    ///
    /// For a view that renders rather than interacts — a document preview, a
    /// rendered chart, a kiosk banner — and the mode to reach for when app
    /// widgets, menus or a dialog have to be operable *over* the page: with the
    /// engine passing input through, an overlay above the view receives the tap
    /// instead of the engine swallowing it.
    ///
    /// The engine half is a request, not a guarantee. A backend that cannot
    /// make its surface input-transparent reports so as a
    /// [`WebViewEvent::ConsoleMessage`]; the Teksilo half (no `touch_action`
    /// declaration, ordinary hit widening, pointer events declined so they
    /// bubble) applies either way.
    Transparent,
}

impl WebViewInput {
    /// Whether the engine owns pointer input over the page.
    pub fn is_native(self) -> bool {
        matches!(self, WebViewInput::Native)
    }
}

/// Why the engine subview is hidden, if it is.
///
/// Three independent reasons, resolved into one `set_visible` call so the
/// engine is never told a visibility that only accounts for one of them: a
/// `WebView` parked in an unselected tab AND scrolled out of view must not
/// reappear when only the scroll changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EngineVisibility {
    /// The framework's per-node activation (a `Switcher` branch, a tab).
    active: bool,
    /// Whether any of the widget's bounds survives its clipping ancestors.
    in_view: bool,
    /// Whether an interactive overlay is standing over the page.
    uncovered: bool,
}

impl EngineVisibility {
    const VISIBLE: Self = Self {
        active: true,
        in_view: true,
        uncovered: true,
    };

    fn resolved(self) -> bool {
        self.active && self.in_view && self.uncovered
    }
}

/// The picture a [`WebViewRendering::Surface`] engine draws into, and where
/// the widget shows it.
///
/// Made once, by the first `build` that finds a surface backend, and kept
/// across rebuilds: the engine holds its writer for the widget's lifetime.
struct SurfaceView {
    source: LiveImageSource,
    signals: LiveImageSignals,
    /// How the engine lays its rows out in the picture.
    orientation: ImageOrientation,
    /// This build's attachment; every `build` attaches anew.
    attachment: RefCell<Option<LiveImageAttachment>>,
    /// Where the picture lies, widget-local, as the last layout placed it:
    /// what `post_paint` draws and input maps through.
    geometry: Cell<Option<ImageGeometry>>,
    /// The touch and pen contacts currently down on the page, so a move is
    /// told apart from a hover.
    contacts: RefCell<Vec<teksilo_core::pointer::PointerId>>,
    /// The page pixel the pointer was last forwarded at: where a scroll with
    /// no position of its own lands, and where a cancelled contact ends.
    last_pointer: Cell<Option<Point>>,
}

impl SurfaceView {
    fn new(orientation: ImageOrientation) -> Self {
        // Opaque: an engine composites its page over its own background.
        let source = LiveImageSource::builder(LivePixelFormat::Rgbx8)
            .label("web view")
            .build();
        Self {
            signals: LiveImageSignals::new(&source),
            source,
            orientation,
            attachment: RefCell::new(None),
            geometry: Cell::new(None),
            contacts: RefCell::new(Vec::new()),
            last_pointer: Cell::new(None),
        }
    }

    /// Where the picture lies in a box of `size`, at `scale` device pixels per
    /// logical pixel: unscaled, one frame pixel per device pixel, anchored at
    /// the box's top-left. While a resize is on its way to the engine the
    /// previous frame keeps its size rather than stretching, as a browser
    /// shows it.
    fn place(&self, size: teksilo_canvas::Size, scale: f32) -> Option<ImageGeometry> {
        let (w, h) = self.signals.frame_size.get()?;
        let (dw, dh) = self.orientation.displayed_size(w, h);
        let content = Rect::new(0.0, 0.0, dw as f32 / scale, dh as f32 / scale);
        let bounds = Rect::new(0.0, 0.0, size.width, size.height);
        Some(ImageGeometry::new(
            (w, h),
            self.orientation,
            content,
            bounds,
        ))
    }

    /// The page pixel under `local`, a widget-local point: the frame as
    /// displayed, origin at its top-left, whatever order its rows were
    /// written in. `None` before the first frame.
    fn page_point(&self, local: Point) -> Option<Point> {
        let placed = self.geometry.get()?;
        let upright = ImageGeometry::new(
            placed.displayed(),
            ImageOrientation::Normal,
            placed.content,
            placed.bounds,
        );
        upright
            .map_to_source_f32(local)
            .map(|(x, y)| Point::new(x, y))
    }
}

/// Shared slot holding the live engine handle once opened. Cloned into the
/// activation-signal effect so the visibility bridge can reach the handle
/// created later in `build`.
type SharedHandle = Rc<RefCell<Option<Box<dyn WebViewHandle>>>>;

/// An embeddable web view. Composing widget: it delegates layout/paint to a
/// style-built overlay and drives a native engine subview on top.
///
/// See the [crate docs](crate) for the dormancy/visibility contract.
pub struct WebView {
    attrs: WebViewAttributes,
    web_view_id: WebViewId,
    handle: SharedHandle,
    /// Shared with the post-mount open action so it can apply the first
    /// `set_bounds` immediately after the engine opens.
    last_bounds: Rc<Cell<Option<Rect>>>,
    /// Host window HiDPI scale, read from `LayoutContext::scale_factor` in
    /// `place_children`. Handed to the backend's `set_bounds` so engines that
    /// position in device pixels (WebKitGTK on X11) land correctly under
    /// fractional scaling. Shared so the post-mount open closure can read it.
    /// Defaults to 1.0 until the first layout.
    scale: Rc<Cell<f32>>,
    /// Guards `run_after_mount` enqueue against rebuilds (queue at most once).
    mount_queued: Cell<bool>,
    /// Window id captured from `BuildContext::window()` (the post-mount
    /// `EventContext` has no direct window-id accessor).
    window_id: Cell<Option<TeksiloWindowId>>,
    /// This widget's own arena id, captured in `build`. `place_children` needs
    /// it to walk its clipping ancestors, and the engine-focus event needs it
    /// to move the toolkit's focus onto the frame.
    self_id: Cell<Option<WidgetId>>,
    style_override: Option<SharedWebViewStyle>,
    root_child_id: Option<WidgetId>,
    /// Internal lifecycle state driving the overlay chrome.
    state_signal: Signal<WebViewVisualState>,
    /// Registry handle, written by the post-mount open action and read by
    /// `Drop` for unregistration. Shared so the moved open closure can set it.
    registry: Rc<RefCell<Option<WebViewRegistry>>>,
    /// Whether the Teksilo-side node holds keyboard focus — i.e. the *frame*
    /// is focused, which is not the same as the page having been entered.
    /// Drives the style's focus ring so a keyboard user can see where Tab
    /// landed even though the widget paints no content of its own.
    focused: Signal<bool>,
    /// Hand keyboard focus straight to the engine the moment the frame gains
    /// focus, instead of waiting for Enter. Off by default — see
    /// [`enter_page_on_focus`](Self::enter_page_on_focus).
    enter_page_on_focus: bool,
    /// Who owns pointer input over the page's rectangle. See [`WebViewInput`].
    input: WebViewInput,
    /// Whether the *page* holds the engine's keyboard focus, as the engine
    /// reports it. Distinct from [`focused`](Self::focused), which is the
    /// toolkit's own focus on the frame.
    page_focused: Signal<bool>,
    /// The three reasons the subview may be hidden, resolved into one
    /// `set_visible`. Shared with the post-mount open action and the
    /// activation effect.
    visibility: Rc<Cell<EngineVisibility>>,
    /// The last `set_visible` value actually issued, so a layout pass that
    /// changes nothing issues nothing.
    visible_applied: Rc<Cell<bool>>,
    /// The picture, when the installed backend renders into one. `None` for a
    /// native subview.
    surface: Option<Rc<SurfaceView>>,

    // Optional bindings.
    /// Two-way: the engine writes the resolved URL on navigation-finish, and
    /// an external `.set()` drives programmatic navigation (guarded against
    /// the echo via `nav_guard`).
    url_signal: Option<Signal<String>>,
    title_signal: Option<Signal<String>>,
    loading_signal: Option<Signal<bool>>,
    // NOTE: can-go-back / can-go-forward bindings are intentionally absent
    // until a history-aware backend can drive them — shipping builders that
    // never update the bound signal would be a silent lie. Re-add alongside
    // the wry/servo history wiring.
    /// The URL the engine last reported / we last drove, so the inbound
    /// navigation effect skips the engine's own echo (no navigate loop).
    nav_guard: Rc<RefCell<Option<String>>>,

    // User event callbacks.
    on_message: Option<MessageCallback>,
    on_title_changed: Option<TitleCallback>,
    on_navigation: Option<NavigationCallback>,
    on_page_load: Option<PageLoadCallback>,
    on_download_started: Option<DownloadStartCallback>,
    on_download_finished: Option<DownloadFinishCallback>,
}

impl std::fmt::Debug for WebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebView")
            .field("web_view_id", &self.web_view_id)
            .field("opened", &self.handle.borrow().is_some())
            .field("source", &self.attrs.source)
            .finish_non_exhaustive()
    }
}

impl Default for WebView {
    fn default() -> Self {
        Self::new()
    }
}

impl WebView {
    /// A blank web view. Set content with [`url`](Self::url) /
    /// [`html`](Self::html) / [`source`](Self::source).
    pub fn new() -> Self {
        Self {
            attrs: WebViewAttributes::default(),
            web_view_id: WebViewId::next(),
            handle: Rc::new(RefCell::new(None)),
            last_bounds: Rc::new(Cell::new(None)),
            scale: Rc::new(Cell::new(1.0)),
            mount_queued: Cell::new(false),
            window_id: Cell::new(None),
            self_id: Cell::new(None),
            style_override: None,
            root_child_id: None,
            state_signal: Signal::new(WebViewVisualState::Loading),
            registry: Rc::new(RefCell::new(None)),
            focused: Signal::new(false),
            enter_page_on_focus: false,
            input: WebViewInput::default(),
            page_focused: Signal::new(false),
            visibility: Rc::new(Cell::new(EngineVisibility::VISIBLE)),
            visible_applied: Rc::new(Cell::new(true)),
            surface: None,
            url_signal: None,
            title_signal: None,
            loading_signal: None,
            nav_guard: Rc::new(RefCell::new(None)),
            on_message: None,
            on_title_changed: None,
            on_navigation: None,
            on_page_load: None,
            on_download_started: None,
            on_download_finished: None,
        }
    }

    /// Navigate to a URL on first open.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.attrs.source = Some(WebSource::Url(url.into()));
        self
    }

    /// Load inline HTML on first open.
    pub fn html(mut self, html: impl Into<String>) -> Self {
        self.attrs.source = Some(WebSource::Html {
            html: html.into(),
            base_url: None,
        });
        self
    }

    /// Set the initial content from a [`WebSource`].
    pub fn source(mut self, source: WebSource) -> Self {
        self.attrs.source = Some(source);
        self
    }

    /// Override the engine `User-Agent`.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.attrs.user_agent = Some(ua.into());
        self
    }

    /// Request a transparent engine background.
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.attrs.transparent = transparent;
        self
    }

    /// Enable engine devtools (debug builds, by convention).
    pub fn devtools(mut self, devtools: bool) -> Self {
        self.attrs.devtools = devtools;
        self
    }

    /// Register a custom-protocol scheme name (`"app"` → `app://`). The
    /// dispatch closure lives app-side; the backend only needs the name.
    pub fn custom_protocol(mut self, scheme: impl Into<String>) -> Self {
        self.attrs.custom_protocols.push(scheme.into());
        self
    }

    /// Two-way URL binding. The engine writes the resolved URL into `signal`
    /// when an in-page navigation completes; calling `signal.set("…")`
    /// externally drives programmatic navigation (equivalent to
    /// [`load_url`](Self::load_url)). The engine's own echo is filtered, so
    /// the two directions don't loop.
    ///
    /// The **initial** page still comes from [`url`](Self::url) /
    /// [`html`](Self::html) / [`source`](Self::source); `url_signal` governs
    /// navigation *after* the first load (the signal's value at build time is
    /// taken as the baseline and does not trigger a navigation).
    pub fn url_signal(mut self, signal: Signal<String>) -> Self {
        self.url_signal = Some(signal);
        self
    }

    /// Bind the page title (read-only — updated on `TitleChanged`).
    pub fn title_signal(mut self, signal: Signal<String>) -> Self {
        self.title_signal = Some(signal);
        self
    }

    /// Bind the loading flag (read-only — true between page-load start/finish).
    pub fn loading_signal(mut self, signal: Signal<bool>) -> Self {
        self.loading_signal = Some(signal);
        self
    }

    /// JS → Rust: called when the page runs `window.ipc.postMessage(...)`.
    pub fn on_message(mut self, cb: impl FnMut(String, &mut EventContext) + 'static) -> Self {
        self.on_message = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Called when the document title changes.
    pub fn on_title_changed(mut self, cb: impl FnMut(String, &mut EventContext) + 'static) -> Self {
        self.on_title_changed = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Called when a navigation starts (observer — see [`NavigationInfo`]; it
    /// cannot veto). Useful for URL-bar sync before the load completes.
    pub fn on_navigation(
        mut self,
        cb: impl FnMut(NavigationInfo, &mut EventContext) + 'static,
    ) -> Self {
        self.on_navigation = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Called when page loading starts and finishes (see [`PageLoadState`]).
    pub fn on_page_load(
        mut self,
        cb: impl FnMut(PageLoadState, &mut EventContext) + 'static,
    ) -> Self {
        self.on_page_load = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Called when the page begins a download (see [`DownloadStart`]).
    pub fn on_download_started(
        mut self,
        cb: impl FnMut(DownloadStart, &mut EventContext) + 'static,
    ) -> Self {
        self.on_download_started = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Called when a download finishes or fails (see [`DownloadOutcome`]).
    pub fn on_download_finished(
        mut self,
        cb: impl FnMut(DownloadOutcome, &mut EventContext) + 'static,
    ) -> Self {
        self.on_download_finished = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Per-call style override (highest precedence).
    pub fn style(mut self, style: impl WebViewStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Enter the page as soon as the frame receives keyboard focus, rather
    /// than on Enter (the default two-step).
    ///
    /// Only appropriate when the web view *is* the window's content and there
    /// is nothing else in the Tab cycle worth reaching — a kiosk view, a
    /// full-window document preview. In a mixed UI it makes Tab a one-way
    /// door: once the engine owns the keyboard, Teksilo sees no more keys and
    /// getting back out is up to the engine and the OS. Off by default for
    /// exactly that reason.
    pub fn enter_page_on_focus(mut self, enter: bool) -> Self {
        self.enter_page_on_focus = enter;
        self
    }

    /// Declare who owns pointer input over the page's rectangle.
    ///
    /// [`WebViewInput::Native`] — the default — gives it to the engine;
    /// [`WebViewInput::Transparent`] keeps it for Teksilo. See
    /// [`WebViewInput`] for everything the choice decides.
    pub fn input_mode(mut self, input: WebViewInput) -> Self {
        self.input = input;
        self
    }

    /// Whether the *page* currently holds the engine's keyboard focus.
    ///
    /// Written from [`WebViewEvent::EngineFocusChanged`], which is the only
    /// thing that can know: once the native subview owns the keyboard, the
    /// toolkit is told nothing more about what happens inside it. Distinct
    /// from [`focused_signal`](Self::focused_signal), which reports Teksilo's
    /// own focus on the *frame*.
    pub fn page_focused_signal(&self) -> Signal<bool> {
        self.page_focused.clone()
    }

    /// Hand keyboard focus to the engine subview, entering the page.
    ///
    /// The programmatic form of the frame's Enter key. No-op before the engine
    /// has opened (the handle is created post-mount).
    pub fn focus_page(&self) {
        self.with_handle(|h| h.set_focus());
    }

    /// Whether the Teksilo-side frame currently holds keyboard focus.
    ///
    /// True while Tab has landed *on* the web view; it says nothing about
    /// whether the page has been entered, because once the engine subview
    /// owns the keyboard the toolkit is no longer told what happens inside it.
    pub fn focused_signal(&self) -> Signal<bool> {
        self.focused.clone()
    }

    /// The stable routing identity of this web view.
    pub fn id(&self) -> WebViewId {
        self.web_view_id
    }

    // --- Imperative controls (call via `ctx.with_widget_mut::<WebView>`) ---

    /// Navigate to `url`.
    pub fn load_url(&self, url: &str) {
        self.with_handle(|h| h.load_url(url));
    }
    /// Rust → JS: dispatch a `teksilo-message` event carrying `msg`.
    pub fn post_message(&self, msg: &str) {
        self.with_handle(|h| h.post_message(msg));
    }
    /// Evaluate JavaScript in the page.
    pub fn eval(&self, script: &str) {
        self.with_handle(|h| h.eval(script));
    }
    /// Reload the page.
    pub fn reload(&self) {
        self.with_handle(|h| h.reload());
    }
    /// Navigate back.
    pub fn go_back(&self) {
        self.with_handle(|h| h.go_back());
    }
    /// Navigate forward.
    pub fn go_forward(&self) {
        self.with_handle(|h| h.go_forward());
    }
    /// Stop the current load.
    pub fn stop(&self) {
        self.with_handle(|h| h.stop());
    }
    /// Open the engine's developer tools (no-op where unsupported, e.g. Servo).
    pub fn open_devtools(&self) {
        self.with_handle(|h| h.open_devtools());
    }
    /// Close the engine's developer tools.
    pub fn close_devtools(&self) {
        self.with_handle(|h| h.close_devtools());
    }

    /// The part of `bounds` that survives every `clips_children` ancestor, or
    /// `None` when nothing does.
    ///
    /// Walks the arena rather than reading `PaintContext::clip_bounds` because
    /// the paint walker skips a subtree it has clipped away entirely — which is
    /// exactly the case that has to reach the engine.
    fn visible_rect(&self, bounds: Rect, ctx: &LayoutContext) -> Option<Rect> {
        let (Some(arena), Some(id)) = (ctx.arena(), self.self_id.get()) else {
            return Some(bounds);
        };
        let mut rect = bounds;
        let mut cursor = arena.parent(id);
        while let Some(ancestor) = cursor {
            if arena.get(ancestor).is_some_and(|node| node.clips_children) {
                rect = intersect(rect, arena.bounds(ancestor))?;
            }
            cursor = arena.parent(ancestor);
        }
        Some(rect)
    }

    /// Place a surface page's picture for this layout: unscaled, its edges on
    /// the device-pixel grid through the ancestors' transforms, recorded on
    /// the attachment for automation and kept for paint and input.
    fn place_surface(&self, surface: &SurfaceView, bounds: Rect, scale: f32, ctx: &LayoutContext) {
        let scale = usable_scale(scale);
        let ancestors = ctx
            .arena()
            .zip(self.self_id.get())
            .map(|(arena, id)| arena.effective_transform(id))
            .unwrap_or(Transform2D::IDENTITY);
        let to_device = ancestors.then(&Transform2D::scale(scale, scale));
        let placed = surface
            .place(bounds.size(), scale)
            .map(|g| g.snapped(Transform2D::translate(bounds.x, bounds.y).then(&to_device)));
        if let Some(attachment) = surface.attachment.borrow().as_ref() {
            attachment.set_snapped_under(Some(ancestors));
            if let Some(g) = placed {
                attachment.set_geometry(g);
            }
        }
        surface.geometry.set(placed);
    }

    fn with_handle(&self, f: impl FnOnce(&dyn WebViewHandle)) {
        if let Some(h) = self.handle.borrow().as_ref() {
            f(h.as_ref());
        }
    }

    /// Build the JS→Rust / lifecycle event callback handed to the registry.
    fn make_event_callback(
        &self,
        self_id: WidgetId,
    ) -> impl FnMut(WebViewEvent, &mut EventContext) + 'static {
        let page_focused = self.page_focused.clone();
        let url_signal = self.url_signal.clone();
        let title_signal = self.title_signal.clone();
        let loading_signal = self.loading_signal.clone();
        let state_signal = self.state_signal.clone();
        let nav_guard = self.nav_guard.clone();
        let on_message = self.on_message.clone();
        let on_title_changed = self.on_title_changed.clone();
        let on_navigation = self.on_navigation.clone();
        let on_page_load = self.on_page_load.clone();
        let on_download_started = self.on_download_started.clone();
        let on_download_finished = self.on_download_finished.clone();

        move |event, ctx| match event {
            WebViewEvent::PageLoadStarted => {
                if let Some(s) = &loading_signal {
                    s.set(true);
                }
                state_signal.set(WebViewVisualState::Loading);
                if let Some(cb) = &on_page_load {
                    (cb.borrow_mut())(PageLoadState::Started, ctx);
                }
            }
            WebViewEvent::PageLoadFinished => {
                if let Some(s) = &loading_signal {
                    s.set(false);
                }
                state_signal.set(WebViewVisualState::Ready);
                if let Some(cb) = &on_page_load {
                    (cb.borrow_mut())(PageLoadState::Finished, ctx);
                }
            }
            WebViewEvent::NavigationStarted { url, can_cancel } => {
                if let Some(cb) = &on_navigation {
                    (cb.borrow_mut())(NavigationInfo { url, can_cancel }, ctx);
                }
            }
            WebViewEvent::NavigationFinished { url, success } => {
                if success {
                    // Record the engine-resolved URL as the guard BEFORE
                    // writing the bound signal, so the inbound navigation
                    // effect (which fires on the `set`) recognises it as the
                    // engine's own echo and does not re-navigate.
                    *nav_guard.borrow_mut() = Some(url.clone());
                    if let Some(s) = &url_signal {
                        s.set(url);
                    }
                    state_signal.set(WebViewVisualState::Ready);
                } else {
                    state_signal.set(WebViewVisualState::Error);
                }
            }
            WebViewEvent::TitleChanged(title) => {
                if let Some(s) = &title_signal {
                    s.set(title.clone());
                }
                if let Some(cb) = &on_title_changed {
                    (cb.borrow_mut())(title, ctx);
                }
            }
            WebViewEvent::Message(msg) => {
                if let Some(cb) = &on_message {
                    (cb.borrow_mut())(msg, ctx);
                }
            }
            WebViewEvent::DownloadStarted {
                url,
                suggested_path,
            } => {
                if let Some(cb) = &on_download_started {
                    (cb.borrow_mut())(
                        DownloadStart {
                            url,
                            suggested_path,
                        },
                        ctx,
                    );
                }
            }
            WebViewEvent::DownloadFinished { path, success } => {
                if let Some(cb) = &on_download_finished {
                    (cb.borrow_mut())(DownloadOutcome { path, success }, ctx);
                }
            }
            WebViewEvent::ConsoleMessage { .. } => {
                // Diagnostics only (backend init / unsupported-op reports);
                // not surfaced to a dedicated app callback today.
            }
            WebViewEvent::EngineFocusChanged(has_focus) => {
                page_focused.set(has_focus);
                // The OS has moved the keyboard into the page. Teksilo's own
                // focus must follow, or whatever held it — a text field, with
                // a blinking caret and an open IME — goes on believing it
                // still does. Moving it onto the frame is the honest answer:
                // the frame is the deepest node Teksilo owns, and the page's
                // own focus ring lives in a tree the toolkit cannot see.
                //
                // Guarded, because the two-step entry path (Enter on the frame
                // → `set_focus`) arrives here with the frame already focused,
                // and a redundant focus request would re-run the whole focus
                // machinery on every engine focus event.
                if has_focus && ctx.focused() != Some(self_id) {
                    ctx.request_focus(self_id);
                }
            }
        }
    }
}

impl Widget for WebView {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();

        // --- Tier-3 chrome: resolve style (per-call > theme slot > default) ---
        let style = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.web_view.clone())
            .unwrap_or_else(|| Rc::new(RecipeWebViewStyle));

        // Empty overlay content placeholder (apps install a richer overlay
        // via a custom WebViewStyle). Keeps the default body self-contained.
        let content = ctx.add(EmptyOverlayContent);
        let body = style.make_body(
            &WebViewStyleConfig {
                state: self.state_signal.clone(),
                focused: self.focused.clone(),
                content,
            },
            ctx,
        );
        self.root_child_id = Some(body);

        self.self_id.set(Some(self_id));

        // --- A surface engine: make its picture, once, and attach it ---
        //
        // The backend is asked now rather than at open, because the picture
        // has to be attached in `build` (and in every `build`: a rebuild
        // detaches it) and handed to the engine when it opens.
        let rendering = ctx
            .app_state::<WebViewRegistry>()
            .map(WebViewRegistry::rendering)
            .unwrap_or(WebViewRendering::NativeSubview);
        if let WebViewRendering::Surface { orientation } = rendering {
            let surface = self
                .surface
                .get_or_insert_with(|| Rc::new(SurfaceView::new(orientation)))
                .clone();
            let attachment = ctx.attach_live_image(&surface.source, &surface.signals);
            *surface.attachment.borrow_mut() = Some(attachment);
            let registry = ctx.binding_registry();
            // A new frame size places the picture anew; a status change only
            // repaints. A commit of pixels alone touches neither.
            surface.signals.frame_size.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::Relayout,
            );
            surface.signals.status.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // --- Keyboard: put the frame in the Tab cycle, then let Enter in ---
        //
        // The page's own focus ring lives in the engine's tree, not ours, so a
        // web view that is not focusable is simply unreachable without a mouse
        // (WCAG 2.1.1 / 2.4.3). Making the *frame* focusable is the first half.
        //
        // The second half is deliberately a **two-step**: landing on the frame
        // does not hand the keyboard to the engine, Enter (or Space) does. A
        // web view has two disjoint focus rings — AccessKit's and the engine's
        // platform tree — and once the native subview owns the keyboard the
        // toolkit stops seeing keys entirely, so an automatic hand-off would
        // turn Tab into a one-way door out of the app's own focus cycle. The
        // same reasoning the HTML `<iframe>` / canvas-embed pattern arrives at.
        // Apps whose web view is the whole window can opt into the one-step
        // form with `enter_page_on_focus(true)`.
        let focused = self.focused.clone();
        let focus_handle = self.handle.clone();
        let enter_on_focus = self.enter_page_on_focus;
        let mut handlers = teksilo_core::widget_builder::HandlerSet::new()
            .focusable(true)
            .on_focus(move |gained, _ctx| {
                focused.set(gained);
                if gained
                    && enter_on_focus
                    && let Some(h) = focus_handle.borrow().as_ref()
                {
                    h.set_focus();
                }
            });

        let key_handle = self.handle.clone();
        let key_page_focused = self.page_focused.clone();
        let key_forwards = self.surface.is_some();
        handlers = handlers.on_key(move |event, _ctx| {
            use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
            // A surface engine has no native view for the OS to send keys to:
            // while the page holds the keyboard, they go to it from here.
            // Ctrl+Tab and Ctrl+Shift+Tab stay Teksilo's, so the page is never
            // a keyboard trap (WCAG 2.1.2).
            if key_forwards
                && key_page_focused.get()
                && let Some(input) = forwarded_key(event)
                && let Some(h) = key_handle.borrow().as_ref()
            {
                h.send_input(&input);
                return EventResponse::Handled;
            }
            // Enter / Space enter the page. Everything else — Tab included —
            // is declined, so the frame never becomes a trap: focus cycles off
            // it exactly as it would off any other control.
            if let WidgetEvent::KeyDown { key, modifiers, .. } = event
                && matches!(key, Key::Enter | Key::Space)
                && *modifiers == Modifiers::NONE
                && let Some(h) = key_handle.borrow().as_ref()
            {
                h.set_focus();
                return EventResponse::Handled;
            }
            EventResponse::Ignored
        });

        // The advertised `Click` needs something behind it: an action a widget
        // declares but does not execute is worse than one it never declared,
        // because AT reports the control as operable when it is not.
        let action_handle = self.handle.clone();
        handlers = handlers.on_access_action(move |action, _ctx| {
            use teksilo_core::event::EventResponse;
            if matches!(
                action,
                teksilo_core::accesskit::Action::Click | teksilo_core::accesskit::Action::Focus
            ) && let Some(h) = action_handle.borrow().as_ref()
            {
                h.set_focus();
                return EventResponse::Handled;
            }
            EventResponse::Ignored
        });

        // --- Who owns the pointer over the page ---
        //
        // In `Native` mode the engine does, and the two declarations below say
        // so to the framework: no default touch behaviour may form on the hit
        // path (`TouchAction::NONE`), and the painted rectangle is the exact
        // contract in both directions (`no_hit_slop`). The handler closes the
        // third gap — a pointer Teksilo *does* see over the page, which is a
        // pointer it will stop seeing samples for the moment the engine takes
        // it. Leaving that interaction alive strands whatever it belonged to:
        // an arbitration waiting for movement that never arrives, a press
        // record waiting for an Up the OS will deliver to the page instead.
        //
        // In `Transparent` mode none of this applies: Teksilo owns the region,
        // so the node widens and bubbles like any other widget and the engine
        // is asked to keep its hands off.
        //
        // One honest note on `Handled` below: it is the correct statement that
        // the page consumed the event, but it is **not** what keeps an ancestor
        // out of the press — the revocation is. An ancestor's own
        // `on_pointer_event` fires in the *preview* pass, before the target's,
        // and is unreachable from here; its recognizers are denied by the
        // cancel. Measured: returning `Ignored` instead leaves every test in
        // `tests/input_and_clip.rs` green.
        if let Some(surface) = self.surface.clone() {
            handlers = forward_surface_pointer(handlers, surface, self.handle.clone(), self.input);
        } else if self.input.is_native() {
            use teksilo_core::event::EventResponse;
            use teksilo_core::pointer::CancelReason;
            use teksilo_core::pointer::touch_action::TouchAction;

            handlers = handlers
                .touch_action(TouchAction::NONE)
                .no_hit_slop()
                .on_pointer_event(move |event, ctx| {
                    use teksilo_core::event::WidgetEvent;
                    match event {
                        WidgetEvent::PointerDown { .. } | WidgetEvent::PointerUp { .. } => {
                            // `Deactivated` is the taxonomy's explicit
                            // catch-all, and it is what this is: the pointer
                            // was not revoked by the platform, by a peer or by
                            // a modal — an embedded native surface simply owns
                            // it from here on. See `docs/web-view.md`.
                            ctx.cancel_pointer_sequence(CancelReason::Deactivated);
                            EventResponse::Handled
                        }
                        WidgetEvent::PointerMove { .. } => {
                            if ctx.press_is_inside() {
                                ctx.cancel_pointer_sequence(CancelReason::Deactivated);
                            }
                            EventResponse::Handled
                        }
                        // Hover transitions are left to bubble: a hover-owner
                        // change is how ancestors keep their `hover_within`
                        // chains honest, and swallowing one buys nothing.
                        _ => EventResponse::Ignored,
                    }
                });
        }

        ctx.apply_self_handlers(handlers);

        // Capture the window id now — the post-mount EventContext has no
        // direct window-id accessor, but BuildContext::window() does.
        self.window_id.set(ctx.window().map(|w| w.id()));

        // --- Display scale: a scale change → `place_children` → set_bounds ---
        // A scale change with an unchanged logical size relayouts nothing on
        // its own, and the engine positions in device pixels on some
        // toolkits: rerun `place_children` when the window's scale changes.
        ctx.device_scale_signal().bind_to(
            self_id,
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::Relayout,
        );

        // --- Visibility bridge: framework activation → engine set_visible ---
        // The single reason this widget needs the activation signal: a native
        // subview ignores the wgpu paint pass, so a Switcher parking us
        // dormant would otherwise leave the engine surface visible. The effect
        // no-ops until the engine handle exists (opened post-mount below).
        let vis = ctx.activation_signal(self_id);
        let effect_handle = self.handle.clone();
        let effect_visibility = self.visibility.clone();
        let effect_applied = self.visible_applied.clone();
        ctx.effect(&vis, move |active| {
            let mut state = effect_visibility.get();
            state.active = *active;
            effect_visibility.set(state);
            apply_visibility(&effect_handle, &effect_visibility, &effect_applied);
        });

        // --- Inbound navigation: external `url_signal.set()` → load_url ---
        // Seed the guard with the signal's current value so the effect's
        // registration tick (it fires immediately with the current value) is
        // treated as the baseline and does NOT navigate — the initial page
        // comes from `attrs.source`, not the binding. Subsequent external
        // changes that differ from the guard drive a navigation; the engine's
        // own echo is filtered because `NavigationFinished` updates the guard
        // before writing the signal.
        if let Some(url_signal) = self.url_signal.clone() {
            *self.nav_guard.borrow_mut() = Some(url_signal.get());
            let nav_guard = self.nav_guard.clone();
            let nav_handle = self.handle.clone();
            ctx.effect(&url_signal, move |url| {
                if nav_guard.borrow().as_deref() == Some(url.as_str()) {
                    return;
                }
                *nav_guard.borrow_mut() = Some(url.clone());
                if let Some(h) = nav_handle.borrow().as_ref() {
                    h.load_url(url);
                }
            });
        }

        // --- Open the native engine subview once, AFTER mount ---
        // Opening is deferred to a post-mount EventContext because that is the
        // only place a widget can read the OS parent window handle
        // (`ctx.parent_window_handle()`) together with `app_state` + `poster`
        // — exactly what a real engine's `build_as_child(parent)` needs.
        if !self.mount_queued.get() {
            self.mount_queued.set(true);
            let web_view_id = self.web_view_id;
            let window_id = self.window_id.get();
            let attrs = self.attrs.clone();
            let handle_slot = self.handle.clone();
            let bounds_slot = self.last_bounds.clone();
            let scale_slot = self.scale.clone();
            let registry_slot = self.registry.clone();
            let activation = vis;
            let on_event = self.make_event_callback(self_id);
            let input = self.input;
            let visibility = self.visibility.clone();
            let visible_applied = self.visible_applied.clone();
            let surface = self.surface.clone();

            ctx.run_after_mount(move |ectx| {
                // Guard against a double-open if a rebuild ever re-queues.
                if handle_slot.borrow().is_some() {
                    return;
                }
                let Some(registry) = ectx.app_state::<WebViewRegistry>().cloned() else {
                    // No engine configured (install_web_view not called) —
                    // the widget renders just its overlay chrome.
                    return;
                };
                let parent = ectx.parent_window_handle();
                let poster = ectx.poster().cloned();
                let wid = window_id.unwrap_or_else(|| TeksiloWindowId::new(0));

                let writer = surface.as_ref().map(|s| s.source.writer());
                let handle =
                    registry.open(web_view_id, wid, parent, attrs, poster, writer, on_event);
                // Apply the bounds layout already resolved, then the current
                // activation state (so a view mounted while its tab is parked
                // opens hidden, not visible-then-flashing).
                if let Some(b) = bounds_slot.get() {
                    handle.set_bounds(b, scale_slot.get());
                }
                // A surface engine takes no input but what the widget forwards,
                // so there is nothing for it to pass through.
                if input == WebViewInput::Transparent && surface.is_none() {
                    handle.set_input_passthrough(true);
                }

                *handle_slot.borrow_mut() = Some(handle);
                *registry_slot.borrow_mut() = Some(registry);
                // The engine subview opens visible, so only a hidden target is
                // issued: a view mounted while its tab is parked, or already
                // scrolled out of its viewport, must be hidden at birth rather
                // than flashing once. `visible_applied` starts `true` for
                // exactly that reason, so an ordinary active open issues
                // nothing at all.
                let mut state = visibility.get();
                state.active = activation.get();
                visibility.set(state);
                apply_visibility(&handle_slot, &visibility, &visible_applied);
            });
        }

        self.children()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
        // Mirror the new bounds onto the native subview. The bounds are logical;
        // `ctx.scale_factor` is the host window's HiDPI device scale (`build`
        // binds `device_scale_signal` at `Relayout`, so a scale change re-runs
        // this). The backend uses
        // both: engines that position in device pixels (WebKitGTK on X11) need
        // logical × scale. Store the scale so the post-mount open path can apply
        // the first bounds at the right scale.
        let scale = ctx.scale_factor;
        let scale_changed = (self.scale.get() - scale).abs() > f32::EPSILON;
        if scale_changed {
            self.scale.set(scale);
        }

        // The rectangle the engine may occupy is not this widget's bounds — it
        // is what survives every clipping ancestor. A subview is parented to
        // the top-level window, so nothing clips it for us: a `WebView` inside
        // a scrolled `ScrollArea` would otherwise keep the page painted over
        // whatever sits outside the viewport, at full size, for as long as it
        // stayed mounted.
        //
        // Mirroring the *intersection* is the only geometric channel there is
        // (`set_bounds` positions and sizes; no engine here exposes a clip
        // region), so a partially-clipped page is laid out to the visible strip
        // rather than cropped to it, and one clipped away entirely is hidden.
        let visible = self.visible_rect(bounds, ctx);
        let mut state = self.visibility.get();
        state.in_view = visible.is_some();
        self.visibility.set(state);

        // A surface page is drawn inside the pass, so the canvas clips it and
        // the engine is given the whole box: a page half-scrolled out of a
        // viewport keeps its layout instead of reflowing to the strip.
        let target = match &self.surface {
            Some(surface) => {
                self.place_surface(surface, bounds, scale, ctx);
                Some(bounds)
            }
            None => visible,
        };
        if let Some(rect) = target
            && (self.last_bounds.get() != Some(rect) || scale_changed)
        {
            self.last_bounds.set(Some(rect));
            self.with_handle(|h| h.set_bounds(rect, scale));
        }
        apply_visibility(&self.handle, &self.visibility, &self.visible_applied);
    }

    fn wants_after_paint(&self) -> bool {
        // Only a native subview has to stand down for an overlay; a surface
        // page is drawn under overlays like any other content.
        self.surface.is_none()
    }

    fn wants_post_paint(&self) -> bool {
        self.surface.is_some()
    }

    /// A surface page is drawn over the style's chrome, where a native
    /// subview would sit: the loading wash shows only until the first frame
    /// covers it.
    fn post_paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let Some(surface) = &self.surface else {
            return;
        };
        let attachment = surface.attachment.borrow();
        let Some(attachment) = attachment.as_ref() else {
            return;
        };
        // Drawn on every paint, with or without a frame yet, so the window
        // takes the engine's commits.
        let content = surface
            .geometry
            .get()
            .map(|g| {
                Rect::new(
                    bounds.x + g.content.x,
                    bounds.y + g.content.y,
                    g.content.width,
                    g.content.height,
                )
            })
            .unwrap_or(Rect::new(bounds.x, bounds.y, 0.0, 0.0));
        canvas.draw_live_image(
            attachment.consumer(),
            &LiveImageDraw::new(content, bounds)
                .orientation(surface.orientation)
                .pixel_snap(true),
        );
    }

    fn after_paint(&self, view: &WidgetTreeView<'_>, _ctx: &PaintContext) {
        // An interactive overlay — a menu, a popover, a modal dialog — renders
        // in the wgpu pass, i.e. *under* the engine subview, and the OS routes
        // a press over that region to the engine, not to the overlay. Standing
        // the subview down while one covers the page is what makes such an
        // overlay both visible and operable; nothing else in the toolkit can
        // reach over a native child.
        //
        // This is the one thing that cannot be decided in `place_children`:
        // overlays are positioned *after* the main tree is laid out, so a
        // layout pass reads the bounds an overlay had before it opened, and
        // nothing marks the tree dirty again once they are known. The paint
        // walk runs after both and is handed the frame's own rects.
        let Some(id) = self.self_id.get() else {
            return;
        };
        let bounds = view.bounds(id);
        let covered = view
            .overlay_rects()
            .iter()
            .any(|r| intersect(*r, bounds).is_some());
        let uncovered = !covered;
        let mut state = self.visibility.get();
        if state.uncovered == uncovered {
            return;
        }
        state.uncovered = uncovered;
        self.visibility.set(state);
        apply_visibility(&self.handle, &self.visibility, &self.visible_applied);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A single Teksilo-side node. The page's own AT tree is published by
        // the engine to the OS directly, so we don't duplicate it; our
        // descendants are just the presentational overlay (already hidden).
        builder.set_role(Role::WebView);
        if let Some(title) = &self.title_signal {
            builder.set_name(title.get());
        }
        // The frame is reachable by Tab and *enterable* by Enter. Both have to
        // be advertised: `Focus` so an AT client can put the toolkit's focus
        // here, `Click` so "activate" from a screen reader means the same as
        // pressing Enter — hand the keyboard to the engine. The `on_key` /
        // `on_access_action` paths both end at `WebViewHandle::set_focus`.
        builder.add_action(teksilo_core::accesskit::Action::Focus);
        builder.add_action(teksilo_core::accesskit::Action::Click);
        if !self.enter_page_on_focus {
            builder.set_keyboard_shortcut("Enter");
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl Drop for WebView {
    fn drop(&mut self) {
        // Unregister the event callback so a late backend event can't route
        // into freed widget state. The engine handle tears down via its own
        // Drop when `self.handle`'s last Rc clone goes — this, the focus / key
        // / access-action handlers held by the arena node, the activation and
        // navigation effects, and the post-mount open action.
        if let Some(registry) = self.registry.borrow().as_ref() {
            registry.unregister(self.web_view_id);
        }
    }
}

/// Zero-size, zero-paint overlay content placeholder. Fills the proposed
/// bounds so the overlay container has a child to size against.
#[derive(Debug)]
struct EmptyOverlayContent;

impl Widget for EmptyOverlayContent {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

/// The overlapping part of two rectangles, or `None` when they do not overlap.
///
/// Zero-area contact counts as no overlap: a page scrolled exactly to its
/// viewport's edge is not visible, and an overlay whose edge merely touches the
/// page's is not standing over it.
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    if right > x && bottom > y {
        Some(Rect::new(x, y, right - x, bottom - y))
    } else {
        None
    }
}

/// A device scale that can divide: the window's, or 1 when it is not usable.
fn usable_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// The input a key event forwards to a surface page, or `None` for what
/// Teksilo keeps: Ctrl+Tab and Ctrl+Shift+Tab, the way out of the page.
fn forwarded_key(event: &teksilo_core::event::WidgetEvent) -> Option<WebViewInputEvent> {
    use teksilo_core::event::{Key, WidgetEvent};
    let escape = |key: &Key, modifiers: &teksilo_core::event::Modifiers| {
        *key == Key::Tab && modifiers.ctrl()
    };
    match event {
        WidgetEvent::KeyDown {
            key,
            modifiers,
            text,
        } if !escape(key, modifiers) => Some(WebViewInputEvent::Key {
            key: *key,
            modifiers: *modifiers,
            pressed: true,
            text: text.clone(),
        }),
        WidgetEvent::KeyUp { key, modifiers } if !escape(key, modifiers) => {
            Some(WebViewInputEvent::Key {
                key: *key,
                modifiers: *modifiers,
                pressed: false,
                text: None,
            })
        }
        WidgetEvent::ImeComposition { text, cursor } => Some(WebViewInputEvent::ImeComposition {
            text: text.clone(),
            cursor: cursor.clone(),
        }),
        WidgetEvent::ImeCommit { text } => {
            Some(WebViewInputEvent::ImeCommit { text: text.clone() })
        }
        _ => None,
    }
}

/// The pointer handlers of a surface page: everything over it is forwarded
/// to the engine at the page pixel it landed on.
///
/// In [`WebViewInput::Native`] the page owns its rectangle, as a native
/// subview does: the same `touch_action(NONE)` and `no_hit_slop`, events
/// answered `Handled`, and a mouse press captured so a drag that leaves the
/// page (a text selection) keeps reaching it. In
/// [`WebViewInput::Transparent`] the page is told and the event still bubbles,
/// so app UI over the page stays operable.
fn forward_surface_pointer(
    handlers: teksilo_core::widget_builder::HandlerSet,
    surface: Rc<SurfaceView>,
    handle: SharedHandle,
    input: WebViewInput,
) -> teksilo_core::widget_builder::HandlerSet {
    use teksilo_core::event::{EventResponse, WidgetEvent};
    use teksilo_core::pointer::touch_action::TouchAction;
    use teksilo_tokens::PointerKind;

    let native = input.is_native();
    let answer = if native {
        EventResponse::Handled
    } else {
        EventResponse::Ignored
    };
    let send = |handle: &SharedHandle, event: WebViewInputEvent| {
        if let Some(h) = handle.borrow().as_ref() {
            h.send_input(&event);
        }
    };

    let mut handlers = handlers;
    if native {
        handlers = handlers.touch_action(TouchAction::NONE).no_hit_slop();
    }

    let (pointer_surface, pointer_handle) = (surface.clone(), handle.clone());
    handlers = handlers.on_pointer_event(move |event, ctx| {
        let (position, pointer, button, phase) = match event {
            WidgetEvent::PointerDown {
                position,
                pointer,
                button,
                ..
            } => (*position, *pointer, Some(*button), Some(true)),
            WidgetEvent::PointerUp {
                position,
                pointer,
                button,
                ..
            } => (*position, *pointer, Some(*button), Some(false)),
            WidgetEvent::PointerMove {
                position, pointer, ..
            } => (*position, *pointer, None, None),
            _ => return EventResponse::Ignored,
        };
        let Some(page) = pointer_surface.page_point(position) else {
            return answer;
        };
        let contact_kind = matches!(pointer.kind, PointerKind::Touch | PointerKind::Pen(_));
        let pen = matches!(pointer.kind, PointerKind::Pen(_));
        let mut contacts = pointer_surface.contacts.borrow_mut();
        let in_contact = contacts.contains(&pointer.id);
        let forwarded = match phase {
            Some(true) if contact_kind => {
                contacts.push(pointer.id);
                WebViewInputEvent::Touch {
                    contact: pointer.id,
                    phase: WebViewTouchPhase::Down,
                    position: page,
                    pen,
                }
            }
            Some(false) if in_contact => {
                contacts.retain(|c| *c != pointer.id);
                WebViewInputEvent::Touch {
                    contact: pointer.id,
                    phase: WebViewTouchPhase::Up,
                    position: page,
                    pen,
                }
            }
            None if in_contact => WebViewInputEvent::Touch {
                contact: pointer.id,
                phase: WebViewTouchPhase::Move,
                position: page,
                pen,
            },
            Some(pressed) => {
                if pressed && native {
                    ctx.capture_pointer();
                }
                WebViewInputEvent::PointerButton {
                    position: page,
                    button: button.unwrap_or(teksilo_core::event::PointerButton::Primary),
                    pressed,
                }
            }
            None => WebViewInputEvent::PointerMove { position: page },
        };
        drop(contacts);
        pointer_surface.last_pointer.set(Some(page));
        send(&pointer_handle, forwarded);
        answer
    });

    let (cancel_surface, cancel_handle) = (surface.clone(), handle.clone());
    handlers = handlers.on_pointer_cancel(move |pointer, _reason, _ctx| {
        let mut contacts = cancel_surface.contacts.borrow_mut();
        if !contacts.contains(&pointer.id) {
            return;
        }
        contacts.retain(|c| *c != pointer.id);
        drop(contacts);
        send(
            &cancel_handle,
            WebViewInputEvent::Touch {
                contact: pointer.id,
                phase: WebViewTouchPhase::Cancel,
                position: cancel_surface.last_pointer.get().unwrap_or(Point::ZERO),
                pen: matches!(pointer.kind, PointerKind::Pen(_)),
            },
        );
    });

    let leave_handle = handle.clone();
    handlers = handlers.on_hover(move |entered, _ctx| {
        if !entered {
            send(&leave_handle, WebViewInputEvent::PointerLeave);
        }
    });

    handlers.on_scroll(move |event, ctx| {
        let WidgetEvent::Scroll {
            delta,
            modifiers,
            window_position,
            ..
        } = event
        else {
            return EventResponse::Ignored;
        };
        // A scroll carries a window position when its producer knows one;
        // otherwise it is where the pointer last was.
        let position = window_position
            .and_then(|p| ctx.to_local(p))
            .and_then(|local| surface.page_point(local))
            .or(surface.last_pointer.get());
        let Some(position) = position else {
            return answer;
        };
        send(
            &handle,
            WebViewInputEvent::Scroll {
                position,
                delta: *delta,
                modifiers: *modifiers,
            },
        );
        answer
    })
}

/// Resolve the three reasons a subview may be hidden into one `set_visible`,
/// and issue it only when the answer changed.
///
/// A no-op before the engine opens: the post-mount open path applies the
/// resolved value once the handle exists, so a view whose tab was already
/// parked (or whose viewport had already scrolled past it) opens hidden instead
/// of flashing.
fn apply_visibility(
    handle: &SharedHandle,
    visibility: &Rc<Cell<EngineVisibility>>,
    applied: &Rc<Cell<bool>>,
) {
    let want = visibility.get().resolved();
    if applied.get() == want {
        return;
    }
    if let Some(h) = handle.borrow().as_ref() {
        h.set_visible(want);
        applied.set(want);
    }
}
