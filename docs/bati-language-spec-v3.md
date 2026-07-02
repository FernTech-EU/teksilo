<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# The `bati!` Language Specification (v3)

**Status:** Design draft, revision 3
**Date:** April 17, 2026
**Companion to:** architecture.md §28.9
**Supersedes:** bati-language-spec-v2.md

---

## Changelog from v2

Four structural changes, all driven by review of v2 against the actual widget catalog and by subsequent design discussion.

**Bindings use `name = Element` instead of `id: name`.** The new form reads like ordinary Rust assignment, removes one keyword from the grammar, and works uniformly at body position and in property-argument position.

**Widget categories are now two, not three.** The framework refactor (see Appendix A) dissolves the former Category C by moving primary content from constructors to setter methods on ScrollArea, Popover, Snackbar, and Dialog. ScrollArea joins Category A (has `.child()`). Popover, Snackbar, and Dialog join Category B (named slots). Wizard is not Category C and was never intended to be; v2 misclassified it.

**The `*_id` convention is universal.** Every widget-accepting slot method on every container now has a twin taking a `WidgetId`, named `*_id`. The existing `.set_child(id)` methods on Panel, Padding, Expand, GroupBox, and Accordion are renamed to `.child_id(id)` / `.content_id(id)` for consistency. SplitView's existing `.first_id` / `.second_id` fits the pattern unchanged. TabWidget gets `.tab_id(label, id)` to match.

**Worked translations reflect the refactored API.** Every code example in §7 is against post-refactor builder signatures. The seven uploaded example files themselves are assumed to be migrated; the framework changes required are listed in Appendix A.

Em-dashes and middle dots in quoted source strings are preserved verbatim. The "no em-dashes in English prose" rule continues to apply to the spec's own writing and does not apply to code being quoted.

---

> **Labels in these examples.** For brevity the examples below pass bare string literals
> (`Button("Save")`). With the default `i18n` feature a widget label is a `LocalizedString`
> and there is no `From<&str>`, so in a real app wrap it: `Button(lit!("Save"))` (untranslated)
> or `Button(tr!(save()))` (translated). `bati!` passes whatever is inside `(…)` verbatim, so
> the wrapping just goes inside the parens. The fake widgets used to demonstrate macro
> mechanics (`Probe`, `Tag`, `Marker`, …) take a plain `&str` and need no wrapping.

## 1. Design Principles

The `bati!` macro is a thin syntactic transform. It is not a new runtime, not a new type system, and not a new reactivity model. Every `bati!` block desugars to a sequence of builder calls against Bastyde API. There is no hidden allocation, no intermediate virtual tree, no diff step. The macro's only job is to remove syntactic noise from code that already expresses a widget tree.

Five rules bind the design.

First, one-to-one desugaring. Every surface form has a unique, mechanically predictable expansion. No form that works only sometimes depending on macro inference.

Second, error spans follow the user. When expansion fails (wrong property name, wrong child type, wrong handler arity), the error points at the user's token, not at a synthetic span inside the expansion.

Third, reactivity and capture stay visible. `Signal<T>`, `Prop<T>`, and closure `move` appear in the source as themselves. The macro never synthesizes binding or capture semantics the user did not ask for.

Fourth, builder interop is symmetric. `bati!` expressions and builder chains can be freely nested in either direction. Neither is a superset of the other.

Fifth, the macro never introduces new capabilities. If a construct cannot be expressed by the V2 builder API, `bati!` will not invent a way to express it. Missing capabilities are fixed in the builder first, then surfaced in the DSL.

---

## 2. Lexical Structure

A `bati!` invocation takes one of two forms.

```rust
bati!(ctx => <root-element>)
bati!(<root-element>)
```

The `ctx =>` preamble binds the name used for the `BuildContext` inside the block, and causes the root element to be inserted into the arena via `ctx.add(...)` so the call returns a `WidgetId`. The shorter form has no preamble and returns a widget value suitable for passing to `.child(...)` or to a named-slot method.

Disambiguation is lexical. The macro parser looks at the first tokens: if the leading form is `ident =>`, the preamble is consumed; otherwise the macro starts parsing elements immediately.

---

## 3. Elements

An element is the fundamental unit of the language. It names a widget type (possibly with an explicit constructor path), optionally carries positional arguments and a body block containing properties, bindings, and child elements.

### 3.1 Grammar

```
element       := type_path ( "::" constructor )? ( "(" positional_args ")" )?
                 ( "{" body "}" )?

type_path     := path_segment ( "::" path_segment )*
constructor   := ident

body          := ( body_item )*
body_item     := property | binding | structural | child_element
property      := ident ":" arg_list
binding       := ident "=" element
arg_list      := arg ( "," arg )*
arg           := element | bound_element | expr
bound_element := ident "=" element

structural    := if_form | for_form | match_form | let_form | spread_form | rust_form

child_element := element
```

Body items are separated by newlines. There are no commas between body items. This eliminates the `,` noise between `.child(...).child(...)` calls that dominates the uploaded example files.

At positions where an `arg` is expected, the parser uses "commit on distinctive prefix" to decide between element and expression: if the leading tokens form a TypePath followed by `(`, `::`, or `{`, or match `ident = TypePath(...)`, commit to element or bound-element parsing. Otherwise commit to expression parsing. This rule is local (no backtracking) and preserves clean error spans.

### 3.2 Constructors

The type path in an element may end with an explicit associated function name. If present, the macro emits that function as the constructor. If absent, the macro emits `::new` as the default.

```rust
Button("Click")              desugars to   Button::new("Click")
TextWidget("Hello")          desugars to   TextWidget::new("Hello")
VStack                       desugars to   VStack::new()

Button::new(lit!("Click"))   desugars to   Button::new(lit!("Click"))
Padding::uniform(24.0)       desugars to   Padding::uniform(24.0)
Padding::symmetric(12.0, 8.0) desugars to  Padding::symmetric(12.0, 8.0)
ProgressBar::indeterminate() desugars to   ProgressBar::indeterminate()
```

Parenthesized arguments after the constructor are passed verbatim in order. An element with no parentheses (`VStack`, `Spacer`) is equivalent to one with empty parentheses.

### 3.3 Bindings: `name = Element`

Naming a widget binds its `WidgetId` to a local so it can be referenced later. Bindings work in two places: at body position inside a container, and in property-argument position.

**Binding at body position** (Category A containers only, see §4):

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
```

Desugars to:

```rust
{
    let open_btn: WidgetId = ctx.add(
        Button::new("Open").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Open))
    );
    ctx.add(
        VStack::new()
            .add_child(open_btn)
            .child(
                TextWidget::new("Status")
                    .linked_to(open_btn)
            )
    )
}
```

The binding is hoisted to the nearest enclosing statement-forming block (the `bati!` expansion here), where it remains in scope for the rest of the block. The container uses `.add_child(id)` to attach the bound element at its body position.

**Binding in property-argument position** (Category B slots):

```rust
bati!(
    Card {
        header: title = TextWidget("Manuscript") { style: t.body_bold.clone() }
        content: VStack {
            TextWidget("Set title:")
            Button("Focus title") {
                on_tap: move |_, ctx| ctx.focus(title)
            }
        }
    }
)
```

Desugars to:

```rust
{
    let title: WidgetId = ctx.add(
        TextWidget::new("Manuscript").style(t.body_bold.clone())
    );
    Card::new()
        .header_id(title)
        .content(
            VStack::new()
                .child(TextWidget::new("Set title:"))
                .child(
                    Button::new("Focus title")
                        .on_tap(move |_, ctx| ctx.focus(title))
                )
        )
}
```

The slot method switches from `.header(widget)` (widget-taking) to `.header_id(id)` (id-taking) to accommodate the binding. Every Category B slot method has an `*_id` twin by framework convention (see Appendix A).

**Scope rules.** A binding is in scope from the point of declaration to the end of the nearest enclosing statement-forming block. Statement-forming blocks are: a `bati!(...)` expansion, a `rust { }` block, a `match` arm, an `if` or `else` arm, a `for` body, and a `let` form's scope. A binding declared in one arm is not visible from a sibling arm.

### 3.4 Properties

A property is `name: arg1, arg2, ...` and desugars to a builder method call with those arguments.

```rust
// Single argument
TextWidget("Hello") {
    style: t.body_bold.clone()
    color: c.text_primary
}

// Desugars to
TextWidget::new("Hello")
    .style(t.body_bold.clone())
    .color(c.text_primary)
```

```rust
// Multiple arguments
TitleBar(host) {
    height: 40.0
    border: theme.colors.text_secondary, 2.0
    background: theme.colors.surface_pressed
}

// Desugars to
TitleBar::new(host)
    .height(40.0)
    .border(theme.colors.text_secondary, 2.0)
    .background(theme.colors.surface_pressed)
```

**Bare lowercase identifier as argument-free method call.** A body item consisting of a single lowercase identifier is a property call with no arguments.

```rust
Expand {
    fills_stack
    TextWidget("Body")
}

// Desugars to
Expand::new()
    .fills_stack()
    .child(TextWidget::new("Body"))
```

The distinction between "bare child element" and "argument-free property" is lexical: Rust naming convention is `UpperCamel` for types and `snake_case` for methods. A bare identifier at body position starting with an uppercase letter is a child element; starting with a lowercase letter, it is a property call.

**Argument list termination.** The argument list of a property terminates at the next newline, unless the last token on the line is inside an open bracket (paren, brace, or square), in which case parsing continues until the brackets balance. This handles struct literals, tuples, and multi-line element values as argument values correctly:

```rust
Panel {
    style: TextStyle {
        family: "sans-serif".into(),
        size: 14.0,
        weight: FontWeight::BOLD,
    }
    offset: (4.0, 2.0)
    TextWidget("Hello")
}
```

Each of these is a single-argument property whose value happens to contain commas inside brackets.

**Multi-argument with element values.** A property argument can be a full element, including one with its own body. This is the TabWidget pattern:

```rust
TabWidget(selected) {
    tab: lit!("Overview"), Card {
        header: TextWidget(lit!("Overview")) { style: t.body_bold.clone() }
        content: VStack { spacing: 12.0, ... }
    }
    tab: lit!("Inspector"), Panel { padding: 20.0, ... }
    trailing_slot: trailing_widget
}
```

The comma after `lit!("Overview")` is at depth 0 (not inside brackets) and separates the two arguments of `tab`. The next `Card { ... }` element opens a brace that may span multiple lines; the parser tracks bracket balance until the Card's closing `}`. After the Card closes, the next newline terminates the argument list and ends the `tab` property.

Desugars to (`TabWidget::tab(label, content)` is the title-only shorthand for `static_tab(TabInfo::new().title(label), content)`):

```rust
TabWidget::new(selected)
    .tab(
        lit!("Overview"),
        Card::new()
            .header(TextWidget::new(lit!("Overview")).style(t.body_bold.clone()))
            .content(VStack::new().spacing(12.0)...)
    )
    .tab(lit!("Inspector"), Panel::new().padding(20.0)...)
    .trailing_slot(trailing_widget)
```

Property ordering is preserved. The macro emits method calls in source order.

Properties are never reinterpreted. `color: c.text_primary` emits `.color(c.text_primary)` whether `c.text_primary` is a `Color`, a `Signal<Color>`, or a `Prop<Color>`. Conversion happens at the type level through `impl Into<Prop<T>>`, not in the macro.

### 3.5 Handlers

Handler attachment is a property. The grammar does not distinguish handlers from configuration. Convention names them `on_*`, but this is enforced by each widget's builder API, not by the macro.

```rust
Button("Click") {
    on_activate_fn: |ctx| ctx.send_intent(AppIntent::Submit)
}

Button("Click") {
    on_tap: |_, ctx| ctx.send_intent(AppIntent::Submit)
}

Button("Click") {
    on_tap: move |_, ctx| {
        if some_signal.get() > 0 {
            ctx.send_intent(AppIntent::Submit);
        }
    }
}
```

All three desugar to the method call named by the property. The macro does not modify closure syntax: `move` stays explicit where the user writes it, and is absent where the user omits it. This is rule 3 of the design principles.

### 3.6 Child Elements

A bare element at body position, with no `name:` prefix and no `name =` binding, is a child element. Children desugar to `.child(...)` calls on the parent, using the inline-child resolution path from architecture §6.1.

```rust
VStack {
    spacing: 12.0
    TextWidget("Title") { style: t.body_bold.clone() }
    TextWidget("Body")  { style: t.body.clone() }
}

// Desugars to
VStack::new()
    .spacing(12.0)
    .child(TextWidget::new("Title").style(t.body_bold.clone()))
    .child(TextWidget::new("Body").style(t.body.clone()))
```

Body items interleave freely. Properties, bindings, and children appear in the output chain in source order:

```rust
VStack {
    TextWidget("Header")
    spacing: 12.0
    TextWidget("Body")
}

// Desugars to
VStack::new()
    .child(TextWidget::new("Header"))
    .spacing(12.0)
    .child(TextWidget::new("Body"))
```

Style guides may recommend "properties first, children last" as convention. The grammar does not enforce it.

Bare child elements are only meaningful for Category A containers (§4.1). For Category B widgets, the equivalent error from the compiler is `no method named 'child' on Card`, which is clear enough without special macro handling.

---

## 4. Widget Categories

Every Bastyde widget falls into one of two categories based on how it accepts content. The category determines which DSL form applies.

### 4.1 Category A: Has `.child()`

These widgets accept one or more children through a `.child(widget)` method and an `.add_child(id)` or `.child_id(id)` twin. Body-block child syntax in the DSL maps directly.

**Members:**

Layout primitives: VStack, HStack, ZStack, Padding, Expand, Switcher, Center, MinSize, MaxSize, FixedSize, AspectRatio, Wrap, Grid.

Flat containers: Panel, Toolbar, StatusBar, GroupBox.

Scrolling: ScrollArea (post-refactor; see Appendix A).

**DSL form:**

```rust
VStack {
    spacing: 12.0
    TextWidget("Hello")
    Button("Click") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Go) }
}
```

Bare children desugar to `.child(widget)`. Bound children desugar to `.add_child(id)` (or `.child_id(id)` where the container uses that name).

### 4.2 Category B: Named Slots

These widgets have no `.child()` method. Content goes through named setter methods, one per semantic slot. Each slot method has both a widget-taking form (`.slot_name(widget)`) and an id-taking twin (`.slot_name_id(id)`).

**Members and their slots:**

- **Card** (`header`, `content`, `footer`)
- **Accordion** (`content`, with `title` taken as constructor arg)
- **SplitView** (`first`, `second`)
- **TitleBar** (`leading`, `center`, `trailing`, plus the non-widget `close_action` handler)
- **DialogContent** (`body`, `footer`, with `title` and `supporting_text` as LocalizedString properties)
- **Breadcrumb** (`item` and `trailing_slot`)
- **TabWidget** (`tab`, `tab_item`, `trailing_slot`, with tabs being multi-arg `(label, widget)` pairs)
- **Popover** (`content`, `trigger`; post-refactor)
- **Snackbar** (`content`, `trigger`; post-refactor)
- **Dialog** (`content` taking a `Fn() -> impl Widget` factory, plus `trigger`; post-refactor)

**DSL form:**

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

Slot values and decoration properties use identical syntax. The widget's own documentation tells the reader which properties are slots. The DSL grammar does not distinguish.

### 4.3 Leaf Widgets

Widgets with no child-accepting methods at all. Buttons, TextWidget, IconWidget, ImageWidget, RectWidget, Badge, Link, Spacer, Divider, Toggle, Checkbox, RadioButton, Slider, ProgressBar. These have properties but no body children or slots. Their DSL form is just `Type(args) { property: value, on_handler: closure, ... }`.

### 4.4 Wizard

Wizard is structurally its own case: it takes a title in the constructor and wires multi-step content through `.step(WizardStep)` and `.steps(iter)` methods. It is not refactored in Appendix A because its shape does not fit cleanly into either Category A or B. For DSL authoring, treat Wizard like Category B with named slots, adding `step` and `steps` to the slot vocabulary. Details of Wizard's DSL form are deferred; the current builder API is usable directly.

---

## 5. Structural Forms

Pure element syntax handles fixed structure, fixed properties, fixed children. The remaining cases, conditional inclusion, iteration, local bindings, side effects, and programmatic subtree splicing, get first-class structural forms rather than forcing users back to builder syntax mid-block.

### 5.1 `if` Forms

The condition is an arbitrary Rust `if` head, including `if let` and `else if` chains.

```rust
VStack {
    if is_logged_in {
        ProfileCard(user.clone())
    } else {
        Button("Sign in") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::SignIn) }
    }
}

VStack {
    if let Some(msg) = error_message.as_ref() {
        ErrorBanner(msg.clone())
    }
}

VStack {
    if count == 0 {
        TextWidget("Empty")
    } else if count == 1 {
        TextWidget("One item")
    } else {
        TextWidget(format!("{} items", count))
    }
}
```

Desugaring:

An `if` without `else` desugars to `.child_opt(if cond { Some(widget) } else { None })`.

An `if/else` with two arms of different widget types desugars via `BatiBranch<L, R>` to a type that implements `IntoBatiChild` by dispatching to the active variant. Three- and four-way branches use `BatiBranch3` and `BatiBranch4`. Branches beyond four arms require explicit `Box<dyn Widget>`.

**Reactive conditionals.** If the condition is a bare identifier whose static type is `Signal<bool>` or `Prop<bool>`, the lowering is `.visible_when(signal)` on the child rather than an arena-level conditional. This is the one place where the macro performs type-directed inference. It is conservative: anything other than a bare identifier of the right type (a boolean expression, a function call, a dereferenced signal, an `if let`) falls into the regular `child_opt` / `BatiBranch` path. To force the build-time conditional when the condition is a Signal identifier, write `if signal.get() { ... }`.

The same binding is also available explicitly as a per-widget property: `Widget { visible_when: signal }` desugars to `.visible_when(signal)` (a `WidgetBuilder` method accepting `bool` / `Signal<bool>` / `Prop<bool>`), equivalent to the imperative `ctx.visible_when(id, signal)`. Use the property form when you want visibility to read as a widget attribute alongside its other properties rather than as a wrapping `if`.

### 5.2 `for` Forms

Iteration produces a stream of children from a regular Rust iterator.

```rust
VStack {
    TextWidget("Items:") { style: t.body_bold.clone() }
    for item in items.iter() {
        let id = item.id;
        let title = item.title.clone();
        ListItem(title) {
            on_tap: move |_, ctx| ctx.send_intent(AppIntent::Select(id))
        }
    }
}

// Desugars to
VStack::new()
    .child(TextWidget::new("Items:").style(t.body_bold.clone()))
    .children(items.iter().map(|item| {
        let id = item.id;
        let title = item.title.clone();
        ListItem::new(title)
            .on_tap(move |_, ctx| ctx.send_intent(AppIntent::Select(id)))
    }))
```

The loop body is a sequence of `let` bindings followed by a single element. The `let` bindings exist to narrow captures to owned values (`let id = item.id;` copies the id out so the `move` closure does not try to capture `&item`). The macro does not inject these bindings automatically.

For dynamic item collections backed by `ListModel<T>`, use the `ListView` widget directly. The `for` form is for static iteration at build time, not reactive item lists.

### 5.3 `match` Forms

```rust
VStack {
    match state {
        State::Loading => Spinner(),
        State::Loaded(data) => DataView(data.clone()),
        State::Error(msg) => ErrorBanner(msg.clone()),
    }
}

// Desugars to
VStack::new()
    .child(match state {
        State::Loading => BatiBranch3::A(Spinner::new()),
        State::Loaded(data) => BatiBranch3::B(DataView::new(data.clone())),
        State::Error(msg) => BatiBranch3::C(ErrorBanner::new(msg.clone())),
    })
```

### 5.4 `let` Forms

A `let` binding at body position introduces a computed value used by subsequent elements.

```rust
VStack {
    let heading_style = t.body_bold.clone();
    let accent = c.accent;
    TextWidget("Title") { style: heading_style.clone(), color: accent }
    TextWidget("Body")  { style: t.body.clone(),        color: accent }
}
```

When a body contains `let` bindings, the desugaring switches from a pure builder chain to a statement sequence. This desugaring also applies to body-position bindings (§3.3), spread forms (§5.5), and pure-side-effect `rust` blocks (§5.6). A body containing only properties and child elements continues to use the pure chain form for readability.

### 5.5 Spread Forms

A spread `..expr` inlines a `Vec<WidgetId>` or an iterator of widgets as children at that position.

```rust
VStack {
    TextWidget("Header")
    ..plugin_widgets
    TextWidget("Footer")
}

// Desugars to
{
    let mut __vstack = VStack::new();
    __vstack = __vstack.child(TextWidget::new("Header"));
    for __id in plugin_widgets {
        __vstack = __vstack.add_child(__id);
    }
    __vstack = __vstack.child(TextWidget::new("Footer"));
    __vstack
}
```

Spread is for programmatic child list assembly (plugin registries, restored workspaces, tab managers).

### 5.6 `rust` Forms

A `rust { ... }` block switches to imperative construction. Two shapes, distinguished by whether the block produces a value.

**Expression-producing form.** The block ends with an expression without a trailing semicolon. The value is used as a child or spread across children via the `IntoBatiChild` trait.

```rust
VStack {
    TextWidget("Header")
    rust {
        let mut items = Vec::new();
        for ch in chapters.iter() {
            if ch.visible {
                items.push(ctx.add(ChapterRow::new(ch.clone())));
            }
        }
        items
    }
    TextWidget("Footer")
}
```

**Side-effect form.** The block's last statement ends with `;` (unit value). The block runs for its side effects and produces no children. Multiple `;`-terminated statements are allowed.

```rust
VStack {
    rust {
        let item_label = self.item_label.clone();
        let app_ctx = self.app_context.clone();
        ctx.subscribe_event(
            Origin::DirectAccess(DirectAccessEntity::Item(EntityEvent::Created)),
            move |event: &Event| {
                if let Some(id) = event.ids.first() {
                    if let Ok(Some(dto)) = item_commands::get_item(&app_ctx, id) {
                        item_label.set(
                            tr!(created_info(title = dto.title, id = dto.id)).resolve_now(),
                        );
                    }
                }
            },
        );
    }
    TextWidget("") {
        text: self.item_label.clone()
    }
}
```

**Disambiguation is mechanical.** The macro looks at the last statement in the `rust` block. If it ends without `;`, the block is expression-producing. If it ends with `;`, the block is side-effect.

**Failure mode for forgotten `;`.** If the user writes a side-effect block without a trailing `;`, and the tail has type `()` (for example, a bare `if let { ...; }` without `else`), the macro treats it as expression form and tries to dispatch `()` through `IntoBatiChild`. The compiler responds with "the trait `IntoBatiChild` is not implemented for `()`" pointing at the block's tail expression. This is a survivable error: the message is clear and the fix is to add the missing `;`.

---

## 6. Escape Hatches

One escape into host Rust, in addition to the `rust { }` block.

### 6.1 Expression Escape: `#{ expr }`

Anywhere a child element or property value is expected, `#{ expr }` takes a Rust expression and inserts its value at that position. If the expression evaluates to a `WidgetId`, the child position emits `.add_child(id)` instead of `.child(widget)`, and slot positions emit `.slot_id(id)` instead of `.slot(widget)`. Dispatch is through the `IntoBatiChild` blanket trait.

```rust
// Inserting a pre-built widget as a child
VStack {
    TextWidget("Header")
    #{ build_complex_subtree(ctx, config) }
    TextWidget("Footer")
}

// Re-using a bound id in a slot
bati!(ctx =>
    VStack {
        title = TextWidget("Manuscript") { style: bold }
        Card {
            header: #{ title }
            content: TextWidget("Body")
        }
    }
)
```

The second case uses `title` (declared with `name = Element` binding) via `#{ title }` in a Category B slot. The escape pulls the `WidgetId` into the slot position; the macro routes it through `.header_id(title)` automatically.

For bare identifiers at property-value positions, `#{ }` is not required: `text: selected_label` parses as a property with a Rust expression value. The escape is only needed where the parser would otherwise try to interpret the value as something else (an element, a structural form).

---

## 7. Worked Translations

Each translation takes a block from one of the uploaded example files and shows the `bati!` equivalent against the actual post-refactor constructor and method names. Translations assume Appendix A has been applied.

### 7.1 simple-button

Source:

```rust
.root(|tree| {
    tree.add(
        Button::new(lit!("Click Me"))
            .style(ButtonVariant::Default)
            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::ButtonClicked))
            .tooltip(lit!("This is a simple button. Click it to see a message in the console.")),
    )
})
```

With `bati!`:

```rust
.root(|tree| bati!(tree =>
    Button::new(lit!("Click Me")) {
        style: ButtonVariant::Default
        on_activate_fn: |ctx| ctx.send_intent(AppIntent::ButtonClicked)
        tooltip_literal: "This is a simple button. Click it to see a message in the console."
    }
))
```

The explicit `::new_literal` names the constructor. The `tooltip_literal` property matches the real method name. Four lines instead of six, property assignments read as assignments.

### 7.2 text-and-layout, outer Padding and VStack

Source:

```rust
let root = ctx.add(
    Padding::uniform(24.0).child(
        VStack::new()
            .spacing(20.0)
            .child(
                HStack::new()
                    .child(
                        TextWidget::new(lit!("Text & Layout"))
                            .style(t.body_bold.clone())
                            .color(c.text_primary),
                    )
                    .child(Spacer::new())
                    .child(
                        Button::new(lit!("Toggle Dark Mode"))
                            .style(ButtonVariant::Regular)
                            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::ToggleDarkMode)),
                    ),
            ),
    ),
);
```

With `bati!`:

```rust
let root = bati!(ctx =>
    Padding::uniform(24.0) {
        VStack {
            spacing: 20.0
            HStack {
                TextWidget::new(lit!("Text & Layout")) {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                Spacer
                Button::new(lit!("Toggle Dark Mode")) {
                    style: ButtonVariant::Regular
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::ToggleDarkMode)
                }
            }
        }
    }
);
```

All `.child(...)` wrappers collapse. Siblings land at equal depth. `::uniform` and `::new_literal` appear where the builder uses them.

### 7.3 text-and-layout, build_color_box helper

Source:

```rust
fn build_color_box(color: Color, label: &str) -> Panel {
    Panel::new()
        .background(color)
        .corner_radius(6.0)
        .padding(8.0)
        .child(
            TextWidget::new(lit!(label))
                .style(TextStyle {
                    family: "sans-serif".into(),
                    size: 14.0,
                    weight: FontWeight::BOLD,
                    line_height: 1.4,
                    letter_spacing: 0.0,
                })
                .color(Color::WHITE),
        )
}
```

With `bati!`:

```rust
fn build_color_box(color: Color, label: &str) -> impl Widget {
    bati!(
        Panel {
            background: color
            corner_radius: 6.0
            padding: 8.0
            TextWidget::new(lit!(label)) {
                style: TextStyle {
                    family: "sans-serif".into(),
                    size: 14.0,
                    weight: FontWeight::BOLD,
                    line_height: 1.4,
                    letter_spacing: 0.0,
                }
                color: Color::WHITE
            }
        }
    )
}
```

The return type changes from `Panel` to `impl Widget` because the macro's output is opaque. The `TextStyle { ... }` struct literal has internal commas grouped by braces; the bracket-aware parser keeps them as a single argument to `.style()`.

### 7.4 title-bar-demo, multi-argument properties and slots

Source:

```rust
TitleBar::new(host)
    .height(40.0)
    .background(theme.colors.surface_pressed)
    .border(theme.colors.text_secondary, 2.0)
    .leading(
        TextWidget::new(lit!("  Bastyde — Title Bar Demo"))
            .style(theme.typography.body_bold.clone())
            .color(theme.colors.text_primary),
    )
    .center(
        TextWidget::new(lit!("drag · double-click maximize · right-click for menu  "))
            .style(theme.typography.small.clone())
            .color(theme.colors.text_secondary),
    )
    .close_action(|ctx| ctx.close_window())
```

With `bati!`:

```rust
bati!(
    TitleBar(host) {
        height: 40.0
        background: theme.colors.surface_pressed
        border: theme.colors.text_secondary, 2.0
        leading: TextWidget::new(lit!("  Bastyde — Title Bar Demo")) {
            style: theme.typography.body_bold.clone()
            color: theme.colors.text_primary
        }
        center: TextWidget::new(lit!("drag · double-click maximize · right-click for menu  ")) {
            style: theme.typography.small.clone()
            color: theme.colors.text_secondary
        }
        close_action: |ctx| ctx.close_window()
    }
)
```

Three things to note. First, `border: color, width` is the multi-argument property form. Second, the em-dash in `"Bastyde — Title Bar Demo"` and the middle dots in `"drag · double-click maximize · right-click for menu  "` are preserved verbatim from the source. Third, `leading:` and `center:` are Category B slot values, written as full nested elements.

### 7.5 tab-widget, full TabWidget with multi-arg element values

Source (abbreviated):

```rust
let selected = ctx.signal(0_usize);
let selected_label = selected.map(|index| match *index {
    0 => "Overview".to_string(),
    1 => "Inspector".to_string(),
    _ => "Activity".to_string(),
});

let trailing = HStack::new()
    .spacing(12.0)
    .child(
        TextWidget::new(lit!(""))
            .text(selected_label)
            .style(theme.typography.small.clone()),
    )
    .child(
        Button::new(lit!("Toggle Theme"))
            .style(ButtonVariant::Flat)
            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::ToggleTheme)),
    );

let tabs = ctx.add(
    TabWidget::new(selected)
        .tab(lit!("Overview"), Card::new()
            .header(TextWidget::new(lit!("Overview"))
                .style(theme.typography.body_bold.clone())
                .color(theme.colors.text_primary))
            .content(VStack::new().spacing(12.0)...))
        .tab(lit!("Inspector"), Panel::new().padding(20.0)...)
        .tab(lit!("Activity"), Panel::new().padding(20.0)...)
        .tab_item(TabItem::new(lit!("Disabled"), Panel::new()...).enabled(false))
        .trailing_slot(trailing),
);
```

With `bati!`:

```rust
let selected = ctx.signal(0_usize);
let selected_label = selected.map(|index| match *index {
    0 => "Overview".to_string(),
    1 => "Inspector".to_string(),
    _ => "Activity".to_string(),
});

let trailing = bati!(
    HStack {
        spacing: 12.0
        TextWidget::new(lit!("")) {
            text: selected_label
            style: theme.typography.small.clone()
        }
        Button::new(lit!("Toggle Theme")) {
            style: ButtonVariant::Flat
            on_activate_fn: |ctx| ctx.send_intent(AppIntent::ToggleTheme)
        }
    }
);

let tabs = bati!(ctx =>
    TabWidget(selected) {
        tab: lit!("Overview"), Card {
            header: TextWidget::new(lit!("Overview")) {
                style: theme.typography.body_bold.clone()
                color: theme.colors.text_primary
            }
            content: VStack {
                spacing: 12.0
                TextWidget::new(lit!("This first Milestone 6 slice ships a real TabWidget..."))
                HStack {
                    spacing: 8.0
                    Badge::new(lit!("Dormant Panes"))
                    Badge::new(lit!("Arrow Navigation"))
                    Badge::new(lit!("Trailing Slot"))
                }
            }
        }
        tab: lit!("Inspector"), Panel {
            padding: 20.0
            VStack {
                spacing: 10.0
                TextWidget::new(lit!("Inspector")) { style: theme.typography.body_bold.clone() }
                TextWidget::new(lit!("Use Tab to move focus..."))
            }
        }
        tab: lit!("Activity"), Panel {
            padding: 20.0
            VStack { spacing: 10.0, ... }
        }
        tab_item: TabItem::new(lit!("Disabled"), Panel {
            padding: 20.0
            TextWidget::new(lit!("Disabled tabs are visible but cannot be activated."))
        }) { enabled: false }
        trailing_slot: trailing
    }
);
```

The `tab: lit!("name"), Card { ... }` pattern is the multi-argument property form with an element-valued second argument (`tab` is `TabWidget`'s title-only shorthand for `static_tab(TabInfo::new().title(label), content)`). The `tab_item:` property takes a full `TabItem` element with its own body (`enabled: false` is a property on `TabItem`). `trailing_slot:` takes the previously-built `trailing` widget. Signals (`selected`, `selected_label`) stay as regular Rust `let` bindings because they are computed values, not widgets.

### 7.6 overlay-demo, Dialog / Popover / Snackbar (post-refactor)

Source with post-refactor API:

```rust
let modal_trigger_id = ctx.add(
    Dialog::new(lit!("Adaptive modal window"))
        .content(move || {
            DialogContent::new()
                .title(lit!("Adaptive modal dialog"))
                .supporting_text(lit!("The framework chooses the best modal presentation..."))
                .body(TextWidget::new(lit!("The app code does not branch...")))
                .footer(Button::new(lit!("Close")).on_tap(|_, ctx| ctx.dismiss_modal()))
        })
        .style(ButtonVariant::Regular),
);

let popover = Popover::new(lit!("Show popover"))
    .content(popover_content)
    .caret_size(12.0)
    .trigger(popover_trigger);

let snackbar = Snackbar::new(lit!("Show snackbar"))
    .content(snackbar_content)
    .auto_dismiss_after(Duration::from_millis(2500));
```

With `bati!`:

```rust
let root = bati!(ctx =>
    ScrollArea {
        widget_resizable: true
        VStack {
            spacing: 24.0
            TextWidget::new(lit!("Dialogs and Popovers")) {
                style: t.body_bold.clone()
                color: c.text_primary
            }
            TextWidget::new(lit!("Bastyde now resolves dialogs through a shared modal presentation pipeline, alongside anchored popovers and timed snackbars.")) {
                style: t.body.clone()
                color: c.text_secondary
            }
            Panel {
                padding: 20.0
                HStack {
                    spacing: 16.0
                    Popover::new(lit!("Show popover")) {
                        content: VStack {
                            spacing: 12.0
                            TextWidget::new(lit!("Popover")) { style: t.small.clone() }
                            TextWidget::new(lit!("Use popovers for compact contextual actions without leaving the current surface.")) {
                                style: t.body.clone()
                                color: c.text_secondary
                            }
                            HStack {
                                spacing: 8.0
                                Badge::new(lit!("Quick actions"))
                                Badge::new(lit!("Inline help"))
                                Badge::new(lit!("Inspector"))
                            }
                        }
                        caret_size: 12.0
                        trigger: Panel {
                            padding: 12.0
                            HStack {
                                spacing: 10.0
                                Badge::new(lit!("Context"))
                                TextWidget::new(lit!("Popover actions")) {
                                    style: t.small.clone()
                                }
                            }
                        }
                    }
                    modal_trigger = Dialog::new(lit!("Adaptive modal window")) {
                        content: move || bati!(
                            DialogContent {
                                title_literal: "Adaptive modal dialog"
                                supporting_text_literal: "The framework chooses the best modal presentation for the current backend: a native modal child window when reliable, otherwise a centered in-tree dialog."
                                body: TextWidget::new(lit!("The app code does not branch on Wayland or window-system support here; it issues one modal request and lets Bastyde resolve it.")) {
                                    style: t.body.clone()
                                    color: c.text_secondary
                                }
                                footer: Button::new(lit!("Close")) {
                                    style: ButtonVariant::Default
                                    on_tap: |_, ctx| ctx.dismiss_modal()
                                }
                            }
                        )
                        style: ButtonVariant::Regular
                    }
                    Snackbar::new(lit!("Show snackbar")) {
                        content: HStack {
                            spacing: 14.0
                            TextWidget::new(lit!("Autosave complete")) {
                                style: t.body.clone()
                                color: c.tooltip_text
                            }
                            Button::new(lit!("Dismiss")) {
                                style: ButtonVariant::Regular
                                on_tap: |_, ctx| ctx.dismiss_top_overlay()
                            }
                        }
                        auto_dismiss_after: Duration::from_millis(2500)
                    }
                }
            }
            // ... additional Notes panel ...
        }
    }
);
```

Five things exercise the language here. First, `ScrollArea` is Category A post-refactor, so its VStack content is a body-block child. Second, the Dialog binding `modal_trigger =` uses the new assignment form to bind the dialog's id. Third, Dialog's `content:` property takes a `move ||` factory closure whose body contains a nested `bati!(...)` building the DialogContent. Fourth, DialogContent is Category B with `title_literal`, `supporting_text_literal`, `body`, and `footer` as slot properties. Fifth, `trigger:` in Popover takes a full element value, and all three trigger-like widgets (Popover, Snackbar, and Dialog as a button itself) appear at the same depth in the HStack.

### 7.7 internationalization, mixed declarative and imperative

Source:

```rust
let direction_signal = bastyde::i18n::current_direction();
let direction_label = ctx.signal(direction_note_label_for(direction_signal.as_ref()));
if let Some(sig) = direction_signal.as_ref() {
    let target = direction_label.clone();
    ctx.effect(sig, move |dir| {
        target.set(direction_note_label(*dir));
    });
}

let heading = ctx.add(
    TextWidget::new(tr!(heading()))
        .style(theme.typography.body_bold.clone())
        .color(theme.colors.text_primary),
);
// ... more let-adds ...
```

With `bati!`:

```rust
let root = bati!(ctx =>
    Panel {
        padding: 24.0
        VStack {
            spacing: 16.0

            let direction_signal = bastyde::i18n::current_direction();
            let direction_label = ctx.signal(
                direction_note_label_for(direction_signal.as_ref())
            );
            rust {
                if let Some(sig) = direction_signal.as_ref() {
                    let target = direction_label.clone();
                    ctx.effect(sig, move |dir| {
                        target.set(direction_note_label(*dir));
                    });
                };
            }

            TextWidget(tr!(heading())) {
                style: theme.typography.body_bold.clone()
                color: theme.colors.text_primary
            }
            TextWidget(tr!(greeting(name = name))) {
                style: theme.typography.body_bold.clone()
                color: theme.colors.text_primary
            }
            TextWidget(tr!(body_paragraph())) {
                style: theme.typography.body.clone()
                color: theme.colors.text_primary
            }
            TextWidget::new(lit!("")) {
                text: direction_label
                style: theme.typography.small.clone()
                color: theme.colors.text_secondary
            }
            HStack {
                spacing: 8.0
                TextWidget(tr!(language_label())) {
                    style: theme.typography.body_bold.clone()
                    color: theme.colors.text_primary
                }
                Button(tr!(lang_english())) {
                    style: ButtonVariant::Regular
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::SetEnglish)
                }
                Button(tr!(lang_french())) {
                    style: ButtonVariant::Regular
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::SetFrench)
                }
                Button(tr!(lang_arabic())) {
                    style: ButtonVariant::Regular
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::SetArabic)
                }
            }
            HStack {
                spacing: 12.0
                Button(tr!(leading_button())) { style: ButtonVariant::Regular }
                Button(tr!(trailing_button())) { style: ButtonVariant::Regular }
            }
        }
    }
);
```

All the hoisted `let id = ctx.add(...)` in the source collapse into declarative elements. The conditional `ctx.effect` registration goes in a side-effect `rust { }` block with a `;` on its tail. The `let` bindings for signal handles use the `let` form at body position, scoping the signals to the VStack construction. `TextWidget(tr!(...))` uses the default `::new` constructor (localized); `TextWidget::new(lit!(""))` uses the literal constructor where the source does.

### 7.8 widget-catalog, event subscription in rust block

Source (abbreviated):

```rust
impl Widget for App {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        {
            let item_label = self.item_label.clone();
            let app_ctx_sub = self.app_context.clone();
            ctx.subscribe_event(
                Origin::DirectAccess(DirectAccessEntity::Item(EntityEvent::Created)),
                move |event: &Event| {
                    if let Some(id) = event.ids.first() {
                        if let Ok(Some(dto)) = item_commands::get_item(&app_ctx_sub, id) {
                            item_label.set(
                                tr!(created_info(title = dto.title, id = dto.id)).resolve_now(),
                            );
                        }
                    }
                },
            );
        }

        let write_signal = self.write_signal.clone();
        let label = self.write_signal.map(|text| format!("BastydeApp Widget Catalog {}", text));
        let item_label_for_bind = self.item_label.clone();
        let item_label_for_handler = self.item_label.clone();
        let app_ctx = self.app_context.clone();

        let root = ctx.add(
            VStack::new()
                .child(
                    TextWidget::new(tr!(title()))
                        .text(label)
                        .style(t.body.clone())
                        .color(c.text_primary),
                )
                .child(
                    Button::new(tr!(write_something_button()))
                        .on_activate_fn(move |_| {
                            write_signal.set("Hello from the button!".to_string());
                        })
                        .style(ButtonVariant::Default),
                )
                // ... more children ...
                .child(Expand::new().fills_stack())
                .child(
                    StatusBar::new().child(
                        TextWidget::new(tr!(milestone_status()))
                            .style(t.tiny.clone())
                            .color(c.text_secondary),
                    ),
                ),
        );
        self.root_child_id = Some(root);
        vec![root]
    }
}
```

With `bati!`:

```rust
impl Widget for App {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        let root = bati!(ctx =>
            VStack {
                let write_signal = self.write_signal.clone();
                let label = self.write_signal.map(|text| format!("BastydeApp Widget Catalog {}", text));
                let item_label_for_bind = self.item_label.clone();
                let item_label_for_handler = self.item_label.clone();
                let app_ctx = self.app_context.clone();

                rust {
                    let item_label = self.item_label.clone();
                    let app_ctx_sub = self.app_context.clone();
                    ctx.subscribe_event(
                        Origin::DirectAccess(DirectAccessEntity::Item(EntityEvent::Created)),
                        move |event: &Event| {
                            if let Some(id) = event.ids.first() {
                                if let Ok(Some(dto)) = item_commands::get_item(&app_ctx_sub, id) {
                                    item_label.set(
                                        tr!(created_info(title = dto.title, id = dto.id))
                                            .resolve_now(),
                                    );
                                }
                            }
                        },
                    );
                }

                TextWidget(tr!(title())) {
                    text: label
                    style: t.body.clone()
                    color: c.text_primary
                }
                Button(tr!(write_something_button())) {
                    style: ButtonVariant::Default
                    on_activate_fn: move |_| {
                        write_signal.set("Hello from the button!".to_string());
                    }
                }
                Button(tr!(create_item_locally_button())) {
                    on_activate_fn: move |_| {
                        let result = create_orphan_item(
                            &app_ctx,
                            None,
                            &CreateItemDto {
                                title: "Local Item".to_string(),
                                ..Default::default()
                            },
                        );
                        if let Ok(item) = result {
                            item_label_for_handler.set(
                                format!("Got: {} (id={})", item.title, item.id)
                            );
                        }
                    }
                }
                Button(tr!(add_item_appcommand_button())) {
                    style: ButtonVariant::Default
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::AddItem)
                }
                TextWidget(tr!(add_item_label())) {
                    text: item_label_for_bind
                    style: t.body.clone()
                    color: c.text_primary
                }
                Button(tr!(toggle_dark_mode_button())) {
                    style: ButtonVariant::Regular
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::ToggleDarkMode)
                }
                Expand { fills_stack }
                StatusBar {
                    TextWidget(tr!(milestone_status())) {
                        style: t.tiny.clone()
                        color: c.text_secondary
                    }
                }
            }
        );
        self.root_child_id = Some(root);
        vec![root]
    }
}
```

The `let` forms at body position handle the signal cloning. The `rust { }` side-effect block registers the event subscription. `Expand { fills_stack }` uses the bare-lowercase-identifier rule for argument-free properties. StatusBar is Category A, so its child is a bare element.

### 7.9 Card, Category B with bound slot widget

An illustration of the `name = Element` binding used in slot position:

```rust
// Builder
let title_id = ctx.add(
    TextWidget::new("Manuscript Title")
        .style(t.body_bold.clone())
        .color(c.text_primary),
);

let card = Card::new()
    .header_id(title_id)
    .content(
        VStack::new()
            .spacing(12.0)
            .child(TextWidget::new("Edit title:"))
            .child(
                Button::new("Focus title")
                    .on_tap(move |_, ctx| ctx.focus(title_id)),
            ),
    )
    .footer(Button::new("Save").on_activate_fn(|ctx| ctx.send_intent(AppIntent::SaveTitle)))
    .padding(16.0);
```

With `bati!`:

```rust
bati!(
    Card {
        header: title = TextWidget("Manuscript Title") {
            style: t.body_bold.clone()
            color: c.text_primary
        }
        content: VStack {
            spacing: 12.0
            TextWidget("Edit title:")
            Button("Focus title") {
                on_tap: move |_, ctx| ctx.focus(title)
            }
        }
        footer: Button("Save") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::SaveTitle) }
        padding: 16.0
    }
)
```

`title =` binds the TextWidget's id at the slot position; the id is available anywhere in the enclosing block, including the `on_tap` closure in the content slot's Button. The macro emits `ctx.add(TextWidget::new(...)...)` as a hoisted statement, then uses `.header_id(title)` on the Card. The on_tap handler captures `title` by value through `move`, which is what the user wrote.

---

## 8. Handler Attachment Rules

The V2 model splits handlers between two attachment patterns (architecture §28.3): handlers on child widgets (Checkbox on MinSize, Accordion on its header) and handlers attached to `self` via `HandlerSet::new()` + `ctx.apply_self_handlers()` (Button, Toggle, Slider, SegmentedControl).

`bati!` does not change this. Handlers written on an element attach via the builder methods of that element. Which attachment mechanism the builder uses internally is a per-widget implementation detail.

For the rarer case of attaching handlers to `self` inside a widget's own `build()` method, `bati!` is not the tool. That is infrastructure code that uses `HandlerSet` and `ctx.apply_self_handlers()` directly. `bati!` is for constructing trees, not for authoring internals.

---

## 9. Error Reporting Discipline

Every span the macro emits must be traceable to a user token. The `tr!` macro established the precedent: a missing translation key produces an error pointing at the key identifier. `bati!` adheres to the same discipline.

### 9.1 Span Mapping Rules

Type errors on widget constructors point at the type path. `Buton::new("x") { ... }` fails with `cannot find type 'Buton'` under the `Buton` identifier.

Type errors on property values point at the value expression. `TextWidget("x") { color: "red" }` fails with `expected Color, found &str` under `"red"`.

Method-not-found errors on properties point at the property name, via the compiler's existing method-resolution diagnostics.

Arity mismatches on handler closures point at the closure parameter list.

Structural form errors (`if` without a valid block, `for` without `in`) point at the structural keyword.

Parsing errors where an element prefix matched but the element failed to parse fully point at the token where parsing went wrong.

### 9.2 Common Errors

```
error: bindings use `=`, not `:`
   --> src/main.rs:15:16
    |
15  |     Button("x") id: my_btn { }
    |                ^^
    |
    = help: use `my_btn = Button("x") { }` instead

error: expected property, binding, or child element, found `,`
   --> src/main.rs:20:29
    |
20  |     VStack { spacing: 8.0, TextWidget("x") }
    |                          ^
    |
    = help: bati! blocks separate items by newlines, not commas

error: no method named `child` found on type `Card`
   --> src/main.rs:25:5
    |
25  |     Card { TextWidget("hi") }
    |            ^^^^^^^^^^^^^^^^
    |
    = note: Card is a Category B widget with named slots (header, content, footer)
    = help: use `content: TextWidget("hi")` to set the content slot
```

The last message is aspirational: the macro can detect bare-child usage in Category B contexts by maintaining a list of known `.child()`-having types and emitting a helpful diagnostic. This is worth the bookkeeping because it saves users from a generic method-resolution error on `.child()` that does not say why.

---

## 10. What `bati!` Does Not Do

**Implicit theme access.** Every reference to theme tokens is an explicit Rust path. Implicit access would require a thread-local (fights multi-window) or an injected ctx parameter (breaks error messages). Mitigation: a small `themed!` helper macro that expands to `let t = &theme.typography; let c = &theme.colors;`.

**Implicit reactive bindings.** `text: model.title` passes the value once. To get reactivity, write `text: signal.map(...)`. This matches `Prop<T>`.

**Automatic animation syntax.** Users call `signal.animate_to(target, duration, easing)` in regular Rust.

**Hot-reload.** `bati!` expansions are Rust code. No runtime parser, no structural hot-reload. Translation hot-reload works through `--translation-dev`.

**Inline doc comments on elements.** Users put doc comments on helper functions or use regular Rust comments inside the block. Worth revisiting later.

**Two-way binding syntax.** Two-way binding in Bastyde is expressed by passing a `Signal<T>` to a widget's bind method; the widget commits changes back through its event handlers. No `:=` form.

**Implicit closure capture.** `move` stays explicit. Rust users know the keyword; eliding it produces confusing errors.

---

## 11. Implementation Notes

The macro lives in a new crate `bastyde-macros`, exported through the `bastyde` umbrella as `bastyde::bati!`. Four responsibilities.

**Lexical parsing** uses `syn` and a hand-written recursive-descent parser for the body grammar. `syn` handles positional-argument paren groups, body braces, and embedded Rust expressions. The "commit on distinctive prefix" rule is a fixed two-token lookahead, no backtracking.

**IR construction** produces a typed tree: `BatiElement`, `BatiProperty`, `BatiBinding`, `BatiStructural`, `BatiSpread`, `BatiLet`, `BatiEscape`, `BatiRust`. One IR node per grammar production.

**Translation** walks the IR and emits `quote!`-generated builder calls, preserving spans via `quote_spanned!`.

**Diagnostic emission.** Statically detectable errors (malformed grammar, `id:` used instead of `=`, `name:` with no arguments, bare child in Category B context) emit `compile_error!` with clean spans. Type errors emit clean builder calls and let the compiler's native diagnostics surface.

**Supporting types.** `BatiBranch<L, R>`, `BatiBranch3`, `BatiBranch4`, `IntoBatiChild` live in `bastyde-core::widget_builder` as public types. They are not DSL-specific: hand-written builder chains can use them.

**Bootstrapping.** Develop, test, and land the macro after the framework changes in Appendix A. The `tr!` macro's infrastructure (crate layout, `trybuild` tests, span discipline, rebuild tracking) is the template. Estimated cost: four to six weeks including `trybuild` corpus and documentation rewrite.

**Test strategy.** `trybuild` fixtures for every error class in §9. Golden-file tests for translation rules using `cargo expand`. Integration tests rewriting the seven uploaded examples to `bati!` form and verifying rendered output is bitwise identical.

---

## 12. Summary

The `bati!` language is a block-structured DSL for Bastyde widget trees. It reads like QML or Kotlin, compiles to V2 builder calls with no runtime overhead, preserves existing reactivity and capture semantics without new syntax, and produces user-facing error spans.

The grammar has three primary forms: elements with explicit constructors, bindings via `name = Element`, and properties including named slots; structural control flow (`if`, `for`, `match`, `let`, `..spread`, `rust { }`); and one escape hatch (`#{ expr }`). Each form has a mechanical desugaring into existing Bastyde infrastructure.

The widget catalog divides into two categories: Category A containers accepting body-block children, and Category B composites with named slot properties. Appendix A specifies the framework changes that complete this split.

---

## Appendix A: Required Framework Changes

The DSL assumes these framework changes are applied. Each is mechanical and takes roughly an hour; all together, an afternoon.

### A.1 Category C Dissolution

Move primary content from constructor argument to setter method on four widgets.

**ScrollArea:**
```
- pub fn new(child: impl Widget + 'static) -> Self
+ pub fn new() -> Self
+ pub fn child(mut self, child: impl Widget + 'static) -> Self
```
Joins Category A. `.from_id(id)` stays as the id-taking alternate constructor.

**Popover:**
```
- pub fn new(label: impl Into<LocalizedString>, content: impl Widget + 'static) -> Self
+ pub fn new(label: impl Into<LocalizedString>) -> Self
+ pub fn content(mut self, content: impl Widget + 'static) -> Self
```
Joins Category B.

**Snackbar:**
```
- pub fn new(label: impl Into<LocalizedString>, content: impl Widget + 'static) -> Self
+ pub fn new(label: impl Into<LocalizedString>) -> Self
+ pub fn content(mut self, content: impl Widget + 'static) -> Self
```
Joins Category B.

**Dialog:**
```
- pub fn new<W, F>(label: impl Into<LocalizedString>, factory: F) -> Self
-     where W: Widget + 'static, F: Fn() -> W + 'static
+ pub fn new(label: impl Into<LocalizedString>) -> Self
+ pub fn content<W, F>(mut self, factory: F) -> Self
+     where W: Widget + 'static, F: Fn() -> W + 'static
```
Joins Category B. `.content()` takes the factory closure (the lazy construction semantics are preserved).

### A.2 Rename `.set_*` Methods

The `.set_*` prefix convention becomes `*_id` universally, matching SplitView's existing `.first_id` / `.second_id`.

**Rename:**
- Panel: `.set_child(id)` → `.child_id(id)`
- Padding: `.set_child(id)` → `.child_id(id)`
- Expand: `.set_child(id)` → `.child_id(id)`
- GroupBox: `.set_child(id)` → `.child_id(id)`
- Accordion: `.set_content(id)` → `.content_id(id)`

### A.3 New Id-Taking Twins

Every Category B slot method gains an `*_id` twin.

**Card:**
```rust
pub fn header_id(mut self, id: WidgetId) -> Self
pub fn content_id(mut self, id: WidgetId) -> Self
pub fn footer_id(mut self, id: WidgetId) -> Self
```

**TitleBar:**
```rust
pub fn leading_id(mut self, id: WidgetId) -> Self
pub fn center_id(mut self, id: WidgetId) -> Self
pub fn trailing_id(mut self, id: WidgetId) -> Self
```

**DialogContent:**
```rust
pub fn body_id(mut self, id: WidgetId) -> Self
pub fn footer_id(mut self, id: WidgetId) -> Self
```

**Breadcrumb:**
```rust
pub fn item_id(mut self, id: WidgetId) -> Self
pub fn trailing_slot_id(mut self, id: WidgetId) -> Self
```

**TabWidget:**
```rust
pub fn tab_id(mut self, label: impl Into<LocalizedString>, id: WidgetId) -> Self
pub fn tab_item_id(mut self, item: TabItem) -> Self  // TabItem carries the id internally
pub fn trailing_slot_id(mut self, id: WidgetId) -> Self
```

**Popover:**
```rust
pub fn content_id(mut self, id: WidgetId) -> Self
pub fn trigger_id(mut self, id: WidgetId) -> Self
```

**Snackbar:**
```rust
pub fn content_id(mut self, id: WidgetId) -> Self
pub fn trigger_id(mut self, id: WidgetId) -> Self
```

**Dialog:**
```rust
pub fn trigger_id(mut self, id: WidgetId) -> Self
// No content_id; the factory closure can use ctx.add and from_id internally if needed.
```

### A.4 BatiBranch Types and IntoBatiChild Trait

Add to `bastyde-core::widget_builder`:

```rust
pub enum BatiBranch<L: Widget, R: Widget> { L(L), R(R) }
pub enum BatiBranch3<A: Widget, B: Widget, C: Widget> { A(A), B(B), C(C) }
pub enum BatiBranch4<A, B, C, D> { ... }

// Widget impl for each variant dispatches to the active arm.

pub trait IntoBatiChild { ... }
// Blanket impls for impl Widget and WidgetId.
// Used by child(), add_child(), slot_id() routing.
```

### A.5 Summary of Effort

Category C dissolution: 4 widgets, roughly 20 lines of change each. Method renames: 5 widgets, roughly 3 lines each. Id-taking twins: 8 widgets, roughly 30 new methods total at 3 lines each. BatiBranch infrastructure: 1 new file, roughly 200 lines including the impls.

Total: an afternoon of mechanical work, plus test updates. The seven uploaded example files need migration to the new API; each example is a dozen lines of change on average.

---

## Appendix B: Known Open Questions

One question carried over from v2's appendix.

Should `let` bindings inside a DSL body ever produce `let mut`, or always `let`? Current answer: always `let`. A user needing a `mut` binding writes a `rust { }` block.

No other open questions remain from prior drafts. Questions about the DSL's semantics that arise during implementation should be resolved against this spec or against the architecture document, with a changelog entry here if either changes.
