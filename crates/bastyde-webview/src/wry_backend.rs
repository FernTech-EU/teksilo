//! `WryBackend` — production engine backend (macOS WKWebView / Windows
//! WebView2 / Linux-X11 WebKitGTK) via the `wry` crate.
//!
//! **Scaffold.** This module is gated behind the `wry-backend` feature and is
//! intentionally inert until the spike wires the real engine: the `wry`
//! dependency is commented out in `Cargo.toml`, and the parent-handle
//! round-trip (`ctx.parent_window_handle()` → `WebViewBuilder::build_as_child`)
//! is the first thing to verify on each platform. Today `open` returns a
//! no-op handle that surfaces a `ConsoleMessage` so enabling the feature
//! compiles and runs without silently pretending to render.
//!
//! Implementation checklist (per the plan, phases 2–3):
//! - `WebViewBuilder::new().build_as_child(&parent)` with the `attrs`.
//! - `with_ipc_handler` → `poster.post_external(WebViewEventPayload {
//!   event: WebViewEvent::Message(..) })`.
//! - `with_navigation_handler` returning false to cancel; emit
//!   `NavigationStarted` / `NavigationFinished`.
//! - `with_asynchronous_custom_protocol` for each `attrs.custom_protocols`.
//! - `set_bounds` issued on every layout change (required on Linux-X11).
//! - Linux Wayland: return a handle whose first action posts
//!   `NavigationFinished { success: false }` + a `ConsoleMessage` (use Servo
//!   there instead).

use std::sync::Arc;

use bastyde_core::AppEventPoster;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;

use crate::backend::{
    ConsoleLevel, NoopWebViewHandle, WebViewAttributes, WebViewBackend, WebViewEvent,
    WebViewEventPayload, WebViewHandle, WebViewId,
};

/// Production engine backend. **Not yet wired** — see the module docs.
#[derive(Debug, Default)]
pub struct WryBackend {
    _private: (),
}

impl WryBackend {
    /// Construct the wry backend.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl WebViewBackend for WryBackend {
    fn open(
        &mut self,
        web_view_id: WebViewId,
        window_id: BastydeWindowId,
        _parent: Option<ParentHandle>,
        _attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
    ) -> Box<dyn WebViewHandle> {
        // Announce that the real engine isn't wired yet, so callers see a
        // clear signal rather than a blank, silently-dead view.
        if let Some(poster) = &poster {
            let payload = WebViewEventPayload {
                window_id_owner: window_id,
                web_view_id,
                event: WebViewEvent::ConsoleMessage {
                    level: ConsoleLevel::Warn,
                    text: "WryBackend is a scaffold — the wry engine is not yet wired (enable \
                           after the spike). No page will render."
                        .to_string(),
                },
            };
            poster.post_external(Box::new(payload) as Box<dyn std::any::Any + Send>);
        }
        // Placeholder handle — every call is a no-op until the engine is wired.
        Box::new(NoopWebViewHandle)
    }
}
