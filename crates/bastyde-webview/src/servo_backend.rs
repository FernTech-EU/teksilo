//! `ServoBackend` — best-effort pure-Rust engine backend for Linux/Wayland
//! via the `servo` crate.
//!
//! **Scaffold.** Gated behind the `servo-backend` feature and inert until the
//! spike wires Servo's embedding API (the `servo` dependency is commented out
//! in `Cargo.toml`). Per the plan this is explicitly *best-effort*: each
//! `WebView` API maps to Supported / Best-effort / Unsupported on Servo, and
//! anything the current embedding API can't reach surfaces a
//! `ConsoleMessage { level: Warn }` plus a "Known Limitations" doc entry —
//! not a bug. Today `open` returns a no-op handle that announces the scaffold
//! state.
//!
//! Implementation reference: Servo's `winit_minimal` example —
//! `WindowRenderingContext` bound to the parent winit handle, a
//! `WebViewDelegate` translating Servo callbacks into `WebViewEvent`s.

use std::sync::Arc;

use bastyde_canvas::Rect;
use bastyde_core::AppEventPoster;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;

use crate::backend::{
    ConsoleLevel, WebViewAttributes, WebViewBackend, WebViewEvent, WebViewEventPayload,
    WebViewHandle, WebViewId,
};

/// Best-effort Wayland engine backend. **Not yet wired** — see the module docs.
#[derive(Debug, Default)]
pub struct ServoBackend {
    _private: (),
}

impl ServoBackend {
    /// Construct the Servo backend.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl WebViewBackend for ServoBackend {
    fn open(
        &mut self,
        web_view_id: WebViewId,
        window_id: BastydeWindowId,
        _parent: Option<ParentHandle>,
        _attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
    ) -> Box<dyn WebViewHandle> {
        if let Some(poster) = &poster {
            let payload = WebViewEventPayload {
                window_id_owner: window_id,
                web_view_id,
                event: WebViewEvent::ConsoleMessage {
                    level: ConsoleLevel::Warn,
                    text: "ServoBackend is a scaffold — the Servo engine is not yet wired \
                           (best-effort, enabled after the spike). No page will render."
                        .to_string(),
                },
            };
            poster.post_external(Box::new(payload) as Box<dyn std::any::Any + Send>);
        }
        Box::new(NoopServoHandle)
    }
}

struct NoopServoHandle;

impl WebViewHandle for NoopServoHandle {
    fn set_bounds(&self, _bounds: Rect) {}
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
}
