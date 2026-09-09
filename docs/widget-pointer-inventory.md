<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Widget pointer inventory

Every file under `crates/teksilo-widgets/src/` that carries pointer, gesture or hover
code, with the touch-migration package that owns it. This is the checklist the
control sweeps (P22–P31) size themselves from, and P25's exit criterion is that every
row here is either migrated with a test or carries a written no-change reason.

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

**78 files** carry real pointer code. **71** are claimed by a migration package;
**7** are not:

| Verdict | Files |
| --- | ---: |
| `P21` (scrollables reference) — done | 2 |
| `P22` (scrollable migration) — done | 10 |
| `P23` (data-view press/reorder) — done | 8 |
| `pending P24` (controls sweep) | 18 |
| `pending P25` (composite / partitioned / docking) | 17 |
| `pending P27` (single-line text) | 5 |
| `pending P28` (rich text + code editor) | 2 |
| `pending P29` (menus, tooltips, toasts) | 5 |
| `pending P30` (window chrome) | 4 |
| `no change needed` | 2 |
| **UNCLAIMED — GAP** | **5** |
| **Total** | **78** |

`pending P31` claims no file in this crate: its two named files are
`drag_preview.rs` (which has no pointer handler — it is a paint-only overlay) and
`drop_target.rs`, which P25 also names and which is listed under P25 here so every
row carries exactly one verdict.

`splitter/handle.rs` is named by **both** P29 (its 400 ms dwell reveal) and P30 (its
grab geometry). Its *pointer* code — the `Ignored`-on-press / `capture_pointer`-on-move
latch — belongs to P30, so that is its verdict; the dwell reveal is row 2 of
[the hover census](hover-affordance-census.md) and stays P29's.

## The unclaimed files

Nine files carried real pointer code that the work breakdown assigned to nobody. Four
of them were the largest single omission in the plan: **`list_view/body_pane.rs` was
claimed by P23, but its four siblings were not**, and all five implement the same
press-time row selection, tap/double-tap activation and `start_drag` reorder that P23
exists to convert. P23 took all four, so four rows below are settled and five remain
— each of those five with a suggested owner, none of them formally claimed.

| File | Why it matters | Owner |
| --- | --- | --- |
| `grid_view/body_pane.rs` | press-time tile selection + `start_drag` reorder | P23 — done |
| `table_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 — done |
| `tree_table_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 — done |
| `tree_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 — done (P23 names `tree_view`, but `tree_view.rs` itself has only prose mentions) |
| `combo_box.rs` | the closed-box trigger tap and field hover; P25 claims only `combo_box/item.rs` | P25 |
| `primitives/text_widget.rs` | inline-link hover cursor + Ctrl-gated tap-to-follow — unreachable on touch | P24 or P29 |
| `avatar.rs` | a tappable control with no target floor | P24 |
| `snackbar.rs` | trigger tap + in-surface action taps | P29 |
| `primitives/dead_zone.rs` | `gesture_dead_zone(true)` plus a no-op tap/drag absorber; A4 preserves the flag but the absorber pair needs re-checking against `PointerSequence` | P08 (core) |

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
| `crates/teksilo-widgets/src/accordion.rs` | `on_tap`, `on_drag`, *DragPhase* | Header tap toggles the disclosure; `on_drag` on the header is the dock panel drag handle (trailing slot wrapped in a `DeadZone`). | pending P25 |
| `crates/teksilo-widgets/src/avatar.rs` | `on_tap` | Optional `on_tap` action on the avatar circle; no press visual, no target floor. | **UNCLAIMED — GAP**; suggested owner P24 (a tappable control with no target floor) |
| `crates/teksilo-widgets/src/breadcrumb.rs` | `on_tap`, `on_hover` | Tap a crumb to navigate; hover tint per crumb; overflow `…` opens a menu. | pending P24 |
| `crates/teksilo-widgets/src/button.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_hover`, *TapEvent* | Press/release interaction state via `on_pointer_event`, `on_tap` activation, `on_hover` tint — the reference control. | pending P24 |
| `crates/teksilo-widgets/src/calendar/cell.rs` | `on_tap` | Tap a day cell to select the date (32 dp cell). | pending P24 |
| `crates/teksilo-widgets/src/calendar/header.rs` | `on_tap` | Tap prev / next / the "Month Year" zoom-out title in the navigation strip. | pending P25 |
| `crates/teksilo-widgets/src/calendar/zoom_grid.rs` | `on_tap`, `on_hover` | Tap a month or year cell in the coarse-grain grid; hover tint. | pending P25 |
| `crates/teksilo-widgets/src/checkbox.rs` | `on_tap`, `on_hover` | Tap toggles; hover tint; 24 dp `MinSize` hit area around a 19 dp visual box. | pending P24 |
| `crates/teksilo-widgets/src/code_editor/completion.rs` | `on_tap` | Tap a suggestion row in the caret-anchored completion popup. | pending P25 |
| `crates/teksilo-widgets/src/code_editor/log_view.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_scroll` | Scroll (wheel + pan) now comes from `common::text_scroll`, the helper the three text surfaces share; double/triple tap selects a line span. | P22 done (scroll) |
| `crates/teksilo-widgets/src/code_editor/mouse.rs` | `PointerDown`, `PointerUp`, `PointerMove` | Press latches a text selection, move extends it, release ends it; drag auto-scroll at the margins. P22 removed this file's `handle_scroll`; the two build sites install `common::text_scroll` instead. | pending P28 |
| `crates/teksilo-widgets/src/code_editor/widget.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_scroll` | Scroll (wheel + pan) now comes from `common::text_scroll`; double/triple tap word and line selection. | P22 done (scroll) |
| `crates/teksilo-widgets/src/color_picker/alpha_strip.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` to set alpha along a 14 dp strip. | pending P24 |
| `crates/teksilo-widgets/src/color_picker/hsv_canvas.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` over a 2-D saturation x value canvas. | pending P24 |
| `crates/teksilo-widgets/src/color_picker/hue_strip.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` to set hue along a 14 dp strip. | pending P24 |
| `crates/teksilo-widgets/src/color_picker/swatch.rs` | `on_tap` | Tap a 22 dp swatch to pick its colour. | pending P24 |
| `crates/teksilo-widgets/src/combo_box.rs` | `on_tap`, `on_hover` | Tap the closed box opens the dropdown overlay; `on_hover` drives the field tint. | **UNCLAIMED — GAP**; suggested owner P25 (P25 claims `combo_box/item.rs` but not the closed-box trigger) |
| `crates/teksilo-widgets/src/combo_box/item.rs` | `on_tap`, `on_hover` | Tap a dropdown row to commit; hover highlights the active row. | pending P25 |
| `crates/teksilo-widgets/src/data_views.rs` | `on_drag` | Shared row drag-and-drop substrate: `RowExport<T>`, drag payload construction and the reorder/foreign-drop verdicts for all five views, plus `deferred_select`, `DragSurface` and `press_absorber`. | P23 done |
| `crates/teksilo-widgets/src/dialog.rs` | `on_tap` | Tap the trigger to present; scrim tap dismisses; footer button taps. | pending P25 |
| `crates/teksilo-widgets/src/docking/activity_bar.rs` | `on_tap`, `on_drag`, `start_drag`, *DragPhase* | Tap a rail item to select or hide its side; `start_drag` reorders rail items and transfers activities between sides. | pending P25 |
| `crates/teksilo-widgets/src/docking/panel.rs` | `on_drag`, `start_drag` | Accordion-header `on_drag` starts a dock drag; the five-zone drop overlay routes the drop to stack/split. | pending P25 |
| `crates/teksilo-widgets/src/docking/resize_handle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_double_tap`, `on_hover`, `capture_pointer` | Press captures the pointer and resizes the side band; double tap snaps to hide; hover shows the grip. 6 dp gutter. | pending P30 |
| `crates/teksilo-widgets/src/drop_target.rs` | `on_drag` | `on_drag_hover` / `on_drop` over the wrapped child, optionally partitioned into five `DropRegion` zones. | pending P25 |
| `crates/teksilo-widgets/src/drop_zone.rs` | `on_drag` | `on_drag_hover` / `on_drop` for external (OS) file, text and URL drops. | pending P25 |
| `crates/teksilo-widgets/src/grid_view.rs` | `on_pointer_event`, `PointerDown`, `on_drag`, `on_scroll` | Scroll (wheel + pan) now comes from `common::scrollable`; container `on_pointer_event` records the additive modifier at press; `on_drag` hosts the marquee, on a `DragSurface` enclosing the body pane. | P22 done (scroll); P23 done (marquee) |
| `crates/teksilo-widgets/src/grid_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Press-time tile selection, tap / double-tap activation, and `start_drag` tile reorder — the GridView twin of `list_view/body_pane.rs`. | P23 done |
| `crates/teksilo-widgets/src/grid_view/selection.rs` | *DragPhase* | Rubber-band marquee: a `DragPhase` handler that skips a press landing on a tile, then paints and auto-scrolls the band at a pointer-kind-aware edge. | P23 done |
| `crates/teksilo-widgets/src/link.rs` | `on_tap`, `on_hover` | Tap follows the link; hover underlines and switches the cursor. | pending P24 |
| `crates/teksilo-widgets/src/list_source.rs` | `on_drag` | `on_drag_out(k)` is a `ListDataSource` policy callback, not a pointer handler — no pointer plumbing to migrate. | no change needed — no pointer plumbing to migrate. |
| `crates/teksilo-widgets/src/list_view/widget_impl.rs` | `on_drag`, `on_scroll` | Scroll (wheel + pan) now comes from `common::scrollable`; owns the drop-indicator signals the body pane writes. | P22 done (scroll) |
| `crates/teksilo-widgets/src/list_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Selection on press for a mouse and on release for a finger, tap / double-tap activation, `start_drag` row export from an enclosing `DragSurface`. | P23 done |
| `crates/teksilo-widgets/src/menu_bar.rs` | `on_tap`, `on_hover` | Tap a trigger opens its dropdown; hover switches between open menus with no click. | pending P29 |
| `crates/teksilo-widgets/src/menu_item.rs` | `PointerLeave`, `on_tap`, `on_hover` | Tap activates; hover opens a submenu after a dwell; `PointerLeave` starts the dismissal grace (safe triangle). | pending P29 |
| `crates/teksilo-widgets/src/notification/log.rs` | `on_tap` | Tap a log row replays its action. | pending P25 |
| `crates/teksilo-widgets/src/overlay_trigger.rs` | `on_tap` | Tap the trigger toggles its overlay. | pending P25 |
| `crates/teksilo-widgets/src/password_field.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerLeave`, `hover_within` | Press/release/leave over the field and the reveal toggle; `hover_within` drives the composite halo. | pending P27 |
| `crates/teksilo-widgets/src/primitives/dead_zone.rs` | `on_tap`, `on_drag` | Sets `gesture_dead_zone(true)` plus a no-op `on_tap`/`on_drag` pair that absorbs a press on the wrapper's own bare area. A4 keeps the flag; the absorber pair needs re-checking against `PointerSequence`. | **UNCLAIMED** — review under P08 (core arbitration) |
| `crates/teksilo-widgets/src/primitives/text_input_field.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_hover` | Installs the pointer handler set for the editable single-line surface; double/triple tap word and line selection; hover cursor. | pending P27 |
| `crates/teksilo-widgets/src/primitives/text_input_field/mouse.rs` | `PointerDown`, `PointerUp`, `PointerMove` | Press places the caret and latches a selection, move extends it, release ends it. | pending P27 |
| `crates/teksilo-widgets/src/primitives/text_widget.rs` | `on_pointer_event`, `PointerMove`, `PointerLeave`, `on_tap`, `on_hover` | Inline-link hit testing: `PointerMove` re-resolves the hovered link run and swaps the cursor, `PointerLeave` clears it, `on_tap` follows (Ctrl-gated). | **UNCLAIMED — GAP**; suggested owner P24 or P29 (inline-link hover/tap; Ctrl-gated follow is unreachable on touch) |
| `crates/teksilo-widgets/src/primitives/twist_arrow.rs` | `on_tap` | Tap the 12 dp chevron toggles a tree node's expansion. | pending P24 |
| `crates/teksilo-widgets/src/radio_button.rs` | `on_tap`, `on_hover` | Tap selects; hover tint; 24 dp `MinSize` hit area around a 19 dp visual dot. | pending P24 |
| `crates/teksilo-widgets/src/radio_tile.rs` | `on_tap`, `on_hover` | Tap the whole card selects the option; hover tint. 44 dp row height already. | pending P25 |
| `crates/teksilo-widgets/src/rich_text.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_drag`, `on_scroll` | Scroll (wheel + pan) now comes from `common::text_scroll`; installs the editor's pointer handler set and the double/triple tap and drag hooks. | P22 done (scroll) |
| `crates/teksilo-widgets/src/rich_text/mouse.rs` | `PointerDown`, `PointerUp`, `PointerMove`, `start_drag` | Press latches a selection or grabs an embedded image's resize grip (9 dp + 5 dp bespoke slop); `start_drag` exports a selected fragment. P22 removed this file's `handle_scroll`; the build site installs `common::text_scroll` instead. | pending P28 |
| `crates/teksilo-widgets/src/scroll_area.rs` | `on_scroll` | Scroll (wheel + pan) now comes from `common::scrollable`, which this file is the reference adopter of; `on_swipe` is declared but unreachable today. | P21 done |
| `crates/teksilo-widgets/src/scroll_bar.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Thumb drag (`on_drag`), track tap paging, hover thickens the bar from 4 dp to 8 dp. The thumb is paint geometry inside one leaf node, reached by a finger through a 48 dp `hit_outset` over the unchanged paint, with the thumb and its paging track published via `target_regions`. | P21 done |
| `crates/teksilo-widgets/src/search_field.rs` | `on_tap`, `on_hover` | Tap the clear glyph; hover tint on the field and the suggestion rows. | pending P27 |
| `crates/teksilo-widgets/src/segmented_control.rs` | `on_hover` | `PointerLeave` clears the hovered segment index at the container level. | pending P24 |
| `crates/teksilo-widgets/src/segmented_control/cell.rs` | `on_tap`, `on_hover` | Tap a segment selects it; hover tint per segment. | pending P24 |
| `crates/teksilo-widgets/src/slider.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap-to-jump plus `on_drag` on a 14 dp thumb over a 4 dp track; hover grows the thumb. | pending P24 |
| `crates/teksilo-widgets/src/snackbar.rs` | `on_tap`, *TapEvent* | Tap the trigger presents the snackbar; tap an action row inside it. | **UNCLAIMED — GAP**; suggested owner P29 (the toast/snackbar family) |
| `crates/teksilo-widgets/src/spin_box.rs` | `on_scroll` | `on_scroll` increments/decrements the value — a wheel handler that is NOT a pan. P22 decided **against** the `touch_action(PAN_Y)` this row asked for: the claimant chain never reaches this handler, so the declaration would only narrow (forbidding a horizontal pan and a pinch through the field) for a guarantee already structural. See the module header. | P22 done (no change, reasoned) |
| `crates/teksilo-widgets/src/spin_box/step_button.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_hover` | Press-and-hold auto-repeat on an 18 x 13 dp step button; tap steps once; hover tint. | pending P24 |
| `crates/teksilo-widgets/src/split_button.rs` | `on_tap`, `hover_within` | Tap the action half or the 22 dp chevron half — an in-node coordinate split; `hover_within` drives the shared frame halo. | pending P25 |
| `crates/teksilo-widgets/src/splitter/handle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_double_tap`, `on_hover`, `capture_pointer` | Returns `Ignored` on press and claims on `PointerMove` via an explicit `capture_pointer` — no recognizer. Double tap collapses. Hover reveals the handle after a dwell; 6 dp gutter. | pending P30 |
| `crates/teksilo-widgets/src/standard_item.rs` | `on_hover` | `on_hover` drives the row hover background for `ListView` / `TreeView` delegates. | pending P25 |
| `crates/teksilo-widgets/src/stepper/indicator.rs` | `on_tap` | Tap a step marker jumps to that step. | pending P24 |
| `crates/teksilo-widgets/src/stepper/wizard.rs` | `on_tap` | Tap the launcher presents the wizard modal. | pending P24 |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | `on_pointer_event`, `on_drag` | `on_pointer_event` remaps the wheel to horizontal strip scrolling; `on_drag` runs tab reorder with edge auto-scroll. P22 made **no change**: the strip owns no `on_scroll` and its remap is a *preview* arm, which a synthesised pan cannot reach (the claimant walk is a direct dispatch per claimant, no preview pass) — so a vertical pan over a horizontal strip already goes to the container around it, which is what is wanted. | P22 done (no change, reasoned) |
| `crates/teksilo-widgets/src/tab_widget/header.rs` | `on_pointer_event`, `PointerUp`, `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap selects the tab, middle-click on `PointerUp` closes it, hover reveals the close button, `on_drag` starts the tab drag. Label vs close button is an in-node split. | pending P29 |
| `crates/teksilo-widgets/src/table_view/widget_impl.rs` | `on_drag`, `on_scroll` | Scroll (wheel + pan + the Shift remap) now comes from `common::scrollable`; owns the drop-indicator and column-drag signals. | P22 done (scroll) |
| `crates/teksilo-widgets/src/table_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Row selection on `PointerDown`, tap / double-tap activation and `start_drag` row reorder — the TableView twin of `list_view/body_pane.rs`, plus the per-cell selection. | P23 done |
| `crates/teksilo-widgets/src/table_view/header.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_drag`, `on_hover`, `capture_pointer`, `start_drag` | Column resize by `capture_pointer` off a 4 dp grip, column reorder by `on_drag`, sort on tap; the cell is split by coordinate into a label zone and a filter zone. The plain press answers `Ignored` so the strip is a pan surface, and a coarse pointer's reorder waits for a `long_press`. | P23 done |
| `crates/teksilo-widgets/src/text_input.rs` | `on_tap` | Tap the 16 dp clear button. | pending P27 |
| `crates/teksilo-widgets/src/title_bar/controls.rs` | `on_tap`, `on_hover` | Tap minimise / maximise / close; hover tint per button. | pending P25 |
| `crates/teksilo-widgets/src/title_bar/drag_region.rs` | `on_pointer_event`, `PointerDown`, `on_double_tap`, `on_drag`, *DragPhase* | `on_drag` from `DragPhase::Started` hands the window move to the OS; double tap maximises; `PointerDown` is recorded but is not the actuation. | pending P30 |
| `crates/teksilo-widgets/src/title_bar/resize_strip.rs` | `on_pointer_event`, `PointerDown` | A raw `PointerDown` starts the OS resize immediately — no recognizer, no slop. 6 dp strip. | pending P30 |
| `crates/teksilo-widgets/src/toast/host.rs` | `on_pointer_event` | `on_pointer_event` drains the queue tick and drives hover-pause for the toast group. | pending P29 |
| `crates/teksilo-widgets/src/toast/surface.rs` | `on_tap`, `on_hover` | Tap the toast body or an action; hover pauses the auto-dismiss timer. | pending P29 |
| `crates/teksilo-widgets/src/toggle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerLeave`, `on_tap`, `on_hover` | Press/release/leave interaction state, tap flips the `Signal<bool>`, hover tint. | pending P24 |
| `crates/teksilo-widgets/src/tool_box.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap a section header expands it; `on_drag` + `start_drag` move a section; hover tint on the rotated header strip. | pending P25 |
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

`common/drag_autoscroll.rs` does not exist yet — P23 creates it by hoisting the five
duplicated `EDGE` / `MAX_VELOCITY` pairs listed in
[the density inventory](density-inventory.md) §3b.
