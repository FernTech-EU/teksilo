// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
//!
//! **Wayland note.** wry's WebKitGTK can only embed as a *child window* under
//! X11; on a native Wayland session `build_as_child` fails and the WebView
//! shows its error wash (a pink fill). Servo is the eventual native-Wayland
//! engine, but its backend isn't frame-driven yet. So on a Wayland session this
//! demo forces itself onto XWayland (`GDK_BACKEND=x11`) so wry can embed —
//! unless built `--features servo`, where the runtime picks Servo instead.

use bastyde::core::binding::BindingLevel;
use bastyde::prelude::*;
use bastyde::web_view::{PageLoadState, WebView};
use bastyde::widgets::{Button, Divider, Expand, HStack, Spacer, Switcher, TextWidget, VStack};

fn main() {
    force_xwayland_for_wry();

    // On Linux, wry's WebKitGTK runs on the GLib main loop; winit doesn't pump
    // it, so the page never paints unless we drive it each turn. Hold the poll
    // source high and pump GTK every tick. No-op off Linux / without wry.
    let poll = std::rc::Rc::new(std::cell::Cell::new(true));

    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .theme(intui::light())
        .on_loop_tick(poll.clone(), || {
            bastyde::web_view::pump_gtk_events();
            false
        })
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
                    let loading = Signal::new(false);

                    // --- The WebView (pre-mounted so the toolbar can drive it) ---
                    // Load the bundled page inline. (Custom-protocol *handlers*
                    // aren't plumbed through WebViewAttributes yet, so an
                    // `app://` URL would 404 on a real engine — inline HTML
                    // renders today and the page is self-contained.)
                    let loading_cb = loading.clone();
                    let webview_id = tree.add(
                        WebView::new()
                            .html(include_str!("../assets/index.html"))
                            .devtools(cfg!(debug_assertions))
                            .bind_url(url.clone())
                            .bind_title(title.clone())
                            .on_message(|msg, _ctx| {
                                println!("JS → Rust: {msg}");
                            })
                            .on_page_load(move |state, _ctx| {
                                loading_cb.set(state == PageLoadState::Started);
                            })
                            .on_navigation(|nav, _ctx| {
                                println!("navigating → {}", nav.url);
                            })
                            .on_download_started(|d, _ctx| {
                                println!("download started: {} → {:?}", d.url, d.suggested_path);
                            })
                            .on_download_finished(|o, _ctx| {
                                println!("download finished (ok={}): {:?}", o.success, o.path);
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
                        ctx.with_widget_mut::<WebView>(
                            webview_id,
                            BindingLevel::RepaintOnly,
                            |w| w.go_back(),
                        );
                    });
                    let fwd = Button::new(lit!("▶")).on_activate_fn(move |ctx| {
                        ctx.with_widget_mut::<WebView>(
                            webview_id,
                            BindingLevel::RepaintOnly,
                            |w| w.go_forward(),
                        );
                    });
                    let reload = Button::new(lit!("↻")).on_activate_fn(move |ctx| {
                        ctx.with_widget_mut::<WebView>(
                            webview_id,
                            BindingLevel::RepaintOnly,
                            |w| w.reload(),
                        );
                    });
                    let send = Button::new(lit!("Send to JS")).on_activate_fn(move |ctx| {
                        ctx.with_widget_mut::<WebView>(
                            webview_id,
                            BindingLevel::RepaintOnly,
                            |w| w.post_message(r#"{"from":"rust"}"#),
                        );
                    });
                    // Runtime DevTools toggle (debug builds; no-op where unsupported).
                    let devtools = Button::new(lit!("DevTools")).on_activate_fn(move |ctx| {
                        ctx.with_widget_mut::<WebView>(
                            webview_id,
                            BindingLevel::RepaintOnly,
                            |w| w.open_devtools(),
                        );
                    });
                    // Programmatic two-way navigation: setting the bound URL signal
                    // drives `load_url` through `bind_url` (the engine's own echo is
                    // filtered, so this doesn't loop).
                    let go_external = {
                        let url = url.clone();
                        Button::new(lit!("Load example.com"))
                            .on_activate_fn(move |_| url.set("https://example.com".into()))
                    };

                    // --- URL display (reactive) ---
                    let url_label = TextWidget::new(lit!("")).bind_text(url.clone());
                    // --- Loading indicator (driven by on_page_load) ---
                    let loading_label = TextWidget::new(lit!(""))
                        .bind_text(loading.map(|l| if *l { "  ⏳" } else { "" }.to_string()));

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
                        .child(devtools)
                        .child(go_external)
                        .child(url_label)
                        .child(loading_label)
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

/// On a Wayland session, force the process onto XWayland so wry's WebKitGTK can
/// embed as a child window (native Wayland has no window reparenting, so
/// `build_as_child` fails there → the WebView's pink error wash). No-op off
/// Linux, when already pinned to a backend, or when built `--features servo`
/// (there the runtime should pick the native Servo engine on Wayland instead).
#[cfg(all(target_os = "linux", not(feature = "servo")))]
fn force_xwayland_for_wry() {
    let on_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    let have_x_display = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
    if on_wayland && have_x_display {
        // winit 0.30 removed WINIT_UNIX_BACKEND; it now picks Wayland purely
        // because WAYLAND_DISPLAY is set. Removing it makes winit fall back to
        // the X11 (XWayland) DISPLAY, and GDK_BACKEND=x11 puts wry's WebKitGTK
        // on X11 too — so the parent handle wry receives is an X11 handle it
        // can embed into.
        // SAFETY: called as the very first statement in `main`, before any
        // winit / GTK / thread initialisation reads the environment.
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::set_var("GDK_BACKEND", "x11");
        }
        eprintln!(
            "web-view-demo: Wayland session detected — switching to XWayland (unset \
             WAYLAND_DISPLAY, GDK_BACKEND=x11) so wry's WebKitGTK can embed. Build with \
             `--features servo` for the native Wayland engine path."
        );
    }
}

/// No-op: not Linux, or built with the Servo engine (runtime picks it on Wayland).
#[cfg(not(all(target_os = "linux", not(feature = "servo"))))]
fn force_xwayland_for_wry() {}
