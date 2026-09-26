<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Widget catalog, last six tabs

Examples: widget-catalog (overlays, data, dragdrop, animations, touch, settings).
37 findings: 1 critical, 14 high, 17 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [catalog-c-01](#catalog-c-01) | widget-catalog | The Settings tab crashes the app in every debug build: a PrivacySettings Toggle trips its own label assertion | critical | all | fixed |
| [catalog-c-02](#catalog-c-02) | widget-catalog | Tabbing away from a control whose focus-opened tooltip is showing sends focus back to that control about 120 ms later | high | all | fixed |
| [catalog-c-03](#catalog-c-03) | widget-catalog | A rich tooltip opened by keyboard focus is never spoken while it shows; its text reaches the reader only when it is dismissed | medium | Linux | open |
| [catalog-c-04](#catalog-c-04) | widget-catalog | Rich tooltip text reaches the reader with its raw link markup ('\[next link\](:tip-b)') | medium | all | open |
| [catalog-c-05](#catalog-c-05) | widget-catalog | Composite tooltips are named 'Tooltip', so every composite trigger's description is the word 'Tooltip' and the content is never spoken on focus | medium | all | open |
| [catalog-c-06](#catalog-c-06) | widget-catalog | Opening the 'Tabbed details' composite tooltip moves Orca's locus of focus to a page tab inside it ('Stats page tab.') | medium | Linux | upstream |
| [catalog-c-07](#catalog-c-07) | widget-catalog | A popover with no focusable content takes focus on an unnamed, role-less wrapper; the reader hears nothing when it opens | high | all | fixed |
| [catalog-c-08](#catalog-c-08) | widget-catalog | Tab inside an open popover with no focusable content closes it and throws focus to the top of the window | high | all | fixed |
| [catalog-c-09](#catalog-c-09) | widget-catalog | The snackbar announces the generic word 'Snackbar', never its message | high | all | open (example) |
| [catalog-c-10](#catalog-c-10) | widget-catalog | Every snackbar after the first is announced from a defunct node and dropped by Orca | high | Linux | fixed |
| [catalog-c-11](#catalog-c-11) | widget-catalog | Snackbar with a custom Button trigger: a screen reader's activation of the focused button does nothing, and the working button is named with the message | high | all | fixed |
| [catalog-c-12](#catalog-c-12) | widget-catalog | A toast's body is never announced, only its title | medium | Linux | open |
| [catalog-c-13](#catalog-c-13) | widget-catalog | Showing a toast re-announces every toast already on screen | medium | Linux | fixed |
| [catalog-c-14](#catalog-c-14) | widget-catalog | The notification bell is destroyed and rebuilt on every toast: focus is re-fired, the toast's announcement is cut, and an open log closes under the reader | high | Linux | fixed |
| [catalog-c-15](#catalog-c-15) | widget-catalog | The bell never tells a reader how many notifications are unread | high | all | open |
| [catalog-c-16](#catalog-c-16) | widget-catalog | Notification log: an unnamed dialog whose 'list' holds its toolbar buttons and role-less entries that the keyboard cannot reach; Tab leaves and closes it | high | all | open |
| [catalog-c-17](#catalog-c-17) | widget-catalog | Escape on a focused toast drops focus to the window itself ('frame.') | medium | all | open |
| [catalog-c-18](#catalog-c-18) | widget-catalog | Accordion content sits inside the Accordion's own button node, so a MessageBox's 'Show details' text is inside a push button | high | all | open |
| [catalog-c-19](#catalog-c-19) | widget-catalog | The catalog's message boxes put their message behind 'Show details', so a reader hears only the title | medium | all | open (example) |
| [catalog-c-20](#catalog-c-20) | widget-catalog | Expanded/collapsed state, tree level and has-popup never reach AT-SPI (or macOS): a reader cannot tell a tree item or disclosure is expandable, or that it opened | high | Linux | upstream |
| [catalog-c-21](#catalog-c-21) | widget-catalog | Expanding or collapsing a tree row destroys the focused row and re-creates it | medium | Linux | open |
| [catalog-c-22](#catalog-c-22) | widget-catalog | StandardListItem and StandardTreeItem have no role: 'unknown' nodes everywhere they are used | medium | all | open |
| [catalog-c-23](#catalog-c-23) | widget-catalog | The Data tab's ListView, TreeView, TableView and TreeTableView are unnamed; focusing the TableView is silent | medium | Linux | open (example) |
| [catalog-c-24](#catalog-c-24) | widget-catalog | Tab never leaves the TableView (it walks cells); nothing tells a reader that Ctrl+Tab is the way out | medium | all | open |
| [catalog-c-25](#catalog-c-25) | widget-catalog | Slider value changes from the keyboard are not published until some other event syncs the tree | high | all | fixed |
| [catalog-c-26](#catalog-c-26) | widget-catalog | A ListView with no selection model does not expose its keyboard row: arrow keys are silent | high | all | open |
| [catalog-c-27](#catalog-c-27) | widget-catalog | ListView keyboard reorder: 'Moved to N of M' is emitted in the same update as the focus move, so Orca cuts it even without K2 | medium | Linux | fixed |
| [catalog-c-28](#catalog-c-28) | widget-catalog | Touch tab controls lack useful names: unnamed text field, unnamed list, a slider called 'Value' | medium | all | open (example) |
| [catalog-c-29](#catalog-c-29) | widget-catalog | The DropTarget offers a reader no way to drop without dragging, and is an unnamed panel | medium | all | open (example) |
| [catalog-c-30](#catalog-c-30) | widget-catalog | Each DropZone's empty live status label emits an empty announcement at launch | low | Linux | open |
| [catalog-c-31](#catalog-c-31) | widget-catalog | Animations tab: three toggles are all named 'Visible' | medium | all | open (example) |
| [catalog-c-32](#catalog-c-32) | widget-catalog | Content a demo hides stays in the tree: collapsed (Collapse), faded out (Fade), slid out (Slide) | medium | all | open |
| [catalog-c-33](#catalog-c-33) | widget-catalog | Cycle swaps its child in and out of the tree every 1.5 s while nothing happens | low | all | open |
| [catalog-c-34](#catalog-c-34) | widget-catalog | Overlays tab: 'Warning' and 'Error' each name two different buttons (message box and toast rows) | low | all | open (example) |
| [catalog-c-M1](#catalog-c-m1) | widget-catalog | A re-shown tooltip or popover reuses its accessibility node, which AT-SPI already declared defunct: Tab into a sticky tooltip on its second showing is silent | high | Linux | fixed |
| [catalog-c-M2](#catalog-c-m2) | widget-catalog | TableView, TreeTableView and GridView replace every visible row and cell node, and the TableView's column headers, on each keyboard move | low | Linux | open |
| [catalog-c-M3](#catalog-c-m3) | widget-catalog | Tabbing away from a rich-tooltip button with a warm (no-fade) tooltip makes Orca start reading that tooltip's text, markup and all, then cut it | low | Linux | open |

### catalog-c-01 {#catalog-c-01}

The Settings tab crashes the app in every debug build: a PrivacySettings Toggle trips its own label assertion

- **Example:** widget-catalog
- **Scenario:** catalog-c-settings-launch, catalog-c-settings-open
- **Act:** Launch with --tab settings, or Down arrow from the Touch page tab onto the Settings page tab
- **The reader should get:** The Settings tab opens; ThemeSwitcher, TextScaleControl, LanguageSwitcher, ShortcutSettings and the PrivacySettings consent toggles are in the tree, named, with their states spoken
- **The reader gets:** The process panics while building the accessibility tree and exits with status 101. The window leaves the bus and the reader is left with nothing. In release builds the assertion is compiled out and the toggle would be named through labelled\_by, but every debug build crashes, and debug is what app developers and this harness run
- **Platform:** all platforms (the panic is in teksilo-widgets, before any adapter)
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `c1a553ac` (settings-crash). Fixed part: The Settings tab crashes the app in every debug build: a PrivacySettings Toggle trips its own label assertion.
- **Where:** crates/teksilo-widgets/src/privacy\_settings.rs:436-437; crates/teksilo-widgets/src/toggle.rs:448-453
- **Evidence:**
  - `app.log: thread 'main' (2545233) panicked at crates/teksilo-widgets/src/toggle.rs:449:9:`
  - `app.log: Toggle is missing an accessible label, screen readers will announce "switch" with no context. Call .label(...) when constructing the widget.`
  - `launch: +2990.3 ms object:children-changed:remove [application] 'widget-catalog' -> [frame] ''`
  - `launch: +3021.5 ms object:state-changed:defunct 1 [frame] ''`
  - `settings-open, act 'Down arrow from Touch to Settings': COULD NOT RUN:  the application exited (101); app.log: thread 'main' (2845691) panicked at crates/teksilo-widgets/src/toggle.rs:449:9:`
  - `crates/teksilo-widgets/src/privacy_settings.rs:436-437: let toggle_id = ctx.add(Toggle::new(signal).enabled(enabled)); ctx.access_labelled_by(toggle_id, label_id); (no .labelled_externally(), which toggle.rs:139-160 documents for exactly this case)`
  - `git log -S 'access_labelled_by(toggle_id': introduced by 3a1d372a feat(a11y): text ranges on every label`
  - `catalog-c-settings-launch-20260925-132927-3163104/report.txt: +2391.5 ms object:children-changed:remove [application] 'widget-catalog' -> [frame] ''; +2425.0 ms object:state-changed:defunct 1 [frame] ''`
  - `catalog-c-settings-open-20260925-133706-3631049: act 'Down arrow from Touch to Settings' ERR the application exited (101)`
  - `grep Toggle::new over settings.rs, privacy_settings.rs, shortcut_settings.rs and text_scale_control.rs: only privacy_settings.rs:436`
- **Reproduced:** 4 of 4 runs (2 settings-launch and 1 settings-open in this session, plus 2 earlier runs); deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 runs (settings-launch x2, settings-open x1); deterministic
- **Fix idea:** Toggle::new(signal).enabled(enabled).labelled\_externally() in scope\_row. Also add a unit test that builds PrivacySettings with a telemetry handle in debug

### catalog-c-02 {#catalog-c-02}

Tabbing away from a control whose focus-opened tooltip is showing sends focus back to that control about 120 ms later

- **Example:** widget-catalog
- **Scenario:** catalog-c-tooltip-tab-away, catalog-c-overlays-tooltips, catalog-c-\*-tabwalk
- **Act:** Focus 'Hover or hold — level 1' (rich tooltip), 'Province info' (composite tooltip) or the title-bar Theme combo box (composite tooltip), wait until the tooltip is up but not yet sticky (about 0.5 to 2 s), then press Tab
- **The reader should get:** Focus moves to the next control and stays there; the tooltip closes behind it
- **The reader gets:** Focus reaches the next control, then about 120 ms later returns to the control just left. Orca's reading of the new control is cut and it reads the old control again, with its tooltip text. The reader has to press Tab a second time. If the reader waits until the tooltip turns sticky, Tab instead lands inside the tooltip (a \[tool tip\] or \[dialog\] node takes focus), so the Tab order passes through tooltips. The likely cause: a tooltip shown by focus records the focused control as its focus\_restore (overlay\_impl.rs:530-532). tooltip\_focus\_leave\_outside dismisses only tooltips already promoted by focus (the promoted\_by\_focus filter, overlay\_impl.rs:1344-1360). The dismissal that does happen, whether the pointer-leave timer or the end of the fade, calls focus\_ops(restore\_id) without checking that focus has moved on (widget\_tree.rs:1660-1667 and 1714-1722)
- **Platform:** all platforms (focus is moved inside teksilo-core); measured on Linux AT-SPI/Orca
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `b30770d5` (tooltip-snapback). Fixed part: focus returns ~120 ms after Tab from a focus-opened tooltip's anchor.
- **Where:** crates/teksilo-core/src/widget\_tree/overlay\_impl.rs:493-497, 530-531, 541; crates/teksilo-core/src/widget\_tree.rs:1655-1667 (process\_overlay\_fade\_dismissals\_real); crates/teksilo-core/src/overlay.rs dismiss\_because fade deferral
- **Evidence:**
  - `tab-away run 1, act 'wait 1 s on Hover or hold — level 1 (tooltip up, not sticky), Tab':`
  - `  +1031.3 ms object:state-changed:focused 1 [push button] 'Hover or hold — level 2'`
  - `  +1153.3 ms object:state-changed:focused 1 [push button] 'Hover or hold — level 1'`
  - `  orca-debug.out 13:08:46.358033 - SPEECH OUTPUT: 'Hover or hold — level 2 push button.'`
  - `  orca-debug.out 13:08:46.464748 - NULL SPEECH: stop`
  - `  orca-debug.out 13:08:46.464807 - SPEECH OUTPUT: 'Hover or hold — level 1 push button.'`
  - `overlays-tooltips run 125944, act 'Tab on from level 2 while its tooltip is shown': +49.2 ms object:state-changed:focused 1 [push button] 'Hover or hold — level 3' / +158.2 ms object:state-changed:focused 1 [push button] 'Hover or hold — level 2'`
  - `overlays-tabwalk: Tab 7 'Scroll tabs up' -> [combo box] 'Theme'; Tab 17 'Hover or hold — level 2' -> 'Hover or hold — level 1'; Tab 22 'Tabbed details' -> 'Province info' (Orca: 'Tabbed details push button. (CUT)' then 'Province info push button.')`
  - `tab-away runs 1 and 3, act 2: +1021.6 ms object:state-changed:focused 1 [tool tip] 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.' then Orca 'tool tip' / 'More push button.' (Tab went into a non-sticky tooltip)`
  - `verify-catalog-c-bounce-cold-20260925-133654-3621579: +1573.0 ms object:state-changed:focused 1 [push button] 'Tabbed details' / +1696.6 ms object:children-changed:remove [frame] '' -> [tool tip] 'Tooltip' / +1696.9 ms object:state-changed:focused 1 [push button] 'Province info' / +1750.9 ms ORCA SAYS: 'Province info push button.'`
  - `same run, Theme: +1527.6 ms focused 1 [push button] 'Scroll tabs up' / +1652.0 ms remove [tool tip] 'Tooltip' / +1652.3 ms focused 1 [combo box] 'Theme' / ORCA SAYS (CUT): 'Scroll tabs up push button.' then 'landmark Window title bar' 'Theme combo box.' 'Tooltip.'`
  - `catalog-c-tooltip-tab-away-20260925-133054-3241921: +1018.9 ms focused 1 'Hover or hold — level 2' / +1154.4 ms focused 1 'Hover or hold — level 1' / +1224.0 ms ORCA SAYS: 'Hover or hold — level 1 push button.'`
  - `same run, warm 'Province info' act: +1225.7 ms remove [tool tip] 'Tooltip' then +1226.0 ms focused 1 'Tabbed details': pass, no bounce`
  - `crates/teksilo-core/src/widget_tree/focus_impl.rs:487-505 splice_sticky_tooltips_after_anchors: non-sticky tooltips never enter the Tab order`
- **Reproduced:** Tab from level 1 with its tooltip up (not sticky): 2 of 3 runs (in the third the tooltip had already turned sticky, and Tab entered it). Tab from level 2: 2 of 2 tooltips runs. Tab walk: bounce at the Theme combo box in 5 of 5 walks (overlays, dragdrop, animations, touch, plus the earlier harness), at level 2 and at 'Tabbed details' in 2 of 2 overlays walks
- **Verification:** corrected by the verifier. Reproduced: Cold show then Tab: 9 of 9 (verify-catalog-c-bounce-cold x3 runs x 3 controls: 'Province info' composite, title-bar Theme combo box, 'Hover or hold — level 3' rich). Level 1 to level 2: 3 of 3 tab-away runs. Theme combo bounce in 5 of 5 tab walks (overlays, data, touch, animations, dragdrop). Level 2 to level 1 and 'Tabbed details' to 'Province info' in 1 of 1 overlays walk. Warm (no fade) Tab-away passed 3 of 3 The bounce is real and reproduces every time. The mechanism, and one sub-claim, need correcting. (1) Mechanism: when focus shows a tooltip, focus\_restore is set to the anchor (overlay\_impl.rs:530-531) and promoted\_by\_focus is set (overlay\_impl.rs:541). So tooltip\_focus\_leave\_outside does dismiss it; the promoted\_by\_focus filter does not exempt it, contrary to the sweep. Tooltips are shown with a fade of motion.duration\_fast (overlay\_impl.rs:493-497), so the dismissal only starts the fade (overlay.rs dismiss\_because). When the fade ends, process\_overlay\_fade\_dismissals\_real (widget\_tree.rs:1655-1667) calls focus\_ops(restore\_id) without checking that focus has moved on. The pointer-leave timer plays no part. widget\_tree.rs:1714-1722 is the auto-dismiss path, used only by hold-shown tooltips. Evidence for this: in each bounce the tooltip's remove and the focus restore fall in one update, about 120 ms after the Tab (+1696.6 ms remove, +1696.9 ms focus back to 'Province info'). A tooltip shown warm has no fade: it is removed at once and its focus\_restore is discarded, so there is no bounce. The warm Province info act passed 3 of 3 (tooltip added at +402 ms, removed before the focus event). (2) Tab landing in a tooltip: only sticky tooltips enter the Tab order (focus\_impl.rs:487-505). The tooltip Tab reached had been re-shown right after the bounce and had been up for more than 2 s. The \[tool tip\] role on the bus belongs to a node that libatspi already held as defunct, so it can be stale. The claim 'Tab went into a non-sticky tooltip' is therefore unsupported. What does hurt the reader there is new: the re-shown tooltip is defunct and Orca stays silent (see missed M1). Severity stays high: the reader's Tab is undone and they must press it again.
- **Fix idea:** Do not set focus\_restore for a focus-shown tooltip. Or restore focus only when focus is still inside the dismissed overlay or on its anchor. Dismiss any focus-armed tooltip, sticky or not, in tooltip\_focus\_leave\_outside. Keep non-sticky tooltips out of the Tab order

### catalog-c-03 {#catalog-c-03}

A rich tooltip opened by keyboard focus is never spoken while it shows; its text reaches the reader only when it is dismissed

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-tooltips
- **Act:** Focus 'Hover or hold — level 1' and stay 4 s, then press Escape
- **The reader should get:** While the reader rests on the button, its rich tooltip's text is spoken, through the description or an announcement, as docs/tooltips.md 'Keyboard / a11y promotion' promises
- **The reader gets:** The tooltip node is added at about 0.55 s and turns into a dialog at about 2.5 s. The button's description does not change while focus stays, and Orca says only 'Hover or hold — level 1 push button.' On the first focus the button has no description at all: a rich tooltip's content does not exist until it is shown. The text arrives as a description change only when Escape dismisses the tooltip, and Orca then reads it, markup included. On Windows, NVDA never speaks a FullDescription change (per the platform facts), so an NVDA user would not hear it at all
- **Platform:** Linux AT-SPI/Orca measured; Windows inferred from the NVDA description behaviour in the platform facts
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tooltip/rich.rs:184-196, 255-262, 559-561; crates/teksilo-core/src/widget\_tree/accessibility\_description\_impl.rs (hold-back); docs/tooltips.md:563-572
- **Evidence:**
  - `act 'focus Hover or hold — level 1 and stay': +558.7 ms object:children-changed:add [frame] '' -> [tool tip] 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
  - `  +2565.9 ms object:property-change:accessible-role [dialog] 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
  - `  +134.0 ms ORCA SAYS: 'Hover or hold — level 1 push button.' (nothing else in the 4 s act)`
  - `  FAIL 'Hover or hold — level 1' gains the tooltip text as its description while focused: no description change on 'Hover or hold — level 1' in the act`
  - `act 'Escape closes the rich tooltip': +134.2 ms object:property-change:accessible-description [push button] 'Hover or hold — level 1' text='Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
  - `  +144.1 ms ORCA SAYS: 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
  - `launch tree: [push button] 'Hover or hold — level 1' {focusable} (no desc; compare [push button] 'Save' desc='Save the current document')`
  - `catalog-c-overlays-tooltips-20260925-132947-3179986: +574.5 ms object:children-changed:add [frame] '' -> [tool tip] 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.' / +2581.3 ms accessible-role [dialog] / only ORCA SAYS: 'Hover or hold — level 1 push button.'`
  - `same run, Escape: +148.3 ms object:property-change:accessible-description [push button] 'Hover or hold — level 1' text='Level 1 of the cascade…' / +164.0 ms ORCA SAYS: 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
  - `tree-launch.txt: [push button] 'Hover or hold — level 1' {focusable} (no desc) vs [push button] 'Plain among rich' desc='Plain tooltip living in the rich column — diagnostic.'`
  - `catalog-c-tooltip-tab-away-20260925-133054-3241921: second focus of level 1 (after bounce): +1224.0 ms ORCA SAYS: 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
- **Reproduced:** 3 of 3 runs (overlays-tooltips 125944 and 131213, plus the earlier harness's 121744)
- **Verification:** corrected by the verifier. Reproduced: 2 of 2 overlays-tooltips runs; launch tree shows levels 1-3 with no desc in every overlays run (8 runs) Reproduced. The rich tooltip is added at +574 ms and turns into a dialog at +2581 ms. The button's description does not change while focus stays, and Orca says only 'Hover or hold — level 1 push button.'. Escape then publishes the description, and Orca reads it with the markup. Source cause is confirmed. RichTooltipWidget::from\_key (rich.rs:184-196) resolves its content only in build() (rich.rs:255-262), and accessibility() names the node only when content is Some (rich.rs:559-561). A registry-keyed rich tooltip therefore has no name before its first show, and DeferredSubtree's delegated accessibility (deferred\_subtree.rs:218-225) cannot give the anchor its static description. While the tooltip is shown, the description coming from described\_by is held back as long as focus stays (accessibility\_description\_impl.rs). Corrections: (a) The loss is on the first visit only. Once the tooltip has been built, the anchor carries the static description, and a later focus reads it: after the bounce, Orca said 'Hover or hold — level 1 push button.' 'Level 1 of the cascade…'. (b) So the claim that 'an NVDA user would not hear it at all' is wrong in general: NVDA reads FullDescription on focus, and misses it only on the first visit, as Orca does. Severity lowered to medium: supplementary text is missing on first visit only, not an essential control state. docs/tooltips.md:563-572 still says focus 'immediately shows + promotes' the tooltip, which is stale.
- **Fix idea:** Publish a rich tooltip's plain text as the anchor's static description from its TooltipContent even before first show, as plain tooltips do. When focus opens it, announce the text (polite) rather than relying on a held-back description change

### catalog-c-04 {#catalog-c-04}

Rich tooltip text reaches the reader with its raw link markup ('\[next link\](:tip-b)')

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-tooltips
- **Act:** Focus a rich-tooltip button, let the tooltip open, then Escape or Tab away
- **The reader should get:** The tooltip's link reads as its label ('next link'), never as markdown with a registry key
- **The reader gets:** The tooltip node is named with the unparsed markup, and the description copied onto the button carries it too. Orca reads 'Hover the \[next link\](:tip-b) to open level 2.' The cause: RichTooltipWidget::accessibility sets its name to content.text, the source string, not the rendered text
- **Platform:** all platforms (the node name is wrong); spoken on Linux/Orca
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tooltip/rich.rs:559-561
- **Evidence:**
  - `+558.7 ms object:children-changed:add [frame] '' -> [tool tip] 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
  - `+144.1 ms ORCA SAYS: 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
  - `overlays-tabwalk Tab 17: ORCA SAYS 'Level 2 of the cascade. Hover the [final link](:tip-c) for one more.'`
  - `crates/teksilo-widgets/src/tooltip/rich.rs:559-561: if let Some(content) = self.content.as_ref() { builder.set_name(content.text.resolve_now()); }`
  - `catalog-c-overlays-tooltips-20260925-133201-3303311: Escape act ORCA SAYS: 'Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.'`
  - `catalog-c-overlays-tabwalk-20260925-133734-3659642 Tab 18: ORCA SAYS 'Level 2 of the cascade. Hover the [final link](:ti…'`
- **Reproduced:** 3 of 3 tooltips runs, every tab walk through the rich column; deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 overlays-tooltips runs, 3 of 3 tab-away runs, 1 of 1 overlays walk; deterministic
- **Fix idea:** Name the node from the parsed inline text: link labels kept, targets dropped. Use the same plain text for the anchor's description

### catalog-c-05 {#catalog-c-05}

Composite tooltips are named 'Tooltip', so every composite trigger's description is the word 'Tooltip' and the content is never spoken on focus

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-tooltips, catalog-c-overlays-tabwalk
- **Act:** Focus 'Province info', 'Tabbed details' or 'With internal Button' (and the title bar's Theme combo box); Tab into the sticky tooltip
- **The reader should get:** The trigger describes what its tooltip holds (e.g. 'Iberia …'), or says nothing; the tooltip is named for its content
- **The reader gets:** Orca says 'Province info push button.' then 'Tooltip.' at every composite trigger, the Theme combo box included. The content ('Iberia', 'Treasury report') is never spoken while the reader rests on the trigger. Tabbing into the sticky surface gives 'Tooltip dialog Treasury report This quarter: +423 coins.' The cause: CompositeTooltipWidget falls back to the generic a11y\_tooltip\_name, and the not-shown description path copies the content's name onto the anchor. docs/tooltips.md claims all three tiers publish their body as the name, which is not true for composites
- **Platform:** all platforms (names and descriptions); spoken on Linux/Orca
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tooltip/composite.rs:431-438; docs/tooltips.md:625
- **Evidence:**
  - `launch tree: [push button] 'Province info' desc='Tooltip' {focusable} / [push button] 'Tabbed details' desc='Tooltip' / [push button] 'With internal Button' desc='Tooltip' / [combo box] 'Theme' desc='Tooltip'`
  - `act 'focus Province info and stay': +789.3 ms ORCA SAYS: 'Province info push button.' / +789.3 ms ORCA SAYS: 'Tooltip.' / +1046.9 ms object:children-changed:add [frame] '' -> [tool tip] 'Tooltip'`
  - `act 'Tab into the sticky composite tooltip': +87.1 ms ORCA SAYS: 'Tooltip dialog Treasury report This quarter: +423 coins.'`
  - `crates/teksilo-widgets/src/tooltip/composite.rs:431-438: .unwrap_or_else(|| teksilo_i18n::tr_widget!(a11y_tooltip_name()).resolve_now())`
  - `tree-launch.txt: [combo box] 'Theme' desc='Tooltip'; [push button] 'Province info' desc='Tooltip'; 'Tabbed details' desc='Tooltip'; 'With internal Button' desc='Tooltip'`
  - `catalog-c-overlays-tooltips-20260925-132947-3179986: +157.0 ms ORCA SAYS: 'Province info push button.' / 'Tooltip.' / +764.6 ms add [tool tip] 'Tooltip'`
- **Reproduced:** 3 of 3 tooltips runs and every tab walk; deterministic
- **Verification:** confirmed. Reproduced: every overlays run (tree) and 2 of 2 tooltips runs, 3 of 3 bounce-cold runs (Theme 'Tooltip.'), 5 of 5 walks; deterministic
- **Fix idea:** Don't copy a generic fallback name into the anchor's description. Name the composite from its first heading or text by default, or require access\_label. Fix the doc claim

### catalog-c-06 {#catalog-c-06}

Opening the 'Tabbed details' composite tooltip moves Orca's locus of focus to a page tab inside it ('Stats page tab.')

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-tooltips
- **Act:** Focus 'Tabbed details' and stay; the tooltip opens after its delay while focus stays on the button
- **The reader should get:** Nothing spoken about controls the reader is not on; Orca's locus stays on the focused button
- **The reader gets:** The tooltip's TabWidget is added with a selected tab. The AT-SPI adapter emits object:selection-changed on its page tab list, and Orca's onSelectionChanged makes the selected tab its locus of focus and says 'Stats page tab.' Orca now thinks the reader is on 'Stats' while keyboard focus is on 'Tabbed details'. The same mechanism makes Orca read the catalog's own selected page tab at every launch
- **Platform:** Linux AT-SPI/Orca
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0/src/adapter.rs:79-81; crates/teksilo-widgets/src/tab\_widget (selected tab published inside an unfocused transient overlay)
- **Evidence:**
  - `+1394.9 ms object:selection-changed [page tab list] ''`
  - `+1451.0 ms ORCA SAYS: 'Stats page tab.'`
  - `orca-debug.out 13:12:49.937326 - FOCUS MANAGER: Changing locus of focus from [push button: 'Tabbed details'] to [page tab: 'Stats']. Notify: True`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:78-80 (enqueue_selection_changed_if_needed on adding a selected node); /usr/lib/python3/dist-packages/orca/scripts/default.py:1600-1602 (set_locus_of_focus to the selected child)`
  - `catalog-c-overlays-tooltips-20260925-132947-3179986: +102.3 ms ORCA SAYS (CUT): 'Tabbed details push button.' / +780.7 ms object:selection-changed [page tab list] '' / +860.1 ms ORCA SAYS: 'Stats page tab.'`
  - `/usr/lib/python3/dist-packages/orca/scripts/default.py:1600-1602 set_locus_of_focus(event, child)`
- **Reproduced:** 5 of 5 runs of that act (overlays-tooltips 125944, 131213, tab-away run 1, 2 earlier), plus overlays-tabwalk Tab 23
- **Verification:** confirmed. Reproduced: 6 of 6 (2 of 2 overlays-tooltips, 3 of 3 tab-away, 1 of 1 overlays walk Tab 23)
- **Fix idea:** Teksilo cannot change Orca, but it can avoid the event: don't mark a tab selected in the AT tree of a transient, unfocused overlay until the tooltip is sticky or focused. Or build the tooltip's TabWidget with no tab selected. Report the Orca behaviour upstream

### catalog-c-07 {#catalog-c-07}

A popover with no focusable content takes focus on an unnamed, role-less wrapper; the reader hears nothing when it opens

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-dialogs
- **Act:** Space on 'Anchor' (PopoverButton whose content is two labels)
- **The reader should get:** The popover opens as a named dialog; focus lands on it (or its content) and the reader hears 'Popover content …'
- **The reader gets:** Focus moves to an \[unknown\] node with no name that wraps an unnamed \[dialog\]. Orca's generator finds nothing to say ('Results for \[unknown\] are pauses only'), stops speech and is silent. The cause: PopoverWidget sends focus to the PopoverBody content node (focus\_id = content\_id). PopoverBody has no accessibility(), so its role stays Unknown. The surface's dialog name defaults to empty
- **Platform:** all platforms (focus target and tree); UIA would expose a Custom control, macOS AXUnknown (per adapter role maps); measured on Linux/Orca
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `de3bb295` (popover-trigger). Fixed part: 'Anchor dialog Popover content Click outside to dismiss.' (2 of 2.
- **Where:** crates/teksilo-widgets/src/popover\_widget.rs:361, 569-651, 685
- **Evidence:**
  - `+64.5 ms object:children-changed:add [panel] '' -> [unknown] ''`
  - `+65.7 ms object:state-changed:focused 1 [unknown] ''`
  - `orca-debug.out 13:13:13.664324 - FOCUS MANAGER: Changing locus of focus from [push button: 'Anchor'] to [unknown]. Notify: True`
  - `orca-debug.out 13:13:13.705399 - SPEECH GENERATOR: Results for [unknown] are pauses only`
  - `tree: [unknown] '' {focusable,focused} > [dialog] '' > [label] 'Popover content' / [label] 'Click outside to dismiss.'`
  - `crates/teksilo-widgets/src/popover_widget.rs:683-684 (let focus_id = content_id;) and 569-651 (PopoverBody, no accessibility())`
  - `catalog-c-overlays-dialogs-20260925-133059-3247045: +79.8 ms object:children-changed:add [panel] '' -> [unknown] '' / +81.3 ms object:state-changed:focused 1 [unknown] '' (no ORCA SAYS)`
  - `orca-debug.out 13:31:10.156759 - SPEECH GENERATOR: Results for [unknown] are pauses only`
  - `tree-Space-on-Anchor--popover-.txt: [unknown] '' {focusable,focused} > [dialog] '' > [label] 'Popover content' / [label] 'Click outside to dismiss.'`
- **Reproduced:** 4 of 4 overlays-dialogs runs; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 overlays-dialogs runs + 2 of 2 verify-catalog-c-popover runs; deterministic
- **Fix idea:** When the content has no focusable descendant, focus the surface's Dialog node, not the wrapper. Give PopoverBody Role::GenericContainer so it is pruned. Default the surface name to the trigger's label

### catalog-c-08 {#catalog-c-08}

Tab inside an open popover with no focusable content closes it and throws focus to the top of the window

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-dialogs
- **Act:** Space on 'Anchor', then Tab
- **The reader should get:** Focus stays in the popover, or the popover closes and focus goes back to Anchor or on to the next control ('Open Dialog')
- **The reader gets:** The popover goes and focus jumps to the title bar's 'Menu' button, the window's first control. Orca: 'landmark Window title bar', 'Menu push button.' The reader loses their place on the page
- **Platform:** all platforms (focus is moved inside teksilo-core); measured on Linux/Orca
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `de3bb295` (popover-trigger). Fixed part: Tab from the open popover goes to 'Open Dialog', not to the title bar's 'Menu' (2 of 2.
- **Where:** crates/teksilo-widgets/src/popover\_widget.rs:685; crates/teksilo-core/src/widget\_tree/focus\_impl.rs:943-958
- **Evidence:**
  - `+30.7 ms object:children-changed:remove [panel] '' -> [unknown] ''`
  - `+31.2 ms object:state-changed:focused 1 [push button] 'Menu'`
  - `+107.5 ms ORCA SAYS: 'landmark Window title bar'`
  - `+107.5 ms ORCA SAYS: 'Menu push button.'`
  - `orca-debug.out 13:13:17.925731 - FOCUS MANAGER: Changing locus of focus from [unknown] to [push button: 'Menu']. Notify: True`
  - `catalog-c-overlays-dialogs-20260925-133059-3247045: +38.2 ms remove [panel] '' -> [unknown] '' / +38.4 ms focused 1 [push button] 'Menu' / +92.1 ms ORCA SAYS: 'landmark Window title bar' 'Menu push button.'`
  - `verify-catalog-c-popover-20260925-134330-3869397 'Shift+Tab inside the open popover': +46.6 ms focused 1 [push button] 'Notifications' / ORCA SAYS: 'Notifications push button.'`
  - `crates/teksilo-core/src/widget_tree/focus_impl.rs:951-958: cur == None -> enter_scope_edge(scope, reverse)`
- **Reproduced:** 3 of 3 runs (overlays-dialogs 130838, 131129, 131303)
- **Verification:** confirmed. Reproduced: Tab: 3 of 3 overlays-dialogs runs. Shift+Tab: 2 of 2 verify-catalog-c-popover runs. Escape straight after opening correctly returns to Anchor: 2 of 2
- **Fix idea:** Trap Tab inside the popover (it has a Dialog role), or dismiss it with focus restored to the anchor. The next focusable after a focus-only content root must never resolve to the window's first stop

### catalog-c-09 {#catalog-c-09}

The snackbar announces the generic word 'Snackbar', never its message

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** Space on 'Show snackbar'
- **The reader should get:** The reader hears 'File saved successfully'
- **The reader gets:** The alert node is named 'Snackbar' and that is all that is announced and spoken. The message text is a child label nobody reads. The example gives no .announcement(...), which the Snackbar docs ask for, and SnackbarSurface falls back to the generic a11y\_snackbar\_name rather than deriving the name from its content
- **Platform:** all platforms (announced string = node name); measured on Linux/Orca
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/tabs/overlays.rs:384-397; crates/teksilo-widgets/src/snackbar.rs:236-241
- **Evidence:**
  - `+325.8 ms object:announcement [notification] 'Snackbar' text='Snackbar'`
  - `+375.5 ms ORCA SAYS: 'Snackbar'`
  - `FAIL the bus carries an announcement of 'File saved successfully'`
  - `examples/widget_catalog/src/tabs/overlays.rs:387-397 (Snackbar::new(...).content(...).trigger(...).auto_dismiss_after(...), no .announcement)`
  - `crates/teksilo-widgets/src/snackbar.rs:236-241 (unwrap_or_else(|| tr_widget!(a11y_snackbar_name())))`
  - `catalog-c-overlays-notices-20260925-132928-3163533: +78.7 ms object:announcement [notification] 'Snackbar' text='Snackbar' / +87.5 ms ORCA SAYS: 'Snackbar'`
- **Reproduced:** 5 of 5 notices runs; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 overlays-notices runs + 3 of 3 verify-catalog-c-notices (outer-trigger show); deterministic
- **Fix idea:** Example: .announcement(tr!(overlays\_file\_saved\_successfully())). Framework: when no announcement is given, name the alert from its content's text (the Toast convention), not a generic word

### catalog-c-10 {#catalog-c-10}

Every snackbar after the first is announced from a defunct node and dropped by Orca

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** Space on 'Show snackbar', wait for it to auto-dismiss, Space again
- **The reader should get:** The second snackbar is heard like the first
- **The reader gets:** The snackbar surface is one detached node, reused each time it is shown (same AT-SPI path). The adapter marked it defunct when it left the tree after the first show. The second announcement comes from that defunct path and Orca logs 'Ignoring defunct object'. The harness's observation flags it. This is the K2 mechanism on a different node: the K2 fix to the announcer does not cover it
- **Platform:** Linux AT-SPI/Orca (defunct handling is AT-SPI's)
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: same Snackbar mechanism, verified through dialogs-snackbar rather than the catalog-c scenario.
- **Where:** crates/teksilo-widgets/src/snackbar.rs:65-92, 409; accesskit\_atspi\_common-0.20.0/src/adapter.rs:91-106
- **Evidence:**
  - `act 'Space on Show snackbar (second time)': +75.5 ms object:announcement [notification] 'Snackbar' text='Snackbar'`
  - `observed 'Snackbar' came from a node the bus had already been told was defunct, which Orca drops; path /org/a11y/atspi/accessible/0/79228451076681882632059879424`
  - `orca-debug.out 13:14:38.198462 EVENT MANAGER: Ignoring defunct object: [notification: 'Snackbar']`
  - `first show's end: +3250.4 ms object:state-changed:defunct 1 [notification] 'Snackbar'`
  - `crates/teksilo-widgets/src/snackbar.rs:395-406 (content_id created once in build) and 63-92 (present_snackbar re-activates the same content_id)`
  - `catalog-c-overlays-notices-20260925-132928-3163533 act 'Space on Show snackbar (second time)': +60.7 ms object:announcement [notification] 'Snackbar' path /org/a11y/atspi/accessible/0/79228451076681882632059879424 / orca 13:29:44.903996 EVENT MANAGER: Ignoring defunct object: [notification: 'Snackbar']`
  - `first show: +3064.2 ms object:state-changed:defunct 1 [notification] 'Snackbar'`
- **Reproduced:** 5 of 5 runs (3 by Space in this session, 2 earlier by AT-SPI click); deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 overlays-notices runs; deterministic
- **Fix idea:** Give each presentation a fresh AccessKit node id, or keep the surface node in the tree while dormant and announce by name change. The same fix the announcer needs for K2

### catalog-c-11 {#catalog-c-11}

Snackbar with a custom Button trigger: a screen reader's activation of the focused button does nothing, and the working button is named with the message

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** AT-SPI 'click' on the focused 'Show snackbar' button (what VoiceOver's VO-Space / AXPress and NVDA's default action do)
- **The reader should get:** The snackbar opens
- **The reader gets:** Nothing happens: no announcement, no overlay. The trigger is a push button nested in another push button. The outer one is not focusable, is named 'File saved successfully' (the snackbar's label, i.e. its message) and holds the working Click. The inner focusable 'Show snackbar' Button handles Click itself (Button::on\_access\_action returns Handled with no callback), so the action never reaches OverlayTrigger's on\_access\_activate. Keyboard Space works only because the key handlers go on the child
- **Platform:** all platforms (action routing in teksilo); measured through AT-SPI Action
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `de3bb295` (popover-trigger). Fixed part: the reader's click on the focused custom Button trigger of a Snackbar announces 'Snackbar' (3 of 3); the outer push button named 'File saved successfully' is gone from the tree.
- **Where:** crates/teksilo-widgets/src/button.rs:454-465; crates/teksilo-widgets/src/overlay\_trigger.rs:243-252, 291-312; crates/teksilo-widgets/src/snackbar.rs:475-494; example: examples/widget\_catalog/src/tabs/overlays.rs:387
- **Evidence:**
  - `act 'AT-SPI click on the focused Show snackbar': == harness:action click [push button] 'Show snackbar' / FAIL some announcement reaches the bus: no object:announcement in the act`
  - `launch tree: [push button] 'File saved successfully' (states enabled, sensitive, no focusable; actions click) > [push button] 'Show snackbar' {focusable}`
  - `earlier run: AT-SPI click on [push button] 'File saved successfully' -> +67.5 ms object:announcement [notification] 'Snackbar'`
  - `crates/teksilo-widgets/src/button.rs:454-464 (Click -> act_access -> Handled); crates/teksilo-widgets/src/snackbar.rs:475-500 (on_access_activate on the OverlayTrigger's own node, .name(label))`
  - `verify-catalog-c-notices-20260925-133807-3691376: 'AT-SPI click on the focused inner Show snackbar button': no announcement / 'AT-SPI click on the outer trigger node': +69.1 ms object:announcement [notification] 'Snackbar' / +78.8 ms ORCA SAYS: 'Snackbar'`
  - `tree-launch.txt: [push button] 'File saved successfully' > [push button] 'Show snackbar' {focusable}`
- **Reproduced:** 5 of 5 runs; deterministic
- **Verification:** corrected by the verifier. Reproduced: inner click: 3 of 3 overlays-notices + 3 of 3 verify-catalog-c-notices; outer click works 3 of 3 The failure is real. AT-SPI click on the focused inner 'Show snackbar' does nothing in 6 of 6 tries. Click on the outer \[push button\] 'File saved successfully' opens the snackbar 3 of 3, which is my control case. Button handles Click itself and returns Handled (button.rs:454-465), so OverlayTrigger's on\_access\_action (overlay\_trigger.rs:243-252) is never reached. This affects VoiceOver's VO-Space/AXPress, Voice Control and switch access; keyboard Space works. Layer correction: the outer node is named with the message because the example passes the message as Snackbar::new's label, which is documented as the trigger button's text (snackbar.rs:280-281). snackbar.rs:494 applies that label to the OverlayTrigger as .name(label). The framework's part is the dead AT Click, plus a role=Button wrapper around a Button.
- **Fix idea:** When the custom trigger is itself a button, route its activation into present\_snackbar and make OverlayTrigger a GenericContainer with no name and no role, as PopoverButton does with its trigger. Never name the trigger with the snackbar's message

### catalog-c-12 {#catalog-c-12}

A toast's body is never announced, only its title

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** Space on the 'Warning' or 'Error' toast trigger
- **The reader should get:** 'Warning. Take a look when you have a moment.' / 'Build failed. Three errors, two warnings.'
- **The reader gets:** The announcement carries the node name, which is the title only. The body goes to the description, which no adapter announces. live\_atomic is set but AT-SPI's announcement is the name. The reader hears 'Warning' and 'Build failed' and nothing else
- **Platform:** Linux AT-SPI/Orca measured; Windows LiveRegionChanged makes NVDA read the node, likely name only too (not measured)
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toast/surface.rs:426-446
- **Evidence:**
  - `+185.1 ms object:announcement [status bar] 'Warning' text='Warning' / +325.6 ms ORCA SAYS: 'Warning' / FAIL Orca says 'Take a look when you have a moment.'`
  - `+449.0 ms object:announcement [notification] 'Build failed' text='Build failed' / FAIL Orca says 'Three errors, two warnings.'`
  - `crates/teksilo-widgets/src/toast/surface.rs:426-446 (set_name(title); set_description(body))`
  - `catalog-c-overlays-notices-20260925-132928-3163533: +92.8 ms object:announcement [status bar] 'Warning' text='Warning' / +104.4 ms ORCA SAYS: 'Warning' (body unheard); +99.8 ms object:announcement [notification] 'Build failed' / +118.4 ms ORCA SAYS: 'Build failed'`
- **Reproduced:** 5 of 5 notices runs; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 overlays-notices runs; deterministic
- **Fix idea:** Put 'title. body' in the name when no explicit announcement is given, or default the announcement to title + body

### catalog-c-13 {#catalog-c-13}

Showing a toast re-announces every toast already on screen

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** Space on 'Info', then on 'Success'
- **The reader should get:** The reader hears 'Saved' once
- **The reader gets:** The ToastHost rebuilds on every registry change, re-adding each visible toast as a new node (new AT-SPI path), and every live node added with a name is announced again. The reader hears 'Info notice', 'Saved'. Later toasts re-announce 'Warning', 'Build failed' or 'Working…' the same way
- **Platform:** Linux AT-SPI/Orca measured (add-with-name announcement is atspi\_common adapter.rs:71-77; Windows raises LiveRegionChanged on new live nodes similarly)
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: in widget-catalog, showing a toast no longer re-announces the toasts already shown (3 of 3 on every toast act)..
- **Where:** crates/teksilo-widgets/src/toast/host.rs:236-270
- **Evidence:**
  - `act 'Space on toast trigger Success': +244.2 ms object:announcement [status bar] 'Info notice' text='Info notice' / +262.8 ms object:announcement [status bar] 'Saved' text='Saved' / +370.3 ms ORCA SAYS: 'Info notice' / +393.1 ms ORCA SAYS: 'Saved'`
  - `announcement source paths: first 'Info notice' from .../79228451224255835221736292352, re-announcement from .../237684776252784510408824193024 (a new node)`
  - `act 'Error' (run 131038): ann=['Warning', 'Build failed'] said=['Warning', 'Build failed']`
  - `crates/teksilo-widgets/src/toast/host.rs:252-256 (registry.version_signal() bound at BindingLevel::Rebuild)`
  - `catalog-c-overlays-notices-20260925-132928-3163533 'Space on toast trigger Success': +63.7 ms object:announcement [status bar] 'Info notice' / +65.9 ms object:announcement [status bar] 'Saved' / +66.8 ms remove [status bar] 'Info notice' / ORCA SAYS 'Info notice', 'Saved'`
  - `same run, between acts at 13:30:54.568: object:announcement [status bar] 'Working…'; focused 1 [status bar] 'Working…' (new node); defunct 1 [status bar] 'Working…' (old); Orca 13:30:54.402085 re-speaks 'Working…'`
- **Reproduced:** 'Success' re-announcing 'Info notice': 5 of 5 notices runs; some other re-announcement in 5 of 5
- **Verification:** confirmed. Reproduced: 3 of 3 overlays-notices runs (Success act re-announces 'Info notice'); expiry re-announce 3 of 3 verify-catalog-c-notices
- **Fix idea:** Keep existing toast widgets across host updates (keyed reconciliation), so only the new toast is added to the tree

### catalog-c-14 {#catalog-c-14}

The notification bell is destroyed and rebuilt on every toast: focus is re-fired, the toast's announcement is cut, and an open log closes under the reader

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** With focus on the Notifications bell (or inside its open log), a new toast arrives
- **The reader should get:** Focus and the reader's place stay; the toast is heard; the unread count updates
- **The reader gets:** NotificationCenterButton rebinds on the archive version at Rebuild. Every toast replaces the bell (and its popover) with new nodes: the focused bell goes defunct and a focus event fires on the new one. Orca stops the toast announcement to re-read 'Notifications push button.' With the log open and focus on 'Clear all', the log's dialog goes defunct and focus is pulled to the new bell
- **Platform:** Linux AT-SPI/Orca measured; the node replacement happens on all platforms
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: the focused bell is no longer replaced on each toast, so the toast announcement is not cut. Only the unread badge follows the archive..
- **Where:** crates/teksilo-widgets/src/notification/center\_button.rs:230-252
- **Evidence:**
  - `run 125949: +86.5 ms object:announcement [status bar] 'Working…' / +89.0 ms object:announcement [status bar] 'Info notice' / +90.8 ms object:state-changed:focused 1 [push button] 'Notifications' / +91.1 ms object:state-changed:defunct 1 [push button] 'Notifications' / +104.6 ms ORCA SAYS (CUT): 'Working…' / +109.4 ms ORCA SAYS (CUT): 'Info notice' / +208.6 ms ORCA SAYS: 'Notifications push button.'`
  - `run 132036 (log open, focus on 'Clear all'): +91.7 ms object:state-changed:focused 1 [push button] 'Notifications' / +92.0 ms object:state-changed:defunct 1 [dialog] '' / +92.1 ms object:state-changed:defunct 1 [push button] 'Notifications' / +104.4 ms ORCA SAYS (CUT): 'Info notice' / +183.0 ms ORCA SAYS: 'Notifications push button.'`
  - `every toast act: object:state-changed:defunct 1 [push button] 'Notifications'`
  - `crates/teksilo-widgets/src/notification/center_button.rs:248-252 (archive.version_signal().bind_to(..., BindingLevel::Rebuild))`
  - `verify-catalog-c-notices-20260925-133807-3691376: +63.3 ms object:announcement [status bar] 'Info notice' / +65.2 ms focused 1 [push button] 'Notifications' (new path …/237684704052228205909639168000) / +65.5 ms defunct [push button] 'Notifications' (old …/79228379042146274796260818944) / +74.5 ms ORCA SAYS (CUT): 'Info notice' / +132.1 ms ORCA SAYS: 'Notifications push button.'`
  - `catalog-c-overlays-notices-20260925-132928-3163533: +175.3 ms remove [panel] '' -> [dialog] '' / +175.4 ms focused 1 [push button] 'Notifications' / +175.5 ms focused 0 [push button] 'Clear all'`
- **Reproduced:** Bell replaced on every toast: 5 of 5 runs. Announcement cut with focus on the bell: 3 of 3 runs where focus was on it (125949, 122002 earlier, 132036)
- **Verification:** confirmed. Reproduced: bell focused, log closed: 3 of 3 verify-catalog-c-notices; log open: 3 of 3 overlays-notices
- **Fix idea:** Bind the badge label and count at Relayout or RepaintOnly (a Signal&lt;String&gt; for the badge text) and keep the PopoverIconButton alive; never rebuild the focused trigger

### catalog-c-15 {#catalog-c-15}

The bell never tells a reader how many notifications are unread

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** Focus the Notifications bell after five toasts
- **The reader should get:** 'Notifications, 5 unread' (in the name, value or description)
- **The reader gets:** The count is a separate sibling label ('5') outside the button. The button is named 'Notifications' with description 'Notifications' (a duplicate that Orca drops). Orca says 'Notifications push button.' The widget docs say outright that the count is not announced
- **Platform:** all platforms
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/notification/center\_button.rs:14-21, 316-330
- **Evidence:**
  - `+99.5 ms ORCA SAYS: 'Notifications push button.' / FAIL Orca says '5'`
  - `tree: [push button] 'Notifications' desc='Notifications' {focusable} ... [label] '5' (sibling, not a child)`
  - `crates/teksilo-widgets/src/notification/center_button.rs:14-21 ('The badge count is not separately announced')`
  - `tree-Space-on-toast-trigger--Loading-.txt: [push button] 'Notifications' desc='Notifications' {focusable} / [label] '5'`
- **Reproduced:** 5 of 5 notices runs; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 overlays-notices runs; deterministic
- **Fix idea:** Put the count into the bell's accessible name or description (a localized 'N unread'), bound reactively, and hide the badge label from AT

### catalog-c-16 {#catalog-c-16}

Notification log: an unnamed dialog whose 'list' holds its toolbar buttons and role-less entries that the keyboard cannot reach; Tab leaves and closes it

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** Space on the bell, then Tab past 'Clear all'
- **The reader should get:** A dialog named 'Notifications' whose entries are list items the keyboard can reach (and whose actions it can use), with focus kept inside while it is open
- **The reader gets:** Orca: 'dialog Today Show errors (no longer available)', 'Notifications.', 'list.', 'Mark all read push button.' The dialog has no name, so Orca makes one from its text. The \[list\] contains the two toolbar buttons, and its entries are \[unknown\] nodes (StandardListItem) that are not focusable, so no keyboard route reaches them. Tab from 'Clear all' goes out to a toast ('Working… statusbar') and the log closes behind it
- **Platform:** all platforms (tree and focus); spoken on Linux/Orca
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/notification/log.rs:260-300, 556-557; crates/teksilo-widgets/src/notification/center\_button.rs:280-288
- **Evidence:**
  - `+189.0 ms ORCA SAYS: 'dialog Today Show errors (no longer available)' / 'Notifications.' / 'list.' / 'Mark all read push button.'`
  - `tree: [dialog] '' {active} > [list] 'Notifications' > [push button] 'Mark all read' {focusable,focused} / [push button] 'Clear all' / [panel] '' > [label] 'Today' / [unknown] 'Build failed' desc='Three errors, two warnings.' / [unknown] 'Warning' desc='Take a look when you have a moment.'`
  - `act 'Tab from the log's last control': +46.1 ms object:state-changed:focused 1 [status bar] 'Working…' / +47.9 ms object:state-changed:defunct 1 [dialog] '' / +119.6 ms ORCA SAYS: 'Working… statusbar.'`
  - `crates/teksilo-widgets/src/notification/log.rs:556-557 (Role::List on the root that also holds the toolbar); center_button.rs builds the PopoverIconButton with no surface name`
  - `catalog-c-overlays-notices-20260925-133537-3552653 tree: [dialog] '' {active} > [list] 'Notifications' > [push button] 'Mark all read' / [push button] 'Clear all' / [panel] '' > [label] 'Today' / [unknown] 'Working…' desc='Working…' / [unknown] 'Build failed' desc='Three errors, two warnings.'`
  - `catalog-c-overlays-notices-20260925-132928-3163533 'Tab from the log's last control': +33.6 ms remove [panel] '' -> [dialog] '' / +34.0 ms focused 1 [status bar] 'Working…' / ORCA SAYS: 'Working… statusbar.'`
- **Reproduced:** Unnamed dialog and unknown entries: 5 of 5 runs. Tab leaving and closing the log: 3 of 3 runs (131038, 131420, 132036)
- **Verification:** confirmed. Reproduced: tree 3 of 3; Tab out of the log 3 of 3 overlays-notices runs
- **Fix idea:** Name the popover surface 'Notifications'. Keep the toolbar out of the List. Give rows Role::ListItem and a roving-focus or active-descendant model so arrows reach them. Contain Tab in the open popover

### catalog-c-17 {#catalog-c-17}

Escape on a focused toast drops focus to the window itself ('frame.')

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-notices
- **Act:** Focus in a toast (reached by Tab from the notification log), press Escape
- **The reader should get:** The toast is dismissed and focus returns to where the reader was (the log or the bell)
- **The reader gets:** The toast goes defunct and focus moves to the \[frame\]. Orca says 'frame.' and the reader's place is lost
- **Platform:** all platforms (focus moved inside teksilo); measured on Linux/Orca
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toast/surface.rs:373-392; crates/teksilo-widgets/src/toast/host.rs (rebuild)
- **Evidence:**
  - `run 131038: +27.1 ms object:state-changed:focused 1 [frame] '' / +27.1 ms object:state-changed:focused 0 [push button] 'Clear' / +76.7 ms ORCA SAYS: 'frame.'`
  - `run 131420: +30.6 ms object:state-changed:focused 1 [frame] '' / +30.7 ms object:state-changed:defunct 1 [push button] 'Clear' / +30.8 ms object:state-changed:defunct 1 [status bar] 'Working…' / +72.0 ms ORCA SAYS: 'frame.'`
  - `verify-catalog-c-notices-20260925-133807-3691376 'Escape on the focused toast': +37.7 ms remove [frame] '' -> [status bar] 'Working…' / +37.8 ms focused 1 [frame] '' / +92.9 ms ORCA SAYS: 'frame.'`
- **Reproduced:** 2 of 3 runs; in the third a hover tooltip under the harness's stationary pointer took the Escape instead (see harness\_issues)
- **Verification:** confirmed. Reproduced: 3 of 3 verify-catalog-c-notices (clean); 0 of 3 catalog-c-overlays-notices (Escape consumed by a hover tooltip, harness artifact)
- **Fix idea:** Record where focus came from when a toast takes focus and restore it on dismissal; never fall back to the root

### catalog-c-18 {#catalog-c-18}

Accordion content sits inside the Accordion's own button node, so a MessageBox's 'Show details' text is inside a push button

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-dialogs
- **Act:** Open any message box, Shift+Tab to 'Show details', Space
- **The reader should get:** A disclosure button, and the revealed details as a sibling region the reader can move to
- **The reader gets:** The Accordion root is itself Role::Button, and the details region is its child: \[push button\] 'Show details' &gt; \[landmark\] 'Show details' &gt; \[panel\] &gt; \[label\] text. Readers treat a button as a leaf (ARIA's children-presentational; VoiceOver does not navigate into AXButton children), so the text is effectively unreachable. The reveal is not spoken either: Orca said nothing on Space
- **Platform:** all platforms (tree shape); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/accordion.rs:667-682
- **Evidence:**
  - `FAIL the details are not inside the button: [alert] 'Dialog example' > [push button] 'Show details' > [landmark] 'Show details' > [panel] '' > [label] 'This is a Dialog (presented via MessageBox::information).'`
  - `act 'Space on Show details': +82.0 ms object:children-changed:add [panel] '' -> [label] 'This is a Dialog (presented via MessageBox::information).' (no Orca speech in the act)`
  - `crates/teksilo-widgets/src/accordion.rs:668-682 (Role::Button + set_expanded + push_controlled on the root; children() returns header and body); crates/teksilo-widgets/src/message_box.rs:937-952 (detailed text in an Accordion)`
  - `catalog-c-overlays-dialogs-20260925-133059-3247045 'Space on Show details': +104.5 ms object:children-changed:add [panel] '' -> [label] 'This is a Dialog (presented via MessageBox::information).' (no ORCA SAYS) / FAIL the details are not inside the button`
- **Reproduced:** 4 of 4 overlays-dialogs runs; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 overlays-dialogs runs; deterministic
- **Fix idea:** Give the Button role to the header node only, and make the Accordion root a GenericContainer (or group), so the region is the header's sibling. This affects every Accordion (ToolBox, docking panes)

### catalog-c-19 {#catalog-c-19}

The catalog's message boxes put their message behind 'Show details', so a reader hears only the title

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-dialogs
- **Act:** Space on 'Information', 'Warning', 'Error', 'Confirm' or 'Open Dialog'
- **The reader should get:** 'alert Warning. Disk is almost full. OK push button.'
- **The reader gets:** 'alert Warning.' / 'OK push button.' The message ('Disk is almost full.', 'This action cannot be undone.') was passed as detailed\_text, which is hidden behind the disclosure. The example should use .text(...) or .informative\_text(...), which MessageBox writes into the dialog's description
- **Platform:** all platforms
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/tabs/overlays.rs:327-378
- **Evidence:**
  - `+200.2 ms ORCA SAYS (CUT): 'Warning' / +635.4 ms ORCA SAYS: 'alert Warning.' / +635.5 ms ORCA SAYS: 'OK push button.' / FAIL Orca says 'Disk is almost full.'`
  - `examples/widget_catalog/src/tabs/overlays.rs:330, 345, 356, 367, 378 (.detailed_text(...) as the only message)`
  - `catalog-c-overlays-dialogs-20260925-133059-3247045 'Space on Warning': +64.7 ms ORCA SAYS (CUT): 'Warning' / +101.0 ms ORCA SAYS: 'alert Warning.' 'OK push button.'`
- **Reproduced:** 3 of 3 current runs × 5 boxes; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 overlays-dialogs runs x 5 boxes; deterministic
- **Fix idea:** Use .text(...) for the message and keep detailed\_text for genuinely detailed content

### catalog-c-20 {#catalog-c-20}

Expanded/collapsed state, tree level and has-popup never reach AT-SPI (or macOS): a reader cannot tell a tree item or disclosure is expandable, or that it opened

- **Example:** widget-catalog
- **Scenario:** catalog-c-data-views
- **Act:** TreeView: Down to 'Documents', Right to expand, Left to collapse; TreeTableView: expand 'docs'; 'Show details'
- **The reader should get:** 'Documents, collapsed' / 'expanded' / 'collapsed', and 'level 2' on children
- **The reader gets:** Orca says only 'Documents.' before and after. The tree items carry no expandable, expanded or collapsed state and no level attribute. accesskit\_atspi\_common 0.20's state() maps no Expanded/Expandable/Collapsed and exports no level or has-popup. accesskit\_macos 0.27 has no expanded mapping either. accesskit\_windows does expose ExpandCollapsePattern (node.rs:718-723), so Windows is fine
- **Platform:** Linux AT-SPI (measured) and macOS (source); Windows has it
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0/src/node.rs:300-384, 415-437
- **Evidence:**
  - `act 'Down in the TreeView': +165.8 ms ORCA SAYS: 'Documents.' / FAIL Orca says 'collapsed'`
  - `act 'Right (expand Documents)': +212.3 ms ORCA SAYS: 'Documents.' / FAIL Orca says 'expanded' (tree now holds 'Projects')`
  - `TreeTableView: +556.7 ms ORCA SAYS: 'docs.' / FAIL Orca says 'expanded' (tree holds 'README.md')`
  - `tree: [tree item] 'Documents' {selectable} attrs={'posinset': '1'} (no setsize, no level, no expandable state)`
  - `accesskit_atspi_common-0.20.0/src/node.rs:300-384 (state(): no Expanded/Expandable/Collapsed); grep -i 'expand|level|popup' in that crate finds only role mappings`
  - `catalog-c-data-views-20260925-133029-3216175 'Right (expand Documents)': +216.7 ms ORCA SAYS: 'Documents.' / FAIL Orca says 'expanded'`
  - `TreeTableView: ORCA SAYS 'docs.' only`
- **Reproduced:** 3 of 3 data-views runs; deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 data-views runs; 3 of 3 overlays-dialogs ('collapsed'/'expanded' never said)
- **Fix idea:** Upstream: map expanded to STATE\_EXPANDABLE\|EXPANDED\|COLLAPSED and export 'level'. Until then Teksilo could announce 'expanded'/'collapsed' on a keyboard toggle, or put the state in the description on Linux

### catalog-c-21 {#catalog-c-21}

Expanding or collapsing a tree row destroys the focused row and re-creates it

- **Example:** widget-catalog
- **Scenario:** catalog-c-data-views
- **Act:** Right or Left on a TreeView or TreeTableView row
- **The reader should get:** The same row node stays focused; only its state and its children change
- **The reader gets:** The focused row goes defunct and a new node with the same name takes focus. Orca logs 'Ignoring defunct object: \[tree item: 'Documents'\]' and re-reads the row ('Documents.' twice on Left, Left, the first cut). Any reader context on the row (review position, pending events) is lost
- **Platform:** Linux AT-SPI/Orca measured; node replacement happens on all platforms
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tree\_view (row realization on reflatten); crates/teksilo-widgets/src/tree\_table\_view.rs
- **Evidence:**
  - `+74.1 ms object:state-changed:focused 1 [tree item] 'Documents' / +74.2 ms object:state-changed:focused 0 [tree item] 'Documents'`
  - `FAIL no object:state-changed:defunct event from [tree item] '*' / orca-debug.out 13:09:09.618033 EVENT MANAGER: Ignoring defunct object: [tree item: 'Documents']`
  - `act 'Left then Left': ORCA SAYS (CUT): 'Documents.' then 'Documents.'`
  - `catalog-c-data-views-20260925-133029-3216175: +69.7 ms focused 1 [tree item] 'Documents' / +69.7 ms focused 0 [tree item] 'Documents' / +71.8 ms defunct 1 [tree item] 'Documents' / orca 13:30:57.527358 Ignoring defunct object: [tree item: 'Documents']`
- **Reproduced:** 3 of 3 data-views runs
- **Verification:** confirmed. Reproduced: 2 of 2 data-views runs
- **Fix idea:** Key row widgets by NodeId across reflattens so an expand keeps the row's AccessKit id; rebuild only the inserted and removed rows

### catalog-c-22 {#catalog-c-22}

StandardListItem and StandardTreeItem have no role: 'unknown' nodes everywhere they are used

- **Example:** widget-catalog
- **Scenario:** catalog-c-data-views, catalog-c-overlays-notices
- **Act:** Launch on the Data tab; open the notification log
- **The reader should get:** Standalone items expose a list item or tree item role (or none at all); inside a ListView or TreeView the row is not duplicated
- **The reader gets:** Every StandardListItem/StandardTreeItem publishes a name and no role, so AccessKit's default Role::Unknown reaches AT-SPI as 'unknown' (UIA: Custom, macOS: AXUnknown per the adapters' role maps). In ListView and TreeView each \[list item\]/\[tree item\] has an \[unknown\] child repeating its name. Standalone items and the notification-log entries are bare \[unknown\] nodes
- **Platform:** all platforms
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/standard\_item.rs:916-935
- **Evidence:**
  - `[list item] 'Row 1' {selectable} attrs={'posinset': '1', 'setsize': '10'} > [unknown] 'Row 1' attrs={'setsize': '10'}`
  - `[unknown] 'First item' / [unknown] 'Second item' / [unknown] 'Root' / [unknown] 'Child A' (standalone section)`
  - `[tree item] 'Documents' > [unknown] 'Documents'`
  - `launch audit: unknown-role: [unknown] 'Row 1': a node whose role the adapter could not map`
  - `crates/teksilo-widgets/src/standard_item.rs:916-935 (set_name / set_description, no set_role)`
  - `tree-focus-the-ListView.txt: [list item] 'Row 1' {selectable} attrs={'setsize': '10', 'posinset': '1'} > [unknown] 'Row 1'; [unknown] 'First item' / 'Second item' / 'Root'`
- **Reproduced:** every tree taken of the Data tab and every open log (8 runs); deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 data-views trees + 3 of 3 notification-log trees; deterministic
- **Fix idea:** Inside a data view, make the row content a GenericContainer or hide it (the wrapper owns name and role). Standalone, set Role::ListItem / Role::TreeItem

### catalog-c-23 {#catalog-c-23}

The Data tab's ListView, TreeView, TableView and TreeTableView are unnamed; focusing the TableView is silent

- **Example:** widget-catalog
- **Scenario:** catalog-c-data-views, catalog-c-data-tabwalk
- **Act:** Tab walk and data-views on the Data tab
- **The reader should get:** 'People, table' / 'Files, tree table' etc.; at least something spoken when the table takes focus
- **The reader gets:** 'list box.', 'tree.', 'tree table.'. The TableView says nothing at all: Orca's table generator needs the AT-SPI Table interface, which AccessKit does not implement, and with no name there is nothing else. Only the GridView is named ('Tiles')
- **Platform:** Linux/Orca measured (table silence is AT-SPI-specific)
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/tabs/data.rs:66, 88, 135, 171-210
- **Evidence:**
  - `+52.0 ms object:state-changed:focused 1 [list box] '' / +100.6 ms ORCA SAYS: 'list box.'`
  - `+705.9 ms object:state-changed:focused 1 [table] '' (no ORCA SAYS in the act)`
  - `data-tabwalk orca-debug.out 13:18:01.670976 - SPEECH GENERATOR: Results for [table] are pauses only`
  - `examples/widget_catalog/src/tabs/data.rs:62, 87, 126, 210 (no a11y label; compare 283 .a11y_label("Tiles"))`
  - `catalog-c-data-tabwalk-20260925-133937-3759260 Tab 15 [table] '' :: [] / orca-debug.out 13:40:11.335059 SPEECH GENERATOR: Results for [table] are pauses only`
- **Reproduced:** 3 of 3 data-views runs and the data tab walk
- **Verification:** confirmed. Reproduced: 2 of 2 data-views + 1 of 1 data tab walk
- **Fix idea:** Label each view with its section heading (a11y\_label or labelled\_by). Framework: consider naming a table with its row and column count when unnamed

### catalog-c-24 {#catalog-c-24}

Tab never leaves the TableView (it walks cells); nothing tells a reader that Ctrl+Tab is the way out

- **Example:** widget-catalog
- **Scenario:** catalog-c-data-views, catalog-c-data-tabwalk
- **Act:** Tab into the People table, then keep pressing Tab
- **The reader should get:** Tab moves to the next control, or the reader is told how to leave (WCAG 2.1.2)
- **The reader gets:** Each Tab moves to the next cell ('Admin.', '$45000.', 'Blake.' ...). The 30-stop tab walk never left the table. Ctrl+Tab does leave (documented in docs/table-view.md:498-501 as the default tab\_traversal), but no description or hint says so
- **Platform:** all platforms
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/keyboard.rs:431-460; crates/teksilo-widgets/src/table\_view/column.rs:188-192
- **Evidence:**
  - `data-tabwalk: Tab 16 [table cell] '' :: Admin. / Tab 17 :: $45000. / ... / Tab 30 [table cell] '' :: Finn.`
  - `act 'Ctrl+Tab leaves the table': +1643.0 ms object:state-changed:focused 1 [tree table] '' / ORCA SAYS: 'tree table.'`
  - `catalog-c-data-views-20260925-133507-3516143 'Tab five more times': five focus moves, each to a [table cell]; 'Ctrl+Tab leaves the table': ORCA SAYS 'tree table.'`
- **Reproduced:** 3 of 3 data-views runs plus the tab walk; deterministic by design
- **Verification:** confirmed. Reproduced: 2 of 2 data-views + 1 of 1 data tab walk (Tabs 16-30 all table cells)
- **Fix idea:** Default to Tab leaving the table (arrow keys walk cells), or publish a keyboard hint (description or keyshortcuts) naming Ctrl+Tab

### catalog-c-25 {#catalog-c-25}

Slider value changes from the keyboard are not published until some other event syncs the tree

- **Example:** widget-catalog
- **Scenario:** catalog-c-touch
- **Act:** Focus the Touch tab's slider ('Value', 40) and press Right
- **The reader should get:** The reader hears '41' at once
- **The reader gets:** No accessible-value change arrives during the 2.5 s after Right, and Orca is silent. The new value is only emitted when the next event (Tab) forces an accessibility sync, in the same update as the focus move, so Orca's '41' is cut at once. The likely cause: Slider never binds its value for accessibility (no BindingLevel::AccessibilityOnly in slider.rs, unlike SpinBox or ProgressBar). accessibility() reads self.value only when the node is re-walked for another reason
- **Platform:** all platforms (the tree update is never produced); measured on Linux/Orca
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Where:** crates/teksilo-widgets/src/slider.rs:340-420, 786-799
- **Evidence:**
  - `act 'Right on the slider': FAIL the slider's new value reaches the bus: no accessible-value change on a slider in the act / FAIL Orca says '41'`
  - `act 'Tab to the splitter handle': +50.5 ms object:property-change:accessible-value [slider] 'Value' / +60.6 ms object:state-changed:focused 1 [separator] 'Splitter divider' / +72.8 ms ORCA SAYS (CUT): '41'`
  - `crates/teksilo-widgets/src/slider.rs:786-799 (accessibility reads self.value.get()); grep 'AccessibilityOnly' finds no binding in slider.rs`
  - `catalog-c-touch-20260925-132932-3164675 'Right on the slider': FAIL no accessible-value change on a slider / 'Tab to the splitter handle': +43.8 ms object:property-change:accessible-value [slider] 'Value' / +48.8 ms focused 1 [separator] / +85.5 ms ORCA SAYS (CUT): '41'`
- **Reproduced:** 4 of 4 runs (3 current touch runs plus 1 earlier with two Right presses, both unpublished)
- **Verification:** confirmed. Reproduced: 3 of 3 touch runs
- **Fix idea:** self.value.bind\_to(self\_id, ctx.binding\_registry(), BindingLevel::AccessibilityOnly) in Slider::build

### catalog-c-26 {#catalog-c-26}

A ListView with no selection model does not expose its keyboard row: arrow keys are silent

- **Example:** widget-catalog
- **Scenario:** catalog-c-touch
- **Act:** Touch tab: focus the reorderable list, press Down, Down
- **The reader should get:** Each Down moves a focus (or active-descendant) to the next row and the reader hears 'row 1', 'row 2'
- **The reader gets:** No focus event of any kind and no speech, though the list is tracking a current row internally: the next Alt+Down moves 'row 2' and only then does focus appear on it. A reader cannot hear which row they are about to reorder
- **Platform:** all platforms (no tree focus is produced); measured on Linux/Orca
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/list\_view/widget\_impl.rs:436-470, 993-1055
- **Evidence:**
  - `act 'Down in the reorderable list': FAIL focus lands on [list item] 'row 1': no focus change on the bus in this act / FAIL Orca says 'row 1': Orca unheard`
  - `act 'Down again': no focus change on the bus in this act`
  - `act 'Alt+Down': +149.4 ms object:state-changed:focused 1 [list item] 'row 2' / +362.3 ms ORCA SAYS: 'row 2.'`
  - `examples/widget_catalog/src/tabs/touch.rs:117-124 (ListView::new(...).reorderable(true), no .selection(...)); compare the Data tab's ListView with a SelectionModel, where Down is spoken`
  - `catalog-c-touch-20260925-133151-3296560 'Down in the reorderable list' and 'Down again': no focus change on the bus / 'Alt+Down': focused 1 [list item] 'row 2'`
- **Reproduced:** 5 of 5 runs (3 current, 2 earlier); deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 touch runs
- **Fix idea:** Drive AccessKit focus (or active descendant) from the keyboard cursor, not from the selection, so a list without a selection model still exposes its current row

### catalog-c-27 {#catalog-c-27}

ListView keyboard reorder: 'Moved to N of M' is emitted in the same update as the focus move, so Orca cuts it even without K2

- **Example:** widget-catalog
- **Scenario:** catalog-c-touch
- **Act:** Alt+Down on a row of the reorderable list
- **The reader should get:** 'Moved to 3 of 30' heard in full
- **The reader gets:** The announcement reaches the bus before the focus event of the same update, and Orca's stop for the new focus cuts it: 'Moved to 3 of 30 (CUT)', 'row 2.' In 2 of 3 runs it was not spoken at all. This message goes through ctx.announce (list\_view/widget\_impl.rs:262), and its source is the announcer node NodeId(1) (path .../18446744073709551616), so its defunct drop on the 2nd move is K2 and the K2 fix covers that part. The ordering cut is separate, and remains after the fix
- **Platform:** Linux AT-SPI/Orca
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `b518253f` (announce-focus). Fixed part: ListView keyboard reorder cut; same ListView path, verified via the data-collections and drag-and-drop runs.
- **Where:** crates/teksilo-widgets/src/list\_view/widget\_impl.rs:235-262; crates/teksilo-core/src/announcer.rs
- **Evidence:**
  - `+148.8 ms object:announcement [status bar] 'Moved to 3 of 30' text='Moved to 3 of 30' / +149.4 ms object:state-changed:focused 1 [list item] 'row 2' / observed 'Moved to 3 of 30' reached the bus 0.5 ms before the act's focus change`
  - `run 131516: ORCA SAYS (CUT): 'Moved to 3 of 30' then 'row 2.'`
  - `2nd move: observed 'Moved to 4 of 30' came from a node the bus had already been told was defunct; path /org/a11y/atspi/accessible/0/18446744073709551616 (K2)`
  - `crates/teksilo-widgets/src/list_view/widget_impl.rs:240-262 (fi.set(Some(dest)) then ctx.announce(utterance) in one handler)`
  - `catalog-c-touch-20260925-132932-3164675: 13:29:51.466970 object:announcement 'Moved to 3 of 30' / 13:29:51.483932 focused 1 [list item] 'row 2' / orca 13:29:51.557389 SPEECH OUTPUT 'Moved to 3 of 30' / 13:29:51.687837 NULL SPEECH: stop`
  - `catalog-c-touch-20260925-133151-3296560 1st move: orca 13:32:10.463750 EVENT MANAGER: Ignoring defunct object: [DEAD]`
- **Reproduced:** announced-before-focus: 6 of 6 moves over 3 runs; the first move's text heard whole in 0 of 3
- **Verification:** confirmed. Reproduced: 3 of 3 touch runs, 6 of 6 moves announced before focus; spoken-then-cut 2 of 6, dropped (K2) 4 of 6, whole 0 of 6
- **Fix idea:** Announce in the update after the focus move (defer by one sync), or put the position in the moved row's description so the focus reading carries it

### catalog-c-28 {#catalog-c-28}

Touch tab controls lack useful names: unnamed text field, unnamed list, a slider called 'Value'

- **Example:** widget-catalog
- **Scenario:** catalog-c-touch, catalog-c-touch-tabwalk
- **Act:** Tab walk of the Touch tab
- **The reader should get:** Each control named for what it does
- **The reader gets:** 'list box.' / 'Value horizontal slider 40.' / 'entry Tap, hold, drag a handle.': the TextInput has only a placeholder (no label), the reorderable ListView has no name, and the slider's label is the generic 'Value'
- **Platform:** all platforms
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/tabs/touch.rs:118-124, 135, 244
- **Evidence:**
  - `touch-tabwalk: Tab 13 [list box] '' :: list box. / Tab 14 [slider] 'Value' :: Value horizontal slider 40. / Tab 16 [entry] '' :: entry Tap, hold, drag a handle.`
  - `examples/widget_catalog/src/tabs/touch.rs:117-124, 135, 244 (TextInput::new(field).placeholder(...), no label)`
  - `catalog-c-touch-tabwalk-20260925-133855-3726010: Tab 13 [list box] '' / Tab 14 [slider] 'Value' / Tab 16 [entry] ''`
- **Reproduced:** 4 of 4 runs; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 touch runs + 1 of 1 touch walk; deterministic
- **Fix idea:** Name them from their section headings (label or labelled\_by)

### catalog-c-29 {#catalog-c-29}

The DropTarget offers a reader no way to drop without dragging, and is an unnamed panel

- **Example:** widget-catalog
- **Scenario:** catalog-c-dragdrop, catalog-c-dragdrop-tabwalk
- **Act:** Tab past the two DropZones; inspect the DropTarget
- **The reader should get:** A drop target with a keyboard or AT route to the same action (a Browse button, or a named group with an action); WCAG 2.5.7
- **The reader gets:** Tab goes from the second 'Browse…' straight to the title bar. The DropTarget is \[panel\] '' &gt; \[panel\] '' &gt; \[label\], not focusable, no action, no name, and the drop hint only exists during a drag. The DropZones do pass: each has a Browse… button whose zone Orca speaks as context
- **Platform:** all platforms
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/tabs/dragdrop.rs:69-86; crates/teksilo-widgets/src/drop\_target.rs
- **Evidence:**
  - `act 'Tab on past the zones': +50.8 ms object:state-changed:focused 1 [push button] 'Menu' / +313.9 ms ORCA SAYS: 'landmark Window title bar'`
  - `FAIL the DropTarget is focusable or offers an action: found [label] 'DropTarget — wraps a Panel; drop a file to see the border highlight' / ancestor [panel] '' states=['enabled', 'sensitive', 'showing', 'visible'] actions=[]`
  - `examples/widget_catalog/src/tabs/dragdrop.rs:69-86`
  - `catalog-c-dragdrop-20260925-133733-3656396 'Tab on past the zones': ORCA SAYS 'landmark Window title bar' 'Menu push button.'`
  - `catalog-c-dragdrop-tabwalk-20260925-134154-3829482: Tab 14 'Browse…' -> Tab 15 'Menu'`
- **Reproduced:** 2 of 2 runs plus the tab walk; deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 dragdrop runs + 1 of 1 dragdrop walk
- **Fix idea:** Example: add a 'Choose file…' button beside the target. Framework: let DropTarget take an accessible name and document the need for a non-drag alternative

### catalog-c-30 {#catalog-c-30}

Each DropZone's empty live status label emits an empty announcement at launch

- **Example:** widget-catalog
- **Scenario:** catalog-c-dragdrop
- **Act:** Launch on the Drag & Drop tab
- **The reader should get:** No announcement until the status line has text
- **The reader gets:** Two object:announcement events with empty text. The status TextWidget is a live label whose name (from its empty value) is Some(""), and atspi\_common announces any named live node on add. Orca ignores them, but they are noise, and a wasted announcement on other readers
- **Platform:** Linux AT-SPI (adapter.rs:71-77)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/drop\_zone.rs:385-392
- **Evidence:**
  - `13:10:41.920283 object:announcement [label] '' text='' path /org/a11y/atspi/accessible/0/79228404074377982820122361856`
  - `13:10:41.932532 object:announcement [label] '' text='' path /org/a11y/atspi/accessible/0/79228403742336589493350432768`
  - `crates/teksilo-widgets/src/drop_zone.rs:385-392 (TextWidget::new(lit!(String::new())).access_live(Live::Polite))`
  - `catalog-c-dragdrop-20260925-133326-3402266: FAIL no empty announcement reached the bus; launch observed announced-before-focus x2`
- **Reproduced:** 3 of 3 launches on the tab (tree, dragdrop, dragdrop-tabwalk)
- **Verification:** confirmed. Reproduced: 2 of 2 dragdrop runs + 1 of 1 walk
- **Fix idea:** Hide the status label from AT while its text is empty, or publish no value/name when empty

### catalog-c-31 {#catalog-c-31}

Animations tab: three toggles are all named 'Visible'

- **Example:** widget-catalog
- **Scenario:** catalog-c-animations, catalog-c-animations-tabwalk
- **Act:** Tab walk of the Animations tab
- **The reader should get:** 'Fade visible', 'Slide visible', 'Scale visible' (or the section title as group label)
- **The reader gets:** 'Visible toggle button pressed.' three times, at Tab 13, 16 and 18, and nothing tells them apart. Orca does not speak the section headings as context
- **Platform:** all platforms
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/tabs/animations.rs:26-28, 38, 106, 133
- **Evidence:**
  - `FAIL each toggle is named for what it drives: toggle buttons: ['teksu! DSL', 'Visible', 'Expanded', 'Visible', 'Visible'] / 'Visible' names 3 toggles`
  - `animations-tabwalk: Tab 13 [toggle button] 'Visible' / Tab 16 [toggle button] 'Visible' / Tab 18 [toggle button] 'Visible'`
  - `examples/widget_catalog/src/tabs/animations.rs:38, 106, 133`
  - `catalog-c-animations-tabwalk-20260925-134103-3810547: Tab 13, 16, 18 [toggle button] 'Visible' :: 'Visible toggle button pressed.'`
- **Reproduced:** 3 of 3 runs; deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 animations runs + 1 of 1 walk
- **Fix idea:** Label each toggle with its demo ('Fade: visible') or group each demo under its heading

### catalog-c-32 {#catalog-c-32}

Content a demo hides stays in the tree: collapsed (Collapse), faded out (Fade), slid out (Slide)

- **Example:** widget-catalog
- **Scenario:** catalog-c-animations
- **Act:** Space on Expanded (collapse), on Visible (fade out), on Visible (slide out)
- **The reader should get:** Hidden content leaves the tree a reader walks
- **The reader gets:** 'Collapsing content', 'fading' and 'snackbar' are still in the tree after the toggle. Fade and Slide document that they are AT-transparent and that callers must pair them with visible\_when, which the example does not (example layer). Collapse's docs say its content 'is announced by its own subtree when expanded', yet it is still exposed when collapsed (framework)
- **Platform:** all platforms
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/animations/collapse.rs:197-203; crates/teksilo-widgets/src/animations/slide.rs:227-229; (example for Fade: examples/widget\_catalog/src/tabs/animations.rs:38-39)
- **Evidence:**
  - `act 'Space on Expanded: collapse': ORCA SAYS 'not pressed' / FAIL the tree holds no [label] 'Collapsing content': found [label] 'Collapsing content'`
  - `act 'Space: fade the cell out': FAIL the tree holds no [*] 'fading': found [label] 'fading'`
  - `act 'Space: slide the cell out': FAIL the tree holds no [*] 'snackbar': found [label] 'snackbar'`
  - `crates/teksilo-widgets/src/animations/collapse.rs:197-203; crates/teksilo-widgets/src/animations/fade.rs:164-171`
  - `catalog-c-animations-20260925-133629-3610390 tree-Space--slide-the-cell-out.txt: [toggle button] 'Expanded' {focusable} (collapsed) > [panel] '' > [label] 'Collapsing content'; [label] 'fading'; [label] 'snackbar'`
- **Reproduced:** 2 of 2 current runs plus 1 earlier; deterministic
- **Verification:** corrected by the verifier. Reproduced: 2 of 2 animations runs Reproduced: 'fading', 'Collapsing content' and 'snackbar' all stay in the tree after hiding. Layer correction: only Fade documents that callers must pair it with visible\_when (fade.rs:164-171). Slide's accessibility() says only 'The child owns its own a11y' (slide.rs:227-229) and documents no pairing, so Slide is framework, like Collapse (collapse.rs:197-203). Accordion's own collapsed body does leave the tree (its details label is added on expand), so only a bare Collapse leaks.
- **Fix idea:** Collapse: park the child (visible\_when on the expanded signal once the collapse tween ends). Example: pair Fade and Slide with visible\_when

### catalog-c-33 {#catalog-c-33}

Cycle swaps its child in and out of the tree every 1.5 s while nothing happens

- **Example:** widget-catalog
- **Scenario:** catalog-c-animations
- **Act:** Leave the Animations tab idle for 6 s
- **The reader should get:** Decorative motion changes nothing in the accessibility tree
- **The reader gets:** Every 1.5 s a children-changed:add, a children-changed:remove and a defunct. Orca stayed quiet (it passes), but a reader reviewing that area has its object destroyed under it, and every AT client wakes up
- **Platform:** all platforms (tree churn); measured on Linux
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/animations/cycle.rs (Switcher-based)
- **Evidence:**
  - `+418.3 ms object:children-changed:add [panel] '' -> [label] 'Tip 1 — drag the divider'`
  - `+418.6 ms object:children-changed:remove [panel] '' -> [label] 'Tip 3 — F12 opens the inspector'`
  - `+418.7 ms object:state-changed:defunct 1 [label] 'Tip 3 — F12 opens the inspector'`
  - `pass Orca stays quiet`
  - `catalog-c-animations-20260925-133629-3610390 idle act: +484.5 ms add [label] 'Tip 1 — drag the divider' / +484.7 ms remove 'Tip 3 …' / +484.8 ms defunct 'Tip 3 …' / repeats at +1992, +3500, +5006 ms`
- **Reproduced:** 3 of 3 animations runs
- **Verification:** confirmed. Reproduced: 2 of 2 animations runs
- **Fix idea:** Keep all children in the tree and change only which one is visible (hide the others from AT), or expose Cycle as one node whose name changes (non-live)

### catalog-c-34 {#catalog-c-34}

Overlays tab: 'Warning' and 'Error' each name two different buttons (message box and toast rows)

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-tabwalk
- **Act:** Tab walk of the Overlays tab
- **The reader should get:** Distinct names ('Warning message box', 'Warning toast') or a grouped context
- **The reader gets:** 'Warning push button.' at Tab 28 and Tab 34, 'Error push button.' at Tab 29 and 35. The same words open a modal dialog in one place and a toast in the other
- **Platform:** all platforms
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/tabs/overlays.rs:18-58, 340-380
- **Evidence:**
  - `overlays-tabwalk: Tab 28 [push button] 'Warning' :: Warning push button. / Tab 34 [push button] 'Warning' :: Warning push button.`
  - `examples/widget_catalog/src/tabs/overlays.rs:18-58 (toast row) and 340-380 (message box row)`
  - `catalog-c-overlays-tabwalk-20260925-133734-3659642: Tab 28 'Warning' … Tab 34 'Warning'; Tab 29 'Error' … Tab 35 'Error'`
- **Reproduced:** deterministic (tree)
- **Verification:** confirmed. Reproduced: 1 of 1 overlays walk; deterministic (tree)
- **Fix idea:** Name the toast triggers 'Show warning toast' etc., or wrap each row in a named group

### catalog-c-M1 {#catalog-c-m1}

A re-shown tooltip or popover reuses its accessibility node, which AT-SPI already declared defunct: Tab into a sticky tooltip on its second showing is silent

- **Example:** widget-catalog
- **Scenario:** verify-catalog-c-tooltip-reshow, verify-catalog-c-popover, catalog-c-tooltip-tab-away
- **Act:** Focus 'With internal Button' and wait 3.2 s, Tab into the sticky tooltip, then Escape. Focus elsewhere, come back, wait 3.2 s and Tab into it again. Also: open the popover, Escape, then open it again.
- **The reader should get:** The second showing is read as the first: 'Tooltip dialog Treasury report This quarter: +423 coins.'
- **The reader gets:** The first showing is read. On the second, focus moves into the tooltip dialog, which has the same AT-SPI path as the first. Orca logs 'Ignoring defunct object: \[dialog: 'Tooltip'\]' and says nothing, so the reader's focus sits in a silent object. The same happens after a focus bounce (catalog-c-02): the rich 'Level 1' tooltip re-shows on its old path, and Tab into it is silent. A reopened popover also focuses its old, defunct host. The cause is the one behind catalog-c-10 and K2. Overlay content keeps its WidgetId across shows: the tooltip content\_id is registered once at attach, the popover content\_id comes from add\_deferred. When dismissed it goes dormant and leaves the tree, and atspi\_common's remove\_node marks it defunct. The K2 fix, which covers only the announcer, does not reach it. The re-shown composite also briefly returns as \[dialog\], its sticky state kept from the last show, then flips to \[tool tip\] 35 ms later.
- **Platform:** Linux AT-SPI (defunct is atspi\_common's state). The macOS and Windows adapters re-announce and re-expose a re-added node per their source, so they are likely unaffected (not measured).
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: verify-catalog-c-tooltip-reshow had 2 failed checks in the sweep, none now.
- **Where:** crates/teksilo-core/src/build\_context.rs:763-872 (attach\_tooltip\*: one content\_id per anchor); crates/teksilo-core/src/widget\_tree/overlay\_impl.rs:498-545 (show re-activates the same content\_id), 2107 (dormant\_dismissed\_content); crates/teksilo-widgets/src/popover\_widget.rs:670-690
- **Evidence:**
  - `verify-catalog-c-tooltip-reshow (3 runs): first show 13:37:37.342407 children-changed:add -> [tool tip] path …/79228451076681882632059879424; 13:37:43.341874 state-changed:defunct [dialog] same path; second show 13:37:51.390212 add -> [dialog] same path; 13:37:53.895115 focused 1 [dialog] same path; orca 13:37:53.897356 EVENT MANAGER: Ignoring defunct object: [dialog: 'Tooltip']; Orca says only 'With internal Button push button.' 'Tooltip.'`
  - `first show in the same runs: ORCA SAYS 'Tooltip dialog Treasury report This quarter: +423 coins.' (3 of 3)`
  - `catalog-c-tooltip-tab-away (3 runs) 'wait 3 s on Hover or hold — level 1 (tooltip sticky), Tab': focused 1 [tool tip] 'Level 1 …' path …/79228451076681882632059879424 (defunct since 13:31:04.934629); orca 13:31:18.586605 Ignoring defunct object; no speech (3 of 3)`
  - `verify-catalog-c-popover (2 runs) 'Space on Anchor again': +64.2 ms focused 1 [unknown] ''; orca 13:43:49.248871 Ignoring defunct object: [unknown]`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:91-106 remove_node emits StateChanged(Defunct, true)`
- **Reproduced:** tooltip second show silent 3 of 3 verify runs + 3 of 3 tab-away runs; popover reopen 2 of 2
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Give each presentation of overlay content a fresh AccessKit id (or keep the node in the tree, hidden, while dormant, if that does not also leave it defunct). Solve it once in the overlay layer, for tooltips, popovers and the snackbar alike. Reset a tooltip's sticky role before it is re-shown.

### catalog-c-M2 {#catalog-c-m2}

TableView, TreeTableView and GridView replace every visible row and cell node, and the TableView's column headers, on each keyboard move

- **Example:** widget-catalog
- **Scenario:** catalog-c-data-views
- **Act:** Data tab: Down or Tab inside the People table, Down/Right in the TreeTableView, Right in the GridView
- **The reader should get:** A move changes focus between existing cell nodes; the headers and the other rows stay put
- **The reader gets:** A single Down in the TableView makes 90 nodes defunct: 36 cells, 36 labels, 13 rows, 3 column headers and the Filter button. Every Tab makes about 83 nodes defunct, and five Tabs put 445 events on the bus. A GridView Right makes 33 nodes defunct. The focused cell is a new node each time, and Orca logs 'Ignoring defunct object: \[table cell\]' for the old one. Orca's speech itself was intact in these runs. Any AT state tied to an object (review position, NVDA's review cursor) is lost on every keystroke, and about 90 events a keystroke comes close to Orca's 100-event deluge threshold, above which it drops name and description changes. It belongs to the same class as catalog-c-21, but happens on every move rather than only on expand.
- **Platform:** Linux AT-SPI measured; the node replacement happens in teksilo on all platforms
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/table\_view/body.rs, crates/teksilo-widgets/src/tree\_table\_view.rs, crates/teksilo-widgets/src/grid\_view (body pane rebuilt on focus/selection change; exact trigger not isolated)
- **Evidence:**
  - `catalog-c-data-views-20260925-133029-3216175 'Down in the TableView': 90 object:state-changed:defunct (label 36, table cell 36, table row 13, column header 3, push button 1, panel 1)`
  - `same run 'Tab from a cell': 83 defunct; orca 13:31:18.207587 EVENT MANAGER: Ignoring defunct object: [table cell]`
  - `catalog-c-data-views-20260925-133507-3516143 'Tab five more times': 445 events (415 defunct), 5 x 'Ignoring defunct object: [table cell]'`
  - `'Right in the GridView': 33 defunct (table cell 16, label 16, status bar 1)`
- **Reproduced:** 2 of 2 data-views runs; deterministic
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Keep realized rows and cells across focus and selection changes (bind the focus and selection visuals at RepaintOnly or AccessibilityOnly rather than rebuilding the body pane), so a move only flips state on existing nodes

### catalog-c-M3 {#catalog-c-m3}

Tabbing away from a rich-tooltip button with a warm (no-fade) tooltip makes Orca start reading that tooltip's text, markup and all, then cut it

- **Example:** widget-catalog
- **Scenario:** catalog-c-overlays-tabwalk
- **Act:** Tab walk through the rich column: Tab from 'Hover or hold — level 3' to 'Plain among rich' while level 3's tooltip is up
- **The reader should get:** Nothing about the control just left; the new control is read
- **The reader gets:** In the update that moves focus, the old button gains the tooltip's text as its description, and the change reaches the bus before the focus event. Orca still holds the old button as its focus, so it starts reading 'Level 3 — end of the cascade. Press Esc or click outside to dismiss.', then cuts it for the new focus. The hold-back in accessibility\_description\_impl.rs is documented to prevent exactly this: a node focus is on, or was on in the last update, gains nothing in the update focus leaves it. It evidently does not cover the static description written when a tooltip stops showing. With a cold (faded) tooltip the change lands about 50 ms later, in a separate update, and is not spoken.
- **Platform:** Linux AT-SPI/Orca measured. NVDA does not speak description changes (accessibility\_description\_impl.rs header).
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:761-793 (tooltip\_description\_target); crates/teksilo-core/src/widget\_tree/accessibility\_description\_impl.rs (hold-back scope)
- **Evidence:**
  - `catalog-c-overlays-tabwalk-20260925-133734-3659642 Tab 20: +31.8 ms remove [tool tip] 'Level 3 …' / +32.2 ms object:property-change:accessible-description [push button] 'Hover or hold — level 3' text='Level 3 — end of the cascade…' / +32.3 ms focused 1 [push button] 'Plain among rich' / +50.2 ms ORCA SAYS (CUT): 'Level 3 — end of the cascade. Press Esc or click outside to dismiss.'`
  - `the sweep's own walk target/reader-sweep/catalog-c/v2/catalog-c-overlays-tabwalk-20260925-131441-2679904 Tab 20: same sequence, ORCA SAYS (CUT) at +42.4 ms`
  - `cold case, verify-catalog-c-bounce-cold-20260925-133654-3621579: focus at +1063.4 ms, description change at +1113.3 ms (separate update), not spoken`
- **Reproduced:** 2 of 2 overlays tab walks (mine and the sweep's); timing-dependent (needs a warm tooltip); the cold case did not speak it in 3 of 3
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Route the tooltip's not-shown description through the same focus hold-back as the described\_by text, or publish it once at attach and never flip it on dismissal
