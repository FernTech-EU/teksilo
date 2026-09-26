<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Docking

Examples: `docking`.
20 findings: 3 critical, 7 high, 5 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [docking-01](#docking-01) | docking | After a side is hidden and shown again, every control Orca had met before is silent: its focus and value events are dropped as defunct | critical | Linux | fixed |
| [docking-02](#docking-02) | docking | Hiding a side while focus is inside it drops focus onto the unnamed window frame | high | Linux | open |
| [docking-03](#docking-03) | docking | Every structural change rebuilds all sides, and Orca then reads the untouched sides' selected tabs over the real focus and moves its locus of focus onto them | medium | Linux | open |
| [docking-04](#docking-04) | docking | After Hide / Move from a menu, focus lands on the first focusable node of the rebuilt layout, often in another side | high | Linux | open |
| [docking-05](#docking-05) | docking | Dock resize handles have no accessible name: Orca says 'vertical splitter 20.' | high | Linux | open |
| [docking-06](#docking-06) | docking | Home on a resize handle hides the side and then strands focus on a disabled handle: End/Enter can no longer bring the side back, while AT-SPI still reports it enabled with its old value | high | Linux | open |
| [docking-07](#docking-07) | docking | Showing/hiding a side and collapsing/expanding a dock pane are never spoken: no expanded state reaches AT-SPI, and nothing else says it | high | Linux | upstream |
| [docking-08](#docking-08) | docking | Every dock's options button is announced as just 'More actions': the dock name sits on an unknown-role wrapper, not on the button | medium | Linux | open |
| [docking-09](#docking-09) | docking | Keyboard resizing is never heard: Orca speaks only 'vertical splitter' on each arrow, and a named divider's value is never spoken at all | medium | Linux | upstream |
| [docking-10](#docking-10) | docking | In the dock context and options menus, arrow keys make no event and no speech: the reader hears 'menu.' and never an item | critical | Linux | fixed |
| [docking-11](#docking-11) | docking | The 'Move to' / 'Move to side' submenus close themselves about 170 ms after Right opens them, so the keyboard alternative to drag-to-dock cannot pick a side | critical | Linux | fixed |
| [docking-12](#docking-12) | docking | A dock accordion's push button is the whole pane: header actions and the dock's content region are its children | medium | Linux | open |
| [docking-13](#docking-13) | docking | Tab order and structure: a side's content comes before the rail that controls it; strip tab lists are unnamed | low | Linux | open |
| [docking-14](#docking-14) | docking | The example's Toggle Sidebar / Panel / Inspector and Lock Layout are plain buttons: toggling a side or the lock is silent and stateless | medium | Linux | open (example) |
| [docking-15](#docking-15) | docking | Restore renames the 'Source' activity to 'Explorer' | low | all | open |
| [docking-16](#docking-16) | docking | Activating another rail activity (Enter on a non-selected rail tab) is silent | low | Linux | upstream |
| [docking-17](#docking-17) | docking | The rail's decorative top-slot glyph '◆' is exposed as a label | low | all | open (example) |
| [docking-M1](#docking-m1) | docking | A dock's options menu (and a submenu) is silent on every opening after the first: its content comes back with the ids the adapter already declared defunct | high | Linux | fixed |
| [docking-M2](#docking-m2) | docking | A dock header's dropdown action ('More' with a menu) moves focus to an unnamed, unknown-role wrapper instead of the menu: Orca stops speech and says nothing | high | Linux | open |
| [docking-M3](#docking-m3) | docking | The example's status line is not a live region: header actions, Export and Restore report their result only there, and a reader hears nothing | low | Linux | open (example) |

### docking-01 {#docking-01}

After a side is hidden and shown again, every control Orca had met before is silent: its focus and value events are dropped as defunct

- **Example:** docking
- **Act:** docking-reshown-visited: focus the Explorer header (Orca says it), Enter on the Source rail tab (hide), Enter (show), Shift+Tab back through the side to Explorer. docking-reshown-events: focus the Terminal\|Problems divider and press Right, Toggle Panel twice, focus the divider again and press Right.
- **The reader should get:** Controls in a side that was hidden and shown again are announced on focus and their value changes are spoken, exactly as before the hide.
- **The reader gets:** Controls Orca never met before the hide still speak (More actions, Search, Splitter divider, New File all spoken). The Explorer header, which Orca met before the hide, gets focus on the bus and Orca says nothing: 'Ignoring defunct object'. Same for the bottom divider: after Toggle Panel twice, focusing it and pressing Right both produce silence. Hiding a side parks its DockSidePanel dormant. The adapter removes every node and marks it defunct. Showing the side re-adds the same node ids, which libatspi and Orca still hold as defunct (Orca sees the re-shown 'Bottom panel' landmark as states='defunct, enabled, ...'). So a user who works in a panel, hides it and brings it back cannot hear the controls they used. The same thing happens whenever an activity's content is swapped out and back.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows/macOS not measured: their adapters have no defunct state, so this mechanism is AT-SPI specific.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: same mechanism, covered by the general translation and the core tests, not re-run through the harness in this session.
- **Where:** crates/teksilo-widgets/src/docking.rs:311 (visible\_when parks the DockSidePanel dormant; ids reused on reactivation). The same reuse happens for the rail/strip activity switch, and for popover menus at crates/teksilo-widgets/src/popover\_widget.rs:672-695 (see missed M1). accesskit\_atspi\_common-0.20.0/src/adapter.rs:91-112 (remove\_node emits Defunct and cache-removed); Orca event\_manager.py:798 drops the event.
- **Evidence:**
  - `docking-reshown-visited (3 of 3 runs): '== Shift+Tab to Explorer  (focus moves to the Explorer header, which Orca met before the hide, and the reader hears it)'`
  - `'        +29.9 ms object:state-changed:focused 1 [push button] 'Explorer''`
  - `'       FAIL  Orca says 'Explorer'' / '             Orca unheard: 'Explorer''`
  - `orca-debug.out (docking-reshown-visited-20260925-142945-531170): '14:30:29.444223 - EVENT MANAGER: Ignoring defunct object: [push button: 'Explorer']' (the same line once in each of the 3 runs)`
  - `same runs, never-met controls after the re-show: 'Shift+Tab to More actions said: landmark Leading panel | More actions push button.', 'Shift+Tab to Search said: Search push button.', 'Shift+Tab to New File said: Explorer actions tool bar | New File push button.'`
  - `docking-reshown-events (3 of 3 runs): '== Right on the divider (after hide and show)' '        +18.8 ms object:property-change:accessible-value [separator] 'Splitter divider'' '       FAIL  Orca says something' '             Orca said nothing in this act' '             14:25:46.498316 EVENT MANAGER: Ignoring defunct object: [separator: 'Splitter divider']'; before the hide, the same key gave 'said: vertical splitter'`
  - `docking-toolbar-toggles orca-debug.out: re-shown landmark read by Orca as "name='Bottom panel' role='landmark' ... states='defunct, enabled, sensitive, showing, visible'", then '14:15:38.433477 - EVENT MANAGER: Ignoring defunct object: [page tab list]'`
  - `hide (docking-rail-toggle): '       +210.4 ms object:children-changed:remove [frame] '' -> [landmark] 'Leading panel'' ... '       +212.6 ms object:state-changed:defunct 1 [push button] 'Explorer''; show: '        +46.9 ms object:children-changed:add [frame] '' -> [landmark] 'Leading panel'' (same ids, no fresh nodes)`
  - ``source: crates/teksilo-widgets/src/docking.rs:311 `ctx.visible_when(panel, progress.map(|p| *p > COLLAPSED_EPS))`; node ids derive from widget ids (widget_id_to_node_id), so dormant->active re-adds the same ids; accesskit_atspi_common-0.20.0/src/adapter.rs:91-112 remove_node emits StateChanged(Defunct, true); Orca event_manager.py:798 'Ignoring defunct object'``
  - `V=target/reader-verify/docking`
  - `V/docking-reshown-visited-20260925-144103-787596/report.txt: '== Shift+Tab to Search (... never met ...)' '+26.5 ms object:state-changed:focused 1 [push button] 'Search'' 'FAIL  Orca says 'Search'' '14:41:30.717454 EVENT MANAGER: Ignoring defunct object: [push button: 'Search']'; also 14:41:26.798529 [push button: 'More actions'], 14:41:34.641781 [separator: 'Splitter divider'], 14:41:46.409696 [push button: 'Explorer']`
  - `V/docking-reshown-visited-20260925-144249-787596 and -144436-787596: never-met stops spoken ('+158.5 ms ORCA SAYS: 'More actions push button.''), Explorer dropped ('14:43:33.395770 ... Ignoring defunct object: [push button: 'Explorer']', '14:45:20.418386 ...')`
  - `V/docking-reshown-events-20260925-144159-787596: before hide 'Right on the divider' -> '+41.5 ms ORCA SAYS: 'vertical splitter''; after hide+show '+27.4 ms object:property-change:accessible-value [separator] 'Splitter divider'' 'Orca said nothing in this act' '14:42:31.588295 EVENT MANAGER: Ignoring defunct object: [separator: 'Splitter divider']'; same in -144346 and -144533`
  - `V/docking-reshown-events-20260925-144159-787596/orca-debug.out: "name='Bottom panel' role='landmark' ... states='defunct, enabled, sensitive, showing, visible'"; '14:42:23.800392 EVENT MANAGER: Ignoring defunct object: [page tab list]'`
  - `V/verify-docking-reshown-persist-20260925-144855-932995: Explorer visits 1-3 'FAIL  Orca says 'Explorer'' ('14:49:21.208044', '14:49:28.685388', '14:49:36.190729 ... Ignoring defunct object: [push button: 'Explorer']'); Search visit 1 '+153.9 ms ORCA SAYS: 'Search push button.'', visits 2 and 3 'FAIL  Orca says 'Search''. Same pattern in -145607-1078635`
  - `V/verify-docking-activity-switch-20260925-144749-932995: 'Enter on Properties: switch the rail to Properties' '+20.0 ms object:children-changed:remove [landmark] 'Leading panel' -> [scroll pane] 'Source''; later 'focus the Explorer header (met before the switch)' '+39.3 ms object:state-changed:focused 1 [push button] 'Explorer'' 'FAIL  Orca says 'Explorer'' '14:48:38.533222 EVENT MANAGER: Ignoring defunct object: [push button: 'Explorer']'; same in -145200-932995 (14:52:48.452711) and -145532-1148195`
- **Reproduced:** 3 of 3 runs (reshown-visited: Explorer silent every time, never-met controls spoken every time); 3 of 3 runs (reshown-events: divider focus and value change dropped every time); also seen in docking-toolbar-toggles 1 of 1
- **Verification:** confirmed. Reproduced: Met control silent after hide+show: 3 of 3 reshown-visited runs; divider focus and value change silent after hide+show: 3 of 3 reshown-events runs; 2 of 2 reshown-persist runs (3 revisits each, all silent); rail activity switch away and back: 3 of 3 activity-switch runs. Never-met controls also dropped in 1 of 3 reshown-visited runs.
- **Fix idea:** When a dormant subtree becomes active again, give its nodes AccessKit ids they have never had (for example, a per-activation generation folded into the node id), so the adapter announces them as new objects. AT-SPI has no way to take back a defunct state, so re-using an id cannot work.

### docking-02 {#docking-02}

Hiding a side while focus is inside it drops focus onto the unnamed window frame

- **Example:** docking
- **Act:** docking-hide-focused-side: focus the Explorer dock header, then AT-SPI click on Toggle Sidebar (a screen reader's activation, which leaves keyboard focus where it was).
- **The reader should get:** Focus moves to a live, named control that makes sense, such as the side's rail tab (the control that brings it back), and the reader hears it.
- **The reader gets:** Focus goes to the window root frame and Orca says just 'frame.'. The next Tab starts again at the first toolbar button. The same happens whenever the model hides a side that holds focus (set\_side\_visible from any external control).
- **Platform:** Linux AT-SPI / Orca (measured). Framework focus behaviour, so the same loss of focus on Windows/macOS (not measured).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/docking.rs:311 (no focus handoff before the panel is parked); crates/teksilo-core/src/widget\_tree.rs:2464-2490 (revalidate\_interaction\_state sets focus to None)
- **Evidence:**
  - `docking-hide-focused-side (3 of 3 runs): '       +224.5 ms object:state-changed:focusable 1 [frame] ''' '       +224.7 ms object:children-changed:remove [frame] '' -> [landmark] 'Leading panel'' '       +225.0 ms object:state-changed:focused 1 [frame] ''' '       +225.1 ms object:state-changed:focused 0 [push button] 'Explorer''`
  - `orca-debug.out (docking-hide-focused-side-20260925-141926-250821): '14:19:38.563037 - NULL SPEECH: stop', '14:19:38.563154 - SPEECH OUTPUT: 'frame.'', '14:19:38.564622 - EVENT MANAGER: Ignoring defunct object: [push button: 'Explorer']'`
  - `'       FAIL  the reader's focus is still on a live, named control' '             focus moved to [frame] '''`
  - `next act: 'Tab' -> '+8.5 ms object:state-changed:focused 1 [push button] 'Toggle Sidebar'' 'ORCA SAYS: 'Toolbar tool bar'' 'ORCA SAYS: 'Toggle Sidebar push button.''`
  - `source: crates/teksilo-widgets/src/docking.rs:311 parks the panel dormant, with no focus handoff; crates/teksilo-core/src/widget_tree.rs:2464 revalidate_interaction_state clears focus to None when the focused node is no longer active`
  - `V/docking-hide-focused-side-20260925-144136-796992: '+199.3 ms object:children-changed:remove [frame] '' -> [landmark] 'Leading panel'' '+199.5 ms object:state-changed:focused 1 [frame] ''' '+244.5 ms ORCA SAYS: 'frame.'' 'FAIL  the reader's focus is still on a live, named control'; same in -144341 and -144546`
  - `V/verify-docking-hide-focused-bottom-20260925-144958-932995: '+221.4 ms object:state-changed:focused 1 [frame] ''' '+387.5 ms ORCA SAYS: 'frame.''; -145708-1078635: '+325.6 ms ORCA SAYS: 'frame.''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs (leading side, Toggle Sidebar); 2 of 2 runs with the bottom side (focus on the Terminal header, AT-SPI click on Toggle Panel)
- **Fix idea:** When a side goes hidden while focus is inside it, move focus to the side's rail tab if it has one, else to its resize handle's replacement or the control that triggered the hide, before the subtree goes dormant. K1's fix only gives the frame a name; it does not stop focus being lost.

### docking-03 {#docking-03}

Every structural change rebuilds all sides, and Orca then reads the untouched sides' selected tabs over the real focus and moves its locus of focus onto them

- **Example:** docking
- **Act:** Space on Lock Layout / unlock / Restore (docking-lock-layout); Move to new activity (docking-promote, docking-rail-arrows setup); Hide "Properties" (docking-hide-dock); Move to -&gt; Leading (docking-move-tab); Hide "Source" and its restore (docking-hide-activity).
- **The reader should get:** Focus stays where it is (or lands where the action puts it) and the reader hears only that. Tabs in sides the user did not touch are not spoken.
- **The reader gets:** Any structural model change (set\_policy, import\_state, set\_tab\_hidden, move\_tab, promote\_to\_tab, ...) makes DockingLayout destroy and rebuild every side, so every tab list is re-added with new ids. The adapter queues a selection-changed for each re-added selected tab. Orca's onSelectionChanged then sets its locus of focus to each list's selected tab in turn and speaks it, cutting the real focus announcement. The reader's last words are 'Terminal page tab.' or 'Properties page tab.' while keyboard focus is on Lock Layout, on the new Explorer rail tab, or on New Property. Orca's where-am-I and review commands then start from the wrong control. At launch the same three utterances are cut by 'frame.'.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Orca-specific effect: NVDA/VoiceOver do not move focus on a selection event (not measured).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/docking.rs:213-221 (every model-derived side destroyed and rebuilt on any version bump; model.rs:506 notify). Upstream: accesskit\_atspi\_common-0.20.0/src/adapter.rs:79-81 and 269-277 (selection-changed for each newly added selected tab, emitted after the focus event, from a HashSet); Orca scripts/default.py:1540-1602 (onSelectionChanged sets the locus of focus)
- **Evidence:**
  - `docking-lock-layout-20260925-141812-250821 'Space on Lock Layout': '       +80.4 ms object:children-changed:remove [frame] '' -> [landmark] 'Leading panel'' ... '       +86.9 ms object:selection-changed [page tab list] ''' '       +87.3 ms object:selection-changed [page tab list] 'Leading activity bar'' '       +87.3 ms object:selection-changed [page tab list] '''`
  - `'     +159.9 ms ORCA SAYS (CUT): 'Properties page tab.'' '     +189.0 ms ORCA SAYS (CUT): 'Source page tab.'' '     +224.7 ms ORCA SAYS: 'Terminal page tab.'' with '       pass  focus stays where it was'`
  - `orca-debug.out: '14:18:24.372977 - AXObject: Clearing AT-SPI cache on [page tab: 'Properties'] Recursive: False. Reason: Setting locus of focus.' '14:18:24.374224 - FOCUS MANAGER: Locus of focus is [page tab: 'Properties']' ... '14:18:24.441704 - FOCUS MANAGER: Locus of focus is [page tab: 'Terminal']' '14:18:24.461994 - SPEECH OUTPUT: 'Terminal page tab.''`
  - `docking-promote-20260925-141250-184100: '       +249.4 ms object:state-changed:focused 1 [page tab] 'Explorer'' '     +338.5 ms ORCA SAYS (CUT): 'Explorer page tab.'' '     +386.3 ms ORCA SAYS (CUT): 'Properties page tab.'' '     +439.3 ms ORCA SAYS: 'Terminal page tab.''`
  - `docking-move-tab: '       +473.6 ms object:state-changed:focused 1 [push button] 'New Property'' ... '     +592.2 ms ORCA SAYS (CUT): 'New Property push button.'' '     +628.3 ms ORCA SAYS (CUT): 'Properties page tab.'' '     +664.6 ms ORCA SAYS: 'Terminal page tab.''`
  - `launch (tree-docking): '       +494.3 ms object:selection-changed [page tab list] 'Leading activity bar'' '     +543.6 ms ORCA SAYS (CUT): 'Source page tab.'' '     +574.7 ms ORCA SAYS (CUT): 'Properties page tab.'' '     +609.8 ms ORCA SAYS (CUT): 'Terminal page tab.'' '     +655.6 ms ORCA SAYS: 'frame.''`
  - ``source: crates/teksilo-widgets/src/docking.rs:221 `ctx.destroy_subtree(old_side)` for every side on each version bump; docking/model.rs:505-509 notify(); accesskit_atspi_common-0.20.0/src/adapter.rs:79-81 (add_node enqueues selection-changed for a selected node) and 268-277 (emitted after the focus event); Orca scripts/default.py:1580-1605 onSelectionChanged sets locus of focus to the selected child``
  - `V/docking-lock-layout-20260925-144120-794594: 'Space on Lock Layout' '+165.5 ms ORCA SAYS (CUT): 'Properties page tab.'' '+200.2 ms ORCA SAYS (CUT): 'Terminal page tab.'' '+227.6 ms ORCA SAYS: 'Source page tab.''; runs -144312 and -144505 give the same three in other orders`
  - `V/docking-lock-layout-20260925-144505-794594/orca-debug.out: '14:45:36.744607 - FOCUS MANAGER: Locus of focus is [page tab: 'Terminal']' ... '14:45:36.769561 - SPEECH OUTPUT: 'Terminal page tab.'' ... '14:45:36.789880 - FOCUS MANAGER: Locus of focus is [page tab: 'Properties']'`
  - `/usr/lib/python3/dist-packages/orca/scripts/default.py:1563-1566: 'keyString, mods = self.utilities.lastKeyAndModifiers()' 'if keyString == "space": return'`
  - `V/docking-move-tab-20260925-144452-796992: '+601.8 ms ORCA SAYS (CUT): 'New Property push button.'' '+648.4 ms ORCA SAYS (CUT): 'Terminal page tab.'' '+678.3 ms ORCA SAYS: 'Properties page tab.'' (here the last word is the moved tab)`
- **Reproduced:** Lock Layout: 3 of 3 runs, 9 of 9 rebuild acts (lock, unlock, restore) end on another side's tab; promote: 4 of 4 (3 runs plus the rail-arrows setup); hide-dock: 3 of 3; move-tab 1 of 1; hide-activity 2 of 2 acts
- **Verification:** corrected by the verifier. Reproduced: Lock Layout / unlock / Restore: 9 of 9 acts over 3 runs. Menu-driven changes: Hide 'Properties' 3 of 3, Hide 'Source' 2 of 2, Move to Leading 2 of 2, Move to new activity 2 of 2. Launch: every run. The mechanism is real and I confirmed the source. Every rebuild re-adds the three tab lists. The adapter emits a selection-changed for each one after the focus event, and Orca's onSelectionChanged moves its locus to each selected tab and speaks it: 'FOCUS MANAGER: Locus of focus is \[page tab: 'Terminal'\]' in lock-layout-144505. Three corrections. (a) The Lock Layout and Restore evidence was produced with Space, which the harness overstates. Orca never sees the harness's keys. With a real keyboard, Orca's onSelectionChanged returns early when the last key was Space (default.py:1565 `if keyString == "space": return`), so pressing Space on Lock Layout or Restore would not speak the tabs. The effect is real after Enter, which is how every menu command is chosen (Hide, Move to, Move to new activity), and after AT-SPI or mouse activation. (b) The order of the three utterances varies from run to run, because the adapter emits from a HashSet. In 2 of the 7 menu-driven acts I measured, the last word happened to be the right tab (promote-144238: 'Explorer page tab.'; move-tab-144452: 'Properties page tab.'). (c) I rate it medium, not high. The real focus is announced first, but cut. The extra speech misleads, and Orca's locus of focus goes back to the real focus at the next focus event. This is misleading, badly ordered speech, not missing information.
- **Fix idea:** Rebuild only the side(s) a structural change touched, and keep untouched sides' subtrees and node ids, so no unchanged tab list is re-added. A policy change (Lock Layout) needs no rebuild of the tab lists at all. Upstream, accesskit\_atspi\_common could skip selection-changed for a container added in the same update.

### docking-04 {#docking-04}

After Hide / Move from a menu, focus lands on the first focusable node of the rebuilt layout, often in another side

- **Example:** docking
- **Act:** docking-hide-dock: Properties' options button -&gt; Down, Enter (Hide "Properties"). docking-hide-activity: Source rail tab -&gt; Shift+F10 -&gt; Down, Enter (Hide "Source").
- **The reader should get:** Focus lands on a control related to what was hidden: the trailing strip's 'Hidden activities' button, which brings Properties back, or the rail/Settings for Source. The reader is told where they are.
- **The reader gets:** After Hide "Properties" (trailing side), focus jumps to the Explorer header in the leading side, 3 of 3 runs. The 'Hidden activities' button that now exists in the trailing strip is not focused. After Hide "Source" focus goes to the rail's Settings button. The target is not chosen: the framework restores focus to first\_focusable\_descendant of the rebuilt DockingLayout. Combined with docking-03, the reader's last words are about a third place ('Terminal page tab.').
- **Platform:** Linux AT-SPI / Orca (measured); the focus target is framework logic, so the same on Windows/macOS (not measured)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-core/src/widget\_tree/layout\_impl.rs:398-406 (focus\_owner -&gt; pending\_focus\_restore) and :863-868 (first\_focusable\_descendant of the rebuilt DockingLayout); crates/teksilo-widgets/src/docking/context\_menu.rs contains no focus handling at all (no request\_focus anywhere in the file)
- **Evidence:**
  - `docking-hide-dock-20260925-141447-184100: '       +263.9 ms object:state-changed:focused 1 [push button] 'Explorer'' '       +264.9 ms object:state-changed:focused 0 [menu] ''' '     +418.6 ms ORCA SAYS (CUT): 'landmark Leading panel'' '     +418.6 ms ORCA SAYS (CUT): 'Explorer push button.'' '     +452.8 ms ORCA SAYS (CUT): 'Source page tab.'' '     +489.4 ms ORCA SAYS: 'Terminal page tab.''`
  - `tree after Hide Properties: '    [landmark] 'Trailing panel'' '      [page tab list] '' {horizontal}' '        [push button] 'Hidden activities' desc='Hidden activities' {focusable}'`
  - `docking-hide-activity: '       +231.6 ms object:state-changed:focused 1 [push button] 'Settings'' '     +286.5 ms ORCA SAYS (CUT): 'Settings push button.'' '     +309.5 ms ORCA SAYS (CUT): 'Terminal page tab.'' '     +343.1 ms ORCA SAYS: 'Properties page tab.''`
  - ``source: crates/teksilo-core/src/widget_tree/layout_impl.rs:863 `if let Some(root) = self.pending_focus_restore.take() ... first_focusable_descendant(root)`; crates/teksilo-widgets/src/docking/context_menu.rs Hide/Move handlers only mutate the model (set_tab_hidden / move_tab), with no focus target``
  - `V/docking-hide-dock-20260925-144204-794594: '+270.2 ms object:state-changed:focused 1 [push button] 'Explorer'' '+447.4 ms ORCA SAYS (CUT): 'Explorer push button.'' '+495.1 ms ORCA SAYS: 'Terminal page tab.''; -144356 and -144549 the same`
  - `V/docking-hide-activity-20260925-145100-942916: '+248.8 ms object:state-changed:focused 1 [push button] 'Settings'' ... '+396.4 ms ORCA SAYS: 'Terminal page tab.''; -145304: '+314.6 ms ... 'Settings''`
  - `V/docking-promote-20260925-144238-794594/tree-tree--after-Move-to-new-activity.txt: the leading content is only labels (no focusable); '[page tab] 'Explorer' {focusable,focused,selectable,selected}'`
- **Reproduced:** hide-dock 3 of 3 runs focus -&gt; Explorer; hide-activity 1 of 1 -&gt; Settings
- **Verification:** confirmed. Reproduced: Hide 'Properties' -&gt; focus on the Explorer header, 3 of 3 runs; Hide 'Source' -&gt; focus on the rail's Settings button, 2 of 2 runs; restoring Source from the rail background menu -&gt; focus on the Explorer header, 2 of 2
- **Fix idea:** Have DockingLayout pick the focus target for each structural action, after the rebuild: the moved or promoted tab, the neighbouring visible tab, the strip's Hidden activities button, or the rail. Do not fall back to the layout's first focusable.

### docking-05 {#docking-05}

Dock resize handles have no accessible name: Orca says 'vertical splitter 20.'

- **Example:** docking
- **Act:** reader.py tabwalk docking (Tab 17, 21, 29); docking-handle-keys: focus the leading handle
- **The reader should get:** Each side's divider is named for what it resizes (for example 'Resize leading panel' or 'Explorer sidebar'), and its value is given as a percentage.
- **The reader gets:** All four DockResizeHandle nodes are unnamed separators. A reader hears 'vertical splitter 20.' twice (leading and trailing, indistinguishable) and 'horizontal splitter 25.' (bottom), with no idea which panel each resizes or what 20 means.
- **Platform:** Linux AT-SPI / Orca measured. The handle sets no name in accessibility(), so it is unnamed on Windows (UIA) and macOS (AXSplitter) too, by source.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/docking/resize\_handle.rs:489-520 (no set\_name)
- **Evidence:**
  - `tabwalk Tab 17: '        +14.6 ms object:state-changed:focused 1 [separator] ''' '      +43.3 ms ORCA SAYS: 'vertical splitter 20.''`
  - `tabwalk Tab 21: '      +57.3 ms ORCA SAYS: 'vertical splitter 20.''; Tab 29: '      +48.0 ms ORCA SAYS: 'horizontal splitter 25.''`
  - `orca-debug.out (tabwalk): '14:00:23.517831 - SPEECH OUTPUT: 'vertical splitter 20.'' and '14:00:46.202790 - SPEECH OUTPUT: 'horizontal splitter 25.''`
  - `tree: '    [separator] '' {focusable,vertical} value={'current': 20.3125, 'minimum': 0.0, 'maximum': 100.0, 'increment': 0.0, 'text': None}'`
  - `docking-handle-keys (2 of 2 runs): '       FAIL  Orca says one of ('Leading', 'Explorer', 'sidebar')' '             Orca said: 'vertical splitter 20.''`
  - `source: crates/teksilo-widgets/src/docking/resize_handle.rs:489-520 accessibility() sets role, orientation, numeric value, value string, expanded; no set_name`
  - `V/tabwalk-docking-20260925-144956-1025820: 'Tab 17' '+12.7 ms object:state-changed:focused 1 [separator] ''' '+51.8 ms ORCA SAYS: 'vertical splitter 20.''; 'Tab 21' '+113.2 ms ORCA SAYS: 'vertical splitter 20.''; 'Tab 29' '+82.9 ms ORCA SAYS: 'horizontal splitter 25.''`
  - `V/docking-handle-keys-20260925-144715-942916: '+112.6 ms ORCA SAYS: 'vertical splitter 20.'' 'FAIL  Orca says one of ('Leading', 'Explorer', 'sidebar')'`
- **Reproduced:** deterministic; seen in the tabwalk (3 handles) and in 2 of 2 handle-keys runs
- **Verification:** confirmed. Reproduced: Deterministic: tabwalk (Tab 17, 21, 29) and 2 of 2 handle-keys runs
- **Fix idea:** Give the handle a localized name per side (a11y.rs, like side\_label), and link it with `controls` to the side panel as the rail tab does.

### docking-06 {#docking-06}

Home on a resize handle hides the side and then strands focus on a disabled handle: End/Enter can no longer bring the side back, while AT-SPI still reports it enabled with its old value

- **Example:** docking
- **Act:** docking-handle-keys: focus the leading handle, Home, then End, then Enter
- **The reader should get:** Home hides the side, and End (or Enter, or the Expand action) on the same handle shows it again, as resize\_handle.rs documents. The handle's state tells the reader the side is collapsed.
- **The reader gets:** Home hides the side, but DockingLayout disables the handle while its side is hidden (enabled\_when(handle, visible)). The handle keeps keyboard focus at zero width, and End and Enter produce no event at all. AT-SPI still says 'enabled, focusable, focused' with value 20.3125 and extents \[48, 54, 0, 599\]. For a Strip side (trailing/bottom: no rail), an app without an external toggle leaves no keyboard way back once Home is pressed: the hidden side's strip and its Hidden activities button are hidden with it. The empty Top side's disabled handle likewise sits in the reader's tree as an 'enabled, focusable' horizontal splitter at 25 with extents \[0, 0, 0, 0\].
- **Platform:** Linux AT-SPI / Orca measured. The dead End/Enter is framework behaviour on all platforms. The 'enabled' state is an AT-SPI adapter mapping.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/docking.rs:338 (enabled\_when(handle, visible)); crates/teksilo-widgets/src/docking/resize\_handle.rs:366-369 (Enter) and 418-427 (Home/End), unreachable once disabled; resize\_handle.rs:503-512 (the value is side\_size/extent whatever the visibility)
- **Evidence:**
  - `docking-handle-keys (2 of 2 runs): '== End on the handle  (End shows the side again (the handle's own documented key))' '   steps: key End' '       FAIL  the tree holds [landmark] 'Leading panel'' '             no such node in the tree after the act'`
  - `'== Enter on the handle  (Enter toggles the side)' '   steps: key Return' '       FAIL  Orca says one of ('collapsed', 'hidden')' '             Orca said nothing in this act' (no events in either act)`
  - `tree after Home: separator '' ['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'vertical', 'visible'] {'current': 20.3125, 'minimum': 0.0, 'maximum': 100.0, 'increment': 0.0, 'text': None} [48, 54, 0, 599]`
  - `Top handle at launch: separator '' ['enabled', 'focusable', 'horizontal', 'sensitive', 'showing', 'visible'] [0, 0, 0, 0] value current 24.82990074157715`
  - ``source: crates/teksilo-widgets/src/docking.rs:338 `ctx.enabled_when(handle, visible.clone())`; crates/teksilo-widgets/src/docking/resize_handle.rs:418-427 Home -> set_side_visible(false), End (RangeMove::ToMax, line 424) -> set_side_visible(true), unreachable once disabled; accesskit_atspi_common-0.20.0/src/node.rs state(): disabled only becomes ReadOnly for read-only-capable roles, else Enabled|Sensitive (accesskit_consumer-0.39.0/src/node.rs:861-879 is_read_only_supported excludes Splitter)``
  - `V/docking-handle-keys-20260925-144715-942916 tree after Home: separator '' ['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'vertical', 'visible'] value 20.3125 extents [48, 54, 0, 599]; top handle ['enabled', 'focusable', 'horizontal', 'sensitive', 'showing', 'visible'] 24.83 [0, 0, 0, 0]`
  - `run.json acts 'End on the handle' and 'Enter on the handle': 0 events each (both runs)`
  - `accesskit_atspi_common-0.20.0/src/node.rs: 'if state.is_read_only_supported() && state.is_read_only_or_disabled() { atspi_state.insert(State::ReadOnly); } else { atspi_state.insert(State::Enabled | State::Sensitive); }'`
- **Reproduced:** 2 of 2 runs (deterministic)
- **Verification:** confirmed. Reproduced: 2 of 2 runs (deterministic): End and Enter after Home produce 0 events
- **Fix idea:** Keep a hidden side's handle operable for End/Enter/Expand while it holds focus, or move focus to the rail tab / the control that shows the side when Home hides it. Leave an empty or hidden side's handle out of the AT tree (hidden), rather than just disabled.

### docking-07 {#docking-07}

Showing/hiding a side and collapsing/expanding a dock pane are never spoken: no expanded state reaches AT-SPI, and nothing else says it

- **Example:** docking
- **Act:** docking-rail-toggle: Enter / Enter / Space on the active Source rail tab. docking-accordion: focus the Explorer header, Space, Space. docking-handle-keys: Home.
- **The reader should get:** Focusing a dock header says it is expanded or collapsed. Hiding or showing a side from its rail tab (or collapsing a pane from its header) is spoken, for example 'collapsed' / 'expanded'.
- **The reader gets:** Orca says 'Explorer push button.' with no state. Space collapses and expands the pane in silence; the bus carries only a dozen value changes from the pane divider as it animates. Enter on the rail tab hides the whole leading side in silence; the only state change on the bus is the tab's selected flipping to 0, which Orca does not speak for a focused tab. Teksilo does set `expanded` on the accordion, the rail tab and the handle. accesskit\_atspi\_common 0.20 exports no expanded/expandable state at all, and accesskit\_macos 0.27 has no mapping either. Only accesskit\_windows exposes it (ExpandCollapse pattern).
- **Platform:** Linux AT-SPI / Orca (measured); macOS (by source: accesskit\_macos-0.27.0 has no expanded mapping); Windows exposes the state (accesskit\_windows-0.35.0 node.rs:714-723), not measured
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0/src/node.rs:301-386 (state() has no expanded mapping; a case-insensitive grep for 'expand' over the crate's src finds nothing); accesskit\_macos-0.27.0 has no mapping either (the same grep finds nothing). accesskit\_windows-0.35.0/src/node.rs:714-723 and 1486 export ExpandCollapseState (supports\_expand\_collapse, consumer node.rs:661-668).
- **Evidence:**
  - `docking-rail-toggle 'Enter on the active rail tab': '        +34.2 ms object:state-changed:selected 0 [page tab] 'Source'' '        +38.9 ms object:selection-changed [page tab list] 'Leading activity bar'' '       +210.4 ms object:children-changed:remove [frame] '' -> [landmark] 'Leading panel'' '       FAIL  Orca says one of ('collapsed', 'hidden')' '             Orca said nothing in this act'`
  - `'Enter again': '        +22.9 ms object:state-changed:selected 1 [page tab] 'Source'' '       FAIL  Orca says one of ('expanded', 'shown')' '             Orca said nothing in this act'`
  - `docking-accordion: '      +86.1 ms ORCA SAYS: 'landmark Leading panel'' '      +86.1 ms ORCA SAYS: 'Explorer push button.'' '       FAIL  Orca says one of ('expanded',)'`
  - `'Space on the Explorer header': '        +37.6 ms object:property-change:accessible-value [separator] 'Splitter divider'' ... '       +241.6 ms object:property-change:accessible-value [separator] 'Splitter divider'' '       FAIL  Orca says one of ('collapsed',)' '             Orca said nothing in this act'`
  - `tree: '          [push button] 'Explorer' {focusable} rel=['controller-for']' (no expandable/expanded state)`
  - `source: Teksilo sets expanded at crates/teksilo-widgets/src/accordion.rs:672, docking/activity_bar.rs:2259, docking/resize_handle.rs:512; accesskit_atspi_common-0.20.0/src/node.rs:301-380 state() has no Expandable/Expanded branch; rail hidden-as-unselected at docking/activity_bar.rs:2246`
  - `V/docking-rail-toggle-20260925-144756-942916: 'Enter on the active rail tab' '+22.0 ms object:state-changed:selected 0 [page tab] 'Source'' 'Orca said nothing in this act'`
  - `V/docking-accordion-20260925-144831-942916: '+66.8 ms ORCA SAYS: 'Explorer push button.'' 'FAIL  Orca says one of ('expanded',)'; both Space acts 'Orca said nothing in this act'`
- **Reproduced:** deterministic; rail-toggle 3 of 3 acts silent, accordion 2 of 2, handle Home 2 of 2 runs
- **Verification:** confirmed. Reproduced: Deterministic: rail-toggle 3 of 3 acts silent, accordion focus + 2 toggles silent, handle Home silent (2 of 2 runs)
- **Fix idea:** Upstream: map expanded to AT-SPI Expandable/Expanded, and emit state-changed:expanded. Until then, Teksilo could announce side show/hide and pane collapse/expand through ctx.announce (reliable once K2's fix lands), e.g. 'Leading panel hidden'.

### docking-08 {#docking-08}

Every dock's options button is announced as just 'More actions': the dock name sits on an unknown-role wrapper, not on the button

- **Example:** docking
- **Act:** reader.py tabwalk docking (Tab 11, 14, 20, 25, 28); docking-options-menu: focus Explorer's options button
- **The reader should get:** The ⋮ button says which dock it belongs to: 'More actions: Explorer', as the code intends with .access\_label.
- **The reader gets:** Five identical 'More actions push button.' stops. `.access_label("More actions: Explorer")` is applied to the PopoverIconButton (PopoverWidget), which has no AT presence of its own. The override creates a named node of unknown role ('\[unknown\] 'More actions: Explorer''), and the focusable IconButton inside keeps its tooltip name 'More actions'. The toolbar's dropdown actions have the same shape and give '\[unknown\] 'More'' wrappers. The launch audit flags 7 unknown-role nodes.
- **Platform:** Linux AT-SPI / Orca measured. The focusable button's name comes from Teksilo, so it is 'More actions' on every platform (by source).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/docking/panel.rs:926-933 (access\_label on the PopoverIconButton); crates/teksilo-widgets/src/popover\_widget.rs:946-951 (empty accessibility); crates/teksilo-widgets/src/toolbar.rs:335-339 (same pattern)
- **Evidence:**
  - `tabwalk Tab 11: '        +18.6 ms object:state-changed:focused 1 [push button] 'More actions'' '     +231.7 ms ORCA SAYS: 'More actions push button.''; same utterance at Tabs 14, 20, 25, 28 (orca-debug.out lines at 14:00:12.363194, 14:00:17.890013, 14:00:29.240266, 14:00:38.685575, 14:00:44.339018)`
  - `tree: '            [unknown] 'More actions: Explorer'' '              [push button] 'More actions' desc='More actions' {focusable}'`
  - `tree audit: 'unknown-role: [unknown] 'More actions: Explorer': a node whose role the adapter could not map' (also Search, Properties, Terminal, Problems, and 'More' x2)`
  - `docking-options-menu (3 of 3 runs): '       FAIL  Orca says one of ('Explorer',)' '             Orca said: 'More actions push button.''`
  - ``source: crates/teksilo-widgets/src/docking/panel.rs:926-933 `PopoverIconButton::new(IconButton::more()...).access_label(lit!(format!("More actions: {}", ...)))`; crates/teksilo-widgets/src/popover_widget.rs:946-951 PopoverWidget::accessibility is empty ('No AT presence of our own'); crates/teksilo-widgets/src/toolbar.rs:335-339 same pattern for dropdown actions``
  - `V/tabwalk-docking-20260925-144956-1025820: Tab 11 '+179.4 ms ORCA SAYS: 'More actions push button.'' (same at 14, 20, 25, 28)`
  - `V/tree-docking-20260925-144935-1016128 audit: 'unknown-role: [unknown] 'More actions: Explorer'' (and Search, Properties, Terminal, Problems, 'More' x2)`
- **Reproduced:** deterministic (tabwalk plus 3 of 3 options-menu runs)
- **Verification:** corrected by the verifier. Reproduced: Deterministic: tabwalk Tabs 11, 14, 20, 25, 28; options-menu focus act 3 of 3 runs The mechanism and the evidence are right. I lower the severity to medium. The focusable control has a name ('More actions'); what is missing is which dock it belongs to. It always follows its dock's header in Tab order, and Orca announces the landmark when the reader enters a side. That makes the names ambiguous, not missing. The side effect is real: seven named \[unknown\]-role wrapper nodes clutter the tree and fail the launch audit.
- **Fix idea:** Put the label on the trigger (IconButton::more().access\_label(...)), or make PopoverWidget forward access\_\* overrides to its trigger, as it already does for role/has\_popup/expanded.

### docking-09 {#docking-09}

Keyboard resizing is never heard: Orca speaks only 'vertical splitter' on each arrow, and a named divider's value is never spoken at all

- **Example:** docking
- **Act:** docking-handle-keys: Right / Left on the leading handle; tabwalk Tab 12/26: focus a pane 'Splitter divider'; docking-reshown-events: Right on the Terminal\|Problems divider
- **The reader should get:** Each arrow press speaks the new size (e.g. '22 percent'). Focusing a divider says its name and its position.
- **The reader gets:** Right/Left emit object:property-change:accessible-value, and Orca answers 'vertical splitter' with no number. Orca 46's focused-separator format is 'roleName + availability' (formatting.py:467-470), and AccessKit maps Role::Splitter to AT-SPI Separator (node.rs:249), not SplitPane. On focus, Orca speaks the value only when the node has no name ('vertical splitter 20.'), so the named pane dividers are read 'horizontal splitter Splitter divider.' with their 50% never spoken. The value string Teksilo sets ('20%') never reaches AT-SPI: accesskit\_unix's Value interface has no text property and no valuetext attribute, so Orca reads a bare '20'. The pane divider is also named only 'Splitter divider', not what it splits.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows/macOS not measured.
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** Orca formatting.py:467-470 (SEPARATOR focused 'roleName + availability'; unfocused 'roleName + availability + (labelOrName or displayedText or value)'); accesskit\_atspi\_common-0.20.0/src/node.rs:249 (Splitter -&gt; Separator); accesskit\_unix-0.23.0/src/atspi/interfaces/value.rs (min/max/increment/current only, no text); Teksilo side crates/teksilo-widgets/src/docking/resize\_handle.rs:507-510
- **Evidence:**
  - `docking-handle-keys (2 of 2 runs) 'Right arrow on the handle': '        +24.8 ms object:property-change:accessible-value [separator] ''' '      +33.2 ms ORCA SAYS: 'vertical splitter'' '       FAIL  Orca says one of ('%', '21', '22', '23')'`
  - `orca-debug.out: '14:04:38.413161 EVENT MANAGER: object:property-change:accessible-value for [separator] in [application: 'docking'] (0, 0, 0) is not obsoleted' '14:04:38.419801 SPEECH OUTPUT: 'vertical splitter''`
  - `tabwalk Tab 12: '        +17.1 ms object:state-changed:focused 1 [separator] 'Splitter divider'' '      +76.0 ms ORCA SAYS: 'horizontal splitter Splitter divider.'' (tree value current 50.0)`
  - `docking-reshown-events 'Right on the divider (before any hide)': 'said: vertical splitter' (3 of 3 runs)`
  - `source: /usr/lib/python3/dist-packages/orca/formatting.py:467-470 SEPARATOR 'focused': 'roleName + availability'; accesskit_atspi_common-0.20.0/src/node.rs:249 Role::Splitter => AtspiRole::Separator; accesskit_unix-0.23.0/src/atspi/interfaces/value.rs (min/max/increment/current only); crates/teksilo-widgets/src/docking/resize_handle.rs:507-510 set_numeric_value + set_value("{frac:.0}%")`
  - `V/docking-handle-keys-20260925-144715-942916: 'Right arrow on the handle' '+30.6 ms object:property-change:accessible-value [separator] ''' '+40.9 ms ORCA SAYS: 'vertical splitter''`
  - `V/tabwalk-docking-20260925-144956-1025820: Tab 12 '+106.0 ms ORCA SAYS: 'horizontal splitter Splitter divider.''`
- **Reproduced:** deterministic; 2 of 2 handle-keys runs, 3 of 3 reshown-events runs
- **Verification:** confirmed. Reproduced: Deterministic: 2 of 2 handle-keys runs, 3 of 3 reshown-events runs (before the hide), tabwalk
- **Fix idea:** Upstream: expose the value string (valuetext attribute / Value.Text) in accesskit\_unix. Orca speaks value changes for SPLIT\_PANE, so mapping Splitter there would help too. Teksilo could announce the new size after a keyboard step, and name pane dividers after the panes they split.

### docking-10 {#docking-10}

In the dock context and options menus, arrow keys make no event and no speech: the reader hears 'menu.' and never an item

- **Example:** docking
- **Act:** docking-rail-menu: Shift+F10 on the Source rail tab, Down x3. docking-options-menu: Enter on Explorer's options button, Down x2. docking-move-tab: Shift+F10 on Properties, Down Down.
- **The reader should get:** Each arrow press moves to an item and the reader hears it ('Hide "Source"', 'Move to', 'Source check menu item checked', 'Move to new activity', 'Move to side').
- **The reader gets:** Opening puts focus on the unnamed menu and Orca says 'menu.'. Every arrow press after that produces no AT-SPI event and no speech. MenuList moves a private highlight (focused\_index) and never moves focus or an active descendant. Hide, Move to, the activity check list and the size submenus, which are docking's keyboard alternative to dragging, can only be chosen blind.
- **Platform:** Linux AT-SPI / Orca measured. The highlight is invisible to AT on every platform (no focus, no active descendant), by source.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `9636094c` (menus).
- **Where:** crates/teksilo-widgets/src/menu\_list.rs:229-233 (arrows move focused\_index, not focus), 752-821 (key handler), 991-993 (accessibility sets only Role::Menu; no active\_descendant anywhere in the file)
- **Evidence:**
  - `docking-rail-menu 'Shift+F10 on the rail tab': '        +54.1 ms object:children-changed:add [frame] '' -> [menu] ''' '        +55.4 ms object:state-changed:focused 1 [menu] ''' '     +101.2 ms ORCA SAYS: 'menu.'' '       FAIL  Orca says one of ('Hide',)'`
  - `'== Down in the menu  (the highlight moves to the first item and the reader hears it)' '   steps: key Down' '       FAIL  Orca says one of ('Hide',)' '             Orca said nothing in this act' (same for the next two Down acts: 'Move to', 'Source')`
  - `docking-options-menu (3 of 3 runs): 'Down' and 'Down again' acts: '             Orca said nothing in this act'`
  - `docking-move-tab 'Down, Down: Move to': '       FAIL  Orca says one of ('Move to',)' '             Orca said nothing in this act'`
  - ``source: crates/teksilo-widgets/src/menu_list.rs:229-233 ('Arrow / Home / End / type-ahead navigation moves `focused_index`, **not** real tree focus'), 752-821 key handler, 991-993 accessibility sets only Role::Menu``
  - `V/verify-docking-submenu-routes-20260925-144653-932995/tree-Down--Down--highlight-Move-to-side.txt: '[menu] '' {focusable,focused}' '[menu item] 'Move to new activity'' '[menu item] 'Move to side''`
  - `V/docking-options-menu-20260925-144205-796992: 'Down' and 'Down again' 'Orca said nothing in this act' (and -144410, -144616)`
- **Reproduced:** deterministic; rail-menu 3 of 3 Down acts, options-menu 3 of 3 runs, move-tab 1 of 1
- **Verification:** corrected by the verifier. Reproduced: Deterministic: options-menu 3 of 3 runs (Down, Down silent), rail-menu (3 Down acts silent), move-tab 2 of 2, header dropdown 2 of 2 runs It reproduces and the source confirms it. I also checked the tree after 'Down, Down' in verify-docking-submenu-routes. The items carry no state at all ('\[menu item\] 'Move to new activity'', '\[menu item\] 'Move to side'': not focusable, not selected, not focused), so no AT query can find the highlighted item either. The rubric makes this critical, not high. The reader cannot tell which item Enter will activate. These menus are docking's only keyboard route to Hide, Move to, the activity check list and the size modes, so the reader cannot operate them except blind. It is the same root cause as the menus sweep.
- **Fix idea:** Move real focus to the highlighted MenuItem, or set active\_descendant on the Role::Menu, on every highlight change.

### docking-11 {#docking-11}

The 'Move to' / 'Move to side' submenus close themselves about 170 ms after Right opens them, so the keyboard alternative to drag-to-dock cannot pick a side

- **Example:** docking
- **Act:** docking-options-menu: Explorer's options menu, Down, Down, Right (Move to side). docking-move-tab: Properties tab menu, Down, Down, Right (Move to).
- **The reader should get:** Right opens the submenu, and it stays open until the user picks a side or presses Escape/Left.
- **The reader gets:** The submenu is added and focused, and about 170 ms later it is removed with no input: focus returns to the parent menu and Orca says 'menu.' again. At a human pace the side list (Leading/Trailing/Top/Bottom) is never available. Moving a dock or tab to another side worked only by pressing Shift+F10, Down, Down, Right, Down, Enter at 40 ms a key (docking-move-tab). The same self-closing submenu appears in the menus sweep ('Recent').
- **Platform:** Linux, private KWin session with fake keyboard input (measured). The pointer never moves in these runs; where it rests is up to the private KWin, and the closing may depend on it.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `9636094c` (menus).
- **Where:** crates/teksilo-widgets/src/menu\_list.rs:833 and 847 (Enter/Space and inline-forward arrow call ctx.synthetic\_click); crates/teksilo-core/src/widget\_tree/test\_api.rs:82-96 (synthesise\_tap\_with\_ops dispatches WidgetEvent::pointer\_down/up, which carry PointerInfo::mouse: crates/teksilo-core/src/event.rs:854-860); crates/teksilo-widgets/src/menu\_item/widget\_impl.rs:589 (on\_tap chooses SubmenuOpenRoute::Tap(ctx.pointer\_kind())); crates/teksilo-widgets/src/menu\_item.rs:143-158 (Tap(Mouse) -&gt; DismissBehavior::PointerLeave 150 ms)
- **Evidence:**
  - `docking-options-menu-20260925-142036-250821 'Right: open the submenu': '        +18.9 ms object:children-changed:add [frame] '' -> [menu] ''' '        +20.2 ms object:state-changed:focused 1 [menu] ''' '      +70.4 ms ORCA SAYS (CUT): 'menu.'' '       +188.3 ms object:children-changed:remove [frame] '' -> [menu] ''' '       +188.7 ms object:state-changed:focused 1 [menu] ''' '     +270.7 ms ORCA SAYS: 'menu.''`
  - `'       FAIL  the tree holds [menu item] 'Trailing'' '             no such node in the tree after the act'`
  - `docking-move-tab 'Right: the side submenu': '        +24.3 ms object:state-changed:focused 1 [menu] ''' '       +193.2 ms object:state-changed:focused 1 [menu] ''' '       FAIL  the tree holds [menu item] 'Leading''`
  - `fast path worked: 'Shift+F10, Down, Down, Right, Down, Enter at 40 ms a key' -> '       pass  the tree holds [page tab] 'Properties'' with '       +473.6 ms object:state-changed:focused 1 [push button] 'New Property''`
  - `source: dismissal via the overlay's DismissBehavior::PointerLeave grace, crates/teksilo-core/src/widget_tree.rs:1525-1595 (grace starts) and 1727-1795 (dismiss after delay); the submenus come from crates/teksilo-widgets/src/docking/context_menu.rs MenuItem::submenu. The exact trigger with no pointer motion is not pinned down.`
  - `V/verify-docking-submenu-routes-20260925-144653-932995: 'Right on the highlighted row' '+192.7 ms object:children-changed:remove [frame] '' -> [menu] '''; 'AT-SPI click on the Move to side row (activate_item, KeyboardOrAt route)' 'pass  the tree holds [menu item] 'Trailing'' 'pass  no menu leaves the tree during the act'; 'Enter on the highlighted Move to side row' '+183.1 ms object:children-changed:remove [frame] '' -> [menu] '''. Runs -145103 (192.9 / 191.2 ms) and -145305 (202.5 / 196.9 ms) the same`
  - `V/verify-docking-submenu-trace-20260925-145244-1095428/app.log: '[teksilo input] Move PointerId(1) at Point { x: 640.0, y: 416.0 } buttons=ButtonMask(0) t=EventTime(15.760802684s)' (4 such samples in the run, none from the harness)`
- **Reproduced:** 4 of 4 (options-menu 3 of 3 runs, move-tab 1 of 1)
- **Verification:** corrected by the verifier. Reproduced: Right on the highlighted row: submenu removed after 183-203 ms, 3 of 3 options-menu runs, 2 of 2 move-tab runs, 3 of 3 submenu-routes runs, 1 of 1 trace run. Enter on the row: 3 of 3. AT-SPI click on the same row: stays open 3 of 3 (3 s recorded). The defect is real, but the sweep put it in the wrong place. The pointer-leave code works as designed. The fault is the route the keyboard takes. Focus never leaves the MenuList panel, so the MenuItem's own KeyboardOrAt paths (widget\_impl.rs:788 and 814, EscapeOrClickOutside) never run. MenuList activates the row with synthetic\_click, a synthetic mouse tap, so the submenu opens as if a mouse had clicked it and gets a 150 ms pointer-leave dismissal. The discriminating test: the same 'Move to side' row opened by an AT-SPI click goes through activate\_item with KeyboardOrAt and stays open. Right and Enter close it. The grace is started by the resting pointer: TEKSILO\_TRACE\_INPUT=all in app.log shows the private KWin delivering buttonless Move samples at (640, 416), the window centre over the editor, with no pointer input. Any such sample outside the submenu and its row starts the 150 ms countdown. A keyboard user whose mouse rests anywhere else on a real desktop gets the same result. It affects sighted keyboard users too. The fix belongs in MenuList: activate the row with its keyboard route (for example, call the item's activation with KeyboardOrAt) instead of a synthetic mouse click.
- **Fix idea:** A submenu opened from the keyboard (or by AT activation) must not get a pointer-leave dismissal until the pointer actually moves after it opened. Arm the grace only from a real pointer sample taken after the open.

### docking-12 {#docking-12}

A dock accordion's push button is the whole pane: header actions and the dock's content region are its children

- **Example:** docking
- **Act:** reader.py tree docking (launch tree); tabwalk Tab 9-11
- **The reader should get:** The disclosure button covers the header only; the pane's toolbar and content region are siblings of it, not inside a button (button children are presentational).
- **The reader gets:** '\[push button\] 'Explorer'' spans the pane (extents \[48, 54, 260, 296\]) and holds the '\[tool bar\] 'Explorer actions'', the options wrapper and the '\[landmark\] 'Explorer'' content region. The same holds for Search, Terminal (\[0, 698, 637, 162\]) and Problems. Any hit-test inside the pane's content (Orca mouse review, touch exploration) lands in or on a button whose Click collapses the pane.
- **Platform:** Linux AT-SPI (structure measured); the Role::Button node is Teksilo's, so the same on all platforms
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/accordion.rs:668-678 (the Accordion root is the Role::Button, and its children are header and body)
- **Evidence:**
  - `tree: '          [push button] 'Explorer' {focusable} rel=['controller-for']' '            [tool bar] 'Explorer actions' {horizontal}' ... '            [landmark] 'Explorer'' '              [panel] ''' ... '                  [label] 'Cargo.toml''`
  - `extents: push button 'Explorer' [48, 54, 260, 296]; landmark 'Explorer' [48, 86, 260, 264]; push button 'Terminal' [0, 698, 637, 162]`
  - `source: crates/teksilo-widgets/src/accordion.rs:668-678 (Role::Button, name, expanded, Click on the Accordion widget itself, whose children are header and body, lines 690-698)`
  - `V/tree-docking-20260925-144935-1016128/run.json: push button 'Explorer' [48, 54, 260, 296] > landmark 'Explorer' [48, 86, 260, 264]; push button 'Terminal' [0, 698, 637, 162] > landmark 'Terminal' [32, 698, 605, 162]`
- **Reproduced:** deterministic (structure)
- **Verification:** confirmed. Reproduced: Deterministic (structure, 2 launch trees)
- **Fix idea:** Put Role::Button (name, expanded, Click, controls) on the header node only and leave the Accordion root a GenericContainer, so the region is the button's sibling.

### docking-13 {#docking-13}

Tab order and structure: a side's content comes before the rail that controls it; strip tab lists are unnamed

- **Example:** docking
- **Act:** reader.py tabwalk docking
- **The reader should get:** Tab order follows the visual and logical order: the activity rail (left edge), then the panel it controls. Every tab list is named.
- **The reader gets:** Tab 9-14 walk the leading panel's content (Explorer ... More actions) before Tab 15 reaches the Source rail tab that sits to its left and controls it. The trailing and bottom strips' tab lists are '\[page tab list\] ''' (Orca never speaks a page tab's ancestors, so the landmark name is also skipped on entering those sides through their tab).
- **Platform:** Linux AT-SPI / Orca measured; the order is Teksilo's on all platforms
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/docking.rs:340-342 (children order content, rail, handle); Orca speech\_generator.py:1957-1958 (no ancestors for a page tab)
- **Evidence:**
  - `tabwalk: 'Tab 9 ... +13.4 ms object:state-changed:focused 1 [push button] 'Explorer'' ... 'Tab 15 ... +16.9 ms object:state-changed:focused 1 [page tab] 'Source''`
  - `tree: '    [landmark] 'Trailing panel'' '      [page tab list] '' {horizontal}'; '    [landmark] 'Bottom panel'' '      [page tab list] '' {horizontal}'`
  - `Tab 18: '      +65.0 ms ORCA SAYS: 'Properties page tab.'' (no 'landmark Trailing panel'; Orca speech_generator.py _generateAncestors returns [] for a page tab)`
  - `source: crates/teksilo-widgets/src/docking.rs:340-342 children order [content, rail, handle] per side`
  - `V/tabwalk-docking-20260925-144956-1025820: Tab 9 '+131.7 ms ORCA SAYS: 'landmark Leading panel'' ... Tab 15 '+103.3 ms ORCA SAYS: 'Source page tab.''; Tab 18 '+50.6 ms ORCA SAYS: 'Properties page tab.''`
- **Reproduced:** deterministic (1 tabwalk; the order comes from the static child order)
- **Verification:** confirmed. Reproduced: Deterministic (tabwalk)
- **Fix idea:** Order each side's children rail-first (or set an explicit focus order), and name the strip's tab list after the side (a11y.rs has the strings).

### docking-14 {#docking-14}

The example's Toggle Sidebar / Panel / Inspector and Lock Layout are plain buttons: toggling a side or the lock is silent and stateless

- **Example:** docking
- **Act:** docking-toolbar-toggles: focus Toggle Panel, Space, Space
- **The reader should get:** A toggle tells the reader its state: pressed/not pressed, or expanded/collapsed with a controls relation to the side it shows.
- **The reader gets:** The bottom panel disappears and comes back with nothing spoken and no state on the button.
- **Platform:** Linux AT-SPI / Orca measured; the example's widget choice, so all platforms
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/docking/src/main.rs:464-507
- **Evidence:**
  - `'== Space on Toggle Panel  (the bottom panel hides and the reader hears the new state)' '   steps: key space' '       pass  the tree holds no [landmark] 'Bottom panel'' '       FAIL  Orca says one of ('not pressed', 'collapsed', 'hidden')' '             Orca said nothing in this act'`
  - `'== Space again' '        +58.1 ms object:selection-changed [page tab list] ''' '       FAIL  Orca says one of ('pressed', 'expanded', 'shown')'`
  - `source: examples/docking/src/main.rs:464-481 and 495-507 (Button::new(lit!("Toggle Sidebar")) ... Button::new(lit!("Lock Layout")))`
  - `V/docking-toolbar-toggles-20260925-144951-942916: 'Space on Toggle Panel' 'FAIL  Orca says one of ('not pressed', 'collapsed', 'hidden')' 'Orca said nothing in this act'`
- **Reproduced:** 1 of 1 (not timing dependent: no event at all on the button)
- **Verification:** confirmed. Reproduced: 1 of 1 (deterministic: no event on the button; the act is silent)
- **Fix idea:** Use toggle buttons bound to side\_visible\_signal / the lock flag (pressed state), or access\_expanded + access\_controls on the side panel.

### docking-15 {#docking-15}

Restore renames the 'Source' activity to 'Explorer'

- **Example:** docking
- **Act:** docking-lock-layout: Export, then Restore
- **The reader should get:** Restoring an exported layout keeps the activity's name ('Source').
- **The reader gets:** After Restore the rail tab is 'Explorer'. import\_state rebuilds tabs with title None, and set\_tab\_title is documented as app config that is not persisted, but the example never re-applies set\_dock\_activity\_title after import\_state.
- **Platform:** all (model behaviour); measured on Linux
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/docking/model.rs:1683-1690 (import\_state rebuilds every DockTab with title: None, even for a tab id it keeps) against model.rs:1496-1500 (set\_tab\_title doc: 'App-config ... like dock titles; not persisted'); workaround in examples/docking/src/main.rs:511-516
- **Evidence:**
  - `docking-lock-layout-20260925-141812-250821 orca-debug.out: '14:18:43.581306 - FOCUS MANAGER: Locus of focus is [page tab: 'Explorer']' during 'Space on Restore'`
  - `Restore act speech in all 3 runs ends on or includes 'Explorer page tab.' where launch said 'Source page tab.'`
  - `source: crates/teksilo-widgets/src/docking/model.rs:1496-1501 (set_tab_title: 'not persisted'), 1683-1685 (import_state builds DockTab with title: None); examples/docking/src/main.rs:511-516 Restore calls model_i.import_state(&state) only`
  - `docking-lock-layout-144120: last 'page tab: 'Source'' line 14:41:36.103403, first 'page tab: 'Explorer'' line 14:41:51.392041, within 'Space on Restore' (started 14:41:51.157231); -144312: 14:43:28.34 / 14:43:43.63 with Restore at 14:43:43.37; -144505: 14:45:25.32 / 14:45:36.73 with Restore at 14:45:36.58`
- **Reproduced:** 3 of 3 runs
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs: after Restore Orca's log names the rail tab 'Explorer' and never 'Source' again It reproduces, but I place it in the framework. set\_tab\_title says the title is app config 'like dock titles'. Dock titles survive import\_state, because docks are kept. Tab titles are silently reset, even for tab ids that import\_state keeps (DockTabId::from\_raw(tab\_dto.id)). Nothing in import\_state's documentation warns about this. An app that sets titles once at startup, as the doc suggests, loses them on its first mid-run import. The example can work around it by re-applying the title after import\_state. The fix belongs in the model: carry the title of a surviving tab id across the import.
- **Fix idea:** In the example, re-apply set\_dock\_activity\_title after import\_state (or persist tab titles in DockLayoutState).

### docking-16 {#docking-16}

Activating another rail activity (Enter on a non-selected rail tab) is silent

- **Example:** docking
- **Act:** docking-rail-arrows: after promote, Up to Source, Enter
- **The reader should get:** The reader hears that Source is now the shown activity (selected), or hears the content that replaced Explorer.
- **The reader gets:** The bus carries selected 0 on Explorer and selected 1 on Source, and the panel content is swapped. Orca, whose focus is on the Source tab, says nothing: Orca 46 does not speak a focused page tab's selected change. The rail uses manual activation (arrows only move focus), so the Enter is the only feedback point.
- **Platform:** Linux AT-SPI / Orca measured
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** Orca scripts/default.py:1487-1535 (onSelectedChanged announces only after Space) and 1580-1600 (the 'selection redundant' break); Teksilo crates/teksilo-widgets/src/docking/activity\_bar.rs (manual activation)
- **Evidence:**
  - `docking-rail-arrows-20260925-142604-434344 'Enter on Source': '        +16.7 ms object:state-changed:selected 0 [page tab] 'Explorer'' '        +19.5 ms object:state-changed:selected 1 [page tab] 'Source'' '        +22.0 ms object:selection-changed [page tab list] 'Leading activity bar'' '       FAIL  Orca says one of ('selected', 'Search')' '             Orca said nothing in this act'`
  - `source: crates/teksilo-widgets/src/docking/activity_bar.rs:2140-2160 (Enter/Space activation, arrows move focus only)`
  - `V/docking-rail-arrows-20260925-145023-942916: 'Enter on Source' '+19.2 ms object:state-changed:selected 0 [page tab] 'Explorer'' '+22.4 ms object:state-changed:selected 1 [page tab] 'Source'' 'FAIL  Orca says one of ('selected', 'Search')'`
- **Reproduced:** 3 of 3 runs (docking-rail-arrows)
- **Verification:** confirmed. Reproduced: 4 of 4: docking-rail-arrows 2 of 2, and the rail switches in verify-docking-activity-switch (Enter on Properties / Enter on Source silent)
- **Fix idea:** Announce the newly shown activity through ctx.announce, or activate on arrow (automatic activation, the ARIA default for tabs) so the focus move carries it.

### docking-17 {#docking-17}

The rail's decorative top-slot glyph '◆' is exposed as a label

- **Example:** docking
- **Act:** reader.py tree docking
- **The reader should get:** A purely decorative logo glyph is hidden from AT (or named for what it is).
- **The reader gets:** '\[label\] '◆'' is a top-level child of the frame, which review modes read as a black diamond.
- **Platform:** all (the example's TextWidget); measured on Linux
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/docking/src/main.rs:295
- **Evidence:**
  - `tree: '    [label] '◆''`
  - `launch: '       +490.2 ms object:children-changed:add [frame] '' -> [label] '◆''`
  - `` source: examples/docking/src/main.rs:295 `.top_slot(|| TextWidget::new(lit!("◆")).style(TextStyleRole::BodyBold))` ``
  - `V/tree-docking-20260925-144935-1016128/tree-launch.txt: '    [label] '◆''`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: Deterministic (both launch trees)
- **Fix idea:** Add .access\_hidden(true) to the glyph in the example.

### docking-M1 {#docking-m1}

A dock's options menu (and a submenu) is silent on every opening after the first: its content comes back with the ids the adapter already declared defunct

- **Example:** docking
- **Scenario:** verify-docking-menu-reopen (tools/reader/scenarios/verify\_docking.py)
- **Act:** verify-docking-menu-reopen: focus Explorer's ⋮ options button; Enter (open), Escape, three times. verify-docking-submenu-routes: open 'Move to side' by Right (it closes itself), then open it again by AT-SPI click.
- **The reader should get:** Every time the menu opens, the reader hears 'menu', and hears the button again when Escape returns focus to it.
- **The reader gets:** The first opening says 'menu.' and Escape says 'More actions push button.'. The second and third openings are silent: Orca drops the menu's focus event as defunct. The Escape back to the button is silent too, because Orca's locus of focus never left 'More actions'. The reopened 'Move to side' submenu is also dropped as defunct. PopoverWidget builds its panel once (add\_deferred), parks it dormant between openings (visible\_when on popover\_open), and shows it again with the same node ids. The rail tab's context menu is built fresh for each opening, and it is spoken every time, which is the control case. Together with docking-10 (items never spoken), the options menu is completely silent from its second use on.
- **Platform:** Linux AT-SPI / Orca 46.1, measured. The defunct state exists only in the AT-SPI adapter, so Windows and macOS are not affected by this mechanism.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: verify-docking-menu-reopen's second and third openings of the options menu, which failed 6 checks in the sweep, pass now.
- **Where:** crates/teksilo-widgets/src/popover\_widget.rs:672-695 (content built once, parked dormant, re-shown with the same ids); same root as docking-01
- **Evidence:**
  - `V=target/reader-verify/docking`
  - `V/verify-docking-menu-reopen-20260925-145156-1078635: 'Enter: open the options menu, time 1' '+43.0 ms object:state-changed:focused 1 [menu] ''' '+138.5 ms ORCA SAYS: 'menu.''; 'time 2' '+39.1 ms object:state-changed:focused 1 [menu] ''' 'FAIL  Orca says 'menu'' '14:52:16.557192 EVENT MANAGER: Ignoring defunct object: [menu]'; 'Escape: close it, time 2' '+18.4 ms object:state-changed:focused 1 [push button] 'More actions'' 'FAIL  Orca says 'More actions''; time 3 the same (14:52:24.336884)`
  - `V/verify-docking-menu-reopen-20260925-145320-1078635 and -145444-1078635: time 1 spoken ('+188.9 ms ORCA SAYS: 'menu.'', '+157.8 ms ORCA SAYS: 'menu.''), times 2 and 3 'FAIL  Orca says 'menu'' and 'FAIL  Orca says 'More actions''`
  - `control: the same runs, 'Shift+F10: rail context menu, time 2' '+167.3 ms ORCA SAYS: 'menu.'' (3 of 3 spoken)`
  - `V/verify-docking-submenu-routes-20260925-144653-932995: 'AT-SPI click on the Move to side row' '+50.6 ms object:state-changed:focused 1 [menu] ''' '14:47:18.892664 EVENT MANAGER: Ignoring defunct object: [menu]' (also in -145103: 14:51:29.426789)`
  - `source: crates/teksilo-widgets/src/popover_widget.rs:672 ctx.add_deferred(...) once, :686 ctx.set_dormant(content_id), :695 ctx.visible_when(content_id, self.popover_open.clone())`
- **Reproduced:** 3 of 3 runs (options menu, openings 2 and 3 silent every time); reopened submenu dropped in 2 of 3 submenu-routes runs by Orca's log
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Same as docking-01: nodes of a subtree that goes dormant and comes back need AccessKit ids they have never had (a per-activation generation in the node id). Otherwise rebuild the popover content on every open, as the context menu already does.

### docking-M2 {#docking-m2}

A dock header's dropdown action ('More' with a menu) moves focus to an unnamed, unknown-role wrapper instead of the menu: Orca stops speech and says nothing

- **Example:** docking
- **Scenario:** verify-docking-header-dropdown
- **Act:** verify-docking-header-dropdown / verify-docking-menu-reopen: focus the 'More' button in Explorer's header actions toolbar, press Enter.
- **The reader should get:** Focus lands on the menu and the reader hears 'menu', as for the ⋮ options menu.
- **The reader gets:** Focus lands on '\[unknown\] '' {focusable,focused}', a wrapper that holds the '\[menu\]'. Orca logs the focus event, issues a speech stop, and says nothing, including the first time the dropdown opens. The Down key then produces no event either (docking-10). The dropdowns are the dock header's own action menus, from ToolbarAction::menu hosted in the framework's header Toolbar.
- **Platform:** Linux AT-SPI / Orca 46.1, measured. The focused node is unnamed and has no role on every platform, by Teksilo's tree.
- **Severity:** high; **layer:** framework
- **Status:** Open. In the popover-trigger fix topic, not fixed there: The cause is different, now pinned down. The dock header's Toolbar gates each item's Tab stop on its roving index, and the popover content under 'More' inherits that gate. A focus that arrives by AT-SPI grab\_focus or by pointer does not move the index, so first\_focusable\_descendant finds nothing in the MenuList. With this change, focus stays on the named 'More' push button (2 of 2) instead of moving to the unnamed host, but it still does not reach the menu. Fix options: (a) make the Toolbar's roving index follow focus; (b) have first\_focusable\_descendant ignore tab\_stop gates above the root it searches; (c) stop tab\_stop\_effective's ancestor walk at an overlay content root. (b) alone would still leave Tab inside such a menu restarting at the top of the window.
- **Where:** crates/teksilo-widgets/src/toolbar.rs:335-339 with crates/teksilo-widgets/src/popover\_widget.rs:683 (focus target)
- **Evidence:**
  - `V/verify-docking-header-dropdown-20260925-145424-1148195: 'Enter: open the dropdown' '+39.1 ms object:children-changed:add [unknown] 'More' -> [unknown] ''' '+40.1 ms object:state-changed:focused 1 [unknown] ''' 'FAIL  focus lands on [menu] '*'' 'FAIL  Orca says 'menu''`
  - `same run, tree-Enter--open-the-dropdown.txt: '[unknown] 'More'' '  [push button] 'More' desc='More' {focusable}' '  [unknown] '' {focusable,focused}' '    [menu] '' {focusable}' '      [menu item] 'Sort by name'' '      [menu item] 'Sort by date''`
  - `V/verify-docking-header-dropdown-20260925-145458-1148195 orca: '14:55:11.045814 EVENT MANAGER: object:state-changed:focused for [unknown] in [application: 'docking'] (1, 0, 0) is not obsoleted' '14:55:11.107877 NULL SPEECH: stop' (no SPEECH OUTPUT)`
  - `verify-docking-menu-reopen (3 runs): 'Enter: open the More dropdown, time 1' 'FAIL  focus lands on [menu] '*'' '+41.0 ms object:state-changed:focused 1 [unknown] '''`
  - `source: crates/teksilo-widgets/src/toolbar.rs:335-339 (PopoverIconButton::new(btn).bare().content(menu())); popover_widget.rs:683-684 and 779 (request_focus(content_id) lands on the first focusable descendant, which here is the unnamed wrapper). Which widget contributes the focusable wrapper was not pinned down.`
- **Reproduced:** 5 of 5 openings across 5 runs (2 header-dropdown runs, time 1 in 3 menu-reopen runs)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Send focus to the MenuList itself (Role::Menu), as the ⋮ options menu does, or make the wrapper not focusable.

### docking-M3 {#docking-m3}

The example's status line is not a live region: header actions, Export and Restore report their result only there, and a reader hears nothing

- **Example:** docking
- **Scenario:** verify-docking-status-line
- **Act:** verify-docking-status-line: Space on New File (Explorer header action), then Space on Export
- **The reader should get:** The result ('New File (demo action)', 'Exported layout.') is announced.
- **The reader gets:** The status label's text and name change (text-changed, accessible-name), but there is no object:announcement, and Orca says nothing in either act.
- **Platform:** Linux measured; the example's widget choice, so every platform
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/docking/src/main.rs:404-407
- **Evidence:**
  - `V/verify-docking-status-line-20260925-145029-932995: '+42.4 ms object:text-changed:insert [label] ... text='New File (demo acti...' '+42.6 ms object:property-change:accessible-name [label] 'New File (demo action)'' 'FAIL  a object:announcement event from [*] '*''; Export act: '+42.0 ms object:property-change:accessible-name [label] 'Exported layout.'' 'FAIL  a object:announcement event'`
  - `source: examples/docking/src/main.rs:404-407 (TextWidget bound to self.status, no access_live)`
- **Reproduced:** 1 of 1 (not timing dependent: no announcement event is ever emitted)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Give the status line .access\_live(Live::Polite), or announce through ctx.announce.
