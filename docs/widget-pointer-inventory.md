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

**78 files** carry real pointer code. **67** are claimed by a migration package;
**11** are not:

| Verdict | Files |
| --- | ---: |
| `pending P21` (scrollables reference) | 2 |
| `pending P22` (scrollable migration) | 10 |
| `pending P23` (data-view press/reorder) | 4 |
| `pending P24` (controls sweep) | 18 |
| `pending P25` (composite / partitioned / docking) | 17 |
| `pending P27` (single-line text) | 5 |
| `pending P28` (rich text + code editor) | 2 |
| `pending P29` (menus, tooltips, toasts) | 5 |
| `pending P30` (window chrome) | 4 |
| `no change needed` | 2 |
| **UNCLAIMED — GAP** | **9** |
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

Nine files carry real pointer code that the work breakdown assigns to nobody. Four of
them are the largest single omission in the plan: **`list_view/body_pane.rs` is
claimed by P23, but its four siblings are not**, and all five implement the same
press-time row selection, tap/double-tap activation and `start_drag` reorder that P23
exists to convert.

| File | Why it matters | Suggested owner |
| --- | --- | --- |
| `grid_view/body_pane.rs` | press-time tile selection + `start_drag` reorder | P23 |
| `table_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 |
| `tree_table_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 |
| `tree_view/body_pane.rs` | press-time row selection + `start_drag` reorder | P23 (P23 names `tree_view`, but `tree_view.rs` itself has only prose mentions) |
| `combo_box.rs` | the closed-box trigger tap and field hover; P25 claims only `combo_box/item.rs` | P25 |
| `primitives/text_widget.rs` | inline-link hover cursor + Ctrl-gated tap-to-follow — unreachable on touch | P24 or P29 |
| `avatar.rs` | a tappable control with no target floor | P24 |
| `snackbar.rs` | trigger tap + in-surface action taps | P29 |
| `primitives/dead_zone.rs` | `gesture_dead_zone(true)` plus a no-op tap/drag absorber; A4 preserves the flag but the absorber pair needs re-checking against `PointerSequence` | P08 (core) |

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
| `crates/teksilo-widgets/src/code_editor/log_view.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_scroll` | `on_scroll` tail-following scroll; double/triple tap selects a line span. | pending P22 |
| `crates/teksilo-widgets/src/code_editor/mouse.rs` | `PointerDown`, `PointerUp`, `PointerMove` | Press latches a text selection, move extends it, release ends it; drag auto-scroll at the margins. | pending P28 |
| `crates/teksilo-widgets/src/code_editor/widget.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_scroll` | `on_scroll` viewport scroll; double/triple tap word and line selection. | pending P22 |
| `crates/teksilo-widgets/src/color_picker/alpha_strip.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` to set alpha along a 14 dp strip. | pending P24 |
| `crates/teksilo-widgets/src/color_picker/hsv_canvas.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` over a 2-D saturation x value canvas. | pending P24 |
| `crates/teksilo-widgets/src/color_picker/hue_strip.rs` | `on_tap`, `on_drag`, *DragPhase* | Tap-to-jump plus `on_drag` to set hue along a 14 dp strip. | pending P24 |
| `crates/teksilo-widgets/src/color_picker/swatch.rs` | `on_tap` | Tap a 22 dp swatch to pick its colour. | pending P24 |
| `crates/teksilo-widgets/src/combo_box.rs` | `on_tap`, `on_hover` | Tap the closed box opens the dropdown overlay; `on_hover` drives the field tint. | **UNCLAIMED — GAP**; suggested owner P25 (P25 claims `combo_box/item.rs` but not the closed-box trigger) |
| `crates/teksilo-widgets/src/combo_box/item.rs` | `on_tap`, `on_hover` | Tap a dropdown row to commit; hover highlights the active row. | pending P25 |
| `crates/teksilo-widgets/src/data_views.rs` | `on_drag` | Shared row drag-and-drop substrate: `RowExport<T>`, drag payload construction and the reorder/foreign-drop verdicts for all five views. | pending P23 |
| `crates/teksilo-widgets/src/dialog.rs` | `on_tap` | Tap the trigger to present; scrim tap dismisses; footer button taps. | pending P25 |
| `crates/teksilo-widgets/src/docking/activity_bar.rs` | `on_tap`, `on_drag`, `start_drag`, *DragPhase* | Tap a rail item to select or hide its side; `start_drag` reorders rail items and transfers activities between sides. | pending P25 |
| `crates/teksilo-widgets/src/docking/panel.rs` | `on_drag`, `start_drag` | Accordion-header `on_drag` starts a dock drag; the five-zone drop overlay routes the drop to stack/split. | pending P25 |
| `crates/teksilo-widgets/src/docking/resize_handle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_double_tap`, `on_hover`, `capture_pointer` | Press captures the pointer and resizes the side band; double tap snaps to hide; hover shows the grip. 6 dp gutter. | pending P30 |
| `crates/teksilo-widgets/src/drop_target.rs` | `on_drag` | `on_drag_hover` / `on_drop` over the wrapped child, optionally partitioned into five `DropRegion` zones. | pending P25 |
| `crates/teksilo-widgets/src/drop_zone.rs` | `on_drag` | `on_drag_hover` / `on_drop` for external (OS) file, text and URL drops. | pending P25 |
| `crates/teksilo-widgets/src/grid_view.rs` | `on_pointer_event`, `PointerDown`, `on_drag`, `on_scroll` | `on_scroll` viewport scroll; container `on_pointer_event` records the additive modifier at press; `on_drag` hosts the marquee. | pending P22 |
| `crates/teksilo-widgets/src/grid_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Press-time tile selection, tap / double-tap activation, and `start_drag` tile reorder — the GridView twin of `list_view/body_pane.rs`. | **UNCLAIMED — GAP**; suggested owner P23 (parallel to the claimed `list_view/body_pane.rs`) |
| `crates/teksilo-widgets/src/grid_view/selection.rs` | *DragPhase* | Rubber-band marquee: a `DragPhase` handler that skips a press landing on a tile, then paints and auto-scrolls the band. | pending P23 |
| `crates/teksilo-widgets/src/link.rs` | `on_tap`, `on_hover` | Tap follows the link; hover underlines and switches the cursor. | pending P24 |
| `crates/teksilo-widgets/src/list_source.rs` | `on_drag` | `on_drag_out(k)` is a `ListDataSource` policy callback, not a pointer handler — no pointer plumbing to migrate. | no change needed — no pointer plumbing to migrate. |
| `crates/teksilo-widgets/src/list_view.rs` | `on_drag`, `on_scroll` | `on_scroll` viewport scroll; owns the drop-indicator signals the body pane writes. | pending P22 |
| `crates/teksilo-widgets/src/list_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Row selection on `PointerDown`, tap / double-tap activation, `capture_pointer` for the reorder drag, `start_drag` row export. | pending P23 |
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
| `crates/teksilo-widgets/src/rich_text.rs` | `on_pointer_event`, `on_double_tap`, `on_triple_tap`, `on_drag`, `on_scroll` | `on_scroll` viewport scroll; installs the editor's pointer handler set and the double/triple tap and drag hooks. | pending P22 |
| `crates/teksilo-widgets/src/rich_text/mouse.rs` | `PointerDown`, `PointerUp`, `PointerMove`, `start_drag` | Press latches a selection or grabs an embedded image's resize grip (9 dp + 5 dp bespoke slop); `start_drag` exports a selected fragment. | pending P28 |
| `crates/teksilo-widgets/src/scroll_area.rs` | `on_scroll` | `on_scroll` wheel handling with clamping and chaining; `on_swipe` is declared but unreachable today. | pending P21 |
| `crates/teksilo-widgets/src/scroll_bar.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Thumb drag (`on_drag`), track tap paging, hover thickens the bar from 4 dp to 8 dp. The thumb is paint geometry inside one leaf node. | pending P21 |
| `crates/teksilo-widgets/src/search_field.rs` | `on_tap`, `on_hover` | Tap the clear glyph; hover tint on the field and the suggestion rows. | pending P27 |
| `crates/teksilo-widgets/src/segmented_control.rs` | `on_hover` | `PointerLeave` clears the hovered segment index at the container level. | pending P24 |
| `crates/teksilo-widgets/src/segmented_control/cell.rs` | `on_tap`, `on_hover` | Tap a segment selects it; hover tint per segment. | pending P24 |
| `crates/teksilo-widgets/src/slider.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap-to-jump plus `on_drag` on a 14 dp thumb over a 4 dp track; hover grows the thumb. | pending P24 |
| `crates/teksilo-widgets/src/snackbar.rs` | `on_tap`, *TapEvent* | Tap the trigger presents the snackbar; tap an action row inside it. | **UNCLAIMED — GAP**; suggested owner P29 (the toast/snackbar family) |
| `crates/teksilo-widgets/src/spin_box.rs` | `on_scroll` | `on_scroll` increments/decrements the value — a wheel handler that is NOT a pan and must declare `touch_action(PAN_Y)` with no claim. | pending P22 |
| `crates/teksilo-widgets/src/spin_box/step_button.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_hover` | Press-and-hold auto-repeat on an 18 x 13 dp step button; tap steps once; hover tint. | pending P24 |
| `crates/teksilo-widgets/src/split_button.rs` | `on_tap`, `hover_within` | Tap the action half or the 22 dp chevron half — an in-node coordinate split; `hover_within` drives the shared frame halo. | pending P25 |
| `crates/teksilo-widgets/src/splitter/handle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_double_tap`, `on_hover`, `capture_pointer` | Returns `Ignored` on press and claims on `PointerMove` via an explicit `capture_pointer` — no recognizer. Double tap collapses. Hover reveals the handle after a dwell; 6 dp gutter. | pending P30 |
| `crates/teksilo-widgets/src/standard_item.rs` | `on_hover` | `on_hover` drives the row hover background for `ListView` / `TreeView` delegates. | pending P25 |
| `crates/teksilo-widgets/src/stepper/indicator.rs` | `on_tap` | Tap a step marker jumps to that step. | pending P24 |
| `crates/teksilo-widgets/src/stepper/wizard.rs` | `on_tap` | Tap the launcher presents the wizard modal. | pending P24 |
| `crates/teksilo-widgets/src/tab_widget/bar.rs` | `on_pointer_event`, `on_drag` | `on_pointer_event` remaps the wheel to horizontal strip scrolling; `on_drag` runs tab reorder with edge auto-scroll. | pending P22 |
| `crates/teksilo-widgets/src/tab_widget/header.rs` | `on_pointer_event`, `PointerUp`, `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap selects the tab, middle-click on `PointerUp` closes it, hover reveals the close button, `on_drag` starts the tab drag. Label vs close button is an in-node split. | pending P29 |
| `crates/teksilo-widgets/src/table_view.rs` | `on_drag`, `on_scroll` | `on_scroll` viewport scroll; owns the drop-indicator and column-drag signals. | pending P22 |
| `crates/teksilo-widgets/src/table_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Row selection on `PointerDown`, tap / double-tap activation and `start_drag` row reorder — the TableView twin of `list_view/body_pane.rs`. | **UNCLAIMED — GAP**; suggested owner P23 (parallel to the claimed `list_view/body_pane.rs`) |
| `crates/teksilo-widgets/src/table_view/header.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerMove`, `on_drag`, `on_hover`, `capture_pointer`, `start_drag` | Column resize by `capture_pointer` off a 4 dp grip, column reorder by `on_drag`, sort on tap; the cell is split by coordinate into a label zone and a filter zone. | pending P23 |
| `crates/teksilo-widgets/src/text_input.rs` | `on_tap` | Tap the 16 dp clear button. | pending P27 |
| `crates/teksilo-widgets/src/title_bar/controls.rs` | `on_tap`, `on_hover` | Tap minimise / maximise / close; hover tint per button. | pending P25 |
| `crates/teksilo-widgets/src/title_bar/drag_region.rs` | `on_pointer_event`, `PointerDown`, `on_double_tap`, `on_drag`, *DragPhase* | `on_drag` from `DragPhase::Started` hands the window move to the OS; double tap maximises; `PointerDown` is recorded but is not the actuation. | pending P30 |
| `crates/teksilo-widgets/src/title_bar/resize_strip.rs` | `on_pointer_event`, `PointerDown` | A raw `PointerDown` starts the OS resize immediately — no recognizer, no slop. 6 dp strip. | pending P30 |
| `crates/teksilo-widgets/src/toast/host.rs` | `on_pointer_event` | `on_pointer_event` drains the queue tick and drives hover-pause for the toast group. | pending P29 |
| `crates/teksilo-widgets/src/toast/surface.rs` | `on_tap`, `on_hover` | Tap the toast body or an action; hover pauses the auto-dismiss timer. | pending P29 |
| `crates/teksilo-widgets/src/toggle.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `PointerLeave`, `on_tap`, `on_hover` | Press/release/leave interaction state, tap flips the `Signal<bool>`, hover tint. | pending P24 |
| `crates/teksilo-widgets/src/tool_box.rs` | `on_tap`, `on_drag`, `on_hover`, *DragPhase* | Tap a section header expands it; `on_drag` + `start_drag` move a section; hover tint on the rotated header strip. | pending P25 |
| `crates/teksilo-widgets/src/tree_source.rs` | `on_drag` | `on_drag_out(k)` is a `TreeDataSource` policy callback, not a pointer handler — no pointer plumbing to migrate. | no change needed — no pointer plumbing to migrate. |
| `crates/teksilo-widgets/src/tree_table_view.rs` | `on_drag`, `on_scroll` | `on_scroll` viewport scroll; owns the drop-indicator and column-drag signals. | pending P22 |
| `crates/teksilo-widgets/src/tree_table_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Row selection on `PointerDown`, tap / double-tap activation, expand-collapse on the twist arrow and `start_drag` row reorder. | **UNCLAIMED — GAP**; suggested owner P23 (parallel to the claimed `list_view/body_pane.rs`) |
| `crates/teksilo-widgets/src/tree_view/body_pane.rs` | `on_pointer_event`, `PointerDown`, `PointerUp`, `on_tap`, `on_double_tap`, `on_drag`, `start_drag`, *DragPhase* | Row selection on `PointerDown`, tap / double-tap activation, chevron toggle and `start_drag` subtree reorder. | **UNCLAIMED — GAP**; suggested owner P23 (P23 names `tree_view`, but `tree_view.rs` itself has no pointer code) |
| `crates/teksilo-widgets/src/tree_view/widget_impl.rs` | `on_drag`, `on_scroll` | `on_scroll` viewport scroll; `on_drag`/`start_drag` wiring and the drop-third computation for the tree. | pending P22 |

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
