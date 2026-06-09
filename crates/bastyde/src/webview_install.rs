//! `install_web_view` — wire the WebView subsystem into a `BastydeAppBuilder`.
//!
//! Mirrors the `BastydeAppBuilderToastExt` pattern: the extension trait and
//! its impl on `BastydeAppBuilder` live in the `bastyde` umbrella (which
//! depends on both `bastyde-app` and `bastyde-webview`), so the orphan rule is
//! satisfied without `bastyde-webview` having to depend on `bastyde-app`.
//!
//! ```ignore
//! use bastyde::prelude::*;
//!
//! BastydeAppBuilder::new()
//!     .theme(intui::light())
//!     .install_web_view_default()                   // ← the install
//!     .initial_window(WindowConfig::new()
//!         .root(|tree, _state| tree.add(MyRoot::new())))
//!     .run();
//! ```
//!
//! The install registers a [`WebViewRegistry`] into `app_state`. Every
//! `WebView` widget reaches it via `ctx.app_state::<WebViewRegistry>()` to
//! open its native engine subview, and `bastyde-app`'s `AppEvent::External`
//! arm routes backend events back through `WebViewRegistry::deliver`.

use bastyde_app::BastydeAppBuilder;
use bastyde_webview::{WebViewBackend, WebViewRegistry};

/// Extension trait on [`BastydeAppBuilder`] that wires up the WebView
/// subsystem. Available only with the `web-view` feature on `bastyde`.
pub trait BastydeAppBuilderWebViewExt {
    /// Install the WebView subsystem with an explicit engine `backend`.
    ///
    /// Use this to supply a native engine backend (`WryBackend` /
    /// `ServoBackend`, behind `bastyde-webview`'s `wry-backend` /
    /// `servo-backend` features) or a custom / mock backend.
    fn install_web_view<B: WebViewBackend + 'static>(self, backend: B) -> Self;

    /// Convenience: install the engine selected by the enabled cargo feature.
    /// wry is the default everywhere it works; Servo is opt-in and additive.
    /// - `bastyde/web-view-wry` → the production `WryBackend` (macOS WKWebView /
    ///   Windows WebView2 / Linux-X11 WebKitGTK).
    /// - `bastyde/web-view-servo` (which *implies* `web-view-wry`) → both
    ///   engines, chosen at runtime: `ServoBackend` under a Wayland session
    ///   (where wry's WebKitGTK can't reparent), `WryBackend` everywhere else.
    ///   The recommended Linux setup — ship both, let the session decide. See
    ///   [`is_wayland`](bastyde_webview::is_wayland).
    /// - plain `bastyde/web-view` → the inert [`NoopWebViewBackend`] (renders
    ///   nothing — for builds that want the widget without bundling an engine,
    ///   and the only safe long-running default vs the op-accumulating
    ///   `MemoryWebViewBackend` used in tests).
    ///
    /// (Servo *alone* — no wry fallback — isn't reachable via the umbrella
    /// features by design; depend on `bastyde-webview` directly with
    /// `features = ["servo-backend"]` and pass `ServoBackend` to
    /// [`install_web_view`](Self::install_web_view) if you truly want that.)
    ///
    /// [`NoopWebViewBackend`]: bastyde_webview::NoopWebViewBackend
    fn install_web_view_default(self) -> Self;
}

impl BastydeAppBuilderWebViewExt for BastydeAppBuilder {
    fn install_web_view<B: WebViewBackend + 'static>(self, backend: B) -> Self {
        self.app_state(WebViewRegistry::new(backend))
    }

    fn install_web_view_default(self) -> Self {
        // Both engines compiled in: pick per session at runtime.
        #[cfg(all(feature = "web-view-wry", feature = "web-view-servo"))]
        {
            if bastyde_webview::is_wayland() {
                self.app_state(WebViewRegistry::new(bastyde_webview::ServoBackend::new()))
            } else {
                self.app_state(WebViewRegistry::new(bastyde_webview::WryBackend::new()))
            }
        }
        #[cfg(all(feature = "web-view-wry", not(feature = "web-view-servo")))]
        {
            self.app_state(WebViewRegistry::new(bastyde_webview::WryBackend::new()))
        }
        #[cfg(all(feature = "web-view-servo", not(feature = "web-view-wry")))]
        {
            self.app_state(WebViewRegistry::new(bastyde_webview::ServoBackend::new()))
        }
        #[cfg(not(any(feature = "web-view-wry", feature = "web-view-servo")))]
        {
            self.app_state(WebViewRegistry::new(bastyde_webview::NoopWebViewBackend))
        }
    }
}
