<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Widget pointer inventory

Every file under `crates/teksilo-widgets/src/` that carries pointer, gesture or hover
code, with the touch-migration package that owned it. This was the checklist the
control sweeps sized themselves from; it is now **closed** — every row is either
migrated with a named test or carries a written no-change reason, and no row says
"pending".

Two conventions, because the WCAG sweep and the target-conformance harness read
this file:

* a row cites **file + function**, never file + line: re-derived across this
  programme, a `file:line` citation in an artifact rotted almost completely
  within a few packages;
* "no change needed" is a claim about the code, so it names what was measured or
  which existing test already covers it. A row that merely reads plausibly is a
  defect handed to two downstream packages.

## Scope and method

A file is in scope if, **outside** `#[cfg(test)]` blocks, files named `tests.rs`, and
comment lines, it mentions any of:

`on_pointer_event` · `PointerDown` / `PointerUp` / `PointerMove` / `PointerEnter` /
`PointerLeave` · `on_tap` · `on_double_tap` · `on_triple_tap` · `on_long_press` ·
`on_drag*` / `on_drop` · `on_swipe` · `on_hover` · `on_scroll` · `capture_pointer` ·
`hover_within` · `start_drag`

Two supplementary markers, `DragPhase` and `TapEvent`, catch handler bodies factored
into a helper file that never names the builder method. Exactly one file is reached
only that way: `grid_view/selection.rs`, which builds the GridView marquee closure.
Marker columns show the supplementary two in *italics*.

```bash
# reproduce the file list
cd crates/teksilo-widgets/src
grep -rlnE 'on_pointer_event|Pointer(Down|Up|Move|Enter|Leave)|on_tap|on_double_tap|on_triple_tap|on_long_press|on_drag|on_drop|on_swipe|on_hover|on_scroll|capture_pointer|hover_within|start_drag|DragPhase|TapEvent' \
  --include=*.rs . | grep -v tests.rs | sort
```

(The published table applies the comment and `#[cfg(test)]` filters the bare grep
cannot; the raw grep over-reports by five files whose only mentions are in prose —
`tree_view.rs`, `rich_text/context_menu.rs`, `rich_text/frame_loop.rs`,
`toast/registry.rs`, `grid_view/layout/strategy.rs` — plus
`styles/recipe_tab_style.rs`, `table_view/imperative.rs` and `tree_view/builder.rs`.
Those eight are listed at the end under "Prose-only mentions".)

## Result

**78 files** carry real pointer code. Every one now has a verdict:

| Verdict | Files |
| --- | ---: |
| `P21` (scrollables reference) | 2 |
| `P22` (scrollable migration) | 10 |
| `P23` (data-view press / reorder) | 8 |
| `P24` (controls sweep) | 20 |
| `P25` (composite / partitioned / docking) | 19 |
| `P27` (single-line text) | 5 |
| `P28` (rich text + code editor) | 2 |
| `P29` (menus, tooltips, toasts) | 5 |
| `P30` (window chrome) | 4 |
| `P08` (core arbitration — reviewed, kept) | 1 |
| `no change needed` | 2 |
| **Total** | **78** |

**Correction, 2026-09-23.** Re-running the scope method above finds real,
non-test, non-comment pointer-handler code in seven files this inventory does
not list: `common/scrollable.rs` (the shared `on_scroll` installer every
`P22 done (scroll)` row below points at, but which never got its own row),
`primitives/text_input_field/touch.rs` and
`primitives/text_input_field/widget_impl.rs`, `rich_text/touch_mount.rs`,
`menu_bar/trigger.rs`, `menu_item/widget_impl.rs`, and
`text_input/widget_impl.rs`. All seven already existed when this inventory's
closing commit landed (they came from `refactor(widgets): split the eight
largest widget modules along their seams` and
`feat(widgets): a finger selects text in every single-line field`, both
immediately before the commit that closed this file), so the omission is not
later drift — the **78** total and the "every row… no row says pending" claim
below were never quite complete. The seven are not re-triaged here; each sits
beside a file already carrying a verdict in the table below (`text_input.rs`
→ P27, `menu_item.rs` / `menu_bar.rs` → P29, `rich_text.rs` → P28,
`primitives/text_input_field.rs` → P27) and its content reads as that same
package's work, but that is an inference from proximity, not a re-audit, and
is recorded here rather than folded into the table as if it had been checked.

The five files the first pass found **unclaimed** are settled: `combo_box.rs` and
`snackbar.rs` went to P25 (the clause named both), `avatar.rs` and
`primitives/text_widget.rs` to P24, and `primitives/dead_zone.rs` was reviewed
under the arbitration package and kept as it was. Their rows say so.

`P31` claims no file in this crate. `drag_preview.rs` has no pointer handler (it
is a paint-only overlay), and `drop_target.rs` — which the work breakdown named
in both places — is P25's, which is where its row sits.

`splitter/handle.rs` is named by **both** P29 (its 400 ms dwell reveal) and P30 (its
grab geometry). Its *pointer* code — the `Ignored`-on-press / `capture_pointer`-on-move
latch — belongs to P30, so that is its verdict; the dwell reveal is row 2 of
[the hover census](hover-affordance-census.md) and was P29's.

## The files the first pass left unclaimed

Nine files carried real pointer code that the work breakdown assigned to nobody.
Four of them were the largest single omission in the plan: **`list_view/body_pane.rs`
was claimed by P23, but its four siblings were not**, and all five implement the
same press-time row selection, tap/double-tap activation and `start_drag` reorder
that P23 exists to convert. P23 took all four. The remaining five are settled
too, each by the package whose clause named it:

| File | Why it mattered | Settled by |
| --- | --- | --- |
| `grid_view/body_pane.rs` | press-time tile selection + `start_drag` reorder | P23 |
| `table_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 |
| `tree_table_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 |
| `tree_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 (P23 names `tree_view`, but `tree_view.rs` itself has only prose mentions) |
| `combo_box.rs` | the closed-box trigger tap and the field hover | P25 |
| `primitives/text_widget.rs` | inline-link hover cursor and tap-to-follow | P24 — **and it corrected this file**: the Ctrl gate the row blamed lives in `rich_text/mouse.rs`, not here, so an inline link was never unreachable by touch |
| `avatar.rs` | a tappable control with no target floor | P24 |
| `snackbar.rs` | trigger tap + in-surface action taps | P25 |
| `primitives/dead_zone.rs` | `gesture_dead_zone(true)` plus a no-op tap/drag absorber | P08 — reviewed and **kept**: the flag governs member enrolment, the absorbers are what give the wrapper an arena, and the arena is what stops the bubble for a press on its own bare area |

### Release-time commits in the five body panes — guarded under P22

The five panes stay P23's to *convert*, but one class of defect in them was
reachable the moment P22 gave the data views a pan, so P22 fixed it rather than
leaving it to a later package.

A row body commits things on `PointerUp` from a **raw** `on_pointer_event` arm,
not from a gesture: `TreeView`'s chevron toggle, and the deferred collapse of a
multi-selection in `TreeView` / `ListView` / `TableView` / `TreeTableView`
(`GridView`'s is the same shared `deferred_select::on_up`). The arbitration
cannot stop a raw arm — a won pan claim is not an `active_drag`, so the release
is not routed to `handle_drag_drop` — and the row is not reliably a sequence
member either (it is enrolled only when it carries a drag), so it cannot count
on being told it lost. Measured before the fix: a 120 dp finger pan over 200
collapsed branch rows scrolled the tree **and** expanded the row it started on
(`visible_count` 200 → 201); a short pan on a fully-selected view collapsed the
selection to that one row. Each commit now asks
`data_views::release_completes_the_press` (which reads `EventContext::press_is_inside`).
Covered by question 5 of `crates/teksilo-widgets/tests/scrollables_touch.rs`.

**Closed since, against `grid_view.rs`:** a finger did not scroll a `GridView`
whose selection is in `SelectionMode::Multi`. The cause was neither the claim
losing the arbitration nor the marquee running instead — both had been ruled
out, and an early draft that blamed a slop race was wrong. It was the
**capture** dispatch: in `Multi` mode the rubber-band marquee's `on_drag` sat on
the `GridView`'s own node, the one carrying the `PanClaim`, which gave that node
a gesture arena and so the implicit press capture. The router dispatches a move
to the captor *before* it advances the arbitration, so the marquee's
`DragRecognizer` latched at `drag_slop` (18 dp) and decided the sequence on the
first sample — after which the candidate walk yields nothing, the claim is never
evaluated at `pan_slop` (36 dp), and no `Scroll` is synthesised at all. The
marquee then declined the press for landing on a tile. The drag won and did
nothing, which is exactly what the trace showed.

The fix has two halves, each with its own test in
`crates/teksilo-widgets/tests/data_view_drag.rs`: the body pane takes a no-op
tap (`data_views::press_absorber`) so the press is captured *inside* the
claimant rather than by it, and the marquee's drag moves onto a
`data_views::DragSurface` that strictly encloses the pane — the only shape the
tree arms `DragActivation` for, so a finger's marquee now waits for a long press
while a mouse still latches at 5 dp. `a_finger_on_a_multi_select_grid_pans_it`
in `scrollables_touch.rs` is no longer `#[ignore]`d.

Two further files were swept in by the marker set but genuinely need no change:
`list_source.rs` and `tree_source.rs`, whose `on_drag_out(k)` is a data-source policy
callback with no pointer plumbing behind it.

## Inventory

| File | Pointer surface used | Interaction | Migration |
| --- | --- | --- | --- |
| `crates/teksilo-widgets/src/accordion.rs` | `on_tap`, `on_drag`, *DragPhase* | Header tap toggles the disclosure; `on_drag` on the header is the dock panel drag handle (trailing slot wrapped in a `DeadZone`). | P25 — **no change needed**, tested. The header is 28 dp on the ladder and toggles from its tap, so already on the release; the drag needs no declaration because `DragActivation::Auto` resolves to `Immediate` with no pan competitor and to the hold with one. `a_finger_taps_an_accordion_header_shut` and `a_finger_dragging_an_accordion_header_moves_the_dock_instead` (`tests/composite_touch.rs`) pin both halves. |
| `crates/teksilo-widgets/src/avatar.rs` | `on_tap` | Optional `on_tap` action on the avatar circle; no press visual, no target floor. | P24 done — the circle takes a `hit_outset` when it is tappable and `EdgeInsets::ZERO` when it is decoration, which is the gate the outset-declaring widgets all owe. |
| `crates/teksilo-widgets/src/breadcrumb.rs` | `on_tap`, `on_hover` | Tap a crumb to navigate; hover tint per crumb; overflow `…` opens a menu. | P24 done |
| `crates/teksilo-widgets/src/button.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_hover`, *TapEvent* | Press/release interaction state via `on_pointer_event`, `on_tap` activation, `on_hover` tint — the reference control. | P24 done |
| `crates/teksilo-widgets/src/calendar/cell.rs` | `on_tap` | Tap a day cell to select the date (32 dp cell). | P24 done |
| `crates/teksilo-widgets/src/calendar/header.rs` | `on_tap` | Tap prev / next / the "Month Year" zoom-out title in the navigation strip. | P25 — **fixed**: `NavArrow::build` read the raw 24 dp constant, so the arrows stayed 24 dp at Touch while the day cells beside them grew. Now `nav_arrow_extent(&tokens)`, the identity at Compact. `a_calendar_nav_arrow_follows_the_density_ladder` (`tests/composite_touch.rs`). |
| `crates/teksilo-widgets/src/calendar/zoom_grid.rs` | `on_tap`, `on_hover` | Tap a month or year cell in the coarse-grain grid; hover tint. | P25 — **fixed**: the cell wrote `false` into its pressed signal and nothing else, so the appearance its `CalendarStyle` painted was unreachable for a mouse as well as a finger. Now `common::interaction::bind_pressed`. `a_press_on_a_calendar_month_cell_shows_its_pressed_chrome` (`tests/composite_touch.rs`). |
| `crates/teksilo-widgets/src/checkbox.rs` | `on_tap`, `on_hover` | Tap toggles; hover tint; 24 dp `MinSize` hit area around a 19 dp visual box. | P24 done |
| `crates/teksilo-widgets/src/code_editor/completion.rs` | `on_tap` | Tap a suggestion row in the caret-anchored completion popup. | P25 — **fixed**: the row takes the menu row's target floor via `density_min_size`, which raises it 2 dp at Compact (the documented `MinSize`-is-a-hit-box exception — a stack of adjacent rows is the one shape no hit mechanism serves). `a_completion_row_follows_the_density_ladder` (`code_editor/tests.rs`). |
| `crates/teksilo-widgets/src/code_editor/log_view.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_scroll` | Scroll (wheel + pan) now comes from `common::text_scroll`, the helper the three text surfaces share; double/triple tap selects a line span. | P22 done (scroll) |
| `crates/teksilo-widgets/src/code_editor/mouse.rs` | `PointerDown`, `PointerUp`, `PointerMove` | Press latches a text selection, move extends it, release ends it; drag auto-scroll at the margins. P22 removed this file's `handle_scroll`; the two build sites install `common::text_scroll` instead. | P28 done |
| `crates/teksilo-widgets/src/code_editor/widget.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_scroll` | Scroll (wheel + pan) now comes from `common::text_scroll`; double/triple tap word and line selection. | P22 done (scroll) |
| `crates/teksilo-widgets/src/color_picker/alpha_strip.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` to set alpha along a 14 dp strip. | P24 done |
| `crates/teksilo-widgets/src/color_picker/hsv_canvas.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` over a 2-D saturation x value canvas. **P39 made this node focusable** (arrow keys and four named custom actions — SC 2.5.7), so the target audit now measures it where it previously skipped an unfocusable container; its pointer code is unchanged. | P24 done; P39 keyboard |
| `crates/teksilo-widgets/src/color_picker/hue_strip.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` to set hue along a 14 dp strip. | P24 done |
| `crates/teksilo-widgets/src/color_picker/swatch.rs` | `on_tap` | Tap a 22 dp swatch to pick its colour. | P24 done |
| `crates/teksilo-widgets/src/combo_box.rs` | `on_tap`, `on_hover` | Tap the closed box opens the dropdown overlay; `on_hover` drives the field tint. | P25 — **no change needed**, tested: the closed box is one target (the arrow column is paint inside it) and opens the dropdown from its tap, so on the release. `a_finger_taps_the_closed_combo_box_open` (`tests/composite_touch.rs`) pins both halves of that. The field hover is decoration. |
| `crates/teksilo-widgets/src/combo_box/item.rs` | `on_tap`, `on_hover` | Tap a dropdown row to commit; hover highlights the active row. | P25 — **fixed**: `DropdownItem::layout_response` and `combo_box::panel::build_virtualized_list` both read the raw `MENU_ITEM_HEIGHT`, so the combo's own list stayed 24 dp while a `MenuList`'s rows reached 44. Both now resolve `menu_item_height(&tokens)`. `a_combo_box_dropdown_row_follows_the_density_ladder` (`tests/composite_touch.rs`). |
| `crates/teksilo-widgets/src/data_views.rs` | `on_drag` | Shared row drag-and-drop substrate: `RowExport<T>`, drag payload construction and the reorder/foreign-drop verdicts for all five views, plus `deferred_select`, `DragSurface` and `press_absorber`. | P23 done |
| `crates/teksilo-widgets/src/dialog.rs` | `on_tap` | Tap the trigger to present; scrim tap dismisses; footer button taps. | P25 — **no change needed**. The trigger is a `Button` or an `OverlayTrigger`, the footer holds buttons, and all of them actuate on the release; the scrim already declares `no_hit_slop` rather than relying on the slop pass's size formula, covered by `scrim_hit_targeting_tests::a_modal_scrim_never_participates_in_hit_widening` in the file itself. **Not** testable end-to-end headlessly: the trigger queues a `ModalRequest` that only the app layer drains. |
| `crates/teksilo-widgets/src/docking/activity_bar.rs` | `on_tap`, `on_drag`, `start_drag`, *DragPhase* | Tap a rail item to select or hide its side; `start_drag` reorders rail items and transfers activities between sides. | P25 — **no change needed**, tested. Tap on the release, drag through `DragActivation::Auto`, context menu from the hold. `docking::tests::a_finger_taps_a_rail_item_to_show_its_side` and `a_finger_reorders_the_rail_by_dragging_an_item`. The rail item's extent is deliberately **not** density-projected — see the refusal in the density inventory's P25 corrections. |
| `crates/teksilo-widgets/src/docking/panel.rs` | `on_drag`, `start_drag` | Accordion-header `on_drag` starts a dock drag; the five-zone drop overlay routes the drop to stack/split. | P25 — **fixed by proxy**: the pane's five zones are the shared `DropTarget`'s (`DockPanePane::build`), so they inherit its per-axis band floor; a narrow pane's edge zone is no longer a fifth no finger can land in. `docking::tests::a_dock_panes_drop_zones_come_from_the_shared_drop_target` pins the wiring structurally, `tests/composite_touch.rs` the floor itself. |
| `crates/teksilo-widgets/src/docking/resize_handle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_double_tap`, `on_hover`, `capture_pointer` | Press captures the pointer and resizes the side band; double tap snaps to hide; hover shows the grip. 6 dp gutter. | P30 done |
| `crates/teksilo-widgets/src/drop_target.rs` | `on_drag` | `on_drag_hover` / `on_drop` over the wrapped child, optionally partitioned into five `DropRegion` zones. | P25 — **fixed**: an edge band is `zone_size_factor` of the axis **floored per axis** to the density's target size, the floor capped at a third of the extent so the centre can never vanish. One function (`band_depth`) answers the hit test, the drop and the highlight, so the zone a user sees is the zone that drops. Four cases in `tests/composite_touch.rs` plus `drop_target::overlay::tests::the_painted_zone_is_the_floored_band`. |
| `crates/teksilo-widgets/src/drop_zone.rs` | `on_drag` | `on_drag_hover` / `on_drop` for external (OS) file, text and URL drops. | P25 — **no change needed**: the zone is one target and the whole surface of it, and an external drop carries no press to move to a release. The keyboard Browse fallback is the reachability story where no OS backend exists, documented at its builder. |
| `crates/teksilo-widgets/src/grid_view.rs` | `on_pointer_event`, `PointerDown`, `on_drag`, `on_scroll` | Scroll (wheel + pan) now comes from `common::scrollable`; container `on_pointer_event` records the additive modifier at press; `on_drag` hosts the marquee, on a `DragSurface` enclosing the body pane. | P22 done (scroll); P23 done (marquee) |
| `crates/teksilo-widgets/src/grid_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Press-time tile selection, tap / double-tap activation, and `start_drag` tile reorder — the GridView twin of `list_view/body_pane.rs`. | P23 done |
| `crates/teksilo-widgets/src/grid_view/selection.rs` | *DragPhase* | Rubber-band marquee: a `DragPhase` handler that skips a press landing on a tile, then paints and auto-scrolls the band at a pointer-kind-aware edge. | P23 done. P25 verified and changed nothing: the marquee sits on a `DragSurface` enclosing the body pane, which is the only shape the tree arms `DragActivation` for, so a contact's marquee waits for the hold while a mouse latches at its own slop. Stated in the module's touch section. |
| `crates/teksilo-widgets/src/link.rs` | `on_tap`, `on_hover` | Tap follows the link; hover underlines and switches the cursor. | P24 done |
| `crates/teksilo-widgets/src/list_source.rs` | `on_drag` | `on_drag_out(k)` is a `ListDataSource` policy callback, not a pointer handler — no pointer plumbing to migrate. | no change needed — no pointer plumbing to migrate. |
| `crates/teksilo-widgets/src/list_view/widget_impl.rs` | `on_drag`, `on_scroll` | Scroll (wheel + pan) now comes from `common::scrollable`; owns the drop-indicator signals the body pane writes. | P22 done (scroll) |
| `crates/teksilo-widgets/src/list_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Selection on press for a mouse and on release for a finger, tap / double-tap activation, `start_drag` row export from an enclosing `DragSurface`. | P23 done |
| `crates/teksilo-widgets/src/menu_bar.rs` | `on_tap`, `on_hover` | Tap a trigger opens its dropdown; hover switches between open menus with no click. | P29 done |
| `crates/teksilo-widgets/src/menu_item.rs` | `PointerLeave`, `on_tap`, `on_hover` | Tap activates; hover opens a submenu after a dwell; `PointerLeave` starts the dismissal grace (safe triangle). | P29 done |
| `crates/teksilo-widgets/src/notification/log.rs` | `on_tap` | Tap a log row replays its action. | P25 — **no change needed**, tested. The row is a `StandardListItem`, so its floor moved with every other list row; the tap replays on the release. `notification::log::tests::a_finger_taps_a_log_row_to_replay_its_entry`. |
| `crates/teksilo-widgets/src/overlay_trigger.rs` | `on_tap` | Tap the trigger toggles its overlay. | P25 — **no change needed**: the wrapper has no geometry and no press visual of its own. It forwards the caller's child, routes the opening tap onto that child's external bucket (the keys and the AT action go to whichever node takes focus), and the activation is an `on_tap`. Reached through `Dialog` and `Snackbar`, whose own rows carry the tests. |
| `crates/teksilo-widgets/src/password_field.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerLeave`, `hover_within` | Press/release/leave over the field and the reveal toggle; `hover_within` drives the composite halo. | P27 done |
| `crates/teksilo-widgets/src/primitives/dead_zone.rs` | `on_tap`, `on_drag` | Sets `gesture_dead_zone(true)` plus a no-op `on_tap`/`on_drag` pair that absorbs a press on the wrapper's own bare area. A4 keeps the flag. | P08 — reviewed and **kept**: the flag governs whether an ancestor may enrol the subtree's press, the absorbers are what give the wrapper a gesture arena, and the arena is what stops the bubble for a press on its own bare area. Stated in the module's own touch section. |
| `crates/teksilo-widgets/src/primitives/text_input_field.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_hover` | Installs the pointer handler set for the editable single-line surface; double/triple tap word and line selection; hover cursor. | P27 done |
| `crates/teksilo-widgets/src/primitives/text_input_field/mouse.rs` | `PointerDown`, `PointerUp`, `PointerMove` | Press places the caret and latches a selection, move extends it, release ends it. | P27 done |
| `crates/teksilo-widgets/src/primitives/text_widget.rs` | `on_pointer_event`, `PointerMove`, `PointerLeave`, `on_tap`, `on_hover` | Inline-link hit testing: `PointerMove` re-resolves the hovered link run and swaps the cursor, `PointerLeave` clears it, `on_tap` follows. | P24 done — **and it corrected this row**: the Ctrl gate is `rich_text/mouse.rs`'s, not this file's, so an inline link always followed a plain tap. Link runs are text-height and take WCAG 2.2 SC 2.5.8's *inline* exception, so they are neither raised to the floor nor reported as target regions. |
| `crates/teksilo-widgets/src/primitives/twist_arrow.rs` | `on_tap` | Tap the 12 dp chevron toggles a tree node's expansion. | P24 done |
| `crates/teksilo-widgets/src/radio_button.rs` | `on_tap`, `on_hover` | Tap selects; hover tint; 24 dp `MinSize` hit area around a 19 dp visual dot. | P24 done |
| `crates/teksilo-widgets/src/radio_tile.rs` | `on_tap`, `on_hover` | Tap the whole card selects the option; hover tint. 44 dp row height already. | P25 — **fixed**: the tile's pressed appearance was written by the `Space` path alone, and a tap by a contact left it resting `Hovered` with nothing on it. Now `bind_press_state` plus a kind-aware resting state. `a_press_on_a_radio_tile_shows_its_pressed_chrome` and `a_finger_tapping_a_radio_tile_leaves_it_untinted` (`tests/composite_touch.rs`). |
| `crates/teksilo-widgets/src/rich_text.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_drag`, `on_scroll` | Scroll (wheel + pan) now comes from `common::text_scroll`; installs the editor's pointer handler set and the double/triple tap and drag hooks. | P22 done (scroll) |
| `crates/teksilo-widgets/src/rich_text/mouse.rs` | `PointerDown`, `PointerUp`, `PointerMove`, `start_drag` | Press latches a selection or grabs an embedded image's resize grip (9 dp + 5 dp bespoke slop); `start_drag` exports a selected fragment. P22 removed this file's `handle_scroll`; the build site installs `common::text_scroll` instead. | P28 done |
| `crates/teksilo-widgets/src/scroll_area.rs` | `on_scroll` | Scroll (wheel + pan) now comes from `common::scrollable`, which this file is the reference adopter of; `on_swipe` is declared but unreachable today. | P21 done |
| `crates/teksilo-widgets/src/scroll_bar.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Thumb drag (`on_drag`), track tap paging, hover thickens the bar from 4 dp to 8 dp. The thumb is paint geometry inside one leaf node, reached by a finger through a 48 dp `hit_outset` over the unchanged paint, with the thumb and its paging track published via `target_regions`. | P21 done |
| `crates/teksilo-widgets/src/search_field.rs` | `on_tap`, `on_hover` | Tap the clear glyph; hover tint on the field and the suggestion rows. | P27 done |
| `crates/teksilo-widgets/src/segmented_control.rs` | `on_hover` | `PointerLeave` clears the hovered segment index at the container level. | P24 done |
| `crates/teksilo-widgets/src/segmented_control/cell.rs` | `on_tap`, `on_hover` | Tap a segment selects it; hover tint per segment. | P24 done |
| `crates/teksilo-widgets/src/slider.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap-to-jump plus `on_drag` on a 14 dp thumb over a 4 dp track; hover grows the thumb. | P24 done |
| `crates/teksilo-widgets/src/snackbar.rs` | `on_tap`, *TapEvent* | Tap the trigger presents the snackbar; tap an action row inside it. | P25 — **no change needed**, tested: the trigger presents on the release and the in-surface actions are buttons and links with their own targets. `a_finger_taps_a_snackbar_open` (`tests/composite_touch.rs`). |
| `crates/teksilo-widgets/src/spin_box.rs` | `on_scroll` | `on_scroll` increments/decrements the value — a wheel handler that is NOT a pan. P22 decided **against** the `touch_action(PAN_Y)` this row asked for: the claimant chain never reaches this handler, so the declaration would only narrow (forbidding a horizontal pan and a pinch through the field) for a guarantee already structural. See the module header. | P22 done (no change, reasoned) |
| `crates/teksilo-widgets/src/spin_box/step_button.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_hover` | Press-and-hold auto-repeat on an 18 x 13 dp step button; tap steps once; hover tint. | P24 done |
| `crates/teksilo-widgets/src/split_button.rs` | `on_tap`, `hover_within` | Tap the action half or the 22 dp chevron half — an in-node coordinate split; `hover_within` drives the shared frame halo. | P25 — **fixed**, and the surface description above was wrong: the two halves are **separate nodes** with a 1 dp divider between them, not an in-node coordinate split, so `partition_targets` (which the density inventory promised) is the wrong mechanism — a partition would have had to move the painted boundary at Compact. The chevron is a `ChevronRegion` widget declaring a `hit_outset` with the whole shortfall on its leading edge, zero for a precise pointer. Three cases in `tests/composite_touch.rs`, including the mouse's unchanged boundary. |
| `crates/teksilo-widgets/src/splitter/handle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_double_tap`, `on_hover`, `capture_pointer` | Returns `Ignored` on press and claims on `PointerMove` via an explicit `capture_pointer` — no recognizer. Double tap collapses. Hover reveals the handle after a dwell; 6 dp gutter. | P30 done |
| `crates/teksilo-widgets/src/standard_item.rs` | `on_hover` | `on_hover` drives the row hover background for `ListView` / `TreeView` delegates. | P25 — **fixed** (the height) and **measured** (the press). `layout_response` read the raw Compact constants while `StandardItemRecipe` had carried both projections since the density sweep, so every list row stayed 28 dp at Touch: `a_standard_row_follows_the_density_ladder`. The row's press belongs to the view's body pane, not the row — its arena-less node can never own one, measured by `a_list_rows_press_belongs_to_the_body_pane_not_the_row` — so its `Pressed` chrome stays reachable only through a caller-supplied `interaction_signal`. Hover is decoration plus the reveal policy (P29). |
| `crates/teksilo-widgets/src/stepper/indicator.rs` | `on_tap` | Tap a step marker jumps to that step. | P24 done |
| `crates/teksilo-widgets/src/stepper/wizard.rs` | `on_tap` | Tap the launcher presents the wizard modal. | P24 done |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | `on_pointer_event`, `on_drag` | `on_pointer_event` remaps the wheel to horizontal strip scrolling; `on_drag` runs tab reorder with edge auto-scroll. P22 made **no change**: the strip owns no `on_scroll` and its remap is a *preview* arm, which a synthesised pan cannot reach (the claimant walk is a direct dispatch per claimant, no preview pass) — so a vertical pan over a horizontal strip already goes to the container around it, which is what is wanted. | P22 done (no change, reasoned) |
| `crates/teksilo-widgets/src/tab_widget/header.rs` | `on_pointer_event`, `PointerUp`, `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap selects the tab, middle-click on `PointerUp` closes it, hover reveals the close button, `on_drag` starts the tab drag. Label vs close button is an in-node split. | P29 done |
| `crates/teksilo-widgets/src/table_view/widget_impl.rs` | `on_drag`, `on_scroll` | Scroll (wheel + pan + the Shift remap) now comes from `common::scrollable`; owns the drop-indicator and column-drag signals. | P22 done (scroll) |
| `crates/teksilo-widgets/src/table_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Row selection on `PointerDown`, tap / double-tap activation and `start_drag` row reorder — the TableView twin of `list_view/body_pane.rs`, plus the per-cell selection. | P23 done |
| `crates/teksilo-widgets/src/table_view/header.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_drag`, `on_hover`, `capture_pointer`, `start_drag` | Column resize by `capture_pointer` off a 4 dp grip, column reorder by `on_drag`, sort on tap; the cell is split by coordinate into a label zone and a filter zone. The plain press answers `Ignored` so the strip is a pan surface, and a coarse pointer's reorder waits for a `long_press`. | P23 done (press / reorder). **P25 for the split**: the label and filter zones now come from `partition_targets` through one `header_cell_zones` function, which gives the filter zone the density's target floor (its glyph and padding are 20 dp — below the floor at every density), answers in reading order so the hand-written RTL branch is gone, and also answers `Widget::target_regions` — so the geometry an audit measures is the geometry the press test uses. Three cases in `tests/composite_touch.rs`. |
| `crates/teksilo-widgets/src/text_input.rs` | `on_tap` | Tap the 16 dp clear button. | P27 done |
| `crates/teksilo-widgets/src/title_bar/controls.rs` | `on_tap`, `on_hover` | Tap minimise / maximise / close; hover tint per button. | P25 — **no change needed**, measured: a 46 x 32 dp cell clears the 24 dp floor on both axes at every density, and each button activates on the release. Not density-projected on purpose — the cell's height is the title bar's, which the platform chrome sizes. `title_bar::controls::tests::a_window_control_cell_clears_the_conformance_floor_at_every_density`. |
| `crates/teksilo-widgets/src/title_bar/drag_region.rs` | `on_pointer_event`, `PointerDown`, `on_double_tap`, `on_drag`, *DragPhase* | `on_drag` from `DragPhase::Started` hands the window move to the OS; double tap maximises; `PointerDown` is recorded but is not the actuation. | P30 done |
| `crates/teksilo-widgets/src/title_bar/resize_strip.rs` | `on_pointer_event`, `PointerDown` | A raw `PointerDown` starts the OS resize immediately — no recognizer, no slop. 6 dp strip. | P30 done |
| `crates/teksilo-widgets/src/toast/host.rs` | `on_pointer_event` | `on_pointer_event` drains the queue tick and drives hover-pause for the toast group. | P29 done |
| `crates/teksilo-widgets/src/toast/surface.rs` | `on_tap`, `on_hover` | Tap the toast body or an action; hover pauses the auto-dismiss timer. | P29 done |
| `crates/teksilo-widgets/src/toggle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerLeave`, `on_tap`, `on_hover` | Press/release/leave interaction state, tap flips the `Signal<bool>`, hover tint. | P24 done |
| `crates/teksilo-widgets/src/tool_box.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap a section header expands it; `on_drag` + `start_drag` move a section; hover tint on the rotated header strip. | P25 — **fixed**: `HeaderInteraction::Pressed` was written by the keyboard path alone, and a tap by a contact left the header resting `Hovered`. Now `bind_press_state` plus a kind-aware resting state. `a_press_on_a_tool_box_header_shows_its_pressed_chrome` and `a_finger_tapping_a_tool_box_header_leaves_it_untinted` (`tests/composite_touch.rs`). |
| `crates/teksilo-widgets/src/tree_source.rs` | `on_drag` | `on_drag_out(k)` is a `TreeDataSource` policy callback, not a pointer handler — no pointer plumbing to migrate. | no change needed — no pointer plumbing to migrate. |
| `crates/teksilo-widgets/src/tree_table_view/widget_impl.rs` | `on_drag`, `on_scroll` | Scroll (wheel + pan + the Shift remap) now comes from `common::scrollable`; owns the drop-indicator and column-drag signals. | P22 done (scroll) |
| `crates/teksilo-widgets/src/tree_table_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Row selection on `PointerDown`, tap / double-tap activation, expand-collapse on the twist arrow and `start_drag` row reorder. | P23 done |
| `crates/teksilo-widgets/src/tree_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Row selection on `PointerDown`, tap / double-tap activation, chevron toggle and `start_drag` subtree reorder. | P23 done |
| `crates/teksilo-widgets/src/tree_view/widget_impl.rs` | `on_drag`, `on_scroll` | Scroll (wheel + pan) now comes from `common::scrollable`; `on_drag`/`start_drag` wiring and the drop-third computation for the tree. | P22 done (scroll) |

## Prose-only mentions

These eight files match the marker grep but only inside doc comments or `//`
commentary. They carry no pointer handler and are out of scope; they are listed so a
later reader running the bare grep does not think they were missed.

| File | Mention |
| --- | --- |
| `crates/teksilo-widgets/src/tree_view.rs` | doc for `auto_toggle_on_row_press` and the drop-feedback signal |
| `crates/teksilo-widgets/src/rich_text/context_menu.rs` | comment about the dispatch target of a `PointerDown` |
| `crates/teksilo-widgets/src/rich_text/frame_loop.rs` | comment about `PointerMove` calling `request_frame()` |
| `crates/teksilo-widgets/src/toast/registry.rs` | doc referring to the host's `on_pointer_event` handler (the hover-pause state itself lives here as `pause_on_hover_group`, driven by `toast/host.rs`) |
| `crates/teksilo-widgets/src/grid_view/layout/strategy.rs` | doc for the `on_drag_hover` move contract |
| `crates/teksilo-widgets/src/styles/recipe_tab_style.rs` | comment about the drop-indicator's `on_drag_hover` handler |
| `crates/teksilo-widgets/src/table_view/imperative.rs` | comment about a first-`PointerMove` recursion hazard |
| `crates/teksilo-widgets/src/tree_view/builder.rs` | doc for the row-body `PointerUp` auto-toggle |

## Files named by a package that carry no pointer code

The work breakdown names these; none of them has a pointer handler today, so they
appear in no row above. Recorded so the sweeps do not go looking.

`badge.rs` · `color_edit.rs` · `common/scroll.rs` · `date_edit.rs` ·
`date_range_edit.rs` · `date_time_edit.rs` · `drag_preview.rs` ·
`file_picker_field.rs` · `hex_color_input.rs` · `icon_button.rs` · `menu_list.rs` ·
`time_edit.rs` · `title_bar/window_frame.rs` · `toolbar.rs` · `tree_view.rs`

`color_edit.rs` is one of these and was named by P25's own clause: it carries no
pointer handler at all — its swatch, its hex field and its picker popover are each
their own widget with their own row — so there was nothing in it to migrate.

`common/drag_autoscroll.rs` now exists: P23 created it by hoisting the five
duplicated `EDGE` / `MAX_VELOCITY` pairs listed in
[the density inventory](density-inventory.md) §3b.

## What P25 did not do, and why

Two things the clause or an earlier artifact asked for are **refused**, each with
the measurement that justifies it. Both are recorded so a later reader does not
take them for oversights.

1. **The `SplitButton` chevron is not partitioned.** The density inventory
   promised "hit via `partition_targets` with a 24 dp floor"; the two halves are
   separate nodes with a hairline divider between them, so a partition would have
   had to *move the painted boundary* — at Compact, against the programme
   invariant. A kind-gated `hit_outset` on the chevron delivers the same floor
   and leaves a mouse's boundary exactly where it is drawn.
2. **A tab's label and its close button are not partitioned either.** The clause
   asked for "tab label/close through `partition_targets` + `target_regions`";
   the close affordance is a real `IconButton` node at `IconButtonSize::Compact`
   — 24 dp at Compact and on the ladder above it — so there is no in-node split
   to carve, and `tab_widget/header.rs`'s own touch section says so. The reveal
   problem it *does* have (hover-only, invisible to a finger) is the hover
   census's and was answered by `RevealPolicy`.

And one deliberate limit: `docking/activity_bar.rs`'s rail item extent stays
un-projected. See the P25 entry in the density inventory's corrections for the
measurement.
