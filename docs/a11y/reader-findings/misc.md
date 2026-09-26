<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Pickers, files and drag and drop

Examples: `color-picker-demo`, `font-picker`, `recent-projects`, `file-dialogs`, `file-drop`, `drag-and-drop`, `text-and-layout`.
27 findings: 2 critical, 13 high, 8 medium, 4 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [misc-01](#misc-01) | color-picker-demo | A ColorPicker's Live::Polite root makes every named control inside it announce itself whenever the picker enters the tree: at launch, on scroll-in, on a ColorEdit popover | medium | Linux | open |
| [misc-02](#misc-02) | color-picker-demo | The ColorPicker's 'Color changed to #…' announcement never fires: the message is the value of a live group, and adapters announce only names | high | Linux | open |
| [misc-03](#misc-03) | color-picker-demo | Every RGB / HSV / alpha spinner of the ColorPicker has no accessible name ('53 spin button.') | high | all | open |
| [misc-04](#misc-04) | font-picker | Checking 'Monospace only' (a Checkbox) never reaches the bus until focus moves away; neither Space nor AT-SPI click is heard | high | Linux | fixed |
| [misc-05](#misc-05) | font-picker | Arrowing through an open ComboBox list is silent: the font list (searchable) and the writing-system list (not searchable) move a selection no reader hears | critical | Linux | fixed |
| [misc-06](#misc-06) | font-picker | A ComboBox's current value is not exported on AT-SPI: 'Font family combo box.' after picking a font, 'Theme combo box.' everywhere | high | Linux | upstream |
| [misc-07](#misc-07) | drag-and-drop | The row context menu (the non-drag reorder's menu route) is silent: focus sits on an unnamed \[menu\], arrows move a private index | high | Linux | fixed |
| [misc-08](#misc-08) | drag-and-drop | No keyboard route for cross-view drags: songs cannot reach the Playlist, Up next or Trash, and file-drop's drag-out rows and internal DropTarget cannot be reached at all | critical | all | open |
| [misc-09](#misc-09) | drag-and-drop | A keyboard reorder's 'Moved to N of 10' is sent in the same update as focus moving to the rebuilt row, so Orca cuts it: no move was ever heard | high | Linux | fixed |
| [misc-10](#misc-10) | recent-projects | recent-projects: after Pin or Remove on any row, focus jumps to the first row's 'Open' and nothing says what happened | high | Linux | open |
| [misc-11](#misc-11) | recent-projects | recent-projects: pressing 'Hide paths' drops focus to the window ('frame.'); the next Tab restarts at the toolbar | medium | Linux | open (example) |
| [misc-12](#misc-12) | color-picker-demo | The ColorPicker preset grid: arrows move an invisible index (silent), Enter applies a swatch the reader never heard, and every swatch is also a Tab stop | high | all | open |
| [misc-13](#misc-13) | color-picker-demo | A picked swatch never becomes 'selected' for AT: its name and state are computed once at build | high | all | open |
| [misc-14](#misc-14) | color-picker-demo | HexColorInput is two nested entries with the same name, so every focus speaks the field twice | medium | Linux | open |
| [misc-15](#misc-15) | color-picker-demo | The ColorPicker's preview and saturation × brightness field carry their value only as a string value, which AT-SPI does not export | medium | Linux | open |
| [misc-16](#misc-16) | color-picker-demo | ColorEdit triggers are named only by their hex, and the nullable one by an em dash ('— push button.'); its popover dialog is unnamed | high | all | open |
| [misc-17](#misc-17) | color-picker-demo | A plain window opens with focus in its first TextInput, wherever it is: color-picker-demo starts inside the first picker's Hex field, text selected | medium | all | open |
| [misc-18](#misc-18) | drag-and-drop | drag-and-drop: the Songs, Playlist and Up next lists and the Folders tree are unnamed ('multi-select list box.', 'tree.') | high | all | open (example) |
| [misc-19](#misc-19) | drag-and-drop | TreeView hierarchy is invisible on AT-SPI: no expandable/expanded state and no level ('Documents.' before and after Right arrow) | high | Linux | upstream |
| [misc-20](#misc-20) | file-dialogs | Status lines change silently: a file dialog's result, the font size, the seeding of recents | medium | all | open (example) |
| [misc-21](#misc-21) | recent-projects | recent-projects row buttons are all 'Open', 'Pin', 'Remove': no project in the name, pin state only as a '★' in the title | medium | all | open (example) |
| [misc-22](#misc-22) | font-picker | The searchable ComboBox popup: unnamed search entry (placeholder only), an \[unknown\] popup root, and every option doubled (unnamed list item wrapping the named one) | high | Linux | fixed |
| [misc-23](#misc-23) | text-and-layout | Section titles are plain labels, never headings (text-and-layout and every other example); TextWidget offers no heading API | low | all | open |
| [misc-24](#misc-24) | color-picker-demo | Text changes of nodes outside the exported tree (off-screen pickers sharing the colour) still emit text-changed events, 22 per keypress, which Orca drops as defunct | low | Linux | upstream |
| [misc-25](#misc-25) | drag-and-drop | The non-drag alternatives' AccessKit custom actions (Move Up/Down/…, the colour field's four steps) are not exported on AT-SPI | low | Linux | upstream |
| [misc-M1](#misc-m1) | drag-and-drop | Every StandardListItem / StandardTreeItem row exposes a second, role-less node with the row's name (\[unknown\] 'Hyperballad' under \[list item\] 'Hyperballad') | low | Linux | open |
| [misc-M2](#misc-m2) | color-picker-demo | Framework accessibility strings hard-coded in English: the ColorPicker's saturation × brightness field (name, value, announcement, four custom actions) and the searchable ComboBox's 'Search…' field | medium | all | open |

### misc-01 {#misc-01}

A ColorPicker's Live::Polite root makes every named control inside it announce itself whenever the picker enters the tree: at launch, on scroll-in, on a ColorEdit popover

- **Example:** color-picker-demo
- **Scenario:** misc-color-scroll (and every color-picker launch)
- **Act:** launch color-picker-demo; Tab from the 2nd picker's last swatch into the Compact picker (scrolls); Shift+Tab from Theme to the bottom of the page; Space on a ColorEdit trigger
- **The reader should get:** the reader hears the window and the focused control; scrolling or opening a popover says only where focus went
- **The reader gets:** launch: 52 object:announcement events (every swatch, 'Hex', 'R', 'G', 'Hue', 'Color presets', ...) each spoken with interrupt=True, then 'frame.', 'Color picker panel.', 'Hex entry #3584E4'. Scroll into the Compact picker: 5 announcements; Shift+Tab to the page bottom: 53 announcements; ColorEdit popover: 7 announcements ('Hue','Hex','Color picker','Cancel','Hex','Done','Saturation and brightness') before 'dialog'
- **Platform:** Linux AT-SPI/Orca measured. Windows and macOS by source: accesskit\_windows adapter.rs:256-263 raises UIA LiveRegionChanged, and accesskit\_macos event.rs:236-240 posts an announcement, for every added node with a name whose inherited live() is not Off, so both get the same flood
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker.rs:813-833 (builder.set\_live(Live::Polite) on the root Group)
- **Evidence:**
  - `tree-color-picker-demo report.txt launch: '+1601.8 ms object:announcement [push button] 'Swatch #F29933' text='Swatch #F29933'' ... '+1679.8 ms object:announcement [label] 'B' text='B'' then '+1685.9 ms object:state-changed:focused 1 [entry] 'Hex''`
  - `misc-color-channels-20260925-150110 orca-debug.out: '15:01:13.476144 - SPEECH OUTPUT: 'Swatch #F29933'' / '15:01:13.476148 - NULL SPEECH: speak 'Swatch #F29933' interrupt=True' ... '15:01:13.672731 - SPEECH OUTPUT: 'frame.'' / '15:01:13.777490 - SPEECH OUTPUT: 'Color picker panel.''`
  - `launch tally: 52 announcements, 52-53 utterances cut, in 7 of 7 launches (tree, tabwalk, channels x2, hex x2, scroll x3, swatches x2)`
  - `misc-color-scroll report.txt 'Tab into the Compact picker': '+40.7 ms object:children-changed:add [panel] '' -> [panel] 'Color picker'' then '+60.1 ms object:announcement [slider] 'Hue'' ... '+64.2 ms object:announcement [entry] 'Hex'' then 'ORCA SAYS (CUT): 'Hue'' x5; 'Shift+Tab wraps': 'FAIL at most 1 object:announcement events / 53 object:announcement events'; popover: '7 object:announcement events' then 'ORCA SAYS: 'dialog'' (3 of 3 runs identical counts)`
  - `source: color_picker.rs:813-833 builder.set_live(Live::Polite) on the root Group; accesskit_consumer node.rs:906-910 live() inherits from the parent; accesskit_atspi_common adapter.rs:72-77 emits Announcement(name) for every added live node`
  - `misc-color-scroll-20260925-151249 'Tab into the Compact picker': '+44.8 ms object:announcement [entry] 'Hex'' .. '+53.4 ms object:announcement [slider] 'Hue'' then '+55.4 ms object:state-changed:focused 1 [panel] 'Saturation and brightness''; ORCA SAYS (CUT) x5 then '+203.6 ms ORCA SAYS: 'Color picker panel.'' / '+203.7 ms ORCA SAYS: 'Saturation and brightness panel.''`
  - `misc-color-channels-20260925-151406 orca-debug.out: '15:14:09.566211 - NULL SPEECH: speak 'Swatch #F5F5F5' interrupt=True', '15:14:09.572874 - NULL SPEECH: speak 'Opacity' interrupt=True'`
  - `launch tally over my runs (channels x2, hex x2, scroll x3, swatches x2, color-fr): every one 'ann 52, cut 52-53, uncut ['Color picker panel.', 'Hex entry #3584E4', 'Hex entry #3584E4 selected.']'`
- **Reproduced:** deterministic on the bus; cuts 7 of 7 launches, scroll/popover 3 of 3 runs
- **Verification:** corrected by the verifier. Reproduced: 10 of 10 launches (52 object:announcement each, 52-53 utterances cut), 3 of 3 scroll-in acts (5 announcements), 3 of 3 Shift+Tab-to-bottom acts (53), 3 of 3 popover opens (7) The flood is real and deterministic: Live::Polite on the picker's Role::Group is inherited by every descendant (accesskit\_consumer node.rs:906-910), and atspi\_common announces every added live node with a name (adapter.rs:72-77); Orca presents each through presentMessage with interrupt=True (default.py:1424-1428), so each cuts the one before. Severity is corrected from high to medium: in every act I ran, the reading of the real focus survived uncut after the flood ('Color picker panel.' / 'Hex entry #3584E4' 10 of 10 launches; 'Saturation and brightness panel.' 3 of 3 scroll-ins; '— push button.' 3 of 3, delayed to +561..+674 ms; 'dialog' 3 of 3). The reader gets about half a second of word fragments and a delay. Nothing essential is lost, so this is noisy speech, not missing information. Platform claim corrected: for scroll-in and popover the Windows and macOS source supports the claim. UIA raises LiveRegionChanged both for an added node and for one that was filtered out and comes back (accesskit\_windows adapter.rs:256-263, 310-322), and macOS behaves the same way (event.rs:236-240, 301-310). Whether the launch flood happens there depends on whether the pickers arrive after the adapter's first tree, which I did not establish.
- **Fix idea:** Do not make the container live. Announce a committed colour with ctx.announce (or one dedicated live label whose name is the message), so descendants do not inherit Live::Polite.

### misc-02 {#misc-02}

The ColorPicker's 'Color changed to #…' announcement never fires: the message is the value of a live group, and adapters announce only names

- **Example:** color-picker-demo
- **Scenario:** misc-color-hex, misc-color-channels, misc-color-swatches
- **Act:** commit #FF8800 in the Hex field (Ctrl+A, type, Enter); Up arrow on the R spinner; Space on a preset swatch
- **The reader should get:** 'Color changed to #FF8800' (the picker's documented live region, color\_picker.rs:27-29)
- **The reader gets:** Orca says nothing after committing the hex; only '54' after the spinner step; nothing after picking a swatch. The bus carries only accessible-value changes of the spinners and the Hue slider
- **Platform:** Linux measured. Windows (accesskit\_windows adapter.rs:310-322) and macOS (accesskit\_macos event.rs:301-310) also raise their live-region event only on a name change, so the message is lost everywhere
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker.rs:813-833 (set\_value of color-picker-changed-announcement on the live Group); cache effect at ~464-484
- **Evidence:**
  - `misc-color-hex-20260925-150036 'select all and type #FF8800, then Enter': events '+602.4 ms object:property-change:accessible-value [spin button] ''' ... '+611.7 ms object:property-change:accessible-value [slider] 'Hue''; 'FAIL Orca says one of ['Color changed to #FF8800'] / Orca said nothing in the act'; orca-debug.out has no SPEECH OUTPUT between 15:00:49.839 and 15:00:53.586`
  - `misc-color-channels 'Up arrow on it': 'ORCA SAYS: '54'' / 'FAIL Orca says one of ['Color changed to #3684E4', '3684E4']'`
  - `tree: [panel] 'Color picker' {focusable} carries no value on AT-SPI`
  - `source: color_picker.rs:827-830 builder.set_value(color-picker-changed-announcement); atspi node.rs:38-44 names from value only for Role::Label (consumer node.rs:744-746); node.rs:610-622 announces on name change only`
  - `misc-color-hex-20260925-151454 'select all and type #FF8800, then Enter': '+638.6 ms object:property-change:accessible-value [spin button] ''' .. '+646.0 ms object:property-change:accessible-value [slider] 'Hue'' / 'FAIL Orca says one of ['Color changed to #FF8800'] / Orca said nothing in the act'`
  - `misc-color-channels-20260925-151406 'Up arrow on it': 'ORCA SAYS: '54'' only`
- **Reproduced:** deterministic (no announcement event emitted); 2 of 2 hex runs, 2 of 2 channel runs, 2 of 2 swatch runs
- **Verification:** confirmed. Reproduced: deterministic, no object:announcement emitted: hex commit 2 of 2 runs, spinner step 2 of 2, swatch pick 2 of 2
- **Fix idea:** On settle (not mid-drag), call ctx.announce with the localized 'Color changed to {hex}' instead of writing it into the group's value.

### misc-03 {#misc-03}

Every RGB / HSV / alpha spinner of the ColorPicker has no accessible name ('53 spin button.')

- **Example:** color-picker-demo
- **Scenario:** misc-color-channels
- **Act:** Tab from the Hex field through the spinners
- **The reader should get:** 'Red 53 spin button', 'Green 132 …', 'Hue 213 …' (the long labels color-picker-red-label etc. exist in en-US.ftl:314-316 and are unused)
- **The reader gets:** '53 spin button.', '132 spin button.', '228 spin button.', '213 spin button.', '77 spin button.', '89 spin button.'; the painted 'R'/'G'/… is a separate \[label\] not linked to the spinner
- **Platform:** all platforms (the name is missing in the AccessKit tree); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker.rs:862-989 (make\_\*\_spinner\_from\_value never call .label(); spinner\_cell 981-987 adds an unlinked TextWidget)
- **Evidence:**
  - `tree audit at launch: 13 x 'unnamed-control: [spin button] '': a focusable spin button with no name (its text is '53')' …`
  - `tabwalk-color-picker-demo: '+30.8 ms object:state-changed:focused 1 [spin button] ''' / 'ORCA SAYS: '53 spin button.''`
  - `misc-color-channels: 'FAIL Orca says one of ['Red', 'R spin'] / Orca said: '53 spin button.''`
  - `source: color_picker.rs:978-986 spinner_cell() puts TextWidget(lit!(label)) beside the SpinBox; make_*_spinner_from_value (l.859-975) never call .label()`
  - `verify-misc-color-fr-20260925-152754 launch audit: 'unnamed-control: [spin button] '': a focusable spin button with no name (its text is '53')' .. 13 entries`
  - `misc-color-channels-20260925-151406 'Tab from the Hex field…': '+29.2 ms object:state-changed:focused 1 [spin button] ''' / 'ORCA SAYS: '53 spin button.''`
- **Reproduced:** deterministic, every run (tree, tabwalk, channels x2)
- **Verification:** confirmed. Reproduced: deterministic: launch audit lists 13 unnamed spin buttons in every color-picker run; '53 spin button.' / '132 spin button.' 2 of 2 channel runs
- **Fix idea:** Pass .label(resolve\_message\_widget("color-picker-red-label")) etc. to each SpinBox and a11y\_hide the short visual letter (or labelled\_by it).

### misc-04 {#misc-04}

Checking 'Monospace only' (a Checkbox) never reaches the bus until focus moves away; neither Space nor AT-SPI click is heard

- **Example:** font-picker
- **Scenario:** misc-font-mono-toggle, misc-font-picker
- **Act:** Space on the focused 'Monospace only' check box (and AT-SPI click)
- **The reader should get:** object:state-changed:checked at once and 'checked' spoken
- **The reader gets:** no state-changed:checked within 4 s of 2 Spaces and 1 AT-SPI click per run, nor with a following Shift; when focus later moves (Shift+Tab) the stale change is flushed in the same update and Orca's 'checked' is cut by the new focus
- **Platform:** Linux measured; the missing re-walk is in teksilo-core, so all platforms
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Where:** crates/teksilo-widgets/src/checkbox.rs:383-387 and styles/recipe\_checkbox\_style.rs:146-155 (check state bound RepaintOnly only); checkbox.rs:496-600 handlers never request an a11y re-walk
- **Evidence:**
  - `misc-font-picker-20260925-150305 'Space checks Monospace only' (15:03:47.13-15:03:50.32): 'FAIL a object:state-changed:checked event from [check box] '*' / no object:state-changed:checked event'`
  - `same run events.jsonl: '15:03:51.547505 object:state-changed:checked 1' arrives with the Shift+Tab act (started 15:03:51.526); orca-debug.out '15:03:51.558289 - SPEECH OUTPUT: 'checked'' / '15:03:51.603931 - NULL SPEECH: stop' / '15:03:51.604042 - SPEECH OUTPUT: 'Font family combo box.''`
  - `misc-font-mono-toggle x4 runs: 0 object:state-changed:checked events in events.jsonl (9 activations in the 3 isolation runs, including 'AT-SPI click on Monospace only: FAIL … no object:state-changed:checked event')`
  - `corroborated in another agent's run (read-only): target/reader-sweep/catalog-a 'Space on the two-state check box': 'no object:state-changed:checked event'; same for Toggle and RadioButton`
  - `source: checkbox.rs:383-387 binds the check state only through style_state (RepaintOnly body); binding.rs:245-265 only AccessibilityOnly bindings set a11y_dirty; accessibility_impl.rs:20-30 re-walks only on focus/overlay/rebuild/activation/AccessibilityOnly`
  - `verify-misc-mono-tree-20260925-152408 'Space, then read the adapter's tree' (15:24:20.690-15:24:25.879): 'FAIL a object:state-changed:checked event' / 'FAIL the tree the adapter holds shows the box checked / no node matched'; 'do nothing for 6 s': no checked event`
  - `same run events: '15:24:34.457336 object:state-changed:checked 1 Monospace only' (in the 'Tab to Writing system' act); orca-debug.out '15:24:34.468553 - SPEECH OUTPUT: 'checked'' / '15:24:34.506253 - NULL SPEECH: stop' / '15:24:34.506360 - SPEECH OUTPUT: 'Writing system combo box.''`
  - `same run 'AT-SPI click on Monospace only, then read the tree': no checked event, tree still shows checked`
- **Reproduced:** 4 of 4 mono-toggle runs and 2 of 2 font-picker runs
- **Verification:** confirmed. Reproduced: Space: 4 of 4 activations in 2 mono-toggle runs plus 2 of 2 font-picker runs plus 1 verify run; AT-SPI click: 2 of 2 runs; a stale state was flushed only by a later focus move in 3 of 3 runs where one followed
- **Fix idea:** Bind the checked/tristate signal at BindingLevel::AccessibilityOnly on the checkbox node (same for Toggle and RadioButton).

### misc-05 {#misc-05}

Arrowing through an open ComboBox list is silent: the font list (searchable) and the writing-system list (not searchable) move a selection no reader hears

- **Example:** font-picker
- **Scenario:** misc-font-picker
- **Act:** Font family: Alt+Down, Down, Down, type 'mono', Down, Enter. Writing system: Alt+Down, Down, Enter
- **The reader should get:** each Down speaks the highlighted font ('C059', 'D050000L', 'DejaVu Sans Mono') / script ('Latin')
- **The reader gets:** Alt+Down: 'list box', 'Search…'. Every Down: only object:state-changed:selected on a list item and object:selection-changed on the list box, no focus or active-descendant change, Orca silent. Typing 'mono' narrows the list (setsize 277 -&gt; 14) silently. Enter: 'Font family combo box.' Writing system: Alt+Down, Down, Enter all silent. Together with misc-06 the reader can never learn which font is highlighted or chosen
- **Platform:** Linux measured; no focus/active-descendant is emitted in the AccessKit tree, so Windows/macOS readers have nothing to follow either (not measured)
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `f9ffa98c` (combobox). Fixed part: critical): arrowing through the font list and the writing-system list now speaks each font or script (C059, D050000L, DejaVu Sans Mono, Latin); arrows no longer commit, and Enter picks.
- **Where:** crates/teksilo-widgets/src/combo\_box/panel.rs:620-700 (arrows set `selected`, no active\_descendant); combo\_box/item.rs:207-218 (Role::ListBoxOption carries the selected state inside ListView's own item wrapper); combo\_box.rs:904-916 (non-searchable arrows commit through pick\_at)
- **Evidence:**
  - `misc-font-picker-20260925-150305 'Down arrow': '+28.9 ms object:state-changed:selected 1 [list item] 'C059'' / '+30.5 ms object:selection-changed [list box] ''' / 'FAIL Orca says something / Orca said nothing in the act' / 'FAIL … no focus change on the bus in this act'`
  - `same run 'Down arrow in the writing-system list': '+9.3 ms object:state-changed:selected 1 [list item] 'Latin'' / 'FAIL Orca says 'Latin''; 'Enter picks it': 'no focus change on the bus in this act', Orca silent`
  - `orca-debug.out '15:03:18.187945 - SPEECH OUTPUT: 'list box'' / '15:03:18.187977 - SPEECH OUTPUT: 'Search…'' then no SPEECH OUTPUT through the arrows`
  - ``source: combo_box/panel.rs:600-700 ArrowDown/ArrowUp move `selected` while the search TextInput keeps focus; nothing sets active_descendant``
  - `verify-misc-combo-ws-20260925-152340 orca-debug.out: '15:23:57.812696 - OBJECT EVENT: object:selection-changed for [list box]' / '15:23:57.822014 - AXSelection: [list box] reports 0 selected children' / '15:23:57.822764 - SCRIPT UTILITIES: Selected children not retrieved via selection interface.' while the tree holds [list box] '' {focusable} > [panel] '' > [list item] '' {selectable} > [list item] 'Latin' {selectable,selected}`
  - `same run: 'object:state-changed:selected for [list item: 'Latin']' → '15:23:57.812402 - DEFAULT: Event is not toggling of currently-focused object'`
  - `verify-misc-combo-ws 'Alt+Down opens the list': only '+42.4 ms object:children-changed:add [combo box] 'Writing system' -> [unknown] ''' and a selection-changed, 'Orca said nothing in the act'; 'Escape closes the list': only children-changed:remove, silent`
  - `misc-font-picker-20260925-151338 'Down arrow': '+15.6 ms object:children-changed:add [panel] '' -> [label] 'The quick brown fox…'' (the preview re-renders: the value was committed) / '+17.0 ms object:state-changed:selected 1 [list item] 'C059'' / Orca silent`
- **Reproduced:** deterministic (no focus events at all); 2 of 2 font-picker runs
- **Verification:** corrected by the verifier. Reproduced: deterministic: font list 2 of 2 font-picker runs; writing-system list 2 of 2 font-picker runs + 1 verify-misc-combo-ws run The silence is real, but the stated cause is incomplete on Linux. The missing active\_descendant matters on every platform. On Linux there is a second, measurable cause. Orca does handle the list box's object:selection-changed and would move to and speak the selected child (default.py:1579-1602). But AT-SPI's Selection interface reports 0 selected children. The selected option sits one level below ListView's own unselected \[list item\] '' wrapper, and a \[panel\] also stands between it and the list box. AccessKit's items() includes the first item-like descendant and never looks inside it (consumer node.rs:1064-1073, atspi node.rs:1268-1285). Opening and closing the non-searchable combo is also silent: focus stays on the trigger, and AT-SPI exports no expanded state (atspi node.rs:301-385). I keep severity at critical for Linux, because the arrows are not only silent, they commit. In the searchable panel, selected\_for\_nav.set (panel.rs:695-698) writes the bound family at once, and the preview re-renders on every Down. In the non-searchable combo, pick\_at sets the value and fires on\_select. Escape reverts nothing. With misc-06, a Linux reader changes the font or script with every arrow and has no route to learn what it now is. Platform claim corrected: for the non-searchable combo, Windows is probably not silent, because the focused combo's string value is on ValuePattern and changes raise UIA\_ValueValuePropertyId (accesskit\_windows node.rs:592-597, 1346). Not measured.
- **Fix idea:** Publish the highlighted option as active\_descendant of the focused node (search entry, or the trigger for a non-searchable combo), the combobox/listbox pattern, so an active-descendant-changed event names the row.

### misc-06 {#misc-06}

A ComboBox's current value is not exported on AT-SPI: 'Font family combo box.' after picking a font, 'Theme combo box.' everywhere

- **Example:** font-picker
- **Scenario:** misc-font-picker, misc-text-layout
- **Act:** Tab to Theme (every example); Enter to pick 'DejaVu Sans Mono', then Shift+Tab back to Font family
- **The reader should get:** 'Font family combo box DejaVu Sans Mono', 'Theme combo box Light'
- **The reader gets:** 'Font family combo box.' and 'Theme combo box.' only; the tree shows no value, text or child for the combo boxes
- **Platform:** Linux (AT-SPI) only by source: accesskit\_windows node.rs:592-597 exposes a string value through ValuePattern; accesskit\_atspi\_common exposes it nowhere
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/combo\_box.rs:1247-1263 (value set correctly); gap in accesskit\_atspi\_common-0.20.0/src/node.rs:38-44, 486-492
- **Evidence:**
  - `misc-font-picker 'Enter to pick it': '+8.5 ms object:state-changed:focused 1 [combo box] 'Font family'' / 'ORCA SAYS: 'Font family combo box.'' / 'FAIL Orca says one of ['Mono', 'mono']'`
  - `misc-text-layout 'Tab to the theme switcher': 'ORCA SAYS: 'Theme combo box.'' / 'FAIL Orca says one of ['Light', 'Dark', 'System']'`
  - `tree-text-and-layout run.json: combo box 'Theme' interfaces ['Accessible','Action','Component','Selection'], child_count 0`
  - `source: combo_box.rs:1249-1263 set_value(label); accesskit_atspi_common node.rs:38-44 uses value only as the name of a Role::Label, node.rs:486-492 Text/Value interfaces only for text ranges / numeric values`
  - `verify-misc-combo-ws 'Shift+Tab then Tab back to Writing system': '+1026.4 ms object:state-changed:focused 1 [combo box] 'Writing system'' / 'ORCA SAYS: 'Writing system combo box.'' / 'FAIL Orca says 'Greek''`
  - `misc-text-layout-20260925-151707 and -152854 'Tab to the theme switcher': 'ORCA SAYS: 'Theme combo box.'' / FAIL Light|Dark|System`
- **Reproduced:** deterministic, every run that focused a combo box
- **Verification:** confirmed. Reproduced: deterministic, every focus of a combo box in all runs (Theme, Font family, Writing system)
- **Fix idea:** Upstream: export a string value on AT-SPI (e.g. Text interface for a collapsed combo). Teksilo meanwhile: give the collapsed combo a hidden Role::Label child (or description) carrying the selected text so Orca reads it.

### misc-07 {#misc-07}

The row context menu (the non-drag reorder's menu route) is silent: focus sits on an unnamed \[menu\], arrows move a private index

- **Example:** drag-and-drop
- **Scenario:** misc-drag-and-drop
- **Act:** drag-and-drop: Menu key on the Unravel row, Down, Enter
- **The reader should get:** 'Move Up menu item' on Down; Enter runs the heard command
- **The reader gets:** 'menu.' on open; Down emits nothing on the bus and Orca says nothing; Enter runs 'Move Up' (announced 'Moved to 2 of 10', itself lost, see misc-09) although the reader never heard which row was current
- **Platform:** Linux measured; no focus or active-descendant is published, so no adapter can report it
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `9636094c` (menus).
- **Where:** crates/teksilo-widgets/src/menu\_list.rs:227-233 (documented: arrows move focused\_index, not tree focus), 991-993 (Role::Menu, no active\_descendant, no name)
- **Evidence:**
  - `misc-drag-and-drop-20260925-150304 'open the row's context menu': '+22.1 ms object:state-changed:focused 1 [menu] ''' / 'ORCA SAYS: 'menu.''; tree: [menu] '' {focusable,focused} with [menu item] 'Move Up' / 'Move Down' / 'Move to Top' / 'Move to Bottom' (not focusable)`
  - `'Down arrow in the menu': no events; 'FAIL Orca says 'Move''`
  - `'Enter on the current menu row': '+26.3 ms object:announcement [status bar] … text='Moved to 2 of 10''`
  - `source: menu_list.rs:229-233 ('Arrow / Home / End / type-ahead navigation moves focused_index, not real tree focus'), 748-762, and accessibility() at 991-993 sets Role::Menu only (no active_descendant, no name)`
  - `misc-drag-and-drop-20260925-152251 'open the row's context menu': '+22.8 ms object:state-changed:focused 1 [menu] ''' / 'ORCA SAYS: 'menu.''; 'Down arrow in the menu': no events, 'FAIL Orca says 'Move''`
  - `same run 'Enter on the current menu row': '+34.9 ms object:announcement [status bar] … text='Moved to 2 of 10'' then focus back on [list item] 'Unravel'`
- **Reproduced:** 3 of 3 drag-and-drop runs
- **Verification:** confirmed. Reproduced: 3 of 3 drag-and-drop runs (Down in the menu: no event on the bus, Orca silent)
- **Fix idea:** Set active\_descendant on the Menu node to the highlighted MenuItem (and mark it selected), or move real focus to the item.

### misc-08 {#misc-08}

No keyboard route for cross-view drags: songs cannot reach the Playlist, Up next or Trash, and file-drop's drag-out rows and internal DropTarget cannot be reached at all

- **Example:** drag-and-drop
- **Scenario:** misc-drag-and-drop, misc-file-drop
- **Act:** drag-and-drop: context menu on a song; Tab through; file-drop: Tab walk and tree
- **The reader should get:** a menu row / chord such as 'Add to Playlist' (WCAG 2.5.7 / 2.1.1), and focusable drag-out rows
- **The reader gets:** the row menu offers only Move Up/Down/to Top/to Bottom; Playlist, Up next and Trash are drop-only; in file-drop Tab goes straight from the window to the first Browse…, the '⠿ Drag this file out' rows are unfocusable panels with no action, the internal DropTarget is an unnamed panel
- **Platform:** all (no route exists in the widget tree); measured on Linux
- **Severity:** critical; **layer:** framework
- **Status:** Open.
- **Where:** docs/a11y/non-drag-alternatives.md:176-183 (the documented gap: keyboard pick-up / put-down, census row 30); crates/teksilo-widgets/src/list\_view/body\_pane.rs:473-489 (RowCommands extra: Vec::new()); crates/teksilo-widgets/src/drop\_target.rs:864-868 (unnamed Role::Group); examples/file\_drop/src/main.rs:97-176
- **Evidence:**
  - `misc-drag-and-drop 'open the row's context menu': 'FAIL the menu offers a way to copy the song to the playlist / no node matched'`
  - `misc-file-drop 'the tree': 'FAIL the drag-out rows are focusable, or offer an action / no node matched'; 'FAIL the internal drop target is named / no node matched'`
  - `tabwalk-file-drop: Tab 1 -> '[push button] 'Browse…'' ('Drop images here panel.'), Tab 2 -> second Browse…, Tab 3 -> back to the first`
  - `tree-file-drop: [panel] '' > [label] '⠿  Drag this file out →  (main.rs)' (no focusable state)`
  - `docs/a11y/non-drag-alternatives.md 'What is still drag-only': census 14, 19, 29 (cross-widget row export, DropTarget drops) need a keyboard pick-up / put-down mode`
  - `misc-file-drop-20260925-152907 'the tree: a keyboard route…': 'FAIL the drag-out rows are focusable, or offer an action / no node matched', 'FAIL the internal drop target is named / no node matched'`
  - `misc-drag-and-drop-20260925-152826 'open the row's context menu': 'FAIL the menu offers a way to copy the song to the playlist'`
- **Reproduced:** deterministic, 3 of 3 drag-and-drop runs, 3 of 3 file-drop runs
- **Verification:** confirmed. Reproduced: deterministic: 3 of 3 drag-and-drop runs (menu holds only Move rows), 2 of 2 file-drop runs (drag-out rows and internal DropTarget unreachable)
- **Fix idea:** The documented keyboard pick-up / put-down mode (census row 30); until then the examples could add 'Add to playlist' / 'Remove' commands on rows and make drag sources focusable with a copy command.

### misc-09 {#misc-09}

A keyboard reorder's 'Moved to N of 10' is sent in the same update as focus moving to the rebuilt row, so Orca cuts it: no move was ever heard

- **Example:** drag-and-drop
- **Scenario:** misc-drag-and-drop
- **Act:** Alt+Down, Alt+Down, Alt+Up, and the menu's Move Up, on 'Unravel' in Songs
- **The reader should get:** 'Moved to 3 of 10' (the one utterance the non-drag alternative owes)
- **The reader gets:** 'Unravel.' only. The announcement reaches the bus 0.1-11 ms before object:state-changed:focused on a new 'Unravel' node (all ten rows are removed and re-added on every move); the first message of a run is cut (r1, r3) or had already lost its node (r2), later ones are also dropped as defunct (K2)
- **Platform:** Linux measured; the ordering (announcement, then focus in the same update) is Teksilo's
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `b518253f` (announce-focus). Fixed part: drag-and-drop Songs keyboard reorder 'Moved to N of 10' cut.
- **Where:** crates/teksilo-widgets/src/list\_view/widget\_impl.rs:235-262 (commit, select, ctx.announce in one handler; the rebuild then refocuses a new row node)
- **Evidence:**
  - `misc-drag-and-drop-20260925-150304 report: '+39.1 ms object:announcement [status bar] 'Moved to 3 of 10'' / '+49.9 ms object:state-changed:focused 1 [list item] 'Unravel'' / 'ORCA SAYS (CUT): 'Moved to 3 of 10'' / 'ORCA SAYS: 'Unravel.''`
  - `orca-debug.out: '15:03:24.694088 - SPEECH OUTPUT: 'Moved to 3 of 10'' / '15:03:24.728618 - OBJECT EVENT: object:state-changed:focused for [list item: 'Unravel']' / '15:03:24.781776 - NULL SPEECH: stop'`
  - `misc-drag-and-drop-20260925-145506: '+42.5 ms object:children-changed:add [panel] '' -> [list item] 'Hyperballad'' … all ten rows re-added, then '+49.3 ms object:state-changed:focused 1 [list item] 'Unravel''`
  - `source: list_view/widget_impl.rs:235-262 (commit, select, then ctx.announce in the same handler; the rebuild then refocuses a fresh row)`
  - `misc-drag-and-drop-20260925-152251 orca-debug.out: '15:23:12.201677 - SPEECH OUTPUT: 'Moved to 3 of 10'' / '15:23:12.235856 - OBJECT EVENT: object:state-changed:focused for [list item: 'Unravel']' / '15:23:12.288273 - NULL SPEECH: stop' / '15:23:12.288363 - SPEECH OUTPUT: 'Unravel.''`
  - `misc-drag-and-drop-20260925-151249 'Alt+Down moves Unravel': '+45.8 ms object:announcement [<Error>] '' text='Moved to 3 of 10'' / '+46.4 ms object:state-changed:focused 1 [list item] 'Unravel''; 'Alt+Down again': 'ORCA SAYS (CUT): 'Moved to 4 of 10'' (15:13:14.649339)`
  - `misc-drag-and-drop-20260925-152826: first move from [<Error>] ('15:28:46.334374 EVENT MANAGER: Ignoring defunct object: [DEAD]'), third move 'ORCA SAYS (CUT): 'Moved to 3 of 10'' (15:28:55.309084)`
- **Reproduced:** 12 of 12 moves across 3 runs unheard; the first move of each run: 3 of 3 lost (2 cut by the focus change, 1 source gone)
- **Verification:** confirmed. Reproduced: 12 of 12 moves unheard over 3 runs (4 moves each). First move of a run: 2 of 3 lost with its source ('\[&lt;Error&gt;\]', Orca 'Ignoring defunct object: \[DEAD\]'), 1 of 3 spoken then cut by the focus stop. Whenever Orca started speaking a move (3 instances in 3 runs), it was cut 3 of 3.
- **Fix idea:** Keep row identity across a reorder (reuse the row widget so focus does not move), or queue the announcement for the update after the refocus. This message goes through ctx.announce, so the K2 fix covers the defunct drop but not this ordering.

### misc-10 {#misc-10}

recent-projects: after Pin or Remove on any row, focus jumps to the first row's 'Open' and nothing says what happened

- **Example:** recent-projects
- **Scenario:** misc-recent-projects
- **Act:** Space on Teksilo's Pin (3rd row); Space on the 3rd row's Remove
- **The reader should get:** focus stays in the acted-on row (its new 'Unpin') or moves to a neighbour, and the reader hears 'pinned' / 'removed'
- **The reader gets:** 'Open push button.' each time, on playground's (row 1) Open; the next Tab lands on playground's 'Pin'
- **Platform:** Linux measured; focus placement is teksilo-core's, so all platforms
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-core/src/widget\_tree/layout\_impl.rs:863-869 (pending\_focus\_restore → first\_focusable\_descendant of the rebuilt subtree)
- **Evidence:**
  - `misc-recent-projects-20260925-150152 'Space on Teksilo's Pin': '+34.1 ms object:state-changed:focused 1 [push button] 'Open'' / 'ORCA SAYS: 'Open push button.''; tree after the act: row ['playground'] ['Open*','Pin','Remove'], row ['★ Teksilo'] ['Open','Unpin','Remove']`
  - `'Tab after pinning': '+8.5 ms object:state-changed:focused 1 [push button] 'Pin'' / 'ORCA SAYS: 'Pin push button.''`
  - `'Space on the third row's Remove': '+31.4 ms object:state-changed:focused 1 [push button] 'Open'' (playground's) / 'ORCA SAYS: 'Open push button.''`
  - `source: layout_impl.rs:862-868 restores focus to first_focusable_descendant(pending_focus_restore root), i.e. the top of the Repeater`
  - `misc-recent-projects-20260925-151553 'Space on Teksilo's Pin': '+36.8 ms object:state-changed:focused 1 [push button] 'Open'' / 'ORCA SAYS: 'Open push button.''; tree: [label] 'playground' … [push button] 'Open' {focusable,focused}; [label] '★ Teksilo' … [push button] 'Unpin'`
  - `runs -152453 and -152921: same focus target after Pin and after Remove`
- **Reproduced:** 3 of 3 runs (row 1 in r1, row 3 in r2 and r4)
- **Verification:** confirmed. Reproduced: 3 of 3 runs, for both Pin (3rd row) and Remove (3rd row): focus went to playground's (row 1) Open every time
- **Fix idea:** Restore focus to the rebuilt row's counterpart of the destroyed widget (same position within the row), or to the nearest surviving sibling, not the subtree's first focusable. The example should also announce pin/remove.

### misc-11 {#misc-11}

recent-projects: pressing 'Hide paths' drops focus to the window ('frame.'); the next Tab restarts at the toolbar

- **Example:** recent-projects
- **Scenario:** misc-recent-projects
- **Act:** Space on 'Hide paths'
- **The reader should get:** focus on the 'Show paths' button that replaces it
- **The reader gets:** object:state-changed:focused 1 \[frame\] '' and 'frame.'; Tab then goes to 'Theme combo box'
- **Platform:** Linux measured
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/recent\_projects/src/main.rs:256-268 (two buttons swapped with visible\_when)
- **Evidence:**
  - `misc-recent-projects-20260925-150152: '+35.4 ms object:children-changed:add [frame] '' -> [push button] 'Show paths'' / '+37.6 ms object:children-changed:remove [frame] '' -> [push button] 'Hide paths'' / '+38.4 ms object:state-changed:focused 1 [frame] ''' / 'ORCA SAYS: 'frame.''`
  - `'Tab after Hide paths': '+7.5 ms object:state-changed:focused 1 [combo box] 'Theme''`
  - `source: examples/recent_projects/src/main.rs:265-277 swaps two buttons with visible_when; teksilo-core clears focus when the focused node goes dormant (layout_impl.rs:862 only restores after a rebuild)`
  - `verify-misc-recent-show-20260925-152658 'Space on Hide paths': '+36.6 ms object:state-changed:focused 1 [frame] ''' / 'ORCA SAYS: 'frame.''; 'Space on Show paths': '+28.1 ms object:state-changed:focused 1 [frame] ''' / 'ORCA SAYS: 'frame.''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 misc-recent-projects runs + both directions in verify-misc-recent-show (Hide→frame, Show→frame)
- **Fix idea:** Use one button whose label follows show\_paths (or a toggle button with pressed state); the framework could also hand focus to the node that replaces a dormant focused one.

### misc-12 {#misc-12}

The ColorPicker preset grid: arrows move an invisible index (silent), Enter applies a swatch the reader never heard, and every swatch is also a Tab stop

- **Example:** color-picker-demo
- **Scenario:** misc-color-swatches
- **Act:** focus 'Color presets', Right arrow; Tab from the grid
- **The reader should get:** per the module doc (swatch\_grid.rs:4-12): one Tab stop, arrows move focus between swatches and speak them, Tab leaves
- **The reader gets:** Right arrow: no event, Orca silent. Tab from the grid goes to 'Swatch #E84D3D' and then through all 12 swatches (13 Tab stops per grid, 2 grids in view)
- **Platform:** all (no focus/active descendant is published); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker/swatch\_grid.rs:120-189 (arrows set focused\_index only); color\_picker/swatch.rs (each cell focusable)
- **Evidence:**
  - `misc-color-swatches 'Right arrow inside the grid': no events; 'FAIL Orca says 'Swatch''`
  - `'Tab once from the grid': '+26.9 ms object:state-changed:focused 1 [push button] 'Swatch #E84D3D'' / 'FAIL Tab leaves the grid (one tab stop for the whole grid)'`
  - `tabwalk-color-picker-demo: Tab 9 'Color presets.', Tabs 10-21 'Swatch #E84D3D push button.' … 'Swatch #F5F5F5 push button.'`
  - `source: swatch_grid.rs:136-189 arrows only set focused_index (bound to nothing); swatch.rs:204 every ColorSwatch is .focusable(true); the struct comment (l.36-38) admits Tab still reaches each cell`
  - `misc-color-swatches-20260925-152542 'Tab once from the grid': last focus [push button] 'Swatch #E84D3D'; 'Right arrow inside the grid': no events`
- **Reproduced:** 2 of 2 swatches runs + tabwalk
- **Verification:** confirmed. Reproduced: 2 of 2 swatch runs (Right arrow: no event, Orca silent; Tab from grid lands on 'Swatch #E84D3D')
- **Fix idea:** Roving tabindex: only the current swatch is a Tab stop and arrows move real focus to the next swatch (or publish active\_descendant on the grid).

### misc-13 {#misc-13}

A picked swatch never becomes 'selected' for AT: its name and state are computed once at build

- **Example:** color-picker-demo
- **Scenario:** misc-color-swatches
- **Act:** Space on 'Swatch #3685E3' (the hex field then shows #3685E3)
- **The reader should get:** 'Swatch #3685E3, selected' (color-picker-swatch-selected-suffix) and the selected state
- **The reader gets:** no name change, no state change, silence; the tree after the act still has 'Swatch #3685E3' without selected
- **Platform:** all (AccessKit tree is stale); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker/swatch\_grid.rs:88-116 (is\_selected computed at build; `selected` bound RepaintOnly on the grid)
- **Evidence:**
  - `misc-color-swatches 'Space on Swatch #3685E3': only accessible-value events on spin buttons and the Hue slider; 'FAIL Orca says 'selected' / Orca unheard: 'selected''`
  - `tree after the act: name 'Swatch #3685E3' states ['enabled','focusable','focused','sensitive','showing','visible']; Hex entry text '#3685E3'`
  - ``source: swatch_grid.rs:88-116 is_selected = selected.get() == color at build, and `selected` is bound RepaintOnly (l.91-95), so the ColorSwatch's selected flag and accessibility never update``
  - `misc-color-swatches-20260925-152542 tree after 'Space on Swatch #3685E3': '[push button] 'Swatch #3685E3' {focusable,focused}' (no selected, no ', selected' suffix) while '[entry] 'Hex' … text='#3685E3''`
- **Reproduced:** 2 of 2 runs
- **Verification:** confirmed. Reproduced: 2 of 2 runs
- **Fix idea:** Give each ColorSwatch a derived Signal&lt;bool&gt; (selected == color) bound at AccessibilityOnly (and repaint), instead of a build-time bool.

### misc-14 {#misc-14}

HexColorInput is two nested entries with the same name, so every focus speaks the field twice

- **Example:** color-picker-demo
- **Scenario:** misc-color-hex
- **Act:** focus any Hex field (launch focus, Tab, Shift+Tab)
- **The reader should get:** 'Hex entry #FF8800 selected.' once
- **The reader gets:** 'Hex entry #FF8800' (the outer, non-focusable wrapper as context) then 'Hex entry #FF8800 selected.'
- **Platform:** Linux/Orca measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/hex\_color\_input.rs:599-630
- **Evidence:**
  - `tree: [entry] 'Hex' {editable,selectable-text,single-line} text='#3584E4' > [entry] 'Hex' {editable,focusable,focused,…} text='#3584E4' + [status bar] ''`
  - `misc-color-hex-20260925-150036 'Tab away and back': 'ORCA SAYS: 'Hex entry #FF8800'' / 'ORCA SAYS: 'Hex entry #FF8800 selected.''; orca-debug.out '15:00:56.332578 - SPEECH OUTPUT: 'Hex entry #FF8800'' / '15:00:56.332616 - SPEECH OUTPUT: 'Hex entry #FF8800 selected.''`
  - `every launch: '15:01:13.777528 - SPEECH OUTPUT: 'Hex entry #3584E4'' / '15:01:13.777541 - SPEECH OUTPUT: 'Hex entry #3584E4 selected.''`
  - `source: hex_color_input.rs:599-612 sets Role::TextInput + the label on the wrapper around a TextInput whose inner field is already Role::TextInput with that label`
  - `misc-color-hex-20260925-151454 'Tab away and back': 'ORCA SAYS: 'Hex entry #FF8800'' / 'ORCA SAYS: 'Hex entry #FF8800 selected.''`
  - `tree: '[entry] 'Hex' {editable,selectable-text,single-line} text='#3685E3'' > '[entry] 'Hex' {editable,focusable,…}'`
- **Reproduced:** every Hex focus in 9 runs
- **Verification:** confirmed. Reproduced: every Hex focus in all color runs (10 launches, 2 hex refocus acts)
- **Fix idea:** Leave the wrapper a GenericContainer (as TextInput's own composite is) and put label/placeholder on the inner field only.

### misc-15 {#misc-15}

The ColorPicker's preview and saturation × brightness field carry their value only as a string value, which AT-SPI does not export

- **Example:** color-picker-demo
- **Scenario:** misc-color-channels
- **Act:** Shift+Tab to 'Selected color'; Shift+Tab to 'Saturation and brightness'
- **The reader should get:** 'Selected color #3684E4' (color-picker-current-color-readout exists, unused) and 'Saturation 77%, brightness 89%'
- **The reader gets:** 'Selected color push button.' and 'Saturation and brightness panel.'; the pair is only heard through the arrow keys' ctx.announce (K2-affected)
- **Platform:** Linux by measurement and source; Windows exposes a string value via ValuePattern (accesskit\_windows node.rs:592-597)
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker/swatch.rs:363-381; color\_picker/hsv\_canvas.rs:385-401
- **Evidence:**
  - `misc-color-channels: '+504.1 ms object:state-changed:focused 1 [push button] 'Selected color'' / 'ORCA SAYS: 'Selected color push button.'' / 'FAIL Orca says one of ['#3684E4', '3684E4']'`
  - `'Shift+Tab to Saturation and brightness': 'ORCA SAYS: 'Saturation and brightness panel.'' / 'FAIL Orca says one of ['Saturation 77', '77%']'`
  - `source: swatch.rs:363-381 name 'Selected color', hex in set_value; hsv_canvas.rs:385-401 pair in set_value; atspi node.rs:38-44`
  - `misc-color-channels-20260925-152607 'Shift+Tab to Saturation and brightness': 'ORCA SAYS: 'Saturation and brightness panel.''; 'Shift+Tab three times…': 'ORCA SAYS: 'Selected color push button.''`
- **Reproduced:** 2 of 2 channel runs
- **Verification:** confirmed. Reproduced: 2 of 2 channel runs
- **Fix idea:** Name the preview with color-picker-current-color-readout; put the saturation/brightness pair in the canvas's description (read on focus) as well as the value.

### misc-16 {#misc-16}

ColorEdit triggers are named only by their hex, and the nullable one by an em dash ('— push button.'); its popover dialog is unnamed

- **Example:** color-picker-demo
- **Scenario:** misc-color-scroll
- **Act:** Shift+Tab from Theme to the last ColorEdit; Shift+Tab to the first; Space
- **The reader should get:** 'Color #E91E63, opens a picker' / 'Color, none' (color-edit-trigger-name and -name-empty exist in en-US.ftl:334-335, unused); a named dialog
- **The reader gets:** '— push button.', '#E91E63 push button.', then 'dialog', 'Color picker panel.'
- **Platform:** all (names in the AccessKit tree); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_edit.rs:432-467
- **Evidence:**
  - `misc-color-scroll (3 runs): '+156.6 ms object:state-changed:focused 1 [push button] ', '' / 'ORCA SAYS: ',  push button.''`
  - `'Shift+Tab twice to the first ColorEdit': '+258.2 ms object:state-changed:focused 1 [push button] '#E91E63'' / 'ORCA SAYS: '#E91E63 push button.''`
  - `'Space opens its picker popover': tree [dialog] '' {active}; 'ORCA SAYS: 'dialog''`
  - `source: color_edit.rs:430-470 label_signal is the bare hex or color-edit-trigger-empty-placeholder ('—')`
  - `misc-color-scroll-20260925-152736 'Shift+Tab wraps': 'ORCA SAYS: ',  push button.''; 'Shift+Tab twice': '[push button] '#2196F280'' then '#E91E63 push button.'; 'Space opens its picker popover': 'ORCA SAYS: 'dialog''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 color-scroll runs
- **Fix idea:** Use color-edit-trigger-name / -name-empty as the button's accessible name (access\_label) while keeping the hex as the visible text; name the popover dialog (e.g. 'Color picker').

### misc-17 {#misc-17}

A plain window opens with focus in its first TextInput, wherever it is: color-picker-demo starts inside the first picker's Hex field, text selected

- **Example:** color-picker-demo
- **Scenario:** misc-color-channels (launch)
- **Act:** launch color-picker-demo
- **The reader should get:** focus on the window (as the other examples) or an explicitly chosen control
- **The reader gets:** 'Color picker panel.', 'Hex entry #3584E4', 'Hex entry #3584E4 selected.'; the toolbar, headings, canvas, Hue slider and preview come before it; a stray keystroke replaces the selected hex
- **Platform:** all (teksilo-app focus policy); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-app/src/window\_manager.rs:121-135 with crates/teksilo-core/src/widget\_tree/focus\_impl.rs:404-422 and crates/teksilo-widgets/src/text\_input/widget\_impl.rs:457-459
- **Evidence:**
  - `misc-color-channels-20260925-150110 launch: '+1469.6 ms window:activate [frame] ''' / '+1469.8 ms object:state-changed:focused 1 [entry] 'Hex''`
  - `7 of 7 color-picker launches`
  - `source: text_input/widget_impl.rs:457-459 returns the inner field as initial_focus_hint unconditionally (documented for modals, text_input.rs:446-453); window_manager.rs:121-135 honours widget_initial_focus_hint for non-modal windows too`
  - `misc-color-scroll-20260925-151249 launch: '+1592.3 ms window:activate [frame] ''' / '+1592.6 ms object:state-changed:focused 1 [entry] 'Hex''`
- **Reproduced:** 7 of 7 launches
- **Verification:** confirmed. Reproduced: 11 of 11 color-picker launches
- **Fix idea:** For non-modal windows, only honour an explicit app opt-in hint, not TextInput's modal-oriented default.

### misc-18 {#misc-18}

drag-and-drop: the Songs, Playlist and Up next lists and the Folders tree are unnamed ('multi-select list box.', 'tree.')

- **Example:** drag-and-drop
- **Scenario:** misc-drag-and-drop
- **Act:** Tab through the example
- **The reader should get:** 'Songs multi-select list box', 'Playlist …', 'Folders tree' (the visible headings)
- **The reader gets:** 'multi-select list box.', 'multi-select list box.', 'list box.', 'tree.'
- **Platform:** all; measured on Linux
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/drag\_and\_drop/src/main.rs:170, 229, 334, 397 (ListView/TreeView built with no access\_label)
- **Evidence:**
  - `tabwalk-drag-and-drop: '+9.9 ms object:state-changed:focused 1 [list box] ''' / 'ORCA SAYS: 'multi-select list box.''; '+18.9 ms object:state-changed:focused 1 [tree] ''' / 'ORCA SAYS: 'tree.''`
  - `misc-drag-and-drop x3: 'FAIL Orca says 'Songs'', 'FAIL Orca says 'Playlist'', 'FAIL Orca says one of ['Up next', 'queue']', 'FAIL Orca says 'Folders''`
  - `source: examples/drag_and_drop/src/main.rs:165-190, 225-240, 330-345 build ListView/TreeView with no access_label; ListView/TreeView have no label builder (extract_widget_api shows none)`
  - `misc-drag-and-drop-20260925-152826: 'ORCA SAYS: 'multi-select list box.'' (Songs, Playlist), 'list box.' (Up next), 'tree.' (Folders)`
- **Reproduced:** 4 of 4 runs
- **Verification:** confirmed. Reproduced: 3 of 3 drag-and-drop runs
- **Fix idea:** access\_label / labelled\_by the visible titles; a .label() builder on ListView/TreeView (like ComboBox/SpinBox) would make this the obvious path.

### misc-19 {#misc-19}

TreeView hierarchy is invisible on AT-SPI: no expandable/expanded state and no level ('Documents.' before and after Right arrow)

- **Example:** drag-and-drop
- **Scenario:** misc-drag-and-drop
- **Act:** Down to Documents, Right arrow to expand
- **The reader should get:** 'Documents collapsed' then 'expanded', children at level 2
- **The reader gets:** 'Documents.' both times; children appear as flat siblings with posinset restarting
- **Platform:** Linux and macOS by source (accesskit\_atspi\_common node.rs state()/attributes() map neither; accesskit\_macos has no is\_expanded/level); Windows exports both (accesskit\_windows node.rs:500, 719)
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/list\_item\_a11y.rs:270-300 (correct); gap in accesskit\_atspi\_common-0.20.0/src/node.rs:301-436
- **Evidence:**
  - `misc-drag-and-drop 'Down arrow in Folders': 'ORCA SAYS: 'Documents.'' / 'FAIL Orca says one of ['collapsed', 'expandable']'; 'Right arrow expands Documents': 'ORCA SAYS: 'Documents.'' / 'FAIL Orca says 'expanded''`
  - `tree after expanding: Documents states ['enabled','focused','selectable','selected','sensitive','showing','visible'] attrs {'posinset': '1'}; notes.md {'posinset': '1'} as a sibling`
  - `source: list_item_a11y.rs:270-300 TreeItemWrapper sets level and expanded; accesskit_atspi_common node.rs:301-385 (no Expandable/Expanded/Collapsed), 415+ (no level attribute)`
  - `misc-drag-and-drop-20260925-151249 tree after 'Right arrow expands Documents': '[tree item] 'Documents' {focused,selectable,selected} attrs={'posinset': '1'}' / '[tree item] 'notes.md' {selectable} attrs={'posinset': '1'}'`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** Upstream: map expanded/collapsed/expandable states and the level attribute. Teksilo meanwhile could announce expand/collapse through ctx.announce.

### misc-20 {#misc-20}

Status lines change silently: a file dialog's result, the font size, the seeding of recents

- **Example:** file-dialogs
- **Scenario:** misc-file-dialogs, misc-recent-projects
- **Act:** file-dialogs: Escape cancels / pick a file; recent-projects: Bigger font, Seed demo entries
- **The reader should get:** 'Open cancelled.' / 'Opened: …' / 'Font size: 17 pt' spoken, as a sighted user reads them
- **The reader gets:** accessible-name changes on plain labels; Orca says only the refocused button ('Open file… push button.') or nothing
- **Platform:** all (no live region); measured on Linux
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/file\_dialogs/src/main.rs:183-185; examples/recent\_projects/src/main.rs:195-215
- **Evidence:**
  - `misc-file-dialogs-20260925-145946 'Escape cancels the dialog': '+20.7 ms object:property-change:accessible-name [label] 'Open cancelled.'' / 'ORCA SAYS (CUT): 'frame.'' / 'ORCA SAYS: 'Open file… push button.'' / 'FAIL Orca says 'Open cancelled''`
  - `same run, after picking a file: 'pass a object:property-change:accessible-name event from [label] 'Opened:'' / 'FAIL Orca says 'Opened''`
  - `misc-recent-projects 'Space on Bigger font': '+34.5 ms object:property-change:accessible-name [label] 'Font size: 17 pt'' / 'FAIL Orca says '17''; 'Space on Seed demo entries': 'FAIL Orca says something / Orca said nothing in the act'`
  - `source: examples/file_dialogs/src/main.rs:183-185 and examples/recent_projects/src/main.rs:201-213 plain TextWidgets`
  - `misc-file-dialogs-20260925-151733 'Escape cancels the dialog': '+24.8 ms object:property-change:accessible-name [label] 'Open cancelled.'' / '+28.6 ms window:activate [frame]' / '+28.9 ms object:state-changed:focused 1 [push button] 'Open file…'' / 'ORCA SAYS: 'Open file… push button.''`
  - `misc-recent-projects (3 runs) 'Space on Bigger font': '+33.4 ms object:property-change:accessible-name [label] 'Font size: 17 pt'' / Orca silent`
- **Reproduced:** deterministic; 1 file-dialogs run with 2 acts, 3 of 3 recent-projects runs
- **Verification:** confirmed. Reproduced: file-dialogs 2 of 2 runs (cancel and open), recent-projects 3 of 3 (font size, seed)
- **Fix idea:** access\_live(Live::Polite) on the status TextWidget (or ctx.announce the result in the dialog callback).

### misc-21 {#misc-21}

recent-projects row buttons are all 'Open', 'Pin', 'Remove': no project in the name, pin state only as a '★' in the title

- **Example:** recent-projects
- **Scenario:** misc-recent-projects
- **Act:** Tab into the rows
- **The reader should get:** 'Open Skribisto', 'Unpin Skribisto' (or a toggle button with pressed state)
- **The reader gets:** 'Open push button.', 'Pin push button.'
- **Platform:** all; measured on Linux
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/recent\_projects/src/main.rs:330-345
- **Evidence:**
  - `misc-recent-projects 'Tab into the first row': '+195.5 ms object:state-changed:focused 1 [push button] 'Open'' / 'ORCA SAYS: 'Open push button.'' / 'FAIL the focused row button says which project it acts on'`
  - `'Tab through that row': 'ORCA SAYS: 'Pin push button.''`
  - `source: examples/recent_projects/src/main.rs:320-340`
  - `misc-recent-projects-20260925-152921 'Tab into the first row': 'ORCA SAYS: 'Open push button.''; 'Tab through that row': 'ORCA SAYS: 'Pin push button.''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3 runs
- **Fix idea:** access\_label("Open {name}") etc., or name the row panel (labelled\_by the title) so Orca speaks it on entry.

### misc-22 {#misc-22}

The searchable ComboBox popup: unnamed search entry (placeholder only), an \[unknown\] popup root, and every option doubled (unnamed list item wrapping the named one)

- **Example:** font-picker
- **Scenario:** misc-font-picker
- **Act:** Alt+Down on Font family
- **The reader should get:** a named search field and one list item per font
- **The reader gets:** 'list box', 'Search…'; tree \[combo box\] 'Font family' &gt; \[unknown\] '' &gt; \[list box\] '' &gt; \[entry\] '' (placeholder-text 'Search…') + \[status bar\] '' + \[list box\] '' &gt; \[list item\] '' &gt; \[list item\] 'C059' …
- **Platform:** Linux measured
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `f9ffa98c` (combobox). Fixed part: the searchable popup's field is named ("Font family editable combo box") and its options are no longer doubled.
- **Where:** crates/teksilo-widgets/src/combo\_box/item.rs:207-218 (ListBoxOption inside ListView's ListItem wrapper); combo\_box/panel.rs:563-567 (search TextInput with only a lit!("Search…") placeholder)
- **Evidence:**
  - `misc-font-picker tree-open-the-font-list-with-Alt-Down.txt: '[list item] '' {selectable} attrs={'setsize': '277', 'posinset': '1'}' > '[list item] 'C059' {selectable} attrs={'posinset': '1', 'setsize': '277'}' (30 of 60 list items unnamed)`
  - `'+192.8 ms object:state-changed:focused 1 [entry] ''' / 'ORCA SAYS: 'list box'' / 'ORCA SAYS: 'Search…''`
  - `source: combo_box/panel.rs:563-566 TextInput placeholder only; combo_box/item.rs:209 ListBoxOption inside the virtualized list's own item wrapper`
  - `verify-misc-combo-ws orca-debug.out '15:23:57.822014 - AXSelection: [list box] reports 0 selected children'`
  - `misc-font-picker-20260925-151338 tree-open-the-font-list-with-Alt-Down.txt: '[combo box] 'Font family' > [unknown] '' > [list box] '' > [entry] '' (placeholder-text 'Search…') + [status bar] '' + [list box] '' > [panel] '' > [list item] '' > [list item] 'C059''`
- **Reproduced:** 2 of 2 runs
- **Verification:** corrected by the verifier. Reproduced: deterministic, 2 of 2 font-picker runs + verify-misc-combo-ws (the writing-system list has the same shape) The structure is as described. Severity is raised from medium to high: the doubled item is not cosmetic. Because ListView's outer \[list item\] '' is item-like and unselected, AT-SPI's Selection interface reports 0 selected children, so Orca's selection-changed handler has nothing to speak. This is the Linux-side cause of misc-05 (evidence there). The placeholder is also a hard-coded English literal (panel.rs:565) and is the search field's only accessible text.
- **Fix idea:** Label the search field (e.g. 'Search fonts'); make the outer row wrapper a GenericContainer when the delegate already emits ListBoxOption.

### misc-23 {#misc-23}

Section titles are plain labels, never headings (text-and-layout and every other example); TextWidget offers no heading API

- **Example:** text-and-layout
- **Scenario:** misc-text-layout
- **Act:** read the tree of text-and-layout
- **The reader should get:** 'Typography Styles', 'Layout Primitives', 'Text & Layout' as headings a reader can jump between
- **The reader gets:** \[label\] 'Typography Styles', \[label\] 'Layout Primitives'
- **Platform:** all
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/primitives/text\_widget.rs:954-958 (always Role::Label); no Role::Heading emitter in teksilo-widgets outside rich\_text/code\_editor
- **Evidence:**
  - `misc-text-layout: 'FAIL the tree holds [heading] 'Typography Styles' / no such node in the tree after the act' (same for 'Layout Primitives', 'Text & Layout')`
  - `source: text_widget.rs:954-999 always Role::Label; no Role::Heading emitter in teksilo-widgets outside rich_text/code_editor`
  - `misc-text-layout-20260925-152854: 'FAIL the tree holds [heading] 'Typography Styles'' etc.`
- **Reproduced:** deterministic
- **Verification:** corrected by the verifier. Reproduced: deterministic, 2 of 2 text-layout runs The observation is right, but the primary layer is the framework: TextWidget has no heading API, so the example had nothing to call. An .access\_role(Role::Heading) override would hang the text runs off a Heading, which the rich-text walker's own rule forbids. Low severity is appropriate.
- **Fix idea:** A TextWidget::heading(level) that keeps the runs under a Label child (runs may not hang off a Heading, per the rich-text walker's rule).

### misc-24 {#misc-24}

Text changes of nodes outside the exported tree (off-screen pickers sharing the colour) still emit text-changed events, 22 per keypress, which Orca drops as defunct

- **Example:** color-picker-demo
- **Scenario:** misc-color-channels
- **Act:** Up arrow on the R spinner
- **The reader should get:** events only for nodes a reader can reach
- **The reader gets:** object:text-changed from 17 paths absent from the tree (\[&lt;Error&gt;\] sources) and 'EVENT MANAGER: Ignoring defunct object: \[DEAD\]' x22
- **Platform:** Linux
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0/src/adapter.rs:184-195 (emit\_text\_change\_if\_needed) called from node\_updated 287-288 before the filter comparison
- **Evidence:**
  - `misc-color-channels-20260925-145159 'Up arrow on it': '+35.3 ms object:text-changed:delete [<Error>] '' text='3'' … and 22 x 'observed Orca ignored an event whose source was defunct' ('14:52:11.944751 EVENT MANAGER: Ignoring defunct object: [DEAD]')`
  - `17 source paths of these events are not in the launch tree (run.json check)`
  - `source: accesskit_atspi_common adapter.rs:287-289 node_updated calls emit_text_change_if_needed before comparing filter results`
  - `misc-color-channels-20260925-151406 'Up arrow on it': '+36.6 ms object:text-changed:delete [<Error>] '' text='5'' …; 22 x 'EVENT MANAGER: Ignoring defunct object: [DEAD]' (15:14:18.646376 …)`
- **Reproduced:** every spinner/hue step in 2 of 2 channel runs
- **Verification:** confirmed. Reproduced: every spinner/hue step in 2 of 2 channel runs
- **Fix idea:** Upstream: skip text-change emission for nodes whose new filter result is not Include. Harmless now, but it feeds Orca's queue (&gt;100 queued events make Orca drop name changes).

### misc-25 {#misc-25}

The non-drag alternatives' AccessKit custom actions (Move Up/Down/…, the colour field's four steps) are not exported on AT-SPI

- **Example:** drag-and-drop
- **Scenario:** misc-drag-and-drop
- **Act:** read the actions of a ListView row / TreeView row
- **The reader should get:** obligation 3 of docs/a11y/non-drag-alternatives.md: the moves as actions
- **The reader gets:** actions \['click'\] only
- **Platform:** Linux by source and measurement
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0/src/node.rs:532-534 (n\_actions: 1 if clickable else 0)
- **Evidence:**
  - `misc-drag-and-drop tree after 'Right arrow expands Documents': every tree item actions ['click']`
  - `source: accesskit_atspi_common node.rs:533-535 n_actions() returns 1 if clickable else 0`
  - `misc-drag-and-drop tree: every [list item]/[tree item] actions=[{'name': 'click'}]`
- **Reproduced:** deterministic
- **Verification:** confirmed. Reproduced: deterministic (tree actions \['click'\] on every list and tree item in 3 of 3 runs)
- **Fix idea:** Upstream AT-SPI Action support for custom actions; the keyboard chord remains the Linux route.

### misc-M1 {#misc-m1}

Every StandardListItem / StandardTreeItem row exposes a second, role-less node with the row's name (\[unknown\] 'Hyperballad' under \[list item\] 'Hyperballad')

- **Example:** drag-and-drop
- **Scenario:** misc-drag-and-drop (launch)
- **Act:** launch drag-and-drop; arrow through Songs and Folders
- **The reader should get:** one node per row: the list item / tree item, named
- **The reader gets:** each row has a child \[unknown\] node with the same name, which the launch audit flags 13 times. Every arrow press removes and re-adds it (children-changed add/remove, then defunct), which adds bus noise. In object navigation a reader meets a duplicate stop with no role.
- **Platform:** Linux measured (AtspiRole::Unknown); the node exists in the AccessKit tree, so every platform gets it
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/standard\_item.rs:916-935, 1383-1385
- **Evidence:**
  - `misc-drag-and-drop-20260925-151249 launch tree audit: 'unknown-role: [unknown] 'Hyperballad': a node whose role the adapter could not map' … 13 entries ('Documents', 'Downloads', 'README.txt' for the tree too); 13 in each of 3 runs`
  - `tree: '[list item] 'Hyperballad' {selectable} attrs={'setsize': '10', 'posinset': '1'}' > '[unknown] 'Hyperballad' attrs={'setsize': '10'}'`
  - `'Down arrow in Songs': '+10.1 ms object:children-changed:add [list item] 'Hyperballad' -> [unknown] 'Hyperballad'' / '+11.1 ms object:children-changed:remove …' / '+11.5 ms object:state-changed:defunct 1 [unknown] 'Hyperballad''`
  - `source: crates/teksilo-widgets/src/standard_item.rs:916-935 sets a name and no role (comment: the parent sets the role); crates/teksilo-core/src/accessibility.rs:473-478 AccessNodeBuilder starts at Role::Unknown; accesskit_atspi_common node.rs:291 Role::Unknown → AtspiRole::Unknown`
- **Reproduced:** 3 of 3 drag-and-drop runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** StandardListItem/StandardTreeItem should emit no node of their own (GenericContainer, pruned by the walker), leaving the name to the ListItemWrapper/TreeItemWrapper row, or set the name there.

### misc-M2 {#misc-m2}

Framework accessibility strings hard-coded in English: the ColorPicker's saturation × brightness field (name, value, announcement, four custom actions) and the searchable ComboBox's 'Search…' field

- **Example:** color-picker-demo
- **Scenario:** verify-misc-color-fr
- **Act:** focus the saturation × brightness field, arrow it; open a searchable ComboBox (font-picker)
- **The reader should get:** localized strings, like the rest of the picker (color-picker-\* keys exist in 23 locales)
- **The reader gets:** 'Saturation and brightness', 'Saturation {n}%, brightness {n}%', 'Increase saturation' … and 'Search…' in English whatever the locale
- **Platform:** all (strings in the AccessKit tree); by source, not measured in another language
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/color\_picker/hsv\_canvas.rs:97-100, 238-242, 395-400; crates/teksilo-widgets/src/combo\_box/panel.rs:565
- **Evidence:**
  - `crates/teksilo-widgets/src/color_picker/hsv_canvas.rs:97-100 lit!("Increase saturation") etc.; :238-242 ctx.announce(format!("Saturation {}%, brightness {}%", …)); :395-400 set_name(lit!("Saturation and brightness")) and set_value(format!(…))`
  - `crates/teksilo-widgets/src/combo_box/panel.rs:565 .placeholder(lit!("Search…")): that placeholder is the search entry's only accessible text (misc-22)`
  - `no key for either in crates/teksilo-widgets/locales/en-US.ftl (color-picker-saturation-label/-value-label exist but are for other uses)`
  - `measurement not possible here: verify-misc-color-fr (LANG=fr_FR.UTF-8) left every name English, because the example supports only en-US (I18nConfig's supported_locales defaults to [en-US], teksilo-i18n config.rs:66; manager.rs:162-176 only picks an OS locale that is supported)`
- **Reproduced:** source only (the example cannot be switched to another locale)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Add Fluent keys (color-picker-sv-name, -sv-value, -sv-announcement, the four step names, combo-box-search-placeholder/label) and resolve them with resolve\_message\_widget.
