---
name: teksu-macro
description: Read, write, explain, or translate `teksu!` DSL blocks. Use when the user asks to convert a builder chain to `teksu!` (or vice versa), to explain what an existing `teksu!` block expands to, to debug a `teksu!` compile error, to write a new widget tree in `teksu!` form, or asks how to express a specific pattern (`if`/`for`/`match`/`let`/`rust`/spread/escape/binding/Category B slot). Also use for `/teksu-macro` invocations.
user_invocable: true
---

# teksu-macro

The `teksu!` macro is a block-structured DSL that desugars one-to-one to
Teksilo V2 builder calls. This skill covers reading, writing, and
translating between the two forms.

> **Where to run this.** The repo-relative paths below (`docs/…`, `crates/…`, `tools/…`) are
> relative to the root of a **Teksilo framework checkout** — the workspace containing
> `crates/teksilo-macros/`. Locate it with `git rev-parse --show-toplevel` from anywhere inside
> it, confirm that marker is there, and `cd` to that root; if the working directory is not in
> such a checkout, ask the user where it is rather than guessing.
>
> The `../../../` links resolve only when this file is read from inside the checkout. Installed
> elsewhere (a user-level skills directory) they are dead links, though everything the skill
> *teaches* about the DSL still applies — `teksu!` is written the same way in a consumer app,
> where the reference docs are on docs.rs instead.

## Primary references (read these before writing non-trivial `teksu!`)

- [docs/teksu-macro-reference.md](../../../docs/teksu-macro-reference.md)
  — user-facing reference with every surface form, desugaring table,
  diagnostics, and limitations. Consult this for syntax questions.
- [docs/teksu-language-spec-v3.md](../../../docs/teksu-language-spec-v3.md)
  — design spec with worked translations of 9 widget-catalog examples
  (`§7`). Consult this for canonical patterns.
- [crates/teksilo/tests/teksi/pass/](../../../crates/teksilo/tests/teksi/pass/)
  — 28 runnable trybuild fixtures, one per DSL feature (plus 4 under
  `fail/` with expected-stderr files). Consult these when you need a
  minimal self-contained example. Note the directory is spelled
  `teksi`, not `teksu` — a leftover from an earlier name for the macro.

## Mental model

Every `teksu!` block desugars at macro-expansion time to the builder
calls you could have written by hand. There is no runtime. Knowing the
desugaring lets you translate in either direction confidently.

The desugaring cheat sheet in [teksu-macro-reference.md#desugaring-cheat-sheet](../../../docs/teksu-macro-reference.md#desugaring-cheat-sheet)
is the single source of truth — quote from it when explaining a block.

> **Labels in these examples.** For brevity the examples below pass bare string literals
> (`Button("Save")`). With the default `i18n` feature a widget label is a `LocalizedString`
> and there is no `From<&str>`, so in a real app wrap it: `Button(lit!("Save"))` (untranslated)
> or `Button(tr!(save()))` (translated). `teksu!` passes whatever is inside `(…)` verbatim, so
> the wrapping just goes inside the parens. The fake widgets used to demonstrate macro
> mechanics (`Probe`, `Tag`, `Marker`, …) take a plain `&str` and need no wrapping.

## Slash-command invocation

If the user types `/teksu-macro` with no further context, don't guess —
ask which of the triage situations below applies, or ask them to paste
the block / builder chain they want to work on. If they pasted a code
block together with the invocation, pick the matching row.

## Quick triage

When the user asks about `teksu!`, match against these situations:

| User wants | What to do |
|---|---|
| Explain a block | Walk through it top-down, annotating each body item with its desugaring. Use the cheat-sheet expansions. |
| Write from scratch | Start from a builder-chain mental model, then rewrite as DSL. Keep closures and Rust expressions verbatim. |
| Convert builder → `teksu!` | Element-by-element. Flatten `.child(...)` chains into bare elements at body position. Move `.prop(v)` into `name: v` body items. Preserve explicit constructors (`Button::new_literal`, `Padding::uniform`, etc.). |
| Convert `teksu!` → builder | Mechanical: elements to `Type::new(args)`, properties to `.prop(args)`, bare children to `.child(...)`, bindings to hoisted `let name = ctx.add(...); ...child(name)`. |
| Debug a compile error | First check whether rust-analyzer or cargo surfaced it (see Diagnostics below). For cargo errors on `.child()`/arity/type mismatch, the usual cause is the macro routing through the wrong method. |
| Migrate widget_catalog / example | Stage by logical section, verify with `cargo test -p <example>` after each chunk. The existing `scroll_area_fills_remaining_space`-style structural tests catch tree shape regressions. |

## Important routing rules (get these right or it won't compile)

1. **One method per slot — but check whether that slot takes an id.** The
   structural slots (`child`, `content`, `header`, `pane`, `tab`, `item` on the
   id-carrying containers, …) take `impl IntoTeksiChild`, implemented for
   `WidgetId` (attach the node that already exists) and for every `Widget`
   (insert a new one), so `.child(w)` and `.child(id)` are both fine. The
   redundant `add_child` / `child_id` / `*_id` twins for those were removed.

   **This is not universal — the slot name does not tell you.** 65 slot
   declarations still take `impl Widget + 'static` and will **not** accept a
   `WidgetId` (18 distinct names; `composite_tooltip` accounts for 31 of them,
   one per widget). The ones that bite:

   - **`PopoverWidget::content`** — a Category B slot that nonetheless rejects
     an id, so `content: some_id` fails inside the widget you would most expect
     it to work in.
   - `TextInput::leading_slot` / `trailing_slot`; every `StandardListItem` /
     `StandardTreeItem` slot (`leading_slot`, `center_slot`, `trailing_slot`,
     `label_slot`, `subtitle_*_slot`).
   - `MenuList::item` / `header`, `Banner::action`,
     `CompositeTooltipWidget::content`, and the whole `composite_tooltip` family.
   - `Cycle::child` and `RadioGroup::child` — neither has anywhere to put an id.

   Twelve `*_boxed` twins survive for `Box<dyn Widget>`, and
   `Breadcrumb::item_id` survives as a genuinely *different* slot (its `item`
   takes a `BreadcrumbItem` datum, and an id-carrying crumb never collapses into
   the overflow menu) — not a twin.

   When a slot rejects an id the error is a trait-bound failure on
   `IntoTeksiChild` or `Widget`. Confirm the real signature with
   `python3 tools/extract_widget_api.py <Widget>` rather than guessing.

2. **A helper call is a child.** A lowercase identifier that continues into a
   call, a method chain or an index at body position lowers to `.child(expr)`:
   `VStack { my_row(x) }`, `VStack { row(1).spacing(4.0) }`. A keyword-rooted
   head works too (`self.row(x)`, `crate::ui::header()`). A lowercase
   identifier **standing alone** is still the argument-free property, so a
   pre-built local still needs `child: footer`.

3. **Three heads are rejected**, because body items are whitespace-separated
   and Rust would read them as continuing the item before: `(` (a call),
   `*` (a multiplication) and `&` (a bitwise and). Write `child: (expr)`.

4. **Category B widgets** have no `.child()` — content goes through named
   slots, and a bare child element inside one produces a targeted compile-time
   error naming the right slot. The list the macro actually checks
   (`teksilo_parse::diag::is_category_b_widget`) is: `Card`, `Accordion`,
   `TitleBar`, `DialogContent`, `Breadcrumb`, `TabWidget`, `PopoverWidget`,
   `PopoverButton`, `PopoverIconButton`, `PopoverCustom`, `Snackbar`, `Dialog`,
   `Wizard`. Note the **four** popover names: the type is `PopoverWidget<T>`
   and its aliases, and none of them is spelled `Popover` — a bare child in a
   real popover used to fall through to the generic error for exactly that
   reason. Default slot hints: `content` for Card / Accordion / the popovers /
   Snackbar / Dialog, `leading` for TitleBar, `body` for DialogContent, `item`
   for Breadcrumb, `tab` for TabWidget, `step` for Wizard.

5. **`#{ expr }` is the Rust escape**, and it carries a widget value as well as
   a `WidgetId`. It is how a Rust struct literal is passed at body position,
   where the element form would otherwise claim it:
   `#{ Card { title: t, root: None } }`. At a slot it needs no special
   routing either: `header: #{ id }` emits `.header(id)`.

## Writing `teksu!` — preferred patterns

**Simple tree**:

```rust
teksu!(ctx =>
    VStack {
        spacing: 12.0
        TextWidget::new(lit!("Title")) { style: t.body_bold.clone() }
        Button("OK") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Ok) }
    }
)
```

**Referencing a binding from a closure** (Spec §7.9):

```rust
teksu!(ctx =>
    Card {
        header: title = TextWidget("Manuscript") { style: bold }
        content: VStack {
            Button("Focus") {
                on_tap: move |_, ctx| ctx.focus(title)
            }
        }
    }
)
```

**Mixing imperative logic**:

```rust
teksu!(ctx =>
    VStack {
        let accent = theme.colors.accent;
        rust {
            ctx.subscribe_event(origin, move |e| { /* ... */ });
        }
        TextWidget("Status") { color: accent }
    }
)
```

**Body-form for builder methods on an element arg** (replaces method
chains — see the limitations section):

```rust
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

The body reads uniformly with top-level elements — same `name: value`
shape, no mental switch to Rust's method-chain syntax. Prefer this
over `item: (MenuItem::new(lit!("Run")).on_activate_fn(...).tooltip(...))`.

A body item is the widget's **real** builder method, spelled exactly as the
widget declares it — `tooltip: lit!("…")`, not an invented `tooltip_literal:`.
When unsure, dump the surface with
`python3 tools/extract_widget_api.py MenuItem`.

## Reading `teksu!` — translation shortcuts

Scan for these shapes and translate mentally:

- `Type(args) { ... }` → `Type::new(args)` chain
- `Type::ctor(args) { ... }` → `Type::ctor(args)` chain (ctor used verbatim)
- `name: value` → `.name(value)` method call
- `name: a, b` → `.name(a, b)` multi-arg
- `fills_stack` (bare lowercase) → `.fills_stack()` zero-arg call
- Bare `UpperCamel(...)` at body → `.child(UpperCamel::new(...))`
- `name = Element` → hoisted `let name = ctx.add(...); ... .child(name)`
- `#{ expr }` → `.child(expr)` at body position, `.slot(expr)` at a slot; carries a widget or a `WidgetId`
- `if cond { E }` (no else) → `.child_opt(if cond { Some(E) } else { None })`
- `if cond { A } else { B }` / multi-arm `if` / `match` → `.child(TeksiBranch[N]::...(E))` dispatched by arm index
- `for pat in iter { E }` → `.children(iter.map(|pat| E))`
- `..expr` → statement-form spread: `for id in expr { __parent = __parent.child(id); }`
- `rust { ... }` → block either produces a child (no trailing `;`) or runs for side effect

## Diagnostics

The macro pre-empts one common mistake with a targeted message:

- **Bare child inside a Category B widget** → "`<Type>` is a Category B
  widget with named slots — use `<slot>: <widget>` instead of a bare
  child element". Fix: use the suggested slot name.

Commas between body items are accepted as optional separators, so
`Panel { padding: 8.0, color: RED }` on one line works the same as two
newline-separated properties.

All other errors surface as regular rustc diagnostics under the user's
token (unknown property → method resolution error on the prop name,
constructor typo → "cannot find type" on the ident, etc.).

### rust-analyzer

If rust-analyzer shows "expected an expression" at a `teksu!` token, its
proc-macro server has stopped expanding. Reload via Command Palette →
`rust-analyzer: Restart server`. The macro itself works — the DSL
syntax is only visible as errors when pre-expansion fallback parsing
kicks in.

## Known limitations (surface these to users when relevant)

- **4-arm cap on `if`/`match`** — deeper dispatches need `Box<dyn Widget>`.
- **No reactive-if** — `if signal { ... }` where `signal: Signal<bool>`
  does NOT auto-bind `visible_when`. Use `ctx.visible_when(id, signal)`
  on a pre-registered child.
- **Struct literals as property values need parens** —
  `prop: (MyStruct { field: 1 })` (the macro commits to element parsing
  on `UpperCamel { ... }`). Enum variants (`prop: Type::Variant` or
  `prop: Type::Variant(inner)`) are recognized as expressions via the
  `UpperCamel::UpperCamel` shape and don't need parens.
- **Method chains on widgets at prop-arg position are disallowed**:
  `prop: Widget::ctor(args).method(arg)` doesn't parse as you'd expect
  because the DSL already provides the body-form equivalent. Rewrite
  as `prop: Widget::ctor(args) { method: arg }` — that's the canonical
  teksu! way to apply builder methods to a widget value. For lowercase-
  rooted chains (`prop: signal.map(...)`), no workaround is needed.
  For UpperCamel chains that can't fit body form, wrap in parens:
  `prop: (MyWrapper::from(x).finalize())`.
- **Binding hoist scope** — bindings inside `if`/`match`/`for` arms
  hoist to the outermost block, so the widget is created
  unconditionally. Gate construction with `rust { ... }` if it matters.

## If you add a `WidgetBuilder` method (framework repo only)

Adding a method to `WidgetBuilder` obliges you to add its **name** to
[`teksilo_parse::diag::is_widget_builder_method`](../../../crates/teksilo-parse/src/diag.rs).

The lowering pass moves every property whose name is a `WidgetBuilder` method to
the **end** of the emitted chain, because those methods return
`WidgetWithHandlers<T>`, which exposes none of the wrapped widget's own setters.
A method missing from the list is not moved — it stays where the user wrote it,
and the next `.child(..)` / `.spacing(..)` resolves against `WidgetWithHandlers<T>`
instead of the widget. What the user sees is `no method named child found for
struct WidgetWithHandlers`, pointing at a `.child` they did not write, inside a
macro expansion: a diagnostic naming neither the cause nor the file to edit. Two
shipped methods were absent this way for several releases.

`crates/teksilo-teksu-guard` now fails the build on the divergence — it parses
`widget_builder.rs` with `syn`, collects every method returning
`WidgetWithHandlers<Self>`, and compares both directions. So the obligation is
enforced, but read the error as "update the predicate", not "the guard is wrong".

## Formatting

After writing or editing a `teksu!` block, run the dedicated formatter
to canonicalize indentation, spacing, and line-wrapping inside the
block (rustfmt skips `teksu!` body content):

- `cargo teksilo-fmt path/to/file.rs` — format a single file.
- `cargo teksilo-fmt examples/widget_catalog` — walk a directory.
- `cargo teksilo-fmt` — format from CWD (recurses, skips `target/`).
- `cargo teksilo-fmt --check` — read-only; exits 1 if any file would
  change. Use in CI / pre-merge verification.

Run this **after** confirming the block compiles — formatting a
syntactically broken block can mask the original error.

## Verifying changes

- `cargo check -p <user-crate>` — fastest feedback after writing or
  editing a `teksu!` block in an example or widget.
- `cargo teksilo-fmt <path>` — canonicalize formatting once it compiles
  (see "Formatting" above).
- `cargo test -p widget-catalog` — existing structural assertions (e.g.
  `scroll_area_fills_remaining_space`) catch tree-shape regressions
  after a migration.
- `cargo test -p teksilo --test teksi_trybuild` — only needed when
  editing the macro crate itself; exercises every pass/fail fixture.
  (The target is `teksi_trybuild`, matching the fixture directory.)
- `cargo test --workspace` — full regression after non-trivial changes.

Do not claim a `teksu!` block "works" unless it compiles. The macro's
desugaring is mechanical; the compilation step is where routing errors
(wrong method name, missing trait impl) surface.
