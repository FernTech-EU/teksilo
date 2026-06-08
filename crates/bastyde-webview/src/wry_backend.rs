//! `WryBackend` — production engine backend (macOS WKWebView / Windows
//! WebView2 / Linux-X11 WebKitGTK) via the [`wry`] crate.
//!
//! Gated behind the `wry-backend` feature. The native engine subview is
//! created with [`WebViewBuilder::build_as_child`] against the OS parent
//! window handle that the `WebView` widget hands us from its post-mount
//! [`EventContext`](bastyde_core::widget::EventContext) (see
//! `BuildContext::run_after_mount`). Browser events (IPC messages, page-load
//! status, title changes, navigations) are translated into [`WebViewEvent`]s
//! and posted back through the supplied [`AppEventPoster`].
//!
//! Per the plan, this is the macOS / Windows / Linux-X11 backend. On
//! Linux/Wayland `build_as_child` is unsupported by WebKitGTK (X11 reparenting
//! only) — use the Servo backend there.
//!
//! **Known gaps (tracked):** custom-protocol *handlers* are not yet plumbed
//! through `WebViewAttributes` (only scheme names are carried), so `app://`
//! style local serving is not wired here; `go_back`/`go_forward` are driven
//! via `history.back()/forward()` (wry 0.55 exposes no direct history API).

use std::sync::Arc;

use bastyde_canvas::Rect;
use bastyde_core::AppEventPoster;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;

use wry::{
    PageLoadEvent, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
};

use crate::backend::{
    ConsoleLevel, NoopWebViewHandle, WebSource, WebViewAttributes, WebViewBackend, WebViewEvent,
    WebViewEventPayload, WebViewHandle, WebViewId,
};

/// Production engine backend. Construct and hand to
/// `install_web_view(WryBackend::new())`.
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

/// Post a [`WebViewEvent`] back to the UI loop, if a poster is available.
fn post(
    poster: &Option<Arc<dyn AppEventPoster>>,
    window_id: BastydeWindowId,
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

impl WebViewBackend for WryBackend {
    fn open(
        &mut self,
        web_view_id: WebViewId,
        window_id: BastydeWindowId,
        parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
    ) -> Box<dyn WebViewHandle> {
        let Some(parent) = parent else {
            // No OS parent handle (e.g. opened before the window-ops sink was
            // available). Can't create a child subview — report and no-op.
            post(
                &poster,
                window_id,
                web_view_id,
                WebViewEvent::ConsoleMessage {
                    level: ConsoleLevel::Warn,
                    text: "WryBackend: no parent window handle available; webview not created"
                        .to_string(),
                },
            );
            return Box::new(NoopWebViewHandle);
        };

        let mut builder = WebViewBuilder::new();

        match &attrs.source {
            Some(WebSource::Url(url)) => builder = builder.with_url(url.clone()),
            Some(WebSource::Html { html, .. }) => builder = builder.with_html(html.clone()),
            None => {}
        }
        if let Some(ua) = &attrs.user_agent {
            builder = builder.with_user_agent(ua.clone());
        }
        builder = builder
            .with_transparent(attrs.transparent)
            .with_devtools(attrs.devtools);

        // --- Browser event handlers → WebViewEvent (each gets its own clone) ---
        {
            let poster = poster.clone();
            builder = builder.with_ipc_handler(move |req| {
                post(
                    &poster,
                    window_id,
                    web_view_id,
                    WebViewEvent::Message(req.body().clone()),
                );
            });
        }
        {
            let poster = poster.clone();
            builder = builder.with_on_page_load_handler(move |event, url| {
                let ev = match event {
                    PageLoadEvent::Started => WebViewEvent::PageLoadStarted,
                    PageLoadEvent::Finished => WebViewEvent::PageLoadFinished,
                };
                post(&poster, window_id, web_view_id, ev);
                // Surface a NavigationFinished on load completion so the
                // url/Ready bindings settle even without a separate nav-finish
                // callback in wry 0.55.
                if matches!(event, PageLoadEvent::Finished) {
                    post(
                        &poster,
                        window_id,
                        web_view_id,
                        WebViewEvent::NavigationFinished { url, success: true },
                    );
                }
            });
        }
        {
            let poster = poster.clone();
            builder = builder.with_navigation_handler(move |url| {
                post(
                    &poster,
                    window_id,
                    web_view_id,
                    WebViewEvent::NavigationStarted {
                        url,
                        can_cancel: false,
                    },
                );
                true // allow — pre-navigation veto is not yet exposed app-side
            });
        }
        {
            let poster = poster.clone();
            builder = builder.with_document_title_changed_handler(move |title| {
                post(
                    &poster,
                    window_id,
                    web_view_id,
                    WebViewEvent::TitleChanged(title),
                );
            });
        }

        match builder.build_as_child(&parent) {
            Ok(webview) => Box::new(WryHandle { webview }),
            Err(e) => {
                post(
                    &poster,
                    window_id,
                    web_view_id,
                    WebViewEvent::ConsoleMessage {
                        level: ConsoleLevel::Error,
                        text: format!("WryBackend: failed to create webview: {e}"),
                    },
                );
                Box::new(NoopWebViewHandle)
            }
        }
    }
}

/// Live handle wrapping a `wry::WebView`. `!Send`, but the backend lives in
/// app-state and only ever runs on the main thread, so every call here is
/// main-thread. Dropping the handle drops the `WebView`, tearing the native
/// subview down (RAII).
struct WryHandle {
    webview: WebView,
}

impl WebViewHandle for WryHandle {
    fn set_bounds(&self, bounds: Rect) {
        let _ = self.webview.set_bounds(wry::Rect {
            position: LogicalPosition::new(bounds.x, bounds.y).into(),
            size: LogicalSize::new(bounds.width, bounds.height).into(),
        });
    }
    fn load_url(&self, url: &str) {
        let _ = self.webview.load_url(url);
    }
    fn load_html(&self, html: &str, _base_url: Option<&str>) {
        // wry 0.55 has no runtime load_html; emulate via document.write so a
        // post-open HTML swap still works.
        let _ = self.webview.evaluate_script(&format!(
            "document.open();document.write({});document.close();",
            js_string(html)
        ));
    }
    fn eval(&self, script: &str) {
        let _ = self.webview.evaluate_script(script);
    }
    fn post_message(&self, msg: &str) {
        // Rust → JS: dispatch a `bastyde-message` event carrying `msg` as data.
        let _ = self.webview.evaluate_script(&format!(
            "window.dispatchEvent(new MessageEvent('bastyde-message',{{data:{}}}))",
            js_string(msg)
        ));
    }
    fn reload(&self) {
        let _ = self.webview.reload();
    }
    fn go_back(&self) {
        let _ = self.webview.evaluate_script("history.back()");
    }
    fn go_forward(&self) {
        let _ = self.webview.evaluate_script("history.forward()");
    }
    fn stop(&self) {
        let _ = self.webview.evaluate_script("window.stop()");
    }
    fn set_visible(&self, visible: bool) {
        let _ = self.webview.set_visible(visible);
    }
    fn set_focus(&self) {
        let _ = self.webview.focus();
    }
}

/// Encode `s` as a JavaScript string literal (double-quoted, escaped) so it can
/// be safely interpolated into an `evaluate_script` body.
fn js_string(s: &str) -> String {
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
