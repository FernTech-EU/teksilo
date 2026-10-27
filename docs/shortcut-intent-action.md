<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Shortcut / Intent / Action Reference

Teksilo's input-to-behavior pipeline has three first-class concepts:

- **[`Shortcut`](../crates/teksilo-core/src/shortcut.rs)**, a rebindable
  keyboard binding (`KeyStroke` → intent name). Owned by the
  [`ShortcutRegistry`](../crates/teksilo-core/src/shortcut.rs); user
  rebindings layer on top of widget-declared defaults.
- **[`Intent`](../crates/teksilo-core/src/intent.rs)**, a runtime
  "something wants to happen" message: a stable name plus an optional
  type-erased payload. Produced by shortcuts, by widgets via
  `ctx.send_intent(...)`, or programmatically.
- **[`Action`](../crates/teksilo-core/src/action.rs)**, a widget-owned
  handler bound to an intent name. When an intent dispatches, the
  framework walks **source-widget → root** and lets the first matching
  enabled action consume (or propagate) it. From an overlay's content
  (a menu, a popover) the walk goes on from the widget the overlay is
  attached to; an open modal overlay (a dialog in the window, the command
  palette) goes first, as a native modal window does, and what it leaves
  walks on to the page from the widget it was opened from. Keys stop at it.

Pipeline in one line: `KeyStroke → Shortcut → Intent → Action handler`.

Typed DTO bridge between an app's intent enum and the runtime
[`Intent`]: the [`IntentKind`](../crates/teksilo-core/src/intent.rs) trait,
usually derived with `#[derive(IntentKind)]` from `teksilo-macros`.

Full end-to-end example:
[`examples/shortcuts_demo`](../examples/shortcuts_demo/src/main.rs).

---

## Mental model: three paths, one dispatcher

Every intent hits the same dispatcher, actions don't care where the
intent came from. The three firing paths:

| Path                          | How the intent is built                                     | Anchor for source→root walk     |
|-------------------------------|-------------------------------------------------------------|---------------------------------|
| **Shortcut** (keyboard chord) | Registry invokes `on_activate` or synthesizes `Intent::new` | Focused widget or root fallback |
| **Widget handler** (`ctx.send_intent`) | Handler builds or returns an `Into<Intent>` value           | The originating widget; from an overlay, on through the widget it is attached to |
| **From focus** (`ctx.send_intent_from_focus`: a menu-bar row, the command palette) | As above                                   | The focused widget once the handler's overlays have closed, else the originating widget |
| **Native menu item** (macOS menu bar)  | The item's intent, by name                                  | Focused widget of the window with the OS focus, else that window's root; with no window focused, the window whose menu it is, or the application tree for the application menu (`on_activate` runs in the window whose menu it is, or in the application tree for the application menu) |
| **Programmatic** (tests, tools, app callbacks) | Build `Intent` by hand or via `IntentKind::into_intent`     | The app's root (`WidgetTree::run_with_event_context`) |

The **name** is the dispatch key; the **payload** (if any) is
downcastable data the handler extracts when it needs typed fields.

---

## `KeyStroke`

A single chord, one `Key` plus its `Modifiers`:

```rust
KeyStroke::new(Key::S, Modifiers::CTRL)
KeyStroke::ctrl(Key::S)                        // same thing
KeyStroke::ctrl_shift(Key::S)
KeyStroke::command(Key::S)                     // ⌘S on macOS, Ctrl+S elsewhere
KeyStroke::command_shift(Key::S)
KeyStroke::alt(Key::Enter)
KeyStroke::new(Key::PageUp, Modifiers::NONE)   // plain PageUp
```

`Display` renders "Ctrl+S" style text. Widgets displaying shortcuts to users
should call `teksilo_widgets::keystroke_format::format_keystroke()` which handles
platform-specific symbols (⌘ on macOS) and locale-aware modifier names via
`tr_widget!` (e.g., "Strg" in German). `Serialize`/`Deserialize` are derived
so user overrides can be persisted.

### `Ctrl` means ⌘ on macOS

Desktop platforms disagree about which key carries application accelerators, and
on macOS the disagreement is not cosmetic: Control there belongs to the text
system and to the secondary click, while ⌘ is what a user presses for Save or
Find. So a **declared** shortcut default written with `Ctrl` is read as *the
platform's primary accelerator* and resolves to ⌘ on macOS, the convention Qt
spells `Qt::CTRL`, and the one Teksilo's native menu bar has always applied to
its key equivalents. Write the chord once:

```rust
Shortcut::new("editor.find").primary(KeyStroke::ctrl(Key::F)).build()
// Ctrl+F on Windows and Linux, ⌘F on macOS, one declaration, no cfg branching.
```

`KeyStroke::command(Key::F)` is the same chord with the intent stated outright;
prefer it for chords built **outside** the registry, such as the labels a
context menu renders for itself. `Modifiers::COMMAND` and `Modifiers::command()`
are the raw forms, for widgets testing a live `Modifiers` value.

Three consequences worth knowing:

- **User overrides are literal.** A chord captured in a settings UI is taken
  exactly as pressed, so physical ⌃F stays bindable on macOS. Only *declared*
  defaults go through the convention.
- **A chord that names `Super` explicitly is left alone**, so `Ctrl+Super`
  survives as the genuine ⌃⌘ two-modifier chord.
- **Some chords really are Control everywhere.** Ctrl+Tab cycles tabs on macOS
  too (⌘⇥ belongs to the application switcher and never reaches an app), and the
  ⌘ form of Space, H or Q is taken by the system. Declare those with
  `ShortcutBuilder::literal_modifiers()` and no rewriting happens on any
  platform:

```rust
Shortcut::new("view.next_tab")
    .literal_modifiers()
    .primary(KeyStroke::ctrl(Key::Tab))
    .build()
```

### Caret motion is a separate question

Inside a text surface the accelerator is not the whole story. macOS lays the
caret motions out across *three* modifiers, `⌥←/→` for word, `⌘←/→` for the
line edge, `⌘↑/↓` for the document, where Windows and Linux use `Ctrl+←/→`
plus bare `Home`/`End`. No single "is the accelerator held?" flag can express
that, so `RichTextEditor`, `CodeEditor` and every field built on
`TextInputField` read their arrows through
[`common::text_nav`](../crates/teksilo-widgets/src/common/text_nav.rs) instead.
That is internal to the widgets; app-declared `Shortcut`s are unaffected.

---

## `Shortcut`

Declarative, rebindable record. Built with a fluent `ShortcutBuilder`:

```rust
use teksilo::core::shortcut::{KeyStroke, Shortcut, ShortcutScope};

Shortcut::new("app.save")                  // stable id (dispatch key)
    .name("Save")                          // menu/settings label
    .category("file")                      // grouping key (stable, orders groups)
    .category_label(tr!(menu_file()))      // what the group header says
    .primary(KeyStroke::ctrl(Key::S))      // default primary chord
    .secondary(KeyStroke::new(Key::F12, Modifiers::NONE))
    // .scope(ShortcutScope::Global)        // default
    // .scope_to(scope_root_id)             // widget-scoped variant
    // .enabled_when(has_selection_signal)  // reactive "live" predicate
    // .propagate_when_disabled(false)      // consume-when-disabled instead
    // .on_activate(|ks, ctx| AppIntent::ScrollBy(...))  // parametric
    .build();
```

Key fields ([source](../crates/teksilo-core/src/shortcut.rs)):

- **`id: &'static str`**, stable key used for persistence, menu
  lookups (`MenuItem::for_shortcut`), and dispatch. Dot-style
  convention: `"editor.format.bold"`. Doubles as the intent name when
  `.intent(...)` isn't set.
- **`primary` / `secondary`**, the two default chords. User overrides
  (loaded from disk or set through the settings UI) are applied per
  slot independently.
- **`scope: ShortcutScope`**, `Global` (fires regardless of focus) or
  `Scoped(WidgetId)` (fires only when focus is inside that subtree).
  An overlay's content is outside the scopes of the widget it is attached
  to, as a Qt popup is a window of its own and a GTK popover's entry hears
  the key first: a list's scoped Delete does not fire from the field of a
  rename popover attached to a row, which gets the key. The intents sent
  from that field still cross to the row and its ancestors.
  Widget-declared shortcuts default to scoped; app-level declarations
  use global.
- **`on_activate`**, optional closure invoked at activation time.
  Receives the matched `KeyStroke` (so you can branch on which chord
  fired) and an `EventContext` (for side effects). Returns anything
  `Into<Intent>`, typically an `IntentKind` variant. Omit when the
  shortcut only needs the name: the registry synthesizes
  `Intent::new(intent_name)` for you.
- **`enabled_when: Option<Prop<bool>>`**, reactive "is this
  shortcut live?" predicate (the builder takes any `impl Into<Prop<bool>>`,
  so a `Signal<bool>` or a plain `bool`). When `false`, the shortcut is treated as
  *if not registered*, the keystroke falls through to the focused
  widget's normal `on_key` dispatch. Compose composite predicates with
  the [`Signal<bool>` combinators](#composing-enabled_when-predicates)
  (`and` / `or` / `not`) or [`Signal::zip`](../crates/teksilo-core/src/signal.rs)
  for typed tuples.
- **`propagate_when_disabled: bool`**, controls what happens when the
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

// `focus && !readonly && in_editor`, each source registered independently
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

Two-layer store, both keyed by shortcut **id** (`&'static str`):

1. **Defaults**, records registered by widgets during `build()` or
   declared statically via [`Widget::declare_shortcuts`](#static-declaration-widgetdeclare_shortcuts).
   Re-registering the same id **upserts**: code-owned fields are
   refreshed, the user override is preserved. Id is the unique key,
   so two widgets declaring the same id share the entry, see
   [Same-id collisions](#same-id-collisions).
2. **Overrides**, user-supplied keystroke rebindings keyed by
   shortcut id, persisted across widget rebuilds (*graveyard*
   semantics, a widget that disappears and reappears keeps its
   customised bindings).

The merged view is [`EffectiveShortcut`](../crates/teksilo-core/src/shortcut.rs):
primary/secondary = user override if touched, else declared default.
Menus, tooltips, and dispatch consume this shape.

Every mutation bumps `ShortcutRegistry::version()`, a `Signal<u64>`.
Menus, tooltips, and settings widgets observe it and re-read through
`effective(id)` to refresh labels after rebinds.

### Registration from a widget

Inside `build()`:

```rust
// Widget-scoped (default: Scoped(self_id), fires only when focus is
// inside the widget's subtree):
ctx.register_shortcut(
    Shortcut::new("editor.format.bold")
        .name("Bold")
        .primary(KeyStroke::ctrl(Key::B))
        .build(),
);

// App-level (Global, fires regardless of focus):
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

### Static declaration: `Widget::declare_shortcuts`

`ctx.register_shortcut` runs from `build()`, so a chord only enters
the registry once its owning widget has actually been built. That's
fine for always-mounted widgets, `build()` runs immediately on
insert. It's **not** fine when the widget lives behind a lazy
boundary:

- A [`Switcher`](../crates/teksilo-widgets/src/primitives/switcher.rs)
  arm that hasn't been selected yet (lazy mount: the page widget
  stays Boxed until first selection).
- A subtree gated by a feature flag or a closed disclosure.
- Anything else that defers `build()`.

For those cases, the chord won't appear in `ShortcutSettings` (or any
other registry consumer) until the user happens to visit that
subtree at least once. A rebind UI whose contents depend on where
you've clicked is the wrong shape.

`Widget::declare_shortcuts(&self) -> Vec<Shortcut>` opts in to
**eager registration of metadata**, same id and keystrokes, no
handler:

```rust
impl Widget for SaveTools {
    fn declare_shortcuts(&self) -> Vec<Shortcut> {
        // Metadata only, no on_activate, no captured state.
        vec![
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        ]
    }

    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Install the handler. Same id, the registry upserts.
        let do_save = self.do_save.clone();
        ctx.register_shortcut(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .on_activate(move |_, _| {
                    (do_save)();
                    Intent::new("app.save")
                })
                .build(),
        );
        // ...
    }
}
```

The framework walks `declare_shortcuts` at three sites:

- **Insertion**, `tree.add(w)` / `ctx.add_child(parent, w)`, right
  after handler-set extraction, *before* `build()`.
- **Rebuild**, right after `unregister_all_for_owner` wipes the
  previous build's registrations, so declared metadata survives the
  rebuild cycle even if `build()` only conditionally re-registers.
- **`Switcher::build`**, for every still-`Pending` slot. The
  Switcher pre-registers each lazy page's declared shortcuts owned
  by itself, so the chord is visible from the moment the Switcher
  builds, *without* mounting the page. When the page is eventually
  mounted, the insertion walk re-registers the same id owned by the
  page widget; the registry's idempotent upsert transfers ownership
  cleanly and preserves any user override.

**When you need it.** Any widget that might live behind a lazy
boundary, or any widget whose chord *must* appear in a rebind UI on
the first frame regardless of which views the user has visited.

**When you don't.** Always-mounted widgets (app root, top-level
toolbar, modeless docked panels). Build-time `register_shortcut`
already runs immediately on insert, same visibility, no
duplication.

**Pairing convention.** When you opt in, mirror the metadata in
both methods (same id, name, default keystrokes). `declare_shortcuts`
omits `on_activate`; `register_shortcut` adds it. The registry's
upsert refreshes the entry with the handler-bearing version when
`build()` runs.

The default impl is empty (`fn declare_shortcuts(&self) -> Vec<Shortcut> { Vec::new() }`),
so existing widgets keep working unchanged. This is strictly
opt-in.

### Same-id collisions

The registry is keyed by **id**, not by `(id, owner)`. Two widgets
registering the same id is not an error, it's intentional aliasing.
Concrete behaviour:

- `defaults: HashMap<&'static str, Shortcut>`, the second
  registration **replaces** the first (last-write-wins). Metadata,
  default keystrokes, and handler from the loser are discarded.
- `overrides: HashMap<String, KeyStrokeOverride>`, one override per
  id. A user rebind of `"app.save"` applies to whichever shortcut
  is currently in `defaults`. Two widgets sharing an id share the
  user rebind.
- `by_owner` tracks one owner per id at a time. The newer
  registration's `register_owned` calls `detach_owner_index` to pull
  the id off the previous owner's cleanup list, then the new owner
  inherits it. Destroying the *previous* owner doesn't touch the
  entry; destroying the *current* owner removes it (and any
  remaining declaration would have to re-register to refill).

**Use this intentionally.** If two widgets implement the same
logical action (`"app.save"` from a toolbar button, a menu item,
and a keyboard chord all targeting the same code), declaring the
same id is correct, the user rebinds once, all three follow.

**The footgun.** Two *unrelated* widgets accidentally picking the
same id. Rebinding one silently rebinds the other. Hierarchical
dotted ids prevent this in practice (`editor.format.bold`, not just
`bold`); there's no namespacing enforcement at the type level.
Framework-internal chords use a `__` prefix by convention
(`__teksilo_inspector.pick`) so app ids can't collide with them.

### Same-chord precedence

Distinct ids may bind the **same chord**, a normal IDE pattern (a
global `Ctrl+W` "close window" alongside a panel-scoped `Ctrl+W` "close
tab"). When a chord matches more than one enabled shortcut, the
dispatcher resolves them by **focus and scope specificity**, not by id
order:

1. **Applicability first.** A `Scoped(id)` binding is a candidate only
   when the focused widget is inside `id`'s subtree, by its own parents:
   an overlay's content is not inside the widget it is attached to. A
   `Global` binding is a candidate wherever focus is, except in a modal
   overlay (a dialog in the window, the command palette): there only a
   global binding registered by a widget inside that modal is, or one
   registered with no owner (the standard edit shortcuts, a window's copies
   of the application menu's shortcuts), as in a native
   modal window's tree (see "Scoped vs global actions"). With nothing
   focused while a modal is open, the key is judged as if focus were in the
   topmost modal.
2. **Most-specific scope wins.** An applicable `Scoped` binding beats a
   `Global` one. Among nested applicable scopes, the one closest to the
   focused widget (deepest) wins.
3. **Id order is only a tiebreak** within equal specificity (the
   deterministic `(category, id)` order of `iter_effective`).

So with focus in the editor, the editor-scoped `Ctrl+W` fires; move
focus to the sidebar and the global `Ctrl+W` fires instead. An
inapplicable scoped binding never "eats" the chord from an applicable
global one, and a global binding never shadows an in-focus scoped one.

Scope applicability needs the widget tree (descendant checks), which the
registry can't see, so it hands back every candidate via
`matches_by_keystroke` and the dispatcher does the focus-aware
selection. (`find_by_keystroke`, which returns just the first by id
order, ignores scope and is for non-dispatch queries only.)

This is *cross-id* collision resolution by focus; it is distinct from
the rebind-time conflict check ([`find_conflict`](#per-slot-overrides),
used by the settings UI to auto-unbind), and from the *same-id*
aliasing above.

### Per-slot overrides

User overrides are per-slot (`SlotOverride::{Default, Bound(ks),
Unbound}`):

- `Default`, delegate to whatever default the shortcut currently
  declares (a later code-side change flows through).
- `Bound(ks)`, lock the slot to this chord.
- `Unbound`, lock the slot to *no* chord.

Rebinding primary does not disturb secondary, and vice versa. The
registry's `rebind_primary` / `rebind_secondary` only touch the
targeted slot, they do **not** auto-unbind conflicting shortcuts.
Use `ShortcutRegistry::find_conflict(keystroke, excluding_id)` before
rebinding if you want the "exactly one effective binding per chord"
invariant; that is what the pre-built
[`ShortcutSettings`](../crates/teksilo-widgets/src/shortcut_settings.rs)
widget does when it records a chord. By default it asks first: the
row shows "Ctrl+S is already used by Save" with Reassign and Cancel,
and nothing changes until the user answers.

---

## `CaptureHandle`: one-shot key capture

Used to implement "press a chord" rebind UIs. `ctx.begin_key_capture`
returns a [`CaptureHandle`](../crates/teksilo-core/src/shortcut.rs): the
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
so dropping the old handle cancels only the old slot, no race with
the newer capture. The pre-built
[`ShortcutSettings`](../crates/teksilo-widgets/src/shortcut_settings.rs)
widget packages this flow: each chord is a control that records a new
chord when activated, refuses keys that cannot be shortcuts (a bare
letter, Shift plus a letter, reserved chords such as Ctrl+Tab), asks
before taking a chord from another command, and resets a changed row. A
rebind of a shortcut the [application menu](native-menu.md#shortcuts-and-key-equivalents)
names applies to the whole application: `teksilo-app` takes it from the
window's registry to the application tree's, and every window follows.

---

## `Intent`

Runtime message, name + optional payload. Construction:

```rust
use teksilo::core::Intent;

// Name-only (parameter-less):
let i = Intent::new("app.save");

// Typed payload (any T: 'static, stored in an Rc<dyn Any>):
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

- **`Handled`** (default), stop walking; the intent is consumed here.
- **`Propagated`**, observe-and-keep-going; ancestor widgets also get
  a chance. Useful when a widget wants to react (update a draft
  indicator) but lets an ancestor perform the primary action.

`ActionBuilder::on_invoke` always reports `Handled`, use
`on_invoke_with_response` when you need to propagate.

---

## `IntentKind`: typed DTO bridge

Use `#[derive(IntentKind)]` on an enum that catalogs the app's intents.
Each variant declares its name via `#[name = "..."]`:

```rust
use teksilo::IntentKind;

#[derive(Debug, IntentKind)]
enum AppIntent {
    // Unit variants, no payload fields:
    #[name = "app.save"]       Save,
    #[name = "app.quit"]       Quit,

    // Tuple variants, whole variant is the payload:
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

The macro never inspects fields. Any variant shape works, unit,
tuple, struct, arbitrary user types, because the whole variant is
stored as the payload. The only requirement: the enum itself is
`'static` (typically trivially true).

Trade-off this codifies: Teksilo sits between Flutter's fully-typed
Intents (no strings anywhere) and Qt's string-keyed `QAction`.
Names are the dispatch key; `IntentKind` layers compile-time checking
on top when the app opts in. Third-party widgets can still declare
intents without knowing the consuming app's enum.

---

## `Action`

Widget-owned handler for one intent name:

```rust
use teksilo::core::{Action, IntentResponse};

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
- **`intent: &'static str`**, the dispatch key. Must exactly match
  `Intent::name`. Typo-safety comes from `IntentKind`'s name attributes,
  not from the action side.
- **`enabled_when: Option<Prop<bool>>`**, reactive predicate (builder
  takes `impl Into<Prop<bool>>`). When
  `false`, the action is skipped during dispatch (the intent
  propagates past this level as if no match existed here, unless the
  firing shortcut has `propagate_when_disabled == false`, in which case
  it is consumed dormant).
- **`on_invoke(|intent, ctx| …)`**, handler that always reports
  `Handled`.
- **`on_invoke_with_response(|intent, ctx| …) -> IntentResponse`**, when
  the handler needs to decide `Handled` vs `Propagated` at runtime.

### Scoped vs global actions

`ctx.register_action(action)` attaches the action to the **registering widget's
node**, it only fires when that widget is on the intent's source→root walk.
That's right for actions co-located with their UI (a panel handling a command
fired from within itself), and for app-wide commands registered on the window's
root, which every walk in the window ends at: an overlay's walk goes on from
the widget the overlay is attached to, so a menu or a popover reaches the root
too. A **modal** overlay (a dialog presented in the window, the command palette)
goes first while it is open, as the native modal window macOS and Windows
present a dialog in does: an intent sent inside it reaches the actions on its
walk inside the modal, then the window-global actions registered by widgets
inside the modal. What they leave walks on to the page, from the widget the
modal was opened from (when that one is gone, such as a row of the closed menu
that opened the dialog, from the widget focus goes back to when the modal
closes; else from the window's root), then reaches the page's other
window-global actions, and what the window leaves goes to the application
tree. A dialog opened over another dialog leaves its intents to that one
first. A native dialog window's leftovers go the same way, to the window it
blocks (see "Modal dialogs" in the [multi-window guide](multi-window.md)).

Keys stop at the modal. With focus in it, only the global shortcuts registered
by widgets inside the modal apply, and the page's scoped shortcuts do not fire
either; with nothing focused while a modal is up, a key is judged as if focus
were in the topmost modal. A shortcut's intent reaches nothing behind the
modal: it is offered the actions inside it and the global actions registered
inside it, and what they leave goes to the application tree, not to the
page. So the page's Ctrl+N bound to an application-tree action does
not fire inside an in-window dialog, while a dialog button that sends
`app.new` reaches it. Two kinds of shortcut apply everywhere, in a dialog
too: the standard edit shortcuts the tree registers itself, and the unowned
copy a window holds of a shortcut registered in the application tree when a
row of the [application menu](native-menu.md) names it. A row chosen in the
macOS menu bar is input too: its intent stops at the modal as a shortcut's
does, and its row is greyed out when only the page would take it. The same
holds in a native dialog window: what it leaves of a shortcut's intent or a
menu bar row's goes to the application tree only, never to the windows the
dialog blocks.

An intent sent as the modal closes (a dialog's button that closes the dialog
and then asks the page for something, a command palette's row) is dispatched
once it has closed, so the modal's own actions no longer take it:
`ctx.send_intent(..)` walks on from the same widget an intent the open modal
leaves would, and `ctx.send_intent_from_focus(..)` starts at the focus the
modal gave back, or at the window's root when it gave none back. An intent
sent from outside the modal while it is up (only a programmatic send can be:
no input reaches the page) is the page's and reaches every window-global
action.

`ctx.register_action_global(action)` registers a **window-global** action
consulted as a dispatch **fallback**, *after* the source→root walk finds no
consuming node action, so it fires no matter where in the window the intent
originated: from inside a modal overlay the registering widget is not in, only
once the intent has walked on to the page, and never for a key pressed there
(above). Use it when the widget that owns the handler is not on the walk:

- A **global shortcut** with no widget focused starts at the window's root; a
  scoped action deep in the tree won't be on that walk.
- A command whose handler belongs to a widget off to the side, such as a
  popover's [`open_action`](../crates/teksilo-widgets/src/popover_widget.rs),
  fired from a menu row or a shortcut.

A **menu bar** needs neither. A `MenuModel` entry chosen in the window sends its
intent from the focused widget, as its shortcut does, once the menu has closed
and given focus back to the widget the user was working in (the bar's titles
take no focus from a click, and after F10 or Alt the bar hands focus back as a
command is chosen): an action on that widget, one of its ancestors or the
window's root takes it, and what the window leaves goes to the application
tree, where the [application menu](native-menu.md)'s commands live. Such a row
with no closure, and no state to write (a two-way check or radio row writes
one), is greyed out while choosing it would do nothing: nothing on that
route takes its intent, or the action the intent would stop at (the first
enabled one, past any that pass it on) has nothing to act on (the lookup a
dispatch makes, run without invoking anything). A hand-built
`MenuItem` row sends from focus the same way with
`ctx.send_intent_from_focus(...)` in its `on_activate_fn`, but does not grey
out by itself (give it `.enabled(..)` for that); sent with `ctx.send_intent(...)`, its walk goes from the row to
the bar's trigger and up the bar's ancestors, which reaches the window's root but
not a sibling of the bar, such as an editor.

`register_action_global` is the action-side counterpart to
`register_shortcut_global`. Ownership applies: the action is torn down when the
registering widget rebuilds or is destroyed. Multiple globals for the same intent
name fire in registration order, honouring `IntentResponse` (`Handled` stops,
`Propagated` continues to the next global).

```rust
// App root: command reachable from the menu bar, a shortcut, and content alike.
ctx.register_shortcut_global(
    Shortcut::new("view.toggle_sidebar").primary(KeyStroke::ctrl(Key::B)).build(),
);
ctx.register_action_global(
    Action::new("view.toggle_sidebar").on_invoke(|_i, _c| sidebar.toggle()),
);
```

### Handler patterns: extract only when needed

The framework already name-matches before invoking a handler, an
action's invocation is proof of `intent.name == action.intent`. You
only call `from_intent` when you need the *typed fields*.

```rust
// Unit intent, no fields to extract, react by name alone.
// This also means the handler fires whether the intent came from
// a shortcut (name-only) or from `send_intent(AppIntent::Save)`.
Action::new("app.save").on_invoke(|_intent, _ctx| {
    println!("[action] Save");
});

// Data-bearing intent, extract the typed variant:
Action::new("app.open").on_invoke(|intent, _ctx| {
    if let Some(AppIntent::Open(path)) = AppIntent::from_intent(intent) {
        open_file(path);
    }
});
```

---

## Dispatch walk

From
[`widget_tree::dispatch_intent`](../crates/teksilo-core/src/widget_tree.rs):

1. Build the chain `source → parent → … → root`. When it reaches the
   content root of an open overlay, which has no parent, it continues
   from the widget the overlay is attached to (its anchor), and through
   every overlay that widget is in (a submenu is attached to a row of its
   parent menu). The anchor is the link, not the widget focus returns to
   when the overlay closes: that one can be unrelated to the popup (a
   context menu opened on a list row while a search field had focus).
   An intent sent by a handler that also closes its overlay still
   crosses it: the crossing is noted when the intent is sent. An open
   modal overlay ends this walk at its content. Unless the intent is a
   shortcut's or a native menu bar row's (both input, as a key is, and
   kept in the modal), a further walk follows, once the modal's own
   actions and the global actions registered inside it (step 3) have
   passed it by: from the
   widget the modal is attached to, or, when that one is gone, the widget
   focus goes back to when it closes, or else the window's root. A modal
   that has closed by the time the intent is dispatched is crossed to the
   same widget like any other overlay.
2. For each `id` in the chain:
   - Skip if the node is inactive or disabled.
   - Find the first action on that node whose `intent == intent.name`.
     If none, continue to the parent.
   - If the action is disabled: restore it and either `continue` (when
     `propagate_when_disabled`) or `return` (otherwise).
   - Invoke the handler. On `Handled` → return. On `Propagated` →
     continue.
3. **Global fallback.** If the chain walk consumed nothing, consult the
   window-global actions (registered via `register_action_global`) in
   registration order. First enabled match handles it (`Handled` → stop,
   `Propagated` → next global). This is position-independent, so it catches
   intents whose walk never passes the widget that registered the action
   (a popover's open action, fired from a menu row or a shortcut). After a
   walk that ends at an open modal overlay, only the global actions
   registered inside that modal are consulted, before the next walk; the
   others come after the last walk, unless it ends at a modal (a
   shortcut's intent, which stays in the modal).

Handlers may call `ctx.send_intent(...)` from inside; those intents
queue and drain after the current one, until the queue empties. FIFO
ordering.

### Source anchoring

- **Shortcut path**: anchor is the focused widget for scoped shortcuts.
  Global shortcuts use the focused widget when present, otherwise the
  content of the topmost open modal overlay, otherwise the window's root
  (the app's own, inside any wrapper a post-root hook put around it), so
  global shortcuts fire even before anything has been focused or after the
  focused widget is destroyed by a rebuild.
- **`ctx.send_intent(...)`**: anchor is the widget whose handler ran.
  Default `propagate_when_disabled = true`, programmatic sends have
  no shortcut to consult and take the least-surprising path.
  When the handler runs in an **overlay** (menu dropdown, popover),
  the walk goes from the overlay's content on to the widget the overlay is
  attached to and its ancestors, so a context menu's rows reach the actions
  of the widget that was right-clicked, and a popover's buttons those of the
  button that opened it. A modal overlay (a dialog in the window) goes
  first while it is open: the walk reaches its content, then the global
  actions registered inside it, and only then walks on from the widget the
  modal was opened from; one the handler closed is crossed like the
  others.
- **`ctx.send_intent_from_focus(...)`**: anchor is the widget that holds
  focus once everything the handler did has been applied, every overlay it
  closed included, so a row of a closing menu sends from the widget the menu
  gave focus back to. A `MenuModel` entry and the command palette send their
  commands this way. With nothing focused while a modal overlay is up, it
  starts in the topmost modal, as if focus were there (unless the handler
  is in a modal itself). Otherwise, with nothing focused, it falls back to
  the handler's widget, crossing the overlays it was in as they were when
  it ran; when the handler closed the modal it was in, it starts at the
  window's root instead, as an intent with no source widget does.
- **Outside any handler** (a test, an app-layer hook): `WidgetTree::dispatch_intent`
  is crate-internal; call `ctx.send_intent(...)` inside
  `tree.run_with_event_context(ops, |ctx| …)`, which anchors at the app's
  root: the widget the window's root builder made, inside any wrapper a
  post-root hook (the debug inspector, the toast host) put around it. A
  global shortcut with nothing focused starts there too.

### Focus invalidation on destroy

`WidgetTree::destroy_subtree` clears the focused and hovered widget
when they point at the widget about to be destroyed. Without this, a
rebuild of a currently-focused subtree (classic scenario: hitting
Rebind and editing a chord) would leave focus pointing at a dead id,
making subsequent global shortcuts look dead until the user clicked
elsewhere.

### Interaction with `on_key_preview`

A KeyDown event flows through three stages, in this order:

1. **Shortcut resolution.** The registry is consulted *before* any
   widget dispatch. `ShortcutRegistry::matches_by_keystroke` yields
   **every** enabled shortcut bound to the chord; the dispatcher then
   picks the one whose scope applies to the current focus (see
   [Same-chord precedence](#same-chord-precedence) above). If an
   applicable shortcut is found, its intent is dispatched, and the key
   event is consumed **if an action handled the intent**. When nothing
   in the window did (no action matched, or every one returned
   `Propagated`), the intent goes to the
   [application tree](multi-window.md#the-application-tree) if an
   enabled action there takes it, and the key is consumed; otherwise the
   event falls through to stage 2 as if no shortcut had matched. So
   does an event for which no candidate applies (every match is a
   `Scoped` binding outside the focused subtree). The application
   tree's answer is what it registered when it last ran; an action there
   that returns `Propagated` when it runs still uses the key. The action
   learns which window the key was pressed in from
   `intent.origin_window()`.
2. **Ancestor key preview.** If no shortcut matched, the framework
   walks the focused widget's strict ancestors root → parent-of-target,
   firing `on_key_preview` on each. Returning `EventResponse::Handled`
   consumes the event.
3. **Focused widget bubble.** If preview returned `Ignored` for every
   ancestor, the focused widget's own `on_key` runs, then the event
   bubbles to ancestors via their `on_key` slots.

Two things run ahead of stage 1: an armed `begin_key_capture` takes the
KeyDown outright, and a focused surface built with
`.keyboard_capture(true)` (a terminal) skips shortcut resolution entirely,
so a host `Ctrl+C` shortcut cannot steal the chord it forwards to its child.

**Implication: shortcuts always win over `on_key_preview`.** An
ancestor that wants to override a registered shortcut should *also*
register a shortcut (with `enabled_when` gating which one fires when
both are eligible), `on_key_preview` cannot stop a shortcut because
shortcuts are resolved first. Use `on_key_preview` for chords *not*
in the registry: a messenger composer claiming Enter that nobody
registered as a shortcut, a list view consuming arrow keys that no
ancestor declared.

A composer built on `RichTextEditor` claims plain Enter (send) and leaves
Shift+Enter to the editor, which breaks the line inside the paragraph, as
chat composers do:

```rust
let composer = VStack::new()
    .child(RichTextEditor::editor(doc).min_lines(1).max_lines(6))
    .on_key_preview(move |event, ctx| match event {
        WidgetEvent::KeyDown { key: Key::Enter, modifiers, .. }
            if *modifiers == Modifiers::NONE =>
        {
            ctx.send_intent(ChatIntent::Send);
            EventResponse::Handled
        }
        _ => EventResponse::Ignored,
    });
```

To have Shift+Enter start a new paragraph instead, claim it in the same
`on_key_preview` and call `EditorHandle::insert_block()`. Shift+Enter is a
key of the editor, not a shortcut, for the same reason: a shortcut would
resolve before the focused widget and take Shift+Enter from every text
field in the window.

### Standard edit shortcuts: cut, copy, paste, undo, redo, select all

Every widget tree registers seven global shortcuts at creation, one per
[`StandardEdit`](../crates/teksilo-core/src/shortcut/standard.rs) command.
The id is also the intent name:

| Command | Id | Default chord |
|---|---|---|
| Undo | `edit.undo` | `Ctrl+Z` |
| Redo | `edit.redo` | `Ctrl+Shift+Z` and `Ctrl+Y` (`Ctrl+Y` first on Windows) |
| Cut | `edit.cut` | `Ctrl+X` |
| Copy | `edit.copy` | `Ctrl+C` |
| Paste | `edit.paste` | `Ctrl+V` |
| Paste Unformatted | `edit.paste_plain` | `Ctrl+Shift+V` |
| Select All | `edit.select_all` | `Ctrl+A` |

`Ctrl` is ⌘ on macOS, as for every declared chord. They sit in the `edit`
category, so `ShortcutSettings` lists them under Edit, in the user's
language (`teksilo-app` names them in every window it opens), and a user
can rebind them or give them a second chord like any other shortcut.

The text widgets (`TextInputField` and everything built on it,
`RichTextEditor`, `CodeEditor`, `LogView`) and the select-all of the data
views (`ListView`, `TreeView`, `TableView`, `TreeTableView`, `GridView`)
do not match these chords in `on_key`. Each registers an action per command
on its own node, so the focused widget, the start of the walk, runs the
command. A widget that declines one (Cut in a read-only field, a command
its filter refuses, select-all in a single-selection list) returns
`Propagated`: the intent goes on up the walk, and if nothing takes it the
key reaches the focused widget as a plain `KeyDown`.

The text widgets also say when a command has nothing to act on, which is
what greys out an Edit menu row for it (in a window's `MenuBar` and in the
macOS menu bar): Copy with nothing selected, Cut also in a read-only widget,
Paste in a read-only one, Select All in an empty one, Undo and Redo with no
history that way. A code editor and a log view copy the caret's line, so
their Copy and Cut need text, not a selection. The row asks the action its
command would stop at, the first on the route that does not pass the
command on: a field with nothing selected greys Copy even when the page
above it has a Copy of its own, because Copy chosen there stops at the
field and the page's never runs; a read-only field passes Cut on, so the
Cut row is enabled exactly when the action it passes Cut to would act. The
same holds for Undo with `set_empty_history_passes_on`: on, an empty
history leaves the row to the application's Undo; off, the widget keeps
Undo and the row greys. This changes no dispatch: a shortcut whose command
has nothing to act on still reaches the focused widget's action, which
handles or declines it as above.

A widget of your own that copies, pastes or selects all should do the
same: register an `Action` for the `edit.*` id rather than match the
chord. A widget that still matches `Ctrl+C` keeps copying on it after the
user has moved Copy elsewhere.

An action of your own says the same two things on its builder.
`ActionBuilder::available_when(|| ..)` answers whether it has anything to
act on now; `ActionBuilder::passes_on_when(|| ..)` answers whether its
handler (built with `on_invoke_with_response`) would pass the intent on.
Both run on every query, outside any event handler, so they read state the
action already holds and must be cheap; dispatch asks neither. A control
other than a menu row greys itself from the same answer with
`BuildContext::command_available("edit.copy")`, a `Signal<bool>` the tree
keeps current from the window's focus:

```rust,ignore
let selection = self.selection.clone();
ctx.register_action(
    Action::new("doc.copy")
        .available_when(move || !selection.get().is_empty())
        .on_invoke(|_, ctx| copy_selection(ctx)),
);
let can_copy = ctx.command_available("doc.copy");
ctx.add(IconButton::new(icon).enabled(can_copy));
```

### Editor shortcuts: format and code commands

The editors' own commands work the same way, with one difference: they
exist only while an editor that runs them does.

| Command | Id | Default chord | Declared by |
|---|---|---|---|
| Bold | `format.bold` | `Ctrl+B` | `RichTextEditor` |
| Italic | `format.italic` | `Ctrl+I` | `RichTextEditor` |
| Underline | `format.underline` | `Ctrl+U` | `RichTextEditor` |
| Insert Paragraph | `format.insert_paragraph` | `Ctrl+Enter` | `RichTextEditor` |
| Toggle Comment | `code.toggle_comment` | `Ctrl+/` and `Ctrl+Shift+/` | `CodeEditor` |
| Duplicate | `code.duplicate` | `Ctrl+D` | `CodeEditor` |

The ids are on
[`FormatShortcut` and `CodeShortcut`](../crates/teksilo-widgets/src/editor_shortcuts.rs).
`Ctrl+Shift+/` is there for the layouts that type `/` with Shift (AZERTY,
German): the chord arrives with Shift held. Each editable editor declares its
set with `BuildContext::register_shortcut_shared`, so `ShortcutSettings`
lists Bold under Format only in an application that has a rich-text editor,
and a read-only viewer declares nothing.

A shared registration is one global entry with several owners. The editors
cannot use `register_shortcut_global`: an owned id belongs to one widget and
goes with it, so destroying one editor would take Bold from every other.
`register_shortcut_shared` keeps the entry while any owner lives, the last one
to go removes it, and the user's binding waits in the registry for the next
editor, as every override does. Use it for any command several instances of a
widget run, each from an `Action` of its own.

### Routing an application's own undo

An application that wants **one** Undo command (one chord, one menu row)
handles `edit.undo`; it does not register `Ctrl+Z` itself, which would
conflict with the standard shortcut:

```rust
ctx.register_action_global(Action::new(StandardEdit::Undo.id()).on_invoke(|_, _| {
    app_undo();
}));
```

The order of the walk does the routing. With a text widget focused, its own
action is reached first and undoes its text; with focus anywhere else, the
walk ends at the application's action. A rename box in a table cell, a
search field, an input inside a dialog the application did not write: each
undoes itself, without the application knowing it exists.

A `Toolbar` command sent from focus (`ctx.send_intent_from_focus`) takes this
route too: the bar's buttons take no focus from a click, so the editor still
holds it, as it does under a menu bar's menu. A button of the application's own
beside the editor needs `.focus_on_click(false)` for the same: a click on a
button that takes focus moves it there, and the walk starts at the button.

One case still needs the application to ask which widget holds the caret:

- **A refused history.** An editor whose filter refuses Undo (a
  forward-only writing mode, a read-only review pass) declines it, and the
  intent then reaches the application's action. Routing it to the
  application's own history there would defeat the mode.

Every text widget calls
[`BuildContext::register_text_surface`](../crates/teksilo-core/src/build_context.rs),
so the tree answers it completely:

```rust
// Once, during build, the handle shares the tree's focus signal, so it
// stays live and can be read from a frame tick.
let surfaces = ctx.text_surfaces();

// In the global edit.undo action:
match surfaces.focused() {
    Some(surface) if surface.history_frozen() => {} // refused: do nothing
    Some(surface) => surface.undo(),                // drive the caret's own widget
    None => app_undo(),                             // no text surface: the app's own history
}
```

`TextSurfaces::focused()` yields an `Rc<dyn TextSurface>`, undo/redo,
`history_frozen`, selection, read-only, clipboard, select-all, for
whichever widget holds the focus, whatever kind it is. Registrations are
owned by the registering widget and torn down on its rebuild or destroy,
exactly like `register_action_global`.

`surface.accepts(command)` says whether that widget takes a command at all
in its current mode, by the rule its own keys follow: a forward-only editor
takes a paste and refuses a cut, a read-only one refuses both, a masked
password refuses a copy. Grey an Edit row with it, as
`!surface.accepts(StandardEdit::Cut) || !surface.has_selection()`. The
surface's own `cut`, `paste`, `undo` and the others do nothing for a command
it refuses, so a routed shortcut cannot go around the widget's mode.

A custom text widget should call `register_text_surface` too. Failing to
is not a compile error and will not be noticed until an application
routes one of these commands from a menu and your widget quietly loses it.

#### Falling back when the caret's history is empty

By default a focused text widget answers Undo and Redo even when it has
nothing to step through, so from the keyboard the application's action is
reached only where no text widget has focus. An application whose Undo
falls back to a wider history once the caret's own is exhausted (and says
so, in a menu row that names what it will undo) turns that on for the
whole tree:

```rust
// Once, in the root's build.
ctx.text_surfaces().set_empty_history_passes_on(true);
```

`RichTextEditor`, `TextInputField` and every field built on it, and
`CodeEditor` then decline Undo when their document has nothing to undo,
and Redo when it has nothing to redo, and the intent climbs to the
application's action as it does from a refused history. They ask the
document when the command runs, not the debounced `can_undo` signal, so a
character typed a moment earlier is still undone in the widget. Leave it
off unless the application announces the fallback: otherwise Ctrl+Z in an
empty search field undoes something the user is not looking at.

---

## End-to-end skeleton

```rust
use teksilo::IntentKind;
use teksilo::core::{Action, Intent};
use teksilo::core::shortcut::{KeyStroke, Shortcut};
use teksilo::prelude::*;

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
        // The File menu below renders in an overlay attached to its trigger
        // in the menu bar, so its row's walk reaches `Root` through the bar.
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

        // --- UI, menus, buttons, tooltips all reference shortcuts
        //     by id. Labels refresh when the user rebinds because the
        //     widgets observe `shortcut_registry.version()`.
        let menu = MenuBar::new().menu(lit!("File"), || {
            Box::new(MenuList::new().item(
                MenuItem::new(lit!("Save"))
                    .for_shortcut("app.save")
                    .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save)),
            ))
        });

        let save_button = Button::new(lit!("Save"))
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
| Declare metadata eagerly (lazy-safe)       | `fn declare_shortcuts(&self) -> Vec<Shortcut>` on the Widget impl    |
| Parametric payload                         | `.on_activate(\|ks, ctx\| AppIntent::X(…))`                          |
| Disable reactively                         | `.enabled_when(signal)`                                              |
| Composite predicate (AND/OR/NOT)           | `a.and(&b.not())`, `a.or(&b)`, `s.not()` on `Signal<bool>`           |
| Tuple multi-source signal                  | `a.zip(&b)`, `a.zip3(&b, &c)`                                        |
| Switch to a selected inner signal          | `selector.flat_map(\|t\| inner_signal(t))`                          |
| Consume when disabled                      | `.propagate_when_disabled(false)`                                    |
| Declare a handler                          | `Action::new("id").on_invoke(\|intent, ctx\| …)`                     |
| Propagate after observing                  | `.on_invoke_with_response(\|i, c\| IntentResponse::Propagated)`      |
| Register handler on widget                 | `ctx.register_action(action)`                                        |
| Register window-global handler (owner off the walk) | `ctx.register_action_global(action)`                        |
| Fire programmatically                      | `ctx.send_intent(AppIntent::X)`                                      |
| Fire from focus (menu row, palette)        | `ctx.send_intent_from_focus(AppIntent::X)`                           |
| Typed enum bridge                          | `#[derive(IntentKind)]` + `#[name = "…"]` on each variant            |
| Recover typed variant                      | `AppIntent::from_intent(intent)`                                     |
| Raw payload lookup                         | `intent.payload::<T>()`                                              |
| Observe registry changes                   | `ctx.shortcut_registry().version()`, `Signal<u64>`                  |
| Effective view of a shortcut               | `ctx.effective_shortcut("id")`, merged defaults + overrides         |
| Menu label follows rebinds                 | `MenuItem::new(...).for_shortcut("id")`                              |
| Tooltip shows chord + rebinds live         | `TooltipContent::new(...).for_shortcut("id")`                        |
| Rebind UI out of the box                   | `ShortcutSettings::new()`                                            |
| One-shot key capture                       | `ctx.begin_key_capture(\|ks, registry, ctx\| …)`, returns `CaptureHandle` |

---

## See also

- Working demo: [`examples/shortcuts_demo/src/main.rs`](../examples/shortcuts_demo/src/main.rs)
- Source: [`crates/teksilo-core/src/shortcut.rs`](../crates/teksilo-core/src/shortcut.rs),
  [`intent.rs`](../crates/teksilo-core/src/intent.rs),
  [`action.rs`](../crates/teksilo-core/src/action.rs)
- Derive macro: [`crates/teksilo-macros/src/intent_kind.rs`](../crates/teksilo-macros/src/intent_kind.rs)
- Pre-built settings widget: [`crates/teksilo-widgets/src/shortcut_settings.rs`](../crates/teksilo-widgets/src/shortcut_settings.rs)
- [Architecture §11](architecture.md#input-and-accessibility): one-paragraph summary pointing back here
