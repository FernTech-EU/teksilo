<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Generated from the sweep of 25 and 26 September 2026 (on 261a218f) and its re-measure of 27 September 2026 (on c198e4d1). See ../reader-findings.md. -->

# Tabs

Examples: `tab-widget`, `tab-migration`.
18 findings: 1 critical, 4 high, 7 medium, 6 low.
Swept on `261a218f` on 25 and 26 September 2026, measured again on
`c198e4d1` on 27 September 2026.
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
| [tabs-08](#tabs-08) | tab-widget | The tab's context menu is silent while the reader moves through it (MenuList exposes no current item) | low | all | partly fixed |
| [tabs-09](#tabs-09) | tab-migration | tab-migration: a tab can be moved to the other group only by dragging; there is no keyboard or AT route | high | all | open |
| [tabs-10](#tabs-10) | tab-migration | A tab list cannot be named, so tab-migration's two groups sound identical | medium | all | open |
| [tabs-11](#tabs-11) | tab-widget | A disabled tab is exported as enabled and sensitive on AT-SPI, so Orca never says it is unavailable | medium | Linux | fixed |
| [tabs-12](#tabs-12) | tab-widget | The tab list's AT structure: non-tab children, the unpinned tabs nested in an unnamed panel, and setsize on every descendant | low | Linux, Windows | open |
| [tabs-13](#tabs-13) | tab-widget | Tabs scrolled out of an overflowing strip leave the AT tree, and come back under defunct ids | low | Linux | partly fixed |
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
- **The reader got (`261a218f`):** Focus lands on the button on the bus, but Orca says nothing: it logs 'Ignoring defunct object' for the focus event. Space on the button then works (the strip turns vertical) and is silent too. Leaving a tab removes its whole panel subtree from the AT tree (every node gets object:state-changed:defunct 1). Returning brings back the same NodeIds, and libatspi, which already cached them, keeps them defunct. Any control the reader visited in a panel becomes unreadable after one switch away and back, for static and dynamic tabs alike.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The defunct state is an AT-SPI mechanism (accesskit\_atspi\_common remove\_node plus libatspi's cache). The UIA and macOS adapters have no equivalent, so Windows and macOS are probably unaffected; not measured.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** On the second visit the reader hears the panel's button as on the first: 'Toggle orientation push button.' and 'Make an edit push button.', by Enter, by Tab and by the screen reader's own focus request. The returned button no longer carries an id the bus was told was defunct, and Orca ignores nothing.
- **Measured again:** tabs-panel-revisit 2 of 2, tabs-doc-revisit 2 of 2, verify-tabs-revisit-tab 2 of 2 (Tab and AT focus routes)
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-panel-revisit 'Enter into the returned Settings panel': +14.1 ms object:state-changed:focused 1 [push button] 'Toggle orientation'; +63.8 ms ORCA SAYS: 'Toggle orientation push button.'; pass the node focus lands on was not announced defunct earlier in the run`
  - `pass2 tabs-panel-revisit 'Enter into the returned Settings panel': +29.3 ms object:state-changed:focused 1 [push button] 'Toggle orientation'; +316.7 ms ORCA SAYS: 'Toggle orientation push button.'`
  - `pass1 tabs-doc-revisit 'Enter into Doc 1's returned panel': +62.4 ms object:state-changed:focused 1 [push button] 'Make an edit'; +579.8 ms ORCA SAYS: 'Make an edit push button.'`
  - `pass2 tabs-doc-revisit 'Enter into Doc 1's returned panel': +15.9 ms object:state-changed:focused 1 [push button] 'Make an edit'; +82.1 ms ORCA SAYS: 'Make an edit push button.'`
  - `pass1 verify-tabs-revisit-tab 'Tab x5 into the returned panel': +1599.3 ms object:state-changed:focused 1 [push button] 'Toggle orientation'; +2377.4 ms ORCA SAYS: 'Toggle orientation push button.'`
  - `pass2 verify-tabs-revisit-tab 'AT-SPI grab_focus on the returned Toggle orientation': +29.3 ms object:state-changed:focused 1 [push button] 'Toggle orientation'; +183.4 ms ORCA SAYS: 'Toggle orientation push button.'`
- **Where (`c198e4d1`):** crates/teksilo-core/src/accessibility.rs:1985-1996 (widget\_id\_to\_node\_id, unchanged in behaviour); crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-187 (the ids handed to the adapter, new for a node that comes back); crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs:196-210; crates/teksilo-widgets/src/primitives/switcher.rs:233-241; crates/teksilo-widgets/src/tab\_widget.rs:1498-1503
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** The tab order is unchanged, yet an announcement claims the move, and Orca speaks it. Every move offered on Settings (keys, context menu, custom actions) does nothing, and each keyboard one is announced as done.
- **Platform:** Linux AT-SPI / Orca measured. By source, all platforms: the announced text is platform-independent (Windows raises LiveRegionChanged and the client reads the node's name; macOS posts the text).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The tab order does not change, yet the reader hears the move as done: 'Doc 1 moved to 1 of 6' for Alt+Home, 'Doc 1 moved to 3 of 6' for Alt+Left, and 'Settings moved to 3 of 6' for Alt+Right on the static Settings tab. The second false message used to be dropped and is now spoken too. Settings' context menu still offers Move Left, Move Right, Move to Start and Move to End.
- **Measured again:** tabs-reorder-rejected, 2 of 2 runs; tabs-reorder-static, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-reorder-rejected 'Alt+Home on Doc 1': +29.3 ms object:announcement [status bar] 'Doc 1 moved to 1 of 6' text='Doc 1 moved to 1 of 6'; +43.8 ms ORCA SAYS: 'Doc 1 moved to 1 of 6'; pass the page tabs read in the order ['Welcome', 'Settings', 'Locked', 'Doc 1', 'Doc 2', 'Doc 3']`
  - `pass1 tabs-reorder-rejected 'Alt+Left on Doc 1': +26.2 ms object:announcement [status bar] 'Doc 1 moved to 3 of 6' text='Doc 1 moved to 3 of 6'; +34.2 ms ORCA SAYS: 'Doc 1 moved to 3 of 6'`
  - `pass2 tabs-reorder-rejected 'Alt+Home on Doc 1': +52.2 ms ORCA SAYS: 'Doc 1 moved to 1 of 6'; 'Alt+Left on Doc 1': +40.4 ms ORCA SAYS: 'Doc 1 moved to 3 of 6'; order unchanged`
  - `pass1 tabs-reorder-static 'Alt+Right on Settings': +26.8 ms object:announcement [status bar] 'Settings moved to 3 of 6' text='Settings moved to 3 of 6'; +36.8 ms ORCA SAYS: 'Settings moved to 3 of 6'; order unchanged`
  - `pass2 tabs-reorder-static 'Alt+Right on Settings': +40.7 ms ORCA SAYS: 'Settings moved to 3 of 6'; 'the context-menu key on Settings': menu items 'Move Left', 'Move Right', 'Move to Start', 'Move to End'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/header.rs:809-832, 1086-1101, 1249-1270; crates/teksilo-widgets/src/tab\_widget.rs:1074-1096
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** A move rebuilds the whole tab bar: a new page tab list and new tab nodes, with focus re-requested on the new header. The announcement reaches the bus 0.4-3 ms before that focus change, and Orca stops speech to present the focus. The reader hears 'Doc 1 page tab.' The first message of a session is cut; later ones are also dropped (K2).
- **Platform:** Linux AT-SPI / Orca measured. Windows and macOS not measured; by source the ordering (node changes before focus) is the consumer's, and whether NVDA/VoiceOver cut depends on the reader.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `b518253f` (announce-focus).
- **Now (`c198e4d1`):** After a keyboard move the reader hears the tab and then where it went, whole: 'Doc 1 page tab.' then 'Doc 1 moved to 5 of 6'. The announcement now reaches the bus after the focus change, so Orca no longer cuts it. The same holds for Move Left from the context menu, Alt+End in tab-migration and Alt+Up in a vertical strip.
- **Measured again:** tabs-reorder 2 of 2 (4 of 4 moves), tabs-context-menu 2 of 2, tabs-migration-move 2 of 2, verify-tabs-vertical 2 of 2, fix-announce-focus-tabs 2 of 2 (4 of 4 moves)
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-reorder 'Alt+Right moves Doc 1 after Doc 2': +35.7 ms object:state-changed:focused 1 [page tab] 'Doc 1'; +40.0 ms object:announcement [status bar] 'Doc 1 moved to 5 of 6' text='Doc 1 moved to 5 of 6'; +109.3 ms ORCA SAYS: 'Doc 1 page tab.'; +123.6 ms ORCA SAYS: 'Doc 1 moved to 5 of 6'`
  - `pass2 tabs-reorder 'Alt+Right again': +40.7 ms object:state-changed:focused 1 [page tab] 'Doc 1'; +45.3 ms object:announcement [status bar] 'Doc 1 moved to 6 of 6'; +129.4 ms ORCA SAYS: 'Doc 1 page tab.'; +149.0 ms ORCA SAYS: 'Doc 1 moved to 6 of 6'`
  - `pass2 tabs-context-menu 'Enter on the focused item': +25.5 ms object:state-changed:focused 1 [page tab] 'Doc 2'; +28.9 ms object:announcement [status bar] 'Doc 2 moved to 4 of 6'; +99.7 ms ORCA SAYS: 'Doc 2 page tab.'; +120.3 ms ORCA SAYS: 'Doc 2 moved to 4 of 6'`
  - `pass1 tabs-migration-move 'Alt+End on Alpha': +128.4 ms object:state-changed:focused 1 [page tab] 'Alpha'; +156.0 ms object:announcement [status bar] 'Alpha moved to 3 of 3'; +620.5 ms ORCA SAYS: 'Alpha page tab.'; +817.2 ms ORCA SAYS: 'Alpha moved to 3 of 3'`
  - `pass2 verify-tabs-vertical 'Alt+Up on Doc 3': +42.9 ms object:state-changed:focused 1 [page tab] 'Doc 3'; +62.3 ms object:announcement [status bar] 'Doc 3 moved to 5 of 6'; +112.2 ms ORCA SAYS: 'Doc 3 page tab.'; +133.5 ms ORCA SAYS: 'Doc 3 moved to 5 of 6'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/header.rs:821-835; crates/teksilo-core/src/announcer.rs:71-106, 287-320 (a message waits out an update that moves focus)
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** On AT-SPI, Doc 1 exposes only 'click'. The header's AccessKit custom actions ('Close' and the four moves) are published by no AccessKit adapter on any platform. The × button is culled from the tree unless a pointer hovers. The tab's context menu lists only the moves. Delete on the focused tab works, but nothing tells the reader it exists (no key shortcut on the node, no hint).
- **Platform:** All platforms by source: accesskit\_atspi\_common 0.20, accesskit\_windows 0.35 and accesskit\_macos 0.27 contain no custom-action support. Linux measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** A closable tab still exposes only 'click' on AT-SPI, no close button is in the tree, and the tab's context menu lists only the four moves. Delete on the focused tab works and opens the confirmation, but nothing on the tab tells the reader that it exists.
- **Measured again:** tabs-launch-tree 2 of 2, tabs-context-menu 2 of 2, tabs-close-keys 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-launch-tree 'read the tab strip at launch': FAIL a closable tab offers a close route on AT-SPI; [page tab] 'Doc 1' desc='Document #1' ... actions=['click'] rel=None children=0; close buttons in the tree: none`
  - `pass2 tabs-launch-tree 'read the tab strip at launch': same, actions=['click'], close buttons in the tree: none`
  - `pass1 tabs-context-menu 'the context-menu key on Doc 2': FAIL the menu offers closing the tab; Move Left / Move Right / Move to Start / Move to End`
  - `pass2 tabs-close-keys 'Delete on Doc 1': +31.1 ms object:announcement [alert] 'Close tab?' text='Close tab?'; +36.7 ms object:state-changed:focused 1 [push button] 'No'`
  - `accesskit_atspi_common-0.21.0, accesskit_windows-0.35.1, accesskit_macos-0.27.1, accesskit_unix-0.24.0: no match for custom_action or CustomAction in src`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/header.rs:611-627, 1084-1101, 1273-1285
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** Welcome, the first tab, becomes selected and focused, and its panel is shown. The reader hears 'Welcome page tab.', far from where they were working.
- **Platform:** All platforms (selection logic). Linux measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Closing the selected Doc 1 still selects and focuses Welcome, the first tab, and the reader hears 'Welcome page tab.', not Doc 2 next to the closed tab.
- **Measured again:** tabs-close 2 of 2, tabs-close-keys 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-close-keys 'Space on Yes closes Doc 1': +246.1 ms object:state-changed:focused 1 [page tab] 'Welcome'; +667.9 ms ORCA SAYS: 'Welcome page tab.'; FAIL the one selected page tab is 'Doc 2' / selected page tabs: ['Welcome']`
  - `pass2 tabs-close-keys 'Space on Yes closes Doc 1': +40.8 ms object:state-changed:focused 1 [page tab] 'Welcome'; +122.0 ms ORCA SAYS: 'Welcome page tab.'; selected page tabs: ['Welcome']`
  - `pass1 tabs-close 'answer Yes (AT-SPI click)': +340.0 ms object:state-changed:focused 1 [page tab] 'Welcome'; +1027.5 ms ORCA SAYS: 'Welcome page tab.'`
  - `pass2 tabs-close 'answer Yes (AT-SPI click)': +58.8 ms object:state-changed:focused 1 [page tab] 'Welcome'; +160.6 ms ORCA SAYS: 'Welcome page tab.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/bar.rs:559, 1187-1195; crates/teksilo-widgets/src/tab\_widget.rs:1395-1407
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** Each of these rebuilds the whole TabBar: a new page tab list with new tab nodes, the old one defunct. The adapter emits object:selection-changed for the new tab list, because it was added with a selected child. Orca answers a selection change on a tab list by moving its locus of focus to the selected tab and speaking it: the reader hears 'Welcome page tab.' while keyboard focus is still on the button, and Orca's where-am-I now reports Welcome. The new tab (Doc 4) is never mentioned. The same event at launch makes Orca start 'Welcome page tab.' before the window (cut by 'frame.').
- **Platform:** Linux AT-SPI / Orca measured (Orca behaviour, Teksilo trigger). Windows/macOS not measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Enter on '+ New tab' or Orient, or a screen reader's click on '+ New tab' or Sizing, still rebuilds the tab bar and Orca says 'Welcome page tab.' and moves its locus to Welcome while keyboard focus stays on the button. Space no longer does this: Space now reaches Orca, which ignores a selection change after Space, so Space on '+ New tab', Orient or Sizing is silent. Either way the new tab (Doc 4) is never mentioned.
- **Measured again:** verify-tabs-activate-other 2 of 2 (Enter and AT click routes); tabs-overflow, tabs-orient and tabs-dropdown 2 of 2 each (Space route, now silent)
- **Evidence (`c198e4d1`):**
  - `pass1 verify-tabs-activate-other 'Enter on '+ New tab'': +36.1 ms object:selection-changed [page tab list] ''; +84.3 ms ORCA SAYS: 'Welcome page tab.'; orca-debug.out 16:49:10.485643 - FOCUS MANAGER: Changing locus of focus from [push button: '+ New tab'] to [page tab: 'Welcome']. Notify: True`
  - `pass1 verify-tabs-activate-other 'Enter on Orient': +50.4 ms object:selection-changed [page tab list] ''; +130.5 ms ORCA SAYS: 'Welcome page tab.'`
  - `pass2 verify-tabs-activate-other 'AT-SPI click on Sizing': +38.5 ms object:selection-changed [page tab list] ''; +91.8 ms ORCA SAYS: 'Welcome page tab.'`
  - `pass2 verify-tabs-activate-other 'AT-SPI click on '+ New tab' (Orca's locus left where the last act put it)': +126.8 ms object:selection-changed [page tab list] ''; no speech (Orca: selection redundant)`
  - `pass1 tabs-overflow 'Space on '+ New tab'': +37.4 ms object:selection-changed [page tab list] ''; FAIL Orca says something / Orca said nothing in this act`
  - `pass2 tabs-orient 'Space on Orient': +42.1 ms object:selection-changed [page tab list] ''; pass Orca does not present a tab while focus stays on Orient`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget.rs:1395-1407
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** Focus lands on an unnamed list box and Orca says only 'list box.' Down produces no focus or active-descendant event and no speech, and Enter does nothing. Tab closes the popup and moves on to Sizing. No entry marks the current tab. The trigger advertises HasPopup::Menu, but the popup is a list box (Windows publishes haspopup=menu). Only an AT-SPI click on an entry's button works.
- **Platform:** Linux AT-SPI / Orca measured; the keyboard behaviour is platform-independent by source.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Space on 'Show all tabs' puts focus on an unnamed list box and Orca says only 'list box.' Down gives no event and no speech, Enter does nothing (Orca echoes only 'return'), Tab closes the popup and goes to Sizing, and no entry marks the current tab. The trigger still says it opens a menu.
- **Measured again:** tabs-dropdown, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-dropdown 'Space on 'Show all tabs'': +163.6 ms object:state-changed:focused 1 [list box] ''; +753.8 ms ORCA SAYS: 'list box.'`
  - `pass1 tabs-dropdown 'Down in the dropdown': FAIL Orca says something / Orca said nothing in this act; FAIL focus lands on [list item] '*' / no focus change on the bus in this act`
  - `pass2 tabs-dropdown 'Enter in the dropdown': +6.4 ms ORCA SAYS: 'return'; FAIL focus lands on [page tab] '*' / no focus change on the bus in this act`
  - `pass2 tabs-dropdown 'Tab in the dropdown': +15.7 ms object:state-changed:focused 1 [push button] 'Sizing'; +71.9 ms ORCA SAYS: 'Sizing push button.'`
  - `pass1 tabs-dropdown tree after opening: [list box] '' {focusable,focused,vertical} > [list item] 'Welcome' {selectable} ... [list item] 'Doc 7' {selectable} (none selected)`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/bar.rs:2802-2874; crates/teksilo-widgets/src/list\_view.rs:159; crates/teksilo-widgets/src/list\_view/widget\_impl.rs:498-507, 588
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** Focus goes to the unnamed menu container and Orca says 'menu.' Down produces no event and no speech, and no item ever carries focused or selected state. Enter then runs an item the reader never heard (here Move Left: 'Doc 2 moved to 4 of 6'). The same happens on tab-migration (Alpha's menu).
- **Platform:** All platforms by source (no active descendant, no item focus); Linux measured.
- **Severity:** low; **layer:** framework
- **Severity in the sweep:** high. The reader now hears every item they move to and Enter runs the one they heard; only the bare 'menu.' on opening remains.
- **Status:** Partly fixed by `9636094c` (menus). What remains is under **Now**.
- **Now (`c198e4d1`):** Moving through the menu is now heard: Down focuses 'Move Left' and Orca says 'Move Left.', and Enter runs the item the reader heard. What remains is the opening: the menu key or Shift+F10 puts focus on the unnamed menu itself, Orca says only 'menu.', and no item is current until the first arrow.
- **Measured again:** tabs-context-menu 2 of 2, tabs-migration-move 2 of 2, fix-menus-context 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-context-menu 'the context-menu key on Doc 2': +96.8 ms object:state-changed:focused 1 [menu] ''; +406.8 ms ORCA SAYS: 'menu.'; FAIL focus lands on [menu item] '*'`
  - `pass2 tabs-context-menu 'Shift+F10 on Doc 2': +32.7 ms object:state-changed:focused 1 [menu] ''; +80.6 ms ORCA SAYS: 'menu.'`
  - `pass2 tabs-context-menu 'Down in the menu': +13.8 ms object:state-changed:focused 1 [menu item] 'Move Left'; +50.0 ms ORCA SAYS: 'Move Left.'`
  - `pass1 tabs-context-menu 'Down in the menu': +48.3 ms object:state-changed:focused 1 [menu item] 'Move Left'; +309.1 ms ORCA SAYS: 'Move Left.'`
  - `pass2 tabs-migration-move 'the context-menu key on Alpha': +19.9 ms object:state-changed:focused 1 [menu] ''; +50.1 ms ORCA SAYS: 'menu.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/menu\_list.rs:596-603 (the highlight starts empty), 878-892 (Down highlights the first row), 1121-1156 (the highlighted row is the menu's active descendant)
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** The menu offers only 'Move Right' and 'Move to End' inside group A. Alt+End moves Alpha to the end of group A. A further Alt+Right does nothing and says nothing: the order is unchanged and Orca is silent.
- **Platform:** All platforms (logic); Linux measured.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Alpha's context menu offers only 'Move Right' and 'Move to End', inside group A. Alt+End moves Alpha to the end of group A and is now heard ('Alpha moved to 3 of 3'), but a further Alt+Right does nothing and says nothing. There is still no keyboard or AT route to Group B.
- **Measured again:** tabs-migration-move, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-migration-move 'the context-menu key on Alpha': FAIL the menu offers moving the tab to the other group / menu items: ['Move Right', 'Move to End']`
  - `pass2 tabs-migration-move 'the context-menu key on Alpha': menu items: ['Move Right', 'Move to End']`
  - `pass1 tabs-migration-move 'Alt+Right on Alpha at the end of group A': FAIL Orca says something / Orca said nothing in this act; pass the page tabs read in the order ['Bravo', 'Charlie', 'Alpha', 'Xeno', 'Yotta']`
  - `pass2 tabs-migration-move 'Alt+Right on Alpha at the end of group A': Orca said nothing in this act; order unchanged`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/bar.rs:1367-1417; crates/teksilo-widgets/src/common/ordered\_move.rs:443-460; crates/teksilo-widgets/src/tab\_widget/header.rs:821-823
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** Both page tab lists are unnamed, and Orca says only 'Alpha page tab.' and 'Xeno page tab.' TabWidget and TabBar offer no way to name the TabList. An .access\_label on the TabWidget lands on its Role::GenericContainer, which the consumer's filter removes. The example's 'Group A' heading is a sibling label.
- **Platform:** All platforms (tree); Linux measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Both tab lists are still unnamed, and a reader landing on a tab hears only 'Alpha page tab.' or 'Xeno page tab.', with nothing to tell Group A from Group B.
- **Measured again:** tabs-migration-tree 2 of 2, tabs-migration-walk 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-migration-tree 'read the two groups at launch': FAIL each tab list has a name that tells the two groups apart; [page tab list] '' desc=None states=['enabled', 'horizontal', 'sensitive', 'showing', 'visible'] (twice)`
  - `pass2 tabs-migration-tree: same, two unnamed page tab lists`
  - `pass2 tabs-migration-walk 'Tab to group A's strip': +103.4 ms ORCA SAYS: 'Alpha page tab.'; FAIL Orca says 'Group A'`
  - `pass2 tabs-migration-walk 'Tab to group B's strip': +80.9 ms ORCA SAYS: 'Xeno page tab.'; FAIL Orca says 'Group B'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/bar.rs:2199-2218
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** Locked carries enabled and sensitive. 'selectable' is absent, which shows the adapter saw is\_disabled, so the flag reached AccessKit. Orca says 'Locked page tab.' plus the example's own tooltip 'Disabled tabs cannot be activated.', which is all that tells this reader it is unavailable; a disabled tab without such a tooltip reads as available. The tab also accepts AT focus.
- **Platform:** Linux only. By source, Windows publishes IsEnabled = !is\_disabled (accesskit\_windows node.rs:535) and macOS isAccessibilityEnabled = !is\_disabled (accesskit\_macos node.rs:658-660).
- **Severity:** medium; **layer:** upstream
- **Status:** Fixed by `d217ee62` (accesskit-update).
- **Now (`c198e4d1`):** Locked is no longer published as enabled or sensitive, and when focus lands on it Orca says 'Locked page tab grayed.' before the example's tooltip. The tab still accepts a screen reader's focus request, which ARIA allows for a disabled tab.
- **Measured again:** tabs-launch-tree 2 of 2, tabs-at-activation 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-launch-tree 'read the tab strip at launch': pass a disabled tab (Locked) is not reported enabled/sensitive`
  - `pass2 tabs-at-activation tree: [page tab] 'Locked' desc='Disabled tabs cannot be activated' {focusable} attrs={'setsize': '6', 'posinset': '3'}`
  - `pass1 tabs-at-activation 'AT-SPI grab_focus on the disabled Locked tab': +12.5 ms object:state-changed:focused 1 [page tab] 'Locked'; +45.9 ms ORCA SAYS: 'Locked page tab grayed.'; +45.9 ms ORCA SAYS: 'Disabled tabs cannot be activated.'`
  - `pass2 tabs-at-activation 'AT-SPI grab_focus on the disabled Locked tab': +79.2 ms ORCA SAYS: 'Locked page tab grayed.'; pass the reader is told Locked is unavailable`
  - `accesskit_atspi_common-0.21.0/src/node.rs:376-382 inserts Enabled|Sensitive only when the node is not disabled`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/header.rs:1216-1222; accesskit\_atspi\_common-0.21.0/src/node.rs:376-382
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** The page tab list's children are a label ' Showcase ', the pinned Welcome tab, an unnamed panel (the ScrollArea) holding the other five tabs, and the bar-slot controls (Sizing, Orient, Theme combo box, '+ New tab'). Every one of these, and the panel, carries setsize=6. With Orca's position speaking on, its sibling count would give Welcome '2 of 7' and Settings '1 of 5' instead of the published 1 and 2 of 6. The overflow popup is also mounted inside the tab list.
- **Platform:** AT-SPI and UIA both resolve setsize from the nearest ancestor with size\_of\_set (atspi\_common node.rs:397-400; accesskit\_windows node.rs:689-691), so every descendant gets it on Linux and Windows; macOS publishes neither. Linux measured.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The page tab list still holds a ' Showcase ' label, the pinned Welcome tab, an unnamed panel with the other five tabs, and Sizing, Orient, Theme and '+ New tab', every one with setsize=6. The 'Show all tabs' popup is still mounted inside the tab list.
- **Measured again:** tabs-launch-tree 2 of 2, tabs-dropdown 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-launch-tree 'read the tab strip at launch': FAIL the tab list holds only page tabs; [label] ' Showcase ' attrs={'setsize': '6'} / [panel] '' attrs={'setsize': '6'} / [push button] 'Sizing' / [push button] 'Orient' / [combo box] 'Theme' / [push button] '+ New tab', each attrs={'setsize': '6'}`
  - `pass2 tabs-launch-tree: FAIL every page tab is a direct child of the tab list / direct page-tab children: ['Welcome'] / page tabs nested under another node: ['Settings', 'Locked', 'Doc 1', 'Doc 2', 'Doc 3']`
  - `pass1 tabs-dropdown tree after 'Space on 'Show all tabs'': [page tab list] '' > [panel] '' attrs={'setsize': '10'} > [list box] ''`
  - `accesskit_atspi_common-0.21.0/src/node.rs:399-403 and accesskit_windows-0.35.1/src/node.rs:689-691 still take setsize from the nearest ancestor that has one (accesskit_consumer-0.39.1/src/node.rs:629-641)`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/bar.rs:1619-1760, 2199-2218
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** With 10 tabs, only 8 page tabs are in the tree (Doc 6 and Doc 7 are missing) while each carries setsize=10. After End, the earlier tabs leave the tree and are marked defunct. On return, Orca drops the focus event from the returning Settings tab as defunct. It still speaks 'Settings page tab.', but only because the tab list's selection-changed (automatic activation) makes it present the selected tab: the same mechanism as tabs-01, rescued here by selection.
- **Platform:** Linux AT-SPI / Orca measured; the clip filter is in accesskit\_consumer and applies to every adapter's tree.
- **Severity:** low; **layer:** upstream
- **Status:** Partly fixed by `85624a1a` (node-ids). What remains is under **Now**.
- **Now (`c198e4d1`):** The defunct part is gone: a tab that scrolls back into view is heard normally, and Orca ignores none of its events. Tabs outside the visible strip still leave the tree: with ten tabs, Doc 6 and Doc 7 are missing until the strip scrolls, and after End the earlier tabs leave in turn, so object navigation and flat review cannot find every tab.
- **Measured again:** tabs-overflow 2 of 2, tabs-overflow-arrows 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-overflow 'three more new tabs: the strip overflows': FAIL the tree holds [page tab] 'Doc 7' / no such node in the tree after the act`
  - `pass2 tabs-overflow 'three more new tabs: the strip overflows': FAIL the tree holds [page tab] 'Doc 7'`
  - `pass1 tabs-overflow-arrows 'End: to Doc 7, scrolled out of view': +28.5 ms object:children-changed:add [panel] '' -> [page tab] 'Doc 6'; +29.3 ms object:children-changed:add [panel] '' -> [page tab] 'Doc 7'; +33.5 ms object:state-changed:defunct 1 [page tab] 'Settings'`
  - `pass1 tabs-overflow-arrows 'Right: to Settings (scrolled back into view)': +24.7 ms object:state-changed:focused 1 [page tab] 'Settings'; +199.1 ms ORCA SAYS: 'Settings page tab.'; pass the node focus lands on was not announced defunct earlier in the run; pass Orca's log lines with ('Ignoring defunct',)`
  - `pass2 tabs-overflow-arrows 'Right: to Settings (scrolled back into view)': +25.5 ms object:state-changed:focused 1 [page tab] 'Settings'; +139.5 ms ORCA SAYS: 'Settings page tab.'; not announced defunct earlier`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/bar.rs (headers inside a clipping ScrollArea); accesskit\_consumer-0.39.1/src/filters.rs:65-87 (the clip filter, unchanged from 0.39.0); crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-187 (new ids for a returning tab)
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** desc='Welcome', which repeats the name (Orca does not speak it), and the author's text is lost for sighted hover too.
- **Platform:** All platforms; Linux measured.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The pinned Welcome tab's description is still 'Welcome', its own name, instead of its declared tooltip 'Welcome — start here'.
- **Measured again:** tabs-launch-tree, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-launch-tree 'read the tab strip at launch': FAIL the pinned tab keeps its declared tooltip 'Welcome — start here' as its description; [page tab] 'Welcome' desc='Welcome'`
  - `pass2 tabs-launch-tree: [page tab] 'Welcome' desc='Welcome' states=['enabled', 'focusable', 'selectable', 'selected', 'sensitive', 'showing', 'visible']`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/header.rs:559-561
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** Ctrl+Tab behaves as a plain Tab (wraps to the Settings tab, still selected); Ctrl+PageDown does nothing.
- **Platform:** All platforms; Linux measured.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** From inside the Settings panel, Ctrl+Tab behaves as Tab and lands on the Settings tab ('Settings page tab.'), with Settings still selected, and Ctrl+PageDown does nothing (Orca echoes only the keys).
- **Measured again:** tabs-panel, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabs-panel 'Ctrl+Tab inside the panel': +26.9 ms object:state-changed:focused 1 [page tab] 'Settings'; +79.2 ms ORCA SAYS: 'Settings page tab.'; FAIL the one selected page tab is 'Doc 1' / selected page tabs: ['Settings']`
  - `pass2 tabs-panel 'Ctrl+PageDown inside the panel': +10.6 ms ORCA SAYS (CUT): 'left control'; +27.1 ms ORCA SAYS: 'page down'; FAIL the one selected page tab is 'Doc 1' / selected page tabs: ['Settings']`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget.rs (no key handler on TabWidget); crates/teksilo-core/src/widget\_tree/pointer\_router.rs:1109-1144
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** On the 4th press the arrow leaves the tree while it holds focus. Focus goes to the unnamed window frame and Orca says 'frame.' The next Tab starts over at Welcome, so the reader has lost their place. An arrow that comes back later reuses its old id (the tabs-01 mechanism), so focusing it again would also be silent (by mechanism, not measured).
- **Platform:** Linux AT-SPI / Orca measured; the focus loss is platform-independent by source
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** On the fourth press the focused 'Scroll tabs right' leaves the tree while it holds focus. Focus falls to the window and Orca says 'TabWidget — Showcase frame.', and the next Tab starts over at Welcome, so the reader loses their place.
- **Measured again:** verify-tabs-scroll-arrow-hide, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 verify-tabs-scroll-arrow-hide 'Space on the focused 'Scroll tabs right', press 4': +282.3 ms object:children-changed:remove [page tab list] '' -> [push button] 'Scroll tabs right'; +285.3 ms object:state-changed:focused 1 [frame] 'TabWidget — Showcase'; +701.8 ms ORCA SAYS: 'TabWidget — Showcase frame.'`
  - `pass2 verify-tabs-scroll-arrow-hide 'Space on the focused 'Scroll tabs right', press 4': +210.9 ms object:children-changed:remove [page tab list] '' -> [push button] 'Scroll tabs right'; +212.7 ms object:state-changed:focused 1 [frame] 'TabWidget — Showcase'; +278.5 ms ORCA SAYS: 'TabWidget — Showcase frame.'`
  - `pass2 verify-tabs-scroll-arrow-hide 'Tab from where focus is now': +38.1 ms object:state-changed:focused 1 [page tab] 'Welcome'; +74.3 ms ORCA SAYS: 'Welcome page tab.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/bar.rs:1699-1741
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** Focus returns to 'Show all tabs' (spoken, cut), then the tab list's selection-changed moves Orca's locus to Doc 6 and Orca says 'Doc 6 page tab.'. Keyboard focus is still on the trigger: Tab goes to Sizing, not into Doc 6's panel. The reader believes they are on Doc 6.
- **Platform:** Linux AT-SPI / Orca measured. The focus restore is platform-independent; the locus move is Orca's onSelectionChanged, which runs here because no Space was pressed.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Picking Doc 6 from 'Show all tabs' still selects Doc 6 but returns keyboard focus to the trigger, and Tab goes on to Sizing. When the dropdown was opened with Enter, Orca then moves its locus to Doc 6 and says 'Doc 6 page tab.', so the reader believes they are on Doc 6. When it was opened with Space, Orca stays on the trigger and says only 'Show all tabs push button.', and the reader is not told Doc 6 was chosen.
- **Measured again:** verify-tabs-dropdown-realistic 2 of 2 (Space route); remeasure-tabs-dropdown-enter 1 of 1 (Enter route)
- **Evidence (`c198e4d1`):**
  - `pass1 verify-tabs-dropdown-realistic 'AT-SPI click on the 'Doc 6' entry': +486.6 ms object:state-changed:focused 1 [push button] 'Show all tabs'; +580.0 ms object:selection-changed [page tab list] ''; +1608.7 ms ORCA SAYS: 'Show all tabs push button.'; pass the one selected page tab is 'Doc 6'; FAIL focus lands on [page tab] 'Doc 6'`
  - `pass2 verify-tabs-dropdown-realistic 'AT-SPI click on the 'Doc 6' entry': +80.4 ms object:state-changed:focused 1 [push button] 'Show all tabs'; +254.0 ms ORCA SAYS: 'Show all tabs push button.'; 'Tab from there': +87.2 ms ORCA SAYS: 'Sizing push button.'`
  - `judge remeasure-tabs-dropdown-enter-20260927-175822-55094 'AT-SPI click on the 'Doc 6' entry' (opened with Enter): +109.9 ms object:state-changed:focused 1 [push button] 'Show all tabs'; +127.5 ms object:selection-changed [page tab list] ''; +808.8 ms ORCA SAYS (CUT): 'Show all tabs push button.'; +958.4 ms ORCA SAYS: 'Doc 6 page tab.'; orca-debug.out 17:58:36.749728 - FOCUS MANAGER: Changing locus of focus from [push button: 'Show all tabs'] to [page tab: 'Doc 6']. Notify: True`
  - `same run 'Tab from there': +33.4 ms object:state-changed:focused 1 [push button] 'Sizing'; +310.5 ms ORCA SAYS: 'Sizing push button.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/bar.rs:2826-2836
- **Evidence (`261a218f`):**
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
- **The reader got (`261a218f`):** No focus change and no speech. Tab goes on to Sizing, and the Welcome text is never a Tab stop. The same holds for Locked's panel. The text can only be reached through Orca's own review commands.
- **Platform:** All platforms (logic); Linux measured
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Enter on Welcome, whose panel has nothing focusable, moves nothing and says nothing (Orca echoes only 'return'), and Tab goes on to Sizing, so the Welcome text is never a Tab stop.
- **Measured again:** verify-tabs-enter-empty-panel, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 verify-tabs-enter-empty-panel 'Enter on Welcome (its panel has no focusable control)': +6.3 ms ORCA SAYS: 'return'; no focus change on the bus`
  - `pass2 verify-tabs-enter-empty-panel 'Enter on Welcome (its panel has no focusable control)': +7.7 ms ORCA SAYS: 'return'`
  - `pass2 verify-tabs-enter-empty-panel 'Tab from there': +15.0 ms object:state-changed:focused 1 [push button] 'Sizing'; +82.1 ms ORCA SAYS: 'Sizing push button.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/tab\_widget/header.rs:943-966; crates/teksilo-widgets/src/tab\_widget/info.rs:73-75
- **Evidence (`261a218f`):**
  - `verify-tabs-enter-empty-panel-20260925-143911-753707 act 'Enter on Welcome (its panel has no focusable control)': no events, Orca said nothing; act 'Tab from there': ORCA SAYS 'Sizing push button.'`
  - `crates/teksilo-widgets/src/tab_widget/header.rs:943-966: Enter/Space call ctx.request_focus_into(panel_id), a documented no-op when the panel has nothing focusable`
  - `crates/teksilo-widgets/src/tab_widget/info.rs:73-75 TabInfo::focusable_panel exists (default false); examples/tab_widget/src/main.rs:206-233 does not set it for its text-only Welcome and Locked panels`
- **Reproduced:** 2 of 2 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** The example should set .focusable\_panel(true) on Welcome and Locked. The framework could make a content-less panel focusable by default, or have Enter fall back to focusing the panel node.
