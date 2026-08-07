// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `WebView`. See `docs/styling-system.md`.
//!
//! A `WebView` paints almost nothing itself — the actual page is drawn by a
//! native OS engine subview (WKWebView / WebView2 / WebKitGTK / Servo) that
//! sits *on top of* the wgpu surface. What Teksilo owns is the **overlay
//! chrome** the engine surface sits behind/under: the loading shimmer shown
//! before the first paint arrives, an error banner when navigation fails, and
//! the focus ring. This trait themes that chrome.
//!
//! Because the chrome reacts to the page's lifecycle, the config carries a
//! `Signal<WebViewVisualState>` (the same reactive pattern `DropZone` uses)
//! rather than a static state — `make_body` binds the overlay's appearance to
//! it so it updates without a rebuild.

use std::rc::Rc;

use teksilo_tokens::{BorderRole, SurfaceRole};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Lifecycle/visual state of a web view, driving the overlay chrome. Defined
/// here (not in `teksilo-webview`) so the core style trait and the default
/// recipe can both name it — mirroring `DropZoneVisualState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebViewVisualState {
    /// The engine is loading the current page — show the loading shimmer.
    Loading,
    /// The page has finished loading and is displayed by the engine subview.
    /// The overlay is fully transparent; only the focus ring may show.
    Ready,
    /// Navigation or engine init failed — show the error chrome. Also the
    /// state the Wayland-without-Servo / unsupported-platform path lands in.
    Error,
}

impl WebViewVisualState {
    /// Background surface-tint role for the overlay in this state.
    pub fn surface_role(self) -> SurfaceRole {
        match self {
            Self::Loading => SurfaceRole::Sunken,
            Self::Ready => SurfaceRole::Transparent,
            Self::Error => SurfaceRole::StatusError,
        }
    }

    /// Border role for the overlay/focus chrome in this state.
    pub fn border_role(self) -> BorderRole {
        match self {
            Self::Loading => BorderRole::Default,
            Self::Ready => BorderRole::Default,
            Self::Error => BorderRole::Error,
        }
    }
}

/// Inputs handed to a [`WebViewStyle`] to build the web view's overlay chrome.
#[derive(Clone)]
pub struct WebViewStyleConfig {
    /// Reactive lifecycle state — bind overlay surface/border/opacity to it.
    pub state: Signal<WebViewVisualState>,
    /// Pre-built overlay content (e.g. a spinner + status label) the chrome
    /// centers. The native engine surface is composited by the OS on top of
    /// this; the overlay is what shows through before/around the page.
    pub content: WidgetId,
}

/// Tier-3 style protocol for [`WebView`](../../teksilo_webview/struct.WebView.html).
/// Produces the overlay body shown behind/around the native engine surface.
pub trait WebViewStyle: 'static {
    fn make_body(&self, cfg: &WebViewStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

/// Shared, theme-installable handle to a [`WebViewStyle`].
pub type SharedWebViewStyle = Rc<dyn WebViewStyle>;
