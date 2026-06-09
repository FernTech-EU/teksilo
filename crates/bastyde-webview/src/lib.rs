//! `bastyde-webview` — an embeddable [`WebView`] widget for Bastyde.
//!
//! A web view is the one widget that **cannot** render into Bastyde's wgpu
//! surface: every realistic engine (WKWebView, WebView2, WebKitGTK, Servo)
//! owns its own rendering and lives as a native subview *on top of* the wgpu
//! pass. This crate accepts that reality and mirrors the established
//! platform-backend pattern — a swappable [`WebViewBackend`] creates an
//! engine-specific [`WebViewHandle`], and a per-app [`WebViewRegistry`]
//! (installed in app-state) routes JS→Rust / lifecycle events back to the
//! widget.
//!
//! ```ignore
//! use bastyde_webview::WebView;
//!
//! WebView::new()
//!     .url("https://example.com")
//!     .bind_title(title_signal.clone())
//!     .bind_loading(loading_signal.clone())
//!     .on_message(|msg, _ctx| println!("JS said: {msg}"));
//! ```
//!
//! # The Switcher / dormancy caveat
//!
//! Because the engine surface lives *outside* the wgpu pass, "not painted"
//! does NOT mean "hidden" for a `WebView`. When a [`Switcher`] /
//! `TabWidget` / `visible_when` gate parks the widget dormant, the framework
//! simply stops painting it — but the native subview keeps floating over the
//! output. `WebView` closes this gap by bridging the framework's per-node
//! **activation signal** (`BuildContext::activation_signal`) to the engine's
//! `set_visible`: tab-away → `set_visible(false)`, tab-back → `set_visible
//! (true)`. This is the one place a widget must explicitly mirror framework
//! visibility onto an OS resource, and it is wired automatically here.
//!
//! [`Switcher`]: https://docs.rs/bastyde-widgets

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
    ConsoleLevel, MemoryWebViewBackend, MemoryWebViewRecords, NoopWebViewBackend, WebSource,
    WebViewAttributes, WebViewBackend, WebViewEvent, WebViewEventPayload, WebViewHandle, WebViewId,
    WebViewOp, WebViewRegistry, memory_registry,
};
pub use recipe_web_view_style::RecipeWebViewStyle;

// Re-export the Tier-3 style surface (the trait lives in bastyde-core so the
// core slot bag can name it, same as every other themable widget).
pub use bastyde_core::styles::{
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
/// timers. Call this from `BastydeAppBuilder::on_loop_tick` with a poll source
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

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::Role;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::window::BastydeWindowId;

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
/// but Bastyde delivers backend events on a later event-loop tick (events are
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
    window_id: Cell<Option<BastydeWindowId>>,
    style_override: Option<SharedWebViewStyle>,
    root_child_id: Option<WidgetId>,
    /// Internal lifecycle state driving the overlay chrome.
    state_signal: Signal<WebViewVisualState>,
    /// Registry handle, written by the post-mount open action and read by
    /// `Drop` for unregistration. Shared so the moved open closure can set it.
    registry: Rc<RefCell<Option<WebViewRegistry>>>,

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
            style_override: None,
            root_child_id: None,
            state_signal: Signal::new(WebViewVisualState::Loading),
            registry: Rc::new(RefCell::new(None)),
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
    /// [`html`](Self::html) / [`source`](Self::source); `bind_url` governs
    /// navigation *after* the first load (the signal's value at build time is
    /// taken as the baseline and does not trigger a navigation).
    pub fn bind_url(mut self, signal: Signal<String>) -> Self {
        self.url_signal = Some(signal);
        self
    }

    /// Bind the page title (read-only — updated on `TitleChanged`).
    pub fn bind_title(mut self, signal: Signal<String>) -> Self {
        self.title_signal = Some(signal);
        self
    }

    /// Bind the loading flag (read-only — true between page-load start/finish).
    pub fn bind_loading(mut self, signal: Signal<bool>) -> Self {
        self.loading_signal = Some(signal);
        self
    }

    /// JS → Rust: called when the page runs `window.ipc.postMessage(...)`.
    pub fn on_message(mut self, cb: impl FnMut(String, &mut EventContext) + 'static) -> Self {
        self.on_message = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Called when the document title changes.
    pub fn on_title_changed(
        mut self,
        cb: impl FnMut(String, &mut EventContext) + 'static,
    ) -> Self {
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

    /// The stable routing identity of this web view.
    pub fn id(&self) -> WebViewId {
        self.web_view_id
    }

    // --- Imperative controls (call via `ctx.with_widget_mut::<WebView>`) ---

    /// Navigate to `url`.
    pub fn load_url(&self, url: &str) {
        self.with_handle(|h| h.load_url(url));
    }
    /// Rust → JS: dispatch a `bastyde-message` event carrying `msg`.
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

    fn with_handle(&self, f: impl FnOnce(&dyn WebViewHandle)) {
        if let Some(h) = self.handle.borrow().as_ref() {
            f(h.as_ref());
        }
    }

    /// Build the JS→Rust / lifecycle event callback handed to the registry.
    fn make_event_callback(&self) -> impl FnMut(WebViewEvent, &mut EventContext) + 'static {
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
                content,
            },
            ctx,
        );
        self.root_child_id = Some(body);

        // Capture the window id now — the post-mount EventContext has no
        // direct window-id accessor, but BuildContext::window() does.
        self.window_id
            .set(ctx.window().map(|w| w.id()));

        // --- Visibility bridge: framework activation → engine set_visible ---
        // The single reason this widget needs the activation signal: a native
        // subview ignores the wgpu paint pass, so a Switcher parking us
        // dormant would otherwise leave the engine surface visible. The effect
        // no-ops until the engine handle exists (opened post-mount below).
        let vis = ctx.activation_signal(self_id);
        let effect_handle = self.handle.clone();
        ctx.effect(&vis, move |active| {
            if let Some(h) = effect_handle.borrow().as_ref() {
                h.set_visible(*active);
            }
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
            let on_event = self.make_event_callback();

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
                let wid = window_id.unwrap_or_else(|| BastydeWindowId::new(0));

                let handle =
                    registry.open(web_view_id, wid, parent, attrs, poster, on_event);
                // Apply the bounds layout already resolved, then the current
                // activation state (so a view mounted while its tab is parked
                // opens hidden, not visible-then-flashing).
                if let Some(b) = bounds_slot.get() {
                    handle.set_bounds(b, scale_slot.get());
                }
                // The engine subview opens visible by default, so only act on
                // the parked case: a view mounted while its tab is dormant must
                // be hidden at birth (no visible-then-hidden flash). Active
                // opens need no redundant set_visible(true).
                if !activation.get() {
                    handle.set_visible(false);
                }

                *handle_slot.borrow_mut() = Some(handle);
                *registry_slot.borrow_mut() = Some(registry);
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
        // `ctx.scale_factor` is the host window's HiDPI device scale (a scale
        // change triggers a relayout, so this runs then too). The backend uses
        // both: engines that position in device pixels (WebKitGTK on X11) need
        // logical × scale. Store the scale so the post-mount open path can apply
        // the first bounds at the right scale.
        let scale = ctx.scale_factor;
        let scale_changed = (self.scale.get() - scale).abs() > f32::EPSILON;
        if scale_changed {
            self.scale.set(scale);
        }
        if self.last_bounds.get() != Some(bounds) || scale_changed {
            self.last_bounds.set(Some(bounds));
            self.with_handle(|h| h.set_bounds(bounds, scale));
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A single Bastyde-side node. The page's own AT tree is published by
        // the engine to the OS directly, so we don't duplicate it; our
        // descendants are just the presentational overlay (already hidden).
        builder.set_role(Role::WebView);
        if let Some(title) = &self.title_signal {
            builder.set_name(title.get());
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
        // Drop when `self.handle`'s last Rc clone (this + the effect) goes.
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
