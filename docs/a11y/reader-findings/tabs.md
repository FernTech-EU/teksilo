<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Tabs

Examples: `tab-widget`, `tab-migration`.
18 findings: 1 critical, 5 high, 7 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [tabs-01](#tabs-01) | tab-widget | Coming back to a tab makes every control in its panel silent to Orca (the panel's nodes return under ids already marked defunct) | critical | Linux | fixed |
| [tabs-02](#tabs-02) | tab-widget | A tab move that does not happen is announced as done ('Doc 1 moved to 1 of 6', 'Settings moved to 3 of 6') | high | Linux | open |
| [tabs-03](#tabs-03) | tab-widget | Every keyboard tab move is announced in the same update as a focus move to a rebuilt tab, so Orca cuts the announcement | high | Linux | fixed |
| [tabs-04](#tabs-04) | tab-widget | Closing a tab has no route a screen reader can find: the 'Close' AT action reaches no adapter, the close button is hover-only, and the context menu has no Close | medium | all | open |
| [tabs-05](#tabs-05) | tab-widget | Closing a tab sends selection and focus to the first tab, not to the closed tab's neighbour | medium | all | open |
| [tabs-06](#tabs-06) | tab-widget | Every rebuild of the tab bar while focus is elsewhere makes Orca say 'Welcome page tab.' and move its locus of focus there | medium | Linux | open |
| [tabs-07](#tabs-07) | tab-widget | The 'Show all tabs' overflow dropdown cannot be operated from the keyboard | high | Linux | open |
| [tabs-08](#tabs-08) | tab-widget | The tab's context menu is silent while the reader moves through it (MenuList exposes no current item) | high | all | fixed |
| [tabs-09](#tabs-09) | tab-migration | tab-migration: a tab can be moved to the other group only by dragging; there is no keyboard or AT route | high | all | open |
| [tabs-10](#tabs-10) | tab-migration | A tab list cannot be named, so tab-migration's two groups sound identical | medium | all | open |
| [tabs-11](#tabs-11) | tab-widget | A disabled tab is exported as enabled and sensitive on AT-SPI, so Orca never says it is unavailable | medium | Linux | upstream |
| [tabs-12](#tabs-12) | tab-widget | The tab list's AT structure: non-tab children, the unpinned tabs nested in an unnamed panel, and setsize on every descendant | low | Linux, Windows | open |
| [tabs-13](#tabs-13) | tab-widget | Tabs scrolled out of an overflowing strip leave the AT tree, and come back under defunct ids | low | Linux | fixed |
| [tabs-14](#tabs-14) | tab-widget | A pinned tab's declared tooltip is replaced by its title, for AT and for hover | low | all | open |
| [tabs-15](#tabs-15) | tab-widget | No Ctrl+Tab / Ctrl+PageDown to switch tabs from inside a panel | low | all | open |
| [tabs-v1](#tabs-v1) | tab-widget | The focused 'Scroll tabs right' hides itself at the end of the strip, and focus falls to the window | medium | Linux | open |
| [tabs-v2](#tabs-v2) | tab-widget | Picking a tab from 'Show all tabs' leaves keyboard focus on the trigger while Orca announces the chosen tab | medium | Linux | open |
| [tabs-v3](#tabs-v3) | tab-widget | Enter or Space on a tab whose panel has nothing focusable does nothing and says nothing, and that panel cannot be reached with Tab | low | all | open (example) |

### tabs-01 {#tabs-01}

Coming back to a tab makes every control in its panel silent to Orca (the panel's nodes return under ids already marked defunct)

- **Example:** tab-widget
- **Scenario:** tabs-panel-revisit, tabs-doc-revisit
- **Act:** tabs-panel-revisit: Right to Settings, Enter into its panel (Toggle orientation), back to the Settings tab, Right to Doc 1, Left back to Settings, Enter into the panel again. tabs-doc-revisit: the same for Doc 1 and its 'Make an edit' button.
- **The reader should get:** On the second visit, the same as the first: focus lands on the button and Orca says 'Toggle orientation push button.' ('Make an edit push button.').
- **The reader gets:** Focus lands on the button on the bus, but Orca says nothing: it logs 'Ignoring defunct object' for the focus event. Space on the button then works (the strip turns vertical) and is silent too. Leaving a tab removes its whole panel subtree from the AT tree (every node gets object:state-changed:defunct 1). Returning brings back the same NodeIds, and libatspi, which already cached them, keeps them defunct. Any control the reader visited in a panel becomes unreadable after one switch away and back, for static and dynamic tabs alike.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The defunct state is an AT-SPI mechanism (accesskit\_atspi\_common remove\_node plus libatspi's cache). The UIA and macOS adapters have no equivalent, so Windows and macOS are probably unaffected; not measured.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: tabs-panel-revisit returned panel button heard 3/3; tabs-doc-revisit heard 1/1.
- **Where:** crates/teksilo-core/src/accessibility.rs:1984-1989; crates/teksilo-widgets/src/primitives/switcher.rs:233-241; crates/teksilo-widgets/src/tab\_widget.rs:1498-1503
- **Evidence:**
  - `tabs-panel-revisit-20260925-135207 events.jsonl seq 29: 13:52:18.775710 object:state-changed:focused 1 [push button] 'Toggle orientation' path /org/a11y/atspi/accessible/0/79228165779338038640134586368 (first visit; Orca: 'Toggle orientation push button.')`
  - `act 'Right to Doc 1': +44.9 ms object:state-changed:defunct 1 [push button] 'Toggle orientation' (events.jsonl seq 57, 13:52:25.893126, same path)`
  - `act 'Enter into the returned Settings panel': +8.5 ms object:state-changed:focused 1 [push button] 'Toggle orientation' (same path); no utterance in the act`
  - `orca-debug.out: 13:52:33.732371 - EVENT MANAGER: Ignoring defunct object: [push button: 'Toggle orientation']`
  - `act 'Space on Toggle orientation in the returned panel': the tab list is rebuilt vertical; 'FAIL Orca says something / Orca said nothing in this act'`
  - `tabs-doc-revisit-20260925-135211: earlier 13:52:28.990073 object:state-changed:defunct 1 [push button] 'Make an edit' path /org/a11y/atspi/accessible/0/79228166554101289735935754240; after returning, +10.2 ms object:state-changed:focused 1 [push button] 'Make an edit' (same path); orca-debug.out 13:52:36.762485 EVENT MANAGER: Ignoring defunct object: [push button: 'Make an edit']`
  - `crates/teksilo-widgets/src/tab_widget.rs:1499 panes live in a Switcher (memoized WidgetIds); crates/teksilo-widgets/src/primitives/switcher.rs:9-10,233-241 hides a page through visible_when, which makes it dormant and excludes it from the AT tree`
  - `crates/teksilo-core/src/accessibility.rs:1984-1989 widget_id_to_node_id is a pure function of the WidgetId, so a re-activated pane gets the same NodeIds`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:91-108 remove_node emits StateChanged(Defunct, true) for a node that leaves the filtered tree; orca/event_manager.py:797-798 drops events whose source is defunct`
  - `verify-tabs-revisit-tab-20260925-143341-628028 act 'Tab x5 into the returned panel': +1556.7 ms object:state-changed:focused 1 [push button] 'Toggle orientation', path /org/a11y/atspi/accessible/0/79228165779338038640134586368, earlier: 14:33:59.587979 object:state-changed:defunct 1 [push button] 'Toggle orientation'; orca-debug.out 14:34:08.898318 EVENT MANAGER: Ignoring defunct object: [push button: 'Toggle orientation']; last utterance of the act: '+ New tab push button.'`
  - `same run, act 'AT-SPI grab_focus on the returned Toggle orientation': +22.6 ms object:state-changed:focused 1 [push button] 'Toggle orientation'; 14:34:15.116238 EVENT MANAGER: Ignoring defunct object: [push button: 'Toggle orientation']; Orca said nothing`
  - `tabs-panel-revisit-20260925-141923-287860 act 'Enter into the returned Settings panel': +6.9 ms object:state-changed:focused 1 [push button] 'Toggle orientation' (earlier 14:19:41.895013 defunct 1, same path); 14:19:49.759789 EVENT MANAGER: Ignoring defunct object: [push button: 'Toggle orientation']`
  - `act 'Left back to Settings': +9.3 ms object:children-changed:add [panel] '' -> [scroll pane] 'Settings' (the panel itself returns under its old id too)`
- **Reproduced:** 3 of 3 runs (tabs-panel-revisit) and 3 of 3 runs (tabs-doc-revisit): Orca silent with 'Ignoring defunct object' every time
- **Verification:** confirmed. Reproduced: panel-revisit 3 of 3, doc-revisit 3 of 3, verify-tabs-revisit-tab 3 of 3 (Tab route 3/3 and AT grab\_focus 3/3 silent)
- **Fix idea:** This is the same mechanism as K2, but for widget nodes, so the K2 fix (fresh ids only for announcer nodes, commit b9586ea2b) does not cover it. Give a subtree re-entering the AT tree fresh NodeIds, for example a generation counter bumped when a dormant subtree is activated and folded into widget\_id\_to\_node\_id. The alternative is upstream: accesskit\_atspi\_common should emit only children-changed:remove, not Defunct, for a node that merely leaves the filtered tree. This affects every Switcher / visible\_when user, not only TabWidget.

### tabs-02 {#tabs-02}

A tab move that does not happen is announced as done ('Doc 1 moved to 1 of 6', 'Settings moved to 3 of 6')

- **Example:** tab-widget
- **Scenario:** tabs-reorder-rejected, tabs-reorder-static
- **Act:** tabs-reorder-rejected: Alt+Home on the dynamic tab Doc 1. tabs-reorder-static: Alt+Right on the static tab Settings. The context menu on Settings offers Move Left / Move Right / Move to Start / Move to End.
- **The reader should get:** A dynamic tab cannot pass the static tabs and a static tab cannot move, so either the move happens and is announced, or nothing is announced (or 'cannot move'); a static tab should not be offered moves at all.
- **The reader gets:** The tab order is unchanged, yet an announcement claims the move, and Orca speaks it. Every move offered on Settings (keys, context menu, custom actions) does nothing, and each keyboard one is announced as done.
- **Platform:** Linux AT-SPI / Orca measured. By source, all platforms: the announced text is platform-independent (Windows raises LiveRegionChanged and the client reads the node's name; macOS posts the text).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/header.rs:809-832; crates/teksilo-widgets/src/tab\_widget.rs:1074-1096
- **Evidence:**
  - `tabs-reorder-rejected-20260925-135215 act 'Alt+Home on Doc 1': +22.4 ms object:announcement [status bar] 'Doc 1 moved to 1 of 6' text='Doc 1 moved to 1 of 6'; tree after: page tabs in tree order: ['Welcome', 'Settings', 'Locked', 'Doc 1', 'Doc 2', 'Doc 3']`
  - `orca-debug.out: 13:52:27.025341 - SPEECH OUTPUT: 'Doc 1 moved to 1 of 6'`
  - `act 'Alt+Left on Doc 1': +20.6 ms object:announcement [status bar] 'Doc 1 moved to 1 of 6' text='Doc 1 moved to 3 of 6' (order unchanged; this second message is dropped by K2)`
  - `tabs-reorder-static-20260925-135417 act 'Alt+Right on Settings': +25.0 ms object:announcement [status bar] 'Settings moved to 3 of 6' text='Settings moved to 3 of 6'; order unchanged; orca-debug.out 13:54:27.844715 - SPEECH OUTPUT: 'Settings moved to 3 of 6'`
  - `context menu on Settings: [menu item] 'Move Left', 'Move Right', 'Move to Start', 'Move to End'`
  - `crates/teksilo-widgets/src/tab_widget/header.rs:809-832: reorder_perform computes dest from the unified header index over all tabs, calls reorder(dest, ctx) (824) and then ctx.announce(move_announcement(...)) (825) unconditionally`
  - `crates/teksilo-widgets/src/tab_widget.rs:1079-1095: the default on_reorder moves only when from >= static_count && to >= static_count, and otherwise silently rejects (warn_cross_boundary_reorder_once)`
  - `crates/teksilo-widgets/src/tab_widget/header.rs:1249-1270 (custom actions) and 1086-1101 (context menu) offer the moves on any enabled unpinned tab, including static ones`
  - `tabs-reorder-rejected-20260925-143118-577545 act 'Alt+Home on Doc 1': +27.8 ms object:announcement [status bar] 'Doc 1 moved to 1 of 6'; orca-debug.out 14:31:29.850357 - SPEECH OUTPUT: 'Doc 1 moved to 1 of 6'; page tabs in tree order unchanged`
  - `tabs-reorder-static-20260925-143122-582003 act 'Alt+Right on Settings': +23.7 ms object:announcement [status bar] 'Settings moved to 3 of 6'; 14:31:33.263495 - SPEECH OUTPUT: 'Settings moved to 3 of 6'; menu items on Settings: 'Move Left', 'Move Right', 'Move to Start', 'Move to End'`
- **Reproduced:** 4 of 4 runs spoke 'Doc 1 moved to 1 of 6' (tabs-reorder-rejected); 3 of 3 runs spoke 'Settings moved to 3 of 6' (tabs-reorder-static); the bus announcement is in every run
- **Verification:** confirmed. Reproduced: Alt+Home 3 of 3 runs spoken; static Alt+Right 3 of 3 spoken; menu offers the moves 3 of 3
- **Fix idea:** Have on\_reorder\_to report whether the move was applied, and announce only then. Better, compute the offered moves from the region the tab may actually move in (the dynamic range), and offer none on a static tab, so the chord, the menu and the custom actions agree with the handler. Goes through ctx.announce: the K2 fix makes more of these wrong messages audible, it does not correct them.

### tabs-03 {#tabs-03}

Every keyboard tab move is announced in the same update as a focus move to a rebuilt tab, so Orca cuts the announcement

- **Example:** tab-widget
- **Scenario:** tabs-reorder, tabs-context-menu, tabs-migration-move
- **Act:** tabs-reorder: Alt+Right on Doc 1. tabs-context-menu: Enter on 'Move Left' in the tab's context menu. tabs-migration-move: Alt+End on Alpha.
- **The reader should get:** The reader hears 'Doc 1 moved to 5 of 6' (focus stays on the tab they moved).
- **The reader gets:** A move rebuilds the whole tab bar: a new page tab list and new tab nodes, with focus re-requested on the new header. The announcement reaches the bus 0.4-3 ms before that focus change, and Orca stops speech to present the focus. The reader hears 'Doc 1 page tab.' The first message of a session is cut; later ones are also dropped (K2).
- **Platform:** Linux AT-SPI / Orca measured. Windows and macOS not measured; by source the ordering (node changes before focus) is the consumer's, and whether NVDA/VoiceOver cut depends on the reader.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `b518253f` (announce-focus). Fixed part: every keyboard tab move announcement cut by the focus move to the rebuilt tab.
- **Where:** crates/teksilo-widgets/src/tab\_widget/header.rs:821-835
- **Evidence:**
  - `tabs-reorder-20260925-135412 act 'Alt+Right moves Doc 1 after Doc 2': +36.1 ms object:announcement [status bar] 'Doc 1 moved to 5 of 6' text='Doc 1 moved to 5 of 6'; +36.7 ms object:children-changed:add [panel] '' -> [page tab list] ''; +37.0 ms object:state-changed:focused 1 [page tab] 'Doc 1'; +37.9 ms object:state-changed:defunct 1 [page tab list] ''`
  - `orca-debug.out: 13:54:24.305357 - SPEECH OUTPUT: 'Doc 1 moved to 5 of 6' / 13:54:24.362129 - NULL SPEECH: stop / 13:54:24.362191 - SPEECH OUTPUT: 'Doc 1 page tab.'`
  - `report: observed  Orca's 'Doc 1 moved to 5 of 6' was cut by a stop 57 ms in (estimated)`
  - `tabs-context-menu-20260925-135956 act 'Enter on the focused item': +19.8 ms object:announcement [status bar] 'Doc 2 moved to 4 of 6'; +21.3 ms object:state-changed:focused 1 [page tab] 'Doc 2'; Orca's 'Doc 2 moved to 4 of 6' was cut by a stop 73 ms in (estimated)`
  - `tabs-migration-move-20260925-140817 act 'Alt+End on Alpha': Orca said (cut) 'Alpha moved to 3 of 3' then 'Alpha page tab.'; the other run (140023) dropped it: 14:00:41.243930 EVENT MANAGER: Ignoring defunct object: [status bar: 'Alpha moved to 3 of 3'] (K2)`
  - `crates/teksilo-widgets/src/tab_widget/header.rs:824-830 announces right after reorder(dest); the comment at 832-835 says 'The bar rebuilds around the moved tab, so the header that had focus is gone; the reorder handler is what re-establishes it'`
  - `accesskit_consumer-0.39.0 tree.rs:640-673 hands node changes to adapters before the focus event; Orca default.py ~698-705 stops speech to present a new focus`
  - `tabs-reorder-20260925-143442-655146 act 'Alt+Right moves Doc 1 after Doc 2': +30.1 ms object:announcement [status bar] 'Doc 1 moved to 5 of 6'; +31.2 ms object:state-changed:focused 1 [page tab] 'Doc 1'; orca-debug.out 14:34:54.165928 SPEECH OUTPUT: 'Doc 1 moved to 5 of 6' / 14:34:54.189309 FOCUS MANAGER: Changing locus of focus from [page tab: 'Doc 1'] to [page tab: 'Doc 1'] / 14:34:54.210028 NULL SPEECH: stop / 14:34:54.210117 SPEECH OUTPUT: 'Doc 1 page tab.'`
  - `tabs-context-menu-20260925-142011-313911 act 'Enter on the focused item': +19.2 ms object:announcement [status bar] 'Doc 2 moved to 4 of 6'; +19.5 ms focused 1 [page tab] 'Doc 2'; +21.8 ms object:state-changed:defunct 1 [status bar] 'Doc 2 moved to 4 of 6'; 14:20:38.924231 EVENT MANAGER: Ignoring defunct object: [status bar: 'Doc 2 moved to 4 of 6'] (K2 drop)`
  - `verify-tabs-vertical-20260925-142834-499385 act 'Alt+Up on Doc 3': +31.4 ms announcement 'Doc 3 moved to 5 of 6'; +32.0 ms focused 1 [page tab] 'Doc 3'; ORCA SAYS (CUT) 'Doc 3 moved to 5 of 6' then 'Doc 3 page tab.'`
- **Reproduced:** 3 of 3 runs cut (tabs-reorder, Alt+Right); 1 of 1 cut (context-menu move); tab-migration Alt+End lost in 2 of 2 runs (1 cut, 1 dropped by K2)
- **Verification:** confirmed. Reproduced: 13 of 13 runs lost (Alt+Right 4/4 cut; context-menu Move Left 1 cut + 3 dropped by K2; tab-migration Alt+End 1 cut + 3 dropped by K2; vertical Alt+Up 1/1 cut)
- **Fix idea:** The message goes through ctx.announce, so the K2 fix covers the drop of the second and later messages. It does not cover the cut: that fix gives each message a fresh node but still adds it in the same update as the focus move. Either keep the tab header nodes across a reorder (move them in place instead of rebuilding the bar, which also stops the defunct churn), or queue the announcement for the update after focus has landed on the rebuilt header.

### tabs-04 {#tabs-04}

Closing a tab has no route a screen reader can find: the 'Close' AT action reaches no adapter, the close button is hover-only, and the context menu has no Close

- **Example:** tab-widget
- **Scenario:** tabs-launch-tree, tabs-context-menu, tabs-close-keys
- **Act:** tabs-launch-tree (Doc 1's actions on AT-SPI); tabs-context-menu (items of the tab's menu); tabs-close-keys (Delete).
- **The reader should get:** A closable tab offers its close through something a reader can discover: an AT action, a menu item, a focusable button, or at least an exposed key shortcut.
- **The reader gets:** On AT-SPI, Doc 1 exposes only 'click'. The header's AccessKit custom actions ('Close' and the four moves) are published by no AccessKit adapter on any platform. The × button is culled from the tree unless a pointer hovers. The tab's context menu lists only the moves. Delete on the focused tab works, but nothing tells the reader it exists (no key shortcut on the node, no hint).
- **Platform:** All platforms by source: accesskit\_atspi\_common 0.20, accesskit\_windows 0.35 and accesskit\_macos 0.27 contain no custom-action support. Linux measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/header.rs:611-627, 1084-1101, 1273-1285
- **Evidence:**
  - `tabs-launch-tree-20260925-135331: [page tab] 'Doc 1' desc='Document #1' states=['enabled', 'focusable', 'selectable', 'sensitive', 'showing', 'visible'] attrs={'posinset': '4', 'setsize': '6'} actions=['click'] rel=None children=0 / close buttons in the tree: none`
  - `tabs-context-menu-20260925-135608: FAIL the menu offers closing the tab / Move Left / Move Right / Move to Start / Move to End`
  - `tabs-close-keys act 'Delete on Doc 1': +25.2 ms object:announcement [alert] 'Close tab?'; +26.7 ms object:state-changed:focused 1 [push button] 'No' (the Delete route works)`
  - `crates/teksilo-widgets/src/tab_widget/header.rs:1273-1285 pushes the 'Close' CustomAction; header.rs:605-631 builds the close IconButton .focusable(false) and hides it with visible_when(hover) unless RevealPolicy::Always`
  - `crates/teksilo-widgets/src/tab_widget/header.rs:1084-1101 default context menu = append_move_items only (common/ordered_move.rs:443-460)`
  - `accesskit_atspi_common-0.20.0/src/node.rs:532-544 n_actions / get_action_name expose only 'click'; grep for CustomAction/custom_action in accesskit_atspi_common-0.20.0, accesskit_windows-0.35.0, accesskit_macos-0.27.0 src finds nothing`
- **Reproduced:** 3 of 3 launch-tree runs (tree), 3 of 3 context-menu runs (menu items); deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 launch-tree runs; 4 of 4 context-menu runs (no Close item)
- **Fix idea:** Put 'Close tab' (with its Delete accelerator) in the header's default context menu whenever on\_close is set, and publish the Delete chord on the node (keyboard shortcut). Do not rely on AccessKit custom actions, which no current adapter publishes; the header.rs comments treat them as a working AT route.

### tabs-05 {#tabs-05}

Closing a tab sends selection and focus to the first tab, not to the closed tab's neighbour

- **Example:** tab-widget
- **Scenario:** tabs-close, tabs-close-keys
- **Act:** tabs-close / tabs-close-keys: Tab to Doc 1 (4th of 6), Delete, answer Yes (AT-SPI click, or Shift+Tab then Space).
- **The reader should get:** As the code documents ('positional fallback = next neighbor of the closed tab; browser convention'), Doc 2 becomes selected and focused, and the reader hears 'Doc 2 page tab.'
- **The reader gets:** Welcome, the first tab, becomes selected and focused, and its panel is shown. The reader hears 'Welcome page tab.', far from where they were working.
- **Platform:** All platforms (selection logic). Linux measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs:559, 1187-1195; crates/teksilo-widgets/src/tab\_widget.rs:1395-1407
- **Evidence:**
  - `tabs-close-keys-20260925-140727 act 'Space on Yes closes Doc 1': object:state-changed:focused 1 [page tab] 'Welcome'; FAIL the one selected page tab is 'Doc 2' / selected page tabs: ['Welcome']`
  - `orca-debug.out: 14:07:46.889951 - SPEECH OUTPUT: 'Welcome page tab.'`
  - `tabs-close-20260925-135339 act 'answer Yes (AT-SPI click)': +44.2 ms object:state-changed:focused 1 [page tab] 'Welcome'; +43.5 ms object:text-changed:insert [label] 'Doc 1' text='Welcome (pinned static)' (the example's 'Active:' label)`
  - ``crates/teksilo-widgets/src/tab_widget/bar.rs:560 from_list_source starts the private index at `selected: Signal::new(0_usize)`; TabWidget constructs a new TabBar on every build (tab_widget.rs:1393-1405)``
  - ``crates/teksilo-widgets/src/tab_widget/bar.rs:1153-1157 (comment: 'stale id falls back to the previously-selected index clamped into range (positional fallback = next neighbor of the closed tab)') and 1187-1195: `let clamped = self.selected.get().min(n - 1)` always reads the fresh 0``
  - `verify-tabs-close-middle-20260925-143523-669771 act 'answer Yes: Doc 2 closes': +41.3 ms object:state-changed:focused 1 [page tab] 'Welcome'; ORCA SAYS 'Welcome page tab.'; selected page tabs: ['Welcome']`
  - `tabs-close-keys-20260925-142053-337127 act 'Space on Yes closes Doc 1': +49.0 ms focused 1 [page tab] 'Welcome'; selected page tabs: ['Welcome']`
- **Reproduced:** 4 of 4 runs (tabs-close x2, tabs-close-keys x2); deterministic
- **Verification:** confirmed. Reproduced: 4 of 4 selected-tab closes (deterministic)
- **Fix idea:** Seed the bar's index from TabWidget's own switcher\_index, which still holds the old position (tab\_widget.rs:1307-1316), or keep the last index across TabWidget rebuilds, so the documented next-neighbour fallback actually happens.

### tabs-06 {#tabs-06}

Every rebuild of the tab bar while focus is elsewhere makes Orca say 'Welcome page tab.' and move its locus of focus there

- **Example:** tab-widget
- **Scenario:** tabs-overflow, tabs-orient, tabs-dropdown
- **Act:** Space on '+ New tab' (tabs-overflow), Space on Orient (tabs-orient), Space on Sizing (tabs-dropdown, 'Space on what Tab reached'). Focus stays on the button in each case.
- **The reader should get:** Pressing '+ New tab' tells the reader a tab opened (or moves them to it); pressing Orient or Sizing says nothing about a tab. Orca's idea of focus stays on the button.
- **The reader gets:** Each of these rebuilds the whole TabBar: a new page tab list with new tab nodes, the old one defunct. The adapter emits object:selection-changed for the new tab list, because it was added with a selected child. Orca answers a selection change on a tab list by moving its locus of focus to the selected tab and speaking it: the reader hears 'Welcome page tab.' while keyboard focus is still on the button, and Orca's where-am-I now reports Welcome. The new tab (Doc 4) is never mentioned. The same event at launch makes Orca start 'Welcome page tab.' before the window (cut by 'frame.').
- **Platform:** Linux AT-SPI / Orca measured (Orca behaviour, Teksilo trigger). Windows/macOS not measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget.rs:1395-1407
- **Evidence:**
  - `tabs-overflow-20260925-140615 act "Space on '+ New tab'": object:children-changed:add [panel] '' -> [page tab list] ''; object:state-changed:defunct 1 [page tab list] ''; object:selection-changed [page tab list] ''; no focus change`
  - `orca-debug.out: 14:06:25.251526 - FOCUS MANAGER: Changing locus of focus from [push button: '+ New tab'] to [page tab: 'Welcome']. Notify: True / 14:06:25.265252 - SPEECH OUTPUT: 'Welcome page tab.'`
  - `tabs-orient-20260925-140632 act 'Space on Orient': +43.8 ms object:selection-changed [page tab list] ''; orca-debug.out 14:06:43.094403 - SPEECH OUTPUT: 'Welcome page tab.' (no focus change: focus is still on Orient)`
  - `tabs-dropdown-20260925-140731 act 'Space on what Tab reached' (Sizing): 14:08:04.209044 SPEECH OUTPUT: 'Welcome page tab.'`
  - `launch (every run): +290.6 ms object:selection-changed [page tab list] '' then ORCA SAYS (CUT): 'Welcome page tab.' before 'frame.'`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:109-111, 249-277 enqueue selection-changed on the container when a selected item-like node is added or removed; orca/scripts/default.py:1579-1602 onSelectionChanged sets the locus of focus to the selected page tab when its name differs from the focus's`
  - `verify-tabs-activate-other-20260925-143429-645455: act "Enter on '+ New tab'" orca-debug.out 14:34:39.389041 FOCUS MANAGER: Changing locus of focus from [push button: '+ New tab'] to [page tab: 'Welcome'] / 14:34:39.410795 SPEECH OUTPUT: 'Welcome page tab.' (no focus change on the bus)`
  - `same run, AT click on '+ New tab' with Orca's locus left on Welcome: 14:34:43.386931 DEFAULT: [page tab: 'Welcome'] 's selection redundant to [page tab: 'Welcome'] (silent)`
  - `same run, AT click on Sizing: 14:35:01.857480 FOCUS MANAGER: Changing locus of focus from [push button: 'Sizing'] to [page tab: 'Welcome'] / 14:35:01.880134 SPEECH OUTPUT: 'Welcome page tab.'`
  - `/usr/lib/python3/dist-packages/orca/scripts/default.py:1564-1566 (keyString == "space" early return in onSelectionChanged); orca/input_event.py:257-258 (' ' becomes keyval_name 'space')`
- **Reproduced:** '+ New tab' 2 of 2 runs, Orient 2 of 2 runs, Sizing 2 of 2 runs (5 of 5 bar rebuilds with focus elsewhere spoke 'Welcome page tab.')
- **Verification:** corrected by the verifier. Reproduced: Enter on '+ New tab' 3/3, Enter on Orient 3/3, AT click on '+ New tab' 2/2, AT click on Sizing 2/2; second activation silent 3/3; with Space the harness shows it 7/7 but a real Orca would not speak (source) Real, but the sweep's evidence over-reports it. All three sweep acts pressed Space. Orca's onSelectionChanged returns early when the last key it saw was Space (default.py:1565, `if keyString == "space": return`; lastKeyAndModifiers maps ' ' to 'space'). The harness's keys never reach Orca, so in the harness that guard never fires. A real Orca user who presses Space on '+ New tab', Orient or Sizing hears nothing, not 'Welcome page tab.'. I re-measured with activations that pass the guard. Enter on '+ New tab' gave 'Welcome page tab.' 3 of 3, Enter on Orient 3 of 3, AT-SPI click on '+ New tab' 2 of 2 and AT-SPI click on Sizing 2 of 2, with Orca's locus on the button first. In each, FOCUS MANAGER moves Orca's locus from the button to \[page tab: 'Welcome'\] while keyboard focus stays on the button. A second activation while Orca's locus is still on Welcome is silent ('\[page tab: Welcome\] 's selection redundant to \[page tab: Welcome\]', 3 of 3). So the reader who uses Enter or their screen reader's activation hears an unrelated tab, Orca's review and where-am-I point at Welcome, and later presses of the same button give no feedback at all. The mechanism is as the sweep says: a new TabBar is built on every TabWidget build (tab\_widget.rs:1395-1407), and atspi\_common enqueues selection-changed when a selected item is added (adapter.rs:78-80). Medium stands. The launch-time part (Welcome spoken before 'frame.') is harmless noise next to K1.
- **Fix idea:** Keep the TabList and header nodes across a TabWidget rebuild (memoize the bar and its headers as the panes are), so an unchanged selection emits no selection-changed. Separately, the example or TabWidget should select and announce a newly opened tab.

### tabs-07 {#tabs-07}

The 'Show all tabs' overflow dropdown cannot be operated from the keyboard

- **Example:** tab-widget
- **Scenario:** tabs-dropdown, tabs-dropdown-at
- **Act:** tabs-dropdown: open four new tabs so the strip overflows, focus 'Show all tabs', Space; then Down, Enter, Tab.
- **The reader should get:** Focus moves to the list, the entries are announced as the reader arrows through them (the current tab marked), and Enter selects one and returns to the strip.
- **The reader gets:** Focus lands on an unnamed list box and Orca says only 'list box.' Down produces no focus or active-descendant event and no speech, and Enter does nothing. Tab closes the popup and moves on to Sizing. No entry marks the current tab. The trigger advertises HasPopup::Menu, but the popup is a list box (Windows publishes haspopup=menu). Only an AT-SPI click on an entry's button works.
- **Platform:** Linux AT-SPI / Orca measured; the keyboard behaviour is platform-independent by source.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs:2802-2874; crates/teksilo-widgets/src/list\_view.rs:160; crates/teksilo-widgets/src/list\_view/widget\_impl.rs:496-505, 586
- **Evidence:**
  - `tabs-dropdown-20260925-140731 act "Space on 'Show all tabs'": object:children-changed:add [page tab list] '' -> [panel] ''; object:state-changed:focused 1 [list box] ''; orca-debug.out 14:07:48.181763 - SPEECH OUTPUT: 'list box.'`
  - `tree after opening: list box '' ['focusable', 'focused', 'vertical'] > list item 'Welcome' ['selectable'] ... list item 'Doc 7' ['selectable'] (none selected)`
  - `act 'Down in the dropdown': FAIL focus lands on [list item] '*' / no focus change on the bus in this act; FAIL Orca says something / Orca said nothing in this act`
  - `act 'Enter in the dropdown': no focus change on the bus in this act; Orca said nothing`
  - `act 'Tab in the dropdown': object:children-changed:remove [page tab list] '' -> [panel] ''; object:state-changed:focused 1 [push button] 'Sizing'; 14:08:00.236805 SPEECH OUTPUT: 'Sizing push button.'`
  - `tabs-dropdown-at-20260925-135921: AT-SPI click on push button 'Doc 6' selects Doc 6 (13:59:39.038226 FOCUS MANAGER: Changing locus of focus from [push button: '+ New tab'] to [page tab: 'Doc 6'])`
  - `crates/teksilo-widgets/src/tab_widget/bar.rs:2824-2839 ListView::new(model, delegate) with no selection model and no on_activate; rows are Buttons; list_view/widget_impl.rs:495-499 Enter only selects through a selection model`
  - `crates/teksilo-widgets/src/tab_widget/bar.rs:2873 .has_popup_kind(HasPopup::Menu); accesskit_windows-0.35.0/src/node.rs:485-494 publishes it as haspopup=menu`
  - `tabs-dropdown-20260925-143148-593536: 'Space on Show all tabs' ORCA SAYS 'list box.'; 'Down in the dropdown' no focus change, Orca said nothing; 'Enter in the dropdown' nothing; 'Tab in the dropdown' -> 'Sizing push button.'`
- **Reproduced:** 3 of 3 runs (tabs-dropdown)
- **Verification:** corrected by the verifier. Reproduced: 2 of 2 my runs (tabs-dropdown); 2 of 2 realistic AT-click picks Symptoms confirmed 2 of 2 in my runs (3 of 3 counting the sweep's): 'list box.' on open, Down and Enter silent with no bus event, Tab leaves to Sizing, and no entry is marked selected. I refine the cause. The dropdown is a ListView with no selection model and no on\_activate (bar.rs:2824-2839). A ListView's keyboard cursor, focused\_index, is a plain Rc&lt;Cell&gt; (list\_view.rs:160), set on navigation at widget\_impl.rs:586 with no accessibility refresh, so without a selection model a cursor move publishes nothing. The active\_descendant (widget\_impl.rs:1051) is never re-read. Enter only selects or activates through a selection model or on\_activate (widget\_impl.rs:496-505), and the dropdown has neither. Any ListView built without a selection model therefore has silent arrows, not only this dropdown. Also, choosing an entry by AT click, the only route that works, leaves keyboard focus on the trigger while Orca announces the chosen tab (listed under missed as tabs-v2). HasPopup::Menu on a list box is right as stated (bar.rs:2873; accesskit\_windows node.rs:485-494).
- **Fix idea:** Build the dropdown as a MenuList of checkable/radio rows (current tab checked) whose activation sets selected\_id and returns focus to that tab, matching HasPopup::Menu. Or give the ListView a single-selection model seeded with the current tab plus an on\_activate, name it ('Tabs'), and set HasPopup::Listbox.

### tabs-08 {#tabs-08}

The tab's context menu is silent while the reader moves through it (MenuList exposes no current item)

- **Example:** tab-widget
- **Scenario:** tabs-context-menu, tabs-migration-move
- **Act:** tabs-context-menu: on Doc 2, the Menu key (or Shift+F10), then Down, then Enter.
- **The reader should get:** The first item gets focus (or active descendant) and is spoken ('Move Left menu item'), Down speaks the next item, and Enter runs the item the reader heard.
- **The reader gets:** Focus goes to the unnamed menu container and Orca says 'menu.' Down produces no event and no speech, and no item ever carries focused or selected state. Enter then runs an item the reader never heard (here Move Left: 'Doc 2 moved to 4 of 6'). The same happens on tab-migration (Alpha's menu).
- **Platform:** All platforms by source (no active descendant, no item focus); Linux measured.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `9636094c` (menus).
- **Where:** crates/teksilo-widgets/src/menu\_list.rs:752-797, 991-993
- **Evidence:**
  - `tabs-context-menu-20260925-140735 act 'the context-menu key on Doc 2': +14.1 ms object:children-changed:add [frame] '' -> [menu] ''; object:state-changed:focused 1 [menu] ''; orca-debug.out 14:07:47.387556 - SPEECH OUTPUT: 'menu.'`
  - `act 'Down in the menu': FAIL focus lands on [menu item] '*' / no focus change on the bus in this act; FAIL Orca says something / Orca said nothing in this act`
  - `tree after Down: menu '' ['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible'] > menu item 'Move Left' ['enabled', 'sensitive', 'showing', 'visible'] (no item focused or selected)`
  - `tabs-context-menu-20260925-135956 act 'Enter on the focused item': +19.8 ms object:announcement [status bar] 'Doc 2 moved to 4 of 6'`
  - `crates/teksilo-widgets/src/menu_list.rs:759-794 arrows only set the internal focused_index; menu_list.rs:949 the menu node itself is the focusable; menu_list.rs:991-993 accessibility sets Role::Menu and no active_descendant`
- **Reproduced:** 3 of 3 runs (tabs-context-menu: 'menu.' on open; Down silent in 2 of 2 runs that pressed it); tab-migration menu 2 of 2 'menu.'
- **Verification:** confirmed. Reproduced: 4 of 4 runs (Down silent), tab-migration menu 'menu.' 4 of 4
- **Fix idea:** Publish the highlighted item as active\_descendant of the Role::Menu while it has focus (as ListView does, list\_view/widget\_impl.rs:1025-1055), or move real focus to the items; highlight the first item on keyboard open. This is MenuList-wide, so the menus sweep may report the same root cause.

### tabs-09 {#tabs-09}

tab-migration: a tab can be moved to the other group only by dragging; there is no keyboard or AT route

- **Example:** tab-migration
- **Scenario:** tabs-migration-move
- **Act:** tabs-migration-move: on Alpha (Group A), the context-menu key; Escape; Alt+End; Alt+Right at the end of group A.
- **The reader should get:** A non-drag alternative exists for the example's one function (WCAG 2.1.1 / 2.5.7): a 'Move to Group B' menu item, a chord, or an AT action. At minimum, a move that cannot go further says so.
- **The reader gets:** The menu offers only 'Move Right' and 'Move to End' inside group A. Alt+End moves Alpha to the end of group A. A further Alt+Right does nothing and says nothing: the order is unchanged and Orca is silent.
- **Platform:** All platforms (logic); Linux measured.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs:1367-1417; crates/teksilo-widgets/src/common/ordered\_move.rs:443-460; crates/teksilo-widgets/src/tab\_widget/header.rs:821-823
- **Evidence:**
  - `tabs-migration-move-20260925-140817 act 'the context-menu key on Alpha': FAIL the menu offers moving the tab to the other group / menu items: ['Move Right', 'Move to End']`
  - `act 'Alt+Right on Alpha at the end of group A': FAIL Orca says something / Orca said nothing in this act; pass the page tabs read in the order ['Bravo', 'Charlie', 'Alpha', 'Xeno', 'Yotta']`
  - `crates/teksilo-widgets/src/tab_widget/bar.rs:1368-1417: cross-bar transfer exists only as a drag payload plus on_drag_ended; crates/teksilo-widgets/src/common/ordered_move.rs:443-460 the default menu holds in-bar moves only`
  - ``crates/teksilo-widgets/src/tab_widget/header.rs:821-823 `let Some(dest) = mv.destination(index, count) else { return; };` returns without feedback at an end``
  - `examples/tab_migration/src/main.rs:147-170: accept_external_tabs(true) with no TabInfo::context_menu or other route`
- **Reproduced:** 2 of 2 runs (deterministic)
- **Verification:** confirmed. Reproduced: 4 of 4 runs (deterministic)
- **Fix idea:** When accept\_external\_tabs is on, let TabWidget register its peers (or take a list of named targets) and add 'Move to &lt;group&gt;' items to the header's default context menu, running the same on\_tab\_received / on\_transfer\_out pair a drop does. The example can do it today through TabInfo::context\_menu. At a strip end, announce that the tab is already first/last.

### tabs-10 {#tabs-10}

A tab list cannot be named, so tab-migration's two groups sound identical

- **Example:** tab-migration
- **Scenario:** tabs-migration-tree, tabs-migration-walk
- **Act:** tabs-migration-tree; tabs-migration-walk: Tab to Alpha (Group A), Tab into its panel, Tab to Xeno (Group B).
- **The reader should get:** Each tab list is named by its visible heading ('Group A' / 'Group B'), and a reader landing on a tab learns which group they are in.
- **The reader gets:** Both page tab lists are unnamed, and Orca says only 'Alpha page tab.' and 'Xeno page tab.' TabWidget and TabBar offer no way to name the TabList. An .access\_label on the TabWidget lands on its Role::GenericContainer, which the consumer's filter removes. The example's 'Group A' heading is a sibling label.
- **Platform:** All platforms (tree); Linux measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs:2199-2218
- **Evidence:**
  - `tabs-migration-tree-20260925-140834: FAIL each tab list has a name that tells the two groups apart / [page tab list] '' desc=None states=['enabled', 'horizontal', 'sensitive', 'showing', 'visible'] (twice)`
  - `tabs-migration-walk-20260925-140804: orca-debug.out 14:08:12.919916 - SPEECH OUTPUT: 'Alpha page tab.' / 14:08:20.722656 - SPEECH OUTPUT: 'Xeno page tab.'`
  - `crates/teksilo-widgets/src/tab_widget/bar.rs:2199-2218 TabList accessibility sets role, orientation and size_of_set, no name or labelled_by; tab_widget.rs:1549-1551 TabWidget is Role::GenericContainer; accesskit_consumer-0.39.0/src/filters.rs:30-33 drops GenericContainer nodes`
  - `python3 tools/extract_widget_api.py TabWidget lists no label/name builder`
  - `examples/tab_migration/src/main.rs:159-168 the group heading is a sibling TextWidget of the TabWidget`
- **Reproduced:** 2 of 2 runs (tree and walk)
- **Verification:** confirmed. Reproduced: tree 2 of 2, walk 1 of 1 (deterministic)
- **Fix idea:** Add TabWidget/TabBar::label(impl Into&lt;Prop&lt;String&gt;&gt;) and labelled\_by(WidgetId), forwarded to the TabList node, and use them in tab-migration.

### tabs-11 {#tabs-11}

A disabled tab is exported as enabled and sensitive on AT-SPI, so Orca never says it is unavailable

- **Example:** tab-widget
- **Scenario:** tabs-launch-tree, tabs-at-activation
- **Act:** tabs-launch-tree (Locked's states); tabs-at-activation: AT-SPI grab\_focus on the disabled Locked tab.
- **The reader should get:** Locked is published without enabled/sensitive, and Orca says 'grayed' (or focus is refused).
- **The reader gets:** Locked carries enabled and sensitive. 'selectable' is absent, which shows the adapter saw is\_disabled, so the flag reached AccessKit. Orca says 'Locked page tab.' plus the example's own tooltip 'Disabled tabs cannot be activated.', which is all that tells this reader it is unavailable; a disabled tab without such a tooltip reads as available. The tab also accepts AT focus.
- **Platform:** Linux only. By source, Windows publishes IsEnabled = !is\_disabled (accesskit\_windows node.rs:535) and macOS isAccessibilityEnabled = !is\_disabled (accesskit\_macos node.rs:658-660).
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/tab\_widget/header.rs:1216-1222
- **Evidence:**
  - `tabs-launch-tree-20260925-135331: [page tab] 'Locked' desc='Disabled tabs cannot be activated' states=['enabled', 'focusable', 'sensitive', 'showing', 'visible'] attrs={'setsize': '6', 'posinset': '3'} actions=[]`
  - `tabs-at-activation-20260925-140821: object:state-changed:focused 1 [page tab] 'Locked'; orca-debug.out 14:08:37.024551 - SPEECH OUTPUT: 'Locked page tab.' / 14:08:37.024581 - SPEECH OUTPUT: 'Disabled tabs cannot be activated.'; FAIL the reader is told Locked is unavailable`
  - `accesskit_atspi_common-0.20.0/src/node.rs:374-380 inserts Enabled|Sensitive unless is_read_only_supported() && is_read_only_or_disabled(); accesskit_consumer-0.39.0/src/node.rs:861-879 is_read_only_supported excludes Tab (and Button); node.rs:346-351 drops Selectable when disabled`
  - `crates/teksilo-widgets/src/tab_widget/header.rs:1222 adds Action::Focus even when disabled`
- **Reproduced:** tree 3 of 3 launch-tree runs; speech 2 of 2 runs
- **Verification:** confirmed. Reproduced: tree 2 of 2, speech 2 of 2
- **Fix idea:** Upstream fix: accesskit\_atspi\_common should drop Enabled\|Sensitive for any disabled node, not only read-only-capable roles. Teksilo could meanwhile omit Action::Focus on a disabled tab, so AT cannot land on it, and file the adapter bug (it affects every disabled button too).

### tabs-12 {#tabs-12}

The tab list's AT structure: non-tab children, the unpinned tabs nested in an unnamed panel, and setsize on every descendant

- **Example:** tab-widget
- **Scenario:** tabs-launch-tree
- **Act:** tabs-launch-tree: read the strip at launch.
- **The reader should get:** A tab list owns its tabs directly (ARIA tablist: required owned elements = tab), and only the tabs carry position/set size.
- **The reader gets:** The page tab list's children are a label ' Showcase ', the pinned Welcome tab, an unnamed panel (the ScrollArea) holding the other five tabs, and the bar-slot controls (Sizing, Orient, Theme combo box, '+ New tab'). Every one of these, and the panel, carries setsize=6. With Orca's position speaking on, its sibling count would give Welcome '2 of 7' and Settings '1 of 5' instead of the published 1 and 2 of 6. The overflow popup is also mounted inside the tab list.
- **Platform:** AT-SPI and UIA both resolve setsize from the nearest ancestor with size\_of\_set (atspi\_common node.rs:397-400; accesskit\_windows node.rs:689-691), so every descendant gets it on Linux and Windows; macOS publishes neither. Linux measured.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs:1619-1760, 2199-2218
- **Evidence:**
  - `tabs-launch-tree-20260925-135331: FAIL the tab list holds only page tabs / [label] ' Showcase ' attrs={'setsize': '6'} / [panel] '' attrs={'setsize': '6'} / [push button] 'Sizing' attrs={'setsize': '6'} / [push button] 'Orient' attrs={'setsize': '6'} / [combo box] 'Theme' attrs={'setsize': '6'} / [push button] '+ New tab' attrs={'setsize': '6'}`
  - `FAIL every page tab is a direct child of the tab list / direct page-tab children: ['Welcome'] / page tabs nested under another node: ['Settings', 'Locked', 'Doc 1', 'Doc 2', 'Doc 3']`
  - `tabs-dropdown: tree 'page tab list '' > list box ''' (the popup is a child of the tab list)`
  - `orca/script_utilities.py:3249-3296 getPositionAndSetSize counts the filtered siblings in the parent`
  - `crates/teksilo-widgets/src/tab_widget/bar.rs:1619-1760 composes slots, pinned strip, arrows, the ScrollArea and the dropdown under the one node whose accessibility() is Role::TabList (2199-2218)`
- **Reproduced:** 3 of 3 launch-tree runs (deterministic)
- **Verification:** confirmed. Reproduced: 2 of 2 (deterministic)
- **Fix idea:** Put Role::TabList on the node that holds only the headers (pinned strip plus scroll content, with the ScrollView's own node made GenericContainer so it is pruned), and make the slots, arrows and dropdown siblings of the tab list in a toolbar/group container.

### tabs-13 {#tabs-13}

Tabs scrolled out of an overflowing strip leave the AT tree, and come back under defunct ids

- **Example:** tab-widget
- **Scenario:** tabs-overflow, tabs-overflow-arrows
- **Act:** tabs-overflow (tree after the strip overflows); tabs-overflow-arrows: visit Settings and Doc 1, End (strip scrolls to Doc 7), Home, Right back to Settings.
- **The reader should get:** Every tab stays in the tree (off-screen) so object navigation and flat review can find all ten, and returning focus to a tab is heard normally.
- **The reader gets:** With 10 tabs, only 8 page tabs are in the tree (Doc 6 and Doc 7 are missing) while each carries setsize=10. After End, the earlier tabs leave the tree and are marked defunct. On return, Orca drops the focus event from the returning Settings tab as defunct. It still speaks 'Settings page tab.', but only because the tab list's selection-changed (automatic activation) makes it present the selected tab: the same mechanism as tabs-01, rescued here by selection.
- **Platform:** Linux AT-SPI / Orca measured; the clip filter is in accesskit\_consumer and applies to every adapter's tree.
- **Severity:** low; **layer:** upstream
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: tabs-overflow-arrows' check that the tab scrolled back into view is not defunct failed in the sweep and passes now.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs (headers inside a clipping ScrollArea)
- **Evidence:**
  - `tabs-overflow-20260925-135612 act 'three more new tabs': outline of the page tab list: page tabs Welcome..Doc 5 each attrs setsize '10' (8 page tabs); FAIL the tree holds [page tab] 'Doc 7'`
  - `tabs-overflow-arrows-20260925-140406 act 'Right: to Settings (scrolled back into view)': object:children-changed:add [panel] '' -> [page tab] 'Settings'; object:state-changed:focused 1 [page tab] 'Settings'`
  - `orca-debug.out: 14:04:36.036893 EVENT MANAGER: Ignoring defunct object: [page tab: 'Settings'] ... 14:04:36.047930 EVENT MANAGER: object:selection-changed for [page tab list] ... 14:04:36.110147 SPEECH OUTPUT: 'Settings page tab.'`
  - `accesskit_consumer-0.39.0/src/filters.rs:64-86 excludes a clipped child whose box and both neighbours' boxes miss the clipping parent's box`
- **Reproduced:** defunct drop 3 of 3 runs that visited Settings before scrolling; speech rescued 3 of 3; the tree count deterministic (2 of 2)
- **Verification:** confirmed. Reproduced: tree 3 of 3; defunct-then-rescued 1 of 1 in my run (3 of 3 in the sweep's)
- **Fix idea:** The same re-keying as tabs-01 would stop the defunct drop. To keep off-screen tabs findable, the header row could be a non-clipping node for AT (clip only in paint), or the bar could publish all headers.

### tabs-14 {#tabs-14}

A pinned tab's declared tooltip is replaced by its title, for AT and for hover

- **Example:** tab-widget
- **Scenario:** tabs-launch-tree
- **Act:** tabs-launch-tree: read the pinned Welcome tab.
- **The reader should get:** The Welcome tab's description is its declared tooltip 'Welcome — start here' (example main.rs:211).
- **The reader gets:** desc='Welcome', which repeats the name (Orca does not speak it), and the author's text is lost for sighted hover too.
- **Platform:** All platforms; Linux measured.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/header.rs:559-561
- **Evidence:**
  - `tabs-launch-tree-20260925-135331: FAIL the pinned tab keeps its declared tooltip 'Welcome — start here' as its description / [page tab] 'Welcome' desc='Welcome'`
  - ``crates/teksilo-widgets/src/tab_widget/header.rs:553-561: `if self.pinned { self.tooltip = Some(self.at_name.clone()); }` runs unconditionally, although the comment says a caller-set tooltip is respected (and TabWidget's delegate already promotes the title only when no tooltip is set, tab_widget.rs:1003-1015)``
- **Reproduced:** 3 of 3 launch-tree runs (deterministic)
- **Verification:** confirmed. Reproduced: 2 of 2 (deterministic)
- **Fix idea:** Only promote at\_name when self.tooltip is None.

### tabs-15 {#tabs-15}

No Ctrl+Tab / Ctrl+PageDown to switch tabs from inside a panel

- **Example:** tab-widget
- **Scenario:** tabs-panel
- **Act:** tabs-panel: in the Settings panel (on Toggle orientation), Ctrl+Tab; then Ctrl+PageDown.
- **The reader should get:** The desktop tab-control chord selects the next tab from inside the panel (Windows tab control and property sheets: Ctrl+Tab / Ctrl+PageDown; GTK notebook: Ctrl+PageDown), so a reader can switch tabs without first leaving the panel.
- **The reader gets:** Ctrl+Tab behaves as a plain Tab (wraps to the Settings tab, still selected); Ctrl+PageDown does nothing.
- **Platform:** All platforms; Linux measured.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget.rs (no key handler on TabWidget)
- **Evidence:**
  - `tabs-panel-20260925-141128 act 'Ctrl+Tab inside the panel': object:state-changed:focused 1 [page tab] 'Settings'; selected page tabs: ['Settings']; orca-debug.out 14:11:49.521413 SPEECH OUTPUT: 'Settings page tab.'`
  - `act 'Ctrl+PageDown inside the panel': no events; Orca said nothing in this act`
  - `crates/teksilo-core/src/widget_tree/pointer_router.rs:1103-1140 Ctrl+Tab is only reserved for keyboard-capture nodes, otherwise it cycles focus like Tab; no chord handler in crates/teksilo-widgets/src/tab_widget*`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 1 of 1 (plus the sweep's 3 of 3); deterministic
- **Fix idea:** Add an on\_key\_preview on TabWidget for Ctrl+PageDown/PageUp (and Ctrl+Tab/Ctrl+Shift+Tab where not reserved) that selects the next/previous enabled tab and focuses its header.

### tabs-v1 {#tabs-v1}

The focused 'Scroll tabs right' hides itself at the end of the strip, and focus falls to the window

- **Example:** tab-widget
- **Scenario:** verify-tabs-scroll-arrow-hide
- **Act:** verify-tabs-scroll-arrow-hide: open four new tabs, AT focus on Welcome, Tab to 'Scroll tabs right' (a Tab stop), then Space on it repeatedly
- **The reader should get:** The strip scrolls. When nothing is left to scroll, focus moves to a sensible neighbour (the last tab, or 'Show all tabs') and the reader hears where they are, or the arrow is not a Tab stop at all.
- **The reader gets:** On the 4th press the arrow leaves the tree while it holds focus. Focus goes to the unnamed window frame and Orca says 'frame.' The next Tab starts over at Welcome, so the reader has lost their place. An arrow that comes back later reuses its old id (the tabs-01 mechanism), so focusing it again would also be silent (by mechanism, not measured).
- **Platform:** Linux AT-SPI / Orca measured; the focus loss is platform-independent by source
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs:1699-1741
- **Evidence:**
  - `verify-tabs-scroll-arrow-hide-20260925-143303-615829 act 'press 4': +214.8 ms object:children-changed:remove [page tab list] '' -> [push button] 'Scroll tabs right'; +218.8 ms object:state-changed:focused 1 [frame] ''; +218.8 ms focused 0 [push button] 'Scroll tabs right'; +304.7 ms ORCA SAYS 'frame.'; 14:33:29.104114 EVENT MANAGER: Ignoring defunct object: [push button: 'Scroll tabs right']`
  - `same run, act 'Tab from where focus is now': ORCA SAYS 'Welcome page tab.'`
  - `verify-tabs-scroll-arrow-hide-20260925-143352-633693 act 'press 4': +219.5 ms children-changed:remove 'Scroll tabs right'; +219.6 ms focused 1 [frame] ''; ORCA SAYS 'frame.'`
  - `crates/teksilo-widgets/src/tab_widget/bar.rs:1724-1741 trailing arrow hidden with ctx.visible_when(arrow_id, x + 0.5 < max), with no focus hand-off; the leading arrow likewise at 1699-1714; build_scroll_arrow (bar.rs:2699-2756) makes a focusable IconButton`
- **Reproduced:** 2 of 2 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Make the scroll arrows non-focusable, since keyboard users reach every tab through the arrows and the dropdown. Or, before hiding a focused arrow, move focus to the nearest tab.

### tabs-v2 {#tabs-v2}

Picking a tab from 'Show all tabs' leaves keyboard focus on the trigger while Orca announces the chosen tab

- **Example:** tab-widget
- **Scenario:** verify-tabs-dropdown-realistic
- **Act:** verify-tabs-dropdown-realistic: four new tabs; AT focus on 'Show all tabs'; Space (focus goes into the list box); AT-SPI click on the 'Doc 6' entry (the only route that works, see tabs-07); then Tab
- **The reader should get:** Doc 6 is selected, the popup closes, and keyboard focus lands on the Doc 6 tab, so what Orca says and where the keyboard is agree.
- **The reader gets:** Focus returns to 'Show all tabs' (spoken, cut), then the tab list's selection-changed moves Orca's locus to Doc 6 and Orca says 'Doc 6 page tab.'. Keyboard focus is still on the trigger: Tab goes to Sizing, not into Doc 6's panel. The reader believes they are on Doc 6.
- **Platform:** Linux AT-SPI / Orca measured. The focus restore is platform-independent; the locus move is Orca's onSelectionChanged, which runs here because no Space was pressed.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs:2826-2836
- **Evidence:**
  - `verify-tabs-dropdown-realistic-20260925-143307-616636 act "AT-SPI click on the 'Doc 6' entry": +74.8 ms object:state-changed:focused 1 [push button] 'Show all tabs'; +80.0 ms object:selection-changed [page tab list] ''; ORCA SAYS (CUT) 'Show all tabs push button.' then 'Doc 6 page tab.'; orca-debug.out 14:33:23.129927 FOCUS MANAGER: Changing locus of focus from [list box] to [push button: 'Show all tabs'] / 14:33:23.180589 ... from [push button: 'Show all tabs'] to [page tab: 'Doc 6']`
  - `same run, act 'Tab from there': ORCA SAYS 'Sizing push button.'`
  - `crates/teksilo-widgets/src/tab_widget/bar.rs:2833-2835 the entry sets selected_id and calls ctx.dismiss_self_overlay_chain(), with no focus request onto the chosen header`
- **Reproduced:** 2 of 2 runs (plus 2 of 2 tabs-dropdown-at, where focus returned to '+ New tab' instead)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** After setting selected\_id, request focus on the chosen tab's header (it is in header\_ids), as arrow selection does, and let the dismiss skip its focus restore.

### tabs-v3 {#tabs-v3}

Enter or Space on a tab whose panel has nothing focusable does nothing and says nothing, and that panel cannot be reached with Tab

- **Example:** tab-widget
- **Scenario:** verify-tabs-enter-empty-panel
- **Act:** verify-tabs-enter-empty-panel: Tab to Welcome, Enter, then Tab
- **The reader should get:** Enter moves the reader into the panel (ARIA: a tabpanel without focusable content takes tabindex=0), or at least says something, so the panel's text is reachable from the keyboard.
- **The reader gets:** No focus change and no speech. Tab goes on to Sizing, and the Welcome text is never a Tab stop. The same holds for Locked's panel. The text can only be reached through Orca's own review commands.
- **Platform:** All platforms (logic); Linux measured
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** crates/teksilo-widgets/src/tab\_widget/header.rs:943-966
- **Evidence:**
  - `verify-tabs-enter-empty-panel-20260925-143911-753707 act 'Enter on Welcome (its panel has no focusable control)': no events, Orca said nothing; act 'Tab from there': ORCA SAYS 'Sizing push button.'`
  - `crates/teksilo-widgets/src/tab_widget/header.rs:943-966: Enter/Space call ctx.request_focus_into(panel_id), a documented no-op when the panel has nothing focusable`
  - `crates/teksilo-widgets/src/tab_widget/info.rs:73-75 TabInfo::focusable_panel exists (default false); examples/tab_widget/src/main.rs:206-233 does not set it for its text-only Welcome and Locked panels`
- **Reproduced:** 2 of 2 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** The example should set .focusable\_panel(true) on Welcome and Locked. The framework could make a content-less panel focusable by default, or have Enter fall back to focusing the panel node.
