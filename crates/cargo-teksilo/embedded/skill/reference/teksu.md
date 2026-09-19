<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# The `teksu!` DSL — working reference

`teksu!` is a block-structured macro that desugars **one-to-one** to Teksilo V2
builder calls at macro-expansion time. There is no runtime and no virtual tree:
whatever a block expands to is exactly what you could have typed by hand.
Knowing the desugaring is what lets you translate in either direction with
confidence.

It is **optional**. It earns its keep on deep, nested trees where
`.child(...).child(...)` chains stop being scannable; a flat three-widget tree
reads just as well in plain builder form. The two nest in either direction, so
adopt it where it helps.

> Verified against **teksilo 0.12.1**. Routing rules and slot arities are
> version-sensitive — when a rule here disagrees with the compiler, the compiler
> is right, and `cargo teksilo symbol <Widget>` will tell you the real signature
> for the version your app pins.

## Where the deeper material is

Two framework guides carry the full surface language. They ship inside the
version-matched corpus, so they are reachable from your own app with no
checkout:

```bash
cargo teksilo search "teksu desugaring cheat sheet"   # the user-facing reference
cargo teksilo search "teksu language spec"            # the design spec, with worked translations
cargo teksilo search "teksu trybuild fixture"         # minimal self-contained examples, one per feature
```

The desugaring cheat sheet in the `teksu-macro-reference` guide is the single
source of truth for surface forms — quote from it when explaining a block rather
than paraphrasing from memory.

> **Labels in these examples.** For brevity some examples below pass bare string
> literals (`Button("Save")`). With the default `i18n` feature a widget label is a
> `LocalizedString` and there is no `From<&str>`, so in a real app wrap it:
> `Button(lit!("Save"))` (untranslated) or `Button(tr!(save()))` (translated).
> `teksu!` passes whatever is inside `(…)` verbatim, so the wrapping just goes
> inside the parens.

## Mental model

```rust,ignore
teksu!(ctx => VStack { /* … */ })   // inserts the root via ctx.add → returns WidgetId
teksu!(VStack { /* … */ })          // returns a widget value → pass to .child(...) / a slot
```

The `ctx =>` preamble names your `BuildContext` local; `=>` is literal.

## Quick triage

| User wants | What to do |
|---|---|
| Explain a block | Walk through it top-down, annotating each body item with its desugaring. Use the cheat-sheet expansions. |
| Write from scratch | Start from a builder-chain mental model, then rewrite as DSL. Keep closures and Rust expressions verbatim. |
| Convert builder → `teksu!` | Element-by-element. Flatten `.child(...)` chains into bare elements at body position. Move `.prop(v)` into `name: v` body items. Preserve explicit constructors (`Button::new_literal`, `Padding::uniform`, …). |
| Convert `teksu!` → builder | Mechanical: elements to `Type::new(args)`, properties to `.prop(args)`, bare children to `.child(...)`, bindings to hoisted `let name = ctx.add(...); …child(name)`. |
| Debug a compile error | First check whether rust-analyzer or cargo surfaced it (see *Diagnostics*). For cargo errors on `.child()` / arity / type mismatch, the usual cause is the macro routing through the wrong method — confirm the real signature with `cargo teksilo symbol <Widget>`. |
| Migrate an existing screen | Stage by logical section and run `cargo check -p <your-crate>` after each chunk. Structural tests over tree shape catch regressions a `check` cannot. |

## Routing rules — get these right or it will not compile

### 1. One method per slot — but check whether that slot takes an id

The structural slots (`child`, `content`, `header`, `pane`, `tab`, `item` on the
id-carrying containers, …) take `impl IntoTeksiChild`, implemented for
`WidgetId` (attach a node that already exists) and for every `Widget` (insert a
new one), so `.child(w)` and `.child(id)` are both fine. The redundant
`add_child` / `child_id` / `*_id` twins for those were removed in 0.12.

**This is not universal, and the slot name does not tell you.** As of 0.12.1, 65
slot declarations still take `impl Widget + 'static` and will **not** accept a
`WidgetId` (18 distinct names; `composite_tooltip` accounts for 31 of them, one
per widget). The ones that bite:

- **`PopoverWidget::content`** — a named slot on a widget you would most expect
  to take an id, and it does not, so `content: some_id` fails there.
- `TextInput::leading_slot` / `trailing_slot`; every `StandardListItem` /
  `StandardTreeItem` slot (`leading_slot`, `center_slot`, `trailing_slot`,
  `label_slot`, `subtitle_*_slot`).
- `MenuList::item` / `header`, `Banner::action`,
  `CompositeTooltipWidget::content`, and the whole `composite_tooltip` family.
- `Cycle::child` and `RadioGroup::child` — neither has anywhere to put an id.

Twelve `*_boxed` twins survive for `Box<dyn Widget>`, and `Breadcrumb::item_id`
survives as a genuinely *different* slot (its `item` takes a `BreadcrumbItem`
datum, and an id-carrying crumb never collapses into the overflow menu) — not a
twin.

When a slot rejects an id, the error is a trait-bound failure on
`IntoTeksiChild` or `Widget`. Confirm the real signature with
`cargo teksilo symbol <Widget>` rather than guessing.

### 2. A helper call is a child

A lowercase identifier that continues into a call, a method chain or an index at
body position lowers to `.child(expr)`: `VStack { my_row(x) }`,
`VStack { row(1).spacing(4.0) }`. A keyword-rooted head works too
(`self.row(x)`, `crate::ui::header()`). A lowercase identifier **standing alone**
is still the argument-free property, so a pre-built local still needs
`child: footer`.

### 3. Three heads are rejected

Body items are whitespace-separated, so Rust would read these as continuing the
item before: `(` (a call), `*` (a multiplication) and `&` (a bitwise and). Write
`child: (expr)`.

### 4. Category B widgets have no `.child()`

Content goes through named slots, and a bare child element inside one produces a
targeted compile-time error naming the right slot. The list the macro actually
checks (`teksilo_parse::diag::is_category_b_widget`) is: `Card`, `Accordion`,
`TitleBar`, `DialogContent`, `Breadcrumb`, `TabWidget`, `PopoverWidget`,
`PopoverButton`, `PopoverIconButton`, `PopoverCustom`, `Snackbar`, `Dialog`,
`Wizard`.

Note the **four** popover names: the type is `PopoverWidget<T>` and its aliases,
and none of them is spelled `Popover` — a bare child in a real popover used to
fall through to the generic error for exactly that reason.

Default slot hints: `content` for Card / Accordion / the popovers / Snackbar /
Dialog, `leading` for TitleBar, `body` for DialogContent, `item` for Breadcrumb,
`tab` for TabWidget, `step` for Wizard.

### 5. `#{ expr }` is the Rust escape

It carries a widget value as well as a `WidgetId`. It is how a Rust struct
literal is passed at body position, where the element form would otherwise claim
it: `#{ Card { title: t, root: None } }`. At a slot it needs no special routing
either: `header: #{ id }` emits `.header(id)`.

## Writing `teksu!` — preferred patterns

**Simple tree:**

```rust,ignore
teksu!(ctx =>
    VStack {
        spacing: 12.0
        TextWidget::new(lit!("Title")) { style: TextStyleRole::BodyBold }
        Button(lit!("OK")) { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Ok) }
    }
)
```

> The single most common mistake: it is
> `on_activate_fn: |ctx| ctx.send_intent(AppIntent::Ok)` — a **closure that
> fires the intent**, not `on_activate: AppIntent::Ok`. Handlers are always
> closures, and `move`, capture and arity stay exactly as written.

**Referencing a binding from a closure:**

```rust,ignore
teksu!(ctx =>
    Card {
        header: title = TextWidget(lit!("Manuscript")) { style: TextStyleRole::BodyBold }
        content: VStack {
            Button(lit!("Focus")) {
                on_tap: move |_, ctx| ctx.focus(title)
            }
        }
    }
)
```

**Mixing imperative logic:**

```rust,ignore
teksu!(ctx =>
    VStack {
        let accent = theme.colors.accent;
        rust {
            ctx.subscribe_event(origin, move |e| { /* … */ });
        }
        TextWidget(lit!("Status")) { color: accent }
    }
)
```

**Body-form for builder methods on an element argument** — this replaces method
chains (see *Known limitations*):

```rust,ignore
teksu!(ctx =>
    MenuList {
        item: MenuItem::new(lit!("Run")) {
            on_activate_fn: |ctx| ctx.send_intent(AppIntent::Run)
            tooltip: lit!("Runs the thing")
        }
    }
)
// emits .item(MenuItem::new(lit!("Run"))
//              .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Run))
//              .tooltip(lit!("Runs the thing")))
```

The body reads uniformly with top-level elements — same `name: value` shape, no
mental switch to Rust's method-chain syntax. Prefer it over
`item: (MenuItem::new(lit!("Run")).on_activate_fn(…).tooltip(…))`.

A body item is the widget's **real** builder method, spelled exactly as the
widget declares it — `tooltip: lit!("…")`, not an invented `tooltip_literal:`.
When unsure, dump the surface with `cargo teksilo symbol MenuItem`.

## Reading `teksu!` — translation shortcuts

Scan for these shapes and translate mentally:

- `Type(args) { … }` → `Type::new(args)` chain
- `Type::ctor(args) { … }` → `Type::ctor(args)` chain (ctor used verbatim)
- `name: value` → `.name(value)` method call
- `name: a, b` → `.name(a, b)` multi-arg
- `fills_stack` (bare lowercase) → `.fills_stack()` zero-arg call
- Bare `UpperCamel(...)` at body → `.child(UpperCamel::new(...))`
- `name = Element` → hoisted `let name = ctx.add(…); … .child(name)`
- `#{ expr }` → `.child(expr)` at body position, `.slot(expr)` at a slot; carries a widget or a `WidgetId`
- `if cond { E }` (no else) → `.child_opt(if cond { Some(E) } else { None })`
- `if cond { A } else { B }` / multi-arm `if` / `match` → `.child(TeksiBranch[N]::…(E))` dispatched by arm index
- `for pat in iter { E }` → `.children(iter.map(|pat| E))`
- `..expr` → statement-form spread: `for id in expr { __parent = __parent.child(id); }`
- `rust { … }` → block either produces a child (no trailing `;`) or runs for side effect

Commas between body items are accepted as optional separators, so
`Panel { padding: 8.0, color: RED }` on one line works the same as two
newline-separated properties.

## Diagnostics

The macro pre-empts one common mistake with a targeted message:

- **Bare child inside a Category B widget** → "`<Type>` is a Category B widget
  with named slots — use `<slot>: <widget>` instead of a bare child element".
  Fix: use the suggested slot name.

All other errors surface as ordinary rustc diagnostics under your own token
(unknown property → method-resolution error on the prop name, constructor typo →
"cannot find type" on the ident, and so on).

### `no method named child found for struct WidgetWithHandlers`

Worth recognising, because the message names neither the cause nor anything you
wrote. The lowering pass moves every property whose name is a `WidgetBuilder`
method (`on_tap`, `focusable`, `cursor`, `access_label`, …) to the **end** of the
emitted chain, because those methods return `WidgetWithHandlers<T>`, which
exposes none of the wrapped widget's own setters. A `WidgetBuilder` method the
macro's predicate does not know about is *not* moved — it stays where you wrote
it, and the next `.child(..)` / `.spacing(..)` then resolves against
`WidgetWithHandlers<T>` instead of the widget.

If you hit this on a property you believe is a real `WidgetBuilder` method, the
local workaround is to write that property **last** in the body yourself. It is
a framework-side bug (a missing entry in the parser's predicate) — report it with
the method name. The framework has a build-time guard comparing both directions,
so it should be rare from 0.12 on; two shipped methods were absent this way for
several releases before the guard existed.

### rust-analyzer

If rust-analyzer shows "expected an expression" at a `teksu!` token, its
proc-macro server has stopped expanding. Reload via Command Palette →
`rust-analyzer: Restart server`. The macro itself works — the DSL syntax is only
visible as errors when pre-expansion fallback parsing kicks in.

## Known limitations — surface these when relevant

- **4-arm cap on `if` / `match`** — deeper dispatches need a helper returning
  `Box<dyn Widget>`.
- **No reactive-if.** `if signal { … }` where `signal: Signal<bool>` does **not**
  auto-bind `visible_when` (the macro never invents reactivity the builder does
  not have). Use `ctx.visible_when(id, signal)` on a pre-registered child.
- **Struct literals as property values need parens** — `prop: (MyStruct { field: 1 })`,
  because the macro commits to element parsing on `UpperCamel { … }`. Enum
  variants (`prop: Type::Variant` / `prop: Type::Variant(inner)`) are recognised
  as expressions via the `UpperCamel::UpperCamel` shape and need no parens.
- **Method chains on widgets at prop-arg position are disallowed.**
  `prop: Widget::ctor(args).method(arg)` does not parse as you would expect,
  because the DSL already provides the body-form equivalent. Rewrite as
  `prop: Widget::ctor(args) { method: arg }` — the canonical `teksu!` way to
  apply builder methods to a widget value. Lowercase-rooted chains
  (`prop: signal.map(…)`) need no workaround. For UpperCamel chains that cannot
  fit body form, wrap in parens: `prop: (MyWrapper::from(x).finalize())`.
- **Binding hoist scope** — bindings inside `if` / `match` / `for` arms hoist to
  the outermost block, so the widget is created unconditionally. Gate
  construction with `rust { … }` if that matters.

## Formatting

rustfmt skips `teksu!` body content, so the DSL has its own formatter. Install
it once, matching the framework version your app pins:

```bash
cargo install cargo-teksilo-fmt --version <the teksilo version your app pins>
```

Then:

- `cargo teksilo-fmt path/to/file.rs` — format a single file.
- `cargo teksilo-fmt src/ui` — walk a directory.
- `cargo teksilo-fmt` — format from the current directory (recurses, skips `target/`).
- `cargo teksilo-fmt --check` — read-only; exits 1 if any file would change. Use in CI.

Run it **after** confirming the block compiles — formatting a syntactically
broken block can mask the original error.

## Verifying

- `cargo check -p <your-crate>` — fastest feedback after writing or editing a block.
- `cargo teksilo-fmt <path>` — canonicalize formatting once it compiles.
- `cargo test -p <your-crate>` — structural assertions over tree shape catch
  regressions a `check` cannot see (a child attached to the wrong parent still
  compiles).

Do not claim a `teksu!` block "works" unless it compiles. The desugaring is
mechanical; the compilation step is where routing errors (wrong method name,
missing trait impl, a slot that rejects an id) actually surface.
