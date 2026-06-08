//! Backend abstraction for [`WebView`](crate::WebView).
//!
//! A web view is the one widget that cannot render into Bastyde's wgpu
//! surface — every realistic engine (WKWebView, WebView2, WebKitGTK, Servo)
//! owns its own rendering and lives as a native subview *on top of* the wgpu
//! pass. This module mirrors the established platform-backend pattern
//! ([`FileDialogBackend`](bastyde_platform::file_dialog::FileDialogBackend) /
//! `ExternalDndBackend`): a swappable [`WebViewBackend`] trait creates an
//! engine-specific [`WebViewHandle`], and a per-app [`WebViewRegistry`]
//! (registered in app-state) owns the backend and routes JS→Rust /
//! browser-lifecycle events back into the originating widget tree.
//!
//! The default build ships only the [`MemoryWebViewBackend`] (headless,
//! deterministic). The native `wry` / `servo` backends live behind the
//! `wry-backend` / `servo-backend` features.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bastyde_canvas::Rect;
use bastyde_core::AppEventPoster;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::widget::EventContext;
use bastyde_core::window::BastydeWindowId;

/// Process-unique identity for a single web view instance. Allocated once at
/// `WebView` construction and stable across rebuilds, so backend events route
/// to the correct widget. Same shape as `MenuItemId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebViewId(u64);

impl WebViewId {
    /// Allocate the next process-unique id.
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw numeric value (diagnostics / map keys).
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// What a web view should initially display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSource {
    /// Navigate to a URL.
    Url(String),
    /// Load an inline HTML string, with an optional base URL for relative
    /// asset resolution.
    Html {
        html: String,
        base_url: Option<String>,
    },
}

/// Severity of a [`WebViewEvent::ConsoleMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    Log,
    Warn,
    Error,
}

/// Engine configuration accumulated by the [`WebView`](crate::WebView)
/// builders and handed to [`WebViewBackend::open`].
#[derive(Debug, Clone, Default)]
pub struct WebViewAttributes {
    /// Initial content. `None` means "blank page".
    pub source: Option<WebSource>,
    /// Override the engine's `User-Agent`.
    pub user_agent: Option<String>,
    /// Transparent engine background (compose over Bastyde content).
    pub transparent: bool,
    /// Enable the engine's devtools (debug builds only by convention).
    pub devtools: bool,
    /// Custom-protocol scheme names the app wants to serve (`app` → `app://`).
    /// The dispatch closures live app-side; the backend only needs the names
    /// at open time to register the schemes.
    pub custom_protocols: Vec<String>,
}

/// A live native engine subview. Dropping the handle tears the subview down
/// (RAII, same contract as `ExternalDndGuard`).
///
/// All methods are `&self` — the handle is cheaply shareable and the engine
/// state lives behind the platform's own interior mutability.
pub trait WebViewHandle: 'static {
    /// Reposition / resize the native subview within its parent window, in
    /// logical pixels. Issued whenever the widget's layout bounds change.
    fn set_bounds(&self, bounds: Rect);
    /// Navigate to a URL.
    fn load_url(&self, url: &str);
    /// Load inline HTML.
    fn load_html(&self, html: &str, base_url: Option<&str>);
    /// Evaluate JavaScript in the page.
    fn eval(&self, script: &str);
    /// Rust → JS: dispatch a `bastyde-message` `MessageEvent` carrying `msg`.
    fn post_message(&self, msg: &str);
    /// Reload the current page.
    fn reload(&self);
    /// Navigate back in history.
    fn go_back(&self);
    /// Navigate forward in history.
    fn go_forward(&self);
    /// Stop the current load.
    fn stop(&self);
    /// Show / hide the native subview. **Load-bearing**: a native subview
    /// lives outside the wgpu pass, so framework dormancy (a `Switcher`
    /// parking the page) does NOT hide it — the `WebView` widget bridges
    /// its activation signal to this call. See `WebView`'s rustdoc.
    fn set_visible(&self, visible: bool);
    /// Give the engine subview keyboard focus.
    fn set_focus(&self);
}

/// A browser lifecycle / JS→Rust event surfaced by a backend.
#[derive(Debug, Clone)]
pub enum WebViewEvent {
    /// A navigation is starting. `can_cancel` is true on backends that
    /// support pre-navigation veto.
    NavigationStarted { url: String, can_cancel: bool },
    /// A navigation finished (or failed).
    NavigationFinished { url: String, success: bool },
    /// The page began loading resources.
    PageLoadStarted,
    /// The page finished loading.
    PageLoadFinished,
    /// The document title changed.
    TitleChanged(String),
    /// `window.ipc.postMessage(payload)` fired in the page.
    Message(String),
    /// A download began.
    DownloadStarted {
        url: String,
        suggested_path: PathBuf,
    },
    /// A download finished (or failed).
    DownloadFinished { path: PathBuf, success: bool },
    /// A console message (forwarded in debug builds / by best-effort
    /// backends to report unsupported operations).
    ConsoleMessage { level: ConsoleLevel, text: String },
}

/// Boxed inside `AppEvent::External` when a backend produces an event.
/// `bastyde-app`'s app-event handler downcasts to this type and routes to
/// [`WebViewRegistry::deliver`]. Mirrors `FileDialogEventPayload`.
pub struct WebViewEventPayload {
    /// The window the web view lives in — routes delivery to the right tree.
    pub window_id_owner: BastydeWindowId,
    /// Which web view the event belongs to.
    pub web_view_id: WebViewId,
    /// The event itself.
    pub event: WebViewEvent,
}

/// Swappable web-view engine backend.
///
/// The real backends (`WryBackend` / `ServoBackend`, behind their features)
/// create a native engine subview parented to the host window. The test
/// backend ([`MemoryWebViewBackend`]) records calls and synthesizes events.
pub trait WebViewBackend {
    /// Create a native engine subview for `web_view_id`, parented to
    /// `window_id`'s OS window. The backend MUST deliver browser events by
    /// calling [`AppEventPoster::post_external`] on `poster` with a boxed
    /// [`WebViewEventPayload`] whose `web_view_id` / `window_id_owner` match.
    ///
    /// `parent` is `None` when the host context can't surface an OS handle
    /// (headless tests, or a build-time open before the window-ops sink is
    /// available); native backends treat `None` as "defer until a handle
    /// arrives" rather than failing hard.
    fn open(
        &mut self,
        web_view_id: WebViewId,
        window_id: BastydeWindowId,
        parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
    ) -> Box<dyn WebViewHandle>;
}

// ============================================================
// WebViewRegistry — per-app service (app-state)
// ============================================================

/// Callback the `WebView` widget installs to receive its own backend events.
type EventCallback = Box<dyn FnMut(WebViewEvent, &mut EventContext)>;

struct Registered {
    window_id: BastydeWindowId,
    callback: EventCallback,
}

struct RegistryState {
    backend: RefCell<Box<dyn WebViewBackend>>,
    callbacks: RefCell<HashMap<WebViewId, Registered>>,
    /// Bumped per `open`, purely for diagnostics.
    open_count: Cell<u64>,
}

/// Per-app web-view service. Registered in app-state by
/// [`install_web_view`](crate::BastydeAppBuilderWebViewExt::install_web_view);
/// reachable from any `build()` / handler via
/// `ctx.app_state::<WebViewRegistry>()`. Cloneable; clones share the same
/// backend and event-callback map.
#[derive(Clone)]
pub struct WebViewRegistry {
    inner: Rc<RegistryState>,
}

impl WebViewRegistry {
    /// Build a registry wrapping `backend`.
    pub fn new<B: WebViewBackend + 'static>(backend: B) -> Self {
        Self {
            inner: Rc::new(RegistryState {
                backend: RefCell::new(Box::new(backend)),
                callbacks: RefCell::new(HashMap::new()),
                open_count: Cell::new(0),
            }),
        }
    }

    /// Open a native subview and register the widget's event callback in one
    /// step. Returns the live [`WebViewHandle`] (dropped on widget removal).
    pub fn open(
        &self,
        web_view_id: WebViewId,
        window_id: BastydeWindowId,
        parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
        on_event: impl FnMut(WebViewEvent, &mut EventContext) + 'static,
    ) -> Box<dyn WebViewHandle> {
        self.inner
            .open_count
            .set(self.inner.open_count.get().wrapping_add(1));
        self.inner.callbacks.borrow_mut().insert(
            web_view_id,
            Registered {
                window_id,
                callback: Box::new(on_event),
            },
        );
        self.inner
            .backend
            .borrow_mut()
            .open(web_view_id, window_id, parent, attrs, poster)
    }

    /// Route a backend-produced payload to its registered widget callback.
    /// Called by `bastyde-app` from the `AppEvent::External` arm. Dropped
    /// silently if the callback was already purged (window/widget gone).
    pub fn deliver(&self, payload: WebViewEventPayload, ctx: &mut EventContext) {
        // Take the callback out across the user code so the borrow on the
        // map isn't held while the (re-entrant-capable) callback runs, then
        // put it back. A callback removed mid-call (widget dropped) simply
        // isn't reinserted.
        let entry = self.inner.callbacks.borrow_mut().remove(&payload.web_view_id);
        let Some(mut reg) = entry else {
            return;
        };
        if reg.window_id != payload.window_id_owner {
            // Stale routing — drop, don't reinsert.
            return;
        }
        (reg.callback)(payload.event, ctx);
        self.inner
            .callbacks
            .borrow_mut()
            .entry(payload.web_view_id)
            .or_insert(reg);
    }

    /// Drop the registration for a single web view (widget removed).
    pub fn unregister(&self, web_view_id: WebViewId) {
        self.inner.callbacks.borrow_mut().remove(&web_view_id);
    }

    /// Drop every registration owned by `window_id`. Called by
    /// `bastyde-app`'s window-close path so callbacks capturing widget state
    /// cannot fire into a torn-down tree. Mirrors
    /// `FileDialogHandle::purge_window`.
    pub fn purge_window(&self, window_id: BastydeWindowId) {
        self.inner
            .callbacks
            .borrow_mut()
            .retain(|_, r| r.window_id != window_id);
    }

    /// Number of registered web views. Test helper.
    pub fn registered_count(&self) -> usize {
        self.inner.callbacks.borrow().len()
    }
}

impl std::fmt::Debug for WebViewRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebViewRegistry")
            .field("registered", &self.inner.callbacks.borrow().len())
            .field("opens", &self.inner.open_count.get())
            .finish_non_exhaustive()
    }
}

// ============================================================
// MemoryWebViewBackend (headless test backend)
// ============================================================

/// One recorded backend operation. Lets tests assert the exact call sequence
/// (open → set_bounds → set_visible(false) → set_visible(true) → …) without a
/// real engine, window, or GPU.
#[derive(Debug, Clone, PartialEq)]
pub enum WebViewOp {
    Open { web_view_id: WebViewId },
    SetBounds { web_view_id: WebViewId, bounds: Rect },
    LoadUrl { web_view_id: WebViewId, url: String },
    LoadHtml { web_view_id: WebViewId },
    Eval { web_view_id: WebViewId, script: String },
    PostMessage { web_view_id: WebViewId, msg: String },
    Reload { web_view_id: WebViewId },
    GoBack { web_view_id: WebViewId },
    GoForward { web_view_id: WebViewId },
    Stop { web_view_id: WebViewId },
    SetVisible { web_view_id: WebViewId, visible: bool },
    SetFocus { web_view_id: WebViewId },
    Dropped { web_view_id: WebViewId },
}

/// Shared, cloneable recorder. Both the backend and the test hold a clone, so
/// the test can read the op log after driving the tree.
#[derive(Clone, Default)]
pub struct MemoryWebViewRecords {
    ops: Rc<RefCell<Vec<WebViewOp>>>,
}

impl MemoryWebViewRecords {
    /// All recorded ops, in order.
    pub fn ops(&self) -> Vec<WebViewOp> {
        self.ops.borrow().clone()
    }

    /// Every op for a given web view.
    pub fn ops_for(&self, id: WebViewId) -> Vec<WebViewOp> {
        self.ops
            .borrow()
            .iter()
            .filter(|op| op_web_view_id(op) == id)
            .cloned()
            .collect()
    }

    /// The ordered `set_visible` booleans for a web view — the headline
    /// dormancy assertion (`[false, true]` across a tab-away / tab-back).
    pub fn visibility_log(&self, id: WebViewId) -> Vec<bool> {
        self.ops
            .borrow()
            .iter()
            .filter_map(|op| match op {
                WebViewOp::SetVisible {
                    web_view_id,
                    visible,
                } if *web_view_id == id => Some(*visible),
                _ => None,
            })
            .collect()
    }

    fn push(&self, op: WebViewOp) {
        self.ops.borrow_mut().push(op);
    }
}

fn op_web_view_id(op: &WebViewOp) -> WebViewId {
    match op {
        WebViewOp::Open { web_view_id }
        | WebViewOp::SetBounds { web_view_id, .. }
        | WebViewOp::LoadUrl { web_view_id, .. }
        | WebViewOp::LoadHtml { web_view_id }
        | WebViewOp::Eval { web_view_id, .. }
        | WebViewOp::PostMessage { web_view_id, .. }
        | WebViewOp::Reload { web_view_id }
        | WebViewOp::GoBack { web_view_id }
        | WebViewOp::GoForward { web_view_id }
        | WebViewOp::Stop { web_view_id }
        | WebViewOp::SetVisible { web_view_id, .. }
        | WebViewOp::SetFocus { web_view_id }
        | WebViewOp::Dropped { web_view_id } => *web_view_id,
    }
}

/// In-memory deterministic backend for headless tests. Records every op into a
/// shared [`MemoryWebViewRecords`]; never renders. Mirrors `MemoryFileDialog`.
pub struct MemoryWebViewBackend {
    records: MemoryWebViewRecords,
}

impl MemoryWebViewBackend {
    /// Build a backend plus its shared recorder; clone the returned records
    /// before moving the backend into a [`WebViewRegistry`].
    pub fn new() -> (Self, MemoryWebViewRecords) {
        let records = MemoryWebViewRecords::default();
        (
            Self {
                records: records.clone(),
            },
            records,
        )
    }
}

struct MemoryWebViewHandle {
    web_view_id: WebViewId,
    records: MemoryWebViewRecords,
}

impl WebViewHandle for MemoryWebViewHandle {
    fn set_bounds(&self, bounds: Rect) {
        self.records.push(WebViewOp::SetBounds {
            web_view_id: self.web_view_id,
            bounds,
        });
    }
    fn load_url(&self, url: &str) {
        self.records.push(WebViewOp::LoadUrl {
            web_view_id: self.web_view_id,
            url: url.to_string(),
        });
    }
    fn load_html(&self, _html: &str, _base_url: Option<&str>) {
        self.records.push(WebViewOp::LoadHtml {
            web_view_id: self.web_view_id,
        });
    }
    fn eval(&self, script: &str) {
        self.records.push(WebViewOp::Eval {
            web_view_id: self.web_view_id,
            script: script.to_string(),
        });
    }
    fn post_message(&self, msg: &str) {
        self.records.push(WebViewOp::PostMessage {
            web_view_id: self.web_view_id,
            msg: msg.to_string(),
        });
    }
    fn reload(&self) {
        self.records.push(WebViewOp::Reload {
            web_view_id: self.web_view_id,
        });
    }
    fn go_back(&self) {
        self.records.push(WebViewOp::GoBack {
            web_view_id: self.web_view_id,
        });
    }
    fn go_forward(&self) {
        self.records.push(WebViewOp::GoForward {
            web_view_id: self.web_view_id,
        });
    }
    fn stop(&self) {
        self.records.push(WebViewOp::Stop {
            web_view_id: self.web_view_id,
        });
    }
    fn set_visible(&self, visible: bool) {
        self.records.push(WebViewOp::SetVisible {
            web_view_id: self.web_view_id,
            visible,
        });
    }
    fn set_focus(&self) {
        self.records.push(WebViewOp::SetFocus {
            web_view_id: self.web_view_id,
        });
    }
}

impl Drop for MemoryWebViewHandle {
    fn drop(&mut self) {
        self.records.push(WebViewOp::Dropped {
            web_view_id: self.web_view_id,
        });
    }
}

impl WebViewBackend for MemoryWebViewBackend {
    fn open(
        &mut self,
        web_view_id: WebViewId,
        _window_id: BastydeWindowId,
        _parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        _poster: Option<Arc<dyn AppEventPoster>>,
    ) -> Box<dyn WebViewHandle> {
        self.records.push(WebViewOp::Open { web_view_id });
        // Replay the initial source as the corresponding load op so tests can
        // see what the widget asked to display.
        match attrs.source {
            Some(WebSource::Url(url)) => self.records.push(WebViewOp::LoadUrl { web_view_id, url }),
            Some(WebSource::Html { .. }) => {
                self.records.push(WebViewOp::LoadHtml { web_view_id })
            }
            None => {}
        }
        Box::new(MemoryWebViewHandle {
            web_view_id,
            records: self.records.clone(),
        })
    }
}

/// Convenience: a registry backed by a fresh [`MemoryWebViewBackend`], plus
/// its shared recorder. The one-liner headless-test setup.
pub fn memory_registry() -> (WebViewRegistry, MemoryWebViewRecords) {
    let (backend, records) = MemoryWebViewBackend::new();
    (WebViewRegistry::new(backend), records)
}
