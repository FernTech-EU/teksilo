# Shortcut / Intent / Action Reference

FernUI's input-to-behavior pipeline has three first-class concepts:

- **[`Shortcut`](../crates/fern-core/src/shortcut.rs)** — a rebindable
  keyboard binding (`KeyStroke` → intent name). Owned by the
  [`ShortcutRegistry`](../crates/fern-core/src/shortcut.rs); user
  rebindings layer on top of widget-declared defaults.
- **[`Intent`](../crates/fern-core/src/intent.rs)** — a runtime
  "something wants to happen" message: a stable name plus an optional
  type-erased payload. Produced by shortcuts, by widgets via
  `ctx.send_intent(...)`, or programmatically.
- **[`Action`](../crates/fern-core/src/action.rs)** — a widget-owned
  handler bound to an intent name. When an intent dispatches, the
  framework walks **source-widget → root** and lets the first matching
  enabled action consume (or propagate) it.

Pipeline in one line: `KeyStroke → Shortcut → Intent → Action handler`.

Typed DTO bridge between an app's intent enum and the runtime
[`Intent`]: the [`IntentKind`](../crates/fern-core/src/intent.rs) trait,
usually derived with `#[derive(IntentKind)]` from `fern-ui-macros`.

Full end-to-end example:
[`examples/shortcuts_demo`](../examples/shortcuts_demo/src/main.rs).

---

## Mental model: three paths, one dispatcher

Every intent hits the same dispatcher — actions don't care where the
intent came from. The three firing paths:

| Path                          | How the intent is built                                     | Anchor for source→root walk     |
|-------------------------------|-------------------------------------------------------------|---------------------------------|
| **Shortcut** (keyboard chord) | Registry invokes `on_activate` or synthesizes `Intent::new` | Focused widget or root fallback |
| **Widget handler** (`ctx.send_intent`) | Handler builds or returns an `Into<Intent>` value           | The originating widget          |
| **Programmatic** (tests, tools)        | Build `Intent` by hand or via `IntentKind::into_intent`     | Caller-supplied source id       |

The **name** is the dispatch key; the **payload** (if any) is
downcastable data the handler extracts when it needs typed fields.

---

## `KeyStroke`

A single chord — one `Key` plus its `Modifiers`:

```rust
KeyStroke::new(Key::S, Modifiers::CTRL)
KeyStroke::ctrl(Key::S)                        // same thing
KeyStroke::ctrl_shift(Key::S)
KeyStroke::alt(Key::Enter)
KeyStroke::new(Key::PageUp, Modifiers::NONE)   // plain PageUp
```

`Display` renders "Ctrl+S" style text. Widgets displaying shortcuts to users
should call `fern_widgets::keystroke_format::format_keystroke()` which handles
platform-specific symbols (⌘ on macOS) and locale-aware modifier names via
`tr_widget!` (e.g., "Strg" in German). `Serialize`/`Deserialize` are derived
so user overrides can be persisted.

---

## `Shortcut`

Declarative, rebindable record. Built with a fluent `ShortcutBuilder`:

```rust
use fern_ui::core::shortcut::{KeyStroke, Shortcut, ShortcutScope};

Shortcut::new("app.save")                  // stable id (dispatch key)
    .name("Save")                          // menu/settings label
    .category("File")                      // settings-UI grouping
    .primary(KeyStroke::ctrl(Key::S))      // default primary chord
    .secondary(KeyStroke::new(Key::F12, Modifiers::NONE))
    // .scope(ShortcutScope::Global)        // default
    // .scope_to(scope_root_id)             // widget-scoped variant
    // .enabled_when(has_selection_signal)  // reactive "live" predicate
    // .propagate_when_disabled(false)      // consume-when-disabled instead
    // .on_activate(|ks, ctx| AppIntent::ScrollBy(...))  // parametric
    .build();
```

Key fields ([source](../crates/fern-core/src/shortcut.rs)):

- **`id: &'static str`** — stable key used for persistence, menu
  lookups (`MenuItem::for_shortcut`), and dispatch. Dot-style
  convention: `"editor.format.bold"`. Doubles as the intent name when
  `.intent(...)` isn't set.
- **`primary` / `secondary`** — the two default chords. User overrides
  (loaded from disk or set through the settings UI) are applied per
  slot independently.
- **`scope: ShortcutScope`** — `Global` (fires regardless of focus) or
  `Scoped(WidgetId)` (fires only when focus is inside that subtree).
  Widget-declared shortcuts default to scoped; app-level declarations
  use global.
- **`on_activate`** — optional closure invoked at activation time.
  Receives the matched `KeyStroke` (so you can branch on which chord
  fired) and an `EventContext` (for side effects). Returns anything
  `Into<Intent>` — typically an `IntentKind` variant. Omit when the
  shortcut only needs the name: the registry synthesizes
  `Intent::new(intent_name)` for you.
- **`enabled_when: Option<Signal<bool>>`** — reactive "is this
  shortcut live?" predicate. When `false`, the shortcut is treated as
  *if not registered* — the keystroke falls through to the focused
  widget's normal `on_key` dispatch. Compose composite predicates with
  the [`Signal<bool>` combinators](#composing-enabled_when-predicates)
  (`and` / `or` / `not`) or [`Signal::zip`](../crates/fern-core/src/signal.rs)
  for typed tuples.
- **`propagate_when_disabled: bool`** — controls what happens when the
  matching `Action` is disabled: `true` (default) lets the intent
  continue bubbling; `false` consumes at that level ("owned but
  dormant").

### Composing `enabled_when` predicates

`enabled_when` takes any `Signal<bool>`, and `Signal` ships combinators
for multi-source predicates that correctly dirty-track every upstream
root:

```rust
let editor_focused: Signal<bool> = …;
let readonly:       Signal<bool> = …;
let in_editor:      Signal<bool> = …;

// `focus && !readonly && in_editor` — each source registered independently
// with the binding registry, so widgets observing `when` re-render on any flip.
let when = editor_focused.and(&readonly.not()).and(&in_editor);

Shortcut::new("edit.format.bold")
    .primary(KeyStroke::ctrl(Key::B))
    .enabled_when(when)
    .build();
```

Available on `Signal<bool>`: `and`, `or`, `not`. Available on any
`Signal<T: Clone>`: `zip(&Signal<U>) -> Signal<(T, U)>`,
`zip3(&Signal<U>, &Signal<V>) -> Signal<(T, U, V)>`, and `map` for
arbitrary projections. The same combinators work for `Action::enabled_when`.

---

## `ShortcutRegistry`

Two-layer store:

1. **Defaults** — records registered by widgets during `build()`.
   Re-registering the same id **upserts**: code-owned fields are
   refreshed, the user override is preserved.
2. **Overrides** — user-supplied keystroke rebindings keyed by
   shortcut id, persisted across widget rebuilds (*graveyard*
   semantics — a widget that disappears and reappears keeps its
   customised bindings).

The merged view is [`EffectiveShortcut`](../crates/fern-core/src/shortcut.rs):
primary/secondary = user override if touched, else declared default.
Menus, tooltips, and dispatch consume this shape.

Every mutation bumps `ShortcutRegistry::version()`, a `Signal<u64>`.
Menus, tooltips, and settings widgets observe it and re-read through
`effective(id)` to refresh labels after rebinds.

### Registration from a widget

Inside `build()`:

```rust
// Widget-scoped (default: Scoped(self_id) — fires only when focus is
// inside the widget's subtree):
ctx.register_shortcut(
    Shortcut::new("editor.format.bold")
        .name("Bold")
        .primary(KeyStroke::ctrl(Key::B))
        .build(),
);

// App-level (Global — fires regardless of focus):
ctx.register_shortcut_global(
    Shortcut::new("app.save")
        .name("Save")
        .primary(KeyStroke::ctrl(Key::S))
        .build(),
);
```

Both register **with ownership**: when the widget is destroyed or
rebuilt, the framework calls `unregister_all_for_owner(widget_id)` so
stale entries don't leak.

### Per-slot overrides

User overrides are per-slot (`SlotOverride::{Default, Bound(ks),
Unbound}`):

- `Default` — delegate to whatever default the shortcut currently
  declares (a later code-side change flows through).
- `Bound(ks)` — lock the slot to this chord.
- `Unbound` — lock the slot to *no* chord.

Rebinding primary does not disturb secondary, and vice versa. The
registry's `rebind_primary` / `rebind_secondary` only touch the
targeted slot — they do **not** auto-unbind conflicting shortcuts.
Use `ShortcutRegistry::find_conflict(keystroke, excluding_id)` before
rebinding if you want the "exactly one effective binding per chord"
invariant; that is what the pre-built
[`ShortcutSettings`](../crates/fern-widgets/src/shortcut_settings.rs)
widget does in its capture-event handler.

---

## `CaptureHandle` — one-shot key capture

Used to implement "press a chord" rebind UIs. `ctx.begin_key_capture`
returns a [`CaptureHandle`](../crates/fern-core/src/shortcut.rs): the
**next** KeyDown bypasses shortcut resolution and runs the callback
with access to the registry and an `EventContext`. RAII: dropping the
handle cancels an unfired capture.

```rust
let handle = ctx.begin_key_capture(|ks, registry, _ctx| {
    // Escape cancels, Del/Backspace unbinds, everything else rebinds.
    registry.rebind_primary("app.save", Some(ks));
});
self.active_capture = Some(handle);   // hold onto it
```

Re-arming (calling `begin_key_capture` again while a previous handle
is still alive) creates a fresh slot; the old slot is already orphaned
so dropping the old handle cancels only the old slot — no race with
the newer capture. The pre-built
[`ShortcutSettings`](../crates/fern-widgets/src/shortcut_settings.rs)
widget packages this flow (Rebind buttons, conflict resolution, reset).

---

## `Intent`

Runtime message — name + optional payload. Construction:

```rust
use fern_ui::core::Intent;

// Name-only (parameter-less):
let i = Intent::new("app.save");

// Typed payload (any T: 'static — stored in an Rc<dyn Any>):
let i = Intent::with_payload("app.scroll_by", -1_i32);
let i = Intent::with_payload("app.add_item", my_dto);

// Blanket conversion from any IntentKind variant:
let i: Intent = AppIntent::Save.into();
```

Recover the payload by type:

```rust
if let Some(&delta) = intent.payload::<i32>() {
    …
}
```

`from_intent` is the typed counterpart when the payload was built from
an `IntentKind`:

```rust
if let Some(AppIntent::Open(path)) = AppIntent::from_intent(intent) {
    open_file(path);
}
```

### `IntentResponse`

Action handlers return `IntentResponse`:

- **`Handled`** (default) — stop walking; the intent is consumed here.
- **`Propagated`** — observe-and-keep-going; ancestor widgets also get
  a chance. Useful when a widget wants to react (update a draft
  indicator) but lets an ancestor perform the primary action.

`ActionBuilder::on_invoke` always reports `Handled` — use
`on_invoke_with_response` when you need to propagate.

---

## `IntentKind` — typed DTO bridge

Use `#[derive(IntentKind)]` on an enum that catalogs the app's intents.
Each variant declares its name via `#[name = "..."]`:

```rust
use fern_ui::IntentKind;

#[derive(Debug, IntentKind)]
enum AppIntent {
    // Unit variants — no payload fields:
    #[name = "app.save"]       Save,
    #[name = "app.quit"]       Quit,

    // Tuple variants — whole variant is the payload:
    #[name = "app.open"]       Open(String),
    #[name = "app.scroll_by"]  ScrollBy(i32),

    // Struct variants work identically:
    #[name = "app.goto_line"]  GoToLine { line: u32 },

    // Complex payloads are fine too:
    #[name = "app.add_item"]   AddItem { id: i64, dto: CreateItemDto },
}
```

What the derive generates (verbatim):

```rust
impl IntentKind for AppIntent {
    fn into_intent(self) -> Intent {
        let name: &'static str = match &self {
            Self::Save            => "app.save",
            Self::Open(..)        => "app.open",
            Self::GoToLine { .. } => "app.goto_line",
            // ...
        };
        Intent::with_payload(name, self)
    }

    fn from_intent(intent: &Intent) -> Option<&Self> {
        intent.payload::<Self>()
    }
}
```

A blanket `impl<K: IntentKind> From<K> for Intent` lets most call sites
skip the explicit `.into_intent()`:

```rust
ctx.send_intent(AppIntent::Save);                    // unit
ctx.send_intent(AppIntent::Open(path));              // tuple
ctx.send_intent(AppIntent::GoToLine { line: 42 });   // struct
```

### Why the derive is dumb on purpose

The macro never inspects fields. Any variant shape works — unit,
tuple, struct, arbitrary user types — because the whole variant is
stored as the payload. The only requirement: the enum itself is
`'static` (typically trivially true).

Trade-off this codifies: FernUI sits between Flutter's fully-typed
Intents (no strings anywhere) and Qt's fully-string-keyed QAction
(easy to typo). Names are the dispatch key; `IntentKind` layers
compile-time checking on top when the app opts in. Third-party widgets
can still declare intents without knowing the consuming app's enum.

---

## `Action`

Widget-owned handler for one intent name:

```rust
use fern_ui::core::{Action, IntentResponse};

ctx.register_action(
    Action::new("app.save")
        .on_invoke(|_intent, _ctx| {
            println!("saved");
        }),
);
```

Key bits:

- **One action per intent name per widget.** Register multiple for
  different names on the same widget if needed; at a given level, if
  two actions match the same name, the first (by declaration order)
  wins.
- **`intent: &'static str`** — the dispatch key. Must exactly match
  `Intent::name`. Typo-safety comes from `IntentKind`'s name attributes,
  not from the action side.
- **`enabled_when: Option<Signal<bool>>`** — reactive predicate. When
  `false`, the action is skipped during dispatch (the intent
  propagates past this level as if no match existed here — unless the
  firing shortcut has `propagate_when_disabled == false`, in which case
  it is consumed dormant).
- **`on_invoke(|intent, ctx| …)`** — handler that always reports
  `Handled`.
- **`on_invoke_with_response(|intent, ctx| …) -> IntentResponse`** — when
  the handler needs to decide `Handled` vs `Propagated` at runtime.

### Handler patterns: extract only when needed

The framework already name-matches before invoking a handler — an
action's invocation is proof of `intent.name == action.intent`. You
only call `from_intent` when you need the *typed fields*.

```rust
// Unit intent — no fields to extract, react by name alone.
// This also means the handler fires whether the intent came from
// a shortcut (name-only) or from `send_intent(AppIntent::Save)`.
Action::new("app.save").on_invoke(|_intent, _ctx| {
    println!("[action] Save");
});

// Data-bearing intent — extract the typed variant:
Action::new("app.open").on_invoke(|intent, _ctx| {
    if let Some(AppIntent::Open(path)) = AppIntent::from_intent(intent) {
        open_file(path);
    }
});
```

---

## Dispatch walk

From
[`widget_tree::dispatch_intent`](../crates/fern-core/src/widget_tree.rs):

1. Build the chain `source → parent → … → root`.
2. For each `id` in the chain:
   - Skip if the node is inactive or disabled.
   - Find the first action on that node whose `intent == intent.name`.
     If none, continue to the parent.
   - If the action is disabled: restore it and either `continue` (when
     `propagate_when_disabled`) or `return` (otherwise).
   - Invoke the handler. On `Handled` → return. On `Propagated` →
     continue.

Handlers may call `ctx.send_intent(...)` from inside; those intents
queue and drain after the current one, until the queue empties. FIFO
ordering.

### Source anchoring

- **Shortcut path**: anchor is the focused widget for scoped shortcuts.
  Global shortcuts use the focused widget when present, otherwise fall
  back to the first arena root — so global shortcuts fire even before
  anything has been focused or after the focused widget is destroyed
  by a rebuild.
- **`ctx.send_intent(...)`**: anchor is the widget whose handler ran.
  Default `propagate_when_disabled = true` — programmatic sends have
  no shortcut to consult and take the least-surprising path.
- **`tree.dispatch_intent(source, intent, propagate)`**: caller chooses.

### Focus invalidation on destroy

`WidgetTree::destroy_subtree` clears `self.focused` and `self.hovered`
when they point at the widget about to be destroyed. Without this, a
rebuild of a currently-focused subtree (classic scenario: hitting
Rebind and editing a chord) would leave focus pointing at a dead id,
making subsequent global shortcuts look dead until the user clicked
elsewhere.

### Interaction with `on_key_preview`

A KeyDown event flows through three stages, in this order:

1. **Shortcut resolution.** `ShortcutRegistry::resolve` is consulted
   *before* any widget dispatch. If the chord matches an enabled
   shortcut whose scope contains the focused widget, the registry
   activates the shortcut's intent and returns — the key event is
   consumed.
2. **Ancestor key preview.** If no shortcut matched, the framework
   walks the focused widget's strict ancestors root → parent-of-target,
   firing `on_key_preview` on each. Returning `EventResponse::Handled`
   consumes the event.
3. **Focused widget bubble.** If preview returned `Ignored` for every
   ancestor, the focused widget's own `on_key` runs, then the event
   bubbles to ancestors via their `on_key` slots.

**Implication: shortcuts always win over `on_key_preview`.** An
ancestor that wants to override a registered shortcut should *also*
register a shortcut (with `enabled_when` gating which one fires when
both are eligible) — `on_key_preview` cannot stop a shortcut because
shortcuts are resolved first. Use `on_key_preview` for chords *not*
in the registry: a messenger composer claiming Enter that nobody
registered as a shortcut, a list view consuming arrow keys that no
ancestor declared.

---

## End-to-end skeleton

```rust
use fern_ui::IntentKind;
use fern_ui::core::{Action, Intent};
use fern_ui::core::shortcut::{KeyStroke, Shortcut};
use fern_ui::prelude::*;

#[derive(Debug, IntentKind)]
enum AppIntent {
    #[name = "app.save"]      Save,
    #[name = "app.open"]      Open(String),
    #[name = "app.scroll_by"] ScrollBy(i32),
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // --- Shortcuts ---
        ctx.register_shortcut_global(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        ctx.register_shortcut_global(
            Shortcut::new("app.scroll_by")
                .name("Scroll by page")
                .primary(KeyStroke::new(Key::PageUp, Modifiers::NONE))
                .secondary(KeyStroke::new(Key::PageDown, Modifiers::NONE))
                // Parametric: chord drives the payload.
                .on_activate(|ks, _ctx| {
                    let delta = if ks.key == Key::PageUp { -1 } else { 1 };
                    AppIntent::ScrollBy(delta)
                })
                .build(),
        );

        // --- Actions ---
        ctx.register_action(
            Action::new("app.save")
                .on_invoke(|_intent, _ctx| println!("saved")),
        );
        ctx.register_action(Action::new("app.open").on_invoke(|intent, _ctx| {
            if let Some(AppIntent::Open(path)) = AppIntent::from_intent(intent) {
                open_file(path);
            }
        }));
        ctx.register_action(Action::new("app.scroll_by").on_invoke(|intent, _ctx| {
            if let Some(AppIntent::ScrollBy(delta)) = AppIntent::from_intent(intent) {
                scroll(*delta);
            }
        }));

        // --- UI — menus, buttons, tooltips all reference shortcuts
        //     by id. Labels refresh when the user rebinds because the
        //     widgets observe `shortcut_registry.version()`.
        let menu = MenuBar::new().menu_literal("File", || {
            Box::new(MenuList::new().item(
                MenuItem::new_literal("Save")
                    .for_shortcut("app.save")
                    .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save)),
            ))
        });

        let save_button = Button::new_literal("Save")
            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save));

        let root = ctx.add(VStack::new().child(menu).child(save_button));
        self.root_child_id = Some(root);
        vec![root]
    }

    // layout_response delegates to root child…
}
```

---

## Cheat sheet

| Task                                       | API                                                                  |
|--------------------------------------------|----------------------------------------------------------------------|
| Declare a keyboard shortcut                | `Shortcut::new("id").primary(KeyStroke::…).build()`                  |
| Register widget-scoped                     | `ctx.register_shortcut(shortcut)`                                    |
| Register app-level                         | `ctx.register_shortcut_global(shortcut)`                             |
| Parametric payload                         | `.on_activate(\|ks, ctx\| AppIntent::X(…))`                          |
| Disable reactively                         | `.enabled_when(signal)`                                              |
| Composite predicate (AND/OR/NOT)           | `a.and(&b.not())`, `a.or(&b)`, `s.not()` on `Signal<bool>`           |
| Tuple multi-source signal                  | `a.zip(&b)`, `a.zip3(&b, &c)`                                        |
| Consume when disabled                      | `.propagate_when_disabled(false)`                                    |
| Declare a handler                          | `Action::new("id").on_invoke(\|intent, ctx\| …)`                     |
| Propagate after observing                  | `.on_invoke_with_response(\|i, c\| IntentResponse::Propagated)`      |
| Register handler on widget                 | `ctx.register_action(action)`                                        |
| Fire programmatically                      | `ctx.send_intent(AppIntent::X)`                                      |
| Typed enum bridge                          | `#[derive(IntentKind)]` + `#[name = "…"]` on each variant            |
| Recover typed variant                      | `AppIntent::from_intent(intent)`                                     |
| Raw payload lookup                         | `intent.payload::<T>()`                                              |
| Observe registry changes                   | `ctx.shortcut_registry().version()` — `Signal<u64>`                  |
| Effective view of a shortcut               | `ctx.effective_shortcut("id")` — merged defaults + overrides         |
| Menu label follows rebinds                 | `MenuItem::new(...).for_shortcut("id")`                              |
| Tooltip shows chord + rebinds live         | `TooltipContent::new(...).for_shortcut("id")`                        |
| Rebind UI out of the box                   | `ShortcutSettings::new()`                                            |
| One-shot key capture                       | `ctx.begin_key_capture(\|ks, registry, ctx\| …)` — returns `CaptureHandle` |

---

## See also

- Working demo: [`examples/shortcuts_demo/src/main.rs`](../examples/shortcuts_demo/src/main.rs)
- Source: [`crates/fern-core/src/shortcut.rs`](../crates/fern-core/src/shortcut.rs),
  [`intent.rs`](../crates/fern-core/src/intent.rs),
  [`action.rs`](../crates/fern-core/src/action.rs)
- Derive macro: [`crates/fern-ui-macros/src/intent_kind.rs`](../crates/fern-ui-macros/src/intent_kind.rs)
- Pre-built settings widget: [`crates/fern-widgets/src/shortcut_settings.rs`](../crates/fern-widgets/src/shortcut_settings.rs)
- Architecture §11: keyboard & shortcut design rationale
