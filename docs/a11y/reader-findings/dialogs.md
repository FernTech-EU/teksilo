<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Dialogs and popovers

Examples: `dialogs-and-popovers`.
15 findings: 1 critical, 10 high, 2 medium, 2 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [dialogs-01](#dialogs-01) | dialogs-and-popovers | The custom-trigger popover ('Show popover') is not a Tab stop and cannot take focus, so no keyboard user can open it | critical | all | fixed |
| [dialogs-02](#dialogs-02) | dialogs-and-popovers | The custom-trigger Dialog ('Open dialog') puts focus on an unnamed panel: Orca says just 'panel.' when Tab reaches it and again when the dialog closes | high | all | fixed |
| [dialogs-03](#dialogs-03) | dialogs-and-popovers | Opening the popover moves focus to an unnamed role-less node outside an unnamed dialog, and Orca says nothing | high | all | fixed |
| [dialogs-04](#dialogs-04) | dialogs-and-popovers | Snackbar and popover reuse the same accessibility nodes on every showing, so after the first showing Orca drops them as defunct: the 2nd and later snackbars are silent, and focus into a reopened popover is ignored | high | Linux | partly fixed |
| [dialogs-05](#dialogs-05) | dialogs-and-popovers | The snackbar announces only the generic word 'Snackbar', and its message 'Autosave complete' is never announced | high | all | open |
| [dialogs-06](#dialogs-06) | dialogs-and-popovers | When the snackbar times out, it pulls focus back to the control that was focused when it appeared, even though the reader has moved on | high | all | fixed |
| [dialogs-07](#dialogs-07) | dialogs-and-popovers | The snackbar times out while the reader's focus is on its Dismiss button, and removes the button from under them | high | all | open |
| [dialogs-08](#dialogs-08) | dialogs-and-popovers | An in-tree modal MessageBox is not modal to assistive technology: AT-SPI click and focus reach the page behind it, and a second and third modal box stack up | high | Linux | fixed |
| [dialogs-09](#dialogs-09) | dialogs-and-popovers | The 'Don't show this again' check box in the Welcome MessageBox publishes its new state only at the next focus move, so the toggle is never heard | high | all | fixed |
| [dialogs-10](#dialogs-10) | dialogs-and-popovers | Every MessageBox title is spoken twice on opening: the live announcement is cut at once by the focus reading, which says the title again | low | Linux | open |
| [dialogs-11](#dialogs-11) | dialogs-and-popovers | No disclosure state reaches Orca: 'Show details' never says collapsed or expanded, and pressing it is silent; the Dialog and popover triggers never say expanded or has-popup | high | Linux | upstream |
| [dialogs-12](#dialogs-12) | dialogs-and-popovers | Native-window modals (Windows/macOS) drop ModalRequest::on\_dismiss: the Dialog trigger stays 'expanded' after its dialog closes, and closing a MessageBox window reports no answer | medium | Windows | open |
| [dialogs-13](#dialogs-13) | dialogs-and-popovers | The example's trigger names do not match their visible text ('Show popover' shows 'Popover actions'; 'Open dialog' shows 'Review changes') | low | all | open (example) |
| [dialogs-v-01](#dialogs-v-01) | dialogs-and-popovers | The MessageBox's 'Show details' disclosure (every Accordion) publishes its content INSIDE the push button, where Orca's flat review never looks | high | Linux | open |
| [dialogs-v-02](#dialogs-v-02) | dialogs-and-popovers | Tab out of an open popover throws focus to the first control of the window (the toolbar's Theme box), not to the control after the popover | medium | all | fixed |

### dialogs-01 {#dialogs-01}

The custom-trigger popover ('Show popover') is not a Tab stop and cannot take focus, so no keyboard user can open it

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-popover
- **Act:** Tab through the page (tabwalk, and dialogs-popover 'Tab twice from the window'); AT-SPI grab\_focus on \[push button\] 'Show popover'
- **The reader should get:** 'Show popover' is a Tab stop between the Theme box and the other triggers. Tab lands on it, Orca says 'Show popover push button', and Enter or Space opens the popover.
- **The reader gets:** Tab goes Theme -&gt; \[panel\] '' (the 'Open dialog' trigger's child) -&gt; 'Show snackbar', and 'Show popover' is never visited. The node has no focusable state, and AT-SPI grab\_focus on it changes nothing. The only way to open the popover is an AT-SPI click from object navigation (for example Orca flat review). A keyboard user, sighted or not, cannot open it.
- **Platform:** all (framework; the tree is the same in-tree popover on every platform). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `de3bb295` (popover-trigger). Fixed part: critical): 'Show popover' is now a Tab stop and an AT-SPI grab\_focus target (3 of 3 for both, Orca says 'Show popover push button.'), and Enter opens it.
- **Where:** crates/teksilo-widgets/src/overlay\_trigger.rs:208-243,291-311; crates/teksilo-widgets/src/popover\_widget.rs:879-882
- **Evidence:**
  - `tree-launch.txt: "[push button] 'Show popover'" has no {focusable} (compare "[push button] 'Show snackbar' {focusable}")`
  - `tabwalk Tab 2: "+12.7 ms object:state-changed:focused 1 [panel] ''" / ORCA SAYS 'panel.'; Tab 3: "object:state-changed:focused 1 [push button] 'Show snackbar'"`
  - `dialogs-popover 'AT-SPI grab_focus on the popover trigger': FAIL focus lands on [push button] 'Show popover' - "no focus change on the bus in this act" (2 of 2 runs)`
  - `crates/teksilo-widgets/src/overlay_trigger.rs:205-239: on_activate is routed as on_tap/on_key onto the CHILD (ctx.apply_handlers(child_id, handlers) at :238), and nothing sets .focusable(true) on either node; Role::Button, the name and Click sit on the OverlayTrigger node itself (:291-306)`
  - `crates/teksilo-widgets/src/popover_widget.rs:878-882: the custom-trigger path adds only has_popup/expanded/on_activate, so the Enter/Space on_key on the child can never fire because the child never gets focus`
  - `r1 dialogs-popover note: "the popover trigger at launch: states=['enabled', 'sensitive', 'showing', 'visible'] actions=[{'name': 'click'...}]"; tree-launch.txt "[push button] 'Show popover'" > "[panel] ''" (neither focusable) vs "[push button] 'Open dialog'" > "[panel] '' {focusable}"`
  - `r1/r2/r3 'Tab twice from the window': "+8.6 ms object:state-changed:focused 1 [combo box] 'Theme'", "+195.2 ms object:state-changed:focused 1 [panel] ''", "+272.0 ms ORCA SAYS: 'panel.'" (r1); r3 "+194.0 ms ... [panel] ''" / "+275.7 ms ORCA SAYS: 'panel.'"`
  - `verify-dialogs-snackbar-distance r2 'Space shows it, then Shift+Tab until Dismiss': "+211.3 ms focused 1 [panel] ''", "+399.1 ms focused 1 [combo box] 'Theme'", "+585.6 ms focused 1 [push button] 'Dismiss'" (r3 identical: +210.8/+398.5/+584.7 ms)`
  - `'AT-SPI grab_focus on the popover trigger': FAIL "no focus change on the bus in this act" in r1, r2, r3`
- **Reproduced:** yes: tabwalk 1/1, dialogs-popover 2/2 runs (deterministic)
- **Verification:** confirmed. Reproduced: yes: 3 of 3 runs (dialogs-popover r1, r2, r3), plus 2 of 2 Shift+Tab walks (verify-dialogs-snackbar-distance r2, r3). Deterministic.
- **Fix idea:** OverlayTrigger should make ITS OWN node (the Role::Button node) focusable and route keys there, or merge role/name onto the focused child. PopoverCustom needs the same focusable(true) that Dialog and Snackbar add.

### dialogs-02 {#dialogs-02}

The custom-trigger Dialog ('Open dialog') puts focus on an unnamed panel: Orca says just 'panel.' when Tab reaches it and again when the dialog closes

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-review-dialog
- **Act:** dialogs-review-dialog: Tab from Theme to the trigger; Escape or Enter on Cancel closes the dialog
- **The reader should get:** Focus lands on a node that says 'Open dialog, push button' (with has-popup dialog). When the dialog closes, focus returns there and the reader hears the name again.
- **The reader gets:** Focus lands on the trigger's child \[panel\] '' and Orca says 'panel.'. The name, Button role, Click action, has\_popup and expanded all sit on the parent OverlayTrigger node, which is not focusable. On close, focus goes back to the same unnamed panel ('panel.'). Enter still opens the dialog, but the reader was never told what the control is.
- **Platform:** all (framework). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `de3bb295` (popover-trigger). Fixed part: the Dialog's custom trigger now takes focus as \[push button\] 'Open dialog', on Tab and when the dialog closes (fix-popover-trigger-dialog 3 of 3.
- **Where:** crates/teksilo-widgets/src/dialog.rs:783-855; crates/teksilo-widgets/src/overlay\_trigger.rs:236-243,291-298
- **Evidence:**
  - `tree-launch.txt: "[push button] 'Open dialog'" (not focusable) > "[panel] '' {focusable}"`
  - `'Tab reaches the Open dialog trigger': "+6.6 ms object:state-changed:focused 1 [panel] ''" / "+61.8 ms ORCA SAYS: 'panel.'" (2 of 2 runs)`
  - `'Escape closes it': "+26.4 ms object:state-changed:focused 1 [panel] ''" / "+213.7 ms ORCA SAYS: 'panel.'"; 'Enter on Cancel closes it': "+29.5 ms object:state-changed:focused 1 [panel] ''" / "ORCA SAYS: 'panel.'"`
  - `crates/teksilo-widgets/src/dialog.rs:783-786: the handler set with .focusable(true) is handed to OverlayTrigger and applied to the child (overlay_trigger.rs:238), while overlay_trigger.rs:292-296 sets Role::Button and the name on the trigger node. Snackbar's custom-trigger path does the same (snackbar.rs:449), but this example does not use it`
  - `The AT-SPI click on [push button] 'Open dialog' does work (dialog opens, Orca reads it), so the problem is focus and naming only`
  - `r1 'Tab reaches the Open dialog trigger': "+8.1 ms object:state-changed:focused 1 [panel] ''" / "+104.2 ms ORCA SAYS: 'panel.'"; r2: "+63.3 ms ORCA SAYS: 'panel.'"`
  - `r1 'Escape closes it': "+12.2 ms object:state-changed:focused 1 [panel] ''" / "+88.0 ms ORCA SAYS: 'panel.'"; 'Enter on Cancel closes it': "+31.0 ms ... [panel] ''" / "+95.8 ms ORCA SAYS: 'panel.'"`
  - `r1 'AT-SPI click on the trigger opens it': "+127.9 ms ORCA SAYS: 'Review Changes dialog Dialogs open centered, ...'" / "'Cancel push button.'"`
- **Reproduced:** yes: 2 of 2 runs (deterministic)
- **Verification:** confirmed. Reproduced: yes: 2 of 2 runs of dialogs-review-dialog (r1, r2). The same unnamed \[panel\] also takes focus back on every popover close in dialogs-popover r1-r3. Deterministic.
- **Fix idea:** Put focusability on the OverlayTrigger node (the one with the role and name), or have the walker merge the trigger's role/name onto the focused child. The same fix as dialogs-01.

### dialogs-03 {#dialogs-03}

Opening the popover moves focus to an unnamed role-less node outside an unnamed dialog, and Orca says nothing

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-popover
- **Act:** dialogs-popover: AT-SPI click on \[push button\] 'Show popover'
- **The reader should get:** Focus moves into a named dialog, or stays on the trigger with the popover announced. The reader hears the popover's name and content ('Popover. Use popovers for compact contextual actions…').
- **The reader gets:** Focus goes to \[unknown\] '' (PopoverBody, which has no accessibility()), the parent of an unnamed \[dialog\] ''. Orca's speech generator produces 'pauses only', so the reader hears silence and does not know a popover opened. The dialog is unnamed because surface\_name defaults to empty, and the example sets none.
- **Platform:** all (framework: request\_focus targets a non-focusable node whose role is Unknown). Measured on Linux. On Windows (by source) UIA gets WindowOpened for the unnamed dialog node and a focus on a Role::Unknown element.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `de3bb295` (popover-trigger). Fixed part: opening the popover puts focus on \[dialog\] 'Show popover' and Orca says 'Show popover dialog Popover Use popovers ...' (3 of 3 in each of 3 scenarios, plus verify-dialogs-popover-more 3 of 3.
- **Where:** crates/teksilo-widgets/src/popover\_widget.rs:361,569-658,685,779; crates/teksilo-core/src/widget\_tree/pointer\_router.rs:3040; crates/teksilo-widgets/src/popover\_surface.rs:262-268
- **Evidence:**
  - `events: "+31.1 ms object:state-changed:focused 1 [unknown] ''"; tree after the act: "[panel] '' > [unknown] '' ['enabled','focusable','focused',...] > [dialog] '' > [label] 'Popover' ..."`
  - `checks: FAIL the popover's dialog has a name - "[dialog] ''"; FAIL focus lands inside the popover's dialog - "path: [application] 'dialogs-and-popovers' > [frame] '' > [panel] '' > [panel] '' > [unknown] ''"; FAIL Orca says 'Use popovers' (2 of 2 runs)`
  - `orca-debug.out (run 130342): "13:03:58.955848 - FOCUS MANAGER: Changing locus of focus from [panel] to [unknown]. Notify: True" then "13:03:59.005916 - SPEECH GENERATOR: Results for [unknown] are pauses only"; run 131000: "13:10:19.705231 - SPEECH GENERATOR: Results for [unknown] are pauses only"`
  - `crates/teksilo-widgets/src/popover_widget.rs:685 (focus_id = content_id) and :779 (ctx_evt.request_focus(focus_id)); crates/teksilo-core/src/widget_tree/pointer_router.rs:3040 'first_focusable_descendant(id).unwrap_or(id)' focuses the non-focusable host when the content has no control`
  - `PopoverBody (popover_widget.rs:575-655) has no accessibility(), so its role is the builder default Role::Unknown (teksilo-core/src/accessibility.rs:475); popover_widget.rs:361 surface_name: String::new(), popover_surface.rs:267-268 set_name(&self.name)`
  - `orca-debug.out: "13:17:58.416660 - SPEECH GENERATOR: Results for [unknown] are pauses only" (r1), "13:23:03.800830" (r2), "13:26:38.019238" (r3), "13:25:25.867811" / "13:29:00.180509" (popover-more r2/r3)`
  - `r1 tree after opening: "[unknown] '' {focusable,focused}" > "[dialog] ''" > "[label] 'Popover'", "[label] 'Use popovers for compact contextual actions without leaving the current surface.'", three badge labels`
  - `verify-dialogs-popover-more r2: "+18.7 ms object:state-changed:focused 1 [unknown] ''" / "+18.9 ms object:state-changed:focused 0 [push button] 'Show snackbar'"; FAIL Orca says 'Use popovers'`
- **Reproduced:** yes: 2 of 2 runs
- **Verification:** confirmed. Reproduced: yes: 5 of 5 first openings (dialogs-popover r1, r2, r3; verify-dialogs-popover-more r2, r3), including one opened while focus was on a real button ('Show snackbar').
- **Fix idea:** Use request\_focus\_into (no fallback), or keep focus on the trigger when the content has no control. Give the surface a name by default (the trigger's name, as popover\_surface.rs's own comment says). Never focus a Role::Unknown host. The example could also set .surface\_name("Popover").

### dialogs-04 {#dialogs-04}

Snackbar and popover reuse the same accessibility nodes on every showing, so after the first showing Orca drops them as defunct: the 2nd and later snackbars are silent, and focus into a reopened popover is ignored

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-snackbar, dialogs-popover
- **Act:** dialogs-snackbar: Space on 'Show snackbar' three times, each left to time out; dialogs-popover: open, Escape, open again
- **The reader should get:** Every showing is heard ('Snackbar' or better the message), and every popover opening moves Orca's focus.
- **The reader gets:** The snackbar surface and the popover body are persistent deferred subtrees, built once and re-activated with the same WidgetIds and so the same AT-SPI paths. accesskit\_atspi\_common marks them defunct when they leave the tree, and libatspi keeps them defunct when they return. Snackbar: showing 1 is spoken; showings 2 and 3 come from the same path the bus was told was defunct, and Orca logs 'Ignoring defunct object'. Popover: on the second opening the focus event on \[unknown\] is ignored as defunct, and Orca's locus of focus stays on the old panel. None of these go through ctx.announce, so the K2 announcer fix does not cover them.
- **Platform:** Linux AT-SPI/Orca (the defunct cache is libatspi's). On Windows/macOS by source, a reused NodeId is not held defunct, so this part is Linux-only.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: snackbar part (dialogs-snackbar showings 2 and 3 heard 1/1); the popover's reopening no longer drops events as defunct, but its focus-into-popover defect is a separate topic.
- **Where:** crates/teksilo-widgets/src/snackbar.rs:408-418; crates/teksilo-widgets/src/popover\_widget.rs:586-588,655-657,672-682
- **Evidence:**
  - `snackbar: 3 of 3 runs, showing 1 spoken ('SPEECH OUTPUT: 'Snackbar''); showings 2 and 3 dropped in 6 of 6 (for example run 130812: "13:08:37.969043 - EVENT MANAGER: Ignoring defunct object: [notification: 'Snackbar']", "13:08:44.147574 - EVENT MANAGER: Ignoring defunct object: [notification: 'Snackbar']")`
  - `events.jsonl run 130424: all three announcements come from one path: "object:announcement" source "/org/a11y/atspi/accessible/0/79228166185166408261744721920" at 13:04:33.499620, 13:04:40.206104, 13:04:46.478610; after each time-out: "object:state-changed:defunct 1 [notification] 'Snackbar'"`
  - `observation: "'Snackbar' came from a node the bus had already been told was defunct, which Orca drops"`
  - `popover run 130342: close -> "+9.7 ms object:state-changed:defunct 1 [unknown] ''"; reopen -> "+20.3 ms object:state-changed:focused 1 [unknown] ''" and orca-debug.out "13:04:07.027053 - EVENT MANAGER: object:state-changed:focused for [unknown] ... is not obsoleted" / "13:04:07.027085 - EVENT MANAGER: Ignoring defunct object: [unknown]" (2 of 2 runs)`
  - `crates/teksilo-widgets/src/snackbar.rs:408-418 (add_detached_deferred, built once); crates/teksilo-widgets/src/popover_widget.rs:666-676 (add_deferred) and :651-653 (preserves_children_on_rebuild); accesskit_atspi_common-0.20.0/src/adapter.rs:90-107 remove_node emits StateChanged(Defunct); /usr/lib/python3/dist-packages/orca/event_manager.py:798`
  - `snackbar: "Ignoring defunct object: [notification: 'Snackbar']" at 13:18:38.564828 / 13:18:44.787836 (r1), 13:23:44.246310 / 13:23:50.458135 (r2), 13:27:18.051794 / 13:27:24.238743 (r3); every announcement from path /org/a11y/atspi/accessible/0/79228166185166408261744721920`
  - `NEW, verify-dialogs-snackbar-distance r2 'Space shows it, then Shift+Tab until Dismiss' (2nd showing): "+585.6 ms object:state-changed:focused 1 [push button] 'Dismiss'" and orca-debug.out "13:25:01.730051 - EVENT MANAGER: object:state-changed:focused for [push button: 'Dismiss'] ... (1, 0, 0) is not obsoleted" / "13:25:01.730081 - EVENT MANAGER: Ignoring defunct object: [push button: 'Dismiss']"; last locus change was "13:25:01.559042 FOCUS MANAGER: Changing locus of focus from [panel] to [combo box: 'Theme']"; r3: "13:28:36.265734 Ignoring defunct object: [push button: 'Dismiss']"`
  - `NEW, verify-dialogs-popover-more r2/r3 'Escape closes opening 2' and 'opening 3': "+8.1 ms object:state-changed:focused 1 [push button] 'Show snackbar'", no ORCA SAYS, FAIL Orca says 'Show snackbar' (4 of 4); the openings: "Ignoring defunct object: [unknown]" at 13:25:34.600641 / 13:25:42.376334 (r2), 13:29:08.886874 / 13:29:16.725143 (r3)`
  - `dialogs-popover r1 'AT-SPI click opens it a second time': "+13.2 ms object:state-changed:focused 1 [unknown] ''" / "13:18:06.167356 EVENT MANAGER: Ignoring defunct object: [unknown]" (r2 13:23:11.493433, r3 13:26:45.779564)`
- **Reproduced:** yes: snackbar 3 of 3 runs (6 of 6 re-showings dropped, 3 of 3 first showings heard); popover reopen 2 of 2 runs
- **Verification:** corrected by the verifier. Reproduced: yes. Snackbar: 3 of 3 runs, 6 of 6 re-showings' announcements dropped, 3 of 3 first showings heard. Focus into a re-shown snackbar's Dismiss dropped in 2 of 2 runs. Popover: every reopening's focus dropped, in 3 of 3 dialogs-popover runs and 4 of 4 reopenings in verify-dialogs-popover-more r2/r3. Real, and wider than reported. Besides the dropped announcement, Orca also ignores KEYBOARD FOCUS moving into a re-shown subtree. On a second snackbar showing, Shift+Tab reaches Dismiss, and Orca logs 'Ignoring defunct object: \[push button: 'Dismiss'\]'. It says nothing, and its locus of focus stays on the Theme combo box while the keys now act on Dismiss. For the popover, the reopened body's focus is ignored, so the Escape that closes it is silent too: focus returns to 'Show snackbar', which Orca believes it never left. A reopened popover is therefore silent both opening and closing. Location corrected: popover\_widget.rs:672-682 (add\_deferred), :586-588 (PopoverBody::build returns the cached body\_id) and :655-657 (preserves\_children\_on\_rebuild), not 666-676/651-653. snackbar.rs:408-418 is right. The cause is Teksilo reusing the WidgetId, and so the NodeId, across showings. accesskit\_atspi\_common marks a removed node defunct and never clears it when the node returns, and Orca's event\_manager.py:798 checks AXObject.is\_dead/AXUtilities.is\_defunct. MessageBox and Dialog content is built fresh per presentation (ModalRequest::deferred), which is why all 38 MessageBox openings I ran were heard. Inference, not measured: a popover whose content holds a text field (for example table\_view/header.rs's filter popover) would, from its second opening, take typing in a field Orca ignores entirely. Severity stays high. The K2 announcer fix does not cover these nodes.
- **Fix idea:** Give a re-shown overlay subtree fresh AccessKit NodeIds per showing (an epoch in the id mapping, the same fix as K2's announcer nodes), or rebuild the surface on each showing.

### dialogs-05 {#dialogs-05}

The snackbar announces only the generic word 'Snackbar', and its message 'Autosave complete' is never announced

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-snackbar
- **Act:** dialogs-snackbar / dialogs-snackbar-focus / dialogs-snackbar-reach: show the snackbar (Space, or AT-SPI click)
- **The reader should get:** The reader hears the notification's message, 'Autosave complete', politely, while focus stays where it was.
- **The reader gets:** The only object:announcement is the alert's name, 'Snackbar' (the en-US fallback a11y-snackbar-name), which is jargon and carries no information. The content is Live::Off, so the message is not announced. A reader hears the message only by Tabbing into the snackbar ('notification Snackbar.' / 'Autosave complete' / 'Dismiss push button.') before it times out.
- **Platform:** all: on Windows, UIA LiveRegionChanged makes NVDA read the node's name ('Snackbar'); on macOS the announcement carries the name (by source, atspi/windows/macos all announce the name). Measured on Linux.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/snackbar.rs:227-243; crates/teksilo-widgets/src/styles/recipe\_snackbar\_style.rs:171-188
- **Evidence:**
  - `every showing: "+36.7 ms object:announcement [notification] 'Snackbar' text='Snackbar'" and "ORCA SAYS: 'Snackbar'"; FAIL the bus carries an announcement of 'Autosave complete'; FAIL Orca says 'Autosave complete' (all 9 showings in 3 runs, plus snackbar-focus and snackbar-reach runs)`
  - `orca-debug.out run 130424: "13:04:33.524141 - SPEECH OUTPUT: 'Snackbar'"`
  - `crates/teksilo-widgets/src/snackbar.rs:235-242: name = announcement or tr_widget!(a11y_snackbar_name) ('Snackbar', locales/en-US.ftl:33); crates/teksilo-widgets/src/styles/recipe_snackbar_style.rs:172-188 sets the content root Live::Off unconditionally, even when no announcement was given`
  - `example: examples/dialogs_and_popovers/src/main.rs:360-364 builds the Snackbar without .announcement(...)`
  - `r1 'Space shows the snackbar, showing 1': "+31.4 ms object:announcement [notification] 'Snackbar' text='Snackbar'" / "+38.5 ms ORCA SAYS: 'Snackbar'"; FAIL the bus carries an announcement of 'Autosave complete' (same in r2 +33.1 ms, r3 +32.2 ms)`
- **Reproduced:** yes: 3 of 3 runs, every showing
- **Verification:** confirmed. Reproduced: yes: 3 of 3 runs of dialogs-snackbar, every showing; also every showing in snackbar-focus, snackbar-reach and snackbar-distance (r1-r3).
- **Fix idea:** When no .announcement() is set, name the alert from its content's text (as MessageBox does with its title), or leave the content live. The example should add .announcement(lit!("Autosave complete")).

### dialogs-06 {#dialogs-06}

When the snackbar times out, it pulls focus back to the control that was focused when it appeared, even though the reader has moved on

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-snackbar-focus
- **Act:** dialogs-snackbar-focus: Space on 'Show snackbar', Tab to 'Adaptive modal window', wait for the 2.5 s time-out
- **The reader should get:** The snackbar goes away, and focus stays on 'Adaptive modal window', where the user put it (the snackbar never took focus).
- **The reader gets:** On time-out focus jumps back to 'Show snackbar' and Orca says 'Show snackbar push button.' unprompted. The user loses their place, and a keyboard user typing into a field would be moved out of it.
- **Platform:** all (framework overlay manager). Measured on Linux.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `b30770d5` (tooltip-snapback). Fixed part: measured on the combined build, 1 run: dialogs-snackbar-focus had 3 failed checks in the sweep, none now.
- **Where:** crates/teksilo-core/src/widget\_tree.rs:1713-1721; crates/teksilo-core/src/widget\_tree/pointer\_router.rs:2882-2900
- **Evidence:**
  - `'it times out while focus is on the next control': "+887.2 ms object:state-changed:focused 1 [push button] 'Show snackbar'" / "+887.4 ms object:state-changed:focused 0 [push button] 'Adaptive modal window'" / "+983.0 ms ORCA SAYS: 'Show snackbar push button.'" (3 of 3 runs)`
  - `orca-debug.out run 130629: "13:07:24.588068 - SPEECH OUTPUT: 'Adaptive modal window push button.'" then, with no key pressed, "13:07:26.309228 - SPEECH OUTPUT: 'Show snackbar push button.'"`
  - `crates/teksilo-core/src/widget_tree/pointer_router.rs:2882-2900: a timed overlay records focus_restore = the focus at show time (set_top_focus_restore); crates/teksilo-core/src/widget_tree.rs:1714-1722 (process_auto_dismiss_overlays_impl) restores it on time-out whenever the node is active, without checking whether focus is still where it was or inside the overlay`
  - `r1 'it times out while focus is on the next control': "+993.9 ms object:state-changed:focused 1 [push button] 'Show snackbar'" / "+1090.8 ms ORCA SAYS: 'Show snackbar push button.'"; r2 "+1086.7 ms" / "+1137.3 ms"; r3 "+1068.2 ms" / "+1133.2 ms"`
  - `orca-debug.out SPEECH OUTPUT 'Show snackbar push button.' at 13:19:12.050734 (r1), 13:24:17.543670 (r2), 13:27:51.371457 (r3), each ~1.7 s after 'Adaptive modal window push button.'`
- **Reproduced:** yes: 3 of 3 runs
- **Verification:** confirmed. Reproduced: yes: 3 of 3 runs of dialogs-snackbar-focus (r1, r2, r3).
- **Fix idea:** On auto-dismiss (and any dismissal of an overlay that never took focus), restore focus only when focus is inside the dismissed overlay, or still on the recorded node.

### dialogs-07 {#dialogs-07}

The snackbar times out while the reader's focus is on its Dismiss button, and removes the button from under them

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-snackbar-reach
- **Act:** dialogs-snackbar-reach: with focus on 'Custom buttons', AT-SPI click 'Show snackbar', Tab to Dismiss, wait
- **The reader should get:** The time-out pauses while focus (or the reader) is inside the snackbar, as it would for a hovered pointer, so the user can act on it (WCAG 2.2.1).
- **The reader gets:** The 2.5 s time-out runs regardless. Focus is thrown back to 'Custom buttons' and Orca says 'Custom buttons push button.'. In one run the time-out fired 150 ms after focus reached Dismiss, so Orca never said 'Dismiss' at all.
- **Platform:** all (framework). Measured on Linux.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/snackbar.rs:65-96
- **Evidence:**
  - `runs 130526 and 131245: Tab -> "object:state-changed:focused 1 [push button] 'Dismiss'" / "ORCA SAYS: 'notification Snackbar.'" "'Autosave complete'" "'Dismiss push button.'", then about 1 s into the next act "object:state-changed:focused 1 [push button] 'Custom buttons'" / "ORCA SAYS: 'Custom buttons push button.'"`
  - `run 130925: "+26.0 ms object:state-changed:focused 1 [push button] 'Dismiss'" then "+173.7 ms object:state-changed:focused 1 [push button] 'Custom buttons'" and "13:09:44.311220 EVENT MANAGER: Ignoring defunct object: [push button: 'Dismiss']"; Orca said only 'Custom buttons push button.'`
  - `crates/teksilo-widgets/src/snackbar.rs:65-96: present_snackbar uses show_overlay_for(request, duration) with no pause on focus; crates/teksilo-core/src/overlay.rs:731 has pause_auto_dismiss, but the snackbar never calls it; widget_tree.rs:1696-1712 dismisses on elapsed time alone`
  - `reach r2: "13:24:30.145028 NULL SPEECH: speak 'notification Snackbar.'", "...145058 speak 'Autosave complete'", "...145097 speak 'Dismiss push button.'" then "13:24:31.823425 NULL SPEECH: stop" / "speak 'Custom buttons push button.'"; r1 stop 13:19:36.243275 after 13:19:34.5298; r3 stop 13:28:16.039442 after 13:28:14.3496`
  - `reach r1/r2/r3 'focus stays on Dismiss while the time-out passes': "+1029.9 / +930.6 / +1011.6 ms object:state-changed:focused 1 [push button] 'Custom buttons'" and "ORCA SAYS: 'Custom buttons push button.'"`
  - `verify-dialogs-snackbar-distance r2 'Space shows it, then Tab seven times at once': Adaptive modal window > Save changes? > Delete file? > Could not open file > Welcome > Custom buttons > "+1141.6 ms focused 1 [push button] 'Dismiss'" > "+2529.1 ms focused 1 [push button] 'Show snackbar'"; Orca: "+1223.8 ms ORCA SAYS (CUT): 'notification Snackbar.'" / "(CUT): 'Autosave complete'" / "(CUT): 'Dismiss push button.'" / "+2591.9 ms 'Show snackbar push button.'" (r3: Dismiss at +1138.5 ms, same outcome)`
  - `crates/teksilo-widgets/src/snackbar.rs:65-96 show_overlay_for with no pause; crates/teksilo-core/src/overlay.rs:745 pause_auto_dismiss; crates/teksilo-core/src/widget/event_context.rs:1614; crates/teksilo-core/src/widget_tree/pointer_router.rs:2806-2818 (the queue's only producer is the toast host)`
- **Reproduced:** yes: 3 of 3 runs (the time-out fired with focus on Dismiss each time)
- **Verification:** corrected by the verifier. Reproduced: yes: 3 of 3 runs of dialogs-snackbar-reach (r1-r3), plus 2 of 2 runs of verify-dialogs-snackbar-distance. Real. Severity raised from medium to high because what the reader gets is worse than reported. (1) The sweep's 'passed' line says Orca reads "'notification Snackbar.' 'Autosave complete' 'Dismiss push button.'" on entering Dismiss. In all 3 reach runs that reading, about 4 s at Orca's rate, is stopped 1.68-1.71 s in by the time-out's focus jump. 'Dismiss push button.' is never heard, and the report misses the cut because the stop falls in the next act. (2) Dismiss is the LAST Tab stop of the window: 7 Tabs from 'Show snackbar', or 3 Shift+Tabs. A reader who listens to each stop cannot get there in 2.5 s. Pressing Tab every 0.12 s without listening reached it at +1.14 s, and it was still taken away at 2.5 s. (3) On a re-showing, focus into Dismiss is not spoken at all (dialogs-04). So a reader effectively cannot operate the snackbar's action, which fails WCAG 2.2.1 (Level A). In this example the action is only Dismiss, which is why this is high and not critical. Two corrections: pause\_auto\_dismiss is at overlay.rs:745, not :731, and the snackbar does not pause on hover either. The only pause route is EventContext::pause\_overlay\_auto\_dismiss (event\_context.rs:1614), used by the toast host's hover-pause, and snackbar.rs never calls it. The sweep's 'as it would for a hovered pointer' is wrong.
- **Fix idea:** Pause the snackbar's auto-dismiss while focus\_within is true (pause\_auto\_dismiss / resume\_auto\_dismiss already exist), and resume on focus-out.

### dialogs-08 {#dialogs-08}

An in-tree modal MessageBox is not modal to assistive technology: AT-SPI click and focus reach the page behind it, and a second and third modal box stack up

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-modal-inert
- **Act:** dialogs-modal-inert: open 'Save changes?'; AT-SPI click on \[push button\] 'Welcome' behind it; AT-SPI grab\_focus on 'Delete file?' behind it; real Space
- **The reader should get:** While a modal box is up, an activation or focus request aimed at the page behind it does nothing (the scrim blocks the pointer and Tab is trapped; the AT path should be gated the same way).
- **The reader gets:** The AT-SPI click on 'Welcome' opens a second modal box on top. grab\_focus moves focus to 'Delete file?' behind both boxes (Orca: 'Delete file? push button.'), and a real Space there opens a third modal box. After Escape closes the top box, focus is restored to a control behind the two remaining modals. A reader who activates by object navigation (Orca flat review, NVDA object navigator, VoiceOver cursor) can trigger actions behind a 'Save changes?' question.
- **Platform:** Linux measured (in-tree modals). The dispatch is platform-independent, so any in-tree modal on Windows/macOS behaves the same by source; native-window modals there were not measured.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `e7764b0f` (modal-at). Fixed part: an AT-SPI click behind an in-tree MessageBox no longer opens a second or third box, and grab\_focus behind it no longer moves focus. The next real Space presses the box's own button..
- **Where:** crates/teksilo-core/src/widget\_tree/pointer\_router.rs:1152-1230; crates/teksilo-app/src/app.rs:2103-2141,2448-2471
- **Evidence:**
  - `'AT-SPI click on Welcome behind the modal box': "+47.5 ms object:announcement [alert] 'Welcome to Teksilo'" / "+49.2 ms object:state-changed:focused 1 [push button] 'OK'"; FAIL the tree holds no [alert] 'Welcome to Teksilo' - "found [alert] 'Welcome to Teksilo'" (2 of 2 runs)`
  - `'AT-SPI grab_focus on Delete file? behind the boxes': "+17.9 ms object:state-changed:focused 1 [push button] 'Delete file?'" / "ORCA SAYS: 'Delete file? push button.'"`
  - `'Real Space on the control behind the boxes': "+46.1 ms object:announcement [alert] 'Delete file?'" / "+47.5 ms object:state-changed:focused 1 [push button] 'No'", so three modal alerts are up`
  - `crates/teksilo-core/src/widget_tree/accessibility_impl.rs:298-316 dispatch_access_action routes any AccessAction to its target with no modal check; crates/teksilo-core/src/widget_tree/focus_impl.rs:441-447 traps only Tab (topmost_centered Cycle scope), and its test at :2229-2231 explicitly accepts 'an AccessKit action or app code can' force focus outside a modal`
  - `r3 'AT-SPI click on Welcome behind the modal box': "+36.7 ms object:announcement [alert] 'Welcome to Teksilo'" / "+37.8 ms object:state-changed:focused 1 [push button] 'OK'"; FAIL the tree holds no [alert] 'Welcome to Teksilo'`
  - `r3 'AT-SPI grab_focus on Delete file? behind the boxes': "+28.6 ms object:state-changed:focused 1 [push button] 'Delete file?'" / "+97.9 ms ORCA SAYS: 'Delete file? push button.'"; 'Real Space ...': "+32.7 ms object:announcement [alert] 'Delete file?'" / "+34.0 ms focused 1 [push button] 'No'"`
  - `r1 'Escape': "+10.6 ms object:state-changed:focused 1 [push button] 'Delete file?'" (a page control) while [alert] 'Welcome to Teksilo' and [alert] 'Save changes?' are still up; 'Escape again' closes Welcome and focuses 'Save'`
- **Reproduced:** yes: 2 of 2 runs (deterministic)
- **Verification:** corrected by the verifier. Reproduced: yes: 3 of 3 runs of dialogs-modal-inert (r1, r2, r3). Deterministic. Reproduced in every run. An AT-SPI click on 'Welcome' opens a second box over 'Save changes?'. grab\_focus puts focus on 'Delete file?' behind both, and Orca says 'Delete file? push button.'. A real Space then opens a third box. Escape then restores focus to the page control behind the two remaining boxes. Location corrected: accessibility\_impl.rs:298-316 (dispatch\_access\_action) is the automation bridge's entry. The platform route is teksilo-app/src/app.rs:2103-2141 (handle\_accessibility\_actions), then the AccessAction arm at pointer\_router.rs:1152-1230, which delivers any action to an active target and serves Focus through focus\_with\_origin\_ops with no modal check. Platform claim widened, by source and not measured: the native-window case on Windows/macOS is no safer. A parent blocked by a native modal swallows only user-input events (app.rs:2448-2469, input\_loop.rs:119-128). AccessKit's ActionHandler queues the request and calls request\_redraw (teksilo-platform/src/window.rs:1150-1153). RedrawRequested is not input, so it is delivered to the blocked parent, where handle\_accessibility\_actions (app.rs:2471) drains the queue and dispatches the action into the parent's tree.
- **Fix idea:** While a centered modal is up, refuse (or redirect into the modal) AccessAction and Focus requests whose target is outside the topmost modal's content, the same way the scrim refuses pointer input.

### dialogs-09 {#dialogs-09}

The 'Don't show this again' check box in the Welcome MessageBox publishes its new state only at the next focus move, so the toggle is never heard

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-welcome
- **Act:** dialogs-welcome: Tab to the check box, Space (and later an AT-SPI click)
- **The reader should get:** object:state-changed:checked is emitted as the box toggles, and Orca says 'checked' / 'not checked' at once.
- **The reader gets:** Nothing reaches the bus for Space or for the AT-SPI click, even with a 4 s record window. The checked event is emitted only with the next Tab, in the same update as the focus change, so Orca's 'checked' is cut by 'OK push button.'. The AT-SPI click's uncheck is never announced at all.
- **Platform:** all (framework: the AT tree is not re-walked). Measured on Linux.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Where:** crates/teksilo-widgets/src/styles/recipe\_checkbox\_style.rs:147; crates/teksilo-widgets/src/checkbox.rs (build binds nothing)
- **Evidence:**
  - `'Space checks it': FAIL a object:state-changed:checked event from [check box] - "no object:state-changed:checked event from [check box] '*'"; FAIL Orca says 'checked' (2 of 2 runs)`
  - `'Tab to OK' (run 130728): "+6.8 ms object:state-changed:checked 1 [check box] \"Don't show this again\"" / "+7.0 ms object:state-changed:focused 1 [push button] 'OK'"; orca-debug.out "13:07:51.774699 - SPEECH OUTPUT: 'checked'" then "13:07:51.808672 - NULL SPEECH: stop" and "13:07:51.808750 - SPEECH OUTPUT: 'OK push button.'"`
  - `'AT-SPI click unchecks it': FAIL no object:state-changed:checked; FAIL Orca says 'not checked' (2 of 2 runs)`
  - `crates/teksilo-widgets/src/styles/recipe_checkbox_style.rs:147 binds the state at BindingLevel::RepaintOnly only; checkbox.rs binds nothing at AccessibilityOnly; a RepaintOnly change does not set a11y_dirty (crates/teksilo-core/src/binding.rs:22-49; accessibility_impl.rs:23-30)`
  - `Same root cause as the check-box finding in catalog-a (catalog-a-toggles, same missing event); reported here because it is what the MessageBox's check box does`
  - `r3 'Tab to OK': "+7.6 ms object:state-changed:checked 1 [check box] \"Don't show this again\"" / "+7.9 ms object:state-changed:focused 1 [push button] 'OK'" / "+15.8 ms ORCA SAYS (CUT): 'checked'" / "+46.8 ms ORCA SAYS: 'OK push button.'"; r1 same at +6.5/+6.8 ms, r2 at +9.7 ms`
  - `'Space checks it' and 'AT-SPI click unchecks it': FAIL "no object:state-changed:checked event from [check box] '*'" in r1, r2, r3; the result label reads "Welcome → Ok (checkbox=false, dismissal=Button)" afterwards, so the toggle itself happened`
- **Reproduced:** yes: 2 of 2 runs (Space and AT-SPI click both)
- **Verification:** confirmed. Reproduced: yes: 3 of 3 runs of dialogs-welcome (r1, r2, r3), both Space and the AT-SPI click.
- **Fix idea:** Bind the check-state signal to the Checkbox node at BindingLevel::AccessibilityOnly (in addition to the RepaintOnly visual binding).

### dialogs-10 {#dialogs-10}

Every MessageBox title is spoken twice on opening: the live announcement is cut at once by the focus reading, which says the title again

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-save-changes (and every MessageBox scenario)
- **Act:** Space (or AT-SPI click) on any MessageBox trigger: 'Save changes?', 'Delete file?', 'Could not open file', 'Welcome', 'Custom buttons'
- **The reader should get:** The title is heard once, then the text, then the focused default button.
- **The reader gets:** The AlertDialog is an assertive live region. Its object:announcement reaches the bus 1-14 ms before the focus change in the same update. Orca starts the title, stops it to present the new focus, then reads 'alert &lt;title&gt;' + description + '&lt;button&gt; push button'. The reader hears a clipped fragment of the title and then the whole title again. Nothing is lost, but the live channel is wasted, and the stutter repeats on every box.
- **Platform:** Linux AT-SPI/Orca (in-tree presentation). On Windows/macOS a MessageBox opens as a native window (supports\_native\_modal\_windows() is true), and the adapters emit no announcement for a new window's initial tree, so by source this is Linux-only unless an app forces ModalPresentation::InTree.
- **Severity:** low; **layer:** framework
- **Status:** Open. In the announce-focus fix topic, not fixed there: Different cause. The live region is the MessageBox AlertDialog itself (message\_box.rs:1115-1124, Live::Assertive). Focus moves into it through initial\_focus\_hint, and it is not an announcer message. A held-back dialog node is impossible, because focus cannot land inside a node that is not in the tree. The fix belongs in the widget: do not make the in-tree box live when presenting it moves focus into it, since the focus reading already says 'alert &lt;title&gt;' plus the description.
- **Where:** crates/teksilo-widgets/src/message\_box.rs:1115-1124
- **Evidence:**
  - `24 of 24 MessageBox openings in 11 runs: the title announcement is cut and the focus reading 'alert <title>' follows (dialogs-save-changes 3 runs x 3 openings, delete-file, could-not-open x2, welcome x2, custom-buttons, modal-inert x2)`
  - `run 130212, first opening: "+37.7 ms object:announcement [alert] 'Save changes?' text='Save changes?'" / "+39.7 ms object:state-changed:focused 1 [push button] 'Save'" / "+47.9 ms ORCA SAYS (CUT): 'Save changes?'" / "+102.8 ms ORCA SAYS: 'alert Save changes?'"`
  - `orca-debug.out: "13:02:21.893882 - NULL SPEECH: speak 'Save changes?' interrupt=True" -> "13:02:21.948418 - NULL SPEECH: stop" -> "13:02:21.948789 - SPEECH OUTPUT: 'alert Save changes?'" -> "13:02:21.948825 - SPEECH OUTPUT: 'You have unsaved changes in report.skrib. Your changes will be lost if you don't save them.'" -> "13:02:21.948870 - SPEECH OUTPUT: 'Save push button.'"`
  - `crates/teksilo-widgets/src/message_box.rs:1115-1123 (set_live(Live::Assertive) on the AlertDialog named by its title); the consumer hands the added node to the adapter before the focus event (accesskit_consumer tree.rs:640-673); Orca default.py:702-705 presentationInterrupt on a focus change`
  - `gap from Orca's 'NULL SPEECH: speak <title> interrupt=True' to its 'NULL SPEECH: stop': r1 49-78 ms (8 openings), r2 41-107 ms (13), r3 37-78 ms (17); every one followed by "SPEECH OUTPUT: 'alert <title>'"`
  - `r1 dialogs-save-changes orca-debug.out: "13:17:55.784831 - NULL SPEECH: speak 'Save changes?' interrupt=True" / "13:17:55.834077 - NULL SPEECH: stop" / "13:17:55.834187 - SPEECH OUTPUT: 'alert Save changes?'" / "'You have unsaved changes in report.skrib. Your changes will be lost if you don't save them.'" / "'Save push button.'"`
- **Reproduced:** yes: 24 of 24 openings across 11 runs
- **Verification:** corrected by the verifier. Reproduced: yes: 38 of 38 MessageBox openings in 9 runs (r1: 8, r2: 13, r3: 17) had the title utterance stopped by the focus reading. The mechanism is real. MessageBox's AlertDialog is Live::Assertive (message\_box.rs:1122), its announcement reaches the bus 1-3 ms before the focus change in the same update, and Orca's focus presentation stops it. But 'spoken twice' overstates what a reader gets. Orca handed the title to the speech server and cancelled it 37-107 ms later in all 38 of my openings (the sweep's own 24 openings: 47-287 ms). That is one or two characters at Orca's rate, and with a real synthesizer's start-up latency it is usually inaudible. The only complete reading is the focus one: 'alert &lt;title&gt;' + description + '&lt;button&gt; push button.'. Nothing is lost or misordered, and the defect is a wasted, self-interrupting live announcement. That makes it low (cosmetic) rather than medium. The fix idea stands: do not make the in-tree box live when presenting it moves focus into it. The Linux-only platform claim is right, since on Windows/macOS the box is a native window whose initial tree is announced by no adapter.
- **Fix idea:** Do not make the in-tree box a live region when presenting it moves focus into it (the focus reading already says 'alert &lt;title&gt;' + description). Keep Live::Assertive only for a box presented without focus.

### dialogs-11 {#dialogs-11}

No disclosure state reaches Orca: 'Show details' never says collapsed or expanded, and pressing it is silent; the Dialog and popover triggers never say expanded or has-popup

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-could-not-open, dialogs-adaptive-dialog
- **Act:** dialogs-could-not-open: Tab to 'Show details', Space; dialogs-adaptive-dialog: open the dialog and read the opener's states
- **The reader should get:** 'Show details, push button, collapsed'. Space says 'expanded' (and the details become readable). The Dialog trigger reads 'expanded' while its dialog is open.
- **The reader gets:** Orca says 'Show details push button.' with no state. Space emits only children-changed:add for the details label, and Orca says nothing, so the reader gets no feedback that anything happened. The trigger's AT-SPI states never include expandable or expanded. Teksilo sets both (Accordion set\_expanded, OverlayTrigger/Button has\_popup + set\_expanded), but accesskit\_atspi\_common 0.20 maps neither.
- **Platform:** Linux AT-SPI only. Windows maps both (accesskit\_windows node.rs:485-494 aria haspopup, :714-721 ExpandCollapse). macOS publishes neither (no 'expand' in accesskit\_macos-0.27.0 by grep).
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/accordion.rs:668-678 (sets expanded correctly; the loss is in accesskit\_atspi\_common-0.20.0/src/node.rs:301-386)
- **Evidence:**
  - `'Tab from Retry wraps to the Show details toggle': "ORCA SAYS: 'Show details push button.'"; FAIL [push button] 'Show details' has expandable - "states=['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']" (2 of 2 runs)`
  - `'Space expands the details': only "+54.7 ms object:children-changed:add [panel] '' -> [label] 'Underlying OS error: EACCES (permission denied)\nopen(\"report.skrib\", O_RDWR) → errno 13'"; FAIL Orca says 'expanded'; no ORCA SAYS line in the act`
  - `dialogs-adaptive-dialog 'Space opens the dialog': FAIL [push button] 'Adaptive modal window' has expanded - "states=['enabled', 'focusable', 'sensitive', 'showing', 'visible']" (2 of 2 runs)`
  - `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/accesskit_atspi_common-0.20.0/src/node.rs:301-386 state() sets no Expandable/Expanded; grep finds no has_popup or expand mapping anywhere in accesskit_atspi_common-0.20.0`
  - `crates/teksilo-widgets/src/accordion.rs:669-672 sets Role::Button + set_expanded`
  - `r2 'Tab from Retry wraps to the Show details toggle': "+67.7 ms ORCA SAYS: 'Show details push button.'"; FAIL [push button] 'Show details' has expandable`
  - `r2 'Space expands the details': "+64.5 ms object:children-changed:add [panel] '' -> [label] 'Underlying OS error: EACCES ...'" and no ORCA SAYS in the act`
- **Reproduced:** yes: 2 of 2 runs each (deterministic)
- **Verification:** confirmed. Reproduced: yes: 2 of 2 runs of dialogs-could-not-open (r1, r2) and of dialogs-adaptive-dialog (r1, r2). Deterministic.
- **Fix idea:** Upstream: map is\_expanded to State::Expandable/Expanded and has\_popup to State::HasPopup (plus the haspopup attribute) in accesskit\_atspi\_common. Until then Teksilo could announce the new state from the Accordion toggle (a polite announce of 'expanded'/'collapsed'), since otherwise Space on it is silent.

### dialogs-12 {#dialogs-12}

Native-window modals (Windows/macOS) drop ModalRequest::on\_dismiss: the Dialog trigger stays 'expanded' after its dialog closes, and closing a MessageBox window reports no answer

- **Example:** dialogs-and-popovers
- **Scenario:** none (source reading)
- **Act:** Source reading only: MessageBox::present / Dialog on a platform where supports\_native\_modal\_windows() is true
- **The reader should get:** Closing the modal window by any route runs the request's on\_dismiss: Dialog resets is\_open so the trigger reads collapsed, and MessageBox reports its escape answer (as it does in-tree on Escape or click-outside).
- **The reader gets:** process\_modal\_requests destructures the request with '..', so on\_dismiss is discarded for the native window. Dialog's is\_open (which drives set\_expanded on the trigger) is reset only by that callback (dialog.rs:766-768 says so). On Windows, where UIA exposes ExpandCollapse, NVDA would read the trigger as 'expanded' after the dialog has closed. A MessageBox window closed from its title bar or Alt+F4 never calls on\_result: 'Save changes?' closed that way tells the caller nothing. Escape still answers, through MessageBox's own shortcut.
- **Platform:** Windows, macOS (by source; not measured). The Linux in-tree path is correct (measured: the trigger's state follows and every close route reports a result).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-app/src/app.rs:699-790
- **Evidence:**
  - `crates/teksilo-app/src/app.rs:723-730: 'let ModalRequest { content, title, size, focus_target, close_behavior, .. } = queued.request;' (on_dismiss not read); crates/teksilo-platform/src/window_system.rs:106-118 supports_native_modal_windows() is true outside the Linux/BSD family`
  - `crates/teksilo-widgets/src/dialog.rs:764-775: 'The dismiss callback resets it to false ... Only in-tree presentations fire this callback.'`
  - `crates/teksilo-widgets/src/message_box.rs:810-850: the result on non-button dismissal comes only from on_dismiss (present)`
  - `accesskit_windows-0.35.0/src/node.rs:714-721 (expand_collapse_state from is_expanded)`
  - `not reproduced here: the harness is Linux-only`
  - `crates/teksilo-core/src/modal.rs:69-73 (on_dismiss doc: native-window modals do not fire it)`
  - `crates/teksilo-widgets/src/message_box.rs:799-803 (on_result promises every close route)`
- **Reproduced:** no (source reading; Windows/macOS not available to the harness)
- **Verification:** confirmed. Reproduced: no: source reading only. Windows and macOS are not available to the harness.
- **Fix idea:** When a native modal window closes (by any route), run the request's on\_dismiss with a DismissReason (for example WindowClosed) on the parent window's tree.

### dialogs-13 {#dialogs-13}

The example's trigger names do not match their visible text ('Show popover' shows 'Popover actions'; 'Open dialog' shows 'Review changes')

- **Example:** dialogs-and-popovers
- **Scenario:** tabwalk
- **Act:** tree at launch; Tab to the Open dialog trigger
- **The reader should get:** An accessible name that contains the visible label (WCAG 2.5.3), so a reader and a voice-control user find the control by the words on screen.
- **The reader gets:** The Button-role nodes are named 'Show popover' and 'Open dialog', while the painted text is 'Context / Popover actions' and 'Modal / Review changes'. A sighted helper saying 'press Review changes' and a reader hearing 'Open dialog' are talking about different words.
- **Platform:** all
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/dialogs\_and\_popovers/src/main.rs:263-283,327-328,333,360
- **Evidence:**
  - `tree-launch.txt: "[push button] 'Show popover'" > "[label] 'Context'" "[label] 'Popover actions'"; "[push button] 'Open dialog'" > "[panel] '' {focusable}" > "[label] 'Modal'" "[label] 'Review changes'"`
  - `examples/dialogs_and_popovers/src/main.rs:273-275 (OverlayTrigger::around(popover_trigger).named("Show popover")), :281 (Dialog::new(lit!("Open dialog")) with .trigger(dialog_trigger) showing 'Review changes')`
  - `tree-launch.txt (r1): "[push button] 'Show popover'" > "[panel] ''" > "[label] 'Context'" "[label] 'Popover actions'"; "[push button] 'Open dialog'" > "[panel] '' {focusable}" > "[label] 'Modal'" "[label] 'Review changes'"`
- **Reproduced:** yes (static tree)
- **Verification:** corrected by the verifier. Reproduced: yes: static, in all 3 launch trees (dialogs-popover r1-r3). Real, but the example line numbers are wrong. OverlayTrigger::around(popover\_trigger).named("Show popover") is at main.rs:327-328, and the visible text 'Context' / 'Popover actions' at :263-272. Dialog::new(lit!("Open dialog")) is at :333 with .trigger(dialog\_trigger) at :360, and the visible 'Modal' / 'Review changes' at :274-283. The sweep cited :273-275 and :281. The framework names a custom Dialog trigger with the Dialog's label (dialog.rs .name(label)), which is a reasonable design, so the mismatch is the example's choice.
- **Fix idea:** Name the triggers after their visible text ('Popover actions', 'Review changes'), or change the visible text to match.

### dialogs-v-01 {#dialogs-v-01}

The MessageBox's 'Show details' disclosure (every Accordion) publishes its content INSIDE the push button, where Orca's flat review never looks

- **Example:** dialogs-and-popovers
- **Scenario:** dialogs-could-not-open
- **Act:** dialogs-could-not-open: Tab from Retry to 'Show details', Space to expand, read the tree
- **The reader should get:** The disclosure button is a leaf, and the details region is its sibling (controlled by it). Every route a reader uses to read a dialog then reaches 'Underlying OS error: EACCES ...'.
- **The reader gets:** The tree is \[push button\] 'Show details' &gt; \[landmark\] 'Show details' &gt; \[panel\] '' &gt; \[label\] 'Underlying OS error: EACCES (permission denied)...'. Orca's flat review, the route for reading a non-document window, treats a push button as a single leaf and does not descend (script\_utilities.py:1505-1506 returns \[root\] for any button in getOnScreenObjects, used by flat\_review.py:750). The details are neither in the alert's description nor focusable, and expanding them is silent (dialogs-11). So an Orca user is not told they exist, and only Orca 46's object navigator (object\_navigator.py:190-203), which descends into any child, can reach them. The Accordion doc says 'The header is announced as Role::Button', but the code gives Role::Button to the Accordion node itself, whose children() is its whole root, region included. The sweep's 'passed' line about the details being reachable comes from the harness's raw walk, which ignores Orca's rule.
- **Platform:** Linux: the tree is measured, the Orca consequence is from Orca 46.1's source. Windows and macOS were not measured: the same button-with-children structure is exported, and whether NVDA/VoiceOver descend into a button's children was not verified.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/accordion.rs:668-697
- **Evidence:**
  - `r2 dialogs-could-not-open tree-Space-expands-the-details.txt lines 36-39: "[push button] 'Show details' {focusable,focused} rel=['controller-for']" / "  [landmark] 'Show details'" / "    [panel] ''" / "      [label] 'Underlying OS error: EACCES (permission denied)\nopen(\"report.skrib\", O_RDWR) → errno 13'" (r1 identical); collapsed, the [landmark] > [panel] still hang under the button (tree-Tab-from-Retry...txt:36-38)`
  - `[alert] 'Could not open file' desc='report.skrib could not be opened.\nYou may not have permission.' (the details are not in the description)`
  - `crates/teksilo-widgets/src/accordion.rs:668-678 (Role::Button, name, expanded, Click on the Accordion node; push_controlled(region)), :680-682 + :688-697 (children() = the root holding header AND region); module doc :13-19 says the HEADER is the button`
  - `/usr/lib/python3/dist-packages/orca/script_utilities.py:1505-1506 "if AXUtilities.is_button(root) or AXUtilities.is_combo_box(root): return [root]"`
- **Reproduced:** yes: 2 of 2 runs (dialogs-could-not-open r1, r2). Deterministic structure.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Give the Accordion node a GenericContainer/Group role, and put Role::Button, expanded, Click and controls on the header node, so the region is the header's sibling. Consider putting the detailed text in the alert's description, or announcing it on expand.

### dialogs-v-02 {#dialogs-v-02}

Tab out of an open popover throws focus to the first control of the window (the toolbar's Theme box), not to the control after the popover

- **Example:** dialogs-and-popovers
- **Scenario:** verify-dialogs-popover-more
- **Act:** verify-dialogs-popover-more: with focus on 'Show snackbar', AT-SPI click 'Show popover' (focus goes to the PopoverBody host), then Tab
- **The reader should get:** The popover closes (disclosure pattern), and focus moves to the next control after the trigger ('Open dialog') or back to where it was. The reader hears that control.
- **The reader gets:** The popover closes, and focus lands on \[combo box\] 'Theme' at the top of the window, so Orca says 'Toolbar tool bar' / 'Theme combo box.'. The reader has lost their place in the page. The cause is the focus parked on the non-focusable PopoverBody host (dialogs-03). That node is not a Tab-cycle entry, so navigate\_scope starts the cycle from the beginning.
- **Platform:** all (framework focus traversal). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `de3bb295` (popover-trigger). Fixed part: Tab from the open popover goes to 'Open dialog', not to the Theme box (fix-popover-trigger-tab-out 3 of 3, verify-dialogs-popover-more 3 of 3.
- **Where:** crates/teksilo-widgets/src/popover\_widget.rs:685,779; crates/teksilo-core/src/widget\_tree/pointer\_router.rs:3040
- **Evidence:**
  - `r2 'Tab from inside the popover': "+10.0 ms object:children-changed:remove [panel] '' -> [unknown] ''" / "+10.2 ms object:state-changed:focused 1 [combo box] 'Theme'" / "+86.2 ms ORCA SAYS: 'Toolbar tool bar'" / "+86.2 ms ORCA SAYS: 'Theme combo box.'"`
  - `r3 same act: "+10.5 ms object:state-changed:focused 1 [combo box] 'Theme'" / "+78.5 ms ORCA SAYS: 'Toolbar tool bar'" / "'Theme combo box.'"`
  - `crates/teksilo-core/src/widget_tree/pointer_router.rs:3040 (request_focus falls back to the non-focusable host); crates/teksilo-core/src/widget_tree/focus_impl.rs:474 (navigate_scope from a self.focused that is not an entry); popover_widget.rs's own test doc says the content's natural Tab slot follows the trigger`
- **Reproduced:** yes: 2 of 2 runs (verify-dialogs-popover-more r2, r3).
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Do not park focus on a non-focusable host (request\_focus\_into, or keep focus on the trigger when the panel has no control). When focus leaves a popover by Tab, continue from the trigger's position in the cycle.
