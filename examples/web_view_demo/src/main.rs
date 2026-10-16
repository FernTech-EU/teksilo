// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `web-view-demo` — showcase for the embeddable [`WebView`] widget.
//!
//! The toolbar says which way the engine shows its page: wry's engines are a
//! native view over the window, Servo's page is drawn inside it.
//!
//! - **Dormancy.** The WebView lives inside a tabbed layout (a `Switcher`):
//!   switching to the "Native UI" tab parks it dormant, and the framework's
//!   activation signal tells the engine to `set_visible(false)`. A native view
//!   is hidden (its visibility cannot ride the wgpu paint pass); a Servo page
//!   stops rendering. Switching back re-shows it with in-page state intact.
//! - **Overlays.** "Popover over the page" opens a popover across the page.
//!   It sits over a Servo page; a native view steps aside while it is open.
//! - **The page.** Its buttons open `alert()`, `confirm()`, `prompt()` and a
//!   `<select>` (on Servo, shown with the application's own dialogs and
//!   lists), send a message to Rust, and offer a text field for typing,
//!   copying and pasting. The toolbar says whether the page holds the
//!   keyboard.
//!
//! Run:
//! - `cargo run -p web-view-demo` — renders via wry (the default engine:
//!   macOS WKWebView / Windows WebView2 / Linux-X11 WebKitGTK).
//! - `cargo run -p web-view-demo --features servo` — also ships Servo, used
//!   under a Wayland session (wry elsewhere).
//!
//! **Wayland note.** wry's WebKitGTK can only embed as a *child window* under
//! X11; on a native Wayland session `build_as_child` fails and the WebView
//! shows its error wash (a pink fill). Servo is the native-Wayland engine. So
//! on a Wayland session this demo forces itself onto XWayland
//! (`GDK_BACKEND=x11`) so wry can embed, unless built `--features servo`,
//! where the runtime picks Servo instead.

use teksilo::canvas::Path;
use teksilo::core::binding::BindingLevel;
use teksilo::prelude::*;
use teksilo::web_view::{PageLoadState, WebView, WebViewRegistry, WebViewRendering};
use teksilo::widgets::{
    Button, Expand, HStack, IconWidget, PopoverButton, Spacer, Switcher, TextWidget, Toolbar,
    ToolbarAction, ToolbarItem, VStack,
};

fn main() {
    force_xwayland_for_wry();

    // On Linux, wry's WebKitGTK runs on the GLib main loop; winit doesn't pump
    // it, so the page never paints unless we drive it each turn. Hold the poll
    // source high and pump GTK every tick. No-op off Linux / without wry.
    let poll = std::rc::Rc::new(std::cell::Cell::new(true));

    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .theme(intui::light())
        .on_loop_tick(poll.clone(), || {
            teksilo::web_view::pump_gtk_events();
            false
        })
        .install_inspector_in_debug()
        .install_web_view_default()
        .initial_window(
            WindowConfig::new()
                .title("Teksilo — WebView demo")
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
                    let web_view = WebView::new()
                        .html(include_str!("../assets/index.html"))
                        .devtools(cfg!(debug_assertions))
                        .url_signal(url.clone())
                        .title_signal(title.clone())
                        .on_message(|msg, _ctx| {
                            println!("JS → Rust: {msg}");
                        })
                        // The page's console (Servo) and the backend's own reports.
                        .on_console_message(|level, text, _ctx| {
                            println!("console {level:?}: {text}");
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
                        });
                    let page_focused = web_view.page_focused_signal();
                    let webview_id = tree.add(web_view);

                    // --- Which way the installed engine shows its page ---
                    let drawn_in_window = tree
                        .app_context()
                        .app_state::<WebViewRegistry>()
                        .is_some_and(|r| matches!(r.rendering(), WebViewRendering::Surface { .. }));
                    let engine_label = TextWidget::new(if drawn_in_window {
                        lit!("Servo: page drawn in the window")
                    } else {
                        lit!("wry: page in a native view")
                    });

                    // --- Tab buttons ---
                    let browser_tab = {
                        let sel = selected.clone();
                        Button::new(lit!("Browser")).on_activate_fn(move |_| sel.set(0))
                    };
                    let native_tab = {
                        let sel = selected.clone();
                        Button::new(lit!("Native UI")).on_activate_fn(move |_| sel.set(1))
                    };

                    // --- Toolbar ---
                    // The tabs are pinned; the commands collapse into the
                    // toolbar's overflow menu when the window is too narrow.
                    // Programmatic two-way navigation: setting the bound URL
                    // signal drives `load_url` (the engine's own echo is
                    // filtered, so this doesn't loop).
                    let load_example = {
                        let url = url.clone();
                        move |_: &mut EventContext| url.set("https://example.com".into())
                    };
                    let toolbar = Toolbar::new()
                        .label(lit!("Web view"))
                        .item(ToolbarItem::custom(browser_tab))
                        .item(ToolbarItem::custom(native_tab))
                        .item(ToolbarItem::separator())
                        .item(command(
                            "◀",
                            "Back",
                            IconWidget::chevron_left,
                            on_page(webview_id, |w| w.go_back()),
                        ))
                        .item(command(
                            "▶",
                            "Forward",
                            IconWidget::chevron_right,
                            on_page(webview_id, |w| w.go_forward()),
                        ))
                        .item(command(
                            "↻",
                            "Reload",
                            no_icon,
                            on_page(webview_id, |w| w.reload()),
                        ))
                        .item(command(
                            "Send to JS",
                            "Send to JS",
                            no_icon,
                            on_page(webview_id, |w| w.post_message(r#"{"from":"rust"}"#)),
                        ))
                        // Runtime DevTools toggle (debug builds; no-op where unsupported).
                        .item(command(
                            "DevTools",
                            "DevTools",
                            no_icon,
                            on_page(webview_id, |w| w.open_devtools()),
                        ))
                        .item(command(
                            "Load example.com",
                            "Load example.com",
                            no_icon,
                            load_example,
                        ));

                    // --- URL display (reactive) and loading indicator ---
                    let url_label = TextWidget::new(lit!("")).text(url.clone()).single_line();
                    let loading_label = TextWidget::new(lit!(""))
                        .text(loading.map(|l| if *l { "⏳" } else { "" }.to_string()));

                    // --- Status line: dormancy and who holds the keyboard ---
                    let status = selected.zip(&page_focused).map(|(s, focused)| {
                        let tab = if *s == 0 {
                            "Browser tab: page shown"
                        } else {
                            "Native UI tab: page parked and hidden"
                        };
                        let keys = if *focused {
                            "The page holds the keyboard"
                        } else {
                            "The app holds the keyboard"
                        };
                        format!("{tab}. {keys}.")
                    });
                    let status_label = TextWidget::new(lit!("")).text(status).single_line();

                    // --- An overlay across the page ---
                    let popover = PopoverButton::new(Button::new(lit!("Popover over the page")))
                        .content(
                            VStack::new()
                                .spacing(4.0)
                                .child(TextWidget::new(lit!("A Teksilo popover.")))
                                .child(TextWidget::new(lit!(
                                    "Over a Servo page it sits on top like over any widget. \
                                     A native view steps aside while it is open."
                                ))),
                        );

                    // --- Native-UI tab content (pure Teksilo) ---
                    let native_panel = VStack::new()
                        .spacing(8.0)
                        .child(TextWidget::new(lit!("This tab is pure Teksilo.")))
                        .child(TextWidget::new(lit!(
                            "Switching here parks the Browser tab's WebView dormant: \
                             a native view is hidden, a Servo page stops rendering."
                        )));

                    // --- Tabbed body ---
                    let body = Switcher::new(selected.clone())
                        .child(webview_id)
                        .child(native_panel);

                    // --- Second row: the single-line labels shrink before the
                    // popover button does ---
                    let status_row = HStack::new()
                        .spacing(16.0)
                        .child(engine_label.single_line())
                        .child(status_label)
                        .child(url_label)
                        .child(loading_label)
                        .child(Spacer::new())
                        .child(popover);

                    tree.add(
                        VStack::new()
                            .spacing(8.0)
                            .child(toolbar)
                            .child(status_row)
                            .child(Expand::new().child(body)),
                    )
                }),
        )
        .run();
}

/// A toolbar command: a text button inline, a labelled menu row once the
/// toolbar has run out of room for it.
fn command(
    text: &'static str,
    label: &'static str,
    icon: fn(f32) -> IconWidget,
    run: impl Fn(&mut EventContext) + Clone + 'static,
) -> ToolbarItem {
    let inline = run.clone();
    ToolbarItem::custom(
        Button::new(lit!(text))
            .tooltip(lit!(label))
            .on_activate_fn(move |ctx| inline(ctx)),
    )
    .overflow_as(ToolbarAction::new(lit!(label), move || icon(16.0)).on_activate(run))
}

/// A handler that drives the page by its widget id.
fn on_page(id: WidgetId, f: fn(&mut WebView)) -> impl Fn(&mut EventContext) + Clone + 'static {
    move |ctx| ctx.with_widget_mut::<WebView>(id, BindingLevel::RepaintOnly, f)
}

/// An empty glyph, for a menu row with no fitting icon.
fn no_icon(size: f32) -> IconWidget {
    IconWidget::from_path(Path::new(), size)
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
