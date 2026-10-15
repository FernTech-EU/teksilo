// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Backend abstraction for [`WebView`](crate::WebView).
//!
//! An engine shows its page in one of two ways, and a backend says which with
//! [`WebViewBackend::rendering`]:
//!
//! - [`WebViewRendering::NativeSubview`]: the engine owns a native OS view
//!   placed *on top of* the wgpu pass (WKWebView, WebView2 and WebKitGTK, all
//!   through wry). The OS delivers its input; the widget positions, clips and
//!   hides it.
//! - [`WebViewRendering::Surface`]: the engine renders offscreen and writes each
//!   frame into the [`LiveImageWriter`] the widget hands
//!   [`WebViewBackend::open`]. The widget draws the picture in the wgpu pass
//!   like any other content and forwards input with
//!   [`WebViewHandle::send_input`].
//!
//! This module mirrors the established platform-backend pattern
//! (`FileDialogBackend` / `ExternalDndBackend`): a swappable
//! [`WebViewBackend`] trait creates an engine-specific [`WebViewHandle`], and a
//! per-app [`WebViewRegistry`] (registered in app-state) owns the backend and
//! routes JS→Rust / browser-lifecycle events back into the originating widget
//! tree.
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

use teksilo_canvas::live_image::LiveImageWriter;
use teksilo_canvas::{ImageOrientation, Point, Rect};
use teksilo_core::AppEventPoster;
use teksilo_core::event::{Key, Modifiers, PointerButton, ScrollDelta};
use teksilo_core::pointer::PointerId;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::widget::CursorIcon;
use teksilo_core::widget::EventContext;
use teksilo_core::window::TeksiloWindowId;

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
    /// Transparent engine background (compose over Teksilo content).
    pub transparent: bool,
    /// Enable the engine's devtools (debug builds only by convention).
    pub devtools: bool,
    /// Custom-protocol scheme names the app wants to serve (`app` → `app://`).
    /// The dispatch closures live app-side; the backend only needs the names
    /// at open time to register the schemes.
    pub custom_protocols: Vec<String>,
}

/// How an engine shows its page, as its backend reports through
/// [`WebViewBackend::rendering`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebViewRendering {
    /// A native OS view placed over the host window. The OS delivers its
    /// input, and the widget keeps it positioned, clipped to its visible
    /// part and hidden when dormant or covered by an overlay.
    NativeSubview,
    /// Offscreen rendering into the [`LiveImageWriter`] the widget passes to
    /// [`WebViewBackend::open`]. The widget draws the picture inside the wgpu
    /// pass, so clipping, overlays and dormancy need no help from the engine,
    /// and it forwards input through [`WebViewHandle::send_input`].
    Surface {
        /// How the engine's rows lie in the picture:
        /// [`ImageOrientation::FlipVertical`] for an engine that writes its
        /// frame bottom row first, as an OpenGL readback does, so the widget
        /// turns it upright when drawing instead of the engine copying every
        /// row in reverse.
        orientation: ImageOrientation,
    },
}

/// The phase of a touch contact in [`WebViewInputEvent::Touch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebViewTouchPhase {
    /// The contact landed.
    Down,
    /// The contact moved.
    Move,
    /// The contact lifted.
    Up,
    /// The contact was taken away (palm rejection, a gesture claimed it, the
    /// window lost it). Nothing may activate from it.
    Cancel,
}

/// A clipboard command the page should carry out on its own selection or
/// focused field, sent as [`WebViewInputEvent::Edit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebViewEditCommand {
    /// Copy the selection.
    Copy,
    /// Cut the selection.
    Cut,
    /// Paste at the caret.
    Paste,
}

/// Input the widget forwards to a [`WebViewRendering::Surface`] engine.
///
/// A surface engine has no native view for the OS to deliver input to, so the
/// widget receives it like any other widget and passes it on. Every position
/// is in **page pixels**: the pixels of the frame the engine last wrote into
/// its [`LiveImageWriter`], as displayed, with the origin at the page's
/// top-left whatever orientation the rows were written in. The widget converts
/// from its own coordinates with the same placement it painted the frame
/// with, so a click lands on the pixel the user saw, even while a resize is
/// still on its way to the engine.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WebViewInputEvent {
    /// A mouse, or a pen hovering, moved over the page.
    PointerMove {
        /// Where the pointer is.
        position: Point,
    },
    /// A mouse or pen button went down or up.
    PointerButton {
        /// Where the pointer is.
        position: Point,
        /// Which button.
        button: PointerButton,
        /// `true` on press, `false` on release.
        pressed: bool,
    },
    /// The pointer left the page.
    PointerLeave,
    /// A finger, or a pen in contact with the screen.
    Touch {
        /// Identifies the contact from `Down` to `Up` or `Cancel`.
        contact: PointerId,
        /// What happened to it.
        phase: WebViewTouchPhase,
        /// Where it is.
        position: Point,
        /// Whether the contact is a pen rather than a finger.
        pen: bool,
    },
    /// A wheel or trackpad scroll, as Teksilo reports it.
    Scroll {
        /// Where the pointer is.
        position: Point,
        /// How far, in lines or in logical pixels.
        delta: ScrollDelta,
        /// The modifiers held during the scroll.
        modifiers: Modifiers,
    },
    /// A key went down or up while the page held the keyboard.
    Key {
        /// The logical key.
        key: Key,
        /// The modifiers held.
        modifiers: Modifiers,
        /// `true` on press, `false` on release.
        pressed: bool,
        /// The text the key produces, on a press that produces any.
        text: Option<String>,
    },
    /// An input method updated its composition.
    ImeComposition {
        /// The text being composed.
        text: String,
        /// The caret or selection inside it, in bytes.
        cursor: Option<std::ops::Range<usize>>,
    },
    /// An input method committed text.
    ImeCommit {
        /// The committed text.
        text: String,
    },
    /// A clipboard command, from the application's standard edit shortcuts.
    Edit(WebViewEditCommand),
}

/// A live engine view. Dropping the handle tears it down (RAII, same contract
/// as `ExternalDndGuard`).
///
/// All methods are `&self` — the handle is cheaply shareable and the engine
/// state lives behind the platform's own interior mutability.
pub trait WebViewHandle: 'static {
    /// Reposition / resize the native subview within its parent window.
    /// `bounds` is in **logical** pixels (Teksilo's coordinate system);
    /// `scale_factor` is the host window's HiDPI scale. Most engines position
    /// in logical units, but some need device pixels (`bounds × scale_factor`)
    /// because their own toolkit runs at a different scale than the wgpu
    /// surface — notably WebKitGTK on X11/XWayland, which uses integer GDK
    /// scaling and ignores fractional factors. Issued whenever the widget's
    /// layout bounds or the window scale change.
    fn set_bounds(&self, bounds: Rect, scale_factor: f32);
    /// Navigate to a URL.
    fn load_url(&self, url: &str);
    /// Load inline HTML.
    fn load_html(&self, html: &str, base_url: Option<&str>);
    /// Evaluate JavaScript in the page.
    fn eval(&self, script: &str);
    /// Rust → JS: dispatch a `teksilo-message` `MessageEvent` carrying `msg`.
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
    /// Ask the engine to stop taking pointer input over its own rectangle, so
    /// the OS delivers those events to the host window and Teksilo routes them
    /// — the engine half of [`WebViewInput::Transparent`].
    ///
    /// **Not every engine can do this** — the call needs control over the
    /// native surface's hit region, which the embedding API may simply not
    /// expose — and one that cannot must say so through
    /// [`WebViewEvent::ConsoleMessage`] — the channel this crate
    /// already reserves for reporting an unsupported operation — rather than
    /// accept the call and change nothing. There is deliberately no return
    /// value and no default implementation: an answer invented here would be
    /// an answer for an engine nobody asked.
    ///
    /// [`WebViewInput::Transparent`]: crate::WebViewInput::Transparent
    fn set_input_passthrough(&self, passthrough: bool);
    /// Open the engine's developer tools (no-op on backends that don't
    /// expose them — Servo's embedding API has no clean devtools hook today).
    fn open_devtools(&self) {}
    /// Close the engine's developer tools (no-op where unsupported).
    fn close_devtools(&self) {}
    /// Take the keyboard away from the page: the frame lost Teksilo's focus.
    /// Only a [`WebViewRendering::Surface`] engine needs telling; a native
    /// subview loses the OS focus with the frame.
    fn blur(&self) {}
    /// Pass input to the page. Only a [`WebViewRendering::Surface`] engine
    /// receives this: a native subview gets its input from the OS. See
    /// [`WebViewInputEvent`] for the coordinate space.
    fn send_input(&self, _event: &WebViewInputEvent) {}
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
    /// The engine's own keyboard focus changed: `true` when the page took the
    /// keyboard, `false` when it gave it up.
    ///
    /// A web view has two disjoint focus rings — the toolkit's and the
    /// engine's platform tree — and the engine's is the one Teksilo cannot
    /// see. Without this event a tap inside the page moves the OS focus while
    /// Teksilo goes on believing a text field elsewhere still owns it, caret
    /// blinking. The `WebView` widget follows the event with
    /// `EventContext::request_focus` on its own frame, so the toolkit's focus
    /// agrees with the OS.
    EngineFocusChanged(bool),
    /// The page wants this cursor over itself. Sent by a
    /// [`WebViewRendering::Surface`] engine; a native subview sets the OS
    /// cursor itself.
    CursorChanged(CursorIcon),
    /// Where the page's text caret is, in page pixels, while an editable
    /// element in it has focus (`None` once none has), so the input method's
    /// candidate window opens beside it. Sent by a surface engine.
    InputMethodArea(Option<Rect>),
}

/// Boxed inside `AppEvent::External` when a backend produces an event.
/// `teksilo-app`'s app-event handler downcasts to this type and routes to
/// [`WebViewRegistry::deliver`]. Mirrors `FileDialogEventPayload`.
pub struct WebViewEventPayload {
    /// The window the web view lives in — routes delivery to the right tree.
    pub window_id_owner: TeksiloWindowId,
    /// Which web view the event belongs to.
    pub web_view_id: WebViewId,
    /// The event itself.
    pub event: WebViewEvent,
}

/// Boxed inside `AppEvent::External` when a [`WebViewRendering::Surface`]
/// engine asks to run. `teksilo-app` answers it on the main thread with
/// [`WebViewRegistry::pump`].
///
/// A surface engine works on threads of its own and needs its embedder to call
/// back on the thread that owns it, which is the UI thread. Posting this
/// payload is how it asks; a backend coalesces its requests so that one
/// payload is in flight at a time.
#[derive(Debug, Default)]
pub struct WebViewPumpPayload;

/// Ask the UI loop to call [`WebViewRegistry::pump`], if a poster is available.
#[allow(dead_code)] // used only by the feature-gated engine backends
pub(crate) fn post_pump(poster: &Option<Arc<dyn AppEventPoster>>) {
    if let Some(poster) = poster {
        poster.post_external(Box::new(WebViewPumpPayload) as Box<dyn std::any::Any + Send>);
    }
}

/// Post a [`WebViewEvent`] back to the UI loop, if a poster is available.
/// Shared by every engine backend so the emit path lives in one place.
#[allow(dead_code)] // used only by the feature-gated engine backends
pub(crate) fn post_event(
    poster: &Option<Arc<dyn AppEventPoster>>,
    window_id: TeksiloWindowId,
    web_view_id: WebViewId,
    event: WebViewEvent,
) {
    if let Some(poster) = poster {
        let payload = WebViewEventPayload {
            window_id_owner: window_id,
            web_view_id,
            event,
        };
        poster.post_external(Box::new(payload) as Box<dyn std::any::Any + Send>);
    }
}

/// Encode `s` as a JavaScript string literal (double-quoted, fully escaped) so
/// it can be safely interpolated into an `evaluate_script` body. Lives in the
/// shared backend module (not in one engine's file) so every JS-executing
/// backend uses the same audited escaper — an incomplete escape is a JS
/// injection / silent-SyntaxError hazard.
///
/// Escapes the JS-significant characters: `"`, `\`, the C0 controls (incl.
/// `\n` / `\r` / `\t`), and U+2028 / U+2029 (LINE / PARAGRAPH SEPARATOR — these
/// are line terminators *inside* JS string literals pre-ES2019 and silently
/// break the literal otherwise).
#[allow(dead_code)] // used only by the feature-gated engine backends
pub(crate) fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Swappable web-view engine backend.
///
/// The real backends (`WryBackend` / `ServoBackend`, behind their features)
/// open an engine for the host window. The test backend
/// ([`MemoryWebViewBackend`]) records calls and synthesizes events.
pub trait WebViewBackend {
    /// How this backend's engines show their page. The widget asks before it
    /// opens anything, so it knows whether to make a picture for the engine
    /// to draw into.
    fn rendering(&self) -> WebViewRendering;

    /// Open an engine view for `web_view_id` in `window_id`'s OS window. The
    /// backend MUST deliver browser events by calling
    /// [`AppEventPoster::post_external`] on `poster` with a boxed
    /// [`WebViewEventPayload`] whose `web_view_id` / `window_id_owner` match.
    ///
    /// `parent` is `None` when the host context can't surface an OS handle
    /// (headless tests). A native backend cannot open without one and reports
    /// the failure through the events above.
    ///
    /// `surface` is `Some` exactly when [`rendering`](Self::rendering) is
    /// [`WebViewRendering::Surface`]: the picture the widget draws, for the
    /// engine to write its frames into.
    fn open(
        &mut self,
        web_view_id: WebViewId,
        window_id: TeksiloWindowId,
        parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
        surface: Option<LiveImageWriter>,
    ) -> Box<dyn WebViewHandle>;

    /// Let the engine run, on the UI thread. Called by
    /// [`WebViewRegistry::pump`] when a [`WebViewPumpPayload`] arrives; a
    /// backend whose engine never posts one has nothing to do here.
    fn pump(&mut self) {}
}

// ============================================================
// WebViewRegistry — per-app service (app-state)
// ============================================================

/// Callback the `WebView` widget installs to receive its own backend events.
type EventCallback = Box<dyn FnMut(WebViewEvent, &mut EventContext)>;

struct Registered {
    window_id: TeksiloWindowId,
    callback: EventCallback,
}

struct RegistryState {
    backend: RefCell<Box<dyn WebViewBackend>>,
    callbacks: RefCell<HashMap<WebViewId, Registered>>,
    /// Bumped per `open`, purely for diagnostics.
    open_count: Cell<u64>,
    /// The `(web_view_id, window)` of the delivery currently in flight, if any
    /// (`deliver` removes the callback, runs it, then reinserts). If a
    /// re-entrant `unregister`/`purge_window` hits this id/window while the
    /// callback runs, `delivery_aborted` is set and `deliver` skips the
    /// reinsert — so a since-purged callback is never resurrected even if
    /// widget teardown becomes synchronous. `deliver` is not itself re-entrant
    /// (backend events are posted, not delivered inline), so a single slot
    /// suffices.
    delivering: Cell<Option<(WebViewId, TeksiloWindowId)>>,
    delivery_aborted: Cell<bool>,
}

/// Per-app web-view service. Registered in app-state by
/// `TeksiloAppBuilderWebViewExt::install_web_view` (in the `teksilo` umbrella
/// crate); reachable from any `build()` / handler via
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
                delivering: Cell::new(None),
                delivery_aborted: Cell::new(false),
            }),
        }
    }

    /// How the installed backend's engines show their page.
    pub fn rendering(&self) -> WebViewRendering {
        self.inner.backend.borrow().rendering()
    }

    /// Let the installed backend's engine run. Called by `teksilo-app` on the
    /// UI thread when a [`WebViewPumpPayload`] arrives.
    pub fn pump(&self) {
        self.inner.backend.borrow_mut().pump();
    }

    /// Open an engine view and register the widget's event callback in one
    /// step. Returns the live [`WebViewHandle`] (dropped on widget removal).
    /// `surface` follows the rule of [`WebViewBackend::open`].
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &self,
        web_view_id: WebViewId,
        window_id: TeksiloWindowId,
        parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
        surface: Option<LiveImageWriter>,
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
            .open(web_view_id, window_id, parent, attrs, poster, surface)
    }

    /// Route a backend-produced payload to its registered widget callback.
    /// Called by `teksilo-app` from the `AppEvent::External` arm. Dropped
    /// silently if the callback was already purged (window/widget gone).
    pub fn deliver(&self, payload: WebViewEventPayload, ctx: &mut EventContext) {
        // Take the callback out so the map borrow isn't held while the
        // (re-entrant-capable) callback runs — it may itself open another web
        // view, which inserts. Then put it back via `or_insert`, so a *newer*
        // registration created during the callback wins and is not clobbered.
        //
        // The `delivering` slot guards the one remaining hazard: if the
        // callback synchronously tears the widget/window down (a re-entrant
        // `unregister` / `purge_window` for this id/window), we must NOT
        // resurrect the dead callback. That marks `delivery_aborted`, and we
        // skip the reinsert below. (Today teardown is deferred so this never
        // fires, but the guard makes the invariant hold unconditionally.)
        let entry = self
            .inner
            .callbacks
            .borrow_mut()
            .remove(&payload.web_view_id);
        let Some(mut reg) = entry else {
            return;
        };
        if reg.window_id != payload.window_id_owner {
            // Stale routing — drop, don't reinsert.
            return;
        }
        self.inner
            .delivering
            .set(Some((payload.web_view_id, reg.window_id)));
        self.inner.delivery_aborted.set(false);

        (reg.callback)(payload.event, ctx);

        self.inner.delivering.set(None);
        if !self.inner.delivery_aborted.get() {
            self.inner
                .callbacks
                .borrow_mut()
                .entry(payload.web_view_id)
                .or_insert(reg);
        }
    }

    /// Drop the registration for a single web view (widget removed).
    pub fn unregister(&self, web_view_id: WebViewId) {
        self.inner.callbacks.borrow_mut().remove(&web_view_id);
        if matches!(self.inner.delivering.get(), Some((id, _)) if id == web_view_id) {
            self.inner.delivery_aborted.set(true);
        }
    }

    /// Drop every registration owned by `window_id`. Called by
    /// `teksilo-app`'s window-close path so callbacks capturing widget state
    /// cannot fire into a torn-down tree. Mirrors
    /// `FileDialogHandle::purge_window`.
    pub fn purge_window(&self, window_id: TeksiloWindowId) {
        self.inner
            .callbacks
            .borrow_mut()
            .retain(|_, r| r.window_id != window_id);
        if matches!(self.inner.delivering.get(), Some((_, win)) if win == window_id) {
            self.inner.delivery_aborted.set(true);
        }
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
    Open {
        web_view_id: WebViewId,
        /// Whether the widget handed the backend a picture to draw into.
        surface: bool,
    },
    SetBounds {
        web_view_id: WebViewId,
        bounds: Rect,
    },
    LoadUrl {
        web_view_id: WebViewId,
        url: String,
    },
    LoadHtml {
        web_view_id: WebViewId,
    },
    Eval {
        web_view_id: WebViewId,
        script: String,
    },
    PostMessage {
        web_view_id: WebViewId,
        msg: String,
    },
    Reload {
        web_view_id: WebViewId,
    },
    GoBack {
        web_view_id: WebViewId,
    },
    GoForward {
        web_view_id: WebViewId,
    },
    Stop {
        web_view_id: WebViewId,
    },
    SetVisible {
        web_view_id: WebViewId,
        visible: bool,
    },
    SetFocus {
        web_view_id: WebViewId,
    },
    SetInputPassthrough {
        web_view_id: WebViewId,
        passthrough: bool,
    },
    OpenDevtools {
        web_view_id: WebViewId,
    },
    CloseDevtools {
        web_view_id: WebViewId,
    },
    Blur {
        web_view_id: WebViewId,
    },
    /// Input forwarded with [`WebViewHandle::send_input`].
    Input {
        web_view_id: WebViewId,
        event: WebViewInputEvent,
    },
    Dropped {
        web_view_id: WebViewId,
    },
}

/// Shared, cloneable recorder. Both the backend and the test hold a clone, so
/// the test can read the op log after driving the tree.
#[derive(Clone, Default)]
pub struct MemoryWebViewRecords {
    ops: Rc<RefCell<Vec<WebViewOp>>>,
    /// The scale factor of each `set_bounds`, beside its op: kept apart so
    /// `WebViewOp::SetBounds` stays scale-independent.
    scales: Rc<RefCell<Vec<(WebViewId, f32)>>>,
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

    /// The input forwarded to a web view, in order.
    pub fn input_log(&self, id: WebViewId) -> Vec<WebViewInputEvent> {
        self.ops
            .borrow()
            .iter()
            .filter_map(|op| match op {
                WebViewOp::Input { web_view_id, event } if *web_view_id == id => {
                    Some(event.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The ordered scale factors the `set_bounds` of a web view were given,
    /// one per [`WebViewOp::SetBounds`]: what an engine that positions in
    /// device pixels converts the logical bounds with.
    pub fn scale_log(&self, id: WebViewId) -> Vec<f32> {
        self.scales
            .borrow()
            .iter()
            .filter(|(web_view_id, _)| *web_view_id == id)
            .map(|(_, scale)| *scale)
            .collect()
    }

    fn push(&self, op: WebViewOp) {
        self.ops.borrow_mut().push(op);
    }
}

fn op_web_view_id(op: &WebViewOp) -> WebViewId {
    match op {
        WebViewOp::Open { web_view_id, .. }
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
        | WebViewOp::SetInputPassthrough { web_view_id, .. }
        | WebViewOp::OpenDevtools { web_view_id }
        | WebViewOp::CloseDevtools { web_view_id }
        | WebViewOp::Blur { web_view_id }
        | WebViewOp::Input { web_view_id, .. }
        | WebViewOp::Dropped { web_view_id } => *web_view_id,
    }
}

/// In-memory deterministic backend for headless tests. Records every op into a
/// shared [`MemoryWebViewRecords`]. Mirrors `MemoryFileDialog`.
///
/// [`new`](Self::new) behaves as a [`WebViewRendering::NativeSubview`] engine
/// and draws nothing. [`surface`](Self::surface) behaves as a
/// [`WebViewRendering::Surface`] engine: each `set_bounds` commits one opaque
/// frame of the bounds' device-pixel size, the way an engine answers a resize,
/// so a test can lay the widget out against a frame and check where input
/// lands.
pub struct MemoryWebViewBackend {
    records: MemoryWebViewRecords,
    surface: bool,
}

/// The colour of every frame a surface [`MemoryWebViewBackend`] commits:
/// opaque, and neither black nor white, so it is told apart from a cleared
/// buffer.
pub const MEMORY_SURFACE_COLOUR: [u8; 4] = [0x33, 0x66, 0x99, 0xff];

impl MemoryWebViewBackend {
    /// A native-subview backend plus its shared recorder; clone the returned
    /// records before moving the backend into a [`WebViewRegistry`].
    pub fn new() -> (Self, MemoryWebViewRecords) {
        let records = MemoryWebViewRecords::default();
        (
            Self {
                records: records.clone(),
                surface: false,
            },
            records,
        )
    }

    /// A surface backend plus its shared recorder.
    pub fn surface() -> (Self, MemoryWebViewRecords) {
        let (mut backend, records) = Self::new();
        backend.surface = true;
        (backend, records)
    }
}

struct MemoryWebViewHandle {
    web_view_id: WebViewId,
    records: MemoryWebViewRecords,
    surface: Option<LiveImageWriter>,
}

impl WebViewHandle for MemoryWebViewHandle {
    fn set_bounds(&self, bounds: Rect, scale_factor: f32) {
        // Record logical bounds (scale-independent) so test assertions stay
        // resolution-agnostic; the scale goes to its own log.
        self.records.push(WebViewOp::SetBounds {
            web_view_id: self.web_view_id,
            bounds,
        });
        self.records
            .scales
            .borrow_mut()
            .push((self.web_view_id, scale_factor));
        if let Some(writer) = &self.surface {
            let width = (bounds.width * scale_factor).round().max(1.0) as u32;
            let height = (bounds.height * scale_factor).round().max(1.0) as u32;
            let frame: Vec<u8> = MEMORY_SURFACE_COLOUR
                .iter()
                .copied()
                .cycle()
                .take(width as usize * height as usize * 4)
                .collect();
            let _ = writer.write_frame(width, height, &frame, width as usize * 4);
        }
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
    fn set_input_passthrough(&self, passthrough: bool) {
        self.records.push(WebViewOp::SetInputPassthrough {
            web_view_id: self.web_view_id,
            passthrough,
        });
    }
    fn open_devtools(&self) {
        self.records.push(WebViewOp::OpenDevtools {
            web_view_id: self.web_view_id,
        });
    }
    fn close_devtools(&self) {
        self.records.push(WebViewOp::CloseDevtools {
            web_view_id: self.web_view_id,
        });
    }
    fn blur(&self) {
        self.records.push(WebViewOp::Blur {
            web_view_id: self.web_view_id,
        });
    }
    fn send_input(&self, event: &WebViewInputEvent) {
        self.records.push(WebViewOp::Input {
            web_view_id: self.web_view_id,
            event: event.clone(),
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
    fn rendering(&self) -> WebViewRendering {
        if self.surface {
            WebViewRendering::Surface {
                orientation: ImageOrientation::Normal,
            }
        } else {
            WebViewRendering::NativeSubview
        }
    }

    fn open(
        &mut self,
        web_view_id: WebViewId,
        _window_id: TeksiloWindowId,
        _parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        _poster: Option<Arc<dyn AppEventPoster>>,
        surface: Option<LiveImageWriter>,
    ) -> Box<dyn WebViewHandle> {
        self.records.push(WebViewOp::Open {
            web_view_id,
            surface: surface.is_some(),
        });
        // Replay the initial source as the corresponding load op so tests can
        // see what the widget asked to display.
        match attrs.source {
            Some(WebSource::Url(url)) => self.records.push(WebViewOp::LoadUrl { web_view_id, url }),
            Some(WebSource::Html { .. }) => self.records.push(WebViewOp::LoadHtml { web_view_id }),
            None => {}
        }
        Box::new(MemoryWebViewHandle {
            web_view_id,
            records: self.records.clone(),
            surface,
        })
    }
}

/// Convenience: a registry backed by a fresh [`MemoryWebViewBackend`], plus
/// its shared recorder. The one-liner headless-test setup.
pub fn memory_registry() -> (WebViewRegistry, MemoryWebViewRecords) {
    let (backend, records) = MemoryWebViewBackend::new();
    (WebViewRegistry::new(backend), records)
}

/// The same with a surface backend ([`MemoryWebViewBackend::surface`]).
pub fn memory_surface_registry() -> (WebViewRegistry, MemoryWebViewRecords) {
    let (backend, records) = MemoryWebViewBackend::surface();
    (WebViewRegistry::new(backend), records)
}

/// A backend that renders nothing and records nothing — every call is a no-op.
///
/// Unlike [`MemoryWebViewBackend`] (which accumulates an unbounded op log for
/// test assertions), this is safe to install in a long-running app as the
/// placeholder default until a native engine backend is wired. Used by
/// `install_web_view_default`.
#[derive(Debug, Default)]
pub struct NoopWebViewBackend;

/// A [`WebViewHandle`] whose every method is a no-op. Returned by
/// [`NoopWebViewBackend`], and by the `WryBackend` / `ServoBackend` engine
/// backends on their failure paths (no parent handle, engine-init error) so a
/// failed open still yields a live, harmless handle. Defined once so a method
/// added to the trait is implemented in exactly one place. `pub(crate)` —
/// backends return it boxed; apps never name it.
pub(crate) struct NoopWebViewHandle;

impl WebViewHandle for NoopWebViewHandle {
    fn set_bounds(&self, _bounds: Rect, _scale_factor: f32) {}
    fn load_url(&self, _url: &str) {}
    fn load_html(&self, _html: &str, _base_url: Option<&str>) {}
    fn eval(&self, _script: &str) {}
    fn post_message(&self, _msg: &str) {}
    fn reload(&self) {}
    fn go_back(&self) {}
    fn go_forward(&self) {}
    fn stop(&self) {}
    fn set_visible(&self, _visible: bool) {}
    fn set_focus(&self) {}
    fn set_input_passthrough(&self, _passthrough: bool) {
        // No surface, so nothing to make transparent.
    }
}

impl WebViewBackend for NoopWebViewBackend {
    fn rendering(&self) -> WebViewRendering {
        WebViewRendering::NativeSubview
    }

    fn open(
        &mut self,
        _web_view_id: WebViewId,
        _window_id: TeksiloWindowId,
        _parent: Option<ParentHandle>,
        _attrs: WebViewAttributes,
        _poster: Option<Arc<dyn AppEventPoster>>,
        _surface: Option<LiveImageWriter>,
    ) -> Box<dyn WebViewHandle> {
        Box::new(NoopWebViewHandle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A surface backend that counts the pumps it is given.
    struct CountingSurfaceBackend(Rc<Cell<u32>>);

    impl WebViewBackend for CountingSurfaceBackend {
        fn rendering(&self) -> WebViewRendering {
            WebViewRendering::Surface {
                orientation: ImageOrientation::FlipVertical,
            }
        }
        fn open(
            &mut self,
            _web_view_id: WebViewId,
            _window_id: TeksiloWindowId,
            _parent: Option<ParentHandle>,
            _attrs: WebViewAttributes,
            _poster: Option<Arc<dyn AppEventPoster>>,
            _surface: Option<LiveImageWriter>,
        ) -> Box<dyn WebViewHandle> {
            Box::new(NoopWebViewHandle)
        }
        fn pump(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn the_registry_answers_for_its_backend_and_passes_pumps_on() {
        let pumps = Rc::new(Cell::new(0));
        let registry = WebViewRegistry::new(CountingSurfaceBackend(pumps.clone()));
        assert_eq!(
            registry.rendering(),
            WebViewRendering::Surface {
                orientation: ImageOrientation::FlipVertical
            }
        );
        registry.pump();
        registry.pump();
        assert_eq!(pumps.get(), 2);
        assert_eq!(
            WebViewRegistry::new(NoopWebViewBackend).rendering(),
            WebViewRendering::NativeSubview
        );
    }
}
