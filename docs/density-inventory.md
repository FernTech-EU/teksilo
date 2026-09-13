<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Density inventory

Every dp dimension that `teksilo-widgets`, the three theme presets and
`teksilo-preview-ui` bake in, with the density treatment package **P20** applied to
each — the route by which that dimension reaches a density, or the reason it has
none. This is the artifact to audit conformance against, and the fixture list P40's
target-conformance gate walks; it is also the evidence behind the target numbers
quoted in [the touch design](events-and-gestures.md).

**Status: P20 has landed.** The treatment column is no longer a plan. Each row
either names the function that resolves it (`FooRecipe::for_tokens`,
`foo_bar(&InputTokens)`, `density_min_size(..)`) or says, in the same column, why
the dimension is fixed and which mechanism carries its touch conformance instead.
`crates/teksilo-widgets/tests/density_projection.rs` parses this document and holds
it to those rules, so a row that claims a treatment the code does not implement
fails a test rather than misleading a reader.

## How a dimension reaches a density

The projection is not a token read at paint time. `ComponentStyleSlots` is empty in
every shipped preset, so there is nothing in a `Theme` to re-run; instead **each
recipe reads the tokens at its own construction site, in the crate that owns it**.
Three things make that work:

1. Every `Recipe*Style` and its `*Recipe` gained `for_tokens(&InputTokens) -> Self`,
   and `Default` is now *defined as* `for_tokens(&InputTokens::default())` — so the
   Compact column below cannot drift from the shipped values.
2. Every widget's lazy fallback — `Rc::new(RecipeFooStyle::default())` — became
   `Rc::new(RecipeFooStyle::for_tokens(&ctx.theme().input))`, a one-line change at a
   site that already existed.
3. A preset that installs Tier-3 slots of its own registers a
   `teksilo_core::styles::DensityProjection` in its theme extensions.
   `Theme::with_density` calls it when present, so Fluent, macOS and Material 3
   *rebuild* their slots for the new density instead of carrying Compact dimensions
   across. A slot an **app** installed stays untouched — a hand-written style owns
   its own metrics — which is documented behaviour, not an oversight.

`WidgetTree::set_input_density` marks `BindingLevel::Rebuild`, because a dimension
baked in `build()` cannot be moved by a layout+paint mark.

## The floor rule, and why some Targets do not scale

`dp(base, TargetRole::Target, tokens)` is a **floor** — `base.max(target_size)` —
so routing a dimension whose Compact value is *below* 24 dp through it would raise
it **at Compact**, which the programme's no-regression invariant forbids. Therefore:

> A `Target` dimension is routed through `dp(.., Target, ..)` only when its Compact
> value already meets `min_target_conformance` (24 dp). Every sub-24 dimension keeps
> its painted value at every density and takes its conformance from the hit
> mechanisms — `hit_outset`, `target_regions`, `partition_targets`, the miss-only
> slop pass — which is exactly what A10 designed them for ("hit-only: no layout
> moves, Compact renders identically").

The one exception is a `MinSize`: a `MinSize` *is* a hit box, and 24 dp is the floor
that governs hit boxes, so `density_min_size` enforces it there. The sites in the
whole tree that were below it are enumerated in
[§0](#0--the-sites-that-change-at-compact), and each of them changes at Compact.
That list is the only place the count lives, so a later package that takes another
such exception adds a row there rather than a number here.

## §0 — the sites that change at Compact

The only Compact-visible changes the density work makes, and the enumeration the
rest of this document and the target-conformance gate both defer to: an exception
that is not a row here has not been taken. Each was an interactive control whose own
minimum hit box sat below WCAG 2.2 SC 2.5.8 (level AA). Rows are not call sites — the
macOS entry below is four sites, three of them on one constant and the fourth on
two rungs of its own.

| Site | Was | Is | What a reader sees at Compact |
| --- | ---: | ---: | --- |
| `teksilo-preview-ui/src/navigator.rs` — a navigator tree row | 22 dp | 24 dp | Each row in the previewer's navigator is 2 dp taller; a full-height list shows marginally fewer rows. |
| `teksilo-theme-macos` — the push button, the text field and the switch (one number: `MACOS_CONTROL_HEIGHT`), plus the icon button (`IconButtonRecipe`'s own two small rungs) | 22 dp, and **18 dp** at `IconButtonSize::Compact` | 24 dp | Four macOS-preset controls get a 24 dp minimum hit box. **The painted metric is unchanged in every one of them** — Apple's 22 dp bezel, field chrome and switch track, and the icon button's own square, which is 22 dp at its default rung and **18 dp** at `IconButtonSize::Compact` — and only the box around it moves, with the paint centred inside. So three of the four gain 2 dp and the icon button's small rung gains **6**. The button and the field take theirs from a `MinSize`; the icon button's is the shared conformance-box helper (`teksilo-widgets/src/common/conformance_box.rs`, also the calendar nav arrow's mechanism) called from `RecipeIconButtonStyle::make_body` — under every other preset, whose recipes already clear the floor, the helper is the identity and builds no box at all — and the switch's is the floor in `MacOsSwitchBody::layout_response` that its Int UI counterpart already had. Added by the preset-gate package, which measured all four under the floor at **every** density — the floor does not scale, so this was never a Compact-only shortfall. |
| `teksilo-widgets/src/code_editor/completion.rs` — a suggestion row (added by P25) | 22 dp measured | 24 dp | Up to 2 dp taller per row of the completion popup. The text and its padding are unchanged; the floor is a `MinSize` around them. The 22 dp is the row's height in a headless tree under the shipped typography — a real text backend with a taller line may already clear the floor, in which case the `MinSize` is inert at Compact as well. |

Nothing else in the rows below changes at Compact.

**One row left this table, and P40 measured why.** `text_input.rs`'s clear button
was a fourth entry here: 16 dp raised to a 24 dp `MinSize` box. A later touch
package replaced that with a hit-only mechanism — the slot is
`crate::button::HitTarget::fixed(16.0, 16.0)` declaring a `Widget::hit_outset`,
so the paint stays 16 dp at every density and the field's row does not widen.
`text_input.rs`'s own module doc states the trade ("raising its box would widen
every field in the workspace at Compact"), and the target-conformance gate
measures the result: the slot paints 16 × 16 and reaches 24 × 24 at Compact,
pinned by `an_outsets_claim_survives_the_slop_pass_in_the_shipped_controls`. It
is therefore **not** a Compact-visible change and cannot sit in a table whose
whole job is to enumerate those. That figure is the slot **with a query typed**;
with the field empty `HitTarget::active` withdraws the outset and the same slot
reaches 22 × 24 while still taking the press — a written allow-list entry of its
own, measured by the `search_field/empty` fixture, and a targeting question
rather than a dimension one. The §1 row for its old `MinSize::new` call site
is corrected in place rather than deleted, so the site P02 audited is still
accounted for.

The completion-popup row is the same exception as the others and worth stating once
more, because it is the case where the floor rule's escape hatch does not exist: a
stack of **adjacent** rows cannot take its conformance from `hit_outset`, since the
only neighbour a row could borrow space from is another row, and an outset on each
of them just moves the boundary between the two. A `MinSize` is a hit box, 24 dp is
the floor that governs hit boxes, and the paint has to move.

Dimensions live in **three** homes, and P20 has to reach all three:

1. `MinSize::new(w, h)` call sites — the existing hit-area pattern (§1).
2. `crates/teksilo-widgets/src/styles/recipe_*_style.rs` — the 39 default Tier-3
   recipes, whose module constants are the real dimension table for every themable
   widget (§2).
3. `const … : f32` in the widget modules themselves, plus the theme presets'
   own constants (§3, §4).

## Classification

| Class | Meaning | P20 treatment |
| --- | --- | --- |
| **Target** | something a finger must hit — a control, a row, a cell, a tappable column | `scales with target_size` (24 / 32 / 44 dp) **when its Compact value already clears 24 dp**; otherwise visual-fixed with a named hit mechanism |
| **Grab** | a drag affordance — a gutter, a thumb, a resize grip, an auto-scroll band | `scales with grab_size` (6 / 10 / 16 dp), or hit-only via `hit_outset` / `target_regions` with the visual preserved |
| **Spacing** | padding, gap, inset, margin, indent | `scales with spacing_factor` (1.00 / 1.15 / 1.30) |
| **Decoration** | purely visual — corner radius, hairline, glyph metric, elevation, container clamp | `fixed — <reason>` |

Two treatments recur and mean something specific:

- *"visual fixed; grab via `hit_outset`"* — the painted geometry is **unchanged at
  every density** (Compact renders identically, which is the programme invariant);
  only the hit rectangle grows. A 6 dp splitter gutter still paints 6 dp and is
  grabbable at 24 dp (44 at Touch).
- *"moves into `GestureProfile`"* — the constant is a recognizer threshold, not a
  layout dimension. It leaves the widget entirely and becomes a per-pointer-kind
  token; density does not enter into it.

## How each count was measured

```bash
# §1 — MinSize::new call sites (all, before excluding tests)
grep -rn "MinSize::new" --include=*.rs crates/ | wc -l                        # 40

# §2 — recipe files, and the one carrying a min_size field
ls crates/teksilo-widgets/src/styles/recipe_*_style.rs | wc -l                # 39
grep -rln "min_size" crates/teksilo-widgets/src/styles/recipe_*_style.rs      # 1 file
grep -rnE '^\s*(pub )?const [A-Z_0-9]+: f32' \
     crates/teksilo-widgets/src/styles/recipe_*_style.rs | wc -l              # 233

# §3 — widget-module constants outside styles/
cd crates/teksilo-widgets/src
grep -rnE '^\s*pub const [A-Z_0-9]+: f32' --include=*.rs . \
  | grep -v '/styles/' | grep -v 'tests.rs' | wc -l                           # 53
grep -rnE '^\s*const [A-Z_0-9]+: f32' --include=*.rs . \
  | grep -v '/styles/' | grep -v 'tests.rs' | wc -l                           # 113

# §4 — theme presets and the previewer UI
grep -rnE '^\s*(pub )?const [A-Z_0-9]+: f32' --include=*.rs \
     crates/teksilo-theme-{fluent,macos,material3}/src \
     crates/teksilo-preview-ui/src | grep -v tests.rs | wc -l                 # 133
```

## Totals

Class counts **after** P20's corrections (the pre-flight classification is in
[Corrections](#corrections-p20-made-to-this-document); 27 sub-24 dp `Target` rows
moved to a visual-fixed treatment, which is why `Decoration` is larger and `Target`
smaller than the P02 pass reported).

| Home | Rows | Target | Grab | Spacing | Decoration | Not a dimension |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| §1 `MinSize::new` (production) | 29 | 28 | 0 | 0 | 1 | 0 |
| §2 recipe constants | 232 | 43 | 14 | 67 | 94 | 14 |
| §3a widget-module `pub const` | 53 | 10 | 3 | 23 | 14 | 3 |
| §3b widget-module private `const` | 112 | 11 | 25 | 16 | 32 | 28 |
| §4 theme presets + previewer UI | 133 | 25 | 14 | 25 | 64 | 5 |
| **Total** | **559** | **117** | **56** | **131** | **205** | **50** |

Of those, **228 rows carry a live density route** (97 through `dp(.., Target, ..)`
or `density_min_size`, 128 through `spacing(..)`, plus the three `MinSize` floor
sites); the rest are fixed, each with its reason in its own row.

The §2 and §3b row counts are one below their grep counts (233 and 113): each table
drops a single-letter local alias declared inside a function body —
`recipe_tab_style.rs:484` (`const T: f32 = TAB_UNDERLINE_ACTIVE;`) and
`preview_catalog/icons.rs:22` (`const SZ: f32 = 16.0;`). Neither is a dimension
definition.

"Not a dimension" rows are ratios, alphas, shadow densities, epsilons and durations
that the `const … : f32` grep sweeps up; they are listed so a later reader can see
they were considered and dismissed, not missed.

## Corrections to the pre-flight estimates

| Estimate | Real | Why |
| --- | --- | --- |
| 37 non-test `MinSize::new` sites | **29** production sites (40 total) | 37 = 40 minus the three sites in files *named* `tests.rs`. It does not exclude the five sites inside inline `#[cfg(test)]` modules (`button.rs:1287-1288`, `popover_widget.rs:907`, `min_size.rs:226`/`:238`, `scroll_bar.rs:868`) nor the two doc-comment occurrences (`min_size.rs:22`, `scroll_area.rs:25`). |
| exactly one recipe has `min_size` | **confirmed** — `recipe_button_style.rs` only | The field is on `ButtonRecipe`; three variant constructors set it (lines 267, 298, 325), all to `Size::new(72.0, 24.0)`. |
| ~53 dimension constants in widget modules | **53** `pub const … : f32` outside `styles/`, of which **50 are dimensions** | The three non-dimensions are `shadow.rs:46/48/50` (`DENSITY_TOOLTIP` / `DENSITY_SURFACE` / `DENSITY_DIALOG`, unitless elevation multipliers). The 53 figure is exact but it is the *public* subset — a further **113** private `const … : f32` (112 after dropping one in-function alias) live in the same modules and P20 must touch them too (§3b): they hold the splitter gutter, the dock gutter, every `SCROLLBAR_THICKNESS`, the five auto-scroll `EDGE` / `MAX_VELOCITY` copies and the text-drag threshold. |
| ~276 `pub const` f32 in teksilo-widgets | **278** `pub const … : f32`, of which 225 are in `styles/` | `grep -rnE 'pub const [A-Z_0-9]+: f32' --include=*.rs crates/teksilo-widgets/src \| wc -l` |

## Corrections P20 made to this document

The counts above survived; four *classifications* and one *value* did not, and the
row that made the largest difference was one nobody had looked at twice.

| Correction | Rows | Why |
| --- | ---: | --- |
| **A sub-24 dp `Target` cannot "scale with `target_size`."** | 27 | `dp` is a floor, so the treatment as written would have raised 27 dimensions **at Compact** — from a 12 dp twist arrow to a 22 dp colour swatch — against the invariant the whole programme rests on. Each is now visual-fixed with the hit mechanism that carries it named. See [the floor rule](#the-floor-rule-and-why-some-targets-do-not-scale). |
| `ACCORDION_FILL_HEADER_EXTENT` is **30 dp**, not 28 | 3 | The three §1 rows quoted 28; `accordion.rs` says `30.0`. The dock header strip is 30 dp at Compact and reaches 32 / 44 above it. |
| `RAIL_ITEM_SPACING` is Spacing, not Target | 1 | A 2 dp gap between rail items was classed as something a finger must hit. |
| Four `docking/activity_bar.rs` gaps are **fixed**, not scaled | 4 | See the activity-rail note in §3b: the same two constants feed the rail's layout, its overflow-capacity estimate and its drop-insertion geometry, two of them from pure functions with their own unit tests and no theme in scope. Scaling one without the others makes the rail's capacity disagree with its layout — a worse outcome than a rail whose gaps do not grow. Owner for any future change: P30. |
| Seven rows pointed at files the module split had emptied | 7 | `EDGE` / `MAX_VELOCITY` in `list_view.rs`, `table_view.rs`, `tree_table_view.rs` now live in each view's `widget_impl.rs`, and `MEAN_ADVANCE_OVER_LINE_HEIGHT` moved to `rich_text/body.rs`. The paths are corrected; the five auto-scroll copies are now **deleted**, folded into `common::drag_autoscroll`. |
| `SPLITTER_MIN_PANE_SIZE` and `RESIZE_MIN_EDGE` are container clamps | 2 | Both are minimum *content* extents (96 dp and 24 dp) read where no theme is in scope, and `dp(.., Grab, ..)` is the identity for both at every density. Classed Decoration with the reason written at the row. |

## Corrections P25 made to this document

Every count above survived. What did not is the assumption behind them: **a
projected recipe field is only a projection if something reads it.** Four
consumers measured against the raw Compact constant while the recipe beside them
carried the projected value, so the ladder was inert for four of the most common
targets in a real application.

| Correction | Where | Why it mattered |
| --- | --- | --- |
| `StandardItemRecipe::min_height_single_line` / `min_height_two_line` had **no reader at all** | `standard_item.rs`, `StandardListItem::layout_response` | Every `ListView` / `TreeView` row measured itself against the raw 28 / 44 dp, so a Touch build laid out 28 dp rows under a theme that had already decided on 44. The two projected fields were dead. |
| `CalendarRecipe::nav_arrow_size` was not read by the arrows | `calendar/header.rs`, `NavArrow::build` and its `layout_response` | The header's chevrons stayed 24 dp at Touch while the day cells beside them grew — a strip whose two halves followed different ladders. |
| `MenuItemRecipe::item_height` was not read by the ComboBox | `combo_box/item.rs`, `DropdownItem::layout_response`; `combo_box/panel.rs`, `build_virtualized_list` | A combo box's own list stayed 24 dp while a `MenuList`'s rows reached 44, and the panel sizing itself for `max_visible_items` used the same raw number, so the two at least agreed while both were wrong. |
| The completion popup had no projection at all | `code_editor/completion.rs` | Its rows are menu rows in every other respect. Now floored through `density_min_size`, which is the §0 site above. |
| A `Target` classed "visual fixed; hit via `partition_targets`" that is **not** an in-node split | `split_button.rs`, `SPLIT_BUTTON_CHEVRON_WIDTH` | The mechanism named in the row could not be implemented as written: the chevron is its own node beside its own sibling, and partitioning a node's rectangle only makes sense when one node paints both zones. The row now names `hit_outset`. |

### One refusal, with its measurement

`docking/activity_bar.rs`'s `item_extent` maps an `IconButtonSize` to a rail
item's square extent by reading the raw `ICON_BUTTON_SIZE_*` constants, and P25
**left it that way**. The reason is the one already recorded for the rail's gaps:
the same function feeds the strip's layout, its overflow-capacity estimate and its
drop-insertion geometry, two of them pure functions with their own unit tests and
no theme in scope, and projecting one without the others makes the rail's capacity
disagree with its layout. What the projection would buy is comfort, not reach —
every `IconButtonSize` a rail can carry (24 / 24 / 30 / 40 / 50 dp) already clears
`min_target_conformance`, which is 24 dp at **every** density, and the rail's
default is 40. Owner for any future change: whoever revisits the rail's geometry
as a whole.

## Corrections P40 made to this document

The target-conformance audit measures a target's **reachable** extent rather than
its painted one, so it is the first reader of this document that could not take a
named mechanism on trust: a row naming a mechanism that does not exist measures
short and the gate says so. Five rows below were wrong, in three ways — **two**
named a mechanism the code could not implement, **two** named one that is
implemented and does not deliver, and **one** understated a floor. The first two
are the same failure P25 recorded for `SPLIT_BUTTON_CHEVRON_WIDTH` — *a
mechanism named in the treatment column that the code could not implement as
written* — which makes it the failure mode the next reader of this table should
look for first.

| Correction | Where | Why it mattered |
| --- | --- | --- |
| **The `SpinBox` step buttons are reached by nothing, and `partition_targets` was never a candidate** | `spin_box.rs`, `build_step_buttons`; `spin_box/step_button.rs`, `StepButton::new` | `partition_targets` carves **one** node's rectangle into zones, so it can only be the mechanism where a single node paints both. The two step buttons are their own nodes stacked beside their own sibling, exactly as the `SplitButton` chevron is its own node beside its own — P25's correction, recurring. Naming the wrong mechanism hid a real gap behind a plausible sentence: nothing reaches these buttons. An outset cannot (it never escapes its parent, and the parent is a column exactly one button wide and two buttons tall, so the space it would claim vertically is the other step's); the miss-only pass cannot (the field beside them takes presses of its own, so an eligible bubble owner sits at distance zero); and `TouchTarget` cannot while the pair is stacked inside the field's own height. The row now says so, and `step_button.rs` had already said it at the code. This is an allow-listed WCAG 2.2 SC 2.5.8 failure at every density, conforming by SC 2.5.8 *Equivalent* — the same value is set by typing in the field, by Up/Down and PageUp/PageDown, by the wheel, and by the `Increment` / `Decrement` assistive actions. Closing it geometrically means changing the *layout* (a side-by-side −/+ at coarse densities), which is a design decision and not a targeting one. |
| **`TAB_CLOSE_BUTTON_SIZE` is not reached by `partition_targets` either, and nothing reads it** | `recipe_tab_style.rs`; the affordance is built in `tab_widget/header.rs` | The row promised an in-node split on the tab, and P25 had already refused that for this exact affordance with the measurement: a tab's close `×` is a real `IconButton` node at `IconButtonSize::Compact`, so there is no single rectangle to carve. The correction P25 wrote into `docs/widget-pointer-inventory.md` was never carried back to this table — the twin left standing. Worse, `close_button_size` has no reader anywhere in the workspace (the field is set by the IntUI recipe and by the Fluent and macOS presets, and read by nothing), so the 16 dp it names sizes nothing at all. The affordance a finger has to hit is the `IconButton`, which clears the floor at Compact and follows the ladder. What that affordance *does* have is a reveal problem, not a size one, and it belongs to the hover census. |
| **The tree chevron's `hit_outset` is declared and inert** | `recipe_standard_item_style.rs`, `STANDARD_ITEM_CHEVRON_COLUMN_WIDTH`; the wrapper is in `standard_item.rs`, `StandardTreeItem::build` | The row named `hit_outset`, and the hook is implemented — but `build` wraps the chevron in `FixedSize::new().width(chevron_size)`, a wrapper exactly its own size on both axes, and an outset is only ever offered points every ancestor's rectangle already contains. Measured: 16 × 16 in a `TreeView` at all three densities, reach equal to paint, no mechanism credited, while the same chevron in a bare row reaches 24 × 24. This is A10's *reach limit*, recorded there from the inspector's resize strip and now measured in a shipped widget. Deleting the wrapper is necessary and not sufficient — the reach becomes 20 × 20 / 24 × 18.8 / 30 × 17.6, still short at Compact, because at depth 0 the chevron is flush against the row's content box. Allow-listed in the gate with an owner; pinned by `the_chevrons_outset_is_inert_inside_a_standard_tree_row`. |
| **A grab ring can reach past a neighbouring target's centre, and at Touch the dock gutter's does** | `docking.rs`, `DOCK_GUTTER`; the hook is in `docking/resize_handle.rs`, `DockResizeHandle::hit_outset` | The row's treatment ("grab via `hit_outset`, 24 dp, 44 at Touch") is true and incomplete: the gutter inflates to `TargetRole::Target`, so at Touch its ring is ±19 dp, and the dock tab strip begins 0 dp below it. Measured: the strip's header reaches **0 × 0** at Touch and is reachable at Compact, where the ring is ±9. A10's precedence chain settles an outset against *another outset* by distance to the uninflated rectangle and says nothing about an outset against a plain target that contains the point — the gap is in the mechanism, not in this row, but the row is where a reader meets the number. The splitter gutter one row up declares the same ring; nothing was measured against it, because the panes on either side of it in the fixture take no press. |
| **The clear button is no longer a Compact-visible `MinSize` exception** | `text_input.rs`, `TextInput::build`; the §0 table and the §1 row for its old call site | §0 listed the clear button as raised from 16 dp to a 24 dp box, and §1 listed the `MinSize::new(16.0, 16.0)` that did it. Neither is in the code: the slot is a `HitTarget::fixed(16.0, 16.0)` declaring a `hit_outset`, which is the whole point of the arrangement (`text_input.rs`'s module doc: "raising its box would widen every field in the workspace at Compact"). Measured: the slot paints 16 × 16 and reaches 24 × 24 at Compact, and reaching 22 × 24 instead is what reverting `arena.rs`'s `won_through_outset` costs — pinned by `an_outsets_claim_survives_the_slop_pass_in_the_shipped_controls`. The row mattered because everything in this document, the target-conformance gate's allow-list included, defers to §0 as *the* enumeration of Compact-visible exceptions: a row that no longer changes Compact inflates that count for every later reader. |
| **The table header's zone floor is the density's `target_size`, not a fixed 24 dp** | `table_view/header.rs`, `HeaderCell::build` and `header_cell_zones` | The paragraph under the table said "a 24 dp floor per zone". `HeaderCell::build` reads `ctx.theme().input.target_size`, so the floor is 24 / 32 / 44 — understating it made the one row where `partition_targets` genuinely *is* the mechanism look like the only one that does not follow the ladder. The same paragraph cited `header.rs:529` for where `filter_zone_width` is read; that line now holds unrelated code, and the two real readers are named by function instead. |

## Dimensions that are *not* constants

Three dimensions the audit flagged were inline literals with no constant to rename.
P20 introduced one for each; all three are below the 24 dp floor, so the constant
names a painted extent that stays put at every density, and the treatment column says
which hit mechanism — if any — makes up the shortfall.

This table's second column names the **function**, not a line: of 34 line
citations in one earlier artifact exactly one still resolved 36 packages later,
and two of the four rows P40 rewrote below carried an off-by-one. A function
name survives a refactor that renumbers a file, which is the same reason
`TargetMeasurement::path` is a chain of type names.

| File | Site | Dimension | Value | Class | P20 treatment |
| --- | --- | --- | ---: | --- | --- |
| `crates/teksilo-widgets/src/title_bar/window_frame.rs` | `WindowFrame::new` | resize-strip `thickness` field default | `6.0` | Grab | named `WINDOW_FRAME_RESIZE_THICKNESS`; visual fixed at 6 dp, grab via `hit_outset` (24 dp, 44 at Touch) — and the corner grips on the same hook reach **nothing**, which is an allow-listed finding in the target-conformance gate, pinned by `a_window_corner_grip_is_boxed_in_by_its_own_edge_strips` |
| `crates/teksilo-widgets/src/spin_box.rs` | `build_step_buttons` | `button_width` for the step buttons | `18.0` | Target | named `SPIN_BOX_STEP_BUTTON_WIDTH`; visual fixed at 18 dp, and **no hit mechanism reaches it** — an allow-listed SC 2.5.8 failure, conformance by *Equivalent* (**corrected by P40** — see [Corrections P40](#corrections-p40-made-to-this-document)) |
| `crates/teksilo-widgets/src/spin_box/step_button.rs` | `StepButton::new` | step-button `width` | `18.0` | Target | named `SPIN_BOX_STEP_BUTTON_WIDTH`; visual fixed at 18 dp, and **no hit mechanism reaches it** — an allow-listed SC 2.5.8 failure, conformance by *Equivalent* (**corrected by P40** — see [Corrections P40](#corrections-p40-made-to-this-document)) |

`table_view/header.rs` splits one header cell into a label zone and a filter zone by
coordinate inside a single node (`filter_zone_width`, read in `HeaderCell::build` and
in `HeaderCell::target_regions`); it has no dp constant at all and is handled by
`core::partition_targets` through one `header_cell_zones` function, not by a density
projection. Its floor per zone is the **density's** `target_size` — 24 dp at Compact,
32 and 44 above it — read once in `HeaderCell::build` and cached, because a density
change marks the tree at `BindingLevel::Rebuild`. It is not a fixed 24 dp.

## §1 — `MinSize::new` call sites

29 production sites. Every one is a hit-area or control-height floor; each becomes
`density_min_size(base, axes, &tokens)` under P20, except the three marked Decoration.

| File | Line | Expression | Compact value | Class | P20 treatment |
| --- | ---: | --- | ---: | --- | --- |
| `crates/teksilo-preview-ui/src/navigator.rs` | 138 | `MinSize::new(0.0, 28.0)` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-preview-ui/src/navigator.rs` | 192 | `MinSize::new(0.0, 22.0)` | 22 | Target | scales with `target_size` — **below the 24 dp AA floor today** |
| `crates/teksilo-theme-fluent/src/styles/button.rs` | 96 | `MinSize::new(0.0, MIN_HEIGHT)` | 32 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/text_input.rs` | 83 | `MinSize::new(0.0, MIN_HEIGHT)` | 32 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/button.rs` | 107 | `MinSize::new(0.0, MACOS_CONTROL_HEIGHT)` | 22 | Target | scales with `target_size` — **below the 24 dp AA floor today** |
| `crates/teksilo-theme-macos/src/styles/text_input.rs` | 81 | `MinSize::new(0.0, MACOS_CONTROL_HEIGHT)` | 22 | Target | scales with `target_size` — **below the 24 dp AA floor today** |
| `crates/teksilo-widgets/src/accordion.rs` | 464 | `MinSize::new(ACCORDION_FILL_HEADER_EXTENT, 0.0)` | 30 | Target | scales with `target_size` (horizontal dock strip) — `accordion_fill_header_extent(&InputTokens)` |
| `crates/teksilo-widgets/src/accordion.rs` | 466 | `MinSize::new(0.0, ACCORDION_FILL_HEADER_EXTENT)` | 30 | Target | scales with `target_size` — `accordion_fill_header_extent(&InputTokens)` |
| `crates/teksilo-widgets/src/checkbox.rs` | 408 | `MinSize::new(hit, hit)` from `CHECKBOX_BOX_HIT_AREA` | 24 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/command_link_button.rs` | 322 | `MinSize::new(0.0, COMMAND_LINK_BUTTON_MIN_HEIGHT)` | 64 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/date_range_edit.rs` | 602 | `MinSize::new(0.0, field_dims::TEXT_FIELD_HEIGHT)` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/date_time_edit.rs` | 737 | `MinSize::new(0.0, field_dims::TEXT_FIELD_HEIGHT)` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/docking/panel.rs` | 989 | `MinSize::new(0.0, ACCORDION_FILL_HEADER_EXTENT)` | 30 | Target | scales with `target_size` — `accordion::accordion_fill_header_extent(&InputTokens)` |
| `crates/teksilo-widgets/src/password_field.rs` | 519 | `MinSize::new(24.0, 24.0)` (reveal toggle) | 24 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/password_field.rs` | 576 | `MinSize::new(min_w, field_dims::TEXT_FIELD_HEIGHT)` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/radio_button.rs` | 270 | `MinSize::new(RADIO_HIT_AREA, RADIO_HIT_AREA)` | 24 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/search_field.rs` | 554 | `MinSize::new(0.0, 0.0)` | 0 | Decoration | fixed — a zero floor used as a layout pass-through, not a target |
| `crates/teksilo-widgets/src/split_button.rs` | 617 | `MinSize::new(SPLIT_BUTTON_MIN_WIDTH, SPLIT_BUTTON_HEIGHT)` | 72 × 24 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/spin_box.rs` | 1224 | `MinSize::new(min_width, field_dims::TEXT_FIELD_HEIGHT)` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 141 | `MinSize::new(recipe.min_size.width, recipe.min_size.height)` | 72 × 24 | Target | scales with `target_size` — the one recipe-driven site |
| `crates/teksilo-widgets/src/styles/recipe_combo_box_style.rs` | 96 | `MinSize::new(0.0, height)` from `COMBO_BOX_HEIGHT` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/styles/recipe_combo_box_style.rs` | 172 | `MinSize::new(0.0, height)` from `COMBO_BOX_HEIGHT` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/styles/recipe_split_button_style.rs` | 85 | `MinSize::new(total_min_width, SPLIT_BUTTON_HEIGHT)` | 24 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 117 | `MinSize::new(0.0, height)` from `TEXT_FIELD_HEIGHT` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 175 | `MinSize::new(0.0, height)` from `TEXT_FIELD_HEIGHT` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/text_input.rs` | `TextInput::build` (was line 746) | `MinSize::new(16.0, 16.0)` (clear button) — **call site removed** | 16 | Target | the `MinSize` is gone: the slot is now `crate::button::HitTarget::fixed(16.0, 16.0)` declaring a `Widget::hit_outset`, so the paint stays 16 dp at every density and the 24 dp target is made up between the pointer and the arena (**corrected by P40** — see [Corrections P40](#corrections-p40-made-to-this-document)) |
| `crates/teksilo-widgets/src/text_input.rs` | 810 | `MinSize::new(min_w, field_dims::TEXT_FIELD_HEIGHT)` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/tool_box.rs` | 722 | `MinSize::new(TOOL_BOX_HEADER_MIN_HEIGHT, 0.0)` | 28 | Target | scales with `target_size` (rotated header) — `density_min_size(.., &InputTokens)` |
| `crates/teksilo-widgets/src/tool_box.rs` | 724 | `MinSize::new(0.0, TOOL_BOX_HEADER_MIN_HEIGHT)` | 28 | Target | scales with `target_size` — `density_min_size(.., &InputTokens)` |

Excluded and why: `crates/teksilo-widgets/src/button.rs:1287`, `:1288`,
`crates/teksilo-widgets/src/popover_widget.rs:907`,
`crates/teksilo-widgets/src/primitives/min_size.rs:226`, `:238`,
`crates/teksilo-widgets/src/scroll_bar.rs:868` (inline `#[cfg(test)]` modules);
`crates/teksilo-widgets/src/combo_box/tests.rs:1167`, `:1201`,
`crates/teksilo-widgets/src/layout_integration_tests.rs:138` (test files);
`crates/teksilo-widgets/src/primitives/min_size.rs:22`,
`crates/teksilo-widgets/src/scroll_area.rs:25` (doc comments). One further production
site lives outside the crates tree, in
`examples/widget_catalog/src/tabs/layout.rs:244`.

## §2 — the 39 default recipes

Every themable widget's Compact dimensions are module constants in its
`recipe_*_style.rs`. Under P20 each `Recipe*Style` gains
`for_tokens(&InputTokens) -> Self`; `Default` stays and equals
`for_tokens(&InputTokens::for_density(Compact))`, so these values are the Compact
column by construction.

Exactly **one** recipe carries a `min_size` field: `recipe_button_style.rs`, on
`ButtonRecipe`, set to `Size::new(72.0, 24.0)` by all three variant constructors
(lines 267, 298, 325) and consumed at line 141. Every other recipe expresses its
target as a bare height/size constant, which is why P20 cannot go through `min_size`
alone.

### §2.1 — fields per recipe

| Recipe file | Dimension fields it carries | Count |
| --- | --- | ---: |
| `recipe_avatar_style.rs` | `AVATAR_SIZE_SMALL`, `AVATAR_SIZE_MEDIUM`, `AVATAR_SIZE_LARGE`, `AVATAR_SIZE_X_LARGE`, `AVATAR_BORDER_DEFAULT`, `AVATAR_PRESENCE_DIAMETER_RATIO`, `AVATAR_PRESENCE_DIAMETER_MIN`, `AVATAR_PRESENCE_DIAMETER_MAX`, `AVATAR_PRESENCE_OUTLINE_WIDTH`, `AVATAR_PRESENCE_INSET`, `AVATAR_FONT_RATIO_1CHAR`, `AVATAR_FONT_RATIO_2CHAR`, `AVATAR_ROUNDED_RADIUS_RATIO` | 13 |
| `recipe_badge_style.rs` | `BADGE_PADDING_HORIZONTAL`, `BADGE_PADDING_VERTICAL`, `BADGE_CORNER_RADIUS` | 3 |
| `recipe_banner_style.rs` | `BANNER_PADDING_HORIZONTAL`, `BANNER_PADDING_VERTICAL`, `BANNER_CORNER_RADIUS`, `BANNER_GLYPH_SIZE`, `BANNER_CONTENT_GAP`, `BANNER_TITLE_DESCRIPTION_GAP` | 6 |
| `recipe_button_style.rs` | `BUTTON_HEIGHT`, `BUTTON_MIN_WIDTH`, `BUTTON_PADDING_HORIZONTAL`, `BUTTON_PADDING_VERTICAL`, `BUTTON_CORNER_RADIUS`, `BUTTON_BORDER_WIDTH`, `BUTTON_ICON_SIZE`, `BUTTON_ICON_LABEL_GAP` | 8 |
| `recipe_calendar_style.rs` | `CALENDAR_OUTER_PADDING`, `CALENDAR_SECTION_GAP`, `CALENDAR_HEADER_HEIGHT`, `CALENDAR_WEEKDAY_ROW_HEIGHT`, `CALENDAR_CELL_SIZE`, `CALENDAR_CELL_RADIUS`, `CALENDAR_CELL_GAP`, `CALENDAR_TODAY_RING_WIDTH`, `CALENDAR_NAV_ICON_SIZE`, `CALENDAR_WEEK_NUMBER_COLUMN_WIDTH`, `CALENDAR_NAV_ARROW_SIZE`, `CALENDAR_NAV_ARROW_RADIUS`, `CALENDAR_HEADER_GAP`, `CALENDAR_ZOOM_CELL_RADIUS` | 14 |
| `recipe_card_style.rs` | `CARD_PADDING`, `CARD_CORNER_RADIUS`, `CARD_BORDER_WIDTH`, `CARD_SHADOW_DENSITY` | 4 |
| `recipe_checkbox_style.rs` | `CHECKBOX_BOX_VISUAL_SIZE`, `CHECKBOX_BOX_HIT_AREA`, `CHECKBOX_LABEL_GAP`, `CHECKBOX_CORNER_RADIUS` | 4 |
| `recipe_color_picker_style.rs` | `CANVAS_WIDTH`, `CANVAS_HEIGHT`, `CANVAS_CORNER_RADIUS`, `STRIP_THICKNESS`, `STRIP_LENGTH`, `STRIP_CORNER_RADIUS`, `INDICATOR_RADIUS`, `INDICATOR_OUTER_STROKE_WIDTH`, `INDICATOR_INNER_STROKE_WIDTH`, `STRIP_THUMB_WIDTH`, `STRIP_THUMB_HEIGHT`, `STRIP_THUMB_CORNER_RADIUS`, `PADDING`, `GAP`, `SWATCH_SIZE`, `SWATCH_SPACING`, `SWATCH_CORNER_RADIUS`, `SWATCH_SELECTED_STROKE_WIDTH`, `CHECKER_CELL`, `PREVIEW_WIDTH`, `PREVIEW_HEIGHT`, `PREVIEW_CORNER_RADIUS`, `SPINNER_FIELD_WIDTH`, `HEX_FIELD_WIDTH` | 24 |
| `recipe_combo_box_style.rs` | `COMBO_BOX_HEIGHT`, `COMBO_BOX_PADDING_HORIZONTAL`, `COMBO_BOX_ARROW_COLUMN_WIDTH`, `COMBO_BOX_CORNER_RADIUS` | 4 |
| `recipe_date_edit_style.rs` | `CALENDAR_BUTTON_WIDTH`, `CALENDAR_ICON_SIZE`, `SEGMENT_GAP` | 3 |
| `recipe_dialog_style.rs` | `DIALOG_CONTENT_PADDING`, `DIALOG_MIN_WIDTH`, `DIALOG_CORNER_RADIUS` | 3 |
| `recipe_drop_target_style.rs` | `DROP_TARGET_CORNER_RADIUS`, `DROP_TARGET_BORDER_WIDTH_DEFAULT`, `DROP_TARGET_BORDER_WIDTH_PROMINENT`, `DROP_TARGET_BORDER_WIDTH_SUBTLE` | 4 |
| `recipe_drop_zone_style.rs` | `DROP_ZONE_CORNER_RADIUS`, `DROP_ZONE_BORDER_WIDTH`, `DROP_ZONE_PADDING` | 3 |
| `recipe_grid_view_style.rs` | *(none — see §2.1)* | 0 |
| `recipe_icon_button_style.rs` | `ICON_BUTTON_SIZE_COMPACT`, `ICON_BUTTON_SIZE_DEFAULT`, `ICON_BUTTON_SIZE_TOOLBAR`, `ICON_BUTTON_SIZE_LARGE`, `ICON_BUTTON_SIZE_HERO`, `ICON_BUTTON_ICON_SIZE`, `ICON_BUTTON_ICON_SIZE_TOOLBAR`, `ICON_BUTTON_ICON_SIZE_LARGE`, `ICON_BUTTON_ICON_SIZE_HERO`, `ICON_BUTTON_CORNER_RADIUS` | 10 |
| `recipe_link_style.rs` | `LINK_CORNER_RADIUS`, `LINK_UNDERLINE_THICKNESS` | 2 |
| `recipe_list_container_style.rs` | *(none — see §2.1)* | 0 |
| `recipe_menu_item_style.rs` | `MENU_ITEM_HEIGHT`, `MENU_ITEM_PADDING_HORIZONTAL`, `MENU_ITEM_PADDING_LEADING`, `MENU_ICON_COLUMN_WIDTH`, `MENU_ICON_LABEL_GAP`, `MENU_SHORTCUT_LEFT_GAP`, `MENU_SEPARATOR_HEIGHT`, `MENU_ITEM_CORNER_RADIUS` | 8 |
| `recipe_panel_style.rs` | `PANEL_PADDING`, `PANEL_CORNER_RADIUS`, `PANEL_BORDER_WIDTH` | 3 |
| `recipe_popover_style.rs` | `POPOVER_PADDING`, `POPOVER_CORNER_RADIUS`, `POPOVER_BORDER_WIDTH`, `MENU_POPUP_CORNER_RADIUS`, `POPOVER_SHADOW_DENSITY` | 5 |
| `recipe_progress_bar_style.rs` | `PROGRESS_BAR_CORNER_RADIUS` | 1 |
| `recipe_radio_style.rs` | `RADIO_VISUAL_SIZE`, `RADIO_HIT_AREA`, `RADIO_LABEL_GAP`, `RADIO_INNER_DOT_SIZE` | 4 |
| `recipe_radio_tile_style.rs` | `RADIO_TILE_CORNER_RADIUS`, `RADIO_TILE_PADDING`, `RADIO_TILE_BORDER_WIDTH`, `RADIO_TILE_SELECTED_BORDER_WIDTH`, `RADIO_TILE_FOCUS_RING_WIDTH`, `RADIO_TILE_SHADOW_DENSITY`, `RADIO_TILE_VERTICAL_ROW_HEIGHT` | 7 |
| `recipe_rich_text_editor_style.rs` | *(none — see §2.1)* | 0 |
| `recipe_scroll_bar_style.rs` | `SCROLLBAR_THICKNESS_IDLE`, `SCROLLBAR_THICKNESS_HOVER`, `SCROLLBAR_MIN_THUMB_LENGTH`, `SCROLLBAR_CORNER_RADIUS`, `OVERRIDE_THUMB_ALPHA_IDLE`, `OVERRIDE_THUMB_ALPHA_HOVER`, `OVERRIDE_THUMB_ALPHA_PRESSED`, `OVERRIDE_TRACK_ALPHA` | 8 |
| `recipe_search_field_style.rs` | `GLYPH_SIZE`, `GLYPH_SLOT_WIDTH`, `INPUT_PANEL_GAP`, `PANEL_PADDING`, `PANEL_CORNER_RADIUS`, `ROW_CORNER_RADIUS`, `ROW_PADDING_HORIZONTAL`, `ROW_PADDING_VERTICAL`, `ROW_HEIGHT` | 9 |
| `recipe_segmented_control_style.rs` | `SEGMENTED_CONTROL_HEIGHT`, `SEGMENTED_CONTROL_PADDING_HORIZONTAL`, `SEGMENTED_CONTROL_PADDING_VERTICAL`, `SEGMENTED_CONTROL_CORNER_RADIUS`, `SEGMENTED_CONTROL_BORDER_WIDTH` | 5 |
| `recipe_slider_style.rs` | `MIN_CROSS_SIZE`, `SLIDER_TRACK_HEIGHT`, `SLIDER_THUMB_DIAMETER`, `SLIDER_TICK_SIZE` | 4 |
| `recipe_snackbar_style.rs` | `SNACKBAR_PADDING_HORIZONTAL`, `SNACKBAR_PADDING_VERTICAL`, `SNACKBAR_CORNER_RADIUS` | 3 |
| `recipe_spin_box_style.rs` | *(none — see §2.1)* | 0 |
| `recipe_split_button_style.rs` | *(none — see §2.1)* | 0 |
| `recipe_splitter_style.rs` | `SPLITTER_DIVIDER_LINE_THICKNESS`, `HOVER_DWELL_DELAY_FRAC` | 2 |
| `recipe_standard_item_style.rs` | `STANDARD_ITEM_ICON_SIZE`, `STANDARD_ITEM_SUBTITLE_ICON_SIZE`, `STANDARD_ITEM_SLOT_GAP`, `STANDARD_ITEM_SUBTITLE_SLOT_GAP`, `STANDARD_ITEM_LABEL_SUBTITLE_GAP`, `STANDARD_ITEM_PADDING_HORIZONTAL`, `STANDARD_ITEM_PADDING_VERTICAL`, `STANDARD_ITEM_MIN_HEIGHT_SINGLE_LINE`, `STANDARD_ITEM_MIN_HEIGHT_TWO_LINE`, `STANDARD_ITEM_LABEL_COLUMN_MIN_WIDTH`, `STANDARD_ITEM_CHEVRON_COLUMN_WIDTH`, `STANDARD_ITEM_TREE_INDENT_STEP`, `STANDARD_ITEM_ITEM_CORNER_RADIUS`, `STANDARD_ITEM_BG_HORIZONTAL_INSET`, `STANDARD_ITEM_FOCUS_RING_WIDTH`, `STANDARD_ITEM_SELECTION_EDGE_WIDTH` | 16 |
| `recipe_tab_style.rs` | `TAB_EDITOR_HEIGHT`, `TAB_TOOL_WINDOW_HEIGHT`, `TAB_PADDING_HORIZONTAL`, `TAB_UNDERLINE_ACTIVE`, `TAB_UNDERLINE_HOVER`, `TAB_CLOSE_BUTTON_SIZE`, `DROP_INDICATOR_WIDTH` | 7 |
| `recipe_table_style.rs` | `ROW_HEIGHT`, `HEADER_HEIGHT`, `CELL_PADDING_HORIZONTAL`, `CELL_PADDING_VERTICAL`, `RESIZE_HANDLE_WIDTH`, `COLUMN_RESIZE_STEP`, `GRID_LINE_THICKNESS`, `CORNER_RADIUS`, `SORT_INDICATOR_SIZE`, `FILTER_INDICATOR_SIZE`, `HEADER_INTER_CELL_SPACING`, `FOCUS_RING_INSET`, `MIN_COLUMN_WIDTH_DEFAULT`, `TREE_INDENT_PER_LEVEL`, `TREE_TWIST_SIZE`, `TREE_TWIST_LABEL_GAP` | 16 |
| `recipe_text_input_style.rs` | `TEXT_FIELD_HEIGHT`, `TEXT_FIELD_PADDING_HORIZONTAL`, `TEXT_FIELD_PADDING_VERTICAL`, `TEXT_FIELD_BORDER_WIDTH`, `TEXT_FIELD_CORNER_RADIUS`, `TEXT_FIELD_CARET_WIDTH`, `TEXT_FIELD_VALIDATION_STRIP_GAP` | 7 |
| `recipe_toast_style.rs` | `TOAST_PADDING_HORIZONTAL`, `TOAST_PADDING_VERTICAL`, `TOAST_CORNER_RADIUS`, `TOAST_GLYPH_SIZE`, `TOAST_CONTENT_GAP`, `TOAST_TITLE_BODY_GAP`, `TOAST_BODY_ACTIONS_GAP` | 7 |
| `recipe_toggle_style.rs` | `TOGGLE_TRACK_WIDTH`, `TOGGLE_TRACK_HEIGHT`, `TOGGLE_THUMB_DIAMETER`, `TOGGLE_THUMB_INSET` | 4 |
| `recipe_tooltip_style.rs` | `TOOLTIP_PADDING_HORIZONTAL`, `TOOLTIP_PADDING_VERTICAL`, `TOOLTIP_CORNER_RADIUS`, `TOOLTIP_MAX_WIDTH`, `TOOLTIP_SHADOW_DENSITY`, `COMPOSITE_TOOLTIP_PADDING_HORIZONTAL`, `COMPOSITE_TOOLTIP_PADDING_VERTICAL`, `COMPOSITE_TOOLTIP_CORNER_RADIUS`, `COMPOSITE_TOOLTIP_MAX_WIDTH`, `COMPOSITE_TOOLTIP_MAX_HEIGHT`, `COMPOSITE_TOOLTIP_SHADOW_DENSITY` | 11 |

Five recipes carry no dimension constant of their own:
`recipe_grid_view_style.rs` (colours + a focus-ring width described in prose only),
`recipe_list_container_style.rs` (surface fill only),
`recipe_rich_text_editor_style.rs` and `recipe_spin_box_style.rs` (both re-export
`recipe_text_input_style as field_dims` and reuse `TEXT_FIELD_*`), and
`recipe_split_button_style.rs` (reuses the `SPLIT_BUTTON_*` constants that live in
`split_button.rs`, §3a).

### §2.2 — every recipe constant

| File | Line | Identifier | Value | Class | P20 treatment |
| --- | ---: | --- | ---: | --- | --- |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 39 | `AVATAR_SIZE_SMALL` | `24.0` | Target | scales with `target_size` — `AvatarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 40 | `AVATAR_SIZE_MEDIUM` | `32.0` | Target | scales with `target_size` — `AvatarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 41 | `AVATAR_SIZE_LARGE` | `48.0` | Target | scales with `target_size` — `AvatarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 42 | `AVATAR_SIZE_X_LARGE` | `64.0` | Target | scales with `target_size` — `AvatarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 46 | `AVATAR_BORDER_DEFAULT` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 49 | `AVATAR_PRESENCE_DIAMETER_RATIO` | `0.28` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 50 | `AVATAR_PRESENCE_DIAMETER_MIN` | `8.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 51 | `AVATAR_PRESENCE_DIAMETER_MAX` | `20.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 53 | `AVATAR_PRESENCE_OUTLINE_WIDTH` | `1.5` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 55 | `AVATAR_PRESENCE_INSET` | `0.0` | Spacing | scales with `spacing_factor` — `AvatarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 58 | `AVATAR_FONT_RATIO_1CHAR` | `0.45` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 59 | `AVATAR_FONT_RATIO_2CHAR` | `0.40` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_avatar_style.rs` | 62 | `AVATAR_ROUNDED_RADIUS_RATIO` | `0.25` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_badge_style.rs` | 25 | `BADGE_PADDING_HORIZONTAL` | `6.0` | Spacing | scales with `spacing_factor` — `BadgeRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_badge_style.rs` | 26 | `BADGE_PADDING_VERTICAL` | `1.0` | Spacing | scales with `spacing_factor` — `BadgeRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_badge_style.rs` | 29 | `BADGE_CORNER_RADIUS` | `9999.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_banner_style.rs` | 30 | `BANNER_PADDING_HORIZONTAL` | `12.0` | Spacing | scales with `spacing_factor` — `BannerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_banner_style.rs` | 31 | `BANNER_PADDING_VERTICAL` | `10.0` | Spacing | scales with `spacing_factor` — `BannerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_banner_style.rs` | 32 | `BANNER_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_banner_style.rs` | 35 | `BANNER_GLYPH_SIZE` | `16.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_banner_style.rs` | 38 | `BANNER_CONTENT_GAP` | `10.0` | Spacing | scales with `spacing_factor` — `BannerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_banner_style.rs` | 41 | `BANNER_TITLE_DESCRIPTION_GAP` | `2.0` | Spacing | scales with `spacing_factor` — `BannerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 38 | `BUTTON_HEIGHT` | `24.0` | Target | scales with `target_size` — `RecipeButtonStyle::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 39 | `BUTTON_MIN_WIDTH` | `72.0` | Target | scales with `target_size` — `RecipeButtonStyle::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 40 | `BUTTON_PADDING_HORIZONTAL` | `14.0` | Spacing | scales with `spacing_factor` — `RecipeButtonStyle::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 41 | `BUTTON_PADDING_VERTICAL` | `0.0` | Spacing | scales with `spacing_factor` — `RecipeButtonStyle::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 42 | `BUTTON_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 43 | `BUTTON_BORDER_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 44 | `BUTTON_ICON_SIZE` | `16.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_button_style.rs` | 45 | `BUTTON_ICON_LABEL_GAP` | `4.0` | Spacing | scales with `spacing_factor` — `RecipeButtonStyle::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 42 | `CALENDAR_OUTER_PADDING` | `8.0` | Spacing | scales with `spacing_factor` — `CalendarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 44 | `CALENDAR_SECTION_GAP` | `4.0` | Spacing | scales with `spacing_factor` — `CalendarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 46 | `CALENDAR_HEADER_HEIGHT` | `28.0` | Target | scales with `target_size` — `CalendarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 48 | `CALENDAR_WEEKDAY_ROW_HEIGHT` | `20.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 50 | `CALENDAR_CELL_SIZE` | `32.0` | Target | scales with `target_size` — `CalendarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 52 | `CALENDAR_CELL_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 54 | `CALENDAR_CELL_GAP` | `0.0` | Spacing | scales with `spacing_factor` — `CalendarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 56 | `CALENDAR_TODAY_RING_WIDTH` | `1.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 58 | `CALENDAR_NAV_ICON_SIZE` | `12.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 60 | `CALENDAR_WEEK_NUMBER_COLUMN_WIDTH` | `28.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 62 | `CALENDAR_NAV_ARROW_SIZE` | `24.0` | Target | scales with `target_size` — `CalendarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 64 | `CALENDAR_NAV_ARROW_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 66 | `CALENDAR_HEADER_GAP` | `4.0` | Spacing | scales with `spacing_factor` — `CalendarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_calendar_style.rs` | 68 | `CALENDAR_ZOOM_CELL_RADIUS` | `6.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_card_style.rs` | 32 | `CARD_PADDING` | `16.0` | Spacing | scales with `spacing_factor` — `CardRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_card_style.rs` | 33 | `CARD_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_card_style.rs` | 34 | `CARD_BORDER_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_card_style.rs` | 36 | `CARD_SHADOW_DENSITY` | `0.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_checkbox_style.rs` | 30 | `CHECKBOX_BOX_VISUAL_SIZE` | `19.0` | Decoration | fixed — visual box; the target is `CHECKBOX_BOX_HIT_AREA` |
| `crates/teksilo-widgets/src/styles/recipe_checkbox_style.rs` | 31 | `CHECKBOX_BOX_HIT_AREA` | `24.0` | Target | scales with `target_size` — `CheckboxRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_checkbox_style.rs` | 32 | `CHECKBOX_LABEL_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `CheckboxRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_checkbox_style.rs` | 33 | `CHECKBOX_CORNER_RADIUS` | `3.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 27 | `CANVAS_WIDTH` | `224.0` | Target | scales with `target_size` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 28 | `CANVAS_HEIGHT` | `192.0` | Target | scales with `target_size` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 29 | `CANVAS_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 32 | `STRIP_THICKNESS` | `14.0` | Grab | visual fixed at 14 dp — the thumb is the grab target, not the strip; hit via `target_regions` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 33 | `STRIP_LENGTH` | `192.0` | Target | scales with `target_size` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 34 | `STRIP_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 37 | `INDICATOR_RADIUS` | `7.0` | Grab | visual fixed; hit via `target_regions` on the HSV canvas indicator |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 38 | `INDICATOR_OUTER_STROKE_WIDTH` | `1.5` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 39 | `INDICATOR_INNER_STROKE_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 44 | `STRIP_THUMB_WIDTH` | `18.0` | Grab | visual fixed; hit via `target_regions` (24 dp, 44 at Touch) |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 45 | `STRIP_THUMB_HEIGHT` | `8.0` | Grab | visual fixed; hit via `target_regions` (24 dp, 44 at Touch) |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 46 | `STRIP_THUMB_CORNER_RADIUS` | `2.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 49 | `PADDING` | `12.0` | Spacing | scales with `spacing_factor` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 50 | `GAP` | `10.0` | Spacing | scales with `spacing_factor` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 53 | `SWATCH_SIZE` | `22.0` | Target | visual fixed at 22 dp (below the floor); coarse hit via `hit_outset` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 54 | `SWATCH_SPACING` | `6.0` | Spacing | scales with `spacing_factor` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 55 | `SWATCH_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 56 | `SWATCH_SELECTED_STROKE_WIDTH` | `2.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 60 | `CHECKER_CELL` | `6.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 66 | `PREVIEW_WIDTH` | `64.0` | Target | scales with `target_size` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 67 | `PREVIEW_HEIGHT` | `28.0` | Target | scales with `target_size` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 68 | `PREVIEW_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 71 | `SPINNER_FIELD_WIDTH` | `56.0` | Target | scales with `target_size` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_color_picker_style.rs` | 72 | `HEX_FIELD_WIDTH` | `96.0` | Target | scales with `target_size` — `ColorPickerRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_combo_box_style.rs` | 42 | `COMBO_BOX_HEIGHT` | `28.0` | Target | scales with `target_size` — `ComboBoxRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_combo_box_style.rs` | 43 | `COMBO_BOX_PADDING_HORIZONTAL` | `9.0` | Spacing | scales with `spacing_factor` — `ComboBoxRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_combo_box_style.rs` | 44 | `COMBO_BOX_ARROW_COLUMN_WIDTH` | `23.0` | Decoration | fixed — an in-node column inside the combo box's own target, not a target itself |
| `crates/teksilo-widgets/src/styles/recipe_combo_box_style.rs` | 45 | `COMBO_BOX_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_date_edit_style.rs` | 23 | `CALENDAR_BUTTON_WIDTH` | `24.0` | Target | scales with `target_size` — `DateEditRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_date_edit_style.rs` | 25 | `CALENDAR_ICON_SIZE` | `14.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_date_edit_style.rs` | 28 | `SEGMENT_GAP` | `1.0` | Spacing | scales with `spacing_factor` — `DateEditRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_dialog_style.rs` | 31 | `DIALOG_CONTENT_PADDING` | `24.0` | Spacing | scales with `spacing_factor` — `DialogRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_dialog_style.rs` | 32 | `DIALOG_MIN_WIDTH` | `280.0` | Target | scales with `target_size` — `DialogRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_dialog_style.rs` | 33 | `DIALOG_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_drop_target_style.rs` | 53 | `DROP_TARGET_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_drop_target_style.rs` | 55 | `DROP_TARGET_BORDER_WIDTH_DEFAULT` | `2.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_drop_target_style.rs` | 57 | `DROP_TARGET_BORDER_WIDTH_PROMINENT` | `3.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_drop_target_style.rs` | 59 | `DROP_TARGET_BORDER_WIDTH_SUBTLE` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_drop_zone_style.rs` | 26 | `DROP_ZONE_CORNER_RADIUS` | `12.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_drop_zone_style.rs` | 28 | `DROP_ZONE_BORDER_WIDTH` | `2.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_drop_zone_style.rs` | 30 | `DROP_ZONE_PADDING` | `20.0` | Spacing | scales with `spacing_factor` — `DropZoneRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 32 | `ICON_BUTTON_SIZE_COMPACT` | `24.0` | Target | scales with `target_size` — `IconButtonRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 33 | `ICON_BUTTON_SIZE_DEFAULT` | `24.0` | Target | scales with `target_size` — `IconButtonRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 34 | `ICON_BUTTON_SIZE_TOOLBAR` | `30.0` | Target | scales with `target_size` — `IconButtonRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 35 | `ICON_BUTTON_SIZE_LARGE` | `40.0` | Target | scales with `target_size` — `IconButtonRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 36 | `ICON_BUTTON_SIZE_HERO` | `50.0` | Target | scales with `target_size` — `IconButtonRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 37 | `ICON_BUTTON_ICON_SIZE` | `16.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 38 | `ICON_BUTTON_ICON_SIZE_TOOLBAR` | `18.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 39 | `ICON_BUTTON_ICON_SIZE_LARGE` | `24.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 40 | `ICON_BUTTON_ICON_SIZE_HERO` | `32.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs` | 41 | `ICON_BUTTON_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_link_style.rs` | 20 | `LINK_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_link_style.rs` | 21 | `LINK_UNDERLINE_THICKNESS` | `1.0` | Decoration | fixed — hairline rule |
| `crates/teksilo-widgets/src/styles/recipe_menu_item_style.rs` | 33 | `MENU_ITEM_HEIGHT` | `24.0` | Target | scales with `target_size` — `MenuItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_menu_item_style.rs` | 35 | `MENU_ITEM_PADDING_HORIZONTAL` | `12.0` | Spacing | scales with `spacing_factor` — `MenuItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_menu_item_style.rs` | 37 | `MENU_ITEM_PADDING_LEADING` | `6.0` | Spacing | scales with `spacing_factor` — `MenuItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_menu_item_style.rs` | 38 | `MENU_ICON_COLUMN_WIDTH` | `16.0` | Decoration | fixed — glyph column; the row carries the target |
| `crates/teksilo-widgets/src/styles/recipe_menu_item_style.rs` | 39 | `MENU_ICON_LABEL_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `MenuItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_menu_item_style.rs` | 40 | `MENU_SHORTCUT_LEFT_GAP` | `24.0` | Spacing | scales with `spacing_factor` — `MenuItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_menu_item_style.rs` | 41 | `MENU_SEPARATOR_HEIGHT` | `9.0` | Spacing | scales with `spacing_factor` — `MenuItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_menu_item_style.rs` | 43 | `MENU_ITEM_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_panel_style.rs` | 35 | `PANEL_PADDING` | `12.0` | Spacing | scales with `spacing_factor` — `PanelRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_panel_style.rs` | 36 | `PANEL_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_panel_style.rs` | 37 | `PANEL_BORDER_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_popover_style.rs` | 32 | `POPOVER_PADDING` | `16.0` | Spacing | scales with `spacing_factor` — `PopoverRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_popover_style.rs` | 33 | `POPOVER_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_popover_style.rs` | 34 | `POPOVER_BORDER_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_popover_style.rs` | 37 | `MENU_POPUP_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_popover_style.rs` | 39 | `POPOVER_SHADOW_DENSITY` | `0.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_progress_bar_style.rs` | 32 | `PROGRESS_BAR_CORNER_RADIUS` | `2.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_radio_style.rs` | 29 | `RADIO_VISUAL_SIZE` | `19.0` | Decoration | fixed — visual dot; the target is `RADIO_HIT_AREA` |
| `crates/teksilo-widgets/src/styles/recipe_radio_style.rs` | 30 | `RADIO_HIT_AREA` | `24.0` | Target | scales with `target_size` — `RadioRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_radio_style.rs` | 31 | `RADIO_LABEL_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `RadioRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_radio_style.rs` | 32 | `RADIO_INNER_DOT_SIZE` | `7.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_radio_tile_style.rs` | 41 | `RADIO_TILE_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_radio_tile_style.rs` | 42 | `RADIO_TILE_PADDING` | `14.0` | Spacing | scales with `spacing_factor` — `RadioTileRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_radio_tile_style.rs` | 43 | `RADIO_TILE_BORDER_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_radio_tile_style.rs` | 44 | `RADIO_TILE_SELECTED_BORDER_WIDTH` | `1.5` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_radio_tile_style.rs` | 45 | `RADIO_TILE_FOCUS_RING_WIDTH` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_radio_tile_style.rs` | 47 | `RADIO_TILE_SHADOW_DENSITY` | `0.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_radio_tile_style.rs` | 50 | `RADIO_TILE_VERTICAL_ROW_HEIGHT` | `44.0` | Target | scales with `target_size` — `RadioTileRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 37 | `SCROLLBAR_THICKNESS_IDLE` | `4.0` | Grab | visual fixed; coarse grab via `hit_outset` (48 dp) + `target_regions` |
| `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 38 | `SCROLLBAR_THICKNESS_HOVER` | `8.0` | Grab | visual fixed; coarse grab via `hit_outset` (48 dp) + `target_regions` |
| `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 39 | `SCROLLBAR_MIN_THUMB_LENGTH` | `24.0` | Grab | scales with `target_size` (24 dp Compact → 44 dp Touch) — `ScrollBarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 40 | `SCROLLBAR_CORNER_RADIUS` | `2.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 47 | `OVERRIDE_THUMB_ALPHA_IDLE` | `0.50` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 48 | `OVERRIDE_THUMB_ALPHA_HOVER` | `0.72` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 49 | `OVERRIDE_THUMB_ALPHA_PRESSED` | `0.92` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 50 | `OVERRIDE_TRACK_ALPHA` | `0.12` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 20 | `GLYPH_SIZE` | `14.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 23 | `GLYPH_SLOT_WIDTH` | `22.0` | Decoration | fixed — glyph slot; the field carries the target |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 26 | `INPUT_PANEL_GAP` | `2.0` | Spacing | scales with `spacing_factor` — `SearchFieldRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 28 | `PANEL_PADDING` | `4.0` | Spacing | scales with `spacing_factor` — `SearchFieldRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 30 | `PANEL_CORNER_RADIUS` | `6.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 32 | `ROW_CORNER_RADIUS` | `2.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 33 | `ROW_PADDING_HORIZONTAL` | `10.0` | Spacing | scales with `spacing_factor` — `SearchFieldRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 34 | `ROW_PADDING_VERTICAL` | `4.0` | Spacing | scales with `spacing_factor` — `SearchFieldRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_search_field_style.rs` | 35 | `ROW_HEIGHT` | `26.0` | Target | scales with `target_size` — `SearchFieldRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_segmented_control_style.rs` | 28 | `SEGMENTED_CONTROL_HEIGHT` | `24.0` | Target | **projected but unread — this row's original claim was half true.** `for_tokens` really does yield `height = 24 -> 44`, but **nothing reads `recipe.height`** (`recipe_combo_box_style.rs` and `recipe_text_input_style.rs` read theirs; this one has no reader), and `segmented_control.rs`'s `layout_response` floors on the raw `SEGMENTED_CONTROL_HEIGHT` const instead. Measured: 36 dp at all three densities. Same class as the four projected-but-unread recipe fields the P25 note records; conformance is separately measured by P40, which lists two segmented-control fixtures. |
| `crates/teksilo-widgets/src/styles/recipe_segmented_control_style.rs` | 29 | `SEGMENTED_CONTROL_PADDING_HORIZONTAL` | `12.0` | Spacing | scales with `spacing_factor` — `SegmentedControlRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_segmented_control_style.rs` | 30 | `SEGMENTED_CONTROL_PADDING_VERTICAL` | `6.0` | Spacing | scales with `spacing_factor` — `SegmentedControlRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_segmented_control_style.rs` | 31 | `SEGMENTED_CONTROL_CORNER_RADIUS` | `3.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_segmented_control_style.rs` | 32 | `SEGMENTED_CONTROL_BORDER_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_slider_style.rs` | 30 | `MIN_CROSS_SIZE` | `24.0` | Target | **fixed at every density — this row's original claim was false.** It is a module-level `const` read directly in `SliderBody::layout_response`, not a recipe field, and `SliderRecipe::for_tokens` takes `_tokens` (provably unused). Measured through `set_input_density`: Slider is 24 dp at Compact, Comfortable AND Touch, where Button is 24/32/44. Conformance is not affected and is not this row's business: `Slider` declares `target_regions` and is a P40 fixture, so the gate measures its thumb by its declared region. |
| `crates/teksilo-widgets/src/styles/recipe_slider_style.rs` | 33 | `SLIDER_TRACK_HEIGHT` | `4.0` | Grab | fixed — track is decoration; the thumb carries the target |
| `crates/teksilo-widgets/src/styles/recipe_slider_style.rs` | 34 | `SLIDER_THUMB_DIAMETER` | `14.0` | Grab | visual fixed; hit via `SliderStyle::thumb_diameter_for(&self, cfg, &InputTokens)` + `target_regions` (24/44 dp) |
| `crates/teksilo-widgets/src/styles/recipe_slider_style.rs` | 35 | `SLIDER_TICK_SIZE` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_snackbar_style.rs` | 26 | `SNACKBAR_PADDING_HORIZONTAL` | `12.0` | Spacing | scales with `spacing_factor` — `SnackbarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_snackbar_style.rs` | 27 | `SNACKBAR_PADDING_VERTICAL` | `10.0` | Spacing | scales with `spacing_factor` — `SnackbarRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_snackbar_style.rs` | 28 | `SNACKBAR_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_splitter_style.rs` | 28 | `SPLITTER_DIVIDER_LINE_THICKNESS` | `1.0` | Decoration | fixed — hairline rule |
| `crates/teksilo-widgets/src/styles/recipe_splitter_style.rs` | 33 | `HOVER_DWELL_DELAY_FRAC` | `0.75` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 28 | `STANDARD_ITEM_ICON_SIZE` | `16.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 29 | `STANDARD_ITEM_SUBTITLE_ICON_SIZE` | `12.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 30 | `STANDARD_ITEM_SLOT_GAP` | `8.0` | Spacing | scales with `spacing_factor` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 31 | `STANDARD_ITEM_SUBTITLE_SLOT_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 32 | `STANDARD_ITEM_LABEL_SUBTITLE_GAP` | `2.0` | Spacing | scales with `spacing_factor` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 33 | `STANDARD_ITEM_PADDING_HORIZONTAL` | `8.0` | Spacing | scales with `spacing_factor` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 34 | `STANDARD_ITEM_PADDING_VERTICAL` | `4.0` | Spacing | scales with `spacing_factor` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 35 | `STANDARD_ITEM_MIN_HEIGHT_SINGLE_LINE` | `28.0` | Target | scales with `target_size` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 36 | `STANDARD_ITEM_MIN_HEIGHT_TWO_LINE` | `44.0` | Target | scales with `target_size` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 41 | `STANDARD_ITEM_LABEL_COLUMN_MIN_WIDTH` | `48.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 43 | `STANDARD_ITEM_CHEVRON_COLUMN_WIDTH` | `16.0` | Target | visual fixed at 16 dp (below the floor); `hit_outset` is declared and **delivers nothing here** — `StandardTreeItem::build` wraps the chevron in a `FixedSize` of exactly this width, and an outset is only offered points every ancestor already contains (**corrected by P40** — see [Corrections P40](#corrections-p40-made-to-this-document)) |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 43 | `STANDARD_ITEM_TREE_INDENT_STEP` | `16.0` | Spacing | scales with `spacing_factor` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 44 | `STANDARD_ITEM_ITEM_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 45 | `STANDARD_ITEM_BG_HORIZONTAL_INSET` | `4.0` | Spacing | scales with `spacing_factor` — `StandardItemRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 47 | `STANDARD_ITEM_FOCUS_RING_WIDTH` | `1.5` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs` | 54 | `STANDARD_ITEM_SELECTION_EDGE_WIDTH` | `1.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_tab_style.rs` | 45 | `TAB_EDITOR_HEIGHT` | `50.0` | Target | scales with `target_size` — `TabRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_tab_style.rs` | 46 | `TAB_TOOL_WINDOW_HEIGHT` | `28.0` | Target | scales with `target_size` — `TabRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_tab_style.rs` | 47 | `TAB_PADDING_HORIZONTAL` | `12.0` | Spacing | scales with `spacing_factor` — `TabRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_tab_style.rs` | 48 | `TAB_UNDERLINE_ACTIVE` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_tab_style.rs` | 49 | `TAB_UNDERLINE_HOVER` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_tab_style.rs` | 51 | `TAB_CLOSE_BUTTON_SIZE` | `16.0` | Target | visual fixed at 16 dp, but it has **no reader in this workspace** and so carries no target: the tab's close affordance is a real `IconButton` at `IconButtonSize::Compact`, 24 dp at Compact and on the ladder above it (**corrected by P40** — see [Corrections P40](#corrections-p40-made-to-this-document)) |
| `crates/teksilo-widgets/src/styles/recipe_tab_style.rs` | 52 | `DROP_INDICATOR_WIDTH` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 35 | `ROW_HEIGHT` | `28.0` | Target | scales with `target_size` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 37 | `HEADER_HEIGHT` | `32.0` | Target | scales with `target_size` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 39 | `CELL_PADDING_HORIZONTAL` | `8.0` | Spacing | scales with `spacing_factor` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 41 | `CELL_PADDING_VERTICAL` | `4.0` | Spacing | scales with `spacing_factor` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 49 | `RESIZE_HANDLE_WIDTH` | `4.0` | Grab | visual fixed at 4 dp; grab via `hit_outset` (24 dp, 44 at Touch) |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 53 | `COLUMN_RESIZE_STEP` | `8.0` | Grab | fixed — keyboard step, not a hit dimension |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 55 | `GRID_LINE_THICKNESS` | `1.0` | Decoration | fixed — hairline rule |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 57 | `CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 59 | `SORT_INDICATOR_SIZE` | `10.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 61 | `FILTER_INDICATOR_SIZE` | `12.0` | Decoration | fixed — indicator glyph; the header cell carries the target |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 63 | `HEADER_INTER_CELL_SPACING` | `0.0` | Spacing | scales with `spacing_factor` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 65 | `FOCUS_RING_INSET` | `1.0` | Spacing | scales with `spacing_factor` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 67 | `MIN_COLUMN_WIDTH_DEFAULT` | `32.0` | Target | scales with `target_size` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 69 | `TREE_INDENT_PER_LEVEL` | `16.0` | Spacing | scales with `spacing_factor` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 72 | `TREE_TWIST_SIZE` | `12.0` | Target | visual fixed at 12 dp (below the floor); coarse hit via `hit_outset`, which is credited but earns **one side only** — the chevron sits at its cell's leading edge, measuring 18 × 24 at Compact and 22 × 28 at Comfortable, clearing the floor at Touch (**corrected by P40** — see [Corrections P40](#corrections-p40-made-to-this-document)) |
| `crates/teksilo-widgets/src/styles/recipe_table_style.rs` | 73 | `TREE_TWIST_LABEL_GAP` | `4.0` | Spacing | scales with `spacing_factor` — `TableRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 46 | `TEXT_FIELD_HEIGHT` | `28.0` | Target | scales with `target_size` — `TextInputRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 47 | `TEXT_FIELD_PADDING_HORIZONTAL` | `4.0` | Spacing | scales with `spacing_factor` — `TextInputRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 48 | `TEXT_FIELD_PADDING_VERTICAL` | `4.0` | Spacing | scales with `spacing_factor` — `TextInputRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 49 | `TEXT_FIELD_BORDER_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 50 | `TEXT_FIELD_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 51 | `TEXT_FIELD_CARET_WIDTH` | `1.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/styles/recipe_text_input_style.rs` | 52 | `TEXT_FIELD_VALIDATION_STRIP_GAP` | `4.0` | Spacing | scales with `spacing_factor` — `TextInputRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_toast_style.rs` | 26 | `TOAST_PADDING_HORIZONTAL` | `14.0` | Spacing | scales with `spacing_factor` — `ToastRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_toast_style.rs` | 28 | `TOAST_PADDING_VERTICAL` | `12.0` | Spacing | scales with `spacing_factor` — `ToastRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_toast_style.rs` | 31 | `TOAST_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_toast_style.rs` | 34 | `TOAST_GLYPH_SIZE` | `16.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/styles/recipe_toast_style.rs` | 37 | `TOAST_CONTENT_GAP` | `12.0` | Spacing | scales with `spacing_factor` — `ToastRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_toast_style.rs` | 39 | `TOAST_TITLE_BODY_GAP` | `2.0` | Spacing | scales with `spacing_factor` — `ToastRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_toast_style.rs` | 41 | `TOAST_BODY_ACTIONS_GAP` | `8.0` | Spacing | scales with `spacing_factor` — `ToastRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_toggle_style.rs` | 36 | `TOGGLE_TRACK_WIDTH` | `28.0` | Grab | scales with `target_size` (the track IS the Toggle target) |
| `crates/teksilo-widgets/src/styles/recipe_toggle_style.rs` | 37 | `TOGGLE_TRACK_HEIGHT` | `16.0` | Grab | fixed — track and knob are one coupled pill; the Toggle's target is the whole control |
| `crates/teksilo-widgets/src/styles/recipe_toggle_style.rs` | 38 | `TOGGLE_THUMB_DIAMETER` | `12.0` | Grab | fixed — the whole Toggle is the target, not the knob |
| `crates/teksilo-widgets/src/styles/recipe_toggle_style.rs` | 39 | `TOGGLE_THUMB_INSET` | `2.0` | Spacing | scales with `spacing_factor` — `ToggleRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 27 | `TOOLTIP_PADDING_HORIZONTAL` | `10.0` | Spacing | scales with `spacing_factor` — `TooltipRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 28 | `TOOLTIP_PADDING_VERTICAL` | `6.0` | Spacing | scales with `spacing_factor` — `TooltipRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 29 | `TOOLTIP_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 30 | `TOOLTIP_MAX_WIDTH` | `320.0` | Decoration | fixed — container clamp, not a target |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 32 | `TOOLTIP_SHADOW_DENSITY` | `1.0` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 34 | `COMPOSITE_TOOLTIP_PADDING_HORIZONTAL` | `12.0` | Spacing | scales with `spacing_factor` — `TooltipRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 35 | `COMPOSITE_TOOLTIP_PADDING_VERTICAL` | `12.0` | Spacing | scales with `spacing_factor` — `TooltipRecipe::for_tokens` |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 36 | `COMPOSITE_TOOLTIP_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 37 | `COMPOSITE_TOOLTIP_MAX_WIDTH` | `480.0` | Decoration | fixed — container clamp, not a target |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 38 | `COMPOSITE_TOOLTIP_MAX_HEIGHT` | `480.0` | Decoration | fixed — container clamp, not a target |
| `crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs` | 40 | `COMPOSITE_TOOLTIP_SHADOW_DENSITY` | `0.7` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |

## §3 — widget-module constants outside `styles/`

### §3a — `pub const … : f32` (53 rows, 50 of them dimensions)

| File | Line | Identifier | Value | Class | P20 treatment |
| --- | ---: | --- | ---: | --- | --- |
| `crates/teksilo-widgets/src/accordion.rs` | 118 | `ACCORDION_HEADER_HEIGHT` | `28.0` | Target | scales with `target_size` — `accordion_header_height(&InputTokens)` |
| `crates/teksilo-widgets/src/accordion.rs` | 120 | `ACCORDION_HEADER_PADDING_HORIZONTAL` | `8.0` | Spacing | scales with `spacing_factor` — `accordion_header_padding_horizontal(&InputTokens)` |
| `crates/teksilo-widgets/src/accordion.rs` | 122 | `ACCORDION_INDICATOR_SIZE` | `12.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/accordion.rs` | 124 | `ACCORDION_INDICATOR_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `accordion_indicator_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/accordion.rs` | 126 | `ACCORDION_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/breadcrumb.rs` | 86 | `BREADCRUMB_ITEM_HEIGHT` | `20.0` | Target | visual fixed at 20 dp (below the floor); coarse hit via `hit_outset` |
| `crates/teksilo-widgets/src/breadcrumb.rs` | 88 | `BREADCRUMB_ITEM_PADDING_HORIZONTAL` | `6.0` | Spacing | scales with `spacing_factor` — `breadcrumb_item_padding_horizontal(&InputTokens)` |
| `crates/teksilo-widgets/src/breadcrumb.rs` | 90 | `BREADCRUMB_SEPARATOR_GAP` | `4.0` | Spacing | scales with `spacing_factor` — `breadcrumb_separator_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/breadcrumb.rs` | 92 | `BREADCRUMB_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/command_link_button.rs` | 37 | `COMMAND_LINK_BUTTON_ICON_SIZE` | `28.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/command_link_button.rs` | 38 | `COMMAND_LINK_BUTTON_ICON_TEXT_GAP` | `14.0` | Spacing | scales with `spacing_factor` — `command_link_button_icon_text_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/command_link_button.rs` | 39 | `COMMAND_LINK_BUTTON_TITLE_DESCRIPTION_GAP` | `4.0` | Spacing | scales with `spacing_factor` — `command_link_button_title_description_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/command_link_button.rs` | 40 | `COMMAND_LINK_BUTTON_PADDING_HORIZONTAL` | `16.0` | Spacing | scales with `spacing_factor` — `command_link_button_padding_horizontal(&InputTokens)` |
| `crates/teksilo-widgets/src/command_link_button.rs` | 41 | `COMMAND_LINK_BUTTON_PADDING_VERTICAL` | `14.0` | Spacing | scales with `spacing_factor` — `command_link_button_padding_vertical(&InputTokens)` |
| `crates/teksilo-widgets/src/command_link_button.rs` | 42 | `COMMAND_LINK_BUTTON_MIN_HEIGHT` | `64.0` | Target | scales with `target_size` — `command_link_button_min_height(&InputTokens)` |
| `crates/teksilo-widgets/src/group_box.rs` | 52 | `GROUP_BOX_CONTENT_INDENT` | `24.0` | Spacing | scales with `spacing_factor` — `group_box_content_indent(&InputTokens)` |
| `crates/teksilo-widgets/src/group_box.rs` | 54 | `GROUP_BOX_TITLE_CONTENT_SPACING` | `8.0` | Spacing | scales with `spacing_factor` — `group_box_title_content_spacing(&InputTokens)` |
| `crates/teksilo-widgets/src/group_box.rs` | 56 | `GROUP_BOX_CHECKBOX_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `group_box_checkbox_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/primitives/divider.rs` | 89 | `DIVIDER_THICKNESS` | `1.0` | Decoration | fixed — hairline rule |
| `crates/teksilo-widgets/src/shadow.rs` | 46 | `DENSITY_TOOLTIP` | `1.0` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/shadow.rs` | 48 | `DENSITY_SURFACE` | `0.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/shadow.rs` | 50 | `DENSITY_DIALOG` | `0.3` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/split_button.rs` | 66 | `SPLIT_BUTTON_HEIGHT` | `24.0` | Target | scales with `target_size` — `split_button_height(&InputTokens)` |
| `crates/teksilo-widgets/src/split_button.rs` | 67 | `SPLIT_BUTTON_MIN_WIDTH` | `72.0` | Target | scales with `target_size` — `split_button_min_width(&InputTokens)` |
| `crates/teksilo-widgets/src/split_button.rs` | 68 | `SPLIT_BUTTON_PADDING_HORIZONTAL` | `14.0` | Spacing | scales with `spacing_factor` — `split_button_padding_horizontal(&InputTokens)` |
| `crates/teksilo-widgets/src/split_button.rs` | 69 | `SPLIT_BUTTON_PADDING_VERTICAL` | `0.0` | Spacing | scales with `spacing_factor` — `split_button_padding_vertical(&InputTokens)` |
| `crates/teksilo-widgets/src/split_button.rs` | 70 | `SPLIT_BUTTON_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-widgets/src/split_button.rs` | 71 | `SPLIT_BUTTON_BORDER_WIDTH` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/split_button.rs` | 72 | `SPLIT_BUTTON_CHEVRON_WIDTH` | `22.0` | Target | visual fixed at 22 dp; hit via `hit_outset` on the `ChevronRegion` node, whole shortfall on the leading edge (**corrected by P25** — the two halves are separate nodes, so `partition_targets` would have had to move the painted boundary at Compact) |
| `crates/teksilo-widgets/src/split_button.rs` | 73 | `SPLIT_BUTTON_DIVIDER_WIDTH` | `1.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/split_button.rs` | 74 | `SPLIT_BUTTON_CHEVRON_ICON_SIZE` | `12.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/split_button.rs` | 76 | `SPLIT_BUTTON_ICON_LABEL_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `split_button_icon_label_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/splitter/model.rs` | 46 | `SPLITTER_GUTTER_THICKNESS` | `6.0` | Grab | visual fixed at 6 dp; grab via `hit_outset` (24 dp, 44 at Touch) |
| `crates/teksilo-widgets/src/splitter/model.rs` | 48 | `SPLITTER_MIN_PANE_SIZE` | `96.0` | Decoration | fixed — container clamp, not a target: it is a pane's minimum *content* extent (96 dp, four times the floor), read from `PaneDescriptor::default` where no theme is in scope |
| `crates/teksilo-widgets/src/splitter/model.rs` | 50 | `SPLITTER_KEYBOARD_STEP` | `24.0` | Grab | fixed — keyboard step, not a hit dimension |
| `crates/teksilo-widgets/src/splitter/model.rs` | 52 | `SPLITTER_SNAP_OFFSET` | `30.0` | Grab | fixed — snap distance, not a hit dimension |
| `crates/teksilo-widgets/src/status_bar.rs` | 37 | `STATUS_BAR_HEIGHT` | `22.0` | Decoration | fixed — container band; the items inside it carry the targets |
| `crates/teksilo-widgets/src/status_bar.rs` | 38 | `STATUS_BAR_PADDING_HORIZONTAL` | `8.0` | Spacing | scales with `spacing_factor` — `status_bar_padding_horizontal(&InputTokens)` |
| `crates/teksilo-widgets/src/status_bar.rs` | 39 | `STATUS_BAR_ITEM_GAP` | `2.0` | Spacing | scales with `spacing_factor` — `status_bar_item_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 100 | `DEFAULT_PINNED_TAB_WIDTH` | `32.0` | Target | scales with `target_size` — `default_pinned_tab_width(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 88 | `DEFAULT_MIN_TAB_WIDTH` | `96.0` | Target | scales with `target_size` — `default_min_tab_width(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 90 | `DEFAULT_MAX_TAB_WIDTH` | `240.0` | Decoration | fixed — container clamp, not a target |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 95 | `DEFAULT_TAB_SPACING` | `0.0` | Spacing | scales with `spacing_factor` — `default_tab_spacing(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 98 | `DEFAULT_BAR_SLOT_SPACING` | `8.0` | Spacing | scales with `spacing_factor` — `default_bar_slot_spacing(&InputTokens)` |
| `crates/teksilo-widgets/src/toast/body.rs` | 60 | `TOAST_BODY_DISCLOSURE_GAP` | `2.0` | Spacing | scales with `spacing_factor` — `toast_body_disclosure_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/toast/body.rs` | 63 | `TOAST_DISCLOSURE_ACTION_GAP` | `12.0` | Spacing | scales with `spacing_factor` — `toast_disclosure_action_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/toolbar.rs` | 92 | `TOOLBAR_HEIGHT_DEFAULT` | `40.0` | Target | scales with `target_size` — `toolbar_height_default(&InputTokens)` |
| `crates/teksilo-widgets/src/toolbar.rs` | 93 | `TOOLBAR_SPACING` | `4.0` | Spacing | scales with `spacing_factor` — `toolbar_spacing(&InputTokens)` |
| `crates/teksilo-widgets/src/tool_box.rs` | 240 | `TOOL_BOX_HEADER_MIN_HEIGHT` | `28.0` | Target | scales with `target_size` — `tool_box_header_min_height(&InputTokens)` |
| `crates/teksilo-widgets/src/tool_box.rs` | 241 | `TOOL_BOX_HEADER_PADDING_HORIZONTAL` | `12.0` | Spacing | scales with `spacing_factor` — `tool_box_header_padding_horizontal(&InputTokens)` |
| `crates/teksilo-widgets/src/tool_box.rs` | 242 | `TOOL_BOX_ICON_TEXT_SPACING` | `8.0` | Spacing | scales with `spacing_factor` — `tool_box_icon_text_spacing(&InputTokens)` |
| `crates/teksilo-widgets/src/tool_box.rs` | 243 | `TOOL_BOX_CHEVRON_SIZE` | `12.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/tool_box.rs` | 244 | `TOOL_BOX_INDICATOR_THICKNESS` | `1.0` | Decoration | fixed — hairline rule |

### §3b — private `const … : f32` (113 rows)

Not public API, but P20 had to reach them anyway: this is where the splitter gutter,
the dock gutter, all five `SCROLLBAR_THICKNESS` copies, the five duplicated
`EDGE` / `MAX_VELOCITY` auto-scroll pairs and the text-drag threshold lived.

**The five auto-scroll copies are gone.** `ListView`, `TreeView`, `TableView`,
`TreeTableView`, `GridView`'s marquee and the `TabBar` strip each carried their own
`EDGE = 32.0` / `MAX_VELOCITY = 12.0` and their own copy of the ramp; all six now
call [`common::drag_autoscroll`](../crates/teksilo-widgets/src/common/drag_autoscroll.rs).
Its `band_for(PointerKind)` is the reason the constants left this table rather than
gaining a density route: the edge band is a property of the *device* — a cursor
lands where it is put, a fingertip's reported centre wanders — so it widens for a
coarse pointer (32 → 64 dp) and is unchanged for a precise one. It does not move
with `TargetDensity`, which describes how big the *UI's* targets are. The velocity
cap is a rate, not a dimension, and is the same everywhere.

**The activity rail's gaps are fixed, and that is a decision, not an omission.**
`RAIL_ITEM_SPACING` and `RAIL_PADDING` are read by three co-dependent computations:
the rail's layout (`VStack::spacing`, `Padding::uniform`), its overflow-capacity
estimate (`item_stride` → `shown_capacity`) and its drop-insertion geometry
(`rail_insertion`). Two of those are pure functions with their own unit tests and no
theme in scope. Scaling the layout without the capacity estimate would make the rail
show more items than fit; threading tokens through the pure pair would rewrite
assertions this package is not allowed to touch. The rail's touch conformance comes
from its *items* — `DockRailItemSize` resolves through `IconButtonRecipe`, which is
`target_size`-routed — not from the gaps between them. Owner for any future change:
**P30**.

| File | Line | Identifier | Value | Class | P20 treatment |
| --- | ---: | --- | ---: | --- | --- |
| `crates/teksilo-widgets/src/accordion.rs` | 661 | `GAP` | `2.0` | Spacing | scales with `spacing_factor` — named `ACCORDION_FILL_GAP` (it was a function-local `const`) and resolved by `accordion_fill_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/animations/collapse.rs` | 42 | `COLLAPSED_PROGRESS_EPSILON` | `0.005` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/animations/shake.rs` | 44 | `DEFAULT_AMPLITUDE` | `8.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/animations/shake.rs` | 45 | `DEFAULT_CYCLES` | `4.0` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/animations/smooth_size.rs` | 65 | `SIZE_CHANGE_EPSILON` | `0.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/animations/unroll.rs` | 57 | `ROLLED_UP_PROGRESS_EPSILON` | `0.005` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/breadcrumb.rs` | 60 | `FALLBACK_CHAR_WIDTH` | `8.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/breadcrumb.rs` | 61 | `FALLBACK_LINE_HEIGHT` | `16.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/calendar/zoom_grid.rs` | 41 | `CELL_SPACING` | `4.0` | Spacing | scales with `spacing_factor` — `cell_spacing(&InputTokens)` |
| `crates/teksilo-widgets/src/code_editor/gutter.rs` | 49 | `GUTTER_PAD_LEADING` | `8.0` | Spacing | scales with `spacing_factor` — `gutter_pad_leading(&InputTokens)` |
| `crates/teksilo-widgets/src/code_editor/gutter.rs` | 50 | `GUTTER_PAD_TRAILING` | `12.0` | Spacing | scales with `spacing_factor` — `gutter_pad_trailing(&InputTokens)` |
| `crates/teksilo-widgets/src/code_editor/log_stream.rs` | 66 | `FOLLOW_EPSILON` | `1.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/code_editor/log_view.rs` | 50 | `SCROLLBAR_THICKNESS` | `12.0` | Grab | visual fixed; coarse grab via `hit_outset` + `target_regions` |
| `crates/teksilo-widgets/src/code_editor/mouse.rs` | 175 | `MARGIN` | `20.0` | Decoration | fixed — the editor's own auto-scroll band, tighter than `common::drag_autoscroll`'s 32 dp because a caret drag inside text should start scrolling later; a finger selects text through P26's handles, not this ramp |
| `crates/teksilo-widgets/src/code_editor/mouse.rs` | 176 | `MAX_PER_SEC` | `60.0 * 60.0` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/code_editor/widget.rs` | 45 | `SCROLLBAR_THICKNESS` | `12.0` | Grab | visual fixed; coarse grab via `hit_outset` + `target_regions` |
| `crates/teksilo-widgets/src/combo_box.rs` | 1098 | `MIN_WIDTH` | `120.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/command_palette.rs` | 100 | `SELECTION_MARKER_WIDTH` | `3.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/command_palette.rs` | 97 | `ROW_HEIGHT` | `44.0` | Target | scales with `target_size` — `row_height(&InputTokens)` |
| `crates/teksilo-widgets/src/common/column_geometry.rs` | 106 | `EPSILON` | `1e-4` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/common/column_geometry.rs` | 458 | `EPSILON` | `1e-4` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/common/editor_runtime.rs` | 38 | `CARET_BLINK_INTERVAL` | `0.5` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/common/editor_runtime.rs` | 43 | `DEBOUNCE_WINDOW_SECS` | `0.150` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/docking/activity_bar.rs` | 113 | `RAIL_ITEM_SPACING` | `2.0` | Spacing | fixed — the rail's gaps feed three co-dependent computations (layout, overflow capacity, drop-insertion geometry); see the activity-rail note in §3b |
| `crates/teksilo-widgets/src/docking/activity_bar.rs` | 115 | `RAIL_PADDING` | `4.0` | Spacing | fixed — the rail's gaps feed three co-dependent computations (layout, overflow capacity, drop-insertion geometry); see the activity-rail note in §3b |
| `crates/teksilo-widgets/src/docking/activity_bar.rs` | 118 | `LABELED_TITLE_ALLOWANCE` | `72.0` | Spacing | fixed — the rail's gaps feed three co-dependent computations (layout, overflow capacity, drop-insertion geometry); see the activity-rail note in §3b |
| `crates/teksilo-widgets/src/docking/activity_bar.rs` | 121 | `LABELED_TOP_MARGIN` | `6.0` | Spacing | fixed — the rail's gaps feed three co-dependent computations (layout, overflow capacity, drop-insertion geometry); see the activity-rail note in §3b |
| `crates/teksilo-widgets/src/docking/geometry.rs` | 32 | `EPS` | `0.01` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/docking/resize_handle.rs` | 34 | `KEYBOARD_STEP` | `16.0` | Grab | fixed — keyboard step, not a hit dimension |
| `crates/teksilo-widgets/src/docking/resize_handle.rs` | 35 | `SNAP_OFFSET` | `30.0` | Grab | fixed — snap distance, not a hit dimension |
| `crates/teksilo-widgets/src/docking.rs` | 65 | `COLLAPSED_EPS` | `0.01` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/docking.rs` | 67 | `DOCK_GUTTER` | `6.0` | Grab | visual fixed at 6 dp; grab via `hit_outset` (24 dp, 44 at Touch) — and at Touch that ring reaches **past the centre line of the dock tab strip beside it**, which the audit reports as an unreachable target (**corrected by P40** — see [Corrections P40](#corrections-p40-made-to-this-document)) |
| `crates/teksilo-widgets/src/drop_target/overlay.rs` | 40 | `REGION_FILL_ALPHA` | `0.22` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/drop_target.rs` | 134 | `DEFAULT_ZONE_SIZE_FACTOR` | `0.2` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/grid_view.rs` | 131 | `SCROLLBAR_THICKNESS` | `12.0` | Grab | visual fixed; coarse grab via `hit_outset` + `target_regions` |
| `crates/teksilo-widgets/src/grid_view/selection.rs` | 65 | `MARQUEE_EDGE_ZONE` | `32.0` | Grab | now an alias of `common::drag_autoscroll::EDGE_BAND_PRECISE`; the ramp is that module's `step`, and `marquee_auto_scroll_step_for(.., PointerKind)` widens the band for a finger |
| `crates/teksilo-widgets/src/grid_view/selection.rs` | 67 | `MARQUEE_MAX_VELOCITY` | `12.0` | Grab | fixed — now an alias of `common::drag_autoscroll::MAX_VELOCITY`; a rate, not a hit dimension |
| `crates/teksilo-widgets/src/list_view.rs` | 108 | `DEFAULT_ITEM_HEIGHT` | `32.0` | Target | scales with `target_size` — `default_item_height(&InputTokens)` |
| `crates/teksilo-widgets/src/list_view.rs` | 110 | `SCROLLBAR_THICKNESS` | `12.0` | Grab | visual fixed; coarse grab via `hit_outset` + `target_regions` |
| `crates/teksilo-widgets/src/list_view/widget_impl.rs` | 709 | `EDGE` | `32.0` | Grab | **deleted** — the five hand-rolled copies of this ramp are folded into `common::drag_autoscroll`, whose `band_for(PointerKind)` is 32 dp for a precise pointer (unchanged) and 64 dp for a finger |
| `crates/teksilo-widgets/src/list_view/widget_impl.rs` | 710 | `MAX_VELOCITY` | `12.0` | Grab | **deleted** — folded into `common::drag_autoscroll::MAX_VELOCITY`; a rate, not a dimension, so it is the same under every pointer and every density |
| `crates/teksilo-widgets/src/menu_item.rs` | 123 | `MENU_INDICATOR_GLYPH_SIZE` | `12.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/message_box.rs` | 429 | `SEVERITY_ICON_SIZE` | `48.0` | Target | scales with `target_size` — `severity_icon_size(&InputTokens)` |
| `crates/teksilo-widgets/src/message_box.rs` | 438 | `DETAILS_MAX_HEIGHT` | `220.0` | Decoration | fixed — container clamp, not a target |
| `crates/teksilo-widgets/src/notification/log.rs` | 76 | `DEFAULT_PREFERRED_WIDTH` | `380.0` | Target | scales with `target_size` — `default_preferred_width(&InputTokens)` |
| `crates/teksilo-widgets/src/notification/log.rs` | 80 | `DEFAULT_PREFERRED_HEIGHT` | `320.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/primitives/column_flow.rs` | 80 | `DEFAULT_MIN_COLUMN_WIDTH` | `240.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/primitives/image_widget.rs` | 386 | `EPS` | `0.01` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/primitives/linear_layout.rs` | 34 | `EPS` | `1.0e-3` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/primitives/text_input_field.rs` | 100 | `SCROLL_MARGIN` | `4.0` | Spacing | scales with `spacing_factor` — `scroll_margin(&InputTokens)` |
| `crates/teksilo-widgets/src/primitives/text_input_field.rs` | 107 | `DEFAULT_TEXT_HEIGHT` | `20.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/progress_bar.rs` | 59 | `DEFAULT_THICKNESS` | `4.0` | Decoration | fixed — hairline rule |
| `crates/teksilo-widgets/src/progress_bar.rs` | 64 | `INDETERMINATE_SWEEP_RATIO` | `0.42` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/radio_tile.rs` | 64 | `TILE_ROW_GAP` | `10.0` | Spacing | scales with `spacing_factor` — `tile_row_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/radio_tile.rs` | 66 | `TILE_TITLE_DESC_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `tile_title_desc_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/rich_text/mouse.rs` | 125 | `TEXT_DRAG_THRESHOLD` | `4.0` | Grab | moves into `GestureProfile.drag_slop` (5 dp mouse / 18 dp touch), not a density scale — P26 owns the switch |
| `crates/teksilo-widgets/src/rich_text/mouse.rs` | 53 | `RESIZE_MIN_EDGE` | `24.0` | Decoration | fixed — a minimum image edge (a content clamp), not a hit dimension; `dp(24, Grab, ..)` is the identity at every density anyway, and the grip's coarse hit is `Widget::hit_outset` (P26) |
| `crates/teksilo-widgets/src/rich_text/mouse.rs` | 61 | `RESIZE_HANDLE_SLOP` | `5.0` | Grab | absorbed by `Widget::hit_outset` with `PointerKindMask::ALL` |
| `crates/teksilo-widgets/src/rich_text/paint.rs` | 156 | `SELECTED_IMAGE_TINT_ALPHA` | `0.28` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/rich_text/paint.rs` | 163 | `SELECTED_IMAGE_BORDER_WIDTH` | `2.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-widgets/src/rich_text/body.rs` | 54 | `MEAN_ADVANCE_OVER_LINE_HEIGHT` | `0.335` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/segmented_control/overflow.rs` | 40 | `EPS` | `0.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/segmented_control.rs` | 114 | `FALLBACK_LINE_HEIGHT` | `16.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/segmented_control.rs` | 118 | `OVERFLOW_ICON_SIZE` | `12.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/shadow.rs` | 55 | `SUB_PERCEPTUAL` | `1.0 / 255.0` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/spin_box.rs` | 214 | `MIN_WIDTH_WITH_BUTTONS` | `72.0` | Target | scales with `target_size` — `min_width_with_buttons(&InputTokens)` |
| `crates/teksilo-widgets/src/spin_box.rs` | 217 | `MIN_WIDTH_NO_BUTTONS` | `48.0` | Target | scales with `target_size` — `min_width_no_buttons(&InputTokens)` |
| `crates/teksilo-widgets/src/spin_box.rs` | 221 | `DEFAULT_PREFERRED_WIDTH` | `120.0` | Target | scales with `target_size` — `default_preferred_width(&InputTokens)` |
| `crates/teksilo-widgets/src/spin_box/step_button.rs` | 43 | `INITIAL_DELAY_SECS` | `0.400` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/spin_box/step_button.rs` | 46 | `MIN_INTERVAL_SECS` | `0.045` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/spin_box/step_button.rs` | 48 | `START_INTERVAL_SECS` | `0.120` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/spin_box/step_button.rs` | 52 | `ACCELERATION` | `0.88` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/spinner.rs` | 42 | `DEFAULT_SIZE` | `20.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/spinner.rs` | 44 | `DEFAULT_ARC_FRACTION` | `0.25` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/spinner.rs` | 45 | `DEFAULT_STROKE_FRACTION` | `0.12` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/splitter/distribute.rs` | 22 | `EPS` | `0.01` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/splitter.rs` | 65 | `COLLAPSED_VISIBLE_EPSILON` | `0.01` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/standard_item.rs` | 1548 | `ROW_W` | `680.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/standard_item.rs` | 1572 | `ROW_W` | `680.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/standard_item.rs` | 1597 | `ROW_W` | `400.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/stepper/indicator.rs` | 35 | `MARKER_LABEL_GAP` | `10.0` | Spacing | scales with `spacing_factor` — `marker_label_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/table_view/header.rs` | 66 | `DRAG_REORDER_THRESHOLD` | `5.0` | Grab | moves into `GestureProfile.drag_slop` (kind-derived), not a density scale |
| `crates/teksilo-widgets/src/table_view.rs` | 101 | `SCROLLBAR_THICKNESS` | `12.0` | Grab | visual fixed; coarse grab via `hit_outset` + `target_regions` |
| `crates/teksilo-widgets/src/table_view/widget_impl.rs` | 683 | `EDGE` | `32.0` | Grab | **deleted** — the five hand-rolled copies of this ramp are folded into `common::drag_autoscroll`, whose `band_for(PointerKind)` is 32 dp for a precise pointer (unchanged) and 64 dp for a finger |
| `crates/teksilo-widgets/src/table_view/widget_impl.rs` | 684 | `MAX_VELOCITY` | `12.0` | Grab | **deleted** — folded into `common::drag_autoscroll::MAX_VELOCITY`; a rate, not a dimension, so it is the same under every pointer and every density |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 103 | `SCROLL_ARROW_STEP` | `120.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 108 | `WHEEL_LINE_PIXELS` | `64.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 111 | `DRAG_EDGE_ZONE` | `32.0` | Grab | now an alias of `common::drag_autoscroll::EDGE_BAND_PRECISE`; the strip's `on_drag_tick` reads `band_for(ctx.pointer_kind())` |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 114 | `DRAG_MAX_VELOCITY` | `12.0` | Grab | fixed — now an alias of `common::drag_autoscroll::MAX_VELOCITY`; a rate, not a hit dimension |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 2763 | `DROPDOWN_WIDTH` | `240.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 2766 | `DROPDOWN_MAX_HEIGHT` | `320.0` | Decoration | fixed — container clamp, not a target |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 2769 | `DROPDOWN_ROW_HEIGHT` | `28.0` | Target | scales with `target_size` — `dropdown_row_height(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 2771 | `DROPDOWN_PADDING` | `4.0` | Spacing | scales with `spacing_factor` — `dropdown_padding(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | 353 | `REVEAL_EPSILON` | `0.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-widgets/src/tab_widget/header.rs` | 50 | `NATURAL_MIN_WIDTH` | `72.0` | Target | scales with `target_size` — `natural_min_width(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/header.rs` | 52 | `HEADER_PADDING_V` | `6.0` | Spacing | scales with `spacing_factor` — `header_padding_v(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/header.rs` | 54 | `INNER_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `inner_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/tab_widget/header.rs` | 57 | `FALLBACK_CHAR_WIDTH` | `8.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/toast/body.rs` | 319 | `LINE_H` | `16.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/toast/body.rs` | 320 | `WIDTH` | `160.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-widgets/src/toolbar.rs` | 95 | `CHEVRON_EXTENT` | `30.0` | Target | scales with `target_size` — `chevron_extent(&InputTokens)` |
| `crates/teksilo-widgets/src/toolbar.rs` | 96 | `ICON_SIZE` | `16.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-widgets/src/tooltip/composite.rs` | 209 | `FOOTER_GAP` | `6.0` | Spacing | scales with `spacing_factor` — `footer_gap(&InputTokens)` |
| `crates/teksilo-widgets/src/tooltip/dwell_indicator.rs` | 31 | `DWELL_INDICATOR_SIZE` | `14.0` | Decoration | fixed — indicator glyph, not an interactive target |
| `crates/teksilo-widgets/src/tree_table_view.rs` | 110 | `SCROLLBAR_THICKNESS` | `12.0` | Grab | visual fixed; coarse grab via `hit_outset` + `target_regions` |
| `crates/teksilo-widgets/src/tree_table_view/widget_impl.rs` | 671 | `EDGE` | `32.0` | Grab | **deleted** — the five hand-rolled copies of this ramp are folded into `common::drag_autoscroll`, whose `band_for(PointerKind)` is 32 dp for a precise pointer (unchanged) and 64 dp for a finger |
| `crates/teksilo-widgets/src/tree_table_view/widget_impl.rs` | 672 | `MAX_VELOCITY` | `12.0` | Grab | **deleted** — folded into `common::drag_autoscroll::MAX_VELOCITY`; a rate, not a dimension, so it is the same under every pointer and every density |
| `crates/teksilo-widgets/src/tree_view/body_pane.rs` | 557 | `PREVIEW_WIDTH` | `240.0` | Decoration | fixed — the drag preview's footprint, painted under the pointer; no hit consequence |
| `crates/teksilo-widgets/src/tree_view.rs` | 85 | `DEFAULT_ITEM_HEIGHT` | `28.0` | Target | scales with `target_size` — `default_item_height(&InputTokens)` |
| `crates/teksilo-widgets/src/tree_view.rs` | 86 | `SCROLLBAR_THICKNESS` | `12.0` | Grab | visual fixed; coarse grab via `hit_outset` + `target_regions` |
| `crates/teksilo-widgets/src/tree_view/widget_impl.rs` | 949 | `EDGE` | `32.0` | Grab | **deleted** — the five hand-rolled copies of this ramp are folded into `common::drag_autoscroll`, whose `band_for(PointerKind)` is 32 dp for a precise pointer (unchanged) and 64 dp for a finger |
| `crates/teksilo-widgets/src/tree_view/widget_impl.rs` | 950 | `MAX_VELOCITY` | `12.0` | Grab | **deleted** — folded into `common::drag_autoscroll::MAX_VELOCITY`; a rate, not a dimension, so it is the same under every pointer and every density |

## §4 — theme presets and the previewer UI

`teksilo-theme-fluent`, `teksilo-theme-macos` and `teksilo-theme-material3` install
their own Tier-3 styles with their own constants, so P20's projection has to reach
them or a preset silently stays Compact under Touch. `teksilo-preview-ui` is included
because its navigator rows (22 / 28 dp) are the smallest interactive rows in the
workspace.

**How a preset re-projects.** Each preset's `*_style()` metric factory gained a
`*_style_for(&InputTokens)` twin, and the zero-argument original is now defined as
the Compact call — so no existing caller or test changed, and `install_styles` reads
`theme.input`. Each preset then registers a `teksilo_core::styles::DensityProjection`
in its theme extensions; `Theme::with_density` calls it, and the preset rebuilds its
own slots against the new tokens while colours, id, palette extension and any slot
the *app* installed ride across untouched. Its own tests assert this
(`a_density_switch_re_derives_the_installed_slots`).

**The three presets do not share a ladder.** Material 3 keeps its published **48 dp**
touch target (`material3::input_tokens`), so an M3 button is 40 dp at Compact and
48 dp at Touch, not 44. Fluent has no published touch ladder — WinUI's 40 px / 7.5 mm
is a recommendation, not a metric its controls scale by — so it uses the generic one.
macOS has none at all (there is no macOS touchscreen), so it uses the generic one
too, and its `[unpublished]` 22 dp control height and `[measured]` 14 dp glyph
controls stay at Apple's numbers **in the paint** at every density: growing them
would falsify a preset whose whole purpose is to reproduce those numbers. What rises
on macOS is the minimum *hit box* around a control — the two `MinSize` sites in §1.

| File | Line | Identifier | Value | Class | P20 treatment |
| --- | ---: | --- | ---: | --- | --- |
| `crates/teksilo-preview-ui/src/doc_export.rs` | 42 | `MIN_INK_RATIO` | `0.002` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-preview-ui/src/png_export.rs` | 19 | `EXPORT_SCALE` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/shape.rs` | 37 | `FLUENT_CONTROL_CORNER_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-fluent/src/shape.rs` | 39 | `FLUENT_OVERLAY_CORNER_RADIUS` | `8.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-fluent/src/shape.rs` | 41 | `FLUENT_FOCUS_RING_WIDTH` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/shape.rs` | 44 | `FLUENT_FOCUS_RING_INNER_WIDTH` | `1.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/shape.rs` | 48 | `FLUENT_FOCUS_RING_OFFSET` | `1.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/button.rs` | 49 | `PADDING_H` | `11.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/button.rs` | 51 | `PADDING_TOP` | `5.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/button.rs` | 53 | `PADDING_BOTTOM` | `6.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/button.rs` | 55 | `MIN_HEIGHT` | `32.0` | Target | scales with `target_size` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/checkbox.rs` | 37 | `BOX_SIZE` | `20.0` | Target | visual fixed at `[WinUI]` 20 dp (below the floor); coarse hit via `hit_outset` |
| `crates/teksilo-theme-fluent/src/styles/checkbox.rs` | 39 | `GLYPH_SIZE` | `12.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-theme-fluent/src/styles/checkbox.rs` | 41 | `STROKE` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-theme-fluent/src/styles/menu_item.rs` | 36 | `ITEM_HEIGHT` | `8.0 + 20.0 + 9.0` | Target | scales with `target_size` — `fluent_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/menu_item.rs` | 38 | `PADDING_H` | `11.0` | Spacing | scales with `spacing_factor` — `fluent_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/menu_item.rs` | 40 | `ICON_COLUMN` | `16.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/menu_item.rs` | 43 | `ICON_LABEL_GAP` | `12.0` | Spacing | scales with `spacing_factor` — `fluent_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/menu_item.rs` | 45 | `SHORTCUT_GAP` | `24.0` | Spacing | scales with `spacing_factor` — `fluent_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/menu_item.rs` | 47 | `SEPARATOR_HEIGHT` | `3.0` | Spacing | scales with `spacing_factor` — `fluent_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/metrics.rs` | 50 | `FLUENT_CONTROL_HEIGHT` | `32.0` | Target | scales with `target_size` — the preset's `fluent_*_style_for(&InputTokens)` factories |
| `crates/teksilo-theme-fluent/src/styles/radio.rs` | 45 | `OUTER` | `20.0` | Target | visual fixed at `[WinUI]` 20 dp (below the floor); coarse hit via `hit_outset` |
| `crates/teksilo-theme-fluent/src/styles/radio.rs` | 47 | `STROKE` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-theme-fluent/src/styles/radio.rs` | 49 | `DOT` | `12.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/radio.rs` | 51 | `DOT_HOVER` | `14.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/radio.rs` | 53 | `DOT_PRESSED` | `10.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/slider.rs` | 39 | `TRACK` | `4.0` | Grab | fixed — track is decoration; the thumb carries the target |
| `crates/teksilo-theme-fluent/src/styles/slider.rs` | 42 | `THUMB` | `22.0` | Grab | visual fixed; hit via `SliderStyle::thumb_diameter_for(&self, cfg, &InputTokens)` + `target_regions` |
| `crates/teksilo-theme-fluent/src/styles/slider.rs` | 44 | `INNER` | `12.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/slider.rs` | 46 | `INNER_HOVER` | `14.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/slider.rs` | 48 | `INNER_PRESSED` | `8.5` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/slider.rs` | 50 | `CROSS` | `32.0` | Target | scales with `target_size` — `cross(&InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/slider.rs` | 52 | `TICK` | `4.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/standard_item.rs` | 41 | `PILL_WIDTH` | `3.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/standard_item.rs` | 43 | `PILL_HEIGHT` | `16.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/standard_item.rs` | 45 | `PILL_RADIUS` | `1.5` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-fluent/src/styles/standard_item.rs` | 47 | `ROW_HEIGHT` | `40.0` | Target | scales with `target_size` — `fluent_standard_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/standard_item.rs` | 49 | `ROW_HEIGHT_TWO_LINE` | `60.0` | Target | scales with `target_size` — `fluent_standard_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-fluent/src/styles/standard_item.rs` | 51 | `ICON_SIZE` | `16.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-theme-fluent/src/styles/text_input.rs` | 42 | `MIN_HEIGHT` | `32.0` | Target | scales with `target_size` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/text_input.rs` | 53 | `PADDING_LEADING` | `10.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/text_input.rs` | 54 | `PADDING_TRAILING` | `6.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/text_input.rs` | 56 | `EDGE_REST` | `1.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/text_input.rs` | 57 | `EDGE_FOCUSED` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 45 | `TRACK_W` | `40.0` | Target | scales with `target_size` |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 47 | `TRACK_H` | `20.0` | Grab | fixed — track and knob are one coupled pill (a `const _: () = assert!` ties them); `ROW_H` carries the target |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 49 | `KNOB` | `12.0` | Grab | scales with `grab_size` |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 51 | `KNOB_HOVER` | `14.0` | Grab | scales with `grab_size` |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 53 | `KNOB_PRESSED_W` | `17.0` | Grab | scales with `grab_size` |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 54 | `KNOB_PRESSED_H` | `14.0` | Grab | scales with `grab_size` |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 57 | `KNOB_INSET` | `4.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 60 | `KNOB_MARGIN` | `3.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 62 | `TRACK_STROKE` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-theme-fluent/src/styles/toggle.rs` | 65 | `ROW_H` | `32.0` | Target | scales with `target_size` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-fluent/src/typography.rs` | 54 | `FLUENT_BODY_SIZE` | `14.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-fluent/src/typography.rs` | 56 | `FLUENT_CAPTION_SIZE` | `12.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/palette.rs` | 104 | `SECONDARY_LABEL_ALPHA_LIGHT` | `0.50` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-theme-macos/src/palette.rs` | 109 | `SECONDARY_LABEL_ALPHA_DARK` | `0.55` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-theme-macos/src/palette.rs` | 298 | `FLOOR` | `4.5` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-theme-macos/src/shape.rs` | 62 | `MACOS_CONTROL_CORNER_RADIUS` | `6.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-macos/src/shape.rs` | 64 | `MACOS_OVERLAY_CORNER_RADIUS` | `10.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-macos/src/shape.rs` | 67 | `MACOS_MENU_CORNER_RADIUS` | `9.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-macos/src/shape.rs` | 71 | `MACOS_FOCUS_RING_WIDTH` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/shape.rs` | 74 | `MACOS_FOCUS_RING_HALO_WIDTH` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/shape.rs` | 76 | `MACOS_FOCUS_RING_HALO_ALPHA` | `0.35` | — not a dimension | fixed — unitless (ratio / alpha / duration / epsilon) |
| `crates/teksilo-theme-macos/src/shape.rs` | 80 | `MACOS_FOCUS_RING_OFFSET` | `0.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/shape.rs` | 90 | `MACOS_CONTROL_HEIGHT` | `22.0` | Target | visual fixed at Apple's `[unpublished]` 22 dp; the §1 `MinSize` sites carry the 24 dp floor |
| `crates/teksilo-theme-macos/src/shape.rs` | 99 | `MACOS_SMALL_CONTROL_SIZE` | `14.0` | Target | visual fixed at Apple's `[measured]` 14 dp; coarse hit via `hit_outset` |
| `crates/teksilo-theme-macos/src/styles/button.rs` | 57 | `PADDING_H` | `10.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-macos/src/styles/button.rs` | 60 | `PADDING_V` | `3.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-macos/src/styles/checkbox.rs` | 40 | `BOX_SIZE` | `MACOS_SMALL_CONTROL_SIZE` | Target | visual fixed at `MACOS_SMALL_CONTROL_SIZE` (14 dp); coarse hit via `hit_outset` |
| `crates/teksilo-theme-macos/src/styles/checkbox.rs` | 42 | `CORNER` | `3.5` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-macos/src/styles/checkbox.rs` | 44 | `GLYPH_SIZE` | `10.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-theme-macos/src/styles/checkbox.rs` | 46 | `STROKE` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-theme-macos/src/styles/chrome.rs` | 62 | `CONTROL_SHADOW_OFFSET_Y` | `0.5` | Decoration | fixed — elevation, not geometry |
| `crates/teksilo-theme-macos/src/styles/chrome.rs` | 66 | `CONTROL_SHADOW_BLUR` | `1.5` | Decoration | fixed — elevation, not geometry |
| `crates/teksilo-theme-macos/src/styles/chrome.rs` | 68 | `CATCH_LIGHT_THICKNESS` | `1.0` | Decoration | fixed — hairline rule |
| `crates/teksilo-theme-macos/src/styles/menu_item.rs` | 41 | `ITEM_HEIGHT` | `22.0` | Target | visual fixed at AppKit's `[measured]` 22 dp; coarse hit via `hit_outset` — the gaps around it scale, `macos_menu_item_recipe_for` |
| `crates/teksilo-theme-macos/src/styles/menu_item.rs` | 43 | `PADDING_H` | `10.0` | Spacing | scales with `spacing_factor` — `macos_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/menu_item.rs` | 45 | `ICON_COLUMN` | `14.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/styles/menu_item.rs` | 48 | `ICON_LABEL_GAP` | `8.0` | Spacing | scales with `spacing_factor` — `macos_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/menu_item.rs` | 51 | `SHORTCUT_GAP` | `32.0` | Spacing | scales with `spacing_factor` — `macos_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/menu_item.rs` | 53 | `SEPARATOR_HEIGHT` | `11.0` | Spacing | scales with `spacing_factor` — `macos_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/menu_item.rs` | 55 | `HIGHLIGHT_RADIUS` | `4.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-macos/src/styles/menu_item.rs` | 57 | `HIGHLIGHT_INSET` | `5.0` | Spacing | scales with `spacing_factor` — `macos_menu_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/metrics.rs` | 368 | `FLUENT_CONTROL_HEIGHT` | `32.0` | Target | scales with `target_size` — the preset's `macos_*_style_for(&InputTokens)` factories |
| `crates/teksilo-theme-macos/src/styles/metrics.rs` | 52 | `MACOS_HELP_TAG_CORNER_RADIUS` | `5.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-macos/src/styles/metrics.rs` | 56 | `MACOS_ROW_HEIGHT` | `24.0` | Target | scales with `target_size` — the preset's `macos_*_style_for(&InputTokens)` factories |
| `crates/teksilo-theme-macos/src/styles/radio.rs` | 41 | `OUTER` | `MACOS_SMALL_CONTROL_SIZE` | Target | visual fixed at `MACOS_SMALL_CONTROL_SIZE` (14 dp); coarse hit via `hit_outset` |
| `crates/teksilo-theme-macos/src/styles/radio.rs` | 44 | `DOT` | `4.5` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/styles/radio.rs` | 46 | `STROKE` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-theme-macos/src/styles/radio.rs` | 48 | `CORNER` | `3.5` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-macos/src/styles/slider.rs` | 38 | `TRACK` | `4.0` | Grab | fixed — track is decoration; the thumb carries the target |
| `crates/teksilo-theme-macos/src/styles/slider.rs` | 41 | `THUMB` | `18.0` | Grab | visual fixed; hit via `SliderStyle::thumb_diameter_for(&self, cfg, &InputTokens)` + `target_regions` |
| `crates/teksilo-theme-macos/src/styles/slider.rs` | 43 | `CROSS` | `MACOS_CONTROL_HEIGHT` | Target | visual fixed at `MACOS_CONTROL_HEIGHT` (22 dp, below the floor); coarse hit via `hit_outset` |
| `crates/teksilo-theme-macos/src/styles/slider.rs` | 45 | `TICK` | `4.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/styles/slider.rs` | 47 | `DEFAULT_LENGTH` | `160.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/styles/slider.rs` | 49 | `KNOB_SHADOW_OFFSET_Y` | `0.5` | Decoration | fixed — elevation, not geometry |
| `crates/teksilo-theme-macos/src/styles/slider.rs` | 50 | `KNOB_SHADOW_BLUR` | `1.5` | Decoration | fixed — elevation, not geometry |
| `crates/teksilo-theme-macos/src/styles/standard_item.rs` | 53 | `ROW_HEIGHT` | `24.0` | Target | scales with `target_size` — `macos_standard_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/standard_item.rs` | 56 | `ROW_HEIGHT_TWO_LINE` | `40.0` | Target | scales with `target_size` — `macos_standard_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/standard_item.rs` | 58 | `ICON_SIZE` | `16.0` | Decoration | fixed — glyph metric; follows text scale, not target density |
| `crates/teksilo-theme-macos/src/styles/standard_item.rs` | 60 | `CAPSULE_RADIUS` | `5.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-macos/src/styles/standard_item.rs` | 62 | `CAPSULE_INSET` | `5.0` | Spacing | scales with `spacing_factor` — `macos_standard_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/standard_item.rs` | 70 | `PADDING_H` | `CAPSULE_INSET + LABEL_INSET_IN_CAPSULE` | Spacing | scales with `spacing_factor` — `macos_standard_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/standard_item.rs` | 72 | `LABEL_INSET_IN_CAPSULE` | `8.0` | Spacing | scales with `spacing_factor` — `macos_standard_item_recipe_for(&InputTokens)` |
| `crates/teksilo-theme-macos/src/styles/text_input.rs` | 57 | `PADDING_H` | `6.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-macos/src/styles/toggle.rs` | 43 | `TRACK_W` | `38.0` | Target | scales with `target_size` |
| `crates/teksilo-theme-macos/src/styles/toggle.rs` | 46 | `TRACK_H` | `MACOS_CONTROL_HEIGHT` | Grab | fixed — `MACOS_CONTROL_HEIGHT`; the switch is one coupled shape (a `const _: () = assert!` ties knob to track) |
| `crates/teksilo-theme-macos/src/styles/toggle.rs` | 48 | `KNOB` | `18.0` | Grab | scales with `grab_size` |
| `crates/teksilo-theme-macos/src/styles/toggle.rs` | 50 | `KNOB_INSET` | `2.0` | Spacing | scales with `spacing_factor` — the style's own `make_body`, from `ctx.theme().input` |
| `crates/teksilo-theme-macos/src/styles/toggle.rs` | 52 | `TRACK_STROKE` | `1.0` | Decoration | fixed — hairline stroke; scaling it thickens the design language |
| `crates/teksilo-theme-macos/src/styles/toggle.rs` | 54 | `KNOB_SHADOW_OFFSET_Y` | `0.5` | Decoration | fixed — elevation, not geometry |
| `crates/teksilo-theme-macos/src/styles/toggle.rs` | 55 | `KNOB_SHADOW_BLUR` | `1.5` | Decoration | fixed — elevation, not geometry |
| `crates/teksilo-theme-macos/src/typography.rs` | 66 | `MACOS_BODY_SIZE` | `13.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/typography.rs` | 68 | `MACOS_CALLOUT_SIZE` | `12.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/typography.rs` | 70 | `MACOS_SUBHEADLINE_SIZE` | `11.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/typography.rs` | 73 | `MACOS_TRACKING_13` | `-0.08` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/typography.rs` | 75 | `MACOS_TRACKING_12` | `0.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-macos/src/typography.rs` | 77 | `MACOS_TRACKING_11` | `0.06` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-material3/src/shape.rs` | 23 | `M3_FOCUS_RING_WIDTH` | `3.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-material3/src/shape.rs` | 25 | `M3_RADIUS_POPUP` | `12.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-material3/src/styles/button.rs` | 35 | `HEIGHT` | `40.0` | Target | scales with `target_size` — `m3_button_style(&InputTokens)` |
| `crates/teksilo-theme-material3/src/styles/button.rs` | 37 | `PADDING_H` | `24.0` | Spacing | scales with `spacing_factor` — `m3_button_style(&InputTokens)` |
| `crates/teksilo-theme-material3/src/styles/button.rs` | 39 | `PADDING_H_TEXT` | `16.0` | Spacing | scales with `spacing_factor` — `m3_button_style(&InputTokens)` |
| `crates/teksilo-theme-material3/src/styles/button.rs` | 41 | `FOCUS_WIDTH` | `3.0` | Decoration | fixed — decorative geometry, no hit consequence |
| `crates/teksilo-theme-material3/src/styles/card.rs` | 20 | `M3_CARD_RADIUS` | `12.0` | Decoration | fixed — corner radius is shape identity, not a hit target |
| `crates/teksilo-theme-material3/src/styles/toggle.rs` | 26 | `TRACK_W` | `52.0` | Target | scales with `target_size` |
| `crates/teksilo-theme-material3/src/styles/toggle.rs` | 27 | `TRACK_H` | `32.0` | Target | scales with `target_size` |
| `crates/teksilo-theme-material3/src/styles/toggle.rs` | 28 | `THUMB_OFF` | `16.0` | Grab | scales with `grab_size` |
| `crates/teksilo-theme-material3/src/styles/toggle.rs` | 29 | `THUMB_ON` | `24.0` | Grab | scales with `grab_size` |
| `crates/teksilo-theme-material3/src/styles/toggle.rs` | 30 | `THUMB_PRESSED` | `28.0` | Grab | scales with `grab_size` |
| `crates/teksilo-theme-material3/src/styles/toggle.rs` | 31 | `TRACK_OUTLINE_W` | `2.0` | Decoration | fixed — decorative geometry, no hit consequence |
