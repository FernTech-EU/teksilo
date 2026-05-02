# Multi-Window API Plan

Synchronous, state-driven multi-window API for FernUI. No drain queue for
creates, no pending-id placeholders, no dual boolean/enum state, no
backwards-compatibility shims. One path end-to-end.

## Context

FernUI today has working multi-window *infrastructure* in
[`WindowManager`](../../crates/fern-app/src/window_manager.rs) — per-window
`WidgetTree` + `PlatformWindow`, modal dialogs, theme/locale broadcast,
string-id lookup, custom chrome per window — but no *public* API to open a
secondary window from a handler. The only entry points today are
`FernAppBuilder::initial_window` at boot and the internal
`ModalRequest::NativeWindow` path in
[`app.rs:377`](../../crates/fern-app/src/app.rs#L377).

Window state (fullscreen, maximize, size) is also split: maximize lives on
[`PlatformTitleBarHost`](../../crates/fern-core/src/window_chrome.rs) as a
chrome-coupled `Signal<bool>`, everything else is absent. A toolbar button
can't reflect fullscreen state reactively without new plumbing.

This plan unifies both problems behind a single `WindowState` abstraction
reached through `ctx.window()` from any handler or `build()` site, plus a
synchronous `ctx.open_window(config)` for creation.

### Reference reading

- [`docs/shortcut-intent-action.md`](../shortcut-intent-action.md) — the
  Action / Intent / Shortcut triad that this plan plugs into.
- Compose Desktop window management ([JetBrains docs][1]) — source of the
  `WindowPlacement` enum design.
- Compose issues [#1489][2] and [#4006][3] — the cautionary tales for
  OS↔app state sync drift.

[1]: https://www.jetbrains.com/help/kotlin-multiplatform-dev/compose-desktop-top-level-windows-management.html
[2]: https://github.com/JetBrains/compose-multiplatform/issues/1489
[3]: https://github.com/JetBrains/compose-multiplatform/issues/4006

## Design targets

1. **Declarative window state**. `WindowState` carries `Signal`s for
   placement, title, size, position, focus, resizability — widgets bind
   directly, no imperative setter parade.
2. **Synchronous `open_window`**. Call returns the real `FernWindowId`; the
   winit window is created inside the same tick. No reserved placeholders,
   no deferred creation, no deferred handle types.
3. **Single source of truth**. OS-initiated state changes (user presses
   F11, drags to top edge, green-lights zoom on macOS) write back into the
   same signals that app code writes — distinguished only by a
   re-entrancy guard (`applying_from_os`) so the writeback doesn't re-loop
   to the OS.
4. **No back-compat.** `WindowConfig::size/fullscreen/maximized/custom_chrome`
   fields are rewritten, not augmented.
    `FernAppBuilder::{window_title, window_size, root}` convenience methods
   are deleted. The `ModalRequest::NativeWindow` special path is collapsed
   into the generic `open_window` flow.
5. **Structural clarity**. `FernApp → WindowState → root Widget`. The root
   widget lives *inside* a window; it does not *become* one (Slint's
   mistake). Actions, Intents and Shortcuts remain app-wide.

## 1. New types — `fern-core`

### `WindowPlacement` (`crates/fern-core/src/window/placement.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPlacement {
    Floating,
    Maximized,
    Fullscreen,
    Minimized,
}
```

Size and position are *not* inside `Floating`. They are independent
signals on `WindowState` that always hold the last-known *restored*
values. That matches OS behavior (macOS `frameAutosaveName`, Windows
`WINDOWPLACEMENT`) and avoids "what size do I go back to?" ambiguity.

### `DecorationsMode`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationsMode {
    Native,          // OS chrome
    CustomChrome,    // PlatformTitleBarHost (Wayland / Windows / macOS)
    None,            // borderless, no host
}
```

Replaces `custom_chrome: bool`. Three-valued, explicit.

### `WindowState` (`crates/fern-core/src/window/state.rs`)

```rust
#[derive(Clone)]
pub struct WindowState(Rc<WindowStateInner>);

struct WindowStateInner {
    id: FernWindowId,
    string_id: Option<String>,

    // Writable signals — bound by widgets.
    placement: Signal<WindowPlacement>,
    title: Signal<String>,
    size: Signal<(u32, u32)>,           // restored size; OS-synced
    position: Signal<(i32, i32)>,       // restored pos;  OS-synced
    focused: Signal<bool>,
    resizable: Signal<bool>,
    always_on_top: Signal<bool>,

    // OS commands emitted by app-side signal writes. Drained by
    // WindowManager each tick AFTER event dispatch. Not a deferral —
    // a single write-back channel for a state-driven model.
    pending_os_commands: RefCell<Vec<WindowCommand>>,

    // Re-entrancy guard: true while applying an OS-originated change.
    // Blocks the observer from pushing the same change back to the OS.
    applying_from_os: Cell<bool>,
}

pub(crate) enum WindowCommand {
    SetPlacement(WindowPlacement),
    SetTitle(String),
    SetSize(u32, u32),
    SetPosition(i32, i32),
    SetResizable(bool),
    SetAlwaysOnTop(bool),
    RequestAttention(UserAttentionKind),
    Focus,
    Close,
}
```

Public API:

```rust
impl WindowState {
    pub fn id(&self) -> FernWindowId;
    pub fn string_id(&self) -> Option<&str>;

    pub fn placement(&self)     -> &Signal<WindowPlacement>;
    pub fn title(&self)         -> &Signal<String>;
    pub fn size(&self)          -> &Signal<(u32, u32)>;
    pub fn position(&self)      -> &Signal<(i32, i32)>;
    pub fn focused(&self)       -> &Signal<bool>;
    pub fn resizable(&self)     -> &Signal<bool>;
    pub fn always_on_top(&self) -> &Signal<bool>;

    pub fn request_attention(&self, kind: UserAttentionKind);
    pub fn focus(&self);
    pub fn close(&self);
}
```

Each signal is wired in the constructor with an observer that, **only
when `applying_from_os == false`**, pushes a `WindowCommand` into
`pending_os_commands`. The setter for OS-originated changes is
`pub(crate)` on `WindowStateInner`:

```rust
pub(crate) fn set_placement_from_os(&self, p: WindowPlacement) {
    self.applying_from_os.set(true);
    self.placement.set(p);
    self.applying_from_os.set(false);
}
```

This is the single concrete mechanism behind the re-entrancy guard.

## 2. `WindowConfig` rewrite — `fern-app`

```rust
pub struct WindowConfig {
    title: String,
    string_id: Option<String>,
    size: (u32, u32),                    // restored size; always set
    position: Option<(i32, i32)>,
    min_size: Option<(u32, u32)>,
    max_size: Option<(u32, u32)>,
    initial_placement: WindowPlacement,  // default Floating
    decorations: DecorationsMode,        // default Native
    resizable: bool,                     // default true
    always_on_top: bool,                 // default false
    skip_taskbar: bool,                  // default false
    icon: Option<WindowIcon>,
    modal: Option<ModalConfig>,
    root_builder: Box<dyn FnOnce(&mut WidgetTree, WindowState) -> WidgetId>,
}

pub struct ModalConfig {
    pub parent: FernWindowId,
    pub focus_target: Option<WidgetId>,
}
```

Notes:

- `root_builder` now takes `(tree, WindowState)`. Widgets that need to
  bind against their own window pick up the `WindowState` clone from the
  builder closure.
- `modal` is `Option<ModalConfig>` — you cannot say `.modal(true)`
  without naming a parent. The type system enforces what today's code
  only checks at runtime.
- Deleted: `.custom_chrome(bool)`, separate `.width` / `.height`,
  `.fullscreen(bool)`, `.maximized(bool)`. All subsumed by
  `.size(...)` / `.initial_placement(...)` / `.decorations(...)`.

## 3. `EventContext` extensions — `fern-core`

```rust
impl EventContext<'_> {
    pub fn window(&self) -> &WindowState;

    pub fn open_window(&mut self, config: WindowConfig) -> FernWindowId;
    pub fn find_window(&self, string_id: &str) -> Option<FernWindowId>;
    pub fn focus_window(&mut self, id: FernWindowId);
    pub fn close_window_by_id(&mut self, id: FernWindowId);
    pub fn window_state(&self, id: FernWindowId) -> Option<&WindowState>;
    pub fn windows(&self) -> impl Iterator<Item = &WindowState>;

    // close_window() keeps its current signature and routes to
    // close_window_by_id(self.window().id()).
}
```

All of these call through `WindowOps` (see §4). `open_window` returns the
freshly-allocated `FernWindowId` **synchronously** — the winit window is
created, the `ManagedWindow` is inserted into the registry, and the first
layout+paint happens in the next event-loop tick.

### `BuildContext` mirror

```rust
impl BuildContext<'_> {
    pub fn window(&self) -> &WindowState;
}
```

So widgets can grab signals during `build()`:

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let fs = ctx.window().placement().map(|p| matches!(p, WindowPlacement::Fullscreen));
    // ...
}
```

## 4. Dispatch restructure — `fern-app`

This is the core of "no deferred creates." Today `EventContext` has no
way to reach `WindowManager` — that is why the drain pattern exists. We
fix that by threading a `WindowOps` borrow into dispatch.

### Split `ManagedWindow`

```rust
pub(crate) struct ManagedWindow {
    pub fern_id: FernWindowId,
    pub string_id: Option<String>,
    pub tree: WidgetTree,                 // borrowed mutably during dispatch
    pub state: WindowState,               // cloned into WidgetTree + WindowOps
    pub platform_window: PlatformWindow,
    pub translation_state: TranslationState,
    pub current_modifiers: ModifiersState,
    pub modal: Option<ModalConfig>,
    pub title_bar_host: Option<Rc<dyn PlatformTitleBarHost>>,
    pub focused: bool,
    pub occluded: bool,
}
```

### `WindowOps` — dispatch context

```rust
pub struct WindowOps<'a> {
    wm: &'a mut WindowManager,      // current window temporarily removed
    event_loop: &'a ActiveEventLoop,
    typesetter: Option<&'a SharedTypesetter>,
    theme: &'a Theme,
}

impl WindowOps<'_> {
    pub(crate) fn create_window(&mut self, config: WindowConfig) -> FernWindowId {
        // ActiveEventLoop is right here — synchronous winit create.
        self.wm.create_window(config, self.event_loop)
    }
    pub(crate) fn focus(&mut self, id: FernWindowId);
    pub(crate) fn close(&mut self, id: FernWindowId);
    pub(crate) fn state(&self, id: FernWindowId) -> Option<&WindowState>;
    pub(crate) fn states(&self) -> impl Iterator<Item = &WindowState>;
}
```

### Re-entry pattern on `WindowManager`

```rust
pub fn dispatch_in_window<F, R>(
    &mut self,
    fern_id: FernWindowId,
    event_loop: &ActiveEventLoop,
    f: F,
) -> R
where
    F: FnOnce(&mut WidgetTree, &mut WindowOps) -> R,
{
    let winit_id = self.fern_to_winit[&fern_id];
    let mut current = self.windows.remove(&winit_id)
        .expect("dispatch_in_window on unknown window");

    let mut ops = WindowOps {
        wm: self,
        event_loop,
        typesetter: self.typesetter.as_ref(),
        theme: &self.theme,
    };
    let result = f(&mut current.tree, &mut ops);

    self.windows.insert(winit_id, current);
    result
}
```

Temporary removal + reinsertion is the standard Rust pattern for "re-enter
a collection while holding one of its elements." Zero runtime cost,
borrow-checker happy, no interior mutability, no unsafe.

### Callers

Every `tree.dispatch_event(evt)` site in [`app.rs`](../../crates/fern-app/src/app.rs)
becomes:

```rust
self.wm.dispatch_in_window(fern_id, event_loop, |tree, ops| {
    tree.dispatch_event(evt, ops)
});
```

`WidgetTree::dispatch_event` takes `&mut WindowOps` and hands it to each
`EventContext` it constructs.

## 5. `WindowManager::create_window` rewrite

- Accept the new `WindowConfig`.
- Build the `WindowState` with initial signal values from config.
- Wire the OS-sync observers: one `on_change` per signal, guarded by
  `applying_from_os`, pushing into `pending_os_commands`.
- Pass `state.clone()` to `root_builder`.
- Store `state.clone()` on `ManagedWindow` and on the `WidgetTree`
  (reachable from `BuildContext::window()` / `EventContext::window()`).

### Post-dispatch drain

```rust
fn drain_window_commands(&mut self) {
    for managed in self.windows.values_mut() {
        for cmd in managed.state.0.pending_os_commands.borrow_mut().drain(..) {
            apply_command(&managed.platform_window, cmd);
        }
    }
}
```

`apply_command` is a match on `WindowCommand` calling the appropriate
winit `Window` method. **This is not a deferral of creation** — it is the
app→OS writeback for reactive state changes, the only correct way to push
state to winit without borrow conflicts against the owning window.

### OS-originated writeback

In `handle_window_event_inner` for every OS event that changes state:

```rust
WindowEvent::Resized(new_size) => {
    let managed = self.wm.get_by_winit_mut(window_id).unwrap();
    managed.platform_window.resize(new_size);
    managed.state.0.set_size_from_os(logical_size(new_size, sf));
    managed.state.0.set_placement_from_os(query_os_placement(&managed.platform_window));
}
WindowEvent::Focused(focused) => {
    managed.state.0.set_focused_from_os(focused);
}
```

The platform-specific "is this window fullscreen / maximized / minimized?"
query lives in `fern-platform` and runs from the `Resized` handler —
matches the pattern established in
[`window_chrome.rs:60`](../../crates/fern-core/src/window_chrome.rs#L60)
for maximize.

## 6. Tree-level wiring — `fern-core::WidgetTree`

- Store `WindowState` on `WidgetTree` (`Option` until a window has
  attached — always `Some` under an app).
- Expose it via `BuildContext::window()` and `EventContext::window()`.
- Drain slots `pending_locale_request` and `close_window_requested` stay.
  Those are lifecycle one-shots, orthogonal to the state-driven design.
  Migrating them onto the same `Signal + pending_os_commands` model is a
  follow-up (see §12).

## 7. Modal dialog path

[`app.rs:377`](../../crates/fern-app/src/app.rs#L377) (the
`ModalRequest::NativeWindow` path) is deleted and replaced by a helper
that builds a `WindowConfig` and calls `ctx.open_window(...)`. One path
for "open a window," zero special-casing in `FernAppHandler`.

```rust
// fern-core, on EventContext:
pub fn open_modal(&mut self, request: ModalRequest) -> FernWindowId {
    let config = WindowConfig::new()
        .title(request.title.unwrap_or_default())
        .size(request.size.unwrap_or((480, 320)))
        .modal(ModalConfig {
            parent: self.window().id(),
            focus_target: request.focus_target,
        })
        .root(move |tree, _state| (request.build)(tree));
    self.open_window(config)
}
```

The `modal_blocked` machinery in `WindowManager` (input-blocking on the
parent, refocus-on-stolen-focus) stays — triggered by
`config.modal.is_some()` at create time.

## 8. `FernAppBuilder`

```rust
impl FernAppBuilder {
    pub fn initial_window(mut self, config: WindowConfig) -> Self;
    // DELETED: window_title, window_size, root.
}
```

No back-compat aliases. Users write `WindowConfig::new().title(..).size(..).root(..)`
explicitly. The `main()` shape in §13 is the one canonical shape.

## 9. Platform-title-bar host cleanup

`PlatformTitleBarHost::{is_maximized_signal, minimize, toggle_maximize,
close, is_maximized, notify_window_resized}` become redundant — those
operations all go through `WindowState` now.

- **Delete them from the trait.** Keep `reserved_leading_inset`,
  `reserved_trailing_inset`, `renders_custom_controls`,
  `needs_custom_resize_handles`, `begin_drag`, `begin_resize`,
  `show_window_menu`, `update_hit_regions`. Those are genuinely
  chrome-specific.
- The `TitleBar` widget binds to `ctx.window().placement()` for its
  maximize button, not to a chrome-specific signal. Works in
  `DecorationsMode::Native` too — new capability, not a regression.
- OS-state querying moves into `fern-platform` and is driven by
  `WindowEvent::Resized` in the manager, not by the host.

## 10. Cross-cutting

### Id allocation

`FernWindowId::new(n)` stays as the `Copy` opaque id. Allocation remains
in `WindowManager::alloc_id`. `open_window` reserves and creates in one
call — returns the real id. No placeholders.

### Threading

`open_window` asserts UI-thread. Background tasks that want a window emit
an `AppEvent` through the `TreeAppContext` → UI-thread handler calls
`open_window` → done. Unchanged from today's background-event story.

### i18n seeding

The per-window locale/direction seed in
[`window_manager.rs:320`](../../crates/fern-app/src/window_manager.rs#L320)
stays. Runs before `root_builder`. `WindowState` has no locale — i18n is
app-wide.

### Accessibility

`WidgetTree::set_accessibility_preferences` call stays. Unchanged.

## 11. Implementation order

Each step is a self-contained PR that compiles and tests green.

1. **Types.** Introduce `WindowPlacement`, `WindowState`, `WindowCommand`,
   `DecorationsMode` in `fern-core`. No wiring; standalone types with
   tests (re-entrancy guard, pending-command accumulation).
2. **`WindowConfig` rewrite** in `fern-app`. Update the one call site in
   `app.rs:377` and every example. Delete the convenience methods on
   `FernAppBuilder`.
3. **`WindowOps` + `dispatch_in_window`** on `WindowManager`. Thread
   through `WidgetTree::dispatch_event`. Update every call site.
4. **`WindowState` into `ManagedWindow` + `WidgetTree`**.
   `BuildContext::window()` + `EventContext::window()` work.
5. **`EventContext::open_window`** and the rest. Delete the `queue_create`
   / `pending_creates` plumbing on `WindowManager`.
6. **OS→signal writeback** in `WindowEvent` handlers. `set_*_from_os`
   setters. Drop `notify_window_resized`.
7. **App→OS drain** (`drain_window_commands`) in `post_event`.
8. **`PlatformTitleBarHost` trim** — delete the state methods. Update
   `TitleBar` widget to bind to `WindowState::placement`.
9. **Modal rewrite** — delete `ModalRequest::NativeWindow` special path,
   route through `open_window`.
10. **Multi-window example** — `examples/multi_window/` with a main editor
    + detached inspector. Exercises `open_window`, cross-window signal
    reads, `focus_window`, fullscreen toggle.

Sites touched: `fern-core` (new `window/` submodule, `build_context.rs`,
`widget.rs`, `widget_tree.rs`), `fern-app` (`app.rs`, `window_config.rs`,
`window_manager.rs`), `fern-platform` (OS-placement query helper),
`fern-widgets` (`title_bar.rs` — signal source swap), every example
(`WindowConfig` API change).

## 12. Out of scope — intentional

- `always_on_bottom`, `content_protected`, per-monitor targeting, icon
  hot-swap, cursor operations on the window handle. Same mechanism
  extends trivially — add a variant to `WindowCommand`.
- Web backend multi-window semantics (FernUI is desktop).
- Theme / locale migration onto the same signal-driven writeback.
  Follow-up refactor after this lands.
- `FernWindowId` → string-id idempotence caching in `open_window`. Apps do
  the lookup themselves via `find_window` — simpler, explicit.

## 13. Canonical `main()` shape

```rust
use fern_ui::prelude::*;
use fern_ui::app::{FernAppBuilder, WindowConfig, WindowPlacement, DecorationsMode};
use fern_ui::core::{Action, shortcut::{KeyStroke, Shortcut}};

#[derive(Debug, IntentKind)]
enum AppIntent {
    #[name = "app.help"]              ShowHelp,
    #[name = "app.toggle_fullscreen"] ToggleFullscreen,
    #[name = "app.quit"]              Quit,
}

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI Demo")
                .id("main")
                .size(1200, 800)
                .min_size(640, 400)
                .initial_placement(WindowPlacement::Floating)
                .decorations(DecorationsMode::CustomChrome)
                .root(|tree, _state| tree.add(AppRoot::new())),
        )
        .run();
}

impl Widget for AppRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_shortcut_global(
            Shortcut::new("app.help")
                .name("Open help")
                .primary(KeyStroke::new(Key::F1, Modifiers::empty()))
                .build(),
        );
        ctx.register_shortcut_global(
            Shortcut::new("app.toggle_fullscreen")
                .name("Toggle fullscreen")
                .primary(KeyStroke::new(Key::F11, Modifiers::empty()))
                .build(),
        );

        ctx.register_action(Action::new("app.help").on_invoke(|_i, ctx| {
            if let Some(id) = ctx.find_window("help") {
                ctx.focus_window(id);
            } else {
                ctx.open_window(
                    WindowConfig::new()
                        .title("Help")
                        .id("help")
                        .size(720, 480)
                        .parent(ctx.window().id())
                        .root(|tree, _state| tree.add(HelpRoot)),
                );
            }
        }));

        ctx.register_action(Action::new("app.toggle_fullscreen").on_invoke(|_i, ctx| {
            let w = ctx.window();
            let next = match w.placement().get() {
                WindowPlacement::Fullscreen => WindowPlacement::Floating,
                _ => WindowPlacement::Fullscreen,
            };
            w.placement().set(next);
        }));

        let fs = ctx.window().placement().map(|p| matches!(p, WindowPlacement::Fullscreen));
        // ... content binding fs to a toolbar button's label ...
        todo!()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}
```
