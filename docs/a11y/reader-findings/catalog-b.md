<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Widget catalog, middle eight tabs

Examples: widget-catalog (indicators, charts, scene, text, richtext, datetime, color, menus).
24 findings: 4 critical, 11 high, 8 medium, 1 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [catalog-b-01](#catalog-b-01) | widget-catalog | Orca goes silent on any control the reader has already met once its page is reopened or it scrolls back into view | critical | Linux | fixed |
| [catalog-b-02](#catalog-b-02) | widget-catalog | RichTextEditor: focus lands on an unnamed 'section' wrapper, so Orca never reads the document, the caret or edits | critical | Linux | fixed |
| [catalog-b-03](#catalog-b-03) | widget-catalog | Menus: arrows inside an open menu or the standalone MenuList move a highlight no platform can see; Orca says only 'menu.' | critical | Linux | partly fixed |
| [catalog-b-04](#catalog-b-04) | widget-catalog | SceneView: the lightweight items (tiles, the 'draggable' rects) cannot be focused, selected or moved without a pointer, and the view itself is unnamed | critical | Linux | open |
| [catalog-b-05](#catalog-b-05) | widget-catalog | ColorPicker: the seven channel spin boxes (R, G, B, A, H, S, V) have no accessible name | high | Linux | open |
| [catalog-b-06](#catalog-b-06) | widget-catalog | ColorPicker: a live region over the whole picker floods 27 names when it appears, and its own 'Color changed' message is never announced | high | Linux | open |
| [catalog-b-07](#catalog-b-07) | widget-catalog | ColorPicker swatch grid: arrows move an index nothing exposes, and Enter picks a swatch the reader never heard; every swatch is also its own Tab stop | high | Linux | open |
| [catalog-b-08](#catalog-b-08) | widget-catalog | Saturation/brightness area: its position is not exposed on AT-SPI, so focus reads only 'Saturation and brightness panel.' | high | Linux | open |
| [catalog-b-09](#catalog-b-09) | widget-catalog | Accessible names stay English in a French UI: chart names, 'Chart legend', the HSV area and its messages are English literals, and Hue/Opacity/inner fields are frozen at build time | high | Linux | open |
| [catalog-b-10](#catalog-b-10) | widget-catalog | Rich text: the document text runs blocks together with no separator, and lists lose their structure and markers | high | Linux | open |
| [catalog-b-11](#catalog-b-11) | widget-catalog | DateEdit and TimeEdit: Tab lands on an unnamed inner entry, so the reader hears only 'entry \_\_/\_\_/\_\_\_\_' | high | Linux | fixed |
| [catalog-b-12](#catalog-b-12) | widget-catalog | ProgressBar and Spinner announce their labels whenever their page opens, ahead of the tab name: cut on the first visit, dropped on later ones | medium | Linux | open |
| [catalog-b-13](#catalog-b-13) | widget-catalog | Chart marks: the first arrow speaks the datum twice (announcement + focus), and every later press will too once K2 is fixed | medium | Linux | open |
| [catalog-b-14](#catalog-b-14) | widget-catalog | Pie/donut slices are named ', Storage: 18': a leading comma and no share, though the chart draws percentages | medium | Linux | open |
| [catalog-b-15](#catalog-b-15) | widget-catalog | Charts' names describe only their shape: the two bar charts are indistinguishable, the axes and subject are missing, and the legend is an empty list | medium | Linux | open |
| [catalog-b-16](#catalog-b-16) | widget-catalog | Wrapped text fields announce twice: HexColorInput and SearchField nest a named entry inside a second entry | medium | Linux | open |
| [catalog-b-17](#catalog-b-17) | widget-catalog | Content below the fold is missing from the accessibility tree until it is scrolled into view | medium | Linux | open |
| [catalog-b-18](#catalog-b-18) | widget-catalog | Unnamed controls and indistinguishable duplicates on the catalog pages (the example labels nothing) | high | Linux | open (example) |
| [catalog-b-19](#catalog-b-19) | widget-catalog | A disabled menu item is exposed as enabled, and a menu item's shortcut never reaches AT-SPI | high | Linux | upstream |
| [catalog-b-20](#catalog-b-20) | widget-catalog | The colour of the ColorEdit trigger and of the picker's 'Selected color' well never reaches the reader | medium | Linux | open |
| [catalog-b-21](#catalog-b-21) | widget-catalog | Decorative chrome reaches AT: an unnamed separator after every SpinBox, and an empty status bar after every text field | low | Linux | open |
| [catalog-b-M1](#catalog-b-m1) | widget-catalog | ColorEdit's popover is an unnamed dialog, and opening it replays the ColorPicker's live flood each time | high | Linux | open |
| [catalog-b-M2](#catalog-b-m2) | widget-catalog | The rich-text editor and viewer are silent on every return visit, even on the same page | high | Linux | fixed |
| [catalog-b-M3](#catalog-b-m3) | widget-catalog | libatspi rejects every AddAccessible/RemoveAccessible cache signal AccessKit sends (wrong D-Bus signature), so a libatspi client never learns a removed node came back | medium | Linux | open |

### catalog-b-01 {#catalog-b-01}

Orca goes silent on any control the reader has already met once its page is reopened or it scrolls back into view

- **Example:** widget-catalog
- **Scenario:** catalog-b-text-return, every census's 'after reopening' walk, catalog-b-charts-marks (Shift+Tab 3)
- **Act:** Tab to Username, switch to the Scene page and back, Tab to Username again. The same happens after Up/Down on the page tab in every tab's census. Separately: Tab down the Charts page to the donut, then Shift+Tab back to the first bar chart, which had scrolled out of view.
- **The reader should get:** Every time focus lands on a control, the reader hears its name, role and value, as it did the first time.
- **The reader gets:** Orca speaks nothing. The widget's AccessKit id comes from its WidgetId, so a node that leaves the filtered tree comes back with the same AT-SPI path. A node leaves when its page goes dormant, or when it scrolls out of a ScrollArea, which marks clips\_children. accesskit\_atspi\_common emitted defunct=1 for that path when the node left, and never clears it when the same id is added again. libatspi therefore keeps the defunct state for every object it had already cached, and Orca drops each of that object's events ('Ignoring defunct object'). The listener's own libatspi shows the same thing: the tree taken after reopening marks every node on the page {defunct}. Each census walked every tab's page twice. In the first walk every stop was spoken. After reopening, every control Orca had met was silent: Username, Read-only field, Password, Browse, the links, all four charts, File/Edit/Bar width, the calendar cells and buttons, Brand color, Theme accent, Saturation and brightness, Font, Click me. The same drop hits live regions: a progress bar that reappears is dropped (see catalog-b-12).
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows UIA and macOS have no defunct state; by reading accesskit\_windows/accesskit\_macos, not affected.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: catalog-b-text-return rounds 2 and 3 heard 3/3; catalog-b-charts-marks Shift+Tab 3 back to the scrolled-out bar chart heard 3/3.
- **Where:** crates/teksilo-core/src/accessibility.rs:1983-1988 (widget\_id\_to\_node\_id); crates/teksilo-widgets/src/scroll\_area.rs:1305; accesskit\_atspi\_common-0.20.0/src/adapter.rs:49-106
- **Evidence:**
  - `text-return, round 2 (report.txt): '+1669.0 ms object:state-changed:focused 1 [entry] 'Username'' then 'FAIL  Orca says 'Username' / Orca unheard: 'Username''; orca-debug.out: '13:00:26.077787 - EVENT MANAGER: Ignoring defunct object: [entry: 'Username']'`
  - `text census, hiding the page ('Up to the Scene tab'): '+62.0 ms object:state-changed:defunct 1 [entry] 'Username''; after reopening, Tab 4: '+24.5 ms object:state-changed:focused 1 [entry] 'Username'' and Orca '13:16:45.637097 - EVENT MANAGER: Ignoring defunct object: [entry: 'Username']' (c4/catalog-b-text-20260925-131556-2722724)`
  - `charts census, after reopening: '13:15:19.390355 - EVENT MANAGER: Ignoring defunct object: [document frame: 'Bar chart: 3 series, 4 categories']', '13:15:23.239913 - EVENT MANAGER: Ignoring defunct object: [document frame: 'Line chart: 3 series, 4 points']', '13:15:25.328711 - EVENT MANAGER: Ignoring defunct object: [document frame: 'Pie chart: 5 slices']'`
  - `charts-marks, scrolling: 'Tab to the pie chart' '+95.0 ms object:state-changed:defunct 1 [document frame] 'Bar chart: 3 series, 4 categories''; 'Shift+Tab 3' '+501.0 ms object:children-changed:add [panel] '' -> [document frame] 'Bar chart: 3 series, 4 categories'', '+501.8 ms object:state-changed:focused 1 [document frame] 'Bar chart: 3 series, 4 categories'', then 'FAIL  Orca says 'Bar chart'' and '13:00:51.162502 EVENT MANAGER: Ignoring defunct object: [document frame: 'Bar chart: 3 series, 4 categories']'`
  - `listener tree after reopening the Scene page (c1/catalog-b-scene-20260925-130154-2081993/tree-Down-back-to-the-Scene-tab.txt): '[push button] 'Click me' {defunct,focusable}'`
  - `Reproduced: text-return rounds 2 and 3 silent in 3 of 3 runs. Scroll-back silence in charts-marks in 3 of 3 runs. Every census's after-reopening walk was silent in 8 of 8 tabs (c3/c4/c5 runs).`
  - `crates/teksilo-core/src/accessibility.rs:1984-1989 widget_id_to_node_id (the NodeId is the WidgetId's ffi value, so it is stable across dormancy)`
  - `crates/teksilo-widgets/src/scroll_area.rs:1305 builder.inner_mut().set_clips_children(); accesskit_consumer-0.39.0/src/filters.rs:64-88 excludes a clipped child's subtree when it is out of the parent's box`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:91-106 remove_node emits StateChanged(Defunct, true); add_node (adapter.rs:49-80) never emits Defunct false`
  - `orca/event_manager.py:796-798 'if AXObject.is_dead(event.source) or AXUtilities.is_defunct(event.source): ... Ignoring defunct object'`
  - `a1/catalog-b-text-return-20260925-132603-2989355 orca-debug.out: '13:26:25.618890 - EVENT MANAGER: Ignoring defunct object: [entry: 'Username']' (round 2: FAIL Orca says 'Username')`
  - `a3/catalog-b-charts-marks-20260925-133202-3303740 orca-debug.out: '13:32:58.179152 - EVENT MANAGER: Ignoring defunct object: [document frame: 'Bar chart: 3 series, 4 categories']' (Shift+Tab 3, said=[])`
  - `v1/verify-catalog-b-reopen-unmet-20260925-133501-3505595: after reopening, Tab 4 '13:35:24.012651 - EVENT MANAGER: Ignoring defunct object: [entry: 'Username']' and said=[]; Tab 5 '13:35:26.079337 - SPEECH OUTPUT: 'Read-only field read only entry Read-only value selected.'' (never met before, spoken)`
  - `v1/verify-catalog-b-datetime-reopen-20260925-133504-3509618: after reopening, Tabs 4-26 said=[] with orca-dropped-defunct, including [date editor] 'Date', 'Time', 'Start date', 'End date' and 'Open range calendar'`
  - `c1/catalog-b-text-20260925-133724-3645771/tree-Down-back-to-the-Text-tab.txt: '[scroll pane] 'Text' {defunct}', '[panel] '' {defunct}' (the listener's own libatspi keeps the bit)`
  - `every run's stdout, e.g. a1-catalog-b-text-return.stdout (354 lines): '(process:2989740): dbind-WARNING **: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau' and 'Unknown signature so for RemoveAccessible'`
- **Reproduced:** 3 of 3 runs (text-return, charts-marks); 8 of 8 tab censuses
- **Verification:** confirmed. Reproduced: 3 of 3 runs (text-return a1/a2/a3; charts-marks a1/a2/a3); 8 of 8 censuses; 2 of 2 reopen-unmet; 1 of 1 datetime-reopen (27-stop second walk)
- **Fix idea:** Give a widget a new AccessKit id each time it re-enters the filtered tree, as the K2 fix does for the announcer, for example an arena generation carried in the NodeId. Alternatively patch accesskit\_atspi\_common's add\_node to emit StateChanged(Defunct,false) for an id it removed earlier. For scrolling, stop exporting ScrollArea as clips\_children, so off-screen content is never removed from the tree (this also fixes catalog-b-17).

### catalog-b-02 {#catalog-b-02}

RichTextEditor: focus lands on an unnamed 'section' wrapper, so Orca never reads the document, the caret or edits

- **Example:** widget-catalog
- **Scenario:** catalog-b-richtext-documents, catalog-b-richtext
- **Act:** Tab into the editor, press Down x3 and Ctrl+End, type 'x', then Ctrl+Tab to the read-only viewer and press Down/Ctrl+End.
- **The reader should get:** Focus lands on the editable text (AT-SPI entry, multi-line) with its name. Each caret move reads the line reached, and typed text lands in the focused text object. The viewer behaves the same as a document.
- **The reader gets:** Focus goes to the outer RichTextEditor wrapper. It is focusable and has Role::GenericContainer, which the consumer's filter keeps because it is focused, and AT-SPI exposes it as an unnamed 'section'. Orca says only 'section.'. The body that has the text role is never focused. accesskit\_atspi\_common emits text-caret-moved only for a focused text node, so no caret event ever reaches the bus: Down, Down, Down and Ctrl+End produce nothing, and Orca says nothing. Typing 'x' changes the unfocused \[entry\] and Orca stays silent. The read-only viewer is the same: 'section.' and silent arrows.
- **Platform:** Linux AT-SPI / Orca (measured). On Windows and macOS the focused element is likewise the GenericContainer wrapper, which has no text pattern (reading the source), so the caret is probably not tracked there either.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `731cc2e1` (editor-focus).
- **Where:** crates/teksilo-widgets/src/rich\_text.rs:3853, 4281-4291
- **Evidence:**
  - `f1 richtext-documents 'Tab into the editor': '+1776.8 ms object:state-changed:focused 1 [section] ''' ; orca-debug.out '13:09:58.776657 - SPEECH OUTPUT: 'section.''`
  - `'Down in the editor' x3 and 'ctrl+End in the editor': 'FAIL  a object:text-caret-moved event from [*] '*' / no object:text-caret-moved event from [*] '*'' and 'Orca said nothing'`
  - `'type 'x' in the editor': '+31.6 ms object:text-changed:insert [entry] '' text='x'' (the entry was never focused)`
  - `'Ctrl+Tab to the read-only viewer': '+58.5 ms object:state-changed:focused 1 [section] ''' ; '13:10:26.887647 - SPEECH OUTPUT: 'section.''`
  - `census walks (c2 and c4 richtext): 'first walk, Tab 4 (Tab): focus=[section] '' | said=['section.']'. The next Tab typed '\t' into the editor ('+298.1 ms object:text-changed:insert [entry] '' text='\t'') with no focus event.`
  - `crates/teksilo-widgets/src/rich_text.rs:3853 '.focusable(true)' on the wrapper; rich_text.rs:4281-4291 wrapper accessibility() sets Role::GenericContainer; rich_text.rs:3500-3514 says the body 'itself is non-focusable'`
  - `accesskit_consumer-0.39.0/src/filters.rs:18-20 keeps a focused node whatever its role; accesskit_atspi_common-0.20.0/src/adapter.rs:216-218 returns before emitting CaretMoved unless the text node is focused`
  - `b2/catalog-b-richtext-documents-20260925-133335-3406572 orca-debug.out: '13:33:47.181412 - SPEECH OUTPUT: 'section.''`
  - `b1 richtext-documents 'type 'x' in the editor': '+27.0 ms object:text-changed:insert [entry] '' text='x'', '+27.3 ms object:property-change:accessible-name [label] '10x'' (the caret did move to the end; no caret event reached the bus)`
  - `v1/verify-catalog-b-richtext-tab-20260925-133507-3514799 'Tab inside the editor': '+272.9 ms object:text-changed:insert [entry] '' text='\t'', no focus change and no speech; 'Shift+Tab inside the editor': no event at all`
  - `same run 'Ctrl+Shift+Tab back to the editor': '+49.2 ms object:state-changed:defunct 1 [section]', '+49.6 ms object:state-changed:focused 1 [section]', then '13:35:35.093460 - EVENT MANAGER: Ignoring defunct object: [section]' and Orca said nothing`
- **Reproduced:** 4 of 4 runs (f1 richtext-documents, c2 and c4 richtext censuses in both walks); structural, not timing
- **Verification:** confirmed. Reproduced: 2 of 2 richtext-documents runs (b1, b2); 1 of 1 census (c2); 3 of 3 verify-richtext-tab runs
- **Fix idea:** Put the platform focus on the body node: make the body the focusable node, or have the wrapper redirect a11y focus to the body. The TextInput pattern the wrapper's comment cites has a focusable inner field. Then give the editor a name, for example from the section heading through labelled\_by.

### catalog-b-03 {#catalog-b-03}

Menus: arrows inside an open menu or the standalone MenuList move a highlight no platform can see; Orca says only 'menu.'

- **Example:** widget-catalog
- **Scenario:** catalog-b-menus-open, catalog-b-menus
- **Act:** Tab to File, press Down to open it, Down to Open, Escape, Right to Edit, Down, End, Right into Alignment. Then Tab to the standalone MenuList and press Down, Down.
- **The reader should get:** Opening a menu announces the menu by name and its first item, and each arrow announces the item reached. The standalone MenuList announces its item.
- **The reader gets:** Opening File (and Edit, and Alignment) moves focus to an unnamed \[menu\], and Orca says 'menu.'. Down, End and Right-in-menu then emit no focus and no active-descendant event, and Orca says nothing, so no menu item is ever heard. Right on File opens Edit's menu the same way ('menu.'). The standalone MenuList behaves the same: 'menu.', then silence on arrows. MenuList keeps a private focused\_index and exposes no active\_descendant. The menu bars in the catalog's title bar and on this page share the widget. Also: each top-level menu (File, Edit) is its own Tab stop, and Orca says just 'File.', without the menu role or a popup hint.
- **Platform:** Linux AT-SPI / Orca (measured). By source the same on Windows and macOS: no node ever carries the highlighted item.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `9636094c` (menus). Fixed part: arrow part and unnamed menu; not the one-Tab-stop-per-menu-bar-item part.
- **Where:** crates/teksilo-widgets/src/menu\_list.rs:748-800, 991-993
- **Evidence:**
  - `f2 menus-open 'Down opens File': '+27.9 ms object:children-changed:add [frame] '' -> [menu] ''', '+29.4 ms object:state-changed:focused 1 [menu] '''; orca-debug.out '13:12:18.727188 - SPEECH OUTPUT: 'menu.''`
  - `'Down to Open': 'FAIL  focus lands on [menu item] 'Open' / no focus change on the bus in this act', 'Orca unheard: 'Open''`
  - `'Right opens Alignment': '+34.8 ms object:state-changed:focused 1 [menu] ''' then '13:12:42.091902 - SPEECH OUTPUT: 'menu.''`
  - `'Down in the MenuList': 'no focus change on the bus in this act', 'Orca unheard: 'Cut''`
  - `menus census first walk: '[menu item] 'File' | [menu item] 'Edit' | [slider] 'Bar width' | [menu item] 'File' | [menu item] 'Edit' | [menu] '''`
  - `crates/teksilo-widgets/src/menu_list.rs:748-762 ArrowDown only does focused_index.set(Some(next)); menu_list.rs:991-993 accessibility() sets Role::Menu and nothing else (no active_descendant, no name)`
  - `b2/catalog-b-menus-open-20260925-133338-3409934 orca-debug.out: '13:33:54.598783 - SPEECH OUTPUT: 'menu.'' (Down opens File) and '13:34:06.239813 - SPEECH OUTPUT: 'menu.'' (Right opens Edit's menu)`
  - `c1/verify-catalog-b-menu-tree-20260925-133730-3653210 'Down to the second item': 'FAIL a object:state-changed event from [*] '*' / no object:state-changed event'; tree before and after: '[menu item] 'New' states=['enabled', 'sensitive', 'showing', 'visible']', '[menu item] 'Open' states=['enabled', 'sensitive', 'showing', 'visible']'`
- **Reproduced:** 2 of 2 menus-open runs, 3 of 3 menus censuses (structural)
- **Verification:** confirmed. Reproduced: 2 of 2 menus-open (b1, b2); 1 of 1 census (c2); 1 of 1 verify-menu-tree
- **Fix idea:** Expose the highlighted row: set\_active\_descendant on the Menu node to the focused MenuItem's node (Orca's onActiveDescendantChanged accepts it because the menu is focused), or move real focus to the item. Name the menu after its trigger ('File'). Make a menu bar a single Tab stop with roving arrows.

### catalog-b-04 {#catalog-b-04}

SceneView: the lightweight items (tiles, the 'draggable' rects) cannot be focused, selected or moved without a pointer, and the view itself is unnamed

- **Example:** widget-catalog
- **Scenario:** catalog-b-scene-items, catalog-b-scene
- **Act:** Tab to the scene view, press Right/Down (pan), press Space. Then an AT-SPI grab\_focus on 'draggable 1', Tab to the card's button, and Shift+Tab back.
- **The reader should get:** The viewport is named. A keyboard or screen-reader user can reach an item, select it and move it, since Alt+Arrow nudging is the documented alternative to dragging. Selection and panning give some feedback.
- **The reader gets:** The SceneView is an unnamed \[panel\], and Orca says 'panel.' followed by an unrelated label from the card, 'A real Button at scene coordinates.'. The synthetic item nodes are named ('tile 1'..'tile 4', 'decorative zigzag', 'draggable 1/2'). They have no actions and no focusable or selectable state, and the view emits no selection state. Arrows pan silently. Space selects nothing (no selected event). An AT-SPI grab\_focus on 'draggable 1' produces no focus change. Tab goes straight from the view to 'Click me'. Alt+Arrow nudge moves only selected items, and selection needs a pointer, so the non-drag alternative cannot be reached. SceneView::focus\_in\_direction exists, but nothing calls it for Tab, and ItemFlags::IS\_FOCUSABLE is 'Declared only'. The 'drag me' labels and the 'Lightweight items' group appear as flat siblings, not as children of their items.
- **Platform:** Linux AT-SPI / Orca (measured); the missing keyboard and AT path is platform-independent (source).
- **Severity:** critical; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-scene/src/view/gestures\_impl.rs:1085-1270; view/builder\_impl.rs:402-443; view/a11y\_impl.rs:154-161
- **Evidence:**
  - `scene-items 'Tab to the scene view': '+1736.9 ms object:state-changed:focused 1 [panel] ''' ; orca-debug.out '13:09:58.436363 - SPEECH OUTPUT: 'panel.'' and '13:09:58.436409 - SPEECH OUTPUT: 'A real Button at scene coordinates.''`
  - `'Right in the scene view' / 'Down in the scene view': 'FAIL  Orca says something / Orca said nothing'`
  - `'Space in the scene view': 'FAIL  a object:state-changed:selected event from [*] '*' / no object:state-changed:selected event from [*] '*''`
  - `'look for a way to operate 'draggable 1'': '[panel] 'draggable 1': actions=None states=['enabled', 'sensitive', 'showing', 'visible']'`
  - `'AT-SPI grab_focus on 'draggable 1'': '+24.5 ms == harness:grab-focus  [panel] 'draggable 1'' then 'no focus change on the bus in this act'`
  - `crates/teksilo-scene/src/view/gestures_impl.rs:1134-1140 (Alt+Arrow nudges the selection), 1249-1270 (arrows pan, +/- zoom); crates/teksilo-scene/src/items/rect.rs:281-287 (GraphicsObject + name, no actions); crates/teksilo-scene/src/flags.rs:43-47 (IS_FOCUSABLE 'Declared only ... nothing reads this bit'); view/builder_impl.rs:402-431 focus_in_direction has no caller`
  - `crates/teksilo-scene/src/view/a11y_impl.rs:154-161 names the view only when a11y_label is set; examples/widget_catalog/src/tabs/scene.rs:125-127 sets no access_label`
  - `b2/catalog-b-scene-items-20260925-133341-3413026 orca-debug.out: '13:33:53.187084 - SPEECH OUTPUT: 'panel.''; 'Tab to the card's button': '+18.1 ms object:state-changed:focused 1 [push button] 'Click me'' straight from [panel] '' (b1, same)`
  - `b1/b2 'look for a way to operate 'draggable 1'': FAIL, actions=None, states=['enabled','sensitive','showing','visible']; 'AT-SPI grab_focus on 'draggable 1'': FAIL no focus change`
  - `grep: next_focus/previous_focus/focus_in_direction are used only in crates/teksilo-scene/src/view/tests.rs`
- **Reproduced:** 1 of 1 scene-items run plus 3 of 3 scene censuses for the unnamed view; structural
- **Verification:** confirmed. Reproduced: 2 of 2 scene-items (b1, b2); 1 of 1 scene census (c2); structural
- **Fix idea:** Give SceneView keyboard item navigation (Tab or arrows through focus\_in\_direction, exposed as active\_descendant or focus on the synthetic item nodes), with selection on Space/Enter and a Selected state plus a Click/Select action on selectable items, so Alt+Arrow becomes usable. Give the view a default name, and have the example label it. Optionally announce pan and zoom.

### catalog-b-05 {#catalog-b-05}

ColorPicker: the seven channel spin boxes (R, G, B, A, H, S, V) have no accessible name

- **Example:** widget-catalog
- **Scenario:** catalog-b-color, catalog-b-color-channels
- **Act:** Tab from Hex through the channel spin boxes; Up on the R channel.
- **The reader should get:** Each spin box is announced by its channel, for example 'Red, 136, spin button'.
- **The reader gets:** '136 spin button.', '68 spin button.', '187 spin button.' and so on. There is no way to tell which channel is which. The visible 'R', 'G', 'B' are separate label nodes that are not associated with the field. Value changes are spoken ('138').
- **Platform:** Linux AT-SPI / Orca (measured); the missing name is platform-independent.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker.rs:862-987
- **Evidence:**
  - `color census first walk 'Tab 11..17': focus=[spin button] '' and orca-debug.out '13:10:18.157288 - SPEECH OUTPUT: '137 spin button.'', '13:10:26.237797 - SPEECH OUTPUT: '66 spin button.'' (color-channels)`
  - `color-channels 'Tab on to the R channel': 'FAIL  the spin box is named for its channel / focus on [spin button] '''`
  - `tree: '[label] 'R'' followed by '[spin button] '' {editable,focusable,selectable-text,single-line} value={'current': 136.0, ...'`
  - `crates/teksilo-widgets/src/color_picker.rs:981-987 spinner_cell puts TextWidget::new(lit!(label)) beside the spinner, with no label or labelled_by on the SpinBox; color_picker.rs:862-979 build the SpinBoxes without .label()`
  - `the full names exist unused: crates/teksilo-widgets/locales/*.ftl color-picker-red-label, color-picker-green-label, color-picker-blue-label, color-picker-saturation-label, color-picker-value-label (grep finds no use in src)`
  - `a3/catalog-b-color-channels-20260925-133208-3310168 orca-debug.out: '13:32:42.361919 - SPEECH OUTPUT: '137 spin button.''`
  - `a3/catalog-b-color-fr-20260925-133212-3311510 tree: '[label] 'R'' then '[spin button] '' {editable,focusable,...} value={'current': 136.0, 'minimum': 0.0, 'maximum': 255.0 ...}'`
- **Reproduced:** 4 of 4 runs (c1 and c5 color censuses, color-channels x2); structural
- **Verification:** confirmed. Reproduced: 3 of 3 color-channels (a1, a2, a3); 1 of 1 census (c1); structural
- **Fix idea:** Label each SpinBox with the full channel name (color-picker-red-label etc.), or labelled\_by the short visible label and describe it with the full one.

### catalog-b-06 {#catalog-b-06}

ColorPicker: a live region over the whole picker floods 27 names when it appears, and its own 'Color changed' message is never announced

- **Example:** widget-catalog
- **Scenario:** catalog-b-color, catalog-b-color-channels, catalog-b-color-fr, catalog-b-menus (Up to the Color tab)
- **Act:** Launch on the Color tab, or open the Color page from Menus. Switch the UI to French. Press Enter on a swatch in the grid.
- **The reader should get:** Opening the page reads the tab. Changing the colour (a swatch, a channel, a strip) announces the new colour once.
- **The reader gets:** The picker's root Group is Live::Polite, and AccessKit inherits live to every descendant, so each adapter announces every named descendant as it is added. That is 27 announcements whenever the picker appears ('Swatch #3685E3', 'Hex', 'V', 'Swatch #A866D9', 'Color presets', 'H', ... 'Saturation and brightness', 'B'), and 15 more on a language switch, because each name change is announced again ('Nuance #3685E3', 'Nuances prédéfinies', 'Sélecteur de couleur', ...). Orca spoke them ahead of the tab name, almost all cut. The message the live region exists for ('color-picker-changed-announcement', 'Color changed to #…') is set as the root's \*value\*, and every adapter announces a live node's \*name\*. So choosing a swatch with Enter changed the colour (spin buttons and Hex updated on the bus) and Orca said nothing.
- **Platform:** Linux AT-SPI / Orca (measured). By source, Windows (UIA LiveRegionChanged on node\_added) and macOS (announcement on add) also announce every descendant on add, and none reads the value.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker.rs:813-832
- **Evidence:**
  - `color census launch: 135 object:announcement lines in report.txt, e.g. '+2341.7 ms object:announcement [slider] 'Hue' text='Hue'', '+2360.1 ms object:announcement [push button] 'Swatch #F5F5F5' text='Swatch #F5F5F5'', '+2361.3 ms object:announcement [label] 'S' text='S''`
  - `menus census 'Up to the Color tab': Orca said (all cut) 'Selected color', 'Swatch #F5F5F5', 'Swatch #0F0F0F', 'Hue', 'R', 'Color presets', ... 27 names, then 'Color page tab.'`
  - `color-fr 'activate Français': '+186.2 ms object:announcement [push button] 'Nuance #3685E3' text='Nuance #3685E3'', '+188.5 ms object:announcement [panel] 'Sélecteur de couleur' text='Sélecteur de couleur''`
  - `color-channels 'Enter in the swatch grid': text/value changes only ('+65.0 ms object:text-changed:insert [entry] 'Hex' text='F5D445''), no object:announcement, 'FAIL  Orca says one of ['Color changed', '#'] / Orca said nothing' (2 of 2 runs)`
  - `crates/teksilo-widgets/src/color_picker.rs:813-832 set_role(Group), set_live(Live::Polite), set_value(color-picker-changed-announcement)`
  - `accesskit_consumer-0.39.0/src/node.rs:906-910 live() inherits from the parent; accesskit_atspi_common-0.20.0/src/adapter.rs:72-77 announces a live node's name on add`
  - `v2/catalog-b-color-fr-20260925-134232-3840378 orca-debug.out: '13:42:41.455054 - SPEECH OUTPUT: 'Nuance #3685E3'' … '13:42:41.680064 - SPEECH OUTPUT: 'Sélecteur de couleur'' … '13:42:41.736917 - SPEECH OUTPUT: 'Nuance #6BB359'' (15 utterances, no stop between them)`
  - `a1/catalog-b-color-channels-20260925-132612-2994108 'Enter in the swatch grid': '+31.6 ms object:text-changed:insert [entry] 'Hex' text='F5D445'' and the spin-button value changes, no object:announcement, 'FAIL Orca says one of ['Color changed', '#'] / Orca said nothing'`
  - `/usr/lib/python3/dist-packages/orca/speechdispatcherfactory.py:462-464 '#if interrupt: #    self._cancel()'`
- **Reproduced:** flood: 5 of 5 first appearances (c1 and c5 color launch, color-channels x2, color-fr, menus census page open); silent change: 2 of 2 runs
- **Verification:** confirmed. Reproduced: flood at first appearance: 7 of 7 (color-channels a1/a2/a3, color-fr a3/v2/v3, color census c1) plus the menus census page-open (c2); language-switch flood spoken whole 3 of 3; silent swatch Enter 3 of 3
- **Fix idea:** Take live off the picker root. Announce a colour change from a dedicated live node whose \*name\* is the message, or through ctx.announce (which the K2 fix makes reliable).

### catalog-b-07 {#catalog-b-07}

ColorPicker swatch grid: arrows move an index nothing exposes, and Enter picks a swatch the reader never heard; every swatch is also its own Tab stop

- **Example:** widget-catalog
- **Scenario:** catalog-b-color-channels, catalog-b-color
- **Act:** Tab to 'Color presets', press Right twice, then Enter.
- **The reader should get:** The grid is one Tab stop. Arrows announce the swatch reached (roving focus or active descendant), and Enter confirms the choice.
- **The reader gets:** Right and Right produce no event and no speech. Enter then applied the third swatch (#F5D445) silently. Tab stops on the grid itself and then on each of its 12 swatches (13 stops).
- **Platform:** Linux AT-SPI / Orca (measured); by source the same elsewhere.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker/swatch\_grid.rs:125-225
- **Evidence:**
  - `color-channels 'Right in the swatch grid, 1' and '2': 'FAIL  a object:active-descendant-changed event from [*] '*' / no object:active-descendant-changed event from [*] '*'', 'Orca unheard: 'Swatch'' (2 of 2 runs)`
  - `'Enter in the swatch grid': '+65.0 ms object:text-changed:insert [entry] 'Hex' text='F5D445'' and 'Orca said nothing'`
  - `color census Tab stops: '[table] 'Color presets' | [push button] 'Swatch #E84D3D' | [push button] 'Swatch #F29933' | ...'`
  - `crates/teksilo-widgets/src/color_picker/swatch_grid.rs:136-188 arrows only update focused_index ('Each ColorSwatch cell remains independently focusable via Tab', :36-38); swatch_grid.rs:221-225 accessibility() sets no active_descendant; color_picker/swatch.rs:204 .focusable(true) per swatch`
  - `a2/catalog-b-color-channels-20260925-132807-3089040 'Right in the swatch grid, 1' and '2': no events at all; 'Enter in the swatch grid': Hex text 'F5D445' and Orca said nothing`
  - `c1/catalog-b-color-20260925-133721-3644770: first walk Tab 18 [table] 'Color presets', Tabs 19-30 each '[push button] 'Swatch #…''`
- **Reproduced:** 2 of 2 color-channels runs; structural
- **Verification:** confirmed. Reproduced: 3 of 3 color-channels (a1, a2, a3); 1 of 1 color census
- **Fix idea:** Implement the roving pattern that the swatch grid's own doc comment describes: move real focus to the swatch at focused\_index (or set active\_descendant), take the swatches out of the Tab order, and say the selection on Enter.

### catalog-b-08 {#catalog-b-08}

Saturation/brightness area: its position is not exposed on AT-SPI, so focus reads only 'Saturation and brightness panel.'

- **Example:** widget-catalog
- **Scenario:** catalog-b-color-channels, catalog-b-color
- **Act:** Tab to the saturation/brightness area, press Right.
- **The reader should get:** Focus reads the area and where the marker is ('Saturation 64%, brightness 73%'), and each arrow reads the new position.
- **The reader gets:** On focus Orca says 'Color picker panel.' and 'Saturation and brightness panel.', with no position. The canvas carries its position as a string value on a Role::Group, and accesskit\_atspi\_common turns a string value into a name only for Role::Label, so the value never reaches AT-SPI. Arrow feedback comes only through ctx.announce ('Saturation 65%, brightness 73%'), which K2 drops on main ('Ignoring defunct object: \[DEAD\]'). The K2 fix covers that part, but not the missing value on focus.
- **Platform:** Linux AT-SPI / Orca (measured). By source, Windows would expose the value through the UIA Value pattern (accesskit\_windows node.rs:592-597).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker/hsv\_canvas.rs:384-400
- **Evidence:**
  - `color-channels 'Tab to the saturation/brightness area': '13:10:01.903430 - SPEECH OUTPUT: 'Saturation and brightness panel.'' and 'FAIL  Orca says one of ['64%', '64 %', '64 percent']' (2 of 2 runs, and c5 color census)`
  - `'Right on the saturation/brightness area': '+199.1 ms object:announcement [status bar] 'Saturation 65%, brightness 73%' text='Saturation 65%, brightness 73%'' then '13:10:05.703101 - EVENT MANAGER: Ignoring defunct object: [DEAD]' (the announcer, path /org/a11y/atspi/accessible/0/18446744073709551616)`
  - `tree: '[panel] 'Saturation and brightness' ... actions=None ... value None text None'`
  - `crates/teksilo-widgets/src/color_picker/hsv_canvas.rs:394-399 set_role(Group) + set_value(format!("Saturation {}%, brightness {}%")); accesskit_atspi_common-0.20.0/src/node.rs:37-43 name() uses value() only when label_comes_from_value`
  - `a1/catalog-b-color-channels-20260925-132612-2994108 'Right on the saturation/brightness area': said 'Saturation 65%, brightness 73%' (first announcer message); a2 and a3: announcement on the bus, orca-dropped-defunct, said=[]`
  - `accesskit_atspi_common-0.20.0/src/node.rs:532-545 n_actions = 1 if clickable, name 'click' only`
- **Reproduced:** 3 of 3 runs (color-channels x2, c5 color census)
- **Verification:** confirmed. Reproduced: 3 of 3 (color-channels a1, a2, a3; also the color census)
- **Fix idea:** Put the position where AT-SPI carries it: in the description, or in a named child, or as the name suffix. Consider two sliders (saturation, brightness) for the keyboard path.

### catalog-b-09 {#catalog-b-09}

Accessible names stay English in a French UI: chart names, 'Chart legend', the HSV area and its messages are English literals, and Hue/Opacity/inner fields are frozen at build time

- **Example:** widget-catalog
- **Scenario:** catalog-b-charts-fr, catalog-b-color-fr
- **Act:** Activate the title bar's 'Français' button through AT-SPI, then read the tree and Tab to a chart or arrow on the HSV area.
- **The reader should get:** Every accessible name follows the UI language, as every visible string on the page does ('Graphiques', 'Sélecteur de couleur', 'Teinte').
- **The reader gets:** Charts: 'Bar chart: 3 series, 4 categories', 'Line chart: 3 series, 4 points', 'Pie chart: 5 slices' and '\[list\] 'Chart legend'' stay English, and Orca says 'Bar chart: 3 series, 4 categories document frame.' on the French page. Colour: 'Saturation and brightness' and its arrow message 'Saturation 65%, brightness 73%' are English literals. 'Hue' and 'Opacity' stay English although fr-FR has 'Teinte'/'Opacité', because they are resolved once at build and never re-resolved. The HexColorInput's inner entry keeps 'Brand color' while its wrapper becomes 'Couleur de marque'. The TabWidget strip's 'Scroll tabs down' and 'Show all tabs' also stay English.
- **Platform:** Linux AT-SPI / Orca (measured); platform-independent (the strings are built in Teksilo).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-charts/src/bar\_chart.rs:783-786, line\_chart.rs:774-777, pie\_chart.rs:714, legend.rs:639-640; crates/teksilo-widgets/src/color\_picker/hsv\_canvas.rs:238-242, 395; color\_picker.rs:534, 546; tab\_widget/bar.rs:2736, 2817
- **Evidence:**
  - `charts-fr 'look at the charts' names after the switch': 'found [document frame] 'Bar chart: 3 series, 4 categories'', 'found [document frame] 'Line chart: 3 series, 4 points'', 'found [list] 'Chart legend'' (the page tab is already 'Graphiques')`
  - `charts-fr 'Tab to the first chart, in French': Orca said 'Bar chart: 3 series, 4 categories document frame.'`
  - `color-fr tree after the switch: '[panel] 'Sélecteur de couleur'' > '[panel] 'Saturation and brightness'', '[slider] 'Hue'', '[slider] 'Opacity''; '[entry] 'Couleur de marque'' > '[entry] 'Brand color''; 'FAIL  the tree holds [slider] 'Teinte''`
  - `color-fr 'Right ... in French': Orca said 'Saturation 65%, brightness 73%'`
  - `crates/teksilo-charts/src/bar_chart.rs:783-786, line_chart.rs:774, pie_chart.rs:714 (format!("Bar chart: {} series, {} categories") etc.), legend.rs:639-640 set_name("Chart legend")`
  - `crates/teksilo-widgets/src/color_picker/hsv_canvas.rs:238-242 and 394-399 (lit!("Saturation and brightness"), format!("Saturation {}%, brightness {}%")); color_picker.rs:534 and 546 .label(resolve_message_widget(...)) resolved once at build`
  - `b2/catalog-b-charts-fr-20260925-133347-3417680 'Tab to the first chart, in French': Orca said 'Graphiques page tab.' then 'Bar chart: 3 series, 4 categories document frame.'`
  - `a3/catalog-b-color-fr-20260925-133212-3311510 tree: '[entry] 'Couleur de marque'' > '[entry] 'Brand color''; '[panel] 'Saturation and brightness''; '[slider] 'Hue''; '[slider] 'Opacity''; '[push button] 'Scroll tabs down' desc='Scroll tabs down''`
  - `b1 charts-fr tree: '[label] 'BarChart'', '[label] 'LineChart'' (example literals, still English)`
- **Reproduced:** 1 of 1 run each (charts-fr, color-fr); deterministic (the strings are literals)
- **Verification:** confirmed. Reproduced: 2 of 2 charts-fr (b1, b2); 3 of 3 color-fr (a3, v2, v3); deterministic
- **Fix idea:** Route the chart, legend and HSV strings through the framework's Fluent bundle (as color-picker-\* already are), and resolve labels at accessibility() time or bind them to the locale signal so a live switch re-resolves them.

### catalog-b-10 {#catalog-b-10}

Rich text: the document text runs blocks together with no separator, and lists lose their structure and markers

- **Example:** widget-catalog
- **Scenario:** catalog-b-richtext-documents, catalog-b-richtext
- **Act:** Read the AT-SPI Text of the editor and of the read-only viewer (tree after 'Tab into the editor').
- **The reader should get:** Block boundaries are in the text, so word and sentence reading do not merge across paragraphs. List items are exposed as lists with their bullets or numbers.
- **The reader gets:** The editor's text is 'Type here — bold, italic, and code all work.editable bulletsecond bulletA blockquote you can edit.KnobValuemin\_lines3max\_lines10'. The viewer's is 'Read-only viewerRichTextEditor::read\_only renders ... Ctrl+A) — but rejects every mutating key.ListsBold, italic, and inline codeNested bullets reflow:second levelsecond level again...Ordered lists tooWith their own numbering...'. The tree has no list or list item nodes, and neither the bullets nor the ordered numbering (1., 2., 3.) appear anywhere. Headings, blockquotes and tables do get structure.
- **Platform:** Linux AT-SPI (measured from the tree). The text comes from accesskit\_consumer, so the same runs reach every adapter.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/rich\_text/body/flow\_walk.rs:331-397
- **Evidence:**
  - `richtext-documents 'Tab into the editor': 'FAIL  the editor's text keeps its paragraphs apart / [entry] '': text='Type here — bold, italic, and code all work.editable bulletsecond bulletA blockquote you can edit.KnobValuemin_lines3max_lines10''`
  - `'FAIL  the viewer's text keeps its headings and paragraphs apart / [document frame] '': text='Read-only viewerRichTextEditor::read_only renders a TextDocument without a caret. It supports selection, mouse-wheel scrolling, and keyboard''`
  - `richtext census tree: '[entry] '' ... > [block quote] '' > [label] 'A blockquote you can edit.'', '[table] '' > [table row] '' > [table cell] ''', with no list nodes`
  - `crates/teksilo-widgets/src/rich_text/body/flow_walk.rs:331-397 block() emits TextRunSource::from_geometry(&block.text, ...) with no paragraph separator, and no list role or marker for list blocks`
  - `b2/catalog-b-richtext-documents-20260925-133335-3406572 'Tab into the editor': 'FAIL the editor's text keeps its paragraphs apart / [entry] '': text='Type here — bold, italic, and code all work.editable bulletsecond bulletA blockquote you can edit.KnobValuemin_lines3max_lines10''`
- **Reproduced:** 3 of 3 runs (f1 richtext-documents, c2 and c4 censuses show the same text); deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 richtext-documents (b1, b2); deterministic
- **Fix idea:** End each block's last run with a hard break (the text\_runs contract already models a break as one character), and emit Role::List/ListItem, or at least the list marker text, for list blocks.

### catalog-b-11 {#catalog-b-11}

DateEdit and TimeEdit: Tab lands on an unnamed inner entry, so the reader hears only 'entry \_\_/\_\_/\_\_\_\_'

- **Example:** widget-catalog
- **Scenario:** catalog-b-datetime, catalog-b-datetime-fields
- **Act:** Tab through the Date & Time tab to the DateEdit and the TimeEdit.
- **The reader should get:** 'Date, entry, …' and 'Time, entry, …', with the field's value or placeholder.
- **The reader gets:** Focus goes to the inner \[entry\] ''. Orca says 'entry \_\_/\_\_/\_\_\_\_' and 'entry \_\_:\_\_ \_\_'. The name 'Date'/'Time' and the value sit on the unfocused outer \[date editor\], which Orca does not speak. The DateTimeEdit and DateRangeEdit parts are focused on named date editors ('Date date editor.', 'Start date date editor.'), so the two widgets disagree.
- **Platform:** Linux AT-SPI / Orca (measured); the name gap is platform-independent.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `2d0446fc` (dateedit). Fixed part: DateEdit/TimeEdit focused entry is named ('Date entry \_\_/\_\_/\_\_\_\_'.
- **Where:** crates/teksilo-widgets/src/date\_edit.rs:1081-1111
- **Evidence:**
  - `datetime census first walk: 'Tab 18 (Tab): focus=[entry] '' | said=['entry __/__/____']', 'Tab 20 (Tab): focus=[entry] '' | said=['entry __:__ __']', 'Tab 21 ... focus=[date editor] 'Date' | said=['Date date editor.']'`
  - `orca-debug.out (datetime-fields): '13:12:22.350764 - SPEECH OUTPUT: 'entry __/__/____'', '13:12:34.884106 - SPEECH OUTPUT: 'entry __:__ __''`
  - `tree: '[date editor] 'Date' {editable,focusable,...} attrs={'placeholder-text': 'Select a date'}' > '[entry] '' {editable,focusable,...} attrs={'placeholder-text': '__/__/____'}'`
  - `crates/teksilo-widgets/src/date_edit.rs:1081-1111 names the DateInput wrapper; the inner TextInput, which takes focus, is left unnamed`
  - `b2/catalog-b-datetime-fields-20260925-133344-3413888 orca-debug.out: '13:34:04.040779 - SPEECH OUTPUT: 'entry __/__/____''`
  - `c2/catalog-b-datetime-20260925-133926-3749370/tree-launch.txt:175-176 '[date editor] 'Date' … {'placeholder-text': 'Select a date'}' > '[entry] '' … {'placeholder-text': '__/__/____'}'`
- **Reproduced:** 4 of 4 runs (c2 and c4 datetime censuses, datetime-fields); structural
- **Verification:** confirmed. Reproduced: 2 of 2 datetime-fields (b1, b2); 2 of 2 datetime walks (c2 census, v1 datetime-reopen)
- **Fix idea:** Name the inner field (labelled\_by the DateInput, or copy its name), or focus the DateInput node itself, as the DateTimeEdit parts already do.

### catalog-b-12 {#catalog-b-12}

ProgressBar and Spinner announce their labels whenever their page opens, ahead of the tab name: cut on the first visit, dropped on later ones

- **Example:** widget-catalog
- **Scenario:** catalog-b-indicators-open, catalog-b-indicators, catalog-b-charts (Up to Indicators)
- **Act:** From the Inputs tab, press Down to open Indicators, then Up and Down again (4 visits).
- **The reader should get:** Opening the page reads the page tab. A progress indicator speaks when its progress changes, not every time it is shown. If Teksilo means them to be heard on appearing, they should at least not talk over the tab name.
- **The reader gets:** Both named indicators ('Loading…', the Spinner; '60 %', the determinate bar) are Live::Polite. Every adapter announces a named live node when it enters the tree, so each page open emits two object:announcement events in the same update as the focus move, and before it. Visit 1: 'Loading…' was cut in 3 of 3 runs, and '60 %' in 3 of 4 first opens (spoken once, in the charts census). Visits 2-4: dropped as defunct in 3 of 3 runs ('Ignoring defunct object: \[progress bar: 'Loading…'\]'), via catalog-b-01. On Windows and macOS, by source, both are spoken on every page open, and whenever a bar scrolls back into view. The same pattern re-announces the Password field's validation strip ('Use at least 8 characters') whenever the Text page reopens.
- **Platform:** Linux AT-SPI / Orca (measured); Windows UIA LiveRegionChanged in node\_added and macOS announcement on add, by source.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/progress\_bar.rs:296-315; crates/teksilo-widgets/src/spinner.rs:191-199
- **Evidence:**
  - `indicators-open 'Down to Indicators, visit 1': '+85.2 ms object:announcement [progress bar] 'Loading…' text='Loading…'', '+96.0 ms object:announcement [progress bar] '60 %' text='60 %'', '+99.5 ms object:state-changed:focused 1 [page tab] 'Indicators''; Orca: ''Loading…' was cut by a stop 144 ms in (estimated)'`
  - `visits 2-4: '12:58:19.000324 EVENT MANAGER: Ignoring defunct object: [progress bar: 'Loading…']', '12:58:19.015176 EVENT MANAGER: Ignoring defunct object: [progress bar: '60 %']'`
  - `text census 'Down back to the Text tab': ann=['Use at least 8 characters'] with obs announced-from-defunct / orca-dropped-defunct`
  - `crates/teksilo-widgets/src/progress_bar.rs:296-315 set_live(Live::Polite) in both states; crates/teksilo-widgets/src/spinner.rs:191-199 set_live(Live::Polite)`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:72-77; accesskit_windows-0.35.0/src/adapter.rs:255-262; accesskit_macos-0.27.0/src/event.rs:236-241`
  - `a1/catalog-b-indicators-open-20260925-132609-2993228 visit 1: '+56.6 ms object:announcement [progress bar] '60 %'', '+62.6 ms object:announcement [progress bar] 'Loading…'', '+63.5 ms object:state-changed:focused 1 [page tab] 'Indicators''; both cut`
  - `a2/catalog-b-indicators-open-20260925-132804-3085818: '13:28:22.037205 - EVENT MANAGER: Ignoring defunct object: [progress bar: 'Loading…']', '13:28:22.040191 - … [progress bar: '60 %']'`
- **Reproduced:** 3 of 3 indicators-open runs (visit 1 cut or partly cut; visits 2-4 dropped in all); 3 of 3 censuses
- **Verification:** confirmed. Reproduced: 3 of 3 indicators-open (a1, a2, a3); 1 of 1 indicators census; 1 of 1 charts census (Up to Indicators)
- **Fix idea:** Leave the ProgressBar and Spinner non-live by default, as ARIA does. Announce a change of state (started, completed, a coarse percentage) from the widget through ctx.announce, or make live opt-in.

### catalog-b-13 {#catalog-b-13}

Chart marks: the first arrow speaks the datum twice (announcement + focus), and every later press will too once K2 is fixed

- **Example:** widget-catalog
- **Scenario:** catalog-b-charts-marks
- **Act:** Tab to the first bar chart, press Right, then Right, Down, End, Home.
- **The reader should get:** Each mark is read once: 'Revenue, Q1: 41'.
- **The reader gets:** The chart moves the platform focus to the mark's synthetic node, and also ctx.announce()s the same text. First press, 3 of 3 runs: Orca said 'Revenue, Q1: 41' and then 'Revenue, Q1: 41 panel.'. Later presses were said once only because the K2 announcer node is defunct ('Ignoring defunct object: \[status bar: 'Revenue, Q1: 41'\]'). With the K2 fix, every arrow press would be read twice, or the announcement would be cut by the focus change in the same update. NVDA would get both a focus event and a LiveRegionChanged.
- **Platform:** Linux AT-SPI / Orca (measured); Windows and macOS by source (focus + live announcement).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-charts/src/hit.rs:600-630
- **Evidence:**
  - `charts-marks 'Right to the first mark': '+22.7 ms object:announcement [status bar] 'Revenue, Q1: 41' text='Revenue, Q1: 41'', '+24.1 ms object:state-changed:focused 1 [panel] 'Revenue, Q1: 41''; orca-debug.out '13:15:43.315190 - SPEECH OUTPUT: 'Revenue, Q1: 41'' then '13:15:43.372148 - SPEECH OUTPUT: 'Revenue, Q1: 41 panel.''; 'FAIL  Orca says 'Revenue, Q1: 41' once'`
  - `'Right to the next mark': '13:00:09.729544 EVENT MANAGER: Ignoring defunct object: [status bar: 'Revenue, Q1: 41']' (announcer path /org/a11y/atspi/accessible/0/18446744073709551616)`
  - `crates/teksilo-charts/src/hit.rs:626-629 ctx.announce(mark_description(m)) in drive_readout_keys, while bar_chart.rs:803-815 sets the mark as active descendant / focus`
  - `a1/catalog-b-charts-marks-20260925-132606-2990321 orca-debug.out: '13:26:17.983934 - SPEECH OUTPUT: 'Revenue, Q1: 41'', '13:26:18.028459 - SCRIPT UTILITIES: Not interrupting for locusOfFocus change: old locusOfFocus is ancestor with name of new locusOfFocus', '13:26:18.028536 - SPEECH OUTPUT: 'Revenue, Q1: 41 panel.''`
  - `v1/verify-catalog-b-line-marks-20260925-133510-3522035 'Right to the first point': said 'Revenue, Q1: 41' and 'Revenue, Q1: 41 panel.'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 charts-marks (a1, a2, a3); 1 of 1 line-marks (v1)
- **Fix idea:** Drop the ctx.announce on keyboard steps, since focus on the mark already reads it. Keep it only for the pointer and touch readout (hit.rs:512).

### catalog-b-14 {#catalog-b-14}

Pie/donut slices are named ', Storage: 18': a leading comma and no share, though the chart draws percentages

- **Example:** widget-catalog
- **Scenario:** catalog-b-charts-marks
- **Act:** Tab to the pie chart and press Right.
- **The reader should get:** 'Storage: 18, 12%' (category, value and the share the chart shows with show\_percentages(true)).
- **The reader gets:** ', Storage: 18 panel.'. A pie built from points has an empty series name, and mark\_description always prefixes the series with ', '. The percentage exists only in the painted label.
- **Platform:** Linux AT-SPI / Orca (measured); name is platform-independent.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-charts/src/hit.rs:381-383; pie\_chart.rs:835-850
- **Evidence:**
  - `charts-marks 'Right on the pie chart': '+43.2 ms object:state-changed:focused 1 [panel] ', Storage: 18''; orca-debug.out '13:16:14.882572 - SPEECH OUTPUT: ', Storage: 18 panel.''`
  - `'FAIL  the slice's name starts with its category, not a comma', 'FAIL  the slice's name carries its share, as the chart draws it' (3 of 3 runs)`
  - `crates/teksilo-charts/src/hit.rs:381-383 format!("{}, {}: {}", m.series_name, m.category_label, m.value); pie_chart.rs:845 series_name: view.name.to_string() (empty for ChartModel::from_points); pie_chart.rs:1037 paints format!("{} ({:.0}%)") for the visible label`
  - `a3/catalog-b-charts-marks-20260925-133202-3303740 orca-debug.out: '13:32:46.445076 - SPEECH OUTPUT: ', Storage: 18 panel.''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 charts-marks (a1, a2, a3)
- **Fix idea:** Omit an empty series name, and add the share to pie mark names when percentages are shown.

### catalog-b-15 {#catalog-b-15}

Charts' names describe only their shape: the two bar charts are indistinguishable, the axes and subject are missing, and the legend is an empty list

- **Example:** widget-catalog
- **Scenario:** catalog-b-charts, catalog-b-charts-marks
- **Act:** Tab through the Charts page and read the tree.
- **The reader should get:** Each chart is named for what it shows (a title or axis labels such as 'USD (k) by Quarter'), so the two bar charts differ. The legend either lists its series or is not exposed.
- **The reader gets:** Both bar charts are 'Bar chart: 3 series, 4 categories', and Orca says the identical name at two consecutive Tab stops. The y axis 'USD (k)' and the x axis 'Quarter' reach no node. Each chart has a '\[list\] 'Chart legend'' with no items. The non-interactive legend paints its rows itself, so the list is empty.
- **Platform:** Linux AT-SPI / Orca (measured); platform-independent.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-charts/src/bar\_chart.rs:775-816; legend.rs:638-645
- **Evidence:**
  - `charts census first walk: 'Tab 4 ... said=['Bar chart: 3 series, 4 categories document frame.']', 'Tab 5 ... said=['Bar chart: 3 series, 4 categories document frame.']'`
  - `charts-marks 'Tab to the second bar chart': 'FAIL  the second bar chart's name tells it from the first' (3 of 3 runs)`
  - `tree: '[document frame] 'Bar chart: 3 series, 4 categories' {focusable}' ... '[list] 'Chart legend'' with no children`
  - `crates/teksilo-charts/src/bar_chart.rs:783-786 name built from counts only, with no title API (tools/extract_widget_api.py BarChart lists none); legend.rs:638-645 Role::List + children() = row_ids (empty unless interactive)`
  - `example: examples/widget_catalog/src/tabs/charts.rs:190-199 sets no .access_label on either chart`
  - `c1/catalog-b-charts-20260925-133718-3643862 tree-launch.txt: '[document frame] 'Bar chart: 3 series, 4 categories' {focusable}' twice, each ending with '[list] 'Chart legend'' with no children`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 charts-marks (a1, a2, a3); 1 of 1 charts census (c1)
- **Fix idea:** Build the default name from the axis labels, and offer a title setter. Have the example label each chart. Hide the legend list when it has no rows, or emit its rows as list items.

### catalog-b-16 {#catalog-b-16}

Wrapped text fields announce twice: HexColorInput and SearchField nest a named entry inside a second entry

- **Example:** widget-catalog
- **Scenario:** catalog-b-color, catalog-b-text, catalog-b-text-fields
- **Act:** Tab to 'Brand color' (HexColorInput), to the picker's 'Hex', and to the SearchField.
- **The reader should get:** One entry, read once.
- **The reader gets:** Orca says 'Brand color entry #CC6633' and then 'Brand color entry #CC6633 selected.' (the same for 'Hex'). The SearchField gives 'entry' and then 'entry Type a fruit — Apple, Banana, …'. In both, a non-focusable wrapper with an entry role (TextInput / SearchInput) holds the focusable entry, and Orca reads the ancestor as context. The SearchField is also unnamed (see catalog-b-18).
- **Platform:** Linux AT-SPI / Orca (measured).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/hex\_color\_input.rs:599-620; search\_field.rs:772-805
- **Evidence:**
  - `color census 'Tab 4': said=['Brand color entry #CC6633', 'Brand color entry #CC6633 selected.']; 'Tab 10': said=['Hex entry #8844BBFF', 'Hex entry #8844BBFF selected.']`
  - `text census 'Tab 7': focus=[entry] '' said=['entry', 'entry Type a fruit — Apple, Banana, …']`
  - `tree: '[entry] 'Brand color' {editable,selectable-text,single-line}' > '[entry] 'Brand color' {editable,focusable,...}'`
  - `crates/teksilo-widgets/src/hex_color_input.rs:599-609 wrapper sets Role::TextInput + name + value; crates/teksilo-widgets/src/search_field.rs:772-776 wrapper sets Role::SearchInput`
  - `c1/catalog-b-color-20260925-133721-3644770 first walk Tab 4: said=['Brand color entry #CC6633', 'Brand color entry #CC6633 selected.']`
  - `b2/catalog-b-text-fields-20260925-133350-3421224 'type 'ap' into the SearchField': '+41.1 ms object:children-changed:add [entry] '' -> [list box] ''' … 'FAIL Orca says one of ['Apple', 'Apricot', 'suggestion', 'result'] / Orca said nothing'`
- **Reproduced:** 4 of 4 runs (c1, c5 color; c1, c4 text)
- **Verification:** confirmed. Reproduced: color: 1 census + 3 color-channels + 3 color-fr; SearchField: 1 text census (c1), 1 text-fields (b2), 2 reopen-unmet walks
- **Fix idea:** Give the wrapper a non-text role (or GenericContainer), and move the role, name, value, popup and active\_descendant onto the focused inner field.

### catalog-b-17 {#catalog-b-17}

Content below the fold is missing from the accessibility tree until it is scrolled into view

- **Example:** widget-catalog
- **Scenario:** catalog-b-charts (launch tree), catalog-b-charts-marks
- **Act:** Launch on the Charts tab and read the tree; then Tab down to the donut.
- **The reader should get:** A reader exploring by object navigation or structure finds every chart on the page, not only the ones on screen.
- **The reader gets:** At launch the tree ends with '\[label\] 'PieChart (donut + center slot)'' and holds no pie chart node. The donut is added only when Tab scrolls it into view ('children-changed:add ... \[document frame\] 'Pie chart: 5 slices''). Charts that scroll off the top are removed ('defunct 1 \[document frame\] 'Bar chart: 3 series, 4 categories''). ScrollArea marks clips\_children, and accesskit\_consumer's common\_filter excludes an off-screen child unless it is next to one in view. This behaviour is also what turns a scroll into catalog-b-01.
- **Platform:** Linux AT-SPI (measured). The consumer filter is shared, so by source the same on Windows and macOS.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/scroll\_area.rs:1303-1306
- **Evidence:**
  - `tree-charts launch (v2/tree-charts/.../tree-launch.txt): the page ends '[label] 'PieChart (donut + center slot)'' then '[status bar] 'Status''`
  - `charts census 'Tab 7': '+69.9 ms object:children-changed:add [panel] '' -> [document frame] 'Pie chart: 5 slices''`
  - `crates/teksilo-widgets/src/scroll_area.rs:1305 set_clips_children(); accesskit_consumer-0.39.0/src/filters.rs:64-88`
  - `c1/catalog-b-charts-20260925-133718-3643862/tree-launch.txt: the page ends '[label] 'PieChart (donut + center slot)'' then '[status bar] 'Status''; report.txt: '+38.3 ms object:children-changed:add [panel] '' -> [document frame] 'Pie chart: 5 slices''`
- **Reproduced:** 2 of 2 launches on Charts (tree-charts, c3 census)
- **Verification:** confirmed. Reproduced: 1 of 1 charts census launch (c1) plus 3 of 3 charts-marks runs (the pie is added on Tab, the bar charts removed); deterministic for a given scroll position
- **Fix idea:** Do not export clips\_children for ScrollArea, or at least keep off-screen subtrees with an offscreen state rather than removing them. This is AccessKit's upstream behaviour, but the choice to hand it clips\_children is Teksilo's.

### catalog-b-18 {#catalog-b-18}

Unnamed controls and indistinguishable duplicates on the catalog pages (the example labels nothing)

- **Example:** widget-catalog
- **Scenario:** catalog-b-text, catalog-b-scene, catalog-b-menus, catalog-b-indicators, catalog-b-datetime, catalog-b-richtext
- **Act:** Tab walks and launch trees of each tab.
- **The reader should get:** Every focusable control has a name, and repeated widgets are told apart. The visible section headings ('SpinBox', 'SearchField', 'BarChart (ChartStyle override …)', 'Calendar — date range') are what a sighted user uses to tell them apart.
- **The reader gets:** Text: '0.00 spin button.' for the SpinBox, and 'entry … Type a fruit' for the SearchField. Scene: 'panel.'. Menus: two unnamed \[menu bar\] nodes and an unnamed standalone \[menu\] ('menu.'). Rich Text: the editor and viewer have no name. Indicators: four unnamed progress indicators (indeterminate bar, vertical 40% bar, two spinners) and a determinate bar whose name is its value text '60 %'. Date & Time: two calendars both named 'Calendar, September 2026'. Charts: two identical bar chart names. The catalog's section() helper draws the heading as a separate TextWidget and never associates it with the control (no labelled\_by / access\_label).
- **Platform:** Linux AT-SPI / Orca (measured); platform-independent.
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/shared.rs:261-277; examples/widget\_catalog/src/tabs/text.rs:103-113
- **Evidence:**
  - `text census 'Tab 6 (Tab): focus=[spin button] '' | said=['0.00 spin button.']'`
  - `scene census 'Tab 4 (Tab): focus=[panel] '' | said=['panel.', 'A real Button at scene coordinates.']'`
  - `menus census 'Tab 9 (Tab): focus=[menu] '' | said=['menu.']'; tree '[menu bar] ''' x2`
  - `indicators tree: '[progress bar] '' {indeterminate}', '[progress bar] '' value={'current': 0.4000000059604645, ...}', '[progress bar] '60 %' value={'current': 0.6000000238418579, 'minimum': 0.0, 'maximum': 1.0 ...}'`
  - `datetime tree: '[table] 'Calendar, September 2026' {focusable}' twice`
  - `examples/widget_catalog/src/shared.rs:261-277 section() adds TextWidget::new(title) beside body with no association; examples/widget_catalog/src/tabs/text.rs:98-104 SpinBox::new(...) with no .label; indicators.rs:33-57; scene.rs:125-127; richtext.rs:82-99; menus.rs:114-121`
  - `c1/catalog-b-indicators-20260925-133727-3647697/tree-launch.txt:45-53 '[progress bar] '60 %'', '[progress bar] '' {indeterminate}', '[progress bar] '' value={'current': 0.4…}', '[progress bar] '' {indeterminate}' x2`
  - `c2/catalog-b-datetime-20260925-133926-3749370/tree-launch.txt:45,110 '[table] 'Calendar, September 2026' {focusable}' twice`
- **Reproduced:** every census run (structural)
- **Verification:** confirmed. Reproduced: every census (c1 charts/color/text/indicators, c2 menus/scene/richtext/datetime); structural
- **Fix idea:** In section(), label the body by its heading (labelled\_by / access\_label) and use a heading role for the heading. Name the SpinBox, SearchField, SceneView, editors, MenuList and progress bars. On the framework side, consider default names for SceneView, MenuList and RichTextEditor, and a debug\_assert like Avatar's for an unnamed ProgressBar/Spinner.

### catalog-b-19 {#catalog-b-19}

A disabled menu item is exposed as enabled, and a menu item's shortcut never reaches AT-SPI

- **Example:** widget-catalog
- **Scenario:** catalog-b-menus-open
- **Act:** Read the standalone MenuItems 'With shortcut' (shortcut\_label 'Ctrl+S') and 'Disabled item' (enabled(false)).
- **The reader should get:** 'Disabled item' is not enabled or sensitive (Orca: 'grayed'), and 'With shortcut' carries Ctrl+S (action key binding).
- **The reader gets:** 'Disabled item' has states \['enabled', 'sensitive', 'showing', 'visible'\]. Teksilo sets disabled (the walker calls set\_disabled from the arena), but accesskit\_atspi\_common inserts Enabled\|Sensitive for every role outside its read-only list, so a disabled Button, MenuItem, Link or Tab is reported enabled. 'With shortcut' has actions=\[{'name': 'click', 'description': '', 'key\_binding': ''}\]: Teksilo sets keyboard\_shortcut, and atspi\_common always exports an empty key binding.
- **Platform:** Linux AT-SPI (measured). On Windows and macOS, by source, the adapters do carry the disabled state.
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0/src/node.rs:376-380, 1097
- **Evidence:**
  - `menus-open 'look at the standalone items': 'FAIL  'Disabled item' is exposed as disabled / [menu item] 'Disabled item': states=['enabled', 'sensitive', 'showing', 'visible']'`
  - `'FAIL  'With shortcut' carries its Ctrl+S / [menu item] 'With shortcut': actions=[{'name': 'click', 'description': '', 'key_binding': ''}] desc=None attrs=None' (2 of 2 runs)`
  - `accesskit_atspi_common-0.20.0/src/node.rs:376-380 'if state.is_read_only_supported() && state.is_read_only_or_disabled() { ReadOnly } else { Enabled | Sensitive }'; accesskit_consumer-0.39.0/src/node.rs:861-879 read-only-supported roles exclude Button/MenuItem/Link; atspi node.rs:1097 key_binding: ""`
  - `crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:584-588 set_disabled from the arena; crates/teksilo-widgets/src/menu_item/widget_impl.rs:39 enabled_when, :994-996 set_keyboard_shortcut`
  - `c1/verify-catalog-b-menu-tree-20260925-133730-3653210: '[menu item] 'Disabled item' states=['enabled', 'sensitive', 'showing', 'visible']'`
- **Reproduced:** 2 of 2 runs
- **Verification:** confirmed. Reproduced: 2 of 2 menus-open (b1, b2); 1 of 1 verify-menu-tree
- **Fix idea:** Upstream: have accesskit\_atspi\_common drop Enabled/Sensitive for a disabled node whatever its role, and export keyboard\_shortcut as the action key binding. Until then, Teksilo could append the shortcut to the description on Linux.

### catalog-b-20 {#catalog-b-20}

The colour of the ColorEdit trigger and of the picker's 'Selected color' well never reaches the reader

- **Example:** widget-catalog
- **Scenario:** catalog-b-color, catalog-b-color-channels
- **Act:** Tab to 'Theme accent' (ColorEdit) and to 'Selected color' (ColorWell).
- **The reader should get:** The reader hears the colour each holds (#55AADD, #8844BB).
- **The reader gets:** 'Theme accent push button.' and 'Selected color push button.', with no colour. A ColorEdit given .label(...) replaces the hex text entirely, and the visible swatch is not exposed. The ColorWell carries the hex as a string value, which accesskit\_atspi\_common drops for a push button.
- **Platform:** Linux AT-SPI / Orca (measured). By source, Windows would expose the ColorWell value through UIA Value.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_edit.rs:457-467; color\_picker/swatch.rs:363-381
- **Evidence:**
  - `color census 'Tab 5': said=['Theme accent push button.']; 'Tab 9': said=['Selected color push button.']`
  - `color-channels 'Tab to the ColorEdit': 'FAIL  Orca says one of ['55AADD', '#55']'`
  - `crates/teksilo-widgets/src/color_edit.rs:457-467 ('App-supplied .label(...) replaces the entire visible text (and therefore the AT name)'); color_picker/swatch.rs:363-381 name + set_value(hex); accesskit_atspi_common-0.20.0/src/node.rs:37-43`
  - `c1/catalog-b-color-20260925-133721-3644770: Tab 5 said=['Theme accent push button.'], Tab 9 said=['Selected color push button.']`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 color-channels (a1, a2, a3); 1 of 1 color census
- **Fix idea:** Keep the hex (or a colour name) in the name or description when a label is given, and put the ColorWell's hex in its name or description on AT-SPI.

### catalog-b-21 {#catalog-b-21}

Decorative chrome reaches AT: an unnamed separator after every SpinBox, and an empty status bar after every text field

- **Example:** widget-catalog
- **Scenario:** catalog-b-color, catalog-b-text
- **Act:** Read the tree of the Color and Text pages.
- **The reader should get:** Only meaningful nodes.
- **The reader gets:** '\[separator\] ''' follows each of the 7 picker spin boxes, from the SpinBox recipe's vertical Divider. '\[status bar\] ''' (an empty validation strip) follows Username, Read-only field, Password, FilePicker, SearchField, Hex and Brand color. A reader walking objects meets 'separator' and 'status bar' with nothing in them.
- **Platform:** Linux AT-SPI (measured).
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/styles/recipe\_spin\_box\_style.rs:76
- **Evidence:**
  - `color tree: '[spin button] '' ... value={'current': 136.0, ...}' then '[separator] '''`
  - `text tree: '[entry] 'Username' ...' then '[status bar] '''`
  - `crates/teksilo-widgets/src/styles/recipe_spin_box_style.rs:76 Divider::vertical() inside the SpinBox chrome`
  - `a3/catalog-b-color-fr-20260925-133212-3311510 tree: '[spin button] '' … value={'current': 136.0 …}' then '[separator] '''; '[entry] 'Hex'' > '[entry] 'Hex'', '[status bar] '''`
- **Reproduced:** every color and text census (structural)
- **Verification:** confirmed. Reproduced: every color/text tree (c1, a3/v2/v3 color-fr); structural
- **Fix idea:** Hide decorative dividers from AT (access\_hidden), and keep an empty validation strip out of the tree until it holds a message.

### catalog-b-M1 {#catalog-b-m1}

ColorEdit's popover is an unnamed dialog, and opening it replays the ColorPicker's live flood each time

- **Example:** widget-catalog
- **Scenario:** verify-catalog-b-coloredit-popover
- **Act:** Color tab: Tab to 'Theme accent' (ColorEdit), press Space, Tab twice inside, Escape (verify-catalog-b-coloredit-popover).
- **The reader should get:** The popover is announced as a dialog named for what it edits (e.g. 'Theme accent, dialog'), then the control that has focus. Nothing else talks over it.
- **The reader gets:** 8 object:announcement events (Color picker, Hex, Cancel, Hue, Hex, Saturation and brightness, Done, Opacity) reach the bus just ahead of the focus move, and Orca speaks all 8 and cuts them. Then it says 'dialog' with no name, 'Color picker panel.' and 'Saturation and brightness panel.'. The tree shows '\[dialog\] '' {active}'. The popover holds a ColorPicker, whose root is Live::Polite (catalog-b-06), so every named descendant, 'Cancel' and 'Done' included, is announced each time the popover opens. The PopoverButton's dialog name comes from surface\_name, which defaults to empty and which ColorEdit never sets.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows by source: WindowOpened on the unnamed dialog (accesskit\_windows adapter.rs:248-255) and LiveRegionChanged for each named live descendant (:256-262); macOS announces each on add (event.rs:236-241).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_edit.rs:475-478; crates/teksilo-widgets/src/popover\_widget.rs:305-307, 505-512
- **Evidence:**
  - `v1/verify-catalog-b-coloredit-popover-20260925-133513-3527996 report.txt 'Space opens its popover': '+86.9 ms object:announcement [panel] 'Color picker'' … '+93.3 ms object:announcement [push button] 'Done'', '+94.0 ms object:announcement [slider] 'Opacity'', '+94.8 ms object:children-changed:add [panel] '' -> [dialog] ''', '+95.7 ms object:state-changed:focused 1 [panel] 'Saturation and brightness''`
  - `same run orca-debug.out: '13:35:29.483073 - SPEECH OUTPUT: 'Cancel'', '13:35:29.602196 - NULL SPEECH: stop', '13:35:29.602274 - SPEECH OUTPUT: 'dialog'', '13:35:29.602305 - SPEECH OUTPUT: 'Color picker panel.''`
  - `tree-Space-opens-its-popover.txt: '[dialog] '' {active}' > '[panel] 'Color picker' {focusable}' > … '[push button] 'Cancel'', '[push button] 'Done''`
  - `v2 (…-134223-3836388) '13:42:39.616147 - SPEECH OUTPUT: 'dialog''; v3 (…-134316-3864100) '13:43:32.569075 - SPEECH OUTPUT: 'dialog''`
  - `crates/teksilo-widgets/src/color_edit.rs:475-478 PopoverButton::new(trigger).content(picker).placement(..).dismiss_behavior(..) with no surface_name; crates/teksilo-widgets/src/popover_widget.rs:305-307 and 505-512 (surface_name 'Empty by default')`
- **Reproduced:** 3 of 3 runs (v1, v2, v3)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Have ColorEdit pass its label (or 'Color picker') as the popover's surface\_name, or have PopoverButton label its dialog by the trigger (labelled\_by) whenever no name is given. Removing the live flag from the ColorPicker root (catalog-b-06) removes the flood here too.

### catalog-b-M2 {#catalog-b-m2}

The rich-text editor and viewer are silent on every return visit, even on the same page

- **Example:** widget-catalog
- **Scenario:** verify-catalog-b-richtext-tab
- **Act:** Rich Text tab: Tab into the editor, Ctrl+Tab to the viewer, Ctrl+Shift+Tab back to the editor, Ctrl+Tab to the viewer again (verify-catalog-b-richtext-tab).
- **The reader should get:** Coming back to the editor or the viewer reads it again, as the first visit did.
- **The reader gets:** The first visit says 'section.' (catalog-b-02). Every later visit says nothing. The focused node is the wrapper, a Role::GenericContainer, which the consumer's filter keeps only while it is focused (filters.rs:18-20 then 31-33). When focus leaves it, it is removed: accesskit\_atspi\_common emits defunct=1 for its path (adapter.rs:91-106). When focus comes back, the same NodeId returns, libatspi still holds it as defunct, and Orca drops the focus event ('Ignoring defunct object: \[section\]'). This is the catalog-b-01 mechanism, triggered by focus alone, with no page switch or scroll. The census shows it too: the viewer's focus event at 'first walk, Tab 6' is already an orca-dropped-defunct act.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows and macOS have no defunct state (by source), but there too the focused element is the role-less wrapper.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: verify-catalog-b-richtext-tab had 2 failed checks and 6 defunct drops in the sweep, none now.
- **Where:** crates/teksilo-widgets/src/rich\_text.rs:3853, 4281-4291
- **Evidence:**
  - `v1/verify-catalog-b-richtext-tab-20260925-133507-3514799 'Ctrl+Shift+Tab back to the editor': '+49.2 ms object:state-changed:defunct 1 [section] ''', '+49.6 ms object:state-changed:focused 1 [section] ''', 'FAIL Orca says something / Orca said nothing'; orca-debug.out '13:35:35.093460 - EVENT MANAGER: Ignoring defunct object: [section]'`
  - `same run 'Ctrl+Tab to the viewer again': '+38.2 ms object:state-changed:defunct 1 [section] ''', '+38.5 ms object:state-changed:focused 1 [section] ''', Orca said nothing; '13:35:39.334951 - EVENT MANAGER: Ignoring defunct object: [section]'`
  - `v2 (…-134226-3837054) '13:42:53.907518 - EVENT MANAGER: Ignoring defunct object: [section]'; v3 (…-134322-3865696) '13:43:49.706982 - EVENT MANAGER: Ignoring defunct object: [section]', both acts silent`
  - `crates/teksilo-widgets/src/rich_text.rs:3853 (.focusable(true) on the wrapper), rich_text.rs:4281-4291 (Role::GenericContainer); accesskit_consumer-0.39.0/src/filters.rs:17-34`
- **Reproduced:** 3 of 3 runs (v1, v2, v3); also in the c2 richtext census
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** The same fix as catalog-b-02: put platform focus on the body node (MultilineTextInput / Document), which the filter never drops, or give the focused wrapper a real role. A fresh NodeId on re-entry (the catalog-b-01 fix) would also end the silence, but not the 'section'.

### catalog-b-M3 {#catalog-b-m3}

libatspi rejects every AddAccessible/RemoveAccessible cache signal AccessKit sends (wrong D-Bus signature), so a libatspi client never learns a removed node came back

- **Example:** widget-catalog
- **Scenario:** catalog-b-text-return (any)
- **Act:** Any run; seen in every one (e.g. catalog-b-text-return).
- **The reader should get:** When AccessKit adds or removes a node it emits org.a11y.atspi.Cache AddAccessible / RemoveAccessible with the signature libatspi parses. A client's cached record of an object is then replaced when the object comes back.
- **The reader gets:** The listener's libatspi (the same libatspi 2.52 Orca uses) logs 'AddAccessible with unknown signature (so)(so)(so)iiassusau' and 'Unknown signature so for RemoveAccessible' hundreds of times per run and discards the signals. libatspi.so.0 contains the signatures it does accept: '((so)(so)(so)iiassusau)' and '((so)(so)(so)a(so)assusau)', which wrap the body in one struct. accesskit\_unix 0.23.0 emits the cache item through zbus emit\_signal as the body itself (bus.rs:434-473), which zbus sends flattened into separate arguments. So the only thing a libatspi client learns when a node leaves is the StateChanged(Defunct,true) event. When the same path comes back, nothing refreshes its cached state, and the defunct bit stays (the listener's own tree shows it: '\[scroll pane\] 'Text' {defunct}'). This is the platform half of catalog-b-01 and K2. I read what libatspi does on a parsed AddAccessible from its behaviour, not its source (not installed), so I claim only that the refresh signal never lands.
- **Platform:** Linux AT-SPI (measured: every run's stdout). Not applicable to Windows or macOS.
- **Severity:** medium; **layer:** upstream
- **Status:** Open. In the node-ids fix topic, not fixed there: Upstream: accesskit\_unix 0.23 emits the org.a11y.atspi.Cache AddAccessible/RemoveAccessible body flattened, and libatspi rejects the signature. As instructed, I did not patch AccessKit. New ids on re-entry make the stale libatspi cache irrelevant for this defect.
- **Where:** accesskit\_unix-0.23.0/src/atspi/bus.rs:434-473
- **Evidence:**
  - `a1-catalog-b-text-return.stdout (354 such lines from the listener, process 2989740): '(process:2989740): dbind-WARNING **: 13:26:42.657: AT-SPI: Unknown signature so for RemoveAccessible'; a1-catalog-b-charts-marks.stdout: '(process:2992029): dbind-WARNING **: 13:26:49.313: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau'`
  - `strings /lib/x86_64-linux-gnu/libatspi.so.0: '((so)(so)(so)a(so)assusau)', '((so)(so)(so)iiassusau)', 'AT-SPI: AddAccessible with unknown signature %s', 'AT-SPI: Unknown signature %s for RemoveAccessible'`
  - `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/accesskit_unix-0.23.0/src/atspi/bus.rs:434-473 (emit_cache_add / emit_cache_remove -> emit_cache_signal(signal_name, body) with the CacheItem / ObjectRef as the whole body); Cargo.lock pins accesskit_unix 0.23.0`
  - `c1/catalog-b-text-20260925-133724-3645771/tree-Down-back-to-the-Text-tab.txt: '[scroll pane] 'Text' {defunct}', '[panel] '' {defunct}'`
- **Reproduced:** every run (46 of 46 runs' stdout carry the warnings)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Upstream: emit the cache signals with the body wrapped in a one-element tuple, so the signature is '((so)(so)(so)iiassusau)' / '((so))'. Teksilo cannot fix it, but it does not need to: a fresh NodeId on re-entry (the K2 approach) avoids the stale cache entirely.
