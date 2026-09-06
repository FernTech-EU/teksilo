<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Drag-operation census

Every operation in the workspace whose only route is a pointer drag. WCAG 2.2
**SC 2.5.7 Dragging Movements** (AA) requires each of them to gain a single-pointer
alternative that is not a drag; package **P39** is sized from this list, and its exit
criterion is that no row below still reads "none" in the *Non-drag route today*
column.

Measured against `wt-touch` @ `89c7a03c`. Every line number was read, not inferred.
Rows marked **Compliant** already satisfy 2.5.7 and are recorded so P39 does not
re-litigate them.

```bash
# reproduce the candidate set
grep -rnE '\.on_drag\(|start_drag(_with_preview)?\(|capture_pointer\(|DragPhase' \
     --include=*.rs crates/ | grep -v tests.rs
```

## Summary

| Status | Rows |
| --- | ---: |
| Compliant today (keyboard and/or AT route exists) | 17 |
| Partial — an AT action or a menu exists but no keyboard chord, or only part of the operation is covered | 5 |
| **No non-drag route at all** | **12** |
| Not a drag / no obligation (recorded and dismissed) | 4 |

The single highest-leverage fix is row 31: a framework-level keyboard
pick-up / put-down mode in `teksilo-core`'s drag pipeline would retire rows 15, 18,
20 and 30 at once.

## Census

| # | File : line | Operation | What it changes | Non-drag route today | Proposed non-drag command |
| ---: | --- | --- | --- | --- | --- |
| 1 | `crates/teksilo-widgets/src/slider.rs:351` | Drag the thumb to set the value | the bound `Signal<f32>` | **Yes.** `on_key` :389 — arrows ±step, Home/End = min/max; `on_access_action` :427 `Increment`/`Decrement`, advertised :504-505. No PageUp/PageDown. | **Compliant.** Optional: PageUp/PageDown (×10) and `Action::SetValue`. |
| 2 | `crates/teksilo-widgets/src/color_picker/hue_strip.rs:162` | Drag the hue strip | hue `Signal<f32>` (0–360) | **Yes.** `on_key` :190 — arrows ±1°, PageUp/Down ±15°, Home/End; `on_access_action` :243, advertised :358-360. | **Compliant.** |
| 3 | `crates/teksilo-widgets/src/color_picker/alpha_strip.rs:153` | Drag the alpha strip | alpha `Signal<f32>` (0–1) | **Yes.** `on_key` :181 — arrows ±0.01, PageUp/Down ±0.10, Home/End; `on_access_action` :229. | **Compliant.** |
| 4 | `crates/teksilo-widgets/src/color_picker/hsv_canvas.rs:145` | 2-D drag over the saturation × value canvas | `saturation` + `value_hsv` via `set_hsv` | **None directly.** `.focusable(false)` :139, no `on_key`, no `on_access_action`; `accessibility()` :264 emits a bare `GenericContainer` and the parent excludes the subtree (`color_picker.rs:496`). Indirect only: the RGB spinners and hex input (both default on) reach the same colour; the **S/V spinners are default off** (`color_picker.rs:199`). | Default `show_hsv_spinners` to true (or auto-enable it whenever the canvas is shown), **and** make the canvas focusable: arrows ±1 % S/V, Shift+arrows ±10 %, with two `Role::Slider` proxies carrying `Increment`/`Decrement`. |
| 5 | `crates/teksilo-widgets/src/splitter/handle.rs:261` (capture at :296) | Resize a splitter pane | `SplitterModel` pane sizes via `commit_resize` | **Yes.** `on_key` :480 — arrows ±`keyboard_step_px`, Home/End = min/max, Enter = collapse; `on_double_tap` :427; `on_access_action` :555 Increment/Decrement/Collapse/Expand, advertised :712-717. | **Compliant.** |
| 6 | `crates/teksilo-widgets/src/docking/resize_handle.rs:234` | Resize a dock side | `DockingModel::set_side_size` / `set_side_visible` | **Yes.** `on_key` :293 — arrows ±`KEYBOARD_STEP`, Home = hide, End = show, Enter = toggle; `on_double_tap` :285; `on_access_action` :329, advertised :405-409. | **Compliant.** |
| 7 | `crates/teksilo-widgets/src/table_view/header.rs:538` | **Column resize** (TableView *and* TreeTableView — one shared header) | `column_widths` + `column_widths_signal` via `commit_resize` | **Partial.** `on_access_action` :837 Increment/Decrement by `COLUMN_RESIZE_STEP`, advertised :942-943. **No keyboard route:** the header cell is `.focusable(false)` :880 and has no `on_key`. | Make the header cell focusable (roving tab-index across the header row): ←/→ = ±step, Home/End = min/max. Or a header context menu "Column width ▸ Wider / Narrower / Fit to content / Reset". |
| 8 | `crates/teksilo-widgets/src/table_view/header.rs:678` | **Column reorder** (TableView + TreeTableView) | `column_order_signal: Signal<Vec<String>>` (strip `on_drop` :1380) | **None.** No `on_key`, no custom AT action, no context menu — only the programmatic `TableView::set_column_order` (`table_view.rs:1211`). | Header context menu "Move column ▸ Left / Right / To start / To end" plus AT custom actions, mirroring `tab_widget/header.rs:1069-1105`. Optionally Ctrl+Shift+←/→ on a focused header cell. |
| 9 | `crates/teksilo-widgets/src/list_view/body_pane.rs:471` (`start_drag` :507/:509) | ListView row reorder | source `accept_drop` → `ListModel` order; selection follows | **Yes.** `list_view.rs:1229` — `Alt+↑/↓` builds a synthetic `RowDragData` through the same `accept_drop`. | **Compliant** for in-view reorder (export is row 14). |
| 10 | `crates/teksilo-widgets/src/tree_view/body_pane.rs:536` (`start_drag` :570/:572) | TreeView row reorder **and reparent** | `source.keyboard_reorder(..)` → tree shape | **Partial.** `tree_view/widget_impl.rs:450` `Alt+↑/↓` covers **sibling reorder only**; the drag's `DropPosition::Into` (reparenting) has no keyboard route. | `Alt+←/→` = outdent / indent — the outliner convention. |
| 11 | `crates/teksilo-widgets/src/table_view/body_pane.rs:628` (`start_drag` :685/:696) | TableView row reorder | `accept_drop` → model order | **None.** `table_view/keyboard.rs` has no reorder branch and no `keyboard_reorder` call exists anywhere under `table_view*`. | Add the `Alt+↑/↓` branch the other four data views already have. This is the one data view missing it. |
| 12 | `crates/teksilo-widgets/src/tree_table_view/body_pane.rs:695` (`start_drag` :777/:785) | TreeTableView row reorder **and reparent** | `source.keyboard_reorder` → tree shape | **Partial.** `tree_table_view.rs:1775` `Alt+↑/↓`, gated on no active sort. Reparenting: none. | Same as row 10 — `Alt+←/→` outdent / indent. |
| 13 | `crates/teksilo-widgets/src/grid_view/body_pane.rs:405` (`start_drag` :447/:449) | GridView tile reorder | `accept_drop_fn` → model order | **Yes.** `grid_view/keyboard.rs:159` — `Alt+arrows` (±1 and ±`cols`) expressed as a same-view drop. | **Compliant.** |
| 14 | `crates/teksilo-widgets/src/data_views.rs:1030` (`on_drag_ended` :1038) | **Cross-widget / OS row export** from all five views (`.exportable`, `.export_external`) | `on_rows_transferred_out` on the source, `on_rows_received` on the target | **None.** No `access_custom_action` anywhere in the data views; the `Alt+arrow` routes are strictly in-view. | A cut/paste two-step: Ctrl+X marks the selection for transfer, Ctrl+V on the target view or `DropTarget` completes it; expose as AT custom actions "Cut rows" / "Paste rows here". Subsumed by row 31 if that lands. |
| 15 | `crates/teksilo-widgets/src/grid_view/selection.rs:102`, attached at `grid_view.rs:1280` | GridView marquee (rubber-band) selection | `SelectionModel::select_indices(hits, additive)` | **Yes.** `grid_view/keyboard.rs:140` Ctrl/⌘+A = select all (Ctrl+Shift+A = clear); :312/:414 Shift+arrows extend the range. | **Compliant** — every selection the marquee can produce is reachable. |
| 16 | `crates/teksilo-widgets/src/tab_widget/header.rs:882` (`start_drag_with_preview` :894) | Tab reorder within a bar | `on_reorder(from, to)` | **Partial — AT only.** `on_access_action_request` :820 handles `CustomAction` 0 = Move Left/Up, 1 = Move Right/Down, advertised :1069-1105. **No keyboard chord** — `on_key` :743-816 covers arrows/Home/End/Delete/Enter/Space only. | Bind Ctrl+Shift+←/→ (or Alt+←/→) on a focused tab header to the same `on_reorder_to`; optionally add both moves to `TabInfo::context_menu`. |
| 17 | `crates/teksilo-widgets/src/tab_widget/bar.rs:1880-1990` (`on_drop` / `on_tab_received`) | Cross-bar tab transfer (`accept_external_tabs`) | `on_tab_received(item, to_index)`; `on_transfer_out` on the source | **None.** The two custom actions only move within one bar. | AT custom action "Move to other group" + a tab context-menu "Move to ▸ \<bar\>", mirroring `docking/context_menu.rs`'s `move_to_submenu`. |
| 18 | `crates/teksilo-widgets/src/accordion.rs:568` → `crates/teksilo-widgets/src/docking/panel.rs:863` | Drag a dock out of a split pane | `promote_to_tab` / `move_dock` / `split_into_tab` / `stack_into_tab` | **Partial (menu).** The `⋮` options menu (`docking/context_menu.rs:152`) offers "Move to new activity" (:167) and "Move to side ▸" (:176), keyboard-reachable because the button is focusable. The drag itself is gated on `DockPolicy::allow_dock_drag`. | Mostly covered; the missing placements are row 19. |
| 19 | `crates/teksilo-widgets/src/docking/panel.rs:1061` (5 zones declared :1054-1058) | **Drag-to-dock: split or stack into a specific pane position** | `stack_into_tab` (centre), `split_into_tab(before/after)` (edges) | **None.** Neither `split_into_tab` nor `stack_into_tab` appears in `docking/context_menu.rs`. | Extend `dock_options_menu` with "Split ▸ Left / Right / Above / Below" and "Group with ▸ \<other dock in this activity\>". |
| 20 | `crates/teksilo-widgets/src/docking/panel.rs:305` and `:637` | Drop a tab or dock onto a whole side | `move_tab(tab, side, at)` / `move_dock(dock, side)` | **Yes (menu).** `activity_context_menu` "Move to ▸ \<enabled side\>" (`context_menu.rs:102`, `move_to_submenu` :278), reachable by Menu key / Shift+F10 on the focusable rail item or tab. | **Compliant.** |
| 21 | `crates/teksilo-widgets/src/docking/activity_bar.rs:2125` (drop at :808) | Activity-rail **reorder**, and rail → rail / strip transfer | `move_tab(tab, side, visible_pos)` / `promote_to_tab` | **Transfer: yes** (rail item `.focusable(true)` :2146 + `.context_menu(activity_context_menu)` :2138). **Reorder within the rail: none** — the menu has Hide / Move to / checklist / size, no move-up-down. | Add "Move up / Move down" to `activity_context_menu`, and `Alt+↑/↓` in the rail's `on_key` (:2082) using the same visible-index mapping the drop handler uses. |
| 22 | `crates/teksilo-widgets/src/tool_box.rs:888` (`on_header_drag`) | ToolBox section header drag (app-defined hook) | whatever the app's `on_header_drag(idx, ctx)` does | **None from the framework.** The header has `on_key` :787 and `Action::Click/Expand/Collapse` :942-946, but nothing invokes the drag hook. Unused inside the workspace today. | Ship a paired `on_header_move(idx, dir)` hook the widget calls from `Alt+↑/↓` and an AT custom action, so every consumer inherits an alternative. |
| 23 | `crates/teksilo-widgets/src/rich_text/mouse.rs:279` (advance :474, commit :583) | Resize an embedded image by its corner grip | `on_image_resized(ImageResize{..})` → the `text-document` model | **None.** No image keys in `rich_text/keyboard.rs`; `rich_text/context_menu.rs` has no image entries. | Context menu "Image size ▸ Larger / Smaller / Original / Custom…" on the selected image, plus Ctrl+Alt+←/→ while `selected_image` is set; advertise both as AT custom actions. |
| 24 | `crates/teksilo-widgets/src/rich_text/mouse.rs:175` (armed via `PendingTextDrag` :460) | Drag selected text (move / copy) | clipboard-equivalent payload; document mutation on drop | **Yes.** Ctrl+X / C / V and Ctrl+Shift+V in `rich_text/keyboard.rs:296-303` reach the same outcome. | **Compliant.** |
| 25 | `crates/teksilo-scene/src/view/gestures_impl.rs:868` (hit branch ~:955) | SceneView item drag-to-move | `SceneModel::set_local_pos` for every selected item | **Yes.** `on_key` :689 → :717 `Alt+arrows` nudges the selection (Shift = ×10); the comment at :711 cites 2.5.7. | **Compliant.** (v1 ignores view rotation.) |
| 26 | same handler, `:967` (commit :1145-1161) | SceneView marquee selection | `SceneSelection` | **None.** No select-all, no Shift/Ctrl+arrow extend, and the synthetic `SyntheticKind::SceneItem` nodes advertise no `Action::Click` / `Focus` (`view/a11y_impl.rs`). Lightweight items are not focusable widgets. | Ctrl+A = select all in the scene; Tab / Shift+Tab rove a focus ring over items with Space = toggle-select and Shift+Space = extend; advertise `Click` + `Focus` on the synthetic nodes. |
| 27 | same handler, `:929-943` (port drag) | Magnetism: connect two magnets by dragging a wire | the consumer's `on_connect(MagnetConnection)` | **Yes.** `view/magnetism.rs:172` — the `connect_key` (default `m`) enters connect mode, arrows/Home/End move the target, Enter/Space confirms, Esc cancels; synthetic `SceneMagnet` AT nodes with roving `active_descendant`. | **Compliant.** |
| 28 | `crates/teksilo-widgets/src/drop_zone.rs:471` (hover :442) | Drop OS files onto a DropZone | `on_files_dropped` / `on_text_dropped` / `on_urls_dropped` | **Yes.** The built-in **Browse…** button (:371-414, `show_browse_button` defaults true at :105) opens the native file dialog and calls the same `on_files` callback. | **Compliant** — but the alternative disappears if an app calls `.show_browse_button(false)`; P39 should warn or refuse. |
| 29 | `crates/teksilo-widgets/src/drop_target.rs:695` (`on_region_drop` :708) | Drop onto a wrapping DropTarget, including its five-zone form | the app's `on_drop` / `on_region_drop(region, ..)` | **None.** `accessibility()` :759 sets only `Role::Group`; no keys, no actions, no menu. It is a generic container, so the alternative has to be framework-supplied or app-side. | An optional `.on_paste_here(..)` plus one AT custom action per declared region ("Drop here", "Drop above", …), and a documented mark-source-then-activate-target pattern. Subsumed by row 31. |
| 30 | `crates/teksilo-core/src/widget_tree/drag_drop_impl.rs` (`start_drag` / `start_drag_with_preview`; `begin_external_drag` :138) | **The generic drag-and-drop pipeline itself** | `DragSession` + the target's `on_drop` | **Escape cancels only.** There is no keyboard pick-up / put-down mode anywhere in core. | A framework-level keyboard DnD mode: a "lift" command parks the payload, Tab moves to a target, Enter drops, Esc cancels. **Highest-leverage single fix** — it retires rows 14, 17, 19 and 29. |
| 31 | `crates/teksilo-widgets/src/title_bar/drag_region.rs:130` | **Move the window** (custom chrome) | `PlatformTitleBarHost::begin_drag()` | **None.** `accessibility()` :215 explicitly `set_hidden()` with the comment "no keyboard or AT analogue". The fallback window menu (`title_bar/window_menu.rs:61-85`) has Restore / Maximize / Minimize / Close — **no Move**. `on_double_tap` :139 toggles maximize, which is a non-drag route for maximize only. | Add **Move** to `build_window_menu`, driving a keyboard move mode (arrows nudge, Enter commits, Esc reverts) — the Win32 system-menu convention. |
| 32 | `crates/teksilo-widgets/src/title_bar/resize_strip.rs:115` | **Resize the window** (custom chrome) | `PlatformTitleBarHost::begin_resize(edge)` | **None.** No keys, no AT node, and the fallback window menu has no **Size** entry. | Add **Size** to `build_window_menu` (pick an edge, then arrows), plus a `WindowState`-driven programmatic resize the menu can call. |
| 33 | `crates/teksilo-widgets/src/scroll_bar.rs:393` | Drag the scrollbar thumb | `set_scroll` → the owner's scroll `Signal<f32>` | **Yes.** Track click pages (`on_tap` :434) — a single-pointer route to any position; wheel via `ScrollArea::on_scroll` :647; `ScrollArea` advertises `Action::ScrollUp/Down/Left/Right` (`scroll_area.rs:1188-1199`, handled :793-811). **Caveat:** the bar's own `on_key` :477 is dead code — the bar is `.focusable(false)` :372, `accessibility()` :582 calls `set_hidden()`, and it is a *sibling* of the content, so KeyDown never reaches it. | **Compliant** via track click + AT. Cleanup: delete the dead `on_key` or move those chords onto `ScrollArea`. |
| 34 | `crates/teksilo-widgets/src/primitives/text_input_field/mouse.rs:53` | Drag-select text in TextInput / SearchField / SpinBox / PasswordField | `cursor.set_position(.., KeepAnchor)` | **Yes.** `text_input_field/keyboard.rs:81` Shift+arrow extend, Ctrl/⌘+A select-all; double/triple tap = word/line. | **Compliant.** |
| 35 | `crates/teksilo-widgets/src/rich_text/mouse.rs:442` | Drag-select text in RichTextEditor / Viewer | `cursor.set_position(.., KeepAnchor)` | **Yes.** `rich_text/keyboard.rs:130-223` Shift+arrows including table-cell extension; the Ctrl+A ladder :164-279. | **Compliant.** |
| 36 | `crates/teksilo-widgets/src/code_editor/mouse.rs:110` | Drag-select (and Alt-click multi-caret drag) in CodeEditor / LogView | `DragState::Selecting` → selection ranges | **Yes.** `code_editor/keyboard.rs` carries the full Shift+arrow / Ctrl+A set. | **Compliant.** |
| 37 | `crates/teksilo-terminal/src/terminal.rs:1229` | Drag-select terminal text | `engine.selection_update(row, col, side)` | **Partial.** `on_double_tap` :660 / `on_triple_tap` :667 give word and line selection without a drag. `TerminalController::select_all()` exists (:194) but **no key is bound to it**; the only chords are Ctrl+Shift+C :1452 and Ctrl+Shift+V :1461. No Shift+arrow selection. | Bind Ctrl+Shift+A → `select_all()`, and add Shift+arrow / Shift+Home/End range selection over the grid (the terminal has no caret, so this needs a scrollback selection cursor). |

## Recorded and dismissed

| Site | Why it carries no 2.5.7 obligation |
| --- | --- |
| Drag auto-scroll ticks — `list_view.rs:1608`, `tree_view/widget_impl.rs:947`, `table_view.rs:2073`, `tree_table_view.rs:2105`, `tab_widget/bar.rs:2060`, `grid_view.rs` | not an operation, just edge scrolling during an already-started drag |
| `crates/teksilo-scene/src/view/gestures_impl.rs:591` pinch-zoom and `:877` hand-drag panning | keyboard equivalents in the same `on_key` (:804-807 pan, zoom clamps :747+) |
| `crates/teksilo-scene/src/minimap.rs:242` | `on_tap` only — not a drag |
| `crates/teksilo-widgets/src/primitives/text_widget.rs:320` `PointerMove` | inline-link hover cursor, not a drag |

`teksilo-inspector`, `teksilo-preview-ui`, `teksilo-webview` and `teksilo-charts`
contain no drag operations at all.

## Cross-references

- Grab geometry for every handle above is in
  [the density inventory](density-inventory.md) (Grab class).
- The hover-gated affordances that guard some of these handles — the Splitter
  handle's dwell reveal in particular — are in
  [the hover census](hover-affordance-census.md).
