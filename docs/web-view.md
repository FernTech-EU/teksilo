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
established platform-backend pattern ([`FileDialogBackend`](../crates/teksilo-platform/src/file_dialog.rs),
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

Pinned versions: `wry = 0.56.1`, `servo = 0.5.0`.

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
    .input_mode(WebViewInput::Native)    // who owns the pointer over the page (see below)
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

## Who owns the pointer over the page

A native subview is above the wgpu surface for *input* as well as for pixels: the
OS routes a press over its rectangle to the engine, and Teksilo is not told.
`input_mode(WebViewInput)` is the declaration of which side owns that rectangle.

| | `Native` (default) | `Transparent` |
|---|---|---|
| `touch_action` over the region | `NONE` | unset (`AUTO`) |
| miss-only slop / grip outsets | off (`no_hit_slop`) | as any other widget |
| a pointer event that does reach the node | answered, and the pointer's live sequence revoked | declined, so it bubbles |
| the engine is asked to pass input through | no | yes |

**`Native`** is what a browser-shaped view wants: links, form fields, the page's
own scrolling and its own long-press menus are the page's business, so Teksilo
claims nothing over the region. `touch_action(NONE)` stops a pan, a pinch or a
tree-owned hold forming anywhere on the hit path — an enclosing `ScrollArea` must
not also move under the finger while the page scrolls itself — and `no_hit_slop`
makes the painted rectangle the exact contract in both directions: no neighbouring
control may claim a press that landed on the page, and the page claims none that
missed it.

The third thing `Native` does is tear down a pointer it is handed. A press
Teksilo *does* see over the page is a press it will stop seeing samples for the
moment the engine takes it, and leaving that interaction alive strands whatever
it belonged to — an arbitration waiting for movement that never arrives, a press
record waiting for a release the OS will deliver to the page instead. So the
widget revokes it through the one cancel funnel
(`EventContext::cancel_pointer_sequence`). The reason it uses is `Deactivated`,
the taxonomy's explicit catch-all: the pointer was not revoked by the platform,
by a peer or by a modal — an embedded native surface simply owns it from here on,
and no variant says that. What the widget cannot see is a pointer already
*captured* elsewhere that wanders over the page: a captured pointer's moves route
to its captor, so the crossing is invisible to the web view.

**`Transparent`** is for a view that renders rather than interacts — a document
preview, a rendered chart, a kiosk banner — and it is the mode to reach for when
app widgets, menus or a dialog have to be operable *over* the page. Teksilo then
owns the rectangle: the node widens and bubbles like any other widget, no
sequence is revoked, and the engine is asked to stop taking input
(`WebViewHandle::set_input_passthrough`).

**The engine half is a request, not a guarantee**, and which engine honours it is
a property of that engine's embedding API, not of Teksilo:

| Backend | `set_input_passthrough(true)` |
|---|---|
| `wry` | **Unsupported.** `wry::WebView`'s whole mutating surface is `set_cookie`, `set_background_color`, `set_bounds` and `set_visible`; none of them touches the native surface's hit region. The call is answered with a `WebViewEvent::ConsoleMessage`, this crate's channel for an operation a backend cannot perform. |
| `servo` | Honoured trivially, and inverted: Servo takes input only through `notify_input_event`, which that backend never calls, so its surface already passes everything through. |
| `MemoryWebViewBackend` | Recorded as `WebViewOp::SetInputPassthrough`, which is what lets a headless test assert the widget asked. |
| `NoopWebViewBackend` | No surface, nothing to do. |

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

### Three reasons, one `set_visible`

Dormancy is one of three independent reasons the subview may have to stand down,
and they are resolved into a single call so the engine is never told a visibility
that accounts for only one of them (a page parked in an unselected tab *and*
scrolled out of view must not reappear when only the scroll changes):

1. **Dormant** — the activation signal above.
2. **Clipped away** — nothing clips a subview for us. It is parented to the
   top-level window, so a `WebView` inside a scrolled `ScrollArea` would keep the
   page painted over whatever sits outside the viewport, at full size, for as long
   as it stayed mounted. `place_children` therefore walks the widget's
   `clips_children` ancestors and mirrors the **intersection**: the visible strip
   while some of the page is in view, and `set_visible(false)` once none of it is.
   Mirroring the intersection is the only geometric channel there is — `set_bounds`
   positions and sizes, and no engine here exposes a clip region — so a partly
   clipped page is laid out to the visible strip rather than cropped to it.
3. **Covered by an overlay** — see the next section.

The overlay check is the one thing that cannot be decided in `place_children`:
overlays are positioned *after* the main tree is laid out, so a layout pass reads
the bounds an overlay had before it opened, and nothing marks the tree dirty again
once they are known. The paint walk runs after both and is handed the frame's own
rects, so the check lives in `Widget::after_paint`.

## JS ↔ Rust messaging

- **JS → Rust:** the page calls `window.ipc.postMessage("…")`; it surfaces as
  `on_message(|msg, ctx| …)`. (wry built-in; on Servo this is best-effort.)
- **Rust → JS:** `webview.post_message("…")` dispatches a `teksilo-message`
  `MessageEvent`; the page listens with
  `addEventListener('teksilo-message', e => …)`. `e.data` is the opaque string
  you sent (the app layer decides JSON / MsgPack / plain text).

## Z-order with overlays

Native subviews sit **above** the wgpu surface, so a Teksilo overlay — a tooltip,
a popover, a dropdown, a modal dialog — would render *under* a `WebView` where the
two overlap, and the OS would route a press over that region to the engine rather
than to the overlay.

So the subview **stands down while an interactive overlay covers the page**: the
widget intersects its own bounds with `OverlayManager::interactive_rects()` in
`after_paint` and issues `set_visible(false)` for as long as one of them overlaps,
restoring the page when the overlay goes. That is the only way a menu or a dialog
over a web view is both visible and operable; nothing in the toolkit can reach
over a native child.

Two consequences worth stating. The page is *hidden*, not dimmed — an overlay
covering a corner of a large web view blanks all of it, because visibility is the
only lever the embedding APIs give us. And a fading overlay does not count:
`interactive_rects` excludes overlays whose fade-out has begun, so a dismissing
tooltip gives the page straight back.

An app that wants a Teksilo overlay to sit *beside* a live page rather than
replace it should keep the two regions disjoint, or open the overlay as a popup OS
window via `ctx.open_window(...)` (the approach Electron uses for context menus
over webviews).

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
[tests/input_and_clip.rs](../crates/teksilo-webview/tests/input_and_clip.rs)
covers the input model (both modes, by mouse and by finger, against a fixture
with a tappable ancestor over the page — the only shape in which the two modes
give different answers), the pointer teardown, the clip chain, the overlay yield
and the engine-focus hand-off. Both run on the crate's **default** features,
i.e. with no engine at all, so what they assert is the framework half of each
mechanism: whether a real engine honours `set_input_passthrough` is a property of
that engine and no headless test can reach it.
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
  verified here.
- **Engine focus is reported by the page, not by the engine.** A click inside the
  page takes the OS keyboard, and Teksilo's own focus follows it onto the frame —
  otherwise whatever held focus goes on believing it still does, caret blinking.
  The event that says so is `WebViewEvent::EngineFocusChanged(bool)`, published as
  `WebView::page_focused_signal()`. wry exposes no focus-changed callback, so the
  wry backend injects an initialization script that forwards `window`'s `focus` /
  `blur` over the same IPC channel behind a reserved message prefix
  (`FOCUS_IPC_PREFIX`) — a page that posts that exact prefix itself loses the
  message. The *blur* direction moves nothing: it says the page gave the keyboard
  up, not where it went. None of this is exercised by the headless suite; it needs
  a real engine and a display.

[`WebViewBackend`]: ../crates/teksilo-webview/src/backend.rs
[`WebViewHandle`]: ../crates/teksilo-webview/src/backend.rs
[`WebViewRegistry`]: ../crates/teksilo-webview/src/backend.rs
[`MemoryWebViewBackend`]: ../crates/teksilo-webview/src/backend.rs
[`WebViewStyle`]: ../crates/teksilo-core/src/styles/web_view_style.rs
[`EventContext`]: ../crates/teksilo-core/src/widget/event_context.rs
[`Switcher`]: ../crates/teksilo-widgets/src/primitives/switcher.rs
