<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Tooltips, menu bars and window chrome

Examples: `tooltips-showcase`, `tool-box`, `splitter`, `shortcuts-demo`, `title-bar-demo`, `collapsible-menu-bar`, `native-menu`.
34 findings: 1 critical, 11 high, 13 medium, 9 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [chrome-01](#chrome-01) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Tabbing away from a button whose focus-summoned rich/composite tooltip is showing snaps focus back to that button | high | all | fixed |
| [chrome-02](#chrome-02) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | A registry-keyed rich tooltip's text is not heard on the first visit to its button | high | Linux | open |
| [chrome-03](#chrome-03) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | A rich tooltip's accessible name and the anchor's description are its raw markup | medium | all | open |
| [chrome-04](#chrome-04) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Buttons with a composite tooltip are described as 'Tooltip' | medium | all | open |
| [chrome-05](#chrome-05) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | A promoted rich tooltip exposes neither its links nor its shortcut chip, so the ':key' cascade is pointer-only | medium | all | open |
| [chrome-06](#chrome-06) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | A collapsed Accordion's body stays in the tree and is read (rich tooltip 'More') | medium | all | open |
| [chrome-07](#chrome-07) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Anything that leaves the tree and comes back keeps its node ids and stays defunct to libatspi: a re-shown tooltip, a re-opened ToolBox section, a restored Splitter pane, a reopened menu | high | Linux | fixed |
| [chrome-08](#chrome-08) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Expanded/collapsed is never exposed on Linux: opening a ToolBox section, collapsing a Splitter pane or opening a disclosure is silent | high | Linux | upstream |
| [chrome-09](#chrome-09) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | A disabled push button reads as enabled, sensitive and focusable (ToolBox 'Build tasks', ShortcutSettings 'Reset') | medium | Linux | upstream |
| [chrome-10](#chrome-10) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | A Splitter divider's position is never spoken, on focus or while resizing with the arrows | high | Linux | upstream |
| [chrome-11](#chrome-11) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Splitter collapse/restore animation sends a value change every frame; Orca says 'vertical splitter' about ten times per Enter | medium | Linux | open |
| [chrome-12](#chrome-12) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Every Splitter divider is named 'Splitter divider'; a reader cannot tell which panes it separates | medium | all | open |
| [chrome-13](#chrome-13) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | ShortcutSettings: 42 buttons named 'Rebind', 'Rebind 2nd' or 'Reset' with no shortcut or current chord | high | all | open |
| [chrome-14](#chrome-14) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | The rebind capture hint 'Press any key…' is cut: the whole panel rebuilds and focus moves to a new Rebind node after it | high | Linux | open |
| [chrome-15](#chrome-15) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | The outcome of a rebind is never spoken, including a conflict that silently unbinds another shortcut | high | all | open |
| [chrome-16](#chrome-16) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Collapsed menu bar: Alt+letter reveals the bar and hides it again ~200 ms later; the menu never opens | high | all | open |
| [chrome-17](#chrome-17) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Menu items are never announced in these examples' menus (shared MenuList cause) | critical | Linux | partly fixed |
| [chrome-18](#chrome-18) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | native-menu: the in-window menu's commands do nothing on Linux | high | Linux | open (example) |
| [chrome-19](#chrome-19) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Global shortcuts do nothing while no widget has focus (as at launch) | medium | all | open |
| [chrome-20](#chrome-20) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | native-menu: View ▸ Show Grid is toggled twice per activation, so it never changes | medium | see entry | open (example) |
| [chrome-21](#chrome-21) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | The tooltips-showcase context menu (rich-tooltip menu items) has no keyboard route | medium | all | open (example) |
| [chrome-22](#chrome-22) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Status lines that are the only feedback are not live | low | all | open (example) |
| [chrome-23](#chrome-23) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Orca drops a plain tooltip that resembles the button's name ('Open a file', 'Close the tab') | low | Linux | upstream |
| [chrome-24](#chrome-24) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | At launch the vertical TabWidget's selection makes Orca say 'Food page tab' and move its point of regard to an unfocused tab | low | Linux | upstream |
| [chrome-25](#chrome-25) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Mnemonics, shortcut keys and has-popup never reach AT-SPI | low | Linux | upstream |
| [chrome-26](#chrome-26) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | A second bare Alt tap does not leave the revealed menu bar | low | all | open |
| [chrome-27](#chrome-27) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Custom title bar: no keyboard route to its window controls or window menu | low | all | open |
| [chrome-v01](#chrome-v01) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | The collapsible menu bar goes silent after its first use: every later reveal (F10, Alt tap, hamburger) lands focus on a trigger Orca treats as defunct | high | Linux | fixed |
| [chrome-v02](#chrome-v02) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | native-menu: after choosing an item from a submenu (File &gt; Open Recent &gt; document-1.txt), the next Alt+F does nothing | medium | all | open |
| [chrome-v03](#chrome-v03) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | A composite tooltip holding a TabWidget moves Orca's locus of focus to a tab inside the tooltip ('Stats page tab') while the button keeps focus | medium | Linux | upstream |
| [chrome-v04](#chrome-v04) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Tabbing away from a rich-tip anchor makes Orca start reading the anchor's tooltip text, then cut it for the next control | low | Linux | open |
| [chrome-v05](#chrome-v05) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | StatusBar::announce\_changes(true) cannot announce its content: the live node's name is the constant 'Status' | medium | all | open |
| [chrome-v06](#chrome-v06) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Every menu-bar trigger is its own Tab stop, so Tab walks File, Edit, View, Help one by one | low | all | open |
| [chrome-v07](#chrome-v07) | tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu | Composite tooltip bodies contain unnamed progress bars | low | all | open (example) |

### chrome-01 {#chrome-01}

Tabbing away from a button whose focus-summoned rich/composite tooltip is showing snaps focus back to that button

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: focus 'Hover or hold — level 1' (or 'Province info', or 'Hover or hold — level 3'), wait ~1.2 s so the tip has shown (0.5 s delay) but not yet promoted (2.5 s), then press Tab. Also hit by a plain Tab walk (tooltips-showcase Tab 5 and Tab 10, shortcuts-demo Tab 7 on 'Save (button)').
- **The reader should get:** Focus moves to the next control and stays; the reader hears the next control once.
- **The reader gets:** Focus lands on the next button, then ~120-140 ms later jumps back to the previous button. Orca's reading of the next button is cut and it reads the previous button (and its tip) again; a second Tab is needed to leave. Once the tip has promoted (after 2.5 s) Tab enters the tip instead, which is the documented behaviour.
- **Platform:** All platforms (the logic is in teksilo-core); measured on Linux AT-SPI/Orca 46.1
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `b30770d5` (tooltip-snapback). Fixed part: tooltips-showcase snap-back on level 1 / Province info / level 3.
- **Where:** crates/teksilo-core/src/widget\_tree/overlay\_impl.rs:530-531, :1370-1373; crates/teksilo-core/src/widget\_tree.rs:1661-1667
- **Evidence:**
  - `chrome-tips-snapback-20260925-150332-1280727, case 'Province info': "+4414.8 ms object:state-changed:focused 1 [push button] 'Tabbed details'" … "+4456.5 ms ORCA SAYS (CUT): 'Tabbed details push button.'" … "+4543.5 ms object:state-changed:focused 1 [push button] 'Province info'" … "+4586.7 ms ORCA SAYS: 'Province info push button.'"`
  - `orca-debug.out of the same run: "15:03:53.668838 - SPEECH OUTPUT: 'Tabbed details push button.'", "15:03:53.798962 - NULL SPEECH: stop", "15:03:53.799063 - SPEECH OUTPUT: 'Province info push button.'"`
  - `chrome-walk-tooltips Tab 5: "+7.9 ms object:state-changed:focused 1 [push button] 'Hover or hold — level 2'" then "+128.5 ms object:state-changed:focused 1 [push button] 'Hover or hold — level 1'"; "ORCA SAYS (CUT): 'Hover or hold — level 2 push button.'" then "ORCA SAYS: 'Hover or hold — level 1 push button.'"`
  - `chrome-walk-shortcuts Tab 7: "+10.9 ms object:state-changed:focused 1 [push button] \"Open 'from-button.md'\"" then "+138.0 ms object:state-changed:focused 1 [push button] 'Save (button)'" and "+138.3 ms object:state-changed:defunct 1 [tool tip] 'Save the current document.'"`
  - `Cause: crates/teksilo-core/src/widget_tree/overlay_impl.rs:530-531 gives a focus-armed tip's overlay focus_restore = the anchor (set_top_focus_restore(focused)); :493 gives a cold tip a fade; :1371 tooltip_focus_leave_outside dismisses it with that fade; crates/teksilo-core/src/widget_tree.rs:1660-1666 process_overlay_fade_dismissals_real then calls focus_ops(restore_id) when the fade ends, moving focus back to the anchor the user just left. A warm reshow (session_active) has no fade and does not snap, which is why every second Tab works.`
  - `chrome-tips-snapback-20260925-152607 'Province info': '+4424.7 ms object:state-changed:focused 1 [push button] 'Tabbed details'', '+4560.2 ms object:state-changed:focused 1 [push button] 'Province info''; orca-debug.out '15:26:27.877041 - SPEECH OUTPUT: 'Tabbed details push button.'', '15:26:28.008582 - NULL SPEECH: stop', '15:26:28.008679 - SPEECH OUTPUT: 'Province info push button.''`
  - `verify-chrome-tips-tab-arrival-20260925-153629 try 1: '+11.1 ms focused 1 'Hover or hold — level 1'', '+1400.4 ms focused 1 'Hover or hold — level 2'', '+1525.3 ms focused 1 'Hover or hold — level 1'' (tries 2 and 3: +1397.8/+1522.2, +1398.0/+1527.8)`
- **Reproduced:** 9 of 9 cases in 3 runs of chrome-tips-snapback (v2: 145929, 150332, 150415; rich and composite tips), plus 2 in chrome-walk-tooltips, 1 in chrome-walk-shortcuts, 1 in chrome-tips-interactive (144644)
- **Verification:** confirmed. Reproduced: 15 of 15 cases: 9 in 3 reruns of chrome-tips-snapback (level 1, Province info, level 3, each run), 6 in 2 runs of verify-chrome-tips-tab-arrival (arrival by a real Tab, not AT-SPI focus); also chrome-walk-tooltips Tab 5/Tab 10 and chrome-walk-shortcuts Tab 7
- **Fix idea:** When tooltip\_focus\_leave\_outside dismisses a tip because focus moved elsewhere, drop the overlay's focus\_restore (or dismiss without restore); restore focus only on Escape/click-outside.

### chrome-02 {#chrome-02}

A registry-keyed rich tooltip's text is not heard on the first visit to its button

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Shift+Tab onto 'Hover or hold — level 3' (Button::rich\_tooltip(KEY)) for the first time and wait 4 s.
- **The reader should get:** The reader hears the tip text with the button, as a plain tip's text is heard (the anchor's description).
- **The reader gets:** Only 'Hover or hold — level 3 push button.' The anchor has no description at launch; the tip node appears (tool tip, later dialog) but Orca does not present tooltips by default, and the description is not written while focus stays. The text is heard only after Escape or on a second visit. Inline-content rich tips (shortcuts-demo 'Save (button)', rich\_tooltip\_content) do carry the description at launch.
- **Platform:** Linux AT-SPI/Orca measured; the empty description at launch is platform-independent (the name probe runs before the widget has content)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tooltip/rich.rs:186-197, :559-561; crates/teksilo-widgets/src/tooltip/attach.rs:96; crates/teksilo-core/src/deferred\_subtree.rs:222
- **Evidence:**
  - `chrome-tips-rich-desc launch check: "[push button] 'Hover or hold — level 3' desc=None states=['enabled', 'focusable', 'sensitive', 'showing', 'visible']"`
  - `"+64.4 ms ORCA SAYS: 'Hover or hold — level 3 push button.'"; "+536.9 ms object:children-changed:add [frame] '' -> [tool tip] 'Level 3 — end of the cascade. Press Esc or click outside to dismiss.'"; "+2530.2 ms object:property-change:accessible-role [dialog] 'Level 3 — end of the cascade. …'"; FAIL "Orca says 'end of the cascade'"`
  - `Second visit (same run): "+2312.6 ms ORCA SAYS: 'Level 3 — end of the cascade. Press Esc or click outside to dismiss.'"`
  - `Cause: crates/teksilo-widgets/src/tooltip/rich.rs:186-197 RichTooltipWidget::from_key leaves content None until build; rich.rs:559-561 sets a name only when content is Some; attach.rs:96 builds it deferred; crates/teksilo-core/src/widget_tree/accessibility_impl.rs tooltip_access_description probes that unbuilt widget's name, so the anchor description is empty until the first show. Orca: settings.py:221 presentToolTips = False, scripts/default.py:1649-1653 ignores tool-tip showing unless it is on.`
  - `chrome-tips-rich-desc-20260925-153023: FAIL "[push button] 'Hover or hold — level 3' desc=None"; '+531.2 ms object:children-changed:add [frame] '' -> [tool tip] 'Level 3 — end of the cascade...''; '+2526.2 ms object:property-change:accessible-role [dialog]'; only 'Hover or hold — level 3 push button.' spoken`
- **Reproduced:** 3 of 3 runs of chrome-tips-rich-desc; also every rich button's first stop in chrome-walk-tooltips
- **Verification:** confirmed. Reproduced: 2 of 2 reruns of chrome-tips-rich-desc (desc=None at launch; tip text not heard on the first visit). Every snapback run also shows the anchor's description being written only when focus leaves the anchor.
- **Fix idea:** Resolve the registry key when the widget is constructed (or have the name probe consult the registry) so the static description exists from launch.

### chrome-03 {#chrome-03}

A rich tooltip's accessible name and the anchor's description are its raw markup

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Tab onto a rich-tooltip button (second visit), or Tab into a promoted rich tip; Tab onto the vertical 'Food' tab.
- **The reader should get:** The reader hears the tip as written for a reader: 'Level 1 of the cascade. Hover or hold the next link to open level 2.'
- **The reader gets:** Orca reads the markdown source, brackets, link target and asterisks: '… the \[next link\](:tip-b) to open level 2.' and "\*\*Food\*\* modifies your population's growth rate. Linked to \[trade\](:stat-trade)."
- **Platform:** All platforms (the name string itself carries the markup); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tooltip/rich.rs:560
- **Evidence:**
  - `orca-debug.out (chrome-tips-rich): "14:45:24.755497 - SPEECH OUTPUT: 'Level 1 of the cascade. Hover — or hold, with a finger — the [next link](:tip-b) to open level 2.'"`
  - `chrome-walk-tooltips Tab 14: "object:property-change:accessible-description [page tab] 'Food' text=\"**Food** modifies your population's growth rate. Linked to [trade](:stat-trade).\"" and "ORCA SAYS (CUT): \"**Food** modifies your population's growth rate. Linked to [trade](:stat-trade).\""`
  - `Cause: crates/teksilo-widgets/src/tooltip/rich.rs:560 builder.set_name(content.text.resolve_now()) publishes the unparsed text, while the visible body TextWidget renders it with .markup(true) (rich.rs:382-387).`
  - `chrome-tips-rich-20260925-153126 'Tab again': '+48.6 ms ORCA SAYS: 'Level 1 of the cascade. Hover — or hold, with a finger — the [next link](:tip-b) to open level 2. dialog Open the Accordion to read this long-form body without leaving the tooltip.''`
  - `chrome-walk-tooltips-20260925-154113 Tab 14: "ORCA SAYS (CUT): '**Food** modifies your population's growth rate. Linked to [trade](:stat-trade).'"`
- **Reproduced:** every run that reached a rich tip (chrome-tips-rich, chrome-walk-tooltips, chrome-tips-snapback ×3, chrome-tips-rich-desc ×3)
- **Verification:** confirmed. Reproduced: 2 of 2 reruns of chrome-tips-rich (Tab into the promoted tip) and chrome-walk-tooltips Tab 14 (the 'Food' tab's description)
- **Fix idea:** Publish the markup-stripped plain text (the same parse the body uses) as the tip's name.

### chrome-04 {#chrome-04}

Buttons with a composite tooltip are described as 'Tooltip'

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Tab onto 'Province info', 'Tabbed details' or 'With internal Button'.
- **The reader should get:** The button's description is the tip's content, or nothing; never the generic word.
- **The reader gets:** 'Province info push button. Tooltip.' The tree carries desc='Tooltip' on all three buttons. The composite tip itself is named 'Tooltip', so the dialog a reader Tabs into is 'Tooltip dialog'.
- **Platform:** All platforms (tree content); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tooltip/composite.rs:434-438; crates/teksilo-widgets/src/button.rs:784
- **Evidence:**
  - `tree-launch.txt: "[push button] 'Province info' desc='Tooltip' {focusable}"`
  - `orca-debug.out (chrome-tips-composite): "14:46:20.217386 - SPEECH OUTPUT: 'Province info push button.'", "14:46:20.217400 - SPEECH OUTPUT: 'Tooltip.'"`
  - `"+75.9 ms ORCA SAYS: 'Tooltip dialog Iberia Province overview.'"`
  - `Cause: crates/teksilo-widgets/src/tooltip/composite.rs:434-438 falls back to tr a11y_tooltip_name ('Tooltip', locales/en-US.ftl:32); the AccessKit pass harvests that name as the anchor's static description (accessibility_impl.rs tooltip_access_description).`
  - `chrome-tips-composite-20260925-153229: '+70.0 ms ORCA SAYS: 'Province info push button.'', '+70.0 ms ORCA SAYS: 'Tooltip.''; 'Tab into': '+69.9 ms ORCA SAYS: 'Tooltip dialog Iberia Province overview.''`
- **Reproduced:** every tooltips-showcase run (deterministic)
- **Verification:** confirmed. Reproduced: every tooltips-showcase launch (tree-launch 'desc='Tooltip'' on all three buttons); spoken in 2 of 2 chrome-tips-composite reruns and in the walk
- **Fix idea:** Do not harvest a generic fallback name as a description; for composites, derive the description from the body's text or leave it empty; the example could give each composite an access label.

### chrome-05 {#chrome-05}

A promoted rich tooltip exposes neither its links nor its shortcut chip, so the ':key' cascade is pointer-only

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: focus 'Hover or hold — level 1', wait for promotion, Tab into the tip.
- **The reader should get:** The link 'next link' (opens level 2) and the 'F1' shortcut chip are nodes a reader can reach and activate.
- **The reader gets:** The dialog holds only its name and the 'More' disclosure; the body text (with its link) and the chip are hidden from AT. No keyboard or AT route opens a cascaded tip.
- **Platform:** All platforms (tree content); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tooltip/rich.rs:382-395
- **Evidence:**
  - `tree-Tab-again--into-the-tip-that-focus-promoted.txt: "[dialog] 'Level 1 of the cascade. Hover — or hold, with a finger — the [next link](:tip-b) to open level 2.' {active,focusable,focused}" whose only child is "[push button] 'More' {focusable} rel=['controller-for']"`
  - `Cause: crates/teksilo-widgets/src/tooltip/rich.rs:376-395: the body TextWidget (which carries .on_link_click) and the shortcut chip are .a11y_hidden().`
  - `chrome-tips-rich-20260925-153126 tree-Tab-again: "[dialog] 'Level 1 of the cascade…' {active,focusable,focused}" > "[push button] 'More' {focusable} rel=['controller-for']"`
- **Reproduced:** chrome-tips-rich and 3 chrome-tips-snapback runs (deterministic tree)
- **Verification:** confirmed. Reproduced: 2 of 2 reruns of chrome-tips-rich (static tree)
- **Fix idea:** Keep the name on the dialog but expose the body as text with link children (or a focusable link per :key) and the chip as the dialog's keyboard shortcut / description.

### chrome-06 {#chrome-06}

A collapsed Accordion's body stays in the tree and is read (rich tooltip 'More')

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Tab into a promoted rich tip whose 'More' disclosure is collapsed.
- **The reader should get:** While collapsed, only the 'More' button (collapsed) is presented; its body is hidden.
- **The reader gets:** Orca reads the collapsed body with the dialog: '… dialog Open the Accordion to read this long-form body without leaving the tooltip.' The button node also contains its own region.
- **Platform:** All platforms (tree shape); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/accordion.rs:533-535, :668-677
- **Evidence:**
  - `orca-debug.out (chrome-tips-rich): "14:45:20.158877 - SPEECH OUTPUT: 'Level 1 of the cascade. Hover — or hold, with a finger — the [next link](:tip-b) to open level 2. dialog Open the Accordion to read this long-form body without leaving the tooltip.'"`
  - `tree: "[push button] 'More' {focusable} rel=['controller-for']" > "[landmark] 'More'" > "[label] 'Open the Accordion to read this long-form body without leaving the tooltip.'"`
  - `Cause: crates/teksilo-widgets/src/accordion.rs:533-535 wraps the vertical body in Collapse, whose accessibility is empty (animations/collapse.rs:197-202) and never hides the content; accordion.rs:668-677 puts Role::Button on the node whose children are header and region.`
  - `chrome-tips-rich-20260925-153126 run.json: "push button 'More' … extents [325, 399, 278, 18]" > "landmark 'More' … [325, 417, 16, 595]" > "label 'Open the Accordion to read this long-form body…'"`
- **Reproduced:** 4 runs (chrome-tips-rich, chrome-tips-snapback 145751/145840/…), deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 reruns of chrome-tips-rich (deterministic)
- **Fix idea:** Hide the region (set\_hidden or visible\_when) while collapsed, as ToolBoxPanel does, and put the Button role on the header node rather than on the container.

### chrome-07 {#chrome-07}

Anything that leaves the tree and comes back keeps its node ids and stays defunct to libatspi: a re-shown tooltip, a re-opened ToolBox section, a restored Splitter pane, a reopened menu

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: show 'Province info''s composite tip, Tab in, Escape, Shift+Tab away, Tab back, wait, Tab in again. tool-box: close Outline, re-open it. splitter: Enter twice on the first divider (collapse, restore). Any menu opened a second time.
- **The reader should get:** The second showing is presented like the first.
- **The reader gets:** Second Tab into the tip: Orca says nothing and logs 'Ignoring defunct object'. The re-opened Outline region and every label in it are defunct; the restored Sidebar pane and its labels are defunct. Same for reopened menus ('Ignoring defunct object: \[menu\]').
- **Platform:** Linux AT-SPI only (accesskit\_atspi\_common + libatspi); UIA and macOS adapters handle removal differently
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: same mechanism, covered by the general translation and the core tests, not re-run through the harness in this session.
- **Where:** crates/teksilo-core/src/accessibility.rs (widget\_id\_to\_node\_id); crates/teksilo-widgets/src/menu\_bar/widget\_impl.rs:216-252 (reveal re-activates the same bar\_id); upstream accesskit\_atspi\_common-0.20.0/src/adapter.rs:91-105, node.rs:676-691
- **Evidence:**
  - `chrome-tips-reshow: "Tab into it (second showing)" "+7.0 ms object:state-changed:focused 1 [dialog] 'Tooltip'", FAIL "Orca says 'Iberia'", Orca "15:00:42.422672 - EVENT MANAGER: Ignoring defunct object: [dialog: 'Tooltip']"`
  - `events.jsonl: same path both times: "15:00:29.951089 object:state-changed:defunct 1 dialog /79228166056039199745777860608", "15:00:37.644761 object:children-changed:add 5 frame … -> dialog /79228166056039199745777860608", "15:00:42.420354 object:state-changed:focused 1 dialog /79228166056039199745777860608"`
  - `chrome-toolbox-20260925-150812: FAIL "the [landmark] 'Outline' subtree is not defunct": "[landmark] 'Outline' states=['defunct', 'enabled', 'sensitive', 'showing', 'visible']", "[label] 'Chapter 1 — Opening' states=['defunct', …]"`
  - `chrome-splitter (3 runs) tree-Enter-again--restore-.txt: "[panel] 'Sidebar' {defunct}" > "[label] 'Project' {defunct}"`
  - `chrome-native-menu: "EVENT MANAGER: Ignoring defunct object: [menu]" (14:55:54.190064)`
  - `Cause: accesskit_atspi_common-0.20.0 adapter.rs:91-105 remove_node emits state-changed:defunct; add_node (49-83) re-registers the same path but nothing clears libatspi's defunct flag; Teksilo maps WidgetId to NodeId 1:1 (widget_id_to_node_id), and dormant subtrees reactivate with the same ids. K2 is this mechanism for the announcer's two nodes only.`
  - `verify-chrome-menu-reopen-20260925-153505 orca-debug.out: '15:35:24.255746 - EVENT MANAGER: Dequeued object:state-changed:focused for [menu] … (1, 0, 0)', '15:35:24.255842 - EVENT MANAGER: Ignoring defunct object: [menu]'; FAIL "Orca says 'menu'" on the second open`
  - `verify-chrome-cmb-rereveal-20260925-153356 'F10 (second reveal)': '+9.4 ms object:state-changed:focused 1 [menu item] 'File'', FAIL "Orca says 'File'", orca '15:34:12.371823 - EVENT MANAGER: Ignoring defunct object: [menu item: 'File']'`
  - `chrome-tips-reshow-20260925-152814: '15:28:44.264608 EVENT MANAGER: Ignoring defunct object: [dialog: 'Tooltip']'`
  - `chrome-toolbox-20260925-152916: FAIL "[landmark] 'Outline' states=['defunct', …]"`
  - `verify-chrome-titlebar-toggle-20260925-154036: "[push button] 'Maximize' states=['defunct', 'enabled', 'sensitive', 'showing', 'visible'] actions=['click']" after Restore; the next AT-SPI click still maximised`
- **Reproduced:** tooltip: 3 of 3 runs (chrome-tips-reshow); splitter: 3 of 3 runs; toolbox: 1 run (deterministic)
- **Verification:** corrected by the verifier. Reproduced: tooltip reshow 3 of 3; ToolBox re-open 2 of 2; Splitter restore 3 of 3; dropdown menu opened a second time 3 of 3 (verify-chrome-menu-reopen); collapsible menu bar revealed again 11 of 11 reveals (3 verify-chrome-cmb-rereveal runs x2, 2 chrome-cmb-return, 2 verify-chrome-menu-intents-cmb, ...); title-bar Maximize back after Restore, defunct 2 of 2 Real and wider than the sweep reported. Correction 1: the sweep's evidence for reopened menus, the native-menu line 14:55:54.190064 'Ignoring defunct object: \[menu\]', is the benign focused-0 event (0,0,0) of a menu that is closing. It is not a reopen. I measured the real case. A MenuBar dropdown opened a second time has the same AT-SPI path as the first time (/79228172604633345912668684288 both times), and Orca drops its focused-1 event (1,0,0), so the second File menu is silent. The MenuOverlayHost is a DeferredSubtree that is reused across openings. Correction 2: the worst case is missing from the sweep. Every reveal of the collapsible menu bar after the first is silent. Focus lands on 'File' (same path /79228162588051313888382156800 each time), and Orca ignores it as defunct. The sweep's own chrome-cmb-return 'tap Alt (reveal again)' act showed this. Mechanism detail: for a subtree hidden in place (Splitter pane, ToolBox section), atspi\_common emits defunct but no children-changed:remove/add on the parent, because notify\_children\_changes (node.rs:676-691) runs only when the parent node itself changes. libatspi's child list therefore keeps the defunct nodes. A defunct node still accepts AT-SPI actions: a click on the defunct Maximize worked. This was measured on Linux only; the UIA and macOS adapters do not have this sticky-defunct path.
- **Fix idea:** Give a node that re-enters the tree after removal a fresh AccessKit id (a generation in the NodeId), or keep dormant subtrees in the tree as hidden rather than removing them; report upstream that a re-added path should clear defunct.

### chrome-08 {#chrome-08}

Expanded/collapsed is never exposed on Linux: opening a ToolBox section, collapsing a Splitter pane or opening a disclosure is silent

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tool-box: Tab to 'Outline', Down to 'Properties', Space. splitter: Enter on the first divider (collapse), Enter again (restore).
- **The reader should get:** 'Outline, button, expanded'; 'Properties, collapsed'; after Space, 'expanded'.
- **The reader gets:** 'Outline push button.' / 'Properties push button.' with no state; Space: 'Orca said nothing in this act'. Splitter collapse and restore: only 'vertical splitter'. No expandable/expanded state on any header, divider, menubar trigger or hamburger.
- **Platform:** Linux AT-SPI (measured) and macOS (accesskit\_macos-0.27.0 has no expanded mapping, source read); Windows exposes it (accesskit\_windows node.rs:718-723 ExpandCollapse pattern)
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** upstream accesskit\_atspi\_common-0.20.0/src/node.rs:301
- **Evidence:**
  - `chrome-toolbox 'Space to open Properties': "+49.9 ms object:children-changed:add [panel] '' -> [landmark] 'Properties'", FAIL "Orca says one of ('expanded',)" "Orca said nothing in this act"`
  - `tree-tool-box: "push button 'Outline' ['enabled', 'focusable', 'sensitive', 'showing', 'visible']" (no expandable/expanded)`
  - `chrome-splitter 'Enter (collapse the sidebar)': 10 utterances, all "ORCA SAYS: 'vertical splitter'"`
  - `Teksilo sets it: tool_box.rs:1022 builder.set_expanded(is_active); splitter/handle.rs:835-837; menu_bar/trigger.rs:215`
  - `accesskit_atspi_common-0.20.0/src/node.rs:301-384 state() maps no Expandable/Expanded`
  - `chrome-toolbox-20260925-152916 'Space to open Properties': '+47.6 ms object:children-changed:add [panel] '' -> [landmark] 'Properties'', FAIL "Orca says one of ('expanded',)" 'Orca said nothing in this act'`
- **Reproduced:** tool-box: 2 runs + the re-open act (3 openings, all silent); splitter: 3 runs
- **Verification:** confirmed. Reproduced: ToolBox 2 of 2 runs (Tab/Down/Space all without state); Splitter 3 of 3 (collapse/restore say only 'vertical splitter')
- **Fix idea:** Upstream: map expanded to AT-SPI EXPANDABLE/EXPANDED and emit state-changed:expanded. Until then Teksilo could announce the state change for disclosure widgets on Linux.

### chrome-09 {#chrome-09}

A disabled push button reads as enabled, sensitive and focusable (ToolBox 'Build tasks', ShortcutSettings 'Reset')

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tool-box launch tree: the disabled 'Build tasks' header.
- **The reader should get:** The button is exposed as unavailable (not enabled/sensitive) and not focusable.
- **The reader gets:** states=\['enabled', 'focusable', 'sensitive', 'showing', 'visible'\], no actions. A reader browsing the palette hears an ordinary button. Tab correctly skips it.
- **Platform:** Linux AT-SPI (measured); Windows (accesskit\_windows node.rs:535 is\_enabled = !is\_disabled) and macOS (node.rs:659-660) expose disabled correctly
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** upstream accesskit\_atspi\_common-0.20.0/src/node.rs:376; crates/teksilo-widgets/src/tool\_box.rs:1033; crates/teksilo-widgets/src/menu\_item/widget\_impl.rs:984
- **Evidence:**
  - `chrome-toolbox FAIL: "[push button] 'Build tasks' desc=None states=['enabled', 'focusable', 'sensitive', 'showing', 'visible'] attributes={} relations={} value=None"`
  - `accesskit_atspi_common-0.20.0/src/node.rs:376: 'if state.is_read_only_supported() && state.is_read_only_or_disabled() { ReadOnly } else { Enabled | Sensitive }'; accesskit_consumer-0.39.0/src/node.rs:861-879 is_read_only_supported excludes Role::Button`
  - `Framework part: tool_box.rs:1033 adds Action::Focus even to a disabled header, which the adapter turns into 'focusable' (consumer node.rs:111-113).`
  - `chrome-toolbox-20260925-152916: "[push button] 'Build tasks' desc=None states=['enabled', 'focusable', 'sensitive', 'showing', 'visible']"`
  - `chrome-native-menu-20260925-152842 run.json: "menu item 'Save' ['enabled', 'sensitive', 'showing', 'visible'] ['click']" (File > Save is .enabled(can_save=false))`
- **Reproduced:** 2 runs (static)
- **Verification:** confirmed. Reproduced: 2 of 2 ToolBox runs (static); the same in native-menu's disabled File &gt; Save in 2 of 2 chrome-native-menu runs
- **Fix idea:** Upstream: insert Enabled\|Sensitive only when !is\_disabled for every role. Teksilo: advertise Action::Focus only on enabled headers.

### chrome-10 {#chrome-10}

A Splitter divider's position is never spoken, on focus or while resizing with the arrows

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** splitter: Tab to the first divider, Right, Right.
- **The reader should get:** 'Splitter divider, 28 %', then '30 %', '33 %'.
- **The reader gets:** 'vertical splitter Splitter divider.' on focus; each Right: 'vertical splitter'. The value changes on the bus (property-change:accessible-value) but is never read.
- **Platform:** Linux AT-SPI/Orca (measured). The value is exposed; Windows (UIA Separator + RangeValue) and macOS (NSAccessibilitySplitterRole) not measured
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/splitter/handle.rs:797-813; upstream /usr/lib/python3/dist-packages/orca/formatting.py:467
- **Evidence:**
  - `"+96.4 ms ORCA SAYS: 'vertical splitter Splitter divider.'" FAIL "Orca says one of ('28', '27')"`
  - `"+20.3 ms object:property-change:accessible-value [separator] 'Splitter divider'" "+33.7 ms ORCA SAYS: 'vertical splitter'" FAIL "Orca says one of ('30', '31')"`
  - `orca-debug.out: "15:13:02.614812 - SPEECH OUTPUT: 'vertical splitter Splitter divider.'", "15:13:06.574108 - SPEECH OUTPUT: 'vertical splitter'"`
  - `accesskit_atspi_common-0.20.0/src/node.rs:249 Role::Splitter => AtspiRole::Separator; orca/formatting.py:467-470 SEPARATOR 'focused': 'roleName + availability' (no value), 'unfocused': '… (labelOrName or displayedText or value)' so the name wins; orca/generator.py:1128-1129`
  - `chrome-splitter-20260925-152609 orca-debug.out: '15:26:17.303292 - SPEECH OUTPUT: 'vertical splitter Splitter divider.'', '15:26:21.259993 - SPEECH OUTPUT: 'vertical splitter'' (after '+26.3 ms object:property-change:accessible-value [separator] 'Splitter divider'')`
- **Reproduced:** 3 of 3 runs of chrome-splitter
- **Verification:** confirmed. Reproduced: 3 of 3 reruns of chrome-splitter
- **Fix idea:** Teksilo: on keyboard resize, announce the new position politely (e.g. 'Sidebar 30 %'), or carry the position in the description. Upstream: discuss mapping Splitter to a role whose value Orca reads.

### chrome-11 {#chrome-11}

Splitter collapse/restore animation sends a value change every frame; Orca says 'vertical splitter' about ten times per Enter

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** splitter: Enter on the focused first divider (collapse), Enter again (restore).
- **The reader should get:** One state change, spoken once.
- **The reader gets:** 20 accessible-value events in pairs over ~250 ms; 8-10 utterances of 'vertical splitter'. Other dividers also emit per-frame values during 'Add / Remove Inspector' and 'Collapse Sidebar'.
- **Platform:** Linux AT-SPI/Orca measured; the per-frame tree updates are platform-independent
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/splitter/handle.rs:800-813
- **Evidence:**
  - `run.json (chrome-splitter-20260925-151254): "Enter (collapse the sidebar) value events: 20 utterances: 10 ['vertical splitter', 'vertical splitter', 'vertical splitter']"`
  - `report.txt: "+16.8 ms object:property-change:accessible-value [separator] 'Splitter divider'" "+17.0 ms …" "+29.8 ms ORCA SAYS: 'vertical splitter'" "+35.3 ms …" … "+256.5 ms ORCA SAYS: 'vertical splitter'"`
  - `Cause: crates/teksilo-widgets/src/splitter/handle.rs:800-813 computes value from the live layout sizes on every walk, so each frame of the collapse tween changes the AT value.`
  - `chrome-splitter-20260925-152609 run.json 'Enter (collapse the sidebar)': value events at 15:26:29.161470 (…470623866880), 15:26:29.161871 (…323204763648), 15:26:29.184764, 15:26:29.185845 … through 15:26:29.370153`
- **Reproduced:** 3 of 3 runs of chrome-splitter (count varies 8-10)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 reruns: collapse 26/22/22 value events and 13/11/11 'vertical splitter' utterances; restore 24/20/24 events and 12/10/10 utterances Confirmed, with one correction to the detail. The value events come in pairs because both dividers change each frame, not because one node emits twice: paths …470623866880 (the focused divider) and …323204763648 (the Editor\|Inspector divider, whose share changes as the Editor grows). Orca voices only the focused one, so a single Enter produces about 11 utterances. The cause is correct: handle.rs:800-813 computes the value from the live, animated layout sizes on every walk.
- **Fix idea:** Publish the model's target size (or freeze the AT value while a collapse tween runs) instead of the animated layout size.

### chrome-12 {#chrome-12}

Every Splitter divider is named 'Splitter divider'; a reader cannot tell which panes it separates

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** splitter: Tab from the first divider to the second.
- **The reader should get:** 'Divider between Sidebar and Editor' vs 'between Editor and Inspector' (the panes are labelled with pane\_label).
- **The reader gets:** Both: 'vertical splitter Splitter divider.' The controller-for relation to the two panes exists but no AT-SPI reader speaks it.
- **Platform:** All platforms (name); measured Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/splitter/handle.rs:798
- **Evidence:**
  - `chrome-walk-splitter Tab 6 and Tab 7: "ORCA SAYS: 'vertical splitter Splitter divider.'" for both`
  - `run.json: both separators "Splitter divider None {'controller-for': [...]}"`
  - `Cause: crates/teksilo-widgets/src/splitter/handle.rs:798 set_name(tr a11y_splitter_divider_name) for every handle`
  - `chrome-splitter-20260925-152609 'Tab to the second divider': '+49.0 ms ORCA SAYS: 'vertical splitter Splitter divider.''`
- **Reproduced:** 4 runs (walk + 3 chrome-splitter), static
- **Verification:** confirmed. Reproduced: 3 of 3 reruns (static)
- **Fix idea:** Name (or describe) each handle from its neighbouring panes' labels when they are set.

### chrome-13 {#chrome-13}

ShortcutSettings: 42 buttons named 'Rebind', 'Rebind 2nd' or 'Reset' with no shortcut or current chord

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** shortcuts-demo: Tab into the Shortcut settings panel and on.
- **The reader should get:** 'Rebind Cycle Bounds Overlay, currently Ctrl+B, button' (name or description ties each button to its row).
- **The reader gets:** 'Rebind push button.', 'Rebind 2nd push button.', 'Rebind push button.' … for all 14 shortcuts. On entering the panel Orca adds its unrelated-labels guess 'Cycle Bounds Overlay Go to line… Scroll by page'.
- **Platform:** All platforms (names); measured Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/shortcut\_settings.rs:424, :469-476
- **Evidence:**
  - `chrome-walk-shortcuts Tab 10: "ORCA SAYS: 'Shortcut settings panel.'" "ORCA SAYS: 'Cycle Bounds Overlay Go to line… Scroll by page'" "ORCA SAYS: 'Rebind push button.'"; Tab 11-14: "'Rebind 2nd push button.'", "'Rebind push button.'" …`
  - `orca-debug.out: "GENERATION TIME: 0.2278 ----> unrelatedLabels=[Cycle Bounds Overlay Go to line… Scroll by page]"`
  - `Cause: crates/teksilo-widgets/src/shortcut_settings.rs:473-484 Button::new(lit!(slot_label)) and :424 Button::new(lit!("Reset")); rows are flat HStacks with no labelled-by/description linking the row's name and chord labels.`
  - `chrome-shortcuts-rebind-20260925-153330: FAIL "[push button] 'Rebind' desc=None"; Orca 'Shortcut settings panel.', 'Cycle Bounds Overlay Go to line… Scroll by page', 'Rebind push button.', then 'Rebind 2nd push button.'`
- **Reproduced:** 3 runs (walk, 2 × chrome-shortcuts-rebind), static
- **Verification:** confirmed. Reproduced: 2 of 2 chrome-shortcuts-rebind reruns, plus every capture, conflict and walk run (static)
- **Fix idea:** Label each button with the row: access\_label 'Rebind &lt;name&gt;' and a description with the current chord, or a named Role::Row group per shortcut.

### chrome-14 {#chrome-14}

The rebind capture hint 'Press any key…' is cut: the whole panel rebuilds and focus moves to a new Rebind node after it

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** shortcuts-demo: Tab to the first Rebind, press Space.
- **The reader should get:** 'Press any key. Delete to clear. Escape to cancel.' heard whole, focus unchanged.
- **The reader gets:** The live status node's announcement reaches the bus 50-125 ms before a focus change to a newly created 'Rebind' node (the panel is rebuilt: 388 children-changed events); Orca cuts the hint to read 'Rebind push button.' The old focused node is defunct ('Ignoring defunct object: \[push button: 'Rebind'\]'). This message goes through a live Role::Status node added with a name, not ctx.announce, so the K2 fix does not cover it.
- **Platform:** Linux AT-SPI/Orca measured; the rebuild and focus move are platform-independent
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/shortcut\_settings.rs:208-209, :478-486
- **Evidence:**
  - `"+64.1 ms object:announcement [status bar] 'Press any key. Delete to clear. Escape to cancel.'" "+114.6 ms object:state-changed:focused 1 [push button] 'Rebind'" "+209.1 ms ORCA SAYS (CUT): 'Press any key. Delete to clear. Escape to cancel.'" "+248.6 ms ORCA SAYS: 'Rebind push button.'" (chrome-shortcuts-capture-20260925-150702)`
  - `observed: "'Press any key. Delete to clear. Escape to cancel.' reached the bus 123.2 ms before the act's focus change"; "EVENT MANAGER: Ignoring defunct object: [push button: 'Rebind']"`
  - `Cause: shortcut_settings.rs:486 capturing.set(...) and :208-209 bind capturing at BindingLevel::Rebuild, rebuilding every row; LiveStatusText (:339-376, Role::Status + Live::Polite + name) is announced on add by accesskit_atspi_common adapter.rs:72-77, then focus is re-placed on the rebuilt button.`
  - `chrome-shortcuts-capture-20260925-153044: '15:30:57.015102 object:announcement status bar 'Press any key. Delete to clear. Escape to cancel.'', '15:30:57.095603 object:state-changed:focused push button 'Rebind'', ORCA '15:30:57.226071 (CUT) 'Press any key…'', '15:30:57.270820 'Rebind push button.''`
- **Reproduced:** 4 of 4 runs (3 chrome-shortcuts-capture, 1 chrome-shortcuts-conflict)
- **Verification:** confirmed. Reproduced: 5 of 5 (3 chrome-shortcuts-capture reruns, 2 chrome-shortcuts-conflict reruns); announcement 70-80 ms ahead of the focus move each time
- **Fix idea:** Do not rebuild the panel on capture (update only the row's slot), keep focus on the same Rebind node, and announce the hint after the focus settles (or through the announcer).

### chrome-15 {#chrome-15}

The outcome of a rebind is never spoken, including a conflict that silently unbinds another shortcut

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** shortcuts-demo: Space on Rebind, then press Ctrl+K; or press Ctrl+I, which belongs to Italic.
- **The reader should get:** 'Cycle Bounds Overlay: Ctrl+K'; for the conflict, 'Ctrl+I removed from Italic'.
- **The reader gets:** Only 'Rebind push button.' (the panel rebuilds again and refocuses). Italic loses Ctrl+I without a word.
- **Platform:** All platforms (nothing is published); measured Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/shortcut\_settings.rs (handle\_capture\_event)
- **Evidence:**
  - `chrome-shortcuts-capture (3 runs): "+120.2 ms object:state-changed:focused 1 [push button] 'Rebind'" "+243.3 ms ORCA SAYS: 'Rebind push button.'" FAIL "Orca says 'Ctrl+K'"`
  - `chrome-shortcuts-conflict: "+164.8 ms object:state-changed:focused 1 [push button] 'Rebind'" "+381.4 ms ORCA SAYS: 'Rebind push button.'" FAIL "Orca says one of ('Italic', 'conflict', 'assigned')"`
  - `Cause: shortcut_settings.rs handle_capture_event applies the rebind (and with confirm_conflicts off, unbinds the other shortcut) and nothing announces it; the chord lives in a plain label in the rebuilt row.`
  - `chrome-shortcuts-conflict-20260925-153228 tree-press-Ctrl-I: 'Cycle Bounds Overlay' / 'Ctrl+I' … 'Italic' / '—'; Orca '15:32:45.639504 'Rebind push button.'' only`
- **Reproduced:** 4 of 4 runs
- **Verification:** confirmed. Reproduced: 5 of 5 (3 capture reruns: only 'Rebind push button.' after Ctrl+K; 2 conflict reruns)
- **Fix idea:** Announce the new binding and any auto-resolved conflict; expose the chord on the button (see chrome-13).

### chrome-16 {#chrome-16}

Collapsed menu bar: Alt+letter reveals the bar and hides it again ~200 ms later; the menu never opens

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** collapsible-menu-bar at launch: press Alt+V (real keys).
- **The reader should get:** The bar is revealed with View's menu open; the reader hears the first item.
- **The reader gets:** Focus goes to 'View', then ~225-240 ms later to the frame; the whole bar is removed (defunct). Orca: 'View.' (cut), then 'frame.' F10, a bare Alt tap and the hamburger do reveal the bar correctly.
- **Platform:** All non-macOS platforms (the Alt+letter branch); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-app/src/app.rs:1247-1278; crates/teksilo-widgets/src/menu\_bar/widget\_impl.rs:232-248
- **Evidence:**
  - `"+25.3 ms object:state-changed:focused 1 [menu item] 'View'" "+100.4 ms ORCA SAYS (CUT): 'View.'" "+225.3 ms object:state-changed:focused 1 [frame] ''" "+225.6 ms object:state-changed:defunct 1 [menu bar] ''" "+280.6 ms ORCA SAYS: 'frame.'" (chrome-cmb-alt-letter-20260925-150527)`
  - `orca-debug.out: "15:05:35.339064 - SPEECH OUTPUT: 'View.'" "15:05:35.519213 - NULL SPEECH: stop" "15:05:35.519277 - SPEECH OUTPUT: 'frame.'" "15:05:35.520283 - EVENT MANAGER: Ignoring defunct object: [menu item: 'View']"`
  - `Likely cause: crates/teksilo-app/src/app.rs:1247-1278 OpenMenu runs the reveal, one layout, focuses the trigger and synthesises pointer_down/up at the trigger centre; the reveal overlay is shown rolled up (crates/teksilo-widgets/src/menu_bar/widget_impl.rs:238-246, reveal_progress 0 → 1) with DismissBehavior::EscapeOrClickOutside (:232), so the synthetic click lands outside it and dismisses it.`
  - `chrome-cmb-alt-letter-20260925-152611: '+31.5 ms object:state-changed:focused 1 [menu item] 'View'', '+100.1 ms ORCA SAYS (CUT): 'View.'', '+238.4 ms object:children-changed:remove [frame] '' -> [menu bar] ''', '+238.5 ms object:state-changed:focused 1 [frame] ''', '+282.0 ms ORCA SAYS: 'frame.''`
  - `verify-chrome-cmb-altv-revealed-20260925-153439: 'Alt+V on the revealed bar' '+30.0 ms children-changed:add [frame] -> [menu]', '+81.2 ms ORCA SAYS: 'menu.''`
- **Reproduced:** 4 of 4 runs
- **Verification:** confirmed. Reproduced: 3 of 3 reruns of chrome-cmb-alt-letter; cause check 4 of 4 (verify-chrome-cmb-altv-revealed)
- **Fix idea:** For OpenMenu on a collapsed bar, open the trigger's menu directly (MenuContext::open\_at) instead of synthesising a pointer click, or show the reveal fully unrolled first.

### chrome-17 {#chrome-17}

Menu items are never announced in these examples' menus (shared MenuList cause)

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** shortcuts-demo F10, Down, Down; native-menu Alt+F / F10, Down, Down; collapsible-menu-bar F10, Right, Down.
- **The reader should get:** Each arrow reads the highlighted item ('Save, Ctrl+S').
- **The reader gets:** Opening focuses an unnamed \[menu\]: Orca 'menu.'; arrows produce no event and no speech. In the collapsible bar Right on 'File' opens Edit's menu without ever saying 'Edit', and Escape returns focus to 'File'. Same cause as reported for menus-and-dropdowns (arrows move a private highlight index, no focus move, no active descendant); listed because every menu in these examples inherits it.
- **Platform:** Linux measured; the tree lacks focus/active-descendant on all platforms
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `9636094c` (menus). Fixed part: arrow and name part.
- **Where:** crates/teksilo-widgets/src/menu\_list.rs:752-794
- **Evidence:**
  - `chrome-shortcuts-menu: "+23.3 ms object:state-changed:focused 1 [menu] ''" "+120.2 ms ORCA SAYS: 'menu.'"; 'Down to Save': FAIL "Orca says 'Save'" (no events)`
  - `chrome-native-menu: "+16.9 ms object:state-changed:focused 1 [menu] ''" "ORCA SAYS: 'menu.'"; 'Down, Down': FAIL "Orca says 'Save'"`
  - `chrome-cmb-f10 'Right to Edit': "+48.0 ms object:state-changed:focused 1 [menu] ''" "+246.8 ms ORCA SAYS: 'menu.'"; Escape: "+6.8 ms object:state-changed:focused 1 [menu item] 'File'"`
  - `Cause: crates/teksilo-widgets/src/menu_list.rs on_key handler moves focused_index only (no request_focus, no active descendant); the Role::Menu node has no name`
  - `chrome-shortcuts-menu-20260925-152732: '+24.4 ms object:state-changed:focused 1 [menu] ''', '+56.3 ms ORCA SAYS: 'menu.''; 'Down to Save': no event, "Orca unheard: 'Save'"`
  - `chrome-cmb-f10-20260925-152950 'Right to Edit': '+13.5 ms object:state-changed:focused 1 [menu] ''', '+45.2 ms ORCA SAYS: 'menu.''; 'Escape': '+7.1 ms object:state-changed:focused 1 [menu item] 'File''`
- **Reproduced:** 3 examples, 5 runs, deterministic
- **Verification:** confirmed. Reproduced: shortcuts-menu 2 of 2, native-menu 2 of 2, cmb-f10 2 of 2, plus the Down act in every verify menu run
- **Fix idea:** Move real focus to the highlighted MenuItem (or publish active\_descendant) and name the menu after its trigger.

### chrome-18 {#chrome-18}

native-menu: the in-window menu's commands do nothing on Linux

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** native-menu: Alt+F then an AT-SPI click on 'New'; Alt+F then N (mnemonic); Alt+E, Down, Return (Cut).
- **The reader should get:** The status line reads 'New chosen' / 'Cut chosen'.
- **The reader gets:** The menu closes and the status line never changes (no text-changed event). The same actions run from Ctrl+O when a body button has focus ('Open chosen'). In shortcuts-demo and collapsible-menu-bar the equivalent MenuItem paths do run their actions ('\[action\] Save', 'New' printed).
- **Platform:** Linux (measured); affects every user, not only readers
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/native\_menu/src/main.rs:83; examples/shortcuts\_demo/src/main.rs:190; examples/collapsible\_menu\_bar/src/main.rs:122; trap in crates/teksilo-core/src/widget\_tree.rs:3538-3546
- **Evidence:**
  - `chrome-native-activate-20260925-151058: "+1226.9 ms == harness:action click [menu item] 'New'" "+1230.5 ms object:state-changed:focused 1 [menu item] 'File'" FAIL "the tree holds [label] 'New chosen'"; events.jsonl has no text-changed after 15:11:16.771098`
  - `chrome-native-focused-menu: FAIL "the tree holds [label] 'New chosen'" and FAIL "the tree holds [label] 'Cut chosen'"`
  - `chrome-native-activate: "+837.8 ms object:text-changed:insert [label] 'Choose a menu item…' text='Open chosen'" (Ctrl+O from focused 'Add recent file' works)`
  - `chrome-shortcuts-menu-activate: "pass  the example printed '[action] Save'" (menu click); chrome-cmb-menu-activate: "pass  the example printed 'New'"`
  - `Cause not traced. Structural difference from the working examples: MenuBar::from_model builds dropdowns with build_menu_list, which wraps every item in MenuList::item_when (crates/teksilo-widgets/src/menu/model.rs:797-805), while shortcuts-demo/collapsible use MenuList::item; the item's on_activate sends Intent::new(name) (model.rs:150-156).`
  - `verify-chrome-menu-intents-sc-20260925-153834: FAIL "the example printed '[action] Save' during this act (not later)" 'printed during the act: '''; 'focus Go to line 7, Ctrl+S' pass; note "arena parent chain of the MenuItem holding 'Save' … MenuOverlayHost -> DeferredSubtree; the top is 'DeferredSubtree'"`
  - `verify-chrome-menu-intents-cmb-20260925-153859: menu 'New' FAIL; 'Undo' (println closure) pass; slider + Ctrl+N pass`
  - `verify-chrome-native-diag-20260925-153530: '+2266.6 ms object:text-changed:insert [label] 'Choose a menu item…' text='Opened document-1.txt'' (closure entry works)`
  - `examples/native_menu/src/main.rs:83, :87, :90; examples/shortcuts_demo/src/main.rs:190-229; examples/collapsible_menu_bar/src/main.rs:122-124 use ctx.register_action`
- **Reproduced:** 3 acts in 3 runs (chrome-native-mnemonic, chrome-native-activate ×2, chrome-native-focused-menu)
- **Verification:** corrected by the verifier. Reproduced: native-menu 6 of 6 intent acts (2 chrome-native-activate, 1 focused-menu, 1 mnemonic, 2 native-diag Show Grid/New); shortcuts-demo menu Save 2 of 2 (per-act check); collapsible-menu-bar menu New 2 of 2 (per-act check); closure-only entries run 5 of 5 Real, but not specific to native-menu, and the cause is now traced. Every menu entry that sends an intent is dead in all three examples. The sweep's contrast ('\[action\] Save' / 'New' printed from the menu) was a false pass. Its `printed` check reads app.log from the mark to the end of the run, and each of those sweep runs has exactly one such line in app.log (chrome-shortcuts-menu-activate-…151529: 1 '\[action\] Save'; chrome-cmb-menu-activate-…151558: 1 'New'), which came from the later shortcut act. With a per-act window, the menu click prints nothing in either example, while Ctrl+S / Ctrl+N from a focused control do. Activation itself works: closure entries run ('Opened document-1.txt', 'Undo'). The intent walk (widget\_tree.rs:3538-3546, arena.parent only) never reaches Root. Through the bridge I read the MenuItem's arena chain: 'MenuItem -&gt; ZStack -&gt; KeyboardHighlightWrapper -&gt; VStack -&gt; Padding -&gt; PopoverSurface -&gt; MenuList -&gt; MenuOverlayHost -&gt; DeferredSubtree', a parentless top. All three examples register their commands with ctx.register\_action, and docs/shortcut-intent-action.md:615-620 names exactly this as 'the most common footgun' and prescribes register\_action\_global. That makes it example misuse. The framework trap remains: dispatch\_intent could continue from an overlay's anchor. It affects every user, not only readers.
- **Fix idea:** Trace the intent's source→root chain from a model-built item (item\_when wrapper, overlay parent) and add a regression test that a MenuModel entry's intent reaches an ancestor's register\_action.

### chrome-19 {#chrome-19}

Global shortcuts do nothing while no widget has focus (as at launch)

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** shortcuts-demo right after launch: Ctrl+S. native-menu right after launch: Ctrl+O.
- **The reader should get:** A global shortcut runs its action wherever focus is.
- **The reader gets:** Nothing: no '\[action\] Save' in app.log, no 'Open chosen'. The same chords work once a control inside the root widget has focus.
- **Platform:** All platforms (teksilo-core dispatch); measured Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-core/src/widget\_tree/pointer\_router.rs:741-747
- **Evidence:**
  - `chrome-shortcuts-nofocus: FAIL "the example printed '[action] Save'" "app.log after the act started: ''"; chrome-shortcuts-menu-activate: "pass  the example printed '[action] Save'" after focusing 'Go to line 7'`
  - `chrome-native-activate: 'Ctrl+O (the Open shortcut)' FAIL "the tree holds [label] 'Open chosen'"; after grab_focus 'Add recent file': pass`
  - `Cause: crates/teksilo-core/src/widget_tree/pointer_router.rs:741-747 anchors a Global shortcut with no focus at self.arena.roots().first(), 'an arbitrary root'; the arena has other parentless roots (deferred tooltip hosts, overlay content), so the intent walk need not reach the window root's actions.`
  - `verify-chrome-shortcuts-nofocus-diag-20260925-154018 note: 'the arena has 7 parentless roots … #0 DeferredSubtree (dormant), #1 DeferredSubtree (dormant), #2 DeferredSubtree (dormant), #3 DeferredSubtree (dormant), #4 DeferredSubtree (dormant), #5 InspectorShell, #6 Panel (dormant)'`
  - `verify-chrome-native-diag-20260925-154308: FAIL "the tree holds [label] 'New chosen'" after Ctrl+N; note '#0 DeferredSubtree (dormant) … #3 InspectorShell'`
- **Reproduced:** 1 run in each of 2 examples (not timing-dependent)
- **Verification:** confirmed. Reproduced: shortcuts-demo 8 of 8 (2 chrome-shortcuts-nofocus reruns, 3 diag runs x 2 acts; app.log has 0 lines for the single-act runs and only the focused act's line otherwise); native-menu 5 of 5 (2 chrome-native-activate, 3 native-diag Ctrl+N)
- **Fix idea:** Anchor at the window's content root (the root the window was built from), not the first arena root; or focus a sensible initial control at launch.

### chrome-20 {#chrome-20}

native-menu: View ▸ Show Grid is toggled twice per activation, so it never changes

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** native-menu: Alt+V, then an AT-SPI click on 'Show Grid'.
- **The reader should get:** The check clears and the body label reads 'Grid: hidden'.
- **The reader gets:** 'Show Grid' stays checked and the label stays 'Grid: visible (toggle via View ▸ Show Grid)'.
- **Platform:** In-window bar (Linux measured)
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/native\_menu/src/main.rs:132-136
- **Evidence:**
  - `chrome-native-activate: FAIL "the tree holds [label] 'Grid: hidden'"; chrome-native-check tree: "[check menu item] 'Show Grid' {checkable,checked}" and "[label] 'Grid: visible (toggle via View ▸ Show Grid)'"`
  - `examples/native_menu/src/main.rs:127-133 uses .checkable(grid_visible) plus .on_activate(move |_| g.set(!g.get())); MenuEntry::checkable is two-way (crates/teksilo-widgets/src/menu/model.rs:107-111) and MenuItem flips the signal itself (menu_item/widget_impl.rs:445-448), so the example's own flip undoes it (whether or not chrome-18 also blocks activation)`
  - `verify-chrome-native-diag-20260925-153530 'Alt+V, click Show Grid': '+1232.3 ms object:state-changed:defunct 1 [check menu item] 'Show Grid'', FAIL "the tree holds [label] 'Grid: hidden'"`
- **Reproduced:** 2 runs; the double flip is from source
- **Verification:** confirmed. Reproduced: 3 of 3 (2 chrome-native-activate, 1 native-diag), now shown to be the double flip rather than chrome-18
- **Fix idea:** Use .checkable(grid\_visible) alone, or .checked(grid\_visible) (reflect-only) with the on\_activate.

### chrome-21 {#chrome-21}

The tooltips-showcase context menu (rich-tooltip menu items) has no keyboard route

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Tab from 'With internal Button'.
- **The reader should get:** The 'Right-click here for a menu' panel is a Tab stop that opens its menu with Shift+F10 / the Menu key.
- **The reader gets:** The panel and everything in it are unfocusable; Tab goes straight to the 'Food' tab. The menu-item tooltips it demonstrates are pointer-only.
- **Platform:** All platforms
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/tooltips\_showcase/src/main.rs:251
- **Evidence:**
  - `chrome-tips-menu: FAIL "[panel] '' states=['enabled', 'sensitive', 'showing', 'visible']; focusable nodes inside: 0"`
  - `"+499.2 ms object:state-changed:focused 1 [page tab] 'Food'"`
  - `examples/tooltips_showcase/src/main.rs: Panel::new()…context_menu(..) with no .focusable(true)`
  - `chrome-tips-menu-20260925-153513: FAIL "[panel] '' states=['enabled', 'sensitive', 'showing', 'visible']; focusable nodes inside: 0"; '+499.2 ms object:state-changed:focused 1 [page tab] 'Food''`
- **Reproduced:** 2 runs, static
- **Verification:** confirmed. Reproduced: 2 of 2 reruns of chrome-tips-menu (static)
- **Fix idea:** Make the panel focusable (and name it) so the keyboard context-menu path reaches its factory.

### chrome-22 {#chrome-22}

Status lines that are the only feedback are not live

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** splitter: activate 'Export layout'. native-menu: Ctrl+O from a focused button. collapsible-menu-bar: Home on the width slider (the second bar folds).
- **The reader should get:** 'Exported v1 · …', 'Open chosen', 'State: collapsed …' spoken.
- **The reader gets:** Only text-changed events on a plain label; Orca says nothing (splitter), nothing (native-menu), or only the slider's '60'.
- **Platform:** All platforms
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/splitter/src/main.rs:177; examples/native\_menu/src/main.rs (StatusBar); examples/collapsible\_menu\_bar/src/main.rs (demo\_state label)
- **Evidence:**
  - `chrome-splitter-buttons: "+33.6 ms object:text-changed:insert [label] … text='Exported  v1 · #0=220px  #1=0px  #2=280px'" FAIL "Orca says 'Exported'"`
  - `chrome-cmb-slider: "+21.8 ms object:text-changed:insert [label] 'State: expanded …' text='collapsed → click the ☰ to reveal the bar trailing it'" "+66.1 ms ORCA SAYS: '60'"`
  - `native-menu StatusBar without announce_changes(true) (crates/teksilo-widgets/src/status_bar.rs:95, :283-285 default false)`
  - `chrome-splitter-buttons-20260925-152816: '+14.3 ms object:text-changed:insert [label] … text='Exported  v1 · #0=220px  #1=0px  #2=280px'', FAIL "Orca says 'Exported'"`
  - `chrome-cmb-slider-20260925-153849: '+18.5 ms object:text-changed:insert [label] … 'collapsed → click the ☰…'', '+35.7 ms ORCA SAYS: '60''`
- **Reproduced:** 1 run each, not timing-dependent
- **Verification:** corrected by the verifier. Reproduced: splitter Export 2 of 2; collapsible slider 2 of 2 The behaviour is confirmed, but the fix idea for native-menu is wrong. StatusBar::announce\_changes(true) alone would still announce nothing on AT-SPI. The live node is the StatusBar, whose name is the constant 'Status' (status\_bar.rs:277-285). atspi\_common announces a live node only when it is added or when its name changes (adapter.rs:72-77, node.rs:610-622), and the child label's text change changes neither. The status line needs .name(status.clone()).announce\_changes(true) or ctx.announce. See also missed chrome-v05.
- **Fix idea:** StatusBar::announce\_changes(true) in native-menu; a live status (or ctx.announce) for the splitter and collapsible demos' state lines.

### chrome-23 {#chrome-23}

Orca drops a plain tooltip that resembles the button's name ('Open a file', 'Close the tab')

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Tab to Open, Close.
- **The reader should get:** 'Open push button. Open a file.'
- **The reader gets:** 'Open push button.' only; the description is in the tree but Orca judges it redundant.
- **Platform:** Linux/Orca only (NVDA/JAWS not measured)
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** n/a (orca/script\_utilities.py stringsAreRedundant)
- **Evidence:**
  - `orca-debug.out: "14:44:52.370828 - SCRIPT UTILITIES: Similarity between 'Open', 'Open a file': 0.53 (threshold: 0.5)"; "Similarity between 'Close', 'Close the tab': 0.56"`
  - `chrome-tips-plain: FAIL "Orca says 'Open a file'" "Orca said: 'Open push button.'"`
  - `orca/script_utilities.py:4173-4183 stringsAreRedundant; orca/generator.py:478-490`
  - `chrome-tips-plain-20260925-153536 'Tab to Open': '+46.6 ms ORCA SAYS: 'Open push button.'', FAIL "Orca says 'Open a file'"`
- **Reproduced:** 2 runs (walk, chrome-tips-plain), deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 reruns of chrome-tips-plain (deterministic)
- **Fix idea:** Nothing needed in Teksilo; tooltip text that repeats the label is itself a content smell (the example could say 'Choose a file to open').

### chrome-24 {#chrome-24}

At launch the vertical TabWidget's selection makes Orca say 'Food page tab' and move its point of regard to an unfocused tab

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase launch.
- **The reader should get:** The window is announced; no tab is read before the window and nothing moves Orca's locus to an unfocused tab.
- **The reader gets:** 'Food page tab.' (cut) then 'frame.'; Orca sets its locus of focus to the 'Food' tab, which does not have focus.
- **Platform:** Linux AT-SPI/Orca
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** upstream accesskit\_atspi\_common-0.20.0/src/adapter.rs:78-80
- **Evidence:**
  - `"+296.3 ms object:selection-changed [page tab list] ''" before "+314.9 ms window:activate [frame] ''"; "ORCA SAYS (CUT): 'Food page tab.'" "ORCA SAYS: 'frame.'"`
  - `orca-debug.out: "14:35:33.109285 - FOCUS MANAGER: Changing locus of focus from None to [page tab: 'Food']. Notify: True" (via default.onSelectionChanged)`
  - `chrome-tips-snapback-20260925-152607 launch: '+257.9 ms object:children-changed:add [frame] '' -> [panel] ''' … '+260.0 ms object:selection-changed [page tab list] ''', '+316.8 ms ORCA SAYS (CUT): 'Food page tab.'', '+372.4 ms ORCA SAYS: 'frame.''`
- **Reproduced:** every tooltips-showcase launch (more than 10 runs)
- **Verification:** corrected by the verifier. Reproduced: every tooltips-showcase launch in my runs (13 launches) Real, but Teksilo does not emit an initial selection change. accesskit\_atspi\_common's add\_node queues SelectionChanged for every node added already selected (adapter.rs:78-80 -&gt; enqueue\_selection\_changed\_if\_needed, :260-267). Teksilo's content arrives in the update after the bare window root, so the selected 'Food' tab counts as added, and the event comes in the same burst as the children-changed:add events. Orca's default.onSelectionChanged then moves its locus to the tab, until 'frame.' 23-55 ms later. The same mechanism does worse inside a tooltip: see missed chrome-v04.
- **Fix idea:** Do not emit an initial selection change for the TabWidget's first selection (emit selection only on user changes), or select before the window becomes active.

### chrome-25 {#chrome-25}

Mnemonics, shortcut keys and has-popup never reach AT-SPI

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** native-menu / collapsible-menu-bar: F10 onto 'File'; Tab onto the hamburger 'Menu'.
- **The reader should get:** 'File, menu, Alt+F' (Orca's MNEMONIC/accelerator slots) and 'Menu, button, has popup, collapsed'.
- **The reader gets:** 'File.' and 'Menu push button.' Every action has key\_binding '' and no has-popup/expanded state exists on AT-SPI, although Teksilo sets access\_key, keyboard shortcut and has\_popup.
- **Platform:** Linux AT-SPI (measured). Windows/macOS not checked for access key
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** upstream accesskit\_atspi\_common-0.20.0/src/node.rs:1097; crates/teksilo-widgets/src/menu\_bar/trigger.rs:224
- **Evidence:**
  - `tree-native-menu run.json: "menu item 'File' ['enabled', 'focusable', 'sensitive', 'showing', 'visible'] [{'name': 'click', 'description': '', 'key_binding': ''}]"; "push button 'Menu' … [{'name': 'click', 'description': '', 'key_binding': ''}] … Menu"`
  - `accesskit_atspi_common-0.20.0/src/node.rs:1097 key_binding: "".into(); no has_popup/access_key mapping anywhere in the crate`
  - `Teksilo sets them: crates/teksilo-widgets/src/menu_bar/trigger.rs:213 set_has_popup, :227-231 set_access_key`
  - `chrome-native-menu-20260925-152842 run.json: "menu item 'New' ['enabled', 'sensitive', 'showing', 'visible'] ['click'] None None" (no accelerator anywhere)`
- **Reproduced:** static, every run
- **Verification:** corrected by the verifier. Reproduced: static, every native-menu / collapsible-menu-bar run The platform claim is wider than stated. No adapter in use maps access\_key or keyboard\_shortcut. atspi\_common 0.20 has key\_binding "" (node.rs:1097), and accesskit\_windows-0.35.0 and accesskit\_macos-0.27.0 contain no reference to either. The trigger.rs:224-231 comment ('announced by Windows Narrator as Access key: F') is therefore untrue for these versions. has\_popup is mapped only on Windows (accesskit\_windows node.rs:485-494, aria properties). The menu items' Ctrl+N / Ctrl+O accelerators (MenuItem set\_keyboard\_shortcut, widget\_impl.rs:986-995) are also lost on all three platforms.
- **Fix idea:** Upstream: fill Action.key\_binding from access\_key/keyboard\_shortcut and map has\_popup to HAS\_POPUP.

### chrome-26 {#chrome-26}

A second bare Alt tap does not leave the revealed menu bar

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** collapsible-menu-bar: focus the slider, tap Alt (bar revealed, focus on File), tap Alt again.
- **The reader should get:** Menu mode ends and focus returns to the slider (Windows/GTK convention).
- **The reader gets:** No focus change; the bar stays revealed with focus on File. Escape does return to the slider.
- **Platform:** All non-macOS
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/menu\_bar.rs:515-525
- **Evidence:**
  - `chrome-cmb-return (2 runs): 'tap Alt again' FAIL "focus lands on [slider] 'Bar width'" with no focus event`
  - `crates/teksilo-widgets/src/menu_bar.rs:515-525 on_alt_tap always returns FocusTrigger(first)`
  - `chrome-cmb-return-20260925-153431 'tap Alt again': FAIL "focus lands on [slider] 'Bar width'" 'no focus change on the bus in this act'`
- **Reproduced:** 2 of 2 runs
- **Verification:** confirmed. Reproduced: 2 of 2 reruns of chrome-cmb-return (plus the sweep's 2)
- **Fix idea:** When a trigger already has focus, make on\_alt\_tap leave menu mode.

### chrome-27 {#chrome-27}

Custom title bar: no keyboard route to its window controls or window menu

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** title-bar-demo: Tab through; Alt+Space.
- **The reader should get:** A keyboard path (as the native frame offers) to minimise/maximise/close and the window menu, or reliance on the desktop's own shortcuts stated.
- **The reader gets:** Tab reaches only the Theme combo; the three buttons are not focusable; Alt+Space does nothing. The buttons are named and a screen reader's click works (Maximize then reads 'Restore').
- **Platform:** All platforms; on Wayland/KWin the compositor's own shortcuts still work
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/title\_bar/drag\_region.rs:187-225
- **Evidence:**
  - `chrome-walk-titlebar: Tab 2-6 produce no focus change after 'Theme combo box.'`
  - `chrome-titlebar 'Alt+Space': FAIL "Orca says something" "Orca said nothing in this act"`
  - `crates/teksilo-widgets/src/title_bar/drag_region.rs:195-230 window menu only from pointer (right-click/long press)`
  - `chrome-titlebar-20260925-153709 'Alt+Space': FAIL "Orca says something" (no events)`
- **Reproduced:** 2 runs, static
- **Verification:** confirmed. Reproduced: 2 of 2 chrome-titlebar reruns + chrome-walk-titlebar (Tab reaches only 'Theme')
- **Fix idea:** Offer Alt+Space (or the host's convention) to open the window menu from the keyboard when the chrome is custom.

### chrome-v01 {#chrome-v01}

The collapsible menu bar goes silent after its first use: every later reveal (F10, Alt tap, hamburger) lands focus on a trigger Orca treats as defunct

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** collapsible-menu-bar: F10 (reveal), Escape (hide), F10 again; or Tab to the hamburger and Space after a first reveal.
- **The reader should get:** Each reveal says 'File' as the first one does.
- **The reader gets:** Only the first reveal is heard. Later reveals move focus to 'File' with the same AT-SPI path, and Orca drops the event ('Ignoring defunct object: \[menu item: 'File'\]'). Right then opens a menu heard only as 'menu.'. The sweep's 'passed' entry for F10/Alt/hamburger reveals is true only for the first reveal.
- **Platform:** Linux AT-SPI/Orca (measured); UIA/macOS do not have the sticky-defunct path
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: verify-chrome-cmb-rereveal second and third reveal heard 1/1.
- **Where:** crates/teksilo-widgets/src/menu\_bar/widget\_impl.rs:216-252
- **Evidence:**
  - `verify-chrome-cmb-rereveal-20260925-153356 'F10 (second reveal)': '+9.4 ms object:state-changed:focused 1 [menu item] 'File'', FAIL "Orca says 'File'", orca '15:34:12.371708 - EVENT MANAGER: Dequeued object:state-changed:focused for [menu item: 'File'] … (1, 0, 0)', '15:34:12.371823 - EVENT MANAGER: Ignoring defunct object: [menu item: 'File']'`
  - `same path each reveal: events.jsonl focused 1 'File' /79228162588051313888382156800 at 15:34:04.586962, 15:34:12.363428, 15:34:20.221890, 15:34:26.109886`
  - `third reveal via hamburger: '+1218.9 ms object:state-changed:focused 1 [menu item] 'File'', FAIL "Orca says 'File'"`
  - `Cause: menu_bar/widget_impl.rs:216-252: the reveal re-activates and re-shows the same bar_id subtree. accesskit_atspi_common adapter.rs:91-105 marked those node ids defunct when the bar hid, and nothing clears that.`
- **Reproduced:** 11 of 11 later reveals: 3 of 3 verify-chrome-cmb-rereveal runs (2 each), 2 of 2 chrome-cmb-return, 2 of 2 verify-chrome-menu-intents-cmb, plus the sweep's own chrome-cmb-return 151139/151216
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Give re-revealed nodes fresh AccessKit ids (see chrome-07), or keep the revealed bar in the tree while hidden instead of removing it.

### chrome-v02 {#chrome-v02}

native-menu: after choosing an item from a submenu (File &gt; Open Recent &gt; document-1.txt), the next Alt+F does nothing

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** native-menu: click 'Add recent file'; Alt+F, click 'Open Recent', click 'document-1.txt' (AT-SPI); then Alt+F.
- **The reader should get:** File opens.
- **The reader gets:** No event at all; a second Alt+F opens File. After a plain item ('Open'), Alt+F works the first time.
- **Platform:** All platforms (menu\_bar logic); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open. In the menus fix topic, not fixed there: Not reproduced here and a separate cause: MenuContext::open\_index goes stale after a submenu choice, and app.rs's OpenMenu synthesises a click that then toggles the menu closed. It belongs to the menu-bar state work with menus-09.
- **Where:** crates/teksilo-widgets/src/menu\_bar/trigger.rs:76-80; crates/teksilo-widgets/src/menu\_bar/widget\_impl.rs:551-553
- **Evidence:**
  - `verify-chrome-native-submenu-reopen-20260925-153937: 'Alt+F after the submenu item': FAIL "focus lands on [menu]" 'no focus change on the bus in this act'; 'Alt+F once more': '+25.0 ms object:state-changed:focused 1 [menu] '''; control 'Alt+F again' after 'Open': pass`
  - `verify-chrome-native-diag (3 runs): after document-1.txt, 'Escape, Escape' and 'Alt+F' produce no events; the act errors 'no node matches … name New'`
  - `Cause not fully traced, but consistent with stale state. The trigger's tap toggles on MenuContext::open_index (menu_bar/trigger.rs:76-80: open_index == Some(index) -> close), and open_index is reset only in MenuOverlayHost's FocusLost handler (menu_bar/widget_impl.rs:551-553). OpenMenu synthesises a click (app.rs:1256-1277), so a stale Some(0) closes instead of opening.`
- **Reproduced:** 5 of 5 (2 verify-chrome-native-submenu-reopen, 3 verify-chrome-native-diag)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Reset open\_index whenever the menu overlay is dismissed (the overlay's on\_dismiss), not only on FocusLost; and let OpenMenu open explicitly instead of toggling via a synthetic click.

### chrome-v03 {#chrome-v03}

A composite tooltip holding a TabWidget moves Orca's locus of focus to a tab inside the tooltip ('Stats page tab') while the button keeps focus

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Tab onto 'Tabbed details' and wait for its composite tip (first showing).
- **The reader should get:** The reader hears 'Tabbed details push button' and nothing moves Orca's locus.
- **The reader gets:** When the tip appears, the TabWidget inside it emits selection-changed. Orca's default.onSelectionChanged moves its locus to the unfocused 'Stats' tab and says 'Stats page tab.'. Orca's where-am-I and review then refer to a tab in a tooltip. On the second showing the tab list is defunct and ignored (chrome-07).
- **Platform:** Linux AT-SPI/Orca (measured)
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** upstream accesskit\_atspi\_common-0.20.0/src/adapter.rs:78-80
- **Evidence:**
  - `verify-chrome-tips-tabbed-20260925-154248 try 1: '+502.1 ms object:state-changed:focused 1 [push button] 'Tabbed details'', '+1224.9 ms object:selection-changed [page tab list] ''', '+1289.8 ms ORCA SAYS: 'Stats page tab.''`
  - `orca-debug.out: '15:42:59.495580 - FOCUS MANAGER: Changing locus of focus from [push button: 'Tabbed details'] to [page tab: 'Stats']. Notify: True', '15:42:59.527061 - SPEECH OUTPUT: 'Stats page tab.''; try 2: '15:43:09.027238 - EVENT MANAGER: Ignoring defunct object: [page tab list]'`
  - `chrome-walk-tooltips-20260925-154113 Tab 11: '+178.0 ms object:selection-changed [page tab list] ''', '+233.1 ms ORCA SAYS: 'Stats page tab.''`
  - `Cause: accesskit_atspi_common-0.20.0 adapter.rs:78-80 queues SelectionChanged for a node added already selected (same mechanism as chrome-24); Orca default.onSelectionChanged sets the locus of focus`
- **Reproduced:** 3 of 3 verify-chrome-tips-tabbed runs (first showing) + chrome-walk-tooltips (mine) + the sweep's chrome-walk-tooltips-144102 ('Changing locus of focus from \[push button: 'Tabbed details'\] to \[page tab: 'Stats'\]')
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Upstream: do not emit selection-changed for nodes that arrive already selected. Teksilo: publish tooltip subtrees in a way that does not carry a selected item into a non-focused overlay, or report upstream.

### chrome-v04 {#chrome-v04}

Tabbing away from a rich-tip anchor makes Orca start reading the anchor's tooltip text, then cut it for the next control

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Tab from 'Hover or hold — level 3' (after its tip has shown) to 'Plain among rich'.
- **The reader should get:** The reader hears the next control only.
- **The reader gets:** The level-3 anchor's description is written in the same update as the focus move, and ahead of it. Orca speaks 'Level 3 — end of the cascade…' and stops it about 37 ms later for 'Plain among rich push button.'. The same happens for level 2 on every snap-back.
- **Platform:** Linux AT-SPI/Orca (measured)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-core/src/widget\_tree/accessibility\_description\_impl.rs
- **Evidence:**
  - `chrome-walk-tooltips-20260925-154113 Tab 8: '+7.6 ms object:property-change:accessible-description [push button] 'Hover or hold — level 3' text='Level 3 — end of the cascade…'', '+7.7 ms object:state-changed:focused 1 [push button] 'Plain among rich'', '+21.8 ms ORCA SAYS (CUT): 'Level 3 — end of the cascade. Press Esc or click outside to dismiss.'', '+59.3 ms ORCA SAYS: 'Plain among rich push button.''`
  - `chrome-tips-snapback (3 runs): "ORCA SAYS (CUT): 'Level 2 of the cascade. Hover — or hold — the [final link](:tip-c) for one more.'" as focus leaves level 2`
  - `Cause: the description is held back while focus stays on the node (accessibility_description_impl.rs) and written in the update where focus leaves; the consumer hands node changes before the focus event (accesskit_consumer tree.rs:640-673)`
- **Reproduced:** walk 1 of 1 (Tab 8); snap-back level-2 fragment 3 of 3; rich-desc 2 of 2
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Do not write a pending description onto a node in the update where focus leaves it; write it in the next update.

### chrome-v05 {#chrome-v05}

StatusBar::announce\_changes(true) cannot announce its content: the live node's name is the constant 'Status'

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** Any app that follows the StatusBar docs: StatusBar::new().child(label bound to a Signal).announce\_changes(true), then change the label.
- **The reader should get:** The new status text ('Saved') is announced, as status\_bar.rs:4-13 and :63-69 promise.
- **The reader gets:** Nothing on AT-SPI (and nothing on UIA/macOS, which key on the same live node). The live node is the StatusBar, named 'Status' (or a fixed .name()). A child label's text change changes neither its presence nor its name, and those are the only triggers (atspi\_common adapter.rs:72-77, node.rs:610-622). The platform facts give the announced string as the name.
- **Platform:** All platforms (source); not measured: no example in scope uses announce\_changes(true)
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/status\_bar.rs:277-285
- **Evidence:**
  - `crates/teksilo-widgets/src/status_bar.rs:277-285 accessibility(): set_role(Status), set_name(self.name or tr a11y_status_bar_name), set_live(Polite) only when announce_changes`
  - `grep: no example or crate calls announce_changes(true) outside status_bar.rs's own test (status_bar.rs:347)`
  - `accesskit_atspi_common-0.20.0 node.rs:610-622: Announcement emitted only when name != old name for a live node`
- **Reproduced:** not measured (source only)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Derive the StatusBar's name from its content while announce\_changes is on (or route changes through ctx.announce), and document .name(signal) as the way to feed it.

### chrome-v06 {#chrome-v06}

Every menu-bar trigger is its own Tab stop, so Tab walks File, Edit, View, Help one by one

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** shortcuts-demo / native-menu / collapsible-menu-bar: Tab through the window.
- **The reader should get:** A menu bar is at most one Tab stop, with arrows between its menus (ARIA menubar; GTK/Qt/Win32 keep it out of the Tab order and reach it with F10/Alt).
- **The reader gets:** shortcuts-demo Tab 2-5: 'File.', 'Edit.', 'View.', 'Help.'; native-menu Tab 1-3; collapsible-menu-bar's inline second bar Tab 3-5.
- **Platform:** All platforms; measured on Linux
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/menu\_bar/trigger.rs
- **Evidence:**
  - `chrome-walk-shortcuts-20260925-154151: Tab 2 [('menu item','File')] 'File.' … Tab 5 [('menu item','Help')] 'Help.'`
  - `chrome-walk-native-20260925-154305: Tab 1-3 File/Edit/View`
  - `chrome-walk-cmb-20260925-154244: Tab 3-5 File/Edit/View`
- **Reproduced:** 3 of 3 walks (static)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Roving tab stop on the menu bar: one trigger focusable at a time.

### chrome-v07 {#chrome-v07}

Composite tooltip bodies contain unnamed progress bars

- **Example:** tooltips-showcase, tool-box, splitter, shortcuts-demo, title-bar-demo, collapsible-menu-bar, native-menu
- **Act:** tooltips-showcase: Tab into the promoted 'Province info' (or 'With internal Button') tip.
- **The reader should get:** Each progress bar has a name ('Development 65 %').
- **The reader gets:** "\[progress bar\] '' value={'current': 0.65…}" inside the dialog.
- **Platform:** All platforms
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/tooltips\_showcase/src/main.rs:193
- **Evidence:**
  - `chrome-tips-composite-20260925-153229 tree-Tab-into-the-composite-tooltip: "[dialog] 'Tooltip'" > "[panel] ''" > "[progress bar] '' value={'current': 0.6499999761581421, …}"`
  - `examples/tooltips_showcase/src/main.rs:193 .child(ProgressBar::new(0.42)) (interactive body), likewise the province body`
- **Reproduced:** 2 of 2 (static)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Give each ProgressBar a label in the example.
