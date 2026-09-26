<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Radio tiles and close confirmation

Examples: `radio-tile`, `close-confirmation`.
7 findings: 3 high, 1 medium, 3 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [radioclose-01](#radioclose-01) | close-confirmation | A close requested by the compositor (close button, window menu) shows no confirmation until some later, unrelated input event | high | Linux | open |
| [radioclose-02](#radioclose-02) | close-confirmation | Checkbox check-state changes never reach the accessibility tree until something else (a focus move) re-walks it | high | Linux | fixed |
| [radioclose-03](#radioclose-03) | close-confirmation | Each further close request stacks another identical confirmation dialog | medium | Linux | open |
| [radioclose-04](#radioclose-04) | close-confirmation | The message box title is spoken twice as the dialog opens, the first time cut after a syllable | low | Linux | open |
| [radioclose-05](#radioclose-05) | radio-tile | Radio group membership is not exported on AT-SPI: Orca repeats the group name on every arrow, and its where-am-I has no 'N of M' | low | Linux | upstream |
| [radioclose-06](#radioclose-06) | radio-tile | Each group's name is also a separate visible label just before it, so object navigation reads it twice | low | Linux | open (example) |
| [radioclose-M1](#radioclose-m1) | close-confirmation | The in-tree message box is modal only to the pointer and Tab: AT-SPI click and grab\_focus reach the window's controls behind it, which stay in the tree, focusable and enabled | high | Linux | partly fixed |

### radioclose-01 {#radioclose-01}

A close requested by the compositor (close button, window menu) shows no confirmation until some later, unrelated input event

- **Example:** close-confirmation
- **Scenario:** radioclose-close-escape, radioclose-close-discard, radioclose-close-save, radioclose-close-space, radioclose-close-twice, radioclose-sugar-blocked
- **Act:** With the document dirty and focus on the checkbox, close the window through KWin (run.close\_window(), as the close button does); for the sugar window, close only that window through KWin
- **The reader should get:** The guard vetoes the close and the Save/Discard/Cancel (or Yes/No) message box appears at once: focus on Save (No), and Orca says 'alert Close window?', the text, 'Save push button.'
- **The reader gets:** Nothing happens for as long as nothing else happens: no event on the bus and no speech for the whole 6 s act (3-4 s in the twice/space variants). The dialog appears only on the next input event of any kind (pressing Shift here). A reader who closed the window hears silence and has no sign the close was refused. A second close request in the meantime stacks a second dialog (see radioclose-03). The in-app route (Space on 'Close window (ctx.close\_window)') is not affected: its dialog appears within ~45 ms.
- **Platform:** Linux AT-SPI/Orca 46.1, measured. The cause is in platform-independent teksilo-app code, so the Windows/macOS OS close routes run the same code by source; there the dialog appears only if some other event follows (a key-up after Alt+F4/Cmd+W, the pointer re-entering the client area). Not measured there.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-app/src/app.rs:959-972 (post\_event order), app.rs:2486-2493 (CloseRequested), app.rs:3169-3185 (about\_to\_wait); crates/teksilo-app/src/window\_manager.rs:1390-1401, 1422-1480 (process\_pending / evaluate\_close\_guard)
- **Evidence:**
  - `radioclose-close-escape-20260925-141941-250141 report.txt: "== close the window as its close button does" / "steps: close the window through KWin" / "FAIL  focus lands on [push button] 'Save'" / "no focus change on the bus in this act"`
  - `same run, notes: "note: 'close the window as its close button does': no dialog reached the bus in 6 s; pressed Shift to wake the app"`
  - `same run, next act "== press Shift, which does nothing, to wake the app": "+14.3 ms object:announcement [alert] 'Close window?' text='Close window?'"`
  - `radioclose-close-escape-20260925-135845-4113830, Shift act: "+19.2 ms object:announcement [alert] 'Close window?' text='Close window?'" / "+20.2 ms object:children-changed:add [frame] '' -> [alert] 'Close window?'" / "+20.4 ms object:state-changed:focused 1 [push button] 'Save'" / "+117.7 ms ORCA SAYS: 'alert Close window?'"`
  - `radioclose-sugar-blocked-20260925-140717-70217: "steps: close the window 'can_close' through KWin" / "FAIL  focus lands on [push button] 'No'" / "no focus change on the bus in this act"; then the Shift act: "+20.2 ms object:announcement [alert] 'Close this window?' text='Close this window?'"`
  - `contrast, in-app route, radioclose-close-title-20260925-140258-4176546: "steps: key space" / "+45.0 ms object:announcement [alert] 'Close window?' text='Close window?'"`
  - ``crates/teksilo-app/src/app.rs:2486-2493: `WindowEvent::CloseRequested` only calls `self.wm.request_close(fid)`; the handler ends in `post_event` (app.rs:2839)``
  - ``crates/teksilo-app/src/app.rs:959-972 `post_event`: `let had_modal_requests = self.process_modal_requests(event_loop);` (line 970) runs BEFORE `self.process_pending(event_loop);` (line 972). `process_pending` -> window_manager.rs:1390-1393 -> `evaluate_close_guard` runs the guard, which calls `present_message_box` -> `present_modal`; that request is queued after the drain and nothing requests a redraw, so it waits for the next event. `about_to_wait` (app.rs:3169-3185) calls `process_pending` only.``
  - ``Why the button route works: `ctx.close_window()` goes through `drain_close_window_requests` (window_manager.rs:1945-1965), whose `true` makes `post_event` call `request_redraw_all()`; the redraw's own `post_event` drains the modal. The OS route sets no such flag.``
  - `verify-radioclose-hold-20260925-143305-508149 report.txt: 'note: hold: an [alert] was added during the 15 s wait: False' / act 'close through KWin, then nothing for 15 s': 'pass  no object:children-changed:add event from [frame] '*'' / 'pass  no object:announcement event' / 'pass  the tree holds no [alert] '*''`
  - `same run, 'press Shift, which does nothing': '+16.3 ms object:announcement [alert] 'Close window?' text='Close window?'' / '+17.3 ms object:children-changed:add [frame] '' -> [alert] 'Close window?'' / '+85.1 ms ORCA SAYS: 'alert Close window?''`
  - `radioclose-close-escape-20260925-142516-438728: 'note: 'close the window as its close button does': no dialog reached the bus in 6 s; pressed Shift to wake the app'; Shift act '+13.9 ms object:announcement [alert] 'Close window?''`
  - `radioclose-sugar-blocked-20260925-143258-564379: both closes 'no dialog reached the bus in 6 s; pressed Shift to wake the app'`
  - `verify-radioclose-space-leak-20260925-143350-508149: 'note: space-leak: an [alert] came by itself in 3 s: False'; after Escape: 'ORCA SAYS: 'Document has unsaved changes check box checked.'' / 'pass  the checkbox reads checked on the bus'; second close + Shift: 'pass  the tree holds [alert] 'Close window?''`
- **Reproduced:** 11 of 11 compositor-route closes on a dirty document (close-escape x3, close-discard x1, close-save x1, close-space x1, close-twice x3 first close, sugar-blocked x2 closes) held the dialog until the next input event. Not timing-dependent: follows from the call order.
- **Verification:** confirmed. Reproduced: 16 of 16 dirty closes asked for through KWin (run.close\_window / close\_only) held the dialog until the next input event: close-escape x3, sugar-blocked x2 (2 closes each), hold x1 (15 s with no input), space-leak x1 (2 closes), close-twice x2, twice-both x1, close-discard x1, close-save x1, close-space x1. Not timing-dependent: it follows from the call order.
- **Fix idea:** In `post_event`, run `process_pending` (the close guards) before `process_modal_requests`, or drain modal requests again after `process_pending` (setting `had_modal_requests` so a redraw is requested). Also do this in `about_to_wait`, and on the `CloseWindowRequest` (custom title bar) `user_event` path.

### radioclose-02 {#radioclose-02}

Checkbox check-state changes never reach the accessibility tree until something else (a focus move) re-walks it

- **Example:** close-confirmation
- **Scenario:** radioclose-checkbox, radioclose-close-clean
- **Act:** Tab to 'Document has unsaved changes' (checked), press Space; press Space again; do an AT-SPI 'click' on it; then Tab away and Shift+Tab back
- **The reader should get:** Each toggle emits object:state-changed:checked, Orca says 'not checked' / 'checked', and the bus reports the new state
- **The reader gets:** Space and AT-SPI click both really toggle the flag: in close-clean the next close quits with no dialog, so `dirty` was false. But no object:state-changed:checked event is emitted, Orca says nothing, and the checkbox keeps reading its old state ('checked') on the bus. The change surfaces only when focus leaves: the stale 'checked 0' event arrives together with the focus move, and Orca's 'not checked' is cut by the new focus. In this example that checkbox decides whether closing loses work, and the reader is told the opposite of the truth.
- **Platform:** Linux AT-SPI/Orca 46.1, measured. The stale value is in the AccessKit tree Teksilo publishes (no TreeUpdate carries the new `toggled`), so by Teksilo's source UIA and macOS get the same stale state. Not measured there.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Where:** crates/teksilo-widgets/src/checkbox.rs:362-420, 535-548, 644-670; crates/teksilo-core/src/widget\_tree/layout\_impl.rs:92-106
- **Evidence:**
  - `radioclose-checkbox-20260925-141232-134730 report.txt, "== Space: uncheck": "FAIL  a object:state-changed:checked event from [check box] '*'" / "no object:state-changed:checked event from [check box] '*'" / "FAIL  Orca says 'not checked'" / "FAIL  the checkbox reads not checked on the bus" / "states=['checkable', 'checked', 'enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']"`
  - `same run, "== AT-SPI click: uncheck": "+7.0 ms == harness:action click [check box] 'Document has unsaved changes'" then the same three FAILs, "states=['checkable', 'checked', ...]"`
  - `same run, "== Tab away and Shift+Tab back": "+4.4 ms object:state-changed:checked 0 [check box] 'Document has unsaved changes'" / "+4.6 ms object:state-changed:focused 1 [push button] 'Close window (ctx.close_window)'" / "+15.8 ms ORCA SAYS (CUT): 'not checked'"`
  - `orca-debug.out of that run: "14:12:55.555071 - SPEECH OUTPUT: 'not checked'" then "14:12:55.592996 - NULL SPEECH: stop" then "14:12:55.593128 - SPEECH OUTPUT: 'Close window (ctx.close_window) push button.'"`
  - `radioclose-close-clean-20260925-140457-4176546: "== Space: clear the checkbox" -> "FAIL  a object:state-changed:checked event from [check box] '*'"; next act "close the window as its close button does" -> "COULD NOT RUN:  the application exited (0)" (the guard saw dirty=false and let the close through)`
  - ``crates/teksilo-widgets/src/checkbox.rs:362-420 `build` binds the check state only through the style body's repaint (`style_state` map); :535-548 the KeyUp path toggles via `kind_key.toggle()`; :644-670 `accessibility` reads `check_state()`. checkbox.rs has 0 `bind_to` and 0 `request_accessibility_update` calls``
  - ``crates/teksilo-core/src/widget_tree/layout_impl.rs:92-106: "the unconditional `a11y_dirty = true` was removed from `layout()`" (commit 3a1d372a, 2026-09-06). A repaint no longer re-walks AT; only AccessibilityOnly bindings, activation changes, focus etc. do (accessibility_impl.rs:25-61)``
  - ``Contrast crates/teksilo-widgets/src/radio_tile.rs:430-436, which binds `selected` at `BindingLevel::AccessibilityOnly` for exactly this reason``
  - `radioclose-checkbox-20260925-142857-508149 'Space: uncheck': 'FAIL  a object:state-changed:checked event from [check box] '*'' / 'no object:state-changed:checked event from [check box] '*'' / 'FAIL  the checkbox reads not checked on the bus' / 'states=['checkable', 'checked', 'enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']'`
  - `same run, 'Tab away and Shift+Tab back': '+3.8 ms object:state-changed:checked 0 [check box] 'Document has unsaved changes'' / '+4.1 ms object:state-changed:focused 1 [push button] 'Close window (ctx.close_window)'' / '+16.4 ms ORCA SAYS (CUT): 'not checked''; orca-debug.out '14:29:21.040350 - SPEECH OUTPUT: 'not checked'' then '14:29:21.074665 - NULL SPEECH: stop'`
  - `verify-radioclose-sugar-toggle-20260925-144637-923487 'Space: unlock': 'FAIL  a object:state-changed:checked event from [check box] '*'' / 'FAIL  Orca says 'not checked'  Orca unheard'; then 'close the sugar window through KWin': '+11.9 ms object:children-changed:remove [application] 'close-confirmation' -> [frame] ''' (closed with no question); same in -144841-923487`
  - `radioclose-close-clean-20260925-143441-564379: 'FAIL  a object:state-changed:checked event from [check box] '*'' then 'COULD NOT RUN:  the application exited (0)'`
- **Reproduced:** 4 of 4 runs (radioclose-checkbox x3 with Space and AT-SPI click each time, plus close-clean x1). Not timing-dependent.
- **Verification:** confirmed. Reproduced: 7 of 7 runs: radioclose-checkbox x2 (Space twice and an AT-SPI click each run, none emitting a checked event), close-clean x1 (no event, then the close quit with no dialog), verify-radioclose-sugar-toggle x2 on the second Checkbox 'Locked against closing' (no event, no speech, then the close went through with no question; a third run died on the harness's winit panic), and verify-radioclose-behind-modal-box x3 (an AT-SPI click toggled it with no event). Not timing-dependent.
- **Fix idea:** In Checkbox::build, bind the check-state signal (both TwoState and TriState) at BindingLevel::AccessibilityOnly, as RadioTile does. toggle.rs and radio\_button.rs also have 0 `bind_to` calls in source and very likely share the defect (not measured). A framework test that toggles a Checkbox and asserts the next `sync_accessibility` update carries the new `toggled` would pin it.

### radioclose-03 {#radioclose-03}

Each further close request stacks another identical confirmation dialog

- **Example:** close-confirmation
- **Scenario:** radioclose-close-twice, radioclose-close-again
- **Act:** (a) Close through KWin twice before the held dialog shows, as a user who heard nothing would; (b) open the dialog with the in-app button, then close through KWin again while it is up
- **The reader should get:** One dialog, however many times the close is asked for. One Cancel/Escape ends it and returns focus to the checkbox.
- **The reader gets:** Two 'Close window?' alerts are in the tree. Escape removes the top one, and focus lands on the Save button of the second. Orca reads 'alert Close window? … Save push button.' again, so the reader believes Cancel did nothing and has to answer twice. In (b) the second dialog arrives with focus moving Save-&gt;Save, and Orca only says 'Close window?', so the reader does not know they are now in a second dialog.
- **Platform:** Linux AT-SPI/Orca 46.1, measured. The guard re-run is platform-independent teksilo-app code.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-app/src/window\_manager.rs:1390-1401, 1422-1480; examples/close\_confirmation/src/main.rs:62-95
- **Evidence:**
  - `radioclose-close-twice-20260925-141153-134730, "== close through KWin again, as a user who heard nothing would": "+24.8 ms object:announcement [alert] 'Close window?' text='Close window?'" / "+27.1 ms object:children-changed:add [frame] '' -> [alert] 'Close window?'" / "+30.1 ms object:announcement [alert] 'Close window?' text='Close window?'" / "+31.5 ms object:children-changed:add [frame] '' -> [alert] 'Close window?'" / "FAIL  the tree holds exactly 1 [alert]" / "[alert] 'Close window?'" / "[alert] 'Close window?'"`
  - `same run, "== Escape: Cancel": "+5.4 ms object:children-changed:remove [frame] '' -> [alert] 'Close window?'" / "+5.8 ms object:state-changed:focused 1 [push button] 'Save'" / "+79.2 ms ORCA SAYS: 'alert Close window?'" / "+79.3 ms ORCA SAYS: 'Save push button.'" / "FAIL  the tree holds no [alert] '*'" / "found [alert] 'Close window?'"`
  - `radioclose-close-again-20260925-141420-215583, after a KWin close while the button-opened dialog is up, then Shift: "+7.3 ms object:announcement [alert] 'Close window?' text='Close window?'" / "+8.2 ms object:children-changed:add [frame] '' -> [alert] 'Close window?'" / "+8.4 ms object:state-changed:focused 1 [push button] 'Save'" / "+19.7 ms ORCA SAYS: 'Close window?'" / "FAIL  the tree holds exactly 1 [alert]"`
  - `crates/teksilo-app/src/window_manager.rs:1390-1401 runs the guard for every pending guarded close; evaluate_close_guard (1422-1480) has no check that the window already shows the confirmation its guard presented`
  - `examples/close_confirmation/src/main.rs:62-95: the documented veto-then-reissue guard presents a fresh MessageBox on every call`
  - `radioclose-close-again-20260925-142820-438728, 'press Shift' after the KWin close: '+8.6 ms object:announcement [alert] 'Close window?'' / '+11.6 ms object:children-changed:add [frame] '' -> [alert] 'Close window?'' / '+11.8 ms object:state-changed:focused 1 [push button] 'Save'' / '+27.3 ms ORCA SAYS: 'Close window?'' / 'FAIL  the tree holds exactly 1 [alert]'; orca-debug.out '14:28:44.769169 - DEFAULT: old focus [push button: 'Save'] believed to be same as new focus [push button: 'Save']'`
  - `verify-radioclose-twice-both-20260925-143139-564379 'Escape: first Cancel': '+6.6 ms object:state-changed:focused 1 [push button] 'Save'' / '+53.6 ms ORCA SAYS: 'alert Close window?'' / 'FAIL  the tree holds no [alert] '*''; 'Escape: second Cancel': '+5.4 ms object:state-changed:focused 1 [check box] 'Document has unsaved changes''`
  - `radioclose-close-twice-20260925-142741-438728 second KWin close: two 'object:children-changed:add [frame] '' -> [alert] 'Close window?'' at +42.7 and +55.3 ms, 'FAIL  the tree holds exactly 1 [alert]'`
- **Reproduced:** 5 of 5 runs (close-twice x3, close-again x2). Not timing-dependent.
- **Verification:** confirmed. Reproduced: 9 of 9 runs: close-twice x2, verify-radioclose-twice-both x1 (two KWin closes -&gt; two alerts, two Escapes needed), close-again x2 (KWin close while the button-opened box is up -&gt; second alert). Also verify-radioclose-behind-modal x2: an AT-SPI click on the in-app Close button behind the box stacked a second one. Not timing-dependent.
- **Fix idea:** Skip (or coalesce) a guarded close for a window that already has a modal presented by its own close guard, e.g. veto silently while that modal is up, or give the guard a 'confirmation pending' fact. At minimum document the dedupe in the veto-then-reissue idiom and do it in the example. Fixing radioclose-01 removes the main way a user triggers this, but not case (b).

### radioclose-04 {#radioclose-04}

The message box title is spoken twice as the dialog opens, the first time cut after a syllable

- **Example:** close-confirmation
- **Scenario:** radioclose-close-title (and every dialog opening in the close/sugar scenarios)
- **Act:** Space on 'Close window (ctx.close\_window)' (or any route that opens the MessageBox)
- **The reader should get:** The reader hears the dialog once: 'alert Close window?', its text, 'Save push button.' (the MessageBox doc says 'the title is the one thing announced, once')
- **The reader gets:** The AlertDialog is an assertive live region. atspi\_common emits object:announcement for its name as it is added, 1-2 ms before focus moves to Save in the same update. Orca starts 'Close window?' (interrupt=True), then stops it to present the new focus, and says 'alert Close window?' again. The reader hears a clipped 'Clo-' before the full reading. Nothing is lost, but the live region adds only this stutter, because focus always enters the box.
- **Platform:** Linux AT-SPI/Orca 46.1, measured. Windows by source: accesskit\_windows adapter.rs:248-262 raises both UIA WindowOpened and LiveRegionChanged for the dialog node as it is added, ahead of the focus event, so NVDA gets the same double presentation (not measured).
- **Severity:** low; **layer:** framework
- **Status:** Open. In the announce-focus fix topic, which did not report it fixed.
- **Where:** crates/teksilo-widgets/src/message\_box.rs:1115-1124, 1130
- **Evidence:**
  - `radioclose-close-title-20260925-140258-4176546: "+45.0 ms object:announcement [alert] 'Close window?' text='Close window?'" / "+47.0 ms object:state-changed:focused 1 [push button] 'Save'" / "+59.2 ms ORCA SAYS (CUT): 'Close window?'" / "+121.4 ms ORCA SAYS: 'alert Close window?'" / "FAIL  Orca says 'Close window?' exactly 1 time(s)"`
  - `same run, observed: "'Close window?' reached the bus 2.0 ms before the act's focus change, which Orca interrupts to read the new focus"`
  - `orca-debug.out: "14:03:25.148893 - NULL SPEECH: speak 'Close window?' interrupt=True" / "14:03:25.210987 - NULL SPEECH: stop" / "14:03:25.211179 - NULL SPEECH: speak 'alert Close window?' interrupt=False"`
  - `sugar window, radioclose-sugar-button-20260925-141023-134730: "+31.9 ms object:announcement [alert] 'Close this window?' text='Close this window?'" / "+43.4 ms ORCA SAYS (CUT): 'Close this window?'" / "+83.0 ms ORCA SAYS: 'alert Close this window?'"`
  - ``crates/teksilo-widgets/src/message_box.rs:1115-1124 (`set_live(Live::Assertive)` at 1122) together with `initial_focus_hint` (1130) moving focus to the default button in the same update; accesskit_atspi_common adapter.rs:72-77; accesskit_consumer tree.rs:640-673 (node changes before the focus event); Orca default.py ~698-705``
  - `radioclose-close-title-20260925-142857-438728: '+47.8 ms object:announcement [alert] 'Close window?'' / '+49.9 ms object:state-changed:focused 1 [push button] 'Save'' / '+65.0 ms ORCA SAYS (CUT): 'Close window?'' / '+174.3 ms ORCA SAYS: 'alert Close window?'' / 'FAIL  Orca says 'Close window?' exactly 1 time(s)'; orca-debug.out '14:29:13.758387 - NULL SPEECH: speak 'Close window?' interrupt=True' / '14:29:13.867547 - NULL SPEECH: stop' / '14:29:13.867714 - NULL SPEECH: speak 'alert Close window?' interrupt=False'`
  - `same in radioclose-close-title-20260925-142928-438728 (stop 41 ms in) and -142958-438728 (57 ms in)`
- **Reproduced:** 16 of 16 dialog openings recorded across 15 runs. The announcement always precedes the focus event by 1.0-2.4 ms in the same update.
- **Verification:** corrected by the verifier. Reproduced: 37 of 37 alert announcements recorded across my runs reached the bus 0.8-3.2 ms before a focus change in the same update. Of the 35 speak('Close window?'/'Close this window?', interrupt=True) calls in Orca's logs, 30 were stopped 33-113 ms later by the focus presentation. The rest were stacked-dialog cases (radioclose-03). The mechanism is as stated: message\_box.rs:1115-1124 sets Live::Assertive on the AlertDialog, and initial\_focus\_hint (1130) moves focus to the default button in the same update. atspi\_common emits the announcement as the node is added, and Orca default.py:698-705 calls presentationInterrupt before reading the new focus. Windows by source, verified: accesskit\_windows-0.35.0 adapter.rs:248-262 raises both UIA WindowOpened and LiveRegionChanged for the dialog as it is added. Correction to the claim: 'the reader hears a clipped Clo-' is not established. The null speech server shows only a speak followed 33-113 ms later by a stop. On a real synthesizer that is probably inaudible, or a click. What is certain is a redundant presentation, which also carries interrupt=True and stops whatever Orca was saying. The module doc (message\_box.rs:95-103, 'the title is the one thing announced, once') is true of the live event, but Orca presents the title twice. Low severity is right.
- **Fix idea:** Do not make the AlertDialog a live region when focus is going to move into it (it always does, via initial\_focus\_hint). Every reader presents a focused dialog's name and description on focus entry. Keep the live name only for a box presented without moving focus, if that case exists.

### radioclose-05 {#radioclose-05}

Radio group membership is not exported on AT-SPI: Orca repeats the group name on every arrow, and its where-am-I has no 'N of M'

- **Example:** radio-tile
- **Scenario:** radioclose-tile-row, radioclose-tile-list, radioclose-tile-grid, radioclose-tile-tree
- **Act:** Tab into a RadioTileGroup, then Right/Left/Home/End/Up/Down
- **The reader should get:** On entry the group name, then '&lt;tile&gt; selected radio button &lt;description&gt;'. On each arrow within the group, only the new tile and its state, not the group again.
- **The reader gets:** Every arrow is spoken as '&lt;Group&gt;. &lt;tile&gt;. selected radio button &lt;desc&gt;', e.g. 'Template.' 'None.' 'selected radio button' 'empty binder.'. On entry the name comes twice: 'Project format panel.' (the ancestor) and 'Project format.' (the radio group label). The tiles do declare their group (`push_to_radio_group`), but accesskit\_atspi\_common 0.20 exports no MEMBER\_OF relation. Orca's `_generateNewRadioButtonGroup` suppresses the group name only when the previous focus is among the MEMBER\_OF targets. Orca's where-am-I `_generatePositionInGroup` also counts MEMBER\_OF targets only, so it gives no position even though posinset/setsize are correct on the bus. The where-am-I part is by source reading; that key is Orca's own and was not exercised.
- **Platform:** Linux AT-SPI/Orca 46.1, measured for the repetition. Windows by source: accesskit\_windows exports UIA PositionInSet (+1) and SizeOfSet (node.rs:682-693, 1326-1327), and radio grouping is not a UIA relation, so NVDA is not affected this way. macOS publishes neither.
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** upstream: accesskit\_atspi\_common-0.20.0/src/node.rs:959-976 (Teksilo side correct at crates/teksilo-widgets/src/radio\_tile.rs:778-784)
- **Evidence:**
  - `radioclose-tile-row-20260925-135127-4005518, "== Right: Bundle": "+6.6 ms object:state-changed:focused 1 [radio button] 'Bundle'" / "+101.9 ms ORCA SAYS: 'Project format.'" / "+101.9 ms ORCA SAYS: 'Bundle.'" / "FAIL  Orca does not say 'Project format'" / "Orca said: 'Project format.'"`
  - `radioclose-tile-list-20260925-141702-250141 orca-debug.out: "14:17:18.531965 - EVENT MANAGER: object:state-changed:focused for [radio button: 'None'] in [application: 'radio-tile'] (1, 0, 0)" then "14:17:18.576387 - SPEECH OUTPUT: 'Template.'" / "14:17:18.576421 - SPEECH OUTPUT: 'None.'"; report: Home, End, Up and Down each "FAIL  Orca does not say 'Template'"`
  - `entry, tabwalk-radio-tile-20260925-134956-3988920: "+54.9 ms ORCA SAYS: 'Project format panel.'" / "+54.9 ms ORCA SAYS: 'Project format.'" / "+54.9 ms ORCA SAYS: 'Single file.'" / "+54.9 ms ORCA SAYS: 'selected radio button'"`
  - `radioclose-tile-tree-20260925-141641-250141: "FAIL  [radio button] 'Single file' has a member-of relation" / "[radio button] 'Single file' relations={}"`
  - `` ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/accesskit_atspi_common-0.20.0/src/node.rs:959-976 `relation_set` inserts only `RelationType::ControllerFor` ``
  - ``/usr/lib/python3/dist-packages/orca/speech_generator.py:1528-1554 `_generateNewRadioButtonGroup` (returns the group unless priorObj is in MEMBER_OF); generator.py:1171-1193 `_generateRadioButtonGroup` walks up to the panel; speech_generator.py:2169-2201 `_generatePositionInGroup` uses MEMBER_OF only; formatting.py:421-423 radio 'unfocused'/'basicWhereAmI' formats``
  - ``Teksilo side is right: crates/teksilo-widgets/src/radio_tile.rs:778-784 `push_to_radio_group` for every sibling``
  - `radioclose-tile-list-20260925-143118-508149 'Home: None': '+7.3 ms object:state-changed:focused 1 [radio button] 'None'' then orca-debug.out '14:31:34.738861 - SPEECH OUTPUT: 'Template.'' / '14:31:34.738879 - SPEECH OUTPUT: 'None.'' / '14:31:34.738890 - SPEECH OUTPUT: 'selected radio button'' / '14:31:34.738901 - SPEECH OUTPUT: 'empty binder.''`
  - `radioclose-tile-row-20260925-143012-508149 'Right: Bundle': '+78.6 ms ORCA SAYS: 'Project format.'' / 'FAIL  Orca does not say 'Project format''`
  - `radioclose-tile-tree-20260925-143159-508149: 'FAIL  [radio button] 'Single file' has a member-of relation' / 'relations={}'; posinset/setsize check passes`
- **Reproduced:** Deterministic: every arrow in 4 runs (tile-row x1, tile-list x2, tile-grid x1) and every group entry in the tabwalk and 6 other runs.
- **Verification:** corrected by the verifier. Reproduced: Deterministic: the group name was spoken on every arrow in tile-row x2 and tile-list x1, and tile-tree x1 shows relations={} on the tiles. Checked the sources. accesskit\_atspi\_common-0.20.0 node.rs:959-976 relation\_set inserts only ControllerFor, and Role::RadioGroup maps to Panel (node.rs:228). Orca speech\_generator.py:1528-1555 suppresses the group only when priorObj is in MEMBER\_OF, and generator.py:1171-1193 walks up to the named panel. The Teksilo side is right (radio\_tile.rs:778-784). Correction to severity: the measured effect is the group name repeated on every arrow, e.g. 'Template.' 'None.' 'selected radio button' 'empty binder.'. That is verbosity, low. The loss of 'N of M' is narrower than the finding says. Orca's focus format for a radio button (formatting.py:421-423, 'unfocused') uses positionInList, which counts siblings (script\_utilities.py:3249-3296). The tiles are the panel's only children, so with enablePositionSpeaking on Orca would say the position on arrows. Only where-am-I (basicWhereAmI -&gt; positionInGroup, speech\_generator.py:2169-2201) needs MEMBER\_OF and loses it, and I read that from source without exercising it. posinset/setsize are right on the bus. RadioButton/RadioGroup share this.
- **Fix idea:** Upstream: have accesskit\_atspi\_common's relation\_set export MemberOf from `radio_group()` (and LabelledBy/DescribedBy while there). Teksilo has no other lever on Linux; it already publishes the membership. This affects RadioButton/RadioGroup the same way. The double group name on entry (ancestor panel + radio group label) is Orca's default-script behaviour for a named panel and would remain.

### radioclose-06 {#radioclose-06}

Each group's name is also a separate visible label just before it, so object navigation reads it twice

- **Example:** radio-tile
- **Scenario:** radioclose-tile-tree (tree-radio-tile)
- **Act:** Walk the tree a reader walks (flat review / object navigation)
- **The reader should get:** The group is named once, from its visible heading
- **The reader gets:** Each section is a \[label\] 'Project format' followed by a \[panel\] 'Project format', and the same for 'Template' and 'Publication stage'. The example gives the group a copy of the heading's string instead of relating the two.
- **Platform:** Linux AT-SPI, measured in the tree. The same nodes reach every adapter.
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/radio\_tile/src/main.rs:49-59, 64, 83, 119
- **Evidence:**
  - `tree-radio-tile-20260925-134921-3980323/tree-launch.txt: "    [label] 'Project format'" / "    [panel] 'Project format' {focusable}" / "    [label] 'Template'" / "    [panel] 'Template' {focusable}" / "    [label] 'Publication stage'" / "    [panel] 'Publication stage' {focusable}"`
  - ``examples/radio_tile/src/main.rs:49-59 `section()` adds a visible TextWidget title; :64, :83, :119 `.label(lit!("Project format"))` etc. name the group with the same string``
  - `radioclose-tile-tree-20260925-143159-508149 tree: "    [label] 'Project format'" / "    [panel] 'Project format' {focusable}" / "    [label] 'Template'" / "    [panel] 'Template' {focusable}"`
- **Reproduced:** Deterministic (tree at launch, 3 runs).
- **Verification:** corrected by the verifier. Reproduced: Deterministic (radioclose-tile-tree-20260925-143159-508149 tree). The tree shows exactly this: \[label\] 'Project format' followed by \[panel\] 'Project format', and the same for Template and Publication stage. RadioTileGroup::label is AT-only (radio\_tile\_group.rs:142-146, set\_name at 608-610), and the example adds its own visible heading. Correction to the location: section() is at examples/radio\_tile/src/main.rs:47-56, and the group labels are at lines 62, 83 and 122 (the sweep gave 49-59, 64, 83, 119). Low: redundancy in object navigation, and the same pattern as a web heading plus aria-label.
- **Fix idea:** Mark the section heading `.a11y_hidden()` where the group carries the name, or name the group with `access_labelled_by(heading_id)` (the consumer derives the name from labelled\_by) and hide the heading from object navigation.

### radioclose-M1 {#radioclose-m1}

The in-tree message box is modal only to the pointer and Tab: AT-SPI click and grab\_focus reach the window's controls behind it, which stay in the tree, focusable and enabled

- **Example:** close-confirmation
- **Scenario:** verify-radioclose-behind-modal-box, verify-radioclose-behind-modal-focus, verify-radioclose-behind-modal (tools/reader/scenarios/verify\_radio\_close.py)
- **Act:** close-confirmation: open the Save/Discard/Cancel box (Space on 'Close window (ctx.close\_window)'), then act on the window's own controls behind it, as a screen reader's own activation or focus request does: (a) AT-SPI click on the checkbox, then Cancel, then close through KWin; (b) AT-SPI grab\_focus on 'Open can\_close-sugar window…', then a real Space; (c) AT-SPI click on 'Close window (ctx.close\_window)'
- **The reader should get:** While the modal box is up, the content beneath it is inert to assistive technology, as it is to the pointer and Tab: not in the tree a reader walks (or at least not actionable), focus requests outside the box refused, and actions on it ignored.
- **The reader gets:** The background controls stay in the tree beside the \[alert\] {modal}, marked {focusable}. (a) The AT-SPI click toggles the checkbox behind the box, silently because of radioclose-02. After Cancel the reader hears 'not checked', and the next close quits the app with no confirmation: in a real app the unsaved work is gone. (b) grab\_focus moves focus out of the box onto the background button (Orca reads it), and the next real Space activates it: the second window opens while the box is still up. (c) The click on the background Close button runs the guard again and stacks a second box. After an AT-SPI Cancel on the lower box, focus goes back to its opener behind the box still showing.
- **Platform:** Linux AT-SPI/Orca 46.1, measured. The box is presented in the window's own tree only on non-macOS Unix (teksilo-platform window\_system.rs:106-116; message\_box.rs:843-849 Deferred + ModalPresentation::Auto). On Windows and macOS it becomes a native modal window. Teksilo's AT action dispatch has no modality check on any platform, so an action on the blocked parent's nodes is probably dispatched there too, but that is not verified. Every in-tree modal (ModalContent::ExistingWidget, in-tree Dialogs) shares the dispatch path on all platforms. Not measured.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `e7764b0f` (modal-at). Fixed part: the actionable part (its 'or at least not actionable' clause): a click behind 'Close window?' no longer toggles the checkbox or re-runs the close guard, grab\_focus no longer leaves the box, and no second window opens behind it..
- **Where:** crates/teksilo-core/src/widget\_tree/pointer\_router.rs:1152-1200 (AccessAction dispatch); crates/teksilo-core/src/widget\_tree/focus\_impl.rs:441-449 (only Tab is confined); crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs (the walker exports the background); crates/teksilo-app/src/app.rs:155-200 (present\_in\_tree\_modal\_request)
- **Evidence:**
  - `verify-radioclose-behind-modal-box-20260925-143234-607761, 'AT-SPI click on the checkbox behind the dialog': '+4.7 ms == harness:action click [check box] 'Document has unsaved changes'' (no event); 'Escape: Cancel': '+4.5 ms object:state-changed:checked 0 [check box] 'Document has unsaved changes''; 'Shift+Tab to the checkbox': '+63.4 ms ORCA SAYS: 'Document has unsaved changes check box not checked.''; 'close through KWin': 'COULD NOT RUN:  the application exited (0)' / '+13.5 ms object:children-changed:remove [application] 'close-confirmation' -> [frame] '''`
  - `verify-radioclose-behind-modal-focus-20260925-143605-607761, 'AT-SPI grab_focus on 'Open can_close-sugar window…' behind the dialog': '+5.2 ms object:state-changed:focused 1 [push button] 'Open can_close-sugar window…'' / '+41.0 ms ORCA SAYS: 'Open can_close-sugar window… push button.''; 'Space, where focus now is': '+55.8 ms object:children-changed:add [application] 'close-confirmation' -> [frame] ''' / '+93.5 ms window:activate [frame] '''; tree-Space--where-focus-now-is.txt holds the first frame's [alert] 'Close window?' {active,focusable,modal} AND a second [frame] '' {active,focusable,focused} with 'Locked against closing'`
  - `verify-radioclose-behind-modal-20260925-143042-564379, 'AT-SPI click on 'Close window (ctx.close_window)' behind the dialog': '+21.0 ms object:announcement [alert] 'Close window?'' / '+22.6 ms object:children-changed:add [frame] '' -> [alert] 'Close window?'' / 'FAIL  the tree holds exactly 1 [alert]'; 'AT-SPI grab_focus on the checkbox behind the dialog': '+7.4 ms object:state-changed:focused 1 [check box] 'Document has unsaved changes'' / '+41.1 ms ORCA SAYS: 'Document has unsaved changes check box not checked.''; 'AT-SPI click on Cancel (1)': '+13.7 ms object:state-changed:focused 1 [push button] 'Close window (ctx.close_window)'' while the second alert is still in the tree`
  - `radioclose-close-escape-20260925-142516-438728 tree-press-Shift--which-does-nothing--to-wake-the-app.txt: "[check box] 'Document has unsaved changes' {checkable,checked,focusable}" / "[push button] 'Close window (ctx.close_window)' {focusable}" beside "[alert] 'Close window?' … {active,focusable,modal}"`
  - ``crates/teksilo-core/src/widget_tree/pointer_router.rs:1152-1162: an AccessAction is delivered to its target whenever `self.arena.is_active(id)`, with no check against the topmost modal; Action::Focus goes to focus_with_origin_ops (1180-1195) unchecked``
  - ``crates/teksilo-core/src/widget_tree/focus_impl.rs:441-449: only cycle_focus (Tab) is confined to `overlay_manager.topmost_centered()`; focus_impl.rs:2209-2238 test `a_centered_modal_is_never_dismissed_by_focus_moving` says an AccessKit action may force focus out of a modal (by design, but nothing then keeps the keyboard off the background)``
  - `docs/events-and-gestures.md:54: the modal scrim swallows pointer events through the preview pass; AT actions target the node directly and never pass the scrim`
- **Reproduced:** (a) 3 of 3 runs (verify-radioclose-behind-modal-box x3: toggled, then quit with no dialog). (b) 3 of 3 runs (verify-radioclose-behind-modal-focus x3: focus left the box, and Space opened the second window beneath it; 2 of the 3 then hit the harness's winit panic after the window opened). (c) 2 of 2 runs (verify-radioclose-behind-modal x2). Not timing-dependent.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** While a centered in-tree modal is up, (1) drop an AccessAction (click, focus, and the rest) whose target is outside the topmost modal's content and the overlays above it, reporting it unhandled, and (2) keep the background out of the AT tree, as aria-modal does in browsers: the walker could treat the window's non-modal roots as hidden while a centered modal is active. That also stops flat review reading the background. A test: present a MessageBox in a headless tree, dispatch\_access\_action(Click) on a control beneath it, and assert that nothing ran and focus stayed in the box.
