<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Grid view

Examples: `grid-view`.
17 findings: 1 critical, 5 high, 5 medium, 6 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [gridview-01](#gridview-01) | grid-view | The selection-count announcement (and the grid's value) is the Fluent message id 'grid-view-selection-count', not '1 item selected' | high | Linux | fixed |
| [gridview-02](#gridview-02) | grid-view | Every selection change rebuilds all realized tiles, so the focused tile comes back as a new node: a spurious focus event follows each count announcement and cuts it, and no 'selected' state change ever reaches the bus | high | Linux | fixed |
| [gridview-03](#gridview-03) | grid-view | A screen reader putting focus on a tile splits AT focus from the grid's cursor: Enter opens a different tile, and the next arrow drops keyboard focus out of the grid onto the window | critical | Linux | fixed |
| [gridview-04](#gridview-04) | grid-view | The tile's context menu (the keyboard reorder menu) is silent to a reader: 'menu.', then nothing on arrows, and Enter runs a row the reader never heard | high | Linux | fixed |
| [gridview-05](#gridview-05) | grid-view | After a reorder the move announcement names the wrong tile, and type-ahead lands on the wrong tile, because the tile names come from a stale by-index caption snapshot | high | Linux | open (example) |
| [gridview-06](#gridview-06) | grid-view | Type-ahead with a growing prefix skips the tile it is already on, so typing a word lands past the first match | medium | Linux | open |
| [gridview-07](#gridview-07) | grid-view | The album (section) a tile belongs to is never spoken, and in the tree its row header comes after all the tiles; sections whose tiles are realized have no header node | medium | Linux | open |
| [gridview-08](#gridview-08) | grid-view | A tile's selected / not-selected state is never spoken on Linux, and the grid's AT-SPI Selection always reports 0 selected children | high | Linux | upstream |
| [gridview-09](#gridview-09) | grid-view | The grid is a layout table to Orca: no role, no size, no row/column or 'N of 60' for a tile | medium | Linux | upstream |
| [gridview-10](#gridview-10) | grid-view | Alt+arrow reorder makes two announcements (the move and a count), and the move is emitted before the focus change to the moved tile, which cuts it | medium | Linux | partly fixed |
| [gridview-11](#gridview-11) | grid-view | The tiles' Move custom actions (a documented non-drag reorder route) reach no screen reader: AT-SPI offers only 'click' | low | Linux | upstream |
| [gridview-12](#gridview-12) | grid-view | Tiles are unnamed cells: the launch audit flags 55 focusable table cells with no name | low | Linux | open |
| [gridview-13](#gridview-13) | grid-view | The reorder announcement is English-only (lit!, not translatable) | low | all | open |
| [gridview-v1](#gridview-v1) | grid-view | Alt+arrow reorder collapses a multi-selection to the moved tile, and the reader is not told | medium | Linux | open |
| [gridview-v2](#gridview-v2) | grid-view | Type-ahead with no cursor yet skips the first tile | low | Linux | open |
| [gridview-v3](#gridview-v3) | grid-view | Alt+arrow with no cursor moves the first tile, which the reader never heard | low | Linux | open |
| [gridview-v4](#gridview-v4) | grid-view | setsize=60 is published on every descendant of the grid (labels, the body pane, row headers, the pinned label) | low | Linux | upstream |

### gridview-01 {#gridview-01}

The selection-count announcement (and the grid's value) is the Fluent message id 'grid-view-selection-count', not '1 item selected'

- **Example:** grid-view
- **Act:** Any key that changes how many tiles are selected: Right from no cursor, Space, Ctrl+Space, Shift+Right, Ctrl+A, Ctrl+Shift+A, type-ahead, Alt+Right on an unselected tile
- **The reader should get:** The reader hears the new count in words: '1 item selected', 'No item selected', '60 items selected' (en-US.ftl:439-444)
- **The reader gets:** The announcement text on the bus is the message id 'grid-view-selection-count' for every count; the grid's AccessKit value (grid\_view.rs:1970) carries the same id. On main K2 also drops it, so Orca says nothing; with K2 fixed Orca would read out 'grid-view-selection-count'.
- **Platform:** Linux AT-SPI/Orca measured. Windows by source: the grid has a value and is not a Label, so accesskit\_windows exposes the UIA Value pattern (node.rs:592-597) and a UIA client reading the grid's value gets the raw id.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `8448bb9a` (gridview). Fixed part: the selection count (and the grid's value) is now said in English words with no I18nManager installed ('1 item selected', 'No item selected', '60 items selected'), not the message id. Root cause fixed in the tr!/tr\_widget!/tr\_signal! macro fallback, so every message with a selector, plural, function call or reference is covered (command-palette-result-count too)..
- **Where:** crates/teksilo-i18n-macros/src/lib.rs:479-519 and 824-866 (no fallback for selector messages); crates/teksilo-i18n/src/resolve.rs:22-24; crates/teksilo-widgets/src/grid\_view/selection\_count.rs:31-36
- **Evidence:**
  - `gridview-arrows-20260925-135433-4064709/report.txt: "+83.6 ms object:announcement [<Error>] '' text='grid-view-selection-count'"`
  - `gridview-reorder-20260925-135823-4109511/orca-debug.out: "13:58:39.670287 - EVENT MANAGER: object:announcement for [status bar: 'grid-view-selection-count'] in [application: 'grid-view'] (1, 0, grid-view-selection-count)"`
  - `Every one of the 59 count announcements in 31 runs carried text='grid-view-selection-count' (e.g. gridview-selection-20260925-141416-213061, all 7 selection-changing acts)`
  - `crates/teksilo-widgets/src/grid_view/selection_count.rs:31-36 builds the text with tr_widget!(grid_view_selection_count(count)).resolve_now()`
  - `crates/teksilo-i18n/src/resolve.rs:22-24: resolve_message_widget(...) = with_active(...).unwrap_or_else(|| key.to_string()), so with no I18nManager the framework's own string is its id`
  - `crates/teksilo-app/src/app.rs:3716-3721: without .i18n(config), tr!-expanded code falls back to the literal key; examples/grid_view/src/main.rs installs no I18nConfig`
  - `Goes through ctx.announce (selection_count.rs:81), so K2 also applies; the K2 fix does not change the text`
  - `gridview-selection-20260925-142651-464380/run.json 'Right: the first tile, selected': bus 14:27:03.629773 object:announcement [<Error>] '' text='grid-view-selection-count'`
  - `tree after 'Space: select it again' (same run): table 'Photo library' interfaces ['Accessible','Component','Selection'], no Value/Text, selected_children 0`
  - `awk over crates/teksilo-widgets/locales/en-US.ftl: only command-palette-result-count and grid-view-selection-count contain a selector`
  - `Fix: in the macro, fall back to the selector's default (*) variant (or format with a built-in en-US bundle) instead of the key`
- **Reproduced:** deterministic; 59/59 count announcements across 31 runs
- **Verification:** corrected by the verifier. Reproduced: deterministic; 68 of 68 count announcements in 35 verifier runs The finding is real and deterministic: 68 of 68 count announcements in my 35 runs carried text='grid-view-selection-count'. The cause is narrower than the sweep says. It is not true that with no manager every framework string is its id. The tr\_widget!/tr! macro builds a compile-time English fallback for any message made of literal text and plain {$var} (crates/teksilo-i18n-macros/src/lib.rs:283-291, 824-866). It builds none when a message has a selector, plural or function call (lib.rs:479-519, `fallback_ok = false`). Such a message falls through to resolve\_message\_widget, and with no manager that returns the key (crates/teksilo-i18n/src/resolve.rs:22-24). grid-view-selection-count is a plural selector (en-US.ftl:439-444). Only one other widget message has a selector: command-palette-result-count. So the fault is in the macro's missing fallback for selector messages, not in the example leaving out I18nConfig (examples are allowed to skip it). Platform: on Linux the grid's value (grid\_view.rs:1970) is not exposed at all. The table's AT-SPI interfaces are only Accessible, Component and Selection, so the id reaches a Linux reader only through the announcement. On Windows, by source, the Value pattern is supported because has\_value && !label\_comes\_from\_value (accesskit\_windows node.rs:591-596), so the raw id is the grid's UIA value. On main, K2 drops every one of these messages. With K2 fixed, the reader would hear the id (and see gridview-02). The message goes through ctx.announce (selection\_count.rs:81), so the K2 fix covers delivery but not the text.
- **Fix idea:** Give framework-owned strings (tr\_widget!) a built-in en-US fallback bundle when no I18nManager is installed, instead of returning the id; or have TeksiloAppBuilder always install a framework-only manager.

### gridview-02 {#gridview-02}

Every selection change rebuilds all realized tiles, so the focused tile comes back as a new node: a spurious focus event follows each count announcement and cuts it, and no 'selected' state change ever reaches the bus

- **Example:** grid-view
- **Act:** Space / Ctrl+Space on the current tile, Ctrl+A, Ctrl+Shift+A (cursor does not move); also every arrow that replaces the selection
- **The reader should get:** Focus stays put. The tile's selected state changes in place (object:state-changed:selected) and the count announcement is heard in full
- **The reader gets:** Each selection-changing key removes and re-adds every realized tile and row header: 237 events per key (57 add, 57 remove, 115 defunct), against 2 for a cursor-only Ctrl+Right. The adapter then reports focus on a new table-cell node, and Orca stops speech and reads the tile's name again ('Sunset 1.' after Space). In 59 of 63 announcements across 31 runs, the announcement reached the bus 0.0-100.5 ms before that focus change in the same act (the other 4 are the second message of an Alt+Right, gridview-10). Where K2 did not drop the message, Orca spoke it and then cut it for the refocus. So the K2 fix alone will not make the count audible. There were no object:state-changed:selected or object:selection-changed events in any run, so a toggle is never heard as a state change.
- **Platform:** Linux AT-SPI/Orca measured. The same rebuild gives every adapter a new focused node (the consumer hands node changes before the focus event, tree.rs:640-673). NVDA's handling was not verified.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `8448bb9a` (gridview). Fixed part: a change of selection no longer rebuilds every realized tile. The tile under the cursor keeps its node, so there is no spurious focus event after the count and Orca speaks the count in full. object:state-changed:selected now reaches the bus. A Space is 11-13 AT-SPI events, down from 237..
- **Where:** crates/teksilo-widgets/src/grid\_view/body\_pane.rs:252-259 (selection effect -&gt; Rebuild)
- **Evidence:**
  - `gridview-selection-20260925-135520-4074007/report.txt, 'Space: unselect it': "+89.0 ms object:announcement [<Error>] '' text='grid-view-selection-count'" / "+89.8 ms object:state-changed:focused 1 [table cell] ''" / "+234.5 ms ORCA SAYS: 'Sunset 1.'" / "FAIL  no object:state-changed:focused event from [*] '*'"`
  - `same act, orca-debug.out: "13:55:36.628408 EVENT MANAGER: Ignoring defunct object: [DEAD]" then "13:55:36.707590 NULL SPEECH: stop" then "13:55:36.707667 SPEECH OUTPUT: 'Sunset 1.'"`
  - `gridview-reorder-20260925-141204-172548/orca-debug.out (a message K2 did not drop): "14:12:20.483247 - NULL SPEECH: speak 'Sunset 1 moved to 2 of 60' interrupt=True" / "14:12:20.634221 - NULL SPEECH: stop" / "14:12:20.634292 - SPEECH OUTPUT: 'Sunset 1.'"`
  - `per-act event counts, gridview-selection-20260925-135520: Space total=237 add=57 rem=57 defunct=115; Ctrl+Right total=2`
  - `0 events of type *selected*/*selection* in events.jsonl across all 31 runs`
  - ``crates/teksilo-widgets/src/grid_view/body_pane.rs:252-259: ctx.effect(&sel.selection_signal(), ...) bumps `version`, bound at BindingLevel::Rebuild (body_pane.rs:209)``
  - `crates/teksilo-widgets/src/grid_view/selection_count.rs:81 announces inside the key handler; the rebuild and its focus event come later`
  - `gridview-selection-20260925-142651-464380 'Space: unselect it': bus 14:27:07.465526 object:announcement text='grid-view-selection-count'; 14:27:07.502714 object:state-changed:focused 1 [table cell]; Orca 14:27:07.580661 EVENT MANAGER: Ignoring defunct object: [DEAD]; 14:27:07.632804 NULL SPEECH: stop; 14:27:07.632855 SPEECH OUTPUT: 'Sunset 1.'`
  - `gridview-reorder-20260925-144014-724775/orca-debug.out: 14:40:30.459669 NULL SPEECH: speak 'Sunset 1 moved to 2 of 60' interrupt=True / 14:40:30.608309 NULL SPEECH: stop / 14:40:30.608366 SPEECH OUTPUT: 'Sunset 1.'`
  - `verify-gridview-reorder-multi-20260925-143848-724775/orca-debug.out: 14:39:26.799500 NULL SPEECH: speak 'Trail 3 moved to 4 of 60' interrupt=True / 14:39:26.976787 NULL SPEECH: stop / 14:39:26.976841 SPEECH OUTPUT: 'Trail 3.'`
  - `/usr/lib/python3/dist-packages/orca/scripts/default.py:698-702 presentationInterrupt on a locus-of-focus change`
- **Reproduced:** 59 of 63 announcements (31 runs) preceded a focus change in the same act; refocus with no cursor move on Space/Ctrl+Space/Ctrl+A/Ctrl+Shift+A: 3 of 3 selection runs and 3 of 3 repeat-count runs; spoken-then-cut observed 1 of 4 reorder runs (dropped by K2 in the other 3)
- **Verification:** confirmed. Reproduced: refocus with the count ahead of it: 28 of 28 no-move acts in 8 runs; spoken-then-cut: 2 of 2 announcer messages Orca did speak in my runs (move messages; count messages were all K2-dropped)
- **Fix idea:** Rebind selection at AccessibilityOnly/Repaint and update each realized tile's `selected` (and the delegate's is\_selected paint) in place rather than rebuilding the pane, so node ids survive; or at least keep tile ids stable across rebuilds (key by index).

### gridview-03 {#gridview-03}

A screen reader putting focus on a tile splits AT focus from the grid's cursor: Enter opens a different tile, and the next arrow drops keyboard focus out of the grid onto the window

- **Example:** grid-view
- **Act:** Right (cursor on 'Sunset 1'), then AT-SPI grab\_focus on the 'Trail 3' cell (the tile advertises Action::Focus), then Enter, Right, Right
- **The reader should get:** Focus and the grid's cursor move to 'Trail 3'. Enter opens 'Trail 3', Right goes to 'Picnic 4', then 'Summit 5'
- **The reader gets:** Orca says 'Trail 3.', but Teksilo's focus lands on the TileA11y node itself and the grid's focused\_index stays 0. Enter activates tile 0: the example printed 'activate tile 0'. The next Right moves the grid's hidden cursor and changes the selection. The pane rebuild then destroys the focused tile node, and focus falls to the window: Orca says 'frame.'. The following Right produces nothing, because keyboard focus is no longer in the grid.
- **Platform:** Linux AT-SPI measured (grab\_focus). By source the same path is reached from macOS: setAccessibilityFocused -&gt; Action::Focus (accesskit\_macos node.rs:663-675), which VoiceOver sends when 'keyboard focus follows the VoiceOver cursor' is on (the default). Also from Windows: UIA SetFocus -&gt; Action::Focus (accesskit\_windows node.rs:1129-1131), which NVDA's move-focus-to-navigator-object command calls.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `8448bb9a` (gridview). Fixed part: a screen reader's focus request on a tile no longer splits AT focus from the grid's cursor. Enter and Space no longer act on a tile the reader did not choose, and focus no longer falls to the window. Done the house way (the tile does not offer Focus, like ListItemWrapper and calendar days), not by moving the cursor; see not\_fixed..
- **Where:** crates/teksilo-widgets/src/grid\_view/a11y.rs:94; crates/teksilo-widgets/src/grid\_view/body\_pane.rs:513-545; crates/teksilo-core/src/widget\_tree/pointer\_router.rs:1187-1201
- **Evidence:**
  - `gridview-at-focus-20260925-141557-241986/report.txt, 'AT-SPI grab_focus on Trail 3': "+12.0 ms object:state-changed:focused 1 [table cell] ''" then ORCA SAYS: 'Trail 3.' (quoted from run 140542-34278)`
  - `'Enter on the focused tile': "FAIL  the example activated tile 2 ('Trail 3')" / "the example printed ['activate tile 0']" (app.log 'activate tile 0' in 3/3 runs)`
  - `'Right from Trail 3': "+88.1 ms object:state-changed:focused 1 [frame] ''" / "ORCA SAYS: 'frame.'" / "14:16:23.118571 EVENT MANAGER: Ignoring defunct object: [table cell]"`
  - `'Right again': "no focus change on the bus in this act", Orca said nothing`
  - `crates/teksilo-widgets/src/grid_view/a11y.rs:94 TileA11y adds Action::Focus`
  - `crates/teksilo-core/src/widget_tree/pointer_router.rs:1187-1201: advertises_focus_action -> first_focusable_descendant(id).unwrap_or(id) -> focus_with_origin_ops(tile), which focuses the non-focusable tile node`
  - `crates/teksilo-widgets/src/grid_view/body_pane.rs:513-545: the tile's on_access_action handles ScrollIntoView and Click (which does set focused_index) but not Focus`
  - `crates/teksilo-widgets/src/grid_view/keyboard.rs:113-121: every key acts on cfg.focused_index, not on the focused node`
  - `gridview-at-focus-20260925-142830-464380, -143509-624211, -144153-724775: app.log 'activate tile 0'; 'Right from Trail 3' focus -> [frame], Orca 'frame.'; 'Right again' no event`
  - `verify-gridview-at-focus-keys-20260925-143527-623194 'Space on the focused tile': 14:35:47.846014 object:property-change:accessible-name [label] '0 selected'; 14:35:47.846670 object:state-changed:focused 1 [frame]; ORCA 14:35:47.986646 SPEECH OUTPUT: 'frame.'`
  - `same run 'Tab': focused 1 [table cell], Orca 'Sunset 1.'`
- **Reproduced:** 3 of 3 runs (Enter on tile 0, focus to \[frame\], dead keys)
- **Verification:** confirmed. Reproduced: 3 of 3 (gridview-at-focus: Enter opens tile 0, focus goes to the frame, keys go dead) + 2 of 2 (verify-gridview-at-focus-keys: Space toggles tile 0, focus goes to the frame, Tab recovers)
- **Fix idea:** Handle Action::Focus on the tile the way Click is handled (set focused\_index, focus the grid, which then publishes active\_descendant), or stop TileA11y from advertising Focus and let the router focus the grid.

### gridview-04 {#gridview-04}

The tile's context menu (the keyboard reorder menu) is silent to a reader: 'menu.', then nothing on arrows, and Enter runs a row the reader never heard

- **Example:** grid-view
- **Act:** Ctrl+Right twice (cursor on 'Harbor 2'), Menu key, Down, Enter
- **The reader should get:** The menu is announced with its current row ('Move Left'). Down moves to and speaks 'Move Right'. Enter runs the row the reader heard
- **The reader gets:** Focus goes to an unnamed \[menu\] and Orca says 'menu.'. No row is ever focused and the menu sets no active descendant, so Down makes no event and Orca says nothing. Enter runs 'Move Left' (the first Down highlighted it visually): 'Harbor 2 moved to 1 of 60'. The rows exist on the bus but none carries focused/selected state. This menu is one of the grid's documented non-drag reorder routes (grid\_view.rs:1313-1318).
- **Platform:** Linux AT-SPI/Orca measured. By source MenuList publishes only Role::Menu on every platform, with no active\_descendant, so no adapter can name the current row.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `9636094c` (menus).
- **Where:** crates/teksilo-widgets/src/menu\_list.rs:228-231, 991-993
- **Evidence:**
  - `gridview-context-menu-20260925-141641-250092/report.txt: "+18.0 ms object:state-changed:focused 1 [menu] ''" / "+107.6 ms ORCA SAYS: 'menu.'"`
  - `tree after Down: "menu rows: [('Move Left', ['enabled', 'sensitive', 'showing', 'visible']), ('Move Right', ['enabled', 'sensitive', 'showing', 'visible']), ('Move to Start', ['enabled', 'sensitive', 'showing', 'visible']), ('Move to End', ['enabled', 'sensitive', 'showing', 'visible'])]" / "menus: [('', ['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible'])]"`
  - `Down act: no events, Orca said nothing`
  - `Enter act: "+62.8 ms object:announcement [<Error>] '' text='Harbor 2 moved to 1 of 60'" (Move Left ran)`
  - `crates/teksilo-widgets/src/menu_list.rs:229-236: arrow/Home/End/type-ahead move focused_index, not real focus, which 'stays on the panel'`
  - `crates/teksilo-widgets/src/menu_list.rs:991-993: accessibility() sets Role::Menu only (no name, no active_descendant)`
  - `This is MenuList-wide, so every MenuList-based context menu is affected, not just the grid's`
  - `gridview-context-menu-20260925-142913-464380: 14:29:44.216487 object:state-changed:focused 1 [menu] ''; ORCA 14:29:44.253654 SPEECH OUTPUT: 'menu.'; Down act: no events; Enter: 14:29:52.558011 object:announcement text='Harbor 2 moved to 1 of 60'`
  - `verify-gridview-menu-escape-20260925-143610-623194: Shift+F10 -> focused 1 [menu], 'menu.'; Escape -> focused 1 [table cell], 'Harbor 2.'`
- **Reproduced:** 4 of 4 runs
- **Verification:** confirmed. Reproduced: 3 of 3 gridview-context-menu runs + 1 verify-gridview-menu-escape run (Menu key and Shift+F10)
- **Fix idea:** Publish MenuList's focused\_index as active\_descendant on the Menu node, pointing at the row's MenuItem (or move real focus to the row), and name the menu.

### gridview-05 {#gridview-05}

After a reorder the move announcement names the wrong tile, and type-ahead lands on the wrong tile, because the tile names come from a stale by-index caption snapshot

- **Example:** grid-view
- **Act:** Ctrl+Right, Alt+Right (Sunset 1 to position 2), Alt+Down (Sunset 1 to position 7); later Home, type 's'
- **The reader should get:** 'Sunset 1 moved to 7 of 60'. Typing 's' from 'Harbor 2' (now first) reaches 'Summit 5' (now fourth)
- **The reader gets:** The bus carries 'Harbor 2 moved to 7 of 60' although Sunset 1 moved. Typing 's' lands on 'Garden 6': the stale label for index 4 is 'Summit 5', but the tile now at index 4 is Garden 6. On main K2 hides the wrong announcement; with K2 fixed the reader would hear the wrong name.
- **Platform:** Linux AT-SPI/Orca measured; the announced string is the same on all platforms
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/grid\_view/src/main.rs:77,105; contributing crates/teksilo-widgets/src/grid\_view.rs:1345-1352
- **Evidence:**
  - `gridview-reorder-20260925-141511-230793/report.txt, Alt+Down: "object:announcement [status bar] 'grid-view-selection-count' text='Harbor 2 moved to 7 of 60'" and "FAIL  the bus carries an announcement of 'Sunset 1 moved to 7 of 60'"`
  - `type 's' act: "ORCA SAYS: 'Garden 6.'" / "FAIL  Orca says 'Summit 5'"`
  - ``examples/grid_view/src/main.rs:77,105: `.type_ahead_label(move |i| cap_for_type.get(i)...)` over `captions`, a Vec snapshotted at startup and never reordered``
  - `` API contributing: crates/teksilo-widgets/src/grid_view.rs:1345-1352 names the moved tile with `(with_item_str)(index, &|_item: &T| label(index))`, which throws away the item it holds; grid_view.rs:1420 feeds the same index-keyed closure to type-ahead; common/ordered_move.rs:388 `let name = (self.name)(from)` ``
  - `The move announcement goes through ctx.announce (grid_view.rs:1385), so K2 applies`
  - `gridview-reorder-20260925-142745-464380 'Alt+Down': 14:28:05.518806 object:announcement text='Harbor 2 moved to 7 of 60'; 'type s': ORCA 14:28:17.392584 SPEECH OUTPUT: 'Garden 6.'`
  - `gridview-reorder-20260925-143423-624211 and -144014-724775: same`
- **Reproduced:** wrong name 4 of 4 reorder runs; wrong type-ahead target 3 of 3
- **Verification:** confirmed. Reproduced: 3 of 3 reorder runs: wrong name, wrong type-ahead target
- **Fix idea:** Example: read the caption from the model (`model.with_item(i, |p| p.caption.clone())`). Framework: offer an item-keyed label (Fn(&T) -&gt; String) for type-ahead and the move name, since the grid is reorderable and already holds the item.

### gridview-06 {#gridview-06}

Type-ahead with a growing prefix skips the tile it is already on, so typing a word lands past the first match

- **Example:** grid-view
- **Act:** From 'Canyon 11' type 'cab' quickly; from 'Sunset 1' (Home) type 'can' quickly
- **The reader should get:** 'cab' reaches 'Cabin 5' (the next caption starting 'cab'). 'can' from Sunset 1 reaches 'Canyon 11'
- **The reader gets:** Each keystroke searches again from the tile after the current one, so a tile that still matches the longer prefix is skipped. 'cab' goes c-&gt;Cabin 5, ca-&gt;Canyon 7, cab-&gt;Cabin 1, and Orca says 'Cabin 1.'. 'can' goes c-&gt;Cabin 9, ca-&gt;Canyon 11, can-&gt;Canyon 7, and Orca says 'Canyon 7.'. The reader hears a tile other than the one they typed toward.
- **Platform:** Linux measured; the logic is platform-independent
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/common/type\_ahead.rs:95-101
- **Evidence:**
  - `gridview-type-ahead-20260925-141801-267545/report.txt: "== type 'cab' quickly" ... "+1218.4 ms ORCA SAYS: 'Cabin 1.'" / "FAIL  the focused cell holds 'Cabin 5'" / "focused cell holds 'Cabin 1'"`
  - `same run: "== type 'can' quickly" ... "+716.0 ms ORCA SAYS: 'Canyon 7.'"`
  - `the intermediate targets show as focus events on already-destroyed nodes: "+264.6 ms object:state-changed:focused 1 [<Error>] ''"`
  - ``crates/teksilo-widgets/src/common/type_ahead.rs:88-101: every call scans `for offset in 1..=count { let i = (current + offset) % count; ...}`, including a multi-character prefix``
  - `The same TypeAheadState is used by ListView, TreeView, TableView and TreeTableView (grep TypeAheadState)`
  - `gridview-type-ahead-20260925-143005-464380: 'type cab quickly' ORCA SPEECH OUTPUT 'Cabin 1.'; 'type can quickly' 'Canyon 7.'`
  - `verify-gridview-type-ahead-start-20260925-143401-623194 'type su quickly from Sunset 1': Orca 'Summit 1.', focused cell holds 'Summit 1'`
- **Reproduced:** 'cab' -&gt; Cabin 1 in 3 of 3 runs; 'can' from Sunset 1 -&gt; Canyon 7 in 2 of 2 runs; deterministic by source
- **Verification:** confirmed. Reproduced: 3 of 3 gridview-type-ahead runs + 2 of 2 verify-gridview-type-ahead-start ('su' -&gt; Summit 1)
- **Fix idea:** When the buffer grows past one character, start the search at `current` (offset 0), not `current + 1`.

### gridview-07 {#gridview-07}

The album (section) a tile belongs to is never spoken, and in the tree its row header comes after all the tiles; sections whose tiles are realized have no header node

- **Example:** grid-view
- **Act:** Down from 'Canyon 11' (last row of Travel) into 'Summit 1' (first row of Family); Up back
- **The reader should get:** The reader hears that they are now in the Family album (and Travel on the way back)
- **The reader gets:** Orca says only 'Summit 1.' / 'Canyon 11.'. In the tree, the Role::RowHeader nodes ('Travel', 'Family') are the last children of the body pane, after all 55 realized tiles (index\_in\_parent 55, 56). There is no header node for Work or Nature, although their tiles are realized (only headers inside the visible rect are built). No relation links a tile to its header. A stray plain \[label\] copy of the pinned header ('Travel', later 'Work' while the only row header is 'Nature') sits as the grid's last child.
- **Platform:** Linux AT-SPI/Orca measured. Orca reads row headers only through the AT-SPI Table interface, which AccessKit does not implement. accesskit\_windows implements no GridItem/TableItem patterns either (node.rs:648-667, 'TODO: tables (#29)').
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/grid\_view/body\_pane.rs:609-632
- **Evidence:**
  - `gridview-sections-20260925-141843-278097/report.txt: "== Down: into the Family album" ... "+293.7 ms ORCA SAYS: 'Summit 1.'" / "Orca unheard: 'Family'"`
  - `tree-grid-view-20260925-134800-3956180 launch tree: row header 'Travel' index_in_parent 55, 'Family' index_in_parent 56, after [table cell] posinset 1..55; then "[label] 'Travel'" directly under the table`
  - `gridview-far-jumps-20260925-135615-4081386/tree-End--the-last-tile.txt: "[row header] 'Nature'" ... "[label] 'Work'"`
  - `Orca: "AXTable: [table: 'Photo library'] is layout only: True (Doesn't support table interface.)"`
  - ``crates/teksilo-widgets/src/grid_view/body_pane.rs:609-632: headers only from strategy.headers_in_range(scroll, viewport), appended after the tiles (`ids.extend(self.header_entries...)`, line 632)``
  - `gridview-far-jumps-20260925-142738-464934 tree after End: row header 'Nature' (index 50), cells posinset 11..60, grid-child label 'Work'`
  - `tabwalk-grid-view-20260925-144119-794350/orca-debug.out 14:29:18-style line: 'AXTable: [table: 'Photo library'] is layout only: True (Doesn't support table interface.)'`
- **Reproduced:** 3 of 3 runs (section never spoken); tree order deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 sections runs; tree order deterministic
- **Fix idea:** Emit each section header before its tiles in child order (or group each section in a named Role::Group/RowGroup), realize headers for every section with realized tiles, and optionally name tiles with their section via tile\_a11y\_label. Hide the pinned duplicate from AT.

### gridview-08 {#gridview-08}

A tile's selected / not-selected state is never spoken on Linux, and the grid's AT-SPI Selection always reports 0 selected children

- **Example:** grid-view
- **Act:** Ctrl+Right onto an unselected tile; any move onto a selected tile; Escape back to the grid with 'Market 7' selected
- **The reader should get:** In a multi-selection grid the reader hears whether the tile they land on is selected, and the grid can tell how many are selected
- **The reader gets:** Orca says only the caption ('Harbor 2.'), with no 'selected'/'not selected', although the cell carries (or lacks) STATE\_SELECTED correctly on the bus. The grid's Selection interface reports 0 selected children while 'Market 7' is selected, so Orca's AXSelection also reads 0.
- **Platform:** Linux AT-SPI/Orca 46.1 measured
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/grid\_view/body\_pane.rs:808-811 (Group between grid and cells); crates/teksilo-widgets/src/grid\_view/a11y.rs:82-99
- **Evidence:**
  - `gridview-selection-20260925-141416-213061/report.txt, Ctrl+Right: ORCA SAYS: 'Harbor 2.' / "FAIL  Orca says the tile is 'not selected'" while "pass  the cell of 'Harbor 2' lacks state 'selected'"`
  - `gridview-at-actions-20260925-140503-22076 tree after AT click: table node 'selected_children': 0 while "the cell of 'Market 7' has state 'selected'"`
  - `gridview-at-actions-20260925-142149-357596/orca-debug.out: "14:22:10.630949 - AXSelection: [table: 'Photo library'] reports 0 selected children" (Escape, Market 7 selected)`
  - `Orca formatting.py:499-519: TABLE_CELL speech has no selection state. speech_generator.py _generateUnselectedCell needs a Selection-capable parent and a non-layout table; the cells' parent is the body pane Role::Group (body_pane.rs:808-811)`
  - ``accesskit_atspi_common-0.20.0 node.rs:1268-1276 counts only `items(filter)`; accesskit_consumer-0.39.0 node.rs:920-936: GridCell is not item-like, so it is never a selected child and never triggers selection-changed``
  - `gridview-selection-20260925-144059-724775 'Ctrl+Right: move the cursor only': Orca said ['Harbor 2.']; FAIL 'Orca says the tile is not selected'`
  - `gridview-at-actions-20260925-142909-464934/orca-debug.out 14:29:30.562290 - AXSelection: [table: 'Photo library'] reports 0 selected children (Market 7 selected)`
  - `accesskit_windows-0.35.0/src/node.rs:648-667 is_selection_item_pattern_supported: Role::GridCell => true`
- **Reproduced:** 3 of 3 selection runs; AXSelection 0 in every run
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 selection runs; AXSelection 0 in every run Real on Linux (3 of 3 selection runs). Ctrl+Right onto an unselected tile says only 'Harbor 2.'. The grid's AT-SPI Selection reports 0 selected children while Market 7 is selected ('AXSelection: \[table: Photo library\] reports 0 selected children' at 14:29:30.562 in the at-actions run). Two corrections. (1) The finding is Linux-only. accesskit\_windows exposes SelectionItem for Role::GridCell unconditionally (node.rs:664, `Role::GridCell => true`), so a UIA client gets IsSelected. (2) It is not purely upstream. Orca's \_generateUnselectedCell (speech\_generator.py:1062-1100) requires that the cell's parent support Selection, and the cell's parent is Teksilo's own body-pane Role::Group (body\_pane.rs:808-811), not the grid. That Group defeats Orca even if AccessKit exported tables. The count of 0 is upstream: n\_selected\_children counts only item-like children (accesskit\_atspi\_common node.rs:1268-1276), and GridCell is not item-like (accesskit\_consumer node.rs:920-936). Orca never says 'selected' for a table cell, only 'not selected' (the same function).
- **Fix idea:** Teksilo could say the state itself: add 'selected' to the tile's name or description (tile\_a11y\_label default), or model tiles with item-like roles that Orca reads selection for (ListBox/ListBoxOption). Upstream: count GridCell as item-like in AccessKit's selection containers.

### gridview-09 {#gridview-09}

The grid is a layout table to Orca: no role, no size, no row/column or 'N of 60' for a tile

- **Example:** grid-view
- **Act:** Tab onto the grid; arrow between tiles
- **The reader should get:** The reader learns this is a grid of 60 photos in 5 columns, and where the current tile is (row/column or position)
- **The reader gets:** Orca says 'Photo library.' only, with no role and no count. Per tile it says only the caption. Cells publish posinset/setsize attributes (1-based), which Orca's default script never reads. Row/column indices and the grid's row\_count/column\_count reach no AT-SPI interface.
- **Platform:** Linux AT-SPI/Orca measured. Windows by source: no GridItem/TableItem patterns in accesskit\_windows 0.35.
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/grid\_view.rs:1939-1955
- **Evidence:**
  - `tabwalk-grid-view-20260925-134840-3966959/report.txt: "+17.7 ms object:state-changed:focused 1 [table] 'Photo library'" / "+86.5 ms ORCA SAYS: 'Photo library.'"`
  - `orca-debug.out: "13:48:48.382407 - AXTable: [table: 'Photo library'] is layout only: True (Doesn't support table interface.)"`
  - `tree: "[table cell] '' {focusable,selectable} attrs={'setsize': '60', 'posinset': '1'}"`
  - `crates/teksilo-widgets/src/grid_view.rs:1939-1955 sets row/column counts and size_of_set; a11y.rs:87-91 sets row/column index, none of which the AT-SPI adapter exports`
  - `tabwalk-grid-view-20260925-144119-794350: Tab 1 focus [table] 'Photo library', Orca 'Photo library.'`
  - `orca-debug.out: 'AXTable: [table: 'Photo library'] is layout only: True (Doesn't support table interface.)'`
- **Reproduced:** every run
- **Verification:** confirmed. Reproduced: every run (tab walk, 2 arrows runs)
- **Fix idea:** Until AccessKit exports tables: give the grid a description such as '60 photos, 5 columns', and let tiles carry their position/section in their description.

### gridview-10 {#gridview-10}

Alt+arrow reorder makes two announcements (the move and a count), and the move is emitted before the focus change to the moved tile, which cuts it

- **Example:** grid-view
- **Act:** Ctrl+Right (cursor only, nothing selected), Alt+Right
- **The reader should get:** One message: 'Sunset 1 moved to 2 of 60', heard in full
- **The reader gets:** Two announcements: 'Sunset 1 moved to 2 of 60', then 'grid-view-selection-count' (the reorder selects the destination inside the key handler's count voice). The move message precedes the focus event on the moved tile by 0.1-10.6 ms. In 1 of 4 runs Orca spoke it and cut it after about 150 ms to read 'Sunset 1.'. In the other 3, K2 dropped it as defunct. The whole grid body is also replaced ('children-changed:remove \[table\] Photo library -&gt; \[panel\]') on every reorder.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `b518253f` (announce-focus). Fixed part: ordering part only: the move message now reaches the bus after the focus change to the moved tile.
- **Where:** crates/teksilo-widgets/src/grid\_view.rs:1371-1385 (select then announce before the rebuild), 1429 (voice around on\_key)
- **Evidence:**
  - `gridview-reorder-20260925-141204-172548/report.txt: "+56.5 ms object:announcement [status bar] 'grid-view-selection-count' text='Sunset 1 moved to 2 of 60'" ... "+67.0 ms object:state-changed:focused 1 [table cell] ''"`
  - `same run orca-debug.out: "14:12:20.483247 - NULL SPEECH: speak 'Sunset 1 moved to 2 of 60' interrupt=True" / "14:12:20.634221 - NULL SPEECH: stop" / "14:12:20.634292 - SPEECH OUTPUT: 'Sunset 1.'" then "Ignoring defunct object: [status bar: 'grid-view-selection-count']" for the count`
  - `gridview-reorder-20260925-135823-4109511: "+63.2 ms object:announcement ... text='Sunset 1 moved to 2 of 60'" and "+78.1 ms object:announcement ... text='grid-view-selection-count'"`
  - `crates/teksilo-widgets/src/grid_view.rs:1372-1385 (sel.select(dest) then ctx.announce(utterance)) runs inside the SelectionCountVoice wrapped around on_key (grid_view.rs:1429)`
  - `Both messages go through ctx.announce (K2)`
  - `gridview-reorder-20260925-142745-464380 Alt+Right: 14:28:01.608555 announcement 'Sunset 1 moved to 2 of 60'; 14:28:01.609507 focused 1 [table cell]; 14:28:01.615362 announcement 'grid-view-selection-count'`
  - `same run Alt+Down: 14:28:05.518806 'Harbor 2 moved to 7 of 60' only, focus 14:28:05.528049`
  - `gridview-context-menu-* Enter: one message 'Harbor 2 moved to 1 of 60', status label '0 selected' -> '1 selected', no count message`
- **Reproduced:** double announcement 4 of 4 runs; move message before the focus change 4 of 4; spoken-then-cut 1 of 4 (dropped by K2 in 3)
- **Verification:** corrected by the verifier. Reproduced: move message before the focus change 12 of 12 acts; double message 6 of 12 (only when the count changed); spoken-then-cut 2 of 2 spoken The ordering is confirmed: in 12 of 12 move acts the move message reached the bus 0.0-22.7 ms before the focus change on the moved tile. These were Alt+Right/Alt+Down in 3 reorder runs, 2 reorder-multi runs and 1 alt-no-cursor run, plus the context-menu Enter in 3 runs. Two move messages Orca did speak were cut (see 02). Correction: the reorder does not always make two announcements. The count follows only when the move changes the selection count, which happened in 6 of 12 acts: Alt+Right with the tile unselected, or a multi-selection collapsing to 1 (see missed). Alt+Down on the tile already selected alone made one message (3 of 3). The context-menu route never adds the count, although it too selects the destination (status '0 selected' -&gt; '1 selected'): its perform runs outside the key handler's SelectionCountVoice. The count, when present, comes AFTER the focus change, so the refocus does not cut it; it would talk over the tile's re-read. Both messages use ctx.announce (K2). The body pane and the pinned label are both replaced on every reorder (children-changed add/remove \[table\] 'Photo library' -&gt; \[panel\], \[label\] 'Travel').
- **Fix idea:** Suppress the count voice around a reorder (the count of selected tiles did not change in any meaningful way). Emit the move announcement after the focus has settled on the moved tile, e.g. deferred to after the rebuild.

### gridview-11 {#gridview-11}

The tiles' Move custom actions (a documented non-drag reorder route) reach no screen reader: AT-SPI offers only 'click'

- **Example:** grid-view
- **Act:** Read a tile's actions on AT-SPI
- **The reader should get:** Move Left / Move Right / Move to Start / Move to End offered as actions
- **The reader gets:** Each table cell lists actions \[{'name': 'click'}\] only. No AccessKit adapter exposes custom actions. The route grid\_view.rs describes as one of four ('the tile's AccessKit custom actions') does not exist for a reader; the Alt+arrow chord works but is announced as in gridview-02/10, and the context menu is silent (gridview-04).
- **Platform:** Linux measured. Windows and macOS by source: accesskit\_windows-0.35.0 and accesskit\_macos-0.27.0 contain no custom-action code.
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/common/ordered\_move.rs:477
- **Evidence:**
  - `tree-grid-view-20260925-134800-3956180 run.json: table cell "'actions': [{'name': 'click', 'description': '', 'key_binding': ''}]"`
  - `accesskit_atspi_common-0.20.0 node.rs:532-545: n_actions is 1 or 0 and the only name is 'click'`
  - `crates/teksilo-widgets/src/common/ordered_move.rs:468-480 installs access_custom_action per move; grid_view.rs:1313-1318 counts it as a route`
  - `gridview-sections-20260925-142828-464934 launch tree: table cell actions [{'name': 'click', 'description': '', 'key_binding': ''}]`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: deterministic (every tree)
- **Fix idea:** Don't count on custom actions until an adapter exposes them. Make sure the context menu route is readable (gridview-04).

### gridview-12 {#gridview-12}

Tiles are unnamed cells: the launch audit flags 55 focusable table cells with no name

- **Example:** grid-view
- **Act:** launch (tree audit)
- **The reader should get:** Each focusable cell carries its caption as its name
- **The reader gets:** 'unnamed-control: \[table cell\] '': a focusable table cell with no name' for every realized tile. The caption lives in a \[label\] child. Orca 46 still reads it (from the cell's displayed text), but any client asking the cell its name gets ''. accesskit\_consumer derives names from descendants only for button-like roles, so a GridCell stays unnamed unless the app supplies tile\_a11y\_label, which this example does not.
- **Platform:** Linux measured (audit); name '' on every adapter by source
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/grid\_view/a11y.rs:82-99
- **Evidence:**
  - `tree-grid-view-20260925-134800-3956180/report.txt: "unnamed-control: [table cell] '': a focusable table cell with no name" (x55)`
  - `accesskit_consumer-0.39.0 node.rs:719-735: FromDescendants only for Button/CheckBox/Link/MenuItem/RadioButton...`
  - `crates/teksilo-widgets/src/grid_view/a11y.rs:33-36, 84-86: name only when tile_a11y_label is given; examples/grid_view/src/main.rs uses no tile_a11y_label`
  - `gridview-arrows-20260925-144032-728573/report.txt: 55 x "unnamed-control: [table cell] '': a focusable table cell with no name"`
- **Reproduced:** every run
- **Verification:** confirmed. Reproduced: every run (55 per launch audit)
- **Fix idea:** Default the tile name to the type-ahead label (or the delegate's text) when tile\_a11y\_label is not set; example: pass .tile\_a11y\_label.

### gridview-13 {#gridview-13}

The reorder announcement is English-only (lit!, not translatable)

- **Example:** grid-view
- **Act:** Alt+arrow or context-menu Move
- **The reader should get:** '&lt;name&gt; moved to N of M' in the user's language
- **The reader gets:** Built with lit!(format!("{name} moved to {position} of {count}")) and 'Moved to {position} of {count}', so it is English in every locale
- **Platform:** all (source only; not run in another locale)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/common/ordered\_move.rs:102-103, 260-267, 316-323
- **Evidence:**
  - `crates/teksilo-widgets/src/common/ordered_move.rs:316-323`
  - `bus text seen: "text='Sunset 1 moved to 2 of 60'" (gridview-reorder-20260925-141204-172548)`
  - `gridview-context-menu-20260925-142913-464380 tree: menu rows 'Move Left', 'Move Right', 'Move to Start', 'Move to End'`
- **Reproduced:** source reading; the English text was seen in 4 of 4 reorder runs
- **Verification:** corrected by the verifier. Reproduced: source reading; the English strings seen in every reorder and context-menu run Real, and wider than stated. Besides move\_announcement (ordered\_move.rs:316-323), the menu-row and custom-action labels are also lit!: 'Move Left/Right/to Start/to End', 'Move Up/Down/to Top/to Bottom' (ordered\_move.rs:260-267), and 'Move Into Previous'/'Move Out One Level' (102-103). So the reorder menu is English in every locale too. Not run in another locale.
- **Fix idea:** Move the utterance to the widgets' .ftl as a tr\_widget! message with name/position/count arguments.

### gridview-v1 {#gridview-v1}

Alt+arrow reorder collapses a multi-selection to the moved tile, and the reader is not told

- **Example:** grid-view
- **Scenario:** verify-gridview-reorder-multi (tools/reader/scenarios/verify\_gridview.py)
- **Act:** Ctrl+Right, then Ctrl+Space three times on Sunset 1, Harbor 2 and Trail 3 (status '3 selected'), then Alt+Right on Trail 3
- **The reader should get:** The tile moves and the three tiles stay selected (or the selection change is said in words)
- **The reader gets:** The selection drops to the moved tile alone: the status line goes to '1 selected'. The only hint is the count message, which is the raw id (gridview-01) and is dropped by K2 on main. Orca says 'Trail 3 moved to 4 of 60' in 1 of 2 runs, cut by the refocus, then 'Trail 3.'. The context-menu route does the same (select(dest)) and says no count at all.
- **Platform:** Linux AT-SPI/Orca measured; the selection loss is platform-independent (model logic)
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/grid\_view.rs:1373
- **Evidence:**
  - `verify-gridview-reorder-multi-20260925-143435-623194: 'check: three selected' pass [label] '3 selected'; 'Alt+Right': 14:35:13.284011 object:announcement text='Trail 3 moved to 4 of 60'; 14:35:13.284650 object:property-change:accessible-name [label] '1 selected'; 14:35:13.293114 object:announcement text='grid-view-selection-count'`
  - `verify-gridview-reorder-multi-20260925-143848-724775: same, plus orca-debug.out 14:39:26.799500 NULL SPEECH: speak 'Trail 3 moved to 4 of 60' interrupt=True / 14:39:26.976787 NULL SPEECH: stop`
  - `crates/teksilo-widgets/src/grid_view.rs:1371-1373: focused.set(Some(dest)); sel.select(dest)`
  - `crates/teksilo-data/src/selection_model.rs:149-160: select() replaces the whole set`
- **Reproduced:** 2 of 2 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Keep the selection on a reorder (remap the selected indices through the move), or at least say the change in words

### gridview-v2 {#gridview-v2}

Type-ahead with no cursor yet skips the first tile

- **Example:** grid-view
- **Scenario:** verify-gridview-type-ahead-start
- **Act:** Tab onto the grid, type 's'
- **The reader should get:** 'Sunset 1', the first caption starting with s
- **The reader gets:** Orca says 'Summit 5.' and the focused cell holds Summit 5. With no cursor, the key handler passes current = 0, and the search starts at current + 1, so tile 0 cannot be reached on the first keystroke.
- **Platform:** Linux measured; the logic is platform-independent
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/grid\_view/keyboard.rs:126, 210-216
- **Evidence:**
  - `verify-gridview-type-ahead-start-20260925-143401-623194 'type s with no cursor yet': ORCA SPEECH OUTPUT 'Summit 5.'; FAIL "the focused cell holds 'Sunset 1'" / "focused cell holds 'Summit 5'"`
  - `verify-gridview-type-ahead-start-20260925-143940-724775: same`
  - `` crates/teksilo-widgets/src/grid_view/keyboard.rs:126 `let current = cursor.unwrap_or(0);` then type_ahead.rs:95-96 `for offset in 1..=count { let i = (current + offset) % count` ``
- **Reproduced:** 2 of 2 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** With no cursor, search from index 0 inclusive, the same way the arrows land ON the end tile (keyboard.rs:228-233)

### gridview-v3 {#gridview-v3}

Alt+arrow with no cursor moves the first tile, which the reader never heard

- **Example:** grid-view
- **Scenario:** verify-gridview-alt-no-cursor
- **Act:** Tab onto the grid (no cursor), Alt+Right
- **The reader should get:** Nothing moves until the reader has a tile under the cursor (the arrows treat 'no cursor' as distinct from 'tile 0')
- **The reader gets:** Sunset 1 is moved to position 2 and selected ('Sunset 1 moved to 2 of 60', then the count id). The reader hears only 'Sunset 1.' when focus lands on it.
- **Platform:** Linux measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/grid\_view/keyboard.rs:158-178
- **Evidence:**
  - `verify-gridview-alt-no-cursor-20260925-143656-623194: announcements ['Sunset 1 moved to 2 of 60', 'grid-view-selection-count']; 14:37:08.482741 accessible-name [label] '1 selected'; ORCA SPEECH OUTPUT 'Sunset 1.'`
  - ``crates/teksilo-widgets/src/grid_view/keyboard.rs:126 current = cursor.unwrap_or(0); 158-175 reorders `current` with no check that a cursor exists``
- **Reproduced:** 1 of 1 run; deterministic by source
- **Verification:** found by the verifier, which the sweep missed.

### gridview-v4 {#gridview-v4}

setsize=60 is published on every descendant of the grid (labels, the body pane, row headers, the pinned label)

- **Example:** grid-view
- **Act:** launch (tree)
- **The reader should get:** Only the tiles carry a set size
- **The reader gets:** Every node under the grid has attrs {'setsize': '60'}: the body-pane panel, each caption label, the row headers, the stray pinned label. accesskit\_atspi\_common derives setsize for any node from its nearest filtered ancestor with size\_of\_set, and Teksilo puts that on the Grid, as AccessKit's model requires. Orca's default script ignores it; another AT reading attributes would see set members that are not items.
- **Platform:** Linux measured (tree); cosmetic
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/grid\_view.rs:1952-1954
- **Evidence:**
  - `gridview-sections-20260925-142828-464934 launch tree: panel '' attrs {'setsize': '60'}; label 'Sunset 1' attrs {'setsize': '60'}; row header 'Travel' attrs {'setsize': '60'}; grid-child label 'Travel' attrs {'setsize': '60'}`
  - `accesskit_atspi_common-0.20.0/src/node.rs:397-401 size_of_set() = size_of_set_from_container(&filter) for every node; accesskit_consumer-0.39.0/src/node.rs:629-641`
- **Reproduced:** every tree
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Upstream: emit setsize only for nodes that have a position\_in\_set
