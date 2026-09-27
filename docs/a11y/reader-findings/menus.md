<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Generated from the sweep of 25 and 26 September 2026 (on 261a218f) and its re-measure of 27 September 2026 (on c198e4d1). See ../reader-findings.md. -->

# Menus and drop-downs

Examples: `menus-and-dropdowns`.
22 findings: 2 critical, 8 high, 5 medium, 7 low.
Swept on `261a218f` on 25 and 26 September 2026, measured again on
`c198e4d1` on 27 September 2026.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [menus-01](#menus-01) | menus-and-dropdowns | Moving through an open menu is silent: the highlighted item is never exported | low | Linux | partly fixed |
| [menus-02](#menus-02) | menus-and-dropdowns | A menu-bar menu opened a second time is dead to Orca (defunct), with stale check states | high | Linux | fixed |
| [menus-03](#menus-03) | menus-and-dropdowns | A submenu opened from the keyboard closes itself about 165 ms later | critical | all | fixed |
| [menus-04](#menus-04) | menus-and-dropdowns | A combo box list opened a second time is dead to Orca: the options and the value go unspoken | high | Linux | fixed |
| [menus-05](#menus-05) | menus-and-dropdowns | A control scrolled out of view and back can be dead to Orca | high | Linux | fixed |
| [menus-06](#menus-06) | menus-and-dropdowns | A combo box's selected value never reaches AT-SPI, so Orca never says it | high | Linux | partly fixed |
| [menus-07](#menus-07) | menus-and-dropdowns | The 10 000-item combo list says nothing on any arrow: options nested inside ListView rows, selection invisible to AT-SPI | high | Linux | fixed |
| [menus-08](#menus-08) | menus-and-dropdowns | Menus are unnamed: opening File, Edit or View is announced as just 'menu.' | medium | Linux | fixed |
| [menus-09](#menus-09) | menus-and-dropdowns | Escape does not leave the menu bar, and after a command focus stays on the menu bar | medium | all | open |
| [menus-10](#menus-10) | menus-and-dropdowns | Escape after moving between menus with Left or Right drops focus on the window | medium | all | open |
| [menus-11](#menus-11) | menus-and-dropdowns | Both context menus are unreachable by keyboard and by screen reader | critical | Linux | open (example) |
| [menus-12](#menus-12) | menus-and-dropdowns | Disabled menu items are exported as enabled and sensitive on AT-SPI | high | Linux | fixed |
| [menus-13](#menus-13) | menus-and-dropdowns | Radio (and other) menu items export no position and no group: 'Dark Theme, 2 of 3' is impossible | low | Linux | partly fixed |
| [menus-14](#menus-14) | menus-and-dropdowns | The first item is skipped: Down from an empty combo box picks the second fruit, and menu type-ahead from no highlight lands on the second match | low | all | partly fixed |
| [menus-15](#menus-15) | menus-and-dropdowns | The Opacity slider inside the View options menu ignores the arrow keys, and is read as '0.6499999761581421' | high | Linux | fixed |
| [menus-16](#menus-16) | menus-and-dropdowns | Five combo boxes have no name, and their placeholders are never spoken | high | Linux | open (example) |
| [menus-17](#menus-17) | menus-and-dropdowns | No 'has popup' and no 'expanded/collapsed' on AT-SPI: menu-bar items, the Add button and combo boxes give no cue that they open anything | medium | Linux | upstream |
| [menus-18](#menus-18) | menus-and-dropdowns | The searchable combo's search field is unnamed and filtering is silent | medium | Linux | partly fixed |
| [menus-19](#menus-19) | menus-and-dropdowns | The combo box's controller-for relation points at an unnamed role-less node, which sits between the combo box and its list | low | Linux | fixed |
| [menus-v1](#menus-v1) | menus-and-dropdowns | Every menu-bar trigger is its own Tab stop, so Tab walks File, Edit and View before reaching content | low | all | open |
| [menus-v2](#menus-v2) | menus-and-dropdowns | The searchable combo box's search field placeholder is an English literal in framework code | low | all | fixed |
| [menus-v3](#menus-v3) | menus-and-dropdowns | The first character typed into the combo's search field is never reported as inserted text | low | Linux | fixed |

### menus-01 {#menus-01}

Moving through an open menu is silent: the highlighted item is never exported

- **Example:** menus-and-dropdowns
- **Scenario:** menus-menubar-f10, menus-view-items, menus-popover-typeahead, menus-context-mouse, menus-submenu
- **Act:** Open any MenuList: the File menu with F10 then Down, the View menu with Alt+V, the Add popover menu with Enter, or a context menu. Then press Down, Down, End, or type-ahead letters.
- **The reader should get:** Each move puts the highlighted item in front of the reader, through focus or an active descendant, and Orca says the item, for example 'Open', 'Save', 'Quit', 'Word Wrap check menu item checked'.
- **The reader got (`261a218f`):** Opening says only 'menu.'. Every arrow, Home, End and type-ahead press emits no event on the bus and Orca says nothing. A reader cannot tell which command Enter will run. AT-SPI grab\_focus on an item does nothing, because menu items are not focusable. The only way to reach an item is AT-SPI 'click', which runs it blind.
- **Platform:** Linux AT-SPI / Orca 46.1, measured. Windows and macOS by source: the AccessKit tree Teksilo builds has no focus change and no active\_descendant for a highlight move, so UIA and NSAccessibility receive nothing either.
- **Severity:** low; **layer:** framework
- **Severity in the sweep:** critical. The keyboard path now speaks every item before Enter runs it. What remains, an assistive technology's request to focus a menu item, has the arrows as a way round.
- **Status:** Partly fixed by `9636094c` (menus). What remains is under **Now**.
- **Now (`c198e4d1`):** Each Down, End and type-ahead press in an open menu now moves the reader's focus to the highlighted item, and Orca says it ('New.', 'Open.', 'Quit.', 'Word Wrap check menu item checked.'). What remains: an AT-SPI grab\_focus on a menu item still does nothing, so a screen reader cannot put focus on an item by itself.
- **Measured again:** menus-menubar-f10, menus-view-items, menus-popover-typeahead, menus-context-mouse, menus-at-actions, fix-menus-menubar, fix-menus-view-items: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-menubar-f10 'Down: highlight to Open': +13.9 ms object:state-changed:focused 1 [menu item] 'New' / +51.0 ms ORCA SAYS: 'New.'`
  - `pass2 menus-menubar-f10 'End: highlight to Quit': +15.4 ms object:state-changed:focused 1 [menu item] 'Quit' / +63.9 ms ORCA SAYS: 'Quit.'`
  - `pass1 menus-view-items 'Down: Word Wrap': +16.1 ms object:state-changed:focused 1 [check menu item] 'Word Wrap' / +44.8 ms ORCA SAYS: 'Word Wrap check menu item checked.'`
  - `pass1 menus-popover-typeahead 'type-ahead 'n'': +16.4 ms object:state-changed:focused 1 [menu item] 'New folder' / +77.3 ms ORCA SAYS: 'New folder.'`
  - `pass2 menus-context-mouse 'Down': +17.3 ms object:state-changed:focused 1 [menu item] 'Undo' / +67.0 ms ORCA SAYS: 'Undo.'`
  - `pass1 menus-at-actions 'AT-SPI grab_focus on 'Paste' in the open menu': FAIL focus lands on [menu item] 'Paste' / FAIL Orca says 'Paste' (no event in the act)`
  - `pass2 menus-at-actions 'AT-SPI grab_focus on 'Paste' in the open menu': FAIL focus lands on [menu item] 'Paste' / FAIL Orca says 'Paste'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_list.rs:878-955 (arrows, Home, End and page keys move the highlight), 1053-1071 (type-ahead), 1121-1156 (accessibility: Role::Menu, labelled\_by, size\_of\_set, active\_descendant on the highlighted row), 222-229 (wrapper a11y)
- **Evidence (`261a218f`):**
  - `menus-menubar-f10 report: '== Down: highlight to Open' -> 'FAIL  focus lands on [*] 'Open'' / 'no focus change on the bus in this act' / 'FAIL  Orca says 'Open'' / 'Orca unheard: 'Open''`
  - `same for '== Down: highlight to Save' and '== End: highlight to Quit' (3 of 3 runs)`
  - `orca-debug.out (menus-menubar-f10-20260925-130500-2201897): '13:05:13.153598 - SPEECH OUTPUT: 'menu.'' is followed by no event at all until '13:05:28.725895 - EVENT MANAGER: object:children-changed:remove for [frame] in [application: 'menus-and-dropdowns'] (-1, 0, [menu])' (Escape), across Down, Down and End`
  - `menus-view-items: 'FAIL  Orca says 'Word Wrap'', 'FAIL  Orca says 'partially checked'', 'FAIL  Orca says 'Dark Theme'' (3 of 3)`
  - `menus-at-actions: '== AT-SPI grab_focus on 'Paste' in the open menu' -> 'FAIL  focus lands on [menu item] 'Paste'' / 'no focus change on the bus in this act' (2 of 2)`
  - `crates/teksilo-widgets/src/menu_list.rs:748-760 (ArrowDown only sets focused_index, shows the highlight tooltip, reveals); 781-795 (Home/End); 926-940 (type-ahead); 229-231 doc comment: navigation 'moves focused_index, not real tree focus'; 992: accessibility sets Role::Menu and nothing else`
  - `Orca would follow it: /usr/lib/python3/dist-packages/orca/scripts/default.py:1627-1632 (onFocusedChanged reads a focused container's first selected child), and Orca handles object:active-descendant-changed`
  - `target/reader-verify/menus/menus-menubar-f10-20260925-132448-2964636/run.json: acts 'Down: highlight to Open', 'Down: highlight to Save' and 'End: highlight to Quit' each hold 0 events`
  - `menus-at-actions (3 runs): act 'AT-SPI grab_focus on 'Paste'' error=None, events=['harness:grab-focus'] only`
- **Reproduced:** deterministic; menubar-f10 3/3, view-items 3/3, popover-typeahead 3/3, context-mouse 2/2, submenu 3/3
- **Verification:** confirmed. Reproduced: deterministic: menubar-f10 3/3, view-items 3/3, popover-typeahead 3/3, context-mouse 3/3 (Down: 'Undo' unheard), at-actions grab\_focus 3/3
- **Fix idea:** Publish the highlight: set active\_descendant on the MenuList node to the highlighted MenuItem (Orca, NVDA and VoiceOver all follow it), or move real focus onto the item. Either way, make items reachable by grab\_focus (the Focus action).

### menus-02 {#menus-02}

A menu-bar menu opened a second time is dead to Orca (defunct), with stale check states

- **Example:** menus-and-dropdowns
- **Scenario:** menus-reopen, menus-toggle-truth
- **Act:** F10, then Down to open File, Escape, then Down (or Enter) to open File again. Or: Alt+V, Down, Space to turn Word Wrap off, then Alt+V again.
- **The reader should get:** The second opening reads as the first ('menu', later 'File menu'). Escape back to the bar says 'File'. The reopened View menu says Word Wrap is not checked.
- **The reader got (`261a218f`):** From the second opening on, Orca drops the menu's focus event ('Ignoring defunct object: \[menu\]') and says nothing. Escape back to File is then silent too, because Orca still holds File as its locus of focus. The reader's tree shows the menu and every item 'defunct', with the states libatspi cached at the first opening: Word Wrap reads 'checked' although the application holds it off. The menu's nodes keep their ids from one opening to the next. The adapter marks them defunct when the menu closes and re-adds them under the same ids. libatspi 2.52 cannot parse the adapter's AddAccessible signal, so it never revives them. This is the K2 mechanism, reached by ordinary widgets.
- **Platform:** Linux AT-SPI / Orca 46.1 measured. The defunct state is an AT-SPI/libatspi mechanism; Windows and macOS were not assessed.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** Every opening of File is heard the same way ('File menu.'), and Escape back to the bar says 'File.' each time. The reopened View menu reads Word Wrap as not checked, and Orca says 'Word Wrap check menu item not checked.'
- **Measured again:** menus-reopen, menus-toggle-truth: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-reopen 'Down opens File (second time)': +19.1 ms object:state-changed:focused 1 [menu] 'File' / +76.7 ms ORCA SAYS: 'File menu.' / pass the open menu is live (not defunct) to libatspi`
  - `pass2 menus-reopen 'Escape again': +15.7 ms object:state-changed:focused 1 [menu item] 'File' / +83.2 ms ORCA SAYS: 'File.'`
  - `pass2 menus-reopen 'Enter opens File (third time)': +21.2 ms object:state-changed:focused 1 [menu] 'File' / +68.8 ms ORCA SAYS: 'File menu.'`
  - `pass1 menus-toggle-truth 'Alt+V (second open)': +165.0 ms ORCA SAYS: 'Word Wrap check menu item not checked.' / pass [check menu item] 'Word Wrap' is not checked / pass the open menu is live (not defunct) to libatspi`
  - `pass2 menus-toggle-truth 'Alt+V (second open)': +137.1 ms ORCA SAYS: 'Word Wrap check menu item not checked.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_bar/widget\_impl.rs:94-95 (one host per menu, dormant between openings); crates/teksilo-core/src/accessibility.rs:1991 (widget\_id\_to\_node\_id); the id handed to the adapter is now renewed by crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-187 (AdapterIds::deliver), called from crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs:196
- **Evidence (`261a218f`):**
  - `menus-reopen report, '== Down opens File (second time)': 'FAIL  Orca says 'menu'' and 'FAIL  the open menu is live (not defunct) to libatspi' / '[menu] '' states=['defunct', 'enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']; first items: 'New' ['defunct', 'enabled', 'sensitive', 'showing', 'visible'], ...'`
  - `orca-debug.out (menus-reopen-20260925-130547-2231770): '13:06:04.129365 - EVENT MANAGER: Ignoring defunct object: [menu]'`
  - `same log, Escape after the second opening: '13:06:08.207019 - FOCUS MANAGER: Setting locus of focus to existing locus of focus' (no speech)`
  - `menus-toggle-truth report, '== Alt+V (second open)': 'FAIL  [check menu item] 'Word Wrap' is not checked' / '[check menu item] 'Word Wrap' states=['checkable', 'checked', 'defunct', 'enabled', 'sensitive', 'showing', 'visible'] attributes={} relations={}'; note (bridge, read after the acts): 'with View open again, the application holds Word Wrap toggled=false'`
  - `listener stderr (tree-stdout.txt): 'dbind-WARNING **: 12:55:31.301: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau'`
  - `crates/teksilo-widgets/src/menu_bar/widget_impl.rs:93-96 (each menu's host is created once with add_detached_deferred and made dormant between openings); crates/teksilo-core/src/accessibility.rs:1984 (widget_id_to_node_id is a pure function of the WidgetId); accesskit_atspi_common-0.20.0/src/adapter.rs:91-112 (remove_node emits StateChanged(Defunct, true))`
  - `menus-toggle-truth-20260925-132554-2985325 report, 'Alt+V (second open)': '[check menu item] 'Word Wrap' states=['checkable', 'checked', 'defunct', ...]' plus Orca '13:26:14.731151 - EVENT MANAGER: Ignoring defunct object: [menu]'`
  - `logs/*.txt: 'dbind-WARNING **: 13:24:49.997: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau'`
- **Reproduced:** reopen 3 of 3 complete runs (second and third openings both silent); toggle-truth 3 of 3
- **Verification:** confirmed. Reproduced: menus-reopen 3/3 (second and third openings both dropped), menus-toggle-truth 3/3
- **Fix idea:** Generalise the K2 fix. A subtree that leaves the tree and comes back needs fresh NodeIds, for example a per-activation generation mixed into the NodeId, because under AT-SPI an id once announced defunct can never be reused. The same fix covers menus-04 and menus-05.

### menus-03 {#menus-03}

A submenu opened from the keyboard closes itself about 165 ms later

- **Example:** menus-and-dropdowns
- **Scenario:** menus-submenu, menus-submenu-enter
- **Act:** Alt+F, Down x4 to Recent, then Right (or Enter).
- **The reader should get:** The Recent submenu opens with focus in it and stays open until Escape, Left or a choice. The reader can then pick project-alpha.toml, notes.md or budget.csv.
- **The reader got (`261a218f`):** The submenu opens and takes focus, then is removed 162-168 ms later, and focus drops back to the File menu. Orca says 'menu.' twice, the first one cut. Enter again repeats the cycle. No recent file can be opened from the keyboard, for any user. Cause: MenuList opens a submenu row with ctx.synthetic\_click, which dispatches a real mouse press and release. MenuItem's on\_tap therefore takes SubmenuOpenRoute::Tap(Mouse), whose dismissal is PointerLeave with a 150 ms delay. The KeyboardOrAt route, which does not auto-dismiss, is never reached, because the item never has focus. An AT-SPI click takes the KeyboardOrAt route, and that submenu stays open.
- **Platform:** All platforms (widget logic). Measured on Linux.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `9636094c` (menus).
- **Now (`c198e4d1`):** Right or Enter on Recent opens the submenu, Orca says 'Recent menu.', and it is still open 2.5 s later. Down reaches project-alpha.toml and notes.md, and Enter on notes.md opens it.
- **Measured again:** menus-submenu, menus-submenu-enter, verify-menus-submenu-held, fix-menus-submenu, fix-menus-submenu-enter: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-submenu 'Right opens Recent': +25.1 ms object:state-changed:focused 1 [menu] 'Recent' / +67.6 ms ORCA SAYS: 'Recent menu.' / pass 2 menu(s) in the tree after the act`
  - `pass2 menus-submenu-enter 'Enter opens Recent; still open 2.5 s later': +81.4 ms ORCA SAYS: 'Recent menu.' / pass no object:children-changed:remove event from [frame] '*'`
  - `pass1 verify-menus-submenu-held 'Right, pressed by a key client that stays connected 3 s': +103.2 ms ORCA SAYS: 'Recent menu.' / pass 2 menu(s) in the tree after the act`
  - `pass1 fix-menus-submenu '2.5 s later': pass 2 menu(s) in the tree after the act`
  - `pass2 fix-menus-submenu 'Enter on notes.md': pass the example ran 'Recent: notes'`
  - `pass2 fix-menus-submenu-enter 'Down: project-alpha.toml': +54.5 ms ORCA SAYS: 'project-alpha.toml.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_list.rs:254-268 (activate\_row: a MenuItem is activated through its own keyboard route, synthetic\_click only for other rows), 956-981 (Enter, Space and the forward arrow call activate\_row); crates/teksilo-widgets/src/menu\_item/widget\_impl.rs:550 (KeyboardOrAt route), 616 (Tap route); crates/teksilo-widgets/src/menu\_item.rs:136-160 (dismissal per route)
- **Evidence (`261a218f`):**
  - `menus-submenu-20260925-130534-2222500 report, '== Right opens Recent': '+16.9 ms object:children-changed:add [frame] '' -> [menu] ''', '+18.4 ms object:state-changed:focused 1 [menu] ''', '+179.9 ms object:children-changed:remove [frame] '' -> [menu] ''', '+180.2 ms object:state-changed:focused 1 [menu] ''', 'FAIL  2 menu(s) in the tree after the act' / '1 [menu] in the tree after the act: '' [...] ['New', 'Open', 'Save', 'Recent']'`
  - `open-to-close measured per run: 163.0, 168.0, 162.1 ms (Right); 167.7, 162.0 ms (Enter); app.log never shows a 'Recent:' line in these runs`
  - `menus-submenu-enter report: 'FAIL  no object:children-changed:remove event from [frame] '*'' / '+179.1 ms object:children-changed:remove [frame] '' -> [menu] '''`
  - `contrast, menus-at-actions (AT-SPI click on 'Recent'): 'pass  2 menu(s) in the tree after the act' (2 of 2)`
  - `crates/teksilo-widgets/src/menu_list.rs:833, 847 (Enter/Space and Right call ctx.synthetic_click); crates/teksilo-core/src/widget_tree/test_api.rs:82-96 (synthesise_tap_with_ops dispatches WidgetEvent::pointer_down/pointer_up, whose pointer is a mouse); crates/teksilo-widgets/src/menu_item/widget_impl.rs:589 (on_tap: SubmenuOpenRoute::Tap(ctx.pointer_kind())); crates/teksilo-widgets/src/menu_item.rs:141-159 and :117 (a hovering pointer gets PointerLeave { delay: 150 ms })`
  - `verify-menus-submenu-held-20260925-133450-3488718: '+17.3 ms object:children-changed:add -> [menu]; +181.5 ms object:children-changed:remove -> [menu]; key client exited at +3038 ms'`
  - `same run, 'Right, then Enter 1.5 s later': '+175.6 ms ...remove -> [menu]; +1546.7 ms ...add -> [menu]'; 'the example printed [] during the act'`
  - `menus-submenu reports: 'observed Orca's 'menu.' was cut by a stop 168 ms in' / 161 / 169`
- **Reproduced:** 5 of 5 runs (Right 3/3, Enter 2/2)
- **Verification:** confirmed. Reproduced: 9 of 9 runs: submenu 3/3, submenu-enter 3/3, verify-menus-submenu-held 3/3 (with the key client held connected)
- **Fix idea:** Have MenuList open a submenu through the keyboard route: call the item's activation with SubmenuOpenRoute::KeyboardOrAt, or give synthetic\_click a keyboard pointer kind, instead of a synthesised mouse tap. Add a test that a Right-opened submenu is still open after advance\_time(1 s).

### menus-04 {#menus-04}

A combo box list opened a second time is dead to Orca: the options and the value go unspoken

- **Example:** menus-and-dropdowns
- **Scenario:** menus-combo-reopen, menus-combo-color
- **Act:** Focus the Color combo box, Alt+Down (list opens), Escape, Alt+Down again, Down. Or Down, Down, Enter, then Up.
- **The reader should get:** The second opening is heard like the first ('List with 5 items', 'Blue.'), and each arrow says the new option.
- **The reader got (`261a218f`):** The first opening is heard. From the second on, Orca ignores the list box and every option as defunct. Opening and every arrow are silent. Because the combo's own value never reaches AT-SPI (menus-06), a Linux reader has no way left to learn what the combo box is set to. Same mechanism as menus-02: the dropdown panel keeps its widget ids across openings.
- **Platform:** Linux AT-SPI / Orca 46.1, measured.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** The second opening of the Color list is heard like the first ('List with 5 items', 'Blue.'), and Down in the reopened list says 'Yellow.'. The list box is live, not defunct.
- **Measured again:** menus-combo-reopen, menus-combo-color: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-combo-reopen 'Alt+Down (second open)': +35.5 ms object:state-changed:focused 1 [list item] 'Blue' / +123.1 ms ORCA SAYS: 'List with 5 items' / +123.2 ms ORCA SAYS: 'Blue.' / pass the list box is live (not defunct)`
  - `pass2 menus-combo-reopen 'Down: Yellow (in the reopened list)': +91.6 ms object:state-changed:focused 1 [list item] 'Yellow' / +722.4 ms ORCA SAYS: 'Yellow.'`
  - `pass1 menus-combo-color 'Up (the list opens a second time): Yellow': +73.7 ms ORCA SAYS: 'List with 5 items' / +73.7 ms ORCA SAYS: 'Yellow.'`
  - `pass2 menus-combo-color 'Up (the list opens a second time): Yellow': +587.5 ms ORCA SAYS: 'Yellow.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box.rs:850-861 (panel built once with add\_deferred, dormant and visible\_when between openings); crates/teksilo-core/src/accessibility.rs:1991 (widget\_id\_to\_node\_id); crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-187 (a node that comes back is handed a new id)
- **Evidence (`261a218f`):**
  - `menus-combo-reopen report, '== Alt+Down (second open)': 'FAIL  Orca says 'List'' and 'FAIL  the list box is live (not defunct)' / '[list box] '' states=['defunct', 'enabled', 'sensitive', 'showing', 'vertical', 'visible'] attributes={} relations={}'`
  - `orca-debug.out (menus-combo-reopen-20260925-130648-2274025): '13:07:01.110733 - SPEECH OUTPUT: 'List with 5 items'' and '13:07:01.110748 - SPEECH OUTPUT: 'Blue.'' on the first opening; then '13:07:09.260532 - EVENT MANAGER: Ignoring defunct object: [list box]', '13:07:13.399856 - EVENT MANAGER: Ignoring defunct object: [list item: 'Blue']', '13:07:13.416754 - EVENT MANAGER: Ignoring defunct object: [list box]'`
  - `menus-combo-color, '== Up (the list opens a second time): Yellow': 'FAIL  Orca says 'Yellow'' + 'observed  Orca ignored an event whose source was defunct' (3 of 3)`
  - `crates/teksilo-widgets/src/combo_box.rs:790-801 (dropdown created once with add_deferred, dormant/visible_when between openings)`
  - `menus-combo-color (3 runs), 'Up (the list opens a second time)': selection-changed on the list box, 'FAIL Orca says 'Yellow'', 'observed Orca ignored an event whose source was defunct'`
- **Reproduced:** combo-reopen 3 of 3, combo-color 3 of 3
- **Verification:** confirmed. Reproduced: combo-reopen 3/3, combo-color 3/3
- **Fix idea:** Same as menus-02: fresh NodeIds for a subtree that re-enters the tree.

### menus-05 {#menus-05}

A control scrolled out of view and back can be dead to Orca

- **Example:** menus-and-dropdowns
- **Scenario:** menus-scroll-back
- **Act:** Focus the Huge combo box, Tab to Add, Tab to Search (the page scrolls and the combo row leaves the viewport), Shift+Tab to Add, Shift+Tab to Huge, Shift+Tab to Country.
- **The reader should get:** Each control focused after scrolling back is spoken, 'combo box'.
- **The reader got (`261a218f`):** When the combo row leaves the viewport, AccessKit's filter drops the clipped combo boxes and their labels, and the adapter marks them defunct. Focus later returns to them under the same ids. In 1 of 3 runs Orca dropped the Country combo box's focus event as defunct and said nothing. In the other 2 it spoke, presumably because Orca had not cached that object before it was culled. The harness's own AT-SPI client showed those combo boxes as 'defunct' in all 3 runs. The Huge combo box was never culled (the filter keeps one off-screen neighbour) and was always spoken.
- **Platform:** Linux AT-SPI / Orca 46.1, measured.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** Controls scrolled out of view and back are spoken again: Shift+Tab back to the Huge and Country combo boxes says 'combo box.' each time, including a combo box the reader had visited before the scroll. The combo boxes are live, not defunct.
- **Measured again:** menus-scroll-back, verify-menus-scroll-visited: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-scroll-back 'Tab to Search (the combo boxes leave the view)': +19.5 ms object:state-changed:defunct 1 [combo box] ''`
  - `pass2 menus-scroll-back 'Shift+Tab back to the Huge combo box (scrolled back into view)': +64.7 ms object:state-changed:focused 1 [combo box] '' / +190.6 ms ORCA SAYS: 'combo box.' / pass the combo boxes are live (not defunct)`
  - `pass1 menus-scroll-back 'Shift+Tab to the Country combo box': +68.8 ms ORCA SAYS: 'combo box.'`
  - `pass1 verify-menus-scroll-visited 'Shift+Tab back to Country (visited before the scroll)': +26.7 ms object:state-changed:focused 1 [combo box] '' / +70.2 ms ORCA SAYS: 'combo box.'`
  - `pass2 verify-menus-scroll-visited 'Shift+Tab back to Country (visited before the scroll)': +31.1 ms object:state-changed:focused 1 [combo box] '' / +86.9 ms ORCA SAYS: 'combo box.'`
- **Where (`c198e4d1`):** crates/teksilo-core/src/accessibility.rs:1991 (widget\_id\_to\_node\_id); a node that comes back is handed a new id by crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-187; accesskit\_consumer-0.39.1/src/filters.rs:65-88 (clip culling, unchanged)
- **Evidence (`261a218f`):**
  - `menus-scroll-back-20260925-131644-2765174 report, '== Tab to Search (the combo boxes leave the view)': '+13.1 ms object:state-changed:defunct 1 [combo box] ''' ... '+17.7 ms object:state-changed:defunct 1 [label] 'Fruit''`
  - `same run, '== Shift+Tab to the Country combo box': '+27.2 ms object:state-changed:focused 1 [combo box] ''' then 'FAIL  Orca says 'combo box'' / 'Orca unheard: 'combo box''`
  - `orca-debug.out same run: '13:17:15.666253 - EVENT MANAGER: Ignoring defunct object: [combo box]'`
  - `all 3 new-version runs: 'FAIL  the combo boxes are live (not defunct)' / '[combo box] '' states=['defunct', 'enabled', 'focusable', 'sensitive', 'showing', 'visible']'`
  - `accesskit_consumer-0.39.0/src/filters.rs:65-88 (a child of a clips_children parent whose bounds miss the parent's, with no in-bounds neighbour, is ExcludeSubtree)`
  - `verify-menus-scroll-visited-20260925-133849-3719976 report: 'Shift+Tab back to Country (visited before the scroll)' -> '+23.8 ms object:state-changed:focused 1 [combo box]', 'FAIL Orca says 'combo box'', 'Ignoring defunct object: [combo box]'; 'Shift+Tab to Color (never visited)' -> '+61.7 ms ORCA SAYS: 'combo box.''`
  - `same run, 'Tab to Search': '+11.8 ms object:state-changed:defunct 1 [combo box]' x4, '[label] 'Color'', 'Fruit', 'Country (searchable)', 'Huge (10 000 items)', 'Size (disabled)'`
- **Reproduced:** Orca silent in 1 of 3 runs; the reader-side tree showed defunct nodes in 3 of 3
- **Verification:** corrected by the verifier. Reproduced: verify-menus-scroll-visited 3/3 silent for a visited control; menus-scroll-back 1/6 silent (the sweep saw 1/3) Real, and more reliable than '1 of 3' suggests. The sweep's scenario never guarantees that Orca has met the Country combo box before it is culled. In my runs of that scenario, Orca was silent on Country in 1 of 6. verify-menus-scroll-visited focuses Country first, so Orca has met it, then Tabs to Search (the combo boxes are culled: 'object:state-changed:defunct 1 \[combo box\]' x4 plus their labels) and Shift+Tabs back. Orca then drops Country's focus event in 3 of 3 runs ('13:39:20.885967 - EVENT MANAGER: Ignoring defunct object: \[combo box\]') and says nothing. In the same runs, Color, which Orca had never met, is spoken 'combo box.' 3/3, although the listener's own tree shows it defunct. Correction: the drop is deterministic for any control the reader has already visited and then scrolled out of view. That is the ordinary case for a user who comes back to a control. The 'k of n' depends only on whether Orca's libatspi cached the object earlier. The cause is as stated: consumer filters.rs:65-88 clip-culls the off-viewport children of a clips\_children parent, remove\_node marks them defunct, and they return under the same ids. The layer is framework (id reuse), meeting the upstream AddAccessible parse failure. Severity stays high.
- **Fix idea:** Fresh NodeIds on re-entry, as in menus-02. Or stop ScrollArea content from leaving the adapter's tree: AccessKit's clip culling exists to let off-screen items be scrolled into view, and here it turns every scroll into remove/defunct/add churn.

### menus-06 {#menus-06}

A combo box's selected value never reaches AT-SPI, so Orca never says it

- **Example:** menus-and-dropdowns
- **Scenario:** tabwalk, menus-combo-color, menus-country-search
- **Act:** Tab to (or focus) the Color combo box, which holds Blue; the Theme combo box in the toolbar. Commit France in the Country combo box.
- **The reader should get:** 'Color combo box Blue'. After a commit, the reader hears the new value.
- **The reader got (`261a218f`):** Orca says only 'combo box.' ('Theme combo box.'), with no value, on focus and after every commit. accesskit\_atspi\_common exports a string value only as a Label's name. A ComboBox has no Text interface, and only a numeric value reaches the Value interface. After the list closes, Orca's locus of focus stays on the removed option, because focus never left the combo box and no event puts it back.
- **Platform:** Linux AT-SPI / Orca 46.1, measured. Windows exposes it: accesskit\_windows node.rs:592-596 gives nodes with has\_value() a ValuePattern. macOS: accesskit\_macos node.rs:344 (AXValue).
- **Severity:** high; **layer:** upstream
- **Status:** Partly fixed by `f9ffa98c`, found by the re-measure: that commit did not report it. What remains is under **Now**.
- **Now (`c198e4d1`):** Orca still says only 'combo box.' on focus and after a commit, with no value: accesskit\_atspi\_common 0.21.0 still exports a string value only as a Label's name. What changed: closing the list now puts the reader's focus back on the combo box, so Orca no longer stays on the removed option; it says the window's title, then 'combo box.'.
- **Measured again:** menus-combo-color, menus-country-search, menus-combo-huge, tabwalk-menus-and-dropdowns: 2 of 2 runs each; adapter read from source
- **Evidence (`c198e4d1`):**
  - `pass1 menus-combo-color 'focus the Color combo box (Blue selected)': +71.2 ms ORCA SAYS: 'combo box.' / FAIL Orca says 'Blue'`
  - `pass2 menus-combo-color 'Enter closes the list': +140.6 ms object:state-changed:focused 1 [combo box] '' / +925.5 ms ORCA SAYS: 'combo box.' / FAIL Orca says 'Purple'`
  - `pass1 menus-country-search 'Enter commits': +22.7 ms object:state-changed:focused 1 [combo box] '' / +115.0 ms ORCA SAYS: 'combo box.' / FAIL Orca says 'France'`
  - `pass2 menus-combo-huge 'Enter closes': +64.7 ms ORCA SAYS: 'combo box.' / FAIL Orca says 'Item #09989'`
  - `pass1 tabwalk-menus-and-dropdowns 'Tab 5': +78.1 ms ORCA SAYS: 'Theme combo box.'`
  - `accesskit_atspi_common-0.21.0/src/node.rs:38-44 (name from the value only when label_comes_from_value), 476-482 (Text only with text ranges, Value only for a numeric value)`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box.rs:1338-1350 (the selection is set as a string value); accesskit\_atspi\_common-0.21.0/src/node.rs:38-44, 476-482
- **Evidence (`261a218f`):**
  - `tabwalk report: 'Tab 7' -> '+10.4 ms object:state-changed:focused 1 [combo box] ''' / '+128.8 ms ORCA SAYS: 'combo box.'' (Color, Blue selected); 'Tab 5' -> 'ORCA SAYS: 'Theme combo box.''`
  - `menus-combo-color: 'FAIL  Orca says 'Blue'' (3 of 3); orca-debug.out '13:07:25.615824 - SPEECH OUTPUT: 'combo box.''`
  - `menus-country-search '== Enter commits': '+78.0 ms ORCA SAYS: 'combo box.'' and 'FAIL  Orca says 'France'' (3 of 3)`
  - `menus-combo-color orca-debug.out after Enter closes the list: '13:13:28.013545 - FOCUS MANAGER: Locus of focus is [list item: 'Purple']', still the same at '13:13:39.765324'`
  - `accesskit_atspi_common-0.20.0/src/node.rs:38-44 (name comes from value only when label_comes_from_value, i.e. Role::Label), :486-492 (Text only for supports_text_ranges, Value only for a numeric current_value), :577-579; crates/teksilo-widgets/src/combo_box.rs:1259-1264 (the selection is set_value)`
  - `Orca formatting.py:185-187 (focused combo box: 'labelOrName + roleName + expandableState')`
  - `tabwalk-menus-and-dropdowns-20260925-133923-3745945: the four content combo boxes each 'ORCA SAYS: 'combo box.''`
  - `menus-combo-color-20260925-132747-3074457/orca-debug.out: 'FOCUS MANAGER: Locus of focus is [list item: 'Purple']' persisting after the list closed`
- **Reproduced:** deterministic; combo-color 3/3, country-search 3/3, tab walk
- **Verification:** confirmed. Reproduced: deterministic: combo-color 3/3, country-search 3/3, tab walk
- **Fix idea:** Upstream: have accesskit\_atspi\_common expose a non-Label string value (a Text interface over the value, as Chromium does for a select). Teksilo could, for Linux, keep the selected option as an exported child of the closed combo box (Orca's getComboBoxValue reads the selected child). Teksilo can also re-emit focus on the combo box when its list closes.

### menus-07 {#menus-07}

The 10 000-item combo list says nothing on any arrow: options nested inside ListView rows, selection invisible to AT-SPI

- **Example:** menus-and-dropdowns
- **Scenario:** menus-combo-huge
- **Act:** Focus the Huge combo box, Alt+Down, then Down, Down, End, PageUp, Enter.
- **The reader should get:** Each move says the selected item ('Item #00001', ...). Opening says a list of 10000 items.
- **The reader got (`261a218f`):** Opening and every move are silent. The virtualized path nests a ListView (Role::ListBox) inside the dropdown's ListBox, a Role::Group body pane, then ListView row wrappers (unselected ListBoxOption) around the combo's own ListBoxOption (selected). The list box that fires selection-changed has 0 selected children, so Orca finds nothing to speak. posinset/setsize are right (10000 of 10000).
- **Platform:** Linux AT-SPI / Orca 46.1, measured. The nesting is in the AccessKit tree, so every platform gets it.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `f9ffa98c` (combobox).
- **Now (`c198e4d1`):** The 10 000-item list speaks every move: 'Item #00001.', 'Item #00002.', End 'Item #09999.', PageUp 'Item #09989.'. Opening says 'List with 30 items' and 'Item #00000.', because Orca counts the rows the virtualized list exports; each item carries its true place (posinset 10000 of setsize 10000).
- **Measured again:** menus-combo-huge, fix-combobox-huge: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-combo-huge 'Alt+Down opens the 10 000-item list': +89.4 ms object:state-changed:focused 1 [list item] 'Item #00000' / +237.1 ms ORCA SAYS: 'List with 30 items' / +237.1 ms ORCA SAYS: 'Item #00000.'`
  - `pass1 menus-combo-huge 'Down': +25.4 ms object:state-changed:focused 1 [list item] 'Item #00002' / +104.5 ms ORCA SAYS: 'Item #00002.'`
  - `pass2 menus-combo-huge 'End: the last item': +290.4 ms ORCA SAYS: 'Item #09999.' / pass [list item] 'Item #09999' has posinset=10000 / pass [list item] 'Item #09999' has setsize=10000`
  - `pass2 menus-combo-huge 'PageUp': +19.8 ms object:state-changed:focused 1 [list item] 'Item #09989' / +83.8 ms ORCA SAYS: 'Item #09989.'`
  - `pass1 and pass2 fix-combobox-huge: every check passed`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box/panel.rs:136-230 (virtualized list; the ListView is made presentational at 225-228); crates/teksilo-widgets/src/list\_view/widget\_impl.rs:997
- **Evidence (`261a218f`):**
  - `menus-combo-huge: 'FAIL  Orca says 'Item #09999'', 'FAIL  Orca says 'Item #09989'', and '== Alt+Down opens ...' 'FAIL  Orca says one of ('10000', '10,000', 'List')' / 'Orca said nothing in this act' (3 of 3)`
  - `the same runs pass 'pass  [list item] 'Item #09999' has posinset=10000' / 'setsize=10000'`
  - `orca-debug.out in the three runs: '13:07:12.574528 - AXSelection: [list box] reports 0 selected children', '13:12:10.344206 - AXSelection: [list box] reports 0 selected children', '13:16:07.682050 - AXSelection: [list box] reports 0 selected children'`
  - `tree after Alt+Down: '[combo box] '' {focusable,focused} rel=['controller-for']' > '[unknown] ''' > '[list box] '' {vertical}' > '[list box] '' {focusable,vertical} attrs={'setsize': '10000'}' > '[panel] ''' > '[list item] '' {selectable} attrs={'setsize': '10000', 'posinset': '1'}' > '[list item] 'Item #00000' {selectable} ...'`
  - `crates/teksilo-widgets/src/combo_box/panel.rs:169-193 (the delegate ignores the ListView's own _selected and wraps a DropdownItem in ListView::from_list_source); crates/teksilo-widgets/src/list_view/widget_impl.rs:994 (Role::ListBox); list_view/body_pane.rs:700-707 (Role::Group)`
  - `menus-combo-huge-20260925-132548-2982454/orca-debug.out:1106 '13:26:04.788821 - AXSelection: [list box] reports 0 selected children'`
- **Reproduced:** deterministic; 3 of 3
- **Verification:** confirmed. Reproduced: 3/3
- **Fix idea:** Drive the ListView's SelectionModel from the combo's selection, so its row wrapper carries selected. Then drop the inner DropdownItem's ListBoxOption role (or the outer ListBox), leaving one listbox &gt; option level.

### menus-08 {#menus-08}

Menus are unnamed: opening File, Edit or View is announced as just 'menu.'

- **Example:** menus-and-dropdowns
- **Scenario:** menus-menubar-f10, menus-menubar-alt, menus-view-items, menus-popover-typeahead
- **Act:** F10 then Down, or Alt+F / Alt+E / Alt+V, or Enter on Add.
- **The reader should get:** 'File menu' (the menu labelled by its trigger).
- **The reader got (`261a218f`):** 'menu.' every time. The Role::Menu node has no name and no labelled\_by.
- **Platform:** Linux AT-SPI / Orca 46.1, measured. The name is missing from the AccessKit node on every platform.
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `9636094c` (menus).
- **Now (`c198e4d1`):** Menus opened from the bar or a button are named: 'File menu.', 'Edit menu.', 'View menu.', 'Add menu.', 'View options menu.'. A context menu still says only 'menu.', since nothing opened it to name it after.
- **Measured again:** menus-menubar-f10, menus-menubar-alt, menus-view-items, menus-popover-typeahead, menus-context-mouse: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-menubar-f10 'Down on File opens the File menu': +57.8 ms object:state-changed:focused 1 [menu] 'File' / +335.0 ms ORCA SAYS: 'File menu.' / pass the tree holds [menu] 'File'`
  - `pass2 menus-menubar-alt 'Alt+E on the menu bar (first open of Edit)': +143.7 ms ORCA SAYS: 'Edit menu.'`
  - `pass1 menus-view-items 'Alt+V opens View': +80.7 ms ORCA SAYS: 'View menu.'`
  - `pass2 menus-popover-typeahead 'Enter opens its menu': +112.2 ms ORCA SAYS: 'Add menu.'`
  - `pass1 menus-context-mouse 'right-click the Edit Menu panel (bridge)': +135.8 ms ORCA SAYS: 'menu.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_list.rs:1124-1129 (labelled\_by the opener); crates/teksilo-widgets/src/menu\_context.rs:87-117 (unchanged)
- **Evidence (`261a218f`):**
  - `menus-menubar-f10 '== Down on File opens the File menu': '+44.7 ms object:state-changed:focused 1 [menu] ''', '+200.1 ms ORCA SAYS: 'menu.'', 'FAIL  the tree holds [menu] 'File'' (3 of 3)`
  - `menus-menubar-alt: 'FAIL  Orca says one of ('File menu', 'New')' / 'Orca said: 'menu.'' (3 of 3)`
  - `crates/teksilo-widgets/src/menu_list.rs:991-993 (Role::Menu only); crates/teksilo-widgets/src/menu_context.rs:91-117 (open_at knows the trigger but names nothing)`
- **Reproduced:** deterministic; 3 of 3 in each scenario
- **Verification:** confirmed. Reproduced: deterministic 3/3 in each scenario
- **Fix idea:** Label the MenuList by its opener: labelled\_by the MenuBarTrigger, the submenu MenuItem or the PopoverButton.

### menus-09 {#menus-09}

Escape does not leave the menu bar, and after a command focus stays on the menu bar

- **Example:** menus-and-dropdowns
- **Scenario:** menus-f10-escape, menus-menubar-alt, menus-menubar-f10
- **Act:** From the Color combo box press F10, then Escape. Or Alt+F, Escape, Escape. Or Alt+E, then c (Copy).
- **The reader should get:** Escape on the bar returns focus to the control the reader came from, and so does running a command. Windows and GTK both do this.
- **The reader got (`261a218f`):** Escape on a focused menu-bar item does nothing, and focus stays on File. Tab then walks the bar (Edit, View, Settings) before reaching content. After Copy runs, focus lands on the Edit menu-bar item, not back on the Fruit combo box. The reader is left in the menu bar and must Tab out through it.
- **Platform:** All platforms (widget logic). Measured on Linux.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Escape on a focused menu-bar item still does nothing (Orca echoes only 'escape'), and Tab from there goes on to Edit. After Copy runs from the Edit menu, focus lands on the Edit menu-bar item ('Edit.'), not back on the Fruit combo box.
- **Measured again:** menus-f10-escape, menus-menubar-alt, menus-menubar-f10: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-f10-escape 'Escape on the menu bar': +8.3 ms ORCA SAYS: 'escape' / FAIL focus lands on [combo box] '*'`
  - `pass2 menus-f10-escape 'Tab': +18.5 ms object:state-changed:focused 1 [menu item] 'Edit' / +48.3 ms ORCA SAYS: 'Edit.'`
  - `pass1 menus-menubar-alt 'Escape again': +6.9 ms ORCA SAYS: 'escape' / FAIL focus lands on [combo box] '*'`
  - `pass2 menus-menubar-alt 'bare C in the Edit menu (mnemonic of Copy)': pass the example ran 'Copy' / +16.9 ms object:state-changed:focused 1 [menu item] 'Edit' / +76.6 ms ORCA SAYS: 'Edit.'`
  - `pass1 menus-menubar-f10 'Escape on the menu bar': +6.0 ms ORCA SAYS: 'escape' / FAIL focus lands on [frame] '*'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_bar/trigger.rs:95-127; crates/teksilo-widgets/src/menu\_bar.rs:456-525
- **Evidence (`261a218f`):**
  - `menus-f10-escape '== Escape on the menu bar': 'FAIL  focus lands on [combo box] '*'' / 'no focus change on the bus in this act'; '== Tab': '+8.1 ms object:state-changed:focused 1 [menu item] 'Edit'' / '+42.2 ms ORCA SAYS: 'Edit.'' (3 of 3)`
  - `menus-menubar-alt '== Escape again': 'FAIL  focus lands on [combo box] '*'' / 'no focus change on the bus in this act' (3 of 3)`
  - `menus-menubar-alt '== bare C in the Edit menu (mnemonic of Copy)': 'pass  the example ran 'Copy'', '+13.0 ms object:state-changed:focused 1 [menu item] 'Edit'', '+85.9 ms ORCA SAYS: 'Edit.'' (3 of 3)`
  - `crates/teksilo-widgets/src/menu_bar/trigger.rs:99-127 (on_key: Down/Enter/Space/Left/Right, no Escape); crates/teksilo-widgets/src/menu_bar.rs:462-470 (F10 FocusTrigger records no return target)`
- **Reproduced:** deterministic; 3 of 3 in each
- **Verification:** confirmed. Reproduced: f10-escape 3/3, menubar-alt 3/3
- **Fix idea:** Record the focus that was current when the bar was entered (F10, Alt, Alt+letter). Restore it on Escape from a trigger and after a command dismisses the chain.

### menus-10 {#menus-10}

Escape after moving between menus with Left or Right drops focus on the window

- **Example:** menus-and-dropdowns
- **Scenario:** menus-escape-after-right, menus-submenu
- **Act:** Alt+F, Right (Edit opens), Escape.
- **The reader should get:** Edit closes and focus lands on the Edit menu-bar item, as after Escape on a menu opened directly, and the reader hears 'Edit'.
- **The reader got (`261a218f`):** Focus lands on the unnamed frame, and Orca says 'frame.'. Escape is taken by the router before the menu sees it (pointer\_router.rs:627-641), which restores the overlay's recorded focus\_restore if that widget is still active. show\_overlay records whatever had focus at show time (overlay\_impl.rs:1965-1971). MenuContext::navigate opens the next menu while focus is still inside the previous one, so the recorded target is the previous menu's MenuList, dormant by the time Escape runs. MenuContext::close, which would focus the trigger, never runs.
- **Platform:** All platforms (widget and core logic). Measured on Linux.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** After Alt+F and Right to Edit, Escape still puts focus on the window, not on the Edit menu-bar item. Orca now says the window's title followed by 'frame.' where it said only 'frame.', and a following Tab goes to File.
- **Measured again:** menus-escape-after-right, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 menus-escape-after-right 'Escape closes Edit': +21.0 ms object:state-changed:focused 1 [frame] 'Teksilo — Menus & Dropdowns (Milestone 4)' / +131.3 ms ORCA SAYS: 'Teksilo — Menus & Dropdowns (Milestone 4) frame.' / FAIL focus lands on [menu item] 'Edit'`
  - `pass2 menus-escape-after-right 'Escape closes Edit': +27.5 ms object:state-changed:focused 1 [frame] 'Teksilo — Menus & Dropdowns (Milestone 4)' / +180.4 ms ORCA SAYS: 'Teksilo — Menus & Dropdowns (Milestone 4) frame.' / FAIL Orca says 'Edit'`
  - `pass2 menus-escape-after-right 'Tab after that': +83.1 ms object:state-changed:focused 1 [menu item] 'File' / +114.6 ms ORCA SAYS: 'File.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_context.rs:137-155 (navigate), 122-134 (close); crates/teksilo-core/src/widget\_tree/pointer\_router.rs:627-641 (Escape restores focus\_restore only if it is still active), 2889-2897 (the ctx drain records the focus when it shows the overlay); crates/teksilo-core/src/widget\_tree/overlay\_impl.rs:2007-2014 (show\_overlay records the focus)
- **Evidence (`261a218f`):**
  - `menus-escape-after-right '== Escape closes Edit': '+8.8 ms object:state-changed:focused 1 [frame] ''', '+68.8 ms ORCA SAYS: 'frame.'', 'FAIL  focus lands on [menu item] 'Edit'' (3 of 3)`
  - `menus-submenu '== Escape' after Left moved to the View menu: '+9.4 ms object:state-changed:focused 1 [frame] ''', '+69.9 ms ORCA SAYS: 'frame.'' (3 of 3)`
  - `crates/teksilo-core/src/widget_tree/pointer_router.rs:627-641; crates/teksilo-core/src/widget_tree/overlay_impl.rs:1965-1971; crates/teksilo-widgets/src/menu_context.rs:137-155 (navigate), :122-134 (close)`
- **Reproduced:** 3 of 3 (escape-after-right), 3 of 3 (submenu)
- **Verification:** confirmed. Reproduced: escape-after-right 3/3, submenu 3/3
- **Fix idea:** In MenuContext::open\_at, set the new overlay's focus\_restore to the trigger (the menu bar owns the return target), or carry the outgoing overlay's focus\_restore across navigate.

### menus-11 {#menus-11}

Both context menus are unreachable by keyboard and by screen reader

- **Example:** menus-and-dropdowns
- **Scenario:** menus-context-keys, menus-context-mouse
- **Act:** Look for a way to reach the Edit Menu and File Menu panels: Tab, Shift+F10 and the Menu key from the nearest control, and the panel's AT-SPI actions.
- **The reader should get:** A keyboard or screen-reader user can open Cut/Copy/Paste and the file operations.
- **The reader got (`261a218f`):** The panels are not focusable, hold no focusable control, and offer no AT-SPI action. Tab from the Huge combo box goes straight to Add. Shift+F10 and the Menu key open nothing, because the router targets the focused widget's ancestor chain. Only a secondary click, done here through the bridge, opens them. Once opened by the mouse the menu is usable to the extent menus-01 allows, and Escape returns focus correctly.
- **Platform:** Linux measured. On every platform the panel advertises ShowContextMenu, which no AccessKit adapter handles.
- **Severity:** critical; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Neither context-menu panel can be reached by keyboard or by an assistive technology: the panels are not focusable and offer no action, Shift+F10 and the Menu key open nothing, and Tab from the Huge combo box goes straight to Add. Only a secondary click opens them.
- **Measured again:** menus-context-keys, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 menus-context-keys 'look at the panels': FAIL the 'Edit Menu' panel (it owns a context menu) can be reached by keyboard or offers an action / [panel] holding 'Edit Menu': states=['enabled', 'sensitive', 'showing', 'visible'] actions=None`
  - `pass2 menus-context-keys 'Shift+F10 on the Huge combo box (nearest focusable control)': FAIL the tree holds [menu] '*' / no such node in the tree after the act`
  - `pass1 menus-context-keys 'Menu key': FAIL the tree holds [menu] '*'`
  - `pass2 menus-context-keys 'Tab from the Huge combo box': +197.8 ms object:state-changed:focused 1 [push button] 'Add' / +850.8 ms ORCA SAYS: 'Add push button.'`
  - `No match for ShowContextMenu in accesskit_atspi_common-0.21.0, accesskit_windows-0.35.1 or accesskit_macos-0.27.1`
- **Where (`c198e4d1`):** examples/menus\_and\_dropdowns/src/main.rs:239-276, 338-382
- **Evidence (`261a218f`):**
  - `menus-context-keys '== look at the panels': 'FAIL  the 'Edit Menu' panel (it owns a context menu) can be reached by keyboard or offers an action' / '[panel] holding 'Edit Menu': states=['enabled', 'sensitive', 'showing', 'visible'] actions=None; ...' (2 of 2)`
  - `'== Shift+F10 on the Huge combo box ...': 'FAIL  the tree holds [menu] '*''; '== Menu key': same; '== Tab from the Huge combo box': '+24.6 ms object:state-changed:focused 1 [push button] 'Add''`
  - `examples/menus_and_dropdowns/src/main.rs:239-276 and 338-382 (Panel::new()...context_menu on a panel of TextWidgets only)`
  - `crates/teksilo-core/src/widget_tree/pointer_router.rs:1264-1283 (keyboard route starts from the focused widget); crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:856-858 (ShowContextMenu advertised)`
- **Reproduced:** deterministic; 2 of 2
- **Verification:** confirmed. Reproduced: 3/3
- **Fix idea:** Example: make each panel focusable and named (.focusable(true).access\_label(...)), so Tab and Shift+F10 reach it. Framework: warn in debug when a context\_menu owner has no focusable self or descendant.

### menus-12 {#menus-12}

Disabled menu items are exported as enabled and sensitive on AT-SPI

- **Example:** menus-and-dropdowns
- **Scenario:** menus-scroll-back, menus-context-mouse
- **Act:** Read the inline 'Disabled item' at launch. Open the File panel's context menu and read 'Export as PDF'. Both are built with enabled(false).
- **The reader should get:** The items are not enabled or sensitive, so Orca says 'grayed'/'unavailable'.
- **The reader got (`261a218f`):** Both carry 'enabled' and 'sensitive'. Teksilo marks the nodes disabled, but accesskit\_atspi\_common gives Enabled\|Sensitive to any disabled node whose role has no read-only support (MenuItem, Button, ...).
- **Platform:** Linux only. accesskit\_windows node.rs:535 (IsEnabled = !is\_disabled) and accesskit\_macos node.rs:658-660 map it correctly.
- **Severity:** high; **layer:** upstream
- **Status:** Fixed by `d217ee62` (accesskit-update).
- **Now (`c198e4d1`):** 'Disabled item' and 'Export as PDF' are no longer enabled or sensitive on AT-SPI: accesskit\_atspi\_common 0.21.0 leaves Enabled and Sensitive off a disabled node.
- **Measured again:** menus-scroll-back, menus-context-mouse: 2 of 2 runs each; adapter read from source
- **Evidence (`c198e4d1`):**
  - `pass1 menus-scroll-back 'look at the tree at launch': pass [menu item] 'Disabled item' is not enabled / pass [menu item] 'Disabled item' is not sensitive`
  - `pass2 menus-scroll-back 'look at the tree at launch': pass [menu item] 'Disabled item' is not enabled`
  - `pass1 menus-context-mouse 'right-click the File Menu panel (bridge)': pass [menu item] 'Export as PDF' is not enabled / pass [menu item] 'Export as PDF' is not sensitive`
  - `pass2 menus-context-mouse 'right-click the File Menu panel (bridge)': pass [menu item] 'Export as PDF' is not enabled`
  - `accesskit_atspi_common-0.21.0/src/node.rs:376-382 (if !state.is_disabled() { ... Enabled | Sensitive })`
- **Where (`c198e4d1`):** accesskit\_atspi\_common-0.21.0/src/node.rs:376-382
- **Evidence (`261a218f`):**
  - `menus-scroll-back '== look at the tree at launch': 'FAIL  [menu item] 'Disabled item' is not enabled' / '[menu item] 'Disabled item' states=['enabled', 'sensitive', 'showing', 'visible'] attributes={} relations={}' (4 of 4)`
  - `menus-context-mouse: 'FAIL  [menu item] 'Export as PDF' is not enabled' / '[menu item] 'Export as PDF' states=['enabled', 'sensitive', 'showing', 'visible'] attributes={} relations={}' (2 of 2)`
  - `accesskit_atspi_common-0.20.0/src/node.rs:373-377 ('if state.is_read_only_supported() && state.is_read_only_or_disabled() { ReadOnly } else { Enabled | Sensitive }'); accesskit_consumer-0.39.0/src/node.rs:861-879 (MenuItem is not read_only_supported); Teksilo sets disabled: crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:585-588`
  - `verify-menus-disabled-bridge-20260925-133904-3732410 note: "bridge: [{... 'disabled': True, 'id': 4294967425, 'label': 'Disabled item', 'role': 'MenuItem'}]"`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: deterministic (scroll-back 6/6, context-mouse 3/3, bridge 1/1)
- **Fix idea:** Upstream: in atspi\_common, omit Enabled\|Sensitive for any is\_disabled() node. Until then Teksilo cannot correct it through AccessKit's API.

### menus-13 {#menus-13}

Radio (and other) menu items export no position and no group: 'Dark Theme, 2 of 3' is impossible

- **Example:** menus-and-dropdowns
- **Scenario:** menus-view-items
- **Act:** Alt+V, then read the three theme radio items in the tree, and arrow onto them.
- **The reader should get:** Each radio item carries its place in its group (posinset 2, setsize 3, or a member-of relation) so a reader can say '2 of 3'.
- **The reader got (`261a218f`):** No posinset, no setsize, no relation. MenuItem pushes the group with push\_to\_radio\_group, which no adapter reads, and sets no position\_in\_set / size\_of\_set. Orca could not say it anyway (menus-01, and position speaking is off by default). NVDA, which reads UIA PositionInSet/SizeOfSet (accesskit\_windows node.rs:1326-1327), gets nothing either.
- **Platform:** Linux measured. Windows and macOS by source: no adapter reads radio\_group.
- **Severity:** low; **layer:** framework
- **Severity in the sweep:** medium. Items now carry a position and the menu a count, so a reader that speaks positions gets one; only the count within the radio group is missing.
- **Status:** Partly fixed by `9636094c`, found by the re-measure: that commit did not report it. What remains is under **Now**.
- **Now (`c198e4d1`):** Each menu item now carries its place among the menu's shown rows, so Dark Theme reads posinset 4 of setsize 6, not 2 of 3 in its radio group, and there is still no member-of relation. Orca does not speak positions by default and says 'Dark Theme not selected radio menu item.'
- **Measured again:** menus-view-items, fix-menus-view-items: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-view-items 'Alt+V opens View': FAIL [radio menu item] 'Dark Theme' has posinset=2 / [radio menu item] 'Dark Theme' states=['checkable', 'enabled', 'sensitive', 'showing', 'visible'] attributes={'posinset': '4', 'setsize': '6'} relations={}`
  - `pass2 menus-view-items 'Alt+V opens View': FAIL [radio menu item] 'Dark Theme' has a member-of relation`
  - `pass2 menus-view-items 'Down: Dark Theme (radio 2 of 3)': +227.5 ms ORCA SAYS: 'Dark Theme not selected radio menu item.' / FAIL Orca says '2 of 3'`
  - `pass1 fix-menus-view-items 'Alt+V opens View': pass [radio menu item] 'Dark Theme' has posinset=4 / pass [radio menu item] 'Dark Theme' has setsize=6`
  - `No reader of radio_group in accesskit_consumer-0.39.1, accesskit_atspi_common-0.21.0, accesskit_windows-0.35.1 or accesskit_macos-0.27.1`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_item/widget\_impl.rs:954-961 (position among the menu's shown rows), 986-998 (push\_to\_radio\_group, still read by no adapter)
- **Evidence (`261a218f`):**
  - `menus-view-items '== Alt+V opens View': 'FAIL  [radio menu item] 'Dark Theme' has posinset=2', 'FAIL  [radio menu item] 'Dark Theme' has setsize=3', 'FAIL  [radio menu item] 'Dark Theme' has a member-of relation' / '[radio menu item] 'Dark Theme' states=['checkable', 'enabled', 'sensitive', 'showing', 'visible'] attributes={} relations={}' (3 of 3)`
  - `'FAIL  Orca says '2 of 3'' (3 of 3)`
  - `crates/teksilo-widgets/src/menu_item/widget_impl.rs:950-962 (push_to_radio_group only); Orca settings.py:218 'enablePositionSpeaking = False'`
- **Reproduced:** deterministic; 3 of 3
- **Verification:** confirmed. Reproduced: 3/3
- **Fix idea:** Have MenuList set position\_in\_set on each item and size\_of\_set on the Menu (per radio group, or per menu), as ComboBox does for its options.

### menus-14 {#menus-14}

The first item is skipped: Down from an empty combo box picks the second fruit, and menu type-ahead from no highlight lands on the second match

- **Example:** menus-and-dropdowns
- **Scenario:** menus-combo-fruit, menus-popover-typeahead
- **Act:** Focus the empty Fruit combo box and press Down. Open the Add menu, press n three times (each after the 500 ms reset), then Enter.
- **The reader should get:** Down from nothing selects Apple. n goes to 'New file', then 'New folder', then 'New project…', and Enter runs New project.
- **The reader got (`261a218f`):** Down selects Banana ('List with 7 items', 'Banana.'). The first n lands on 'New folder', the third wraps to 'New file', and Enter runs New file, a different command from the one the reader aimed at, with nothing spoken in between (menus-01).
- **Platform:** All platforms (widget logic). Measured on Linux.
- **Severity:** low; **layer:** framework
- **Severity in the sweep:** medium. The combo box half is fixed, and in a menu each type-ahead landing is now spoken, so the reader hears which item Enter will run and can move on.
- **Status:** Partly fixed by `f9ffa98c` (combobox). What remains is under **Now**.
- **Now (`c198e4d1`):** The combo box half is gone: Down from the empty Fruit combo box says 'List with 7 items', 'Apple.'. The menu half remains: in the Add menu the first n lands on 'New folder', the third on 'New file', and Enter runs New file, though each landing is now spoken.
- **Measured again:** menus-combo-fruit, menus-popover-typeahead: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-combo-fruit 'Down from nothing selected': +35.9 ms object:state-changed:focused 1 [list item] 'Apple' / +157.1 ms ORCA SAYS: 'List with 7 items' / +157.1 ms ORCA SAYS: 'Apple.'`
  - `pass2 menus-combo-fruit 'Down from nothing selected': +775.6 ms ORCA SAYS: 'Apple.'`
  - `pass1 menus-popover-typeahead 'type-ahead 'n'': +16.4 ms object:state-changed:focused 1 [menu item] 'New folder' / +77.3 ms ORCA SAYS: 'New folder.'`
  - `pass2 menus-popover-typeahead 'type-ahead 'n' a third time': +15.7 ms object:state-changed:focused 1 [menu item] 'New file' / +76.3 ms ORCA SAYS: 'New file.'`
  - `pass1 and pass2 menus-popover-typeahead 'Enter runs New project': FAIL the example ran 'NewProject' / the example printed ['NewFileFromPopoverIcon'] during the act`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box.rs:984-1000 (no value is before the first item); crates/teksilo-widgets/src/menu\_list.rs:1053-1071 (type-ahead: the start is the highlight or 0, and the search begins one past it)
- **Evidence (`261a218f`):**
  - `menus-combo-fruit '== Down from nothing selected': '+177.4 ms ORCA SAYS: 'List with 7 items'', '+177.4 ms ORCA SAYS: 'Banana.'', 'FAIL  Orca says 'Apple'' (3 of 3)`
  - `menus-popover-typeahead '== Enter runs New project': 'FAIL  the example ran 'NewProject'' / 'the example printed ['NewFileFromPopoverIcon'] during the act' (3 of 3)`
  - `crates/teksilo-widgets/src/combo_box.rs:1031-1050 ('Treat "no selection" as an implicit cursor at index 0 — ArrowDown advances to index 1'); crates/teksilo-widgets/src/menu_list.rs:926-934 (start = focused or 0; the search begins at start+1)`
- **Reproduced:** deterministic; 3 of 3 each
- **Verification:** confirmed. Reproduced: combo-fruit 3/3, combo-huge 3/3, popover-typeahead 3/3
- **Fix idea:** With no selection or highlight, Down selects index 0, and type-ahead starts its search at offset 0.

### menus-15 {#menus-15}

The Opacity slider inside the View options menu ignores the arrow keys, and is read as '0.6499999761581421'

- **Example:** menus-and-dropdowns
- **Scenario:** menus-rich-menu
- **Act:** Tab to View options, Enter, Tab x10 to the Opacity slider, then Right and End. Also Right after AT-SPI grab\_focus on it.
- **The reader should get:** Right steps to 0.66 and the reader hears the new value; the value is read as a sensible figure.
- **The reader got (`261a218f`):** Focus reaches the slider, and the bus and the bridge both say it is focused. Right and End change nothing: no value event, and the application itself still holds 0.6499999761581421. The value is spoken raw ('Opacity horizontal slider 0.6499999761581421.'), because an f32 is widened to f64 unrounded. Cause of the ignored keys not traced: Slider's on\_key and range\_nav::range\_move handle ArrowRight, and no ancestor on the path registers on\_key\_preview.
- **Platform:** Linux AT-SPI / Orca 46.1, measured. The raw float is in the AccessKit node on every platform.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Now (`c198e4d1`):** Right on the Opacity slider now says '0.66', End says '1', and Up, PageUp and Home are spoken as they happen. The value is read as '0.65', not '0.6499999761581421'.
- **Measured again:** menus-rich-menu, verify-menus-rich-keys: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-rich-menu 'Tab 10 inside the menu': +25.1 ms object:state-changed:focused 1 [slider] 'Opacity' / +105.3 ms ORCA SAYS: 'Opacity horizontal slider 0.65.'`
  - `pass1 menus-rich-menu 'Right on the slider (focus reached by Tab)': +18.1 ms object:property-change:accessible-value [slider] 'Opacity' / +28.8 ms ORCA SAYS: '0.66'`
  - `pass2 menus-rich-menu 'End on the slider': +82.8 ms object:property-change:accessible-value [slider] 'Opacity' / +171.2 ms ORCA SAYS: '1'`
  - `pass1 verify-menus-rich-keys 'Page_Up on the slider': +18.4 ms object:property-change:accessible-value [slider] 'Opacity' / +25.6 ms ORCA SAYS: '0.77'`
  - `pass1 verify-menus-rich-keys 'Home on the slider': +22.4 ms ORCA SAYS: '0'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/slider.rs:350-356 (value bound at AccessibilityOnly), 800 and 844-860 (the published value is put on the step's grid)
- **Evidence (`261a218f`):**
  - `menus-rich-menu '== Tab 10 inside the menu': '+11.4 ms object:state-changed:focused 1 [slider] 'Opacity'', '+81.0 ms ORCA SAYS: 'Opacity horizontal slider 0.6499999761581421.''`
  - `'== Right on the slider (focus reached by Tab)': 'FAIL  a object:property-change:accessible-value event from [slider] '*'' / 'no object:property-change:accessible-value event from [slider] '*''; the act carried 0 events (3 of 3 Tab-route runs; the AT-SPI-focus route 4 of 4)`
  - `note (bridge, after the acts, r3 and r4): 'with the menu still open, the application holds the Opacity slider at 0.6499999761581421 (node: {... 'focused': True, ... 'numeric_value': 0.6499999761581421, 'role': 'Slider'})'`
  - `crates/teksilo-widgets/src/slider.rs:592-618 (on_key), :445-452 (adjust_by_step), :791 (set_numeric_value(self.value.get() as f64))`
  - `verify-menus-rich-keys-20260925-133535-3547351 report, 'Tab to the embedded Theme combo box': '+10.1 ms object:property-change:accessible-value [slider] 'Opacity'', '+23.5 ms ORCA SAYS (CUT): '0''; orca-debug.out '13:36:43.475433 - SPEECH OUTPUT: '0''`
  - `same run: 'Enter on the embedded Previous button' pass 'the example ran 'Prev''; 'Space on the Pin toggle' '+30.8 ms object:state-changed:pressed 1 [toggle button] 'Pin (bistate)'', 'ORCA SAYS: 'pressed''`
  - `bridge note after the acts: "'label': 'Opacity', 'numeric_value': 0.0, 'role': 'Slider'"`
- **Reproduced:** Right via Tab focus 3 of 3; via AT-SPI focus 4 of 4; application state confirmed unchanged in 2 runs
- **Verification:** corrected by the verifier. Reproduced: rich-menu 3/3; verify-menus-rich-keys 3/3 (value published only on the next Tab, spoken '0') The symptom reproduces: no accessible-value event and nothing spoken on Right or End (rich-menu 3/3). The claim that the slider 'ignores the arrow keys' is wrong. In verify-menus-rich-keys (3/3), Right, Up, PageUp, Home and Left each produced no event. Then Tab to the embedded Theme combo box produced '+10.1 ms object:property-change:accessible-value \[slider\] 'Opacity'', and Orca said '0' (then cut). The value had changed all along; Home and Left had taken it to 0.0. After the acts the bridge read 'numeric\_value': 0.0. Keys reach embedded controls generally: Enter on Previous printed 'Prev', Space on Pin printed 'TogglePin' with state-changed:pressed, and Down on the embedded combo box said 'List with 4 items' / 'Light.' (3/3). The real defect is that a keyboard change of a Slider's value is not published to AT until an unrelated update (the next focus move) re-walks the node. The sweep's bridge read of 0.6499999761581421 was that same stale node: its runs made no focus move after the keys. Source: slider.rs never binds its value at BindingLevel::AccessibilityOnly (no BindingLevel reference in the file), while progress\_bar.rs:250-257 does exactly that 'so a progress update re-walks the AT tree'. The defect is not menu-specific. The catalog sweep saw the same on the widget-catalog touch-tab slider ('no accessible-value change on a slider in the act', target/reader-sweep/catalog-c/catalog-c-touch-20260925-122626-1598460). The raw-float part is confirmed: slider.rs:791 set\_numeric\_value(self.value.get() as f64) widens f32 0.65 to 0.6499999761581421, which Orca speaks ('13:28:33.100318 - SPEECH OUTPUT: 'Opacity horizontal slider 0.6499999761581421.''). Severity stays high: value changes are never spoken as they happen.
- **Fix idea:** Trace where the KeyDown goes when a slider inside a MenuList overlay has focus (a headless test: open the popover, focus the slider, press\_key(ArrowRight)). Round the published numeric value to the step's precision.

### menus-16 {#menus-16}

Five combo boxes have no name, and their placeholders are never spoken

- **Example:** menus-and-dropdowns
- **Scenario:** tabwalk, menus-combo-fruit, menus-combo-huge
- **Act:** Tab through the combo section, or focus the Fruit combo box (empty, placeholder 'Select a fruit...').
- **The reader should get:** 'Fruit combo box, Select a fruit...'.
- **The reader got (`261a218f`):** 'combo box.' at each of the four reachable combo boxes. Nothing says which is Fruit, Color, Country or Huge. The visible labels are separate TextWidgets and the example never calls ComboBox::label. The placeholder is exported as placeholder-text, but Orca 46's focused combo-box format has no placeholder field.
- **Platform:** Linux measured. The missing name affects every platform. The placeholder being dropped is Orca's format; NVDA reads UIA HelpText.
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** The Fruit, Color, Country and Huge combo boxes (and the disabled Size one) still have no name: each is spoken 'combo box.'. The placeholders ('Select a fruit...', 'Open me') are not spoken either.
- **Measured again:** tabwalk-menus-and-dropdowns, menus-combo-fruit, menus-combo-huge: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-combo-fruit launch tree audit: unnamed-control: [combo box] '': a focusable combo box with no name (5 times)`
  - `pass1 tabwalk-menus-and-dropdowns 'Tab 6': +16.2 ms object:state-changed:focused 1 [combo box] '' / +73.6 ms ORCA SAYS: 'combo box.'`
  - `pass2 tabwalk-menus-and-dropdowns 'Tab 9': +18.4 ms object:state-changed:focused 1 [combo box] '' / +68.8 ms ORCA SAYS: 'combo box.'`
  - `pass2 menus-combo-fruit 'focus the Fruit combo box (nothing selected)': +836.4 ms ORCA SAYS: 'combo box.' / FAIL Orca says 'Fruit' / FAIL Orca says 'Select a fruit'`
  - `pass1 menus-combo-huge 'focus the Huge combo box': +115.3 ms ORCA SAYS: 'combo box.' / FAIL Orca says 'Open me'`
- **Where (`c198e4d1`):** examples/menus\_and\_dropdowns/src/main.rs:118-223; crates/teksilo-widgets/src/combo\_box.rs:1331-1333 (name only from .label()), 1345-1349 (placeholder)
- **Evidence (`261a218f`):**
  - `tree audit (every run): 'unnamed-control: [combo box] '': a focusable combo box with no name' x5`
  - `tabwalk: 'Tab 6' .. 'Tab 9' each 'ORCA SAYS: 'combo box.''`
  - `menus-combo-fruit: 'FAIL  Orca says 'Fruit'', 'FAIL  Orca says 'Select a fruit'' (3 of 3); menus-combo-huge 'FAIL  Orca says 'Open me'' (3 of 3)`
  - `examples/menus_and_dropdowns/src/main.rs:108-126 (TextWidget 'Fruit' beside ComboBox::new(...).placeholder(...), no .label()); crates/teksilo-widgets/src/combo_box.rs:1252-1254 (name only from .label()); Orca formatting.py:185-187`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: deterministic
- **Fix idea:** Example: .label(lit!("Fruit")) and so on for each combo box (or labelled\_by the TextWidget). Framework: ComboBox could fall back to its placeholder as a description so it is read.

### menus-17 {#menus-17}

No 'has popup' and no 'expanded/collapsed' on AT-SPI: menu-bar items, the Add button and combo boxes give no cue that they open anything

- **Example:** menus-and-dropdowns
- **Scenario:** menus-menubar-f10, tabwalk
- **Act:** F10 onto File; Tab onto Add; Tab onto a combo box; open and close menus and lists.
- **The reader should get:** 'File menu' or 'File, has submenu / collapsed', 'Add menu button', 'combo box collapsed' / 'expanded'.
- **The reader got (`261a218f`):** 'File.', 'Add push button.', 'combo box.'. Teksilo sets has\_popup and expanded on every trigger, but accesskit\_atspi\_common 0.20 exports neither: no Expandable, Expanded or HasPopup state.
- **Platform:** Linux only. accesskit\_windows exposes ExpandCollapse (node.rs:714-723, 1485-1486).
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** Still 'File.', 'Add push button.' and 'combo box.': no cue that they open a menu or a list, and no collapsed or expanded state. accesskit\_atspi\_common 0.21.0 still exports no Expandable, Expanded or HasPopup state.
- **Measured again:** menus-menubar-f10, tabwalk-menus-and-dropdowns: 2 of 2 runs each; adapter read from source
- **Evidence (`c198e4d1`):**
  - `pass1 menus-menubar-f10 'F10': +59.2 ms object:state-changed:focused 1 [menu item] 'File' / +93.1 ms ORCA SAYS: 'File.'`
  - `pass2 tabwalk-menus-and-dropdowns 'Tab 10': +28.7 ms object:state-changed:focused 1 [push button] 'Add' / +141.1 ms ORCA SAYS: 'Add push button.'`
  - `pass1 tabwalk-menus-and-dropdowns 'Tab 7': +26.4 ms object:state-changed:focused 1 [combo box] '' / +89.9 ms ORCA SAYS: 'combo box.'`
  - `accesskit_atspi_common-0.21.0/src/node.rs:301-389: the state set has no Expandable, Expanded or HasPopup (no match for Expand or HasPopup in the crate)`
- **Where (`c198e4d1`):** accesskit\_atspi\_common-0.21.0/src/node.rs:301-389; crates/teksilo-widgets/src/menu\_bar/trigger.rs:213-215; crates/teksilo-widgets/src/combo\_box.rs:1329, 1353
- **Evidence (`261a218f`):**
  - `menus-menubar-f10 '== F10': '+158.9 ms ORCA SAYS: 'File.'' (3 of 3); tabwalk: 'ORCA SAYS: 'Add push button.'', 'ORCA SAYS: 'combo box.''`
  - `tree-launch.txt: '[menu item] 'File' {focusable}' with states ['enabled', 'focusable', 'sensitive', 'showing', 'visible'] and no attributes`
  - `accesskit_atspi_common-0.20.0/src/node.rs:300-380 (the state set has no Expandable/Expanded/HasPopup); crates/teksilo-widgets/src/menu_bar/trigger.rs:213-215 (set_has_popup, set_expanded)`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: deterministic
- **Fix idea:** Upstream: map has\_popup and is\_expanded to AT-SPI HAS\_POPUP / EXPANDABLE / EXPANDED. Nothing to do in Teksilo beyond keeping the properties set.

### menus-18 {#menus-18}

The searchable combo's search field is unnamed and filtering is silent

- **Example:** menus-and-dropdowns
- **Scenario:** menus-country-search
- **Act:** Focus the Country combo box, Enter, type 'fr'.
- **The reader should get:** 'Search countries, entry' (or similar), then how many match, or the first match.
- **The reader got (`261a218f`):** Orca says 'list box' and 'Search…' (the placeholder of an entry named ''). Typing filters the list with no announcement; 'France' is heard only after Down.
- **Platform:** Linux AT-SPI / Orca 46.1, measured.
- **Severity:** medium; **layer:** framework
- **Status:** Partly fixed by `f9ffa98c` (combobox). What remains is under **Now**.
- **Now (`c198e4d1`):** The search field is named now: Enter says 'Search editable combo box.'. Filtering is still silent: typing 'fr' gives only the key echo 'r', and France is heard only after Down.
- **Measured again:** menus-country-search, fix-combobox-country: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-country-search 'Enter opens the searchable list': +104.0 ms object:state-changed:focused 1 [entry] 'Search' / +164.6 ms ORCA SAYS: 'Search editable combo box.'`
  - `pass2 menus-country-search 'type 'fr'': +492.5 ms ORCA SAYS: 'r' / FAIL Orca says 'France'`
  - `pass1 menus-country-search 'Down: the first match': +137.1 ms ORCA SAYS: 'List with 1 item' / +137.1 ms ORCA SAYS: 'France.'`
  - `pass2 fix-combobox-country 'Enter opens the searchable list': +152.6 ms ORCA SAYS: 'Search editable combo box.' / pass the tree holds no [unknown] '*'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box/panel.rs:645-651 (the named search field), 664-668 (typing drops the highlight; nothing announces the matches)
- **Evidence (`261a218f`):**
  - `orca-debug.out (menus-country-search, third run): '13:18:48.212065 - SPEECH OUTPUT: 'list box'', '13:18:48.212085 - SPEECH OUTPUT: 'Search…''`
  - `tree after Enter: entry '' attributes {'setsize': '30', 'placeholder-text': 'Search…'}`
  - `'== type 'fr'': 'FAIL  Orca says 'France'' (3 of 3); '== Down: the first match': 'pass  Orca says 'France'' (3 of 3)`
- **Reproduced:** deterministic; 3 of 3
- **Verification:** confirmed. Reproduced: 3/3
- **Fix idea:** Name the search TextInput after the combo's label, and announce the match count (or the first match) as the filter changes.

### menus-19 {#menus-19}

The combo box's controller-for relation points at an unnamed role-less node, which sits between the combo box and its list

- **Example:** menus-and-dropdowns
- **Scenario:** menus-combo-reopen
- **Act:** Open any combo box list and read the tree.
- **The reader should get:** controller-for names the list box.
- **The reader got (`261a218f`):** It names an '\[unknown\]' node, the DeferredSubtree host, which, once built, emits no role. That node sits in the reader's tree between the combo box and its list box.
- **Platform:** Linux measured. The dangling relation target and the Role::Unknown node are in the AccessKit tree on every platform.
- **Severity:** low; **layer:** framework
- **Status:** Fixed by `f9ffa98c`, found by the re-measure: that commit did not report it.
- **Now (`c198e4d1`):** The list box is now the combo box's direct child, with no '\[unknown\]' node in between, and controller-for names the list box.
- **Measured again:** menus-combo-reopen, menus-combo-color, menus-combo-fruit, fix-combobox-country: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-combo-reopen 'Alt+Down (first open)': +32.8 ms object:children-changed:add [combo box] '' -> [list box] ''`
  - `pass1 menus-combo-reopen run.json tree after 'Alt+Down (first open)': [combo box] relations {'controller-for': ['/org/a11y/atspi/accessible/0/79228168970624763391887015936']}, the path of [list box] ''`
  - `pass2 menus-combo-fruit 'Down from nothing selected': +89.4 ms object:children-changed:add [combo box] '' -> [list box] ''`
  - `pass1 fix-combobox-country 'Enter opens the searchable list': pass the tree holds no [unknown] '*'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box.rs:1355-1365 (controls the list box itself)
- **Evidence (`261a218f`):**
  - `combo box relations {'controller-for': ['/org/a11y/atspi/accessible/0/79228163123006892025959153664']}; that path: 'unknown '' ... ['Accessible', 'Component']'; the list box: '/org/a11y/atspi/accessible/0/79228168970624763391887015936'`
  - `events: '+33.7 ms object:children-changed:add [combo box] '' -> [unknown] '''`
  - `crates/teksilo-widgets/src/combo_box.rs:790 (dropdown_content_id is the add_deferred host) and :1282 (push_controlled(dropdown_content_id)); crates/teksilo-core/src/deferred_subtree.rs:222-226 (accessibility delegates only while pending)`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: deterministic (every combo opening in combo-color, combo-fruit, combo-reopen, country-search, combo-huge)
- **Fix idea:** Give the built DeferredSubtree host Role::GenericContainer, so the filter drops it, and point controls at the DropdownPanel's own id.

### menus-v1 {#menus-v1}

Every menu-bar trigger is its own Tab stop, so Tab walks File, Edit and View before reaching content

- **Example:** menus-and-dropdowns
- **Act:** Tab through the window (reader.py tabwalk menus-and-dropdowns).
- **The reader should get:** The menu bar is one Tab stop (WAI-ARIA menubar, roving tabindex), or outside the Tab order and reached with F10 or Alt, as on GTK and Windows.
- **The reader got (`261a218f`):** The stops are 'File.', 'Edit.', 'View.', 'Settings push button.', then 'Theme combo box.' and the content. Each trigger is focusable on its own, so every Tab cycle passes three menu triggers. This also makes menus-09 worse: after Escape or a command, the reader has to Tab out through the bar.
- **Platform:** All platforms (widget logic); measured on Linux with Orca.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The Tab order still starts 'File.', 'Edit.', 'View.', 'Settings push button.' before the toolbar's 'Theme combo box.', and after View options Tab comes back to 'File.'.
- **Measured again:** tabwalk-menus-and-dropdowns, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabwalk-menus-and-dropdowns 'Tab 1': +79.8 ms object:state-changed:focused 1 [menu item] 'File' / +119.1 ms ORCA SAYS: 'File.'`
  - `pass1 tabwalk-menus-and-dropdowns 'Tab 2' / 'Tab 3': ORCA SAYS: 'Edit.' / ORCA SAYS: 'View.'`
  - `pass2 tabwalk-menus-and-dropdowns 'Tab 5': +66.9 ms ORCA SAYS: 'Toolbar tool bar' / +67.0 ms ORCA SAYS: 'Theme combo box.'`
  - `pass2 tabwalk-menus-and-dropdowns 'Tab 13': +15.5 ms object:state-changed:focused 1 [menu item] 'File' / +93.4 ms ORCA SAYS: 'File.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_bar/trigger.rs:157
- **Evidence (`261a218f`):**
  - `tabwalk-menus-and-dropdowns-20260925-133923-3745945 report: '+14.2 ms object:state-changed:focused 1 [menu item] 'File'' / 'ORCA SAYS: 'File.'', then 'Edit.', 'View.', 'Settings push button.', 'Toolbar tool bar' 'Theme combo box.'`
  - `crates/teksilo-widgets/src/menu_bar/trigger.rs:157 (.focusable(true) on every MenuBarTrigger; no roving tab index in menu_bar/)`
- **Reproduced:** 1/1 tab walk (deterministic by construction)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Make the bar one Tab stop with a roving tab index across its triggers, or leave the triggers out of Tab order and reach them only with F10, Alt or Alt+letter.

### menus-v2 {#menus-v2}

The searchable combo box's search field placeholder is an English literal in framework code

- **Example:** menus-and-dropdowns
- **Act:** Open the Country combo box with Enter.
- **The reader should get:** The search field's hint follows the application's locale.
- **The reader got (`261a218f`):** Orca says 'Search…' from lit!("Search…") inside ComboBox's panel, in every locale. I measured it only in English; that other locales get it is inferred from the source.
- **Platform:** All platforms (by source); measured in English on Linux.
- **Severity:** low; **layer:** framework
- **Status:** Fixed by `f9ffa98c`, found by the re-measure: that commit did not report it.
- **Now (`c198e4d1`):** The search field's placeholder now comes from the localized string a11y-builtin-search ('Search' in English, 'Rechercher' in French). So does its name, unless the combo box has a label of its own, which then names it. Measured in English only: Orca says 'Search editable combo box.'.
- **Measured again:** menus-country-search, 2 of 2 runs; the other locales read from source
- **Evidence (`c198e4d1`):**
  - `pass1 menus-country-search 'Enter opens the searchable list': +104.0 ms object:state-changed:focused 1 [entry] 'Search'`
  - `pass1 menus-country-search 'Enter opens the searchable list': +164.6 ms ORCA SAYS: 'Search editable combo box.'`
  - `pass2 menus-country-search 'Enter opens the searchable list': +119.1 ms ORCA SAYS: 'Search editable combo box.'`
  - `crates/teksilo-widgets/src/combo_box/panel.rs:646-647 '.label(self.search_label.clone())' then '.placeholder(teksilo_i18n::tr_widget!(a11y_builtin_search()))': the placeholder is always the localized string`
  - `crates/teksilo-widgets/src/combo_box.rs:831-834 'search_label: self.label.clone().unwrap_or_else(|| teksilo_i18n::tr_widget!(a11y_builtin_search()))': only the name falls back from the label`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box/panel.rs:646-647; crates/teksilo-widgets/src/combo\_box.rs:831-834; crates/teksilo-widgets/locales/en-US.ftl:54
- **Evidence (`261a218f`):**
  - `menus-country-search (3/3) orca-debug.out: '13:30:45.421568 - SPEECH OUTPUT: 'Search…''`
  - `crates/teksilo-widgets/src/combo_box/panel.rs:565 '.placeholder(lit!("Search…"))'`
- **Reproduced:** 3/3 spoken; the locale point is from source
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Take the hint from a localized framework string, or let the application set it (ComboBox::search\_placeholder), and name the field (menus-18).

### menus-v3 {#menus-v3}

The first character typed into the combo's search field is never reported as inserted text

- **Example:** menus-and-dropdowns
- **Act:** Country combo box, Enter, type 'fr'.
- **The reader should get:** A text-changed:insert event for each character ('f', then 'r').
- **The reader got (`261a218f`):** Only 'r' is reported ('object:text-changed:insert \[entry\] '' text='r''). For 'f' the bus carries only the list's refilter (the inner list box removed, 60 options defunct) and a caret move to 1. Orca's key echo on a real keyboard comes from key events, so the loss is small, but a client that follows text changes (braille, or Orca's handling of inserted text) misses the first character. Cause not traced. atspi\_common reports a text change only when both the old and new node support text ranges (adapter.rs:120-123), and Teksilo's run emitter does emit one run for empty text (text\_runs.rs test empty\_text\_emits\_one\_run\_with\_a\_caret\_rect).
- **Platform:** Linux AT-SPI, measured.
- **Severity:** low; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Now (`c198e4d1`):** Both characters typed into the search field are now reported as inserted text: 'f', then 'r'.
- **Measured again:** menus-country-search, fix-combobox-country: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 menus-country-search 'type 'fr'': +44.0 ms object:text-changed:insert [entry] 'Search' text='f' / +160.5 ms object:text-changed:insert [entry] 'Search' text='r'`
  - `pass2 menus-country-search 'type 'fr'': +81.4 ms object:text-changed:insert [entry] 'Search' text='f' / +309.4 ms object:text-changed:insert [entry] 'Search' text='r'`
  - `pass1 fix-combobox-country 'type 'fr'': +34.6 ms object:text-changed:insert [entry] 'Search' text='f'`
- **Where (`c198e4d1`):** crates/teksilo-core/src/accessibility/text\_runs.rs:269-281 (a text measured with no lines, such as an empty field, now gets one run, so the adapter reports the first character inserted); crates/teksilo-widgets/src/combo\_box/panel.rs:645-651 (the search field, a TextInput)
- **Evidence (`261a218f`):**
  - `menus-country-search, all 3 runs: act 'type 'fr'' inserts [('r', ...)] only, caret-moved detail1 1 then 2`
  - `menus-country-search-20260925-132717-3048174 report: '+47.6 ms object:text-changed:insert [entry] '' text='r''`
- **Reproduced:** 3/3
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Check that the empty search field emits its empty text run before the first keystroke, and that the refilter of the list, which runs in the same update, does not swallow the entry's change.
