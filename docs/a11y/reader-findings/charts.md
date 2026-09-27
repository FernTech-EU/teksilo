<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Generated from the sweep of 25 and 26 September 2026 (on 261a218f) and its re-measure of 27 September 2026 (on c198e4d1). See ../reader-findings.md. -->

# Charts

Examples: `chart-demo`.
16 findings: 6 high, 5 medium, 5 low.
Swept on `261a218f` on 25 and 26 September 2026, measured again on
`c198e4d1` on 27 September 2026.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [charts-01](#charts-01) | chart-demo | Selecting a datum is never exposed or spoken (bar, line, pie; keyboard and AT click) | high | Linux | open |
| [charts-02](#charts-02) | chart-demo | Interactive legend rows offer no action a screen reader can activate | high | Linux | open |
| [charts-03](#charts-03) | chart-demo | Resting on a live chart floods speech, and the focused datum silently becomes another sample every tick | high | Linux | open |
| [charts-04](#charts-04) | chart-demo | The first move into a chart is spoken twice (announcer + focus); by source, every move on Windows/macOS | medium | Linux | open |
| [charts-05](#charts-05) | chart-demo | Live charts keep ~150 AT-SPI events a second on the bus for the life of the app, so Orca is never idle | medium | Linux | open |
| [charts-06](#charts-06) | chart-demo | Datum values are spoken as raw floats, ignoring the axis formatter the tooltip uses | medium | Linux | open |
| [charts-07](#charts-07) | chart-demo | What the numbers mean is missing: no axis titles or units, and no slice share | medium | Linux | open |
| [charts-08](#charts-08) | chart-demo | The ChartStyle and chart-kind toggles say the selected option is 'not selected' | high | Linux | fixed |
| [charts-09](#charts-09) | chart-demo | Chart strings are hardcoded English and not localizable | medium | all | open |
| [charts-10](#charts-10) | chart-demo | Pie slice names start with a stray ', ' when the series is unnamed | low | Linux | open |
| [charts-11](#charts-11) | chart-demo | The chart's name counts hidden series | low | Linux | open |
| [charts-12](#charts-12) | chart-demo | Every datum is announced as a 'panel' | low | Linux | open |
| [charts-13](#charts-13) | chart-demo | The example's segmented controls have no group name; 'Feed', 'Rollup bucket' and 'fn' are unassociated text | low | all | open (example) |
| [charts-M1](#charts-m1) | chart-demo | A chart or datum the reader has already visited goes silent after it leaves the tree and comes back (legend hide/show, ChartStyle toggle round trip) | high | Linux | fixed |
| [charts-M2](#charts-m2) | chart-demo | The chart-kind control ('Bars') goes silent once the reader has visited the live charts: the ScrollArea clip drops it from the tree and it comes back defunct | high | Linux | fixed |
| [charts-M3](#charts-m3) | chart-demo | The two live charts are named identically and carry none of their visible titles | low | all | open (example) |

### charts-01 {#charts-01}

Selecting a datum is never exposed or spoken (bar, line, pie; keyboard and AT click)

- **Example:** chart-demo
- **Scenario:** charts-bar-keys, charts-lines, charts-donut, charts-mark-click
- **Act:** Enter on a focused bar (charts-bar-keys), Space on a focused line point (charts-lines), Enter on a focused donut slice (charts-donut), AT-SPI click on the bar 'Cost, Q2: 51' (charts-mark-click)
- **The reader should get:** The datum becomes selected: its node carries a selected state and the reader hears it (e.g. 'selected'). In the donut, the reader learns the selected slice and its share, which the centre slot shows.
- **The reader got (`261a218f`):** Nothing. The mark node keeps states \['enabled','focused','sensitive','showing','visible'\] with no 'selected' state, and Orca says nothing. The only change on the bus is a plain label elsewhere changing its text ('Selected: Revenue · Q1 = 41', or the donut centre labels 'Total'-&gt;'Apps', '150'-&gt;'6%'). None of these is a live region, and focus is not on them. The selection is painted only (an accent outline).
- **Platform:** Linux AT-SPI/Orca measured. Windows/macOS: by source, the same (the mark node never carries is\_selected, so no SelectionItem state on UIA and no AXSelected on macOS).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Selecting a datum is still neither exposed nor spoken. After Enter on a bar or slice, Space on a line point, or an AT-SPI click on a bar, the mark has no selected state and no selected event is sent; for the selection, the only change on the bus is the readout label, or the donut's centre labels, changing their text. Orca says the key's echo or nothing, and when the chart already has focus an AT-SPI click also moves focus to the clicked mark, so Orca reads only its name: 'Cost, Q2: 51 panel.'
- **Measured again:** charts-bar-keys, charts-mark-click, charts-donut, charts-lines, verify-charts-quiet-bar, verify-charts-quiet-donut: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 charts-bar-keys 'Enter selects the reached bar': 'FAIL  the mark 'Revenue, Q1: 41' has state 'selected'' / '[panel] 'Revenue, Q1: 41' states=['enabled', 'focused', 'sensitive', 'showing', 'visible']'; 'FAIL  the reader hears that the datum is selected' / 'Orca said ['return']' (pass2 the same)`
  - `pass1 charts-mark-click 'AT-SPI click on the mark 'Cost, Q2: 51'': +37.1 ms object:text-changed:insert [label] 'Click a bar or point to select it.' text='Selected: Cost · Q2 = 51'; [panel] 'Cost, Q2: 51' states=['enabled', 'sensitive', 'showing', 'visible']; Orca said []. pass2 the same (+25.3 ms, same states, Orca said [])`
  - `pass1 charts-donut 'Enter selects the slice; the centre slot shows it': +76.5 ms object:text-changed:insert [label] 'Total' text='Apps', +80.9 ms object:text-changed:insert [label] '150' text='6%'; [panel] ', Apps: 9' states=['enabled', 'focused', 'sensitive', 'showing', 'visible']; Orca said [] (pass2 the same)`
  - `pass1 charts-lines 'Space selects the reached point': +19.5 ms object:text-changed:insert [label] 'Click a bar or point to select it.' text='Selected: Profit · Q4 = 18'; [panel] 'Profit, Q4: 18' states=['enabled', 'focused', 'sensitive', 'showing', 'visible']; Orca said [] (pass2 the same)`
  - `pass1 and pass2 verify-charts-quiet-bar 'Enter selects the reached bar': 'FAIL  a object:state-changed:selected event from [*] '*'' / 'no object:state-changed:selected event from [*] '*''; Orca said []`
  - `pass1 verify-charts-quiet-bar 'AT-SPI click on the mark 'Cost, Q2: 51'': +32.6 ms object:state-changed:focused 1 [panel] 'Cost, Q2: 51', +100.6 ms ORCA SAYS: 'Cost, Q2: 51 panel.'; [panel] 'Cost, Q2: 51' states=['enabled', 'focused', 'sensitive', 'showing', 'visible']; Orca said ['Cost, Q2: 51 panel.']. pass2 the same: +35.1 ms object:state-changed:focused 1 [panel] 'Cost, Q2: 51', +149.2 ms ORCA SAYS: 'Cost, Q2: 51 panel.'`
  - `pass1 and pass2 verify-charts-quiet-donut 'Enter selects the slice': [panel] ', Apps: 9' states=['enabled', 'focused', 'sensitive', 'showing', 'visible']; Orca said []`
  - `crates/teksilo-charts/src/hit.rs:705-724 emit_mark_node sets role, name, numeric value, actions and bounds, never set_selected; hit.rs:632-644 Enter/Space and hit.rs:758-761 the Click action only call select_point`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/hit.rs:705-724 (emit\_mark\_node never sets selected), hit.rs:632-644 (Enter/Space: select\_point only), hit.rs:758-761 (Click action: select\_point only), bar\_chart.rs:345-349 / line\_chart.rs:306-310 / pie\_chart.rs:327-331 (selection\_signal bound RepaintOnly only)
- **Evidence (`261a218f`):**
  - `charts-bar-keys-20260925-143311-618997/report.txt, 'Enter selects the reached bar': '+18.3 ms object:text-changed:insert [label] 'Click a bar or point to select it.' text='Selected: Revenue · Q1 = 41'' then 'FAIL  the mark 'Revenue, Q1: 41' has state 'selected'' / '[panel] 'Revenue, Q1: 41' states=['enabled', 'focused', 'sensitive', 'showing', 'visible']' / 'FAIL  the reader hears that the datum is selected' 'Orca said []'`
  - `charts-mark-click-20260925-140748-83118/report.txt: '+28.0 ms == harness:action click [panel] 'Cost, Q2: 51'', '+39.5 ms object:text-changed:insert [label] 'Click a bar or point to select it.' text='Selected: Cost · Q2 = 51'', 'FAIL  the mark 'Cost, Q2: 51' has state 'selected'' '[panel] 'Cost, Q2: 51' states=['enabled', 'sensitive', 'showing', 'visible']', 'Orca said []'`
  - `charts-donut-20260925-142921-523119/report.txt, 'Enter selects the slice': '+14.5 ms object:text-changed:insert [label] 'Total' text='Apps'', '+14.9 ms object:text-changed:insert [label] '150' text='6%'', 'FAIL  the mark ', Apps: 9' has state 'selected'', 'Orca said []'`
  - `Same result in charts-bar-keys runs 140305 and 142500, charts-lines 141100 (Space), charts-donut 141443: 6 of 6 keyboard selections, 1 of 1 AT click`
  - `crates/teksilo-charts/src/hit.rs:704-725 emit_mark_node sets role, name, numeric value, actions and bounds, never set_selected`
  - `crates/teksilo-charts/src/bar_chart.rs:345-349 selection_signal bound at BindingLevel::RepaintOnly only (no AccessibilityOnly), so even a selected state would not refresh the AT tree; same pattern in line_chart.rs / pie_chart.rs`
  - `crates/teksilo-charts/src/hit.rs:634-645 drive_readout_keys: Enter/Space call selection.select_point and nothing else (no announce, no AT state)`
  - `verify-charts-quiet-bar-20260925-144821-978687/report.txt 'Enter selects the reached bar': '+18.7 ms object:text-changed:insert [label] ... text='Selected: Profit · Q1 = 27'', 'FAIL the mark 'Profit, Q1: 27' has state 'selected'' / 'selectable' '[panel] 'Profit, Q1: 27' states=['enabled', 'focused', 'sensitive', 'showing', 'visible']', 'FAIL a object:state-changed:selected event from [*] '*'', 'Orca said []'`
  - `same run 'Space on the next bar selects it': FAIL selected; Orca said ['Revenue, Q2: 58 panel.'] (the focus move only)`
  - `verify-charts-quiet-donut-20260925-144824-979341 'Enter selects the slice': '+16.0 ms object:text-changed:insert [label] 'Total' text='Apps'', '+16.8 ms ... '150' text='6%'', FAIL selected, 'Orca said []'`
  - `/usr/lib/python3/dist-packages/orca/scripts/default.py:1518-1526: announceState only if keyString == 'space' (or Down/Up on a table cell)`
- **Reproduced:** deterministic: 7 of 7 selections across 6 runs
- **Verification:** corrected by the verifier. Reproduced: deterministic: 8 of 8 selections across 6 of my runs (bar Enter in charts-bar-keys 144244 and 144803, quiet-bar Enter and Space, AT-SPI click in quiet-bar and charts-mark-click, donut Enter in charts-donut 144248 and quiet-donut) Reproduced with an idle Orca (feed paused), so the backlog plays no part. Enter on 'Profit, Q1: 27' changes only the plain readout label; the mark keeps states \[enabled, focused, sensitive, showing, visible\], with no 'selected' or 'selectable' state. The bus carries no object:state-changed:selected and Orca says nothing. Space on the next bar and an AT-SPI click on 'Cost, Q2: 51' give the same result, and so do the donut's centre labels. Every source line cited checks out. The fix idea needs correcting. Setting set\_selected is necessary but not enough on Orca: default.py:1487-1526 onSelectedChanged speaks a selected-state change only when Orca's own last key was 'space' (lastKeyAndModifiers), so an Enter commit would still be silent, and so would an AT click. The chart should also announce the commit, for example 'Selected: Revenue, Q1: 41', through ctx.announce (which carries K2 on main). The severity stays high: the reader gets no feedback when committing a selection.
- **Fix idea:** In emit\_mark\_node, set\_selected(selection.is\_selected(sid, idx)) on every mark when the chart has a selection (which also makes AT-SPI 'selectable'), and bind selection\_signal at AccessibilityOnly as well as RepaintOnly. On a keyboard/AT commit, AT-SPI then emits state-changed:selected on the focused node, which Orca speaks.

### charts-02 {#charts-02}

Interactive legend rows offer no action a screen reader can activate

- **Example:** chart-demo
- **Scenario:** charts-bar-legend
- **Act:** AT-SPI 'click' on the legend check box 'Cost' (what a screen reader's own activation does)
- **The reader should get:** The legend row, a checkable check box, can be toggled by the screen reader's activation, as a click or Space toggles it.
- **The reader got (`261a218f`):** The row exposes no AT-SPI action at all ('offers no action on AT-SPI'), so the series cannot be toggled that way. Only the Space key (KeyDown arms, KeyUp fires) works, and only while keyboard focus is on the row.
- **Platform:** Linux AT-SPI measured. Windows, by source: the Toggle pattern is offered (toggled is set), and Toggle() sends Action::Click (accesskit\_windows node.rs:1336-1338, 952-953), which the row neither advertises nor handles, so NVDA's toggle does nothing. macOS, by source: accessibilityPerformPress is offered only on clickable nodes (accesskit\_macos node.rs:1240-1241), so VoiceOver's VO+Space finds no press.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The legend rows still offer no AT-SPI action. A screen reader's own activation cannot toggle a series; only Space works, while keyboard focus is on the row. By source, the Windows Toggle pattern still sends a Click that the row neither advertises nor handles, and macOS still offers no press.
- **Measured again:** charts-bar-legend, verify-charts-quiet-bar: 2 of 2 runs each; Windows and macOS read from source
- **Evidence (`c198e4d1`):**
  - `pass1 charts-bar-legend 'AT-SPI click on the Cost legend row': FAIL the row offers an action a screen reader can activate: run.action failed: {'path': '/org/a11y/atspi/accessible/0/79228163934663631269179424768', 'name': 'Cost', 'role': 'check box'} offers no action on AT-SPI; no object:state-changed:checked event from [check box] 'Cost'`
  - `pass2 charts-bar-legend 'AT-SPI click on the Cost legend row': same failure`
  - `pass1 and pass2 verify-charts-quiet-bar 'AT-SPI action 0 on the Cost legend row (whatever it offers)': offers no action on AT-SPI`
  - `crates/teksilo-charts/src/legend.rs:227-236 LegendRow::accessibility adds only Action::Focus; legend.rs:99-133 on_tap and on_key(Space) only, no on_access_action`
  - `crates/teksilo-core/src/widget_tree/pointer_router.rs:2412-2491 an AT action fires only installed on_access_action(_request) slots and access overrides, with no fallback to on_tap`
  - `accesskit_windows-0.35.1/src/node.rs:1336 Toggle() and :953 do_action(Action::Click); :576 is_toggle_pattern_supported; accesskit_macos-0.27.1/src/node.rs:1240-1241 accessibilityPerformPress only when is_clickable`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/legend.rs:227-236 (LegendRow::accessibility: CheckBox, name, toggled, only Action::Focus) and legend.rs:99-133 (on\_tap + on\_key(Space) only, no on\_access\_action); crates/teksilo-core/src/widget\_tree/pointer\_router.rs:2412-2491 (AT action dispatch)
- **Evidence (`261a218f`):**
  - `charts-bar-legend-20260925-141736-261283/report.txt 'AT-SPI click on the Cost legend row': 'FAIL  the row offers an action a screen reader can activate' 'run.action failed: {'path': '/org/a11y/atspi/accessible/0/79228163934663631269179424768', 'name': 'Cost', 'role': 'check box'} offers no action on AT-SPI' / 'FAIL  a object:state-changed:checked event from [check box] 'Cost''`
  - `Same in charts-bar-legend-20260925-140743-80432 (2 of 2 runs)`
  - `tree-chart-demo-20260925-135332-4041370/run.json: check box 'Revenue' / 'Cost' / 'Profit' actions= None, while every push button and mark has [{'name': 'click'}]`
  - `crates/teksilo-charts/src/legend.rs:227-236 LegendRow::accessibility sets Role::CheckBox, name, toggled, and only add_action(Action::Focus); no Action::Click`
  - `crates/teksilo-charts/src/legend.rs:93-133 handlers are on_tap + on_key(Space) only; no on_access_action, so an AT Click routed by crates/teksilo-core/src/widget_tree/pointer_router.rs:2369-2450 finds no slot and is ignored`
  - `verify-charts-quiet-bar-20260925-144821-978687/report.txt 'AT-SPI click on the Cost legend row': 'run.action failed: {... 'name': 'Cost', 'role': 'check box'} offers no action on AT-SPI', 'no object:state-changed:checked event from [check box] 'Cost''; 'AT-SPI action 0 on the Cost legend row': same failure`
  - `charts-bar-legend-20260925-144250-825193 and -144800-825193: 'FAIL the row offers an action a screen reader can activate'`
- **Reproduced:** deterministic: 2 of 2 runs
- **Verification:** confirmed. Reproduced: deterministic: 3 of 3 of my runs (charts-bar-legend 144250, charts-bar-legend 144800, verify-charts-quiet-bar 144821). AT-SPI action 'click' and action index 0 both fail: 'offers no action on AT-SPI'
- **Fix idea:** In LegendRow: add\_action(Action::Click) and handle it with on\_access\_action (the same toggle as on\_tap).

### charts-03 {#charts-03}

Resting on a live chart floods speech, and the focused datum silently becomes another sample every tick

- **Example:** chart-demo
- **Scenario:** charts-live
- **Act:** Focus the raw live LineChart and wait; press End to reach the newest sample and wait (feed Running)
- **The reader should get:** The reader keeps their place: the datum under focus stays the datum it was, and nothing is re-read every 600 ms unless the reader asks. The chart's own name does not change with every sample.
- **The reader got (`261a218f`):** On a point: every tick the focused node's name and value change to the next sample ('Windowed, 102: 14.740126' -&gt; 'Windowed, 103: 18.282179' -&gt; ...). Orca speaks the name change, then re-presents the object on the value change: two utterances every 600 ms, indefinitely (164 utterances in the ~49 s the harness waited for Orca to go idle, which it never did). The datum the reader is on is no longer the one they reached. On the chart itself: its name encodes the point count, so Orca reads 'Line chart: 1 series, 19 points', '… 20 points', … on every tick while the window fills. The rollup chart's name grows without bound ('… 117 points'). With the feed paused, the same acts are silent.
- **Platform:** Linux AT-SPI/Orca measured. Windows/macOS: by source, the adapters raise name/value changes on the focused element too (not measured).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Resting on a point of the running live chart, the reader is read a new sample every 600 ms, twice each time: 'Windowed, 88: 25.194046' then 'Windowed, 88: 25.194046 panel.', and so on, with no key pressed. The focused node's name and value change to the next sample each tick, so the datum the reader reached is gone. The rollup chart's own name still grows every bucket and Orca reads it ('Line chart: 1 series, 36 points', '37 points', ...). Paused, the same acts are silent.
- **Measured again:** charts-live and verify-charts-live-full, 2 of 2 runs each; tabwalk-chart-demo 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 charts-live 'rest on the point for 3 s while samples arrive': 16 utterances: ['Windowed, 88: 25.194046', 'Windowed, 88: 25.194046 panel.', 'Windowed, 89: 41.50537', 'Windowed, 89: 41.50537 panel.', ...]`
  - `pass2 charts-live 'rest on the point for 3 s while samples arrive': 18 utterances: ['Windowed, 85: 21.760786', 'Windowed, 85: 21.760786 panel.', 'Windowed, 86: 15.691626', ...]`
  - `pass1 verify-charts-live-full 'rest on the oldest point for 3 s': focused path /org/a11y/atspi/accessible/0/322556545301543237323430617085437280256: 5 name changes, 5 value changes in 3.1 s; 12 utterances inside the act's own 3.1 s window`
  - `pass2 verify-charts-live-full 'rest on the rollup chart for 6 s': +436.8 ms object:property-change:accessible-name [document frame] 'Line chart: 1 series, 42 points', +2835.8 ms ... '43 points', +5245.0 ms ... '44 points'; Orca '17:06:49.440343 'Line chart: 1 series, 42 points'', '17:06:51.734716 'Line chart: 1 series, 43 points''`
  - `pass1 tabwalk-chart-demo 'Tab 16': +117.0 ms ORCA SAYS: 'Line chart: 1 series, 50 points document frame.', +918.0 ms ORCA SAYS: 'Line chart: 1 series, 51 points', +3331.5 ms ORCA SAYS: 'Line chart: 1 series, 52 points'`
  - `pass1 charts-live 'rest on the point of the paused chart for 3 s': pass nothing is said while the feed is paused`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/hit.rs:674-681 (mark\_element\_id by (SeriesId, point\_idx)), hit.rs:715 (numeric\_value on every mark), line\_chart.rs:766-777 (point count in the name)
- **Evidence (`261a218f`):**
  - `charts-live-20260925-143211-601779/report.txt 'End: the newest sample': '+6.3 ms object:state-changed:focused 1 [panel] 'Windowed, 102: 14.740126'', '+732.1 ms object:property-change:accessible-name [panel] 'Windowed, 103: 18.282179' text='Windowed, 103: 18.282179'', '+732.1 ms object:property-change:accessible-value [panel] 'Windowed, 103: 18.282179'', '+872.7 ms ORCA SAYS: 'Windowed, 103: 18.282179'', '+880.8 ms ORCA SAYS: 'Windowed, 103: 18.282179 panel.''`
  - `charts-live-20260925-143211-601779/orca-debug.out: '14:33:15.922808 - OBJECT EVENT: object:property-change:accessible-name for [panel: 'Windowed, 103: 18.282179'] in [application: 'chart-demo'] (0, 0, Windowed, 103: 18.282179)', '14:33:15.929183 - SPEECH OUTPUT: 'Windowed, 103: 18.282179'', '14:33:15.929413 - OBJECT EVENT: object:property-change:accessible-value for [panel: 'Windowed, 103: 18.282179']', '14:33:15.937248 - SPEECH OUTPUT: 'Windowed, 103: 18.282179 panel.''`
  - `'rest on the point for 3 s while samples arrive': 164 utterances in each of charts-live 142453, 142809, 143211 (3 of 3 runs); 'FAIL  no object:property-change:accessible-name event from [panel] 'Windowed, ''`
  - `charts-live-20260925-141558-243739: the name changes all arrive on the one focused path /org/a11y/atspi/accessible/0/201711337751106591172059418403260071936 (first 'Windowed, 17: 32.190796', then 'Windowed, 18: 43.487053', 'Windowed, 19: 60.05414', …), with no focus event`
  - `charts-live-20260925-142453-428769/report.txt 'rest on the chart for 3 s': '+597.4 ms object:property-change:accessible-name [document frame] 'Line chart: 1 series, 19 points'', '+613.4 ms ORCA SAYS: 'Line chart: 1 series, 19 points'', '+1233.2 ms ORCA SAYS: 'Line chart: 1 series, 20 points'' (6 to 9 such utterances per act, 3 of 3 runs)`
  - `Paused control: 'rest on the point of the paused chart for 3 s': 0 utterances, pass (3 of 3)`
  - `crates/teksilo-charts/src/hit.rs:672-681 mark_element_id hashes (SeriesId, point_idx): a mark's identity is its index, so when a ChartWindow slides, every index carries a new sample`
  - `crates/teksilo-charts/src/line_chart.rs:766-777 name = format!("Line chart: {} series, {} points", …)`
  - `Orca default.py:1430 onNameChanged and :1838 onValueChanged present the locus of focus on every change`
  - `verify-charts-live-full-20260925-144827-982092/report.txt 'rest on the oldest point for 3 s': 'focused path /org/a11y/atspi/accessible/0/322556545301543237323430617085437280256: 5 name changes, 5 value changes in 3.1 s', '10 utterances inside the act's own 3.1 s window (14:51:20.918943..14:51:24.019142); 164 in Orca's whole window', '14:51:21.377505 'Windowed, 261: 36.257782'', '14:51:21.394622 'Windowed, 261: 36.257782 panel.'', '14:51:21.785590 'Windowed, 262: 24.225292''`
  - `same run 'rest on the full raw chart for 3 s': 'pass no object:property-change:accessible-name event from [document frame] 'Line chart: 1 series, 24''`
  - `same run 'rest on the rollup chart for 6 s': '+2112.0 ms object:property-change:accessible-name [document frame] 'Line chart: 1 series, 114 points'', '+4512.5 ms ... '115 points'', Orca '14:53:02.716223 'Line chart: 1 series, 114 points'', '14:53:05.144292 'Line chart: 1 series, 115 points''`
  - `charts-live-20260925-144246-823973: 'rest on the point': 10 utterances in the act's 3.1 s window ('Windowed, 181: 73.006996', 'Windowed, 181: 73.006996 panel.', ...); charts-live-20260925-144645: 12; charts-live-20260925-145048: 10; paused control 0 in all three`
- **Reproduced:** 3 of 3 runs (charts-live 142453, 142809, 143211); also the first, pre-fix version of the scenario (141558)
- **Verification:** corrected by the verifier. Reproduced: resting on a point while the feed runs: 5 of 5 runs (charts-live 144246, 144645, 145048 at End; verify-charts-live-full at Home). The chart's own name changing: only while the window fills (3 of 3), stable once full (1 of 1). Rollup chart name changing every bucket: 1 of 1 (verify-charts-live-full) The defect is real, and the Home (oldest) point behaves the same as End. The utterance figure needs correcting: '164 utterances' counts the harness's 45 s Orca catch-up window. Inside the act's own 3.1 s window there are 10 to 12 utterances: two per 600 ms tick, the name change (onNameChanged -&gt; presentMessage) and then the value change (onValueChanged -&gt; generateSpeech). The focused path gets 5 name changes and 5 value changes in 3.1 s with no focus event. The raw chart's own name ('Line chart: 1 series, N points') changes only while the 24-sample window fills, in the first ~14 s after launch. Once full it is stable: 'no accessible-name event from \[document frame\] Line chart: 1 series, 24' passed while resting on the full chart. The sweep's 'while the window fills' is right, and the problem is transient there. The rollup chart, which the sweep did not measure, grows without bound: resting on it, Orca reads 'Line chart: 1 series, 114 points' then '... 115 points' every 2.4 s (every bucket), for the life of the app. The paused control is silent (3 of 3). The layer is framework: the example's replace\_series\_data bridge is the documented way to feed a ChartWindow, and identity by index renames every mark whatever the feed does.
- **Fix idea:** Key a mark's AT identity by its datum (the category, or a per-point id the model hands out) rather than by index, so a sliding window adds one node and removes one per tick, and the focused node keeps its sample (or disappears, moving focus deliberately). Keep the point count out of the chart's name, or put it in the description. Optionally let an app mark a streaming chart so the AT tree is refreshed at a bounded rate.

### charts-04 {#charts-04}

The first move into a chart is spoken twice (announcer + focus); by source, every move on Windows/macOS

- **Example:** chart-demo
- **Scenario:** charts-bar-keys, charts-donut, charts-style-toggle, charts-live
- **Act:** Right (or End) on a focused chart, moving from the chart node to its first mark
- **The reader should get:** The reader hears the datum once.
- **The reader got (`261a218f`):** Orca says 'Revenue, Q1: 41' and then 'Revenue, Q1: 41 panel.', neither cut. The chart publishes the datum both as its active\_descendant (a focus move) and through ctx.announce (the framework announcer). Orca does not stop speech for this focus change: 'Not interrupting for locusOfFocus change: old locusOfFocus is ancestor with name of new locusOfFocus'. Later moves (mark to mark) are heard once on main only because K2 drops the announcer's later messages as defunct. Once K2 is fixed, every entry into a chart will be doubled on Orca. On later moves Orca will interrupt, cutting the announcement just after it starts (source reading of Orca's interrupt rule, not measured).
- **Platform:** Linux AT-SPI/Orca measured. Windows (source): the focus change and the announcer's LiveRegionChanged are both raised, so NVDA is expected to speak both on every move. macOS (source): a focus change plus an announcement request carrying the same text. Not measured on either.
- **Severity:** medium; **layer:** framework
- **Status:** Open. It has changed since the sweep: see **Now**.
- **Now (`c198e4d1`):** Every move between datums is now heard twice, not only the first move into a chart: first 'Revenue, Q1: 41 panel.' from the focus change, then 'Revenue, Q1: 41' from the announcement, and neither reading is cut. A key at a clamp (Left on the first bar) repeats the datum with no focus change.
- **Measured again:** charts-bar-keys, charts-lines, charts-donut, charts-style-toggle, charts-bar-legend, charts-live, verify-charts-quiet-bar, verify-charts-quiet-donut, verify-charts-reshown-visited: 2 of 2 runs each, every datum move
- **Evidence (`c198e4d1`):**
  - `pass1 charts-bar-keys 'Right: first bar': +12.4 ms object:state-changed:focused 1 [panel] 'Revenue, Q1: 41', +29.6 ms object:announcement [status bar] 'Revenue, Q1: 41' text='Revenue, Q1: 41', +42.9 ms ORCA SAYS: 'Revenue, Q1: 41 panel.', +54.4 ms ORCA SAYS: 'Revenue, Q1: 41'`
  - `pass1 charts-bar-keys 'Right: second bar': +20.5 ms object:state-changed:focused 1 [panel] 'Cost, Q1: 34', +40.2 ms object:announcement [status bar] 'Cost, Q1: 34' text='Cost, Q1: 34', +78.4 ms ORCA SAYS: 'Cost, Q1: 34 panel.', +103.3 ms ORCA SAYS: 'Cost, Q1: 34'`
  - `pass1 charts-bar-keys orca-debug.out: '15:57:25.092718 - NULL SPEECH: stop', '15:57:25.092866 - NULL SPEECH: speak 'Cost, Q1: 34 panel.' interrupt=False', '15:57:25.117712 - NULL SPEECH: speak 'Cost, Q1: 34' interrupt=True' (no stop between the two readings)`
  - `pass1 charts-bar-keys 'Left at the first bar (clamps)': +50.9 ms object:announcement [status bar] 'Revenue, Q1: 41' text='Revenue, Q1: 41', +332.0 ms ORCA SAYS: 'Revenue, Q1: 41', 'pass  no object:state-changed:focused event from [*] '*''; pass2 the same: +15.0 ms object:announcement [status bar] 'Revenue, Q1: 41' text='Revenue, Q1: 41', +21.1 ms ORCA SAYS: 'Revenue, Q1: 41'`
  - `pass2 charts-bar-keys: 'FAIL  Orca says ... once' on Right x3, End and Home, each with 'Orca said: '... panel.'' then 'Orca said: '...''`
  - `pass1 verify-charts-quiet-bar 'Right: second bar (idle Orca)': Orca said at 16:39:04.410569: 'Cost, Q1: 34 panel.', Orca said at 16:39:04.440424: 'Cost, Q1: 34'; pass2: Orca said at 17:06:20.166054: 'Cost, Q1: 34 panel.', Orca said at 17:06:20.195199: 'Cost, Q1: 34'`
  - `pass2 verify-charts-quiet-donut 'Right: second slice (idle Orca)': Orca said at 17:06:42.043831: ', Apps: 9 panel.', Orca said at 17:06:42.073911: ', Apps: 9'`
  - `same order, focus reading then announcement, on every datum move of charts-lines, charts-donut, charts-style-toggle, charts-bar-legend, verify-charts-reshown-visited and the End act of charts-live, both passes; no datum reading is marked CUT except in 'Space on the next bar selects it', where the Space pressed straight after Right stops both readings, as the next key does for any typist`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/hit.rs:628 (ctx.announce on every arrow/Home/End) alongside bar\_chart.rs:814 / line\_chart.rs:801 / pie\_chart.rs:738 set\_active\_descendant
- **Evidence (`261a218f`):**
  - `charts-bar-keys-20260925-143311-618997/report.txt 'Right: first bar': '+8.3 ms object:announcement [status bar] 'Revenue, Q1: 41' text='Revenue, Q1: 41'', '+10.0 ms object:state-changed:focused 1 [panel] 'Revenue, Q1: 41'', '+20.1 ms ORCA SAYS: 'Revenue, Q1: 41'', '+94.8 ms ORCA SAYS: 'Revenue, Q1: 41 panel.'', 'FAIL  Orca says 'Revenue, Q1: 41' once'`
  - `charts-bar-keys-20260925-143311-618997/orca-debug.out: '14:33:25.015402 - NULL SPEECH: speak 'Revenue, Q1: 41' interrupt=True', '14:33:25.089869 - SCRIPT UTILITIES: Not interrupting for locusOfFocus change: old locusOfFocus is ancestor with name of new locusOfFocus', '14:33:25.090094 - NULL SPEECH: speak 'Revenue, Q1: 41 panel.' interrupt=False'`
  - `charts-donut-20260925-141443-223023: 'ORCA SAYS: ', Storage: 18'' then 'ORCA SAYS: ', Storage: 18 panel.''`
  - `Tally over every run's first announcement: doubled in 9 of 12 (bar-keys x3, donut x2, style-toggle, live x3). In the other 3 (lines 141100, bar-legend 141736, live 142809), Orca dropped the announcement as defunct (K2), e.g. charts-lines-20260925-141100-146529/orca-debug.out '14:12:07.850222 - EVENT MANAGER: Dequeued object:announcement for [status bar: 'Revenue, Q1: 41']' then '14:12:07.850273 - EVENT MANAGER: Ignoring defunct object: [status bar: 'Revenue, Q1: 41']'`
  - `crates/teksilo-charts/src/hit.rs:620-630 drive_readout_keys: focus.set(Some(key)) and ctx.announce(mark_description(m)) on every arrow/Home/End`
  - `crates/teksilo-charts/src/bar_chart.rs:814, line_chart.rs:801, pie_chart.rs:738 builder.set_active_descendant(node) for the same datum`
  - `Orca script_utilities.py:4148-4152 (no interrupt when the old focus is a named ancestor of the new one)`
  - `K2 coverage: this announcement (hit.rs:628, all three chart kinds' keyboard traversal) and the touch pin announcement (hit.rs:512) are the chart-demo messages that go through ctx.announce`
  - `verify-charts-quiet-bar-20260925-144821-978687/orca-debug.out: '14:48:52.410739 - NULL SPEECH: speak 'Revenue, Q1: 41' interrupt=True', '14:48:52.494157 - SCRIPT UTILITIES: Not interrupting for locusOfFocus change: old locusOfFocus is ancestor with name of new locusOfFocus', '14:48:52.494326 - NULL SPEECH: speak 'Revenue, Q1: 41 panel.' interrupt=False'`
  - `verify-charts-quiet-donut-20260925-144824-979341: '14:49:00.575303 NULL SPEECH: speak ', Storage: 18' interrupt=True', '14:49:00.659563 ... ', Storage: 18 panel.' interrupt=False'`
  - `charts-donut-20260925-144248-824566/orca-debug.out: '14:45:35.768730 Queueing object:announcement for [status bar: ', Storage: 18']', '14:45:35.824620 Dequeued ...', '14:45:35.824720 Ignoring defunct object: [status bar: ', Storage: 18']' (bus: defunct at +49.9 ms)`
  - `charts-bar-keys-20260925-144244-821412 'Left at the first bar (clamps)': '+8.8 ms object:announcement [status bar] 'Revenue, Q1: 41'', '14:46:52.453143 EVENT MANAGER: Ignoring defunct object: [status bar: 'Revenue, Q1: 41']'`
- **Reproduced:** 9 of 12 runs doubled on the first move; the other 3 lost the announcement to K2
- **Verification:** confirmed. Reproduced: first entry doubled in 10 of 11 of my runs, including 5 of 5 with an idle Orca (feed paused: quiet-bar, quiet-donut, reshown-visited x3). The 11th (charts-donut 144248) lost the announcement to K2: the node was defunct 50 ms after the message, and Orca, lagging behind the live stream, dequeued it at +56 ms
- **Fix idea:** Drop the ctx.announce on keyboard traversal. The active\_descendant move already makes every adapter present the datum. Keep the announcement only on the touch-pin path (hit.rs:512), where focus does not move.

### charts-05 {#charts-05}

Live charts keep ~150 AT-SPI events a second on the bus for the life of the app, so Orca is never idle

- **Example:** chart-demo
- **Scenario:** charts-lines, reader.py tree
- **Act:** Any: from launch, whatever the reader is doing elsewhere in the window
- **The reader should get:** A chart the reader is not on costs the screen reader little; an update adds or changes the few nodes that changed.
- **The reader got (`261a218f`):** Every 600 ms tick renames, re-values and re-bounds every mark of the sliding window (and the growing rollup), plus the status label. Over a 295 s run: 44,331 events (150/s), 44,230 from the live charts. Orca dequeued 13,176 events, 11,378 of them the marks' value changes. It never went idle (the harness's 45 s catch-up ceiling was hit on nearly every act of every chart-demo run). The backlog delays everything else. In charts-lines, the reader's first chart announcement was queued behind 31 of these events and handled 32 ms late, after the announcer node was gone, so it was dropped as defunct.
- **Platform:** Linux AT-SPI/Orca measured; Windows/macOS by source raise the same property changes.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** While the feed runs, the live charts keep about 123 AT-SPI events a second on the bus in a two-minute run, and the rate grows with the rollup. Nearly all come from the live charts and their status label. Orca handles thousands of the marks' value changes, so it is never idle, and a reader's own events wait behind them.
- **Measured again:** charts-lines 2 of 2 runs; verify-charts-live-full 1 of 1 counted; every running-feed run shows the stream
- **Evidence (`c198e4d1`):**
  - `pass1 charts-lines events.jsonl: 14925 events over 121.0 s = 123.4/s; 14820 from the live charts and status label (5749 object:bounds-changed, 4442 accessible-name, 4176 accessible-value)`
  - `pass2 charts-lines events.jsonl: 15050 events over 119.7 s = 125.7/s; 14945 from the live charts and status label`
  - `pass1 charts-lines orca-debug.out: 4126 'Dequeued object:property-change:accessible-value' of 4903 'Dequeued' lines`
  - `pass1 charts-lines 'Right: first point': +83.0 ms object:state-changed:focused 1 [panel] 'Revenue, Q1: 41', +1347.4 ms ORCA SAYS: 'Revenue, Q1: 41 panel.'`
  - `pass1 verify-charts-live-full events.jsonl: 11644 events over 94.7 s = 123.0/s`
  - `pass1 and pass2 verify-charts-quiet-bar 'quiet for 2 s after pausing': pass no object:property-change:accessible-value event from [panel] '*'`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/hit.rs:674-681 (identity by index) and hit.rs:715 (set\_numeric\_value, which makes every mark an AT-SPI Value that re-emits accessible-value on every tick)
- **Evidence (`261a218f`):**
  - `charts-lines-20260925-141100-146529/events.jsonl measured: 'total 44331 events over 295s = 150.1/s; from live charts/status label: 44230 = 149.8/s' (19523 object:bounds-changed, 12110 accessible-name, 11477 accessible-value)`
  - `charts-lines-20260925-141100-146529/orca-debug.out: 11378 'Dequeued object:property-change:accessible-value' of 13176 'Dequeued' lines`
  - `charts-lines-20260925-141100-146529/orca-debug.out: '14:12:07.818134 - EVENT MANAGER: Queueing object:announcement for [status bar: 'Revenue, Q1: 41']', then Orca processes 'object:property-change:accessible-value for [panel: 'Windowed, 99: 50.41903']', 'Windowed, 100: 32.71357' …, '14:12:07.850222 - EVENT MANAGER: Dequeued object:announcement …', '14:12:07.850273 - EVENT MANAGER: Ignoring defunct object: [status bar: 'Revenue, Q1: 41']'`
  - `tree-chart-demo-20260925-135332-4041370/report.txt launch: every ~600 ms 'object:property-change:accessible-name [document frame] 'Line chart: 1 series, N points'', 'object:children-changed:add', 'object:property-change:accessible-name [panel] 'Rollup, 0: 61.972805'', 'object:text-changed:delete/insert [label] 'N samples — window: …''`
  - `crates/teksilo-charts/src/hit.rs:672 identity by point index (same root cause as charts-03); crates/teksilo-charts/src/line_chart.rs binds structure_version at AccessibilityOnly, so every tick re-walks every mark`
  - `target/reader-sweep/charts/charts-lines-20260925-141100-146529/orca-debug.out 'Dequeued' by type: 11378 object:property-change:accessible-value, 629 accessible-name, 484 text-changed:insert, 484 text-changed:delete (13176 total); grep -c bounds-changed = 0`
  - `same events.jsonl, per 30 s bin: 82.8/s, 139.1, 140.6, 145.4, 154.8, 158.0, 163.8, 167.9, 176.6 events/s; by source: 'Windowed' marks 33,597, 'Rollup' marks 8,393`
  - `verify-charts-*-quiet: 'quiet for 2 s after pausing' passes (no name/value events), so the stream is entirely the live feed`
- **Reproduced:** every chart-demo run (16 runs) shows the stream; the dropped announcement 1 of 12 first-announcement acts attributable to the backlog (charts-lines)
- **Verification:** corrected by the verifier. Reproduced: stream present in every running-feed run (my 14 runs); rate recomputed from the sweep's charts-lines events.jsonl The numbers check out: 44,331 events in 295.3 s = 150.1/s. The rate grows over time, from 83/s in the first 30 s to 177/s after 4 minutes, because the rollup grows without bound. Two corrections. (1) Orca's load comes from the marks' VALUE changes. Orca dequeued 11,378 accessible-value events but only 629 of 12,111 accessible-name events: it discards name changes in a deluge and keeps value changes, and onValueChanged makes D-Bus queries for each one. bounds-changed (19,524) is not listened to at all (no 'bounds-changed' line in orca-debug.out). Identity by datum would remove almost all name and value churn. The bounds churn would stay, but Orca does not listen to it. (2) The dropped announcement cited as the backlog's consequence is K2's second failure mode (the announcer node is removed ~20-50 ms after its message), exposed by the lag, as in my charts-donut 144248. It is not independent evidence of this finding. What remains, and is real: Orca is never idle while the feed runs, and every act costs it tens to hundreds of ms of queue.
- **Fix idea:** Same as charts-03 (identity by datum, so a slide is one add and one remove). Beyond that, a way for a streaming chart to throttle its AT refresh.

### charts-06 {#charts-06}

Datum values are spoken as raw floats, ignoring the axis formatter the tooltip uses

- **Example:** chart-demo
- **Scenario:** charts-live
- **Act:** End on the live raw chart
- **The reader should get:** The value as the chart shows it: the example's axis formatter is '{:.0}', so '63' (the hover tooltip reads 'Windowed: 25 = 63').
- **The reader got (`261a218f`):** 'Windowed, 25: 63.004726', 'Windowed, 102: 14.740126 panel.'. The node name uses Rust's Display of the raw f32. The tooltip text goes through self.axis\_y.format(m.value); the AT name does not. It is also not locale-aware (a '.' decimal separator in every locale).
- **Platform:** Linux AT-SPI/Orca measured; the name is the same string on every platform (source).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Live chart values are still read as raw floats, 'Windowed, 75: 78.41107', while the axis and the tooltip show '78'. The name uses the raw f32 and is not locale-aware.
- **Measured again:** charts-live, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 charts-live 'End: the newest sample': FAIL the value is said as the axis shows it, not as a raw float: Orca said 'Windowed, 75: 78.41107 panel.', Orca said 'Windowed, 74: 65.73283'`
  - `pass2 charts-live 'End: the newest sample': FAIL ...: Orca said 'Windowed, 71: 44.92637 panel.', Orca said 'Windowed, 71: 44.92637'`
  - `pass1 tabwalk-chart-demo tree-launch.txt: [panel] 'Windowed, 12: 20.554934'`
  - `crates/teksilo-charts/src/hit.rs:381-383 mark_description formats m.value with Display; line_chart.rs:748-753 tooltip uses self.axis_y.format(m.value)`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/hit.rs:381-383 (mark\_description uses Display of the f32)
- **Evidence (`261a218f`):**
  - `charts-live-20260925-142453-428769/report.txt 'End: the newest sample': 'FAIL  the value is said as the axis shows it, not as a raw float' 'Orca said 'Windowed, 25: 63.004726'' 'Orca said 'Windowed, 25: 63.004726 panel.'' (3 of 3 live runs)`
  - `tree-chart-demo-20260925-135332-4041370/tree-launch.txt: '[panel] 'Windowed, 5: 79.801186'', '[panel] 'Rollup, 0: 65.81698''`
  - `crates/teksilo-charts/src/hit.rs:381-383 mark_description: format!("{}, {}: {}", m.series_name, m.category_label, m.value)`
  - `crates/teksilo-charts/src/line_chart.rs:748-753 tooltip: format!("{}: {} = {}", …, self.axis_y.format(m.value))`
  - `charts-live-20260925-144246-823973 'End: the newest sample': 'FAIL the value is said as the axis shows it, not as a raw float' ('Windowed, 99: 50.41903')`
- **Reproduced:** deterministic: 3 of 3 runs
- **Verification:** confirmed. Reproduced: deterministic: 4 of 4 of my live runs ('Windowed, 99: 50.41903', 'Windowed, 102: 14.740126', 'Windowed, 261: 36.257782', ...)
- **Fix idea:** Build the mark's name (and announcement) with the value axis formatter the tooltip uses (pie: format\_pie\_value plus the share), falling back to a locale-aware number format.

### charts-07 {#charts-07}

What the numbers mean is missing: no axis titles or units, and no slice share

- **Example:** chart-demo
- **Scenario:** charts-donut, reader.py tree
- **Act:** Tab to the bar chart and arrow through it; arrow through the donut
- **The reader should get:** A reader learns what they are hearing: the value axis is 'USD (k)', the category axis 'Quarter'. A donut slice's share of the whole (12%), which the outside labels (show\_percentages(true)) and the hover tooltip ('Storage: 18 (12.0%)') give a sighted user, is in the slice's AT name.
- **The reader got (`261a218f`):** 'Bar chart: 3 series, 4 categories document frame.', then 'Revenue, Q1: 41 panel.': no unit, no axis title anywhere in the tree. A slice reads ', Storage: 18' with no share.
- **Platform:** Linux AT-SPI/Orca measured; names are the same on every platform (source).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** No axis title or unit reaches the tree: the bar chart reads 'Bar chart: 3 series, 4 categories document frame.' and a bar 'Revenue, Q1: 41'. A donut slice reads ', Storage: 18' with no share, while the tooltip and the outside labels show 12%.
- **Measured again:** charts-donut and verify-charts-quiet-donut, 2 of 2 runs each; tabwalk-chart-demo tree 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 tabwalk-chart-demo tree-launch.txt: no 'USD' and no 'Quarter' axis text (grep count 0; only [label] 'Quarterly Performance')`
  - `pass1 tabwalk-chart-demo 'Tab 4': ORCA SAYS: 'Bar chart: 3 series, 4 categories document frame.'`
  - `pass1 charts-donut 'Right: first slice': FAIL the reader hears the slice's share (12%), which the tooltip and the labels show: Orca said [', Storage: 18 panel.', ', Storage: 18']`
  - `pass2 verify-charts-quiet-donut 'Right: first slice (idle Orca)': FAIL the reader hears the slice's share: Orca said [', Storage: 18 panel.', ', Storage: 18']`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/bar\_chart.rs:775-786, line\_chart.rs:766-777, pie\_chart.rs:708-714, hit.rs:381
- **Evidence (`261a218f`):**
  - `tree-chart-demo-20260925-135332-4041370/tree-launch.txt holds no 'USD' and no 'Quarter' axis text (grep count 0; only the GroupHeader label 'Quarterly Performance')`
  - `charts-donut-20260925-142921-523119/report.txt 'Right: first slice': 'FAIL  the reader hears the slice's share (12%), which the tooltip and the labels show' 'Orca said [', Storage: 18', ', Storage: 18 panel.']' (2 of 2 runs)`
  - `crates/teksilo-charts/src/bar_chart.rs:776-786 and line_chart.rs:766-777 set only role + a shape name; axis_x/axis_y labels are painted only`
  - `crates/teksilo-charts/src/pie_chart.rs:690-695 the tooltip carries '({:.1}%)', while hit.rs:381 mark_description does not`
  - `verify-charts-quiet-bar-20260925-144821-978687/tree-launch.txt: grep 'USD|Quarter'' = 0; document frames: name 'Bar chart: 3 series, 4 categories', description None`
  - `verify-charts-quiet-donut-20260925-144824-979341 'Right: first slice': 'FAIL the reader hears the slice's share' Orca said [', Storage: 18', ', Storage: 18 panel.']`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: deterministic: no 'USD'/'Quarter' anywhere in any tree of my runs; slice share absent in 3 of 3 donut runs
- **Fix idea:** Put the axis titles in the chart's description (or name), include the value-axis unit in each mark's name, and add the percentage to a pie slice's name.

### charts-08 {#charts-08}

The ChartStyle and chart-kind toggles say the selected option is 'not selected'

- **Example:** chart-demo
- **Scenario:** charts-style-toggle, charts-lines, tabwalk
- **Act:** Tab to the chart kind ('Bars') or the ChartStyle toggle ('Default'); Right to 'Gradient theme' / 'Lines'; also Feed / bucket / fn
- **The reader should get:** 'Default, selected radio button' (checked), and after Right 'Gradient theme' checked.
- **The reader got (`261a218f`):** 'Default.' 'not selected radio button' for the selected segment, and 'Gradient theme.' 'not selected radio button' right after choosing it. The segment carries AT-SPI 'selected' but not 'checked', and Orca reads a radio button's state from checked. Also at launch, before the window activates, Orca speaks five cut 'X. not selected radio button' from the controls' selection-changed events. This is the same SegmentedControl defect catalog-a covers; reported here because it is the ChartStyle toggle and chart-kind switch a chart-demo reader meets.
- **Platform:** Linux measured. Windows (source): a RadioButton exposes SelectionItem only from toggled (accesskit\_windows node.rs:654-656, 674), so NVDA also hears it not checked. macOS (source): value comes from toggled (accesskit\_macos node.rs:345-347), so no checked value.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `70183c50` (segmented).
- **Now (`c198e4d1`):** The chosen segment is now checked, and Orca reads it as selected: 'Bars.' 'selected radio button', 'Default.' 'selected radio button', and after Right 'Gradient theme.' 'selected radio button'. The same holds for Feed, Rollup bucket and fn. At launch Orca no longer speaks the five 'not selected radio button' readings.
- **Measured again:** tabwalk-chart-demo, charts-style-toggle, charts-lines, fix-segmented-charts: 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 tabwalk-chart-demo 'Tab 2': +32.9 ms object:state-changed:focused 1 [radio button] 'Bars', +114.1 ms ORCA SAYS: 'Bars.', +114.2 ms ORCA SAYS: 'selected radio button'`
  - `pass1 tabwalk-chart-demo 'Tab 12': ORCA SAYS: 'Running.', ORCA SAYS: 'selected radio button'`
  - `pass1 charts-style-toggle 'Right: Gradient theme': +194.3 ms ORCA SAYS: 'Gradient theme.', +194.3 ms ORCA SAYS: 'selected radio button'; pass Orca does not say 'not selected'`
  - `pass2 charts-style-toggle 'Right: Gradient theme': +152.3 ms ORCA SAYS: 'Gradient theme.', +152.3 ms ORCA SAYS: 'selected radio button'`
  - `pass1 fix-segmented-charts 'Right: Lines': pass [radio button] 'Lines' is checked; pass [radio button] 'Bars' is not checked; pass no object:selection-changed event from [*] '*'`
  - `pass1 tabwalk-chart-demo tree-launch.txt: [radio button] 'Bars' {checkable,checked}`
  - `pass1 charts-bar-keys 'launch': the only speech is ORCA SAYS: 'Teksilo — Chart Demo frame.'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/segmented\_control/cell.rs:285-294 (RadioButton with set\_toggled, no set\_selected)
- **Evidence (`261a218f`):**
  - `tabwalk-chart-demo-20260925-135407-4053592/report.txt 'Tab 3': 'object:state-changed:focused 1 [radio button] 'Default'', 'ORCA SAYS: 'Default.'', 'ORCA SAYS: 'not selected radio button''`
  - `charts-style-toggle-20260925-141053-145354 'Right: Gradient theme': 'object:state-changed:selected 1 [radio button] 'Gradient theme'', 'SAYS: 'Gradient theme.'', 'SAYS: 'not selected radio button''`
  - `tree-launch.txt: '[radio button] 'Default' {selectable,selected}' (no checked)`
  - `crates/teksilo-widgets/src/segmented_control/cell.rs:285-287 builder.set_role(RadioButton); builder.set_selected(self.selected.get() == self.index) with no set_toggled`
  - `verify-charts-quiet-bar-20260925-144821-978687 'Right: Paused': 'ORCA SAYS: 'Paused.'' then 'not selected radio button'; tree-launch.txt '[radio button] 'Bars' {selectable,selected}', '[radio button] 'Default' {selectable,selected}'`
  - `verify-charts-reshown-visited-20260925-145257-1104833 'Shift+Tab to the style toggle, Right: Gradient theme': 'Gradient theme.', 'not selected radio button'`
- **Reproduced:** deterministic: every run (16)
- **Verification:** confirmed. Reproduced: deterministic: every one of my runs (launch: five cut 'X. not selected radio button'; 'Paused.' + 'not selected radio button' right after choosing it; 'Gradient theme.' / 'Default.' + 'not selected radio button' in reshown-visited 3 of 3)
- **Fix idea:** In the segment cell: set\_toggled(Toggled::True/False) (radio semantics) in addition to or instead of set\_selected.

### charts-09 {#charts-09}

Chart strings are hardcoded English and not localizable

- **Example:** chart-demo
- **Scenario:** source
- **Act:** Any chart focus or datum move
- **The reader should get:** The chart's name, legend name and datum phrasing follow the app's locale (the framework has tr\_widget!).
- **The reader got (`261a218f`):** 'Bar chart: {} series, {} categories', 'Line chart: {} series, {} points', 'Pie chart: {} slices', 'Chart legend' and the datum phrasing are format! literals in English. A reader of a French app hears 'Bar chart: 3 series, 4 categories' in a French voice.
- **Platform:** all (source); measured here only in English
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The chart names, 'Chart legend' and the datum phrasing are still English format! literals. A reader of an app in another language hears them in English.
- **Measured again:** read from source
- **Evidence (`c198e4d1`):**
  - `crates/teksilo-charts/src/bar_chart.rs:783-786 set_name(format!("Bar chart: {} series, {} categories", ...))`
  - `crates/teksilo-charts/src/line_chart.rs:774-777 format!("Line chart: {} series, {} points", ...)`
  - `crates/teksilo-charts/src/pie_chart.rs:714 format!("Pie chart: {} slices", n)`
  - `crates/teksilo-charts/src/legend.rs:638-640 set_name("Chart legend")`
  - `grep for tr!, tr_widget! and LocalizedString in crates/teksilo-charts/src finds only reference_line.rs`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/bar\_chart.rs:783-786, line\_chart.rs:774-777, pie\_chart.rs:714, legend.rs:638-640, hit.rs:381
- **Evidence (`261a218f`):**
  - `crates/teksilo-charts/src/bar_chart.rs:783-786 builder.set_name(format!("Bar chart: {} series, {} categories", …))`
  - `crates/teksilo-charts/src/line_chart.rs:774-777 format!("Line chart: {} series, {} points", …)`
  - `crates/teksilo-charts/src/pie_chart.rs:714 format!("Pie chart: {} slices", n)`
  - `crates/teksilo-charts/src/legend.rs:638-640 builder.set_name("Chart legend")`
  - `grep for tr!/tr_widget! in crates/teksilo-charts/src finds none outside reference_line.rs`
- **Reproduced:** source reading (not measured with another locale)
- **Verification:** confirmed. Reproduced: source reading only; not measurable here (the example has no other locale)
- **Fix idea:** Route these through teksilo-i18n messages (tr\_widget!) with plural-aware Fluent messages.

### charts-10 {#charts-10}

Pie slice names start with a stray ', ' when the series is unnamed

- **Example:** chart-demo
- **Scenario:** charts-donut
- **Act:** Right on the donut
- **The reader should get:** 'Storage: 18' (plus its share, charts-07)
- **The reader got (`261a218f`):** ', Storage: 18' and ', Storage: 18 panel.'. The example's pie model comes from ChartModel::from\_points, whose one series is named '' (chart\_model.rs:287-288), and mark\_description always prefixes '{series}, '. Speech usually swallows the comma; braille shows it.
- **Platform:** Linux measured; the same name on every platform
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Pie slice names still start with a stray separator: ', Storage: 18' and ', Storage: 18 panel.'. Braille shows the comma.
- **Measured again:** charts-donut and verify-charts-quiet-donut, 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 charts-donut 'Right: first slice': +156.4 ms object:announcement [status bar] ', Storage: 18' text=', Storage: 18'; FAIL the slice's name does not start with a stray separator: Orca said [', Storage: 18 panel.', ', Storage: 18']`
  - `pass2 charts-donut 'Right: first slice': same failure`
  - `pass1 and pass2 verify-charts-quiet-donut 'Right: first slice (idle Orca)': FAIL no utterance starts with a stray ', '`
  - `crates/teksilo-data/src/chart_model.rs:287-288 from_points uses ChartSeries::new(String::new()); crates/teksilo-charts/src/hit.rs:381-383 always prefixes the series name`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/hit.rs:381-383; crates/teksilo-data/src/chart\_model.rs:287-288
- **Evidence (`261a218f`):**
  - `charts-donut-20260925-142921-523119/report.txt 'Right: first slice': 'object:announcement [status bar] ', Storage: 18'', 'FAIL  the slice's name does not start with a stray separator' 'Orca said [', Storage: 18', ', Storage: 18 panel.']' (2 of 2 runs)`
  - `crates/teksilo-data/src/chart_model.rs:287-288 from_points -> ChartSeries::new(String::new())`
  - `crates/teksilo-charts/src/hit.rs:381-383 format!("{}, {}: {}", m.series_name, …)`
- **Reproduced:** deterministic: 2 of 2 runs
- **Verification:** confirmed. Reproduced: deterministic: 3 of 3 donut runs (charts-donut 144248, 144616, quiet-donut)
- **Fix idea:** Omit the series prefix when the series name is empty (and for a single-series pie generally).

### charts-11 {#charts-11}

The chart's name counts hidden series

- **Example:** chart-demo
- **Scenario:** charts-bar-legend
- **Act:** Space on the 'Revenue' legend row (hides the series), then read the chart
- **The reader should get:** The summary says what is plotted, e.g. '2 of 3 series shown'.
- **The reader got (`261a218f`):** The chart stays 'Bar chart: 3 series, 4 categories' with 8 marks (Cost and Profit only); no name change is emitted.
- **Platform:** Linux measured; the same name on every platform
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** After Space hides Revenue, the chart still reads 'Bar chart: 3 series, 4 categories' while it holds only the 8 Cost and Profit marks. No name change is emitted.
- **Measured again:** charts-bar-legend 2 of 2 runs; verify-charts-quiet-bar 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 charts-bar-legend tree-Space-hides-Revenue.txt: [document frame] 'Bar chart: 3 series, 4 categories' with [panel] 'Cost, Q1: 34', 'Profit, Q1: 27', 'Cost, Q2: 51', 'Profit, Q2: 44', 'Cost, Q3: 68', 'Profit, Q3: 61', 'Cost, Q4: 25', 'Profit, Q4: 18'; [check box] 'Revenue' {checkable,focusable,focused}`
  - `pass1 charts-bar-legend 'Space hides Revenue': +27.1 ms object:children-changed:remove [document frame] 'Bar chart: 3 series, 4 categories' -> [panel] 'Revenue, Q1: 41' (no accessible-name change from the chart)`
  - `crates/teksilo-charts/src/bar_chart.rs:777 n_series = self.model.series_count(); crates/teksilo-data/src/chart_model.rs:631-633 returns order.len()`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/bar\_chart.rs:777 (series\_count counts hidden series; chart\_model.rs:631-633 is order.len()), line\_chart.rs:768
- **Evidence (`261a218f`):**
  - `charts-bar-legend-20260925-141736-261283 tree after 'Space hides Revenue': 'Bar chart: 3 series, 4 categories' ['Cost, Q1: 34', 'Profit, Q1: 27', 'Cost, Q2: 51', 'Profit, Q2: 44', 'Cost, Q3: 68', 'Profit, Q3: 61', 'Cost, Q4: 25', 'Profit, Q4: 18', 'Chart legend']; the Revenue check box states lack 'checked'`
  - `crates/teksilo-charts/src/bar_chart.rs:778 n_series = self.model.series_count() (all series, visible or not)`
  - `verify-charts-quiet-bar-20260925-144821-978687 'Space hides Revenue': '[document frame] 'Bar chart: 3 series, 4 categories': 8 marks ['Cost, Q1: 34', 'Profit, Q1: 27', 'Cost, Q2: 51', 'Profit, Q2: 44', 'Cost, Q3: 68', 'Profit, Q3: 61', 'Cost, Q4: 25', 'Profit, Q4: 18']'`
- **Reproduced:** deterministic: 2 of 2 runs
- **Verification:** confirmed. Reproduced: deterministic: 1 of 1 of my runs with a tree taken (verify-charts-quiet-bar), plus 2 sweep runs
- **Fix idea:** Count visible series (and say how many are hidden).

### charts-12 {#charts-12}

Every datum is announced as a 'panel'

- **Example:** chart-demo
- **Scenario:** charts-bar-keys
- **Act:** Any datum move
- **The reader should get:** A role that says what a datum is, or none (e.g. 'Revenue, Q1: 41').
- **The reader got (`261a218f`):** 'Revenue, Q1: 41 panel.' on every move. Role::GraphicsObject maps to AT-SPI Panel. The marks also expose an AT-SPI Value with minimum/maximum ±1.797e308, because numeric\_value is set with no range; Orca re-presents the object on every value change (see charts-03).
- **Platform:** Linux measured; Windows/macOS map GraphicsObject differently (not checked)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Every datum is still read with the role 'panel': 'Revenue, Q1: 41 panel.'. The marks still expose an AT-SPI Value whose minimum and maximum are about ±1.797e308.
- **Measured again:** every datum move in 2 of 2 runs of every chart scenario; tree from tabwalk-chart-demo 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 charts-bar-keys 'Right: first bar': +42.9 ms ORCA SAYS: 'Revenue, Q1: 41 panel.'`
  - `pass2 charts-lines 'End: last point': Orca said: 'Profit, Q4: 18 panel.'`
  - `pass1 tabwalk-chart-demo tree-launch.txt: [panel] 'Revenue, Q1: 41' value={'current': 41.0, 'minimum': -1.7976931348623157e+308, 'maximum': 1.7976931348623157e+308, 'increment': 0.0, ...}`
  - `accesskit_atspi_common-0.21.0/src/node.rs:175 Role::GraphicsObject => AtspiRole::Panel; node.rs:1761 and :1765 default minimum and maximum to f64::MIN and f64::MAX`
  - `crates/teksilo-charts/src/hit.rs:713-715 set_role(GraphicsObject), set_numeric_value with no range`
- **Where (`c198e4d1`):** crates/teksilo-charts/src/hit.rs:713-715
- **Evidence (`261a218f`):**
  - `charts-bar-keys-20260925-143311-618997: 'ORCA SAYS: 'Cost, Q1: 34 panel.'', 'ORCA SAYS: 'Profit, Q4: 18 panel.''`
  - `accesskit_atspi_common-0.20.0/src/node.rs:175 Role::GraphicsObject => AtspiRole::Panel`
  - `tree-launch.txt: '[panel] 'Revenue, Q1: 41' value={'current': 41.0, 'minimum': -1.7976931348623157e+308, 'maximum': 1.7976931348623157e+308, …}'`
  - `crates/teksilo-charts/src/hit.rs:713-715 set_role(GraphicsObject), set_numeric_value`
- **Reproduced:** deterministic
- **Verification:** corrected by the verifier. Reproduced: deterministic (every mark move in every run: '... panel.') Real and low, but the layer is framework, not upstream. accesskit\_atspi\_common node.rs:175 maps GraphicsObject to Panel, which follows the Graphics-AAM mapping (graphics-object -&gt; ATK panel), so AccessKit is behaving correctly. The word 'panel' comes from Teksilo's choice of role. The ±1.797e308 range comes from AccessKit's defaults when no range is set (node.rs:1726-1731, f64::MIN/f64::MAX), and Teksilo sets a numeric value with no min or max. The numeric value is also what produces the accessible-value storm of charts-05 and the second utterance per tick of charts-03.
- **Fix idea:** Teksilo could pick a role that reads as an item (e.g. ListItem inside the chart), or set a numeric range (min/max from the axis domain) so the Value interface is not ±infinity.

### charts-13 {#charts-13}

The example's segmented controls have no group name; 'Feed', 'Rollup bucket' and 'fn' are unassociated text

- **Example:** chart-demo
- **Scenario:** reader.py tree / tabwalk
- **Act:** Tab to any of the five SegmentedControls
- **The reader should get:** The reader hears what the choice is about ('Chart type', 'Chart style', 'Feed', 'Rollup bucket', 'Aggregate function').
- **The reader got (`261a218f`):** 'Bars. not selected radio button' / '×4. not selected radio button' / 'Mean. …' with no group name: the RadioGroup panels are unnamed ('\[panel\] '' {focusable}'), and the visible labels beside three of them are plain labels not linked to them.
- **Platform:** all (the name is missing from the tree)
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** The five segmented controls still have no group name. The reader hears 'Bars.' 'selected radio button', 'Running.' 'selected radio button', and so on, with nothing saying what the choice is about. The 'Feed', 'Rollup bucket' and 'fn' texts are plain labels beside unnamed groups.
- **Measured again:** tabwalk-chart-demo, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabwalk-chart-demo 'Tab 2': ORCA SAYS: 'Bars.', ORCA SAYS: 'selected radio button'`
  - `pass1 tabwalk-chart-demo 'Tab 13': ORCA SAYS: '×4.', ORCA SAYS: 'selected radio button'`
  - `pass1 tabwalk-chart-demo tree-launch.txt: [label] 'Feed' then [panel] '' {focusable} holding [radio button] 'Running' and 'Paused'; [label] 'Rollup bucket', [label] 'fn' likewise`
  - `examples/chart_demo/src/main.rs:535-553 and 804-810: SegmentedControl::indexed(...).segments(...) with no .label(...)`
- **Where (`c198e4d1`):** examples/chart\_demo/src/main.rs:535-553, 804-810
- **Evidence (`261a218f`):**
  - `tree-launch.txt: '[panel] '' {focusable}' parent of each radio-button set; '[label] 'Feed'', '[label] 'Rollup bucket'', '[label] 'fn'' as siblings`
  - `examples/chart_demo/src/main.rs:535-553 and 804-810 SegmentedControl::indexed(...).segments(...) with no .label(...) / .access_label(...)`
  - `crates/teksilo-widgets/src/segmented_control.rs:1360-1363 names the RadioGroup from its label when one is given`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: deterministic (tree)
- **Fix idea:** Give each SegmentedControl a label (SegmentedControl's label / .access\_label(tr!(...))).

### charts-M1 {#charts-m1}

A chart or datum the reader has already visited goes silent after it leaves the tree and comes back (legend hide/show, ChartStyle toggle round trip)

- **Example:** chart-demo
- **Scenario:** verify-charts-reshown-visited, verify-charts-quiet-bar
- **Act:** (a) Arrow onto 'Revenue, Q1: 41', Tab to the legend, Space (hide Revenue), Space (show), Shift+Tab, Right. (b) With the default chart visited, Shift+Tab to the style toggle, Right (Gradient theme), then Left (Default), Tab to the default chart, Right. Feed paused, idle Orca.
- **The reader should get:** The re-shown datum and the re-shown chart are read on focus, as the first time.
- **The reader got (`261a218f`):** Nothing. The focus event lands on the node (the bus shows object:state-changed:focused 1), but Orca drops it: 'Ignoring defunct object: \[panel: 'Revenue, Q1: 41'\]', and in (b) '\[document frame: 'Bar chart: 3 series, 4 categories'\]' as well as the marks. The reader Tabs onto the chart and hears silence, then arrows onto a datum they had heard before and hears silence. Nodes the reader never visited are read normally: the gradient chart and Cost Q1, which was never hidden, in the same runs. So the sweep's 'passed' claim that re-shown nodes keep working held only because its scenarios never visited them before hiding them. A removed node is marked defunct by accesskit\_atspi\_common and comes back with the same AT-SPI path. For a path Orca had already presented, the defunct state sticks and Orca ignores every later event from it for the life of the app.
- **Platform:** Linux AT-SPI/Orca measured. The defunct mechanism is AT-SPI's. Windows and macOS were not checked.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** A chart or datum the reader has visited is read again after it leaves the tree and comes back. After hiding and showing Revenue, Right onto 'Revenue, Q1: 41' is read. After a Gradient/Default round trip, Tab onto the default chart reads 'Bar chart: 3 series, 4 categories document frame.' and Right reads its first bar. Orca drops no event as defunct.
- **Measured again:** verify-charts-reshown-visited, 2 of 2 runs; verify-charts-quiet-bar 2 of 2
- **Evidence (`c198e4d1`):**
  - `pass1 verify-charts-reshown-visited 'Shift+Tab to the chart, Right: Revenue Q1 (visited, hidden, shown again)': +229.8 ms object:state-changed:focused 1 [panel] 'Revenue, Q1: 41', +279.7 ms ORCA SAYS: 'Revenue, Q1: 41 panel.'; pass Orca says 'Revenue, Q1: 41'`
  - `pass1 verify-charts-reshown-visited 'Tab to the default chart (visited, hidden, shown again)': +63.8 ms ORCA SAYS: 'Bar chart: 3 series, 4 categories document frame.'`
  - `pass1 verify-charts-reshown-visited 'Right: Revenue Q1 of the default chart shown again': +52.8 ms ORCA SAYS: 'Revenue, Q1: 41 panel.'`
  - `pass2 verify-charts-reshown-visited 'Shift+Tab to the chart, Right: Revenue Q1 (visited, hidden, shown again)': +320.9 ms ORCA SAYS: 'Revenue, Q1: 41 panel.'`
  - `pass1 and pass2 verify-charts-reshown-visited orca-debug.out: 0 'Ignoring defunct' lines`
  - `pass1 verify-charts-quiet-bar 'Right: first bar on the second entry (idle Orca)': Orca said at 16:39:48.936233: 'Revenue, Q1: 41 panel.'`
- **Where (`c198e4d1`):** crates/teksilo-core/src/accessibility/adapter\_ids.rs:128 (AdapterIds::deliver hands a new id to a node the adapter had removed), crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs:196 (deliver\_accessibility)
- **Evidence (`261a218f`):**
  - `verify-charts-reshown-visited-20260925-145257-1104833/report.txt 'Shift+Tab to the chart, Right: Revenue Q1 (visited, hidden, shown again)': 'pass focus lands on [panel] 'Revenue, Q1: 41'', 'FAIL Orca says 'Revenue, Q1: 41'', '14:53:41.585206 EVENT MANAGER: Ignoring defunct object: [panel: 'Revenue, Q1: 41']'; control 'Right: Cost Q1 (never hidden)': 'Cost, Q1: 34 panel.' spoken`
  - `same run 'Tab to the default chart (visited, hidden, shown again)': '+7.3 ms object:state-changed:focused 1 [document frame] 'Bar chart: 3 series, 4 categories'', 'Orca unheard', '14:54:01.499158 EVENT MANAGER: Ignoring defunct object: [document frame: 'Bar chart: 3 series, 4 categories']'; then 'Right: Revenue Q1 of the default chart shown again': '14:54:05.390730 Ignoring defunct object: [panel: 'Revenue, Q1: 41']'; control 'Tab to the gradient chart (never visited)': 'Bar chart: 3 series, 4 categories document frame.' spoken`
  - `identical in verify-charts-reshown-visited-20260925-145249-1096180 and -145253-1097743 (3 of 3), and in verify-charts-quiet-bar-20260925-144821-978687 'Right: first bar on the second entry': '14:49:42.132933 Ignoring defunct object: [panel: 'Revenue, Q1: 41']', 'Orca said []' (4 of 4 for the legend case)`
  - `bus: 'Space hides Revenue' '+46.4 ms object:state-changed:defunct 1 [panel] 'Revenue, Q1: 41''; 'Space shows Revenue again' '+34.5 ms object:children-changed:add [document frame] ... -> [panel] 'Revenue, Q1: 41'' (same path)`
  - `contrast: charts-bar-legend-20260925-144250-825193 (the sweep's scenario, whose reader never focused Revenue Q1 before hiding it) reads the re-shown bar normally, which is why the sweep passed it`
  - `crates/teksilo-charts/src/hit.rs:674-681 mark_element_id(series_id, point_idx) is deterministic, so a re-shown mark gets the same NodeId; Switcher pages keep their WidgetIds and so their AT NodeIds`
- **Reproduced:** 3 of 3 runs (reshown-visited) for the Switcher case; 4 of 4 for the legend case
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Mint a fresh AT NodeId whenever a node re-enters the platform tree: a generation that bumps on every exit (dormant or hidden). For chart marks, fold a per-series visibility epoch into mark\_element\_id. The same kind of fix as K2's for the announcer's reserved nodes.

### charts-M2 {#charts-m2}

The chart-kind control ('Bars') goes silent once the reader has visited the live charts: the ScrollArea clip drops it from the tree and it comes back defunct

- **Example:** chart-demo
- **Scenario:** verify-charts-reshown-visited, verify-charts-quiet-donut
- **Act:** From launch, Shift+Tab to the live charts at the bottom (the ScrollArea scrolls about 70 px), then Tab or Shift+Tab back to the chart-kind control 'Bars'
- **The reader should get:** The reader hears 'Bars, selected radio button' (or at least 'Bars').
- **The reader got (`261a218f`):** Silence. Scrolling down removes the 'Quarterly Performance' header, its separator and the whole chart-kind RadioGroup (Bars/Lines/Donut) from the AT tree. AccessKit's common\_filter excludes a child of a clips\_children node when its box and both its filtered neighbours lie outside the parent's box (accesskit\_consumer filters.rs:64-86); the ScrollArea sets clips\_children (scroll\_area.rs:1305). The nodes are marked defunct. Scrolled back into view, they return under the same paths, and Orca ignores the focus event on 'Bars', which it had presented at launch. 'Lines', which Orca had never presented, is read after Right. The theme selector in the same row survives, because its right-hand neighbour, the bar chart, is still visible.
- **Platform:** Linux AT-SPI/Orca measured; Windows/macOS not checked (the defunct mechanism is AT-SPI's)
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** After the reader visits the live charts and the page scrolls, the chart-kind control is read again on return: 'Bars.' 'selected radio button'. It still leaves the tree while scrolled out (a defunct event on 'Bars'), but it comes back under a new id and Orca reads it.
- **Measured again:** verify-charts-reshown-visited and verify-charts-quiet-donut, 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 verify-charts-reshown-visited events.jsonl: 16:38:58.641154 object:state-changed:defunct 1 [radio button] 'Bars'`
  - `pass1 verify-charts-reshown-visited 'Shift+Tab twice to the chart kind (scrolled out and back)': +243.6 ms object:state-changed:focused 1 [radio button] 'Bars', +303.6 ms ORCA SAYS: 'Bars.', +303.6 ms ORCA SAYS: 'selected radio button'; pass Orca says 'Bars'`
  - `pass2 verify-charts-reshown-visited 'Shift+Tab twice to the chart kind (scrolled out and back)': pass Orca says 'Bars'`
  - `pass1 verify-charts-quiet-donut 'Tab six times to the chart kind': +1005.4 ms object:state-changed:focused 1 [radio button] 'Bars', +1255.9 ms ORCA SAYS: 'Bars.'`
  - `pass2 verify-charts-quiet-donut 'Tab six times to the chart kind': +1037.4 ms ORCA SAYS: 'Bars.', +1037.5 ms ORCA SAYS: 'selected radio button'`
  - `orca-debug.out of all four runs: 0 'Ignoring defunct' lines`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/scroll\_area.rs:1305 (clips\_children, unchanged) together with crates/teksilo-core/src/accessibility/adapter\_ids.rs:128 (a new id for a node that comes back)
- **Evidence (`261a218f`):**
  - `verify-charts-reshown-visited-20260925-145257-1104833 events.jsonl: '14:53:07.768831 object:state-changed:defunct 1 radio button 'Bars'', '14:53:07.773449 object:children-changed:remove panel '' -> panel ...8540050432', later '14:53:49.402376 object:children-changed:add panel '' -> panel ...8540050432' (same path)`
  - `same run 'Shift+Tab twice to the chart kind (scrolled out and back)': '+240.4 ms object:state-changed:focused 1 [radio button] 'Bars'', 'FAIL Orca says 'Bars'' 'Orca unheard', '14:54:09.524382 EVENT MANAGER: Ignoring defunct object: [radio button: 'Bars']'; next act 'Right: Lines': 'Lines.' spoken`
  - `same in -145249-1096180 and -145253-1097743 (3 of 3), and in verify-charts-quiet-donut-20260925-144824-979341 'Tab six times to the chart kind': '14:48:48.312277 EVENT MANAGER: Ignoring defunct object: [radio button: 'Bars']' (4 of 4)`
  - `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/accesskit_consumer-0.39.0/src/filters.rs:64-86 (clipped-subtree exclusion); crates/teksilo-widgets/src/scroll_area.rs:1303-1305 (ScrollView + set_clips_children)`
- **Reproduced:** 4 of 4 runs that scroll down then return
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** The same fresh-id-on-re-entry fix as charts-M1. Teksilo cannot see the consumer's filter decision directly, so it could bump a node's AT generation when the ScrollArea moves the node's box out of and back into the viewport. The alternative is to stop setting clips\_children, which loses AccessKit's off-screen semantics.

### charts-M3 {#charts-m3}

The two live charts are named identically and carry none of their visible titles

- **Example:** chart-demo
- **Scenario:** verify-charts-live-full
- **Act:** Tab to the raw live chart, then to the rollup chart
- **The reader should get:** Each chart is named by its visible title ('Raw — ChartWindow (last N samples)', 'Rollup — ChartAggregate over the full history') or something equally distinct.
- **The reader got (`261a218f`):** 'Line chart: 1 series, 24 points document frame.' and 'Line chart: 1 series, 134 points document frame.'. The titles are plain sibling labels, not linked by labelled\_by; only the mark names ('Windowed, …' / 'Rollup, …') tell the two charts apart. The bar chart's 'Quarterly Performance' header is likewise not associated.
- **Platform:** all (the name is in the tree)
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** The two live charts are still told apart only by their point count: 'Line chart: 1 series, 24 points document frame.' and 'Line chart: 1 series, 50 points document frame.'. Their visible titles are sibling labels, not linked, and the bar chart's 'Quarterly Performance' header is not associated either.
- **Measured again:** tabwalk-chart-demo, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabwalk-chart-demo 'Tab 15': +252.6 ms ORCA SAYS: 'Line chart: 1 series, 24 points document frame.'`
  - `pass1 tabwalk-chart-demo 'Tab 16': +117.0 ms ORCA SAYS: 'Line chart: 1 series, 50 points document frame.'`
  - `pass2 tabwalk-chart-demo 'Tab 16': +87.5 ms ORCA SAYS: 'Line chart: 1 series, 50 points document frame.'`
  - `pass1 tabwalk-chart-demo tree-launch.txt: [label] 'Raw — ChartWindow (last N samples)' then [document frame] 'Line chart: 1 series, 24 points'; [label] 'Rollup — ChartAggregate over the full history' then [document frame] 'Line chart: 1 series, 10 points'`
- **Where (`c198e4d1`):** examples/chart\_demo/src/main.rs:503-574
- **Evidence (`261a218f`):**
  - `verify-charts-live-full-20260925-144827-982092 trees: two [document frame] 'Line chart: 1 series, 24 points' / 'Line chart: 1 series, 134 points', preceded by [label] 'Raw — ChartWindow (last N samples)' / [label] 'Rollup — ChartAggregate over the full history'`
  - `examples/chart_demo/src/main.rs:560-574 (titles as TextWidget siblings), 503-529 (charts without .access_label)`
- **Reproduced:** deterministic
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** In the example: .access\_label(...) on each chart (the override replaces the shape name). In the framework: a chart title API that goes into the name, with the shape summary moved to the description.
