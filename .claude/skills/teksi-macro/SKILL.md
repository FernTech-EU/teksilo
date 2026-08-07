---
name: teksu-macro
description: Read, write, explain, or translate `teksu!` DSL blocks. Use when the user asks to convert a builder chain to `teksu!` (or vice versa), to explain what an existing `teksu!` block expands to, to debug a `teksu!` compile error, to write a new widget tree in `teksu!` form, or asks how to express a specific pattern (`if`/`for`/`match`/`let`/`rust`/spread/escape/binding/Category B slot). Also use for `/teksu-macro` invocations.
user_invocable: true
---

# teksu-macro

The `teksu!` macro is a block-structured DSL that desugars one-to-one to
Teksilo V2 builder calls. This skill covers reading, writing, and
translating between the two forms.

## Primary references (read these before writing non-trivial `teksu!`)

- [docs/teksu-macro-reference.md](../../../docs/teksu-macro-reference.md)
  — user-facing reference with every surface form, desugaring table,
  diagnostics, and limitations. Consult this for syntax questions.
- [docs/teksu-language-spec-v3.md](../../../docs/teksu-language-spec-v3.md)
  — design spec with worked translations of 9 widget-catalog examples
  (`§7`). Consult this for canonical patterns.
- [crates/teksilo/tests/teksu/pass/](../../../crates/teksilo/tests/teksu/pass/)
  — runnable trybuild fixtures, one per DSL feature. Consult these
  when you need a minimal self-contained example.

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
| Convert `teksu!` → builder | Mechanical: elements to `Type::new(args)`, properties to `.prop(args)`, bare children to `.child(...)`, bindings to hoisted `let name = ctx.add(...); ...add_child(name)`. |
| Debug a compile error | First check whether rust-analyzer or cargo surfaced it (see Diagnostics below). For cargo errors on `.child()`/arity/type mismatch, the usual cause is the macro routing through the wrong method. |
| Migrate widget_catalog / example | Stage by logical section, verify with `cargo test -p <example>` after each chunk. The existing `scroll_area_fills_remaining_space`-style structural tests catch tree shape regressions. |

## Important routing rules (get these right or it won't compile)

1. **Multi-child containers** (`VStack`, `HStack`, `ZStack`, `Wrap`,
   `Grid`, `Masonry`, `Toolbar`, `StatusBar`) use `.add_child(id)`
   for pre-registered children. A body-position `#{ some_id }` or a
   body-position binding `name = Element` lowers to `.add_child(...)`
   — good on these containers.

2. **Single-child wrappers** (`Panel`, `Padding`, `Expand`, `Center`,
   `FixedSize`, `MinSize`, `MaxSize`, `AspectRatio`, `FocusRing`,
   `GroupBox`) use `.child_id(id)` — NOT `.add_child`. Using `#{ id }`
   at body position on these will emit a call to a method that doesn't
   exist. Workaround: write the id via a property — `child_id: id` —
   which is plain Rust inside the arg position.

3. **Category B widgets** (Card, Accordion, SplitView, TitleBar,
   DialogContent, Breadcrumb, TabWidget, Popover, Snackbar, Dialog,
   Wizard) have no `.child()`. Content goes through named slots. A
   bare child element inside one produces a targeted compile-time
   error pointing at the right slot name.

4. **`#{ }` vs plain ident as property value**: at a Category B slot,
   `#{ id_expr }` forces the `_id` routing — `header: #{ toolbar_id }`
   emits `.header_id(toolbar_id)`. The alternative is writing the `_id`
   method name directly: `header_id: toolbar_id` emits the same call
   without the escape. At a multi-child container (VStack, Panel, …),
   the property form `add_child: toolbar_id` emits `.add_child(toolbar_id)`
   and is usually the cleanest. Plain idents at property-value positions
   are just Rust expressions passed to the named method; the escape is
   only needed when you want the `_id` auto-suffix or when the value
   looks like an element prefix the parser would commit on.

5. **Bindings (`name = Element`) hoist to the outermost `teksu!` block**
   and always use `ctx.add(...)`. `ctx` must be in scope — either via
   the `teksu!(ctx => ...)` preamble or as a local at the call site.

6. **Handler-attachment properties are auto-reordered to the end** of
   the emitted chain. Methods on `WidgetBuilder` (`on_tap`, `on_hover`,
   `on_key`, `focusable`, `tab_index`, `cursor`, `clips_children_on`,
   `context_menu`, `on_drag_hover`, `on_drop`, all the gesture
   handlers) wrap the widget in `WidgetWithHandlers<T>` which doesn't
   expose per-widget setters — so the macro silently moves them past
   any `.child(...)`, `.spacing(...)`, or other widget-specific call.
   Write `on_tap: cb` before or after children; both compile. If you
   see a `WidgetWithHandlers<T>` error at expansion, check whether the
   property name matches a known handler — if not, it's a widget
   method and ordering within the widget's own setters is the
   builder's concern.

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
    Menu {
        item: MenuItem::new(lit!("Run")) {
            on_activate_fn: |ctx| ctx.send_intent(AppIntent::Run)
            tooltip_literal: "Runs the thing"
        }
    }
)
// emits .item(MenuItem::new(lit!("Run")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Run)).tooltip(lit!("...")))
```

The body reads uniformly with top-level elements — same `name: value`
shape, no mental switch to Rust's method-chain syntax. Prefer this
over `item: (MenuItem::new(lit!("Run")).on_activate_fn(...).tooltip(...))`.

## Reading `teksu!` — translation shortcuts

Scan for these shapes and translate mentally:

- `Type(args) { ... }` → `Type::new(args)` chain
- `Type::ctor(args) { ... }` → `Type::ctor(args)` chain (ctor used verbatim)
- `name: value` → `.name(value)` method call
- `name: a, b` → `.name(a, b)` multi-arg
- `fills_stack` (bare lowercase) → `.fills_stack()` zero-arg call
- Bare `UpperCamel(...)` at body → `.child(UpperCamel::new(...))`
- `name = Element` → hoisted `let name = ctx.add(...); ... .add_child(name)`
- `#{ id }` → routes through `add_child(id)` or `.slot_id(id)` per position
- `if cond { E }` (no else) → `.child_opt(if cond { Some(E) } else { None })`
- `if cond { A } else { B }` / multi-arm `if` / `match` → `.child(TeksiBranch[N]::...(E))` dispatched by arm index
- `for pat in iter { E }` → `.children(iter.map(|pat| E))`
- `..expr` → statement-form spread: `for id in expr { __parent = __parent.add_child(id); }`
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
- `cargo test -p teksilo --test teksilo_trybuild` — only needed when
  editing the macro crate itself; exercises every pass/fail fixture.
- `cargo test --workspace` — full regression after non-trivial changes.

Do not claim a `teksu!` block "works" unless it compiles. The macro's
desugaring is mechanical; the compilation step is where routing errors
(wrong method name, missing trait impl) surface.
