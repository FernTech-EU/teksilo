//! `ServoBackend` — best-effort pure-Rust engine backend (Linux/Wayland) via
//! the [`servo`] crate. Gated behind the `servo-backend` feature.
//!
//! Built against Servo 0.2.0's real embedding API: a [`WindowRenderingContext`]
//! bound to the OS window handle, a [`Servo`] instance, and a [`WebView`]
//! created via [`WebViewBuilder`] with a [`WebViewDelegate`] that translates
//! Servo's `notify_*` callbacks into [`WebViewEvent`]s.
//!
//! # Scope: real API, not yet frame-driven
//!
//! Servo's model differs fundamentally from wry's child-subview: it renders the
//! page into its **own** GL/surfman surface tied to the *whole* window
//! ([`WindowRenderingContext::new`] takes the window's display+window handles,
//! not a sub-rectangle), and it must be pumped by the embedder —
//! `servo.spin_event_loop()` each tick, then `webview.paint()` +
//! `rendering_context.present()` to display, woken via an
//! [`EventLoopWaker`]. None of that is wired into bastyde-app's wgpu render
//! loop yet, and Servo's whole-window GL context conflicts with wgpu owning the
//! same surface.
//!
//! So this backend **constructs** a real Servo webview (the API surface is
//! exercised and compiled), but does not yet drive rendering. Completing it is
//! the plan's Phase 4: route an `EventLoopWaker` to bastyde-app's winit proxy,
//! call `spin_event_loop`/`paint`/`present` from the render loop, and composite
//! Servo's surface as a positioned region rather than the whole window. Until
//! then it is best-effort per the plan, and `WebViewEvent::ConsoleMessage`
//! reports the not-yet-driven state. JS↔Rust IPC (`window.ipc`) is unsupported
//! here (Servo has no built-in IPC channel like wry's `with_ipc_handler`).

use std::rc::Rc;
use std::sync::Arc;

use bastyde_canvas::Rect;
use bastyde_core::AppEventPoster;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;

use dpi::PhysicalSize;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use servo::{
    EventLoopWaker, LoadStatus, RenderingContext, Servo, ServoBuilder, WebView as ServoWebView,
    WebViewBuilder, WebViewDelegate, WindowRenderingContext,
};
use url::Url;

use crate::backend::{
    ConsoleLevel, NoopWebViewHandle, WebSource, WebViewAttributes, WebViewBackend, WebViewEvent,
    WebViewHandle, WebViewId, js_string, post_event,
};

/// Best-effort Wayland engine backend. Construct and hand to
/// `install_web_view(ServoBackend::new())`.
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

/// Report engine-init failure: drive the widget to its Error chrome via
/// `NavigationFinished { success: false }`, plus a console message. Returns a
/// no-op handle. Mirrors `wry_backend::fail`.
fn fail(
    poster: &Option<Arc<dyn AppEventPoster>>,
    window_id: BastydeWindowId,
    web_view_id: WebViewId,
    text: String,
) -> Box<dyn WebViewHandle> {
    post_event(
        poster,
        window_id,
        web_view_id,
        WebViewEvent::ConsoleMessage {
            level: ConsoleLevel::Error,
            text,
        },
    );
    post_event(
        poster,
        window_id,
        web_view_id,
        WebViewEvent::NavigationFinished {
            url: String::new(),
            success: false,
        },
    );
    Box::new(NoopWebViewHandle)
}

impl WebViewBackend for ServoBackend {
    fn open(
        &mut self,
        web_view_id: WebViewId,
        window_id: BastydeWindowId,
        parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
    ) -> Box<dyn WebViewHandle> {
        let Some(parent) = parent else {
            return fail(
                &poster,
                window_id,
                web_view_id,
                "ServoBackend: no parent window handle available; webview not created".to_string(),
            );
        };

        let (Ok(display_handle), Ok(window_handle)) =
            (parent.display_handle(), parent.window_handle())
        else {
            return fail(
                &poster,
                window_id,
                web_view_id,
                "ServoBackend: could not resolve raw window/display handles".to_string(),
            );
        };

        // Servo renders into its own GL surface over the whole window. A real
        // size arrives later via set_bounds → resize; start with a placeholder.
        let initial_size = PhysicalSize::new(800u32, 600u32);
        let rendering_context =
            match WindowRenderingContext::new(display_handle, window_handle, initial_size) {
                Ok(ctx) => Rc::new(ctx),
                Err(e) => {
                    return fail(
                        &poster,
                        window_id,
                        web_view_id,
                        format!("ServoBackend: rendering context init failed: {e:?}"),
                    );
                }
            };
        let _ = rendering_context.make_current();

        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(NoopWaker))
            .build();

        let delegate: Rc<dyn WebViewDelegate> = Rc::new(ServoEventDelegate {
            poster: poster.clone(),
            window_id,
            web_view_id,
        });

        let mut builder = WebViewBuilder::new(&servo, rendering_context.clone()).delegate(delegate);
        if let Some(WebSource::Url(url)) = &attrs.source
            && let Ok(parsed) = Url::parse(url)
        {
            builder = builder.url(parsed);
        }
        let webview = builder.build();

        post_event(
            &poster,
            window_id,
            web_view_id,
            WebViewEvent::ConsoleMessage {
                level: ConsoleLevel::Warn,
                text: "ServoBackend: webview constructed but not yet frame-driven (Phase 4 — \
                       spin_event_loop / paint / present not wired into the render loop)."
                    .to_string(),
            },
        );

        Box::new(ServoHandle {
            _servo: servo,
            _rendering_context: rendering_context,
            webview,
        })
    }
}

/// Translates Servo's delegate callbacks into [`WebViewEvent`]s.
struct ServoEventDelegate {
    poster: Option<Arc<dyn AppEventPoster>>,
    window_id: BastydeWindowId,
    web_view_id: WebViewId,
}

impl WebViewDelegate for ServoEventDelegate {
    fn notify_load_status_changed(&self, webview: ServoWebView, status: LoadStatus) {
        match status {
            LoadStatus::Started => {
                post_event(
                    &self.poster,
                    self.window_id,
                    self.web_view_id,
                    WebViewEvent::PageLoadStarted,
                );
            }
            LoadStatus::HeadParsed => {}
            LoadStatus::Complete => {
                post_event(
                    &self.poster,
                    self.window_id,
                    self.web_view_id,
                    WebViewEvent::PageLoadFinished,
                );
                post_event(
                    &self.poster,
                    self.window_id,
                    self.web_view_id,
                    WebViewEvent::NavigationFinished {
                        url: webview.url().map(|u| u.to_string()).unwrap_or_default(),
                        success: true,
                    },
                );
            }
        }
    }

    fn notify_page_title_changed(&self, _webview: ServoWebView, title: Option<String>) {
        post_event(
            &self.poster,
            self.window_id,
            self.web_view_id,
            WebViewEvent::TitleChanged(title.unwrap_or_default()),
        );
    }
}

/// Live handle owning the Servo instance + its rendering context + the webview.
/// All `!Send` (Rc / GL); the backend only runs on the main thread. Dropping
/// tears everything down (RAII).
struct ServoHandle {
    _servo: Servo,
    _rendering_context: Rc<WindowRenderingContext>,
    webview: ServoWebView,
}

impl WebViewHandle for ServoHandle {
    fn set_bounds(&self, bounds: Rect, scale_factor: f32) {
        // Servo's surface is whole-window; honour size, ignore position (it
        // can't render to a sub-rectangle through this context). Size is in
        // device pixels (logical × scale).
        self.webview.resize(PhysicalSize::new(
            (bounds.width * scale_factor).max(1.0) as u32,
            (bounds.height * scale_factor).max(1.0) as u32,
        ));
    }
    fn load_url(&self, url: &str) {
        if let Ok(parsed) = Url::parse(url) {
            self.webview.load(parsed);
        }
    }
    fn load_html(&self, _html: &str, _base_url: Option<&str>) {
        // No direct inline-HTML load on Servo's WebView; would need a data: URL.
    }
    fn eval(&self, script: &str) {
        self.webview
            .evaluate_javascript(script.to_string(), |_result| {});
    }
    fn post_message(&self, msg: &str) {
        // No built-in IPC channel; emulate Rust→JS via a dispatched event.
        // Uses the shared, fully-escaping js_string (the hand-rolled escaper
        // here previously missed \n / \t / U+2028 / U+2029 → silent SyntaxError).
        self.webview.evaluate_javascript(
            format!(
                "window.dispatchEvent(new MessageEvent('bastyde-message',{{data:{}}}))",
                js_string(msg)
            ),
            |_result| {},
        );
    }
    fn reload(&self) {
        self.webview.reload();
    }
    fn go_back(&self) {
        self.webview.go_back(1);
    }
    fn go_forward(&self) {
        self.webview.go_forward(1);
    }
    fn stop(&self) {
        self.webview
            .evaluate_javascript("window.stop()".to_string(), |_r| {});
    }
    fn set_visible(&self, visible: bool) {
        if visible {
            self.webview.show();
        } else {
            self.webview.hide();
        }
    }
    fn set_focus(&self) {
        self.webview.focus();
    }
}

/// A no-op [`EventLoopWaker`]. A real backend wakes bastyde-app's winit event
/// loop so it calls `spin_event_loop`; that integration is Phase-4 work, so
/// this placeholder lets Servo construct without self-waking.
#[derive(Clone)]
struct NoopWaker;

impl EventLoopWaker for NoopWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(NoopWaker)
    }
    fn wake(&self) {}
}
