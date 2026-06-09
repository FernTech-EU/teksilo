# WebView — Embedded Web Content

`WebView` embeds HTML / web content in a Bastyde window — for documentation
panes, license dialogs, OAuth flows, Markdown previews, help centers,
dashboards, or any HTML/SPA-driven surface. It lives in its own crate,
`bastyde-webview`, behind the umbrella `web-view` feature.

Source: [crates/bastyde-webview/](../crates/bastyde-webview/). Demo:
`cargo run -p web-view-demo`.

```rust
use bastyde::prelude::*;            // brings BastydeAppBuilderWebViewExt into scope
use bastyde::web_view::WebView;

BastydeAppBuilder::new()
    .theme(intui::light())
    .install_web_view_default()     // installs the engine (wry by default)
    .initial_window(WindowConfig::new().title("Docs").size(1000, 720).root(|tree, _| {
        tree.add(
            WebView::new()
                .url("https://example.com")
                .bind_title(title_signal.clone())     // window title follows the page
                .bind_loading(loading_signal.clone())
                .on_message(|msg, _ctx| println!("JS said: {msg}")),
        )
    }))
    .run();
```

## The one widget that can't render into wgpu

Every realistic engine — WKWebView (macOS), WebView2 (Windows), WebKitGTK
(Linux/X11), Servo — owns its own rendering and lives as a **native OS subview
on top of** Bastyde's wgpu surface. `WebView` accepts that and mirrors the
established platform-backend pattern ([`FileDialogBackend`](file-dialog),
`ExternalDndBackend`): a swappable [`WebViewBackend`] creates an engine-specific
[`WebViewHandle`]; a per-app [`WebViewRegistry`] (in `app_state`) routes
JS→Rust / lifecycle events back into the widget tree. The engine is pluggable;
the widget feels native to Bastyde.

Two architectural consequences fall out of "the engine is a native subview":

- **Visibility doesn't ride the paint pass** — see [Dormancy bridge](#the-dormancy--visibility-bridge).
- **Z-order is above wgpu** — see [Z-order](#z-order-with-overlays).

## Engines and feature flags

`bastyde-webview` is engine-agnostic; the engine is chosen by cargo feature on
the umbrella `bastyde` crate. **wry is the default engine.**

| Feature | Engine(s) compiled | `install_web_view_default()` installs |
| --- | --- | --- |
| `web-view` | wry | `WryBackend` (macOS / Windows / Linux-X11) |
| `web-view-servo` (implies `web-view`) | wry **+** Servo | `ServoBackend` under a Wayland session, `WryBackend` everywhere else (runtime, via [`is_wayland`](../crates/bastyde-webview/src/lib.rs)) |
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
  depend on `bastyde-webview` directly with `features = ["servo-backend"]` and
  pass `ServoBackend::new()` to `install_web_view(...)`.

Pinned versions: `wry = 0.55.1`, `servo = 0.2.0`.

## Installing the subsystem

`BastydeAppBuilderWebViewExt` (re-exported through `bastyde::prelude`) adds two
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
    .bind_url(url_signal)              // Signal<String> — TWO-WAY (see below)
    .bind_title(title_signal)         // Signal<String> — updated on title change
    .bind_loading(loading_signal)     // Signal<bool>   — true between page-load start/finish
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

**Two-way `bind_url`.** The engine writes the resolved URL into the bound signal
on navigation-finish, and an external `url_signal.set("…")` drives programmatic
navigation (equivalent to `load_url`). The engine's own echo is filtered, so the
two directions don't loop. The **initial** page comes from `.url()` / `.html()`
/ `.source()`; the signal's value at build time is taken as the baseline and
does not trigger a navigation — `bind_url` governs navigation *after* the first
load. (Don't bind the same signal directly to an editable `TextInput`, or every
keystroke navigates — drive navigation from a "Go" button / Enter handler that
sets the signal instead.)

**Observers, not vetoes.** `on_navigation` and `on_download_*` are notification
callbacks. Bastyde delivers backend events on a later event-loop tick (posted,
not delivered inline), so a synchronous decision can't be returned to the
engine: a navigation cannot be *cancelled* from `on_navigation`
(`NavigationInfo::can_cancel` is always `false` today), and a download's
destination path cannot be redirected from `on_download_started`. Use them for
URL-bar sync, logging, progress UI, and toasts.

**Lifecycle.** `build()` creates the style-driven overlay (loading/error chrome)
and captures the host `BastydeWindowId`; the native engine subview is opened
from a **post-mount [`EventContext`]** (`BuildContext::run_after_mount`) because
that is the only place the OS parent window handle, `app_state`, and the event
poster are all reachable together. Bounds track via `place_children`;
visibility via the activation bridge (below); teardown is RAII — dropping the
[`WebViewHandle`] tears down the native subview.

**Styling.** The overlay chrome is a Tier-3 [`WebViewStyle`]
(`bastyde_core::styles`); the default `RecipeWebViewStyle` paints a state-tinted
wash (loading / error / transparent-when-ready). Override per-call with
`.style(...)` or theme-wide via `theme.style_slots.web_view`.

**Accessibility.** The widget emits a single `Role::WebView` node named from the
title binding; the page's own AT tree is published to the OS by the engine, so
Bastyde does not duplicate it.

## The dormancy / visibility bridge

This is the one place `WebView` breaks a framework invariant, and it is handled
automatically — but worth understanding.

Every ordinary widget composites through the wgpu pass, so "not painted"
*means* "not on screen." A `WebView`'s engine subview lives **outside** that
pass, so when a [`Switcher`](../crates/bastyde-widgets/src/primitives/switcher.rs)
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
- **Rust → JS:** `webview.post_message("…")` dispatches a `bastyde-message`
  `MessageEvent`; the page listens with
  `addEventListener('bastyde-message', e => …)`. `e.data` is the opaque string
  you sent (the app layer decides JSON / MsgPack / plain text).

## Z-order with overlays

Native subviews sit **above** the wgpu surface, so Bastyde overlays (tooltips,
popovers, dropdowns) drawn by the `OverlayManager` render *under* a `WebView`
where they overlap. For overlays that must cover a `WebView`, open them as a
popup OS window via `ctx.open_window(...)` (the approach Electron uses for
context menus over webviews).

## Multi-window & lifetime

- A `WebView` is bound to the `BastydeWindowId` it was mounted in.
- `WindowManager::close_window` purges the window's `WebViewRegistry`
  registrations, so a late backend event can't fire into a torn-down tree.
- Moving a `WebView` between windows is not supported in v1 (matches
  Tauri / Electron).

## Testing

`MemoryWebViewBackend` records every backend op (`open` / `set_bounds` /
`set_visible` / `load_url` / …) into a shared `MemoryWebViewRecords`, with no
GPU / window / engine. The headless suite
([tests/basic_lifecycle.rs](../crates/bastyde-webview/tests/basic_lifecycle.rs))
covers open/teardown, bounds tracking, the headline dormancy assertion — a
`WebView` parked in a real `Switcher` issues `set_visible(false)` on tab-away
and `set_visible(true)` on tab-back — plus two-way `bind_url` navigation,
download-event delivery to the callbacks, and the runtime devtools toggle.
Install it with
`install_web_view(MemoryWebViewBackend::new().0)` (or the `memory_registry()`
one-liner) and pump post-mount opens with `tree.run_mount_actions(&mut NoopWindowOps)`.

## Known limitations

- **Custom-protocol handlers** (`app://` serving local SPAs) are not yet plumbed
  through `WebViewAttributes` — only scheme *names* are carried, no dispatch
  closure. Load local content inline with `.html(...)` for now.
- **Servo backend is real-API but not yet frame-driven** (the plan's Phase 4):
  Servo renders whole-window via GL/surfman and must be pumped by
  `spin_event_loop` / `paint` / `present` + an `EventLoopWaker` wired into
  bastyde-app's render loop — not done yet, and its GL context conflicts with
  wgpu owning the same surface. The backend constructs a real Servo webview and
  reports the not-yet-driven state via a console event. JS→Rust IPC
  (`window.ipc`) is unsupported on Servo (no built-in channel).
- **`load_html` `base_url`** is ignored on wry (no runtime load-HTML API;
  emulated via `document.write`).
- **HiDPI / monitor moves** mid-flight: wry handles its native engines; Servo
  handling is unverified.
- **Memory** of an open WebView with heavy content is non-trivial (~50–150 MB
  for WebView2 / WKWebView); a `WebView` is not a cheap widget.

[`WebViewBackend`]: ../crates/bastyde-webview/src/backend.rs
[`WebViewHandle`]: ../crates/bastyde-webview/src/backend.rs
[`WebViewRegistry`]: ../crates/bastyde-webview/src/backend.rs
[`MemoryWebViewBackend`]: ../crates/bastyde-webview/src/backend.rs
[`WebViewStyle`]: ../crates/bastyde-core/src/styles/web_view_style.rs
[`EventContext`]: ../crates/bastyde-core/src/widget/event_context.rs
[`Switcher`]: ../crates/bastyde-widgets/src/primitives/switcher.rs
