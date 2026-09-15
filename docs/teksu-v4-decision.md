<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Should there be a `teksu!` v4?

**Status:** Brainstorm synthesis
**Date:** September 15, 2026
**Companion to:** teksu-language-spec-v3.md, teksu-macro-reference.md, teksilo-designer-composition-model.md
**Method:** eight parallel corpus/prior-art investigations, six independent design positions, one adversarial
verifier per position, plus first-hand probes compiled in a worktree. Every number here was reproduced by at
least two independent runs unless marked otherwise.

The no-em-dash rule applies.

---

## The answer in one paragraph

**Yes, v3 is close enough to good enough that you should not start a v4 now.** But that is not because the
DSL is fine: it ships three silent-wrong-program bugs, one of which I reproduced in three lines, and one
grammar rule that demonstrably shapes how the whole flagship application is written. Fix those first. They
are days of work, not a language revision. Then, if you still want a v4, build the two-rule grammar in §6,
which has been prototyped end to end by an independent check and works. What you should **not** do is
delete `teksu!` on the Freya precedent: all three of Freya's stated reasons were tested against `teksu!`
and none of them applies.

---

## 1. What was actually measured

`teksu!` reaches **0.85%** of Skribisto's UI crate (1,972 of 235,110 lines), **1.48%** of its
widget-building code, **4 of 55** Teksilo examples, and **zero** of the framework's own ~100 widgets. It has
88 blocks in Skribisto and 61 in Skribisto-Pro; the largest is 81 lines and none exceeds 100. Twenty-seven of
the 115 `teksu!` mentions in Skribisto are comments explaining why it was *not* used. The August 2026
migration campaign converted six files, the commit message measured the result, and the author then wrote the
next 42 files without it.

That is the case for the prosecution and it is real. But four of the charges built on it did not survive
verification, and the corrections matter more than the headline.

## 2. Four charges that collapsed

**"The DSL costs 48% more source."** It costs **+3.0%**. The 48% comes from `widget_catalog`, where the
`classic()` functions delegate to helpers defined outside their bodies while the `teksu()` twins inline them.
With call-graph attribution the same corpus is +35%, by token count +17.5%. On the only real before/after
migration in evidence, Skribisto commit `c6b022c9e` (same six files, same tree, both directions), lines go
3,107 to 3,200. And on the metric the author himself chose in that commit message, builder-child call sites,
`teksu!` wins decisively: **144 to 59**.

**"The DSL forces duplication."** Fifteen near-identical copies of one modal-card chrome are 41% of
Skribisto's teksu corpus, and the stated cause was that `fn modal_card(h, b, f) -> impl Widget` could not be
called from inside a block. A verifier rebuilt that factoring against the real widget catalog: it compiles
and mounts, from a plain Rust call site and inline as `child: modal_card(h, b, f)`. None of the 15 blocks
contains a construct a helper parameter cannot carry. The duplication is a refactor not done, not a refactor
forbidden.

**"Follow Freya: delete it."** Freya removed its `rsx!` macro in 0.4 for three reasons. All three were tested
against `teksu!` and none holds.
- *Typos caught by the compiler.* `spacng: 8.0` gives `E0599: no method named 'spacng' found for struct
  'VStack'`, caret on the user's token, `help: there is a method 'spacing' with a similar name`. Freya's
  attributes were stringly typed (`width: "fill"`); `teksu!` properties lower to real method calls.
- *IDE autocomplete.* A verifier drove rust-analyzer 0.3.3049 over raw LSP: **116 completion items at a
  teksu property position against 116 identical items for the equivalent builder chain**; 120 of `Panel`'s
  own surface when nested inside `VStack`; hover returning the real signature and doc; go-to-definition
  landing on `vstack.rs:60`; 18 enum items at a property-value position; 122 at a Category B slot.
- *Stack traces at your line.* A panic inside a block names the user's file, line and column, with no
  `teksilo-macros` frame in the backtrace.

The one real limit: **completion returns null inside a block that does not parse**, which is the state you
are in while typing. That is an argument for a more forgiving grammar, not for deletion.

**"The parse traps are live."** The dangerous `(expr, ELEMENT)` mis-parse has **zero instances** across all
178 blocks in all three corpora. The traps are latent.

## 3. Three charges that stood, and one new one

**Three silent-wrong-program bugs.** I reproduced the first myself:

```rust
teksu!(ctx => Probe { dim_when_inactive: 0.7  Leaf(1)  Leaf(2) })
```

compiles clean and builds `DimWhenInactive > Leaf`. The `Probe` parent and `Leaf(1)` are gone.
`dim_when_inactive` and `dim_when_inactive_default` are the only two `WidgetBuilder` methods returning a
different wrapper (`DimWhenInactive`, `widget_builder.rs:2023` and `:2031`) instead of
`WidgetWithHandlers<Self>`, so `is_widget_builder_method` correctly omits them, `reordered_body` does not
protect them, and `DimWhenInactive::child` *replaces* `pending_child` rather than pushing. `teksilo-teksu-guard`
cannot see it: the guard only looks for methods returning `WidgetWithHandlers<Self>`. The same shape is
reachable from a pure builder chain with no macro anywhere.

The second: `.on_tap(cb).clips_children_on(true)` double-wraps to
`WidgetWithHandlers<WidgetWithHandlers<T>>`, and `take_handler_set` (`widget_builder.rs:1950`) returns only
the outer set, so the tap handler never fires. Also pure-builder-reachable.

The third: two `if`/`else` arms binding the same name hoist two `let` statements into one root block; the
second shadows the first, and the *then* branch renders the *else* arm's widget. Compile-clean, wrong tree,
both widgets constructed unconditionally. Both docs describe binding hoist as a performance concern. It is a
correctness bug.

**One grammar rule shapes the flagship.** `parse/body.rs` decides child-versus-property by the first
letter's case, so a lowercase helper call cannot be a bare child. I compiled it: `Probe { section("one") }`
gives `expected a property name, child element, binding, or '#{ expr }' escape`, caret on the `(`, naming
none of the fixes. Skribisto has **469 widget-returning helper functions**, 5.3 per teksu block, and
**98 of its 110 `child:` values are lowercase**. The archetypal block, `tabs/analysis.rs:613`, has ten
children and not one is a bare child. Rather than a tree, it reads as the builder chain it replaced.

The escape hatch works, which is why this is a shaping force rather than a wall: 149 sites use
`child:` / `child_opt:` / `children:`. But the block form stops paying for itself the moment the tree is
made of your own components rather than the framework's, and that is exactly the band where the DSL is
absent.

**One absent gate.** `cargo teksilo-fmt --check` is documented as a pre-commit gate in
`docs/teksilo-fmt.md` and in `.claude/CLAUDE.md`, and is wired into no workflow and no pre-commit hook.
It runs over 1,166 files in 0.57 s and renders the trailing-comma trap as a diff.

## 4. The thing nobody expected: the tooling is fine

- `teksilo-fmt` reproduces **177 of 178** real corpus blocks byte for byte. The single exception
  (`FixedSize { height: height }`) is a deliberate skip and `--check` reports it clean.
- The designer's 27% round-trip fidelity is therefore **entirely its own fault**. It reuses `teksilo-parse`
  and `teksilo-fmt` and then throws the result away for a mirror IR that loses argument-free properties,
  reorders params before children, discards blank lines, and reads the first block while writing the last.
  Rebuilding the designer on the parser it already depends on is a designer task with no grammar
  prerequisite.
- Compile cost is a non-issue. Token output is 1.00x the hand-written chain, and a wall-clock A/B at 200
  elements puts `teksu!` at or slightly below hand-written builder code in every clean repeat.
- A no-rustc preview interpreter is more viable than assumed: **63.6%** of real property values are
  evaluable without Rust (literals, paths, consts, `lit!`/`tr!`), and `teksilo-preview`'s `CatalogEntry`
  registry already covers **80.3%** of the widget instances in the corpus, reaching 99.6% with 17 more
  registrations.

## 5. Recommendation

### Phase 0, now, about two days. Do this whatever you decide about the language.

1. Fix `take_handler_set` to merge recursively (a ~40-line `merge_under` on `HandlerSet` and
   `EventHandlers`). Merge `access`, do not assign it: `arena.rs:2133` already carries a comment warning
   against exactly that mistake.
2. Fix `dim_when_inactive`. It has **one** correct use in all three corpora
   (`examples/multi_window/src/main.rs:160`), so either give `WidgetWithHandlers` the inherent twin or move
   it off `WidgetBuilder` entirely and require `DimWhenInactive::new().child(w)`. Add it to
   `is_widget_builder_method` in the meantime, and widen `teksilo-teksu-guard` to cover any wrapper-returning
   method rather than only `-> WidgetWithHandlers<Self>`.
3. Wire `cargo teksilo-fmt --check` into `ci.yml` beside the rustfmt job. Note it needs
   `cargo install --path`, and the locally installed copy here is a stale 0.9.0 against a 0.10.0 tree.
4. Correct the documentation that cost roughly 10,000 lines. Seven Skribisto settings panes are excluded
   with the note that "`ListView` takes closures the DSL can't express". Every one of those forms parses
   cleanly. The claim was wrong and was copy-pasted across six files. The reference should lead with worked
   examples of `ListView`, `TabWidget`, `FormLayout`, `Switcher` and `DockingLayout`.
5. Reconcile the specs. `teksu-language-spec-v3.md` promises type-directed reactive `if` that was never
   implemented (its runtime, `IntoTeksiCondition` and `IntoTeksiChild`, is ~80 lines of exported dead code),
   promises two diagnostics that do not exist, still documents a newline-based argument rule that was
   silently replaced, and still lists the deleted `SplitView`. Either mark it historical or make the
   reference normative.

### Phase 1, about a week. The cheap change that reopens the question.

The adoption number was measured under the child rule, so it does not settle the v4 question. One patch
removes that rule's effect, and I have built and tested it:

> **A lowercase identifier that continues into a call, a method chain or an index is a Rust expression
> producing a widget, and lowers to `.child(expr)`. A lowercase identifier standing alone stays the
> argument-free property.**

This is a pure extension: every form it accepts was a parse error before, so no existing program changes
meaning. The patch is **43 lines across 4 files** (`parse/body.rs`, `ir.rs`, `lower.rs`,
`teksilo-fmt/printer.rs`), saved as `exprchild-prototype.patch`. With it applied:

- `Probe { section("a")  section("b") }` mounts two children.
- `Probe { row(1).spacing(4.0)  Leaf }` mounts two: a method chain is a bare child, which also fixes the
  74 hoisted `let`s and the `FormLayout` row problem.
- `Probe { fills  Leaf  Leaf }` still treats `fills` as a property.
- All 32 trybuild fixtures, 48 formatter tests, and the drift guard pass unchanged.
- `cargo-teksilo-fmt --check` reports 531/531 Skribisto files, 54/54 Skribisto-Pro files and 1,167/1,167
  in-tree files still clean.

Known gaps in the prototype, both out of scope for the minimal fix: a head beginning with a keyword path
(`self::section(..)`) or an operator (`*boxed()`) still hits the earlier ident guard.

Pair it with two framework changes that stand on their own merit:

- **`IntoWidget`** with blanket impls for `W: Widget`, `Box<dyn Widget>`, `WidgetId` and `Option<W>`.
  `impl Widget for Box<dyn Widget>` was verified to compile in this worktree. This makes the macro's own
  4-arm-overflow advice true (it currently recommends a type that does not implement `Widget`), removes
  Skribisto's `Boxed` adapter (30 uses, 17 files, one arena node each), and makes `child: cond.then(|| w)`
  work on all 38 containers instead of the 7 that have `child_opt`.
- **Accumulator plurals.** 25 of 41 accumulator builders lack the `impl IntoIterator` twin that the `for`
  form needs. 16 already have one; this is an inconsistency, not a design choice.

Then convert one real screen and measure again. That is the honest experiment, and it has not been run.

### Phase 2, only if Phase 1 says so.

If after Phase 1 the DSL still does not earn its keep, two exits are both verified available.

**The two-rule v4.** Parse the element head as a `syn::Expr` (eager struct braces); a `{` left in the
stream is the body. Rust's own grammar makes the cases disjoint: `Foo { a: 1 }` consumes its braces and is a
struct literal, `VStack::new() { .. }` does not and is an element. That single rule dissolves all five parse
defects by construction and lets you delete both hardcoded widget tables, the reorder rule, the binding
hoist, the structural forms, the statement-sequence lowering, `TeksiBranch`, and `teksilo-teksu-guard`
entirely. An independent verifier implemented this parser plus a migrator and ran it over every real block:
**142 blocks, 0 parse errors, 0 structural divergence from the v3 IR**. The costs are honest and were
measured: no implicit `::new` means roughly **793 constructor rewrites across ~250 blocks**;
`tab: label, Card { .. }` (a documented capability, 1 live site) becomes inexpressible; and a widget held in
a local or a const can never carry a body.

**Deletion.** A 197-line converter was written and independently reproduced: **178 of 178 blocks, 0
errors**, all 28 converted example files compile clean, 42 of 905 comments lost. If you ever want out, the
door is mechanical.

## 6. What I would not do

Do not break the grammar before Skribisto ships. Skribisto-Pro is a live path dependency and would take 61
compile errors the moment a change lands on main; the flagship would take 88. Teksilo does not go public
before Skribisto does, and a language revision is the wrong thing to be holding when the flagship needs a
stable floor.

Do not build the designer on a new grammar. It does not need one. It needs to stop reimplementing the parser
it already depends on.

Do not delete `teksu!` on the Freya precedent. Freya deleted a stringly-typed, HTML-shaped macro inherited
from a web framework. `teksu!` is a typed, Rust-shaped macro that lowers to real method calls, and the three
things Freya cited as its reasons all measure in `teksu!`'s favour.

## 6.5 What Phase 0 and Phase 1 actually landed

Both phases are implemented on branch `wt-teksu-v4`. What follows is the record, including
the three places reality differed from the plan above.

**Phase 0.**
1. `EventHandlers::merge_under` + `HandlerSet::merge_under` + a recursive
   `Widget::take_handler_set` for `WidgetWithHandlers`. Two regression tests redden when the
   mechanism is removed; two others pass either way and say so.
2. `dim_when_inactive` / `dim_when_inactive_default` removed from `WidgetBuilder`.
   `WidgetWithHandlers` gained an inherent `clips_children_on` twin (it was the only trait
   method lacking one, so the count of three in §5 was wrong: it is one).
3. `teksilo_teksu_guard::foreign_wrapper_returns` pins the set of `WidgetBuilder` methods
   returning a foreign wrapper at empty. Reintroducing `dim_when_inactive` reddens it.
4. `cargo run -p cargo-teksilo-fmt -- --check crates examples` added to `ci.yml`.
5. The reference doc gained a `Real widgets` section (`ListView`, `TreeView`, `TableView`,
   `TabWidget` in both forms, `FormLayout`, `MenuList`, `Toolbar`, `Switcher`,
   `DockingLayout`), every example compiled and mounted. The spec gained a Status block
   declaring the reference normative, plus eleven corrections. Skribisto's nine wrong module
   docs were rewritten against the real blockers.

**Where §5 was wrong about the CI gate.** The formatter does **not** flag the
single-property comma trap (`Panel { padding: 8.0, Leaf }`), because that body parses as a
Rust struct literal and `teksilo-fmt` deliberately defers those to rustfmt. It does flag
every multi-item body, and the single-property case is an `E0061` naming the extra argument,
so nothing is silent. §3 overstated this as "the worst diagnostic class".

**Phase 1.**
1. The expression-child rule, extended past the prototype: a lowercase identifier that
   continues into a call, chain or index is a child, and so is a keyword-rooted path
   (`self.row(x)`, `crate::ui::header()`) or a deref (`*boxed`). Two starts are excluded on
   purpose. A bare `(`, because body items are whitespace-separated and Rust reads
   `(a) (b)` as a call, so a parenthesised item would swallow its neighbour; use
   `child: (expr)`. And a bare `&`, because no reference type implements `Widget` and none
   can, so admitting it would only trade the parser's diagnostic for a trait-bound error
   further down.
2. `impl Widget for Box<dyn Widget>` (32 forwarding methods). The box adds no arena node and
   `as_any` forwards, so `with_widget_mut::<W>` still reaches the boxed widget.
3. `child_opt` on **38 of 38** containers, up from 7, each defined in terms of that
   container's own `child`.
4. Accumulator plurals: the dossier's "41 accumulators, 16 plural, ~25 missing" was an
   undercount from scanning only `mut self` receivers. The real figure was 113 accumulators
   across 36 types, 18 already plural; **45** were added, with 45 tests, five of them
   mutation-checked. `FormLayout::line_ids` was renamed to `line_id` and the plural took the
   name.

**The one downstream break**, established by compiling Skribisto against this branch with
`--config "patch.crates-io.teksilo.path=..."`: five `FormLayout::line_ids` call sites in
`crates/teksilo_ui/src/settings/panes/distraction_free_themes.rs:496-500`. A hard error, a
one-word fix each, and zero sites in skribisto-pro or teksilo-designer. Everything else in
both phases is additive.

**The experiment, run on the real block.** `tabs/analysis.rs:613`, the ten-children block
where not one child was bare, rewritten under the new grammar and parsed with
`teksilo_parse`: `child:` 6 to 1, `child_opt:` 3 to 0, bare children 0 to 5, and both
`child_opt: cond.then(|| ..)` closures became ordinary `if` and `if`/`else` forms. Size:
**-99 characters, +1 line**. The gain is that the block reads as a tree rather than as a
chain; it is not a terseness gain, which is consistent with the +3.0 % measured in §2.

One trap survives and should be in the reference: a bare lowercase **variable** is still an
argument-free property, not a child. Only a call or a chain becomes a child. `child: footer`
stays necessary for a pre-built local.

## 7. The honest summary

`teksu!` is a small, real win on static chrome trees of framework widgets, costs about 3% more source than
the builder chain, has better IDE support than anyone assumed, round-trips losslessly, and is used almost
nowhere. One grammar rule explains most of the "almost nowhere", and that rule can be fixed in 43 lines
without breaking anything. Three silent-wrong-program bugs are the only genuinely urgent thing about the
whole area, and two of them are framework bugs that have nothing to do with the DSL.

So: fix the bugs, wire the gate, correct the docs, land the 43 lines, convert one screen, and look again.
A v4 is a decision you are better placed to make after that than before it.
