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
    ConsoleLevel, MemoryWebViewBackend, MemoryWebViewRecords, WebSource, WebViewAttributes,
    WebViewBackend, WebViewEvent, WebViewEventPayload, WebViewHandle, WebViewId, WebViewOp,
    WebViewRegistry, memory_registry,
};
pub use recipe_web_view_style::RecipeWebViewStyle;

// Re-export the Tier-3 style surface (the trait lives in bastyde-core so the
// core slot bag can name it, same as every other themable widget).
pub use bastyde_core::styles::{
    SharedWebViewStyle, WebViewStyle, WebViewStyleConfig, WebViewVisualState,
};

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
    last_bounds: Cell<Option<Rect>>,
    opened: Cell<bool>,
    style_override: Option<SharedWebViewStyle>,
    root_child_id: Option<WidgetId>,
    /// Internal lifecycle state driving the overlay chrome.
    state_signal: Signal<WebViewVisualState>,
    /// Registry clone captured at build for `Drop`-time unregistration.
    registry: RefCell<Option<WebViewRegistry>>,

    // Optional outbound (read-only) bindings.
    url_signal: Option<Signal<String>>,
    title_signal: Option<Signal<String>>,
    loading_signal: Option<Signal<bool>>,
    can_go_back: Option<Signal<bool>>,
    can_go_forward: Option<Signal<bool>>,

    // User event callbacks.
    on_message: Option<MessageCallback>,
    on_title_changed: Option<TitleCallback>,
}

impl std::fmt::Debug for WebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebView")
            .field("web_view_id", &self.web_view_id)
            .field("opened", &self.opened.get())
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
            last_bounds: Cell::new(None),
            opened: Cell::new(false),
            style_override: None,
            root_child_id: None,
            state_signal: Signal::new(WebViewVisualState::Loading),
            registry: RefCell::new(None),
            url_signal: None,
            title_signal: None,
            loading_signal: None,
            can_go_back: None,
            can_go_forward: None,
            on_message: None,
            on_title_changed: None,
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

    /// Two-way-ish URL binding: updated when in-page navigation completes.
    /// (Programmatic navigation is via [`load_url`](Self::load_url).)
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

    /// Bind the "can go back" flag (read-only).
    pub fn bind_can_go_back(mut self, signal: Signal<bool>) -> Self {
        self.can_go_back = Some(signal);
        self
    }

    /// Bind the "can go forward" flag (read-only).
    pub fn bind_can_go_forward(mut self, signal: Signal<bool>) -> Self {
        self.can_go_forward = Some(signal);
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
        let can_go_back = self.can_go_back.clone();
        let can_go_forward = self.can_go_forward.clone();
        let state_signal = self.state_signal.clone();
        let on_message = self.on_message.clone();
        let on_title_changed = self.on_title_changed.clone();

        move |event, ctx| match event {
            WebViewEvent::PageLoadStarted => {
                if let Some(s) = &loading_signal {
                    s.set(true);
                }
                state_signal.set(WebViewVisualState::Loading);
            }
            WebViewEvent::PageLoadFinished => {
                if let Some(s) = &loading_signal {
                    s.set(false);
                }
                state_signal.set(WebViewVisualState::Ready);
            }
            WebViewEvent::NavigationFinished { url, success } => {
                if success {
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
            WebViewEvent::DownloadFinished { .. }
            | WebViewEvent::DownloadStarted { .. }
            | WebViewEvent::NavigationStarted { .. }
            | WebViewEvent::ConsoleMessage { .. } => {
                // Surfaced via dedicated callbacks in a later phase; the
                // can_go_* flags are reserved for a history-aware backend.
                let _ = (&can_go_back, &can_go_forward);
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

        // --- Open the native engine subview once, via the app registry ---
        if !self.opened.get()
            && self.handle.borrow().is_none()
            && let Some(registry) = ctx.app_state::<WebViewRegistry>().cloned()
        {
            let window_id = ctx
                .window()
                .map(|w| w.id())
                .unwrap_or_else(|| BastydeWindowId::new(0));
            let poster = ctx.poster().cloned();
            let on_event = self.make_event_callback();
            let handle = registry.open(
                self.web_view_id,
                window_id,
                // Parent handle is not available from BuildContext (only from
                // an app-driven EventContext's window-ops sink). Native
                // backends treat None as "defer until a handle arrives"; the
                // MemoryWebViewBackend ignores it. Wiring the real handle is
                // the spike's job — see docs/plan.
                None,
                self.attrs.clone(),
                poster,
                on_event,
            );
            *self.handle.borrow_mut() = Some(handle);
            self.opened.set(true);
            *self.registry.borrow_mut() = Some(registry);
        }

        // --- Visibility bridge: framework activation → engine set_visible ---
        // The single reason this widget needs the activation signal: a native
        // subview ignores the wgpu paint pass, so a Switcher parking us
        // dormant would otherwise leave the engine surface visible.
        let vis = ctx.activation_signal(self_id);
        let handle = self.handle.clone();
        ctx.effect(&vis, move |active| {
            if let Some(h) = handle.borrow().as_ref() {
                h.set_visible(*active);
            }
        });

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
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
        // Mirror the new bounds onto the native subview when they change.
        // A bounds change always comes from a relayout, so this is the
        // reliable place to track it (paint is coalesced and may be skipped).
        if self.last_bounds.get() != Some(bounds) {
            self.last_bounds.set(Some(bounds));
            self.with_handle(|h| h.set_bounds(bounds));
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
