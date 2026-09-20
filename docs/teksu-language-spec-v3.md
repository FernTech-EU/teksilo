<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# The `teksu!` Language Specification (v3)

**Status:** Design rationale, revision 3, reconciled with the shipped implementation
**Date:** April 17, 2026, reconciled September 15, 2026
**Companion to:** architecture.md §28.9
**Supersedes:** teksu-language-spec-v2.md

---

## Status: what this document is for

[teksu-macro-reference.md](teksu-macro-reference.md) is **normative for behaviour**. When the
two documents disagree about what the macro does, the reference is right and this file is
stale; where neither settles it, the source does, and the section below names the file and line
for each rule.

This document is the **design rationale**: why the grammar has the shape it has, what the two
widget categories are for, what was deliberately left out, and what the framework had to change
to make the DSL expressible. It is also the historical record of a design written before the
implementation, which did not survive contact with it in every particular.

The body text below has been corrected so that no section asserts something the implementation
does not do. Where a v3 promise was never implemented, the section now says so and points at
what shipped instead, rather than being deleted: the promise is part of the rationale, and any
future v4 discussion needs to see it. For where that discussion stands, see **Why there is no
v4** immediately below.

---

## Why there is no v4

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

A September 2026 review asked whether the grammar should be revised. The answer was that v3 is
close enough to good enough not to start a v4, but not because the DSL was fine: it shipped
three silent-wrong-program bugs and one grammar rule that demonstrably shaped how the flagship
application was written. Those were fixed instead. The evidence is recorded here because the
standalone write-up it came from has been folded into this file, and because a future v4
discussion should not have to re-derive it. Method: eight parallel corpus and prior-art
investigations, six independent design positions, one adversarial verifier per position, plus
first-hand probes compiled in a worktree. Every number below was reproduced by at least two
independent runs.

### What was measured

`teksu!` reaches **0.85 %** of Skribisto's UI crate (1,972 of 235,110 lines), **1.48 %** of its
widget-building code, **4 of 55** Teksilo examples, and **zero** of the framework's own ~100
widgets. 88 blocks in Skribisto and 61 in Skribisto-Pro; the largest is 81 lines. Twenty-seven
of the 115 `teksu!` mentions in Skribisto are comments explaining why it was *not* used. The
August 2026 migration campaign converted six files, and the author then wrote the next 42
without it.

### Four charges that did not survive verification

**"It costs 48 % more source."** It costs **+3.0 %**. The 48 % came from `widget_catalog`,
where the `classic()` functions delegate to helpers defined outside their bodies while the
`teksu()` twins inline them; with call-graph attribution the same corpus is +35 %, by token
count +17.5 %. On the only real before-and-after migration in evidence, Skribisto commit
`c6b022c9e` (same six files, both directions), lines go 3,107 to 3,200. On the metric that
commit message itself chose, builder-child call sites, `teksu!` wins **144 to 59**.

**"It forces duplication."** Fifteen near-identical copies of one modal-card chrome are 41 % of
Skribisto's teksu corpus, and the stated cause was that a `fn modal_card(h, b, f) -> impl
Widget` could not be called from inside a block. A verifier rebuilt that factoring against the
real widget catalog: it compiles and mounts, both from a plain Rust call site and inline. The
duplication was a refactor not done, not a refactor forbidden.

**"Follow Freya and delete it."** Freya removed its `rsx!` in 0.4 for three stated reasons;
none holds here. *Typos*: `spacng: 8.0` gives `E0599: no method named 'spacng' found for struct
'VStack'`, caret on the user's token, with a did-you-mean; Freya's attributes were stringly
typed, `teksu!` properties lower to real method calls. *Autocomplete*: rust-analyzer 0.3.3049
driven over raw LSP returned **116 completion items at a teksu property position against 116
identical items for the equivalent builder chain**, 120 of `Panel`'s own surface when nested in
`VStack`, hover with the real signature, go-to-definition landing on `vstack.rs:60`, 18 enum
items at a property value and 122 at a Category B slot. *Stack traces*: a panic inside a block
names the user's file, line and column, with no `teksilo-macros` frame. The one real limit is
that completion returns null inside a block that does not parse, which is the state you are in
while typing. That argues for a more forgiving grammar, not for deletion.

**"The parse traps are live."** The dangerous `(expr, ELEMENT)` mis-parse has **zero instances**
across all 178 blocks in all three corpora. The traps are latent, which is why they are
documented in the reference rather than designed around.

### What stood, and what was done about it

Three silent-wrong-program bugs, all of them reachable from a pure builder chain with no macro
anywhere, which is why the fixes live in `teksilo-core` rather than in the grammar.

1. `dim_when_inactive` and `dim_when_inactive_default` were the only `WidgetBuilder` methods
   returning a foreign wrapper instead of `WidgetWithHandlers<Self>`, so the reorder rule did
   not protect them and `Probe { dim_when_inactive: 0.7  Leaf(1)  Leaf(2) }` compiled clean
   while building `DimWhenInactive > Leaf(2)`, silently dropping the parent and the first
   child. **Removed from the trait**; the wrapper is constructed directly, and
   `teksilo_teksu_guard::foreign_wrapper_returns` now pins that set at empty, so reintroducing
   one reddens the build.
2. `.on_tap(cb).clips_children_on(true)` double-wrapped to
   `WidgetWithHandlers<WidgetWithHandlers<T>>` and `take_handler_set` returned only the outer
   set, so the tap handler never fired. **Fixed** by `EventHandlers::merge_under` +
   `HandlerSet::merge_under` + a recursive `take_handler_set`.
3. Binding hoist and shadowing. The write-up named the trigger wrongly, as two `if` / `else`
   arms binding the same name; every structural arm is parsed by `parse_element` and a binding
   is not a legal element, so that case does not reach the lowering. The reachable defect is
   the same mechanism one level out: **two bindings sharing a name anywhere in one block**
   hoist two `let`s into the same flat root block, and since the element expression is emitted
   after all of them, *both* attach sites resolve to the later widget. **Still open**, and
   documented as a trap in §3.3 rather than left in a source comment that called it a
   performance concern.

**One grammar rule shaped the flagship.** `parse/body.rs` decided child-versus-property by the
first letter's case, so a lowercase helper call could not be a bare child. Skribisto has **469**
widget-returning helper functions, 5.3 per teksu block, and **98 of its 110 `child:` values are
lowercase**; the archetypal block, `tabs/analysis.rs:613`, had ten children and not one was
bare, so it read as the builder chain it replaced. **Fixed**: a lowercase identifier that
continues into a call, chain or index is now a child, as is a keyword-rooted path. Converting
that same block under the new rule took `child:` from 6 to 1, `child_opt:` from 3 to 0, and
bare children from 0 to 5, at -99 characters and +1 line. The gain is that the block reads as a
tree; it is not a terseness gain, which is consistent with the +3.0 %.

**One absent gate.** `cargo teksilo-fmt --check` was documented as a pre-commit gate and wired
into no workflow. It runs over 1,166 files in 0.57 s and renders the trailing-comma trap as a
diff. **Added to `ci.yml`.**

Three further changes landed alongside: `impl Widget for Box<dyn Widget>` (32 forwarding
methods, no arena node, `as_any` forwards so `with_widget_mut::<W>` still reaches through);
`child_opt` on **38 of 38** containers, up from 7; and **45** accumulator plurals added across
36 types (the first count of "25 missing" was an undercount from scanning only `mut self`
receivers; the real figure was 113 accumulators, 18 already plural).

**The downstream cost.** Established by compiling Skribisto against the branch. Phases 0 and 1
alone broke five `FormLayout::line_ids` call sites and nothing else, in any of the three
consumers. The `*_id` removal that followed is much larger: it is the one genuinely breaking
change in the set, and it cost Skribisto 74 call sites across 33 files.

### The tooling was already fine

- `teksilo-fmt` reproduces **177 of 178** real corpus blocks byte for byte. The one exception
  (`FixedSize { height: height }`) is a deliberate skip that `--check` reports clean.
- The designer's 27 % round-trip fidelity is therefore its own fault. It reuses
  `teksilo-parse` and `teksilo-fmt` and then throws the result away for a mirror IR that loses
  argument-free properties, reorders params before children, discards blank lines, and reads
  the first block while writing the last. Rebuilding it on the parser it already depends on has
  no grammar prerequisite.
- Compile cost is a non-issue: token output is 1.00x the hand-written chain, and a wall-clock
  A/B at 200 elements puts `teksu!` at or below hand-written builder code in every clean repeat.
- A no-rustc preview interpreter is more viable than assumed: **63.6 %** of real property values
  are evaluable without Rust (literals, paths, consts, `lit!` / `tr!`), and `teksilo-preview`'s
  `CatalogEntry` registry already covers **80.3 %** of the widget instances in the corpus,
  reaching 99.6 % with 17 more registrations.

### The two exits, if the question reopens

**A two-rule v4.** Parse the element head as a `syn::Expr` with eager struct braces; a `{` left
in the stream is the body. Rust's own grammar makes the cases disjoint: `Foo { a: 1 }` consumes
its braces and is a struct literal, `VStack::new() { .. }` does not and is an element. That one
rule dissolves all five parse defects by construction and lets you delete both hardcoded widget
tables, the reorder rule, the binding hoist, the structural forms, the statement-sequence
lowering, `TeksiBranch` and `teksilo-teksu-guard`. An independent verifier implemented that
parser plus a migrator and ran it over every real block: **142 blocks, 0 parse errors, 0
structural divergence from the v3 IR**. Costs, measured: no implicit `::new` means roughly **793
constructor rewrites across ~250 blocks**; `tab: label, Card { .. }` (documented, 1 live site)
becomes inexpressible; and a widget held in a local or a const can never carry a body.

**Deletion.** A 197-line converter was written and independently reproduced: **178 of 178
blocks, 0 errors**, all 28 converted example files compile clean, 42 of 905 comments lost. The
door is mechanical.

### What not to do

Do not break the grammar before Skribisto ships. Skribisto-Pro is a live path dependency and
would take 61 compile errors the moment a grammar change lands on main; the flagship would take
88. A language revision is the wrong thing to be holding when the flagship needs a stable floor.

Do not build the designer on a new grammar. It does not need one. It needs to stop
reimplementing the parser it already depends on.

Do not delete `teksu!` on the Freya precedent. Freya deleted a stringly-typed, HTML-shaped macro
inherited from a web framework. `teksu!` is a typed, Rust-shaped macro that lowers to real
method calls, and all three of Freya's stated reasons measure in its favour.

---

## Changelog from v3 as shipped

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Eleven places where v3 as written does not describe v3 as built. Each was verified against the
source before the body text was changed, and every corrected code example was compiled.

**The `*_id` twins of Appendix A.3 no longer exist, and neither does `add_child`.** Every
widget-accepting slot method takes `impl IntoTeksiChild`, implemented for `WidgetId` and
blanket for every `Widget`, so one name carries both: `.child(id)`, `.content(id)`,
`.header(id)`, `.pane(id)`. 80 methods were removed. Appendices A.2 and A.3 are kept as the
record of a refactor that has since been undone, and are marked superseded where they stand.
The macro lost the matching rule: `lower_property` no longer appends an `_id` suffix when a
slot value is a binding or an escape, because there is no second method to route to.

**A bare child need not be an element (§3.6, §4.1).** A lowercase identifier that continues
into a call, a method chain or an index is a Rust expression producing a widget and lowers to
`.child(expr)`, as does a keyword-rooted head. `(`, `*` and `&` are rejected at that position,
because body items are whitespace-separated and Rust would read each as continuing the item
before it.

**`#{ expr }` carries a widget value, not only a `WidgetId` (§6.1).** It lowers to
`.child(expr)`, which is what makes it the answer to a Rust struct literal at body position.

**Reactive conditionals by type-directed inference (§5.1) were never implemented.** `if signal
{ ... }` does not lower to `.visible_when(signal)`. Every `if` at body position lowers to a
plain Rust conditional, so the condition must be a `bool`, and a `Signal<bool>` there is a type
error. The runtime the rule needed, `IntoTeksiCondition`
(`crates/teksilo-core/src/widget_builder_branching.rs:292`), exists and is exported from two
preludes, and nothing emits it. Settled by `crates/teksilo-macros/src/lower.rs:311`, which
emits `.child_opt(if #cond { Some(..) } else { None })` unconditionally. The reactive form that
does work is the `visible_when:` property, which §5.1 already documented as the alternative.

**Two of the diagnostics in §9.2 do not exist, and one of them describes the opposite rule.**
Neither the "bindings use `=`, not `:`" message nor the "expected property, binding, or child
element, found `,`" message appears anywhere in the source. Commas between body items are not
an error at all: they are accepted as optional separators
(`crates/teksilo-parse/src/parse/body.rs:36`). §9.2 now lists the diagnostics the macro
actually emits, taken from the source and from the committed `.stderr` fixtures.

**The newline-based argument rule (§3.4) was replaced by syntactic lookahead.** Proc-macro
token streams carry no newline information. What shipped is a one-token peek past each comma
(`comma_begins_new_body_item`, `crates/teksilo-parse/src/parse/property.rs:62`), and that
module's doc says the replacement outright at line 24. The consequence the old text hid: a
comma followed by an UpperCamel element **continues the argument list** rather than starting a
new child.

**§11's test strategy describes a corpus that does not exist.** There are no golden-file
`cargo expand` tests and no bitwise render comparison. The corpus is trybuild:
`crates/teksilo/tests/teksi/pass/` and `fail/`, driven by
`crates/teksilo/tests/teksi_trybuild.rs`. §11 also predates the crate split: parsing, the IR
and the diagnostics live in `teksilo-parse`, not `teksilo-macros`.

**SplitView (§4.2, Appendix A) no longer exists.** It was replaced by `Splitter`
(`crates/teksilo-widgets/src/splitter.rs`), an N-pane container whose `.child()` and
`.pane()` both take `impl IntoTeksiChild`, so it belongs in Category A, not Category B. The `Popover` entry is stale a
second way: no type is spelled `Popover`. The family is `PopoverWidget<T>` and its three
aliases (`crates/teksilo-parse/src/diag.rs:145`).

**Appendix A's `*_id` convention no longer exists at all.** It was never the invariant the
appendix claimed — measured over `crates/teksilo-widgets/src`, 37 of 101 widget-taking setters
had no id-taking twin, so a binding in a slot position was unusable at 37 places. Rather than
fill the gaps, the twins were deleted: every slot is now **one** method taking
`impl IntoTeksiChild`, which accepts a `WidgetId` and any `Widget + 'static` alike. Appendix A.6
keeps the old measurement as the record of why. See the `*_id` removal entry above.

**§6.1's `IntoTeksiChild` dispatch was never implemented either.** A body-position
`#{ expr }` emits `.child(expr)` and carries a widget value or a `WidgetId`
(`crates/teksilo-macros/src/lower.rs:195`); a widget-valued expression there is a compile error.
`crates/teksilo-parse/src/ir.rs:76` records this in the IR's own doc comment.

**Binding scope is one block, not a nest of them (§3.3).** v3 described five kinds of
statement-forming block and said a binding in one `if` arm is invisible from its sibling. The
lowering keeps one `hoisted` vector for the whole tree and emits every `let` at the root of the
expansion (`crates/teksilo-macros/src/lower.rs:43`), so every binding is visible everywhere in
the block and every bound widget is constructed unconditionally. Bindings are not legal inside
an `if` / `else` / `match` arm in any case: an arm must hold exactly one element.

**A struct literal at a property-value position needs parentheses (§3.4, §7.3).** v3 showed
`style: TextStyle { family: ..., size: ... }` and said the bracket-aware parser would keep it as
one argument. It does not. `TextStyle` followed by `{` is the shape of a teksu **element**, so
the parser reads the fields as properties and emits `TextStyle::new().family(..).size(..)`,
which fails with `no associated function or constant named 'new'`. The fix is the same
parenthesis escape an UpperCamel-rooted method chain needs
(`crates/teksilo/tests/teksi/pass/54_paren_wraps_method_chain.rs`). This one mattered: the
broken shape was v3's own worked example in two places.

**Spread (§5.5) takes ids, not widgets.** v3 said `..expr` inlines "a `Vec<WidgetId>` or an
iterator of widgets". The emitted loop calls `.child(id)`
(`crates/teksilo-macros/src/lower.rs:142`), so a widget value there does not compile. §5.5's
desugaring illustration also named the wrong locals; it now matches what the macro emits.

**Several method names in the worked translations (§7) never existed.** The `*_literal` twins
the examples lean on (`new_literal`, `tooltip_literal`, `title_literal`,
`supporting_text_literal`) are absent from `teksilo-widgets`; the untranslated path is `lit!`
at the argument. `TabWidget` has no `tab_item` method and there is no `TabItem` type (the tab
descriptor is `TabInfo`, passed to `static_tab`), and its bar slots are `bar_leading_slot` /
`bar_trailing_slot`, not `trailing_slot`. §3.4, §4.2 and §7.5 are corrected; §7.6 carries a
warning instead, because its whole Popover shape predates the constructor change.

One rule has also been **added** since v3 was written, and §3.6 and §4 now describe it: a
lowercase identifier that continues into a call or a method chain is a Rust expression
producing a widget, and lowers to `.child(expr)`
(`crates/teksilo-parse/src/parse/body.rs:170`).

---

## Changelog from v2

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Four structural changes, all driven by review of v2 against the actual widget catalog and by subsequent design discussion.

**Bindings use `name = Element` instead of `id: name`.** The new form reads like ordinary Rust assignment, removes one keyword from the grammar, and works uniformly at body position and in property-argument position.

**Widget categories are now two, not three.** The framework refactor (see Appendix A) dissolves the former Category C by moving primary content from constructors to setter methods on ScrollArea, Popover, Snackbar, and Dialog. ScrollArea joins Category A (has `.child()`). Popover, Snackbar, and Dialog join Category B (named slots). Wizard is not Category C and was never intended to be; v2 misclassified it.

**The `*_id` convention is extended.** *(Superseded: the twins below were removed; a slot method now takes `impl IntoTeksiChild`. See the changelog at the top.)* A widget-accepting slot method should have a twin taking a `WidgetId`, named `*_id`, because a body-position binding and a `#{ expr }` escape both route through the id form. The `.set_child(id)` methods on Panel, Padding, Expand, GroupBox, and Accordion are renamed to `.child_id(id)` / `.content_id(id)` for consistency, and TabWidget gets `.tab_id(label, id)` to match.

> **As shipped, there is no twin at all.** v3 said "every widget-accepting slot method on every container now has a twin", and that was never true: 75 slots had one and 89 did not. Rather than add the missing 89, the twins were removed and the slot method widened to `impl IntoTeksiChild`, so one name takes a `WidgetId` and a widget alike.

**Worked translations reflect the refactored API.** Every code example in §7 is against post-refactor builder signatures. The seven uploaded example files themselves are assumed to be migrated; the framework changes required are listed in Appendix A.

Em-dashes and middle dots in quoted source strings are preserved verbatim. The "no em-dashes in English prose" rule continues to apply to the spec's own writing and does not apply to code being quoted.

---

> **Labels in these examples.** For brevity the examples below pass bare string literals
> (`Button("Save")`). With the default `i18n` feature a widget label is a `LocalizedString`
> and there is no `From<&str>`, so in a real app wrap it: `Button(lit!("Save"))` (untranslated)
> or `Button(tr!(save()))` (translated). `teksu!` passes whatever is inside `(…)` verbatim, so
> the wrapping just goes inside the parens. The fake widgets used to demonstrate macro
> mechanics (`Probe`, `Tag`, `Marker`, …) take a plain `&str` and need no wrapping.

## 1. Design Principles

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

The `teksu!` macro is a thin syntactic transform. It is not a new runtime, not a new type system, and not a new reactivity model. Every `teksu!` block desugars to a sequence of builder calls against Teksilo API. There is no hidden allocation, no intermediate virtual tree, no diff step. The macro's only job is to remove syntactic noise from code that already expresses a widget tree.

Five rules bind the design.

First, one-to-one desugaring. Every surface form has a unique, mechanically predictable expansion. No form that works only sometimes depending on macro inference.

Second, error spans follow the user. When expansion fails (wrong property name, wrong child type, wrong handler arity), the error points at the user's token, not at a synthetic span inside the expansion.

Third, reactivity and capture stay visible. `Signal<T>`, `Prop<T>`, and closure `move` appear in the source as themselves. The macro never synthesizes binding or capture semantics the user did not ask for.

Fourth, builder interop is symmetric. `teksu!` expressions and builder chains can be freely nested in either direction. Neither is a superset of the other.

Fifth, the macro never introduces new capabilities. If a construct cannot be expressed by the V2 builder API, `teksu!` will not invent a way to express it. Missing capabilities are fixed in the builder first, then surfaced in the DSL.

---

## 2. Lexical Structure

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

A `teksu!` invocation takes one of two forms.

```rust
teksu!(ctx => <root-element>)
teksu!(<root-element>)
```

The `ctx =>` preamble binds the name used for the `BuildContext` inside the block, and causes the root element to be inserted into the arena via `ctx.add(...)` so the call returns a `WidgetId`. The shorter form has no preamble and returns a widget value suitable for passing to `.child(...)` or to a named-slot method.

Disambiguation is lexical. The macro parser looks at the first tokens: if the leading form is `ident =>`, the preamble is consumed; otherwise the macro starts parsing elements immediately.

---

## 3. Elements

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

An element is the fundamental unit of the language. It names a widget type (possibly with an explicit constructor path), optionally carries positional arguments and a body block containing properties, bindings, and child elements.

### 3.1 Grammar

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

Body items are separated by newlines. This eliminates the `,` noise between `.child(...).child(...)` calls that dominates the uploaded example files.

> **As shipped, a comma between body items is accepted as an optional separator**, so
> `Panel { padding: 8.0, color: RED }` on one line works the same as two newline-separated
> properties (`crates/teksilo-parse/src/parse/body.rs:36`). v3 called commas an error and §9.2
> promised a diagnostic for them; that diagnostic was never written and the relaxation went in
> instead. The one place a comma still changes meaning is at the end of a property's argument
> list, which §3.4 covers.

At positions where an `arg` is expected, the parser uses "commit on distinctive prefix" to decide between element and expression: if the leading tokens form a TypePath followed by `(`, `::`, or `{`, or match `ident = TypePath(...)`, commit to element or bound-element parsing. Otherwise commit to expression parsing. This rule is local (no backtracking) and preserves clean error spans.

### 3.2 Constructors

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Naming a widget binds its `WidgetId` to a local so it can be referenced later. Bindings work in two places: at body position inside a container, and in property-argument position.

**Binding at body position** (Category A containers only, see §4):

```rust
teksu!(ctx =>
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
            .child(open_btn)
            .child(
                TextWidget::new("Status")
                    .linked_to(open_btn)
            )
    )
}
```

The binding is hoisted to the nearest enclosing statement-forming block (the `teksu!` expansion here), where it remains in scope for the rest of the block. The container uses `.child(id)` to attach the bound element at its body position.

**Binding in property-argument position** (Category B slots):

```rust
teksu!(
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
        .header(title)
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

The slot method does not change: `.header` takes `impl IntoTeksiChild`, so the same name accepts the widget and the id. There is no `*_id` twin to route to, and the macro no longer rewrites the method name.

**Scope rules.** A binding is in scope from the point of declaration to the end of the
`teksu!(...)` expansion, and nowhere outside it. v3 listed five kinds of statement-forming block
(the expansion, a `rust { }` block, a `match` arm, an `if` or `else` arm, a `for` body, a `let`
form's scope) and said a binding declared in one arm is not visible from a sibling arm.

**As shipped there is exactly one block.** The lowering carries a single `hoisted` vector for
the whole tree and emits every `let` at the root of the expansion
(`lower_root`, `crates/teksilo-macros/src/lower.rs:43`), so a name bound in one subtree is
visible from any other subtree in the same block, which is what makes the `#{ }` / `on_tap`
cross-references in §6.1 and §7.9 work. The v3 sentence about sibling arms describes a
situation that cannot arise anyway: a binding is not a legal body item inside an `if`, `else`
or `match` arm, which must contain exactly one element
(`if-body must contain exactly one element — wrap multiple in a container like VStack`).

Two consequences worth stating outright, since a single flat block hides both.

**A hoisted `let` runs unconditionally.** A binding is `ctx.add(...)`, so the widget is
constructed and inserted into the arena whether or not the branch that mentions it is taken.
Binding inside a conditionally-mounted subtree is therefore not a way to build it lazily.

**Two bindings sharing a name in one block silently alias.** The lets are emitted in traversal
order and the whole tree expression comes after all of them, so the second `let` shadows the
first and *every* reference to that name, including the attach site of the first binding,
resolves to the later widget. The earlier widget is still constructed and added to the arena,
and is then attached nowhere; the later one is attached twice. Nothing in the macro rejects it
and nothing in the tree looks wrong until it is on screen. Names in one block are one flat
namespace: keep them distinct.

### 3.4 Properties

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

The distinction between "bare child element" and "argument-free property" is lexical, and it
turns on Rust's own naming convention: `UpperCamel` for types, `snake_case` for methods. An
identifier at body position starting with an uppercase letter is a child element. A lowercase
identifier is a property call **when it stands alone**; a lowercase identifier that continues
into a call or a method chain is an expression child instead (§3.6).

**Argument list termination.** v3 specified a newline rule here: the argument list ends at the
next newline unless a bracket is still open. A proc macro receives a `proc_macro2::TokenStream`,
which carries no newlines; line and column are reachable only behind `proc-macro2`'s
`span-locations` feature, and that feature interacts poorly with rust-analyzer's proc-macro
server, so the rule would have behaved one way under `cargo` and another in the editor.
(`teksilo-fmt` does enable `span-locations`, because it reads source as an ordinary binary
rather than from inside an expansion. `teksilo-parse` cannot rely on that: the same parser has
to run in the proc macro.)

**What shipped is a one-token syntactic lookahead past each comma**
(`comma_begins_new_body_item`, `crates/teksilo-parse/src/parse/property.rs:62`; the module doc
states the replacement at line 24). After parsing each argument the parser looks at the next
token:

- Not a comma: the argument list ends.
- A comma followed by `name:`, by a structural keyword (`if`, `for`, `match`, `let`), by a
  spread `..`, by an escape `#{`, or by a binding `name =`: the argument list ends and the comma
  is left for the body parser, which consumes it as an optional separator.
- A comma followed by anything else, **including an UpperCamel element**: the argument list
  continues.

Bracket groups are atomic, because `syn` parses a delimited group as one token tree, so a
multi-line argument value keeps its internal commas whatever they are:

```rust
Panel {
    offset: (4.0, 2.0)
    TextWidget("Hello")
}
```

**A struct literal is the exception, and v3 got it wrong.** v3 showed
`style: TextStyle { family: ..., size: ... }` as "a single-argument property whose value happens
to contain commas inside brackets". It is not. At a property-value position the parser commits
on a distinctive prefix (§3.1), and `TextStyle` followed by `{` is exactly the shape of a teksu
**element**. So the braces are parsed as an element body and the fields as properties, and the
macro emits

```rust
TextStyle::new().family("sans-serif".into()).size(14.0)
```

which fails with `no associated function or constant named 'new' found for struct 'TextStyle'`.
Rust's own grammar has the same ambiguity and resolves it by banning struct literals in an
`if`/`match` scrutinee; teksu resolves it in favour of the element, because an element is the
common case at that position.

**Wrap a struct literal in parentheses.** The outer `(` is not an identifier, so
`peek_element_start` declines and the whole thing goes down the expression path:

```rust
Panel {
    style: (TextStyle {
        family: "sans-serif".into(),
        size: 14.0,
        weight: FontWeight::BOLD,
    })
    offset: (4.0, 2.0)
    TextWidget("Hello")
}
```

This is the same escape hatch an UpperCamel-rooted method chain needs
(`crates/teksilo/tests/teksi/pass/54_paren_wraps_method_chain.rs`), and the lowering strips one
layer of `Expr::Paren` so the emitted call is not double-parenthesized.

Note that the trailing comma after the last field is fine **inside** the parens, where `syn`
owns the parse. A comma after the **last property in a body** is not: nothing follows it for the
lookahead to classify, so the parser looks for another argument and fails with `unexpected end
of input, expected an expression`. `Probe { Tag("a"), spacing: 8.0 }` compiles;
`Probe { spacing: 8.0, }` does not.

**Multi-argument with element values.** A property argument can be a full element, including one with its own body. This is the TabWidget pattern:

```rust
TabWidget(selected) {
    tab: lit!("Overview"), Card {
        header: TextWidget(lit!("Overview")) { style: t.body_bold.clone() }
        content: VStack { spacing: 12.0, ... }
    }
    tab: lit!("Inspector"), Panel { padding: 20.0, ... }
    bar_trailing_slot: trailing_widget
}
```

The comma after `lit!("Overview")` is followed by an UpperCamel element, so it continues the
argument list and the `Card` becomes `tab`'s second argument. The `Card { ... }` body is one
brace group, so `syn` consumes it whole however many lines it spans. After the Card closes there
is no further comma, so the `tab` property ends.

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
    .bar_trailing_slot(trailing_widget)
```

**This is the rule's cost, and it is a real trap.** Because a comma before an UpperCamel element
continues the argument list, a body written with struct-literal-style commas silently feeds the
child into the preceding property:

```rust
Probe {
    spacing: 8.0, Tag("a")
}
```

`Tag("a")` does not become a child. It becomes `spacing`'s second argument, and what the user
sees is an arity error against a method they did not mean to call:

```text
error[E0061]: this method takes 1 argument but 2 arguments were supplied
    |
    |             spacing: 8.0, Tag("a")
    |             ^^^^^^^       --- unexpected argument #2 of type `Tag`
```

There is no diagnostic for this and there cannot be a good one without knowing the widget's
method arities, which the macro does not. Keep children on their own lines. The formatter
(`cargo teksilo-fmt --check`) renders the case as a diff, which is the practical guard.

Property ordering is preserved, except for the reorder in §3.5: the macro emits method calls in source order for every property that is not a wrapping `WidgetBuilder` method.

Properties are never reinterpreted. `color: c.text_primary` emits `.color(c.text_primary)` whether `c.text_primary` is a `Color`, a `Signal<Color>`, or a `Prop<Color>`. Conversion happens at the type level through `impl Into<Prop<T>>`, not in the macro.

### 3.5 Handlers

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

#### Reorder of wrapping properties

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

A property whose name is a method on the `WidgetBuilder` trait returning `WidgetWithHandlers<T>` is moved to the **end** of the emitted chain. Every other body item keeps its source position, and relative order within each of the two groups is preserved.

The criterion is the return type, not the `on_` prefix. A wrapping method replaces the widget with `WidgetWithHandlers<T>`, which exposes none of the widget's own setters; a child or a widget-specific property emitted after it would resolve against the wrapper and fail with a diagnostic naming a `.child` the user never wrote. The reorder is what lets §3.6's free interleaving hold in the presence of handlers.

This is the only respect in which the emitted chain departs from source order, and the only thing the macro needs to know about the framework: which names wrap. That knowledge lives in `teksilo_parse::diag::is_widget_builder_method` and nowhere else. Because it is a hand-written list and the trait grows, the `teksilo-teksu-guard` crate parses the trait and fails the build when the list falls behind it. The drift is otherwise invisible until a user writes the property with a child after it.

An argument-free wrapping method (§3.4's bare-lowercase form) is reordered on the same rule.

### 3.6 Child Elements

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

Body items interleave freely. Properties, bindings, and children appear in the output chain in source order, subject to the §3.5 reorder:

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

**A bare child need not be an UpperCamel element.** v3 said it must, and that rule was the
single biggest constraint the DSL placed on how an application is written: a tree built out of
the author's own `fn row(..) -> impl Widget` helpers could not use the block form at all,
because a helper call is lowercase. That rule has been relaxed. **A lowercase identifier that
continues into a call or a method chain is a Rust expression producing a widget, and lowers to
`.child(expr)`** (`crates/teksilo-parse/src/parse/body.rs:170`). A lowercase identifier standing
alone is still the argument-free property of §3.4.

A **keyword-rooted head** is an expression child too: `self.row(x)`, `Self::header()`,
`crate::ui::header()`, `super::row()`. Three punctuation heads are deliberately excluded,
and the rule behind the exclusion is one sentence: body items are separated by whitespace,
not punctuation, so a head that Rust would read as a *continuation* of the item before it
cannot be admitted. `(` would swallow its neighbour as a call argument (`(a) (b)`), `*` as a
multiplication (`a *b`), `&` as a bitwise and (`a &b`). `&` is doubly excluded, since no
reference type implements `Widget`. For those, use the delimited property form,
`child: (expr)`. Nothing continues into `self`, `Self`, `crate` or `super`, which is what
makes the keyword heads safe.

```rust
Probe {
    fills_stack
    section("one")
    section("two").emphasised()
    Tag("three")
}

// Desugars to
Probe::new()
    .fills_stack()
    .child(section("one"))
    .child(section("two").emphasised())
    .child(Tag::new("three"))
```

This is a pure extension: every form it accepts was a parse error before it, so no program
changed meaning.

**Two heads it does not reach.** The dispatch starts from an identifier, so an expression whose
head is a keyword path (`self::section("x")`, `crate::ui::row()`) or an operator (`*boxed()`)
still falls through to the "expected a property name, child element, binding, or `#{ expr }`
escape" error. Write those through the property form instead, which takes an arbitrary Rust
expression:

```rust
Probe {
    child: self::section("four")
}
```

Bare child elements are only meaningful for Category A containers (§4.1). For the Category B
widgets the macro knows about (`crates/teksilo-parse/src/diag.rs:145`) it pre-empts with a
targeted diagnostic naming the slot; for any other widget without a `.child()` method the
compiler's own `no method named 'child'` error is clear enough.

---

## 4. Widget Categories

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Every Teksilo widget falls into one of two categories based on how it accepts content. The category determines which DSL form applies.

### 4.1 Category A: Has `.child()`

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

These widgets accept one or more children through a `.child(..)` method taking `impl IntoTeksiChild`, so the same name takes a widget or a `WidgetId`. Body-block child syntax in the DSL maps directly.

**Members:**

Layout primitives: VStack, HStack, ZStack, Padding, Expand, Switcher, Center, MinSize, MaxSize, FixedSize, AspectRatio, Wrap, Grid, ColumnFlow, MasonryLayout, DeadZone, Shrinkable.

Flat containers: Panel, Toolbar, StatusBar, GroupBox, DropTarget, FocusScope.

Splitting: Splitter, the N-pane container that replaced SplitView. `.pane(..)` is the primary name and `.child(..)` is its alias; both take an id or a widget.

Scrolling: ScrollArea (post-refactor; see Appendix A).

Animation wrappers: Collapse, Fade, Blur, Pulse, Rotate, Scale, Shake, Slide, SmoothSize, Unroll, Cycle.

This list is illustrative, not exhaustive: Category A is defined by having `.child()`, and new containers join it without an entry here.

**Bound children.** A body-position binding attaches with `.child(id)`, the same method a bare child uses. There is no id-taking twin and nothing for the macro to pick between.

**DSL form:**

```rust
VStack {
    spacing: 12.0
    TextWidget("Hello")
    Button("Click") { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Go) }
}
```

Bare children and bound children both desugar to `.child(..)`.

### 4.2 Category B: Named Slots

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

These widgets have no `.child()` method. Content goes through named setter methods, one per semantic slot. Each slot method takes `impl IntoTeksiChild`, so `.slot_name(..)` accepts a widget or a `WidgetId`; there is no `*_id` twin.

**Members and their slots:**

- **Card** (`header`, `content`, `footer`)
- **Accordion** (`content`, `trailing`, with `title` taken as constructor arg)
- **TitleBar** (`leading`, `center`, `trailing`, plus the non-widget `close_action` handler)
- **DialogContent** (`body`, `footer`, with `title` and `supporting_text` as LocalizedString properties)
- **Breadcrumb** (`item` and `trailing_slot`)
- **TabWidget** (`tab`, `static_tab`, `bar_leading_slot`, `bar_trailing_slot`, with tabs being multi-arg `(label, widget)` pairs). v3 called the last two `trailing_slot` and listed a `tab_item` slot taking a `TabItem`; neither name shipped.
- **PopoverWidget** (`content`). There is no type spelled `Popover`: the family is `PopoverWidget<T>` plus the aliases `PopoverButton`, `PopoverIconButton` and `PopoverCustom`, and all four are recognised. v3 also listed a `trigger` slot; the trigger became a constructor argument instead (`PopoverWidget::new(trigger)`, with `T: PopoverTrigger` implemented for `Button`, `IconButton` and `OverlayTrigger`), so the popover family has exactly one slot.
- **Snackbar** (`content`, `trigger`; post-refactor)
- **Dialog** (`content` taking a `Fn() -> impl Widget` factory, plus `trigger`; post-refactor)
- **Wizard** (`step`, `steps`, `trigger`; see §4.4)

**SplitView is gone.** v3 listed it here with `first` / `second` slots. It was replaced by `Splitter`, which has `.child()` and belongs in Category A (§4.1).

This is also the exact list the macro itself knows, in `is_category_b_widget` (`crates/teksilo-parse/src/diag.rs:145`); it is what drives the targeted "use `content: <widget>` instead of a bare child element" diagnostic. A Category B widget missing from that list still fails to compile, just with the compiler's generic method-resolution error instead of the slot hint.

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

A binding or a `#{ expr }` escape in a slot position uses the slot's own name. The method name is never rewritten.

### 4.3 Leaf Widgets

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Widgets with no child-accepting methods at all. Buttons, TextWidget, IconWidget, ImageWidget, RectWidget, Badge, Link, Spacer, Divider, Toggle, Checkbox, RadioButton, Slider, ProgressBar. These have properties but no body children or slots. Their DSL form is just `Type(args) { property: value, on_handler: closure, ... }`.

### 4.4 Wizard

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Wizard is structurally its own case: it takes a title in the constructor and wires multi-step content through `.step(WizardStep)` and `.steps(iter)` methods. It is not refactored in Appendix A because its shape does not fit cleanly into either Category A or B. For DSL authoring, treat Wizard like Category B with named slots, adding `step` and `steps` to the slot vocabulary.

As shipped the macro agrees: `Wizard` is in `is_category_b_widget`, so a bare child under it gets the slot diagnostic pointing at `step`.

---

## 5. Structural Forms

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Pure element syntax handles fixed structure, fixed properties, fixed children. The remaining cases, conditional inclusion, iteration, local bindings, side effects, and programmatic subtree splicing, get first-class structural forms rather than forcing users back to builder syntax mid-block.

### 5.1 `if` Forms

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

An `if/else` with two arms of different widget types desugars via `TeksiBranch<L, R>` to a type that implements `IntoTeksiChild` by dispatching to the active variant. Three- and four-way branches use `TeksiBranch3` and `TeksiBranch4`. Branches beyond four arms require explicit `Box<dyn Widget>`.

**Reactive conditionals: designed, never implemented.** v3 specified that if the condition is a
bare identifier whose static type is `Signal<bool>` or `Prop<bool>`, the lowering would be
`.visible_when(signal)` on the child rather than an arena-level conditional, and that this would
be the one place where the macro performs type-directed inference.

**No such inference exists.** Every `if` at body position lowers to the plain Rust conditional
described above (`crates/teksilo-macros/src/lower.rs:311`), so the condition must be a `bool`
and a `Signal<bool>` there is a type error:

```text
error[E0308]: mismatched types
   |
   |             if flag {
   |                ^^^^ expected `bool`, found `Signal<bool>`
```

The runtime the rule would have needed was built and is still exported: `IntoTeksiCondition`
(`crates/teksilo-core/src/widget_builder_branching.rs:292`) has impls for `bool`,
`Signal<bool>` and `Prop<bool>`, its doc comment describes the dispatch, and the macro never
calls it. Whether to finish it or delete it is a v4 question, not a documented capability.

**Write the two cases explicitly instead.** For a build-time conditional, read the signal:

```rust
StackLike {
    if flag.get() {
        Tag("banner")
    }
}
```

For reactive visibility, use the property form, which is real: `visible_when` is a
`WidgetBuilder` method accepting `bool` / `Signal<bool>` / `Prop<bool>`, equivalent to the
imperative `ctx.visible_when(id, signal)`.

```rust
Tag("banner") {
    visible_when: flag
}
```

The two are not interchangeable, and the difference is the reason v3 wanted the inference: the
`if` form does not build the widget at all when the flag is false, while `visible_when` always
builds it and binds its visibility. Pick by whether the subtree is expensive or the flag changes
after build. Being explicit about which one you get is arguably the better outcome.

### 5.2 `for` Forms

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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
        State::Loading => TeksiBranch3::A(Spinner::new()),
        State::Loaded(data) => TeksiBranch3::B(DataView::new(data.clone())),
        State::Error(msg) => TeksiBranch3::C(ErrorBanner::new(msg.clone())),
    })
```

### 5.4 `let` Forms

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

A spread `..expr` inlines an iterable of `WidgetId` as children at that position. v3 said "or an
iterator of widgets"; it is ids only, because the emitted loop calls `.child(id)`
(`crates/teksilo-macros/src/lower.rs:142`). For a run of widget *values*, use the container's own
`.children(iter)` as a property.

```rust
VStack {
    TextWidget("Header")
    ..plugin_widgets
    TextWidget("Footer")
}

// Desugars to
{
    let mut __parent = VStack::new();
    __parent = __parent.child(TextWidget::new("Header"));
    for __spread_id in plugin_widgets {
        __parent = __parent.child(__spread_id);
    }
    __parent = __parent.child(TextWidget::new("Footer"));
    __parent
}
```

Spread is for programmatic child list assembly (plugin registries, restored workspaces, tab managers).

### 5.6 `rust` Forms

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

A `rust { ... }` block switches to imperative construction. Two shapes, distinguished by whether the block produces a value.

**Expression-producing form.** The block ends with an expression without a trailing semicolon. The value is used as a child or spread across children via the `IntoTeksiChild` trait.

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

**Failure mode for forgotten `;`.** If the user writes a side-effect block without a trailing `;`, and the tail has type `()` (for example, a bare `if let { ...; }` without `else`), the macro treats it as expression form and tries to dispatch `()` through `IntoTeksiChild`. The compiler responds with "the trait `IntoTeksiChild` is not implemented for `()`" pointing at the block's tail expression. This is a survivable error: the message is clear and the fix is to add the missing `;`.

---

## 6. Escape Hatches

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

One escape into host Rust, in addition to the `rust { }` block.

### 6.1 Expression Escape: `#{ expr }`

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Anywhere a child element or property value is expected, `#{ expr }` takes a Rust expression and
inserts its value at that position. At child position it emits `.child(expr)`; at a slot
position it emits `.slot(expr)` under the slot's own name.

**The expression may be either a widget or a `WidgetId`.** v3 specified dispatch through an
`IntoTeksiChild` trait and left it unimplemented; it is implemented now. The lowering is still
unconditional — `#{ expr }` always emits `.child(expr)` — but every child- and slot-taking
method takes `impl IntoTeksiChild`
(`crates/teksilo-core/src/widget_builder_branching.rs:257`), whose two impls cover `WidgetId`
and any `Widget + 'static`. The routing therefore happens in the type system rather than in the
macro, which is why the one lowering serves both.

```rust
// Inserting a pre-built widget as a child
VStack {
    TextWidget("Header")
    #{ build_complex_subtree(ctx, config) }
    TextWidget("Footer")
}

// Re-using a bound id in a slot
teksu!(ctx =>
    VStack {
        title = TextWidget("Manuscript") { style: bold }
        Card {
            header: #{ title }
            content: TextWidget("Body")
        }
    }
)
```

The second case uses `title` (declared with `name = Element` binding) via `#{ title }` in a Category B slot. The escape pulls the `WidgetId` into the slot position and `.header` takes it directly; the macro rewrites nothing.

For bare identifiers at property-value positions, `#{ }` is not required: `text: selected_label` parses as a property with a Rust expression value. The escape is only needed where the parser would otherwise try to interpret the value as something else (an element, a structural form), or at child position where the head token is one the bare-expression-child rule rejects — see §3.6.

---

## 7. Worked Translations

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Each translation takes a block from one of the uploaded example files and shows the `teksu!` equivalent against the actual post-refactor constructor and method names. Translations assume Appendix A has been applied.

### 7.1 simple-button

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Source:

```rust
.root(|tree| {
    tree.add(
        Button::new(lit!("Click Me"))
            .variant(ButtonVariant::Plain)
            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::ButtonClicked))
            .tooltip(lit!("This is a simple button. Click it to see a message in the console.")),
    )
})
```

With `teksu!`:

```rust
.root(|tree| teksu!(tree =>
    Button::new(lit!("Click Me")) {
        variant: ButtonVariant::Plain
        on_activate_fn: |ctx| ctx.send_intent(AppIntent::ButtonClicked)
        tooltip: lit!("This is a simple button. Click it to see a message in the console.")
    }
))
```

The explicit `::new` names the constructor. Four lines instead of six, property assignments read as assignments.

> v3 wrote `::new_literal` and `tooltip_literal` here, anticipating a `*_literal` twin for every
> `LocalizedString`-taking method. That never shipped: no `new_literal`, `tooltip_literal`,
> `title_literal` or `supporting_text_literal` exists in `teksilo-widgets`. The untranslated
> path is the `lit!` macro at the argument, as shown.

### 7.2 text-and-layout, outer Padding and VStack

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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
                            .variant(ButtonVariant::Plain)
                            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::ToggleDarkMode)),
                    ),
            ),
    ),
);
```

With `teksu!`:

```rust
let root = teksu!(ctx =>
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
                    variant: ButtonVariant::Plain
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::ToggleDarkMode)
                }
            }
        }
    }
);
```

All `.child(...)` wrappers collapse. Siblings land at equal depth. `::uniform` and `::new` appear where the builder uses them.

### 7.3 text-and-layout, build_color_box helper

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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

With `teksu!`:

```rust
fn build_color_box(color: Color, label: &str) -> impl Widget {
    teksu!(
        Panel {
            background: color
            corner_radius: 6.0
            padding: 8.0
            TextWidget::new(lit!(label)) {
                style: (TextStyle {
                    family: "sans-serif".into(),
                    size: 14.0,
                    weight: FontWeight::BOLD,
                    line_height: 1.4,
                    letter_spacing: 0.0,
                })
                color: Color::WHITE
            }
        }
    )
}
```

The return type changes from `Panel` to `impl Widget` because the macro's output is opaque.

> **The parentheses around `TextStyle { ... }` are load-bearing.** v3 wrote this without them
> and explained that "the bracket-aware parser keeps them as a single argument to `.style()`".
> It does not: `TextStyle` followed by `{` is the shape of a teksu element, so without the
> parens the fields are parsed as properties and the macro emits
> `TextStyle::new().family(..).size(..)`. See §3.4.

### 7.4 title-bar-demo, multi-argument properties and slots

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Source:

```rust
TitleBar::new(host)
    .height(40.0)
    .background(theme.colors.surface_pressed)
    .border(theme.colors.text_secondary, 2.0)
    .leading(
        TextWidget::new(lit!("  Teksilo — Title Bar Demo"))
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

With `teksu!`:

```rust
teksu!(
    TitleBar(host) {
        height: 40.0
        background: theme.colors.surface_pressed
        border: theme.colors.text_secondary, 2.0
        leading: TextWidget::new(lit!("  Teksilo — Title Bar Demo")) {
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

Three things to note. First, `border: color, width` is the multi-argument property form. Second, the em-dash in `"Teksilo — Title Bar Demo"` and the middle dots in `"drag · double-click maximize · right-click for menu  "` are preserved verbatim from the source. Third, `leading:` and `center:` are Category B slot values, written as full nested elements.

### 7.5 tab-widget, full TabWidget with multi-arg element values

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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
            .variant(ButtonVariant::Ghost)
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
        .static_tab(TabInfo::new().title(lit!("Disabled")).enabled(false), Panel::new()...)
        .bar_trailing_slot(trailing),
);
```

With `teksu!`:

```rust
let selected = ctx.signal(0_usize);
let selected_label = selected.map(|index| match *index {
    0 => "Overview".to_string(),
    1 => "Inspector".to_string(),
    _ => "Activity".to_string(),
});

let trailing = teksu!(
    HStack {
        spacing: 12.0
        TextWidget::new(lit!("")) {
            text: selected_label
            style: theme.typography.small.clone()
        }
        Button::new(lit!("Toggle Theme")) {
            variant: ButtonVariant::Ghost
            on_activate_fn: |ctx| ctx.send_intent(AppIntent::ToggleTheme)
        }
    }
);

let tabs = teksu!(ctx =>
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
        static_tab: (TabInfo::new().title(lit!("Disabled")).enabled(false)), Panel {
            padding: 20.0
            TextWidget::new(lit!("Disabled tabs are visible but cannot be activated."))
        }
        bar_trailing_slot: trailing
    }
);
```

The `tab: lit!("name"), Card { ... }` pattern is the multi-argument property form with an element-valued second argument (`tab` is `TabWidget`'s title-only shorthand for `static_tab(TabInfo::new().title(label), content)`). The `static_tab:` property takes the full `TabInfo` as its first argument and the content element as its second, which is how a per-tab flag like `enabled(false)` is set. `bar_trailing_slot:` takes the previously-built `trailing` widget. Signals (`selected`, `selected_label`) stay as regular Rust `let` bindings because they are computed values, not widgets.

The parentheses around the `TabInfo` chain are required. `TabInfo` is UpperCamel, so without
them the parser commits to element parsing at `TabInfo::new()` and the trailing `.title(..)` is
a syntax error; the outer `(` sends the whole chain down the expression path instead (the
escape hatch that `crates/teksilo/tests/teksi/pass/54_paren_wraps_method_chain.rs` pins).

> v3 wrote `tab_item: TabItem::new(..)` and `trailing_slot:` here. Neither shipped: there is no
> `TabItem` type and no `tab_item` method, the tab descriptor is `TabInfo`, and the bar slots are
> `bar_leading_slot` / `bar_trailing_slot`. Note also that `TabWidget::new` takes a
> `Signal<Option<TabId>>`, not the `Signal<usize>` this translation shows.

### 7.6 overlay-demo, Dialog / Popover / Snackbar (post-refactor)

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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
        .variant(ButtonVariant::Plain),
);

let popover = Popover::new(lit!("Show popover"))
    .content(popover_content)
    .caret_size(12.0)
    .trigger(popover_trigger);

let snackbar = Snackbar::new(lit!("Show snackbar"))
    .content(snackbar_content)
    .auto_dismiss_after(Duration::from_millis(2500));
```

With `teksu!`:

```rust
let root = teksu!(ctx =>
    ScrollArea {
        widget_resizable: true
        VStack {
            spacing: 24.0
            TextWidget::new(lit!("Dialogs and Popovers")) {
                style: t.body_bold.clone()
                color: c.text_primary
            }
            TextWidget::new(lit!("Teksilo now resolves dialogs through a shared modal presentation pipeline, alongside anchored popovers and timed snackbars.")) {
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
                        content: move || teksu!(
                            DialogContent {
                                title: lit!("Adaptive modal dialog")
                                supporting_text: lit!("The framework chooses the best modal presentation for the current backend: a native modal child window when reliable, otherwise a centered in-tree dialog.")
                                body: TextWidget::new(lit!("The app code does not branch on Wayland or window-system support here; it issues one modal request and lets Teksilo resolve it.")) {
                                    style: t.body.clone()
                                    color: c.text_secondary
                                }
                                footer: Button::new(lit!("Close")) {
                                    variant: ButtonVariant::Plain
                                    on_tap: |_, ctx| ctx.dismiss_modal()
                                }
                            }
                        )
                        variant: ButtonVariant::Plain
                    }
                    Snackbar::new(lit!("Show snackbar")) {
                        content: HStack {
                            spacing: 14.0
                            TextWidget::new(lit!("Autosave complete")) {
                                style: t.body.clone()
                                color: c.tooltip_text
                            }
                            Button::new(lit!("Dismiss")) {
                                variant: ButtonVariant::Plain
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

Five things exercise the language here. First, `ScrollArea` is Category A post-refactor, so its VStack content is a body-block child. Second, the Dialog binding `modal_trigger =` uses the new assignment form to bind the dialog's id. Third, Dialog's `content:` property takes a `move ||` factory closure whose body contains a nested `teksu!(...)` building the DialogContent. Fourth, DialogContent is Category B with `title`, `supporting_text`, `body`, and `footer` as slot properties. Fifth, `trigger:` in Popover takes a full element value, and all three trigger-like widgets (Popover, Snackbar, and Dialog as a button itself) appear at the same depth in the HStack.

> **This block will not compile as written against the shipped API.** It is kept as the
> rationale's worked translation, and two things in it are stale: the `Popover::new(label)` /
> `trigger:` shape (the popover family takes its trigger in the constructor, Appendix A.1), and
> the Dialog binding, which needs `Dialog` to be a Category A or bound-capable position. Treat
> §7.6 as illustrating the *grammar*, not the current widget surface.

### 7.7 internationalization, mixed declarative and imperative

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Source:

```rust
let direction_signal = teksilo::i18n::current_direction();
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

With `teksu!`:

```rust
let root = teksu!(ctx =>
    Panel {
        padding: 24.0
        VStack {
            spacing: 16.0

            let direction_signal = teksilo::i18n::current_direction();
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
                    variant: ButtonVariant::Plain
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::SetEnglish)
                }
                Button(tr!(lang_french())) {
                    variant: ButtonVariant::Plain
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::SetFrench)
                }
                Button(tr!(lang_arabic())) {
                    variant: ButtonVariant::Plain
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::SetArabic)
                }
            }
            HStack {
                spacing: 12.0
                Button(tr!(leading_button())) { variant: ButtonVariant::Plain }
                Button(tr!(trailing_button())) { variant: ButtonVariant::Plain }
            }
        }
    }
);
```

All the hoisted `let id = ctx.add(...)` in the source collapse into declarative elements. The conditional `ctx.effect` registration goes in a side-effect `rust { }` block with a `;` on its tail. The `let` bindings for signal handles use the `let` form at body position, scoping the signals to the VStack construction. `TextWidget(tr!(...))` uses the default `::new` constructor (localized); `TextWidget::new(lit!(""))` uses the literal constructor where the source does.

### 7.8 widget-catalog, event subscription in rust block

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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
        let label = self.write_signal.map(|text| format!("TeksiloApp Widget Catalog {}", text));
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
                        .variant(ButtonVariant::Plain),
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

With `teksu!`:

```rust
impl Widget for App {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        let root = teksu!(ctx =>
            VStack {
                let write_signal = self.write_signal.clone();
                let label = self.write_signal.map(|text| format!("TeksiloApp Widget Catalog {}", text));
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
                    variant: ButtonVariant::Plain
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
                    variant: ButtonVariant::Plain
                    on_activate_fn: |ctx| ctx.send_intent(AppIntent::AddItem)
                }
                TextWidget(tr!(add_item_label())) {
                    text: item_label_for_bind
                    style: t.body.clone()
                    color: c.text_primary
                }
                Button(tr!(toggle_dark_mode_button())) {
                    variant: ButtonVariant::Plain
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

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

An illustration of the `name = Element` binding used in slot position:

```rust
// Builder
let title_id = ctx.add(
    TextWidget::new("Manuscript Title")
        .style(t.body_bold.clone())
        .color(c.text_primary),
);

let card = Card::new()
    .header(title_id)
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

With `teksu!`:

```rust
teksu!(
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

`title =` binds the TextWidget's id at the slot position; the id is available anywhere in the enclosing block, including the `on_tap` closure in the content slot's Button. The macro emits `ctx.add(TextWidget::new(...)...)` as a hoisted statement, then uses `.header(title)` on the Card. The on_tap handler captures `title` by value through `move`, which is what the user wrote.

---

## 8. Handler Attachment Rules

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

The V2 model splits handlers between two attachment patterns (architecture §28.3): handlers on child widgets (Checkbox on MinSize, Accordion on its header) and handlers attached to `self` via `HandlerSet::new()` + `ctx.apply_self_handlers()` (Button, Toggle, Slider, SegmentedControl).

`teksu!` does not change this. Handlers written on an element attach via the builder methods of that element. Which attachment mechanism the builder uses internally is a per-widget implementation detail.

For the rarer case of attaching handlers to `self` inside a widget's own `build()` method, `teksu!` is not the tool. That is infrastructure code that uses `HandlerSet` and `ctx.apply_self_handlers()` directly. `teksu!` is for constructing trees, not for authoring internals.

---

## 9. Error Reporting Discipline

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Every span the macro emits must be traceable to a user token. The `tr!` macro established the precedent: a missing translation key produces an error pointing at the key identifier. `teksu!` adheres to the same discipline.

### 9.1 Span Mapping Rules

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Type errors on widget constructors point at the type path. `Buton::new("x") { ... }` fails with `cannot find type 'Buton'` under the `Buton` identifier.

Type errors on property values point at the value expression. `TextWidget("x") { color: "red" }` fails with `expected Color, found &str` under `"red"`.

Method-not-found errors on properties point at the property name, via the compiler's existing method-resolution diagnostics.

Arity mismatches on handler closures point at the closure parameter list.

Structural form errors (`if` without a valid block, `for` without `in`) point at the structural keyword.

Parsing errors where an element prefix matched but the element failed to parse fully point at the token where parsing went wrong.

### 9.2 Common Errors

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

v3 illustrated this section with three invented messages. Two of them were never written, and
one described a rule that went the other way: commas between body items are accepted, not
rejected (§3.1). What follows is the set the macro actually emits, taken from the source and
from the committed `.stderr` fixtures under `crates/teksilo/tests/teksi/fail/`.

**Emitted by the parser** (`crates/teksilo-parse/src/parse/`):

```text
expected a property name, child element, binding, or `#{ expr }` escape
expected a single expression inside `#{ ... }`
expected a `let` binding at this body position
if-body must contain exactly one element — wrap multiple in a container like VStack
else-body must contain exactly one element
expected `let` binding in for-body
for-body may contain `let` bindings followed by exactly one element
```

**Emitted by the lowering** (`crates/teksilo-macros/src/lower.rs`):

```text
multi-arm `if` requires a final `else` branch — add `else { ... }` or drop the else-if arms
teksu! supports up to 4 if-chain arms; wrap deeper chains in `Box<dyn Widget>` or split into a helper
`match` at body position needs at least 2 arms
teksu! supports up to 4 match arms; wrap deeper dispatches in `Box<dyn Widget>` or split into a helper
```

**Emitted by the Category B check** (`crates/teksilo-parse/src/diag.rs:129`), the one
domain-aware diagnostic, verbatim from its fixture:

```text
error: `Card` is a Category B widget with named slots — use `content: <widget>` instead of a bare child element
  --> tests/teksi/fail/err_bare_child_in_category_b.rs:41:13
   |
41 |             TextWidget("hi")
   |             ^^^^^^^^^^
```

The slot name in that message is a best guess per widget (`content` for Card, `leading` for
TitleBar, `step` for Wizard, and so on). If the user wanted a different slot the hint still
names a real method and the rest of the fix is obvious.

Everything else is a native rustc diagnostic against the emitted builder chain, which is the
design intent of §9.1. A misspelled property is a method-resolution error under the user's
token:

```text
error[E0599]: no method named `nonexistent_prop` found for struct `Leaf` in the current scope
  --> tests/teksi/fail/err_unknown_property.rs:26:13
   |
11 |   struct Leaf;
   |   ----------- method `nonexistent_prop` not found for this struct
...
25 | /         Leaf {
26 | |             nonexistent_prop: 42
   | |            -^^^^^^^^^^^^^^^^ method not found in `Leaf`
   | |____________|
   |
```

and a misspelled type is a type-resolution error under the type path:

```text
error[E0433]: cannot find type `Buton` in this scope
  --> tests/teksi/fail/err_constructor_typo.rs:12:20
   |
12 |     let _ = teksu!(Buton("oops"));
   |                    ^^^^^ use of undeclared type `Buton`
```

**Two failures still surface badly**, both because the macro lacks the information to do
better, and both worth knowing:

- A comma before an UpperCamel element continues the argument list (§3.4), so the element
  lands as an extra argument to the preceding property and the user gets an arity error on a
  call they did not mean to make.
- A binding or `#{ }` escape in a slot whose widget has no `*_id` twin produces
  `no method named 'slot_id'` (Appendix A).

---

## 10. What `teksu!` Does Not Do

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

**Implicit theme access.** Every reference to theme tokens is an explicit Rust path. Implicit access would require a thread-local (fights multi-window) or an injected ctx parameter (breaks error messages). Mitigation: a small `themed!` helper macro that expands to `let t = &theme.typography; let c = &theme.colors;`.

**Implicit reactive bindings.** `text: model.title` passes the value once. To get reactivity, write `text: signal.map(...)`. This matches `Prop<T>`.

**Automatic animation syntax.** Users call `signal.animate_to(target, duration, easing)` in regular Rust.

**Hot-reload.** `teksu!` expansions are Rust code. No runtime parser, no structural hot-reload. Translation hot-reload works through `--translation-dev`.

**Inline doc comments on elements.** Users put doc comments on helper functions or use regular Rust comments inside the block. Worth revisiting later.

**Two-way binding syntax.** Two-way binding in Teksilo is expressed by passing a `Signal<T>` to a widget's bind method; the widget commits changes back through its event handlers. No `:=` form.

**Implicit closure capture.** `move` stays explicit. Rust users know the keyword; eliding it produces confusing errors.

---

## 11. Implementation Notes

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

The macro is exported through the `teksilo` umbrella as `teksilo::teksu!`. v3 put the whole
implementation in one new crate, `teksilo-macros`. **It shipped split across two**, because the
parser has a second consumer: `teksilo-parse` holds lexing, the IR and the diagnostics, and
`teksilo-macros` is the proc-macro crate that depends on it and holds only the lowering
(`crates/teksilo-macros/src/lower.rs`). The split is what lets `teksilo-fmt`, `cargo
teksilo-fmt` and `teksilo-fmt-lsp` format `teksu!` source from the same grammar, and what lets
`teksilo-teksu-guard` fail the build when the wrapping-method list in
`teksilo_parse::diag::is_widget_builder_method` falls behind the `WidgetBuilder` trait.

Four responsibilities.

**Lexical parsing** uses `syn` and a hand-written recursive-descent parser for the body grammar. `syn` handles positional-argument paren groups, body braces, and embedded Rust expressions. The "commit on distinctive prefix" rule is a fixed two-token lookahead, no backtracking.

**IR construction** produces a typed tree in `crates/teksilo-parse/src/ir.rs`. The shipped node
set is `TeksiRoot`, `TeksiElement`, `TeksiProperty`, `PropArg`, `TeksiIf`, `TeksiElse`,
`TeksiMatch`, `TeksiMatchArm`, `TeksiFor`, `RustShape`, and a `BodyItem` enum whose variants
cover the remaining productions (child, expression child, binding, escape, `let`, spread, `rust`).
v3's flat list (`TeksiBinding`, `TeksiStructural`, `TeksiSpread`, `TeksiLet`, `TeksiEscape`,
`TeksiRust`) named nodes that became `BodyItem` variants instead.

**Translation** walks the IR and emits `quote!`-generated builder calls, preserving spans via `quote_spanned!`.

**Diagnostic emission.** Statically detectable errors emit `compile_error!` with clean spans;
type errors emit clean builder calls and let the compiler's native diagnostics surface. The
shipped set is smaller than v3 assumed: bare child in a Category B context is there, but there
is no `id:`-instead-of-`=` check and no `name:`-with-no-arguments check. §9.2 lists what is
actually emitted.

**Supporting types.** `TeksiBranch<L, R>`, `TeksiBranch3`, `TeksiBranch4`, `IntoTeksiChild` and
`IntoTeksiCondition` are public in `teksilo-core`, in `widget_builder_branching.rs` rather than
`widget_builder.rs`, and re-exported from both preludes. They are not DSL-specific:
hand-written builder chains can use them. The three `TeksiBranch*` types are live, emitted by
`if`/`else` and `match` lowering. `IntoTeksiChild` and `IntoTeksiCondition` are **not**: nothing
emits them (§5.1, §6.1).

**Bootstrapping.** Develop, test, and land the macro after the framework changes in Appendix A. The `tr!` macro's infrastructure (crate layout, `trybuild` tests, span discipline, rebuild tracking) is the template. Estimated cost: four to six weeks including `trybuild` corpus and documentation rewrite.

**Test strategy, as shipped.** v3 planned three tiers and two of them do not exist. There are
**no** golden-file `cargo expand` tests (no `expect-test`, `insta` or `macrotest` dependency in
the workspace) and **no** bitwise render comparison against rewritten examples. What exists is:

- **trybuild**, the primary corpus: `crates/teksilo/tests/teksi/pass/` (28 fixtures, one per
  supported form) and `crates/teksilo/tests/teksi/fail/` (4 fixtures with committed `.stderr`),
  driven by `crates/teksilo/tests/teksi_trybuild.rs`.
- **Parser unit tests** in `crates/teksilo-parse/tests/category_b.rs`.
- **The formatter's own suite** in `teksilo-fmt`, which round-trips real corpus blocks and is
  gated in CI by `cargo teksilo-fmt --check`.
- **`teksilo-teksu-guard`**, which parses the `WidgetBuilder` trait and fails the build when the
  wrapping-method list drifts from it.

Note the shape of that corpus: it proves the macro accepts and rejects the right *source*. It
does not pin the emitted chain, so a lowering change that still compiles is invisible to it.
That is the gap a golden-file tier would have closed.

---

## 12. Summary

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

The `teksu!` language is a block-structured DSL for Teksilo widget trees. It reads like QML or Kotlin, compiles to V2 builder calls with no runtime overhead, preserves existing reactivity and capture semantics without new syntax, and produces user-facing error spans.

The grammar has three primary forms: elements with explicit constructors, bindings via `name = Element`, and properties including named slots; structural control flow (`if`, `for`, `match`, `let`, `..spread`, `rust { }`); and one escape hatch (`#{ expr }`). Each form has a mechanical desugaring into existing Teksilo infrastructure.

The widget catalog divides into two categories: Category A containers accepting body-block children, and Category B composites with named slot properties. Appendix A specifies the framework changes that complete this split, and A.6 measures how far the framework actually went.

Three things this design asked for were not built: type-directed reactive `if` (§5.1),
`IntoTeksiChild` routing for `#{ }` (§6.1), and a universal `*_id` twin (Appendix A.6). Two
things arrived after it was written: commas as optional body separators (§3.1) and the
expression child (§3.6). [teksu-macro-reference.md](teksu-macro-reference.md) is normative for
all of it.

---

## Appendix A: Required Framework Changes

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

The DSL assumes these framework changes are applied. Each is mechanical and takes roughly an
hour; all together, an afternoon.

> **Status: A.1, A.2 and A.4 landed as written. A.3 landed partially, and its headline claim
> was never true.** A.1's four constructors all take no content argument now
> (`ScrollArea::new()`, `Snackbar::new(label)`, `Dialog::new(label)`; the popover took a
> different route, see below), A.2's five `.set_*` renames are done and no `set_child` /
> `set_content` survives in `teksilo-widgets`, and A.4's `TeksiBranch*` types exist and are
> emitted. A.3 is measured in A.6.

### A.1 Category C Dissolution

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

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
Joins Category B. **What shipped differs**: there is no `Popover` type. The family is
`PopoverWidget<T: PopoverTrigger>` with the aliases `PopoverButton`, `PopoverIconButton` and
`PopoverCustom`, its constructor takes the trigger widget rather than a label
(`PopoverWidget::new(trigger)`), and `content` is its only slot.

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

> **Superseded.** The `*_id` family this section renames into was removed; a slot method
> now takes `impl IntoTeksiChild` and there is one name per slot. Kept as the record of
> the refactor that was actually performed at the time.


The `.set_*` prefix convention becomes `*_id`, matching the id twins already present elsewhere.

**Rename:**
- Panel: `.set_child(id)` → `.child_id(id)`
- Padding: `.set_child(id)` → `.child_id(id)`
- Expand: `.set_child(id)` → `.child_id(id)`
- GroupBox: `.set_child(id)` → `.child_id(id)`
- Accordion: `.set_content(id)` → `.content_id(id)`

### A.3 New Id-Taking Twins

> **Superseded.** These twins existed and were then removed: 75 slots had one and 89 did
> not, so the convention had more exceptions than instances. A slot method now takes
> `impl IntoTeksiChild`, so the same name accepts a `WidgetId` and a widget alike. Kept
> as the record of what was built at the time.


Every Category B slot method gains an `*_id` twin. The list below is what was asked for; A.6
measures what the framework has.

> **`SplitView` is gone**, so its `.first_id` / `.second_id` are not the precedent this appendix
> cited. The replacement was `Splitter`, whose id form was `.pane_id(id)` while its `.child()`
> alias had no `child_id`. **`TabWidget` never got `.tab_id(label, id)`** under that name
> either, and `tab_item` / `TabItem` do not exist; the id twins that did ship there were
> `.tab_id`, `.static_tab_id`, `.bar_leading_slot_id` and `.bar_trailing_slot_id`. All of them
> are now deleted — see the banner above.

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
pub fn bar_leading_slot_id(mut self, id: WidgetId) -> Self
pub fn bar_trailing_slot_id(mut self, id: WidgetId) -> Self
```

**PopoverWidget:** neither twin landed. `content` has no `content_id`, and `trigger` became a
constructor argument. A binding or `#{ }` escape in a popover's `content:` slot therefore does
not compile.

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

### A.4 TeksiBranch Types and IntoTeksiChild Trait

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Add to `teksilo-core`. **Shipped in `widget_builder_branching.rs`**, not `widget_builder.rs`,
and re-exported from `teksilo_core` and the `teksilo` prelude. The three `TeksiBranch*` enums
are live. `IntoTeksiChild` was written with its blanket impls and is emitted by nothing (§6.1);
a sibling trait `IntoTeksiCondition` was written for §5.1's reactive `if` and is likewise
emitted by nothing.

```rust
pub enum TeksiBranch<L: Widget, R: Widget> { L(L), R(R) }
pub enum TeksiBranch3<A: Widget, B: Widget, C: Widget> { A(A), B(B), C(C) }
pub enum TeksiBranch4<A, B, C, D> { ... }

// Widget impl for each variant dispatches to the active arm.

pub trait IntoTeksiChild { ... }
// Blanket impls for impl Widget and WidgetId.
// Used by child(), add_child(), slot_id() routing.
```

**This one landed**, in `crates/teksilo-core/src/widget_builder_branching.rs`, with one
correction to the last comment line: there is no `add_child` and no `slot_id`. A slot is one
method taking `impl IntoTeksiChild`, so the trait *is* the routing rather than something three
method families consult.

### A.5 Summary of Effort

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

Category C dissolution: 4 widgets, roughly 20 lines of change each. Method renames: 5 widgets, roughly 3 lines each. Id-taking twins: 8 widgets, roughly 30 new methods total at 3 lines each. TeksiBranch infrastructure: 1 new file, roughly 200 lines including the impls.

Total: an afternoon of mechanical work, plus test updates. The seven uploaded example files need migration to the new API; each example is a dozen lines of change on average.

### A.6 What the framework actually has

> **Superseded.** This measured the `*_id` gap in order to ask whether to close it. It was
> closed the other way: the twins were deleted and each slot became one method taking
> `impl IntoTeksiChild`, so none of the 37 gaps below is reachable any more. Kept because the
> numbers are why that decision went the way it did.

A.3's twins landed for the eight widgets it named, minus `PopoverWidget`. What did **not**
happen is the universality this appendix and the v2 changelog both claimed. The convention was
applied where it was asked for and nowhere else, so it is a convention, not an invariant, and
the DSL's binding form is unavailable wherever it was not applied.

Measured over `crates/teksilo-widgets/src`. A slot method is a `pub fn` taking
`impl Widget + 'static` and returning `Self`; the plural accumulators taking
`impl IntoIterator<Item = impl Widget + 'static>` and the `composite_tooltip` /
`child_opt` variants are excluded, and `add_child(id)` counts as `child`'s twin.

| | count |
| --- | --- |
| types with at least one slot method | 69 |
| ...of which every slot has an id twin | 50 |
| ...of which at least one slot does not | 19 |
| slot methods total | 101 |
| slot methods with no id twin | 37 |

Widgets with no twin on any slot, which is where a binding or a `#{ }` escape in a slot fails
to compile:

`StandardListItem` and `StandardTreeItem` (six slots each: `leading_slot`, `center_slot`,
`label_slot`, `trailing_slot`, `subtitle_leading_slot`, `subtitle_trailing_slot`), `RadioTile`
(`body`, `icon`, `trailing_slot`), `MenuList` (`header`, `item`, `item_when`), `MenuBar`
(`leading_slot`, `trailing_slot`), `TextInput` (`leading_slot`, `trailing_slot`), `Button`
(`leading`, `trailing`), `ToolBoxItem` (`leading`, `trailing`), `Banner` (`action`),
`CompositeTooltipWidget` (`content`), `Cycle` (`child`), `DropZone` (`icon`), `PopoverWidget`
(`content`), `RadioGroup` (`child`), `ScrollArea` (`child`, whose id path is the `from_id(id)`
constructor instead), `Stepper` (`chrome`), `Toast` (`leading`), `Wizard` (`trigger`).

One near-miss: `Splitter`'s `.child()` has no `child_id`, but `.child()` is an alias for
`.pane()` and `.pane_id()` exists, so the capability is there under the other name.

Closing the gap is the same mechanical work A.3 described, at roughly 37 methods. Whether it is
worth doing is a live question rather than a settled one: the binding form is exercised by three
of the 28 trybuild fixtures and is rare in real corpora, and §3.6's expression-child extension
removed some of the pressure that made hoisting-then-binding the common shape.

---

## Appendix B: Known Open Questions

> **Non-normative.** Design rationale. [teksu-macro-reference.md](teksu-macro-reference.md) is normative for behaviour; where the two disagree, this file is stale.

One question carried over from v2's appendix.

Should `let` bindings inside a DSL body ever produce `let mut`, or always `let`? Current answer: always `let`. A user needing a `mut` binding writes a `rust { }` block.

No other open questions remain from prior drafts. Questions about the DSL's semantics that arise during implementation should be resolved against this spec or against the architecture document, with a changelog entry here if either changes.
