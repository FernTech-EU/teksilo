<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Multi-Window Reference

Bastyde's multi-window system is **signal-driven** and **synchronous**.
A single `WindowConfig` describes any window you want to open (initial
or runtime); per-window state lives in a reactive
[`WindowState`](../crates/bastyde-core/src/window/state.rs) that widgets
bind against; handlers open, focus, and close windows through
[`EventContext`](../crates/bastyde-core/src/widget.rs) methods that
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

Every Bastyde app opens exactly one initial window via
`BastydeAppBuilder::initial_window(WindowConfig)`. Secondary windows are
opened from handler code via `EventContext::open_window`.

```rust
use bastyde::prelude::*;
use bastyde::app::BastydeAppBuilder;

fn main() {
    BastydeAppBuilder::new()
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

- `BastydeAppBuilder` has no `.window_title`, `.window_size`, `.root`, or
  `.custom_chrome` — every window is described by `WindowConfig`.
  One conceptual surface, no special-casing for the initial window.
- `root_builder` receives `(tree, WindowState)` — the state clone is
  how a widget can bind against its own window's signals at
  construction time, without going through a `BuildContext`.

---

## `WindowConfig`

The single entry point for creating any window. Uniform whether you
pass it to `BastydeAppBuilder::initial_window` at startup or to
`EventContext::open_window` from a handler.

```rust
pub struct WindowConfig {
    pub title: String,                   // also feeds WindowState::title
    pub string_id: Option<String>,       // stable lookup key for find_window
    pub size: (u32, u32),                // restored size; always set
    pub position: Option<(i32, i32)>,    // restored position; None = WM picks
    pub min_size: Option<(u32, u32)>,
    pub max_size: Option<(u32, u32)>,
    pub initial_placement: WindowPlacement,
    pub decorations: DecorationsMode,
    pub resizable: bool,
    pub always_on_top: bool,
    pub skip_taskbar: bool,
    pub icon: Option<WindowIcon>,
    pub modal: Option<ModalConfig>,
    pub root_builder: Option<RootBuilder>,
}
```

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
    .skip_taskbar(true)                        // not in the taskbar/dock
    .icon(WindowIcon::from_rgba(rgba, w, h))
    .root(|tree, state| tree.add(Inspector::new(state)))
```

### `WindowPlacement`

Unified enum for the four top-level placement states every desktop OS
supports:

| Variant | Meaning |
|---|---|
| `Floating` | Regular overlapping window — uses `WindowState::size` / `position` as current geometry |
| `Maximized` | Fills the current monitor's work area |
| `Fullscreen` | Exclusive fullscreen (Space-based on macOS) |
| `Minimized` | Hidden to the taskbar / dock |

Size and position are **not** inside `Floating`. They live on
`WindowState` as their own signals and always hold the last-known
*restored* values — matching macOS `frameAutosaveName` and Windows
`WINDOWPLACEMENT` behavior, so "un-maximize" and "un-fullscreen"
restore the window to the right rect without ambiguity.

Transitions between any two variants are legal; the platform layer
preserves the restored rect as you cross through `Maximized` /
`Fullscreen` / `Minimized`.

### `DecorationsMode`

| Variant | Meaning |
|---|---|
| `Native` | OS-provided title bar, borders, resize handles. Default |
| `CustomChrome` | No native title bar; a `PlatformTitleBarHost` is attached so the app can paint its own. Falls back to `Native` on X11 |
| `None` | Borderless, no host — splash screens, popups, fully chrome-less embeds |

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
input-blocking on the parent, Z-order child-window attachment
(`WM_TRANSIENT_FOR` / `xdg_toplevel.set_parent` / AppKit
`addChildWindow:ordered:`), refocus-on-stolen-focus.

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
pub struct WindowState(Rc<WindowStateInner>);

impl WindowState {
    pub fn id(&self) -> BastydeWindowId;
    pub fn string_id(&self) -> Option<&str>;

    // Writable signals. App-side writes queue a `WindowCommand` to the
    // OS; OS-initiated writes flow back through `*_from_os` setters
    // with a re-entrancy guard.
    pub fn placement(&self)     -> &Signal<WindowPlacement>;
    pub fn title(&self)         -> &Signal<String>;
    pub fn size(&self)          -> &Signal<(u32, u32)>;
    pub fn position(&self)      -> &Signal<(i32, i32)>;
    pub fn focused(&self)       -> &Signal<bool>;
    pub fn resizable(&self)     -> &Signal<bool>;
    pub fn always_on_top(&self) -> &Signal<bool>;

    // Imperative one-shots. Each pushes a single `WindowCommand` on
    // the next drain.
    pub fn request_attention(&self, kind: UserAttentionKind);
    pub fn focus(&self);
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

        let label = fs.map(|f| if f { "Exit fullscreen" } else { "Fullscreen" });

        vec![ctx.add(
            Button::new()
                .label(label)
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

The button label re-renders automatically when fullscreen is toggled —
whether the toggle came from the button itself, the F11 shortcut, or
the user pressing the green traffic light on macOS. All three paths
write into the same `placement()` signal.

### Two-way OS sync — how it works

`WindowState::new` wires an observer to every writable signal. The
observer:

1. Checks the `applying_from_os` flag on `WindowStateInner`.
2. If set (OS-initiated write): does nothing — the OS already knows.
3. If unset (app-initiated write): pushes a `WindowCommand` onto the
   shared `pending_os_commands` queue.

Each event-loop tick:

1. `bastyde-app`'s `handle_window_event_inner` translates winit
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

See [`state.rs`](../crates/bastyde-core/src/window/state.rs) for the
implementation; see [`state.rs` tests](../crates/bastyde-core/src/window/state.rs)
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
    pub fn open_window(&mut self, config: WindowConfig) -> BastydeWindowId;
    pub fn open_modal(&mut self, request: ModalRequest) -> Option<BastydeWindowId>;
    pub fn find_window(&self, string_id: &str) -> Option<BastydeWindowId>;
    pub fn focus_window(&mut self, id: BastydeWindowId);
    pub fn close_window(&mut self);                         // current window, GUARDED
    pub fn close_window_forced(&mut self);                  // current window, bypasses the guard
    pub fn close_window_by_id(&mut self, id: BastydeWindowId);
    pub fn window_state(&self, id: BastydeWindowId) -> Option<WindowState>;
    pub fn windows(&self) -> Vec<WindowState>;
}
```

### `open_window` is synchronous

When you call `ctx.open_window(config)`, the winit-level window is
created **before the call returns**. The returned `BastydeWindowId` is
immediately usable — you can pass it to `focus_window`, read its
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
`WindowManager::create_window(config, event_loop)` — which builds the
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
            .title(format!("{} — My App", path.file_name().unwrap().to_string_lossy()))
            .id(wid)
            .size(1200, 800)
            .root(move |tree, _state| tree.add(DocumentRoot::open(path))),
    );
})
```

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
and calls `open_window`. Use it when you already have a
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

---

## Intercepting close / quit — confirmation guards

A window can refuse to close. Each `WindowConfig` carries an optional
**close guard** that the framework runs — with a real `EventContext`
for that window's own tree — *before* any **interactive** close gesture
tears the window down:

- the OS close button, `Alt+F4`, `Cmd+W` (winit `CloseRequested`);
- a custom-chrome (Bastyde-drawn) title-bar close button;
- a handler calling `ctx.close_window()`.

The guard returns `CloseResponse::Close` to let the close proceed, or
`CloseResponse::Veto` to cancel it. Quitting the app is just the last
window closing, so a guard that vetoes the final window's close also
keeps the app alive.

Guards are **strictly per-window** — closing one window never consults
another's guard — so this is correct for multi-window apps: an editor
window with unsaved changes can veto its own close while a tool palette
beside it closes freely.

### Veto-then-reissue (the async-confirmation pattern)

A confirmation dialog is asynchronous — it waits for a click — so the
guard cannot answer "close?" synchronously. The idiomatic shape is to
**veto now, confirm, then re-issue a forced close**:

```rust
use bastyde::prelude::*;                 // CloseResponse
use bastyde::widgets::{MessageBox, MessageBoxButtons, StandardButton,
                       EventContextMessageBoxExt};

WindowConfig::new()
    .title("Editor")
    .on_close_requested(move |ctx| {
        if !dirty.get() {
            return CloseResponse::Close;     // nothing unsaved → just close
        }
        ctx.present_message_box(
            MessageBox::question(lit!("Close window?"))
                .text(lit!("The document has unsaved changes."))
                .buttons(MessageBoxButtons::SaveDiscardCancel)
                .on_result(move |r, ctx| match r.button {
                    StandardButton::Save    => { save(); ctx.close_window_forced(); }
                    StandardButton::Discard => ctx.close_window_forced(),
                    _                       => {}   // Cancel → stay open
                }),
        );
        CloseResponse::Veto                  // hold the window open for now
    });
```

`ctx.close_window_forced()` is the escape hatch: it closes the window
**unconditionally**, bypassing the guard, so the second close (from the
dialog's button) actually goes through instead of re-prompting.

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
| OS close button / `Alt+F4` / `Cmd+W` | ✅ yes |
| Custom-chrome title-bar close button | ✅ yes |
| `ctx.close_window()` | ✅ yes |
| `ctx.close_window_forced()` | ❌ bypasses |
| `ctx.close_window_by_id(id)` | ❌ bypasses (explicit programmatic close) |
| `WindowState::close()` | ❌ bypasses |
| Modal-dismissal / framework teardown | ❌ bypasses |

A window with no guard configured always closes immediately — the guard
machinery only runs when `on_close_requested` or `can_close` is set.

Working demo: `cargo run -p close-confirmation` (main window: full
`on_close_requested` + Save/Discard/Cancel; second window: the
`can_close` sugar).

---

## `BastydeAppBuilder::run()` lifecycle

1. `run()` builds a `BastydeAppHandler` and spins up the winit event loop.
2. On `resumed()`, the handler calls
   `WindowManager::create_window(initial_window_config, event_loop)` —
   synchronous winit creation, widget tree built, first paint requested.
3. On every `winit::WindowEvent`:
   - Event translation → `WidgetEvent`.
   - `dispatch_in_window(winit_id, evt, event_loop)` — temporarily
     removes the window from the map, constructs `WindowOpsImpl` with
     `&mut WindowManager` + `&ActiveEventLoop`, calls
     `tree.dispatch_event_with_ops(evt, ops)`, reinserts the window.
   - Handlers can call `ctx.open_window(...)` which synchronously
     reaches `wm.create_window(...)` — modal parents attach to either
     the dispatching window (via the stashed raw handle on the ops
     object) or to another window that's still in the map.
4. After dispatch, `post_event`:
   - Drains tree-level pending operations (locale, close-window).
   - Processes in-tree modal requests.
   - Drains every window's `pending_os_commands` and applies them via
     winit calls.
   - Drains `pending_closes` (from any source — `ctx.close_window()`,
     `ctx.close_window_by_id(id)`, `state.close()`, close requests
     via `TitleBarHostCallbacks::request_close`). Each entry is either
     *guarded* (interactive gestures — runs the window's close guard,
     may be vetoed) or *forced* (explicit programmatic closes +
     framework teardown — unconditional). See **Intercepting close /
     quit** above.
5. `handle_redraw_requested` runs `layout_with_ops` + `render_with_ops`
   — both thread ops through, so state-change-triggered handlers
   (data-driven rebuilds, delayed overlays, drag-tick) can open
   windows too.

---

## `WindowOps` and the dispatch re-entry pattern

`WindowOps` is a trait in `bastyde-core`; `bastyde-app` provides
`WindowOpsImpl`. This is what lets `EventContext::open_window` route
into `WindowManager::create_window` synchronously without bastyde-core
depending on bastyde-app.

```rust
// bastyde-core
pub trait WindowOps {
    fn open_window(&mut self, config: WindowConfig) -> BastydeWindowId;
    fn find_window(&self, string_id: &str) -> Option<BastydeWindowId>;
    fn window_state(&self, id: BastydeWindowId) -> Option<WindowState>;
    fn windows(&self) -> Vec<WindowState>;
    fn focus_window(&mut self, id: BastydeWindowId);
    fn close_window_by_id(&mut self, id: BastydeWindowId);
}
```

### Temporary-removal re-entry

Inside `BastydeAppHandler::dispatch_in_window`:

```rust
let Some(mut current) = self.wm.take_managed(winit_id) else { return };
// SAFETY: the current window is held in `current`; the map no longer
// contains it. WindowOpsImpl holds `&mut self.wm` (minus the current
// window) + `&ActiveEventLoop` + the current window's raw handle so
// modal parents pointing at it still resolve.
{
    let mut ops = WindowOpsImpl::new(&mut self.wm, event_loop,
                                      current.bastyde_id,
                                      current_handle);
    current.tree.dispatch_event_with_ops(evt, &mut ops);
}
self.wm.reinsert_managed(winit_id, current);
```

The dispatching window is removed from the windows map for the
duration of the handler run. That releases the mutable borrow on
`self.wm.windows[winit_id]` so `WindowOpsImpl::open_window` can call
`self.wm.create_window(...)` without borrow conflicts. The stashed
raw window handle lets modal-parent lookups reach back to the
dispatching window.

If you're a handler, none of this is visible — you just call
`ctx.open_window(...)` and it returns an id.

---

## Integration points

### `TitleBar` widget

The title bar's maximize / restore / close buttons and double-click
handler now write directly to `WindowState::placement` (through
`ctx.window()`). The button glyph swap is driven by a derived signal:

```rust
let is_maximized = ctx
    .window()
    .map(|w| w.placement().map(|p| p.is_maximized()))
    .unwrap_or_else(|| Signal::new(false));
```

The `PlatformTitleBarHost` trait shrank — it no longer owns `minimize`,
`toggle_maximize`, `close`, `is_maximized`, `is_maximized_signal`, or
`notify_window_resized`. It keeps only what's genuinely chrome-specific
(insets, drag/resize interaction, hit regions, `show_window_menu`).
Custom chrome now works with `DecorationsMode::Native` windows too —
the TitleBar widget binds to `WindowState::placement` either way.

### Tests / headless

Standalone `WidgetTree`s without an attached app use `NoopWindowOps`:

- `tree.dispatch_event(evt)` — wraps with `NoopWindowOps`
- `tree.layout(proposal)` — wraps with `NoopWindowOps`
- `tree.render()` — wraps with `NoopWindowOps`
- `tree.tick_gestures(now)` — wraps with `NoopWindowOps`
- `tree.focus(id)` / `tree.focus_with_origin(id, origin)` — wraps
- `tree.dismiss_overlay(id)` — wraps

A handler that calls `ctx.open_window(...)` from any of these paths
panics (by design — the test has no event loop to create a window in).
`ctx.find_window`, `ctx.window_state`, `ctx.windows` return `None` /
empty.

`bastyde-app` uses the `_with_ops` variants internally so real apps get
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
  - Types — [`crates/bastyde-core/src/window/`](../crates/bastyde-core/src/window/)
  - Dispatch — [`crates/bastyde-core/src/widget_tree/event_dispatch_impl.rs`](../crates/bastyde-core/src/widget_tree/event_dispatch_impl.rs)
  - Window manager — [`crates/bastyde-app/src/window_manager.rs`](../crates/bastyde-app/src/window_manager.rs)
  - `EventContext` methods — [`crates/bastyde-core/src/widget.rs`](../crates/bastyde-core/src/widget.rs)
- Related docs:
  - [`title-bar.md`](title-bar.md) — custom chrome integration
  - [`shortcut-intent-action.md`](shortcut-intent-action.md) — the
    input pipeline that typically drives `open_window` calls
  - [`reactive-theme.md`](reactive-theme.md) — the signal system
    `WindowState` is built on
