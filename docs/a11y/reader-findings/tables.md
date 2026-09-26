<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Tables

Examples: `data-grid`, `tree-table-view`.
19 findings: 5 high, 9 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [tables-01](#tables-01) | data-grid | Tabbing onto the grid says nothing: the table has no name, no cursor and no size | high | Linux | open |
| [tables-02](#tables-02) | data-grid | No table structure reaches any reader: no column header, no row/column position and no table size is ever spoken | high | Linux | upstream |
| [tables-03](#tables-03) | data-grid | Row selection is never spoken: each change rebuilds the table, and the reader hears only the same cell text again | high | Linux | open |
| [tables-04](#tables-04) | tree-table-view | Tree table: expanded / collapsed and the level are never spoken on Linux; expanding is heard as the folder name again | high | Linux | upstream |
| [tables-05](#tables-05) | data-grid | Sorting, column filters, column resize and column reorder cannot be done from the keyboard, and the header offers no action to a screen reader | high | Linux | open |
| [tables-06](#tables-06) | data-grid | Sort state ('sorted ascending') is never exposed to Orca | medium | Linux | upstream |
| [tables-07](#tables-07) | data-grid | Applying a column filter tears the popover down and drops focus on the silent table: no result, no confirmation | medium | Linux | open |
| [tables-08](#tables-08) | data-grid | The filter popover and its field are unnamed, and every trigger is called 'Filter' (untranslated) | medium | Linux | open |
| [tables-09](#tables-09) | data-grid | An empty TextInput has no Text interface on AT-SPI, so the first character typed into it is never reported | medium | Linux | fixed |
| [tables-10](#tables-10) | data-grid | Multi-line cells are read only up to their first line | medium | Linux | open |
| [tables-11](#tables-11) | data-grid | A multi-row TableView does not say it is multi-selectable | low | Linux | open |
| [tables-12](#tables-12) | data-grid | Tab is trapped in the grid: Shift+Tab from the table goes into the first cell, Tab walks all 7000 cells, only Ctrl+Tab leaves, and nothing says so | medium | Linux | open |
| [tables-13](#tables-13) | data-grid | Every cursor or selection move replaces the whole visible table on the bus (216-405 nodes a keystroke) | low | Linux | open |
| [tables-14](#tables-14) | data-grid | The in-cell editor is an unnamed entry: the reader is not told which field they are editing | medium | Linux | open (example) |
| [tables-15](#tables-15) | data-grid | The grid's Selection interface always reports 0 selected children | medium | Linux | upstream |
| [tables-16](#tables-16) | data-grid | data-grid example: 'Reset sort' and 'Reset filters' do nothing, the status line never updates, the Active column reads glyphs | low | all | open (example) |
| [tables-v1](#tables-v1) | data-grid | A live model change the reader did not make re-announces the focused cell and stops current speech | medium | Linux | open |
| [tables-v2](#tables-v2) | data-grid | After a filter is applied or cleared the cursor keeps its index, so the reader lands on a different row without notice | low | Linux | open |
| [tables-v3](#tables-v3) | tree-table-view | An empty cell (a folder's Size) is silent: the reader cannot tell the cursor moved | low | Linux | open (example) |

### tables-01 {#tables-01}

Tabbing onto the grid says nothing: the table has no name, no cursor and no size

- **Example:** data-grid
- **Scenario:** tables-grid-cells
- **Act:** data-grid, tables-grid-cells / tables-grid-tab-trap: 'Tab into the table' (the 5th Tab: Theme, Reset filters, Reset sort, + Append row, then the table)
- **The reader should get:** Focus lands on the grid and the reader hears what it is: a named table with its size (1000 rows, 7 columns), or the current cell (the ARIA grid pattern puts the cursor on a cell when the grid gets focus)
- **The reader gets:** Silence. The focus event names an unnamed \[table\] with no active descendant (no cell cursor until the first arrow key). Orca 46.1 treats a table without the Table interface as a layout table, finds nothing to say ('pauses only'), and only stops speech. The same silence comes back after a column filter is applied (tables-07). TreeTableView fares a little better: Orca says 'tree table.', still with no name and no size.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows/macOS by source only: the grid has no name there either (widget\_impl.rs only names it from a11y\_label).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/widget\_impl.rs:1509-1513 (active\_descendant only when focused\_cell is Some); table\_view/keyboard.rs:141-151 (None = no cursor yet); tree\_table\_view/widget\_impl.rs:1574-1578; examples/data\_grid/src/main.rs:223 and examples/tree\_table\_view/src/main.rs:140 (no .a11y\_label)
- **Evidence:**
  - `3 of 3 runs (v2/135313, r2/140037, r3/140735): r3 report: '+15.7 ms object:state-changed:focused 1 [table] ''' then nothing spoken; check 'Orca has something to say for the new focus' FAIL 'Orca said []'`
  - `r3 orca-debug.out: '14:07:54.841928 - AXTable: [table] is layout only: True (Doesn't support table interface.)' / '14:07:54.846146 - SPEECH GENERATOR: Results for [table] are pauses only' / '14:07:54.848671 NULL SPEECH: stop'`
  - `tree after the act: "[table] name='' interfaces=['Accessible', 'Component', 'Selection'] attributes={} states=['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']"`
  - `tree-table-view, 3 of 3 runs: '14:13:35.243127 - SPEECH OUTPUT: 'tree table.'' and "[tree table] name='' ... states=[... 'multiselectable' ...]"`
  - ``crates/teksilo-widgets/src/table_view/widget_impl.rs:1491-1513: the name is set only from `self.a11y_label`, and active_descendant only when `focused_cell` is Some (it is None until an arrow/Tab moves the cursor); tree_table_view/widget_impl.rs:1559-1578 is the same``
  - ``examples/data_grid/src/main.rs:223 and examples/tree_table_view/src/main.rs:140 never call `.a11y_label(..)` (table_view.rs:1017, tree_table_view.rs:915)``
  - `Orca /usr/lib/python3/dist-packages/orca/ax_table.py:1060-1082 is_layout_table: no Table interface means layout only unless the table has a name`
  - `a1 tables-grid-cells 'Tab into the table': '14:20:39.565653 object:state-changed:focused 1 [table] ''' then orca-debug.out '14:20:39.604841 - AXTable: [table] is layout only: True (Doesn't support table interface.)' / '14:20:39.606954 - SPEECH GENERATOR: Results for [table] are pauses only'`
  - `v1 verify-tables-entry: first entry 'Orca said nothing'; second entry '14:28:46.400559 object:state-changed:focused 1 [table cell]' -> '14:28:46.497483 SPEECH OUTPUT: '1.''`
  - `/usr/lib/python3/dist-packages/orca/ax_table.py:1063-1076 elif order: layout-guess, then 'Doesn't support table interface', only later 'Has name or description'`
  - `run dirs: target/reader-verify/tables/{a1,a2,a3}/tables-grid-cells-*, {v1,v2,v3}/verify-tables-entry-*`
- **Reproduced:** 3 of 3 runs (data-grid); 3 of 3 (tree-table-view 'tree table.' with no name)
- **Verification:** corrected by the verifier. Reproduced: first entry silent 3 of 3 (a1/a2/a3 tables-grid-cells) + 3 of 3 (v1/v2/v3 verify-tables-entry) + 3 of 3 tab-trap; TTV 'tree table.' with no name 3 of 3 (a1/a2/a3) + 3 of 3 (v1/v2/v3 verify-tables-ttv). Re-entry reads the cell 3 of 3 (v1/v2/v3). Real, but narrower and partly mis-sourced. (1) The silence is only the FIRST entry, when the view has no cursor. Once the cursor exists, Tab back in lands on the cell and Orca reads it (verify-tables-entry 'Tab back into the table (second entry)': '+13.5 ms object:state-changed:focused 1 \[table cell\]', Orca '1.', 3 of 3), and a filter applied with a cursor also lands on a cell (see tables-07). (2) The Orca claim 'layout only unless the table has a name' is wrong: ax\_table.py:1063-1076 tests 'Doesn't support table interface' BEFORE 'Has name or description', so a named table is still layout-only. A name WOULD be spoken (formatting.py TABLE focused = 'leaving or (labelAndName + pause + table)', generator.py:390-405), but the size never: generator.py:968 \_generateTable returns \[\] for a layout-only table. So the 'should' (size 1000 x 7) is undeliverable on Linux whatever Teksilo does (upstream). (3) Layer split: the missing name is the examples' omission (both views have .a11y\_label, table\_view.rs:1014-1019 / tree\_table\_view.rs:913-917, whose doc says it is 'required when the page hosts more than one table', i.e. reads as optional); the cursor-less landing that makes the focus stop say nothing is the framework's. Severity high kept: an unnamed focus stop that speaks nothing.
- **Fix idea:** When the view gains focus with no cursor, put the cursor on the selected row's cell or the first cell (so active\_descendant names a cell and the reader hears it), and have the examples name their tables with .a11y\_label(..). A debug-build warning for an unnamed Grid/TreeGrid would catch the rest.

### tables-02 {#tables-02}

No table structure reaches any reader: no column header, no row/column position and no table size is ever spoken

- **Example:** data-grid
- **Scenario:** tables-grid-cells
- **Act:** data-grid and tree-table-view, every cell move: ArrowDown onto row 1, ArrowRight to Name / Email, Ctrl+End to row 1000; tree table ArrowDown / ArrowRight on a leaf
- **The reader should get:** Moving to a new column says that column's header ('Name', 'Email', 'Size'), and a reader can learn where they are ('row 2 of 1000', or the ID of the row). The table's size is spoken on entry.
- **The reader gets:** Only the cell's text: '1.', '2.', 'Blake 1.', 'user1@example.com.', '4321 B.'. After Ctrl+End to row 1000 on the Notes column Orca says only '—': the reader has no idea they are on the last of 1000 rows. Orca cannot find a table for the cell and no row or column index. Of what the widget publishes (Role::Grid/TreeGrid with row\_count/column\_count, Role::ColumnHeader, per-cell row\_index/column\_index, per-row position\_in\_set), Orca 46.1 reads none: row\_count, column\_count, row\_index, column\_index and level are not exported on AT-SPI; posinset is exported on tree rows but Orca's default script does not read it. Of 1000 rows, 25 are on the bus at the top and 14 (987 to 1000) after Ctrl+End, and nothing tells a reader about the other 975.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows by source: accesskit\_windows-0.35.0 implements no UIA Grid/GridItem/Table/TableItem pattern (node.rs:1333-1496 lists Toggle, Invoke, Value, RangeValue, ScrollItem, SelectionItem, Selection, Text, ExpandCollapse, Window only), so NVDA gets no coordinates or header either. macOS by source: accesskit\_macos-0.27.0 exposes only accessibilityRows/SelectedRows (node.rs:1106-1135), with no column headers or indices.
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** upstream accesskit\_atspi\_common-0.20.0/src/node.rs:415-436 (attributes), 494-521 (interfaces); Teksilo mitigation point crates/teksilo-widgets/src/table\_view/a11y.rs:96-101 (CellA11y::with\_name dead code)
- **Evidence:**
  - `3 of 3 runs, check 'Orca says 'ID' as a word' / 'Orca says 'Name'' / 'Orca says 'Email'' / 'Orca says 'row 1'' / 'Orca says '1000'' all FAIL; r3: '14:07:59.692017 - SPEECH OUTPUT: '1.'', '14:08:07.458374 - SPEECH OUTPUT: 'Blake 1.'', '14:08:25.719592 - SPEECH OUTPUT: '—''`
  - `r3 orca-debug.out: '14:07:59.679077 - AXTable: Couldn't find table-implementing ancestor for [table cell]' / '14:07:59.679311 - AXTable: Row and col index attributes for [table cell]: None, None'`
  - `focused cell as a reader can query it: "focused [table cell] name='' states=['enabled', 'focused', 'selectable', 'selected', 'sensitive', 'showing', 'visible'] attributes=None interfaces=['Accessible', 'Component'] actions=None"`
  - `tree-table-view 3 of 3: 'ArrowRight on a leaf moves to the Size column' said '4321 B.' only; check 'Orca says 'Size'' FAIL`
  - `rows on the bus after Ctrl+End: "14 body rows on the bus; first cells: ['987', '988', ... '1000']"`
  - `accesskit_atspi_common-0.20.0/src/node.rs:494-521 (interfaces: no Table/TableCell), node.rs:415-436 (attributes: only placeholder-text, posinset, setsize, id, braillelabel, brailleroledescription)`
  - `Orca ax_table.py:1060-1082 (layout table without Table interface), speech_generator newColumnHeader needs a table-implementing ancestor`
  - `a1 orca-debug.out '14:20:44.652858 - AXTable: Couldn't find table-implementing ancestor for [table cell]' / '14:20:44.653217 - AXTable: Row and col index attributes for [table cell]: None, None'`
  - `a1/a2/a3 Ctrl+End: Orca '—' only; '14 body rows on the bus; first cells: ['987', ... '1000']'`
  - `accesskit_macos-0.27.0/src/node.rs:1106-1135 rows()/selected_rows() use node.items(filter)`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 (a1/a2/a3 tables-grid-cells), 3 of 3 (a1/a2/a3 tables-ttv-tree); deterministic
- **Fix idea:** Upstream: AccessKit needs the AT-SPI Table/TableCell interfaces and the UIA Grid/Table(Item) patterns. Until then Teksilo could carry the missing context itself: name each cell 'Name: Blake 1' on a column change, or give the focused cell a description such as 'row 2 of 1000' (CellA11y::with\_name exists but is dead code, a11y.rs:96-101). An announcement on a column change is the other route, through the K2-fixed announcer.

### tables-03 {#tables-03}

Row selection is never spoken: each change rebuilds the table, and the reader hears only the same cell text again

- **Example:** data-grid
- **Scenario:** tables-grid-selection
- **Act:** data-grid (MultiRow), tables-grid-selection: ArrowDown (selects row 1), Space (unselect), Space (select), Shift+ArrowDown, Ctrl+A
- **The reader should get:** A toggle is a selected-state change on the cell the reader is on (object:state-changed:selected), which Orca turns into 'selected' / 'not selected'. Select-all is reported.
- **The reader gets:** No object:state-changed:selected and no object:selection-changed event, ever. Every selection change replaces the whole body (216 nodes; 405 on the first move, headers included), and a brand-new cell node takes focus. So Orca re-reads the cell text: Space gives '1.', Space again gives '1.', Ctrl+A gives '2.', with no selection word. The tree after the act does hold the new state (row 1 selected, then not). The status line never changes either (tables-16). Also, the table's AT-SPI Selection interface reports 0 selected children throughout (tables-15).
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Orca never saw the Space key (a harness limitation), but its onSelectedChanged path needs a state-changed:selected event on the focused object, which the bus never carried, so a real keyboard would not change this. Windows not measured.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/widget\_impl.rs:205-224 and table\_view/body\_pane.rs:256-272 (selection bumps Rebuild versions); tree\_table\_view/body\_pane.rs:246-268
- **Evidence:**
  - `3 of 3 runs: checks 'a object:state-changed:selected event from [*]' FAIL on ArrowDown, Space, Space; 'a object:selection-changed event from [table]' FAIL; 'the row '1' lacks 'selected'' pass after Space (so the state did change)`
  - `r3 Space act: '+95.6 ms object:state-changed:focused 1 [table cell] ''' (a new focus, not a state change); check 'evidence: the focus gains of the act (at most 0)' FAIL`
  - `r3 orca-debug.out: '14:09:10.399590 - SPEECH OUTPUT: '1.'' (Space off), '14:09:14.908672 - SPEECH OUTPUT: '1.'' (Space on), '14:09:24.131253 - SPEECH OUTPUT: '2.'' (Ctrl+A); 'Orca says 'not selected'' FAIL, 'Orca says 'selected'' FAIL`
  - `defunct counts per act (v2/r2/r3 alike): ArrowDown 405 {'label': 193, 'table cell': 175, 'table row': 26, 'column header': 7, 'panel': 1, 'push button': 3}; Space / Space / Shift+Down / Ctrl+A 216 each`
  - ``crates/teksilo-widgets/src/table_view/widget_impl.rs:205-215 (`rs.observe_for_rebuild` bumps the root `version`, bound at BindingLevel::Rebuild at :19) and table_view/body_pane.rs:256-264 (the same for the body pane): the selected state is baked into freshly built CellA11y/BodyRow nodes instead of being updated on the existing ones``
  - `Orca scripts/default.py:1487-1527 onSelectedChanged: announces only on a state-changed:selected from the locus of focus; speech_generator.py:1062-1104 _generateUnselectedCell needs a parent with the Selection interface (the row has none)`
  - `a1 tables-grid-selection 'Space unselects row 1': '14:21:55.356743 object:state-changed:focused 1 [table cell]'; ORCA '14:21:55.577957: '1.''; row '1' states lack 'selected' afterwards`
  - `v1 verify-tables-ttv 'Space selects the cursor row': '+19.5 ms object:state-changed:focused 1 [table cell]', Orca 'README.', 28 nodes defunct`
  - `run dirs: target/reader-verify/tables/{a1,a2,a3}/tables-grid-selection-*, {v1,v2,v3}/verify-tables-ttv-2*`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 (a1/a2/a3 tables-grid-selection); TreeTableView Space 3 of 3 (v1/v2/v3 verify-tables-ttv); deterministic Reproduced: no object:state-changed:selected / object:selection-changed ever; Space and Ctrl+A each produce a new focus event on a fresh cell and Orca re-reads '1.' / '2.'. Corrections: (a) it is not only the body that is replaced: every selection change replaces the whole TableView subtree, header row included (roles resolved in the selection runs: Space = {'label':100,'table cell':91,'table row':14,'column header':7,'push button':3,'panel':1}; the bus shows children-changed remove/add \[table\]-&gt;\[table row\] AND \[table\]-&gt;\[panel\] in every act). 405 vs 216 is the number of realised rows (26 on the 30 px estimate vs 14 once measured), not headers vs body. (b) It also holds in TreeTableView: Space on a row gives a new focus and 'README.' again, no selection word. (c) There is an upstream part: even with nodes kept, atspi\_common would emit state-changed:selected (node.rs:594-608) but never object:selection-changed for a grid (adapter.rs:260-267 requires is\_item\_like, which excludes Row/Cell), and Orca's 'not selected' on focus is suppressed for a layout-only table (speech\_generator.py:1062-1104). Keeping nodes would still make Orca speak on Space (default.py:1487-1527, keyString == 'space'), so the framework fix stands. Windows by source: in a row-selection table neither Role::Row nor Role::Cell supports UIA SelectionItem (accesskit\_windows node.rs:648-666, 'TODO: tables (#29)'), so NVDA gets no IsSelected at all; macOS isAccessibilitySelected is gated on is\_item\_like (accesskit\_macos node.rs:1265-1268). So the selected state is published only on AT-SPI, and there it is never announced.
- **Fix idea:** Keep row/cell nodes across a selection change and update `selected` in place (bind the selection at AccessibilityOnly/RepaintOnly, as GridView does per the project notes), so the adapter emits state-changed:selected on the focused cell. Announcing the selection count through the announcer, as GridView does, would cover Ctrl+A and Shift ranges.

### tables-04 {#tables-04}

Tree table: expanded / collapsed and the level are never spoken on Linux; expanding is heard as the folder name again

- **Example:** tree-table-view
- **Scenario:** tables-ttv-tree
- **Act:** tree-table-view, tables-ttv-tree: ArrowDown to 'docs', ArrowRight (expand), ArrowDown onto 'README.md', ArrowLeft to parent, ArrowLeft (collapse)
- **The reader should get:** 'docs, collapsed' on arrival, 'expanded' / 'collapsed' when toggled, and the depth ('level 2') on a child
- **The reader gets:** 'docs.' on arrival, 'docs.' after ArrowRight, 'README.md.' on the child, 'docs.' after ArrowLeft. The children do appear and leave the bus, so the function works, but a reader is told nothing. The widget publishes level/expanded on the row and mirrors them onto the tree-column cell (a11y.rs with\_level/with\_expanded), but accesskit\_atspi\_common 0.20 exports no expandable/expanded state and no level attribute at all. Teksilo adds to the loss: every expand/collapse rebuilds the whole tree table (header row and body pane removed and re-added, 35 to 58 nodes defunct), so the change reaches Orca as focus on a new cell, not as a state change on the cell the reader is on.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows by source: accesskit\_windows maps expanded to the ExpandCollapse pattern and level to AriaProperties 'level' (node.rs:500-502, 1485), so NVDA can hear it. macOS by source: accesskit\_macos-0.27.0 has no disclosure/level methods (grep of node.rs finds none).
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** upstream accesskit\_atspi\_common-0.20.0/src/node.rs:301-386 (state), 415-436 (attributes); Teksilo rebuild at crates/teksilo-widgets/src/tree\_table\_view/widget\_impl.rs:131-182
- **Evidence:**
  - `3 of 3 runs: 'Orca says 'collapsed'' FAIL, 'Orca says 'expanded'' FAIL, 'Orca says 'level 1'' / 'level 2'' FAIL; 'the rows on the bus include 'README.md'' pass, 'the children left the bus' pass`
  - `r3 orca-debug.out: '14:13:44.035672 - SPEECH OUTPUT: 'docs.'' (arrive), '14:13:48.727419 - SPEECH OUTPUT: 'docs.'' (ArrowRight), '14:13:52.728374 - SPEECH OUTPUT: 'README.md.'' (child), '14:14:08.250420 - SPEECH OUTPUT: 'docs.'' (ArrowLeft collapse)`
  - `r3 events on ArrowRight: '14:13:48.503815 object:children-changed:add [tree table] -> table row', '14:13:48.508349 object:children-changed:remove [tree table]', '14:13:48.508602 object:state-changed:focused 1 [table cell]'; "35 nodes went defunct in the act: {'<Error>': 23, 'table row': 5, 'column header': 3, 'table cell': 1, 'push button': 2, 'panel': 1}"`
  - `focused cell on 'docs': "states=['enabled', 'focused', 'selectable', 'selected', 'sensitive', 'showing', 'visible'] attributes=None" (no expandable/collapsed state)`
  - `accesskit_atspi_common-0.20.0/src/node.rs:301-386 state(): no State::Expandable/Expanded/Collapsed (grep -i expand finds nothing); node.rs:415-436 attributes(): no level`
  - `crates/teksilo-widgets/src/tree_table_view/widget_impl.rs:131-182 (a projection change bumps the root version, rebuild at :19)`
  - `a3 tables-ttv-tree 'ArrowRight expands docs': 35 defunct {'column header': 3, 'push button': 2, 'table row': 5, ...}; ORCA '14:39:59.473636: 'docs.''`
  - `a1 note 'on docs: focused [table cell] ... states=['enabled','focused','selectable','selected','sensitive','showing','visible'] attributes=None'`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 (a1/a2/a3 tables-ttv-tree); deterministic
- **Fix idea:** Upstream: accesskit\_atspi\_common should export Expandable/Expanded/Collapsed and a level attribute. Teksilo could keep nodes across expand/collapse (update the flags in place), and until the adapter carries the state it could announce 'expanded' / 'collapsed' through the announcer (K2-fixed) on Linux.

### tables-05 {#tables-05}

Sorting, column filters, column resize and column reorder cannot be done from the keyboard, and the header offers no action to a screen reader

- **Example:** data-grid
- **Scenario:** tables-grid-headers
- **Act:** data-grid, tables-grid-headers: read the headers; AT-SPI 'click' on the Name header; tables-grid-tab-trap / tabwalk: Tab order
- **The reader should get:** A keyboard or screen-reader user can sort by a column (focus a header and press Enter/Space, or a menu), open its filter, resize it and move it
- **The reader gets:** The header cells are focusable(false) and never a Tab stop, and keyboard.rs has no chord for sort, reorder or resize. The Filter triggers are not focusable either (reachable only by pointer or by a screen reader's own activation). On AT-SPI the \[column header\] offers no action at all, so a screen reader's activation is refused. The resize path Teksilo built for AT (Increment/Decrement on the header) is unreachable on Linux, where AT-SPI exposes only 'click', and on Windows, where UIA RangeValue needs a numeric value the header deliberately omits. Only macOS can perform it.
- **Platform:** Linux AT-SPI (measured). Windows and macOS by source (accesskit\_windows node.rs:599-601 RangeValue gated on numeric\_value; accesskit\_macos node.rs:1243-1247 allows accessibilityPerformIncrement when the action is supported).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/header.rs:1075 (.focusable(false)), 941-1016 (sort cycle only on PointerUp), 1030-1062 (on\_access\_action handles only Increment/Decrement), 1175-1182 (actions advertised); table\_view/keyboard.rs (no sort/resize/reorder chord); header.rs:555 (Filter trigger)
- **Evidence:**
  - `3 of 3 runs: "[column header] 'ID' interfaces=['Accessible', 'Component'] attributes={} actions=[] states=['enabled', 'sensitive', 'showing', 'visible'] description=None"; the same for 'Name'`
  - `3 of 3 runs, AT-SPI click on the Name header: "refused: {'path': '/org/a11y/atspi/accessible/0/79228162698731778330639466496', 'name': 'Name', 'role': 'column header'} offers no action on AT-SPI"`
  - `Filter triggers on the bus: "3 [push button] 'Filter' nodes on the bus, states: [['enabled', 'sensitive', 'showing', 'visible'], ...]" (no 'focusable'); the Tab walk goes '+ Append row push button.' then the table`
  - ``crates/teksilo-widgets/src/table_view/header.rs:1075 `.focusable(false)`; the sort cycle lives only in the PointerUp arm (header.rs ~984-1000); header.rs:1175-1182 advertises Increment/Decrement only; crates/teksilo-widgets/src/table_view/keyboard.rs has no sort/reorder/resize key``
  - `accesskit_atspi_common-0.20.0/src/node.rs:532-544: n_actions is 1 only when clickable, and the only action name is 'click'`
  - `a3 'AT-SPI click on the Name header': "refused: {'path': '/org/a11y/atspi/accessible/0/79228162698731778330639466496', 'name': 'Name', 'role': 'column header'} offers no action on AT-SPI"`
  - `a1 filter tree: "push button 'Filter' ... states ['enabled','sensitive','showing','visible'] interfaces ['Accessible','Action','Component'] actions [{'name': 'click'}]"`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 (a1/a2/a3 tables-grid-headers); deterministic
- **Fix idea:** Make the header row keyboard-reachable (a roving header row above the body, or a header context menu on Shift+F10 / the Menu key with Sort ascending/descending, Filter…, Move left/right, Wider/Narrower), advertise Action::Click on sortable headers and route it to the sort cycle, and make the Filter trigger a real focusable button.

### tables-06 {#tables-06}

Sort state ('sorted ascending') is never exposed to Orca

- **Example:** data-grid
- **Scenario:** tables-grid-headers
- **Act:** data-grid, tables-grid-headers: read the ID header (the example sorts by ID ascending at launch), then activate 'Reset sort'
- **The reader should get:** The ID header says it is sorted ascending
- **The reader gets:** The header's AT-SPI attributes are empty. The widget sets sort\_direction on the header (header.rs:1187-1193), but accesskit\_atspi\_common exports no 'sort' attribute, and Orca 46.1's default script hard-codes isSorted() to False anyway (only its web script reads aria-sort). A Linux reader can never learn the sort order.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows by source: accesskit\_windows writes AriaProperties 'sort=ascending' (node.rs:514-525); whether NVDA speaks it for a UIA DataItem was not measured. macOS: no sort export in accesskit\_macos-0.27.0 (grep).
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/table\_view/header.rs:1187-1193 (sets sort\_direction); upstream accesskit\_atspi\_common node.rs:415-436, Orca script\_utilities.py:1039-1046
- **Evidence:**
  - `3 of 3 runs: 'the 'ID' header publishes its sort' FAIL: "[column header] 'ID' interfaces=['Accessible', 'Component'] attributes={} actions=[] ..."`
  - `accesskit_atspi_common-0.20.0/src/node.rs:415-436 attributes() carries no sort`
  - `/usr/lib/python3/dist-packages/orca/script_utilities.py:1039-1062: isSorted returns False, so getSortOrderDescription is always ''`
  - `a3 'read the headers': "[column header] 'ID' interfaces=['Accessible', 'Component'] attributes={} actions=[] ... description=None"`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 (a1/a2/a3 tables-grid-headers); deterministic Real and correctly attributed (attributes={} on the ID header; atspi\_common has no sort attribute; Orca's default isSorted() returns False unconditionally; accesskit\_windows writes 'sort=ascending' at node.rs:514-525; no sort in accesskit\_macos). Severity lowered to medium: the order is context, not the content, and the header is not a focus stop, so it is met only in flat review/object navigation. The fix idea (sort in the header's description) would therefore reach only those users on Linux.
- **Fix idea:** Upstream on both sides (AccessKit attribute, Orca default script). Teksilo could put the sort state in the header's description ('sorted ascending'), which Orca does read, until then.

### tables-07 {#tables-07}

Applying a column filter tears the popover down and drops focus on the silent table: no result, no confirmation

- **Example:** data-grid
- **Scenario:** tables-grid-filter
- **Act:** data-grid, tables-grid-filter: AT-SPI click on the Name column's Filter, type 'Blake', press Enter
- **The reader should get:** The table is filtered, the reader is told (at least where focus is; ideally how many rows are left), and focus stays somewhere meaningful (the field, the header, or a cell)
- **The reader gets:** Enter commits the filter (filter.rs applies only on Enter). The resulting TableView rebuild destroys the header and the popover inside it, and focus falls to the table container, which has no name and no active descendant, so Orca says nothing ('pauses only'). Orca also drops the field's focus-lost event as defunct. The rows are filtered correctly (only Blake rows on the bus), but a reader has no way to know. Escape afterwards does nothing: the popover is already gone.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/widget\_impl.rs:120-124 (filters\_signal effect bumps the Rebuild version), 1509-1513; table\_view/filter.rs:188-199
- **Evidence:**
  - `3 of 3 runs: 'Orca has something to say for the new focus' FAIL 'Orca said []'; 'the table holds only Blake rows' pass`
  - `r3 events: '+139.1 ms object:state-changed:focused 1 [table] ''' then '+139.1 ms object:state-changed:focused 0 [entry] '''`
  - `r3 orca-debug.out: '14:11:18.174322 - AXTable: [table] is layout only: True (Doesn't support table interface.)', '14:11:18.184077 - SPEECH GENERATOR: Results for [table] are pauses only', '14:11:18.186609 - EVENT MANAGER: Ignoring defunct object: [entry]'`
  - `note after Enter: "focused [table] name='' states=['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible'] attributes=None interfaces=['Accessible', 'Component', 'Selection']"`
  - `crates/teksilo-widgets/src/table_view/filter.rs:188-199 (commit on Enter; its own comment says a filter change rebuilds the owning TableView, which tears the popover down); table_view/widget_impl.rs filters_signal effect bumps the root rebuild version`
  - `v1 verify-tables-filter-cursor 'Enter applies the filter (with a cursor set before)': '14:29:38.322605 object:state-changed:focused 1 [table cell]', 220 defunct {'dialog':1,'entry':1,'status bar':1,'column header':7,...}, ORCA '14:29:38.519800: 'Blake 1.''`
  - `a1 tables-grid-filter Enter: focus '[table]', 'Orca said []', '14:24:04.433638 EVENT MANAGER: Ignoring defunct object: [entry]'`
- **Reproduced:** 3 of 3 runs
- **Verification:** corrected by the verifier. Reproduced: silence without a cursor 3 of 3 (a1/a2/a3 tables-grid-filter); with a cursor 3 of 3 lands on a cell and is read (v1/v2/v3 verify-tables-filter-cursor) The popover teardown by the rebuild is real (dialog/entry/status bar go defunct, Escape afterwards has nothing to close, 3 of 3). But the silence is only the no-cursor case of tables-01: when the grid already had a cursor, Enter lands focus on a cell at the SAME index and Orca reads it ('Blake 1.', 3 of 3), and Clear does the same. What remains for every reader: no confirmation that the filter applied and no count, and the cursor keeps its index rather than its row (see the missed finding on that). Severity lowered to medium: the usual path (a reader who has been in the grid) hears a matching row; the silent path is tables-01's defect again.
- **Fix idea:** Keep the header (and its open popover) across a filter change, or close the popover explicitly and return focus to the header/first cell. Announce the outcome through the announcer ('12 rows match').

### tables-08 {#tables-08}

The filter popover and its field are unnamed, and every trigger is called 'Filter' (untranslated)

- **Example:** data-grid
- **Scenario:** tables-grid-filter
- **Act:** data-grid, tables-grid-filter: activate the Name column's Filter
- **The reader should get:** A named popover and field ('Filter Name'), and triggers distinguishable by column
- **The reader gets:** Orca says 'Name column header.' 'dialog' 'entry Filter…'. The \[dialog\] and the \[entry\] have no name, and the field is identified only by its placeholder 'Filter…'. It is a hard-coded English literal, as is the trigger name 'Filter', which all three columns share. The popover also holds an empty, unnamed \[status bar\] (0x0 extents) and a 'Clear' button whose description repeats its name.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows by source: the placeholder maps to UIA HelpText, not Name.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/filter.rs:132 (String::from("Filter…")); header.rs:555 (named(lit!("Filter")))
- **Evidence:**
  - `3 of 3 runs: r3 '14:11:09.714216 - SPEECH OUTPUT: 'Name column header.'', '14:11:09.714261 - SPEECH OUTPUT: 'dialog'', '14:11:09.714281 - SPEECH OUTPUT: 'entry Filter…''`
  - `checks 'the tree holds [dialog] 'Name'' FAIL and 'the tree holds [entry] 'Name'' FAIL (r2, r3)`
  - `r2 tree: "'name': '', 'role': 'dialog'", "'name': '', 'role': 'entry', ... 'attributes': {'placeholder-text': 'Filter…'}", "'name': 'Clear', 'role': 'push button', 'description': 'Clear'", "'name': '', 'role': 'status bar', ... 'extents': [303, 156, 0, 0]"`
  - `` crates/teksilo-widgets/src/table_view/filter.rs:132 `placeholder: String::from("Filter…")`; header.rs:555 `OverlayTrigger::around(glyph).named(lit!("Filter").resolve_now())` ``
  - `a1 tree 'activate the Name column's Filter': "[column header] 'Name' > [push button] 'Filter', [dialog] '' {active} > [entry] '' attrs={'placeholder-text': 'Filter…'}, [push button] 'Clear' desc='Clear', [status bar] ''"`
  - `a3 ORCA '14:38:12.808012: 'Name column header.'', '14:38:12.808041: 'dialog'', '14:38:12.808057: 'entry Filter…''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 (a1/a2/a3 tables-grid-filter); deterministic
- **Fix idea:** Name the trigger and the field from the column label through the i18n layer ('Filter Name'), label the popover from it, and drop the empty status node when there is no message.

### tables-09 {#tables-09}

An empty TextInput has no Text interface on AT-SPI, so the first character typed into it is never reported

- **Example:** data-grid
- **Scenario:** tables-grid-filter
- **Act:** data-grid, tables-grid-filter: type 'Blake' into the (empty) filter field
- **The reader should get:** The field supports the Text interface from the start, and each typed character arrives as object:text-changed:insert, the first included
- **The reader gets:** While the field is empty it exposes only Accessible and Component (no Text or EditableText, so there are no text ranges and no caret). The bus then carries inserts for 'l', 'a', 'k', 'e' at offsets 1-4, and none for the 'B' at offset 0. The text-input code intends to emit an empty run for an empty field (text\_input\_field/widget\_impl.rs:1100-1107), but this field still exposes no Text interface; accesskit\_consumer's supports\_text\_ranges needs at least one TextRun (text.rs:1402-1406). The same entry does have Text/EditableText once it holds text (the F2 cell editor). The cause inside the text-run emission path was not traced further. Other text fields probably share this: across other agents' runs the first entry inserts are also often at offset 1.
- **Platform:** Linux AT-SPI (measured). By source the same consumer gate applies on Windows (Text pattern) and macOS.
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-core/src/accessibility/text\_runs.rs:228-296 (TextRunSource::from\_geometry) reached from crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:1108-1140
- **Evidence:**
  - `3 of 3 runs: "focused [entry] name='' states=['editable', ... 'focused', 'selectable-text', ...] attributes={'placeholder-text': 'Filter…'} interfaces=['Accessible', 'Component'] actions=None"`
  - `3 of 3 runs, inserts on the bus: '"detail1": 1 "text": "l" "detail1": 2 "text": "a" "detail1": 3 "text": "k" "detail1": 4 "text": "e"' (no insert at offset 0 for 'B'; the filter then matched 'Blake', so the B was typed)`
  - `compare the non-empty cell editor: "focused [entry] ... interfaces=['Accessible', 'Component', 'EditableText', 'Text']"`
  - ``accesskit_consumer-0.39.0/src/text.rs:1402-1406 supports_text_ranges requires `self.text_runs().next().is_some()`; accesskit_atspi_common-0.20.0/src/node.rs:486-488``
  - `a1 'type Blake': '14:24:00.421858 object:text-changed:insert 1 [entry] '' text='l'' ... 'insert 4 ... text='e'' (no insert at 0 for 'B'); note 'focused [entry] ... attributes={'placeholder-text': 'Filter…'} interfaces=['Accessible', 'Component']'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 6 of 6 runs (a1/a2/a3 tables-grid-filter, v1/v2/v3 verify-tables-filter-cursor): inserts at offsets 1-4 only, empty field interfaces \['Accessible','Component'\]
- **Fix idea:** Make sure TextInputField emits the empty TextRun on its first accessibility pass even before a measurement is retained (the `retained` branch at widget\_impl.rs:1111-1138), and add a test that walks a TreeUpdate for an empty, placeholder-bearing field through accesskit\_consumer and asserts supports\_text\_ranges.

### tables-10 {#tables-10}

Multi-line cells are read only up to their first line

- **Example:** data-grid
- **Scenario:** tables-grid-cells
- **Act:** data-grid: ArrowRight to the Notes column of row 2 ('Onboarded in batch 1.' / 'Pending equipment request.'); tree-table-view: ArrowDown onto 'README.md' (description 'Project overview.' / 'Start here before anything else.')
- **The reader should get:** The whole cell: 'Onboarded in batch 1. Pending equipment request.'; 'README.md, Project overview. Start here before anything else.'
- **The reader gets:** 'Onboarded in batch 1.' and 'README.md.' only. Cells carry no name, so Orca picks the first descendant with text as the cell's content (script\_utilities.realActiveDescendant) and reads that one label. The second and third lines are unreachable without flat review.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows by source: a Role::Cell takes no label from its descendants (accesskit\_consumer node.rs:723-735 derives from descendants only for buttons, links, menu items, check/radio boxes), so the focused UIA DataItem has an empty Name; what NVDA says for it was not measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/a11y.rs:96-101 (with\_name dead code, no public cell label API); examples/data\_grid/src/main.rs:170-180 and examples/tree\_table\_view/src/main.rs:143-155 compose a cell from several labels
- **Evidence:**
  - `r2 and r3: 'Orca says 'Pending equipment request'' FAIL; Orca said 'Editor.' | '$35137.' | '● Yes.' | 'Onboarded in batch 1.' (the act was added after v2, so 2 of 2 runs)`
  - `r3 orca-debug.out: '14:08:19.869046 - AXObject: find_descendant: found [label: 'Onboarded in batch 1.'] in 0.0016s' then '14:08:19.878594 - SPEECH OUTPUT: 'Onboarded in batch 1.''`
  - `tree-table-view 3 of 3: '14:13:52.721394 - AXObject: find_descendant: found [label: 'README.md'] in 0.0018s' / '14:13:52.728374 - SPEECH OUTPUT: 'README.md.''`
  - `/usr/lib/python3/dist-packages/orca/script_utilities.py:1621-1632 (unnamed table cell: first descendant with displayed text)`
  - `crates/teksilo-widgets/src/table_view/a11y.rs:96-101 CellA11y::with_name is #[allow(dead_code)]; no cell is ever named`
  - `a3 ORCA '14:37:38.644568: 'Onboarded in batch 1.'' (Notes of row 2), check 'Orca says 'Pending equipment request'' FAIL`
- **Reproduced:** 2 of 2 runs (data-grid Notes), 3 of 3 runs (tree-table-view README.md)
- **Verification:** confirmed. Reproduced: data-grid Notes 3 of 3 (a1/a2/a3); tree-table-view README.md 3 of 3 (a1/a2/a3)
- **Fix idea:** Name each CellA11y from its content (join the descendant labels, or labelled\_by them), or expose a cell\_label\_fn on Column so a delegate can supply the spoken text.

### tables-11 {#tables-11}

A multi-row TableView does not say it is multi-selectable

- **Example:** data-grid
- **Scenario:** tables-grid-headers
- **Act:** data-grid (TableSelectionMode::MultiRow), tables-grid-headers: read the tree
- **The reader should get:** The grid carries the multiselectable state (UIA CanSelectMultiple on Windows), as TreeTableView already does
- **The reader gets:** The \[table\] states are enabled, focusable, sensitive, showing, visible: no multiselectable. TreeTableView sets it (its own comment explains why), TableView does not.
- **Platform:** Linux AT-SPI (measured); Windows by source (UIA SelectionCanSelectMultiple is read from is\_multiselectable, accesskit\_windows node.rs:1417)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/widget\_impl.rs:1470-1513 (no set\_multiselectable); compare tree\_table\_view/widget\_impl.rs:1539-1557
- **Evidence:**
  - `3 of 3 runs: 'the tree holds [table] '*'' (state multiselectable) FAIL: 'no such node in the tree after the act'`
  - `data-grid tree: "[table] name='' ... states=['enabled', 'focusable', 'sensitive', 'showing', 'visible']"; tree-table-view: "[tree table] ... states=['enabled', 'focusable', 'focused', 'multiselectable', ...]"`
  - `crates/teksilo-widgets/src/table_view/widget_impl.rs:1470-1513 has no set_multiselectable; crates/teksilo-widgets/src/tree_table_view/widget_impl.rs:1539-1557 has it`
  - `a1 selection tree: "[table] ... states ['enabled','focusable','sensitive','showing','visible']"; v1 ttv: "[tree table] ... states=['enabled','focusable','focused','multiselectable',...]"`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 (a1/a2/a3 tables-grid-headers); deterministic Real (data-grid \[table\] states lack multiselectable, tree table has it). Severity lowered: Orca 46.1 speaks multiselectableState only for lists and list boxes (formatting.py:217, 323-327, 943-946), not tables, so no speech is lost on Linux. On Windows, by source, CanSelectMultiple is false (accesskit\_windows node.rs:1417) and the selection event type follows it (adapter.rs:189-199), but row/cell SelectionItem is unsupported there anyway (see tables-03).
- **Fix idea:** Copy the TreeTableView block into TableView::accessibility, gated on MultiRow/MultiCell.

### tables-12 {#tables-12}

Tab is trapped in the grid: Shift+Tab from the table goes into the first cell, Tab walks all 7000 cells, only Ctrl+Tab leaves, and nothing says so

- **Example:** data-grid
- **Scenario:** tables-grid-tab-trap
- **Act:** data-grid, tables-grid-tab-trap: Tab into the table, Shift+Tab, Tab, Ctrl+Tab, Shift+Tab, Ctrl+Shift+Tab; tabwalk of data-grid and tree-table-view
- **The reader should get:** Shift+Tab from the grid returns to '+ Append row'. Tab leaves the grid, or if Tab walks cells the way out is announced (WCAG 2.1.2: a non-standard exit must be advised). The first Tab into the cells lands on the first cell.
- **The reader gets:** Shift+Tab from the table (no cursor yet) moves into cell (1, ID) and Orca says '1.': the reader cannot go back with Shift+Tab. Plain Tab and Shift+Tab move cell by cell (TabTraversal::CellsThenRows is the default) and stay at the ends, so 1000×7 stops. Only Ctrl+Tab / Ctrl+Shift+Tab leave, and nothing on the table (no name, no description) tells a reader. The first Tab from a cursor-less table skips the first cell: data-grid lands on 'Avery 0' (Name column) and tree-table-view on '768 B' (Size column), because Tab steps from the default (0,0) instead of landing on it, which the arrow keys were fixed to do.
- **Platform:** Linux (measured); keyboard behaviour, so the same on every platform
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/keyboard.rs:141-151, 438-462; table\_view/column.rs:186-197
- **Evidence:**
  - `3 of 3 runs: 'Shift+Tab from the table' check 'focus lands on [push button] '+ Append row'' FAIL: '+135.8 ms object:state-changed:focused 1 [table cell] ''', r3 '14:10:02.767056 - SPEECH OUTPUT: '1.''`
  - `3 of 3 runs: Ctrl+Tab '14:10:10.433570 - SPEECH OUTPUT: 'Theme combo box.''; Ctrl+Shift+Tab '14:10:18.287582 - SPEECH OUTPUT: '+ Append row push button.''`
  - `tabwalk data-grid: 'Tab 5' focus [table] with nothing spoken, then 'Tab 6' '+111.8 ms object:state-changed:focused 1 [table cell] ''' / 'ORCA SAYS: 'Avery 0.''; tabwalk tree-table-view: 'Tab 3' 'ORCA SAYS: '768 B.''`
  - ``crates/teksilo-widgets/src/table_view/keyboard.rs:151 `let (row, col) = cursor.unwrap_or((0, 0));` then :439-462 (Tab steps col+1; at the edges `CellsThenRows` returns `Some((row, col))` and stays); column.rs:188-197 default CellsThenRows``
  - `v1 verify-tables-entry 'plain Tab from the cursor-less table': ORCA '14:28:30.977200: 'Avery 0.''; 'Shift+Tab again at the first cell': '+113.1 ms object:state-changed:focused 1 [table cell]', 216 defunct, ORCA '14:28:38.756945: '1.''`
- **Reproduced:** 3 of 3 runs (deterministic); the first-Tab skip in 1 tabwalk per package, deterministic by the code
- **Verification:** confirmed. Reproduced: Shift+Tab into the first cell 3 of 3 (a1/a2/a3 tab-trap); first Tab skips the first cell 3 of 3 (v1/v2/v3 verify-tables-entry: 'Avery 0.'); Shift+Tab at the first cell stays and re-reads 3 of 3
- **Fix idea:** Default to TabTraversal::OutOfTable (one Tab stop, the ARIA grid pattern), or at least let Shift+Tab at the first cell / with no cursor leave backwards. Treat a cursor-less Tab like the arrows (land on the first cell), and state the Ctrl+Tab exit in the grid's description when CellsThenRows is chosen.

### tables-13 {#tables-13}

Every cursor or selection move replaces the whole visible table on the bus (216-405 nodes a keystroke)

- **Example:** data-grid
- **Scenario:** tables-grid-pace
- **Act:** data-grid: any ArrowDown/Right/Tab in MultiRow mode (selection follows the cursor); tree-table-view: any cursor move, expand or collapse
- **The reader should get:** A cursor move changes the active descendant (and, at most, one or two selected states); nodes survive
- **The reader gets:** data-grid: first move 405 nodes defunct (every row, cell and label plus all 7 column headers and the 3 Filter buttons), each later selection-changing move 216 (the whole body pane); only Ctrl+Arrow (cursor without selection) replaces nothing. tree-table-view: every cursor move replaces the body (28-58 nodes) even without a selection change, because the body pane rebuilds on focused\_cell too; expand/collapse also replaces the header row. At a reading pace Orca coped (it drops only the departing cell's focus-lost event as defunct, harmless). At key-repeat pace it dropped the intermediate rows as defunct rather than as obsolete, and still read the row the cursor stopped on. This is the mechanism behind tables-03 and tables-04. A reader parked on a header (flat review) loses it on the first move.
- **Platform:** Linux AT-SPI (measured)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/widget\_impl.rs:120-134 (filter, sort), 136-202 (every model change), 205-224 (selection); table\_view/body\_pane.rs:256-272; tree\_table\_view/body\_pane.rs:246-268 (focused\_cell too)
- **Evidence:**
  - `3 of 3 runs: "405 nodes went defunct in the act: {'label': 193, 'table cell': 175, 'table row': 26, 'column header': 7, 'panel': 1, 'push button': 3}" (first ArrowDown), "216 nodes went defunct in the act: {'<Error>': 213, 'table cell': 1, 'panel': 1, 'table row': 1}" (each later move); '222 events on the bus in the act'`
  - `tables-grid-pace 3 of 3: 'Ctrl+ArrowDown: move the cursor without selecting' 'at most 10 nodes are replaced' pass; 'six ArrowDowns at key-repeat pace' said only '11.' with six 'EVENT MANAGER: Ignoring defunct object' lines, e.g. v2 '13:57:49.596626 EVENT MANAGER: Ignoring defunct object: [DEAD]'; 'Orca does not drop the arriving focus as defunct' pass for the stop row`
  - `tree-table-view r3 per act: ArrowDown 28, Down×2 56, ArrowRight on a leaf (column move only) 51, collapse 58 nodes defunct`
  - `crates/teksilo-widgets/src/table_view/widget_impl.rs:205-215, table_view/body_pane.rs:256-264, tree_table_view/body_pane.rs:246-268 (selection and focused_cell bump the pane's Rebuild version)`
  - `a1 cells: 'ArrowRight to the Name column' children-changed ['add [table]->[table row]','add [table]->[panel]','remove [table]->[table row]','remove [table]->[panel]']`
  - `a1 pace orca-debug.out '14:26:44.363495 - FOCUS MANAGER: Changing locus of focus from [table cell] to [DEAD]. Notify: True' then '14:26:45.092251 - SPEECH OUTPUT: '11.''`
- **Reproduced:** 3 of 3 runs
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 (a1/a2/a3 tables-grid-pace, tables-grid-cells); TTV Ctrl+Down cursor-only replaces 28 nodes 3 of 3 (v1/v2/v3) Mechanism real, description corrected: (a) every rebuild replaces the whole TableView subtree including the header row, not just the body pane (children-changed remove/add \[table\]-&gt;\[table row\] and \[table\]-&gt;\[panel\] in every act; the '&lt;Error&gt;' role counts hid the headers). 405 vs 216 is realised-row count. (b) No-op moves rebuild too: ArrowRight inside the selected row, Shift+Tab at the first cell. (c) At key-repeat pace Orca did not simply drop intermediates as defunct: it processed some arriving focus events and moved its locus of focus to a dead cell ('FOCUS MANAGER: Changing locus of focus from \[table cell\] to \[DEAD\]', 2 of 3 runs), spoke nothing for them, and still read the stop row ('11.', 3 of 3). Low stands for the mechanism alone; its reader-visible consequences are tables-03, tables-04 and the live-update finding below. Aside: the sweep's 'passed' note that only TreeTableView calls ctx.announce is inaccurate: TableView also announces keyboard row moves (table\_view/widget\_impl.rs:514) when reorderable; neither example is, so K2 still bears on nothing here.
- **Fix idea:** Stop baking selection/focus flags into freshly built nodes: keep the row and cell widgets and update is\_selected / is\_focused through signals bound at RepaintOnly/AccessibilityOnly; rebuild only when the realised window moves.

### tables-14 {#tables-14}

The in-cell editor is an unnamed entry: the reader is not told which field they are editing

- **Example:** data-grid
- **Scenario:** tables-grid-edit
- **Act:** data-grid, tables-grid-edit: ArrowRight to the Name cell, F2
- **The reader should get:** 'Name, edit, Avery 0' (the editor labelled by its column)
- **The reader gets:** 'entry Avery 0 selected.' The \[entry\] has no name and no labelled-by relation. The example's placeholder 'Name' is not exposed while the field holds text, and the column context is already lost on Linux (tables-02). Escape brings focus back to the cell, which is read ('Avery 0.').
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/data\_grid/src/main.rs:119-121 (TextInput::new(buffer).placeholder(lit!("Name")), no access\_label); framework improvement point crates/teksilo-widgets/src/table\_view/body\_pane.rs (editor mount)
- **Evidence:**
  - `3 of 3 runs: r3 '14:12:06.128800 - SPEECH OUTPUT: 'entry Avery 0 selected.''; r2/r3 'the tree holds [entry] 'Name'' FAIL`
  - `note while editing: "focused [entry] name='' states=['editable', 'enabled', 'focusable', 'focused', 'selectable-text', 'sensitive', 'showing', 'single-line', 'visible'] attributes=None interfaces=['Accessible', 'Component', 'EditableText', 'Text']"`
  - ``examples/data_grid/src/main.rs name_column: `TextInput::new(buffer).placeholder(lit!("Name"))` (placeholder only)``
  - `a3 ORCA '14:41:53.850812: 'entry Avery 0 selected.''; tree entry {'name': '', 'attributes': None, 'relations': None, 'text': 'Avery 0'}`
- **Reproduced:** 3 of 3 runs
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 (a1/a2/a3 tables-grid-edit); deterministic Real: 'entry Avery 0 selected.', \[entry\] name='' with no relations. The placeholder is not exposed because accesskit\_consumer publishes a placeholder only for an EMPTY text input (node.rs:834-838), as the sweep said. Layer corrected to example: the delegate builds the editor and could label it (.access\_label(lit!("Name"))). The framework could still label every mounted editor from its column, which should be done since the column name is otherwise unavailable on Linux.
- **Fix idea:** When the table mounts an editor for a cell, give it the column label as its name (labelled\_by the header node or set\_name), so every delegate's editor is labelled without app code.

### tables-15 {#tables-15}

The grid's Selection interface always reports 0 selected children

- **Example:** data-grid
- **Scenario:** tables-grid-selection
- **Act:** data-grid, tables-grid-selection: after ArrowDown, Space, Shift+Down and Ctrl+A (1000 rows selected)
- **The reader should get:** AT-SPI Selection (UIA ISelectionProvider.GetSelection) on the grid returns the selected rows
- **The reader gets:** selected\_children = 0 after every act, including after Ctrl+A. accesskit\_consumer builds a container's selection from items(), which counts only 'item-like' direct filtered children (ListItem, TreeItem, ListBoxOption, …), never Row or Cell, and the rows sit under the body pane anyway. Teksilo's own comment (widget\_impl.rs:1471-1483) says Role::Grid gives NVDA/JAWS GetSelection; the pattern is there, but it is always empty.
- **Platform:** Linux AT-SPI (measured); Windows by source (accesskit\_windows node.rs:1420-1427 GetSelection uses the same items())
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/table\_view/widget\_impl.rs:1470-1490 (comment); upstream accesskit\_consumer node.rs:920-936, 1064-1073
- **Evidence:**
  - `3 of 3 runs, the [table] node after each selection act: 'launch selected_children= 0', 'ArrowDown selects row 1 selected_children= 0', ... 'Ctrl+A selects all selected_children= 0'`
  - `accesskit_consumer-0.39.0/src/node.rs:920-936 is_item_like (no Row/Cell/GridCell), node.rs:1064-1073 items(); accesskit_atspi_common-0.20.0/src/node.rs:1268-1276 n_selected_children`
  - `a1 selection tree: 'Ctrl+A selects all selected_children= 0'`
  - `v1 verify-tables-ttv: "[tree table] ... selected_children=0" while row 'README' has 'selected'`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** corrected by the verifier. Reproduced: data-grid 3 of 3 (a1/a2/a3 selection: selected\_children=0 after every act incl. Ctrl+A); tree-table-view 3 of 3 (v1/v2/v3 verify-tables-ttv: selected\_children=0 with a row selected) Real, and wider than reported: it also holds for TreeTableView's Role::TreeGrid, and on macOS accessibilityRows/SelectedRows use the same items() (accesskit\_macos node.rs:1106-1135), so VoiceOver gets zero rows and zero selected rows. Wording fix: items() is not limited to direct children (non-item-like included nodes are ExcludeNode, so it descends), but none of Row/Cell/GridCell is item-like, so it finds nothing. The comment in widget\_impl.rs:1471-1483 promising GetSelection to NVDA/JAWS is wrong in effect.
- **Fix idea:** Upstream: count Row/GridCell (and descendants through generic containers) as items of a Grid/TreeGrid. Teksilo should correct the comment in widget\_impl.rs so nobody relies on GetSelection working.

### tables-16 {#tables-16}

data-grid example: 'Reset sort' and 'Reset filters' do nothing, the status line never updates, the Active column reads glyphs

- **Example:** data-grid
- **Scenario:** tables-grid-headers
- **Act:** data-grid, tables-grid-headers: activate 'Reset sort'; tables-grid-selection: Ctrl+A then read the status line; tables-grid-cells: ArrowRight onto Active
- **The reader should get:** 'Reset sort' restores the ID sort and 'Reset filters' clears the filters; the status line follows the row and selection counts; Active reads 'Yes'/'No'
- **The reader gets:** Activating 'Reset sort' produces no event at all. Both buttons are built with no handler. The status label is formatted once at build time and stays '1000 rows  ·  selection: 0' after Ctrl+A selects everything. The Active cells read '● Yes.' / '○ No.', so a speech synthesizer says the glyph names.
- **Platform:** all (example code); measured on Linux
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/data\_grid/src/main.rs:162, 258-262, 271-272
- **Evidence:**
  - `3 of 3 runs, 'activate 'Reset sort'': only '+4.7 ms == harness:action click [push button] 'Reset sort'' and nothing else on the bus`
  - `3 of 3 runs: 'the tree holds [label] 'selection: 1000'' FAIL; tree label '1000 rows  ·  selection: 0'`
  - `r3: 'ORCA SAYS: '● Yes.''`
  - ``examples/data_grid/src/main.rs:271-272 `Button::new(lit!("Reset filters"))`, `Button::new(lit!("Reset sort"))` with no on_activate; main.rs:258-262 `TextWidget::new(lit!(format!("{} rows  ·  selection: {}", proxy.len(), selection.count())))`; active_column uses "● Yes" / "○ No"``
  - `a3 'check the status line' FAIL 'the tree holds [label] 'selection: 1000''`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 (a1/a2/a3 headers and selection); deterministic
- **Fix idea:** Wire the two buttons (clear table.filters\_signal(); set\_sort(Some("id"), Ascending)), bind the status text to signals (tr\_signal!/Signal::map over the proxy length and selection), and give the Active cell a text label (or an a11y label) of 'Yes'/'No'.

### tables-v1 {#tables-v1}

A live model change the reader did not make re-announces the focused cell and stops current speech

- **Example:** data-grid
- **Scenario:** verify-tables-live
- **Act:** data-grid, verify-tables-live: cursor on row 1 (ID), then an AT-SPI click on '+ Append row' (appends ID 1001, off screen), twice
- **The reader should get:** The row is added out of view; the cell the reader is on stays the same node, no focus event, nothing spoken (or, at most, a polite announcement of the change)
- **The reader gets:** Every append rebuilds the whole TableView: 216 nodes go defunct, a new node for the same cell takes focus, and Orca stops whatever it was saying and says '1.' again. In a table fed by live data (logs, monitoring, a feed) the reader hears the current cell repeated, and interrupted, on every update.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows/macOS by reasoning only: a new focused node is a focus change on every adapter.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/widget\_impl.rs:136-202
- **Evidence:**
  - `v1 'a live append ...': '14:30:28.568527 object:state-changed:focused 1 [table cell]', 216 defunct; orca-debug.out '14:30:28.676130 - NULL SPEECH: stop' then '14:30:28.676251 - SPEECH OUTPUT: '1.''`
  - `v2: '14:34:37.944202 NULL SPEECH: stop' / ''1.''; v3: '14:36:41.692516 NULL SPEECH: stop' / ''1.'' (and the same for each second append)`
  - `crates/teksilo-widgets/src/table_view/widget_impl.rs:136-202: any DataChange from the source bumps the root's Rebuild-bound version (v_for_data.set(next) at :200)`
- **Reproduced:** 3 of 3 runs, 6 of 6 appends
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Do not rebuild realised rows on a change outside them (or keep cell nodes and update in place); at least keep the focused cell's node id stable across a rebuild so no focus event is emitted.

### tables-v2 {#tables-v2}

After a filter is applied or cleared the cursor keeps its index, so the reader lands on a different row without notice

- **Example:** data-grid
- **Scenario:** verify-tables-filter-cursor
- **Act:** data-grid, verify-tables-filter-cursor: cursor on 'Avery 0', filter Name='Blake' (Enter), ArrowDown to 'Blake 15', open the filter again and Clear it (AT-SPI)
- **The reader should get:** The reader stays on the row they were on ('Blake 15') or is told where they are and why
- **The reader gets:** After Enter the cursor is on 'Blake 1' (index 0 of the filtered rows, where 'Avery 0' was); after Clear the reader, who was on 'Blake 15', hears 'Blake 1.' (index 1 of the unfiltered rows). Nothing says the row changed.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured); keyboard/cursor behaviour, so the same elsewhere
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/widget\_impl.rs:120-124, 186-196
- **Evidence:**
  - `v1: '14:29:43.549707 SPEECH OUTPUT: 'Blake 15.'' then after Clear '14:29:49.014669 SPEECH OUTPUT: 'Blake 1.''`
  - `v2: 'Blake 15.' then '14:33:58.268949: 'Blake 1.''; v3: 'Blake 15.' then '14:38:40.076424: 'Blake 1.''`
  - `focused_cell is a plain (row, col) index (table_view/keyboard.rs:141-151); the example uses an index SelectionModel (examples/data_grid/src/main.rs:207)`
- **Reproduced:** 3 of 3 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Anchor the cursor to the row's identity (the RowAnchor the data views already have) across a filter/sort change, or announce where the cursor landed.

### tables-v3 {#tables-v3}

An empty cell (a folder's Size) is silent: the reader cannot tell the cursor moved

- **Example:** tree-table-view
- **Scenario:** verify-tables-ttv-empty
- **Act:** tree-table-view, verify-tables-ttv-empty: cursor on 'docs', Tab to its Size cell
- **The reader should get:** 'blank' (or the column and an empty value)
- **The reader gets:** Nothing. Orca stops the previous speech and speaks an empty utterance; the cell holds an empty \[label\] ''. Tab on to Kind then says 'folder.'.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/tree\_table\_view/src/main.rs:162-167
- **Evidence:**
  - `v3 orca-debug.out '14:37:34.820542 - AXObject: find_descendant: found None' / '14:37:34.828325 - NULL SPEECH: stop' / '14:37:34.828356 - SPEECH: Speak [[], PAUSE, PAUSE, PAUSE, PAUSE]'`
  - `v4 '14:39:58.009232 - SPEECH: Speak [[], PAUSE, PAUSE, PAUSE, PAUSE]'; check 'Orca has something to say for the new focus' FAIL 2 of 2`
  - `tree: "[table cell] '' {focused,selectable,selected} > [label] ''"; examples/tree_table_view/src/main.rs:162-167 renders String::new() for size 0`
- **Reproduced:** 2 of 2 runs (deterministic by the delegate)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Example: render a visible and speakable value ('—' or '0 B', or an access\_label 'no size'). Framework: give an empty cell a name so Orca's layout-table path has something to say.
