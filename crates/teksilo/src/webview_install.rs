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
        let registry = if teksilo_webview::is_wayland() {
            WebViewRegistry::new(teksilo_webview::ServoBackend::new())
        } else {
            WebViewRegistry::new(teksilo_webview::WryBackend::new())
        };
        // wry only.
        #[cfg(all(feature = "web-view", not(feature = "web-view-servo")))]
        let registry = WebViewRegistry::new(teksilo_webview::WryBackend::new());
        // No engine (headless / bring-your-own).
        #[cfg(all(feature = "web-view-headless", not(feature = "web-view")))]
        let registry = WebViewRegistry::new(teksilo_webview::NoopWebViewBackend);

        let builder = self.app_state(registry);
        // The dialogs and lists a page opens in an engine that renders
        // offscreen (Servo), shown with the toolkit's own widgets.
        #[cfg(all(feature = "widgets", feature = "i18n"))]
        let builder = builder.app_state(teksilo_webview::SharedWebViewControlPresenter::new(
            presenter::WidgetsControlPresenter,
        ));
        builder
    }
}

#[cfg(all(feature = "widgets", feature = "i18n"))]
pub use presenter::WidgetsControlPresenter;

/// The default [`WebViewControlPresenter`](teksilo_webview::WebViewControlPresenter):
/// a page's `alert`, `confirm` and `prompt` as a `MessageBox` or an
/// `InputDialog`, and a `<select>`'s list as a modal `MenuList`. Their titles
/// say the text is the page's, so it is not mistaken for the application's.
#[cfg(all(feature = "widgets", feature = "i18n"))]
mod presenter {
    use std::cell::RefCell;
    use std::rc::Rc;

    use teksilo_core::modal::ModalRequest;
    use teksilo_core::signal::Signal;
    use teksilo_core::widget::EventContext;
    use teksilo_i18n::LocalizedString;
    use teksilo_webview::controls::WebViewControlReply;
    use teksilo_webview::{WebViewControl, WebViewControlAnswer, WebViewControlPresenter};
    use teksilo_widgets::{
        InputDialog, MenuItem, MenuList, MessageBox, MessageBoxButtons, ModalContainer,
        StandardButton, TextWidget,
    };

    /// Presents a page's dialogs and lists with the toolkit's widgets. What
    /// [`install_web_view_default`](super::TeksiloAppBuilderWebViewExt::install_web_view_default)
    /// installs; an app installing its own backend adds it with
    /// `.app_state(SharedWebViewControlPresenter::new(WidgetsControlPresenter))`.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct WidgetsControlPresenter;

    /// `reply` callable from every path that may close a control, answering
    /// with the first.
    fn once(reply: WebViewControlReply) -> impl Fn(WebViewControlAnswer) + Clone + 'static {
        let slot = Rc::new(RefCell::new(Some(reply)));
        move |answer| {
            if let Some(reply) = slot.borrow_mut().take() {
                reply(answer);
            }
        }
    }

    fn text(s: impl Into<String>) -> LocalizedString {
        LocalizedString::literal(s)
    }

    impl WebViewControlPresenter for WidgetsControlPresenter {
        fn present(
            &self,
            control: WebViewControl,
            ctx: &mut EventContext,
            reply: WebViewControlReply,
        ) {
            let reply = once(reply);
            match control {
                WebViewControl::Alert { message } => MessageBox::information(text("The page says"))
                    .text(text(message))
                    .buttons(MessageBoxButtons::Ok)
                    .on_result(move |_, _| reply(WebViewControlAnswer::Confirmed))
                    .present(ctx),
                WebViewControl::Confirm { message } => MessageBox::question(text("The page asks"))
                    .text(text(message))
                    .buttons(MessageBoxButtons::OkCancel)
                    .on_result(move |result, _| {
                        reply(if result.button == StandardButton::Ok {
                            WebViewControlAnswer::Confirmed
                        } else {
                            WebViewControlAnswer::Dismissed
                        })
                    })
                    .present(ctx),
                WebViewControl::Prompt { message, default } => {
                    InputDialog::new(text("The page asks"))
                        .prompt(text(message))
                        .default_text(default)
                        .on_result(move |answer, _| {
                            reply(answer.map_or(
                                WebViewControlAnswer::Dismissed,
                                WebViewControlAnswer::Text,
                            ))
                        })
                        .present(ctx)
                }
                WebViewControl::Select {
                    options, selected, ..
                } => {
                    let mut list = MenuList::new().max_visible_items(12);
                    let mut group: Option<String> = None;
                    for option in options {
                        if option.group != group {
                            if let Some(label) = &option.group {
                                list = list.header(TextWidget::new(text(label.clone())));
                            }
                            group = option.group.clone();
                        }
                        let chosen = reply.clone();
                        let id = option.id;
                        list = list.item(
                            MenuItem::new(text(option.label))
                                .enabled(!option.disabled)
                                .reflect_checked(Signal::new(selected == Some(id)))
                                .on_activate_fn(move |ctx| {
                                    chosen(WebViewControlAnswer::Selected(id));
                                    ctx.dismiss_modal();
                                }),
                        );
                    }
                    let dismissed = reply.clone();
                    ctx.present_modal(
                        ModalRequest::deferred(move |tree| tree.add(ModalContainer::new(list)))
                            .on_dismiss(Rc::new(move |_reason, _ctx| {
                                dismissed(WebViewControlAnswer::Dismissed)
                            })),
                    );
                }
                _ => reply(WebViewControlAnswer::Dismissed),
            }
        }
    }
}
