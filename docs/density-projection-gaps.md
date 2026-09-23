<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Density-projection gaps

Every `Recipe*Style` in `teksilo-widgets` carries its dimensions in a plain data
struct, and `for_tokens(&InputTokens)` projects each one onto the active density
— `dp` for a target, `spacing` for a gap, `density_min_size` for a minimum box.
A theme preset then builds that same struct with its own numbers.

**Forty-nine of those projected fields have no reader.** The widget composes the
dimension from the raw `pub const` in the recipe's module instead, so the
projected field is written by `for_tokens`, written again by every preset, and
consulted by nothing. Retuning one changes nothing on screen.

This page is the sweep. Two earlier counts — four, then six — were spot checks
and both were low; the numbers here are derived mechanically by
`crates/teksilo-widgets/tests/density_projection_gaps.rs`, which also holds this
page to the set so a **newly added** projected field with no reader fails a test
instead of joining the pile.

## How the set is derived

Scope: `crates/teksilo-widgets/src/styles/*.rs`. A field is **projected** when it
is initialised in a struct literal whose initialiser expression mentions `dp(`,
`spacing(` or `density_min_size(`. A projected field counts as **read** when the
token `recipe.<field>` — optionally `self.recipe.`, or with the binding named
`rec` / `r` / `m` / `metrics` — appears in the recipe's own file. Everything else
is **unread**.

That rule is a text match, not a type check, and it is stated here because the
guard test applies exactly it: doc and test cannot disagree about membership.
Its cost is **two false positives**, both listed in §4 with the reader named. Its
benefit is that it does not produce false *negatives* from name collisions,
which a whole-crate `.<field>` grep does: `size_large` is a field of both
`AvatarRecipe` and `IconButtonRecipe`, `row_height` of both `TableRecipe` and
`SearchFieldRecipe`, `padding_horizontal` of seven recipes, and a grep that does
not scope by owner reports every one of them as read.

`teksilo-core`'s own projected recipe (`HandleMetrics`, in `text_touch.rs`) is
outside the scope and was checked by hand: both its projected fields are read, by
`text_touch.rs` and `text_touch/affordance_layer.rs`. `TextSelectionRecipe`, once
listed beside it, lives in `teksilo-widgets/src/styles/recipe_text_selection_style.rs`
and is therefore inside the scope; the rule finds both its projected fields read.

## The counts

| | |
|---|---|
| projected fields in scope | 103 |
| of those, unread by the rule | 51 |
| false positives of the rule (§4) | 2 |
| **real gap** | **49** |
| — inert even if wired (§1) | 10 |
| — a shipped preset already sets a different value (§2) | 14 |
| — a preset sets the same value the constant carries (§3a) | 3 |
| — density only, no preset (§3b) | 22 |

Only the **denominator** is pinned by the guard. Every other number here falls
when someone wires one of these fields, which is what the page is for, so making
those cost a test failure would discourage the one change it invites: the tables
are the record, the guard keeps their *membership* honest, and the arithmetic is
a snapshot of the day it was taken.

Five further fields were on this list until the metrics-accessor work that
produced this page wired them, and they are the worked examples for what wiring
one looks like: `TableRecipe::cell_padding_horizontal` / `cell_padding_vertical`
(`TableStyle`), `CalendarRecipe::nav_arrow_size` (`CalendarStyle`) and
`SearchFieldRecipe::row_padding_horizontal` / `row_padding_vertical`
(`SearchFieldStyle`) — a defaulted metrics accessor on the style trait, defaulted
so an out-of-tree style keeps compiling, overridden by the `Recipe*Style` from
its own recipe so a preset's number takes effect.

**A verdict of "safe to wire" means the mechanism is available and the change is
local.** It never means the resulting dimension is right: a preset's number
becoming visible is a deliberate appearance change and the preset's own tests
are what should say whether it is the one intended.

## 1. Inert even if wired

Ten fields where the projection is flat across all three densities **and** no
shipped preset sets a different value, so routing the widget through the field
would move nothing at all. Determined by measurement, not by reading the
expression: every recipe was constructed at Compact, Comfortable and Touch and
the three values compared. `dp` is a floor, so a base already at or above the
44 dp Touch target is flat; `spacing(0.0, ..)` is flat because zero scales to
zero.

| recipe | field | value at every density | why flat |
|---|---|---|---|
| `AvatarRecipe` | `size_large` | 48 | base clears the Touch floor |
| `AvatarRecipe` | `size_x_large` | 64 | base clears the Touch floor |
| `CalendarRecipe` | `cell_gap` | 0 | `spacing(0.0, ..)` |
| `ColorPickerRecipe` | `canvas_height` | 192 | base clears the Touch floor |
| `ColorPickerRecipe` | `canvas_width` | 224 | base clears the Touch floor |
| `ColorPickerRecipe` | `hex_field_width` | 96 | base clears the Touch floor |
| `ColorPickerRecipe` | `preview_width` | 64 | base clears the Touch floor |
| `ColorPickerRecipe` | `spinner_field_width` | 56 | base clears the Touch floor |
| `ColorPickerRecipe` | `strip_length` | 192 | base clears the Touch floor |
| `TableRecipe` | `header_inter_cell_spacing` | 0 | `spacing(0.0, ..)` |

**Verdict: leave them.** Wiring costs a trait method and buys nothing until
someone gives one of them a preset value — and if they do, that preset's own
test is the thing that will say so.

`ColorPickerRecipe::preview_width` is worth one extra sentence: its constant has
no reader **anywhere**, so the colour picker's preview swatch takes its width
from neither the field nor the constant. That is a dead dimension rather than an
unwired one.

## 2. A shipped preset already sets a different value

The fourteen that cost something today. Each is written by Fluent or macOS with
a number chosen for that design language, and each renders the IntUI constant
instead. The **renders** column is what a user sees under any theme.

| recipe | field | renders | Fluent asks | macOS asks | raw-const use site |
|---|---|---|---|---|---|
| `CalendarRecipe` | `cell_size` | 32 | — | 28 | `calendar.rs` → `Calendar::build`, `build_weekday_row` |
| `CalendarRecipe` | `header_height` | 28 | — | 26 | none — see below |
| `ScrollBarRecipe` | `min_thumb_length` | 24 | 30 | 28 | `scroll_bar.rs` → `ScrollBar::build` |
| `SearchFieldRecipe` | `row_height` | 26 | — | 22 | `search_field.rs` → `SuggestionPanel::build` |
| `SegmentedControlRecipe` | `height` | 24 | 32 | 22 | `segmented_control.rs` → `layout_response` |
| `SegmentedControlRecipe` | `padding_horizontal` | 12 | — | 10 | `segmented_control.rs` → `layout_response`; `segmented_control/cell.rs` → `layout_response` |
| `SegmentedControlRecipe` | `padding_vertical` | 6 | — | 3 | `segmented_control.rs` → `layout_response` |
| `StandardItemRecipe` | `min_height_single_line` | 28 | 40 | 24 | `standard_item.rs` → `layout_response` |
| `StandardItemRecipe` | `min_height_two_line` | 44 | 60 | 40 | `standard_item.rs` → `layout_response`; `notification/log.rs` → `build` |
| `TabRecipe` | `editor_height` | 50 | 32 | 26 | `tab_widget/bar.rs` → `build`; `tab_widget/header.rs` → `intrinsic_height` |
| `TabRecipe` | `padding_horizontal` | 12 | 8 | 12 | `tab_widget/header.rs` → `build`, `estimate_natural_width` |
| `TabRecipe` | `tool_window_height` | 28 | 32 | 22 | none — see below |
| `TableRecipe` | `header_height` | 32 | 40 | 24 | `table_view.rs` → `effective_header_height`; `tree_table_view.rs` → `effective_header_height` |
| `TableRecipe` | `row_height` | 28 | 40 | 24 | `table_view.rs` → `effective_row_height`; `tree_table_view.rs` → `effective_row_height` |

Ladders, for the ones whose density projection also moves (Compact / Comfortable
/ Touch): `cell_size` 32/32/44, `CalendarRecipe::header_height` 28/32/44,
`min_thumb_length` 24/32/44, `SearchFieldRecipe::row_height` 26/32/44,
`SegmentedControlRecipe::height` 24/32/44 and `padding_horizontal` 12/13.8/15.6
and `padding_vertical` 6/6.9/7.8, `min_height_single_line` 28/32/44,
`TabRecipe::padding_horizontal` 12/13.8/15.6, `tool_window_height` 28/32/44,
`TableRecipe::header_height` 32/32/44, `row_height` 28/32/44.
`min_height_two_line` (44) and `TabRecipe::editor_height` (50) are flat — their
cost is the preset alone, not the density.

### Verdicts

**Safe to wire, `TableStyle` already has the seam.** `TableRecipe::row_height`
and `header_height` — both already flow through an `Option<f32>` override
(`effective_row_height` / `effective_header_height`), so the change is swapping
the `unwrap_or` default for a style accessor at one site each, in the same shape
`cell_padding_horizontal` just took. Note they feed the virtualization
arithmetic, so a test should assert a laid-out row band and not only the
accessor.

**Safe to wire, one accessor each.** `ScrollBarRecipe::min_thumb_length` —
`ScrollBar::build` already resolves the style and then *recomputes* the floor
from the constant three lines later, so the value is already in hand.
`SearchFieldRecipe::row_height` — the same `SuggestionPanel::build` that now
takes its row gutters from `SearchFieldStyle`.
`StandardItemRecipe::min_height_single_line` / `min_height_two_line` — the one
gap already documented at its own site (`standard_item.rs`'s module header says
the row does not read them); `StandardItemStyle` is the trait, and the row's
`layout_response` is the single call site. This is the highest-visibility row in
the table: it is the height of every list and tree row in a Fluent or macOS app.

**Needs care — the widget owns a composition the field cannot express.**
`SegmentedControlRecipe::height` / `padding_horizontal` / `padding_vertical`:
the segmented control's `layout_response` and its overflow planner compute cell
extents from these three together, and the planner is also fed by
`measure_intrinsic` on dormant cells. Wiring them means threading the style into
the planner, not just into a `Padding`.
`TabRecipe::editor_height` / `padding_horizontal`: the tab bar's header widths
come from `estimate_natural_width`, which runs before a style is resolved in the
bar's own build; the seam has to be chosen before the numbers can move.

**Needs care — nothing applies the dimension at all.**
`CalendarRecipe::header_height` and `TabRecipe::tool_window_height`: their
constants have no reader either, so the calendar's header row and the
tool-window tab strip take their height from their content. Wiring the field is
therefore not "read the style instead of the constant" but "give this row a
fixed height", which is a design decision and not a mechanical one.

## 3. No preset involvement

### 3a. A preset sets the same value the constant carries

Three fields a preset writes with the number the shipped constant already
carries, so nothing is visibly lost today — but the *density* ladder still does
not reach them, and a preset that retuned one would silently do nothing.

| recipe | field | Compact / Comfortable / Touch | preset | raw-const use site |
|---|---|---|---|---|
| `CalendarRecipe` | `outer_padding` | 8 / 9.2 / 10.4 | macOS 8 | `calendar.rs` → `Calendar::build` |
| `StandardItemRecipe` | `tree_indent_step` | 16 / 18.4 / 20.8 | Fluent 16, macOS 16 | `standard_item.rs` → `build` |
| `TableRecipe` | `tree_indent_per_level` | 16 / 18.4 / 20.8 | Fluent 16, macOS 16 | `tree_table_view.rs` → `effective_indent` |

**Verdict: safe to wire.** All three are one-site `spacing` values, and
`effective_indent` already has the `Option<f32>` seam.

### 3b. Density only

The remaining twenty-two. No preset writes them, so the whole cost is that the
Comfortable and Touch ladders never reach the screen for that one dimension.

| recipe | field | Compact / Comfortable / Touch | raw-const use site |
|---|---|---|---|
| `AvatarRecipe` | `size_medium` | 32 / 32 / 44 | `avatar.rs` → `small_medium_large_xlarge_sizes` |
| `AvatarRecipe` | `size_small` | 24 / 32 / 44 | `avatar.rs` → `small_medium_large_xlarge_sizes` |
| `BannerRecipe` | `title_description_gap` | 2 / 2.3 / 2.6 | `banner.rs` → `build` |
| `CalendarRecipe` | `section_gap` | 4 / 4.6 / 5.2 | `calendar.rs` → `Calendar::build` |
| `CheckboxRecipe` | `label_gap` | 6 / 6.9 / 7.8 | `checkbox.rs` → `build` |
| `ColorPickerRecipe` | `preview_height` | 28 / 32 / 44 | `color_picker.rs` → `build`; `color_edit.rs` → `build` |
| `ColorPickerRecipe` | `swatch_spacing` | 6 / 6.9 / 7.8 | `color_picker/swatch_grid.rs` → `build` |
| `DateEditRecipe` | `calendar_button_width` | 24 / 32 / 44 | none |
| `DateEditRecipe` | `segment_gap` | 1 / 1.15 / 1.3 | none |
| `RadioRecipe` | `label_gap` | 6 / 6.9 / 7.8 | `radio_button.rs` → `build` |
| `SearchFieldRecipe` | `input_panel_gap` | 2 / 2.3 / 2.6 | none |
| `SearchFieldRecipe` | `panel_padding` | 4 / 4.6 / 5.2 | `search_field.rs` → `SuggestionPanel::build` |
| `StandardItemRecipe` | `label_subtitle_gap` | 2 / 2.3 / 2.6 | `standard_item.rs` → `build_content` |
| `StandardItemRecipe` | `slot_gap` | 8 / 9.2 / 10.4 | `standard_item.rs` → `build_content` |
| `StandardItemRecipe` | `subtitle_slot_gap` | 6 / 6.9 / 7.8 | `standard_item.rs` → `build_content` |
| `TableRecipe` | `focus_ring_inset` | 1 / 1.15 / 1.3 | `table_view/widget_impl.rs` → `paint`; `tree_table_view/widget_impl.rs` → `paint` |
| `TableRecipe` | `min_column_width_default` | 32 / 32 / 44 | `table_view/widget_impl.rs` → `build`, `place_children`; `tree_table_view/widget_impl.rs` → `build`, `place_children` |
| `TableRecipe` | `tree_twist_label_gap` | 4 / 4.6 / 5.2 | `tree_table_view/body_pane.rs` → `build` |
| `TextInputRecipe` | `padding_vertical` | 4 / 4.6 / 5.2 | `text_input/widget_impl.rs` → `build`; `password_field.rs` → `build`; `date_time_edit.rs` → `build_field`; `date_range_edit.rs` → `build_half`; `spin_box.rs` → `build` |
| `TextInputRecipe` | `validation_strip_gap` | 4 / 4.6 / 5.2 | `text_input/widget_impl.rs` → `build`; `password_field.rs` → `build`; `date_time_edit.rs` → `build`; `date_range_edit.rs` → `build` |
| `ToastRecipe` | `body_actions_gap` | 8 / 9.2 / 10.4 | `toast/surface.rs` → `build` |
| `ToastRecipe` | `title_body_gap` | 2 / 2.3 / 2.6 | `toast/surface.rs` → `build` |

### Verdicts

**Safe to wire — a single `.spacing(..)` or `Padding` at one site.**
`title_description_gap`, `section_gap`, `CheckboxRecipe::label_gap`,
`swatch_spacing`, `panel_padding`, `label_subtitle_gap`, `slot_gap`,
`subtitle_slot_gap`, `RadioRecipe::label_gap`, `tree_twist_label_gap`,
`validation_strip_gap`, `body_actions_gap`, `title_body_gap`. Thirteen of the
twenty-two, all the same shape as the `SearchFieldStyle` row gutters.

**Safe to wire, with a note.** `AvatarRecipe::size_small` / `size_medium` —
`avatar.rs`'s size table is a `match` over `AvatarSize`, so one accessor has to
carry all four sizes (two of which are §1 no-ops) or the table has to be built
from the recipe. `ColorPickerRecipe::preview_height` has two call sites, one of
them in `ColorEdit`, so the accessor must be reachable from both.
`TableRecipe::focus_ring_inset` is read from `paint`, where no `BuildContext`
exists; the resolved value has to be baked in `build` and parked, which is what
`HeaderCell::zone_floor` already does for the target-size floor.

**Needs care.** `TableRecipe::min_column_width_default` is read in
`place_children` as well as `build`, and it is the floor a column resize clamps
to — moving it at Touch changes what a drag can do, not only what is painted,
so it wants a resize test rather than a layout one.

**Nothing to wire.** `DateEditRecipe::calendar_button_width` and `segment_gap`,
`SearchFieldRecipe::input_panel_gap`: like the two in §2, their constants have
no reader either. The dimension is not applied anywhere, so there is nothing to
route through a style.

## 4. The rule's false positives

Two fields the derivation calls unread and that a reader can see are read. Both
are read through a binding the rule's name list does not cover — the widget
constructs the default recipe inline and takes one field off it.

| recipe | field | actual reader |
|---|---|---|
| `CheckboxRecipe` | `box_hit_area` | `checkbox.rs` → `build`, via `CheckboxRecipe::for_tokens(&ctx.theme().input)` bound as `style_recipe` |
| `RadioRecipe` | `hit_area` | `radio_button.rs` → `build`, via `RadioRecipe::for_tokens(&ctx.theme().input)` read inline |

They are listed rather than excluded because the guard test applies the rule as
written, and a row it can see is better than a special case it cannot. Note what
these two also show: reading the *default* recipe is not the same as reading the
*active style*, so a preset that set `box_hit_area` would still not be honoured.
Neither preset sets either, so nothing is lost today.

## The guard

`crates/teksilo-widgets/tests/density_projection_gaps.rs` re-derives the unread
set from the source on every run and holds it to this page:

* a projected field with no reader that this page does not list **fails**, and
  the failure names it and says what to do — wire it, or add a row here;
* a row here naming a field that is no longer projected **fails**, so a rename
  or a deletion cannot leave a stale row behind;
* a row here whose field has since been wired **passes**, with a note printed.
  Wiring one of these is the point of the page, and it must not cost the person
  who does it an edit to a test.

The rule the test applies is the one stated under "How the set is derived", and
its own liveness is checked by a fixture: a projected field with a reader and
one without, both synthesised in the test, so a derivation that silently stops
finding anything fails instead of passing mute.

## Related

* [density-inventory.md](density-inventory.md) — the constant-by-constant audit
  the density sweep produced, and the rules
  `crates/teksilo-widgets/tests/density_projection.rs` holds it to.
* [density-and-targets.md](density-and-targets.md) — what `dp`, `spacing` and
  `density_min_size` mean, and the ladder they walk.
* [touch-and-pen.md](touch-and-pen.md) §10.1 — the dead-API ledger this page was
  split out of.
