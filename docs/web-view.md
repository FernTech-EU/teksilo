<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# WebView — Embedded Web Content

> **Status: prototype.** The **wry** backend (the default) is functional on
> macOS / Windows / Linux-X11 and, via XWayland, on Linux/Wayland. The **Servo**
> backend (the native Wayland path) is **work in progress** — it constructs a
> real engine but is not yet frame-driven, so it does not paint a page. See
> [Servo backend: requirements & status](#servo-backend-requirements--status).

`WebView` embeds HTML / web content in a Teksilo window — for documentation
panes, license dialogs, OAuth flows, Markdown previews, help centers,
dashboards, or any HTML/SPA-driven surface. It lives in its own crate,
`teksilo-webview`, behind the umbrella `web-view` feature.

Source: [crates/teksilo-webview/](../crates/teksilo-webview/). Demo:
`cargo run -p web-view-demo`.

```rust
use teksilo::prelude::*;            // brings TeksiloAppBuilderWebViewExt into scope
use teksilo::web_view::WebView;

TeksiloAppBuilder::new()
    .theme(intui::light())
    .install_web_view_default()     // installs the engine (wry by default)
    .initial_window(WindowConfig::new().title("Docs").size(1000, 720).root(|tree, _| {
        tree.add(
            WebView::new()
                .url("https://example.com")
                .title_signal(title_signal.clone())     // window title follows the page
                .loading_signal(loading_signal.clone())
                .on_message(|msg, _ctx| println!("JS said: {msg}")),
        )
    }))
    .run();
```

## The one widget that can't render into wgpu

Every realistic engine — WKWebView (macOS), WebView2 (Windows), WebKitGTK
(Linux/X11), Servo — owns its own rendering and lives as a **native OS subview
on top of** Teksilo's wgpu surface. `WebView` accepts that and mirrors the
established platform-backend pattern ([`FileDialogBackend`](file-dialog),
`ExternalDndBackend`): a swappable [`WebViewBackend`] creates an engine-specific
[`WebViewHandle`]; a per-app [`WebViewRegistry`] (in `app_state`) routes
JS→Rust / lifecycle events back into the widget tree. The engine is pluggable;
the widget feels native to Teksilo.

Two architectural consequences fall out of "the engine is a native subview":

- **Visibility doesn't ride the paint pass** — see [Dormancy bridge](#the-dormancy--visibility-bridge).
- **Z-order is above wgpu** — see [Z-order](#z-order-with-overlays).

## Engines and feature flags

`teksilo-webview` is engine-agnostic; the engine is chosen by cargo feature on
the umbrella `teksilo` crate. **wry is the default engine.**

| Feature | Engine(s) compiled | `install_web_view_default()` installs |
| --- | --- | --- |
| `web-view` | wry | `WryBackend` (macOS / Windows / Linux-X11) |
| `web-view-servo` (implies `web-view`) | wry **+** Servo | `ServoBackend` under a Wayland session, `WryBackend` everywhere else (runtime, via [`is_wayland`](../crates/teksilo-webview/src/lib.rs)) |
| `web-view-headless` | none | `NoopWebViewBackend` (renders nothing) |

- **wry by default.** Enabling `web-view` gives a working webview with no extra
  flag. `cargo run -p web-view-demo` renders via wry.
- **Servo is additive, Wayland-only at runtime.** `web-view-servo` *implies*
  `web-view`, so it ships both engines; Servo is only selected under Wayland
  (where wry's WebKitGTK can't reparent into a child window). There is no
  "Servo-everywhere" build by design — Servo renders whole-window via GL,
  conflicting with wgpu, and is the wrong engine off Wayland.
- **`web-view-headless`** is the no-engine escape hatch (mirrors
  `file-dialog-trait`): the widget + event routing, the inert no-op backend.
  Use for headless tests, or apps that install their own backend with
  `install_web_view(custom_backend)`.
- **A true Servo-only target** (Linux-only / no-GTK) bypasses the umbrella:
  depend on `teksilo-webview` directly with `features = ["servo-backend"]` and
  pass `ServoBackend::new()` to `install_web_view(...)`.

Pinned versions: `wry = 0.55.1`, `servo = 0.2.0`.

### Linux build dependencies (wry / WebKitGTK)

wry's Linux backend is WebKitGTK, so building anything that enables `web-view`
on Linux (including `web-view-demo`) needs the GTK / WebKit2GTK development
headers. On Debian / Ubuntu:

```bash
sudo apt install libpango1.0-dev libgdk-pixbuf-2.0-dev libatk1.0-dev \
                 libgtk-3-dev libjavascriptcoregtk-4.1-dev libwebkit2gtk-4.1-dev
```

macOS (WKWebView) and Windows (WebView2) need no extra system packages.

#### wry on Linux needs the GTK loop pumped (and X11)

WebKitGTK runs on the GTK / GLib main loop and embeds only as an **X11** child
window. A winit app must therefore, on Linux:

1. **Init GTK** — handled automatically; `WryBackend::open` calls `gtk::init()`.
2. **Pump the GLib loop each turn** — winit doesn't, so the page never paints
   otherwise. Call [`teksilo_webview::pump_gtk_events`] from
   `TeksiloAppBuilder::on_loop_tick`, holding the poll source high while a
   `WebView` is alive:
   ```rust
   let poll = std::rc::Rc::new(std::cell::Cell::new(true));
   TeksiloAppBuilder::new()
       .on_loop_tick(poll.clone(), || { teksilo::web_view::pump_gtk_events(); false })
       // …
   ```
   (`pump_gtk_events` is a no-op off Linux / without the wry engine, so the call
   is portable.)
3. **Run under X11** — winit 0.30 picks Wayland whenever `WAYLAND_DISPLAY` is
   set, and hands wry a Wayland handle it can't embed into. On a Wayland
   session, switch to XWayland *before* the event loop is built (unset
   `WAYLAND_DISPLAY`, set `GDK_BACKEND=x11`), or build `--features servo` for
   the native Wayland engine. `examples/web_view_demo` does this automatically
   (see its `force_xwayland_for_wry`).

The continuous poll (step 2) keeps the loop awake; that is the cost of hosting a
GTK engine inside a winit app today. A future revision may pump only while a
`WebView` is mounted.

## Servo backend: requirements & status

Servo (`servo = 0.2.0`) is the intended **native Wayland** engine — pure Rust,
no GTK reparenting problem. It is **work in progress**: the backend compiles and
constructs a real Servo webview, but it is **not yet frame-driven**, so it does
not paint a page. Building `--features servo` and running on Wayland selects it
(via [`is_wayland`](../crates/teksilo-webview/src/lib.rs)) and you get the
loading wash plus a "constructed but not yet frame-driven" console message — not
web content. For now, use wry + XWayland on Linux.

**Build requirements (Linux).** Servo pulls a large native toolchain on top of
the wry/WebKitGTK deps above. Expect to install (Debian/Ubuntu names; exact set
varies with the Servo release):

```bash
# LLVM/Clang + media + font/graphics stack Servo links against
sudo apt install llvm clang libclang-dev \
                 gstreamer1.0-plugins-base libgstreamer-plugins-base1.0-dev \
                 libgstreamer1.0-dev gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
                 libfontconfig1-dev libfreetype-dev libxcb1-dev libx11-dev \
                 libgl1-mesa-dev libegl1-mesa-dev
```

Servo's own [build setup docs](https://book.servo.org/hacking/setting-up-your-environment.html)
are authoritative; `./mach bootstrap` in a Servo checkout lists the current
system packages for your distro. The first build also downloads and compiles the
**entire Servo tree** — many GB and a long compile.

**What remains (Phase 4).** To make Servo actually render:

1. Wire an `EventLoopWaker` to teksilo-app's winit proxy so Servo gets pumped.
2. Call `servo.spin_event_loop()` + `webview.paint()` +
   `rendering_context.present()` from the render loop.
3. Composite Servo's surface as a **positioned region** rather than the whole
   window — its GL/surfman context currently wants the entire window surface,
   which conflicts with wgpu owning it.

Until then the Servo path is best-effort and documented, not a supported engine.
JS→Rust IPC (`window.ipc`) is also unsupported on Servo (no built-in channel
like wry's `with_ipc_handler`).

## Installing the subsystem

`TeksiloAppBuilderWebViewExt` (re-exported through `teksilo::prelude`) adds two
builder methods:

- `install_web_view_default()` — installs the feature-selected engine (table
  above).
- `install_web_view(backend)` — install an explicit [`WebViewBackend`]
  (a native engine, a custom backend, or [`MemoryWebViewBackend`] for tests).

Both register a [`WebViewRegistry`] in `app_state`; every `WebView` reaches it
via `ctx.app_state::<WebViewRegistry>()`.

## The `WebView` widget

```rust
WebView::new()
    .url("https://example.com")        // OR .html("<!doctype html>…") OR .source(WebSource::*)
    .user_agent("MyApp/1.0")
    .transparent(true)
    .devtools(cfg!(debug_assertions))
    .url_signal(url_signal)              // Signal<String> — TWO-WAY (see below)
    .title_signal(title_signal)         // Signal<String> — updated on title change
    .loading_signal(loading_signal)     // Signal<bool>   — true between page-load start/finish
    .on_message(|msg: String, ctx| { … })       // JS → Rust (window.ipc.postMessage)
    .on_title_changed(|title, ctx| { … })
    .on_navigation(|nav, ctx| { … })             // observer — NavigationInfo (no veto, see below)
    .on_page_load(|state, ctx| { … })            // PageLoadState::{Started, Finished}
    .on_download_started(|d, ctx| { … })         // DownloadStart { url, suggested_path }
    .on_download_finished(|o, ctx| { … })        // DownloadOutcome { path, success }
    .style(MyWebViewStyle)            // Tier-3 overlay chrome override
```

Imperative controls (call via `ctx.with_widget_mut::<WebView>(id, RepaintOnly, |w| …)`):
`load_url`, `eval`, `post_message` (Rust → JS), `reload`, `go_back`,
`go_forward`, `stop`, `open_devtools` / `close_devtools` (runtime toggle; no-op
where unsupported). The stable routing identity is `WebView::id() -> WebViewId`.

**Two-way `url_signal`.** The engine writes the resolved URL into the bound signal
on navigation-finish, and an external `url_signal.set("…")` drives programmatic
navigation (equivalent to `load_url`). The engine's own echo is filtered, so the
two directions don't loop. The **initial** page comes from `.url()` / `.html()`
/ `.source()`; the signal's value at build time is taken as the baseline and
does not trigger a navigation — `url_signal` governs navigation *after* the first
load. (Don't bind the same signal directly to an editable `TextInput`, or every
keystroke navigates — drive navigation from a "Go" button / Enter handler that
sets the signal instead.)

**Observers, not vetoes.** `on_navigation` and `on_download_*` are notification
callbacks. Teksilo delivers backend events on a later event-loop tick (posted,
not delivered inline), so a synchronous decision can't be returned to the
engine: a navigation cannot be *cancelled* from `on_navigation`
(`NavigationInfo::can_cancel` is always `false` today), and a download's
destination path cannot be redirected from `on_download_started`. Use them for
URL-bar sync, logging, progress UI, and toasts.

**Lifecycle.** `build()` creates the style-driven overlay (loading/error chrome)
and captures the host `TeksiloWindowId`; the native engine subview is opened
from a **post-mount [`EventContext`]** (`BuildContext::run_after_mount`) because
that is the only place the OS parent window handle, `app_state`, and the event
poster are all reachable together. Bounds track via `place_children`;
visibility via the activation bridge (below); teardown is RAII — dropping the
[`WebViewHandle`] tears down the native subview.

**Styling.** The overlay chrome is a Tier-3 [`WebViewStyle`]
(`teksilo_core::styles`); the default `RecipeWebViewStyle` paints a state-tinted
wash (loading / error / transparent-when-ready). Override per-call with
`.style(...)` or theme-wide via `theme.style_slots.web_view`.

**Accessibility.** The widget emits a single `Role::WebView` node named from the
title binding; the page's own AT tree is published to the OS by the engine, so
Teksilo does not duplicate it.

**Keyboard: the frame, then the page.** The web view is `focusable`, so Tab
reaches it and the style paints a focus ring around the frame — necessary
because the widget draws no content of its own to show focus on. Landing there
does **not** hand the keyboard to the engine; **Enter** or **Space** does
(`WebViewHandle::set_focus`), and so does an AT-invoked `Click` or `Focus`. Every
other key is declined, so the frame is never a trap: Tab cycles straight off it.

The two-step is deliberate. A `WebView` has **two disjoint focus rings and two
AT trees** — AccessKit's and the engine's platform tree — and once the native
subview owns the keyboard, Teksilo stops receiving keys altogether. An automatic
hand-off on Tab would therefore be a one-way door out of the app's own focus
cycle. Getting back out of an entered page is the engine's and the OS's business,
not something the toolkit can guarantee; this is the same reason the web
platform treats an `<iframe>` as a focus scope you enter rather than fall into.

Apps whose web view **is** the window content can take the one-step form with
`.enter_page_on_focus(true)`. `.focus_page()` is the programmatic equivalent of
Enter, and `.focused_signal()` reports whether the *frame* holds focus (it can
say nothing about what happens once the page has been entered).

A consequence for anyone assembling a conformance artifact: a WebView-embedding
application cannot inherit the toolkit's 2.1.1 or 4.1.2 posture for the page. It
must scope the embedded content separately.

## The dormancy / visibility bridge

This is the one place `WebView` breaks a framework invariant, and it is handled
automatically — but worth understanding.

Every ordinary widget composites through the wgpu pass, so "not painted"
*means* "not on screen." A `WebView`'s engine subview lives **outside** that
pass, so when a [`Switcher`](../crates/teksilo-widgets/src/primitives/switcher.rs)
/ `TabWidget` / `visible_when` gate parks the widget **dormant**, the framework
merely stops painting it — the native surface keeps floating over the output,
showing stale content over whatever is now visible.

`WebView` closes the gap with a framework primitive added for exactly this
case: a per-node **activation signal** (`BuildContext::activation_signal`),
which the arena flips on every `Active↔Dormant` transition (batched at the end
of the visibility pass, mirroring `focus_within`/`hover_within`). The widget
bridges it to the engine: `tab-away → handle.set_visible(false)`,
`tab-back → set_visible(true)`. A `WebView` opened while *already* parked starts
hidden (no flash). This is the only case where a widget must mirror framework
visibility onto an OS resource; any future native-embed widget (video surface,
native map) reuses `activation_signal` the same way.

## JS ↔ Rust messaging

- **JS → Rust:** the page calls `window.ipc.postMessage("…")`; it surfaces as
  `on_message(|msg, ctx| …)`. (wry built-in; on Servo this is best-effort.)
- **Rust → JS:** `webview.post_message("…")` dispatches a `teksilo-message`
  `MessageEvent`; the page listens with
  `addEventListener('teksilo-message', e => …)`. `e.data` is the opaque string
  you sent (the app layer decides JSON / MsgPack / plain text).

## Z-order with overlays

Native subviews sit **above** the wgpu surface, so Teksilo overlays (tooltips,
popovers, dropdowns) drawn by the `OverlayManager` render *under* a `WebView`
where they overlap. For overlays that must cover a `WebView`, open them as a
popup OS window via `ctx.open_window(...)` (the approach Electron uses for
context menus over webviews).

## Multi-window & lifetime

- A `WebView` is bound to the `TeksiloWindowId` it was mounted in.
- `WindowManager::close_window` purges the window's `WebViewRegistry`
  registrations, so a late backend event can't fire into a torn-down tree.
- Moving a `WebView` between windows is not supported in v1 (matches
  Tauri / Electron).

## Testing

`MemoryWebViewBackend` records every backend op (`open` / `set_bounds` /
`set_visible` / `load_url` / …) into a shared `MemoryWebViewRecords`, with no
GPU / window / engine. The headless suite
([tests/basic_lifecycle.rs](../crates/teksilo-webview/tests/basic_lifecycle.rs))
covers open/teardown, bounds tracking, the headline dormancy assertion — a
`WebView` parked in a real `Switcher` issues `set_visible(false)` on tab-away
and `set_visible(true)` on tab-back — plus two-way `url_signal` navigation,
download-event delivery to the callbacks, and the runtime devtools toggle.
Install it with
`install_web_view(MemoryWebViewBackend::new().0)` (or the `memory_registry()`
one-liner) and pump post-mount opens with `tree.run_mount_actions(&mut NoopWindowOps)`.

## Known limitations

- **Custom-protocol handlers** (`app://` serving local SPAs) are not yet plumbed
  through `WebViewAttributes` — only scheme *names* are carried, no dispatch
  closure. Load local content inline with `.html(...)` for now.
- **Servo backend is work in progress** (not yet frame-driven, no render). See
  [Servo backend: requirements & status](#servo-backend-requirements--status)
  for build deps and the remaining Phase-4 work.
- **`load_html` `base_url`** is ignored on wry (no runtime load-HTML API;
  emulated via `document.write`).
- **HiDPI / monitor moves** mid-flight: wry handles its native engines; Servo
  handling is unverified.
- **Memory** of an open WebView with heavy content is non-trivial (~50–150 MB
  for WebView2 / WKWebView); a `WebView` is not a cheap widget.
- **Leaving an entered page** is not under Teksilo's control. Once
  `set_focus()` has handed the keyboard to the engine subview, the toolkit sees
  no further keystrokes, so it cannot offer an escape chord the way a
  `keyboard_capture` surface can. Whether Tab at the end of the document returns
  focus to the host window is engine- and platform-dependent and is **not**
  verified here. Nor is a click on the page mirrored back onto Teksilo's focus
  ring — the native subview receives it directly.

[`WebViewBackend`]: ../crates/teksilo-webview/src/backend.rs
[`WebViewHandle`]: ../crates/teksilo-webview/src/backend.rs
[`WebViewRegistry`]: ../crates/teksilo-webview/src/backend.rs
[`MemoryWebViewBackend`]: ../crates/teksilo-webview/src/backend.rs
[`WebViewStyle`]: ../crates/teksilo-core/src/styles/web_view_style.rs
[`EventContext`]: ../crates/teksilo-core/src/widget/event_context.rs
[`Switcher`]: ../crates/teksilo-widgets/src/primitives/switcher.rs
