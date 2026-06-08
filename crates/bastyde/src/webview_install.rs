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

    /// Convenience: install with the best backend available in this build.
    ///
    /// **Today this registers the headless [`MemoryWebViewBackend`] — it does
    /// not render a real page.** The native `wry` / `servo` backends are
    /// opt-in features on `bastyde-webview` that are not yet wired into this
    /// default (the parent-handle round-trip into `build_as_child` is the
    /// spike's job — see the plan). Apps that need a real engine today pass
    /// one explicitly via [`install_web_view`](Self::install_web_view).
    ///
    /// [`MemoryWebViewBackend`]: bastyde_webview::MemoryWebViewBackend
    fn install_web_view_default(self) -> Self;
}

impl BastydeAppBuilderWebViewExt for BastydeAppBuilder {
    fn install_web_view<B: WebViewBackend + 'static>(self, backend: B) -> Self {
        self.app_state(WebViewRegistry::new(backend))
    }

    fn install_web_view_default(self) -> Self {
        // Inert no-op backend (renders nothing, records nothing) until a native
        // engine feature is wired — safe for a long-running app, unlike the
        // op-accumulating `MemoryWebViewBackend` used in tests.
        self.app_state(WebViewRegistry::new(bastyde_webview::NoopWebViewBackend))
    }
}
