//! `web-view-demo` — showcase for the embeddable [`WebView`] widget.
//!
//! The WebView lives inside a tabbed layout (a `Switcher`) so the demo makes
//! the **dormancy / visibility bridge** visible: switching to the "Native UI"
//! tab parks the WebView dormant, and the framework's activation signal tells
//! the engine to `set_visible(false)` — the one place a native subview's
//! visibility cannot ride the wgpu paint pass. Switching back re-shows it with
//! in-page state intact.
//!
//! Run:
//! - `cargo run -p web-view-demo` — renders via wry (the default engine:
//!   macOS WKWebView / Windows WebView2 / Linux-X11 WebKitGTK).
//! - `cargo run -p web-view-demo --features servo` — also ships Servo, used
//!   under a Wayland session (wry elsewhere).

use bastyde::core::binding::BindingLevel;
use bastyde::prelude::*;
use bastyde::web_view::WebView;
use bastyde::widgets::{Button, Divider, Expand, HStack, Spacer, Switcher, TextWidget, VStack};

fn main() {
    BastydeAppBuilder::new()
        .theme(intui::light())
        .install_inspector_in_debug()
        .install_web_view_default()
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — WebView demo")
                .size(1000, 720)
                .root(|tree, _state| {
                    // --- Reactive state shared across toolbar + view ---
                    let selected = Signal::new(0_usize); // 0 = Browser, 1 = Native UI
                    let url = Signal::new(String::from("app://index.html"));
                    let title = Signal::new(String::from("Loading…"));

                    // --- The WebView (pre-mounted so the toolbar can drive it) ---
                    // Load the bundled page inline. (Custom-protocol *handlers*
                    // aren't plumbed through WebViewAttributes yet, so an
                    // `app://` URL would 404 on a real engine — inline HTML
                    // renders today and the page is self-contained.)
                    let webview_id = tree.add(
                        WebView::new()
                            .html(include_str!("../assets/index.html"))
                            .devtools(cfg!(debug_assertions))
                            .bind_url(url.clone())
                            .bind_title(title.clone())
                            .on_message(|msg, _ctx| {
                                println!("JS → Rust: {msg}");
                            }),
                    );

                    // --- Tab buttons ---
                    let browser_tab = {
                        let sel = selected.clone();
                        Button::new(lit!("Browser")).on_activate_fn(move |_| sel.set(0))
                    };
                    let native_tab = {
                        let sel = selected.clone();
                        Button::new(lit!("Native UI")).on_activate_fn(move |_| sel.set(1))
                    };

                    // --- Navigation buttons (drive the WebView by id) ---
                    let back = Button::new(lit!("◀")).on_activate_fn(move |ctx| {
                        ctx.with_widget_mut::<WebView>(webview_id, BindingLevel::RepaintOnly, |w| {
                            w.go_back()
                        });
                    });
                    let fwd = Button::new(lit!("▶")).on_activate_fn(move |ctx| {
                        ctx.with_widget_mut::<WebView>(webview_id, BindingLevel::RepaintOnly, |w| {
                            w.go_forward()
                        });
                    });
                    let reload = Button::new(lit!("↻")).on_activate_fn(move |ctx| {
                        ctx.with_widget_mut::<WebView>(webview_id, BindingLevel::RepaintOnly, |w| {
                            w.reload()
                        });
                    });
                    let send = Button::new(lit!("Send to JS")).on_activate_fn(move |ctx| {
                        ctx.with_widget_mut::<WebView>(webview_id, BindingLevel::RepaintOnly, |w| {
                            w.post_message(r#"{"from":"rust"}"#)
                        });
                    });

                    // --- URL display (reactive) ---
                    let url_label = TextWidget::new(lit!("")).bind_text(url.clone());

                    // --- Dormancy status line: the visible pass/fail indicator ---
                    let status = selected.map(|s| {
                        if *s == 0 {
                            String::from("Browser tab — WebView subview VISIBLE")
                        } else {
                            String::from("Native UI tab — WebView subview hidden ✓")
                        }
                    });
                    let status_label = TextWidget::new(lit!("")).bind_text(status);

                    // --- Native-UI tab content (pure Bastyde) ---
                    let native_panel = VStack::new()
                        .spacing(8.0)
                        .child(TextWidget::new(lit!("This tab is pure Bastyde.")))
                        .child(TextWidget::new(lit!(
                            "Switching here parks the Browser tab's WebView dormant; \
                             its native subview must disappear."
                        )));

                    // --- Tabbed body ---
                    let body = Switcher::new(selected.clone())
                        .child_id(webview_id)
                        .child(native_panel);

                    // --- Toolbar ---
                    let toolbar = HStack::new()
                        .spacing(8.0)
                        .child(browser_tab)
                        .child(native_tab)
                        .child(Divider::vertical())
                        .child(back)
                        .child(fwd)
                        .child(reload)
                        .child(send)
                        .child(url_label)
                        .child(Spacer::new());

                    tree.add(
                        VStack::new()
                            .spacing(8.0)
                            .child(toolbar)
                            .child(status_label)
                            .child(Expand::new().child(body)),
                    )
                }),
        )
        .run();
}
