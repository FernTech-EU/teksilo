<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# `teksu!` Macro Reference

A block-structured DSL for Teksilo widget trees. `teksu!` is a thin syntactic
transform: every invocation desugars one-to-one to Teksilo V2 builder calls
at macro-expansion time. No hidden allocation, no runtime parsing, no
virtual tree — the output is exactly the code you could have written by
hand.

This document is the user-facing reference, and it is **normative for
behaviour**: where it and the design spec disagree, this document is right.
For the design rationale (why the grammar has this shape, what was left out,
and what shipped differently from what was designed), see
[teksu-language-spec-v3.md](teksu-language-spec-v3.md).

> **Labels in these examples.** For brevity the examples below pass bare string literals
> (`Button("Save")`). With the default `i18n` feature a widget label is a `LocalizedString`
> and there is no `From<&str>`, so in a real app wrap it: `Button(lit!("Save"))` (untranslated)
> or `Button(tr!(save()))` (translated). `teksu!` passes whatever is inside `(…)` verbatim, so
> the wrapping just goes inside the parens. The fake widgets used to demonstrate macro
> mechanics (`Probe`, `Tag`, `Marker`, …) take a plain `&str` and need no wrapping.

---

## Importing

```rust
use teksilo::prelude::*;        // also provides: teksu!
// or:
use teksilo::teksu;
```

---

## Invocation

Two forms:

```rust
teksu!(ctx => <root-element>)    // inserts the root via ctx.add, returns WidgetId
teksu!(<root-element>)           // returns a widget value (for .child(...), etc.)
```

`ctx` in the preamble is an identifier — name it whatever your local
is called (`tree`, `build_ctx`, `ctx`, …). The `=>` is literal syntax.
Expansion routes every internal `add` call through that ident, so
`teksu!(tree => ...)` emits `tree.add(...)`.

Without the preamble, expansion falls back to an unqualified `ctx`
when bindings are present — that local must be in scope at the call
site. A block with no bindings (escapes included: `#{ expr }` lowers to
a plain `.child(expr)`) doesn't need `ctx` available.

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
separators (so `Panel { padding: 8.0, background: RED }` on one line works
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
    respect_intrinsic
    TextWidget("Body")
}
// ↓ Expand::new().respect_intrinsic().child(TextWidget::new("Body"))
```

*Standing alone* is the operative part. A lowercase identifier that continues
into a call or a method chain is a Rust expression producing a widget and
becomes a child instead (`section("Metadata")`, `row(1).spacing(4.0)`), so your
own helper functions are children like any element. See
[Real widgets](#your-own-helpers-are-children-too).

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

Body items are emitted in source order, with one exception: a property
naming a `WidgetBuilder` method is moved to the end of the chain (see
[Handlers](#handlers)). You can interleave properties and children
freely either way.

---

## Category B slots

Widgets with named slots (Card, TabWidget, Dialog, Accordion and others)
address content by slot name, not by bare child. The set the macro
recognises well enough to emit a targeted hint for is
`teksilo_parse::diag::is_category_b_widget`; a widget outside it fails
with the compiler's own "no method named `child`" instead:

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

## Real widgets

Everything above is shown on stacks, panels and single-control widgets, which
makes the DSL look like it only reaches static chrome. It does not. A positional argument list and a
property value are both handed to the builder verbatim, so a delegate closure,
a model handle and a `Signal` pass through untouched, and an accumulator
builder (`add_column`, `line`, `item`, `dock`) is a property written more than
once.

Every block in this section was compiled and mounted in a real `WidgetTree`.

### Data views take their delegate as a positional argument

```rust
teksu!(ctx =>
    ListView(rows, |_index, item: &Row, selected| {
        let subtitle = format!("{} words", item.count);
        Box::new(
            StandardListItem::new(lit!(&item.title))
                .subtitle(lit!(subtitle))
                .selected(selected),
        )
    }) {
        item_height: 44.0
        selection: selection
    }
)
```

Builder equivalent: `ListView::new(rows, delegate).item_height(44.0).selection(selection)`.

Nothing inside `(...)` is parsed by the macro, so the closure body is ordinary
Rust, multi-statement included. An explicit constructor works the same way, and
the shape generalises to every data view:

```rust
teksu!(ctx =>
    TreeView::new_with_context(tree, |item: &Row, entry, selected, row| {
        Box::new(
            StandardTreeItem::new(lit!(&item.title))
                .from_entry(entry)
                .selected(selected)
                .on_chevron_toggle_rc(row.toggle_callback()),
        )
    }) {
        item_height: 28.0
        row_click_expands: true
    }
)
```

Builder equivalent: `TreeView::new_with_context(tree, delegate).item_height(28.0).row_click_expands(true)`.

### A repeated property is how you drive an accumulator

`TableView` has no children, it has columns, and `add_column` is an ordinary
builder method, so write it once per column:

```rust
teksu!(ctx =>
    TableView(rows) {
        add_column: Column::new("title", lit!("Title"), |row: &Row, _cell| {
            Box::new(TextWidget::new(lit!(&row.title)))
        })
        add_column: Column::new("count", lit!("Words"), |row: &Row, _cell| {
            Box::new(TextWidget::new(lit!(row.count.to_string())))
        })
        row_height: 28.0
    }
)
```

Builder equivalent: two chained `.add_column(...)` calls, then `.row_height(28.0)`,
in that order (non-`WidgetBuilder` properties keep their source position).

`FormLayout` is the same shape with a two-argument row method. The comma
continues the argument list, and an UpperCamel element after it stays an
argument instead of becoming a child, which is what makes a row accumulator
readable:

```rust
teksu!(ctx =>
    FormLayout {
        row_spacing: 10.0
        label_gap: 12.0
        line: TextWidget(lit!("Name")), TextInput(name)
        line: TextWidget(lit!("Author")), TextInput(author)
        full_width: Button(lit!("Save"))
    }
)
```

Builder equivalent: `.row_spacing(10.0).label_gap(12.0).line(label, field).line(label, field).full_width(button)`.

`MenuList` mixes an accumulator with the argument-free bare-lowercase form:

```rust
teksu!(ctx =>
    MenuList {
        item: MenuItem::new(lit!("&Save"))
        item: MenuItem::new(lit!("Save &As"))
        separator
        item: MenuItem::new(lit!("&Quit"))
    }
)
```

and `Toolbar` carries two accumulators side by side, `action` and `item`:

```rust
teksu!(ctx =>
    Toolbar {
        action: ToolbarAction::new(lit!("New"), || IconWidget::chevron_down(16.0))
        action: ToolbarAction::new(lit!("Open"), || IconWidget::chevron_up(16.0))
        item: ToolbarItem::separator()
        button_size: IconButtonSize::Compact
    }
)
```

Builder equivalent in both cases: one chained call per line, in source order.

### Tabs

`tab` takes a label and a content widget, which is the "UpperCamel after a
comma stays an argument" rule doing real work:

```rust
teksu!(ctx =>
    TabWidget(selected) {
        tab: lit!("Overview"), Card {
            content: TextWidget(lit!("Summary"))
        }
        tab: lit!("Details"), VStack {
            spacing: 8.0
            TextWidget(lit!("Line one"))
            TextWidget(lit!("Line two"))
        }
    }
)
```

Builder equivalent: `.tab(lit!("Overview"), Card::new().content(..)).tab(lit!("Details"), VStack::new()..)`.

The delegate-driven form is the same property with a `TabInfo` and a factory
closure. `TabInfo { ... }` at an argument position is a teksu element, not a
Rust struct literal, so it lowers to `TabInfo::new().title(..).closable(..)`:

```rust
teksu!(ctx =>
    TabWidget(selected) {
        static_tab_factory: TabInfo { title: lit!("Notes"), closable: true }, |_handle| {
            Box::new(TextWidget::new(lit!("Notes body")))
        }
    }
)
```

Builder equivalent: `.static_tab_factory(TabInfo::new().title(lit!("Notes")).closable(true), factory)`.

### Switcher takes bare children, and a Signal in its constructor

`Switcher` has `.child(..)`, so it is a Category A container whose selection
arrives as a constructor argument:

```rust
teksu!(ctx =>
    Switcher(page) {
        TextWidget(lit!("Page 0"))
        TextWidget(lit!("Page 1"))
        Panel {
            TextWidget(lit!("Page 2"))
        }
    }
)
```

Builder equivalent: `Switcher::new(page).child(..).child(..).child(..)`.

`page` is a `Signal<usize>`. A `Signal` or `Prop` constructor argument needs no
special handling anywhere in a block:

```rust
teksu!(ctx =>
    VStack {
        spacing: 8.0
        Toggle(on)
        Slider(value, 0.0, 1.0)
    }
)
```

### DockingLayout

```rust
teksu!(ctx =>
    DockingLayout(model) {
        center: Panel {
            TextWidget(lit!("Editor"))
        }
        dock: DockWidget::new(explorer, lit!("Explorer"), |_id| {
            TextWidget::new(lit!("Files"))
        })
        dock: DockWidget::new(problems, lit!("Problems"), |_id| {
            TextWidget::new(lit!("No problems"))
        })
    }
)
```

Builder equivalent: `.center(panel).dock(..).dock(..)`. `center` is a slot and
`dock` an accumulator; the DSL does not distinguish between them, and neither
does the builder.

### Your own helpers are children too

A lowercase identifier that continues into a call or a method chain is a Rust
expression producing a widget, and lowers to `.child(expr)`. It works at body
position and inside a structural arm:

```rust
teksu!(ctx =>
    VStack {
        spacing: 8.0
        section("Metadata")
        section("Layout").spacing(4.0)
        if show {
            section("Advanced")
        }
        boxed_branch(5)
    }
)
```

given `fn section(title: &str) -> VStack` and
`fn boxed_branch(n: i32) -> Box<dyn Widget>`. A lowercase identifier standing
alone is still the argument-free property (`respect_intrinsic`), so the two forms do
not collide.

### What genuinely does not work

Each form below was compiled; the message is the one rustc prints.

**An UpperCamel method chain as a property value.**

```rust
Panel {
    child: Button::new(lit!("Save")).tooltip(lit!("Write to disk"))
}
```

```text
error: expected a property name, child element, binding, or `#{ expr }` escape
   |
   |             child: Button::new(lit!("Save")).tooltip(lit!("Write to disk"))
   |                                             ^
```

The caret sits on the `.`. Write the body form instead, which compiles:
`child: Button(lit!("Save")) { tooltip: lit!("Write to disk") }`. This is the
UpperCamel case only: a chain rooted in a lowercase path is an expression and
passes through, so `padding: pad.max(4.0)` is fine.

**A Rust struct literal as a property value, or at body position.** Both are
parsed as teksu elements, so the field names become builder calls and the type
name gets a `::new`:

```rust
Panel {
    hit_slop: HitSlop { radius: 8.0, up_to: 44.0 }
}
```

```text
error[E0599]: no associated function or constant named `new` found for struct `HitSlop` in the current scope
   |
   |             hit_slop: HitSlop { radius: 8.0, up_to: 44.0 }
   |                       ^^^^^^^ associated function or constant not found in `HitSlop`
```

Parenthesise it: `hit_slop: (HitSlop { radius: 8.0, up_to: 44.0 })` compiles.
Enum variants are unaffected: `Type::Variant` and `Type::Variant(inner)` are
recognised as expressions.

**A fifth `if` or `match` arm.**

```text
error: teksu! supports up to 4 if-chain arms; wrap deeper chains in `Box<dyn Widget>` or split into a helper
error: teksu! supports up to 4 match arms; wrap deeper dispatches in `Box<dyn Widget>` or split into a helper
```

Both errors point at the `if` / `match` keyword. `Box<dyn Widget>` implements
`Widget`, so a helper returning one is a child like any other (`boxed_branch(5)`
above).

**A parenthesised child.** Body items are separated by whitespace, and Rust
reads `(a) (b)` as a call, so a parenthesised item would silently swallow the
next one as its argument. The form is rejected rather than allowed to do that:

```text
error: expected a property name, child element, binding, or `#{ expr }` escape
   |
   |             (section("one"))
   |             ^
```

Use the property form, where the argument list is delimited:

```rust
VStack {
    child: (section("one"))
    section("two")
}
```

**A child head that Rust would read as a continuation.** Body items are
separated by whitespace, not punctuation, so a head that can continue the
previous expression is rejected rather than allowed to swallow its neighbour:
`*` (`a *b` is a multiplication), `&` (`a &b` is a bitwise and, and no reference
type implements `Widget` anyway), and `(` as above. A keyword-rooted head is
accepted, because nothing continues into `self`, `Self`, `crate` or `super`:

```rust
VStack {
    self.row(x)
    crate::ui::header()
}
```

**A Rust struct literal at body position is read as an element.** `Card { title: t }`
is `Card::new().title(t)`, because the element form and Rust's struct literal are
the same tokens. If the type has no such builder the compiler says so:

```text
error[E0599]: no associated function or constant named `new` found for struct `Card`
   |
   |             Card { title: t, root: None }
   |             ^^^^ function or associated item not found in `Card`
```

Wrap it in the escape, which is what the escape is for:

```rust
VStack {
    #{ Card { title: t, root: None } }
}
```

---

## Bindings: `name = Element`

A binding names the `WidgetId` of an inserted widget so you can reference
it later. Bindings hoist to the enclosing `teksu!` block:

```rust
teksu!(ctx =>
    VStack {
        status = TextWidget("Ready")
        Button("Open") {
            on_activate_fn: |ctx| ctx.send_intent(AppIntent::Open)
            access_described_by: status
        }
    }
)
// ↓
{
    let status = ctx.add(TextWidget::new("Ready"));
    ctx.add(
        VStack::new()
            .child(status)
            .child(
                Button::new("Open")
                    .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Open))
                    .access_described_by(status),
            )
    )
}
```

A binding name is scoped to the whole block, not to the body it appears
in, and two bindings sharing a name silently alias. See
[Limitations](#limitations).

### Binding at a slot position

A binding works at a slot position with no special routing: a slot method
takes `impl IntoTeksiChild`, so the same name accepts the id.

```rust
Card {
    header: title = TextWidget("Manuscript") { style: bold }
    content: VStack {
        Button("Focus title") {
            on_tap: move |_, ctx| ctx.request_focus(title)
        }
    }
}
// ↓
// {
//     let title = ctx.add(TextWidget::new("Manuscript").style(bold));
//     Card::new()
//         .header(title)
//         .content(VStack::new().child(
//             Button::new("Focus title")
//                 .on_tap(move |_, ctx| ctx.request_focus(title))
//         ))
// }
```

`title` is in scope for any subsequent item in the same `teksu!` block,
including nested closures.

---

## Escape: `#{ expr }`

Insert **any** Rust expression that produces a child at a body or slot
position: a `WidgetId` already in the arena, or a widget value.

```rust
let toolbar_id = ctx.add(build_toolbar());

teksu!(ctx =>
    VStack {
        #{ toolbar_id }                        // an id already in the arena
        #{ Card { title: t, root: None } }     // a Rust struct literal
        #{ registry.lookup(key)? }             // any expression, really
    }
)
```

Both arms go through one method. At body position the escape lowers to
`.child(expr)`, and every container's `child` takes `impl IntoTeksiChild`,
which is implemented for `WidgetId` (attach the existing node) and blanket for
every `Widget` (insert a new one).

The escape earns its keep on the struct literal. A bare `Card { .. }` at body
position is an *element*, so the escape is the one thing that says "this is
Rust, not teksu". Two containers do not take an id this way, `Cycle` and
`RadioGroup`, because neither has anywhere to put one.

A slot takes the escape the same way, under its own name:

```rust
Card {
    header: #{ existing_header_id }    // → .header(existing_header_id)
}
```

A binding or `#{ }` escape is only needed when the same widget ID is
referenced from multiple places (e.g. a handler closure captures it).
If you just want to attach a pre-existing ID once, the equivalent
property form — `child: id` for a body child, `slot_name: id` for a
Category B slot — is shorter and is plain Rust inside the argument
position. There is no `_id` variant to remember: one name per slot,
and it takes either.

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

// Two arms: TeksiBranch<L, R>.
VStack {
    if is_logged_in {
        ProfileCard(user)
    } else {
        Button("Sign in") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::SignIn) }
    }
}

// Three or four arms: TeksiBranch3 / TeksiBranch4.
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
        State::Loading => Spinner(16.0),
        State::Loaded(data) => DataView(data.clone()),
        State::Error(msg) => ErrorBanner(msg.clone()),
    }
}
// ↓ .child(match state { ... TeksiBranch3::{A,B,C}(...) })
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

Inline an iterator of children, each passed to `.child(..)` (so `WidgetId`s
or widget values):

```rust
VStack {
    TextWidget("Header")
    ..plugin_widgets      // for id in plugin_widgets { __parent.child(id) }
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

Under the `ctx =>` preamble the whole tree is the argument of `ctx.add(...)`,
so a block that borrows `ctx` mutably (like the `subscribe_event` call above)
conflicts with that borrow. Make such calls before the macro, or use the
no-preamble form and `ctx.add` the result yourself.

---

## Handlers

Handlers are properties whose value is a closure. The macro preserves
closure syntax verbatim — `move`, capture, and arity stay as you wrote
them.

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

### Reorder rule

A property is **moved to the end** of the emitted builder chain when its
name is a method on the `WidgetBuilder` trait that returns
`WidgetWithHandlers<T>`. Every other body item keeps its source
position, and relative order within each of the two groups is preserved.

The return type is the criterion, not the `on_` prefix: a wrapping method
replaces the widget with `WidgetWithHandlers<T>`, which exposes none of
the widget's own setters, so anything written after it — a child, a
`spacing`, a named slot — would resolve against the wrapper and fail. The
reorder is what lets you write handlers and children in any order.

Every family of `WidgetBuilder` method is covered: gestures, focus and
keyboard, pointer events and cancellation, touch and pointer arbitration
(`touch_action` and the pan/hit-slop declarations), drag and drop,
accessibility overrides, and the framework-level node properties. The
authoritative list is `teksilo_parse::diag::is_widget_builder_method`;
the `teksilo-teksu-guard` crate fails the build if it falls behind the
trait, so a method added to the trait cannot silently stop being
reordered.

A `WidgetBuilder` method that takes no argument is written in the
argument-free bare-lowercase form and is reordered the same way:

```rust
Panel {
    no_hit_slop
    padding: 8.0
    TextWidget("Body")
}
// ↓ Panel::new()
//      .padding(8.0)
//      .child(TextWidget::new("Body"))
//      .no_hit_slop()
```

---

## Desugaring cheat sheet

`‹E›` stands for the recursive lowering of a nested teksu element.

| Surface form | Expansion |
| --- | --- |
| `TypePath(args)` | `TypePath::new(args)` |
| `TypePath::ctor(args)` | `TypePath::ctor(args)` |
| `name: value` | `.name(value)` |
| `name: a, b` | `.name(a, b)` |
| `name` (bare lowercase) | `.name()` |
| Bare `UpperCamel(...)` at body | `.child(‹E›)` |
| Bare lowercase call or method chain at body | `.child(expr)` |
| `name = ‹E›` at body | hoisted `let name = ctx.add(‹E›);` + `.child(name)` |
| `name = ‹E›` in slot `s` | hoisted `let` + `.s(name)` |
| `#{ expr }` at body | `.child(expr)` (an id or a widget) |
| `#{ expr }` in slot `s` | `.s(expr)` (an id or a widget) |
| `if cond { ‹E› }` | `.child_opt(if cond { Some(‹E›) } else { None })` |
| `if cond { ‹A› } else { ‹B› }` | `.child(if cond { TeksiBranch::L(‹A›) } else { TeksiBranch::R(‹B›) })` |
| `match x { p => ‹E›, … }` | `.child(match x { p => TeksiBranchN::…(‹E›), … })` |
| `for p in it { ‹E› }` | `.children((it).map(\|p\| ‹E›))` |
| `..expr` | stmt-form `for id in expr { __parent = __parent.child(id); }` |
| `rust { … expr }` | `.child({ … expr })` |
| `rust { …; }` | inline side-effect block |

---

## Diagnostics

The macro emits one targeted error for the common mistake:

- **Bare child inside a Category B widget** — "`Card` is a Category B
  widget with named slots — use `content: <widget>` instead of a bare
  child element". Points at the misplaced child.

Its other errors are structural and self-explanatory: a multi-arm `if`
without a final `else`, more than four `if` / `match` arms, a `match`
with fewer than two arms, and an `if` / `else` / `for` body holding more
than one element.

Everything else (unknown property, wrong handler arity, constructor
typo, type mismatches on property values) surfaces as a regular rustc
diagnostic under the user's token, thanks to span-preserving emission.

---

## Limitations

- **4-arm cap on `if`/`match`**: chains beyond four arms must be split
  into a helper returning `Box<dyn Widget>` (or refactored to `match`).
  `Box<dyn Widget>` implements `Widget`, so the helper's result is a bare
  child like any other.
- **Binding names are one flat namespace per block**: every binding
  hoists to the same `let` list at the root of the expansion, and the
  tree expression is emitted after all of them. Two bindings sharing a
  name therefore alias. The second shadows the first, *both* attach
  sites resolve to the later widget, and the earlier one is constructed
  and attached nowhere. Nothing rejects it and nothing looks wrong until
  it is on screen, so keep names distinct within a block. (The case does
  not arise from structural arms: a binding is not legal inside an `if` /
  `else` / `match` / `for` arm, which holds exactly one element.)
- **A hoisted `let` runs unconditionally**: a binding is `ctx.add(...)`,
  so the widget is built and inserted into the arena whether or not the
  branch that mentions it is taken. Binding is not a way to build a
  subtree lazily.
- **Reactive-if is not special-cased**: `if signal { ... }` where
  `signal: Signal<bool>` does **not** auto-bind `visible_when`. Write
  `visible_when: signal` as a property on the element (a `WidgetBuilder`
  method, so it is reordered to the end), or bind
  `ctx.visible_when(id, signal)` on a pre-registered child.
- **Struct literals need parens**: `prop: MyStruct { ... }` is parsed as a
  teksu element (per the spec's "commit on distinctive prefix" rule), at a
  property value and at body position alike. To pass a Rust struct literal,
  wrap it: `prop: (MyStruct { ... })` at a property value, `#{ MyStruct { ... } }`
  at body position (where parentheses are rejected). Enum variants don't need this wrapping,
  since `prop: Type::Variant` and `prop: Type::Variant(inner)` are recognized
  as expressions by their `UpperCamel::UpperCamel` shape. The error when you
  forget is in
  [What genuinely does not work](#what-genuinely-does-not-work).
- **No UpperCamel method chains at property-arg position**: write
  `item: MenuItem::new("x").on_activate_fn(f).tooltip("t")` as body form,
  `item: MenuItem("x") { on_activate_fn: f, tooltip: "t" }`. The body form reads
  uniformly with top-level elements and skips the element-vs-expression
  ambiguity. Chains rooted in a lowercase path (`signal.map(...)`,
  `pad.max(4.0)`, `items.iter().collect()`) need no workaround: they go through
  the expression path unconditionally. For an UpperCamel-rooted chain that
  doesn't fit the body form (rare), wrap it in parens:
  `prop: (MyWrapper::from(x).finalize())`.
- **An expression child must not start with punctuation that could continue the
  previous item.** `(`, `*` and `&` are rejected, because body items are
  whitespace-separated and Rust reads `(a) (b)`, `a *b` and `a &b` as one
  expression each. Write `child: (expr)`. Keyword-rooted heads (`self.row(x)`,
  `crate::ui::header()`) are accepted; nothing continues into them.
- **rust-analyzer**: the macro expands cleanly under rust-analyzer's
  proc-macro server; IDE features work on the expanded code. If you see
  "expected an expression" errors on non-Rust-shaped tokens (`#{ }`,
  bare-lowercase properties, `Widget { body }` at body position), the
  proc-macro server has stopped expanding — reload it from the command
  palette (`rust-analyzer: Restart server`) or rebuild the workspace.

---

## Further reading

- [teksu-language-spec-v3.md](teksu-language-spec-v3.md): the design
  rationale, its worked translations, a changelog of where the shipped
  macro diverged from the design, and **Why there is no v4**, which
  carries the September 2026 measurements and the two exits that stay
  available if the question reopens.
- [crates/teksilo/tests/teksi/pass/](../crates/teksilo/tests/teksi/pass/)
  — trybuild fixtures exercising every supported form.
- [crates/teksilo-parse/src/](../crates/teksilo-parse/src/) — the parser
  and IR; [crates/teksilo-macros/src/](../crates/teksilo-macros/src/) — the
  lowering (parse → IR → lower).
