<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Date and time pickers

Examples: `datetime-pickers`.
20 findings: 3 critical, 5 high, 7 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [datetime-01](#datetime-01) | datetime-pickers | DateEdit and DateTimeEdit calendars reopen on a stale cursor, and Enter writes that stale date over the field's value | critical | all | fixed |
| [datetime-02](#datetime-02) | datetime-pickers | A date field's calendar is silent on Orca from its second opening on: its retained nodes come back with ids libatspi holds defunct | critical | Linux | fixed |
| [datetime-03](#datetime-03) | datetime-pickers | The calendar's months/years view cannot be operated from the keyboard or by a screen reader, and Enter there writes the hidden day cursor as the selected date | critical | all | fixed |
| [datetime-04](#datetime-04) | datetime-pickers | Caret and selection moves inside date/time fields are never reported to the platform | high | all | fixed |
| [datetime-05](#datetime-05) | datetime-pickers | Stepping a date or time segment with Up/Down is silent: only a one-character diff reaches the bus | high | Linux | open |
| [datetime-06](#datetime-06) | datetime-pickers | DateTimeEdit's date/time parts and DateRangeEdit's start/end are read without their value on Orca | high | Linux | partly fixed |
| [datetime-07](#datetime-07) | datetime-pickers | The DateEdit and TimeEdit fields where focus lands have no name, and the example labels none of its editors | high | Linux | partly fixed |
| [datetime-08](#datetime-08) | datetime-pickers | Orca never says which day is selected, which is today, or where a range starts; committing a day is silent | high | Linux | upstream |
| [datetime-09](#datetime-09) | datetime-pickers | Committing a range in DateRangeEdit's calendar tells the reader nothing about the range | medium | Linux | open |
| [datetime-10](#datetime-10) | datetime-pickers | Orca treats the calendar grid as a layout table: the calendar and its month are never said on entering it | medium | Linux | upstream |
| [datetime-11](#datetime-11) | datetime-pickers | Header arrows keep the names 'Next month'/'Previous year' in the months and years views, where they step a year or a decade | medium | all | open |
| [datetime-12](#datetime-12) | datetime-pickers | The documented T key (jump to today) never fires from a real keyboard | low | all | open |
| [datetime-13](#datetime-13) | datetime-pickers | Zooming to the months view is heard only as '2026', and the months are twelve Tab stops starting at January | medium | all | fixed |
| [datetime-14](#datetime-14) | datetime-pickers | Opening a date field's calendar is not presented as a popup; its content hangs under an unnamed 'unknown' node at the top of the window | medium | Linux | open |
| [datetime-15](#datetime-15) | datetime-pickers | The DateTimeEdit and DateRangeEdit wrappers expose their halves' text run together with no separator | low | Linux | open |
| [datetime-16](#datetime-16) | datetime-pickers | Inside a calendar, Tab reaches the day grid before the header buttons above it | low | all | open |
| [datetime-17](#datetime-17) | datetime-pickers | Every date/time editor carries an empty unnamed status bar, and Open calendar repeats its name as its description | low | Linux | open |
| [datetime-v01](#datetime-v01) | datetime-pickers | Tab or Shift+Tab out of a date field's open calendar sends the reader to the far end of the window, not back beside the field | medium | all | open |
| [datetime-v02](#datetime-v02) | datetime-pickers | Stepping DateRangeEdit's start past its end silently swaps the two dates under the caret | medium | all | open |
| [datetime-v03](#datetime-v03) | datetime-pickers | The calendar's Role::Grid node holds its header buttons, a separator and the Today button, and declares no row or column count | low | all | open |

### datetime-01 {#datetime-01}

DateEdit and DateTimeEdit calendars reopen on a stale cursor, and Enter writes that stale date over the field's value

- **Example:** datetime-pickers
- **Scenario:** datetime-dateedit-stale, datetime-datetimeedit-stale, datetime-dateedit-popover
- **Act:** DateEdit: Up in the field (year 2026 -&gt; 2027), Alt+Down, Enter. DateTimeEdit: Tab to the date part, Up, Tab, Tab, Space on Open calendar, Enter. Also: open the DateEdit calendar, Right, Escape, Alt+Down again.
- **The reader should get:** The calendar opens on the date the field holds (Sunday, May 2, 2027) and Enter keeps it; after a close/reopen the calendar opens on the field's value again.
- **The reader gets:** The calendar opens on Saturday, May 2, 2026 (the value at build time) and the reader hears that date; Enter commits it, silently reverting the field to 2026-05-02. After Right+Escape, the reopened calendar lands on May 3 (where the last session's cursor was), not on the field's date. The reader is never told the calendar disagrees with the field.
- **Platform:** All platforms (widget logic, measured on Linux AT-SPI/Orca 46.1)
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `2d0446fc` (dateedit). Fixed part: DateEdit/DateTimeEdit/DateRangeEdit calendars open on the field's date (and on the day view) at every opening; Enter keeps the field's value.
- **Where:** crates/teksilo-widgets/src/calendar.rs:295-313 (cursor captured once); crates/teksilo-widgets/src/date\_edit.rs:652-676, 734-764; crates/teksilo-widgets/src/date\_time\_edit.rs:600-649; crates/teksilo-widgets/src/date\_range\_edit.rs:469-532
- **Evidence:**
  - `datetime-dateedit-stale-20260925-131141-2478059 report.txt, Up: '+29.1 ms object:property-change:accessible-name [label] 'Edit: 2027-05-02' text='Edit: 2027-05-02''`
  - `same run, Alt+Down: '+51.0 ms object:state-changed:focused 1 [table cell] 'Saturday, May 2, 2026'' then '+181.4 ms ORCA SAYS: 'Saturday, May 2, 2026.''; orca-debug.out '13:11:54.456223 - SPEECH OUTPUT: 'Saturday, May 2, 2026.''`
  - `same run, Enter: '+27.3 ms object:property-change:accessible-name [label] 'Edit: 2026-05-02' text='Edit: 2026-05-02'' / check 'FAIL the tree holds [label] 'Edit: 2027-05-02''`
  - `datetime-datetimeedit-stale-20260925-130553-2236814: '+71.1 ms object:state-changed:focused 1 [table cell] 'Saturday, May 2, 2026'' then on Enter '+26.3 ms object:property-change:accessible-name [label] 'DateTime: 2026-05-02 14:35' text='DateTime: 2026-05-02 14:35''`
  - `datetime-dateedit-popover-20260925-131250-2552592, Alt+Down reopens it: '+55.8 ms object:state-changed:focused 1 [table cell] 'Sunday, May 3, 2026'' while the field holds 05/02/2026`
  - `Cause: crates/teksilo-widgets/src/calendar.rs:295-299 and 309-311 (Calendar::single captures value.get() once into focused_date/visible_month); date_edit.rs:657-664 syncs only the selection signal (calendar_temp), never the cursor; date_edit.rs:704 add_detached_deferred builds the calendar once and keeps it; the open path date_edit.rs:742-764 never resets the cursor; Enter commits focused_date (calendar.rs:1366-1367, 1419) and DateEdit's on_activate writes it into the value (date_edit.rs:670-676). DateTimeEdit identical: date_time_edit.rs:618-649.`
  - `datetime-dateedit-stale-20260925-133422-3453537: Up '+29.x ms object:property-change:accessible-name [label] 'Edit: 2027-05-02''; Alt+Down 'object:state-changed:focused 1 [table cell] 'Saturday, May 2, 2026'' then 'ORCA SAYS: 'Saturday, May 2, 2026.''; Enter 'object:property-change:accessible-name [label] 'Edit: 2026-05-02'' then 'ORCA SAYS: 'entry 05/02/2026 selected.''`
  - `verify-datetime-reopen-others-20260925-133244-3345824, 'Space reopens the DateTimeEdit calendar': '+58.5 ms object:state-changed:focused 1 [table cell] 'Sunday, May 3, 2026''. 'Space reopens the DateRangeEdit calendar': '+62.9 ms object:state-changed:focused 1 [table cell] 'Sunday, May 3, 2026''`
  - `verify-datetime-rangeedit-stale-20260925-133238-3336374: after Up the range is 'Range edit: 2026-05-16 – 2027-05-02', yet 'Space opens the range calendar' gives '+54.8 ms object:state-changed:focused 1 [table cell] 'Saturday, May 2, 2026'' / 'ORCA SAYS: 'Saturday, May 2, 2026.''`
- **Reproduced:** DateEdit 3 of 3 runs (130553, 131141, 131414); DateTimeEdit 3 of 3 (130553, 131213, 131533); reopen-after-navigation lands on the old cursor 3 of 3 popover runs (131030, 131250, 131414)
- **Verification:** confirmed. Reproduced: DateEdit Up/Alt+Down/Enter reverts 3 of 3 (dateedit-stale 132653, 133105, 133422). DateTimeEdit 3 of 3 (datetimeedit-stale 132707, 133110, 133427). Reopen lands on the previous cursor 3 of 3 for DateEdit (dateedit-popover 132700, 133137, 133417) and 3 of 3 for DateTimeEdit and DateRangeEdit (verify-datetime-reopen-others 133244, 133615, 133813). DateRangeEdit opens on the stale start 2 of 2 (verify-datetime-rangeedit-stale 133238, 133546).
- **Fix idea:** On every open, set the calendar's focused\_date and visible\_month from the field's value (Calendar::focused\_date\_signal()/visible\_month\_signal() can be taken before the widget is moved into add\_detached\_deferred), or make Calendar::single follow external writes to its bound value with an effect that moves the cursor and visible month.

### datetime-02 {#datetime-02}

A date field's calendar is silent on Orca from its second opening on: its retained nodes come back with ids libatspi holds defunct

- **Example:** datetime-pickers
- **Scenario:** datetime-dateedit-popover
- **Act:** DateEdit: Alt+Down, Right, Escape, Alt+Down (reopen), Left, Left, Home.
- **The reader should get:** The reopened calendar speaks like the first time: the day under the cursor on opening and every day the cursor moves to.
- **The reader gets:** Opening again says nothing; moving onto any day met in an earlier opening says nothing (Orca: 'Ignoring defunct object'); only a day never visited before (May 1) or cells rebuilt by a change of month are spoken. The grid node itself is dropped too (its name-change events), and after Enter/Escape from a silent reopen Orca does not even re-announce the field because its locus never left it. Reopening from the months view focuses the grid node, also dropped.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). macOS: accesskit\_macos drops the platform object on removal and creates a new one on return (context.rs:70-83), so no defunct carry-over is expected there (source only). Windows: UIA has no defunct mark by id (source only, not verified by run).
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: reopened calendar and a day met before heard 3/3.
- **Where:** crates/teksilo-core/src/deferred\_subtree.rs (built once, retained) + dormancy that removes the subtree from the AT tree; node ids from teksilo\_core::accessibility::widget\_id\_to\_node\_id; callers date\_edit.rs:704, date\_time\_edit.rs:648, date\_range\_edit.rs:493
- **Evidence:**
  - `datetime-dateedit-popover-20260925-131250-2552592 events.jsonl: the day keeps one path across the close: '13:13:03.834982 FOCUS Sunday, May 3, 2026 /org/a11y/atspi/accessible/0/79228194298004376595101384704', '13:13:07.724833 DEFUNCT Sunday, May 3, 2026 .../79228194298004376595101384704', '13:13:11.645912 FOCUS Sunday, May 3, 2026 .../79228194298004376595101384704'`
  - `same run, Alt+Down reopens it: '+55.8 ms object:state-changed:focused 1 [table cell] 'Sunday, May 3, 2026'' and no speech; orca-debug.out '13:13:11.660009 - EVENT MANAGER: Ignoring defunct object: [table cell: 'Sunday, May 3, 2026']'`
  - `same run, Left, a day back: '+32.6 ms object:state-changed:focused 1 [table cell] 'Saturday, May 2, 2026'', no speech; '13:13:15.543229 - EVENT MANAGER: Ignoring defunct object: [table cell: 'Saturday, May 2, 2026']'`
  - `same run, Left again (a day never visited): '+32.2 ms object:state-changed:focused 1 [table cell] 'Friday, May 1, 2026'' / '+142.8 ms ORCA SAYS: 'Friday, May 1, 2026.''`
  - `same run, Home (month change to April): new cells spoken 'Sunday, April 26, 2026.' but '13:13:23.383781 - EVENT MANAGER: Ignoring defunct object: [table: 'Calendar, April 2026']'`
  - `datetime-dateedit-popover-20260925-130630-2262165: Orca log '13:06:48.552839 - EVENT MANAGER: Ignoring defunct object: [table cell: 'Saturday, May 2, 2026']' on reopen; months-view reopen '+37.8 ms object:state-changed:focused 1 [table] 'Calendar, May 2026'' with 'Ignoring defunct object: [table: 'Calendar, May 2026']' and no speech`
  - `Cause: the calendar is retained ('Built at most once', crates/teksilo-core/src/deferred_subtree.rs:40-45; date_edit.rs:704-705) and parked dormant on close, which removes it from the filtered tree; accesskit_atspi_common adapter.rs:91-106 remove_node emits StateChanged(Defunct, true) for each node; on reopen the same NodeIds (widget_id_to_node_id of unchanged arena ids) return, and Orca 46.1 event_manager.py:797-798 drops any event whose source is dead or defunct. Nodes Orca had never inspected are not yet cached as defunct, which is why an unvisited day still speaks.`
  - `Same retention exists for DateRangeEdit (date_range_edit.rs:493-494) and DateTimeEdit (date_time_edit.rs:648-649); not run here.`
  - `verify-datetime-reopen-others-20260925-133244-3345824: 'Space reopens the DateTimeEdit calendar' '+58.5 ms object:state-changed:focused 1 [table cell] 'Sunday, May 3, 2026'', no speech, '13:33:12.596983 EVENT MANAGER: Ignoring defunct object: [table cell: 'Sunday, May 3, 2026']'. 'Left, back onto 2 May': '13:33:16.449323 EVENT MANAGER: Ignoring defunct object: [table cell: 'Saturday, May 2, 2026']'. 'Left again, to 1 May (never met)': '+118.9 ms ORCA SAYS: 'Friday, May 1, 2026.''`
  - `same run, DateRangeEdit: 'Space reopens the DateRangeEdit calendar' '+62.9 ms object:state-changed:focused 1 [table cell] 'Sunday, May 3, 2026'', no speech, '13:33:48.072492 EVENT MANAGER: Ignoring defunct object: [table cell: 'Sunday, May 3, 2026']'`
  - `datetime-dateedit-popover-20260925-133137-3278961: reopen '+53.2 ms object:state-changed:focused 1 [table cell] 'Sunday, May 3, 2026'' / '13:31:58.492390 EVENT MANAGER: Ignoring defunct object: [table cell: 'Sunday, May 3, 2026']'`
  - `Source: crates/teksilo-widgets/src/date_time_edit.rs:648 ctx.add_deferred (non-detached) shows the same silence as the detached DateEdit (date_edit.rs:704) and DateRangeEdit (date_range_edit.rs:493)`
- **Reproduced:** 4 of 4 popover runs (130630, 131030, 131250, 131414): reopen silent with 'Ignoring defunct object'; visited day silent / unvisited day spoken 3 of 3 (131030, 131250, 131414)
- **Verification:** confirmed. Reproduced: DateEdit reopen silent 3 of 3 (dateedit-popover 132700, 133137, 133417); visited day silent and unvisited day spoken 3 of 3. DateTimeEdit and DateRangeEdit reopen silent 3 of 3 each, visited day silent 3 of 3 (verify-datetime-reopen-others 133244, 133615, 133813). Months-view reopen silent 3 of 3 (verify-datetime-months-dataloss 133254, 133536, 133739).
- **Fix idea:** Same mechanism as K2 but for ordinary widget nodes: a subtree that leaves the filtered tree and comes back must come back under fresh AccessKit ids (e.g. mix an activation epoch into the node id of a re-activated dormant subtree), otherwise every retained popover, menu or dropdown is silent on Orca after its first showing. If the K2 fix only renamed the announcer's two reserved nodes it does not cover this.

### datetime-03 {#datetime-03}

The calendar's months/years view cannot be operated from the keyboard or by a screen reader, and Enter there writes the hidden day cursor as the selected date

- **Example:** datetime-pickers
- **Scenario:** datetime-zoom, datetime-dateedit-popover
- **Act:** Single calendar: Space on the title button (months view), Tab x3 into the months, Down, Enter, Tab, Escape, AT-SPI click on 'March', Space on the title again.
- **The reader should get:** Arrows move among the months and are spoken, Enter/Space (or a screen reader's click) opens that month in the days view, Escape returns to the days, and the selected date is untouched.
- **The reader gets:** Down moves nothing a reader can see (no focus change, silence) but moves the invisible day cursor; Enter on 'January' commits that cursor day (Selected: 2026-05-02 becomes 2026-05-09) and stays in the months view; the months offer no AT-SPI action, so a screen reader cannot activate one; Escape does nothing; the title only goes on to the years view ('2020 to 2029'). There is no keyboard or AT path back to picking a day in a standalone calendar. In a DateEdit popover the months view persists across close/reopen, so the reopened calendar focuses the grid with nothing to pick.
- **Platform:** All platforms for the key handling and missing click action (widget logic); measured on Linux AT-SPI/Orca
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `2d0446fc` (dateedit). Fixed part: months/years view is keyboard- and AT-operable (arrows heard, Enter/Space zoom in, Escape back to days, Click advertised), never commits the hidden day; a date field's calendar never reopens on the months view.
- **Where:** crates/teksilo-widgets/src/calendar/zoom\_grid.rs:355-381, 418-424; crates/teksilo-widgets/src/calendar.rs:1305-1427; crates/teksilo-widgets/src/calendar/header.rs:199-216
- **Evidence:**
  - `datetime-zoom-20260925-131710-2787163, Down in the months view: 'FAIL focus lands on [table cell] 'April'' - 'no focus change on the bus in this act'`
  - `same run, Enter on the focused month: '+31.8 ms object:property-change:accessible-name [label] 'Selected: 2026-05-09' text='Selected: 2026-05-09'' and 'pass the tree holds [push button] '2026'' (still the months view)`
  - `same run, Escape: '[table cell] 'March' interfaces=['Accessible', 'Component'] actions=[]'; AT-SPI click: 'RunError: {... 'name': 'March', 'role': 'table cell'} offers no action on AT-SPI'`
  - `same run, Space on the title in the months view: '+57.1 ms object:property-change:accessible-name [push button] '2020 to 2029' text='2020 to 2029''`
  - `datetime-dateedit-popover-20260925-130630-2262165, Alt+Down reopens the calendar (after closing from the months view): '+37.8 ms object:state-changed:focused 1 [table] 'Calendar, May 2026''`
  - `Cause: ZoomCell registers only on_hover/on_tap/on_access_action (crates/teksilo-widgets/src/calendar/zoom_grid.rs:362-381), no key handler, and its accessibility() never advertises Action::Click (zoom_grid.rs:418-424); keys bubble to the calendar's handler, which has no mode branch: arrows move focused_date and Enter/Space commit it (calendar.rs:1330-1367, 1419); Escape is Ignored in single mode (calendar.rs:1369-1377); the title button only demotes (calendar/header.rs:209-216).`
  - `datetime-zoom-20260925-133622-3600820, 'Enter on the focused month': 'object:property-change:accessible-name [label] 'Selected: 2026-05-09''; 'AT-SPI click on the month March': 'RunError: {... 'name': 'March', 'role': 'table cell'} offers no action on AT-SPI'`
  - `verify-datetime-months-dataloss-20260925-133739-3666083: 'Down in the months view' gives no events and no speech. 'Alt+Down reopens it' gives 'object:state-changed:focused 1 [table] 'Calendar, May 2026'' and '13:38:07.989055 EVENT MANAGER: Ignoring defunct object: [table: 'Calendar, May 2026']'. 'Enter in the reopened calendar' gives 'object:text-changed:insert [entry] '' text='9'', 'object:property-change:accessible-name [label] 'Edit: 2026-05-09'', no SPEECH OUTPUT, and '13:38:12.429211 - DEFAULT: Not speaking inserted string due to lack of cause'`
- **Reproduced:** 2 of 2 complete zoom runs (130331, 131710), plus the previous attempt's run; months view persisting across a DateEdit reopen 4 of 4 popover runs
- **Verification:** confirmed. Reproduced: datetime-zoom 3 of 3 (132725, 133147, 133622). Silent write after a months-view reopen 3 of 3 (verify-datetime-months-dataloss 133254, 133536, 133739), plus 3 of 3 in dateedit-popover (132700, 133137, 133417; the field becomes 2026-04-26 with no utterance).
- **Fix idea:** Give the months/years grids their own keyboard model (roving focus over the cells, arrows move and speak, Enter/Space pick and zoom in, Escape zooms back in), make the calendar's day keys a no-op when mode != Days, and advertise Action::Click on ZoomCell. Reset mode to Days when a date field reopens its calendar.

### datetime-04 {#datetime-04}

Caret and selection moves inside date/time fields are never reported to the platform

- **Example:** datetime-pickers
- **Scenario:** datetime-caret
- **Act:** In the DateEdit field (text selected at launch): Home, Right, Right, End, Shift+Left, then type 7.
- **The reader should get:** Each move emits object:text-caret-moved (and text-selection-changed for Home and Shift+Left), so the reader hears the character or selection and knows which segment Up/Down will step.
- **The reader gets:** No caret or selection event at all for Home, Right, End or Shift+Left, though the keys work (Shift+Left selected the last digit and typing 7 replaced it: Edit: 2027-05-02). A reader cannot review the field by character nor tell which segment is under the caret.
- **Platform:** All platforms (the AccessKit node is not re-walked, so no adapter sees the move); measured on Linux AT-SPI
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:199-230, 440-481
- **Evidence:**
  - `datetime-caret-20260925-131030-2422452 and datetime-caret-20260925-131710-2786962, every navigation act: 'FAIL a object:text-caret-moved event from [entry] '*'' - 'no object:text-caret-moved event from [entry] '*''; Home and Shift+Left also 'no object:text-selection-changed event from [entry] '*''`
  - `same runs, Type 7 over the selection: '+33.4 ms object:text-changed:delete [entry] '' text='6'', '+33.7 ms object:text-changed:insert [entry] '' text='7'' and 'pass the tree holds [label] 'Edit: 2027-05-02''`
  - `Cause: TextInputField binds only text_signal (and feedback) at AccessibilityOnly (crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:452-481, 209-230); a caret key updates cursor_position/cursor_anchor via sync_cursor_signals (text_input_field/state.rs:575-614, called from keyboard.rs:294-298) but nothing binds those signals to an accessibility re-walk; the selection is written only when the node is walked (widget_impl.rs:1146-1159).`
  - `datetime-caret-20260925-133453-3494766: Home, Right, Right, End and Shift+Left all 'FAIL a object:text-caret-moved event from [entry]' / 'no object:text-selection-changed event'`
  - `datetime-timeedit-20260925-132806-3088695, 'Home then Up (hour)': 'object:text-caret-moved [entry]' appears only at +670.2 ms, after 'object:text-changed:insert [entry] '' text='5''`
  - `Precedent: crates/teksilo-widgets/src/rich_text/body.rs:196-215 binds cursor_position and cursor_anchor at RepaintOnly and at AccessibilityOnly`
- **Reproduced:** 2 of 2 runs (131030, 131710); deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 (datetime-caret 132743, 133453); deterministic
- **Fix idea:** Bind the field's cursor\_position and cursor\_anchor signals at BindingLevel::AccessibilityOnly in TextInputField::build, as text\_signal already is.

### datetime-05 {#datetime-05}

Stepping a date or time segment with Up/Down is silent: only a one-character diff reaches the bus

- **Example:** datetime-pickers
- **Scenario:** datetime-timeedit, datetime-dateedit-stale, datetime-datetimeedit-stale
- **Act:** DateEdit field: Up (year). TimeEdit 24 h: Up, Down, Shift+Up, Home then Up. TimeEdit 12 h: End then Up (AM/PM). DateTimeEdit date part: Up.
- **The reader should get:** The reader hears the new value (e.g. '14:36', '05/02/2027', 'AM').
- **The reader gets:** Nothing is spoken. The entry emits text-changed:delete '5' / insert '6' (or '6'-&gt;'7', 'P'-&gt;'A') and nothing else; Orca logs 'Not speaking inserted string due to lack of cause'. The wrapper's value (the date in words) is carried by no AT-SPI interface.
- **Platform:** Linux AT-SPI / Orca 46.1 measured. With a real key Orca would see lastKey=Up: isAutoTextEvent is true only for an editable descendant of a combo box (script\_utilities.py:2442-2443) and a one-character insertion is echoed only with echo-by-character on (script\_utilities.py:3804-3805, off by default), so at best the reader hears a lone '7'. Windows/macOS: the wrapper's value changes (UIA/macOS carry it) but the focus is on the inner edit; not measured.
- **Severity:** high; **layer:** framework
- **Status:** Open. In the dateedit fix topic, not fixed there: Segment stepping is still heard only as a one-character text diff. There are two fixes: an announcement per step, or spin-button semantics per segment, and they need a design decision. The house Listener model (heard\_test.rs) already counts the focused field's value change as heard on UIA and macOS, so an announcement would say the step twice there. Neither option can be measured here for NVDA or VoiceOver. Orca 46.1 would speak a real key's inserted text only as a lone character (default.py onTextInserted: no cause; isSelectedTextInsertionEvent speaks only the inserted diff).
- **Where:** crates/teksilo-widgets/src/date\_edit.rs:914-960; time\_edit.rs:700-720; date\_time\_edit.rs:1295-1310; date\_range\_edit.rs:1020-1035
- **Evidence:**
  - `datetime-timeedit-20260925-130723-2298159, Up: '+42.7 ms object:text-changed:delete [entry] '' text='5'', '+43.1 ms object:text-changed:insert [entry] '' text='6'', '+43.3 ms object:text-selection-changed [entry] ''' and 'FAIL Orca says '14:36'' - 'Orca unheard: '14:36''`
  - `same run orca-debug.out: '13:07:38.608167 - DEFAULT: Not speaking inserted string due to lack of cause'`
  - `same run, End then Up (AM/PM): '+657.8 ms object:text-changed:delete [entry] '' text='P'', '+658.1 ms object:text-changed:insert [entry] '' text='A'', 'FAIL Orca says 'AM''`
  - `datetime-dateedit-stale-20260925-130553-2236815, Up: '+33.7 ms object:text-changed:delete [entry] '' text='6'', '+33.8 ms object:text-changed:insert [entry] '' text='7'', said=[]`
  - `Cause: the step rewrites text_signal (date_edit.rs segment_step, time_edit.rs equivalent), which the field publishes as a minimal diff; no announcement or value-change carries the new value to AT-SPI.`
  - `datetime-timeedit-20260925-133524-3540687: Up, Down, Shift+Up, Home+Up and End+Up all give said=[]; each act's only changes are 'object:text-changed' diffs and the example's label rename`
  - `datetime-timeedit-20260925-132806-3088695 orca-debug.out: '13:28:19.954785 - DEFAULT: Not speaking inserted string due to lack of cause'`
- **Reproduced:** TimeEdit 2 of 2 runs (130723, 131817); DateEdit Up 3 of 3; DateTimeEdit Up 3 of 3; deterministic
- **Verification:** confirmed. Reproduced: TimeEdit 2 of 2 (132806, 133524); DateEdit Up 3 of 3; DateTimeEdit Up 3 of 3; DateRangeEdit start Up 2 of 2
- **Fix idea:** After a step, announce the new value in words through the tree's announcer (after the K2 fix), or expose each segment as a spin button with a numeric value so Orca speaks value-changed on the focus.

### datetime-06 {#datetime-06}

DateTimeEdit's date/time parts and DateRangeEdit's start/end are read without their value on Orca

- **Example:** datetime-pickers
- **Scenario:** (tabwalk), datetime-datetimeedit-stale, datetime-range-edit
- **Act:** Tab onto the DateTimeEdit date part, its time part, the DateRangeEdit start and end dates; return to the DateTimeEdit date part after a calendar commit; Shift+Tab to the DateRangeEdit end date.
- **The reader should get:** The reader hears the name and the value: 'Date, 05/02/2026', 'End date, 05/05/2026'.
- **The reader gets:** 'Date date editor.', 'Time date editor.', 'Start date date editor.', 'End date date editor.' with no value. The value is in the node's Text interface but Orca's default speech format for DATE\_EDITOR is labelOrName + roleName.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows: DateInput/TimeInput map to UIA Edit (accesskit\_windows node.rs:79-91), so the value pattern should be read; macOS maps them to AXDateField/AXTimeField (accesskit\_macos node.rs:85-88); neither measured.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `2d0446fc` (dateedit). Fixed part: DateTimeEdit parts and DateRangeEdit halves are entries read with their date/time.
- **Where:** crates/teksilo-widgets/src/date\_time\_edit.rs:1204-1206; crates/teksilo-widgets/src/date\_range\_edit.rs:957-959
- **Evidence:**
  - `tabwalk-datetime-pickers-20260925-132032-2912751: '+22.3 ms object:state-changed:focused 1 [date editor] 'Date'' / '+127.5 ms ORCA SAYS: 'Date date editor.''; Tab 7: 'ORCA SAYS: 'Start date date editor.''; Tab 8: 'ORCA SAYS: 'End date date editor.''`
  - `tree-datetime-pickers-20260925-125905-2013173 tree-launch.txt: "[date editor] 'Date' {editable,focusable,selectable-text,single-line} text='05/02/2026'"`
  - `datetime-range-edit-20260925-131250-2552648, Shift+Tab to the end date: 'object:state-changed:focused 1 [date editor] 'End date'' / 'ORCA SAYS: 'End date date editor.'' / 'FAIL Orca says '05/05/2026''`
  - `datetime-datetimeedit-stale-20260925-130553-2236814, Enter: '+26.4 ms object:state-changed:focused 1 [date editor] 'Date'' / '+170.7 ms ORCA SAYS: 'Date date editor.''`
  - `Cause: the editable halves are re-roled to DateInput/TimeInput (crates/teksilo-widgets/src/date_time_edit.rs:1204-1206; date_range_edit.rs:950-960), which accesskit_atspi_common maps to AtspiRole::DateEditor (node.rs:268-272); DATE_EDITOR has no entry in Orca's formatting.py, so the 'default' format applies (formatting.py:133-138: 'labelOrName + roleName + availability ...'), which reads no text.`
  - `tabwalk-datetime-pickers-20260925-132959-3197203: Tab 4 'object:state-changed:focused 1 [date editor] 'Date'' / 'ORCA SAYS: 'Date date editor.''; Tab 8 'ORCA SAYS: 'End date date editor.''`
  - `datetime-range-edit-20260925-133501-3506331, Shift+Tab: 'ORCA SAYS: 'End date date editor.'' / FAIL Orca says '05/05/2026'`
- **Reproduced:** tabwalk 1 run; DateTimeEdit return-to-field 3 of 3; DateRangeEdit end date 3 of 3; deterministic
- **Verification:** confirmed. Reproduced: deterministic: tabwalk 132959 (Tab 4, 5, 7, 8); DateTimeEdit return-to-field 3 of 3; DateRangeEdit Shift+Tab to the end date 3 of 3
- **Fix idea:** Keep the editable halves Role::TextInput (Entry on AT-SPI, whose text Orca reads) with their names, and keep the DateInput/DateTimeInput semantics on the wrapper; or report upstream that a text-bearing DateEditor needs Orca's entry format.

### datetime-07 {#datetime-07}

The DateEdit and TimeEdit fields where focus lands have no name, and the example labels none of its editors

- **Example:** datetime-pickers
- **Scenario:** (tree), (tabwalk), datetime-timeedit
- **Act:** Launch (focus in the DateEdit field); Tab to the 24 h and 12 h TimeEdit fields.
- **The reader should get:** The reader hears which field this is: 'Date', or better the example's own label ('24 h time', '12 h time'), then the value.
- **The reader gets:** 'entry 05/02/2026 selected.', 'entry 14:35 selected.', 'entry 02:35:00 PM selected.': no name. The wrapper named 'Date'/'Time' is not spoken: Orca's ancestor format for a date editor is empty. The two TimeEdits' wrappers (and the DateTimeEdit time part) are all named 'Time', so even their names would not tell them apart.
- **Platform:** Linux AT-SPI / Orca 46.1 measured. The focused node is unnamed on every adapter (accesskit\_consumer takes the name from the label, node.rs:744-746); whether a Windows/macOS reader recovers the wrapper's name is not measured.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `2d0446fc` (dateedit). Fixed part: framework part): DateEdit/TimeEdit inner field carries the editor's label or 'Date'/'Time'; access\_label/access\_described\_by on the editor reach it via accessibility\_proxy.
- **Where:** crates/teksilo-widgets/src/date\_edit.rs:841-849; crates/teksilo-widgets/src/time\_edit.rs:604-606, 764-770
- **Evidence:**
  - `tree-datetime-pickers-20260925-125905-2013173 report.txt tree audit: "unnamed-control: [entry] '': a focusable entry with no name (its text is '05/02/2026')", '... (its text is '14:35')', '... (its text is '02:35:00 PM')'`
  - `tree-launch.txt: "[date editor] 'Date' ... [entry] '' {editable,focusable,focused,selectable-text,single-line} text='05/02/2026'" and two "[date editor] 'Time'" wrappers`
  - `same run orca-debug.out: '12:59:09.861887 - SPEECH GENERATOR: Starting ancestor generation for [date editor: 'Date'] (using role: date-editor)' followed by 'GENERATION TIME: 0.0485 ----> newAncestors=[]' and 'GENERATION TIME: 0.0017 ----> labelOrName=[]'`
  - `datetime-timeedit-20260925-131817-2831371: 'FAIL Orca says 'Time'' - 'Orca said: 'entry 02:35:00 PM selected.''`
  - `Cause: DateEdit never forwards its name to the inner TextInput (crates/teksilo-widgets/src/date_edit.rs:841-850, 'the label is intentionally NOT forwarded'); TimeEdit forwards only an explicit .label() (time_edit.rs:604-606), not its default 'Time' (time_edit.rs:764-770); Orca's formatting.py default 'ancestor': '[]' drops a date-editor ancestor. Example: examples/datetime_pickers/src/main.rs:130-162 calls no .label() on any editor.`
  - `tree-datetime-pickers-20260925-132940-3172543 tree-launch.txt: "[date editor] 'Time' ... [entry] '' {editable,focusable,...} text='14:35'" twice under wrappers both named 'Time'`
  - `datetime-timeedit-20260925-132806-3088695 orca-debug.out: '13:28:15.651353 - SPEECH GENERATOR: Starting ancestor generation for [date editor: 'Time'] (using role: date-editor)' then 'newAncestors=[]' and 'labelOrName=[]'`
- **Reproduced:** tree run + tabwalk + timeedit 2 of 2; deterministic
- **Verification:** confirmed. Reproduced: deterministic: tree 132940, tabwalk 132959, timeedit 2 of 2, and every run's launch audit
- **Fix idea:** Name the inner TextInput with the widget's label or default name (TextInput::label names the text node, not the filtered container, per text\_input/tests.rs:766), and have the example give each editor a .label() matching its purpose.

### datetime-08 {#datetime-08}

Orca never says which day is selected, which is today, or where a range starts; committing a day is silent

- **Example:** datetime-pickers
- **Scenario:** datetime-grid-keys, datetime-range-calendar
- **Act:** Tab into the single calendar (on the selected day), arrows back onto the selected day, Enter on May 30, Left/Right back onto it; range calendar: Tab in (today), Enter (start), Right x3, Enter (end), Left onto a day inside the range.
- **The reader should get:** The reader hears 'selected' on the selected day(s), 'today' on today, and a confirmation when Enter selects a day or sets a range start.
- **The reader gets:** Only the date name is ever spoken ('Saturday, May 2, 2026.'), selected or not, today or not. Enter updates the selected state on the bus but nothing is spoken. The range's first Enter changes nothing on the bus at all (the anchor is not exposed). The grid's AT-SPI Selection reports 0 selected children while a day is selected.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). aria-current=date reaches Windows as UIA AriaProperties 'current=date' (accesskit\_windows node.rs:466-478) but neither accesskit\_atspi\_common nor accesskit\_macos maps it (source).
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/calendar/cell.rs:276-303 (anchor not exposed); crates/teksilo-widgets/src/calendar.rs:1366-1373, 1429-1470 (silent anchor set and cancel, silent commit)
- **Evidence:**
  - `datetime-grid-keys-20260925-131909-2876039, Tab into the single calendar: 'object:state-changed:focused 1 [table cell] 'Saturday, May 2, 2026'' / 'ORCA SAYS: 'Saturday, May 2, 2026.'' / 'FAIL Orca says 'selected''; the tree holds "[table cell] 'Saturday, May 2, 2026' {selectable,selected}"`
  - `datetime-grid-keys-20260925-130330-2129926, Enter: '+36.9 ms object:state-changed:selected 1 [table cell] 'Saturday, May 30, 2026'', '+37.0 ms object:state-changed:selected 0 [table cell] 'Saturday, May 2, 2026'', no speech; orca-debug.out '13:04:30.658044 - AXUtilitiesState: [table cell: 'Saturday, May 30, 2026'] is focused but lacks state focusable'`
  - `tree run run.json: table 'Calendar, May 2026' interfaces ['Accessible', 'Component', 'Selection'], 'selected_children': 0`
  - `datetime-range-calendar-20260925-131909-2876024, Enter sets the range's start: 'FAIL a object:state-changed:selected event from [table cell] 'September 25'' - 'no object:state-changed:selected event'; Enter ends the range: '+29.2 ms object:state-changed:selected 1 [table cell] 'Saturday, September 26, 2026'' ... no speech; Tab in: 'ORCA SAYS: 'Friday, September 25, 2026.'' (today, never said)`
  - `Cause (Orca 46.1): REAL_ROLE_TABLE_CELL format has no selected state (formatting.py:499-520); 'not selected' is produced only when the cell's parent supports Selection and the table is not layout-only (speech_generator.py:1079-1095), and the cell's parent here is a table row; onSelectedChanged announces only after Space (scripts/default.py:1518-1526). AccessKit: Selection is exposed on the Grid (accesskit_consumer node.rs:938-950) but counts direct children only; no aria-current attribute on AT-SPI. Teksilo: DayCell exposes selected only for the committed value, never the range anchor (crates/teksilo-widgets/src/calendar/cell.rs:270-293).`
  - `datetime-grid-keys-20260925-133115-3261836 'Enter commits the day under the cursor': 'object:state-changed:selected 0 [table cell] 'Saturday, May 2, 2026'', 'object:state-changed:selected 1 [table cell] 'Saturday, May 30, 2026'', no speech`
  - `verify-datetime-range-escape-20260925-133314-3387389: 'Enter sets a start', 'Escape cancels the pending start' and 'Enter again' each give no bus event and no speech; the label stays 'No range selected'`
  - `accesskit_consumer-0.39.0/src/node.rs:919-935 is_item_like excludes GridCell and Row; accesskit_atspi_common-0.20.0/src/node.rs:1268-1276 n_selected_children counts items() only`
- **Reproduced:** grid-keys 2 of 2 runs, range-calendar 2 of 2 runs; deterministic
- **Verification:** corrected by the verifier. Reproduced: grid-keys 2 of 2 (132832, 133115); range-calendar 2 of 2 (132826, 133504); range Escape 1 run (133314, no events at all, deterministic) The observations reproduce. Orca never says 'selected' or 'today'. Enter commits silently (state-changed:selected 1 and 0 reach the bus). The range's first Enter changes nothing on the bus. Four corrections. (1) The empty AT-SPI Selection is not about 'direct children only'. accesskit\_consumer items() walks through non-item nodes but counts only is\_item\_like roles (node.rs:919-935: ListItem, Tab, TreeItem, option, radio...). GridCell and Row are not among them, so a Grid's Selection is always empty, however deep the cells sit (atspi\_common node.rs:1268-1276). (2) The layer is mixed. Not hearing 'selected' or 'today' is upstream: Orca's TABLE\_CELL format (formatting.py:499-520), speech\_generator.py:1079-1095 (which also needs the cell's parent to support Selection, and here the parent is a row), and no aria-current mapping in accesskit\_atspi\_common or accesskit\_macos (Windows only, node.rs:466-478). The range anchor being invisible is framework: DayCell marks selected only from the committed value (calendar/cell.rs:283-290). The anchor's silent cancel by Escape is framework too (calendar.rs:1366-1373); my verify-datetime-range-escape run shows Enter, Escape and Enter all silent, with no event. (3) With a real keyboard, Orca announces the selection after Space (scripts/default.py:1518-1526). The harness cannot show that, so 'committing is silent' is measured only for Enter. (4) Severity high stands: a reader of a date picker cannot learn which day is chosen.
- **Fix idea:** Teksilo can carry the state in words Orca does read: put 'selected' / 'today' / 'range start' in the day cell's description (Orca's unfocused suffix speaks description), expose the range anchor as selected, and announce a commit ('May 30, 2026 selected'; 'Range starts September 25'). Upstream: aria-current on AT-SPI, and Selection counting through rows.

### datetime-09 {#datetime-09}

Committing a range in DateRangeEdit's calendar tells the reader nothing about the range

- **Example:** datetime-pickers
- **Scenario:** datetime-range-edit
- **Act:** Space on Open range calendar, Enter on 2 May, Right x3, Enter.
- **The reader should get:** The calendar closes, focus goes back to the field and the reader hears the committed range (e.g. 'Saturday, May 2, 2026 to Tuesday, May 5, 2026').
- **The reader gets:** Enter on the start is silent; the final Enter closes the calendar, focus lands on the trigger button and Orca says only 'Open range calendar push button.'
- **Platform:** Linux AT-SPI / Orca 46.1 (measured); the missing announcement and focus choice are the widget's on every platform
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/date\_range\_edit.rs:471-478
- **Evidence:**
  - `datetime-range-edit-20260925-131250-2552648, Enter ends the range: '+34.5 ms object:property-change:accessible-name [label] 'Range edit: 2026-05-02 – 2026-05-05' ...', '+34.7 ms object:state-changed:focused 1 [push button] 'Open range calendar'', '+187.1 ms ORCA SAYS: 'Open range calendar push button.'' / 'FAIL Orca says 'May 5, 2026''`
  - `same run, Enter on 2 May starts a range: said=[] and no event`
  - `Cause: on_range_changed only dismisses the overlay (crates/teksilo-widgets/src/date_range_edit.rs:473-478); unlike DateEdit (date_edit.rs:685) it requests no focus back to the field and announces nothing.`
  - `datetime-range-edit-20260925-133501-3506331 'Enter ends the range': 'object:property-change:accessible-name [label] 'Range edit: 2026-05-02 – 2026-05-05'', 'object:state-changed:focused 1 [push button] 'Open range calendar'', 'ORCA SAYS: 'Open range calendar push button.''`
- **Reproduced:** 3 of 3 runs (130630, 131250, 131611)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 (datetime-range-edit 132857, 133120, 133501) Reproduced 3 of 3. The range is written correctly, the calendar closes, focus lands on 'Open range calendar', and Orca says only 'Open range calendar push button.'. The start Enter is silent. Correction: returning focus to the button that opened the picker is what the APG date-picker dialog does, so the focus target is not the defect. The defect is that nothing confirms the committed range, and nothing says a start was set (on\_range\_changed only dismisses, date\_range\_edit.rs:471-478). No data is lost and the reader heard each day as they picked it, so I lower the severity to medium. It gets worse with 06: going back to the fields reads 'End date date editor.' with no value, so the reader has no easy way to check the result.
- **Fix idea:** After the commit, return focus to the start field and announce the range with the calendar-date-range message (the words the range calendar's value already uses).

### datetime-10 {#datetime-10}

Orca treats the calendar grid as a layout table: the calendar and its month are never said on entering it

- **Example:** datetime-pickers
- **Scenario:** datetime-grid-keys
- **Act:** Tab from Theme into the single calendar; every arrow press after.
- **The reader should get:** Entering the grid, the reader hears it is a calendar of May 2026 (a table, with the day's column).
- **The reader gets:** Only the day's name. Orca judges the table layout-only because it has no Table interface, skips it as an ancestor and finds no table for any cell.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/calendar.rs:776-790
- **Evidence:**
  - `datetime-grid-keys-20260925-130330-2129926 orca-debug.out: '13:03:43.867221 - AXTable: [table: 'Calendar, May 2026'] is layout only: True (Doesn't support table interface.)', '13:03:43.867258 - SCRIPT UTILITIES: [table: 'Calendar, May 2026'] is deemed to be layout only', 'GENERATION TIME: 0.0084 ----> newAncestors=[]', '13:03:43.872526 - AXTable: Couldn't find table-implementing ancestor for [table cell: 'Saturday, May 2, 2026']'`
  - `Quantified: in each grid-keys run (130330, 131909) Orca judged the calendar table layout-only 19 times and never otherwise, and logged 'Couldn't find table-implementing ancestor' 56 times; tabwalk 132032: 8 and 8`
  - `report.txt, Tab into the single calendar: 'FAIL Orca says 'Calendar'' - 'Orca said: 'Saturday, May 2, 2026.''`
  - `tree: table 'Calendar, May 2026' interfaces ['Accessible', 'Component', 'Selection'] (no Table); Orca ax_table.py:1065-1069 'Doesn't support table interface' -> layout; script_utilities isLayoutOnly skips it in _generateAncestors (speech_generator.py:1925-2010)`
  - `datetime-grid-keys-20260925-132832-3110411 orca-debug.out: '13:28:45.171692 - AXTable: [table: 'Calendar, May 2026'] is layout only: True (Doesn't support table interface.)', '13:28:45.177139 - AXTable: Couldn't find table-implementing ancestor for [table cell: 'Saturday, May 2, 2026']'; counts 19 and 56`
- **Reproduced:** 2 of 2 grid-keys runs + tabwalk; deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 grid-keys (132832, 133115) plus tabwalk 132959; deterministic
- **Fix idea:** Upstream: implement the AT-SPI Table/TableCell interfaces in accesskit\_atspi\_common. Meanwhile Teksilo can say the context itself when focus enters the grid (an announcement 'Calendar, May 2026', or the name in the first-focused day's description).

### datetime-11 {#datetime-11}

Header arrows keep the names 'Next month'/'Previous year' in the months and years views, where they step a year or a decade

- **Example:** datetime-pickers
- **Scenario:** datetime-zoom
- **Act:** In the months view, Space on the arrow named 'Next month'.
- **The reader should get:** The button's name says what it does in this view ('Next year'), or the button steps a month.
- **The reader gets:** Focus is on 'Next month push button.'; pressing it moves the calendar a year: the announcer says '2027' and the grid becomes 'Calendar, May 2027'.
- **Platform:** All platforms (names are the widget's)
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/calendar/header.rs:83-160
- **Evidence:**
  - `datetime-zoom-20260925-131710-2787163, positioning: 'ORCA SAYS: 'Next month push button.''; Space: '+18.6 ms object:announcement [status bar] '2027' text='2027'', '+19.8 ms object:property-change:accessible-name [table] 'Calendar, May 2027' text='Calendar, May 2027'', '+26.0 ms ORCA SAYS: '2027''`
  - `Cause: arrow names resolved once in build (crates/teksilo-widgets/src/calendar/header.rs:84-87) while step_single/step_double step 12 or 120 months in the months/years views (header.rs:112-121, 146-150).`
  - `datetime-zoom-20260925-132725-3056683 'Space on the arrow named Next month, in the months view': '+27.0 ms object:property-change:accessible-name [table] 'Calendar, May 2027'', '+27.5 ms object:announcement [status bar] '2027'', 'ORCA SAYS: '2027''`
- **Reproduced:** 2 of 2 zoom runs (130331, 131710)
- **Verification:** confirmed. Reproduced: 3 of 3 (datetime-zoom 132725, 133147, 133622)
- **Fix idea:** Bind the arrows' labels to the mode signal (Previous/Next year in the months view, Previous/Next decade and century in the years view).

### datetime-12 {#datetime-12}

The documented T key (jump to today) never fires from a real keyboard

- **Example:** datetime-pickers
- **Scenario:** datetime-grid-keys
- **Act:** In the single calendar's grid, press T.
- **The reader should get:** The cursor moves to today and the reader hears 'Friday, September 25, 2026'.
- **The reader gets:** Nothing happens: no focus change on the bus, nothing spoken.
- **Platform:** All platforms (key matching); measured on Linux
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/calendar.rs:1377
- **Evidence:**
  - `datetime-grid-keys-20260925-131909-2876039, T jumps to today: 'steps: key t' / 'FAIL focus lands on [table cell] 'Friday, September 25, 2026'' - 'no focus change on the bus in this act'`
  - `Cause: crates/teksilo-widgets/src/calendar.rs:1378 matches Key::Character('t' | 'T'), but teksilo-platform translates the T key to Key::T (crates/teksilo-platform/src/event_translation.rs:1446), so the branch is unreachable from a keyboard.`
  - `datetime-grid-keys-20260925-133115-3261836 'T jumps to today': 'no focus change on the bus in this act', said=[]`
- **Reproduced:** 2 of 2 grid-keys runs (plus the previous attempt's run); deterministic
- **Verification:** corrected by the verifier. Reproduced: 2 of 2 (datetime-grid-keys 132832, 133115) Real and deterministic. A real 't' press causes no focus change and no speech. calendar.rs:1377 matches Key::Character('t' \| 'T'), but translate\_key maps a letter to Key::T (teksilo-platform event\_translation.rs:1446), so that branch cannot be reached from a keyboard. The module docs advertise it (calendar.rs:40). I lower the severity to low. The key is published to no assistive technology (no key-shortcut property), so a reader loses no information. The same cursor move is available through the arrows, PageUp/PageDown and Ctrl+Home. It is a dead documented shortcut for every keyboard user, not a reader-specific loss.
- **Fix idea:** Match Key::T (with no accelerator modifier) instead of Key::Character.

### datetime-13 {#datetime-13}

Zooming to the months view is heard only as '2026', and the months are twelve Tab stops starting at January

- **Example:** datetime-pickers
- **Scenario:** datetime-zoom
- **Act:** Space on the title button 'May 2026'; Tab x3 into the months; Tab again.
- **The reader should get:** The reader hears that the months of 2026 are showing; the months are one Tab stop entered on the shown month (May).
- **The reader gets:** Only '2026' (the focused button's rename). Tab lands on 'January.', and Tab again on 'February.' (every month is a Tab stop).
- **Platform:** All platforms for the tab stops; speech measured on Linux / Orca
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `2d0446fc` (dateedit). Fixed part: zoom-out moves focus into the grid and is heard as the month with its year ('May 2026'); the months are one Tab stop (the grid's.
- **Where:** crates/teksilo-widgets/src/calendar/zoom\_grid.rs:355-381; crates/teksilo-widgets/src/calendar/header.rs:199-216
- **Evidence:**
  - `datetime-zoom-20260925-130331-2131260, Space on the title: '+60.5 ms object:property-change:accessible-name [push button] '2026' text='2026'' / '+98.6 ms ORCA SAYS: '2026'' / 'FAIL Orca says 'month''; Tab x3: '+1302.7 ms object:state-changed:focused 1 [table cell] 'January'' / 'ORCA SAYS: 'January.''`
  - `datetime-zoom-20260925-131710-2787163, Tab from the month: '+33.4 ms object:state-changed:focused 1 [table cell] 'February''`
  - `Cause: every ZoomCell is .focusable(enabled) (crates/teksilo-widgets/src/calendar/zoom_grid.rs:363); the grid's name 'Months' (zoom_grid.rs:163-164) is never spoken (layout table, see datetime-10) and nothing is announced on the mode change (header.rs:209-216).`
  - `datetime-zoom-20260925-133147-3283236: 'Space on the title button' said=['2026']; 'Tab three times' ends 'object:state-changed:focused 1 [table cell] 'January''; 'Tab from the month' 'object:state-changed:focused 1 [table cell] 'February''`
- **Reproduced:** 2 of 2 zoom runs
- **Verification:** confirmed. Reproduced: 3 of 3 (datetime-zoom 132725, 133147, 133622)
- **Fix idea:** Roving focus over the zoom cells (one Tab stop, entered on the shown month or year), and move focus into the grid (or announce 'Months, 2026') when the title zooms out.

### datetime-14 {#datetime-14}

Opening a date field's calendar is not presented as a popup; its content hangs under an unnamed 'unknown' node at the top of the window

- **Example:** datetime-pickers
- **Scenario:** datetime-dateedit-stale, datetime-dateedit-popover
- **Act:** Alt+Down in the DateEdit field.
- **The reader should get:** The reader learns a calendar popup opened (a dialog or named group 'Calendar, May 2026'), placed after the field in reading order; the field says it has a popup and whether it is expanded.
- **The reader gets:** Only 'Saturday, May 2, 2026.'. The popover's root on the bus is '\[unknown\] ''' as the frame's first child (before the toolbar), holding the table. has-popup/expanded set by DateEdit reach no AT-SPI state, and they sit on the container, not on the 'Open calendar' button focus is on.
- **Platform:** Linux AT-SPI / Orca measured; the unknown node exists on every adapter (Role::Unknown is not filtered by common\_filter); has-popup/expanded missing on AT-SPI is upstream
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/date\_edit.rs:1110-1121; crates/teksilo-core/src/deferred\_subtree.rs:222-226; crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:213-236
- **Evidence:**
  - `datetime-dateedit-stale-20260925-130553-2236815, Alt+Down: '+66.2 ms object:children-changed:add [frame] '' -> [unknown] ''' / 'ORCA SAYS: 'Saturday, May 2, 2026.''`
  - `same run tree-Alt-Down-opens-the-calendar.txt: "[frame] '' {active}" / "  [unknown] ''" / "    [table] 'Calendar, May 2026' {focusable}"`
  - `datetime-dateedit-popover-20260925-131250-2552592: 'FAIL Orca says 'Calendar'' on opening`
  - `Cause: once built, DeferredSubtree::accessibility sets nothing (crates/teksilo-core/src/deferred_subtree.rs:222-226), leaving AccessKit's default Role::Unknown; date_edit.rs:1111-1112 put set_has_popup/set_expanded on the DateInput container; accesskit_atspi_common node.rs:301-380 maps neither to an AT-SPI state.`
  - `datetime-dateedit-stale-20260925-132653-3020512 tree-Alt-Down-opens-the-calendar.txt: "[frame] '' {active}" / "  [unknown] ''" / "    [table] 'Calendar, May 2026' {focusable}"`
  - `verify-datetime-reopen-others-20260925-133244-3345824 tree-Space-opens-the-DateRangeEdit-calendar.txt: "[frame] '' {active}" / "  [table] 'Calendar, May 2026'" (no wrapper); tree-Space-opens-the-DateTimeEdit-calendar.txt: the table is a child of "[date editor] 'Date and time'"`
  - `Source: date_edit.rs:1117-1121 push_controlled(widget_id_to_node_id(cal_id)), where cal_id is the add_detached_deferred host; accessibility_emit_impl.rs:219-236 relation_targets excluded from prunable`
- **Reproduced:** 3 of 3 DateEdit-open runs (dateedit-stale) and 4 of 4 popover runs; deterministic
- **Verification:** corrected by the verifier. Reproduced: DateEdit \[unknown\] wrapper in every opening (3 of 3 dateedit-stale, 3 of 3 dateedit-popover, 3 of 3 months-dataloss). DateRangeEdit bare table at the frame's top 3 of 3 (reopen-others). DateTimeEdit inside its date editor 3 of 3. The observation holds for DateEdit: an '\[unknown\] ''' node is the frame's first child, before the toolbar, and holds the table. Nothing tells the reader a popup opened. The cause needs correcting. A materialized DeferredSubtree does set nothing (deferred\_subtree.rs:222-226), but a bare Unknown node is normally pruned as presentational. It survives here only because DateEdit's controls relation targets that DeferredSubtree host (date\_edit.rs:1117-1121), and relation targets are exempt from pruning (teksilo-core accessibility\_emit\_impl.rs:213-236). So the controls relation (AT-SPI CONTROLLER\_FOR) also points at an empty unknown node instead of the grid. DateRangeEdit, which sets no controls relation, has no wrapper: its table is added straight to the frame, also at the top of the window. DateTimeEdit's calendar (add\_deferred, not detached) sits inside its 'Date and time' date editor. has-popup and expanded have no AT-SPI mapping at all (no expanded or popup mapping in accesskit\_atspi\_common). Severity medium stands.
- **Fix idea:** Have DeferredSubtree report Role::GenericContainer once materialized (so every adapter drops it), give the popover content a dialog/group role named by the calendar, and put has-popup/expanded on the trigger button as well.

### datetime-15 {#datetime-15}

The DateTimeEdit and DateRangeEdit wrappers expose their halves' text run together with no separator

- **Example:** datetime-pickers
- **Scenario:** (tree)
- **Act:** Read the tree (flat review / where-am-I on the wrapper).
- **The reader should get:** '05/02/2026 02:35 PM' and '05/02/2026 to 05/16/2026'.
- **The reader gets:** text='05/02/202602:35 PM' and text='05/02/202605/16/2026' on the wrappers' Text interface.
- **Platform:** Linux AT-SPI (measured); the consumer's text ranges are shared by all adapters
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/date\_time\_edit.rs:906-945; crates/teksilo-widgets/src/date\_range\_edit.rs:738-760
- **Evidence:**
  - `tree-datetime-pickers-20260925-125905-2013173 tree-launch.txt: "[date editor] 'Date and time' {editable,focusable,selectable-text,single-line} text='05/02/202602:35 PM'" and "[date editor] 'Date range' ... text='05/02/202605/16/2026'"`
  - `Cause: the wrappers' text-input roles make accesskit_consumer build a text range over every descendant text run (text.rs:1402-1406); the separators are painted or a11y-hidden (date_time_edit.rs:586-589).`
  - `tree-datetime-pickers-20260925-132940-3172543 tree-launch.txt: "[date editor] 'Date and time' {editable,focusable,selectable-text,single-line} text='05/02/202602:35 PM'"`
- **Reproduced:** tree run; deterministic
- **Verification:** confirmed. Reproduced: deterministic (tree 132940; every tree taken)
- **Fix idea:** Give the wrapper a non-text role (Group) carrying the value, or emit the separator as a text run between the halves.

### datetime-16 {#datetime-16}

Inside a calendar, Tab reaches the day grid before the header buttons above it

- **Example:** datetime-pickers
- **Scenario:** (tabwalk)
- **Act:** Tab through the example.
- **The reader should get:** Tab order follows the visual and tree order: header buttons, then the grid, then Today (the APG date-picker order).
- **The reader gets:** Grid day first ('Saturday, May 2, 2026.'), then Previous year, Previous month, title, Next month, Next year, Today.
- **Platform:** All platforms
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/calendar.rs:690-698
- **Evidence:**
  - `tabwalk-datetime-pickers-20260925-132032-2912751: Tab 11 '+29.8 ms object:state-changed:focused 1 [table cell] 'Saturday, May 2, 2026'', Tab 12 '+30.3 ms object:state-changed:focused 1 [push button] 'Previous year'' ... Tab 17 'Today'`
  - `Cause: the Calendar root itself is the grid's Tab stop (calendar.rs:678-681 .focusable(enabled)) and precedes its descendants.`
  - `tabwalk-datetime-pickers-20260925-132959-3197203: Tab 11 'object:state-changed:focused 1 [table cell] 'Saturday, May 2, 2026'', Tab 12 '[push button] 'Previous year'' ... Tab 17 '[push button] 'Today''`
- **Reproduced:** tabwalk 1 run; deterministic
- **Verification:** confirmed. Reproduced: 1 tabwalk run (132959) plus the positioning Tabs of every scenario; deterministic
- **Fix idea:** Order the grid's Tab stop after the header (a tab-index hint), or document the deviation.

### datetime-17 {#datetime-17}

Every date/time editor carries an empty unnamed status bar, and Open calendar repeats its name as its description

- **Example:** datetime-pickers
- **Scenario:** (tree)
- **Act:** Read the tree (object navigation).
- **The reader should get:** No empty landmarks in the reading path; no description equal to the name.
- **The reader gets:** "\[status bar\] ''" under each of the five editors; "\[push button\] 'Open calendar' desc='Open calendar'" and 'Open range calendar' likewise. Orca dedups the description on focus; other readers may read it twice.
- **Platform:** Linux AT-SPI (measured)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/primitives/validation\_strip.rs:147-163; crates/teksilo-widgets/src/date\_edit.rs:777-787
- **Evidence:**
  - `tree-launch.txt: "[status bar] ''" (5 times), "[push button] 'Open calendar' desc='Open calendar' {focusable}"`
  - `Cause: the ValidationStrip keeps Role::Status with no name while pristine (crates/teksilo-widgets/src/primitives/validation_strip.rs:149-163); the trigger's tooltip text is copied to both name and description.`
  - `tree-datetime-pickers-20260925-132940-3172543 tree-launch.txt: "[status bar] ''" under each of the five editors`
- **Reproduced:** tree run; deterministic
- **Verification:** confirmed. Reproduced: deterministic (tree 132940)
- **Fix idea:** Hide the strip from AT while it has nothing to say (keep a live node only when a message exists, taking K2's defunct lesson into account); skip the tooltip description when it equals the name.

### datetime-v01 {#datetime-v01}

Tab or Shift+Tab out of a date field's open calendar sends the reader to the far end of the window, not back beside the field

- **Example:** datetime-pickers
- **Scenario:** verify-datetime-popover-tab, verify-datetime-popover-shifttab
- **Act:** DateEdit: Alt+Down, then Tab six times (grid, 5 header buttons, out). Separately: Alt+Down, then Shift+Tab from the grid.
- **The reader should get:** The popup is a disclosure (Teksilo's own model: Tab leaves it, focus.rs:104-114). Focus should leave to the stop next to the field it belongs to: the 24 h time field on Tab, the DateEdit's own Open calendar button or entry on Shift+Tab.
- **The reader gets:** Tab from the last header button closes the popup and lands on the toolbar's 'Theme' combo box, the first stop of the window's Tab cycle. Shift+Tab from the grid closes it and lands on 'Open range calendar', the DateRangeEdit's button, eight stops from the DateEdit, which the reader can mistake for their own field's button.
- **Platform:** All platforms (Tab order is the framework's); measured on Linux AT-SPI / Orca 46.1
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-core/src/widget\_tree/focus\_impl.rs:437-470 (cycle\_focus); crates/teksilo-widgets/src/date\_edit.rs:704
- **Evidence:**
  - `verify-datetime-popover-tab-20260925-133552-3573378, Tab 6: 'object:children-changed:remove [frame] '' -> [unknown] ''', 'object:state-changed:focused 1 [combo box] 'Theme'', 'ORCA SAYS: 'Toolbar tool bar'', 'ORCA SAYS: 'Theme combo box.''`
  - `verify-datetime-popover-shifttab-20260925-133721-3644612, Shift+Tab 1: 'object:state-changed:focused 1 [push button] 'Open range calendar'' / 'ORCA SAYS: 'Open range calendar push button.''`
  - `The main cycle, from the tabwalk: ... Tab 9 'Open range calendar', Tab 10 'Theme' ... So the popup's stops sit at the wrap point of the whole window's cycle, after the last editor and before the toolbar, not after the DateEdit.`
  - `Cause: WidgetTree::cycle_focus collects Tab stops root by root (crates/teksilo-core/src/widget_tree/focus_impl.rs:445-466). Only sticky tooltips are spliced in after their anchor (splice_sticky_tooltips_after_anchors, focus_impl.rs:491+). The DateEdit calendar is added with add_detached_deferred (date_edit.rs:704), so its stops sit outside the field's place in the order. DateRangeEdit's calendar is detached the same way (date_range_edit.rs:493); not run.`
- **Reproduced:** Tab: 3 of 3 (verify-datetime-popover-tab 133317, 133552, 133726). Shift+Tab: 2 of 2 (verify-datetime-popover-shifttab 133721, 133755).
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Splice a non-modal overlay's Tab stops right after its anchor's stop, as splice\_sticky\_tooltips\_after\_anchors already does for sticky tooltips (the disclosure panel follows its button). Or, on Tab out, dismiss and move focus to the stop after the anchor.

### datetime-v02 {#datetime-v02}

Stepping DateRangeEdit's start past its end silently swaps the two dates under the caret

- **Example:** datetime-pickers
- **Scenario:** verify-datetime-rangeedit-stale
- **Act:** Tab to the DateRangeEdit start date (05/02/2026, end 05/16/2026), press Up on the year.
- **The reader should get:** The start becomes 05/02/2027 and the reader hears it. If that puts the start after the end, the widget either keeps the edit and reports the range invalid, or says what it did.
- **The reader gets:** The halves are reordered without a word. The start field, where the caret still is, now reads 05/16/2026 (the old end) and the end field 05/02/2027. The stepped text '05/02/2027' never reaches the start field on the bus: its diff goes straight from 05/02/2026 to 05/16/2026. Orca says nothing, and the next Up steps a different date from the one the reader thinks they are editing.
- **Platform:** All platforms (widget logic); measured on Linux AT-SPI / Orca 46.1
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/date\_range\_edit.rs:813-834
- **Evidence:**
  - `verify-datetime-rangeedit-stale-20260925-133238-3336374 'Up in the start date (year)': 'object:text-changed:delete [date editor] 'End date' text='16/2026'', 'object:text-changed:insert [date editor] 'End date' text='02/2027'', 'object:text-changed:delete [date editor] 'Start date' text='02'', 'object:text-changed:insert [date editor] 'Start date' text='16'', 'object:property-change:accessible-name [label] 'Range edit: 2026-05-16 – 2027-05-02''; no ORCA SAYS`
  - `Cause: the half's merge builds the outer value with DateRange::new(s, e) (crates/teksilo-widgets/src/date_range_edit.rs:817-826), which orders the pair (calendar.rs:161-167). The ordered range is then written back into both halves. Nothing is announced (05).`
- **Reproduced:** 2 of 2 (verify-datetime-rangeedit-stale 133238, 133546); deterministic
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Do not reorder under the caret: keep each half's own date and mark the range invalid (the ValidationStrip exists for that), or announce the swap in words.

### datetime-v03 {#datetime-v03}

The calendar's Role::Grid node holds its header buttons, a separator and the Today button, and declares no row or column count

- **Example:** datetime-pickers
- **Scenario:** (tree)
- **Act:** Read the tree (object navigation, or any future table navigation).
- **The reader should get:** A grid's children are its rows (a column-header row, then the week rows), with row and column counts. The header and Today buttons sit outside the grid, inside a named group or dialog.
- **The reader gets:** \[table\] 'Calendar, May 2026' has as direct children 5 push buttons, 7 table rows, a separator and 'Today'. No row or column count is set anywhere in the calendar. It costs nothing now, because no adapter implements a Table or Grid interface (10). It would make the table malformed the day AccessKit exposes one, which is 10's upstream fix.
- **Platform:** All adapters (node structure); seen on Linux AT-SPI
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/calendar.rs:776-790
- **Evidence:**
  - `tree-datetime-pickers-20260925-132940-3172543 tree-launch.txt: "[table] 'Calendar, May 2026' {focusable}" / "[push button] 'Previous year'" ... "[table row] ''" x7 / "[separator] ''" / "[push button] 'Today' {focusable}"`
  - `Cause: Calendar::accessibility sets Role::Grid on the calendar root (crates/teksilo-widgets/src/calendar.rs:776-779), whose subtree is header + weekday row + weeks + footer; grep finds no set_row_count or set_column_count under calendar*`
- **Reproduced:** deterministic (every tree)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Put Role::Grid (with row\_count 7 and column\_count 7) on the node that holds only the weekday and week rows; make the calendar root a named Group.
