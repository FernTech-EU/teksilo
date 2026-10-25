<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Multi-Window Reference

Teksilo's multi-window system is **signal-driven** and **synchronous**.
A single `WindowConfig` describes any window you want to open (initial
or runtime); per-window state lives in a reactive
[`WindowState`](../crates/teksilo-core/src/window/state.rs) that widgets
bind against; handlers open, focus, and close windows through
[`EventContext`](../crates/teksilo-core/src/widget/event_context.rs) methods that
return real ids immediately.

Mental model in one line:

```
WindowConfig → (WindowManager::create_window OR ctx.open_window) → (WindowState signals, tree, winit window)
```

Every signal on `WindowState` is two-way: app writes push to the OS,
OS-initiated changes write back into the same signals (re-entrancy
guarded so observers don't echo).

Full end-to-end example:
[`examples/multi_window`](../examples/multi_window/src/main.rs).

---

## Canonical app shape

A Teksilo app opens one initial window via
`TeksiloAppBuilder::initial_window(WindowConfig)` (an app that starts with
no window says so with its quit policy, below). Secondary windows are
opened from handler code via `EventContext::open_window`.

```rust
use teksilo::prelude::*;
use teksilo::app::TeksiloAppBuilder;

fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("My App")
                .size(1200, 800)
                .min_size(640, 400)
                .initial_placement(WindowPlacement::Floating)
                .root(|tree, _state| tree.add(AppRoot::new())),
        )
        .run();
}
```

Notes:

- `TeksiloAppBuilder` has no `.window_title`, `.window_size`, `.root`, or
  `.custom_chrome`, every window is described by `WindowConfig`.
  One conceptual surface, no special-casing for the initial window.
- `root_builder` receives `(tree, WindowState)`, the state clone is
  how a widget can bind against its own window's signals at
  construction time, without going through a `BuildContext`.

### When the app quits: `QuitPolicy`

`TeksiloAppBuilder::quit_policy` decides what closing the last window does:

| Policy | Last window closed | Initial window |
|---|---|---|
| `QuitPolicy::LastWindowClosed` (default) | the app quits | required: `run()` panics without one, since the app would quit before showing anything |
| `QuitPolicy::Explicit` | the app keeps running with no window | optional |

`Explicit` is for an application whose life is not tied to its windows: a
background service, or an application that lives in the system tray. Under
either policy, `ctx.request_quit()` and `ctx.quit()` end the application;
see "Quitting the application" below.

---

## The application tree

Code that must run with no window open (the application menu and its
actions, a single-instance handler, a quit) lives in the **application
tree**: a widget tree that belongs to no window. Give it a root with
`TeksiloAppBuilder::app_root`, which takes the same kind of builder as a
window's root:

```rust
TeksiloAppBuilder::new()
    .quit_policy(QuitPolicy::Explicit)            // live with no window
    .app_root(|tree| tree.add(AppActions::new()))
    .run();

impl Widget for AppActions {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_action_global(Action::new("app.new_window").on_invoke(|_, ctx| {
            ctx.open_window(document_window());
        }));
        ctx.register_action_global(Action::new("app.quit").on_invoke(|_, ctx| {
            ctx.request_quit();
        }));
        // Runs once at startup: an app with no initial window opens its
        // first one here.
        ctx.run_after_mount(|ctx| {
            ctx.open_window(document_window());
        });
        vec![]
    }
    // layout_response: a zero size; the tree is never drawn.
}
```

Without `app_root` the tree still exists, with an empty root of its own.

The tree is never painted. Its handlers run with `ctx.window()` `None`:
everything that names a window by id works (opening, finding and focusing
one, `close_window_by_id`, `request_quit`), and nothing about "this window"
does (`close_window` closes nothing). It shares the application's theme,
locale, text scale, accessibility preferences and app state with the
windows, and a request it makes for an application-wide change
(`set_locale`, `set_theme`, `set_text_scale`) applies everywhere. It is
ticked by the event loop, so its bindings, tweens, idle callbacks and
`request_wake_at` timers run as a window's do.

It receives:

- the intents a window leaves unclaimed: an intent sent in a window (by a
  menu item, a button, `ctx.send_intent`) that no action on its way to that
  window's root and no global action of that window takes is offered to the
  application tree once. So a window's menu can fire `app.new_window`, which
  only the application tree implements. The other way, an intent the
  application tree leaves goes to the window used last (at its frontmost
  native dialog), so its handlers can send a window a command. An intent
  crosses once and never comes back; one nobody takes is dropped, and a debug
  build names it (the framework's own `teksilo.` intents excepted). An intent
  the action that took it sends may cross in its turn, up to 64 crossings in
  one chain, after which a debug build says that two actions seem to answer
  each other. A
  shortcut's intent crosses too, when an enabled action of the application
  tree takes it: Ctrl+N bound in a window to `app.new`, which only the
  application tree implements, opens the window. When the application tree
  does not take it either, the key goes to the focused widget instead, as
  any shortcut nobody handles does (the standard edit shortcuts depend on
  it: Ctrl+C copies in an editor that copies from its own key handler). On
  macOS a chord that is also a native menu item's key equivalent arrives as
  that menu choice, not as a shortcut, and crosses as a menu intent. In a
  test, `HeadlessApp::route_unclaimed_intents` does the crossing;
- `on_external_with_ctx` payloads while no window is open (see the
  single-instance recipe below);
- events of a `subscribe_event_with_ctx` subscription registered in it;
- its own mount actions (`run_after_mount`), at startup and after a rebuild.

What needs a window is not drawn there:

| From the application tree | Happens |
|---|---|
| a modal request (`MessageBox`, `present_modal`) | presented in the window used last, or in a window of its own when none is open; content must be `ModalRequest::deferred` |
| a file dialog | refused with an error: no parent window |
| a toast | shown in every window |
| an overlay, an animated widget | never shown; a debug build says so |
| a tooltip, a `WebView` | never shown |
| `spawn_local_with` | its completion is delivered to windows only, and is lost here; use `spawn_local` and a `Signal` |
| a shortcut registered there | fires in a window when a row of the application menu names it: every window gets a copy (see below). Any other never fires, since keys reach window trees only: register it in each window, and the application tree's action takes it when the window has none (not from inside a dialog presented in the window, where the page's shortcuts do not apply) |

The application tree's actions do not appear in a command palette, which reads
the window's registry; `ShortcutSettings` lists the application menu's
shortcuts through the window's copies.

### The application menu

The application tree declares the application's one menu from its build with
`ctx.set_application_menu(model)` (the `ApplicationMenuExt` trait, in the
prelude). What the application tree registers with `register_action` or
`register_action_global` is the application's action scope: the commands of
the menu's rows that no window takes go there.

```rust
impl Widget for AppActions {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_shortcut_global(
            Shortcut::new("app.new_window").primary(KeyStroke::ctrl(Key::N)).build(),
        );
        ctx.register_action_global(Action::new("app.new_window").on_invoke(|_, ctx| {
            ctx.open_window(document_window());
        }));
        ctx.set_application_menu(MenuModel::new().menu(tr!(file()), |m| {
            m.item(MenuEntry::new(tr!(new_window())).intent("app.new_window").shortcut("app.new_window"))
        }));
        vec![]
    }
}
```

- A window shows it with `MenuBar::application()`, or a menu of its own with
  `MenuBar::from_model(m)`. On Windows and Linux nothing is put in a window by
  itself; on macOS, with `install_native_menu()`, it is the global menu bar
  whenever the key window has no menu of its own, and while no window is open.
- A row's intent is sent from the window's focus: the window's actions take it
  first (Edit ▸ Copy copies in the focused field), the application tree what
  the window leaves. A row's `on_activate` closure runs in the application
  tree, where the signals it captured live, then its intent goes from the
  window's focus.
- A shortcut a row names, registered in the application tree, works in every
  window: each window gets a copy of its declaration unless it registers that
  id itself, owned or shared. A user's rebind of the copy in one window
  applies to the application, so every window follows it. A window's own
  registration of the id, even scoped to one widget, replaces the copy in
  that whole window.
- Register an app-wide command such as Quit or New in the application tree: a
  row's intent goes from the key window's focus and then to the application
  tree, never to another window, so an action only some windows register is
  missed while another window is key (Quit then asks the application to quit
  directly).
- A row with an intent, no closure and no state to write (a two-way check or
  radio row writes one) is greyed out while nothing on that route takes the
  intent.
- Calling `set_application_menu` from a window's tree does nothing, and panics
  in a debug build.

The [native menu guide](native-menu.md) has the details: what macOS shows with
no window open, and the Quit row.

## `WindowConfig`

The single entry point for creating any window. Uniform whether you
pass it to `TeksiloAppBuilder::initial_window` at startup or to
`EventContext::open_window` from a handler.

```rust
pub struct WindowConfig {
    pub title: String,                   // also feeds WindowState::title
    pub string_id: Option<String>,       // stable lookup key for find_window
    pub app_id: Option<String>,          // Wayland xdg app_id / X11 WM_CLASS; ignored elsewhere
    pub size: (u32, u32),                // restored size; always set
    pub position: Option<(i32, i32)>,    // restored position; None = WM picks
    pub min_size: Option<(u32, u32)>,
    pub max_size: Option<(u32, u32)>,
    pub restore_geometry: bool,          // read the saved geometry back? (default true)
    pub initial_placement: WindowPlacement,
    pub decorations: DecorationsMode,
    pub resizable: bool,
    pub size_to_content: SizeToContent,  // Off (default) | Height, dialogs that grow with content
    pub always_on_top: bool,
    pub skip_taskbar: bool,
    pub minimizable: Option<bool>,       // None: no for a modal window, yes otherwise
    pub maximizable: Option<bool>,       // the same
    pub activate_from_env: bool,         // consume an xdg_activation startup token (Wayland)
    pub icon: Option<WindowIcon>,
    pub modal: Option<ModalConfig>,
    pub root_builder: Option<RootBuilder>,
    pub post_root_builder: Option<PostRootBuilder>,
    pub on_close_requested: Option<CloseGuard>,        // see "Intercepting close / quit"
    pub can_close: Option<Prop<bool>>,
    pub on_close_blocked: Option<CloseBlockedCallback>,
    pub on_removed: Option<WindowRemovedCallback>,     // fires once, after teardown
}
```

### Persisting geometry without restoring it

`string_id` normally governs both halves of window-state persistence: a window
with an id has its geometry **saved** on every move/resize, and **restored** at
creation. `restore_geometry` splits them.

They need splitting whenever **several windows share one geometry slot**, a
multi-window (or, like Skribisto, multi-*process*) app that remembers "where the
window was" rather than "where *this document's* window was". Restore the saved
geometry into every window and they all land on the same pixel, stacked. What you
want is:

- the **first** window: restore it, reopen where the user left off;
- any window opened **alongside** it: let the window manager place it (it
  cascades), but **still save** its geometry, so whichever window the user moved
  or closed last is the one that reopens.

That second case is `id(..)` **+** `restore_geometry(false)`:

```rust
WindowConfig::new()
    .id("main")                                   // still persists into this slot
    .restore_geometry(peers.is_empty())           // ...but only the first one restores
```

With `position` left `None`, the WM picks the spot. This is the behaviour of
Word, Firefox and most document apps.

Per-*document* geometry (Scrivener, Sublime, the JetBrains IDEs) is a different
design: it keys the slot on the document, so windows never collide and every one
restores. It only works if the document is known **before** the window is created
,  i.e. a launcher/welcome window that opens a *separate* document window, rather
than a blank window that later loads a document into itself.

Builder form for the common cases:

```rust
WindowConfig::new()
    .title("Inspector")
    .id("inspector")                           // ctx.find_window("inspector") → Some(id)
    .size(420, 640)
    .min_size(320, 400)
    .position(120, 80)
    .initial_placement(WindowPlacement::Floating)
    .decorations(DecorationsMode::CustomChrome)
    .resizable(true)
    .always_on_top(true)                       // floating tool palette
    .skip_taskbar(true)                        // not in the taskbar (Windows, X11) nor the macOS Window menu
    .minimizable(false)                        // ...so nothing to minimise it to (Windows, macOS)
    .icon(WindowIcon::from_rgba(rgba, w, h))
    .root(|tree, state| tree.add(Inspector::new(state)))
```

### `WindowPlacement`

Unified enum for the four top-level placement states every desktop OS
supports:

| Variant | Meaning |
|---|---|
| `Floating` | Regular overlapping window, uses `WindowState::size` / `position` as current geometry |
| `Maximized` | Fills the current monitor's work area |
| `Fullscreen` | Exclusive fullscreen (Space-based on macOS) |
| `Minimized` | Hidden to the taskbar / dock |

Size and position are **not** inside `Floating`. They live on
`WindowState` as their own signals and always hold the last-known
*restored* values, matching macOS `frameAutosaveName` and Windows
`WINDOWPLACEMENT` behavior, so "un-maximize" and "un-fullscreen"
restore the window to the right rect without ambiguity.

Transitions between any two variants are legal; the platform layer
preserves the restored rect as you cross through `Maximized` /
`Fullscreen` / `Minimized`.

### `DecorationsMode`

| Variant | Meaning |
|---|---|
| `Native` | OS-provided title bar, borders, resize handles. Default |
| `CustomChrome` | No native title bar; a `PlatformTitleBarHost` is attached so the app can paint its own. On X11, falls back to `Native` when the window manager lacks `_NET_WM_MOVERESIZE` (see [title-bar.md](title-bar.md)) |
| `None` | Borderless, no host, splash screens, popups, fully chrome-less embeds |

### `ModalConfig`

Modal dialogs are an `Option<ModalConfig>` on `WindowConfig`, not two
separate flags. The type system enforces that a modal always names its
parent:

```rust
.modal(ModalConfig {
    parent: ctx.window().unwrap().id(),
    focus_target: Some(ok_button_id),   // optional explicit initial focus
})
```

Short form when you only need the parent:

```rust
.modal_to(ctx.window().unwrap().id())
```

Modal semantics are preserved from the previous `ModalRequest` path:
input-blocking on the parent, Z-order child-window attachment (an owner
window on Windows, `WM_TRANSIENT_FOR` on X11, AppKit
`addChildWindow:ordered:` on macOS; Wayland has none, since winit offers no
`xdg_toplevel.set_parent`), refocus-on-stolen-focus.

### `WindowIcon`

Raw RGBA8 buffer + dimensions. `width × height × 4` bytes exactly; the
app-level manager validates on creation and logs + drops invalid icons
(the window still opens with the platform default).

```rust
let rgba: Vec<u8> = /* load from disk, decode PNG, … */;
WindowConfig::new().icon(WindowIcon::from_rgba(rgba, 64, 64))
```

---

## `WindowState`

Per-window reactive state. Cloneable handle to an `Rc<WindowStateInner>`
so signals and command queue are shared across clones. Widgets get a
clone from `ctx.window()` (in both `BuildContext` and `EventContext`).

```rust
pub struct WindowState { inner: Rc<WindowStateInner> }

impl WindowState {
    pub fn id(&self) -> TeksiloWindowId;
    pub fn string_id(&self) -> Option<&str>;

    // Writable signals. App-side writes queue a `WindowCommand` to the
    // OS; OS-initiated writes flow back through `*_from_os` setters
    // with a re-entrancy guard.
    pub fn placement(&self)     -> &Signal<WindowPlacement>;
    pub fn title(&self)         -> &Signal<String>;
    pub fn size(&self)          -> &Signal<(u32, u32)>;
    pub fn position(&self)      -> &Signal<(i32, i32)>;
    pub fn resizable(&self)     -> &Signal<bool>;
    pub fn always_on_top(&self) -> &Signal<bool>;

    // OS-driven, read-only in practice: no observer, so an app write
    // reaches no OS call. Pull focus with `focus()` instead.
    pub fn focused(&self)       -> &Signal<bool>;
    pub fn caps_lock(&self)     -> &Signal<bool>;
    pub fn alt_down(&self)      -> &Signal<bool>;

    // Imperative one-shots. Each pushes a single `WindowCommand` on
    // the next drain.
    pub fn request_attention(&self, kind: UserAttentionKind);
    pub fn focus(&self);                       // carries a pending activation token, if any
    pub fn set_activation_token(&self, token: String);   // Wayland raise; consumed by focus()
    pub fn close(&self);
}
```

### Binding widgets to window state

At `build()` time, pick up the state from `ctx.window()` and build
derived signals you pass to widgets:

```rust
impl Widget for AppRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let fs = ctx.window()
            .expect("AppRoot requires a window")
            .placement()
            .map(|p| p.is_fullscreen());

        let label = fs.map(|f| if *f { "Exit fullscreen" } else { "Fullscreen" }.to_string());

        vec![ctx.add(
            Button::new(lit!("Fullscreen"))
                .label(label)                  // Prop<String>; the bound signal wins
                .on_activate_fn(|ctx| {
                    let Some(w) = ctx.window() else { return };
                    let next = if w.placement().get().is_fullscreen() {
                        WindowPlacement::Floating
                    } else {
                        WindowPlacement::Fullscreen
                    };
                    w.placement().set(next);
                }),
        )]
    }
    // ...
}
```

The button label re-renders automatically when fullscreen is toggled,
whether the toggle came from the button itself, the F11 shortcut, or
the user pressing the green traffic light on macOS. All three paths
write into the same `placement()` signal.

### Two-way OS sync: how it works

`WindowState::new` wires an observer to every writable signal
(`focused` has none, it is purely OS-driven). The observer:

1. Checks the `applying_from_os` flag on `WindowStateInner`.
2. If set (OS-initiated write): does nothing, the OS already knows.
3. If unset (app-initiated write): pushes a `WindowCommand` onto the
   shared `pending_os_commands` queue.

Each event-loop tick:

1. `teksilo-app`'s `handle_window_event_inner` translates winit
   `Resized` / `Moved` / `Focused` events into calls like
   `state.set_placement_from_os(new)` / `set_size_from_os(size)`. These
   flip `applying_from_os` to `true` before writing the signal,
   suppressing the observer's outbound echo.
2. After event dispatch, `WindowManager::drain_window_commands` drains
   each live window's command queue and translates each command into
   the corresponding winit call.

This is the re-entrancy guard from [Compose Multiplatform
#1489](https://github.com/JetBrains/compose-multiplatform/issues/1489):
without it, an OS-initiated state change would loop back through the
observer as an app-initiated OS call, desynchronizing OS and app mid-
animation. The guard is the single concrete mechanism that makes
`WindowState` safe as a shared source of truth.

See [`state.rs`](../crates/teksilo-core/src/window/state.rs) for the
implementation; see [`state.rs` tests](../crates/teksilo-core/src/window/state.rs)
for `os_side_write_does_not_enqueue_command` and
`os_side_write_still_notifies_derived_signals`.

---

## `EventContext` multi-window API

Every handler receives a `&mut EventContext` that carries an
`&mut dyn WindowOps` borrowed from the app-level window manager for
the duration of dispatch.

```rust
impl EventContext<'_> {
    pub fn window(&self) -> Option<&WindowState>;
    pub fn open_window(&mut self, config: WindowConfig) -> TeksiloWindowId;
    pub fn open_modal(&mut self, request: ModalRequest) -> Option<TeksiloWindowId>;
    pub fn find_window(&self, string_id: &str) -> Option<TeksiloWindowId>;
    pub fn focus_window(&mut self, id: TeksiloWindowId);
    pub fn close_window(&mut self);                         // current window, GUARDED
    pub fn close_window_forced(&mut self);                  // current window, bypasses the guard
    pub fn close_window_by_id(&mut self, id: TeksiloWindowId);
    pub fn window_state(&self, id: TeksiloWindowId) -> Option<WindowState>;
    pub fn windows(&self) -> Vec<WindowState>;
    pub fn request_activation_token(&mut self, id: TeksiloWindowId,
                                    cb: Box<dyn FnOnce(Option<String>)>);
    pub fn request_activation_token_self(&mut self, cb: Box<dyn FnOnce(Option<String>)>);
}
```

The two `request_activation_token*` calls mint an `xdg_activation_v1` token
to hand to another window or process so it can raise itself on Wayland; the
callback receives `None` wherever the platform cannot provide one.

### `open_window` is synchronous

When you call `ctx.open_window(config)`, the winit-level window is
created **before the call returns**. The returned `TeksiloWindowId` is
immediately usable, you can pass it to `focus_window`, read its
`window_state(id)`, or reference it as a modal parent in a subsequent
`open_window` call in the same handler.

```rust
ctx.register_action(Action::new("app.help").on_invoke(|_i, ctx| {
    if let Some(id) = ctx.find_window("help") {
        ctx.focus_window(id);          // second press → raise existing
        return;
    }
    // First press → create the window; id is valid from this line on.
    let id = ctx.open_window(
        WindowConfig::new()
            .title("Help")
            .id("help")                // stable key for find_window
            .size(720, 480)
            .root(|tree, _state| tree.add(HelpRoot)),
    );
    // Could immediately e.g. write an initial state signal on the new
    // window by looking it up via ctx.window_state(id).
    let _ = id;
}));
```

Under the hood: `WindowOpsImpl::open_window` calls
`WindowManager::create_window(config, event_loop)`, which builds the
winit window, wires `WindowState` observers, runs the root builder,
registers `ManagedWindow` in the windows map, and returns the id.
Nothing is deferred.

### Ergonomic patterns

**Idempotent open** (single-instance preferences, inspector):

```rust
.on_invoke(|_i, ctx| {
    match ctx.find_window("preferences") {
        Some(id) => ctx.focus_window(id),
        None => { ctx.open_window(/* ... */); }
    }
})
```

**Document window** (one window per file):

```rust
.on_invoke(|intent, ctx| {
    let AppIntent::OpenDocument(path) = AppIntent::from_intent(intent).unwrap() else { return; };
    let wid = format!("doc:{}", path.display());
    if let Some(id) = ctx.find_window(&wid) {
        ctx.focus_window(id);
        return;
    }
    let path = path.clone();
    ctx.open_window(
        WindowConfig::new()
            .title(format!("{}, My App", path.file_name().unwrap().to_string_lossy()))
            .id(wid)
            .size(1200, 800)
            .root(move |tree, _state| tree.add(DocumentRoot::open(path))),
    );
})
```

**Opening a window from a background thread** (`on_external_with_ctx`):

Both recipes above run inside a handler, where an `EventContext` already
exists. A background thread has none, and neither does
`TeksiloAppBuilder::on_app_event`, which receives `&AppEvent` and nothing
else, so `ctx.open_window` is simply not reachable from there (calling it on
a standalone context panics: "open_window called outside of a dispatch").

`on_external_with_ctx` is the hook for that case. It is offered every
`AppEvent::External` payload that no framework router claimed, together with
a live `EventContext` minted from the window opened or focused last (or,
with no window open, from the application tree; see below), and returns
`true` when the payload was the app's:

```rust
// A single-instance app. The second launch forwards its argv over a socket
// and exits; this process's listener thread posts it with
// `AppEventProxy::send_external`, and the "document window" recipe above
// runs against the resulting context.
TeksiloAppBuilder::new()
    .on_ready(spawn_ipc_listener)          // background thread → send_external
    .on_external_with_ctx(move |payload, ctx| {
        let Some(req) = payload.downcast_ref::<OpenDocument>() else {
            return false;                  // not ours, leave it unclaimed
        };
        let wid = format!("doc:{}", req.path.display());
        match ctx.find_window(&wid) {
            Some(id) => ctx.focus_window(id),
            None => { ctx.open_window(document_window_config(&req.path)); }
        }
        true
    })
```

Notes:

- Framework payload types (file-dialog results, external drag-and-drop events,
  async completions, native-menu choices, WebView and automation-bridge
  payloads, `CloseWindowRequest`, title-bar synthetics, the deprecated
  `RepaintWindowRequest`)
  are handled *before* this hook and never reach it, so it never has to
  defend against them.
- It is a **single slot**, like `on_app_event`, a second call replaces the
  first. For fan-out use `register_app_event_observer`, which composes (but
  gets no context).
- With no window open, the handler runs in the application tree
  (`TeksiloAppBuilder::app_root`), which no window hosts: `ctx.window()` is
  `None`, and `ctx.open_window` works as in the recipe above. That is what an
  application under `QuitPolicy::Explicit` with every window closed, or a
  tray application, gets when its second launch forwards a document.
- On Wayland, the second launch's launcher hands it an
  `XDG_ACTIVATION_TOKEN` (`DESKTOP_STARTUP_ID` on X11); without it the
  compositor treats the raise as unsolicited and only asks for attention.
  Forward it with the request and post both as one payload,
  `AppEventProxy::send_external(WithActivationToken::new(token, request))`.
  The handler sees the request (so do `on_app_event` and the observers of
  `register_app_event_observer`), and the token goes to the first window it
  raises or opens: `ctx.focus_window(id)` (the window the handler runs in
  included), a `WindowState::focus()` with no token of its own, or
  `ctx.open_window(...)`. When the handler sends an intent that its tree
  leaves to the other side (a window's to the application tree, or the
  other way), the intent takes the unused token with it, and the window the
  action there opens or raises is activated with it. An unused token is
  dropped at the end of the turn.

**Cross-window read** (dim the inspector when the main window is
fullscreen):

```rust
// In a handler on the inspector window:
let main_id = ctx.find_window("main").unwrap();
if let Some(main_state) = ctx.window_state(main_id) {
    let dim = main_state.placement().map(|p| p.is_fullscreen());
    // Use `dim` as a derived signal inside the inspector's UI.
}
```

### Modal dialogs

`EventContext::open_modal` is a thin wrapper that builds a
`WindowConfig` with `ModalConfig { parent: ctx.window().id(), focus_target }`
(plus the request's `title` / `size`) and calls `open_window`. It always
opens a native window, so the request's `presentation` is not consulted,
and it returns `None` outside a dispatch or for a
`ModalContent::ExistingWidget` request (only `Deferred` content can be
built into a fresh tree). Use it when you already have a
`ModalRequest` in hand:

```rust
ctx.open_modal(ModalRequest {
    content: ModalContent::Deferred(Box::new(|tree| tree.add(ConfirmQuit::new()))),
    presentation: ModalPresentation::NativeWindow,
    close_behavior: ModalCloseBehavior::EscapeOrClickOutside,
    title: Some("Confirm quit".into()),
    size: Some((420, 180)),
    focus_target: Some(ok_button_id),
    on_dismiss: None,
});
```

For the general case (may land in-tree or in a native window),
`ctx.present_modal(request)` picks the presentation at dispatch time
based on `ModalPresentation::Auto` and platform capability.

Commands sent in a dialog route the same way whichever presentation it got.
A native modal window has a tree of its own: an intent sent there, or a key
pressed there, reaches that tree's actions and shortcuts, then the
application tree, and never the window it blocks. A dialog presented in the
window (Linux) behaves the same while it is open: from inside it, the walk
stops at its content, and of the window's global actions and global
shortcuts only those registered by widgets inside the dialog apply. An
intent nothing in the window takes still goes to the application tree; a
key goes there only through a shortcut that applies in the dialog, and the
page's do not, so the page's Ctrl+N bound to an application-tree action does
not fire inside an in-window dialog. An intent sent from the page while the
dialog is up (only a programmatic send can be) still reaches every global
action of the window.

The two differ once the dialog closes. An intent sent as an in-window dialog
closes (its button closes it, then sends) is dispatched after it has closed,
and is the page's: it walks on from the widget the dialog was attached to,
or, sent from focus, starts at the focus the dialog gave back. One sent as a
native dialog window closes is that window's, and goes to the application
tree when nothing there takes it. See "Scoped vs global actions" in the
[shortcut guide](shortcut-intent-action.md).

A modal window has a close button and no minimise or maximise button, on
Windows and macOS, unless its config asks for them with `minimizable(true)`
or `maximizable(true)`. A modal window and the window it blocks are
minimised together: minimising the dialog takes the window with it, and
restoring either one brings both back. Where the window manager offers to
minimise a dialog anyway (X11), a click on the window it blocks brings it
back.

---

## Intercepting close / quit: confirmation guards

A window can refuse to close. Each `WindowConfig` carries an optional
**close guard** that the framework runs, with a real `EventContext`
for that window's own tree, *before* any **interactive** close gesture
tears the window down:

- the OS close button, `Alt+F4`, `Cmd+W` (winit `CloseRequested`);
- a custom-chrome (Teksilo-drawn) title-bar close button;
- a handler calling `ctx.close_window()`.

The guard returns `CloseResponse::Close` to let the close proceed, or
`CloseResponse::Veto` to cancel it. Under the default quit policy,
quitting the app is the last window closing, so a guard that vetoes the
final window's close also keeps the app alive.

Guards are **strictly per-window**, closing one window never consults
another's guard, so this is correct for multi-window apps: an editor
window with unsaved changes can veto its own close while a tool palette
beside it closes freely.

### Veto-then-reissue (the async-confirmation pattern)

A confirmation dialog is asynchronous, it waits for a click, so the
guard cannot answer "close?" synchronously. The idiomatic shape is to
**veto now, confirm, then re-issue a forced close**:

```rust
use teksilo::prelude::*;                 // CloseResponse
use teksilo::widgets::{MessageBox, MessageBoxButtons, StandardButton,
                       EventContextMessageBoxExt};

WindowConfig::new()
    .title("Editor")
    .on_close_requested(move |ctx| {
        if !dirty.get() {
            return CloseResponse::Close;     // nothing unsaved → just close
        }
        // The window being closed. The answer below may run in another one.
        let Some(window) = ctx.window().map(|w| w.id()) else {
            return CloseResponse::Close;
        };
        ctx.present_message_box(
            MessageBox::question(lit!("Close window?"))
                .text(lit!("The document has unsaved changes."))
                .buttons(MessageBoxButtons::SaveDiscardCancel)
                .on_result(move |r, ctx| match r.button {
                    StandardButton::Save    => { save(); ctx.close_window_by_id(window); }
                    StandardButton::Discard => ctx.close_window_by_id(window),
                    _                       => {}   // Cancel → stay open
                }),
        );
        CloseResponse::Veto                  // hold the window open for now
    });
```

`ctx.close_window_by_id(window)` is the escape hatch: it closes the window
**unconditionally**, bypassing the guard, so the second close (from the
dialog's button) actually goes through instead of re-prompting.

Name the window by its id rather than calling `ctx.close_window_forced()`
from the answer: that closes the window the answer *runs in*. A
`MessageBox` is presented in the guarded window's own tree on Linux, but in
a native modal window of its own on macOS and Windows, so there
`close_window_forced()` would close the dialog and leave the document
window open. `close_window_forced()` is right where the code runs in the
window to close, such as the guard itself.

### Reactive sugar: `can_close` + `on_close_blocked`

When the gate is a single reactive flag, skip the closure:

```rust
let may_close = dirty.not();              // Signal<bool>

WindowConfig::new()
    .can_close(may_close)                 // false → veto
    .on_close_blocked(move |ctx| {        // fired only when blocked
        ctx.present_message_box(/* confirmation … */);
    });
```

`can_close` is evaluated *before* `on_close_requested`: a `false` signal
short-circuits to a veto and fires `on_close_blocked`; a `true` signal
(or no signal) falls through to the guard, then to closing.

### Which closes are guarded

| Close origin | Guarded? |
| --- | --- |
| OS close button / `Alt+F4` / `Cmd+W` | Yes yes |
| Custom-chrome title-bar close button | Yes yes |
| `ctx.close_window()` | Yes yes |
| `ctx.close_window_forced()` | No bypasses |
| `ctx.close_window_by_id(id)` | No bypasses (explicit programmatic close) |
| `WindowState::close()` | No bypasses |
| Modal-dismissal / framework teardown | No bypasses |
| `ctx.request_quit()` | Yes, each window in turn (below) |
| `ctx.quit()` | No bypasses |

A window with no guard configured always closes immediately, the guard
machinery only runs when `on_close_requested` or `can_close` is set.

### Quitting the application

`ctx.request_quit()` is what a Quit menu item calls. It asks every open
window in turn, as if the user closed it, and the application ends once no
window is left, whatever its `QuitPolicy`. Modal windows are asked before
the windows they block, then the others from the most recently used.

A guard that vetoes stops the quit at its window, and the veto-then-reissue
pattern above carries on from there:

- while the guard's dialog is up (a modal overlay, or a native modal
  window), the quit waits;
- when the window then closes, closed by id as above, the quit goes on
  with the next window;
- when the window is left open with no dialog (the guard vetoed without
  asking, or the dialog was cancelled), the quit is abandoned. So is it
  when another window is closed meanwhile, or by `ctx.cancel_quit()`.

A guard reads `ctx.quit_in_progress()` to word its question for the whole
application, and a dialog's "Quit anyway" answer calls `ctx.quit()`, which
closes every window without asking. A dialog that closes before its window
does, to save in the background and close the window afterwards, abandons
the quit; call `ctx.request_quit()` again once the save is done. Asking for
a guarded quit while one is under way changes nothing.

```rust
// The guard of the close-confirmation demo, quit-aware:
.on_close_requested(move |ctx| {
    if !dirty.get() {
        return CloseResponse::Close;
    }
    let Some(window) = ctx.window().map(|w| w.id()) else {
        return CloseResponse::Close;
    };
    let question = if ctx.quit_in_progress() {
        lit!("Quit with unsaved changes?")
    } else {
        lit!("Close window?")
    };
    ctx.present_message_box(
        MessageBox::question(question)
            .buttons(MessageBoxButtons::SaveDiscardCancel)
            .on_result(move |result, ctx| match result.button {
                StandardButton::Save | StandardButton::Discard => ctx.close_window_by_id(window),
                _ => ctx.cancel_quit(),
            }),
    );
    CloseResponse::Veto
})
```

Working demo: `cargo run -p close-confirmation` (main window: full
`on_close_requested` + Save/Discard/Cancel, and a Quit button; second
window: the `can_close` sugar).

---

## `TeksiloAppBuilder::run()` lifecycle

1. `run()` builds a `TeksiloAppHandler` and spins up the winit event loop.
2. On `resumed()`, the handler calls
   `WindowManager::create_window(initial_window_config, event_loop)`,
   synchronous winit creation, widget tree built, first paint requested.
   An app under `QuitPolicy::Explicit` may have no initial window, and
   skips this.
3. On every `winit::WindowEvent`:
   - Event translation → `WidgetEvent`.
   - `dispatch_in_window(winit_id, evt, event_loop)`, temporarily
     removes the window from the map, constructs `WindowOpsImpl` with
     `&mut WindowManager` + `&ActiveEventLoop`, calls
     `tree.dispatch_event_with_ops(evt, ops)`, reinserts the window.
   - Handlers can call `ctx.open_window(...)` which synchronously
     reaches `wm.create_window(...)`, modal parents attach to either
     the dispatching window (via its `Window`, kept on the ops
     object) or to another window that's still in the map.
4. After dispatch, `post_event`:
   - Drains tree-level pending operations (locale, theme, follow-system
     theme, text scale, close-window).
   - Processes modal requests and dismissals.
   - Drains `pending_closes` (`WindowManager::process_pending`, from
     any source: `ctx.close_window()`, `ctx.close_window_by_id(id)`,
     close requests via `TitleBarHostCallbacks::request_close`). Each
     entry is either *guarded* (interactive gestures, runs the window's
     close guard, may be vetoed) or *forced* (explicit programmatic
     closes + framework teardown, unconditional). See **Intercepting
     close / quit** above.
   - Runs queued post-mount actions.
   - Drains every window's `pending_os_commands`
     (`WindowManager::drain_window_commands`) and applies them via winit
     calls. A `WindowCommand::Close` from `state.close()` is re-queued
     there as a forced close, torn down on the next tick's
     `process_pending`.
5. `handle_redraw_requested` runs `layout_with_ops` + `render_with_ops`
, both thread ops through, so state-change-triggered handlers
   (data-driven rebuilds, delayed overlays, drag-tick) can open
   windows too.
6. After each `process_pending`, a quit under way takes its next steps
   (`WindowManager::advance_quit`). At the end of `post_event` and of
   `about_to_wait`, the loop ends once a quit has completed, or when the
   quit policy says so: under `QuitPolicy::LastWindowClosed`, once no window
   is open.

---

## `WindowOps` and the dispatch re-entry pattern

`WindowOps` is a trait in `teksilo-core`; `teksilo-app` provides
`WindowOpsImpl`. This is what lets `EventContext::open_window` route
into `WindowManager::create_window` synchronously without teksilo-core
depending on teksilo-app.

```rust
// teksilo-core
pub trait WindowOps {
    fn open_window(&mut self, config: WindowConfig) -> TeksiloWindowId;
    fn find_window(&self, string_id: &str) -> Option<TeksiloWindowId>;
    fn window_state(&self, id: TeksiloWindowId) -> Option<WindowState>;
    fn windows(&self) -> Vec<WindowState>;
    fn focus_window(&mut self, id: TeksiloWindowId);
    fn close_window_by_id(&mut self, id: TeksiloWindowId);
    // …plus defaulted hooks: activation tokens, the parent window
    // handle (file dialogs), the IME cursor area, outbound OS drags,
    // soft-keyboard support.
}
```

### Temporary-removal re-entry

Inside `TeksiloAppHandler::dispatch_in_window`:

```rust
let Some(mut current) = self.wm.take_managed(winit_id) else { return };
// SAFETY: the current window is held in `current`; the map no longer
// contains it. WindowOpsImpl holds `&mut self.wm` (minus the current
// window) + `&ActiveEventLoop` + the current window's `Window` so
// modal parents pointing at it still resolve.
{
    let mut ops = WindowOpsImpl::new(&mut self.wm, event_loop,
                                      current.teksilo_id,
                                      current_arc);
    current.tree.dispatch_event_with_ops(evt, &mut ops);
}
self.wm.reinsert_managed(winit_id, current);
```

The dispatching window is removed from the windows map for the
duration of the handler run. That releases the mutable borrow on
`self.wm.windows[winit_id]` so `WindowOpsImpl::open_window` can call
`self.wm.create_window(...)` without borrow conflicts. The stashed
`Window` lets modal-parent lookups reach back to the dispatching
window.

If you're a handler, none of this is visible, you just call
`ctx.open_window(...)` and it returns an id.

---

## Integration points

### `TitleBar` widget

The title bar's maximize / restore / close buttons and double-click
handler now write directly to `WindowState::placement` (through
`ctx.window()`). The button glyph swap is driven by a derived signal:

```rust
let show_restore = ctx
    .window()
    .map(|w| w.placement().map(|p| p.is_maximized() || p.is_fullscreen()))
    .unwrap_or_else(|| Signal::new(false));
```

(A fullscreen window is restorable too, so it shows "restore", never
"maximize".)

The `PlatformTitleBarHost` trait shrank, it no longer owns `minimize`,
`toggle_maximize`, `close`, `is_maximized`, `is_maximized_signal`, or
`notify_window_resized`. It keeps only what's genuinely chrome-specific
(insets, whether it renders its own controls or needs custom resize
handles, drag/resize interaction, hit regions, `show_window_menu` /
`has_window_menu`).
Custom chrome now works with `DecorationsMode::Native` windows too,
the TitleBar widget binds to `WindowState::placement` either way.

### Tests / headless

Standalone `WidgetTree`s without an attached app use `NoopWindowOps`:

- `tree.dispatch_event(evt)`, wraps with `NoopWindowOps`
- `tree.layout(proposal)`, wraps with `NoopWindowOps`
- `tree.render()`, wraps with `NoopWindowOps`
- `tree.tick_gestures(now)`, wraps with `NoopWindowOps`
- `tree.focus(id)` / `tree.focus_with_origin(id, origin)`, wraps
- `tree.dismiss_overlay(id)`, wraps

A handler that calls `ctx.open_window(...)` from any of these paths
panics (by design, the test has no event loop to create a window in).
`ctx.find_window`, `ctx.window_state`, `ctx.windows` return `None` /
empty.

`teksilo-app` uses the `_with_ops` variants internally so real apps get
fully-threaded ops on every code path.

---

## Checklist for common tasks

### Add a fullscreen toggle to my app

1. Register a shortcut for `F11`.
2. Register an `Action` that reads `ctx.window().placement()` and
   writes the opposite `Floating` / `Fullscreen`.
3. Widgets that want to reflect the state derive from
   `ctx.window().placement().map(|p| p.is_fullscreen())`.

### Open a "Preferences" window that's single-instance

```rust
ctx.register_action(Action::new("app.preferences").on_invoke(|_i, ctx| {
    match ctx.find_window("preferences") {
        Some(id) => ctx.focus_window(id),
        None => {
            ctx.open_window(
                WindowConfig::new()
                    .title("Preferences")
                    .id("preferences")
                    .size(640, 480)
                    .root(|tree, _state| tree.add(Preferences::new())),
            );
        }
    }
}));
```

### Show a confirm-quit native modal

```rust
ctx.open_modal(ModalRequest::deferred(|tree| tree.add(ConfirmQuit::new()))
    .presentation(ModalPresentation::NativeWindow)
    .title("Confirm quit")
    .size(420, 180));
```

### Custom chrome on the initial window

```rust
WindowConfig::new()
    .title("My App")
    .size(1200, 800)
    .decorations(DecorationsMode::CustomChrome)
    .root(|tree, _state| tree.add(AppRoot::new()))
```

The root widget typically places a `TitleBar` at the top of its
layout; its maximize / close buttons bind to `WindowState` automatically.

### Read the main window's size from a secondary window

```rust
// In a handler on any window:
if let Some(main_id) = ctx.find_window("main") {
    if let Some(main_state) = ctx.window_state(main_id) {
        let (w, h) = main_state.size().get();
        // Use w, h ...
    }
}
```

Or keep a live subscription by cloning the `Signal<(u32, u32)>` and
installing an observer through the current window's build context.

---

## Reference

- End-to-end demo:
  [`examples/multi_window`](../examples/multi_window/src/main.rs).
- Implementation:
  - Types, [`crates/teksilo-core/src/window/`](../crates/teksilo-core/src/window/)
  - Dispatch, [`crates/teksilo-core/src/widget_tree/pointer_router.rs`](../crates/teksilo-core/src/widget_tree/pointer_router.rs)
  - Window manager, [`crates/teksilo-app/src/window_manager.rs`](../crates/teksilo-app/src/window_manager.rs)
  - `EventContext` methods, [`crates/teksilo-core/src/widget/event_context.rs`](../crates/teksilo-core/src/widget/event_context.rs)
- Related docs:
  - [`title-bar.md`](title-bar.md), custom chrome integration
  - [`shortcut-intent-action.md`](shortcut-intent-action.md), the
    input pipeline that typically drives `open_window` calls
  - [`reactive-theme.md`](reactive-theme.md), the signal system
    `WindowState` is built on
