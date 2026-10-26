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
`TeksiloAppBuilder::initial_window(WindowConfig)` (an app may also start
with none and open its first window from the application tree; see the quit
policy, below). Secondary windows are
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
| `QuitPolicy::LastWindowClosed` (default) | the app quits | optional, but a window must be open once startup has run (the initial window, or one the application tree's `run_after_mount` opens): `run()` panics on its first turn otherwise, since the app would quit before showing anything |
| `QuitPolicy::Explicit` | the app keeps running with no window | optional |

`Explicit` is for an application whose life is not tied to its windows: a
background service, or an application that lives in the system tray (a tray
icon does not keep the application alive by itself). Under either policy,
`ctx.quit()` (which the close guards may refuse) and `ctx.quit_forced()` end
the application; see "Quitting the application" below.

---

## The application tree

Code that must run with no window open (the application menu and its
actions, a single-instance handler, a quit) lives in the **application
tree**: a widget tree that belongs to no window. Give it a root with
`TeksiloAppBuilder::app_root`, which takes a builder like a window's root,
without the `WindowState` (the tree has no window):

```rust
TeksiloAppBuilder::new()
    .quit_policy(QuitPolicy::Explicit)            // live with no window
    .app_root(|tree| tree.add(AppActions::default()))
    .run();

#[derive(Debug, Default)]
struct AppActions {
    opened_first_window: Rc<Cell<bool>>,
}

impl Widget for AppActions {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_action_global(Action::new("app.new_window").on_invoke(|_, ctx| {
            ctx.open_window(document_window());
        }));
        ctx.register_action_global(Action::new("app.quit").on_invoke(|_, ctx| {
            ctx.quit();
        }));
        // Runs once this root is mounted, at startup, and again each time it
        // is rebuilt: an app with no initial window opens its first one
        // here, once.
        let opened = self.opened_first_window.clone();
        ctx.run_after_mount(move |ctx| {
            if !opened.replace(true) {
                ctx.open_window(document_window());
            }
        });
        vec![]
    }
    // layout_response: a zero size; the tree is never drawn.
}
```

Without `app_root` the tree still exists, with an empty root of its own.

The tree is never painted. Its handlers run with `ctx.window()` `None`:
everything that names a window by id works (opening, finding and focusing
one, `close_window_by_id`, `quit`), and nothing about "this window"
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
  application tree leaves goes to the window used last of those not
  minimised, else to the window used last (at its frontmost native dialog),
  so its handlers can send a window a command. It goes to that one window
  only, as AppKit offers what the application leaves to its key window and
  to no other: a command meant for every window is better state they all
  observe (a `Signal` in app state) than an intent. An intent
  crosses once and never comes back; one nobody takes is dropped, and a debug
  build names it (the framework's own `teksilo.` intents excepted). An intent
  the action that took it sends may cross in its turn, up to 64 crossings in
  one chain, after which the framework says, in every build, that two
  actions seem to answer each other. A
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
- its own mount actions (`run_after_mount`), at startup and after each
  rebuild of the widget that enqueued them, so one that must run once (the
  first window above) keeps a flag.

What needs a window is not drawn there:

| From the application tree | Happens |
|---|---|
| a modal request (`MessageBox`, `present_modal`) | presented in the window used last of those not minimised, else in the window used last. Asked in answer to the user's input (a click, a key, a menu row, a screen reader's action, a payload with an activation token), that window is restored and brought to the front; otherwise (a timer, another thread) it asks for attention (the Dock icon bounces and the taskbar button flashes until the user comes to the application; an urgency hint on X11) and stays where it is, as a background application's does. A dialog that would open a window of its own (macOS, Windows) over a minimised window waits instead: shown, in front, on the first turn a window of the application takes the focus, over the window chosen then; a quit answers one still waiting as dismissed (a `MessageBox` reports its escape button). In a window of its own when none is open. Content must be `ModalRequest::deferred` |
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
struct AppActions {
    menu: MenuModel,
}

impl AppActions {
    // Called once, by `app_root` (above): the menu is built here, not in `build`.
    fn new() -> Self {
        Self { menu: application_menu() }
    }
}

impl Widget for AppActions {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_shortcut_global(
            Shortcut::new("app.new_window").primary(KeyStroke::ctrl(Key::N)).build(),
        );
        ctx.register_action_global(Action::new("app.new_window").on_invoke(|_, ctx| {
            ctx.open_window(document_window());
        }));
        ctx.set_application_menu(self.menu.clone());
        vec![]
    }
}

fn application_menu() -> MenuModel {
    MenuModel::new().menu(tr!(file()), |m| {
        m.item(MenuEntry::new(tr!(new_window())).intent("app.new_window").shortcut("app.new_window"))
    })
}
```

- Build the model once and declare a clone of that handle, as above: the same
  handle declared again from a later build changes nothing, while a model
  built inside `build` is a new menu at every rebuild of the root, which
  rebuilds every bar showing it (a debug build says so when it keeps
  happening).

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
    pub minimizable: Option<bool>,       // None: yes; a modal window never has one
    pub maximizable: Option<bool>,       // None: no for a modal window, yes otherwise
    pub activate_from_env: bool,         // consume an xdg_activation startup token (Wayland)
    pub icon: Option<WindowIcon>,
    pub modal: Option<ModalConfig>,
    pub root_builder: Option<RootBuilder>,
    pub post_root_builder: Option<PostRootBuilder>,
    pub on_close_requested: Option<CloseGuard>,        // see "Intercepting close / quit"
    pub can_close: Option<Prop<bool>>,
    pub on_close_blocked: Option<CloseBlockedCallback>,  // Fn(&mut EventContext, CloseDecision)
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
Word, Firefox and most document apps. A position names the top-left corner of
the window's frame, its title bar included, on every platform, as
`WindowState::position` and the saved geometry do.

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
window on Windows, `WM_TRANSIENT_FOR`, `_NET_WM_WINDOW_TYPE_DIALOG` and
`_NET_WM_STATE_MODAL` on X11, AppKit `addChildWindow:ordered:` on macOS; Wayland has none, since
winit offers no `xdg_toplevel.set_parent`), refocus-on-stolen-focus. The
parent is the dialog's owner: the dialog has no minimise of its own and is
minimised, restored and closed with it. See
[Modal dialogs](#modal-dialogs).

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
    pub fn quit(&mut self);                                 // every window, GUARDED
    pub fn quit_forced(&mut self);                          // every window, bypasses the guards
    pub fn quit_in_progress(&self) -> bool;
    pub fn quit_source(&self) -> Option<QuitSource>;        // the quit under way
    pub fn cancel_quit(&mut self);                          // a guarded quit only
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
  payloads, `CloseWindowRequest`, title-bar synthetics, the operating
  system's quit requests, the deprecated `RepaintWindowRequest`)
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
and it returns `None` when there is no current window (outside a
dispatch, or in the application tree, which uses `present_modal` instead)
or for a `ModalContent::ExistingWidget` request (only `Deferred` content can be
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
pressed there, reaches that tree's actions and shortcuts first. A dialog
presented in the window (Linux) goes first in the same way while it is open:
an intent sent inside it reaches the dialog's own actions, then the window's
global actions registered by widgets inside the dialog. What they leave walks
on to the page, from the widget the dialog was opened from, then reaches the
page's other global actions, and what the window leaves goes to the
application tree. An intent a native dialog window leaves unclaimed goes on
to the window it blocks, dispatched from that window's focus, then up the
chain of owners, then to the application tree. Either way the dialog's own
handlers come first and the page's still answer the dialog's buttons.

Keys stop at the dialog. In a native dialog window, the window it blocks has
no part in a key's walk; in an in-window dialog, of the window's global
shortcuts only those registered by widgets inside the dialog apply, and a
shortcut's intent reaches nothing of the page. So the page's Ctrl+N bound to
an application-tree action does not fire inside an in-window dialog, while a
dialog button that sends `app.new` reaches it. A row chosen in the macOS
menu bar is input as a key is: over an in-window dialog its intent stops at
the dialog, and the row is greyed out when only the page would take it. An
intent sent from the page while the dialog is up (only a programmatic send
can be) is the page's and reaches every global action of the window.

An intent sent as a dialog closes (its button closes it, then sends) goes
the same way: from an in-window dialog, it is dispatched after it has closed
and walks on from the widget the dialog was attached to, or, sent from focus,
starts at the focus the dialog gave back; from a native dialog window, it
goes on to the window it blocked. See "Scoped vs global actions" in the
[shortcut guide](shortcut-intent-action.md).

#### A dialog belongs to the window it blocks

A modal window is owned by the window it blocks, its parent, as a dialog is
on every desktop: it stays in front of it, has no taskbar entry of its own,
and has no minimise of its own. It goes and comes back with its owner.

- **Opening it** brings it to the front when it is opened in answer to the
  user's input (a click, a key, a menu row, a screen reader's action, an
  activation token), over a window that has the focus, or for a quit
  waiting on its answer. Opened otherwise (a timer's dialog, another
  thread's, while the user is in another application), it opens over its
  owner without taking the focus. On macOS a dialog is not attached to an
  owner that is in the Dock, since AppKit would draw the owner too while
  leaving it minimised; a debug build says so, and the application tree's
  dialogs wait for the user in that case (see the table above).

- **Its title bar** has a close button and no minimise button, whatever its
  config says (`minimizable(true)` on a modal config is ignored, and a debug
  build says so). It has no maximise button unless asked with
  `maximizable(true)`. The OS title bar follows this on Windows and macOS,
  and so does a Teksilo [`TitleBar`](title-bar.md) on every platform. On X11
  the window manager draws it: a modal window is a transient, modal dialog
  (`WM_TRANSIENT_FOR`, `_NET_WM_WINDOW_TYPE_DIALOG` and
  `_NET_WM_STATE_MODAL`, as GTK marks its own), which window managers
  usually draw with no minimise button. Wayland's compositors draw
  theirs as they see fit.
- **Minimising the owner** takes its dialogs with it, and restoring it brings
  them back, in front. Windows does this itself for an owned window and
  macOS for a child window. On X11 the window manager may (openbox and KWin
  minimise a transient window with its parent); where it does not, Teksilo
  minimises and restores the dialogs itself. winit reports no X11 minimise,
  so Teksilo's own X connection listens to the `_NET_WM_STATE` of each
  dialog and of the window it blocks while the dialog is open, and the
  dialogs follow as soon as the window manager changes it, with nothing
  polled. A dialog is asked of the window manager to minimise and counts as
  minimised (and stops drawing) only once its own state says it is: a
  window manager may refuse (i3 marks the windows of a hidden tab
  `_NET_WM_STATE_HIDDEN` and minimises nothing), and the dialog then stays
  drawn. A dialog of a dialog follows the same way, one step after the
  dialog it follows.
  On macOS the owner, restored from the Dock, takes the focus before AppKit
  shows its dialog again at the end of the restore animation; the dialog is
  raised then, so it is key and the owner refuses the first click as it
  refuses every other. That raise is dropped if the user has gone to
  another application meanwhile.
- **Closing the owner.** While its dialog is up, the owner's close button
  (and Alt+F4, Cmd+W, `ctx.close_window()` on it) brings the dialog forward
  and closes nothing, as a window with a sheet does on macOS and a disabled
  owner on Windows: the dialog is answered first. A close the dialog has
  already queued for itself lets the owner's through. A forced close of the
  owner (`ctx.close_window_by_id(owner)`, `close_window_forced()` in its
  tree, `WindowState::close()`, `ctx.quit_forced()`) closes its dialogs first,
  without asking their close guards, and answers each through its own
  Cancel path: its `on_dismiss` runs as if it had been dismissed, so a
  `MessageBox` reports its escape button (Cancel, or No for a Yes/No box),
  dismissed programmatically. A dialog presented in the window's own tree is
  answered the same way. A
  window the OS destroys on its own (Windows destroys an owned window with
  its owner) is forgotten as if closed.
- **A dialog minimised anyway** (a window manager that offers to, or a
  program minimising it) comes back when the window it blocks is clicked or
  takes the focus (its title bar, Alt+Tab, its taskbar button).
- **Wayland**: winit gives a window no parent there, so the compositor treats
  the dialog as a window of its own, and no protocol tells a client that its
  window was minimised or restored. A dialog therefore does not follow its
  owner: it stays on screen when the user minimises the owner. Teksilo could
  minimise a dialog with an owner it minimised itself (`set_minimized`), but
  it would never learn of the owner's restore and could not bring the dialog
  back with it, so it does not. A dialog the compositor minimised comes back
  only when the user restores it; a click on the window it blocks asks the
  compositor to draw attention to it.

---

## Intercepting close / quit: confirmation guards

A window can refuse to close. Each `WindowConfig` carries an optional
**close guard** that the framework runs, with a real `EventContext`
for that window's own tree, *before* any **interactive** close gesture
tears the window down:

- the OS close button, `Alt+F4`, `Cmd+W` (winit `CloseRequested`);
- a custom-chrome (Teksilo-drawn) title-bar close button;
- a handler calling `ctx.close_window()`;
- a guarded quit (`ctx.quit()`), which asks every window in turn (below).

The guard answers with a `CloseResponse`:

| Answer | Meaning |
| --- | --- |
| `CloseResponse::Close` | the window closes |
| `CloseResponse::Veto` | a final no: the window stays open, and a quit that asked is abandoned |
| `CloseResponse::Defer(decision)` | the answer comes later, through the `CloseDecision` |

Under the default quit policy, quitting the app is the last window
closing, so a guard that refuses the final window's close also keeps the
app alive.

Guards are **strictly per-window**, closing one window never consults
another's guard, so this is correct for multi-window apps: an editor
window with unsaved changes can refuse its own close while a tool palette
beside it closes freely.

### Asking first: `Defer` and `CloseDecision`

A confirmation dialog is asynchronous, it waits for a click, so the
guard cannot answer "close?" on the spot. It **defers**: it returns
`CloseResponse::Defer` with a `CloseDecision`, and the dialog's answer
calls `proceed()` or `cancel()` on a clone of it:

```rust
use teksilo::prelude::*;                 // CloseResponse, CloseDecision
use teksilo::widgets::{MessageBox, MessageBoxButtons, StandardButton,
                       EventContextMessageBoxExt};

WindowConfig::new()
    .title("Editor")
    .on_close_requested(move |ctx| {
        if !dirty.get() {
            return CloseResponse::Close;     // nothing unsaved: just close
        }
        let decision = CloseDecision::new();
        let answer = decision.clone();
        ctx.present_message_box(
            MessageBox::question(lit!("Close window?"))
                .text(lit!("The document has unsaved changes."))
                .buttons(MessageBoxButtons::SaveDiscardCancel)
                .on_result(move |r, _ctx| match r.button {
                    StandardButton::Save    => { save(); answer.proceed(); }
                    StandardButton::Discard => answer.proceed(),
                    _                       => answer.cancel(),   // stay open
                }),
        );
        CloseResponse::Defer(decision)       // answered by the box
    });
```

- `proceed()` closes the window, without asking the guard again.
- `cancel()` leaves it open.
- The first answer counts; later ones are ignored.
- Dropping every clone without answering is a cancel, and a debug build
  says so: a guard that defers owes an answer.
- While the answer is pending, another close of the window (a second click
  on its close button) asks nothing and brings it forward, so the user
  never sees two questions.
- The decision is `Clone + Send + 'static`: answer it from a dialog, an
  async completion (`spawn_local_with`), or another thread. An answer from
  outside the event loop wakes it.
- A decision whose window has closed meanwhile (closed by id, or by a
  forced quit) is answered into the void: nothing happens.

The decision knows which window it is for, so it does not matter where the
answer runs. A `MessageBox` is presented in the guarded window's own tree on
Linux, but in a native modal window of its own on macOS and Windows, where a
`ctx.close_window_forced()` from its answer would close the dialog rather
than the document window. To close a window from code that runs elsewhere,
name it: `ctx.close_window_by_id(id)` closes it unconditionally.

### Reactive sugar: `can_close` + `on_close_blocked`

When the gate is a single reactive flag, skip the closure:

```rust
let may_close = dirty.not();              // Signal<bool>

WindowConfig::new()
    .can_close(may_close)                 // false: veto
    .on_close_blocked(move |ctx, decision| { // run only when blocked
        ctx.present_message_box(/* why, or a confirmation; its answer ends in
                                   decision.proceed() or decision.cancel() */);
    });
```

`can_close` is evaluated *before* `on_close_requested`: a `false` signal
blocks the close; a `true` signal (or no signal) falls through to the guard,
then to closing. What a block does depends on `on_close_blocked`:

- Not set: the close is vetoed. The veto is final, so a quit that reaches
  the blocked window is abandoned.
- Set: the close defers. The callback gets a `CloseDecision` for it and
  answers it, now or from a dialog's result callback: `proceed()` closes the
  window without asking again and lets a quit go on, `cancel()` keeps the
  window and abandons a quit. A callback that only says why calls `cancel()`
  at once; one that drops the decision unanswered cancels, and a debug build
  warns.

### Which closes are guarded

| Close origin | Guarded? |
| --- | --- |
| OS close button / `Alt+F4` / `Cmd+W` | Yes |
| Custom-chrome title-bar close button | Yes |
| `ctx.close_window()` | Yes |
| `ctx.close_window_forced()` | No, bypasses |
| `ctx.close_window_by_id(id)` | No, bypasses (explicit programmatic close) |
| `WindowState::close()` | No, bypasses |
| `CloseDecision::proceed()` | No (the guard has answered) |
| Modal-dismissal / framework teardown | No, bypasses |
| A window's dialogs, when the window closes | No (closed with it, answered through their Cancel path) |
| `ctx.quit()` | Yes, each window in turn (below) |
| `ctx.quit_forced()` | No, bypasses |
| macOS Dock Quit, AppleScript `quit`, `terminate:`, logout | Yes, each window in turn, as `ctx.quit()` |
| Windows sign out, restart, shut down | Asked once, cannot refuse (an overruling quit, below) |
| `SIGTERM`, `SIGHUP` (Unix), unless the process was started with it ignored | Asked once, cannot refuse (an overruling quit, below) |

A window with no guard configured always closes immediately, the guard
machinery only runs when `on_close_requested` or `can_close` is set. A
guarded close of a window a modal dialog blocks never reaches its guard:
the dialog is brought forward instead (see
[A dialog belongs to the window it blocks](#a-dialog-belongs-to-the-window-it-blocks)).

### Quitting the application

`ctx.quit()` is what a Quit menu item calls, the guarded quit, as
`ctx.close_window()` is the guarded close. It asks the application first
(below), then every open window in turn, as if the user closed it, and the
application ends once no window is left, whatever its `QuitPolicy`. Modal
windows are asked before the windows they block, then the others from the
most recently used. Each guard answers as it does a close:

- `Close`: the window closes and the quit goes on with the next one;
- `Veto`: the quit is abandoned at once;
- `Defer`: the quit waits on that window, brought to the front (restored
  if minimised) so the user sees the question. `proceed()` closes it and
  the quit goes on; `cancel()` abandons the quit. A window closed another
  way meanwhile (by id) lets the quit go on too.

A window whose own close is already deferred when the quit comes (the user
clicked its close button and its question is up) is not asked again: the
quit waits on that answer. A window already closing (a guard closed it by
id) is not asked either. A guard may itself ask for the quit (closing the
last document window quits the application) and refuse its window's close:
the quit then asks it again, `ctx.quit_in_progress()` now true.

#### The application can refuse

`TeksiloAppBuilder::on_quit_requested` installs the application's quit
guard, declared on the builder as a window's guard is declared on its
`WindowConfig`. It is asked before any window, in the application tree (so
it can present a dialog with no window open), and answers with the same
`CloseResponse`: `Close` lets the quit go on to the windows, `Veto`
abandons it with every window still open, and `Defer` holds it until its
`CloseDecision` is answered. A forced quit does not ask it.

```rust
TeksiloAppBuilder::new()
    .on_quit_requested(move |ctx| {
        if !downloads.is_busy() {
            return CloseResponse::Close;
        }
        let decision = CloseDecision::new();
        let answer = decision.clone();
        ctx.present_message_box(
            MessageBox::question(lit!("Downloads are running. Quit anyway?"))
                .buttons(MessageBoxButtons::YesNo)
                .on_result(move |r, _ctx| match r.button {
                    StandardButton::Yes => answer.proceed(),
                    _ => answer.cancel(),
                }),
        );
        CloseResponse::Defer(decision)
    })
```

The application is asked first because the quit closes windows as it goes:
asked last, it could only refuse with the windows already gone. Qt does
the same (`QEvent::Quit` reaches the application before its windows are
closed), and so does Electron (`before-quit` before the windows' `close`).
AppKit asks its delegate's `applicationShouldTerminate:` after reviewing
the documents, but that review saves documents and closes no window.

#### Where a quit comes from

- `ctx.quit()`, from any handler of a window or of the application tree:
  the guarded quit above. A Quit row (or the Quit of a tray menu the
  application builds) runs an action that calls it.
- The macOS App menu's Quit: sends its `quit_intent` if the app declared
  one and something takes it, and otherwise asks for the guarded quit
  itself (see the [native menu guide](native-menu.md#quit)).
- `ctx.quit_forced()`: closes every window without asking any guard.
- The operating system's own requests, below.

Every source goes through one request inside `teksilo-app`, which names
the source and can report how the quit ended, so a request from the
operating system is answered once it is decided.

#### When the operating system asks

| Platform | What asks | How the quit goes | `ctx.quit_source()` |
| --- | --- | --- | --- |
| macOS | the Dock menu's Quit, AppleScript's `quit app`, a menu item or another app sending `terminate:` | guarded; AppKit is told the outcome | `UserQuit` |
| macOS | log out, restart, shut down | guarded; a refusal cancels the logout, as AppKit applications do | `SessionEnd` |
| Windows | sign out, restart, shut down (`WM_ENDSESSION`) | overruling | `SessionEnd` |
| Windows | `taskkill` without `/F` (`WM_CLOSE` to each window) | each window's own guarded close, as its close button | `None` |
| Linux, macOS, other Unix | `SIGTERM` (`kill`, a service manager, the session ending), `SIGHUP` (the terminal or the login session that started the app closed), each unless the process was started with it ignored (`nohup`) | overruling | `Signal` |
| Linux, macOS, other Unix | `SIGINT` (Ctrl+C in the terminal that started the app), `SIGKILL` | the process ends at once, nothing asked or written | |

An **overruling** quit is for a source that leaves no time to ask: it is
carried out at once, in one turn. The application's guard and then every
window's (dialogs first) are asked **once**, and read the source, so a
guard can save; whatever a guard answers, its window closes: a `Veto` or
a `Defer` is overruled, and a dialog still up answers through its own
Cancel path. A guard that would ask "Save changes?" therefore saves (or
writes a recovery copy) for `SessionEnd` and `Signal` and returns `Close`,
as the guard in "Where the quit came from" below does. The application ends once every window has
closed, under either quit policy.

**Settings are written on every way out**: a quit that ends the
application, an overruling quit before it returns, and the end of the
event loop. That covers the settings files the application opened itself
(`SettingsFile`, `PersistedListModel`, `MruList`, a `SettingsStore`),
whether or not it has a `SettingsBundle` (`TeksiloAppBuilder::settings`),
and with a bundle its store and window state too.

**macOS.** AppKit asks before it ends the application, for the Dock
menu's Quit, AppleScript's `quit app "…"`, any `terminate:` (a menu item
of the app's own, another application) and a logout, restart or shutdown.
Teksilo answers that it will say later (`NSTerminateLater`) and runs a
guarded quit, so a guard can ask "Save changes?" and the user can answer
it: the windows and their dialogs keep working meanwhile. A logout is told
apart by the reason AppKit gives (`SessionEnd`), so a guard can also save
without asking. AppKit hears the outcome once the quit is decided: when it
completes, the settings are written and AppKit ends the process; when a
guard refuses or the user cancels, the application goes on (a logout is
then cancelled, and macOS says which application cancelled it). The App
menu's Quit row and ⌘Q are the application's own Quit, as before; see
the [native menu guide](native-menu.md#quit).

**Windows.** A session ending (sign out, restart, shut down) is heard on
a hidden window of Teksilo's own, so an application with no window open
hears it too. Teksilo does not hold the session up: Microsoft asks
applications to let it end, and a block puts the user in front of the
full-screen "this app is preventing shutdown" list, where they end the
session anyway. When Windows says the session really ends
(`WM_ENDSESSION`), the application runs an overruling quit with
`SessionEnd` before it answers, because Windows may end the process as
soon as it does: every guard is asked once and should save, the settings
are written, and Windows allows a few seconds for all of it. `taskkill`
without `/F` sends `WM_CLOSE` to every window of the process, which closes
each window through its guard as its close button does; the application
then ends under the default quit policy once its last window closes.
`taskkill /F` ends the process at once, as it ends any program.

**Termination signals.** `SIGTERM` and `SIGHUP` quit through the guards
while the event loop runs. A service manager sends `SIGTERM`, waits, then
sends `SIGKILL`: a guarded quit that opened a dialog nobody sees would
wait for the kill, which is why the quit is overruling. A signal that
arrives once the quit has completed (systemd ends a login session with
`SIGTERM` and `SIGHUP` together) asks nobody again. Before the loop
starts and once it has ended they keep their default meaning (the process
ends). A signal the process was started with ignored stays ignored, during
the loop and after it: under `nohup`, `SIGHUP` neither quits the
application nor ends it. `SIGINT` keeps its default too, as in Qt and GTK applications:
Ctrl+C in a terminal is a developer stopping the program. An application
that handles these signals itself can turn this off on the builder, and
its own handlers then run alone.

**Logging out on Linux.** Teksilo does not register with a session
manager (XSMP, GNOME's `org.gnome.SessionManager`, the desktop portal's
`Inhibit` monitor), so a logout does not ask its guards before the
desktop's logout screen, and nothing holds the logout for an unsaved
document. The logout reaches the application in one of two ways: its
processes are stopped (systemd stopping the session's or the
application's scope sends `SIGTERM`, with `SIGHUP` for a login session,
and `SIGKILL` after its timeout), which is the `Signal` quit above; or the
display server goes away first (the X server or the Wayland compositor
exits), which ends the event loop with no guard asked, after which `run`
writes the settings and returns.

#### Dialogs still up

A dialog still up in or over a window the quit closes is answered through
its own Cancel path before it goes, as for a forced close of its owner: a
`MessageBox` reports its escape button (Cancel, or No for a Yes/No box),
dismissed programmatically. A native dialog
window is asked before the window it blocks, and closes this way.

#### Wording, forcing, cancelling

A guard reads `ctx.quit_in_progress()` to word its question for the whole
application, and a dialog's "Quit anyway" answer calls `ctx.quit_forced()`,
which closes every window without asking. `ctx.cancel_quit()` abandons a
guarded quit from anywhere; it does nothing to a forced quit or to one for
a source that leaves no time to ask (a termination signal, a Windows
session ending now), so a dialog's Cancel arm run as its window closes
cannot stop such a quit with half the windows closed. Asking for a guarded
quit while one is under way changes nothing.

```rust
// The guard of the close-confirmation demo, quit-aware:
.on_close_requested(move |ctx| {
    if !dirty.get() {
        return CloseResponse::Close;
    }
    let question = if ctx.quit_in_progress() {
        lit!("Quit with unsaved changes?")
    } else {
        lit!("Close window?")
    };
    let decision = CloseDecision::new();
    let answer = decision.clone();
    ctx.present_message_box(
        MessageBox::question(question)
            .buttons(MessageBoxButtons::SaveDiscardCancel)
            .on_result(move |result, _ctx| match result.button {
                StandardButton::Save | StandardButton::Discard => answer.proceed(),
                _ => answer.cancel(),
            }),
    );
    CloseResponse::Defer(decision)
})
```

#### Where the quit came from

`ctx.quit_source()` is the source of the quit under way, `None` when no
quit is under way (a guard asked for a window the user closes during a
quit reads the quit's source too). `QuitSource` says who asked: `Application`
(`ctx.quit()`, `ctx.quit_forced()`, the App menu's Quit), `UserQuit` (the
macOS Dock menu's Quit, AppKit's `terminate:`), `SessionEnd` (log out,
restart, shut down) or `Signal` (`SIGTERM`, `SIGHUP`); the enum is
`#[non_exhaustive]`. A guard for a document skips its dialog when the user
is not there to answer it, as AppKit and Qt applications do: for
`SessionEnd` and `Signal` it saves directly and returns `Close` (on
Windows and for a signal the quit is overruling, above, and closes the
window whatever it answers, so a dialog shown then is never answered; a
macOS logout waits for the answer, but nobody logging out expects a
question). The guard of `on_quit_requested`
reads it the same way.

```rust
.on_close_requested(move |ctx| {
    if !dirty.get() {
        return CloseResponse::Close;
    }
    if matches!(ctx.quit_source(), Some(QuitSource::SessionEnd | QuitSource::Signal)) {
        save();
        return CloseResponse::Close;
    }
    // ... ask the user, and defer
})
```

Working demo: `cargo run -p close-confirmation` (main window: a deferring
`on_close_requested` with Save/Discard/Cancel, and a Quit button; second
window: the `can_close` sugar, whose `on_close_blocked` answers the close's `CloseDecision`).

---

## `TeksiloAppBuilder::run()` lifecycle

1. `run()` builds a `TeksiloAppHandler` and spins up the winit event loop.
2. On `resumed()`, the handler calls
   `WindowManager::create_window(initial_window_config, event_loop)`,
   synchronous winit creation, widget tree built, first paint requested.
   An app with no initial window skips this. The application tree's
   mount actions run next (they may open the first window); under
   `QuitPolicy::LastWindowClosed`, no window open after them is a panic.
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
     close guard, may be vetoed or deferred) or *forced* (explicit programmatic
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
6. Each `process_pending` first settles the closes guards deferred whose
   `CloseDecision` was answered; after it, a quit under way takes its next
   steps (`WindowManager::advance_quit`). At the end of `post_event` and of
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
    // soft-keyboard support, and the quit (quit, quit_forced,
    // quit_in_progress, quit_source, cancel_quit).
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
