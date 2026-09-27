<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Generated from the sweep of 25 and 26 September 2026 (on 261a218f) and its re-measure of 27 September 2026 (on c198e4d1). See ../reader-findings.md. -->

# Lists and trees

Examples: `data-collections`.
18 findings: 7 high, 6 medium, 5 low.
Swept on `261a218f` on 25 and 26 September 2026, measured again on
`c198e4d1` on 27 September 2026.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [collections-01](#collections-01) | data-collections | ListView: a cursor-only move (Ctrl+Up/Down) is never published, so the reader hears nothing and AT focus stays on the old row | high | Linux | open |
| [collections-02](#collections-02) | data-collections | Toggling a row's Checkbox (Space, or a screen reader's own AT-SPI click) never reaches the bus; the tree keeps the old state until something else rebuilds the row | high | Linux | fixed |
| [collections-03](#collections-03) | data-collections | A row's checked state lives on a nested check box, not on the focused row, so Orca never says 'checked' / 'not checked' in a list or a tree | high | Linux | open |
| [collections-04](#collections-04) | data-collections | The move / reparent announcement is sent in the same update as a focus change to a freshly built row, so Orca cuts it even when it survives (independent of K2) | high | Linux | fixed |
| [collections-05](#collections-05) | data-collections | Row context menu (Shift+F10): focus stays on an unnamed menu container and arrowing between commands emits nothing | low | Linux | partly fixed |
| [collections-06](#collections-06) | data-collections | TreeView on Linux: expanded/collapsed state, level and the Expand/Collapse actions never reach AT-SPI | high | Linux | upstream |
| [collections-07](#collections-07) | data-collections | A row's subtitle is never heard: the name is hoisted onto the row, its description is left on an 'unknown' child | medium | Linux | open |
| [collections-08](#collections-08) | data-collections | The Role::Group between ListBox and its options hides 'not selected', the row's embedded widgets and the list size from Orca | medium | Linux | open |
| [collections-09](#collections-09) | data-collections | Virtualization: Orca's position counts only the realized rows ('1 of 17', 'Item 200' is '9 of 9') although the rows publish posinset/setsize of 200 | medium | Linux | upstream |
| [collections-10](#collections-10) | data-collections | Row node churn: every TreeView key press re-creates every realized row, and a ListView reorder re-creates the whole pane | medium | Linux | open |
| [collections-11](#collections-11) | data-collections | Move / reparent announcements and the row move-menu labels are English literals, and they never say what moved unless the app sets a type-ahead label | medium | all | open |
| [collections-12](#collections-12) | data-collections | The ListView, the Auto Feed list and the TreeView have no accessible name | medium | Linux | open (example) |
| [collections-13](#collections-13) | data-collections | Auto Feed rows read as '#1', '#2': the message text a row shows is never heard | high | Linux | open (example) |
| [collections-14](#collections-14) | data-collections | Repeater tab: the tags are unnamed panels holding separate '1.' and 'Rust' labels, not a list; Add/Remove Tag are silent | low | Linux | open (example) |
| [collections-15](#collections-15) | data-collections | Row internals clutter the exported tree: a nameful 'unknown'-role node per row, 'focusable' check boxes outside the Tab order, setsize on every descendant | low | Linux | open |
| [collections-m1](#collections-m1) | data-collections | ListView and TreeView take focus with no active descendant when nothing is selected, so the reader hears only the container and no row | low | Linux | open |
| [collections-m2](#collections-m2) | data-collections | TreeView level is never spoken on Linux, even as the level changes: no NODE\_CHILD\_OF relations reach AT-SPI | high | Linux | upstream |
| [collections-m3](#collections-m3) | data-collections | The row context menu is an unnamed menu, and the reader is never told which command a keyboard press will run | low | Linux | partly fixed |

### collections-01 {#collections-01}

ListView: a cursor-only move (Ctrl+Up/Down) is never published, so the reader hears nothing and AT focus stays on the old row

- **Example:** data-collections
- **Scenario:** collections-listview-cursor
- **Act:** ListView tab: Tab into the list, Down (row 1 selected), Ctrl+Down, wait 3 s, Ctrl+Down again, then Ctrl+Space
- **The reader should get:** Each Ctrl+Down moves the list's active descendant to the next row (the visible focus ring does move) and the reader hears 'Item 2', then 'Item 3', each 'not selected'.
- **The reader got (`261a218f`):** No AT-SPI event at all for either Ctrl+Down. Three seconds later the bus still has 'Item 1' as the focused row. The stale focus move arrives only with the next update that re-walks the tree for another reason: with Ctrl+Space, Orca says 'Item 3.' as if the cursor had just jumped there. Ctrl+End does work, because it scrolls and the scroll rebuilds the pane. Seen in 3 of 3 runs. The cause is in Teksilo before any adapter, because no TreeUpdate is produced, so the same silence is expected from UIA/NVDA and VoiceOver; that was reasoned from the source, not measured.
- **Platform:** Linux AT-SPI/Orca measured; Windows/macOS by source (no TreeUpdate is produced at all)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Ctrl+Down and Ctrl+Home still move only the visible cursor. Nothing reaches the bus, Orca says only its echo of the Control key, and three seconds later the bus still has the old row as focused. The move surfaces only with the next update that re-walks the tree for another reason: Ctrl+Space makes Orca say 'Item 3.' as if the cursor had just jumped there, and Shift+Tab back into the list lands on the cursor row.
- **Measured again:** collections-listview-cursor, 2 of 2 runs; verify-collections-cursor-noscroll, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-cursor 'Ctrl+Down: cursor only, row 2': FAIL focus lands on [list item] 'Item 2' / no focus change on the bus in this act; +5.5 ms ORCA SAYS (CUT): 'left control'`
  - `pass1 collections-listview-cursor 'nothing pressed, 3 s later': FAIL the bus holds [list item] 'Item 2' as focused / focused on the bus: ["[list item] 'Item 1'"]`
  - `pass1 collections-listview-cursor 'Ctrl+Down again: row 3': no focus change on the bus in this act; +5.8 ms ORCA SAYS (CUT): 'left control'`
  - `pass1 collections-listview-cursor 'Ctrl+Space: add the cursor's row to the selection': +39.1 ms object:state-changed:focused 1 [list item] 'Item 3', +39.2 ms object:state-changed:focused 0 [list item] 'Item 1'; +152.5 ms ORCA SAYS: 'Item 3.'`
  - `pass2 collections-listview-cursor 'Ctrl+Down: cursor only, row 2': no focus change on the bus in this act; +6.0 ms ORCA SAYS (CUT): 'left control'; 'nothing pressed, 3 s later': focused on the bus: ["[list item] 'Item 1'"]; 'Ctrl+Space': +40.5 ms object:state-changed:focused 1 [list item] 'Item 3', +138.6 ms ORCA SAYS: 'Item 3.'`
  - `pass1 verify-collections-cursor-noscroll 'Ctrl+Home: cursor only, row 1, no scroll needed': no focus change on the bus in this act; +5.4 ms ORCA SAYS (CUT): 'left control'; 'nothing pressed, 3 s later': focused on the bus: ["[list item] 'Item 3'"]; 'Shift+Tab back into the list': +30.7 ms object:state-changed:focused 1 [list item] 'Item 2', +99.8 ms ORCA SAYS: 'Item 2.'`
  - `pass2 verify-collections-cursor-noscroll 'Ctrl+Home: cursor only, row 1, no scroll needed': no focus change on the bus in this act; +43.7 ms ORCA SAYS (CUT): 'left control'; 'Ctrl+Down: cursor only, row 2': no focus change on the bus in this act; 'Shift+Tab back into the list': +93.3 ms object:state-changed:focused 1 [list item] 'Item 2'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/list\_view/widget\_impl.rs:587-622 (fi.set + SelectionOp::Suppress in the nav arm; Ctrl+Up/Down matched at :608), :508-530 (Ctrl+Space arm), :1058-1062 (active\_descendant computed at walk time); crates/teksilo-widgets/src/list\_view.rs:160 (focused\_index is a Cell), :666-676 (current\_row\_widget); crates/teksilo-widgets/src/list\_view/body\_pane.rs:70; crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs:30-124 (sync\_accessibility re-walks only when the tree is dirty)
- **Evidence (`261a218f`):**
  - `run collections-listview-cursor-20260925-131003-2404500 report.txt: '== Ctrl+Down: cursor only, row 2' -> 'FAIL  focus lands on [list item] 'Item 2'' / 'no focus change on the bus in this act'`
  - `same run: '== nothing pressed, 3 s later' -> 'FAIL  the bus holds [list item] 'Item 2' as focused' / 'focused on the bus: ["[list item] 'Item 1'"]'`
  - `same run: '== Ctrl+Down again: row 3' -> 'no focus change on the bus in this act'`
  - `pass-1 run collections-listview-cursor-20260925-130316-2120746, Ctrl+Space act: '+35.1 ms object:state-changed:selected 1 [list item] 'Item 3'', '+38.6 ms object:state-changed:focused 1 [list item] 'Item 3'', '+38.7 ms object:state-changed:focused 0 [list item] 'Item 1''`
  - `Orca (run ...131003...): '13:10:43.084204 NULL SPEECH: stop' then '13:10:43.084346 SPEECH OUTPUT: 'Item 3.'' (in the Ctrl+Space act, not in either Ctrl+Down act)`
  - `crates/teksilo-widgets/src/list_view.rs:160 focused_index is Rc<Cell<Option<usize>>> (not a Signal, so nothing is bound to it)`
  - `crates/teksilo-widgets/src/list_view/widget_impl.rs:586 fi.set(Some(idx)); :604-616 Ctrl+Arrow -> SelectionOp::Suppress (selection untouched, so no row rebuilds)`
  - `list_view/widget_impl.rs:1051-1055 active_descendant is computed at walk time from current_row_widget() (list_view.rs:662-672), but no walk happens`
  - `crates/teksilo-core/src/widget_tree/accessibility_impl.rs:23-37 sync_accessibility re-walks only on focus move / rebuild / dormancy / AccessibilityOnly binding / request_accessibility_update; the ListView key handler never calls request_accessibility_update (no occurrence under list_view/)`
  - `verify-collections-cursor-noscroll-20260925-133722-3645182 report.txt '== Ctrl+Home: cursor only, row 1, no scroll needed' -> 'FAIL  focus lands on [list item] 'Item 1'' / 'no focus change on the bus in this act'; '== nothing pressed, 3 s later' -> 'focused on the bus: ["[list item] 'Item 3'"]'`
  - `same run '== Shift+Tab back into the list': '+23.6 ms object:state-changed:focused 1 [list item] 'Item 2'', ORCA SAYS 'multi-select list box' 'Item 2.'`
  - `collections-listview-cursor-20260925-133209-3310470 Ctrl+Space act: '+34.4 ms object:state-changed:selected 1 [list item] 'Item 3'', '+36.7 ms object:state-changed:focused 1 [list item] 'Item 3''; orca-debug.out:2039 '13:32:43.018160 - SPEECH OUTPUT: 'Item 3.''`
  - `crates/teksilo-widgets/src/list_view/body_pane.rs:68-70 (focused_index shared only with the key and pointer handlers); list_view/widget_impl.rs:956-970 (container ring only when !has_selection)`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs of collections-listview-cursor (Ctrl+Down twice: no event); 2 of 2 runs of verify-collections-cursor-noscroll (Ctrl+Home from row 3 and Ctrl+Down: no event, bus still focused on 'Item 3' 3 s later). Deterministic. Real, and wider than reported. The cursor does move: Ctrl+Space then selects row 3, not row 1. But no TreeUpdate is produced, because focused\_index is a plain Cell and nothing requests an accessibility walk. Two corrections. (1) The whole cursor-only family is silent whenever no scroll is needed, not just Ctrl+Up/Down. Ctrl+Home from row 3 back to row 1 (already on screen) sent no event in 2 of 2 runs. The sweep's 'Ctrl+End does work' is true only because that jump scrolls, and the scroll rebuilds the pane. (2) The sweep says 'the visible focus ring does move'. By source that is wrong: no row paints a cursor ring. The only ring is the container outline, painted while nothing is selected (widget\_impl.rs:956-970), and StandardListItem paints only the selection. So the cursor-only move is probably invisible on screen as well; the harness cannot show that, so it is not verified. The stale state does catch up on the next re-walk: Shift+Tab back into the list lands on the cursor row ('Item 2', 2 of 2 runs). The platform claim is right: the defect is before any adapter, so Windows and macOS get no update either (by source).
- **Fix idea:** Make focused\_index a Signal bound at BindingLevel::AccessibilityOnly on the ListView, or call ctx.request\_accessibility\_update() wherever the key handler writes fi (and in the Space/Ctrl+Space arms). Check TreeView and the other data views for the same Cell.

### collections-02 {#collections-02}

Toggling a row's Checkbox (Space, or a screen reader's own AT-SPI click) never reaches the bus; the tree keeps the old state until something else rebuilds the row

- **Example:** data-collections
- **Scenario:** collections-listview-checkbox, collections-tree-nav
- **Act:** ListView: cursor on 'Item 2' (a checkbox row), press Space; wait 3 s; Up+Space again; then an AT-SPI 'click' on row 4's \[check box\]. TreeView: cursor on the leaf 'Teksilo', press Space.
- **The reader should get:** An object:state-changed:checked from the check box, and the bus reports it as checked right away.
- **The reader got (`261a218f`):** No checked event after Space or after the AT-SPI click. Three seconds later the bus still reports \[check box\] 'Item 2' without 'checked'. The state reaches the bus late, in two ways: a later unrelated update (the next Down) carries 'object:state-changed:checked 1 \[check box\] 'Item 4''; a row rebuilt by a selection change is re-created already checked, with no event at all. Until then the bus reports the wrong state, and Space again (uncheck) left the bus saying 'checked'. The TreeView leaf behaves the same way. ListView 3 of 3 runs, TreeView 3 of 3 runs.
- **Platform:** Linux measured; all platforms by source (no TreeUpdate is produced)
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Now (`c198e4d1`):** Space on a row's check box, and a screen reader's own click on it, now put object:state-changed:checked on the bus at once, in both directions, in the ListView and the TreeView, and the tree holds the new state three seconds later. Nothing stale arrives with the next key. Orca still says nothing for the change, which is collections-03.
- **Measured again:** collections-listview-checkbox, 2 of 2 runs; collections-tree-nav, 2 of 2 runs; verify-collections-tree-levels, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-checkbox 'Space: check row 2': +31.7 ms object:state-changed:checked 1 [check box] 'Item 2'; pass the [check box] 'Item 2' in the tree is checked`
  - `pass1 collections-listview-checkbox 'nothing pressed, 3 s later': pass the [check box] 'Item 2' in the tree is checked`
  - `pass1 collections-listview-checkbox 'Up and Space again: uncheck row 2': +200.4 ms object:state-changed:checked 0 [check box] 'Item 2'`
  - `pass1 collections-listview-checkbox 'AT-SPI click on row 4's check box': +33.2 ms object:state-changed:checked 1 [check box] 'Item 4'`
  - `pass2 collections-listview-checkbox 'Space: check row 2': +18.7 ms object:state-changed:checked 1 [check box] 'Item 2'; 'AT-SPI click on row 4's check box': +45.4 ms object:state-changed:checked 1 [check box] 'Item 4'`
  - `pass1 collections-tree-nav 'Space: check the leaf': +15.0 ms object:state-changed:checked 1 [check box] 'Teksilo', +15.1 ms object:state-changed:indeterminate 1 [check box] 'Projects'`
  - `pass2 collections-tree-nav 'Space: check the leaf': +15.6 ms object:state-changed:checked 1 [check box] 'Teksilo'`
  - `pass1 verify-collections-tree-levels 'Space on Pictures (a branch with a tristate box)': +12.4 ms object:state-changed:checked 1 [check box] 'Pictures'; 'nothing pressed, 3 s later': pass the [check box] 'Pictures' in the tree is checked`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/checkbox.rs:389-396 (check state bound at AccessibilityOnly), :653-676 (accessibility reads check\_state at walk time)
- **Evidence (`261a218f`):**
  - `run collections-listview-checkbox-20260925-131012-2411115: '== Space: check row 2' -> 'FAIL  a object:state-changed:checked event from [check box] 'Item 2'' / 'no object:state-changed:checked event from [check box] 'Item 2''`
  - `same run '== nothing pressed, 3 s later' -> 'FAIL  the [check box] 'Item 2' in the tree is checked' / '[check box] 'Item 2' desc=None states=['checkable', 'enabled', 'focusable', 'sensitive', 'showing', 'visible']'`
  - `same run '== AT-SPI click on row 4's check box' -> '+49.8 ms == harness:action click [check box] 'Item 4'' and 'no object:state-changed:checked event from [check box] 'Item 4''`
  - `same run, next act '== Down: the next update after the AT-SPI click': '+42.0 ms object:state-changed:checked 1 [check box] 'Item 4''`
  - `pass-1 run ...130316-2120779 tree after 'Up and Space again: uncheck row 2': '[check box] 'Item 2' {checkable,checked,focusable}' (model already unchecked)`
  - `run collections-tree-nav-20260925-131115-2411115 '== Space: check the leaf': events: [] ; 'FAIL  the [check box] 'Teksilo' in the tree is checked'`
  - `crates/teksilo-widgets/src/checkbox.rs:644-667 accessibility() reads self.check_state() at walk time; checkbox.rs binds nothing at BindingLevel::AccessibilityOnly (the check state feeds only the style body's repaint via style_state, checkbox.rs:383)`
  - `toggle paths that only set the signal: checkbox.rs:566-579 (AccessAction Click), checkbox.rs:600-612 (set_keyboard_toggle, used by the data views' Space)`
  - `contrast crates/teksilo-widgets/src/icon_button.rs:656 toggled.bind_to(self_id, registry, BindingLevel::AccessibilityOnly)`
  - `collections-listview-checkbox-20260925-133312-3310470: tree-Space--check-row-2.txt line 26 '[check box] 'Item 2' {checkable,focusable}'; tree-Down--the-next-update.txt line 26 '[check box] 'Item 2' {checkable,checked,focusable}' with no checked event in that act (row rebuilt, 'defunct 1 [check box] 'Item 2'')`
  - `same run tree-Up-and-Space-again--uncheck-row-2.txt '[check box] 'Item 2' {checkable,checked,focusable}' after Space unchecked it`
  - `same run 'Down: the next update after the AT-SPI click': '+35.0 ms object:state-changed:checked 1 [check box] 'Item 4''`
  - `verify-collections-tree-levels-20260925-133902-3645182 'Space on Pictures': 'no object:state-changed:checked event from [check box] 'Pictures''; 3 s later still '[check box] 'Pictures' ... states=['checkable', 'enabled', 'focusable', ...]'`
  - `catalog-a-toggles-20260925-133703-3588263 (other agent, 1 run): 'Space on the two-state check box' -> 'no object:state-changed:checked event'; the next scene step: '+41.7 ms object:state-changed:checked 1 [check box] 'Two-state checkbox'' then 'ORCA SAYS (CUT): 'checked''`
- **Reproduced:** ListView 3 of 3 runs; TreeView 3 of 3 runs
- **Verification:** confirmed. Reproduced: ListView 3 of 3 runs; TreeView leaf 3 of 3 runs; TreeView branch (tristate 'Pictures') 2 of 2 runs of verify-collections-tree-levels
- **Fix idea:** In Checkbox::build, bind kind.check\_state\_signal() at BindingLevel::AccessibilityOnly, as IconButton binds its toggled. A standalone focused Checkbox probably has the same staleness; this sweep measured only the embedded one.

### collections-03 {#collections-03}

A row's checked state lives on a nested check box, not on the focused row, so Orca never says 'checked' / 'not checked' in a list or a tree

- **Example:** data-collections
- **Scenario:** collections-listview-arrows, collections-listview-checkbox, collections-tree-nav
- **Act:** Arrow onto a checkbox row ('Item 2' in ListView, 'Teksilo' / 'Projects' in TreeView); Space on it
- **The reader should get:** The reader hears 'Item 2, not checked' on arrival and 'checked' on Space (a checkable option, as a GTK check list reads), and 'Projects, partially checked' for the mixed branch.
- **The reader got (`261a218f`):** Orca says only 'Item 2.' / 'Teksilo.' / 'Projects.'. The focused \[list item\] / \[tree item\] carries no checkable/checked state. The state sits two levels down on '\[check box\] 'Item 2' {checkable,focusable}'. Orca's list-item speech reads checkedStateIfCheckable from the item itself. It reads the item's embedded widgets only when the item's parent is a list box, and here the parent is a panel (see collections-08). A tree item's format has no widgets at all. Orca presents a checked change only for the locus of focus, so even an on-time event from the nested check box (collections-02) would be silent. 3 of 3 runs for each view.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Orca still says only 'Item 2.', 'Teksilo.' and 'Projects.' when the cursor reaches a checkbox row, and nothing when Space checks it. The focused list item or tree item carries no checkable or checked state; the state sits on the nested check box two levels down. That check box's event now arrives at once (collections-02), but Orca presents a checked change only for the locus of focus, so Space stays silent.
- **Measured again:** collections-listview-arrows, 2 of 2 runs; collections-listview-checkbox, 2 of 2 runs; collections-tree-nav, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-arrows 'Down: second row (a checkbox row)': +298.3 ms ORCA SAYS: 'Item 2.'; FAIL [list item] 'Item 2' carries state 'checkable' / states=['enabled', 'focused', 'selectable', 'selected', ...]; grandchild [check box] 'Item 2' states=['checkable', 'enabled', 'focusable', ...]`
  - `pass1 collections-listview-checkbox 'Space: check row 2': +31.7 ms object:state-changed:checked 1 [check box] 'Item 2'; FAIL Orca says 'checked' (not 'not checked') / Orca said nothing in the act`
  - `pass2 collections-listview-checkbox 'Space: check row 2': FAIL Orca says 'checked' (not 'not checked') / Orca said nothing in the act`
  - `pass1 collections-tree-nav 'Down: Teksilo (a leaf, level 3)': +146.6 ms ORCA SAYS: 'Teksilo.'; FAIL Orca says 'not checked'`
  - `pass1 collections-tree-nav 'Left: up to Projects': +153.2 ms ORCA SAYS: 'Projects.'; FAIL Orca says 'partially checked' while pass the [check box] 'Projects' in the tree is indeterminate`
  - `pass2 collections-tree-nav 'Left: up to Projects': +184.1 ms ORCA SAYS: 'Projects.'; 'Space: check the leaf': Orca said nothing in the act`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/standard\_item.rs:693-715; crates/teksilo-widgets/src/list\_item\_a11y.rs:187-215 (ListItemWrapper::accessibility), :285-325 (TreeItemWrapper::accessibility)
- **Evidence (`261a218f`):**
  - `run collections-listview-arrows-20260925-131028-2421766 'Down: second row': '13:10:53.217299 SPEECH OUTPUT: 'Item 2.'' ; 'FAIL  Orca says 'not checked''`
  - `same act: 'FAIL  [list item] 'Item 2' carries state 'checkable'' / '[list item] 'Item 2' desc=None states=['enabled', 'focused', 'selectable', 'selected', 'sensitive', 'showing', 'visible']' / 'grandchild [check box] 'Item 2' desc=None states=['checkable', 'enabled', 'focusable', 'sensitive', 'showing', 'visible']'`
  - `run collections-tree-nav-20260925-131115-2411115 'Left: up to Projects': '13:11:55.493929 SPEECH OUTPUT: 'Projects.'' ; 'FAIL  Orca says 'partially checked'' while 'pass  the [check box] 'Projects' in the tree is indeterminate'`
  - `crates/teksilo-widgets/src/standard_item.rs:693-715 embeds the Checkbox as a child node (cb.access_label(self.label)); list_item_a11y.rs:176-206 ListItemWrapper::accessibility and :270-310 TreeItemWrapper::accessibility publish no toggled`
  - `Orca formatting.py LIST_ITEM 'unfocused': '(labelOrName ...) + checkedStateIfCheckable + ... + listBoxItemWidgets'; speech_generator.py:2333 'if not AXUtilities.is_list_box(AXObject.get_parent(obj))'; scripts/default.py:1221-1226 onCheckedChanged returns unless event.source is the locus of focus`
  - `collections-listview-arrows-20260925-134124-3805659 'Down: second row': '[list item] 'Item 2' ... states=['enabled', 'focused', 'selectable', 'selected', ...]' / 'grandchild [check box] 'Item 2' ... states=['checkable', 'enabled', 'focusable', ...]'; Orca said 'Item 2.'`
  - `/usr/lib/python3/dist-packages/orca/formatting.py:329-331 LIST_ITEM formats; :560-563 TREE_ITEM formats (no checkedState)`
  - `orca scripts/default.py:1221-1245 onCheckedChanged: returns unless event.source is the locus of focus, then presentObject(event.source, alreadyFocused=True)`
- **Reproduced:** 3 of 3 runs (ListView and TreeView)
- **Verification:** corrected by the verifier. Reproduced: ListView 3 of 3 runs (collections-listview-arrows, and the checkbox runs); TreeView 3 of 3 runs (collections-tree-nav) Real, as measured: Orca says only 'Item 2.' / 'Teksilo.' / 'Projects.'. The focused row carries no checkable state, and the state sits on a grandchild check box. The ListView analysis is right. Orca's LIST\_ITEM 'unfocused' format reads checkedStateIfCheckable from the item, and its 'focused' format (used by onCheckedChanged -&gt; presentObject(alreadyFocused=True)) reads it too. So toggled on the row would be spoken on arrival and on change. Correction for the TreeView part of the fix: Orca 46.1's TREE\_ITEM formats have no checked state at all (formatting.py:560-563: 'focused': 'expandableState'; 'unfocused': labelOrName + expandableState + positionInList). Publishing toggled on the tree item would therefore still not be spoken on Orca, on arrival or on toggle. It would reach UIA as the Toggle pattern (accesskit\_windows node.rs:1334), so it fixes Windows; Linux/Orca needs an announcement or an upstream change. So: framework for ListView, framework plus upstream Orca for TreeView.
- **Fix idea:** When a StandardListItem/StandardTreeItem carries a checkbox, publish its toggled (true/false/mixed) on the row wrapper (ListBoxOption / TreeItem with toggled -&gt; AT-SPI checkable/checked/indeterminate on the item, UIA Toggle on the item) and hide the inner Checkbox from AT, or merge it into the row.

### collections-04 {#collections-04}

The move / reparent announcement is sent in the same update as a focus change to a freshly built row, so Orca cuts it even when it survives (independent of K2)

- **Example:** data-collections
- **Scenario:** collections-tree-moves, collections-listview-reorder, collections-listview-menu
- **Act:** TreeView: cursor on 'Documents', Alt+Down (first message of the session), then Alt+Right, Alt+Left. ListView: row 1, Alt+Down x2, Alt+Home; and the row menu's 'Move Down'.
- **The reader should get:** The reader hears 'Moved to 2 of 3' ('Moved to 2 of 200', 'Moved to level 2' and so on) whole.
- **The reader got (`261a218f`):** The announcement reaches the bus 0.5 to 60 ms before the focus event of the same update (12 of 12 runs). The move re-renders the rows, so the moved row is a new node and the active descendant changes, and the consumer hands the announcement to the adapter before the focus event. When the message survives K2 (the first message of a tree session, 2 of 6 runs), Orca speaks it and then stops it for the new focus 141 ms in, and the reader hears 'Documents.' instead. In the other 4 tree runs, and in 3 of 3 ListView Alt+Down runs and 3 of 3 menu Move Down runs, the message was already lost to K2 (retracted/defunct announcer node). The K2 fix (b9586ea2b) keeps the node alive but does not change this ordering, so the cut remains after it. All of these messages go through ctx.announce (list\_view/widget\_impl.rs:262, tree\_view/widget\_impl.rs:431 and 454).
- **Platform:** Linux AT-SPI/Orca measured. Windows: the adapter raises LiveRegionChanged, then the focus change, in the same order (consumer tree.rs:640-673); whether NVDA cuts it is unverified
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `b518253f` (announce-focus).
- **Now (`c198e4d1`):** The move and reparent messages now reach the bus a few milliseconds after the focus event of the same move, and Orca reads the row and then the whole message: 'Item 1.' then 'Moved to 2 of 200', and 'Documents.' then 'Moved to 2 of 3', 'Moved to level 2', 'Moved to level 1'. The row menu's Move Down and an AT-SPI click on Move to Bottom do the same. No stop cuts them, and none is lost to a defunct node.
- **Measured again:** collections-tree-moves, collections-listview-reorder, collections-listview-menu, verify-collections-menu-more, fix-announce-focus-listview, fix-announce-focus-tree: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 collections-tree-moves 'Alt+Down: move Documents after Pictures': +42.2 ms object:state-changed:focused 1 [tree item] 'Documents', +48.7 ms object:announcement [status bar] 'Moved to 2 of 3' text='Moved to 2 of 3'; +175.2 ms ORCA SAYS: 'Documents.', +208.6 ms ORCA SAYS: 'Moved to 2 of 3'`
  - `pass2 collections-tree-moves 'Alt+Right: indent Documents into Pictures': +54.1 ms object:announcement [status bar] 'Moved to level 2' text='Moved to level 2'; +200.1 ms ORCA SAYS: 'Moved to level 2'`
  - `pass1 collections-listview-reorder 'Alt+Down: move row 1 down': +96.4 ms object:state-changed:focused 1 [list item] 'Item 1', +141.0 ms object:announcement [status bar] 'Moved to 2 of 200' text='Moved to 2 of 200'; +245.5 ms ORCA SAYS: 'Item 1.', +264.4 ms ORCA SAYS: 'Moved to 2 of 200'`
  - `pass2 collections-listview-reorder 'Alt+Home: move to top': +66.9 ms object:announcement [status bar] 'Moved to 1 of 200' text='Moved to 1 of 200'; +200.6 ms ORCA SAYS: 'Moved to 1 of 200'`
  - `pass1 collections-listview-menu 'Enter on Move Down': +50.5 ms object:state-changed:focused 1 [list item] 'Item 1', +54.2 ms object:announcement [status bar] 'Moved to 2 of 200' text='Moved to 2 of 200'; +238.5 ms ORCA SAYS: 'Moved to 2 of 200'`
  - `pass2 verify-collections-menu-more 'AT-SPI click on 'Move to Bottom'': +1104.4 ms object:announcement [status bar] 'Moved to 200 of 200' text='Moved to 200 of 200'; +3218.3 ms ORCA SAYS: 'Moved to 200 of 200'`
  - `pass1 and pass2 fix-announce-focus-listview and fix-announce-focus-tree: every move message spoken after the row, no cut observed`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/list\_view/widget\_impl.rs:237-264 (select, fi.set and ctx.announce in one handler); crates/teksilo-widgets/src/tree\_view/widget\_impl.rs:380-384, :431, :454; crates/teksilo-core/src/announcer.rs:71-106 and :238-246 (a message waits through an update that moves focus); crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs:346-360 (settle\_announcers)
- **Evidence (`261a218f`):**
  - `run collections-tree-moves-20260925-130508-2120779 'Alt+Down': '+39.2 ms object:announcement [status bar] 'Moved to 2 of 3' text='Moved to 2 of 3'' then '+45.2 ms object:state-changed:focused 1 [tree item] 'Documents''`
  - `Orca: '13:05:25.187533 - EVENT MANAGER: object:announcement for [status bar: 'Moved to 2 of 3'] in [application: 'data-collections'] (1, 0, Moved to 2 of 3)', '13:05:25.226848 - SPEECH OUTPUT: 'Moved to 2 of 3'', '13:05:25.367461 - NULL SPEECH: stop', '13:05:25.367596 - SPEECH OUTPUT: 'Documents.''`
  - `report observation: 'Orca's 'Moved to 2 of 3' was cut by a stop 141 ms in (estimated)' (same again in run collections-tree-moves-20260925-131216-2411115)`
  - `run collections-tree-moves-20260925-132528-2973050 (K2 loss): '13:25:48.800901 EVENT MANAGER: object:announcement for [status bar: 'Moved to 2 of 3'] ... is not obsoleted', '13:25:48.800915 EVENT MANAGER: Ignoring defunct object: [status bar: 'Moved to 2 of 3']'`
  - `run collections-listview-reorder-20260925-130811-2291087: 'object:announcement [<Error>] '' text='Moved to 2 of 200'' 28.6 ms before 'object:state-changed:focused 1 [list item] 'Item 1''; Orca 'object:announcement for [DEAD] ... (1, 0, Moved to 2 of 200)' then 'Ignoring defunct object: [DEAD]'`
  - `crates/teksilo-widgets/src/list_view/widget_impl.rs:226-262 (sel.select(dest); fi.set(Some(dest)); ... ctx.announce(utterance) in one handler); tree_view/widget_impl.rs:380-384 follow() then :431 / :454 ctx.announce`
  - `accesskit_consumer-0.39.0/src/tree.rs:640-673 node_added/node_updated before focus_moved; Orca scripts/default.py:698-705 presentationInterrupt() on the locus-of-focus change`
  - `collections-tree-moves-20260925-133451-3491445 orca-debug.out: '13:35:11.268829 - SPEECH OUTPUT: 'Moved to 2 of 3'', '13:35:11.387411 - FOCUS MANAGER: Changing locus of focus from [tree item: 'Documents'] to [tree item: 'Documents']', '13:35:11.489990 - NULL SPEECH: stop', 'SPEECH OUTPUT: 'Documents.''`
  - `collections-listview-reorder-20260925-133754-3636242 orca-debug.out: '13:38:18.475472 - SPEECH OUTPUT: 'Moved to 3 of 200'', '13:38:18.499699 - FOCUS MANAGER: Changing locus of focus from [list item: 'Item 1'] to [list item: 'Item 1']', '13:38:18.525580 - NULL SPEECH: stop', '13:38:18.525631 - SPEECH OUTPUT: 'Item 1.''`
  - `verify-collections-menu-more-20260925-133812-3645182 'AT-SPI click on 'Move to Bottom'': '+104.2 ms object:announcement [<Error>] '' text='Moved to 200 of 200'', '+104.5 ms object:state-changed:focused 1 [list item] 'Item 1'', '+104.5 ms object:state-changed:focused 0 [menu] ''', ORCA SAYS 'multi-select list box' 'Item 1.'`
  - `collections-tree-moves-20260925-133233-3324798 (K2 loss): '13:32:53.471582 EVENT MANAGER: Ignoring defunct object: [DEAD]'`
- **Reproduced:** announcement ahead of the focus event in 12 of 12 runs; spoken then cut in 2 of 2 runs where the node survived (6 tree runs total, the other 4 lost to K2)
- **Verification:** confirmed. Reproduced: The announcement reached the bus ahead of the act's focus change for 23 of 23 announcements over 11 runs (tree-moves 9, listview-reorder 9, menu Enter 3, menu AT-SPI click 2). Orca spoke it in 3 of 23 (the other 20 were lost to K2), and all 3 were stopped by the locus-of-focus change to the rebuilt row: 3 of 3 cut.
- **Fix idea:** Have the announcer hold a message raised in an update that also moves focus until the next update (after the focus event has been delivered). Alternatively, keep the moved row's node identity across the reorder, so that no focus change competes. Add an announcer test that replays the focus event after the live node in the same update.

### collections-05 {#collections-05}

Row context menu (Shift+F10): focus stays on an unnamed menu container and arrowing between commands emits nothing

- **Example:** data-collections
- **Scenario:** collections-listview-menu
- **Act:** ListView row 1, Shift+F10, Down, Enter
- **The reader should get:** Focus (or the active descendant) lands on the first command and the reader hears 'Move Down, menu item'; each arrow press reads the next command.
- **The reader got (`261a218f`):** Shift+F10 focuses '\[menu\] ''' and Orca says only 'menu.'. Down produces no event at all: no focus, no active-descendant, no selected state on an item. Enter then runs the highlighted 'Move Down', which the reader never heard. Seen in 3 of 3 runs. The cause is MenuList, so this may overlap the menus sweep.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** low; **layer:** framework
- **Severity in the sweep:** high. The arrows now read every command, so the reader always hears the command Enter will run. What is left is that the menu opens on its container and needs one Down before the first command is heard.
- **Status:** Partly fixed by `9636094c` (menus). What remains is under **Now**.
- **Now (`c198e4d1`):** Each arrow in the row's context menu now moves focus to the command, and Orca reads 'Move Down.', 'Move to Bottom.', 'Move Down.', so Enter runs a command the reader has heard. The menu still opens on its container rather than on its first command: Shift+F10 gives only 'menu.', and the first command is heard only after the first Down.
- **Measured again:** collections-listview-menu, 2 of 2 runs; fix-menus-context, 2 of 2 runs; verify-collections-menu-more, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-menu 'Shift+F10: the row's menu': +45.4 ms object:state-changed:focused 1 [menu] ''; +120.7 ms ORCA SAYS: 'menu.'; FAIL focus lands on [menu item] '*'`
  - `pass1 collections-listview-menu 'Down: to the first command': +18.7 ms object:state-changed:focused 1 [menu item] 'Move Down'; +55.8 ms ORCA SAYS: 'Move Down.'`
  - `pass2 collections-listview-menu 'Shift+F10: the row's menu': +98.8 ms ORCA SAYS: 'menu.'; 'Down: to the first command': +78.3 ms ORCA SAYS: 'Move Down.'`
  - `pass1 fix-menus-context 'Down: Move to Bottom': +17.5 ms object:state-changed:focused 1 [menu item] 'Move to Bottom'; +58.9 ms ORCA SAYS: 'Move to Bottom.'`
  - `pass2 fix-menus-context 'Up: Move Down': +21.5 ms object:state-changed:focused 1 [menu item] 'Move Down'; +94.0 ms ORCA SAYS: 'Move Down.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_list.rs:1121-1156 (Role::Menu, active\_descendant on the highlighted row), :595-602 (highlight bound at AccessibilityOnly); the highlight starts empty, so the menu opens with no active descendant
- **Evidence (`261a218f`):**
  - `run collections-listview-menu-20260925-131509-2699481 '== Shift+F10': '+39.2 ms object:state-changed:focused 1 [menu] ''' ; '13:16:16.315680 SPEECH OUTPUT: 'menu.''`
  - `same run '== Down: to the first command': 'FAIL  focus lands on [menu item] 'Move Down'' / 'no focus change on the bus in this act' ; 'FAIL  Orca says 'Move Down''`
  - `tree after Down: '[menu] '' {focusable,focused}' > '[menu item] 'Move Down'' '[menu item] 'Move to Bottom'' (no item has focused or selected)`
  - `same run '== Enter on Move Down': '+43.4 ms object:announcement [<Error>] '' text='Moved to 2 of 200'' (the command did run)`
  - `crates/teksilo-widgets/src/menu_list.rs:991-993 accessibility() sets Role::Menu only (no active_descendant); the keyboard highlight is a paint-only focused_index Signal (menu_list.rs:140-161, key arms at :745-800); KeyboardHighlightWrapper is a GenericContainer (:218-224)`
  - `verify-collections-menu-more-20260925-133812-3645182 '== Down, Down, Up in the menu' -> 'record: event counts' / '0 events: {}' and 'no object: event from [menu item] '*''; orca-debug.out '13:38:32.633274 - SPEECH OUTPUT: 'menu.''`
  - `same run '== Escape closes the menu': '+11.1 ms object:state-changed:focused 1 [list item] 'Item 1'', ORCA SAYS 'multi-select list box', 'Item 1.'`
  - `tree after Shift+F10: '[menu] '' {focusable,focused}' > '[menu item] 'Move Down'', '[menu item] 'Move to Bottom''`
  - `git log -- crates/teksilo-widgets/src/menu_list.rs: no active_descendant in any menu module (grep over menu*.rs and menu/ returns nothing)`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs of collections-listview-menu; 2 of 2 runs of verify-collections-menu-more (Down, Down, Up in the menu: zero events of any kind)
- **Fix idea:** Publish the highlighted item as the menu's active\_descendant (bound at AccessibilityOnly to focused\_index) or move real focus to the item, and name the menu. Open a keyboard-invoked context menu with its first item highlighted, as GTK and Qt do.

### collections-06 {#collections-06}

TreeView on Linux: expanded/collapsed state, level and the Expand/Collapse actions never reach AT-SPI

- **Example:** data-collections
- **Scenario:** collections-tree-nav
- **Act:** TreeView: Down to 'Documents', Right (expand), Right (into 'Projects'), Right (expand), Left (collapse)
- **The reader should get:** The reader hears 'Documents, collapsed', then 'expanded' / 'collapsed' as branches open and close, and can learn the level (Where Am I). A screen reader's expand/collapse command works.
- **The reader got (`261a218f`):** Orca says only the name each time ('Documents.', and 'Documents.' again after expanding). The tree items carry neither expandable nor expanded, attributes {'posinset': '1'} only (no level), and actions \['click'\] only. Teksilo publishes all of this correctly (TreeItemWrapper sets expanded, level and Expand/Collapse). accesskit\_atspi\_common 0.20 maps no Expanded/Expandable state and no level attribute or NODE\_CHILD\_OF relation, and exports only the 'click' action, so the row's Expand/Collapse and its move custom actions are unreachable on Linux. Windows exports ExpandCollapse and Level (read in its source). 3 of 3 runs.
- **Platform:** Linux AT-SPI/Orca (measured); Windows/UIA exports both (source); macOS not checked
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** Orca still says only the name on arrival, on expand and on collapse ('Documents.', 'Projects.' each time). On AT-SPI the tree items still carry no expandable or expanded state, no level attribute, and only the 'click' action, so a screen reader's own expand and collapse commands cannot reach them. Teksilo still publishes all of these; accesskit\_atspi\_common 0.21.0 maps none of them.
- **Measured again:** collections-tree-nav, 2 of 2 runs; verify-collections-tree-levels, 2 of 2 runs; adapter read from source
- **Evidence (`c198e4d1`):**
  - `pass1 collections-tree-nav 'Right: expand Documents': +140.7 ms ORCA SAYS: 'Documents.'; FAIL [tree item] 'Documents' carries state 'expanded' / states=['enabled', 'focused', 'selectable', 'selected', 'sensitive', 'showing', 'visible'] attrs={'posinset': '1'} actions=['click']`
  - `pass1 collections-tree-nav 'Left: collapse Projects': +161.5 ms ORCA SAYS: 'Projects.'; FAIL Orca says 'collapsed'`
  - `pass1 collections-tree-nav 'tree after the collapse': FAIL Documents offers an expand/collapse action on AT-SPI / actions=['click']`
  - `pass2 collections-tree-nav 'Down: Documents': FAIL [tree item] 'Documents' carries state 'expandable'; FAIL [tree item] 'Documents' carries an attribute 'level'`
  - `pass2 verify-collections-tree-levels 'Right: expand Documents': +958.8 ms ORCA SAYS: 'Documents.'; FAIL Orca says 'expanded'`
  - `accesskit_atspi_common-0.21.0/src/node.rs:301-389 state() has no Expandable/Expanded; :417-439 attributes() has no level; :525-538 n_actions()/get_action_name() only 'click'`
- **Where (`c198e4d1`):** upstream: accesskit\_atspi\_common-0.21.0/src/node.rs:301-389 (no Expandable/Expanded), :417-439 (no level attribute), :525-538 (only 'click'), :972-989 (relation\_set exports only ControllerFor); Teksilo side: crates/teksilo-widgets/src/list\_item\_a11y.rs:285-325, crates/teksilo-widgets/src/tree\_view/body\_pane.rs:272-280
- **Evidence (`261a218f`):**
  - `run collections-tree-nav-20260925-131115-2411115 'Right: expand Documents': '13:11:35.446425 SPEECH OUTPUT: 'Documents.'' ; 'FAIL  Orca says 'expanded''`
  - `'FAIL  [tree item] 'Documents' carries state 'expandable'' / '[tree item] 'Documents' desc=None states=['enabled', 'focused', 'selectable', 'selected', 'sensitive', 'showing', 'visible'] attrs={'posinset': '1'} actions=['click']'`
  - `'FAIL  Documents offers an expand/collapse action on AT-SPI' / 'actions=['click']'`
  - `Teksilo side correct: crates/teksilo-widgets/src/list_item_a11y.rs:270-310 (set_level, set_expanded, Expand/Collapse actions), tree_view/body_pane.rs:272-280`
  - `accesskit_atspi_common-0.20.0/src/node.rs:300-384 state mapping has no Expandable/Expanded (no 'expand' anywhere in the crate); :415-437 attributes() has no level; :532-545 n_actions()/get_action_name() only 'click'`
  - `Orca: formatting.py TREE_ITEM 'unfocused': '(labelOrName or displayedText) + pause + expandableState + pause + positionInList'; script_utilities.py:1362-1392 nodeLevel from RelationType.NODE_CHILD_OF`
  - `accesskit_windows-0.35.0/src/node.rs:695-723 level()/expand_collapse_state(), :1325 UIA_LevelPropertyId, :1485 UIA_ExpandCollapsePatternId`
  - `verify-collections-tree-levels-20260925-133902-3645182 orca-debug.out:2571 '13:39:26.885943 - SPEECH OUTPUT: 'Projects.'' (Down into level 2, no 'tree level 2'); :2970 '13:39:30.967838 - SPEECH OUTPUT: 'Documents.'' (Up to level 1)`
  - `same run 'Right: expand Documents': '+30.2 ms object:state-changed:focused 1 [tree item] 'Documents'', '+30.2 ms object:state-changed:focused 0 [tree item] 'Documents'', orca-debug.out:2172 '13:39:22.853237 - SPEECH OUTPUT: 'Documents.''`
  - `collections-tree-nav-20260925-133407-3310470: '[tree item] 'Documents' desc=None states=['enabled', 'focused', 'selectable', 'selected', ...] attrs={'posinset': '1'} actions=['click']'`
  - `accesskit_windows-0.35.0/src/node.rs:695-723, :1325, :1485`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 runs of collections-tree-nav; 2 of 2 runs of verify-collections-tree-levels
- **Fix idea:** Upstream AccessKit: map expanded to State::Expandable/Expanded plus the state-changed event, level to a 'level' object attribute (or NODE\_CHILD\_OF relations), and expose Expand/Collapse as AT-SPI actions. Until then Teksilo could announce 'expanded'/'collapsed' on a keyboard toggle on Linux only, since Windows already gets it.

### collections-07 {#collections-07}

A row's subtitle is never heard: the name is hoisted onto the row, its description is left on an 'unknown' child

- **Example:** data-collections
- **Scenario:** collections-listview-arrows, collections-tree-nav
- **Act:** ListView Down to 'Item 1' (subtitle 'Item #1 · category'); TreeView Down to 'Documents' (subtitle 'folder · depth 0')
- **The reader should get:** The reader hears 'Item 1, Item #1 · category' (name plus description), as the StandardListItem intends by putting the subtitle in description.
- **The reader got (`261a218f`):** Orca says 'Item 1.' / 'Documents.'. The focused row has desc=None. The subtitle is on a nested '\[unknown\] 'Item 1' desc='Item #1 · category'' node that focus never reaches. 3 of 3 runs for each view.
- **Platform:** Linux AT-SPI/Orca measured (the same tree shape goes to every adapter)
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Orca still says 'Item 1.' and 'Documents.'. The focused row has no description. The subtitle still sits on a nested unknown node that focus never reaches.
- **Measured again:** collections-listview-arrows, 2 of 2 runs; collections-tree-nav, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-arrows 'Down: first row': +210.2 ms ORCA SAYS: 'Item 1.'; FAIL Orca says 'Item #1'; [list item] 'Item 1' desc=None ...; child [unknown] 'Item 1' desc='Item #1 · category'`
  - `pass2 collections-listview-arrows 'Down: first row': +160.8 ms ORCA SAYS: 'Item 1.'; FAIL Orca says 'Item #1'`
  - `pass1 collections-tree-nav 'Down: Documents': +174.9 ms ORCA SAYS: 'Documents.'; FAIL Orca says 'folder'; child [unknown] 'Documents' desc='folder · depth 0'`
  - `pass2 collections-tree-nav 'Down: Documents': +132.9 ms ORCA SAYS: 'Documents.'; FAIL Orca says 'folder'`
- **Where (`c198e4d1`):** crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:171-229 (name-from-content hoists the label only); crates/teksilo-widgets/src/standard\_item.rs:916-935
- **Evidence (`261a218f`):**
  - `run collections-listview-arrows-20260925-131028-2421766 'Down: first row': '13:10:47.450180 SPEECH OUTPUT: 'Item 1.'' ; 'FAIL  Orca says 'Item #1''`
  - `'[list item] 'Item 1' desc=None states=[... 'selected' ...] attrs={'setsize': '200', 'posinset': '1'}' / 'child [unknown] 'Item 1' desc='Item #1 · category''`
  - `run collections-tree-nav-20260925-131115-2411115: 'FAIL  Orca says 'folder'' / 'child [unknown] 'Documents' desc='folder · depth 0''`
  - `crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:141-195 name-from-content copies only the first descendant label (set_label), never its description`
  - `crates/teksilo-widgets/src/standard_item.rs:927-930 builder.set_name(label); builder.set_description(subtitle) on the inner node`
  - `collections-listview-arrows-20260925-134124-3805659 'Down: first row': '[list item] 'Item 1' desc=None ...' / 'child [unknown] 'Item 1' desc='Item #1 · category''; Orca said 'Item 1.'`
  - `collections-tree-nav-20260925-133407-3310470: 'child [unknown] 'Documents' desc='folder · depth 0''; Orca said 'Documents.'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs of collections-listview-arrows; 3 of 3 runs of collections-tree-nav
- **Fix idea:** In the name-from-content pass, also hoist the donor's description when the row has none. Or have the row wrappers take name and description from the StandardListItem directly and hide the inner node.

### collections-08 {#collections-08}

The Role::Group between ListBox and its options hides 'not selected', the row's embedded widgets and the list size from Orca

- **Example:** data-collections
- **Scenario:** collections-listview-cursor, collections-listview-arrows
- **Act:** Tab into the ListView; Ctrl+End (cursor on 'Item 200', not selected); Down onto a checkbox row
- **The reader should get:** The reader hears 'Item 200, not selected' in the multi-select list, the row's embedded check box state, and a size on entering the list.
- **The reader got (`261a218f`):** Orca says 'Item 200.' with no 'not selected' (the row really is unselected on the bus), 'Item 2.' with no check box, and 'multi-select list box.' with no count. Orca's 'not selected' needs the item's parent to implement Selection, its embedded-widget pass needs the parent to be a list box, and its list-size count looks for list items among the list box's direct children. The parent of every row is the pane's '\[panel\]' (interfaces \['Accessible', 'Component'\]). 3 of 3 runs. The count would be of realized rows even without the Group (see collections-09).
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Orca still says 'Item 200.' with no 'not selected' on an unselected row in the multi-select list, 'Item 2.' with nothing of the row's check box, and 'multi-select list box.' with no count. The parent of every row is still the pane's unnamed panel, not the list box.
- **Measured again:** collections-listview-cursor, 2 of 2 runs; collections-listview-arrows, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-cursor 'Ctrl+End: cursor only, to the last row': +242.0 ms ORCA SAYS: 'Item 200.'; FAIL Orca says 'not selected'; pass [list item] 'Item 200' carries no state 'selected'`
  - `pass2 collections-listview-cursor 'Ctrl+End': +213.8 ms ORCA SAYS: 'Item 200.'; FAIL Orca says 'not selected'`
  - `pass1 collections-listview-arrows 'Down: first row': FAIL [list item] 'Item 1''s parent is a [list box] / [list item] 'Item 1''s parent is [panel] '' (its parent [list box] '')`
  - `pass2 collections-listview-arrows 'Tab into the list': +79.7 ms ORCA SAYS: 'multi-select list box.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/list\_view/body\_pane.rs:702-714 (Role::Group on the pane, at :713); crates/teksilo-widgets/src/tree\_view/body\_pane.rs:801-807
- **Evidence (`261a218f`):**
  - `run collections-listview-cursor-20260925-131003-2404500 'Ctrl+End': '13:10:47.626116 SPEECH OUTPUT: 'Item 200.'' ; 'FAIL  Orca says 'not selected'' ; 'pass  [list item] 'Item 200' carries no state 'selected''`
  - `run collections-listview-arrows-20260925-131028-2421766: 'FAIL  [list item] 'Item 1''s parent is a [list box]' / '[list item] 'Item 1''s parent is [panel] '' (its parent [list box] '')'`
  - `listener record of that panel: 'role': 'panel', 'interfaces': ['Accessible', 'Component'], 'child_count': 9; list box: 'interfaces': ['Accessible', 'Component', 'Selection']`
  - `'13:10:42.439593 SPEECH OUTPUT: 'multi-select list box.''`
  - `crates/teksilo-widgets/src/list_view/body_pane.rs:700-708 (and tree_view/body_pane.rs:802-807) set Role::Group on the pane`
  - `Orca speech_generator.py:1051 'if not AXObject.supports_selection(AXObject.get_parent(obj))', :2333 'if not AXUtilities.is_list_box(AXObject.get_parent(obj))', :1588-1591 counts AXUtilities.is_list_item children of the list box`
  - `collections-listview-arrows-20260925-134124-3805659: '[list item] 'Item 1''s parent is [panel] '' (its parent [list box] '')'; Orca 'multi-select list box.'`
  - `collections-listview-cursor-20260925-133209-3310470 'Ctrl+End': Orca 'Item 200.'; 'pass  [list item] 'Item 200' carries no state 'selected''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs (collections-listview-arrows; collections-listview-cursor for Ctrl+End)
- **Fix idea:** Give the pane Role::GenericContainer so the consumer filter drops it and the options become the list box's children (the pruning pass keeps nodes it cannot drop). If a group is really wanted, it needs Selection semantics of its own.

### collections-09 {#collections-09}

Virtualization: Orca's position counts only the realized rows ('1 of 17', 'Item 200' is '9 of 9') although the rows publish posinset/setsize of 200

- **Example:** data-collections
- **Scenario:** collections-listview-arrows, collections-feed, collections-tree-nav
- **Act:** Down to 'Item 1'; End to 'Item 200'; Up to 'Item 199'; feed End to '#300'; tree 'Teksilo'
- **The reader should get:** Position speaking and Where Am I say 'Item 1, 1 of 200', 'Item 200, 200 of 200', '#300, 300 of 300', 'Teksilo, 1 of 2'.
- **The reader got (`261a218f`):** Computed from the exported tree the way Orca 46.1 does it, the answers are 1 of 17, 9 of 9 (row 200), 8 of 9 (row 199), 10 of 10 (feed #300) and 3 of 7 (tree leaf, counted among all flattened rows). Teksilo's own attributes are right: posinset 1..200 / setsize 200, posinset 300 / setsize 300, tree posinset 1. Orca's default script ignores them and counts siblings. A row out of view has no node at all. On Windows UIA reads PositionInSet/SizeOfSet from these properties (source), so 'n of 200' is expected there. 3 of 3 runs.
- **Platform:** Linux Orca (computed from the tree; Orca's own Where Am I key cannot be pressed by the harness); Windows by source
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** The rows still publish the right posinset and setsize (1 to 200 of 200, 300 of 300), but Orca 46.1 counts the realized siblings under the pane, so its position would be 1 of 17, 9 of 9 for row 200, 8 of 9 for row 199, 10 of 10 for feed message 300, and 3 of 7 for the tree leaf.
- **Measured again:** collections-listview-arrows, collections-feed, collections-tree-nav: 2 of 2 runs each (computed from the exported tree as Orca does)
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-arrows 'Down: first row': Orca would count 1 of 17 (parent [panel] '' holds 17 children); the row publishes {'setsize': '200', 'posinset': '1'}`
  - `pass1 collections-listview-arrows 'End: last row': Orca would count 9 of 9 (parent [panel] '' holds 9 children); the row publishes {'posinset': '200', 'setsize': '200'}`
  - `pass2 collections-listview-arrows 'Up from the last row': Orca would count 8 of 9 (parent [panel] '' holds 9 children); the row publishes {'posinset': '199', 'setsize': '200'}`
  - `pass1 collections-feed 'End: last message': Orca would count 10 of 10 (parent [panel] '' holds 10 children); the row publishes {'posinset': '300', 'setsize': '300'}`
  - `pass2 collections-tree-nav 'Down: Teksilo (a leaf, level 3)': Orca would count 3 of 7 (parent [panel] '' holds 7 children); the row publishes {'posinset': '1'}`
- **Where (`c198e4d1`):** upstream: Orca 46.1 script\_utilities.py:3249-3296 (unchanged); for the tree also crates/teksilo-widgets/src/list\_item\_a11y.rs:301-312 (no size\_of\_set, flat rows)
- **Evidence (`261a218f`):**
  - `run collections-listview-arrows-20260925-131028-2421766: 'Orca would count 1 of 17 (parent [panel] '' holds 17 children); the row publishes {'setsize': '200', 'posinset': '1'}'`
  - `'End: last row': 'Orca would count 9 of 9 (parent [panel] '' holds 9 children); the row publishes {'posinset': '200', 'setsize': '200'}'`
  - `run collections-feed-20260925-131115-2404500: 'Orca would count 10 of 10 (parent [panel] '' holds 10 children); the row publishes {'posinset': '300', 'setsize': '300'}'`
  - `run collections-tree-nav-20260925-131115-2411115: 'Orca would count 3 of 7 (parent [panel] '' holds 7 children); the row publishes {'posinset': '1'}'`
  - `Orca script_utilities.py:3249-3296 getPositionAndSetSize counts functional siblings; speech_generator.py:2203-2241 positionInList only with enablePositionSpeaking or forceList`
  - `accesskit_windows-0.35.0/src/node.rs:1326-1327 UIA_PositionInSetPropertyId / UIA_SizeOfSetPropertyId`
  - `collections-listview-arrows-20260925-134124-3805659: 'Orca would count 1 of 17 (parent [panel] '' holds 17 children); the row publishes {'setsize': '200', 'posinset': '1'}', 'End: ... 9 of 9 ... {'setsize': '200', 'posinset': '200'}'`
  - `collections-feed-20260925-134218-3805659 'End: last message': FAIL 'Orca's position for [list item] '#300' would be 300 of 300'`
  - `collections-listview-reorder-20260925-133318-3324798: 17 [list item] nodes before Alt+Down, 9 in tree-Alt-Down--move-row-1-down.txt ('Item 2' posinset 1 ... 'Item 9' posinset 9)`
  - `grep -rn posinset over /usr/lib/python3/dist-packages/orca outside scripts/web: only scripts/apps/Thunderbird/spellcheck.py and Chromium`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs for the list (collections-listview-arrows), the feed (collections-feed) and the tree (collections-tree-nav); deterministic The list part is right. Orca's default script never reads posinset/setsize: grep finds them only in the web, Chromium and Thunderbird scripts. It counts siblings in the exported tree, so the computed answers are 1 of 17, 9 of 9 (row 200), 8 of 9 and 10 of 10 (feed #300). Two corrections. (1) For the TreeView, the '3 of 7' also comes from Teksilo's flat tree: there is no per-branch group and no NODE\_CHILD\_OF, so Orca's functional parent is the whole pane. TreeView publishes no size\_of\_set anywhere (by its own comment), so on Windows UIA SizeOfSet is empty for tree items (by source, accesskit\_windows node.rs:689-693). Teksilo's comment says a per-branch Role::Group would fix both. The tree part is therefore framework plus upstream. (2) The realized count depends on build history: 17 rows before a pane rebuild, only the 9 visible rows after one (Alt+Down, Ctrl+End: 3 of 3). So even the wrong number Orca gives changes from one moment to the next. The list's Windows claim is right by source.
- **Fix idea:** Nothing Teksilo can change in Orca's default script. Report upstream to Orca (use posinset/setsize when present, as its web script does). Flattening the Group (collections-08) at least makes the count 'n of realized rows' consistent for the list.

### collections-10 {#collections-10}

Row node churn: every TreeView key press re-creates every realized row, and a ListView reorder re-creates the whole pane

- **Example:** data-collections
- **Scenario:** collections-tree-nav, collections-listview-reorder
- **Act:** TreeView Down/Right/Left; ListView Alt+Down
- **The reader should get:** An arrow press replaces at most the rows whose content changed, as the ListView already does for selection (two rows).
- **The reader got (`261a218f`):** With only 7 realized tree rows, each tree arrow press sends 18-38 events (all rows removed and re-added, 9-21 defunct). A ListView Alt+Down sends 108 events, 78 of them defunct. The focused row after an expand/collapse is a new node, so Orca re-reads just 'Documents.', and it discards the old row's focus-lost event as defunct on every tree act. The event count grows with the number of realized rows and passes Orca's deluge threshold (&gt;100 queued, where it starts dropping name/description changes) with about 20-25 visible tree rows. 3 of 3 runs.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Every TreeView arrow still re-creates every realized row: 18 to 38 events per press with 7 visible rows, 9 to 21 of them defunct, and Orca drops the old row's focus loss as defunct on every act. A ListView Alt+Down now sends 114 events, 83 of them defunct.
- **Measured again:** collections-tree-nav, 2 of 2 runs; collections-listview-reorder, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-tree-nav per-act counts: 'Down: Documents' 18 events, 9 defunct; 'Right: expand Documents' 20, 9 defunct; 'Down: Teksilo' 38, 21 defunct, 7 add, 7 remove; 'Left: collapse Projects' 36, 21 defunct`
  - `pass2 collections-tree-nav per-act counts: identical (18/9, 20/9, 38/21, 36/21)`
  - `pass1 collections-tree-nav orca-debug.out: '16:04:08.114983 - EVENT MANAGER: Ignoring defunct object: [tree item: 'Documents']' (6 such lines in the run)`
  - `pass1 collections-listview-reorder 'Alt+Down: move row 1 down': 114 events, 83 defunct, 10 add, 17 remove; 'Alt+Down again': 66 events, 42 defunct`
  - `pass2 collections-listview-reorder 'Alt+Down: move row 1 down': 114 events, 83 defunct`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tree\_view/body\_pane.rs:196-207 (observe\_for\_rebuild on selection rebuilds the pane)
- **Evidence (`261a218f`):**
  - `event counts (run collections-tree-nav-20260925-130410-2120779): 'Down: Documents 18 {'object:state-changed:defunct': 9, ...}', 'Down: Teksilo (a leaf, level 3) 38 {'object:state-changed:defunct': 21, 'object:children-changed:add': 7, 'object:children-changed:remove': 7, ...}'`
  - `run collections-listview-reorder-20260925-130454-2120892 'Alt+Down': 'FAIL  row churn: at most 20 object:state-changed:defunct events' / '78 object:state-changed:defunct events in the act' (108 events in total)`
  - `run collections-tree-nav-20260925-131115-2411115 'Right: expand Documents': events [('focused', 1, 'tree item', 'Documents'), ('focused', 0, 'tree item', 'Documents')] ; observation 'EVENT MANAGER: Ignoring defunct object: [tree item: 'Documents']'`
  - `crates/teksilo-widgets/src/tree_view/body_pane.rs:195-206 'Selection changes refresh the selected argument ... so they rebuild the pane'; compare list_view/body_pane.rs:217-223 and list_item_a11y.rs:55-75 (the per-row rebuild boundary ListView adopted for exactly this reason)`
  - `collections-tree-nav-20260925-133407-3310470 per-act counts: 'Down: Documents 18 defunct=9', 'Right: expand Documents 20 defunct=9', 'Down: Teksilo 38 defunct=21 add=7 rm=7', 'Left: collapse Projects 36 defunct=21'`
  - `collections-listview-reorder-20260925-133318-3324798: 'Alt+Down: move row 1 down 108 defunct=78 add=9 rm=17', 'Alt+Down again 64 defunct=42'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs (identical counts in every run)
- **Fix idea:** Give TreeItemWrapper the per-row selection watch ListItemWrapper has, and key rows by the source's stable anchor, so that an expand, collapse or reorder keeps the surviving rows' nodes.

### collections-11 {#collections-11}

Move / reparent announcements and the row move-menu labels are English literals, and they never say what moved unless the app sets a type-ahead label

- **Example:** data-collections
- **Scenario:** collections-listview-reorder, collections-tree-moves, collections-listview-menu
- **Act:** Alt+Down / Alt+Right / Alt+Left; Shift+F10 on a row
- **The reader should get:** Localized sentences such as 'Item 1 moved to 2 of 200' in the user's language, and a localized 'Move Down' command.
- **The reader got (`261a218f`):** The bus carries 'Moved to 2 of 200', 'Moved to 2 of 3', 'Moved to level 2', and the menu reads 'Move Down' / 'Move to Bottom'. All are built with lit!(format!(...)), which teksilo-i18n documents as intentionally untranslated, although teksilo-widgets ships framework locales (fr-FR.ftl and others, none with these strings). The moved item's name is included only when the view has a type\_ahead\_label resolver, and this example sets none, so a reader who presses Alt+Down twice cannot tell which row moved.
- **Platform:** all platforms (by source); bus text measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The bus still carries 'Moved to 2 of 200', 'Moved to 2 of 3', 'Moved to level 2', and the menu still reads 'Move Down' and 'Move to Bottom'. All are English literals, and none names the row that moved, since this example sets no type-ahead label.
- **Measured again:** collections-listview-reorder, collections-tree-moves, fix-menus-context: 2 of 2 runs each; source read
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-reorder 'Alt+Down: move row 1 down': +141.0 ms object:announcement [status bar] 'Moved to 2 of 200' text='Moved to 2 of 200'`
  - `pass2 collections-tree-moves 'Alt+Right: indent Documents into Pictures': +54.1 ms object:announcement [status bar] 'Moved to level 2' text='Moved to level 2'`
  - `pass1 fix-menus-context 'Down: Move Down': object:state-changed:focused 1 [menu item] 'Move Down'; 'Down: Move to Bottom': [menu item] 'Move to Bottom'`
  - `crates/teksilo-widgets/src/common/ordered_move.rs is unchanged since 261a218f; the locale files gained only a CodeEditor string`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/common/ordered\_move.rs:102-103, 260-267, 316-323 (lit! at 320/322), 574-582 (lit! at 578/580); crates/teksilo-i18n/src/lib.rs:141-149 (lit! is intentionally untranslated)
- **Evidence (`261a218f`):**
  - `run collections-listview-reorder-20260925-130811-2291087: 'object:announcement [<Error>] '' text='Moved to 2 of 200''`
  - `run collections-tree-moves-20260925-130508-2120779: 'object:announcement [status bar] 'Moved to 2 of 3' text='Moved to level 2''`
  - `menu tree: '[menu item] 'Move Down'', '[menu item] 'Move to Bottom''`
  - `crates/teksilo-widgets/src/common/ordered_move.rs:316-323 lit!(format!("{name} moved to {position} of {count}")) / lit!(format!("Moved to {position} of {count}")); :575-582 'moved to level'; :102-103 and :260-267 menu/custom-action labels via lit!`
  - `crates/teksilo-i18n/src/lib.rs:139-147 'Wrap an intentionally untranslated string ... Use tr!(...) for anything user-facing'`
  - `name only from type-ahead: ordered_move.rs:355-359 (RowMover.name), list_view/widget_impl.rs:212-223, tree_view/widget_impl.rs:405-413; example main.rs:258-290 and :421-447 set no type_ahead_label`
  - `verify-collections-menu-more-20260925-133812-3645182: 'object:announcement [<Error>] '' text='Moved to 200 of 200''`
  - `grep -i 'moved\|move down' crates/teksilo-widgets/locales/en-US.ftl fr-FR.ftl: no match`
- **Reproduced:** 3 of 3 runs (text on the bus)
- **Verification:** confirmed. Reproduced: Bus text in 3 of 3 runs of each move scenario; localizability by source
- **Fix idea:** Move the utterances and command labels into the framework .ftl bundle (tr!/resolve\_widget with position/count/level/name arguments). Name the moved row from the row's own accessible name (already hoisted onto the wrapper) instead of the type-ahead resolver.

### collections-12 {#collections-12}

The ListView, the Auto Feed list and the TreeView have no accessible name

- **Example:** data-collections
- **Scenario:** collections-listview-arrows, collections-feed, collections-tree-nav
- **Act:** Tab into each view
- **The reader should get:** 'Virtualized List, multi-select list box' / 'Message Feed, list box' / 'File Tree, tree' (each has a visible heading right above it).
- **The reader got (`261a218f`):** Orca says 'multi-select list box.', 'list box.' and 'tree.'. Every container is '\[list box\] ''' / '\[tree\] '''. 3 of 3 runs.
- **Platform:** Linux AT-SPI/Orca measured (a missing name is the same on every platform)
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Orca still says 'multi-select list box.', 'list box.' and 'tree.' on entering the three views. Each container is still unnamed.
- **Measured again:** collections-listview-arrows, collections-feed, collections-tree-nav: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-arrows 'Tab into the list': +78.0 ms ORCA SAYS: 'multi-select list box.'; FAIL the list box has a name / [list box] '' desc=None`
  - `pass2 collections-listview-arrows 'Tab into the list': +79.7 ms ORCA SAYS: 'multi-select list box.'`
  - `pass1 collections-feed 'Tab into the feed': +57.6 ms ORCA SAYS: 'list box.'`
  - `pass2 collections-tree-nav 'Tab into the tree': +76.3 ms ORCA SAYS: 'tree.'`
- **Where (`c198e4d1`):** examples/data\_collections/src/main.rs:258-290, 336-355, 421-447 (no access\_label / labelled\_by); headings at :217, :309, :376
- **Evidence (`261a218f`):**
  - `run collections-listview-arrows-20260925-131028-2421766: 'FAIL  the list box has a name' / '[list box] '' desc=None states=['enabled', 'focusable', 'focused', 'multiselectable', ...]'; '13:10:42.439593 SPEECH OUTPUT: 'multi-select list box.''`
  - `run collections-feed-20260925-130417-2120746: '+56.1 ms ORCA SAYS: 'list box.''`
  - `run collections-tree-nav-20260925-131115-2411115: 'ORCA SAYS: 'tree.''`
  - `examples/data_collections/src/main.rs:258-290, 336-355, 421-447 build the views with no .access_label / labelled_by; the headings at main.rs:217, 309, 376 are plain TextWidgets`
  - `collections-listview-arrows-20260925-134124-3805659 'Tab into the list': FAIL 'the list box has a name' / '[list box] '' desc=None states=['enabled', 'focusable', 'focused', 'multiselectable', ...]'`
  - `collections-feed-20260925-134218-3805659 'Tab into the feed': Orca 'list box.'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs (list box), 3 of 3 (feed), 3 of 3 (tree)
- **Fix idea:** Give each view .access\_label(...) or access\_labelled\_by(heading\_id). The framework could also warn (debug\_assert or audit) on an unnamed ListBox/Tree, as Checkbox does for a missing label.

### collections-13 {#collections-13}

Auto Feed rows read as '#1', '#2': the message text a row shows is never heard

- **Example:** data-collections
- **Scenario:** collections-feed
- **Act:** Tab into the feed, Down, Down, End
- **The reader should get:** 'Message 1', 'Message 2, detail line 2 of message 2, ...', 'Message 300'.
- **The reader got (`261a218f`):** Orca says '#1.', '#2.', '#300.'. The StandardListItem label is '#N' and the message lines are labels in the trailing slot, below the row where focus never goes. 3 of 3 runs.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Orca still says '#1.', '#2.' and '#300.'. The message text and its detail lines are never heard.
- **Measured again:** collections-feed, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-feed 'Down: first message': +228.3 ms ORCA SAYS: '#1.'; FAIL Orca says 'Message 1'`
  - `pass1 collections-feed 'Down: second message': +258.7 ms ORCA SAYS: '#2.'; FAIL Orca says 'detail line 2 of message 2'`
  - `pass2 collections-feed 'End: last message': +340.9 ms ORCA SAYS: '#300.'; FAIL Orca says 'Message 300'`
- **Where (`c198e4d1`):** examples/data\_collections/src/main.rs:347-348
- **Evidence (`261a218f`):**
  - `run collections-feed-20260925-131115-2404500 'Down: second message': '13:11:37.038072 SPEECH OUTPUT: '#2.'' ; 'FAIL  Orca says 'Message 2'' ; 'FAIL  Orca says 'detail line 2 of message 2''`
  - `tree: '[list item] '#2' {focused,selectable,selected}' > '[unknown] '#2'' > '[label] 'Message 2'', '[label] '· detail line 2 of message 2'', '[label] '· detail line 3 of message 2''`
  - `examples/data_collections/src/main.rs:346-349 StandardListItem::new(lit!(format!("#{}", index + 1))) with the content in .trailing_slot(...)`
  - `collections-feed-20260925-134426-3872112: 'Down: second message' Orca ['#2.']; FAIL 'Orca says 'Message 2'' and 'detail line 2 of message 2'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs of collections-feed
- **Fix idea:** Use the message's first line as the label and the detail lines as subtitle (once collections-07 hoists the description), or .access\_label the row with its text.

### collections-14 {#collections-14}

Repeater tab: the tags are unnamed panels holding separate '1.' and 'Rust' labels, not a list; Add/Remove Tag are silent

- **Example:** data-collections
- **Scenario:** collections-repeater
- **Act:** Read the Repeater tab's tree; Space on '+ Add Tag', then on '- Remove Last'
- **The reader should get:** A named list of 4 (then 5) tags, and a status message ('Tag 5 added') for a reader who stays on the button.
- **The reader got (`261a218f`):** Each tag is '\[panel\] ''' with '\[label\] '1.'' and '\[label\] 'Rust''. There is no list, no count and no name, and Repeater::indexed rebuilds every panel on each change. Neither button produces any speech, so the new tag reaches the bus with nothing announced. The Repeater documentation asks the app to opt in with .access\_role(Role::List).access\_label(...). 3 of 3 runs.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Each tag is still an unnamed panel holding a '1.' label and a 'Rust' label, with no list, count or name. '+ Add Tag' and '- Remove Last' still produce no speech, and Add rebuilds every panel.
- **Measured again:** collections-repeater, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-repeater 'tree of the Repeater tab': FAIL the tags read as a list / [panel] '' desc=None states=['enabled', 'sensitive', 'showing', 'visible'] attrs=None actions=[]`
  - `pass1 collections-repeater tree-launch.txt: '[panel] ''' > '[label] '1.'' '[label] 'Rust''`
  - `pass1 collections-repeater 'Space on + Add Tag': +33.5 ms object:children-changed:add [scroll pane] 'Repeater' -> [panel] '' (5 adds, 4 removes); FAIL Orca says 'Tag 5' / Orca unheard: 'Tag 5'`
  - `pass2 collections-repeater 'Space on - Remove Last': FAIL Orca says 'Tag 5' / Orca unheard: 'Tag 5'`
- **Where (`c198e4d1`):** examples/data\_collections/src/main.rs:175-194; crates/teksilo-widgets/src/repeater.rs:65-79
- **Evidence (`261a218f`):**
  - `run collections-repeater-20260925-130540-2120892: 'FAIL  the tags read as a list' / '[panel] '' desc=None states=['enabled', 'sensitive', 'showing', 'visible'] attrs=None actions=[]'`
  - `launch tree: '[panel] ''' > '[label] '1.'' '[label] 'Rust''`
  - `'== Space on + Add Tag': '+36.8 ms object:children-changed:add [scroll pane] 'Repeater' -> [panel] ''' ... 'pass  the new tag reached the bus' ; 'FAIL  Orca says 'Tag 5'' / 'Orca unheard: 'Tag 5''`
  - `examples/data_collections/src/main.rs:175-194 (no access_role/access_label); crates/teksilo-widgets/src/repeater.rs:65-79 accessibility guidance`
  - `collections-repeater-20260925-134047-3805659 tree-launch.txt lines 17-28: '[panel] ''' > '[label] '1.'' '[label] 'Rust'' ...`
  - `same run 'Space on + Add Tag': '+35.5 ms object:children-changed:add [scroll pane] 'Repeater' -> [panel] ''' x5, removes x4; Orca silent`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 2 of 2 runs of collections-repeater
- **Fix idea:** In the example: .access\_role(Role::List).access\_label(...) on the Repeater, one merged Role::ListItem per tag, and ctx.announce on Add/Remove.

### collections-15 {#collections-15}

Row internals clutter the exported tree: a nameful 'unknown'-role node per row, 'focusable' check boxes outside the Tab order, setsize on every descendant

- **Example:** data-collections
- **Scenario:** collections-listview-arrows, collections-tree-nav
- **Act:** Tree after navigating the ListView / TreeView
- **The reader should get:** A row exposes one node carrying name, description and state. Nothing inside it claims to be focusable when it cannot take focus.
- **The reader got (`261a218f`):** Under every row there is a '\[unknown\] 'Item 1'' node duplicating the name (StandardListItem sets no role). The row check boxes are 'focusable' although a data view keeps them out of the Tab order. Every label inside a row carries attrs {'setsize': '200'}. This is noise for flat review and object navigation, and a reader using structural navigation meets 'unknown' objects.
- **Platform:** Linux AT-SPI measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Under every row there is still an '\[unknown\]' node repeating the row's name, the row check boxes still say 'focusable' although they are outside the Tab order, and every label inside a row still carries setsize 200.
- **Measured again:** collections-listview-arrows, 2 of 2 runs; collections-feed and collections-tree-nav trees, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-arrows 'Down: first row': child [unknown] 'Item 1' desc='Item #1 · category' states=['enabled', 'sensitive', 'showing', 'visible'] attrs={'setsize': '200'}; grandchild [label] '   1' ... attrs={'setsize': '200'}`
  - `pass2 collections-listview-arrows 'Down: second row (a checkbox row)': grandchild [check box] 'Item 2' desc=None states=['checkable', 'enabled', 'focusable', 'sensitive', 'showing', 'visible'] attrs={'setsize': '200'}`
  - `pass1 collections-feed audit at 'Tab into the feed': unknown-role [unknown] '#1' ... 'a node whose role the adapter could not map'`
  - `pass1 collections-listview-cursor 'Tab out of the list': focus goes to [combo box] 'Theme', not into a row's check box`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/standard\_item.rs:916-935 (no role; AccessNodeBuilder defaults to Role::Unknown, crates/teksilo-core/src/accessibility.rs:476); crates/teksilo-widgets/src/checkbox.rs:592 .focusable(true); accesskit\_atspi\_common-0.21.0/src/node.rs:399-403 size\_of\_set() walks up from any node
- **Evidence (`261a218f`):**
  - `'child [unknown] 'Item 1' desc='Item #1 · category' states=['enabled', 'sensitive', 'showing', 'visible'] attrs={'setsize': '200'}'`
  - `'grandchild [check box] 'Item 2' desc=None states=['checkable', 'enabled', 'focusable', 'sensitive', 'showing', 'visible'] attrs={'setsize': '200'}'`
  - `'grandchild [label] '   1' desc=None states=['enabled', 'sensitive', 'showing', 'visible'] attrs={'setsize': '200'}'`
  - `crates/teksilo-widgets/src/standard_item.rs:916-935 accessibility() sets name/description but no role; checkbox.rs:583 .focusable(true); accesskit_atspi_common-0.20.0/src/node.rs:397-401 size_of_set() walks up from any node`
  - `collections-listview-arrows-20260925-134124-3805659: 'child [unknown] 'Item 1' desc='Item #1 · category' ... attrs={'setsize': '200'}', 'grandchild [label] '   1' ... attrs={'setsize': '200'}'`
- **Reproduced:** 3 of 3 runs
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs (collections-listview-arrows tree outlines) Real, but the layer is split. The '\[unknown\]' duplicate-named node per row is framework: StandardListItem sets name and description but no role, and the builder's default is Role::Unknown. So are the 'focusable' check boxes, since Checkbox is always focusable(true) even inside a data view that keeps it out of the Tab order. 'setsize' on every descendant, however, is AccessKit's behaviour: the AT-SPI adapter computes size\_of\_set with size\_of\_set\_from\_container for any node, walking up to the first ancestor that has one (accesskit\_consumer node.rs:629-641, atspi\_common node.rs:390-394). Teksilo sets size\_of\_set only on the ListBox. That part is upstream.
- **Fix idea:** Once name and description are hoisted (collections-07) and the check state is on the row (collections-03), mark the StandardListItem node GenericContainer so it is pruned. Set focusable(false) or hide an embedded row control from AT.

### collections-m1 {#collections-m1}

ListView and TreeView take focus with no active descendant when nothing is selected, so the reader hears only the container and no row

- **Example:** data-collections
- **Act:** Tab into the ListView (nothing selected yet), the Auto Feed, or the TreeView
- **The reader should get:** As the ARIA listbox and tree patterns specify: on focus with no selection, the cursor is on the first row (without selecting it in a multi-select list), and the reader hears 'Item 1' / 'Documents' after the container.
- **The reader got (`261a218f`):** Focus rests on the container: 'multi-select list box.', 'list box.', 'tree.'. No row is named until an arrow is pressed. Down then lands on row 1, so nothing is skipped, but the reader learns nothing about the content on entry.
- **Platform:** Linux AT-SPI/Orca measured; the same on every adapter by source (no active\_descendant is published)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Tabbing into the ListView, the Auto Feed or the TreeView with nothing selected still rests focus on the container: Orca says 'multi-select list box.', 'list box.' or 'tree.' and no row until an arrow is pressed. Down then lands on row 1, so nothing is skipped.
- **Measured again:** collections-listview-arrows, collections-feed, collections-tree-nav: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 collections-listview-arrows 'Tab into the list': +22.1 ms object:state-changed:focused 1 [list box] ''; +78.0 ms ORCA SAYS: 'multi-select list box.'`
  - `pass2 collections-feed 'Tab into the feed': +15.7 ms object:state-changed:focused 1 [list box] ''; +62.3 ms ORCA SAYS: 'list box.'`
  - `pass1 collections-tree-nav 'Tab into the tree': +14.3 ms object:state-changed:focused 1 [tree] ''; +78.4 ms ORCA SAYS: 'tree.'`
  - `pass2 collections-tree-nav 'Down: Documents': +24.4 ms object:state-changed:focused 1 [tree item] 'Documents'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/list\_view/widget\_impl.rs:1058-1062; crates/teksilo-widgets/src/list\_view.rs:666-676
- **Evidence (`261a218f`):**
  - `collections-listview-arrows-20260925-134124-3805659 'Tab into the list': '+… object:state-changed:focused 1 [list box] ''' only; Orca 'multi-select list box.'`
  - `collections-tree-nav-20260925-133407-3310470 'Tab into the tree': Orca 'tree.'`
  - `crates/teksilo-widgets/src/list_view/widget_impl.rs:1051-1055: active_descendant only when current_row_widget() is Some; list_view.rs:662-672 returns None with no cursor and no selection`
- **Reproduced:** 3 of 3 runs for each view (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** On focus gain with no cursor, set the cursor to the first row (without selecting it in Multi mode) and request an accessibility update, so the first row becomes the active descendant.

### collections-m2 {#collections-m2}

TreeView level is never spoken on Linux, even as the level changes: no NODE\_CHILD\_OF relations reach AT-SPI

- **Example:** data-collections
- **Act:** TreeView: Right to expand Documents, Down into 'Projects' (level 2), Up back to 'Documents' (level 1)
- **The reader should get:** Orca says 'tree level 2' when the cursor enters level 2, and 'tree level 1' when it comes back. Orca does this by itself for a GTK tree (formatting.py:129 suffix 'newNodeLevel').
- **The reader got (`261a218f`):** Only 'Projects.' and 'Documents.'. Orca's nodeLevel walks NODE\_CHILD\_OF relations and gets -1, because accesskit\_atspi\_common exports only ControllerFor. This is the navigation half of collections-06, which covered only the missing level attribute and Where Am I.
- **Platform:** Linux AT-SPI/Orca measured; Windows exports UIA Level (accesskit\_windows node.rs:695-700, by source)
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** Orca still says only 'Projects.' when the cursor enters level 2 and 'Documents.' when it comes back to level 1, never 'tree level 2' or 'tree level 1'. The tree items still carry no level attribute and no NODE\_CHILD\_OF relation on AT-SPI.
- **Measured again:** verify-collections-tree-levels, 2 of 2 runs; adapter read from source
- **Evidence (`c198e4d1`):**
  - `pass1 verify-collections-tree-levels 'Down: Projects, one level down': +138.9 ms ORCA SAYS: 'Projects.'; FAIL Orca says 'level 2'; FAIL [tree item] 'Projects' carries an attribute 'level' / attrs={'posinset': '1'}`
  - `pass1 verify-collections-tree-levels 'Up: back to Documents, level 1': +133.1 ms ORCA SAYS: 'Documents.'; FAIL Orca says 'level 1'`
  - `pass2 verify-collections-tree-levels 'Down: Projects, one level down': +670.1 ms ORCA SAYS: 'Projects.'; FAIL Orca says 'level 2'`
  - `accesskit_atspi_common-0.21.0/src/node.rs:972-989 relation_set exports only RelationType::ControllerFor`
- **Where (`c198e4d1`):** upstream: accesskit\_atspi\_common-0.21.0/src/node.rs:972-989
- **Evidence (`261a218f`):**
  - `verify-collections-tree-levels-20260925-133902-3645182 orca-debug.out:2571 '13:39:26.885943 - SPEECH OUTPUT: 'Projects.''; :2970 '13:39:30.967838 - SPEECH OUTPUT: 'Documents.''; FAIL 'Orca says 'level 2'' and 'Orca says 'level 1''`
  - `orca speech_generator.py:1472-1490 _generateNewNodeLevel; script_utilities.py:1362-1405 nodeLevel via Atspi.RelationType.NODE_CHILD_OF`
  - `accesskit_atspi_common-0.20.0/src/node.rs:960-977 relation_set: only RelationType::ControllerFor`
- **Reproduced:** 2 of 2 runs of verify-collections-tree-levels (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Upstream: export NODE\_CHILD\_OF / NODE\_PARENT\_OF from AccessKit's tree-item level (or from a per-branch group structure). Until then Teksilo could say the new level through the announcer when a tree arrow changes it, on Linux only.

### collections-m3 {#collections-m3}

The row context menu is an unnamed menu, and the reader is never told which command a keyboard press will run

- **Example:** data-collections
- **Act:** ListView row 1, Shift+F10, then Down, Down, Up, then Enter
- **The reader should get:** 'Row actions menu, Move Down' on open, then each command as the highlight moves.
- **The reader got (`261a218f`):** 'menu.' on open, then nothing at all for the arrows (0 events). Enter runs a command the reader never heard. Escape and an AT-SPI click on a menu item do work. This is an addition to collections-05: the menu node has no name either.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** low; **layer:** framework
- **Severity in the sweep:** high. The part that left the reader blind to the command Enter runs is fixed. What remains is a menu with no name, which the reader can work around by pressing Down.
- **Status:** Partly fixed by `9636094c` (menus). What remains is under **Now**.
- **Now (`c198e4d1`):** The arrows in the row menu now read each command ('Move Down.', 'Move to Bottom.', 'Move Down.'), so Enter runs a command the reader heard. The menu itself is still unnamed: Shift+F10 gives only 'menu.'.
- **Measured again:** verify-collections-menu-more, 2 of 2 runs; collections-listview-menu, 2 of 2 runs; fix-menus-context, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 verify-collections-menu-more 'Shift+F10: the row's menu': +39.0 ms object:state-changed:focused 1 [menu] ''; +84.9 ms ORCA SAYS: 'menu.'`
  - `pass1 verify-collections-menu-more 'Down, Down, Up in the menu': +21.2 ms object:state-changed:focused 1 [menu item] 'Move Down', +202.0 ms object:state-changed:focused 1 [menu item] 'Move to Bottom', +386.3 ms object:state-changed:focused 1 [menu item] 'Move Down'; +427.4 ms ORCA SAYS: 'Move Down.'`
  - `pass2 verify-collections-menu-more 'Down, Down, Up in the menu': +149.7 ms object:state-changed:focused 1 [menu item] 'Move Down'; +1008.3 ms ORCA SAYS: 'Move Down.'`
  - `pass2 verify-collections-menu-more 'Shift+F10 again': +93.1 ms object:state-changed:focused 1 [menu] ''; +519.2 ms ORCA SAYS: 'menu.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_list.rs:1121-1156 (the menu is named only through labelled\_by its opener, :1127-1129; a context menu has none)
- **Evidence (`261a218f`):**
  - `verify-collections-menu-more-20260925-133812-3645182 '== Down, Down, Up in the menu' -> '0 events: {}'`
  - `same run orca-debug.out '13:38:32.633274 - SPEECH OUTPUT: 'menu.''; tree '[menu] '' {focusable,focused}'`
  - `crates/teksilo-widgets/src/menu_list.rs:991-993 accessibility(): set_role(Role::Menu) only`
- **Reproduced:** 2 of 2 runs of verify-collections-menu-more, and 3 of 3 of collections-listview-menu
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** As collections-05: publish the highlighted row as active\_descendant, bound at AccessibilityOnly, and name the row menu (for example after the row it acts on).
