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
//! Per the plan, this is the macOS / Windows / Linux-X11 backend.
//!
//! **Linux requirements.** WebKitGTK runs on the GTK / GLib main loop and can
//! only embed as a child window under **X11**:
//! - GTK must be initialised before the first webview — `open` calls
//!   `gtk::init()` (idempotent, main-thread) since winit doesn't.
//! - The host must pump the GLib loop each turn or the page never paints — see
//!   [`crate::pump_gtk_events`], driven from `BastydeAppBuilder::on_loop_tick`.
//! - The parent must be an X11 window. winit 0.30 picks Wayland whenever
//!   `WAYLAND_DISPLAY` is set, handing wry a Wayland handle it can't embed into
//!   ("window handle kind is not supported"). On a Wayland session, run under
//!   XWayland (unset `WAYLAND_DISPLAY` + `GDK_BACKEND=x11` before winit init),
//!   or use the Servo backend.
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

use wry::{PageLoadEvent, WebView, WebViewBuilder};
// Logical positioning is the macOS / Windows path; Linux converts to Physical.
#[cfg(not(target_os = "linux"))]
use wry::dpi::{LogicalPosition, LogicalSize};

use crate::backend::{
    ConsoleLevel, NoopWebViewHandle, WebSource, WebViewAttributes, WebViewBackend, WebViewEvent,
    WebViewHandle, WebViewId, js_string, post_event,
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

/// Report engine-init failure: drive the widget to its Error chrome via
/// `NavigationFinished { success: false }` (the visual-state signal), plus a
/// console message for diagnostics. Returns a no-op handle so the (now-Some)
/// widget handle stops the open from being retried into the same failure.
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

impl WebViewBackend for WryBackend {
    fn open(
        &mut self,
        web_view_id: WebViewId,
        window_id: BastydeWindowId,
        parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
    ) -> Box<dyn WebViewHandle> {
        // On Linux, WebKitGTK requires GTK to be initialised before any webview
        // is created (and its GLib loop pumped each turn — see
        // `crate::pump_gtk_events`). winit doesn't init GTK, so do it here. It's
        // idempotent and main-thread-only; `open` always runs on the main thread.
        #[cfg(target_os = "linux")]
        if gtk::init().is_err() {
            return fail(
                &poster,
                window_id,
                web_view_id,
                "WryBackend: gtk::init() failed — WebKitGTK needs GTK initialised".to_string(),
            );
        }

        let Some(parent) = parent else {
            // No OS parent handle — can't create a child subview. Surface the
            // Error state so the widget doesn't sit in Loading forever.
            return fail(
                &poster,
                window_id,
                web_view_id,
                "WryBackend: no parent window handle available; webview not created".to_string(),
            );
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
                post_event(
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
                post_event(&poster, window_id, web_view_id, ev);
                // Surface a NavigationFinished on load completion so the
                // url/Ready bindings settle even without a separate nav-finish
                // callback in wry 0.55.
                if matches!(event, PageLoadEvent::Finished) {
                    post_event(
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
                post_event(
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
                post_event(
                    &poster,
                    window_id,
                    web_view_id,
                    WebViewEvent::TitleChanged(title),
                );
            });
        }
        // Downloads are observed, not steered: the started handler returns
        // `true` (allow) with wry's default destination, because the app's
        // callback runs on a later event-loop tick (events are posted, not
        // delivered inline) and so cannot supply a path synchronously. Apps
        // get start/finish notifications for progress UI / toasts.
        {
            let poster = poster.clone();
            builder = builder.with_download_started_handler(move |url, path| {
                post_event(
                    &poster,
                    window_id,
                    web_view_id,
                    WebViewEvent::DownloadStarted {
                        url,
                        suggested_path: path.clone(),
                    },
                );
                true
            });
        }
        {
            let poster = poster.clone();
            builder = builder.with_download_completed_handler(move |_url, path, success| {
                post_event(
                    &poster,
                    window_id,
                    web_view_id,
                    WebViewEvent::DownloadFinished {
                        path: path.unwrap_or_default(),
                        success,
                    },
                );
            });
        }

        match builder.build_as_child(&parent) {
            Ok(webview) => Box::new(WryHandle { webview }),
            Err(e) => fail(
                &poster,
                window_id,
                web_view_id,
                format!("WryBackend: failed to create webview: {e}"),
            ),
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
    fn set_bounds(&self, bounds: Rect, scale_factor: f32) {
        // On Linux the subview is a WebKitGTK child window using integer GDK
        // scaling (GDK_SCALE), which ignores the fractional factor wgpu renders
        // at — so logical bounds land at the wrong place and size. Convert to
        // device pixels (logical × scale) and pass them as Physical so the
        // child lands correctly regardless of GTK's own scale. macOS WKWebView
        // / Windows WebView2 position in logical units and handle DPI
        // themselves, so keep the proven logical path there.
        #[cfg(target_os = "linux")]
        let rect = wry::Rect {
            position: wry::dpi::PhysicalPosition::new(
                (bounds.x * scale_factor) as f64,
                (bounds.y * scale_factor) as f64,
            )
            .into(),
            size: wry::dpi::PhysicalSize::new(
                (bounds.width * scale_factor) as f64,
                (bounds.height * scale_factor) as f64,
            )
            .into(),
        };
        #[cfg(not(target_os = "linux"))]
        let rect = {
            let _ = scale_factor;
            wry::Rect {
                position: LogicalPosition::new(bounds.x, bounds.y).into(),
                size: LogicalSize::new(bounds.width, bounds.height).into(),
            }
        };
        let _ = self.webview.set_bounds(rect);
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
    fn open_devtools(&self) {
        self.webview.open_devtools();
    }
    fn close_devtools(&self) {
        self.webview.close_devtools();
    }
}
