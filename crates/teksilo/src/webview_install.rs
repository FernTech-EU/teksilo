// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `install_web_view` — wire the WebView subsystem into a `TeksiloAppBuilder`.
//!
//! Mirrors the `TeksiloAppBuilderToastExt` pattern: the extension trait and
//! its impl on `TeksiloAppBuilder` live in the `teksilo` umbrella (which
//! depends on both `teksilo-app` and `teksilo-webview`), so the orphan rule is
//! satisfied without `teksilo-webview` having to depend on `teksilo-app`.
//!
//! ```ignore
//! use teksilo::prelude::*;
//!
//! TeksiloAppBuilder::new()
//!     .theme(intui::light())
//!     .install_web_view_default()                   // ← the install
//!     .initial_window(WindowConfig::new()
//!         .root(|tree, _state| tree.add(MyRoot::new())))
//!     .run();
//! ```
//!
//! The install registers a `WebViewRegistry` into `app_state`. Every
//! `WebView` widget reaches it via `ctx.app_state::<WebViewRegistry>()` to
//! open its native engine subview, and `teksilo-app`'s `AppEvent::External`
//! arm routes backend events back through `WebViewRegistry::deliver`.

use teksilo_app::TeksiloAppBuilder;
use teksilo_webview::{WebViewBackend, WebViewRegistry};

/// Extension trait on [`TeksiloAppBuilder`] that wires up the WebView
/// subsystem. Available only with the `web-view` feature on `teksilo`.
pub trait TeksiloAppBuilderWebViewExt {
    /// Install the WebView subsystem with an explicit engine `backend`.
    ///
    /// Use this to supply a native engine backend (`WryBackend` /
    /// `ServoBackend`, behind `teksilo-webview`'s `wry-backend` /
    /// `servo-backend` features) or a custom / mock backend.
    fn install_web_view<B: WebViewBackend + 'static>(self, backend: B) -> Self;

    /// Convenience: install the engine selected by the enabled cargo feature.
    /// wry is the default; Servo is opt-in and additive.
    /// - `teksilo/web-view` → the production `WryBackend` (macOS WKWebView /
    ///   Windows WebView2 / Linux-X11 WebKitGTK). The default engine.
    /// - `teksilo/web-view-servo` (which *implies* `web-view`) → both engines,
    ///   chosen at runtime: `ServoBackend` under a Wayland session (where wry's
    ///   WebKitGTK can't reparent), `WryBackend` everywhere else. The
    ///   recommended Linux setup — ship both, let the session decide. See
    ///   [`is_wayland`](teksilo_webview::is_wayland).
    /// - `teksilo/web-view-headless` (and not `web-view`) → the inert
    ///   [`NoopWebViewBackend`] (renders nothing — headless tests, or apps that
    ///   bring their own backend via [`install_web_view`](Self::install_web_view)).
    ///
    /// (Servo *alone* — no wry fallback — isn't reachable via the umbrella
    /// features by design; depend on `teksilo-webview` directly with
    /// `features = ["servo-backend"]` and pass `ServoBackend` to
    /// [`install_web_view`](Self::install_web_view) if you truly want that.)
    ///
    /// [`NoopWebViewBackend`]: teksilo_webview::NoopWebViewBackend
    fn install_web_view_default(self) -> Self;
}

impl TeksiloAppBuilderWebViewExt for TeksiloAppBuilder {
    fn install_web_view<B: WebViewBackend + 'static>(self, backend: B) -> Self {
        self.app_state(WebViewRegistry::new(backend))
    }

    fn install_web_view_default(self) -> Self {
        // `web-view-servo` implies `web-view` (wry), so both engines are
        // compiled in: pick per session at runtime.
        #[cfg(feature = "web-view-servo")]
        {
            if teksilo_webview::is_wayland() {
                self.app_state(WebViewRegistry::new(teksilo_webview::ServoBackend::new()))
            } else {
                self.app_state(WebViewRegistry::new(teksilo_webview::WryBackend::new()))
            }
        }
        // wry only.
        #[cfg(all(feature = "web-view", not(feature = "web-view-servo")))]
        {
            self.app_state(WebViewRegistry::new(teksilo_webview::WryBackend::new()))
        }
        // No engine (headless / bring-your-own).
        #[cfg(all(feature = "web-view-headless", not(feature = "web-view")))]
        {
            self.app_state(WebViewRegistry::new(teksilo_webview::NoopWebViewBackend))
        }
    }
}
