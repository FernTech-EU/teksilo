<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Generated from the sweep of 25 and 26 September 2026 (on 261a218f) and its re-measure of 27 September 2026 (on c198e4d1). See ../reader-findings.md. -->

# Scenes, animation and the rest

Examples: `scene-showcase`, `scene-corkboard`, `scene-magnetism`, `scene-ink`, `over-constraint`, `theme-styles`, `animations`, `animations-kit`, `async-demo`, `touch-playground`, `teksilo-widgets-previewer`.
32 findings: 2 critical, 11 high, 11 medium, 8 low.
Swept on `261a218f` on 25 and 26 September 2026, measured again on
`c198e4d1` on 27 September 2026.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [sceneetc-01](#sceneetc-01) | scene-showcase | Lightweight scene items cannot be focused, selected, activated or moved from the keyboard or by a screen reader | critical | Linux | open |
| [sceneetc-02](#sceneetc-02) | teksilo-widgets-previewer | Previewer navigator rows are unnamed 'section's that Enter, Space and AT-SPI click cannot activate | critical | Linux | open |
| [sceneetc-03](#sceneetc-03) | animations-kit | A control that scrolls out of view and back is defunct to Orca, so focusing it again is silent | high | Linux | fixed |
| [sceneetc-04](#sceneetc-04) | scene-ink | scene-ink: the focusable page is an unnamed 'section' that swallows the window, and from the second Tab round Orca says nothing | medium | Linux | partly fixed |
| [sceneetc-05](#sceneetc-05) | scene-corkboard | Corkboard: Tab into a card's prose lands on an unnamed 'section', Tab writes a tab character into the note, and Esc does nothing | high | Linux | partly fixed |
| [sceneetc-06](#sceneetc-06) | scene-corkboard | Corkboard: Enter on a card in the main pane sends focus into the Overview pane's copy of its editor | high | Linux | open |
| [sceneetc-07](#sceneetc-07) | scene-magnetism | Magnet keyboard connect flow gives a reader no feedback: no mode, no pending source, no connection | high | Linux | open |
| [sceneetc-08](#sceneetc-08) | scene-magnetism | The FlowTo relation that records a connection reaches no screen reader; the wire is just 'connection' | high | all | upstream |
| [sceneetc-09](#sceneetc-09) | scene-showcase | The SceneView is a focusable, unnamed panel: Orca says 'panel.' and in the showcase reads out 2331 characters | high | Linux | open |
| [sceneetc-10](#sceneetc-10) | scene-showcase | The minimap's reading of the viewport never reaches Linux: on focus Orca says only 'Scene minimap panel.' | medium | Linux | partly fixed |
| [sceneetc-11](#sceneetc-11) | theme-styles | A Toggle's new state reaches the bus only at the next unrelated update: Space is silent, and 'pressed' arrives cut on leaving | high | Linux | fixed |
| [sceneetc-12](#sceneetc-12) | touch-playground | SegmentedControl's current segment is read as 'not selected radio button' | high | Linux | fixed |
| [sceneetc-13](#sceneetc-13) | async-demo | async-demo: neither the start nor the completion of an async task is spoken | high | Linux | partly fixed |
| [sceneetc-14](#sceneetc-14) | teksilo-widgets-previewer | Previewer knob editors and variant radios have no name | high | Linux | open |
| [sceneetc-15](#sceneetc-15) | scene-showcase | Unnamed controls in the examples: combo boxes, card editors, a list box, progress bars | high | Linux | open (example) |
| [sceneetc-16](#sceneetc-16) | animations-kit | A live progress bar is announced again every time it scrolls into view | medium | Linux | open |
| [sceneetc-17](#sceneetc-17) | scene-corkboard | Corkboard 'Add Act': one press, eight announcements | medium | Linux | open |
| [sceneetc-18](#sceneetc-18) | animations-kit | Collapsed and faded-out content is still read as present (animations-kit) | medium | Linux | open |
| [sceneetc-19](#sceneetc-19) | scene-corkboard | Corkboard: the Overview pane repeats every card and editor in the Tab ring | low | Linux | open (example) |
| [sceneetc-20](#sceneetc-20) | scene-showcase | Hundreds of unnamed decorative scene items in the tree (562 unnamed 'panel's on the corkboard) | medium | Linux | open |
| [sceneetc-21](#sceneetc-21) | teksilo-widgets-previewer | Previewer theme, density and locale pickers do not say which choice is current | medium | all | open |
| [sceneetc-22](#sceneetc-22) | scene-corkboard | SceneCard's Focus action does not select the card, so the keyboard route to the transform handles is out of reach | medium | Linux | open |
| [sceneetc-23](#sceneetc-23) | scene-ink | scene-ink: the ink is invisible to assistive technology, and Backspace erases it silently | medium | all | open (example) |
| [sceneetc-24](#sceneetc-24) | touch-playground | touch-playground says 'a screen reader is told once' about a density switch; nothing is told | low | all | open (example) |
| [sceneetc-25](#sceneetc-25) | animations-kit | A rotating Cycle puts remove/add/defunct on the bus every period while idle | low | Linux | open |
| [sceneetc-26](#sceneetc-26) | scene-magnetism | Magnet ports are 'push button's that cannot be pressed | medium | Linux | open |
| [sceneetc-27](#sceneetc-27) | scene-showcase, scene-corkboard, scene-magnetism, scene-ink, over-constraint, theme-styles, animations, animations-kit, async-demo, touch-playground, teksilo-widgets-previewer | SceneCard's 'Edit' action and the transform handles' step actions are invisible on Linux | low | Linux | upstream |
| [sceneetc-28](#sceneetc-28) | scene-showcase, scene-corkboard, scene-magnetism, scene-ink, over-constraint, theme-styles, animations, animations-kit, async-demo, touch-playground, teksilo-widgets-previewer | touch-playground list rows carry an \[unknown\]-role child that repeats the row's name | low | Linux | open |
| [sceneetc-M1](#sceneetc-m1) | animations-kit | Blur's 'click-to-reveal sensitive content' is read out in full by a screen reader while it is obscured | medium | Linux | open |
| [sceneetc-M2](#sceneetc-m2) | scene-corkboard | Transform-mode keyboard steps are silent; only the commit speaks, as 'Moved 1 item' | low | Linux | open |
| [sceneetc-M3](#sceneetc-m3) | scene-showcase | Arrow-key panning of a focused SceneView gives the reader no feedback | low | Linux | open |
| [sceneetc-M4](#sceneetc-m4) | animations-kit | animations-kit: the Shake 'invalid input' demo says nothing to a reader | low | Linux | open (example) |

### sceneetc-01 {#sceneetc-01}

Lightweight scene items cannot be focused, selected, activated or moved from the keyboard or by a screen reader

- **Example:** scene-showcase
- **Scenario:** sceneetc-showcase
- **Act:** scene-showcase: Tab onto the scene, then AT-SPI click and AT-SPI grab\_focus on the lightweight item 'draggable 1' (the page says 'Drag a drag me rect: MOVE')
- **The reader should get:** A reader can move to an item, hear it, select it and move it. A route that is not a drag (WCAG 2.1.1, 2.5.7) should reach every item a pointer can select or drag.
- **The reader got (`261a218f`):** Items are exposed as \[panel\] nodes with a name and nothing else: no Action interface, no focusable, selectable or selected state. The AT-SPI click is refused, grab\_focus produces no focus change, and Tab never enters the items (the scene pane is one tab stop and its arrows pan). Selection comes only from a pointer click or marquee, so the Alt+Arrow keyboard nudge has nothing to move.
- **Platform:** Linux AT-SPI/Orca measured. The node shape (no actions, no selected state, no focus route) is emitted by Teksilo, so UIA and macOS get the same.
- **Severity:** critical; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Lightweight scene items are still bare \[panel\] nodes with a name and nothing else. An AT-SPI click on 'draggable 1' is refused, and grab\_focus on it moves focus to the unnamed scene pane (or, from inside the pane, changes nothing). Tab never enters the items, so a reader cannot select or move one.
- **Measured again:** sceneetc-showcase 2 of 2 runs, verify-sceneetc-item-focus 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-showcase 'AT-SPI click on a lightweight item': FAIL a lightweight item can be activated: refused: {'path': '/org/a11y/atspi/accessible/0/250750518664849208675581914959463841792', 'name': 'draggable 1', 'role': 'panel'} offers no action on AT-SPI`
  - `pass1 sceneetc-showcase 'AT-SPI grab_focus on a lightweight item': +42.5 ms == harness:grab-focus  [panel] 'draggable 1' / FAIL focus lands on [panel] 'draggable 1': no focus change on the bus in this act`
  - `pass1 sceneetc-showcase tree-launch.txt: [panel] 'draggable 1' with no state, under [panel] '' {focusable}`
  - `pass1 verify-sceneetc-item-focus 'AT-SPI grab_focus on the lightweight item 'draggable 1'': +1592.0 ms object:state-changed:focused 1 [panel] '' / ORCA SAYS: 'panel.' / FAIL focus lands on the item itself: last focus: [panel] '' path ...4332763054669824`
  - `pass2 verify-sceneetc-item-focus 'AT-SPI grab_focus on 'draggable 2' (focus now on the pane)': FAIL focus lands on the item itself: no focus change on the bus in this act`
  - `pass2 sceneetc-showcase: same refusal and same 'no focus change on the bus in this act'`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/view/a11y\_impl.rs:600-630, crates/teksilo-scene/src/items.rs:100-150, crates/teksilo-scene/src/view/gestures\_impl.rs:1133-1181; crates/teksilo-core/src/widget\_tree/pointer\_router.rs:1177-1227
- **Evidence (`261a218f`):**
  - `refused: {'path': '/org/a11y/atspi/accessible/0/250750518664849208675581914959463841792', 'name': 'draggable 1', 'role': 'panel'} offers no action on AT-SPI`
  - `+61.0 ms == harness:grab-focus  [panel] 'draggable 1'  -> FAIL  focus lands on [panel] 'draggable 1': no focus change on the bus in this act`
  - `tree: {'name': 'draggable 1', 'role': 'panel', 'interfaces': ['Accessible', 'Component'], states enabled/sensitive/showing/visible only}`
  - `crates/teksilo-scene/src/items/rect.rs:281-287: accessibility() sets Role::GraphicsObject and the label only`
  - `crates/teksilo-scene/src/view/a11y_impl.rs:625: item.accessibility(child, &ctx) then bounds; no selected state, no action`
  - `crates/teksilo-scene/src/flags.rs:47: IS_FOCUSABLE, documented 'Declared only ... nothing reads this bit'`
  - `crates/teksilo-scene/src/view/builder_impl.rs:367: focus_order: 'Apps wire this to a Tab / Shift+Tab handler': there is no built-in item traversal`
  - `crates/teksilo-scene/src/view/gestures_impl.rs:1134: Alt+Arrow nudges selection_for_keys.selected(), and no keyboard path selects a lightweight item`
  - `verify-sceneetc-item-focus (3 runs): 'AT-SPI grab_focus on the lightweight item draggable 1' -> last focus: [panel] '' path ...4332763054669824 (the SceneView), Orca 'panel.' + the 2331-char dump; 'grab_focus on draggable 2' (pane already focused) -> no focus change on the bus in this act`
  - `target/reader-verify/scene-etc/sceneetc-showcase-20260925-152908-1492203 launch tree: {'role': 'panel', 'name': 'draggable 1', 'states': [enabled, sensitive, showing, visible], 'interfaces': ['Accessible', 'Component'], 'actions': None}`
  - `crates/teksilo-core/src/widget_tree/pointer_router.rs:1152-1204 (Focus serviced by the core on the resolved widget) + accessibility_impl.rs:305-306 (synthetic node -> owning widget)`
  - `docs/teksilo-scene-a11y.md:13 'Tab cycles in scene-insertion order' (false); crates/teksilo-scene/src/view/builder_impl.rs:361-386`
- **Reproduced:** click refused 3 of 3 runs; grab\_focus with no focus change 2 of 2 runs that had the act (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 runs for the click refusal and for no focus change from inside the scene; 3 of 3 runs (verify-sceneetc-item-focus) for focus landing on the SceneView pane from outside
- **Fix idea:** Give selectable or draggable lightweight items a roving virtual focus (active\_descendant, as magnets and transform handles already have), driven by Tab or arrows through focus\_in\_direction. Publish their selected state, route Action::Click/Focus to SceneSelection, and keep Alt+Arrow as the move.

### sceneetc-02 {#sceneetc-02}

Previewer navigator rows are unnamed 'section's that Enter, Space and AT-SPI click cannot activate

- **Example:** teksilo-widgets-previewer
- **Scenario:** sceneetc-previewer-nav
- **Act:** teksilo-widgets-previewer: Tab from fr-FR into the navigator, then Enter, Space, and AT-SPI click on the focused row, then Tab to the next rows
- **The reader should get:** Each row reads as its widget ('TextScaleControl', 'default' ...) with its selected state, and Enter, Space or a reader's activation opens it in the canvas.
- **The reader got (`261a218f`):** Every row is read as 'section.'. Enter, Space and the AT-SPI click leave the canvas at 'radio\_group · default', and the click is refused. Each Tab turns the previous row defunct. The whole catalog can only be browsed with a pointer.
- **Platform:** Linux AT-SPI/Orca measured. No keyboard activation exists at all, so every platform is affected.
- **Severity:** critical; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Every navigator row is still read as 'section.'. Enter, Space and an AT-SPI click do not open the row's widget (the canvas stays on 'password-field · default'), and the click is refused. Each Tab still marks the row just left defunct. The catalog can only be browsed with a pointer.
- **Measured again:** sceneetc-previewer-nav, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-previewer-nav 'Tab into the navigator': +79.6 ms object:state-changed:focused 1 [section] '' / ORCA SAYS: 'section.'`
  - `pass1 sceneetc-previewer-nav 'Enter on the navigator row': FAIL the tree holds [*] 'text_scale_control': no such node in the tree after the act (tree-Enter-on-the-navigator-row.txt: [label] 'password-field · default')`
  - `pass1 sceneetc-previewer-nav 'AT-SPI click on the focused navigator row': refused: {'path': '/org/a11y/atspi/accessible/0/79228164469619209406756421632', 'name': '', 'role': 'section'} offers no action on AT-SPI`
  - `pass1 sceneetc-previewer-nav 'Tab to navigator row 2': +60.5 ms object:state-changed:defunct 1 [section] '' / +60.7 ms object:state-changed:focused 1 [section] '' / ORCA SAYS: 'section.'`
  - `pass2 sceneetc-previewer-nav: the same in every act ('section.', Enter, Space and the click all leave the canvas unchanged)`
- **Where (`c198e4d1`):** crates/teksilo-preview-ui/src/navigator.rs:147-154, 207-215
- **Evidence (`261a218f`):**
  - `15:05:12.999376 object:state-changed:focused 1 [section] ''`
  - `ORCA 15:05:13.308984 SPEECH OUTPUT: 'section.'`
  - `Enter on the navigator row: FAIL  the tree holds [*] 'text_scale_control' (canvas label stays 'radio_group · default')`
  - `refused: {'path': '/org/a11y/atspi/accessible/0/79228164469619209406756421632', 'name': '', 'role': 'section'} offers no action on AT-SPI`
  - `Tab to navigator row 2: object:state-changed:defunct 1 [section] '' / EVENT MANAGER: Ignoring defunct object: [section]`
  - `crates/teksilo-preview-ui/src/navigator.rs:148-154 and 208-215: rows are MaxSize (GenericContainer) + on_tap + .focusable(true), with no role, name, selected state, key handler or Click action`
  - `sceneetc-previewer-nav-20260925-154307-1585894: 15:43:19.335527 object:state-changed:focused 1 [section] '' / ORCA 15:43:19.620992 SPEECH OUTPUT: 'section.'`
  - `same run, Tab to row 2: 15:43:35.875191 object:state-changed:defunct 1 [section] '' / ORCA 15:43:36.174568 EVENT MANAGER: Ignoring defunct object: [section]`
  - `tree after Enter/Space/click: [label] 'radio_group · default' (tree-Enter-on-the-navigator-row.txt:64)`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Build the navigator on TreeView/ListView, or give each row Role::TreeItem or ListItem with its label as name, selected state, an Enter/Space on\_key and Action::Click.

### sceneetc-03 {#sceneetc-03}

A control that scrolls out of view and back is defunct to Orca, so focusing it again is silent

- **Example:** animations-kit
- **Scenario:** sceneetc-animkit
- **Act:** animations-kit: Tab from Toggle Fade to 'Hover or hold me', Tab on four times (it scrolls out of the ScrollArea), then Shift+Tab back four times to 'Hover or hold me'
- **The reader should get:** Coming back to a control, the reader hears 'Hover or hold me push button' as the first time.
- **The reader got (`261a218f`):** When the button scrolls out, accesskit\_consumer's clip filter drops it from the filtered tree and the AT-SPI adapter marks it defunct. When it scrolls back it returns under the same id, which libatspi still holds as defunct. Orca ignores its focus event and says nothing. The same mechanism as K2, triggered by any scrolling rather than the announcer.
- **Platform:** Linux AT-SPI/Orca measured (libatspi keeps the defunct object for a reused path). Not measured on Windows or macOS.
- **Severity:** high; **layer:** upstream
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** A control that scrolls out of a ScrollArea and back is heard again. After four Tabs down and four Shift+Tabs back, Orca says 'Hover or hold me push button.', and a later grab\_focus on 'Toggle content' is spoken too. The adapter still marks a node that leaves the filtered tree defunct and never clears it, but Teksilo now hands a returning node a new id.
- **Measured again:** sceneetc-animkit, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-animkit 'Shift+Tab back to Hover or hold me (the bar scrolls back in)': +2660.4 ms object:state-changed:focused 1 [push button] 'Hover or hold me' / ORCA SAYS: 'Hover or hold me push button.'`
  - `pass2 sceneetc-animkit same act: +2824.7 ms object:state-changed:focused 1 [push button] 'Hover or hold me' / ORCA SAYS: 'Hover or hold me push button.'`
  - `pass1 sceneetc-animkit '(scene) focus Toggle content': ORCA SAYS: 'Toggle content push button.'`
  - `pass2 sceneetc-animkit '(scene) focus Toggle content': ORCA SAYS: 'Toggle content push button.'`
  - `accesskit_atspi_common-0.21.0/src/adapter.rs:91-116: remove_node still emits StateChanged(Defunct, true) at 105; add_node (49-83) never clears it`
- **Where (`c198e4d1`):** accesskit\_atspi\_common-0.21.0/src/adapter.rs:49-116; accesskit\_consumer-0.39.1/src/filters.rs:54-90; Teksilo side: crates/teksilo-widgets/src/scroll\_area.rs:1305; the workaround: crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-336, called from crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs:196
- **Evidence (`261a218f`):**
  - `15:06:55.855995 object:state-changed:defunct 1 [push button] 'Hover or hold me'  (focus then on 'Toggle banner')`
  - `15:07:03.878074 object:children-changed:add 1 [panel] '' -> [push button] 'Hover or hold me'`
  - `15:07:03.881830 object:state-changed:focused 1 [push button] 'Hover or hold me'`
  - `ORCA 15:07:03.936774 EVENT MANAGER: Ignoring defunct object: [push button: 'Hover or hold me']  (no SPEECH OUTPUT for the act's last stop)`
  - `accesskit_consumer-0.39.0/src/filters.rs:64-86: a child of a clips_children parent whose box leaves the parent's (and whose neighbours' do too) is ExcludeSubtree`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:103-104: remove_node emits StateChanged(Defunct, true); add_node never clears it for the same id`
  - `sceneetc-animkit-20260925-154345-1585895 '(scene) focus Toggle content': 15:44:30.655150 EVENT MANAGER: object:state-changed:focused for [push button: 'Toggle content'] ... 15:44:30.676118 FOCUS MANAGER: Setting locus of focus to existing locus of focus (no SPEECH OUTPUT), 15:44:30.676348 EVENT MANAGER: Ignoring defunct object: [push button: 'Hover or hold me']`
  - `crates/teksilo-widgets/src/scroll_area.rs:1305 is the only builder.inner_mut().set_clips_children() in teksilo-core/teksilo-widgets`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Teksilo cannot change the adapter. It could stop ScrollArea/SceneView publishing clips\_children to AT so off-screen content stays in the filtered tree, or mint a fresh NodeId for a node re-entering the filtered tree (as the K2 fix does for the announcer). Report the defunct-revival to AccessKit.

### sceneetc-04 {#sceneetc-04}

scene-ink: the focusable page is an unnamed 'section' that swallows the window, and from the second Tab round Orca says nothing

- **Example:** scene-ink
- **Scenario:** sceneetc-ink
- **Act:** scene-ink: Tab four times
- **The reader should get:** Each Tab lands on a named stop and is read.
- **The reader got (`261a218f`):** Tab 1 turns the root VStack (a focusable GenericContainer) into a \[section\] '' that takes the frame's labels and scene as children, and Orca says 'section.'. Tab 2 excludes it again (defunct) and focuses the unnamed scene 'panel.'. Tab 3 refocuses the section under its old id: Orca ignores it as defunct and is silent. Tab 4 returns to the panel, and Orca is silent again because its locus never left the panel. The reader hears nothing on any Tab after the first round.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** medium; **layer:** framework
- **Severity in the sweep:** high. Every Tab is now read. The reader still cannot tell what the page and the scene are, since both are unnamed, but nothing is silent any more.
- **Status:** Partly fixed by `85624a1a`, found by the re-measure: that commit did not report it. What remains is under **Now**.
- **Now (`c198e4d1`):** The silence is gone: every Tab is now spoken, and Orca drops only the focus loss of the section as it leaves. What remains: the focusable page is still an unnamed \[section\] '' that takes the window's labels and the scene as children, so Tab 1 and Tab 3 say just 'section.', and Tab 2 and Tab 4 say 'panel.' followed by the two notes' text.
- **Measured again:** sceneetc-ink, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-ink 'Tab 1': +27.1 ms object:children-changed:add [frame] 'Teksilo — scene ink' -> [section] '' / ORCA SAYS: 'section.'`
  - `pass1 sceneetc-ink 'Tab 3': +10.6 ms object:state-changed:focused 1 [section] '' / ORCA SAYS: 'section.'`
  - `pass1 sceneetc-ink 'Tab 4': +11.5 ms object:state-changed:focused 1 [panel] '' / ORCA SAYS: 'panel.' / ORCA SAYS: 'Under the ink (z = 0) A heavyweight card. Draw across it: the stroke covers it. Over the ink (z = 20) Also a card, one step higher. Draw across it: it covers the stroke.'`
  - `pass1 sceneetc-ink 'Tab 2': 16:27:24.489318 EVENT MANAGER: Ignoring defunct object: [section] (orca-debug.out: the object:state-changed:focused (0, 0, 0) of the section that just left)`
  - `pass2 sceneetc-ink: 'section.', 'panel.', 'section.', 'panel.' on Tabs 1 to 4`
- **Where (`c198e4d1`):** crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:918-927; trigger examples/scene\_ink/src/main.rs:464
- **Evidence (`261a218f`):**
  - `15:07:28.795702 object:children-changed:add 0 [frame] '' -> [section] ''`
  - `15:07:28.797018 object:children-changed:remove -1 [frame] '' -> [panel] ''`
  - `ORCA 15:07:28.861710 SPEECH OUTPUT: 'section.'`
  - `15:07:36.575436 object:state-changed:focused 1 [section] ''  / ORCA 15:07:36.577055 EVENT MANAGER: Ignoring defunct object: [section]  (no speech)`
  - `15:07:40.464715 object:state-changed:focused 1 [panel] ''  / 15:07:40.475017 - FOCUS MANAGER: Setting locus of focus to existing locus of focus  (no speech)`
  - `examples/scene_ink/src/main.rs:464: VStack ... .focusable(true).on_key(Backspace)`
  - `accesskit_consumer filters.rs:17-34: a GenericContainer is kept only while focused`
  - `sceneetc-ink-20260925-154308-1585895 Tab 3: 15:43:24.209164 object:state-changed:focused 1 [section] '' / ORCA 15:43:24.210490 EVENT MANAGER: Ignoring defunct object: [section] (no speech)`
  - `same run Tab 4: 15:43:28.097276 object:state-changed:focused 1 [panel] '' (no speech)`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Framework: when a node is focusable but its role is GenericContainer, publish a real role (e.g. Group) and warn when it has no name. Then it never flips in and out of the filtered tree. Example: give the page a role and label, or put Backspace on the SceneView.

### sceneetc-05 {#sceneetc-05}

Corkboard: Tab into a card's prose lands on an unnamed 'section', Tab writes a tab character into the note, and Esc does nothing

- **Example:** scene-corkboard
- **Scenario:** sceneetc-corkboard-cards
- **Act:** scene-corkboard: Tab onto the card 'Act I — Opening', Tab into its prose, type 'zz', Tab, Escape, Ctrl+Tab, Shift+Tab back
- **The reader should get:** Focus lands on a named editable text. Tab (or at least Esc) leaves the note for the next card. Coming back is read.
- **The reader got (`261a218f`):** Focus goes to the RichTextEditor's focusable wrapper, \[section\] '', and Orca says 'section.'. Typed text changes the entries (both panes') that do not have focus. Tab inserts '\\t' into the note, silently, in both panes' copies. Esc and Shift+Tab produce no event. Only Ctrl+Tab leaves. Shift+Tab back into the same prose refocuses the defunct section and Orca says nothing.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** high; **layer:** framework
- **Status:** Partly fixed by `731cc2e1` (editor-focus). What remains is under **Now**.
- **Now (`c198e4d1`):** Tab into a card's prose now lands on the \[entry\] that holds the text, and Orca reads it ('entry An ordinary morning. The '). Typed text changes the focused entry, and coming back with Shift+Tab is read. Still wrong: the entry has no name, Tab silently writes a tab character into the note (in both panes' copies), and Esc and Shift+Tab do nothing, so only Ctrl+Tab leaves the note.
- **Measured again:** sceneetc-corkboard-cards, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-corkboard-cards 'Tab into the card's prose': +41.6 ms object:state-changed:focused 1 [entry] '' / ORCA SAYS: 'entry An ordinary morning. The '`
  - `pass1 sceneetc-corkboard-cards 'type a word': pass the text that changes is the node focus is on`
  - `pass1 sceneetc-corkboard-cards 'Tab in the prose': +231.1 ms object:text-changed:insert [entry] '' text='\t' / +231.4 ms object:text-changed:insert [entry] '' text='\t' / FAIL focus lands on [panel] 'Inciting Incident': no focus change on the bus in this act`
  - `pass1 sceneetc-corkboard-cards 'Escape': ORCA SAYS: 'escape' / FAIL focus lands on [panel] 'Act I — Opening': no focus change on the bus in this act`
  - `pass1 sceneetc-corkboard-cards 'Shift+Tab back to the first card's prose': ORCA SAYS: 'Act I — Opening panel.' / ORCA SAYS: 'entry zz\tAn ordinary morning. '`
  - `pass1 sceneetc-corkboard-cards 'Shift+Tab to the first card': FAIL focus lands on [panel] 'Act I — Opening': no focus change on the bus in this act`
  - `pass2 sceneetc-corkboard-cards: the same in every act`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/rich\_text.rs:3886 (the focusable wrapper), 4321-4336 (GenericContainer role, now with accessibility\_proxy to the body); crates/teksilo-widgets/src/rich\_text/keyboard.rs:430-500; crates/teksilo-scene/src/scene\_card.rs:995-1012
- **Evidence (`261a218f`):**
  - `15:05:37.776899 object:state-changed:focused 1 [section] ''  / ORCA 15:05:37.851680 SPEECH OUTPUT: 'section.'`
  - `15:05:41.655266 object:text-changed:insert 0 [entry] ''  'z'  (focus path ...02035801260032, entry paths ...12743370457088 and ...13843561447424)`
  - `15:05:45.600978 object:text-changed:insert 2 [entry] ''  '\t'  (Tab; no focus change)`
  - `Escape: no event, Orca said nothing`
  - `15:05:57.176637 object:state-changed:focused 1 [section] ''  / ORCA 15:05:57.177796 EVENT MANAGER: Ignoring defunct object: [section]`
  - `crates/teksilo-widgets/src/rich_text/state.rs:376: the wrapper is the .focusable(true) node (a GenericContainer); the text sweep sees the same in rich-text-editor`
  - `crates/teksilo-scene/src/scene_card.rs:1001-1012: Esc from the body only deactivates when mode == Editing, which Tab-in never sets`
  - `tabwalk-touch-playground-20260925-154451-1604199 Tab 13 -> [section] '' 'section.', Tab 14 -> object:text-changed:insert [entry] '' text='\t' (no focus change)`
  - `crates/teksilo-widgets/src/rich_text/keyboard.rs:430-462 'Otherwise swallow (prevents focus-navigation bleed)'`
- **Reproduced:** 3 of 3 runs (silent return 2 of 2 runs that had the act)
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Put focus on the editor's MultilineTextInput node (the text agent's root cause). SceneCard should let Esc return to the card whenever focus is in the body, and could keep the body out of the Tab ring until Enter. Name the editor after the card.

### sceneetc-06 {#sceneetc-06}

Corkboard: Enter on a card in the main pane sends focus into the Overview pane's copy of its editor

- **Example:** scene-corkboard
- **Scenario:** sceneetc-corkboard-transform
- **Act:** scene-corkboard: focus the main pane's card 'Act I — Opening', press Enter (edit), then Escape
- **The reader should get:** Focus goes into this card's prose in the main pane, and Esc brings it back to the card.
- **The reader got (`261a218f`):** Focus first goes to the main card's editor, which turns defunct, and then jumps to the overview pane's editor. Orca says 'landmark Overview pane / Act I — Setup panel. / Act I — Opening. / section.'. Esc afterwards produces no event at all.
- **Platform:** Linux AT-SPI/Orca measured. The focus move is Teksilo's own, so every platform is affected.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Enter on the main pane's card still sends focus into the main card's editor and then, about 60 ms later, into the Overview pane's copy. Orca says 'landmark Overview pane', 'Act I — Setup panel.', 'Act I — Opening.', 'entry An ordinary morning. The '. Esc afterwards produces no event. The only change is that the editor is now read as an entry rather than 'section.'.
- **Measured again:** sceneetc-corkboard-transform 2 of 2 runs, verify-sceneetc-transform 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-corkboard-transform 'Enter on the main pane's card (edit)': +42.0 ms object:state-changed:focused 1 [entry] '' / +104.4 ms object:state-changed:focused 1 [entry] '' / ORCA SAYS: 'landmark Overview pane' / ORCA SAYS: 'Act I — Setup panel.' / ORCA SAYS: 'Act I — Opening.' / ORCA SAYS: 'entry An ordinary morning. The ' / FAIL focus stays in the pane the user acted in (not the Overview pane): focus in the Overview pane: True`
  - `pass1 sceneetc-corkboard-transform 'Escape (back to the card, selected)': ORCA SAYS: 'escape' / FAIL focus lands on [panel] 'Act I — Opening': no focus change on the bus in this act`
  - `pass2 sceneetc-corkboard-transform: the same two acts, focus in the Overview pane: True, and Escape with no focus change`
  - `pass1 verify-sceneetc-transform 'Enter on the card (selects it, enters editing)': ORCA SAYS: 'landmark Overview pane' ... 'entry An ordinary morning. The '; 'Escape': ORCA SAYS: 'escape' with no focus event`
  - `pass2 verify-sceneetc-transform: the same`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/scene\_card.rs:252-291, 920-927, 1001-1012; trigger examples/scene\_corkboard/src/main.rs:185
- **Evidence (`261a218f`):**
  - `15:17:39.112900 object:state-changed:focused 1 [section] ''  (main card's editor, path ...02035801260032)`
  - `15:17:39.186462 object:state-changed:defunct 1 [section] ''`
  - `15:17:39.186768 object:state-changed:focused 1 [section] ''  (path ...03135992250368, inside [landmark] 'Overview pane')`
  - `ORCA 15:17:39.290138 SPEECH OUTPUT: 'landmark Overview pane'  / 15:17:39.290290 SPEECH OUTPUT: 'section.'`
  - `Escape (back to the card, selected): no event on the bus; FAIL focus lands on [panel] 'Act I — Opening'`
  - `crates/teksilo-scene/src/scene_card.rs:274-291: each view's EditFocusGate calls request_focus_into(body) on the transition to Editing when its own focus_within is false; the example shares one CardMode between both panes (examples/scene_corkboard/src/main.rs:185-188)`
  - `sceneetc-corkboard-transform-20260925-154601-1585896: 15:46:19.760767 focused 1 [section] '' (main) / 15:46:19.821138 defunct 1 [section] '' / 15:46:19.821371 focused 1 [section] '' (overview) / ORCA 15:46:19.951207 'landmark Overview pane', 'Act I — Setup panel.', 'Act I — Opening.', 'section.'`
  - `crates/teksilo-scene/src/scene_card.rs:920-927 (focus_within leaving -> mode Selected) + 1001-1012 (Esc only while Editing)`
- **Reproduced:** 4 of 4 runs
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs (sceneetc-corkboard-transform) and 3 of 3 (verify-sceneetc-transform); Esc afterwards silent 6 of 6 Real in 3 of 3 sweep-scenario runs and 3 of 3 of my probe runs. Enter on the main card focuses the main editor, which then turns defunct, and focus goes to the overview's copy ('landmark Overview pane / Act I — Setup panel. / Act I — Opening. / section.'). I add the mechanism, which also explains the dead Esc. The trigger is the example binding one CardMode signal into both panes' cards (examples/scene\_corkboard/src/main.rs:185 `.mode(card.mode.clone())`; CardData.mode is shared by both delegates, :139-156). Each instance's EditFocusGate pulls focus on the transition to Editing (scene\_card.rs:274-291). Once focus leaves the main card, its focus\_within effect commits the shared mode to Selected (scene\_card.rs:920-927). The Esc preview handler requires Editing (scene\_card.rs:1001-1012), so Esc in the overview editor does nothing. SceneCard::mode (scene\_card.rs:547-552) says any write of Editing focuses the body, and says nothing about several cards on one signal in a multi-view scene. Framework (an API contract that makes a shared mode self-defeating), triggered by the example. High stands: the reader is moved to the other pane and cannot Esc back.
- **Fix idea:** Only the card instance that was activated should pull focus: record which view or instance activated and gate EditFocusGate on it, rather than on the shared mode alone.

### sceneetc-07 {#sceneetc-07}

Magnet keyboard connect flow gives a reader no feedback: no mode, no pending source, no connection

- **Example:** scene-magnetism
- **Scenario:** sceneetc-magnet-connect
- **Act:** scene-magnetism: Tab to the graph, 'm', Enter on 'Input output', Right to 'Blur input', Enter, Escape
- **The reader should get:** 'm' says connect mode is on. Enter on a port says it is the pending source. Enter on the target says what was connected to what.
- **The reader got (`261a218f`):** 'm' moves the active descendant to 'Input output' ('Input panel. / Input output push button.'). That part works, but nothing says a mode began. Enter on the source produces no event and no speech: the pending state is not exposed at all. Enter on the target adds a node named 'connection' and nothing is spoken. Escape returns to the unnamed 'panel.'.
- **Platform:** Linux AT-SPI/Orca measured. No text or state is emitted, so every platform is affected.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** 'm' moves focus to 'Input output' and Orca says 'Input panel.' and 'Input output push button.', but nothing says connect mode began. Enter on the source produces no event and only the key echo 'return'. Enter on the target adds a \[panel\] 'connection' and nothing is spoken.
- **Measured again:** sceneetc-magnet-connect, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-magnet-connect 'm enters connect mode': +10.2 ms object:state-changed:focused 1 [push button] 'Input output' / ORCA SAYS: 'Input panel.' / ORCA SAYS: 'Input output push button.' / FAIL Orca says any of ('connect',)`
  - `pass1 sceneetc-magnet-connect 'Enter picks Input output as the source': ORCA SAYS: 'return' / FAIL Orca says any of ('source', 'selected', 'pressed', 'picked')`
  - `pass1 sceneetc-magnet-connect 'Enter connects the two ports': +9.3 ms object:children-changed:add [panel] '' -> [panel] 'connection' / FAIL Orca says any of ('connect',): Orca said nothing`
  - `pass2 sceneetc-magnet-connect: the same three acts, same speech`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/view/magnetism.rs:171-259; crates/teksilo-scene/src/view/a11y\_impl.rs:708-755
- **Evidence (`261a218f`):**
  - `15:08:10.242308 object:state-changed:focused 1 [push button] 'Input output'  / ORCA 15:08:10.306294 SPEECH OUTPUT: 'Input output push button.'`
  - `Enter picks Input output as the source: no event in the act; Orca said nothing`
  - `15:08:21.914171 object:children-changed:add 5 [panel] '' -> [panel] 'connection'  (no SPEECH OUTPUT in the act)`
  - `crates/teksilo-scene/src/view/magnetism.rs:183-195 (mode toggle), 209-224 (pending.set / on_connect): no ctx.announce and no state published for the pending magnet`
  - `sceneetc-magnet-connect-20260925-152823-1492203: 'Enter picks Input output as the source' -> no event, Orca said nothing; 'Enter connects the two ports' -> +6.8 ms object:children-changed:add [panel] '' -> [panel] 'connection', no SPEECH OUTPUT`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Announce the mode change and the connection through ctx.announce (overridable text). Publish the pending source as a state (pressed or selected) on its SceneMagnet node. Give MagnetismConfig a hook that phrases 'X connected to Y'.

### sceneetc-08 {#sceneetc-08}

The FlowTo relation that records a connection reaches no screen reader; the wire is just 'connection'

- **Example:** scene-magnetism
- **Scenario:** sceneetc-magnet-connect
- **Act:** scene-magnetism: connect Input output to Blur input with the keyboard, then read the tree
- **The reader should get:** A reader can find out what is connected to what (the example says the FlowTo relation 'reads to a screen reader').
- **The reader got (`261a218f`):** The 'Input' node carries no relation on AT-SPI. The only trace is a new \[panel\] 'connection' that names neither end.
- **Platform:** All: from source, accesskit\_atspi\_common exports only ControllerFor, and accesskit\_windows, accesskit\_macos and accesskit\_consumer never read flow\_to. Linux measured.
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** After a keyboard connection the 'Input' node still carries no relation on AT-SPI, and the only trace is a new \[panel\] 'connection' that names neither end. The current adapters still export no flow-to relation on any platform.
- **Measured again:** sceneetc-magnet-connect 2 of 2 runs; adapters read from source
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-magnet-connect 'Enter connects the two ports': pass the tree holds [panel] 'connection' / FAIL the source node carries a flows-to relation: [panel] 'Input': relations=None`
  - `pass2 sceneetc-magnet-connect same act: FAIL the source node carries a flows-to relation`
  - `accesskit_atspi_common-0.21.0/src/node.rs:972-989: relation_set inserts only RelationType::ControllerFor`
  - `grep for flow_to/FlowTo in accesskit_windows-0.35.1/src, accesskit_macos-0.27.1/src, accesskit_consumer-0.39.1/src, accesskit_atspi_common-0.21.0/src: no match`
- **Where (`c198e4d1`):** accesskit\_atspi\_common-0.21.0/src/node.rs:972-989; Teksilo docs crates/teksilo-scene/src/a11y.rs:96-98; examples/scene\_magnetism/src/main.rs:13, 122-157
- **Evidence (`261a218f`):**
  - `FAIL  the source node carries a flows-to relation: [panel] 'Input': relations=None`
  - `the tree holds [panel] 'connection'`
  - `accesskit_atspi_common-0.20.0/src/node.rs:959-977: relation_set inserts only RelationType::ControllerFor`
  - `grep 'flow' in accesskit_windows-0.35.0/src, accesskit_macos-0.27.0/src, accesskit_consumer-0.39.0/src: no match`
  - `examples/scene_magnetism/src/main.rs:144 (access_label 'connection'), 152 (add_a11y_relation FlowTo); crates/teksilo-scene/src/view/a11y_impl.rs:784`
  - `crates/teksilo-scene/src/a11y.rs:96-98 (FlowTo doc claims VoiceOver/NVDA follow it)`
  - `examples/scene_magnetism/src/main.rs:13 'relation so the connection reads to a screen reader'`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Name the wire from its ends ('Input output to Blur input'), or put the connection in each node's description. Treat A11yRelation::FlowTo as decoration until AccessKit exports it, and say so in its docs.

### sceneetc-09 {#sceneetc-09}

The SceneView is a focusable, unnamed panel: Orca says 'panel.' and in the showcase reads out 2331 characters

- **Example:** scene-showcase
- **Scenario:** sceneetc-showcase
- **Act:** Tab onto the scene in scene-showcase (also scene-magnetism, scene-corkboard's main pane, scene-ink)
- **The reader should get:** The scene is announced by a short name ('Story corkboard', 'Node graph').
- **The reader got (`261a218f`):** The pane has no name. In scene-showcase Orca says 'panel.' and then one 2331-character utterance, the text of every label in the scene. In the other examples it says just 'panel.'.
- **Platform:** Linux AT-SPI/Orca measured. The missing name is Teksilo's, so every platform is affected.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The scene pane still has no name. In scene-showcase Orca says 'panel.' and then one 2331-character utterance holding every label in the scene. In scene-magnetism and scene-corkboard's main pane it says 'panel.', and in scene-ink 'panel.' followed by both notes' text.
- **Measured again:** sceneetc-showcase, sceneetc-magnet-connect, sceneetc-corkboard-cards, sceneetc-ink: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-showcase 'Tab onto the scene': +27.2 ms object:state-changed:focused 1 [panel] '' / ORCA SAYS: 'panel.' / FAIL no utterance longer than 300 characters: 2331 characters: 'teksilo-scene showcase — eight labelled sections, all visible at zoom 1.0 Scroll wheel / two-finger trackpad: PAN.   Ctrl+wheel: ZOOM about viewport center.   P'…`
  - `pass2 sceneetc-showcase same act: ORCA SAYS: 'panel.' / FAIL no utterance longer than 300 characters`
  - `pass1 sceneetc-magnet-connect 'Tab into the scene': +41.7 ms object:state-changed:focused 1 [panel] '' / ORCA SAYS: 'panel.'`
  - `pass1 sceneetc-corkboard-cards 'Tab onto the main pane': +50.3 ms object:state-changed:focused 1 [panel] '' / ORCA SAYS: 'panel.'`
  - `pass1 sceneetc-ink 'Tab 2': ORCA SAYS: 'panel.' / ORCA SAYS: 'Under the ink (z = 0) A heavyweight card. Draw across it: the stroke covers it. Over the ink (z = 20) Also a card, one step higher. Draw across it: it covers the stroke.'`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/view/a11y\_impl.rs:151-160
- **Evidence (`261a218f`):**
  - `15:13:46.527011 object:state-changed:focused 1 [panel] ''`
  - `ORCA 15:13:46.712421 SPEECH OUTPUT: 'panel.'`
  - `ORCA 15:13:46.712510 SPEECH OUTPUT: 'teksilo-scene showcase — eight labelled sections, all visible at zoom 1.0 Scroll wheel / two-finger trackpad: PAN. ...' (2331 characters)`
  - `scene-magnetism 15:08: object:state-changed:focused 1 [panel] '' / SPEECH OUTPUT: 'panel.'`
  - `crates/teksilo-scene/src/view/a11y_impl.rs:157-160: Role::Pane, named only when a11y_label is set; gestures_impl.rs:1282 handlers.focusable(true)`
  - `examples: scene_showcase/src/main.rs:871, scene_magnetism/src/main.rs:201, scene_ink/src/main.rs:333, scene_corkboard/src/main.rs:709 (main pane): none sets .a11y_label`
  - `/usr/lib/python3/dist-packages/orca/formatting.py:395-397 (PANEL focused: ... + pause + unrelatedLabels)`
  - `orca/script_utilities.py:1771-1830 unrelatedLabels; orca/speech_generator.py:668-690 minimumWords=3 for panels`
  - `sceneetc-showcase-20260925-152908-1492203: 'Tab to the nested scene' -> ORCA 'landmark Inner scene.' only`
  - `sceneetc-ink runs, Tab 2: 'panel.' then 'Under the ink (z = 0) A heavyweight card. ... Over the ink (z = 20) Also a card ...' (3 of 3)`
- **Reproduced:** 3 of 3 runs in each of showcase, magnetism, corkboard, ink
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs (showcase, magnetism, corkboard, ink) The missing name is confirmed. SceneView is named only by .a11y\_label (a11y\_impl.rs:151-160), and only the nested scenes set one (scene\_showcase main.rs:734, the corkboard overview at main.rs:712). Showcase, magnetism, corkboard main pane and ink all say 'panel.' (3 of 3 each). But the 2331-character utterance is not caused by the missing name, and naming the pane will not stop it. Orca 46.1's focused-panel format is 'labelAndName + roleName + availability + pause + unrelatedLabels' (formatting.py:395-397). For a panel it collects every showing, relation-less label of at least 3 words underneath (speech\_generator.py:668-690, minimumWords=3; script\_utilities.py:1771-1830). SceneView is Role::Pane, which maps to AtspiRole::Panel (accesskit\_atspi\_common node.rs:221), and the scene's text items are labels. A named pane would read '&lt;name&gt; panel.' followed by the same dump. The nested scene (Role::Region -&gt; Landmark, node.rs:229) reads just 'landmark Inner scene.'. The same Orca behaviour makes scene-ink Tab 2 read both notes after 'panel.'. Fix both: give the pane a name, and use a role Orca does not expand (Region), or relate the labels to their items.
- **Fix idea:** Default the SceneView to a name (a debug\_assert or audit warning when a focusable SceneView has none), and have every example call .a11y\_label(tr!(...)).

### sceneetc-10 {#sceneetc-10}

The minimap's reading of the viewport never reaches Linux: on focus Orca says only 'Scene minimap panel.'

- **Example:** scene-showcase
- **Scenario:** sceneetc-showcase
- **Act:** scene-showcase: Tab onto the Scene minimap, then Right arrow three times
- **The reader should get:** On focus the reader hears where the viewport is (the minimap's only content). Each arrow tells where it moved.
- **The reader got (`261a218f`):** Focus: 'Scene minimap panel.' and nothing more. The reading is a string value on a Role::Group, which the AT-SPI adapter never exposes. The arrows announce through ctx.announce: press 1 is heard, presses 2 and 3 come from the defunct NodeId(1) and are dropped. That is K2, and its fix covers them.
- **Platform:** Linux measured. By source, Windows exposes a string value through the UIA ValuePattern (accesskit\_windows node.rs:592-597) and macOS through AXValue (accesskit\_macos node.rs:344-362), so this is Linux-only.
- **Severity:** medium; **layer:** upstream
- **Severity in the sweep:** high. The arrow announcements now all reach the reader, so the position is heard after one key press; only the reading on focus is still missing on Linux.
- **Status:** Partly fixed by `7545b207`, found by the re-measure: that commit did not report it. What remains is under **Now**.
- **Now (`c198e4d1`):** Focus on the minimap still reads only 'Scene minimap panel.': the viewport reading is a string value, which the AT-SPI adapter does not expose. The arrows now work: all three Right presses are spoken ('Viewport at 62% across, ...', '68%', '70%'), so a reader hears the position as soon as they move it.
- **Measured again:** sceneetc-showcase, 2 of 2 runs; adapters read from source
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-showcase 'Tab onto the minimap': +27.3 ms object:state-changed:focused 1 [panel] 'Scene minimap' / ORCA SAYS: 'Scene minimap panel.' / FAIL Orca says any of ('viewport',)`
  - `pass1 sceneetc-showcase 'Right arrow on the minimap, press 2': +40.3 ms object:announcement [status bar] 'Viewport at 68% across, 41% down; showing 64% of the width and 77% of the height' / ORCA SAYS: 'Viewport at 68% across, 41% down; showing 64% of the width and 77% of the height'`
  - `pass1 sceneetc-showcase 'Right arrow on the minimap, press 3': ORCA SAYS: 'Viewport at 70% across, 41% down; showing 60% of the width and 77% of the height'`
  - `pass2 sceneetc-showcase: 'Scene minimap panel.' on focus, and all three presses spoken`
  - `pass1 sceneetc-showcase tree-launch.txt: [panel] 'Scene minimap' {focusable}`
  - `accesskit_atspi_common-0.21.0/src/node.rs:480-482, 510-511, 589-591: Value only for a numeric value; accesskit_windows-0.35.1/src/node.rs:592-597 and accesskit_macos-0.27.1/src/node.rs:344-362 still expose a string value`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/minimap.rs:150-157, 714; accesskit\_atspi\_common-0.21.0/src/node.rs:480-482, 510-511
- **Evidence (`261a218f`):**
  - `15:14:22.677501 object:state-changed:focused 1 [panel] 'Scene minimap'  / ORCA 15:14:22.723497 SPEECH OUTPUT: 'Scene minimap panel.'`
  - `tree: [panel] 'Scene minimap' interfaces ['Accessible', 'Action', 'Component']: no Value or Text interface`
  - `press 1: object:announcement [status bar] 'Viewport at 62% across, 41% down; ...' / SPEECH OUTPUT: 'Viewport at 62% across, ...'`
  - `press 2: 'Viewport at 68% across, ...' came from a node the bus had already been told was defunct (path /org/a11y/atspi/accessible/0/18446744073709551616) / EVENT MANAGER: Ignoring defunct object: [status bar: ...]  (K2)`
  - `accesskit_atspi_common-0.20.0/src/node.rs:486-492: Text only for text ranges, Value only for numeric values`
  - `crates/teksilo-scene/src/minimap.rs:150-157 (the value is the reading), 714 (ctx.announce)`
  - `sceneetc-showcase-20260925-152908-1492203 press 2: object:announcement [status bar] 'Viewport at 62% ...' text='Viewport at 68% across, ...' from path /org/a11y/atspi/accessible/0/18446744073709551616 / 15:30:16.180375 EVENT MANAGER: Ignoring defunct object (K2)`
- **Reproduced:** focus reading missing 3 of 3 runs; K2 drop on presses 2-3 in 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Also publish the reading as the node's description, which AT-SPI carries and Orca reads on focus. The same applies to the transform frame's 'W by H' value (a11y\_impl.rs:437-441).

### sceneetc-11 {#sceneetc-11}

A Toggle's new state reaches the bus only at the next unrelated update: Space is silent, and 'pressed' arrives cut on leaving

- **Example:** theme-styles
- **Scenario:** sceneetc-theme-styles
- **Act:** theme-styles: Space on the 'Notifications' toggle, wait 4 s, then Tab to 'Dark mode'
- **The reader should get:** Space turns it on and Orca says 'pressed'.
- **The reader got (`261a218f`):** No event for more than 4 s after Space. The state-changed:pressed is published only with the next focus change, where Orca's 'pressed' is cut by the new focus.
- **Platform:** Linux AT-SPI/Orca measured. The missing tree update is Teksilo's, so every platform is affected.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Now (`c198e4d1`):** Space on the 'Notifications' toggle is published at once: the bus carries state-changed:pressed within about 25 to 105 ms and Orca says 'pressed'. Turning it off says 'not pressed'. The following Tab carries only the new focus.
- **Measured again:** sceneetc-theme-styles 2 of 2 runs, verify-sceneetc-toggle-tree 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-theme-styles 'Space on the Notifications toggle': +23.9 ms object:state-changed:pressed 1 [toggle button] 'Notifications' / ORCA SAYS: 'pressed'`
  - `pass1 sceneetc-theme-styles 'Tab to the next toggle': pass no object:state-changed:pressed event from [*] 'Notifications' / ORCA SAYS: 'Dark mode toggle button pressed.'`
  - `pass2 sceneetc-theme-styles 'Space on the Notifications toggle': +103.9 ms object:state-changed:pressed 1 [toggle button] 'Notifications' / ORCA SAYS: 'pressed'`
  - `pass1 verify-sceneetc-toggle-tree 'Space, then read the adapter's tree': pass the adapter's tree shows Notifications pressed; 'Space again (turn it off), with no other update': +33.5 ms object:state-changed:pressed 0 [toggle button] 'Notifications' / ORCA SAYS: 'not pressed'`
  - `pass2 verify-sceneetc-toggle-tree: the same`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/toggle.rs:237-248 (the AccessibilityOnly binding), 445-460
- **Evidence (`261a218f`):**
  - `Space on the Notifications toggle (15:04:33.717 -> 15:04:37.905): no event; FAIL a object:state-changed:pressed event`
  - `15:04:39.114523 object:state-changed:pressed 1 [toggle button] 'Notifications'  (the Tab 5.4 s later)`
  - `ORCA 15:04:39.123650 SPEECH OUTPUT: 'pressed'  / 15:04:39.159688 NULL SPEECH: stop  / 15:04:39.159823 SPEECH OUTPUT: 'Dark mode toggle button pressed.'`
  - ``crates/teksilo-widgets/src/toggle.rs:459: accessibility() reads self.on.get(), but build() (245-320) binds `on` only for the style; nothing requests an AT re-walk when it changes (catalog-a sees the same on 'Enable feature')``
  - `sceneetc-theme-styles-20260925-154229-1585894: Space act 15:42:40.94-15:42:45.13 no event; Tab act 15:42:46.337861 object:state-changed:pressed 1 [toggle button] 'Notifications' / ORCA 15:42:46.348942 'pressed' / 15:42:46.393759 NULL SPEECH: stop / 15:42:46.393883 'Dark mode toggle button pressed.'`
  - `verify-sceneetc-toggle-tree-20260925-154100-1580081: 'the adapter's tree shows Notifications pressed' FAIL after Space and after 6 s; after Space x2 the Tab act carries only 15:41:30.248246 focused 1 [toggle button] 'Dark mode', no pressed event`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs (sceneetc-theme-styles) + 3 of 3 (verify-sceneetc-toggle-tree)
- **Fix idea:** Bind `on` to the Toggle's own node at BindingLevel::AccessibilityOnly (or RepaintOnly plus an AT update) in build().

### sceneetc-12 {#sceneetc-12}

SegmentedControl's current segment is read as 'not selected radio button'

- **Example:** touch-playground
- **Scenario:** sceneetc-touch-density
- **Act:** touch-playground: Tab onto the density control (Compact is current), Right to Comfortable; previewer launch (Background 'Themed')
- **The reader should get:** 'Compact, selected radio button, 1 of 3' (or 'checked').
- **The reader got (`261a218f`):** 'Target density. / Compact. / not selected radio button'. After switching, 'Comfortable. / not selected radio button'. The previewer reads 'Themed. / not selected radio button'. The segment sets AccessKit `selected`, which AT-SPI maps to SELECTED, while Orca reads a radio button's CHECKED state.
- **Platform:** Linux AT-SPI/Orca measured. Windows and macOS not checked.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `70183c50` (segmented).
- **Now (`c198e4d1`):** The current segment is now read as checked: 'Target density.', 'Compact.', 'selected radio button', and after Right, 'Comfortable.', 'selected radio button'. In the previewer the 'Themed' background segment is {checkable,checked}.
- **Measured again:** sceneetc-touch-density 2 of 2 runs; previewer tree 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-touch-density 'Tab onto the density control': +163.3 ms object:state-changed:focused 1 [radio button] 'Compact' / ORCA SAYS: 'Compact.' / ORCA SAYS: 'selected radio button' / pass Orca does not say 'not selected'`
  - `pass1 sceneetc-touch-density 'Right arrow: switch to Comfortable': ORCA SAYS: 'Comfortable.' / ORCA SAYS: 'selected radio button'`
  - `pass2 sceneetc-touch-density: the same speech in both acts`
  - `pass1 sceneetc-previewer-nav tree-Enter-on-the-navigator-row.txt: [radio button] 'Themed' {checkable,checked} attrs={'posinset': '1', 'setsize': '4'}`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/segmented\_control/cell.rs:284-294
- **Evidence (`261a218f`):**
  - `15:06:03.487573 object:state-changed:focused 1 [radio button] 'Compact'`
  - `ORCA 15:06:03.546392 SPEECH OUTPUT: 'Compact.'  / 15:06:03.546416 SPEECH OUTPUT: 'not selected radio button'`
  - `tree: [radio button] 'Compact' {selectable,selected} attrs={'posinset': '1', 'setsize': '3'}`
  - `crates/teksilo-widgets/src/segmented_control/cell.rs:287: builder.set_selected(...) and no set_toggled`
  - `accesskit_atspi_common-0.20.0/src/node.rs:345-350 (selected -> State::Selected), 366-372 (toggled -> State::Checked)`
  - `accesskit_windows-0.35.0/src/node.rs:654-656, 669-674 (RadioButton SelectionItem from toggled)`
  - `accesskit_macos-0.27.0/src/node.rs:344-352 (value from toggled)`
  - `orca/generator.py:661-676 (radio state = is_checked)`
- **Reproduced:** 3 of 3 runs (touch-playground); also every previewer launch
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs (touch-playground) + every previewer launch (Background 'Themed') Real in 3 of 3 runs: 'Compact. / not selected radio button', and after switching, 'Comfortable. / not selected radio button'. Orca's radio phrase reads CHECKED (generator.py:661-676, is\_checked), and cell.rs:283-287 sets only `selected`, which AT-SPI maps to SELECTED (atspi node.rs:345-351). Two corrections. First, the platform claim: by source every platform is affected. accesskit\_windows exposes SelectionItem for a RadioButton only when toggled is Some and reads IsSelected from toggled (node.rs:654-656, 669-674). accesskit\_macos takes AXValue from toggled (node.rs:344-352). No adapter reads `selected` for a radio button, so NVDA, JAWS and VoiceOver get no state either. Second, the fix idea: 'keeping selected for UIA SelectionItem' is wrong, because UIA takes it from toggled. set\_toggled is the one field all three read.
- **Fix idea:** Also set toggled(True/False) on the RadioButton segments, as RadioButton does, keeping selected for UIA SelectionItem.

### sceneetc-13 {#sceneetc-13}

async-demo: neither the start nor the completion of an async task is spoken

- **Example:** async-demo
- **Scenario:** sceneetc-async
- **Act:** async-demo: Space on 'Load data (spawn\_blocking)' and wait; Space on 'Fetch + open result window'
- **The reader should get:** The reader hears 'Loading…' and then 'Done — worker returned 42.', and the result window's title and content.
- **The reader got (`261a218f`):** The status label's text and name change, but it is neither live nor focused, so Orca says nothing. The result window activates, and Orca says 'frame.' (K1: no title) and nothing of its content, which has no focusable node.
- **Platform:** Linux AT-SPI/Orca measured. A non-live label is silent on every platform.
- **Severity:** high; **layer:** example
- **Status:** Partly fixed by `89a567cd`, found by the re-measure: that commit did not report it. What remains is under **Now**.
- **Now (`c198e4d1`):** The result window is now announced by its title ('Async result frame.'). Still silent: the status line's 'Loading… (1.2 s on a worker thread)' and 'Done — worker returned 42.', and the result window's content 'Delivered with a fresh EventContext — value 42.', which is never read.
- **Measured again:** sceneetc-async, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-async 'Space on Load data, wait for the worker': +27.9 ms object:property-change:accessible-name [label] 'Loading… (1.2 s on a worker thread)' / +1240.2 ms object:property-change:accessible-name [label] 'Done — worker returned 42.' / FAIL Orca says any of ('Loading',): Orca said nothing / FAIL Orca says any of ('Done',): Orca said nothing`
  - `pass1 sceneetc-async 'Space on Fetch + open result window': +912.0 ms window:activate [frame] 'Async result' / ORCA SAYS: 'Async result frame.' / FAIL Orca says any of ('Delivered',)`
  - `pass2 sceneetc-async: the same; ORCA SAYS: 'Async result frame.'`
- **Where (`c198e4d1`):** examples/async\_demo/src/main.rs:95-120
- **Evidence (`261a218f`):**
  - `15:07:09.872521 object:property-change:accessible-name 0 [label] 'Loading… (1.2 s on a worker thread)'`
  - `15:07:11.084889 object:property-change:accessible-name 0 [label] 'Done — worker returned 42.'  (no SPEECH OUTPUT in the act)`
  - `second act: window:activate [frame] '' / SPEECH OUTPUT: 'frame.'  (content 'Delivered with a fresh EventContext — value 42.' never read)`
  - `examples/async_demo/src/main.rs:120: TextWidget::new(lit!("")).text(self.status.clone()), with no live region and no ctx.announce`
  - `sceneetc-async-20260925-153112-1492251: +1243.8 ms object:property-change:accessible-name [label] 'Done — worker returned 42.' (no SPEECH OUTPUT); second act: window:activate [frame] '' / ORCA 'frame.'`
- **Reproduced:** 2 of 2 runs (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Make the status line a polite live region (access\_live) or ctx.announce the completion. Give the result window a focusable or live element, or announce its content.

### sceneetc-14 {#sceneetc-14}

Previewer knob editors and variant radios have no name

- **Example:** teksilo-widgets-previewer
- **Scenario:** sceneetc-previewer-knobs
- **Act:** teksilo-widgets-previewer --widget=button: launch (focus on the Label knob), Tab to Variant, Tab to Enabled, Shift+Tab back, and the Variant radios
- **The reader should get:** 'Label, entry, Click me', 'Variant, combo box, Plain', 'default, radio button, selected'.
- **The reader got (`261a218f`):** 'entry Click me selected.', 'combo box.' and 'selected radio button' / 'not selected radio button'. Only the Bool knob (Toggle 'Enabled') is named. By source, the slider and segmented choice knobs are unnamed too. The visible row labels are separate TextWidgets and are not attached.
- **Platform:** Linux AT-SPI/Orca measured. The missing names are Teksilo's, so every platform is affected.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The previewer's knob editors and variant radios still have no name. The Label knob reads 'entry Click me selected.', Variant reads 'combo box.', and the variant radios read 'selected radio button' or 'not selected radio button'. Only the Bool knob ('Enabled') is named.
- **Measured again:** sceneetc-previewer-knobs, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-previewer-knobs launch: ORCA SAYS: 'entry Click me selected.'; tree audit: unnamed-control: [radio button] '' x5, [entry] '' (text 'Click me'), [combo box] '', [entry] ''`
  - `pass1 sceneetc-previewer-knobs 'Tab to the Variant knob': +89.1 ms object:state-changed:focused 1 [combo box] '' / ORCA SAYS: 'combo box.'`
  - `pass1 sceneetc-previewer-knobs 'Tab and Shift+Tab back to the variant radio': ORCA SAYS: 'Inspector.' / ORCA SAYS: 'selected radio button'`
  - `pass2 sceneetc-previewer-knobs: the same audit and the same speech`
- **Where (`c198e4d1`):** crates/teksilo-preview-ui/src/knob\_form.rs:97-404; crates/teksilo-preview-ui/src/inspector.rs:205-225
- **Evidence (`261a218f`):**
  - `launch: ORCA 15:07:33.935169 SPEECH OUTPUT: 'entry Click me selected.'`
  - `15:07:44.660115 object:state-changed:focused 1 [combo box] ''  / ORCA 15:07:44.724995 SPEECH OUTPUT: 'combo box.'`
  - `ORCA 15:07:53.053664 SPEECH OUTPUT: 'selected radio button'`
  - `audit: unnamed-control: [radio button] '' ×5, [entry] '' (text 'Click me'), [combo box] '', [entry] ''`
  - `crates/teksilo-preview-ui/src/knob_form.rs:106 (TextInput::new(sig)), 145/168 (Slider::new), 323 (SegmentedControl::indexed), 349/404 (ComboBox::new): no label; inspector.rs:216-221 RadioButton::new with a sibling TextWidget`
  - `sceneetc-previewer-knobs-20260925-153220-1492202 launch audit: unnamed-control [radio button] '' x5, [entry] '' (text 'Click me'), [combo box] '', [entry] ''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Pass decl.label to every editor (.label / access\_label, or labelled\_by the row label), and give RadioButton its variant name via .label().

### sceneetc-15 {#sceneetc-15}

Unnamed controls in the examples: combo boxes, card editors, a list box, progress bars

- **Example:** scene-showcase
- **Scenario:** sceneetc-showcase / sceneetc-overconstraint / sceneetc-corkboard-cards / tabwalks
- **Act:** Tab onto each control
- **The reader should get:** Every control has a name.
- **The reader got (`261a218f`):** scene-showcase's card ComboBox reads 'combo box.' (a 'Pick a fruit' TextWidget sits beside it). over-constraint's toolbar ComboBox reads 'combo box.'. scene-corkboard's 18 card editors (9 per pane) are \[entry\] ''. touch-playground's ListView reads 'list box.'. The animations example has 3 unnamed \[progress bar\] '' and an unnamed page tab list.
- **Platform:** Linux measured. Every platform is affected.
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** All the unnamed controls remain: scene-showcase's card combo box and over-constraint's toolbar combo box read 'combo box.', scene-corkboard's 18 card editors are \[entry\] '', touch-playground's ListView reads 'list box.', and the animations example still has 3 unnamed \[progress bar\] '' and an unnamed \[page tab list\] ''.
- **Measured again:** 2 of 2 runs of each job
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-showcase 'Tab to the card combo box': +31.4 ms object:state-changed:focused 1 [combo box] '' / ORCA SAYS: 'combo box.'`
  - `pass1 sceneetc-overconstraint 'Tab into the toolbar': +128.6 ms object:state-changed:focused 1 [combo box] '' / ORCA SAYS: 'combo box.'`
  - `pass1 sceneetc-corkboard-cards launch audit: unnamed-control: [entry] '': a focusable entry with no name (its text is 'An ordinary morning. The protagonist discovers a strange letter.'), 18 in all`
  - `pass1 tabwalk-touch-playground: +136.1 ms object:state-changed:focused 1 [list box] '' / ORCA SAYS: 'list box.'`
  - `pass1 tabwalk-animations tree-launch.txt: [page tab list] '' {horizontal} and 3x [progress bar] '' {indeterminate}`
  - `pass2: the same in sceneetc-showcase, sceneetc-overconstraint, sceneetc-corkboard-cards, tabwalk-touch-playground and tabwalk-animations`
- **Where (`c198e4d1`):** examples/scene\_showcase/src/main.rs:503; examples/over\_constraint/src/main.rs:64-69; examples/scene\_corkboard/src/main.rs:197; examples/touch\_playground/src/scenarios.rs:126; examples/animations/src/main.rs:185-190 (labelled\_bar), 133
- **Evidence (`261a218f`):**
  - `scene-showcase: object:state-changed:focused 1 [combo box] ''  / SPEECH OUTPUT: 'combo box.'`
  - `over-constraint 14:58:48.515283 object:state-changed:focused 1 [combo box] ''  / SPEECH OUTPUT: 'combo box.'`
  - `corkboard audit: unnamed-control: [entry] '': a focusable entry with no name (its text is 'An ordinary morning. ...') ×18`
  - `touch-playground Tab 11: object:state-changed:focused 1 [list box] ''  / ORCA SAYS: 'list box.'`
  - `sources: examples/scene_showcase/src/main.rs:503, examples/over_constraint/src/main.rs:64-69, examples/scene_corkboard/src/main.rs:197, examples/touch_playground/src/scenarios.rs:126, examples/animations/src/main.rs:195-200`
  - `tabwalk-touch-playground-20260925-154451-1604199 Tab 11: object:state-changed:focused 1 [list box] '' / ORCA 'list box.'`
  - `tabwalk-animations-20260925-154708-1615685 tree-launch: [page tab list] '' and 3x [progress bar] '' {indeterminate}`
- **Reproduced:** deterministic; seen in every run of each example
- **Verification:** confirmed. Reproduced: deterministic, 3 of 3 runs of each example (touch-playground and animations: 1 tabwalk + 3 launch trees)
- **Fix idea:** Label each (ComboBox label / access\_label, RichTextEditor access\_label from the card title, ListView/ProgressBar .label).

### sceneetc-16 {#sceneetc-16}

A live progress bar is announced again every time it scrolls into view

- **Example:** animations-kit
- **Scenario:** sceneetc-animkit
- **Act:** animations-kit: Tab from 'Toggle Fade' to 'Hover or hold me' (the view scrolls the always-present 'Loading' bar in)
- **The reader should get:** Nothing about a bar that has been busy since launch.
- **The reader got (`261a218f`):** 'Loading' is announced as the bar re-enters the filtered tree (clip filter), in the same update as the focus change, so Orca starts it and cuts it.
- **Platform:** Linux measured. By source, Windows raises LiveRegionChanged when a live node goes from filtered-out to included (accesskit\_windows adapter.rs:313-323).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Tabbing from 'Toggle Fade' still scrolls the always-present 'Loading' bar into view, and the bus announces 'Loading' 2 to 11 ms before the focus change, so Orca starts 'Loading' and cuts it to read 'Hover or hold me push button.'.
- **Measured again:** sceneetc-animkit, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-animkit 'Tab from Toggle Fade to the next control (scrolls the Loading bar in)': +35.0 ms object:announcement [progress bar] 'Loading' text='Loading' / +46.2 ms object:state-changed:focused 1 [push button] 'Hover or hold me' / ORCA SAYS (CUT): 'Loading'`
  - `pass1 same act: observed Orca's 'Loading' was cut by a stop 76 ms in (estimated)`
  - `pass2 same act: +38.8 ms object:announcement [progress bar] 'Loading' text='Loading' / +40.8 ms object:state-changed:focused 1 [push button] 'Hover or hold me' / ORCA SAYS (CUT): 'Loading' / observed Orca's 'Loading' was cut by a stop 70 ms in (estimated)`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/progress\_bar.rs:296-309; crates/teksilo-widgets/src/spinner.rs:191-197; accesskit\_atspi\_common-0.21.0/src/adapter.rs:72-78; accesskit\_windows-0.35.1/src/adapter.rs:313-323
- **Evidence (`261a218f`):**
  - `+30.6 ms object:announcement [progress bar] 'Loading' text='Loading'`
  - `+31.2 ms object:state-changed:focused 1 [push button] 'Hover or hold me'`
  - `ORCA 15:06:50.375980 SPEECH OUTPUT: 'Loading' (cut 64 ms in by NULL SPEECH: stop)`
  - `crates/teksilo-widgets/src/progress_bar.rs:296-309 (Live::Polite always); spinner.rs:192-195`
  - `accesskit_atspi_common adapter.rs:71-77 (announce a live node on add); accesskit_consumer filters.rs:64-86 (clip filter adds/removes it as it scrolls)`
  - `sceneetc-animkit-20260925-154345-1585895: 'Loading' reached the bus 0.3 ms before the act's focus change; Orca's 'Loading' cut`
- **Reproduced:** 4 of 4 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Do not make an indeterminate bar or spinner a live region by default. Announce the start and end of the work once through ctx.announce instead.

### sceneetc-17 {#sceneetc-17}

Corkboard 'Add Act': one press, eight announcements

- **Example:** scene-corkboard
- **Scenario:** sceneetc-corkboard-add-act
- **Act:** scene-corkboard: Space on 'Add Act' (twice)
- **The reader should get:** One announcement, e.g. 'Act 4 added'.
- **The reader got (`261a218f`):** Eight: the group 'Act 4' once per pane, and every named node added under the live group (card panel, its title label, 'connector to beat 10'), once per pane. Orca speaks all eight.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Each press on 'Add Act' still puts eight announcements on the bus and Orca speaks all eight: the group 'Act 4' twice, 'Act 4 — new beat' four times (card panel and its title label, once per pane) and 'connector to beat 10' twice. The second press does the same for Act 5.
- **Measured again:** sceneetc-corkboard-add-act, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-corkboard-add-act 'Space on Add Act': +181.6 ms object:announcement [panel] 'Act 4 — new beat' text='Act 4 — new beat' ... +185.2 ms object:announcement [panel] 'Act 4' text='Act 4' (8 announcements, 6 of them 'Act 4')`
  - `pass1 sceneetc-corkboard-add-act 'Space on Add Act': +200.2 ms ORCA SAYS: 'connector to beat 10' / 'Act 4 — new beat' / 'Act 4 — new beat' / 'Act 4 — new beat' / 'connector to beat 10' / 'Act 4' / 'Act 4 — new beat' / 'Act 4'`
  - `pass2 sceneetc-corkboard-add-act 'Space on Add Act': 8 announcements; Orca said 'Act 4' 6 time(s) plus 'connector to beat 10' twice`
  - `pass1 and pass2 sceneetc-corkboard-add-act 'Space on Add Act again': Orca said 'Act 5' 6 time(s), plus 'connector to beat 11' twice`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/view/a11y\_impl.rs:376-379; examples/scene\_corkboard/src/main.rs:431-433; upstream: accesskit\_consumer-0.39.1/src/node.rs:906-910 (live is inherited), accesskit\_atspi\_common-0.21.0/src/adapter.rs:72-78 (every added named live node is announced)
- **Evidence (`261a218f`):**
  - `15:08:53.899163 object:announcement 1 [panel] 'Act 4 — new beat'`
  - `15:08:53.909035 object:announcement 1 [panel] 'Act 4'`
  - `15:08:53.909952 object:announcement 1 [panel] 'connector to beat 10'`
  - `15:08:53.910619 object:announcement 1 [label] 'Act 4 — new beat'  ... (8 in all)`
  - `ORCA 15:08:53.926500 .. 15:08:53.963963 SPEECH OUTPUT: 'Act 4 — new beat' / 'Act 4' / 'connector to beat 10' / 'Act 4 — new beat' / 'Act 4 — new beat' / 'Act 4' / 'connector to beat 10' / 'Act 4 — new beat'`
  - `accesskit_consumer-0.39.0/src/node.rs:906-910 (live is inherited); accesskit_atspi_common adapter.rs:71-77 (every added named live node announced)`
  - `examples/scene_corkboard/src/main.rs:431-433 (set_a11y_live on the group); crates/teksilo-scene/src/view/a11y_impl.rs:374-377 (each view emits the group and its live)`
  - `sceneetc-corkboard-add-act-20260925-153802-1554576: 8 utterances 'Act 4 — new beat' | 'Act 4 — new beat' | 'connector to beat 10' | 'Act 4 — new beat' | 'Act 4' | 'connector to beat 10' | 'Act 4 — new beat' | 'Act 4'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Announce a structural addition once with ctx.announce, not with a live container whose descendants inherit liveness. Emit a scene group's live flag in one view only.

### sceneetc-18 {#sceneetc-18}

Collapsed and faded-out content is still read as present (animations-kit)

- **Example:** animations-kit
- **Scenario:** sceneetc-animkit
- **Act:** animations-kit: the tree at launch with Collapse collapsed; Space on 'Toggle Fade'
- **The reader should get:** A collapsed section and content faded to nothing are not in the tree a reader walks.
- **The reader got (`261a218f`):** 'Hidden content #1..3' sit in the tree with state 'showing' (and 23×110 boxes over other content) while collapsed. The faded-out label stays after the fade.
- **Platform:** Linux measured. The nodes are Teksilo's, so every platform is affected.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** With Collapse collapsed at launch, 'Hidden content #1..3' are still in the tree with 'showing' and 23 by 110 boxes. After 'Toggle Fade' fades the content out, its label stays in the tree. The Slide banner that starts slid out and faded is also in the tree with 'showing' and on-window extents.
- **Measured again:** sceneetc-animkit, 2 of 2 runs; verify-sceneetc-hidden-content, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-animkit 'Collapse is collapsed at launch': FAIL  the tree holds no [*] 'Hidden content #1' / found [label] 'Hidden content #1' (states enabled, sensitive, showing, visible; extents [24, 316, 23, 110])`
  - `pass2 sceneetc-animkit 'Collapse is collapsed at launch': found [label] 'Hidden content #1'`
  - `pass1 and pass2 sceneetc-animkit 'Space on Toggle Fade (fades the content out)': found [label] '  ●  Faded content — opacity tweens between 0 and 1.'`
  - `pass1 and pass2 verify-sceneetc-hidden-content 'the Slide banner, slid out and faded at launch': [label] '⚠  Banner — slides + fades.' states=['enabled', 'sensitive', 'showing', 'visible'] extents=[552, 756, 174, 15]`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/animations/collapse.rs:188-203; slide.rs:227-229; fade.rs:164-171; examples/animations\_kit/src/main.rs:102, 118 (compare crates/teksilo-widgets/src/accordion.rs:532-535)
- **Evidence (`261a218f`):**
  - `[label] 'Hidden content #1' extents [24, 316, 23, 110] states ['enabled', 'sensitive', 'showing', 'visible'] (collapse_expanded = false)`
  - `FAIL  the tree holds no [*] 'Hidden content #1': found [label] 'Hidden content #1'`
  - `FAIL  the tree holds no [*] 'Faded content': found [label] '  ●  Faded content — opacity tweens between 0 and 1.'`
  - `crates/teksilo-widgets/src/animations/collapse.rs:188-203: clips_children for paint, but accessibility() publishes nothing, so the clip never reaches AccessKit; its doc says the content is announced 'when expanded'`
  - `crates/teksilo-widgets/src/animations/fade.rs:164-171 documents that faded content stays in AT; examples/animations_kit/src/main.rs:102, 118 do not pair visible_when`
  - `verify-sceneetc-hidden-content-20260925-155001-1603608: [label] '⚠  Banner — slides + fades.' states=['enabled','sensitive','showing','visible'] extents=[552, 756, 174, 15] while slide_visible=false`
  - `crates/teksilo-widgets/src/accordion.rs:532-535 (Accordion pairs visible_when with Collapse; standalone Collapse does not)`
- **Reproduced:** Collapse 4 of 4 runs; Fade 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs (Collapse, Fade); 3 of 3 (Slide, verify-sceneetc-hidden-content)
- **Fix idea:** Collapse should mark its child dormant or access-hidden when collapsed (as Accordion does with visible\_when). The example should pair Fade/Slide with visible\_when, as Fade's doc says.

### sceneetc-19 {#sceneetc-19}

Corkboard: the Overview pane repeats every card and editor in the Tab ring

- **Example:** scene-corkboard
- **Scenario:** sceneetc-corkboard-overview
- **Act:** scene-corkboard: focus the main pane's last card 'Coda', then Ctrl+Tab four times
- **The reader should get:** The overview, a thumbnail for sighted users, is not a second copy of the board to Tab through, or at least is marked as a copy.
- **The reader got (`261a218f`):** After 'Coda' comes its editor, then 'landmark Overview pane', then 'Act I — Setup panel. / Act I — Opening panel.' again, then its editor. All 9 cards and 9 editors are focusable a second time under identical names.
- **Platform:** Linux measured
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** After the main pane's last card 'Coda' and its editor, Ctrl+Tab goes to 'landmark Overview pane.' and then through the overview's copy of every card and editor under the same names ('Act I — Setup panel.' 'Act I — Opening panel.', then 'entry An ordinary morning. The '). The editors now read as 'entry' with their text, but the second copy of the board is still in the Tab ring.
- **Measured again:** sceneetc-corkboard-overview, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-corkboard-overview 'Ctrl+Tab 1 from the last card': +83.2 ms object:state-changed:focused 1 [entry] '' / +150.8 ms ORCA SAYS: 'entry Months later. Quiet '`
  - `pass1 sceneetc-corkboard-overview 'Ctrl+Tab 2 from the last card': +107.2 ms object:state-changed:focused 1 [landmark] 'Overview pane' / +879.7 ms ORCA SAYS: 'landmark Overview pane.'`
  - `pass1 sceneetc-corkboard-overview 'Ctrl+Tab 3 from the last card': +92.6 ms object:state-changed:focused 1 [panel] 'Act I — Opening' / +166.8 ms ORCA SAYS: 'Act I — Setup panel.' 'Act I — Opening panel.'`
  - `pass2 sceneetc-corkboard-overview: same four stops; 'Ctrl+Tab 4 from the last card': +154.7 ms ORCA SAYS: 'entry An ordinary morning. The '`
- **Where (`c198e4d1`):** examples/scene\_corkboard/src/main.rs:707-713
- **Evidence (`261a218f`):**
  - `note: Ctrl+Tab stops after the main pane's 'Coda': section ''; landmark 'Overview pane'; panel 'Act I — Opening'; section ''`
  - `ORCA SAYS: 'landmark Overview pane.' then 'Act I — Setup panel.' 'Act I — Opening panel.'`
  - `tree: [landmark] 'Overview pane' child_count 286, with focusable [panel] 'Act I — Opening' (extents [1288, 50, 55, 35])`
  - `examples/scene_corkboard/src/main.rs:709-713`
  - `sceneetc-corkboard-overview (3 runs) note: Ctrl+Tab stops after the main pane's 'Coda': section ''; landmark 'Overview pane'; panel 'Act I — Opening'; section ''`
- **Reproduced:** 1 of 1 run (deterministic Tab order; the tree shows the same in every run)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs The Tab order is as reported (3 of 3): after the main pane's 'Coda' come its editor, 'landmark Overview pane.', then 'Act I — Setup panel. / Act I — Opening panel.' and its editor. But medium overstates it. The overview is a second, fully editable view of the same document (sighted users can select and edit in it too). Orca announces 'landmark Overview pane' on entry, so the reader is told which copy they are in, as in any split editor. What remains is verbosity: 18 more stops, and cards with the same names as the main pane's. Low. The example could keep the overview out of the Tab ring or exclude its subtree if it means it as a thumbnail only.
- **Fix idea:** Build the overview pane's cards non-focusable, or exclude the overview subtree (access\_exclude\_subtree) and keep its landmark as a navigable summary.

### sceneetc-20 {#sceneetc-20}

Hundreds of unnamed decorative scene items in the tree (562 unnamed 'panel's on the corkboard)

- **Example:** scene-showcase
- **Scenario:** tabwalk scene-corkboard / sceneetc-showcase
- **Act:** The tree a reader walks (Orca flat review / object navigation)
- **The reader should get:** Decorative items (grid tiles, accent tags, unlabelled dots) are not in the tree.
- **The reader got (`261a218f`):** The corkboard publishes 281 unnamed \[panel\] '' per pane (280 backdrop tiles and the accent tag), 562 in all. The showcase has unnamed item panels in sections 2, 6 and 8.
- **Platform:** Linux measured. Every platform gets the nodes.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The corkboard still publishes 562 unnamed \[panel\] '' nodes under its two panes (the backdrop tiles and the accent tag, once per pane). The showcase still has unnamed item panels inside its scene.
- **Measured again:** sceneetc-corkboard-cards and tabwalk-scene-corkboard, 4 of 4 launches; tabwalk-scene-showcase, 2 of 2
- **Evidence (`c198e4d1`):**
  - `` pass1 sceneetc-corkboard-cards tree-launch.txt: 562 lines of `      [panel] ''` ``
  - `pass2 sceneetc-corkboard-cards tree-launch.txt: 562`
  - `pass1 and pass2 tabwalk-scene-corkboard tree-launch.txt: 562 each`
  - `pass1 and pass2 tabwalk-scene-showcase tree-launch.txt: runs of unnamed [panel] '' under the scene pane (lines 65-101 and on)`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/view/a11y\_impl.rs:600-630; crates/teksilo-scene/src/items/rect.rs:281-287; examples/scene\_corkboard/src/main.rs:337-345
- **Evidence (`261a218f`):**
  - ``tree-launch.txt: 562 lines of `      [panel] ''` under the two panes, e.g. {'name': '', 'role': 'panel', 'extents': [0, 28, 40, 40]}``
  - `examples/scene_corkboard/src/main.rs:341: RectItem::new(cell).stroke_cosmetic(..) with no access_hidden`
  - `crates/teksilo-scene/src/items/rect.rs:281-287: an unlabelled item is still published as a GraphicsObject`
  - `sceneetc-corkboard-cards-20260925-153100-1492203/tree-launch.txt: grep -c "^      [panel] ''$" = 562`
- **Reproduced:** deterministic (every corkboard run)
- **Verification:** confirmed. Reproduced: deterministic, 3 of 3 runs
- **Fix idea:** Publish an unlabelled lightweight item as hidden by default (opt in with access\_label), or have the audit flag it. The example should access\_hidden the tiles.

### sceneetc-21 {#sceneetc-21}

Previewer theme, density and locale pickers do not say which choice is current

- **Example:** teksilo-widgets-previewer
- **Scenario:** sceneetc-previewer-nav
- **Act:** teksilo-widgets-previewer: Tab onto the theme picker's 'Native' button
- **The reader should get:** 'Native, toggle button, pressed' (or a radio group with the selected one).
- **The reader got (`261a218f`):** 'Native push button.' Nine plain buttons whose only mark of the current choice is a visual variant, computed once at build.
- **Platform:** All (no state is emitted). Linux measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Tab onto the theme picker's first button still gives 'Native push button.' with no pressed or selected state. Theme, Density and Locale are still plain push buttons, and their 'Theme:', 'Density:' and 'Locale:' labels are not attached. The Background picker beside them now marks its current choice ('Themed' is checked), but that group is still unnamed.
- **Measured again:** sceneetc-previewer-nav, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-previewer-nav 'Tab onto the theme picker's Native button': +67.9 ms object:state-changed:focused 1 [push button] 'Native' / +104.3 ms ORCA SAYS: 'Native push button.'`
  - `pass2 sceneetc-previewer-nav 'Tab onto the theme picker's Native button': +2235.1 ms ORCA SAYS: 'Native push button.'`
  - `pass1 sceneetc-previewer-nav tree-launch.txt: [label] 'Theme:' / [push button] 'Native' {focusable} / [push button] 'Light' {focusable} / [push button] 'Dark' {focusable}; [label] 'Background:' / [panel] '' {focusable} > [radio button] 'Themed' {checkable,checked}`
- **Where (`c198e4d1`):** crates/teksilo-preview-ui/src/toolbar.rs:52-55, 67-75 (labels beside, not attached), 78-107 (theme), 109-142 (background), 144-182 (locale), 190-221 (density)
- **Evidence (`261a218f`):**
  - `15:05:41.155317 object:state-changed:focused 1 [push button] 'Native'  / ORCA 15:05:41.203606 SPEECH OUTPUT: 'Native push button.'`
  - `crates/teksilo-preview-ui/src/toolbar.rs:78-99, 150-170, 190-215: ButtonVariant::Filled for the current choice via style_sig.get() at build; no pressed or selected state; the 'Theme:' label is not attached`
  - `sceneetc-previewer-nav tree-launch: [label] 'Background:' then [panel] '' {focusable} > [radio button] 'Themed' {selectable,selected}; launch speech 'Themed.' 'not selected radio button'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Use a labelled SegmentedControl (once sceneetc-12 is fixed) or toggle buttons with a bound pressed state, grouped and named 'Theme' / 'Density' / 'Locale'.

### sceneetc-22 {#sceneetc-22}

SceneCard's Focus action does not select the card, so the keyboard route to the transform handles is out of reach

- **Example:** scene-corkboard
- **Scenario:** sceneetc-corkboard-transform
- **Act:** scene-corkboard: AT-SPI grab\_focus on 'Act I — Opening', then Shift+Tab to the pane and 't'
- **The reader should get:** As documented (Action::Focus selects the card), the card becomes selected and 't' on the pane enters transform mode on its frame.
- **The reader got (`261a218f`):** Focus moves but no selected state appears. The frame ('Selection') and the selected state appear only on Enter, which also enters editing and jumps to the overview (sceneetc-06). 't' never reached a handle in 4 runs.
- **Platform:** Linux measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** After an AT-SPI grab\_focus on 'Act I — Opening' the card is focused but not selected. The card is selected only by Enter, which also enters editing and moves focus into the Overview pane's copy of the card. From there, Escape and Shift+Tab put no focus change on the bus, and 't' is typed into the card's prose instead of reaching a handle. The route still works if focus is put back on the main pane by other means: 't' then lands on 'Move selection'.
- **Measured again:** sceneetc-corkboard-transform, 2 of 2 runs; verify-sceneetc-transform, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-corkboard-transform 'the focused card is selected': found [panel] 'Act I — Opening' but states=['enabled', 'focusable', 'focused', 'selectable', 'sensitive', 'showing', 'visible']`
  - `pass1 sceneetc-corkboard-transform 'Enter on the main pane's card (edit)': +40.6 ms object:state-changed:selected 1 [panel] 'Act I — Opening' / +165.7 ms ORCA SAYS: 'landmark Overview pane'`
  - `pass1 sceneetc-corkboard-transform 't enters transform mode': +59.2 ms object:text-changed:insert [entry] '' text='t' / FAIL  focus lands on a node with a name`
  - `pass2 sceneetc-corkboard-transform: the same, 't' inserted into [entry] '' twice`
  - `pass1 verify-sceneetc-transform 't enters transform mode' (after an AT-SPI grab_focus on the main pane): +42.1 ms object:state-changed:focused 1 [push button] 'Move selection' / +125.0 ms ORCA SAYS: 'Selection panel.' 'Move selection push button.'`
  - `read from source: crates/teksilo-core/src/widget_tree/pointer_router.rs:1178-1227 services Action::Focus itself and never dispatches it to the node's on_access_action handler`
- **Where (`c198e4d1`):** crates/teksilo-core/src/widget\_tree/pointer\_router.rs:1178-1227 (dead handlers: crates/teksilo-scene/src/scene\_card.rs:1014-1027, crates/teksilo-scene/src/view/transform.rs:914-920)
- **Evidence (`261a218f`):**
  - `the focused card is selected: FAIL  the tree holds [panel] 'Act I — Opening' (state selected)`
  - `Enter act: 15:17:39.109658 object:state-changed:selected 1 [panel] 'Act I — Opening' / object:children-changed:add 286 [panel] '' -> [panel] 'Selection'`
  - `note: t landed on [section] ''`
  - `crates/teksilo-scene/src/scene_card.rs:1014-1027: on_access_action Focus -> this.select(ctx), which is not observed after an AT-SPI grab_focus`
  - `verify-sceneetc-transform-20260925-154212-1580081: 15:42:40.507984 object:state-changed:focused 1 [push button] 'Move selection' / ORCA 15:42:40.589638 'Selection panel.' / 15:42:40.589672 'Move selection push button.'; Enter commits: 15:42:57.527177 object:announcement [status bar] 'Moved 1 item'`
  - ``crates/teksilo-core/src/widget_tree/pointer_router.rs:1163-1204: `if *action == accesskit::Action::Focus { ... focus_with_origin_ops ... }` with no dispatch_to_widget in that branch``
- **Reproduced:** 4 of 4 runs
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 runs (sceneetc-corkboard-transform) + 3 of 3 (verify-sceneetc-transform) The symptom is real (3 of 3): after the AT-SPI grab\_focus the card is focused but not selected. The sweep blamed SceneCard; the root cause is in teksilo-core. pointer\_router.rs:1163-1204 services Action::Focus itself (focus\_with\_origin\_ops) and never passes it to the node's on\_access\_action or on\_access\_action\_request handlers. SceneCard's `Action::Focus => this.select(ctx)` (scene\_card.rs:1017-1022) is therefore dead code, and so is the transform handles' Focus arm (view/transform.rs:914). A second correction: the claim that 't' never reached a handle comes from the scenario's route, not a hard block. In my probe (3 of 3), Enter selects the card (state selected, and a 'Selection' frame appears in both panes). I then put focus back on the main pane with an AT-SPI grab\_focus. 't' lands on \[push button\] 'Move selection', Orca says 'Selection panel. / Move selection push button.', and Enter commits with 'Moved 1 item'. The remaining barrier is that no key selects a card without entering editing, and Enter throws focus into the overview (sceneetc-06). Medium, framework (core).
- **Fix idea:** Select the card on keyboard focus (an on\_focus handler), not only on an AT Focus action that the dispatcher may consume first. Verify that on\_access\_action receives Focus.

### sceneetc-23 {#sceneetc-23}

scene-ink: the ink is invisible to assistive technology, and Backspace erases it silently

- **Example:** scene-ink
- **Scenario:** sceneetc-ink
- **Act:** scene-ink: the tree, and Backspace on the page
- **The reader should get:** At least one named node says how many strokes the page holds (the example's own comment promises 'the layer carry one named node instead'), and an erase is confirmed.
- **The reader got (`261a218f`):** Strokes are access\_hidden and the WetLayer is set\_hidden. No node represents the ink, and Backspace produces no event and no speech.
- **Platform:** All (source). Linux measured.
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** The page still has no node for the ink: the scene pane holds only the two notes' labels. Backspace on the page puts no event on the bus; the reader hears only Orca's own echo of the key, 'backspace'.
- **Measured again:** sceneetc-ink, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-ink tree-launch.txt: [panel] '' {focusable} > [label] 'Under the ink (z = 0)', [label] 'A heavyweight card. …', [label] 'Over the ink (z = 20)', [label] 'Also a card, one step higher. …' and nothing else`
  - `pass2 sceneetc-ink tree-launch.txt: the same four labels only`
  - `pass1 sceneetc-ink 'Backspace (remove the last stroke; there is none)': +7.0 ms ORCA SAYS: 'backspace' (no event on the bus)`
  - `pass2 sceneetc-ink 'Backspace (remove the last stroke; there is none)': +6.5 ms ORCA SAYS: 'backspace'`
- **Where (`c198e4d1`):** examples/scene\_ink/src/main.rs:424-429, 445-479; crates/teksilo-scene/src/view/paint\_node.rs:684-689
- **Evidence (`261a218f`):**
  - `tree: [panel] '' with only the two notes' labels; no ink node`
  - `Backspace act: Orca said nothing in this act (3 of 3)`
  - `examples/scene_ink/src/main.rs:424-429: '.access_hidden(true)' with the comment 'hide the stroke and let the layer carry one named node instead'`
  - `crates/teksilo-scene/src/view/paint_node.rs:684-689: WetLayer::accessibility -> set_hidden()`
  - `sceneetc-ink tree-launch (3 runs): [panel] '' {focusable} > 4 [label]s only`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Add a named node (e.g. a Group 'Ink, 3 strokes') whose name updates. Announce 'stroke removed' on Backspace.

### sceneetc-24 {#sceneetc-24}

touch-playground says 'a screen reader is told once' about a density switch; nothing is told

- **Example:** touch-playground
- **Scenario:** sceneetc-touch-density
- **Act:** touch-playground: Right arrow on the density control (Compact -&gt; Comfortable, a full rebuild)
- **The reader should get:** One announcement of the new density (as the page promises).
- **The reader got (`261a218f`):** No announcement. The reader hears only the refocused segment, 'Comfortable. / not selected radio button'. The example rebuilds the tree itself, and WidgetTree::set\_input\_density (the only path with an announcement) has no EventContext door.
- **Platform:** All. Linux measured.
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Right arrow on the density control still switches to Comfortable with no announcement on the bus. The reader hears only the refocused segment, which now says it is the current one: 'Target density panel.' 'Target density.' 'Comfortable.' 'selected radio button'.
- **Measured again:** sceneetc-touch-density, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-touch-density 'Right arrow: switch to Comfortable': +229.6 ms object:state-changed:focused 1 [radio button] 'Comfortable' / +367.8 ms ORCA SAYS: 'Target density panel.' 'Target density.' 'Comfortable.' 'selected radio button'; no object:announcement in the act`
  - `pass2 sceneetc-touch-density 'Right arrow: switch to Comfortable': +2395.6 ms ORCA SAYS: 'Target density panel.' ... 'selected radio button'; no object:announcement in the report`
- **Where (`c198e4d1`):** examples/touch\_playground/src/panels.rs:61-80, 125-131; crates/teksilo-core/src/widget\_tree.rs:2239-2257 (set\_input\_density, the only path that announces)
- **Evidence (`261a218f`):**
  - `examples/touch_playground/src/panels.rs:125-131: 'Switching rebuilds the whole tree ... and a screen reader is told once.'`
  - `examples/touch_playground/src/panels.rs:61-80: set_theme + bump_rebuild, no announce`
  - `crates/teksilo-core/src/widget_tree.rs:2189-2207: set_input_density announces through self.announce (K2 path)`
  - `no object:announcement in the Right-arrow act, 3 of 3 runs`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Expose the density switch (with its announcement) on EventContext, or ctx.announce from set\_density in the example.

### sceneetc-25 {#sceneetc-25}

A rotating Cycle puts remove/add/defunct on the bus every period while idle

- **Example:** animations-kit
- **Scenario:** sceneetc-animkit
- **Act:** animations-kit: idle 7 s beside the Cycle of tips
- **The reader should get:** Little or no bus traffic while idle, or a live region if the change matters.
- **The reader got (`261a218f`):** Every 2 s: children-changed add, children-changed remove and a defunct (9 events in 7 s). Not live, so a reader is never told the tip changed, and a review finds whichever tip is current. Looping progress bars, spinners and Pulse put nothing on the bus (passed).
- **Platform:** Linux measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** While idle beside the Cycle of tips, every 2 s the bus carries a children-changed add, a children-changed remove and a defunct: 12 events in the 7 s window. Nothing is live, so Orca says nothing when the tip changes.
- **Measured again:** sceneetc-animkit, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-animkit 'idle beside the Cycle': +270.0 ms object:children-changed:add [panel] '' -> [label] 'Tip 2: press Cmd-K to search' / +271.5 ms object:children-changed:remove [panel] '' -> [label] 'Tip 1: hold Shift to multi-select' / +271.7 ms object:state-changed:defunct 1 [label] 'Tip 1: hold Shift to multi-select' ... 12 event(s) in 7.0 s`
  - `pass2 sceneetc-animkit 'idle beside the Cycle': 12 event(s) in 7.0 s, the same add/remove/defunct triple every 2 s; no Orca speech`
  - `pass1 and pass2 sceneetc-animkit 'idle at the top (spinners, sweep)': pass  at most 0 event(s) on the bus while idle`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/animations/cycle.rs:195-198
- **Evidence (`261a218f`):**
  - `+1245.2 ms object:children-changed:add [panel] '' -> [label] 'Tip 1: hold Shift to multi-select'`
  - `+1245.7 ms object:children-changed:remove [panel] '' -> [label] 'Tip 3: drag the divider to resize'`
  - `+1246.0 ms object:state-changed:defunct 1 [label] 'Tip 3: drag the divider to resize'`
  - `9 event(s) in 7.0 s`
  - `crates/teksilo-widgets/src/animations/cycle.rs (a Switcher driven by a frame tick)`
  - `sceneetc-animkit-20260925-153015-1492251: 15:31:06.429057 children-changed:add 'Tip 1', 15:31:06.429249 remove 'Tip 3', 15:31:06.429333 defunct 'Tip 3' ... 15:31:10.444424 add 'Tip 3' (same node back)`
- **Reproduced:** 4 of 4 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Let Cycle keep one text node and change its name (optionally live polite, opt-in), rather than swapping nodes.

### sceneetc-26 {#sceneetc-26}

Magnet ports are 'push button's that cannot be pressed

- **Example:** scene-magnetism
- **Scenario:** sceneetc-magnet-connect
- **Act:** scene-magnetism: AT-SPI click on 'Blur output'
- **The reader should get:** A node exposed as a button can be activated (to start a connection from it), or it is not called a button.
- **The reader got (`261a218f`):** The click is refused (no Action interface). A port is also not focusable outside connect mode.
- **Platform:** Linux measured. AccessKit Role::Button without Click on every platform.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** An AT-SPI click on the port 'Blur output' is still refused: it is exposed as a push button with no Action interface. In connect mode the ports are reached by arrows and read as 'Blur input push button.', but a reader's own activation does nothing.
- **Measured again:** sceneetc-magnet-connect, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-magnet-connect 'AT-SPI click on a port (a reader's own activation)': refused: {'path': '/org/a11y/atspi/accessible/0/292311708296346216183221947179458363392', 'name': 'Blur output', 'role': 'push button'} offers no action on AT-SPI`
  - `pass2 sceneetc-magnet-connect 'AT-SPI click on a port (a reader's own activation)': the same refusal`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/view/a11y\_impl.rs:708-755
- **Evidence (`261a218f`):**
  - `refused: {'path': '/org/a11y/atspi/accessible/0/292311708296346216183221947179458363392', 'name': 'Blur output', 'role': 'push button'} offers no action on AT-SPI`
  - `crates/teksilo-scene/src/view/a11y_impl.rs:737-752: SceneMagnet node: Role::Button + name + bounds, no action`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Add Action::Click (pick as source / connect, same as Enter in connect mode) and handle it, or use a non-interactive role.

### sceneetc-27 {#sceneetc-27}

SceneCard's 'Edit' action and the transform handles' step actions are invisible on Linux

- **Example:** scene-showcase, scene-corkboard, scene-magnetism, scene-ink, over-constraint, theme-styles, animations, animations-kit, async-demo, touch-playground, teksilo-widgets-previewer
- **Scenario:** (source, tree)
- **Act:** Reading the card nodes' actions on AT-SPI
- **The reader should get:** A reader's action menu offers 'Edit' on a card and Increment/Decrement or steps on a handle.
- **The reader got (`261a218f`):** The AT-SPI adapter publishes only 'click'. The card shows interfaces \['Accessible', 'Component'\] with no Action at all. Enter still works from the keyboard.
- **Platform:** Linux (source + tree). Windows and macOS not checked.
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** A card still shows only the Accessible and Component interfaces on AT-SPI, so a reader's action menu offers neither 'Edit' on a card nor the transform handles' steps. The adapter now used still exposes only 'click', and only on a clickable node. The Windows and macOS adapters now used still have no custom-action support either. Enter remains the keyboard route.
- **Measured again:** read from source; tree from sceneetc-corkboard-cards, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-corkboard-cards run.json: [panel] 'Act I — Opening' interfaces ['Accessible', 'Component']`
  - `read from source: accesskit_atspi_common-0.21.0/src/node.rs:445-447 (supports_action is is_clickable), 524-537 (n_actions 1 and 'click' only when clickable)`
  - `read from source: grep for CustomAction/custom_action in accesskit_windows-0.35.1/src and accesskit_macos-0.27.1/src: no match`
- **Where (`c198e4d1`):** accesskit\_atspi\_common-0.21.0/src/node.rs:445-447, 524-537; crates/teksilo-scene/src/scene\_card.rs:1082-1101; crates/teksilo-scene/src/view/a11y\_impl.rs:527-555
- **Evidence (`261a218f`):**
  - `tree: [panel] 'Act I — Opening' interfaces ['Accessible', 'Component']`
  - `accesskit_atspi_common-0.20.0/src/node.rs:443-445, 532-545: n_actions is 1 ('click') only when clickable`
  - `crates/teksilo-scene/src/scene_card.rs:1082-1101 (custom action 'Edit'); view/a11y_impl.rs:527-555 (handle actions)`
  - `grep -n 'custom_action|CustomAction' accesskit_windows-0.35.0/src accesskit_macos-0.27.0/src: no match`
- **Reproduced:** deterministic (tree)
- **Verification:** corrected by the verifier. Reproduced: deterministic (tree, 3 of 3 corkboard runs) On Linux it is as described. The card node has interfaces \['Accessible','Component'\] and no Action. atspi\_common exposes only 'click', and only when the node is clickable (node.rs:442-444, 532-545). Platform correction: accesskit\_windows-0.35.0/src and accesskit\_macos-0.27.0/src contain no CustomAction handling either, so 'Edit' and the transform handles' step actions reach no platform. Teksilo's own set\_custom\_actions docs say as much ('the list alone is decoration', scene\_card.rs:1093-1098). Enter remains the keyboard route. Low, upstream.
- **Fix idea:** Keep keyboard equivalents documented (Enter / t). Report custom-action export to AccessKit.

### sceneetc-28 {#sceneetc-28}

touch-playground list rows carry an \[unknown\]-role child that repeats the row's name

- **Example:** scene-showcase, scene-corkboard, scene-magnetism, scene-ink, over-constraint, theme-styles, animations, animations-kit, async-demo, touch-playground, teksilo-widgets-previewer
- **Scenario:** tabwalk touch-playground
- **Act:** The tree at launch
- **The reader should get:** A list item with its name, no unmappable child.
- **The reader got (`261a218f`):** \[list item\] 'Row 1' &gt; \[unknown\] 'Row 1' for every row. The audit reports unknown-role ×20. The catalog-c sweep sees the same in its data views.
- **Platform:** Linux measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Every realised row in touch-playground is still \[list item\] 'Row N' with a child \[unknown\] 'Row N', and the launch audit reports unknown-role for all 20.
- **Measured again:** tabwalk-touch-playground and sceneetc-touch-density, 4 of 4 launches
- **Evidence (`c198e4d1`):**
  - `pass1 tabwalk-touch-playground tree-launch.txt:36-37 [list item] 'Row 1' {selectable} attrs={'posinset': '1', 'setsize': '50'} > [unknown] 'Row 1' attrs={'setsize': '50'}`
  - `pass2 tabwalk-touch-playground tree-launch.txt:36-37: the same`
  - `pass1 and pass2 sceneetc-touch-density launch audit: unknown-role: [unknown] 'Row 1' ... 'Row 20': a node whose role the adapter could not map`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/standard\_item.rs:916-935
- **Evidence (`261a218f`):**
  - `{'name': 'Row 1', 'role': 'unknown', 'attributes': {'setsize': '50'}, 'interfaces': ['Accessible', 'Component']}`
  - `audit: unknown-role: [unknown] 'Row 1': a node whose role the adapter could not map`
  - `tabwalk-touch-playground-20260925-154451-1604199/tree-launch.txt:37 [unknown] 'Row 1' attrs={'setsize': '50'} under [list item] 'Row 1'`
- **Reproduced:** 4 of 4 launches
- **Verification:** confirmed. Reproduced: deterministic, 4 of 4 launches
- **Fix idea:** Find which StandardListItem/ListView layer emits Role::Unknown and give it GenericContainer or Label.

### sceneetc-M1 {#sceneetc-m1}

Blur's 'click-to-reveal sensitive content' is read out in full by a screen reader while it is obscured

- **Example:** animations-kit
- **Scenario:** verify-sceneetc-hidden-content
- **Act:** animations-kit: Tab to 'Reveal / Hide' (Blur section, obscured at launch) and read the tree
- **The reader should get:** Numbers the page hides until 'Reveal' are hidden from AT as well, or at least exposed as obscured. The 'Reveal / Hide' button should say which state it is in.
- **The reader got (`261a218f`):** 'Card balance: $42,851.07', 'Account #: 1234-5678-9012-3456' and 'CVV: 042 · Exp: 12/29' are ordinary \[label\] nodes with 'showing'/'visible' states and on-screen extents, so flat review or object navigation reads them aloud. 'Reveal / Hide' is a plain push button with no pressed or expanded state. Blur's module doc recommends it for 'Click-to-reveal sensitive content' (blur.rs:24-27) and says nothing about AT. The in-code note (blur.rs:158-164) says to pair it with visible\_when, which would remove the blurred placeholder visually too. The tool that fits is a reactive access\_hidden(Signal).
- **Platform:** Linux AT-SPI measured. The nodes are Teksilo's, so every platform gets them.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** With the Blur section obscured at launch, 'Card balance: $42,851.07', 'Account #: 1234-5678-9012-3456' and 'CVV: 042  ·  Exp: 12/29' are still ordinary labels with 'showing' and on-screen extents, so flat review reads them aloud. 'Reveal / Hide' still reads as 'Reveal / Hide push button.' with no pressed or expanded state.
- **Measured again:** verify-sceneetc-hidden-content, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 verify-sceneetc-hidden-content 'the Blur section's numbers, obscured at launch': [label] 'Account #: 1234-5678-9012-3456' states=['enabled', 'sensitive', 'showing', 'visible'] extents=[145, 700, 210, 15]; [label] 'CVV: 042  ·  Exp: 12/29' extents=[145, 717, 141, 15]`
  - `pass2 verify-sceneetc-hidden-content: the same two labels, same states and extents; tree also holds [label] 'Card balance: $42,851.07'`
  - `pass1 verify-sceneetc-hidden-content '(scene) Tab four times, to Reveal / Hide': +2462.2 ms ORCA SAYS: 'Reveal / Hide push button.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/animations/blur.rs:24-27, 158-164; examples/animations\_kit/src/main.rs:84-85, 360-395
- **Evidence (`261a218f`):**
  - `verify-sceneetc-hidden-content-20260925-155001-1603608 (15:50:39): [label] 'Account #: 1234-5678-9012-3456' states=['enabled','sensitive','showing','visible'] extents=[145, 700, 210, 15]; [label] 'CVV: 042  ·  Exp: 12/29' extents=[145, 717, 141, 15]`
  - `crates/teksilo-widgets/src/animations/blur.rs:24-27 (doc example 'Click-to-reveal sensitive content') and 158-164 (a11y-transparent, 'pair with visible_when')`
  - `examples/animations_kit/src/main.rs:84-85 (blur_obscured = true, radius 12 at launch), 360-395 (the caption says 'the numerics below are obscured until you Reveal')`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Document the AT half of the pattern in Blur (access\_hidden bound to the obscured signal, or an access\_label such as 'hidden' on the container) and apply it in the example. Give the Reveal button a pressed or expanded state.

### sceneetc-M2 {#sceneetc-m2}

Transform-mode keyboard steps are silent; only the commit speaks, as 'Moved 1 item'

- **Example:** scene-corkboard
- **Scenario:** verify-sceneetc-transform
- **Act:** scene-corkboard: select a card (Enter), focus the main pane, 't', Tab, Tab, Right, Enter, Escape
- **The reader should get:** Each step tells the reader what changed (position, or at least 'moved right'). Tab says which handle it is on, or that there is only one.
- **The reader got (`261a218f`):** 't' is read ('Selection panel. / Move selection push button.'). Tab and Tab again produce no event: with cards only 'Move selection' is offered (TransformConfig MOVE\|CORNERS, but cards are not resizable), so the roving cycles onto itself. Right arrow produces no event and no speech: the Move handle is a Button with no value, and a step changes only bounds. Enter says 'Moved 1 item' without saying where. That message goes through ctx.announce (view/transform.rs:1066), so K2 applies: after the first message of a session, later commits would be dropped until the K2 fix lands.
- **Platform:** Linux AT-SPI/Orca measured. No text or state is emitted per step, so every platform is affected.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** 't' is read ('Selection panel.' 'Move selection push button.'). Tab and Tab again put no event on the bus; the reader hears only Orca's echo 'tab', because 'Move selection' is the only handle offered for cards. Right arrow puts nothing on the bus and Orca says nothing. Enter says 'Moved 1 item' without saying where.
- **Measured again:** verify-sceneetc-transform, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 verify-sceneetc-transform 't enters transform mode': +42.1 ms object:state-changed:focused 1 [push button] 'Move selection' / +125.0 ms ORCA SAYS: 'Selection panel.' 'Move selection push button.'`
  - `pass1 verify-sceneetc-transform 'Tab 1 roves the handles': +7.9 ms ORCA SAYS: 'tab' (no event on the bus)`
  - `pass1 and pass2 verify-sceneetc-transform 'Right arrow steps the handle': FAIL  Orca says something / Orca said nothing in the act`
  - `pass1 verify-sceneetc-transform 'Enter commits': +45.6 ms object:announcement [status bar] 'Moved 1 item' text='Moved 1 item' / +56.1 ms ORCA SAYS: 'Moved 1 item'`
  - `pass2 verify-sceneetc-transform 'Enter commits': +76.2 ms ORCA SAYS: 'Moved 1 item'`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/view/transform.rs:257-276, 761-775, 1066
- **Evidence (`261a218f`):**
  - `verify-sceneetc-transform (3 runs): 'Tab 1 roves the handles' / 'Tab 2' / 'Right arrow steps the handle' -> no event, Orca said nothing (3 of 3); 'Enter commits' -> object:announcement [status bar] 'Moved 1 item' / ORCA 'Moved 1 item'`
  - `tree after 't': [panel] 'Selection' > [push button] 'Move selection' {focused} (only handle)`
  - `crates/teksilo-scene/src/view/transform.rs:761-775 (Tab roves visible_handles), 257-276 (resize handles dropped unless every root supports resize), 1066 (ctx.announce)`
- **Reproduced:** 3 of 3 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Announce each keyboard step (or publish the frame's position as the Move handle's value/description), and have Tab in transform mode say when there is only one handle.

### sceneetc-M3 {#sceneetc-m3}

Arrow-key panning of a focused SceneView gives the reader no feedback

- **Example:** scene-showcase
- **Scenario:** sceneetc-showcase
- **Act:** scene-showcase: Tab onto the scene, Right arrow
- **The reader should get:** The reader learns that the view moved and roughly where, as the minimap's arrows already say ('Viewport at 62% across …').
- **The reader got (`261a218f`):** Nothing is spoken. The bus carries only the example's own 'Pan: (…)' label renamed about 8 times per press (animation frames) and 525-600 object:bounds-changed events in about 140 ms (Orca does not listen to those). Items leave and enter the filtered tree silently.
- **Platform:** Linux AT-SPI/Orca measured. Nothing is emitted for the pan, so every platform is affected.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Right arrow on the focused scene still pans it with nothing spoken. The bus carries only the example's own 'Pan: (…)' label changing name four to seven times per press, and 300 to 525 bounds-changed events in under 200 ms, which Orca does not listen to. The minimap's arrows, by contrast, are announced ('Viewport at 62% across, …').
- **Measured again:** sceneetc-showcase, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 sceneetc-showcase 'Right arrow pans the scene': +36.1 ms object:property-change:accessible-name [label] 'Pan: (  -0.1,    0.0)' text='Pan: (  -0.1,    0.0)' ... +164.6 ms object:property-change:accessible-name [label] 'Pan: (-197.1,    0.0)' text='Pan: (-197.1,    0.0)' (seven name changes); 525 object:bounds-changed in 131 ms (events.jsonl); FAIL  Orca says any of ('pan', 'moved', 'viewport') / Orca said nothing`
  - `pass2 sceneetc-showcase 'Right arrow pans the scene': +92.6 ms object:property-change:accessible-name [label] 'Pan: (  -0.1,    0.0)' text='Pan: (  -0.1,    0.0)' ... +240.9 ms object:property-change:accessible-name [label] 'Pan: (-197.1,    0.0)' text='Pan: (-197.1,    0.0)' (four name changes); 300 object:bounds-changed in 183 ms (events.jsonl); FAIL  Orca says any of ('pan', 'moved', 'viewport') / Orca said nothing`
  - `pass1 sceneetc-showcase 'Right arrow on the minimap, press 1': +59.8 ms ORCA SAYS: 'Viewport at 62% across, 41% down; showing 64% of the width and 77% of the height'`
  - `pass2 sceneetc-showcase 'Right arrow on the minimap, press 1': +630.4 ms ORCA SAYS: 'Viewport at 62% across, 41% down; showing 64% of the width and 77% of the height'`
- **Where (`c198e4d1`):** crates/teksilo-scene/src/view/gestures\_impl.rs:1231-1273 (compare crates/teksilo-scene/src/minimap.rs:706-715)
- **Evidence (`261a218f`):**
  - `sceneetc-showcase (3 runs) 'Right arrow pans the scene': Orca said nothing; e.g. sceneetc-showcase-20260925-152908-1492203: 600 bounds-changed in 149 ms, +18.2..+162.1 ms eight object:property-change:accessible-name [label] 'Pan: ( -60.9, 0.0)' ...`
  - `crates/teksilo-scene/src/view/gestures_impl.rs:1231-1273 (arrow pan: no announce), compare crates/teksilo-scene/src/minimap.rs:706-715 (announces)`
- **Reproduced:** 3 of 3 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Announce keyboard pans through ctx.announce with the same readout the minimap uses (overridable), throttled to the end of the tween.

### sceneetc-M4 {#sceneetc-m4}

animations-kit: the Shake 'invalid input' demo says nothing to a reader

- **Example:** animations-kit
- **Scenario:** verify-sceneetc-hidden-content
- **Act:** animations-kit: Space on 'Submit' (shakes the 'incorrect-password-input-field' card)
- **The reader should get:** Invalid-input feedback reaches the reader (Shake's own doc says to pair it with another cue).
- **The reader got (`261a218f`):** No event and no speech. The shaken card is a plain label, and Submit only bumps the shake trigger.
- **Platform:** Linux AT-SPI/Orca measured. Every platform is affected.
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Space on 'Submit' shakes the card but puts no event on the bus. The reader hears only Orca's echo of the key, 'space'; nothing says the input was invalid.
- **Measured again:** verify-sceneetc-hidden-content, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 verify-sceneetc-hidden-content 'Space on Submit (shakes the 'incorrect password' field)': +48.8 ms ORCA SAYS: 'space' (no event on the bus)`
  - `pass2 verify-sceneetc-hidden-content 'Space on Submit (shakes the 'incorrect password' field)': +14.6 ms ORCA SAYS: 'space' (no event on the bus)`
- **Where (`c198e4d1`):** examples/animations\_kit/src/main.rs:262-280; crates/teksilo-widgets/src/animations/shake.rs:25-30
- **Evidence (`261a218f`):**
  - `verify-sceneetc-hidden-content (3 runs) 'Space on Submit': no event on the bus, Orca said nothing (3 of 3)`
  - `crates/teksilo-widgets/src/animations/shake.rs:25-30 ('Pair with another a11y-friendly cue (red border, error text)')`
  - `examples/animations_kit/src/main.rs:262-280`
- **Reproduced:** 3 of 3 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Have the example announce an error message (ctx.announce or a live error text) alongside the shake, as the Shake doc asks.
