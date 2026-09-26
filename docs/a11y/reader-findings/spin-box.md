<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Spin boxes

Examples: `spin-box`.
13 findings: 2 critical, 3 high, 3 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [spinbox-01](#spinbox-01) | spin-box | After scrolling down and back, every spin box that scrolled out is defunct to libatspi and Orca goes silent on it | critical | Linux | fixed |
| [spinbox-02](#spinbox-02) | spin-box | A live locale switch leaves SpinBox parsing, stepping, focusing and filtering in the build-time locale: typing 12,5 in French writes 125 | critical | all | fixed |
| [spinbox-03](#spinbox-03) | spin-box | Caret moves and selections inside a SpinBox field never reach the accessibility tree | high | all | fixed |
| [spinbox-04](#spinbox-04) | spin-box | Special value text 'Auto' is lost to a reader: Tab-in says 'Timeout 0' | high | all | fixed |
| [spinbox-05](#spinbox-05) | spin-box | Leaving Timeout makes Orca start 'Text unselected.' and cut it: the blur rewrites the text before the focus moves | low | Linux | fixed |
| [spinbox-06](#spinbox-06) | spin-box | A spin box's unit (pt, dB, %, s, Hz) never reaches a reader | high | all | open |
| [spinbox-07](#spinbox-07) | spin-box | Invalid input is reverted silently: the reader is never told the entry was refused | medium | Linux | open |
| [spinbox-08](#spinbox-08) | spin-box | Read-only spin box is announced 'grayed' (as disabled) | medium | Linux | upstream |
| [spinbox-09](#spinbox-09) | spin-box | Each SpinBox's decorative divider is published as an unnamed separator (splitter on macOS) | low | Linux | open |
| [spinbox-10](#spinbox-10) | spin-box | The published step disagrees with the adaptive step a key press takes | low | Linux | open |
| [spinbox-11](#spinbox-11) | spin-box | AccessKit emits text-changed events for off-screen (filtered) nodes; Orca drains them as dead objects and lags | low | Linux | upstream |
| [spinbox-v01](#spinbox-v01) | spin-box | A value clamped as focus leaves is cut at once: type 500 and Tab, and the reader never learns the box holds 96 | medium | Linux | open |
| [spinbox-v02](#spinbox-v02) | spin-box | Orca says 'Text unselected.' on the first character typed into any spin box | low | Linux | upstream |

### spinbox-01 {#spinbox-01}

After scrolling down and back, every spin box that scrolled out is defunct to libatspi and Orca goes silent on it

- **Example:** spin-box
- **Scenario:** spinbox-scroll-back
- **Act:** Tab from Font size down to Port (the page scrolls; the top boxes leave the AT-SPI tree), then Shift+Tab back up to Timeout, Opacity, Gain, Font size, and press Up on Timeout and Font size
- **The reader should get:** Each box is heard again on the way back ('Timeout 0 spin button.', 'Opacity 50 spin button.', ...), and Up on Timeout says '1', as on the way down.
- **The reader gets:** Nothing. Focus lands on each box (object:state-changed:focused 1 is on the bus), but Orca drops every event from them as defunct. Six Shift+Tabs are silent (Frequency, Font size no buttons, Timeout, Opacity, Gain, Font size), and so is Up on Timeout. Orca's locus of focus stays on the last live box, 'Font size mirror'. So the '13' Orca says after Up on Font size is the mirror's value change, spoken as if the mirror had focus. libatspi reports the returned node as defunct and focused at the same time. Why: AccessKit's common\_filter drops a clipping parent's off-screen children, and Teksilo's ScrollArea marks its node clips\_children. accesskit\_atspi\_common's remove\_node then emits state-changed:defunct 1. When the node comes back under the same path it gets only a children-changed:add, and nothing clears the DEFUNCT state libatspi cached. libatspi 2.52 also cannot parse AccessKit's cache AddAccessible signal, as the dbind warning shows. The same filter keeps everything below the fold (Population, Population ungrouped, Port, Reset all) off the bus until Tab scrolls it into view.
- **Platform:** Linux AT-SPI/Orca 46.1, measured. The defunct half is AT-SPI-specific. The off-screen filtering is the consumer's common\_filter, which all three adapters use to list children (from the source, not measured).
- **Severity:** critical; **layer:** upstream
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: spinbox-scroll-back had 6 failed checks and 36 defunct drops in the sweep, none now.
- **Where:** Trigger: crates/teksilo-widgets/src/scroll\_area.rs:1304-1305 (Role::ScrollView + clips\_children; also crates/teksilo-scene/src/scroll\_view.rs:550). Root cause is upstream: accesskit\_atspi\_common-0.20.0/src/adapter.rs:91-105 plus add\_node 49-81, and accesskit\_unix-0.23.0/src/atspi/bus.rs:434-438.
- **Evidence:**
  - `events.jsonl (scroll-back-20260925-131252): 13:13:11.525188 object:children-changed:remove [panel] '' -> [spin button] 'Font size' (path .../79228163325921076836764221440)`
  - `13:13:11.527585 object:state-changed:defunct 1 [spin button] 'Font size' (same path)`
  - `13:13:48.134264 object:children-changed:add [panel] '' -> [spin button] 'Font size' (same path); 13:13:48.137086 object:state-changed:focused 1 [spin button] 'Font size'`
  - `report.txt, act 'Shift+Tab to Font size': FAIL  Orca says 'Font size' / Orca unheard: 'Font size'; FAIL  [spin button] 'Font size' lacks state 'defunct' / [spin button] 'Font size' states=['defunct', 'editable', 'enabled', 'focusable', 'focused', 'selectable-text', 'sensitive', 'showing', 'single-line', 'visible']`
  - `orca-debug.out: 13:13:48.176111 - EVENT MANAGER: Ignoring defunct object: [spin button: 'Font size']`
  - `orca-debug.out: 13:13:32.139167 - EVENT MANAGER: Ignoring defunct object: [spin button: 'Timeout'] (the Shift+Tab to Timeout act; the same for 'Opacity' at 13:13:40.263743 and 'Gain')`
  - `orca-debug.out (Up on Font size): 13:13:52.255719 - AXValue: Current value of [spin button: 'Font size mirror'] is 13.0 / 13:13:52.255879 - FOCUS MANAGER: Locus of focus is [spin button: 'Font size mirror'] / 13:13:52.267605 - SPEECH OUTPUT: '13'`
  - `launch of every run: dbind-WARNING **: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau`
  - `spinbox-tree report: FAIL  the tree holds [spin button] 'Population' / no such node in the tree after the act (the same for 'Port' and [push button] 'Reset all')`
  - `source: accesskit_consumer-0.39.0/src/filters.rs:64-88 (clips_children excludes off-screen subtrees); crates/teksilo-widgets/src/scroll_area.rs:1304-1305 (builder.set_role(Role::ScrollView); builder.inner_mut().set_clips_children()); orca event_manager.py:797-798 (is_defunct -> 'Ignoring defunct object')`
  - `spinbox-scroll-back-20260925-132538-2978315, setup 'Tab until Port': +6907.3 ms object:children-changed:remove [panel] '' -> [spin button] 'Font size' / +6908.7 ms object:state-changed:defunct 1 [spin button] 'Font size'`
  - `same run, act 'Shift+Tab to Timeout': +48.7 ms object:children-changed:add [panel] '' -> [spin button] 'Timeout' / +49.3 ms object:state-changed:focused 1 [spin button] 'Timeout' / FAIL Orca says 'Timeout' / orca-debug.out 13:26:17.672264 - EVENT MANAGER: Ignoring defunct object: [spin button: 'Timeout']`
  - `same run, act 'Up on Font size': 13:26:37.600599 - AXValue: Current value of [spin button: 'Font size mirror'] is 13.0 / 13:26:37.600728 - FOCUS MANAGER: Locus of focus is [spin button: 'Font size mirror'] / 13:26:37.610372 - SPEECH: Last spoke 25.7771 seconds ago / 13:26:37.610435 - SPEECH OUTPUT: '13'`
  - `verify-spinbox-return-state-20260925-133304-3366731, act 'a fresh AT-SPI client reads the returned Font size's states': fresh client read [{'states': ['editable','enabled','focusable','focused','selectable-text','sensitive','showing','single-line','visible']}] / pass [spin button] 'Font size' has state 'defunct' (the listener's cached view)`
  - `same run, acts 'Up'/'Down on the returned Font size': +17.8 ms object:property-change:accessible-value [spin button] 'Font size' / FAIL Orca says something: 'Orca said nothing in the act' / 13:33:41.042091 and 13:33:45.044155 - EVENT MANAGER: Ignoring defunct object: [spin button: 'Font size']`
  - ``accesskit_consumer-0.39.0/src/filters.rs:66-86 (a clipped child outside the parent's box is excluded unless an adjacent filtered sibling intersects it); accesskit_atspi_common-0.20.0/src/adapter.rs:105 StateChanged(State::Defunct, true) in remove_node; add_node 49-81 has no Defunct-false; accesskit_unix-0.23.0/src/atspi/bus.rs:434-438 + 456 (CacheItem struct passed as the signal body); `strings libatspi.so.0.0.1`: '((so)(so)(so)a(so)assusau)', '((so)(so)(so)iiassusau)', 'AT-SPI: AddAccessible with unknown signature %s'; orca event_manager.py:797-798``
  - `spinbox-tree-20260925-133636-3367096: FAIL the tree holds [spin button] 'Population' / 'Port' / [push button] 'Reset all' (below the fold, off the bus at launch)`
- **Reproduced:** 3 of 3 runs (scroll-back 20260925-130708, -130925, -131252): every box that had scrolled out is silent on return in each run.
- **Verification:** confirmed. Reproduced: 4 of 4 of my runs: scroll-back x3 and verify-spinbox-return-state x1, plus the sweep's 3 of 3. In every run each box that had scrolled out was silent on return. A box that never scrolled out (Gain in the return-state run) was heard normally.
- **Fix idea:** Upstream: accesskit\_atspi\_common should not emit defunct for a node that is only filtered out, or should re-register it with defunct cleared when it comes back. Teksilo, until then: stop publishing clips\_children on the ScrollArea's AccessKit node (scroll\_area.rs:1305). The consumer filter then keeps the page's off-screen controls in the tree, which also puts the content below the fold back on the bus. This affects every ScrollArea in the framework, not only this example.

### spinbox-02 {#spinbox-02}

A live locale switch leaves SpinBox parsing, stepping, focusing and filtering in the build-time locale: typing 12,5 in French writes 125

- **Example:** spin-box
- **Scenario:** spinbox-locale
- **Act:** Language combo: Down, Enter picks français (every unfocused box re-renders: Gain '0,0', Frequency '440,00', Population '1 234 567'). Then focus Gain and press Down; focus Frequency, select all, type '12,5', Enter; Tab to Population
- **The reader should get:** Focused and stepped boxes keep the French form ('-0,5'). Typing 12,5 commits 12.5 and the reader hears '12,50'. Population reads '1 234 567'.
- **The reader gets:** Focusing a box switches its text back to the English form (',' -&gt; '.'). Down on Gain says '-0.5'. In Frequency the comma is filtered out as it is typed, so '125' commits and the reader hears '125.00': the value is wrongly written (125 Hz instead of 12.5). Population is announced 'Population 1,234,567 spin button.', which a French reader takes as a decimal. This is not specific to readers: the comment at spin\_box.rs:892-894 expects 'the closures below re-resolve on the next build', but a locale switch does not rebuild. Only the locale effect (line 993) re-resolves NumberPresentation. The focus effect, set\_committed, the parse, step\_silent and the char filter all keep the one resolved at build (line 895).
- **Platform:** All platforms and all users (widget logic). Measured on Linux/Orca.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `a9f25fd0` (spinbox). Fixed part: after a live locale switch a SpinBox now shows, reads, steps, filters and commits in the language switched to (typing 12,5 in French writes 12.5, shown and spoken '12,50'; Down on Gain says '-0,5'; Population reads '1 234 567' with U+202F.
- **Where:** crates/teksilo-widgets/src/spin\_box.rs:895 (resolved once), captured by the value effect 951, set\_committed 1023, parse 1057, step\_silent 1188, char\_filter 1298, focus effect 1400; only the locale effect 983-1003 re-resolves
- **Evidence:**
  - `report.txt (locale-20260925-131609), act 'Language: Down, Enter picks français': pass  [spin button] 'Gain' has value 0 and text '0,0'; pass  [spin button] 'Frequency' has value 440 and text '440,00'`
  - `act 'setup: focus Gain': +41.4 ms object:text-changed:delete [spin button] 'Gain' text=',' / +41.7 ms object:text-changed:insert [spin button] 'Gain' text='.'`
  - `act 'Gain: Down in French': +22.2 ms object:text-changed:insert [spin button] 'Gain' text='-0.5'; orca-debug.out 13:16:30.075621 - SPEECH OUTPUT: '-0.5'`
  - `act 'Frequency: select all, type 12,5 then Enter': FAIL  [spin button] 'Frequency' has value 12.5 and text '12,50' / [spin button] 'Frequency' value={'current': 125.0, ...} text='125.00'; orca-debug.out 13:16:38.305133 - SPEECH OUTPUT: '125.00'`
  - `act 'Tab to Population (grouped) in French': object:text-changed:delete [spin button] 'Population' text=' 234 ' / object:text-changed:insert [spin button] 'Population' text=',234,'; orca-debug.out 13:16:50.996424 - SPEECH OUTPUT: 'Population 1,234,567 spin button.'`
  - `the locale stayed French throughout: the tree after each act holds [tool bar] "Barre d'outils" and [combo box] 'Thème'`
  - `source: crates/teksilo-widgets/src/spin_box.rs:895 (resolved once), captured at 951, 1023, 1057 (parse), 1188 (step_silent), 1298 (char_filter), focus effect 1382-1406; only the locale effect at 983-1003 re-resolves`
  - `spinbox-locale-20260925-133053-3240601, act 'Frequency: select all, type 12,5 then Enter': object:text-changed:insert [spin button] 'Frequency' text='1' / text='2' / text='5' / text='.00'; FAIL value={'current': 125.0, ...} text='125.00'; orca-debug.out 13:31:22.595252 - SPEECH OUTPUT: '125.00' (and 13:32:23.245985 in spinbox-locale-20260925-133154-3240601)`
  - `same runs, act 'Gain: Down in French': object:text-changed:insert [spin button] 'Gain' text='-0.5'; 13:31:14.420577 / 13:32:15.101785 - SPEECH OUTPUT: '-0.5'`
  - `same runs, act 'Tab to Population (grouped) in French': object:text-changed:delete ... text=' 234 ' / insert text=',234,'; 13:31:35.323130 / 13:32:35.965226 - SPEECH OUTPUT: 'Population 1,234,567 spin button.'`
  - `verify-spinbox-locale-blur-20260925-133449-3366731, act 'Gain: Down in French, then Tab away': FAIL [spin button] 'Gain' has value -0.5 and text '-0,5' / text='-0.5' with states lacking 'focused'`
  - `same run, act 'Frequency: ... type 12.5 ... Enter': value 12.5, text='12.50'; 13:35:20.264815 - SPEECH OUTPUT: '12.50'`
  - `crates/teksilo-core/src/widget_tree.rs:1419-1427 (set_locale: signal + mark_all_dirty, no rebuild)`
- **Reproduced:** 2 of 2 runs (locale 20260925-131426 and -131609). The behaviour is deterministic.
- **Verification:** confirmed. Reproduced: 2 of 2 of my runs (spinbox-locale x2) plus verify-spinbox-locale-blur, and the sweep's 2 of 2. Deterministic.
- **Fix idea:** Hold the presentation in a shared Rc&lt;RefCell&lt;NumberPresentation&gt;&gt; (or a Signal) that the locale effect updates, and have every closure (display, focus, commit parse, step, char filter) read it at call time. Or resolve it inside each closure, as the locale effect already does.

### spinbox-03 {#spinbox-03}

Caret moves and selections inside a SpinBox field never reach the accessibility tree

- **Example:** spin-box
- **Scenario:** spinbox-caret
- **Act:** Focus Font size ('12', all selected), then Home, Right, End, Left, Shift+Home, then type '3'
- **The reader should get:** Each move publishes the new caret (object:text-caret-moved), and Shift+Home a text-selection-changed, so a reader reviewing the number hears what the caret crosses and braille follows.
- **The reader gets:** No event on the bus for Home, Right, End, Left or Shift+Home. Text.caretOffset stays at 2 throughout. The caret really moves: typing 3 after Shift+Home gives '32' with the caret at 1. The tree catches up only when the text changes, and then Orca says 'Text unselected.' for the stale whole-text selection it still held. The cause is in TextInputField, the stack TextInput, PasswordField and SearchField share: the key handler calls sync\_cursor\_signals and request\_frame, and nothing binds cursor\_position, cursor\_anchor or has\_selection for AT. Only text\_signal is bound at AccessibilityOnly. So the same probably holds for every text field, but only SpinBox was measured here. For the same reason the selection a field clears on blur is never published, which feeds spinbox-05.
- **Platform:** All platforms by source: the AccessKit tree is not updated, so no adapter can raise AT-SPI caret events, UIA TextSelectionChanged or macOS AXSelectedTextChanged. Measured on Linux.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:452-481 (only text\_signal is bound at AccessibilityOnly among the text state) and 193-207 (caret\_position mirror, no AT binding); keyboard.rs:294-297
- **Evidence:**
  - `report.txt (caret-20260925-131838), act 'Home': FAIL  [spin button] 'Font size' caret at 0 / [spin button] 'Font size' text={'characters': 2, 'text': '12', 'caret': 2}; FAIL  a object:text-caret-moved event from [spin button] '*' / no object:text-caret-moved event from [spin button] '*'`
  - `acts 'Right' and 'Left': the same, text={'characters': 2, 'text': '12', 'caret': 2}`
  - `act 'Shift+Home (select the 1)': FAIL  a object:text-selection-changed event from [spin button] '*' / no object:text-selection-changed event from [spin button] '*'`
  - `act 'type 3': +12.2 ms object:text-changed:delete [spin button] 'Font size' text='1' / +12.6 ms object:text-changed:insert ... text='3' / +12.8 ms object:text-selection-changed / pass  [spin button] 'Font size' caret at 1, text '32'; orca-debug.out 13:19:10.568504 - SPEECH OUTPUT: 'Text unselected.'`
  - `source: crates/teksilo-widgets/src/primitives/text_input_field/keyboard.rs:293-297 (sync_cursor_signals; request_frame; no AT dirtying); state.rs:575-615 (publish_cursor_signals writes cursor_position/cursor_anchor/has_selection); widget_impl.rs:452-481 (only text_signal is bound at AccessibilityOnly)`
  - `spinbox-caret-20260925-133255-3240601 and -133344-3240601, act 'Home': FAIL [spin button] 'Font size' caret at 0 / text={'characters': 2, 'text': '12', 'caret': 2}; FAIL a object:text-caret-moved event / no object:text-caret-moved event (same for Right, End, Left; Shift+Home: no object:text-selection-changed)`
  - `same runs, act 'type 3': +12.0 ms object:text-changed:delete [spin button] 'Font size' text='1' / +12.5 ms insert text='3' / +12.7 ms object:text-selection-changed / pass caret at 1, text '32'; ORCA SAYS 'Text unselected.'`
  - `verify-spinbox-silent-clamp-20260925-133358-3366731: while typing '200', object:text-caret-moved [spin button] 'Font size' follows each object:text-changed:insert, so the caret is published only as a side effect of a text change`
- **Reproduced:** 3 of 3 runs (caret 20260925-130054, -130708, -131838). No timing is involved.
- **Verification:** confirmed. Reproduced: 2 of 2 of my caret runs plus the sweep's 3 of 3. Deterministic.
- **Fix idea:** In TextInputField::build, bind cursor\_position, cursor\_anchor and has\_selection at BindingLevel::AccessibilityOnly, as text\_signal is at widget\_impl.rs:473-481, so every caret or selection change re-walks the field's node.

### spinbox-04 {#spinbox-04}

Special value text 'Auto' is lost to a reader: Tab-in says 'Timeout 0'

- **Example:** spin-box
- **Scenario:** spinbox-special
- **Act:** Tab from Opacity to Timeout (value 0 = minimum, shown 'Auto' while unfocused); also Shift+Tab back to it
- **The reader should get:** The reader hears 'Timeout Auto spin button.', the meaning the box has at its minimum and what a sighted user saw a moment before.
- **The reader gets:** 'Timeout 0 spin button.' The field swaps 'Auto' for '0' in the same update as the focus change, before the focus event, so no reader ever hears Auto on arrival. A reader who hears 'Timeout 0' has no way to know that 0 means automatic. The widget is also inconsistent while focused: Down back to the minimum and Enter at the minimum both put 'Auto' back into the focused field. Stepping is announced 'Auto', but Enter with 0 typed changes the text to 'Auto' silently (Orca says only 'Text unselected.'). The focus-in swap exists so the number can be edited, but Qt's QSpinBox keeps specialValueText while focused.
- **Platform:** All platforms (the value published is the field's text). Measured on Linux/Orca.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `a9f25fd0` (spinbox). Fixed part: at its minimum a special-value SpinBox keeps 'Auto' while focused, so arrival says 'Timeout Auto spin button.', and Down back to the minimum says 'Auto' again (matches Qt QSpinBox specialValueText.
- **Where:** crates/teksilo-widgets/src/spin\_box.rs:1378-1406 (focus effect: format\_for\_display(..., special=None, force\_plain=true)); set\_committed 1017-1044 and step\_silent's display 1193-1203 pass force\_plain=false
- **Evidence:**
  - `tabwalk report, Tab 6: +32.6 ms object:text-changed:delete [spin button] 'Timeout' text='Auto' / +33.0 ms object:text-changed:insert [spin button] 'Timeout' text='0' / +33.1 ms object:state-changed:focused 1 [spin button] 'Timeout' / +292.0 ms ORCA SAYS: 'Timeout 0 spin button.'`
  - `report.txt (special-20260925-131130), act 'Tab to Timeout (value 0 = Auto)': FAIL  Orca says 'Auto' / Orca unheard: 'Auto' / Orca said: 'Timeout 0 spin button.'; orca-debug.out 13:11:43.123012 - SPEECH OUTPUT: 'Timeout 0 spin button.'`
  - `the same run, act 'Down back to the minimum': +17.1 ms object:text-changed:insert [spin button] 'Timeout' text='Auto'; orca-debug.out 13:11:50.922555 - SPEECH OUTPUT: 'Auto'`
  - `the same run, act 'Shift+Tab back to Timeout, select all, type 0, Enter': +1485.4 ms object:text-changed:insert [spin button] 'Timeout' text='Auto' / +1514.9 ms ORCA SAYS: 'Text unselected.' / FAIL  Orca says 'Auto'`
  - `tree at launch: [spin button] 'Timeout' value={'current': 0.0, ...} text={'characters': 4, 'text': 'Auto', 'caret': 0}`
  - `source: crates/teksilo-widgets/src/spin_box.rs:1382-1406 (focus effect formats with special=None, force_plain=true); set_committed 1017-1044 and step_silent's display 1193-1203 format with the special text`
  - `spinbox-special-20260925-132842-2978613, act 'Tab to Timeout (value 0 = Auto)': +15.7 ms object:text-changed:delete [spin button] 'Timeout' text='Auto' / +16.3 ms insert text='0' / +16.4 ms object:state-changed:focused 1 [spin button] 'Timeout'; orca-debug.out 13:28:54.029836 - SPEECH OUTPUT: 'Timeout 0 spin button.'`
  - `same run, act 'Down back to the minimum': +18.4 ms object:text-changed:insert [spin button] 'Timeout' text='Auto'; 13:29:01.857347 - SPEECH OUTPUT: 'Auto'`
  - `same run, act 'Shift+Tab back to Timeout, select all, type 0, Enter': +1485.8 ms object:text-changed:insert [spin button] 'Timeout' text='Auto' / +1524.8 ms ORCA SAYS: 'Text unselected.' / FAIL Orca says 'Auto' (and the same in spinbox-special-20260925-132932-2978613)`
- **Reproduced:** Every Tab-in to Timeout at 0 across 6 runs (tabwalk, special x2, leave-timeout x3 acts, scroll-back setup). The behaviour is deterministic.
- **Verification:** confirmed. Reproduced: Every Tab-in or Shift+Tab-in to Timeout at 0 in my runs said 'Timeout 0 spin button.': special x2 (2 per run), leave-timeout x3 (6 per run), and the setups of scroll-back x3 and return-state. Deterministic.
- **Fix idea:** Keep the special text in the field while focused, as Qt does (the field selects all on focus, so typing replaces it). This also removes the blur flip behind spinbox-05. If the swap stays, at least publish the special text on the node, for example in its description or value, while the number is at the minimum.

### spinbox-05 {#spinbox-05}

Leaving Timeout makes Orca start 'Text unselected.' and cut it: the blur rewrites the text before the focus moves

- **Example:** spin-box
- **Scenario:** spinbox-leave-timeout
- **Act:** Tab away from Timeout while it holds '0' (it flips back to 'Auto' on blur)
- **The reader should get:** The reader hears only the next control, 'Frequency 440.00 spin button.'
- **The reader gets:** A cut fragment 'Text unselected.' first, then Frequency. The blur commit rewrites the field text ('0' -&gt; 'Auto') and clears its selection in the same tree update as the focus move. The consumer hands node changes to the adapter before the focus event. The old node was still focused in the previous tree, so accesskit\_atspi\_common emits text-selection-changed for it (adapter.rs:216-229). Orca handles that first, finds its cached selection '0' gone and says SELECTION\_REMOVED (script\_utilities.py:4001-4003), then stops speech to present the new focus. Boxes whose text does not change on blur produce no such event, only because their cleared selection never reaches the tree (spinbox-03).
- **Platform:** Linux AT-SPI/Orca 46.1, measured. Windows and macOS not measured.
- **Severity:** low; **layer:** framework
- **Status:** Fixed by `a9f25fd0` (spinbox). Fixed part: a side effect, not in this topic's JSON): leaving Timeout at its minimum no longer rewrites the text, so Orca no longer says 'Text unselected.' (3 of 3 spinbox-special runs, 3 of 3 leave-timeout cycles.
- **Where:** crates/teksilo-widgets/src/spin\_box.rs:1372-1376 (on\_blur\_fn -&gt; commit -&gt; set\_committed shows the special text) with the focus swap at 1378-1406
- **Evidence:**
  - `report.txt (leave-timeout-20260925-130924), act 'Tab away from Timeout (1)': +160.5 ms object:text-changed:delete [spin button] 'Timeout' text='0' / +161.0 ms object:text-changed:insert [spin button] 'Timeout' text='Auto' / +163.8 ms object:text-selection-changed [spin button] 'Timeout' / +166.7 ms object:state-changed:focused 1 [spin button] 'Frequency' / +402.7 ms ORCA SAYS (CUT): 'Text unselected.' / +997.3 ms ORCA SAYS: 'Frequency 440.00 spin button.'`
  - `observed  Orca's 'Text unselected.' was cut by a stop 594 ms in (estimated)`
  - `orca-debug.out: 13:09:44.656846 - SPEECH OUTPUT: 'Text unselected.' ... then 13:09:45.251212 - NULL SPEECH: stop / 13:09:45.251429 - SPEECH OUTPUT: 'Frequency 440.00 spin button.'`
  - `the same in acts (2) and (3): 13:09:57.741340 and 13:10:10.964351 - SPEECH OUTPUT: 'Text unselected.', each followed by NULL SPEECH: stop`
  - `source: crates/teksilo-widgets/src/spin_box.rs:1373-1376 (on_blur_fn -> commit -> set_committed -> show special text); accesskit_atspi_common-0.20.0/src/adapter.rs:216-229; orca script_utilities.py:4001-4003`
  - `spinbox-leave-timeout-20260925-132539-2978613, act 'Tab away from Timeout (1)': +12.9 ms object:text-selection-changed [spin button] 'Timeout' / +13.1 ms object:state-changed:focused 1 [spin button] 'Frequency' / +44.1 ms ORCA SAYS (CUT): 'Text unselected.' / +115.3 ms ORCA SAYS: 'Frequency 440.00 spin button.'; orca-debug.out 13:25:55.320240 - NULL SPEECH: speak 'Text unselected.' interrupt=True / 13:25:55.391307 - NULL SPEECH: stop / 13:25:55.391389 - SPEECH OUTPUT: 'Frequency 440.00 spin button.'`
  - `cut offsets across the 9 deliberate Tab-aways: 71, 60, 85, 98, 86, 64, 81, 49, 86 ms; the Shift+Tab-aways all at 0 ms`
  - `spinbox-special-20260925-132842-2978613, act 'Tab away from Timeout (text already Auto)': only +14.7 ms object:state-changed:focused 1 [spin button] 'Frequency' / ORCA SAYS 'Frequency 440.00 spin button.' / pass Orca does not say 'unselected'`
- **Reproduced:** 9 of 12 blur flips across 7 runs said a cut 'Text unselected.': 7 of 7 deliberate single Tab-aways (leave-timeout x3, special x2, tabwalk, scroll-back setup) and 2 of 5 flips inside fast multi-key acts.
- **Verification:** corrected by the verifier. Reproduced: 18 of 18 flips in my 3 leave-timeout runs (3 Tab-aways and 3 Shift+Tab-aways per run), 2 of 2 in special x2, and 1 in each scroll-back/return-state setup. Each time a cut 'Text unselected.' preceded the next control. Counter-case: 0 of 2 when Timeout already showed 'Auto' (stepped to the minimum) as focus left. The mechanism is exactly as described. The blur commit rewrites Timeout '0' -&gt; 'Auto' in the same update as the focus move. accesskit\_atspi\_common emits text-selection-changed for the old node because it was focused in the old tree (adapter.rs:218-231, 'if !old\_node.is\_focused() \|\| ... return'). Orca's cached selection '0' then disappears. Orca never presents text changes in a spin button (script\_utilities.py:3696-3702), so it never refreshed that cache, and it says SELECTION\_REMOVED (script\_utilities.py:4001-4003). The counter-case confirms the flip is the cause. I corrected the severity. On an idle Orca, the stop comes 0-98 ms after 'Text unselected.' starts, for example 13:25:55.320240 speak and 13:25:55.391307 stop, 71 ms apart. A reader therefore hears at most a syllable, or nothing if speech-dispatcher's audio start latency is longer. The 594 ms in the sweep came from a lagging Orca. That makes it cosmetic noise (low), not medium misleading speech. The fix for spinbox-04 removes it.
- **Fix idea:** The fix for spinbox-04 (no special-text swap on focus) removes the flip. Otherwise, defer the reformat until focus has left, so it lands in a later tree update than the focus change.

### spinbox-06 {#spinbox-06}

A spin box's unit (pt, dB, %, s, Hz) never reaches a reader

- **Example:** spin-box
- **Scenario:** spinbox-steps
- **Act:** Focus Font size (or any box with a suffix); step it
- **The reader should get:** 'Font size 12 pt spin button.' (or the unit in the description), so 'Timeout 30' is known to be seconds and 'Frequency 440' hertz.
- **The reader gets:** 'Font size 12 spin button.' No spin button carries its unit anywhere: not in its name, description, text or value. The suffix is painted beside the text and published nowhere. The widget documents this as a known gap (spin\_box.rs:1422-1429, docs/accessibility-internal-audit.md:742-746), and it is reported here because a reader still loses the unit.
- **Platform:** All platforms (the value is the field text; UIA Value and macOS AXValue carry the same string or number). Measured on Linux/Orca.
- **Severity:** high; **layer:** framework
- **Status:** Open. In the spinbox fix topic, not fixed there: The unit (suffix) never reaching a reader cannot be fixed in a small, safe change. The suffix is painted beside the field's text and published nowhere. The right fix, which is what docs/accessibility-internal-audit.md already names and what Qt does (its line edit holds '1 s' as text, measured), is for TextInputField to emit its suffix as a trailing run on the same line as the editable runs, with value = text + suffix. That touches the shared TextInputField primitive (TextInput, SearchField, ColorPicker fields, TextScaleControl and SpinBox all set a suffix). It needs suffix geometry from the suffix engine in the one TextRunSource, because two push\_text\_runs calls would put the unit on a separate 'line' (the next\_on\_line / previous\_on\_line links). It also needs handle\_access\_action's SetTextSelection to map a (run node, index) position to a document offset: today it reads character\_index and ignores the run, a bug that already exists for fields over 255 characters. Tests that pin value == typed text would change. Option B is to put the unit in the spin button's description: a SpinBox-only change, but Orca 46.1 speaks it only on arrival, after the role ('Font size 12 spin button. pt'), never on a step (the SPIN\_BUTTON 'focused' format is only displayedText or value), and the description is also where a tooltip's text, access\_description and described\_by targets are written, so the unit would be mixed with or replaced by the application's own description. I recommend option A as its own change.
- **Where:** crates/teksilo-widgets/src/spin\_box.rs:1419-1453 (the comment at 1428 records the gap; access\_customize sets name/value/range/actions only)
- **Evidence:**
  - `report.txt (steps-20260925-131130), act 'focus Font size': +125.6 ms ORCA SAYS: 'Font size 12 spin button.' / FAIL  Orca says 'pt'`
  - `spinbox-tree report: FAIL  every spin button with a suffix exposes its unit / 'Font size' (unit 'pt'): description=None text='12' Value.text=None ... / 'Timeout' (unit 's'): description=None text='Auto' Value.text=None ... / 'Frequency' (unit 'Hz'): description=None text='440.00' Value.text=None`
  - `source: crates/teksilo-widgets/src/spin_box.rs:1422-1429 ('The unit reaches a reader once the field emits it as text'); 1430-1453 (access_customize sets name/value/range only)`
  - `spinbox-steps-20260925-133656-3367096, act 'focus Font size': +103.0 ms ORCA SAYS: 'Font size 12 spin button.' / FAIL Orca says 'pt'`
  - `spinbox-tree-20260925-133636-3367096: FAIL every spin button with a suffix exposes its unit / 'Timeout' (unit 's'): description=None text='Auto' Value.text=None attributes=None / 'Frequency' (unit 'Hz'): description=None text='440.00' ...`
- **Reproduced:** 2 of 2 steps runs and 2 of 2 tree runs. The behaviour is deterministic.
- **Verification:** confirmed. Reproduced: Deterministic: the steps and tree reruns (1 of 1 each), and every focus utterance in all 25 runs ('Font size 12 spin button.', 'Frequency 440.00 spin button.', ...).
- **Fix idea:** Publish the trimmed suffix on the spin button node. A description is the least intrusive: Orca speaks it on focus, and UIA FullDescription / macOS AXHelp carry it. Or expose the suffix as a trailing non-editable text run, so the announced value and the reviewable text agree, which is what the audit note asks for.

### spinbox-07 {#spinbox-07}

Invalid input is reverted silently: the reader is never told the entry was refused

- **Example:** spin-box
- **Scenario:** spinbox-typing
- **Act:** Gain (custom parser, no character filter): select all, type 'abc', Enter
- **The reader should get:** The reader learns the entry was refused and what the value is, for example 'invalid' or '0.0'.
- **The reader gets:** The text goes back from 'abc' to '0.0' and Orca says nothing about it. The only speech is 'Text unselected.', from typing over the selection. The revert re-publishes the unchanged value, so no value-change event fires. No invalid state is set and nothing is announced. A sighted user sees the text snap back; a reader is left believing 'abc' went in or that nothing happened.
- **Platform:** Linux/Orca measured. Nothing in the tree marks the refusal on any platform (by source).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/spin\_box.rs:1066-1083 (commit\_text: on a failed parse set\_committed(value\_signal.get())) and 1017-1044 (set\_committed notifies only when the value changes)
- **Evidence:**
  - `report.txt (typing-20260925-131130), act 'Gain: select all, type abc then Enter': +340.7 ms object:text-changed:delete [spin button] 'Gain' text='abc' / +341.0 ms object:text-changed:insert [spin button] 'Gain' text='0.0' / FAIL  the reader is told the entry was refused (invalid state, a message, or at least the value it went back to) / Orca said: 'Text unselected.'`
  - `orca-debug.out: 13:12:04.308030 - SPEECH OUTPUT: 'Text unselected.' (the only speech in the act)`
  - `source: crates/teksilo-widgets/src/spin_box.rs:1068-1085 (commit_text: on a failed parse set_committed(value_signal.get()), no feedback); 1017-1044 (set_committed notifies only when the value changes)`
  - `spinbox-typing-20260925-133305-3367096, act 'Gain: select all, type abc then Enter': +338.9 ms object:text-changed:delete [spin button] 'Gain' text='abc' / +339.2 ms object:text-changed:insert text='0.0' / FAIL the reader is told the entry was refused / only speech +264.1 ms 'Text unselected.' (same in spinbox-typing-20260925-133404-3367096)`
  - `verify-spinbox-silent-clamp-20260925-133358-3366731, act 'at 96: select all, type 200, Enter': +335.7 ms object:text-changed:delete [spin button] 'Font size' text='200' / +336.3 ms insert text='96' / FAIL Orca says something: 'Orca said nothing in the act'`
  - `same run, act 'at 4: select all, type 1, Enter': +265.2 ms delete text='1' / +265.5 ms insert text='4' / 'Orca said nothing in the act'`
- **Reproduced:** 2 of 2 runs (typing 20260925-130305 and -131130). The behaviour is deterministic.
- **Verification:** confirmed. Reproduced: 3 of 3 for 'abc': typing x2 plus the silent-clamp act. Also 2 of 2 clamps back to the value already held in the silent-clamp run. Deterministic.
- **Fix idea:** When a commit fails to parse, announce it through the tree's announcer (for example 'Invalid value, kept 0.0'), or drive the field's feedback to Invalid so aria-invalid reaches the node. The K2 fix makes the announcer reliable for this.

### spinbox-08 {#spinbox-08}

Read-only spin box is announced 'grayed' (as disabled)

- **Example:** spin-box
- **Scenario:** spinbox-readonly
- **Act:** Focus 'Font size mirror' (read\_only(true), enabled)
- **The reader should get:** 'Font size mirror 12 spin button, read only', an available control whose value cannot be edited.
- **The reader gets:** 'Font size mirror 12 spin button.' then 'grayed.'. The node has read-only but neither enabled nor sensitive. accesskit\_atspi\_common maps a read-only node to ReadOnly in place of Enabled\|Sensitive (node.rs:376-380), and Orca speaks 'grayed' for anything not sensitive (generator.py:565). Read-only and enabled are independent in AT-SPI.
- **Platform:** Linux AT-SPI only. By source, UIA keeps IsEnabled and reports Value/RangeValue IsReadOnly (accesskit\_windows node.rs:1316, 1347, 1358), and macOS keeps AXEnabled.
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** none in Teksilo; accesskit\_atspi\_common-0.20.0/src/node.rs:376-380
- **Evidence:**
  - `tabwalk report, Tab 9: +352.3 ms ORCA SAYS: 'Font size mirror 12 spin button.' / +352.4 ms ORCA SAYS: 'grayed.'`
  - `report.txt (readonly-20260925-130925): FAIL  Orca does not say 'grayed' / Orca said: 'grayed.'; FAIL  [spin button] 'Font size mirror' has state 'sensitive' / states=['focusable', 'focused', 'read-only', 'selectable-text', 'showing', 'single-line', 'visible']`
  - `orca-debug.out: GENERATION TIME: 0.0005 ----> availability=[grayed] / 13:09:39.821564 - SPEECH OUTPUT: 'grayed.'`
  - `source: accesskit_atspi_common-0.20.0/src/node.rs:376-380; orca generator.py:565`
  - `spinbox-readonly-20260925-133504-3367096, act 'focus the read-only mirror': ORCA SAYS 'Font size mirror 12 spin button.' / 'grayed.'; states=['focusable','focused','read-only','selectable-text','showing','single-line','visible']; orca-debug.out 13:35:13.263671 - SPEECH OUTPUT: 'grayed.' (and 13:35:59.351278 in -133551)`
  - `orca formatting.py:462-465 SPIN_BUTTON 'unfocused': 'labelAndName + (displayedText or value) + roleName + required + pause + invalid + availability + MNEMONIC' (no readOnly)`
- **Reproduced:** 4 of 4 focus landings on the mirror (tabwalk, readonly, scroll-back setup x2). The behaviour is deterministic.
- **Verification:** confirmed. Reproduced: 2 of 2 readonly reruns, plus every Tab or Shift+Tab onto the mirror in the scroll-back, locale and return-state setups. Deterministic.
- **Fix idea:** Upstream: insert Enabled\|Sensitive whenever the node is not disabled, and ReadOnly independently. Teksilo cannot work around it without dropping the read-only flag, which would be worse.

### spinbox-09 {#spinbox-09}

Each SpinBox's decorative divider is published as an unnamed separator (splitter on macOS)

- **Example:** spin-box
- **Scenario:** spinbox-tree
- **Act:** Launch; walk the tree
- **The reader should get:** Only the spin button. The 1 dp line between the field and the step buttons is decoration.
- **The reader gets:** After every box with buttons there is an unnamed \[separator\] ('' extents \[345,161,1,24\] ...), 7 on the first screen. A reader stepping through objects meets 'separator' after each box. On macOS the role is AXSplitter, which suggests a draggable divider. The cause: the chrome adds Divider::vertical(), and Divider::accessibility claims Role::Splitter.
- **Platform:** Linux measured (AT-SPI Separator). By source, Windows gets UIA Separator and macOS NSAccessibilitySplitterRole (accesskit\_windows node.rs:193, accesskit\_macos node.rs:160).
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/primitives/divider.rs:137-139 (Role::Splitter); crates/teksilo-widgets/src/styles/recipe\_spin\_box\_style.rs:76
- **Evidence:**
  - `spinbox-tree report: FAIL  the tree holds no [separator] / [separator] '' extents=[345, 161, 1, 24] index_in_parent=4 / [separator] '' extents=[385, 203, 1, 24] index_in_parent=8 / ... (7 in all)`
  - `tree-launch.txt: [spin button] 'Font size' ... / [separator] '' / [label] '12 pt'`
  - `source: crates/teksilo-widgets/src/styles/recipe_spin_box_style.rs:76 (Divider::vertical()); crates/teksilo-widgets/src/primitives/divider.rs:137-139 (Role::Splitter)`
  - `spinbox-tree-20260925-133636-3367096: FAIL the tree holds no [separator] / [separator] '' extents=[345, 161, 1, 24] index_in_parent=4 ... [separator] '' extents=[337, 484, 1, 24] index_in_parent=32 (7 in all)`
- **Reproduced:** 3 of 3 tree snapshots (tree, spinbox-tree x2). The behaviour is deterministic.
- **Verification:** confirmed. Reproduced: 1 of 1 tree rerun (the same 7 unnamed separators with the same extents as the sweep's), and the separators appear in every scroll's children-changed events. Deterministic.
- **Fix idea:** Give Divider a decorative default (GenericContainer or hidden) with an opt-in semantic separator. At the least, have RecipeSpinBoxStyle hide its divider from AT.

### spinbox-10 {#spinbox-10}

The published step disagrees with the adaptive step a key press takes

- **Example:** spin-box
- **Scenario:** spinbox-adaptive
- **Act:** Focus Frequency (440, StepType::Adaptive), press Up
- **The reader should get:** The Value interface's minimum increment (UIA SmallChange) is the step a press takes at the current value.
- **The reader gets:** The press moves 440 -&gt; 540 (step 100, and the reader hears '540.00', which is right), but the published increment stays 1.0. spin\_box.rs:1442 publishes single\_step, not resolve\_effective\_step(current).
- **Platform:** Linux measured. By source, Windows RangeValue SmallChange comes from numeric\_value\_step (accesskit\_windows node.rs:1361). macOS publishes no step.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/spin\_box.rs:1442
- **Evidence:**
  - `report.txt (adaptive-20260925-131838), act 'Frequency: Up (adaptive step)': +47.7 ms ORCA SAYS: '540.00' / FAIL  the Value interface's minimum increment is the step a press just took / [spin button] 'Frequency' value={'current': 540.0, 'minimum': 0.1, 'maximum': 20000.0, 'increment': 1.0, 'text': None} ...; one Up moved it by 100`
  - `source: crates/teksilo-widgets/src/spin_box.rs:1442 (set_numeric_value_step(single_step)); 2038-2057 (resolve_effective_step)`
  - `spinbox-adaptive-20260925-133434-3240601, act 'Frequency: Up (adaptive step)': ORCA SAYS '540.00' (13:34:46.249222) / FAIL value={'current': 540.0, 'minimum': 0.1, 'maximum': 20000.0, 'increment': 1.0, 'text': None}; one Up moved it by 100`
- **Reproduced:** 1 of 1 run. The behaviour is deterministic, with no timing involved.
- **Verification:** confirmed. Reproduced: 1 of 1 adaptive rerun (plus the sweep's 1 of 1). Deterministic, with no timing involved.
- **Fix idea:** Publish resolve\_effective\_step(step\_type, value, single\_step) and the page equivalent in access\_customize. It already re-runs on every value change, because value is bound at AccessibilityOnly.

### spinbox-11 {#spinbox-11}

AccessKit emits text-changed events for off-screen (filtered) nodes; Orca drains them as dead objects and lags

- **Example:** spin-box
- **Scenario:** spinbox-readonly
- **Act:** Opacity: PageUp x2 (the four other Opacity boxes, bound to the same signal, are below the fold and off the bus)
- **The reader should get:** Events only from nodes on the bus.
- **The reader gets:** Pairs of object:text-changed from sources that do not exist on the bus (\[&lt;Error&gt;\] '' in the listener; 'Ignoring defunct object: \[DEAD\]' in Orca). Orca's queue reached 57 events and its speech for the act came 2.5 s after the keys. node\_updated calls emit\_text\_change\_if\_needed before any filter check.
- **Platform:** Linux AT-SPI (accesskit\_atspi\_common)
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** none in Teksilo (upstream accesskit\_atspi\_common-0.20.0/src/adapter.rs:287-288)
- **Evidence:**
  - `report.txt (readonly-20260925-130925), act 'Opacity: PageUp x2 to 100': +97.8 ms object:text-changed:delete [<Error>] '' text='50' / +115.0 ms object:text-changed:insert [<Error>] '' text='75' (and 14 more such pairs) / +2480.4 ms ORCA SAYS: 'Text unselected.' / +2597.7 ms ORCA SAYS: '100'`
  - `orca-debug.out: vvvvv PROCESS OBJECT EVENT object:text-changed:delete (queue size: 57) vvvvv / 13:10:01.469826 - EVENT MANAGER: Ignoring defunct object: [DEAD]`
  - `source: accesskit_atspi_common-0.20.0/src/adapter.rs node_updated (calls emit_text_change_if_needed before filter(old)/filter(new)); 120-196`
  - `spinbox-readonly-20260925-133504-3367096, act 'Opacity: PageUp x2 to 100': +17.8 ms object:text-changed:delete [<Error>] '' text='50' / +18.4 ms object:text-changed:insert [<Error>] '' text='75' (8 such pairs) / +141.7 ms ORCA SAYS: '75'; orca-debug.out 13:35:29.101570 - EVENT MANAGER: Ignoring defunct object: [DEAD]`
- **Reproduced:** The events are structural: 3 of 3 wrap-mode acts in the run. The lag is 1 of 1 and is not claimed as a timing finding.
- **Verification:** confirmed. Reproduced: The structural events reproduced in 2 of 2 readonly reruns: 16 object:text-changed from sources not on the bus per Opacity step, and 48 'Ignoring defunct object: \[DEAD\]' per run. The lag did not reproduce (0 of 2): Orca spoke 141-221 ms after the key with at most 27-28 events queued. The sweep did not claim the lag.
- **Fix idea:** Upstream: skip emit\_text\_change\_if\_needed when filter(new\_node) is not Include.

### spinbox-v01 {#spinbox-v01}

A value clamped as focus leaves is cut at once: type 500 and Tab, and the reader never learns the box holds 96

- **Example:** spin-box
- **Scenario:** verify-spinbox-blur-clamp
- **Act:** Font size (max 96): Ctrl+A, type 500, Tab. Commit on blur clamps it to 96 and focus moves to Gain.
- **The reader should get:** The reader hears that Font size became 96 (the entry was clamped), then Gain.
- **The reader gets:** Orca starts '96' and a stop cuts it 0-153 ms in, then 'Gain 0.0 spin button.' follows. The clamp's value change reaches the bus 0.1-1 ms before the focus change, in the same tree update. Orca's locus is still Font size, so it speaks the value and then stops to present the new focus. A reader who typed 500 and moved on believes 500 went in. The same applies to any typed value that blur changes (clamp, or the revert of spinbox-07).
- **Platform:** Linux AT-SPI/Orca 46.1, measured. The value-before-focus order comes from accesskit\_consumer, which every adapter shares; Windows and macOS were not measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open. In the announce-focus fix topic, not fixed there: Not a live region: a value change on the node focus is leaving, which Orca speaks and then cuts. It is now a small widget follow-up: a ctx.announce of the kept value in the blur commit (spin\_box.rs:1372-1376) would be held past the focus move by this fix.
- **Where:** crates/teksilo-widgets/src/spin\_box.rs:1372-1376
- **Evidence:**
  - `verify-spinbox-blur-clamp-20260925-133445-3487452, act 'select all, type 500, Tab away (clamps to 96 on blur) (1)': +340.0 ms object:property-change:accessible-value [spin button] 'Font size' / +340.1 ms object:state-changed:focused 1 [spin button] 'Gain' / +458.6 ms ORCA SAYS (CUT): '96' / +508.5 ms ORCA SAYS: 'Gain 0.0 spin button.' / FAIL Orca says '96'`
  - `orca-debug.out: 13:34:57.780758 - NULL SPEECH: speak '96' interrupt=True / 13:34:57.830496 - NULL SPEECH: stop / 13:34:57.830555 - SPEECH OUTPUT: 'Gain 0.0 spin button.' (also 13:35:07.071871 speak '96' / 13:35:07.217266 stop)`
  - `cut offsets over the 9 acts: 0, 145, 72 / 0, 153, 109 / 0, 66, 152 ms`
  - `source: crates/teksilo-widgets/src/spin_box.rs:1372-1376 (on_blur_fn -> commit) and 1017-1044 (set_committed sets the value inside the blur, which lands in the focus update)`
- **Reproduced:** 9 of 9 acts in 3 runs (3 per run).
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** When a blur commit changes what the user typed (clamp or revert), say the kept value through ctx.announce in a later update than the focus change (for example 'Font size 96'). An announcement in the same update would be cut the same way. The K2 announcer fix makes that path reliable. Enter's clamp is already heard, because focus does not move.

### spinbox-v02 {#spinbox-v02}

Orca says 'Text unselected.' on the first character typed into any spin box

- **Example:** spin-box
- **Scenario:** spinbox-typing / spinbox-locale / verify-spinbox-blur-clamp
- **Act:** Focus a spin box (the field selects all), then type a digit
- **The reader should get:** Nothing but the typed character's echo (and the value once committed)
- **The reader gets:** 'Text unselected.' before the committed value, for example before '20'. Focus publishes the whole-text selection. Orca 46 never presents text-changed events in a spin button (script\_utilities.py:3696-3702), so it never refreshes that cached selection. When typing replaces it, text-selection-changed makes it say SELECTION\_REMOVED (script\_utilities.py:4001-4003). This is Orca behaviour for any spin button. Teksilo publishes the selection correctly here.
- **Platform:** Linux Orca 46.1, measured
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** none (Orca)
- **Evidence:**
  - `spinbox-typing-20260925-133305-3367096, act 'type 20 then Enter': +15.2 ms object:text-selection-changed [spin button] 'Font size' / +46.2 ms ORCA SAYS: 'Text unselected.' / +245.5 ms ORCA SAYS: '20' (same in -133404: +28.1 ms 'Text unselected.')`
  - `spinbox-locale-20260925-133053-3240601, act 'Frequency: select all, type 12,5': +251.5 ms ORCA SAYS: 'Text unselected.'`
  - `orca script_utilities.py:3696-3702 (is_spin_button -> 'Event is not being presented due to role'), 4001-4003`
- **Reproduced:** Every first keystroke over a focus selection in my runs: typing x2, locale x2, blur-clamp first iterations x3, silent-clamp Gain act.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Nothing clean in Teksilo; an Orca report is the route. I did not measure whether a GTK spin button gets the same, which it should by Orca's code.
