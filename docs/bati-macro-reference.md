# `bati!` Macro Reference

A block-structured DSL for Bastyde widget trees. `bati!` is a thin syntactic
transform: every invocation desugars one-to-one to Bastyde V2 builder calls
at macro-expansion time. No hidden allocation, no runtime parsing, no
virtual tree — the output is exactly the code you could have written by
hand.

This document is the user-facing reference. For the design rationale and
full grammar, see [bati-language-spec-v3.md](bati-language-spec-v3.md).

---

## Importing

```rust
use bastyde::prelude::*;        // also provides: bati!
// or:
use bastyde::bati;
```

---

## Invocation

Two forms:

```rust
bati!(ctx => <root-element>)    // inserts the root via ctx.add, returns WidgetId
bati!(<root-element>)           // returns a widget value (for .child(...), etc.)
```

`ctx` in the preamble is an identifier — name it whatever your local
is called (`tree`, `build_ctx`, `ctx`, …). The `=>` is literal syntax.
Expansion routes every internal `add` call through that ident, so
`bati!(tree => ...)` emits `tree.add(...)`.

Without the preamble, expansion falls back to an unqualified `ctx`
when bindings or escapes are present — that local must be in scope at
the call site. Pure bati! blocks with no bindings or escapes (just
elements and properties) don't need `ctx` available.

---

## Elements

An element is `TypePath [::ctor] [(args)] [{ body }]`. The constructor
part is optional — the macro emits `::new(args)` when you omit it:

```rust
Button("Click")                 // → Button::new("Click")
Button::new(lit!("Click"))    // → Button::new(lit!("Click"))
VStack                          // → VStack::new()
Padding::uniform(24.0)          // → Padding::uniform(24.0)
```

Dispatch rule: the last path segment's first character decides. A
lowercase first letter (or a leading underscore) marks an explicit
constructor — emitted as-is. An UpperCamel first letter marks a type
name — the macro appends `::new` automatically.

### Positional args

Whatever sits in `(...)` is passed verbatim to the callable:

```rust
TitleBar(host_ident)            // → TitleBar::new(host_ident)
Padding::symmetric(12.0, 8.0)   // → Padding::symmetric(12.0, 8.0)
```

### Body

The `{ ... }` block contains body items: properties, bindings, bare
children, structural forms, body-position escapes. Items are separated
by **newlines**; commas between items are accepted as optional
separators (so `Panel { padding: 8.0, color: RED }` on one line works
the same as two newline-separated properties).

---

## Properties

`name: value` desugars to a builder method call:

```rust
TextWidget("Hello") {
    style: t.body_bold.clone()
    color: c.text_primary
}
// ↓
TextWidget::new("Hello").style(t.body_bold.clone()).color(c.text_primary)
```

### Multi-argument properties

Commas continue the argument list until the parser sees a token that
looks like a new body item:

```rust
TitleBar(host) {
    border: theme.colors.text_secondary, 2.0
    background: theme.colors.surface_pressed
}
// ↓ .border(color, 2.0).background(...)
```

A comma followed by a `name:` property, a structural keyword (`if`,
`for`, `match`, `let`), a spread `..`, an escape `#{`, or a binding
`name =` terminates the arg list — those tokens start a new body item.
An UpperCamel element after a comma stays as a continuation argument (so
`tab: "Overview", Card { ... }` works).

### Argument-free bare-lowercase property

A single lowercase identifier at body position is a zero-arg method call:

```rust
Expand {
    fills_stack
    TextWidget("Body")
}
// ↓ Expand::new().fills_stack().child(TextWidget::new("Body"))
```

---

## Category A children

Stacks, panels, and single-child wrappers accept children by bare
element at body position:

```rust
VStack {
    spacing: 12.0
    TextWidget("Title") { style: t.body_bold.clone() }
    TextWidget("Body")
    Button("OK") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Submit) }
}
// ↓ VStack::new().spacing(12.0)
//      .child(TextWidget::new("Title").style(t.body_bold.clone()))
//      .child(TextWidget::new("Body"))
//      .child(Button::new("OK").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Submit)))
```

Body items are emitted in source order — you can interleave properties
and children freely.

---

## Category B slots

Widgets with named slots (Card, TitleBar, DialogContent, Breadcrumb,
TabWidget, Popover, Snackbar, Dialog, Wizard, Accordion, SplitView)
address content by slot name, not by bare child:

```rust
Card {
    header: TextWidget("Title") { style: t.body_bold.clone() }
    content: VStack {
        spacing: 12.0
        TextWidget("Line one")
        TextWidget("Line two")
    }
    footer: Button("OK") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Ok) }
    padding: 16.0
}
```

Slot values and scalar properties share syntax; the widget's builder
decides what each name means. A bare child inside a Category B widget
produces a targeted compile-time error pointing at the right slot name.

---

## Bindings: `name = Element`

A binding names the `WidgetId` of an inserted widget so you can reference
it later. Bindings hoist to the enclosing `bati!` block:

```rust
bati!(ctx =>
    VStack {
        open_btn = Button("Open") {
            on_activate_fn: |ctx| ctx.send_intent(AppIntent::Open)
        }
        TextWidget("Status") {
            linked_to: open_btn
        }
    }
)
// ↓
{
    let open_btn: WidgetId = ctx.add(Button::new("Open").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Open)));
    ctx.add(
        VStack::new()
            .add_child(open_btn)
            .child(TextWidget::new("Status").linked_to(open_btn))
    )
}
```

### Binding at a slot position

When a slot value is a binding, the macro routes to the slot's `*_id`
twin:

```rust
Card {
    header: title = TextWidget("Manuscript") { style: bold }
    content: VStack {
        Button("Focus title") {
            on_tap: move |_, ctx| ctx.focus(title)
        }
    }
}
// ↓
// {
//     let title = ctx.add(TextWidget::new("Manuscript").style(bold));
//     Card::new()
//         .header_id(title)
//         .content(VStack::new().child(
//             Button::new("Focus title")
//                 .on_tap(move |_, ctx| ctx.focus(title))
//         ))
// }
```

`title` is in scope for any subsequent item in the same `bati!` block,
including nested closures.

---

## Escape: `#{ expr }`

Insert a pre-registered `WidgetId` (or an arbitrary WidgetId expression)
at a body or slot position:

```rust
let toolbar_id = ctx.add(build_toolbar());

bati!(ctx =>
    VStack {
        #{ toolbar_id }             // → .add_child(toolbar_id)
        Expand {
            fills_stack
            child_id: scroll_id     // use property form where the
                                    // container's id method isn't
                                    // .add_child (e.g. single-child
                                    // wrappers use .child_id)
        }
    }
)
```

At a slot position, `#{ expr }` forces the `*_id` slot routing:

```rust
Card {
    header: #{ existing_header_id }    // → .header_id(existing_header_id)
}
```

A binding or `#{ }` escape is only needed when the same widget ID is
referenced from multiple places (e.g. a handler closure captures it).
If you just want to attach a pre-existing ID once, the equivalent
property forms — `add_child: id` for multi-child containers,
`child_id: id` for single-child wrappers, `slot_name_id: id` for
Category B slots — are shorter and plain Rust inside the arg
position.

---

## Structural forms

### `if` / `else if` / `else`

```rust
// No-else: child_opt path.
VStack {
    if is_logged_in {
        ProfileCard(user)
    }
}
// ↓ .child_opt(if is_logged_in { Some(ProfileCard::new(user)) } else { None })

// Two arms: BatiBranch<L, R>.
VStack {
    if is_logged_in {
        ProfileCard(user)
    } else {
        Button("Sign in") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::SignIn) }
    }
}

// Three or four arms: BatiBranch3 / BatiBranch4.
VStack {
    if count == 0 {
        TextWidget("Empty")
    } else if count == 1 {
        TextWidget("One item")
    } else {
        TextWidget(format!("{count} items"))
    }
}
```

Limits: up to 4 arms. Deeper dispatches use `match` or split into a
helper function returning `Box<dyn Widget>`.

Conditions are boolean expressions. For reactive visibility, bind a
`Signal<bool>` through the widget's own API (e.g.
`ctx.visible_when(id, signal)`).

### `match`

```rust
VStack {
    match state {
        State::Loading => Spinner,
        State::Loaded(data) => DataView(data.clone()),
        State::Error(msg) => ErrorBanner(msg.clone()),
    }
}
// ↓ .child(match state { ... BatiBranch3::{A,B,C}(...) })
```

2–4 arms supported. Each arm's body is a single element.

### `for`

```rust
VStack {
    for item in items.iter() {
        let id = item.id;
        let title = item.title.clone();
        ListItem(title) {
            on_tap: move |_, ctx| ctx.send_intent(AppIntent::Select(id))
        }
    }
}
// ↓ .children(items.iter().map(|item| {
//       let id = item.id;
//       let title = item.title.clone();
//       ListItem::new(title).on_tap(move |_, ctx| ctx.send_intent(AppIntent::Select(id)))
//   }))
```

The for-body is zero or more `let` bindings followed by a single
element. The `let`s exist so move-closures capture owned values instead
of references.

### `let` at body position

Introduces a local used by subsequent body items:

```rust
VStack {
    let heading_style = t.body_bold.clone();
    let accent = c.accent;
    TextWidget("Title") { style: heading_style.clone(), color: accent }
    TextWidget("Body")  { style: t.body.clone(),         color: accent }
}
```

Switches the enclosing element to statement-sequence form. The let is
scoped to the element's body block.

### `..spread`

Inline an iterator of `WidgetId`s as children:

```rust
VStack {
    TextWidget("Header")
    ..plugin_widgets      // for id in plugin_widgets { __parent.add_child(id) }
    TextWidget("Footer")
}
```

### `rust { ... }`

Imperative escape for code that isn't a single element. Two shapes,
determined by whether the block's last statement has a trailing `;`:

```rust
// Expression form — block value becomes a child.
VStack {
    TextWidget("Header")
    rust {
        let tag = if cond { "a" } else { "b" };
        MyWidget::new(tag)       // no trailing ;
    }
    TextWidget("Footer")
}

// Side-effect form — runs for effect, produces no child.
VStack {
    rust {
        ctx.subscribe_event(origin, move |e| { /* ... */ });
    }
    TextWidget("Status")
}
```

Side-effect form forces statement-sequence lowering.

---

## Handlers

Handlers are properties whose value is a closure. The macro preserves
closure syntax verbatim — `move`, capture, and arity stay as you wrote
them. Handler-attachment properties (`on_tap`, `on_hover`, `on_key`,
`focusable`, `cursor`, `context_menu`, and every other method on the
`WidgetBuilder` trait) are **automatically moved to the end** of the
emitted builder chain, so you can interleave them with children and
widget-specific properties in any order. Without the reorder, a call
like `.context_menu(...).child(...)` would fail to resolve because the
`WidgetBuilder` methods wrap the widget in `WidgetWithHandlers<T>`
which doesn't expose per-widget setters.

```rust
Button("Click") {
    on_activate_fn: |ctx| ctx.send_intent(AppIntent::Submit)
}

Button("Click") {
    on_tap: |_, ctx| ctx.send_intent(AppIntent::Submit)
}

Button("Click") {
    on_tap: move |_, ctx| {
        if counter.get() > 0 {
            ctx.send_intent(AppIntent::Submit);
        }
    }
}
```

Whether a handler attaches to the element itself or to an inner widget
is the builder's concern; the DSL does not distinguish.

---

## Desugaring cheat sheet

`‹E›` stands for the recursive lowering of a nested bati element.

| Surface form | Expansion |
| --- | --- |
| `TypePath(args)` | `TypePath::new(args)` |
| `TypePath::ctor(args)` | `TypePath::ctor(args)` |
| `name: value` | `.name(value)` |
| `name: a, b` | `.name(a, b)` |
| `name` (bare lowercase) | `.name()` |
| Bare `UpperCamel(...)` at body | `.child(‹E›)` |
| `name = ‹E›` at body | hoisted `let name = ctx.add(‹E›);` + `.add_child(name)` |
| `name = ‹E›` in slot `s` | hoisted `let` + `.s_id(name)` |
| `#{ id_expr }` at body | `.add_child(id_expr)` |
| `#{ id_expr }` in slot `s` | `.s_id(id_expr)` |
| `if cond { ‹E› }` | `.child_opt(if cond { Some(‹E›) } else { None })` |
| `if cond { ‹A› } else { ‹B› }` | `.child(if cond { BatiBranch::L(‹A›) } else { BatiBranch::R(‹B›) })` |
| `match x { p => ‹E›, … }` | `.child(match x { p => BatiBranchN::…(‹E›), … })` |
| `for p in it { ‹E› }` | `.children((it).map(\|p\| ‹E›))` |
| `..expr` | stmt-form `for id in expr { __parent = __parent.add_child(id); }` |
| `rust { … expr }` | `.child({ … expr })` |
| `rust { …; }` | inline side-effect block |

---

## Diagnostics

The macro emits one targeted error for the common mistake:

- **Bare child inside a Category B widget** — "`Card` is a Category B
  widget with named slots — use `content: <widget>` instead of a bare
  child element". Points at the misplaced child.

Everything else (unknown property, wrong handler arity, constructor
typo, type mismatches on property values) surfaces as a regular rustc
diagnostic under the user's token, thanks to span-preserving emission.

---

## Limitations

- **4-arm cap on `if`/`match`**: chains beyond four arms must be split
  into a helper returning `Box<dyn Widget>` (or refactored to `match`).
- **Binding hoist scope**: bindings declared inside `if`/`else`/`match`/
  `for` bodies currently hoist to the outermost `bati!` block. The
  widget is created unconditionally; only the parent's attachment is
  gated by the arm. Usually a non-issue; rearrange the binding site if
  construction cost matters.
- **Reactive-if is not special-cased**: `if signal { ... }` where
  `signal: Signal<bool>` does **not** auto-bind `visible_when`. Bind
  visibility through `ctx.visible_when(id, signal)` directly on a
  pre-registered child, or wrap the widget in your own helper.
- **Struct literals as arg values need parens**: `prop: MyStruct { ... }`
  is parsed as a bati element (per the spec's "commit on distinctive
  prefix" rule). To pass a Rust struct literal, wrap it: `prop: (MyStruct
  { ... })`. Enum variants don't need this wrapping — `prop: Type::Variant`
  and `prop: Type::Variant(inner)` are recognized as expressions because
  of the `UpperCamel::UpperCamel` shape.
- **No method chains on widgets at property-arg position**: write
  `item: MenuItem::new("x").on_activate(cmd).tooltip("t")` as body
  form — `item: MenuItem::new("x") { on_activate: cmd; tooltip: "t" }`.
  The body-form reads uniformly with top-level elements and skips the
  element-vs-expression ambiguity. For non-widget method chains rooted
  in lowercase paths (`signal.map(...)`, `items.iter().collect()`),
  no workaround is needed — lowercase paths go through the expression
  path unconditionally. For UpperCamel-rooted chains that don't fit
  the body form (rare), wrap in parens: `prop: (MyWrapper::from(x).finalize())`.
- **rust-analyzer**: the macro expands cleanly under rust-analyzer's
  proc-macro server; IDE features work on the expanded code. If you see
  "expected an expression" errors on non-Rust-shaped tokens (`#{ }`,
  bare-lowercase properties, `Widget { body }` at body position), the
  proc-macro server has stopped expanding — reload it from the command
  palette (`rust-analyzer: Restart server`) or rebuild the workspace.

---

## Further reading

- [bati-language-spec-v3.md](bati-language-spec-v3.md) — complete
  grammar, design principles, and worked translations of the reference
  examples.
- [crates/bastyde/tests/bati/pass/](../crates/bastyde/tests/bati/pass/)
  — trybuild fixtures exercising every supported form.
- [crates/bastyde-macros/src/](../crates/bastyde-macros/src/) — the
  implementation (parse → IR → lower).
