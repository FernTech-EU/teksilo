<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Widget catalog, first eight tabs

Examples: widget-catalog (palette, layout, visuals, containers, chrome, buttons, styling, inputs).
28 findings: 3 critical, 12 high, 8 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [catalog-a-01](#catalog-a-01) | widget-catalog | Controls a reader has already met go silent after their page (or overlay) leaves the tree and comes back | critical | Linux | fixed |
| [catalog-a-02](#catalog-a-02) | widget-catalog | Checkbox, Toggle, RadioButton and Slider changes reach AT only with the next unrelated update | high | all | fixed |
| [catalog-a-03](#catalog-a-03) | widget-catalog | Tab away from the Theme switcher once its tooltip has shown and focus is thrown back to Theme | high | all | fixed |
| [catalog-a-04](#catalog-a-04) | widget-catalog | The Theme switcher's composite tooltip reaches the reader as the single word 'Tooltip' | high | Linux | open |
| [catalog-a-05](#catalog-a-05) | widget-catalog | Disabled buttons read as enabled on Linux | high | Linux | upstream |
| [catalog-a-06](#catalog-a-06) | widget-catalog | Expanded/collapsed state and has-popup never reach AT-SPI (Accordion, ToolBox, SplitButton, ComboBox) | high | Linux | upstream |
| [catalog-a-07](#catalog-a-07) | widget-catalog | A ComboBox's current value is invisible on Linux (Theme switcher, fruit combo) | high | Linux | upstream |
| [catalog-a-08](#catalog-a-08) | widget-catalog | The Inputs page's fruit ComboBox has no name | high | Linux | open (example) |
| [catalog-a-09](#catalog-a-09) | widget-catalog | Opening a ComboBox says nothing, and the first Down skips the item a reader never heard | medium | Linux | fixed |
| [catalog-a-10](#catalog-a-10) | widget-catalog | The selected SegmentedControl segment is announced 'not selected' | high | Linux | fixed |
| [catalog-a-11](#catalog-a-11) | widget-catalog | Opening a PopoverButton puts focus on a nameless, role-less node and the reader hears nothing | high | Linux | fixed |
| [catalog-a-12](#catalog-a-12) | widget-catalog | Content entering the tree with a selected item moves Orca's locus of focus off the real focus | medium | Linux | upstream |
| [catalog-a-13](#catalog-a-13) | widget-catalog | A Banner announces only its title ('Warning'), never its message; the four banners on Chrome are cut on arrival | high | Linux | open |
| [catalog-a-14](#catalog-a-14) | widget-catalog | Collapsed Accordion content stays in the tree a reader walks | medium | Linux | open |
| [catalog-a-15](#catalog-a-15) | widget-catalog | Slider value read as raw float noise: 'Vertical slider vertical slider 0.30000001192092896' | medium | Linux | fixed |
| [catalog-a-16](#catalog-a-16) | widget-catalog | A Splitter divider's position is never spoken, on focus or when moved | medium | Linux | upstream |
| [catalog-a-17](#catalog-a-17) | widget-catalog | Non-linear Stepper: every step is its own Tab stop and a step's status is not exposed | medium | Linux | open |
| [catalog-a-18](#catalog-a-18) | widget-catalog (palette, layout, visuals, containers, chrome, buttons, styling, inputs) | TabWidget publishes an unnamed ScrollView inside every tab list, and the tab lists are unnamed (confirmed) | low | Linux | open |
| [catalog-a-19](#catalog-a-19) | widget-catalog (palette, layout, visuals, containers, chrome, buttons, styling, inputs) | No page has a heading: section titles and GroupHeader are plain labels | low | Linux | open |
| [catalog-a-20](#catalog-a-20) | widget-catalog | Example misuses: radio set not grouped, misleading IconButton names, unnamed breadcrumb, pointer-only TwistArrows | low | Linux | open (example) |
| [catalog-a-21](#catalog-a-21) | widget-catalog | Framework fallback names are generic and repeat the role ('Toolbar tool bar', 'Splitter divider', 'Tooltip', 'Step content') | low | Linux | open |
| [catalog-a-M1](#catalog-a-m1) | widget-catalog | Controls scrolled out of a page and back are silent: Shift+Tab back up the Inputs page reads 7 controls in a row as nothing | critical | Linux | fixed |
| [catalog-a-M2](#catalog-a-m2) | widget-catalog | A ComboBox's list is silent from its second opening on: arrowing through the options says nothing | critical | Linux | fixed |
| [catalog-a-M3](#catalog-a-m3) | widget-catalog | The Chrome page's Wizard opens as 'Dialog dialog', and its step text is never read | high | Linux | open |
| [catalog-a-M4](#catalog-a-m4) | widget-catalog | Pressing Next in a Wizard/Stepper changes the step silently | high | Linux | open |
| [catalog-a-M5](#catalog-a-m5) | widget-catalog | Escape does not close the Wizard dialog | medium | all | open |
| [catalog-a-M6](#catalog-a-m6) | widget-catalog | An Accordion is one push button whose subtree holds its whole content | medium | Linux | open |
| [catalog-a-M7](#catalog-a-m7) | widget-catalog (palette, layout, visuals, containers, chrome, buttons, styling, inputs) | Every page tab reads a developer note ('See: cargo run -p …') as its description | low | Linux | open (example) |

### catalog-a-01 {#catalog-a-01}

Controls a reader has already met go silent after their page (or overlay) leaves the tree and comes back

- **Example:** widget-catalog
- **Scenario:** catalog-a-return-to-page (also every census's Up/Down round trip, catalog-a-tooltip-bounce)
- **Act:** Inputs page: Tab to 'Two-state checkbox' and 'Tristate checkbox' (first visit), Down to Indicators, Up back to Inputs, Tab to the same two check boxes, then to Option A
- **The reader should get:** On the second visit Orca says 'Two-state checkbox check box not checked.' and 'Tristate checkbox check box not checked.' as it did the first time
- **The reader gets:** Nothing. The focus event reaches the bus on the same AT-SPI path the node had before it was sent defunct. Orca logs 'Ignoring defunct object' and says nothing. Option A, which Orca never touched before, is spoken. The same thing silences the embedded tabs 'Overview' and 'Edit' (Containers), the Stepper's 'Account' step (Chrome), the SegmentedControl's 'First' (Inputs), all four Chrome banners' announcements, and the Theme tooltip's sticky dialog the second time it is shown. A reader who comes back to a page no longer hears the controls they visited or that fired an event.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The defunct state is the AT-SPI adapter's (accesskit\_atspi\_common adapter.rs:90-111). UIA and macOS were not measured.
- **Severity:** critical; **layer:** upstream
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: catalog-a-return-to-page had 2 failed checks and 5 defunct drops in the sweep, none now.
- **Where:** Upstream: accesskit\_atspi\_common-0.20.0 adapter.rs:90-111 (remove\_node emits Defunct), adapter.rs:60-80 (add\_node never clears it); accesskit\_unix-0.23.0 atspi/bus.rs:434-439 (AddAccessible body libatspi 2.52 rejects). Teksilo mitigation point: crates/teksilo-core/src/accessibility.rs:1984-1989 (widget\_id\_to\_node\_id, NodeId = WidgetId).
- **Evidence:**
  - `return-to-page 13:19:38 run: 13:19:57.423951 object:state-changed:focused 1 [check box] 'Tristate checkbox' path /org/a11y/atspi/accessible/0/79228228000205799262452187136`
  - `ORCA 13:19:57.732968 SPEECH OUTPUT: 'Tristate checkbox check box not checked.'`
  - `13:20:05.075286 object:state-changed:defunct 1 (same path, act 'Down: open Indicators')`
  - `13:20:19.881995 object:state-changed:focused 1 [check box] 'Tristate checkbox' (same path)`
  - `ORCA 13:20:19.883827 EVENT MANAGER: Ignoring defunct object: [check box: 'Tristate checkbox']`
  - `'Tab to Option A (after returning; never focused before)': ORCA SAYS 'Option A.' 'selected radio button'`
  - `containers census 12:58:49: 12:58:57.168392 - SPEECH OUTPUT: 'Overview page tab.' (selection-changed at launch); Overview defunct 12:59:04.512979; focus 12:59:18.959672; '12:59:18.978626 - EVENT MANAGER: Ignoring defunct object: [page tab: 'Overview']'`
  - `chrome census 12:59:24, 'Down: open Chrome again': observed ''Warning' came from a node the bus had already been told was defunct'; '12:59:45.649034 EVENT MANAGER: Ignoring defunct object: [status bar: 'Warning']'; Tab 8 '13:00:03.810482 EVENT MANAGER: Ignoring defunct object: [page tab: 'Account']'`
  - `inputs census 13:03:59 Tab 14: '13:04:44.294543 EVENT MANAGER: Ignoring defunct object: [radio button: 'First']'`
  - `tooltip-bounce 13:16:02 'Tab: into the sticky tooltip': +31.1 ms object:state-changed:focused 1 [dialog] 'Tooltip'; '13:16:36.042558 EVENT MANAGER: Ignoring defunct object: [dialog: 'Tooltip']' (nothing spoken)`
  - `source: TabWidget parks unselected pages dormant, the walker prunes them, and they return with the same NodeId because NodeIds derive from stable WidgetIds (crates/teksilo-core/src/accessibility.rs widget_id_to_node_id). accesskit_atspi_common-0.20.0 adapter.rs:103-104 emits StateChanged(Defunct,true) on remove and nothing un-defuncts on re-add. Orca event_manager drops the event ('Ignoring defunct object').`
  - `return-to-page 13:36:05 run: 13:36:24.234239 object:state-changed:focused 1 'Tristate checkbox' path /org/a11y/atspi/accessible/0/79228228000205799262452187136; ORCA 13:36:24.313268 SPEECH OUTPUT: 'Tristate checkbox check box not checked.'; 13:36:31.875637 object:state-changed:defunct 1 (same path, 'Down: open Indicators'); 13:36:46.717214 focused 1 (same path); ORCA 13:36:46.718957 EVENT MANAGER: Ignoring defunct object: [check box: 'Tristate checkbox']`
  - `same run, tree-Up--open-Inputs-again.txt:45-46: [check box] 'Two-state checkbox' {checkable,defunct,focusable} / [check box] 'Tristate checkbox' {checkable,defunct,focusable} (a live node, reported defunct by the listener's libatspi cache)`
  - `stream1.log (the listener's libatspi): (process:3588555): dbind-WARNING **: 13:36:08.945: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau (repeated for every added node, in every run)`
  - `containers census 13:49:59: 13:50:24.521174 EVENT MANAGER: Ignoring defunct object: [page tab: 'Overview']; 13:50:26.407306 ... [page tab: 'Edit']`
  - `chrome census 13:48:59: 13:49:15.569402 Ignoring defunct object: [status bar: 'Error'] (+ Info, Success, Warning, [page tab list: 'Steps']); 13:49:32.045382 Ignoring defunct object: [page tab: 'Account']`
  - `tooltip-bounce 13:47:06: 13:47:22.954949 focused 1 [dialog] 'Tooltip'; ORCA 13:47:22.957411 Ignoring defunct object: [dialog: 'Tooltip']`
  - `run dirs: target/reader-verify/catalog-a/catalog-a-return-to-page-20260925-133605-3588263, -134448-3905733, -134508-3912736; catalog-a-containers-20260925-134959-3928947; catalog-a-chrome-20260925-134859-3928947`
- **Reproduced:** return-to-page 3 of 3 runs (the previous attempt had 1 more). Page round trip in the containers census 2 of 2, chrome census 2 of 2, inputs census 2 of 2. Re-shown tooltip dialog 2 of 2 (tooltip-bounce rounds 2 and 3). The effect is deterministic, not timing: whether a node is dropped depends on whether Orca held a proxy for it before.
- **Verification:** corrected by the verifier. Reproduced: return-to-page 3 of 3 (both check boxes silent each run; Option A, never read by Orca, spoken 3 of 3). Containers census 2 of 2 (Overview, Edit silent). Chrome census 2 of 2 (four banners, the 'Steps' tab list and 'Account' ignored). Inputs census 1 of 1 ('First' ignored). tooltip-bounce 3 of 3 (re-shown tooltip or sticky dialog ignored every round). Two larger cases found: scroll-back 2 of 2, combo reopen 2 of 2 (missed M1, M2). Deterministic. The finding is real and the severity is right, but the layer and the scope need correcting. (1) The cause sits upstream. accesskit\_atspi\_common marks a removed node defunct (adapter.rs:103-104) and emits nothing when the same id comes back (add\_node, adapter.rs:60-80). Every libatspi client keeps that DEFUNCT in its state cache. The AddAccessible cache signal carries the node's fresh state set (accesskit\_unix-0.23.0 atspi/bus.rs:434-439, interfaces/cache.rs:28-55), but libatspi 2.52 rejects its unwrapped signature, so nothing replaces the cached DEFUNCT. The listener's own tree shows the revived check boxes as {checkable,defunct,focusable}. So 'Orca held a proxy before' means precisely: Orca's libatspi had cached that node's states. That is why Option A, never queried, is spoken. (2) The scope is wider than pages and overlays. A node that merely scrolls out of a ScrollArea is dropped by the consumer filter (filters.rs:64-86), sent defunct, and is silent when scrolled back (M1). A ComboBox's list rows go defunct at close, so every later opening is silent (M2). No Teksilo lifecycle event marks the scroll case, so the sweep's fix (bump a generation on dormant to active, or on overlay re-show) cannot cover it. That still helps for pages and overlays, as K2's fix does for the announcer. The durable fix is upstream: emit Defunct=false (or a fresh path) on re-add, fix the AddAccessible signature, or do not mark filter-excluded-but-alive nodes defunct. A reader returning to any page, scroll position or popup hears nothing from controls they already met, so critical stands.
- **Fix idea:** Give a subtree that re-enters the AT tree fresh NodeIds, e.g. a per-widget generation folded into the NodeId that is bumped on dormant→active and on overlay re-show, so that the adapter creates new AT-SPI paths. Alternatively fix upstream: accesskit\_atspi\_common could emit state-changed:defunct=false, or use a new object path, when a removed id is added again.

### catalog-a-02 {#catalog-a-02}

Checkbox, Toggle, RadioButton and Slider changes reach AT only with the next unrelated update

- **Example:** widget-catalog
- **Scenario:** catalog-a-toggles, catalog-a-slider-keys
- **Act:** Space on 'Two-state checkbox', 'Tristate checkbox', 'Enable feature' and 'Option B'; Right twice on the 'Volume' slider; Up on 'Vertical slider'
- **The reader should get:** Each key produces its state-changed:checked / :pressed or accessible-value event at once, and Orca says 'checked', 'pressed', '51', '52'
- **The reader gets:** No event at all within the 3 s act. The change is sent only when something else rebuilds the AT tree, here the next grab\_focus. It arrives in the same update as the focus move, so Orca says 'checked' or '52' and cuts it at once for the new focus. Nothing is spoken for Option B or the vertical slider. A reader who presses Space or an arrow key hears nothing.
- **Platform:** All platforms: no TreeUpdate is produced, so no adapter is told (from source). Measured on Linux AT-SPI / Orca 46.1.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Where:** crates/teksilo-widgets/src/checkbox.rs:362-470 (build: no AccessibilityOnly binding), toggle.rs, radio\_button.rs, slider.rs; crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs:74-111
- **Evidence:**
  - `toggles 13:18:27 run: act 'Space on the two-state check box' 13:18:37.142399..13:18:40.329627, no events`
  - `13:18:40.655056 harness:grab-focus [check box] 'Tristate checkbox' / 13:18:40.670169 object:state-changed:checked 1 [check box] 'Two-state checkbox' / 13:18:40.670430 object:state-changed:focused 1 [check box] 'Tristate checkbox'`
  - `ORCA 13:18:40.679101 SPEECH OUTPUT: 'checked' then ORCA 13:18:40.724004 NULL SPEECH: stop, 13:18:40.724103 SPEECH OUTPUT: 'Tristate checkbox check box not checked.'`
  - `13:18:52.126621 object:state-changed:pressed 1 [toggle button] 'Enable feature' (Space was at 13:18:48.59, the event came with the 13:18:52.114739 grab-focus)`
  - `slider-keys 13:11:32 run: Right presses in acts starting 13:11:45.173122 and 13:11:49.560877, no events. 13:11:56.218950 object:property-change:accessible-value [slider] 'Volume' in the same update as 13:11:56.219237 object:state-changed:focused 1 [slider] 'Vertical slider'`
  - `orca-debug.out: '13:11:56.236388 - SPEECH OUTPUT: '52'' then the new focus is read`
  - `source: crates/teksilo-core/src/widget_tree/accessibility_impl.rs:20-30 and 74-111 (the cached TreeUpdate is returned unless a11y_dirty; 'A plain relayout does not invalidate the cache'); binding.rs:249-271 (only BindingLevel::AccessibilityOnly sets it); checkbox.rs, toggle.rs, radio_button.rs, slider.rs register no AccessibilityOnly binding, while accordion.rs:390-394, radio_tile_group.rs:410 and button.rs:1085 do`
  - `toggles 13:37:03: act 'Space on the two-state check box' 13:37:13.424818..13:37:16.612564 has no events; 13:37:16.954889 object:state-changed:checked 1 'Two-state checkbox' in the same update as 13:37:16.955092 focused 1 'Tristate checkbox'; ORCA 13:37:16.962798 SPEECH OUTPUT: 'checked', 13:37:17.013449 NULL SPEECH: stop`
  - `verify-catalog-a-at-click 13:43:34: 'AT-SPI click on the two-state check box' 13:43:45.507902..13:43:48.555382 no event; 'wait 5 s' and 'Shift' acts no event; 13:43:58.278243 object:state-changed:checked 1 arrives with the Tab (13:43:58.278580 focused 1 'Tristate checkbox'); ORCA 13:43:58.296296 'checked' then 13:43:58.367259 NULL SPEECH: stop`
  - `same run: AT-SPI click on 'Enable feature' 13:44:03.48..13:44:06.53 nothing; 13:44:07.765650 object:state-changed:pressed 1 with the next Tab; ORCA 13:44:07.781024 'pressed' cut`
  - `slider-keys 13:37:44: Right x2 13:37:56.22..13:38:03.80 no event; 13:38:07.547408 object:property-change:accessible-value 'Volume' + 13:38:07.547601 focused 'Vertical slider'; ORCA 13:38:07.558430 '52' then 13:38:07.613648 stop`
- **Reproduced:** toggles 3 of 3 runs (4 of 4 controls each run); slider-keys 3 of 3 runs
- **Verification:** confirmed. Reproduced: toggles 3 of 3 (4 of 4 controls each run); slider-keys 3 of 3; verify-catalog-a-at-click 2 of 2 (AT-SPI click, 5 s wait and a Shift key do not deliver the change; the next focus move does)
- **Fix idea:** Bind each control's state/value signal at BindingLevel::AccessibilityOnly (the Accordion/RadioTileGroup pattern). Better, make any binding that feeds accessibility() dirty the AT cache, so a new control cannot forget it.

### catalog-a-03 {#catalog-a-03}

Tab away from the Theme switcher once its tooltip has shown and focus is thrown back to Theme

- **Example:** widget-catalog
- **Scenario:** catalog-a-tooltip-bounce, and every census Tab walk that wraps past the title bar
- **Act:** Focus 'Theme' (title bar), wait for its composite tooltip to show (0.7 s), press Tab
- **The reader should get:** Focus moves to the Palette (current) page tab and stays; Orca says 'Palette page tab.'
- **The reader gets:** Focus moves to the page tab. About 120 ms later the tooltip's fade ends, the tooltip is removed and focus is moved back to 'Theme'. Orca's 'Palette page tab.' is cut, followed by 'landmark Window title bar. Theme combo box. Tooltip.'. Only a second Tab gets away. The same happens when Tabbing out of the tooltip once it has turned into a sticky dialog. Leaving before the tooltip shows does not bounce.
- **Platform:** All platforms (a framework focus move). Measured on Linux AT-SPI / Orca 46.1.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `b30770d5` (tooltip-snapback). Fixed part: Theme switcher bounce, non-sticky and sticky.
- **Where:** crates/teksilo-core/src/widget\_tree/overlay\_impl.rs:530-532; crates/teksilo-core/src/widget\_tree.rs:1655-1669
- **Evidence:**
  - `palette census 12:55:32 Tab 10: 12:56:11.657771 object:state-changed:focused 1 page tab 'Palette' / 12:56:11.774090 object:children-changed:remove frame '' -> tool tip 'Tooltip' / 12:56:11.774418 object:state-changed:focused 1 combo box 'Theme'`
  - `ORCA 12:56:11.756727 SPEECH OUTPUT: 'Palette page tab.'; ORCA 12:56:11.853282 NULL SPEECH: stop; ORCA 12:56:11.853529 SPEECH OUTPUT: 'landmark Window title bar'; 12:56:11.853567 'Theme combo box.'; 12:56:11.853622 'Tooltip.'`
  - `tooltip-bounce 13:16:02 'Tab away from Theme after its tooltip has shown (1)': +26.9 ms focused 1 [page tab] 'Palette'; +152.0 ms children-changed:remove [frame] '' -> [tool tip] 'Tooltip'; +152.5 ms focused 1 [combo box] 'Theme'; FAIL focus is still on [page tab] 'Palette' when the act ends`
  - `same run 'Tab: out of the sticky tooltip': +21.4 ms focused 1 [page tab] 'Palette'; +143.8 ms children-changed:remove -> [dialog] 'Tooltip'; +144.1 ms focused 1 [combo box] 'Theme'`
  - `control act 'Shift+Tab to Theme and Tab away before its tooltip shows': pass focus is still on [page tab] 'Palette'`
  - ``source: crates/teksilo-core/src/widget_tree/overlay_impl.rs:530-532 (`if by_focus && let Some(focused) = self.focused { self.overlay_manager.set_top_focus_restore(focused); }`); crates/teksilo-core/src/widget_tree.rs:1655-1669 process_overlay_fade_dismissals_real restores focus_restore whenever it is active, whatever has been focused since``
  - `tooltip-bounce 13:47:06 'Tab away from Theme after its tooltip has shown (1)': 13:47:20.461376 focused 1 [page tab] 'Palette'; 13:47:20.569287 children-changed:remove [frame] -> Tooltip; 13:47:20.569613 focused 1 [combo box] 'Theme'; ORCA 13:47:20.527435 'Palette page tab.' / 13:47:20.618238 NULL SPEECH: stop / 13:47:20.618307 'landmark Window title bar' 'Theme combo box.' 'Tooltip.'; 13:47:20.716590 children-changed:add [frame] -> Tooltip`
  - `same run, next Tab ~2.3 s later: 13:47:22.954949 focused 1 [dialog] 'Tooltip'; ORCA 13:47:22.957411 Ignoring defunct object: [dialog: 'Tooltip']; only 13:47:23.743171 focused 1 [page tab] 'Palette' after another Tab`
  - `census walks: palette Tab 10, layout Tab 11, visuals Tab 10, containers Tab 21, chrome Tab 19, buttons Tab 30, styling Tab 28, inputs Tab 26 each hold focused [page tab] then focused [combo box] 'Theme' in the same act`
- **Reproduced:** 21 of 21 acts in which focus left Theme after its tooltip had shown: 16 census walks (8 tabs × 2 runs), tooltip-bounce round 1 2/2, round 3 2/2 non-sticky + 1/1 sticky. Two more in round 2's scene steps. Control: Tab away before the tooltip shows stayed put 3 of 3.
- **Verification:** confirmed. Reproduced: tooltip-bounce 9 of 9 acts over 3 runs; census Tab walks 8 of 8 (every tab); verify-catalog-a-bounce-then-tab 2 of 2; control (leave before the tooltip shows) 3 of 3 no bounce
- **Fix idea:** On a fade (or any) dismissal, restore focus only when focus is still inside the dismissed overlay's content, or nowhere. A focus-shown tooltip that never took focus should record no focus\_restore.

### catalog-a-04 {#catalog-a-04}

The Theme switcher's composite tooltip reaches the reader as the single word 'Tooltip'

- **Example:** widget-catalog
- **Scenario:** catalog-a-theme-tooltip, every census walk
- **Act:** Tab onto 'Theme' in the title bar; stay on it 3 s
- **The reader should get:** Orca says 'Theme combo box' and the tooltip's text ('About switching theme at runtime. Colours retint instantly …') as its description
- **The reader gets:** The combo box's description is 'Tooltip', so Orca says 'Theme combo box. Tooltip.' on every arrival. When the tooltip turns sticky, only object:property-change:accessible-role is sent and nothing is spoken. The text is heard only by a reader who stays on Theme at least 2.7 s and then presses Tab into the sticky dialog, which is itself named just 'Tooltip' (and on later showings Orca ignores it as defunct, see catalog-a-01).
- **Platform:** Linux measured. The description property is what UIA (FullDescription) and macOS (AXHelp) read too, so all three get 'Tooltip' (from source).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tooltip/composite.rs:431-438; crates/teksilo-core/src/widget\_tree/accessibility\_description\_impl.rs:368-409
- **Evidence:**
  - `theme-tooltip 13:16:27 'Tab onto Theme': +20.5 ms object:state-changed:focused 1 [combo box] 'Theme'; +60.7 ms ORCA SAYS: 'Theme combo box.' / 'Tooltip.'`
  - `FAIL [combo box] 'Theme''s description holds 'About switching theme at runtime', [combo box] 'Theme' description='Tooltip'`
  - `'rest on Theme 3 s': +773.9 ms object:property-change:accessible-role [dialog] 'Tooltip'; FAIL Orca says 'About switching theme at runtime' (unheard)`
  - `tree after dwell: [dialog] 'Tooltip' {focusable} > [label] 'About switching theme at runtime' …`
  - `tooltip-bounce 13:12:29 round 2: Tab after a ~3 s dwell: +19.2 ms object:state-changed:focused 1 [dialog] 'Tooltip'; ORCA SAYS: "Tooltip dialog About switching theme at runtime Colours retint instantly …"`
  - `source: crates/teksilo-widgets/src/tooltip/composite.rs:421-438 (name falls back to tr_widget!(a11y_tooltip_name()) = 'Tooltip', locales/en-US.ftl:32); crates/teksilo-core/src/widget_tree/accessibility_description_impl.rs relation_text (~389-410): a target's own name wins, so the body's labels are never walked`
  - `theme-tooltip 13:42:42: 13:42:54.996537 focused 1 [combo box] 'Theme'; ORCA 13:42:55.045211 'Theme combo box.' 13:42:55.045232 'Tooltip.'; FAIL [combo box] 'Theme' description='Tooltip'; 13:42:57.710521 object:property-change:accessible-role [dialog] 'Tooltip', nothing spoken`
- **Reproduced:** theme-tooltip 2 of 2 runs; the 'Tooltip.' description on all 16 census walks that reached Theme
- **Verification:** confirmed. Reproduced: theme-tooltip 2 of 2; 'Tooltip.' as the Theme combo's description on 8 of 8 census walks and every tooltip-bounce arrival
- **Fix idea:** For a described\_by target, prefer its contents' text over a fallback generic name: skip own\_text when it is the framework's generic tooltip name, or have CompositeTooltipWidget leave its name unset in the Tooltip role. Name the sticky dialog from the anchor ('Theme'), not 'Tooltip'.

### catalog-a-05 {#catalog-a-05}

Disabled buttons read as enabled on Linux

- **Example:** widget-catalog
- **Scenario:** catalog-a-disabled, catalog-a-buttons, catalog-a-chrome
- **Act:** Read the Buttons page's 'Button — disabled state' row (Button::enabled(false)) in the tree a reader walks
- **The reader should get:** No 'enabled'/'sensitive' state, so Orca says 'grayed' / unavailable
- **The reader gets:** Exactly the states of the enabled buttons: enabled, focusable, sensitive, plus a 'click' action. Orca's flat review or object navigation presents 'Default push button' as usable. The same holds for the Stepper's gated 'Next' (tree: \[push button\] 'Next' {focusable}) although Tab skips it. Disabled check boxes and toggles are fine: they read as read-only.
- **Platform:** Linux AT-SPI (measured). Windows is correct from source (accesskit\_windows-0.35.0 node.rs:534-536 IsEnabled = !is\_disabled).
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0 node.rs:376-380; crates/teksilo-widgets/src/button.rs:1301-1302
- **Evidence:**
  - `disabled 13:28:15: FAIL [push button] 'Default' #1 has no ['sensitive', 'enabled'], [push button] 'Default' #1 states=['enabled', 'focusable', 'sensitive', 'showing', 'visible']`
  - `same for 'Regular' #1 and 'Flat' #1; pass [push button] 'Default' #0 (enabled) has identical states`
  - `buttons census: disabled row actions [{'name': 'click', ...}]`
  - ``source: accesskit_atspi_common-0.20.0 node.rs:376-380 (`if state.is_read_only_supported() && state.is_read_only_or_disabled() { ReadOnly } else { Enabled | Sensitive }`); accesskit_consumer-0.39.0 node.rs:861-879 is_read_only_supported excludes Button, Tab, Link, MenuItem``
  - `disabled 13:43:10: FAIL [push button] 'Default' #1 states=['enabled', 'focusable', 'sensitive', 'showing', 'visible'] (same for 'Regular' #1, 'Flat' #1)`
- **Reproduced:** 2 of 2 runs (deterministic tree state)
- **Verification:** confirmed. Reproduced: disabled 2 of 2 (deterministic)
- **Fix idea:** Upstream: AccessKit should drop Enabled/Sensitive for any disabled node, not only read-only-capable ones. Teksilo: stop advertising Click/Focus on a disabled Button.

### catalog-a-06 {#catalog-a-06}

Expanded/collapsed state and has-popup never reach AT-SPI (Accordion, ToolBox, SplitButton, ComboBox)

- **Example:** widget-catalog
- **Scenario:** catalog-a-accordion, catalog-a-buttons
- **Act:** Focus the 'Show details' accordion header, press Space twice; Space on the ToolBox's 'Editor' header; Tab onto the SplitButton 'Save'
- **The reader should get:** Orca says 'Show details push button collapsed', then 'expanded' / 'collapsed' on each press; 'Editor … expanded'; the SplitButton says it has a menu
- **The reader gets:** 'Show details push button.' with no state, and nothing at all on Space: no state-changed:expanded event and no 'expandable' state. The SplitButton reads 'Save push button. Show dropdown menu.', with no has-popup and no hint that Down opens it.
- **Platform:** Linux AT-SPI (measured). macOS: accesskit\_macos-0.27.0 publishes no expanded state either (from source). Windows: ExpandCollapse pattern present (accesskit\_windows node.rs:718-724).
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0 node.rs:301-386
- **Evidence:**
  - `accordion 13:27:17: FAIL [push button] 'Show details' #0 has ['expandable'], states=['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']`
  - `'Space on Show details': FAIL a object:state-changed:expanded event … no object:state-changed:expanded event; FAIL Orca says 'expanded', Orca unheard`
  - `'Space on the ToolBox's Editor header': FAIL [push button] 'Editor' #0 has ['expanded'], states=['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']`
  - `buttons census Tab 23: '+83.1 ms ORCA SAYS: 'Save push button.'' '+83.1 ms ORCA SAYS: 'Show dropdown menu.''`
  - `source: accesskit_atspi_common-0.20.0 node.rs:301-386 state() maps no Expandable/Expanded and no has_popup (no 'expand' or 'popup' anywhere in the crate except Role::Menu); Teksilo sets them: accordion.rs:672, tool_box.rs:1022, split_button.rs:1053-1054, combo_box.rs:1250,1274`
  - `accordion 13:36:59: FAIL [push button] 'Show details' #0 states=['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']; act 'Space on Show details' 13:37:15.107..13:37:17.794 holds only object:bounds-changed events (the collapse animating)`
- **Reproduced:** 2 of 2 runs (deterministic)
- **Verification:** confirmed. Reproduced: accordion 2 of 2 completed runs (a third died on the winit pointer panic, see harness\_issues); SplitButton 'Save push button. Show dropdown menu.' in the buttons census
- **Fix idea:** Upstream: map expanded to State::Expandable plus State::Expanded (and has\_popup to State::HasPopup) in accesskit\_atspi\_common and accesskit\_macos. Until then Teksilo could carry 'expanded'/'collapsed' in the description of disclosure headers.

### catalog-a-07 {#catalog-a-07}

A ComboBox's current value is invisible on Linux (Theme switcher, fruit combo)

- **Example:** widget-catalog
- **Scenario:** catalog-a-combo, every census walk (Theme)
- **Act:** Tab onto 'Theme'; in the fruit combo choose Banana (Space, Down, Enter)
- **The reader should get:** 'Theme combo box, &lt;current theme&gt;'; after Enter the reader hears 'Banana' and later reads the combo as 'combo box Banana'
- **The reader gets:** 'Theme combo box.' without the selected theme on every visit. After choosing Banana there is no event and Orca is silent. The combo node has name '', interfaces Accessible/Action/Component/Selection, 0 children and 0 selected children, so the chosen value exists nowhere a Linux reader can reach.
- **Platform:** Linux AT-SPI (measured). Windows carries it in the UIA Value pattern (accesskit\_windows node.rs:592-597, from source).
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/combo\_box.rs:1248-1274
- **Evidence:**
  - `combo 13:11:21, tree after 'Enter: choose it': {'name': '', 'role': 'combo box', 'states': ['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible'], 'interfaces': ['Accessible', 'Action', 'Component', 'Selection'], ... 'selected_children': 0, ... 'child_count': 0}`
  - `'Enter: choose it': 13:11:51.840714 object:children-changed:remove -1 [combo box] '' -> [unknown] ''; FAIL Orca says 'Banana' (unheard)`
  - `every census: 'ORCA SAYS: 'Theme combo box.'' 'ORCA SAYS: 'Tooltip.'' (no value)`
  - `source: combo_box.rs:1259-1264 set_value(label); accesskit_atspi_common-0.20.0 node.rs:38-44 uses value only as a Role::Label's name; node.rs:494-520 interfaces: Text needs text runs, Value needs numeric_value`
  - `combo 13:37:39 'Enter: choose it': only children-changed:remove and defunct events, no value or focus event; tree-Enter--choose-it.txt:73 [combo box] '' {focusable,focused}`
- **Reproduced:** combo 2 of 2 completed runs (a third died, see harness\_issues); Theme without value in 16 of 16 census walks
- **Verification:** confirmed. Reproduced: combo 2 of 2; Theme without a value in 8 of 8 census walks
- **Fix idea:** Upstream: export a string value for ComboBox on AT-SPI, e.g. as its Text or as a selected child. In Teksilo, keep the current item as a (selected) child node of the combo box, which Orca reads for a combo box's value, or publish text runs for the displayed value.

### catalog-a-08 {#catalog-a-08}

The Inputs page's fruit ComboBox has no name

- **Example:** widget-catalog
- **Scenario:** catalog-a-combo, catalog-a-inputs
- **Act:** Tab onto the ComboBox under the 'ComboBox' heading
- **The reader should get:** 'Pick a fruit combo box' (or a real label)
- **The reader gets:** 'combo box.' The node is \[combo box\] ''. 'Pick a fruit' is only a placeholder, which Orca does not read for a combo box. The launch audit missed it because the control is below the fold.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). On Windows the placeholder becomes HelpText, so NVDA may read it as a hint, but the name is still empty (from source).
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/tabs/inputs.rs:265-274; crates/teksilo-widgets/src/combo\_box.rs:1252-1254
- **Evidence:**
  - `combo 13:29:46: +36.1 ms object:state-changed:focused 1 [combo box] ''; +142.2 ms ORCA SAYS: 'combo box.'; FAIL the tree holds [combo box] 'fruit'`
  - `inputs census 13:03:59 Tab 19: '+29.8 ms object:state-changed:focused 1 [combo box] ''' '+102.4 ms ORCA SAYS: 'combo box.''`
  - `source: examples/widget_catalog/src/tabs/inputs.rs:265-274 (ComboBox::from_items(...).placeholder(tr!(inp_combo_placeholder())), no .label()); combo_box.rs:1252-1254 names the node only from label`
  - `combo 13:37:39: +39.7 ms focused 1 [combo box] ''; +138.4 ms ORCA SAYS: 'combo box.'`
- **Reproduced:** 4 of 4 (combo 2 of 2, inputs census 2 of 2)
- **Verification:** confirmed. Reproduced: combo 2 of 2; inputs census 1 of 1; combo-reopen 2 of 2 ('combo box.')
- **Fix idea:** Example: add .label(...) (and teksu twin at inputs.rs:350-359). Framework: debug\_assert on an unlabelled ComboBox like Checkbox does, or fall back to the placeholder for the name.

### catalog-a-09 {#catalog-a-09}

Opening a ComboBox says nothing, and the first Down skips the item a reader never heard

- **Example:** widget-catalog
- **Scenario:** catalog-a-combo
- **Act:** Space on the fruit combo box, then Down
- **The reader should get:** On open, the reader hears the list and the current (highlighted) item, 'Apple'; Down moves to the next item
- **The reader gets:** Space: only children-changed:add \[combo box\] '' -&gt; \[unknown\] '' and silence (no selection, focus or active-descendant change). The first Down selects 'Banana', and Orca says 'List with 3 items. Banana.' 'Apple' is never announced. An \[unknown\] role node sits between the combo box and its list box.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `f9ffa98c` (combobox). Fixed part: opening names the option the list opens on ("Apple" on Space); the \[unknown\] node between the combo box and its list is gone.
- **Where:** crates/teksilo-widgets/src/combo\_box.rs:1031-1051; crates/teksilo-widgets/src/combo\_box/panel.rs:746-752
- **Evidence:**
  - `combo 13:11:21 'Space: open the combo box': 13:11:43.842088 object:children-changed:add 0 [combo box] '' -> [unknown] ''; FAIL Orca says 'Apple'`
  - `'Down in the open list': +28.8 ms object:state-changed:selected 1 [list item] 'Banana'; +29.1 ms object:selection-changed [list box] ''; +93.5 ms ORCA SAYS: 'List with 3 items'; 'Banana.'`
  - `tree after Space: [combo box] '' {focusable,focused} rel=['controller-for'] > [unknown] '' > [list box] '' {vertical} > [list item] 'Apple' {selectable} …`
  - `combo 13:37:39 'Space: open the combo box': only +33.2 ms children-changed:add [combo box] '' -> [unknown] ''; 'Down in the open list': +33.7 ms state-changed:selected 1 [list item] 'Banana'; ORCA 'List with 3 items' 'Banana.'`
- **Reproduced:** 2 of 2 completed runs
- **Verification:** corrected by the verifier. Reproduced: combo 2 of 2; first opening in combo-reopen 2 of 2 Real, but the cause and scope need correcting. Opening says nothing only when the combo has no value: no list item is selected and there is no active descendant. With a value, the list opens with a selected row and the adapter sends selection-changed. On a second opening that is lost to catalog-a-01 (see M2), not to this finding. The skipped 'Apple' is a deliberate convention, not an accessibility-only gap. combo\_box.rs:1031-1051: 'Treat no selection as an implicit cursor at index 0 — ArrowDown advances to index 1', so Down from nothing picks Banana. The rows' highlight is selection-driven (combo\_box/item.rs:108-113), so nothing is highlighted at open and sighted keyboard users skip Apple too. Add combo\_box.rs:1031-1051 to the location. The \[unknown\] wrapper between the combo and its list box is confirmed.
- **Fix idea:** On open, mark the highlighted row selected (or set active\_descendant on the combo box) so the adapters announce it. Give the popup wrapper GenericContainer so it is filtered out.

### catalog-a-10 {#catalog-a-10}

The selected SegmentedControl segment is announced 'not selected'

- **Example:** widget-catalog
- **Scenario:** catalog-a-segments, catalog-a-inputs
- **Act:** Tab into the 'Example choice' SegmentedControl (First is selected), then Right
- **The reader should get:** 'First, selected radio button'; after Right, 'Second, selected radio button'
- **The reader gets:** 'Example choice panel. Example choice. First. not selected radio button', then 'Second. not selected radio button'. Segments carry selectable/selected but not checkable/checked, and Orca reads checked for a radio button. The same happens for the overflow demo's selected 'Overview'.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows reads UIA SelectionItem IsSelected, so it is probably right there (not verified).
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `70183c50` (segmented).
- **Where:** crates/teksilo-widgets/src/segmented\_control/cell.rs:284-287
- **Evidence:**
  - `segments 13:28:34 'Tab into the segmented control': ORCA SAYS 'Example choice panel.' 'Example choice.' 'First.' 'not selected radio button'`
  - `FAIL [radio button] 'First' #0 has ['checked'], states=['enabled', 'focused', 'selectable', 'selected', 'sensitive', 'showing', 'visible']`
  - `segments 13:08:50: ORCA 13:09:03.034256 SPEECH OUTPUT: 'not selected radio button'`
  - `'Right: select the second segment': +25.1 ms object:state-changed:selected 1 [radio button] 'Second'; ORCA SAYS: 'Second.' 'not selected radio button'`
  - `source: crates/teksilo-widgets/src/segmented_control/cell.rs:284-287 (Role::RadioButton + set_selected); accesskit_atspi_common node.rs:336-337 and 366-372 (Checkable and Checked come from toggled only)`
  - `segments 13:38:24 'Tab into the segmented control': ORCA 'Example choice panel.' 'Example choice.' 'First.' 'not selected radio button'; FAIL [radio button] 'First' #0 states=['enabled', 'focused', 'selectable', 'selected', ...]`
- **Reproduced:** segments 2 of 2, inputs census 2 of 2 (deterministic)
- **Verification:** corrected by the verifier. Reproduced: segments 2 of 2; inputs census 1 of 1; scroll-reveal and scroll-back ('Overview ... not selected radio button') every run Reproduced on Linux. Orca's \_generateRadioState reads CHECKED (generator.py:661-676), and AT-SPI CHECKED comes only from toggled (node.rs:336-337, 366-372). The platform claim 'Windows probably right' is wrong. accesskit\_windows node.rs:648-656 exposes SelectionItem for Role::RadioButton only when toggled is Some, and node.rs:669-675 takes IsSelected from toggled == True. Toggle needs toggled too (576-578). accesskit\_macos node.rs:344-352 gives a Bool value only from toggled (or Role::Tab). A segment that sets only set\_selected (segmented\_control/cell.rs:284-287) therefore has no checked or selected state on any of the three platforms (from source).
- **Fix idea:** Set toggled(true/false) on each segment, as RadioButton does, alongside or instead of selected.

### catalog-a-11 {#catalog-a-11}

Opening a PopoverButton puts focus on a nameless, role-less node and the reader hears nothing

- **Example:** widget-catalog
- **Scenario:** catalog-a-popover
- **Act:** Tab to 'Open popover' (Buttons page), press Space
- **The reader should get:** Orca announces the popover (a named dialog) and its content 'Popover content. Click outside to dismiss.'
- **The reader gets:** Focus moves to \[unknown\] ''. Orca's generator produces 'pauses only', so nothing is said. The surface is \[dialog\] '' with the two labels inside, never read. Escape correctly returns focus to 'Open popover'.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The node is Role::Unknown on every platform (from source).
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `de3bb295` (popover-trigger). Fixed part: 'Open popover dialog Popover content Click outside to dismiss.' (3 of 3.
- **Where:** crates/teksilo-widgets/src/popover\_widget.rs:361, 584-660, 683-684
- **Evidence:**
  - `popover 13:12:05: 13:12:25.407646 object:children-changed:add 31 [panel] '' -> [unknown] ''; 13:12:25.408509 object:state-changed:focused 1 [unknown] ''`
  - `orca-debug.out '13:12:25.457588 - SPEECH GENERATOR: Results for [unknown] are pauses only'`
  - `tree: [unknown] '' {focusable,focused} > [dialog] '' > [label] 'Popover content', [label] 'Click outside to dismiss.'`
  - `popover 13:27:37 rerun: FAIL Orca says 'Popover' (unheard); Escape: pass focus lands on [push button] 'Open popover'`
  - `source: crates/teksilo-widgets/src/popover_widget.rs:683-684 ('Focus targets the panel; request_focus walks to its first focusable descendant', none here, so the PopoverBody itself); PopoverBody (583-660) has no accessibility() and so gets Role::Unknown (crates/teksilo-core/src/accessibility.rs:473-478); surface name defaults to '' (popover_widget.rs:361, 504-511) though popover_surface.rs:262-268 says 'Every dialog node must have an accessible name; use the trigger's label'`
  - `popover 13:38:53: +82.5 ms focused 1 [unknown] ''; orca-debug.out 13:39:14.856257 'SPEECH GENERATOR: Results for [unknown] are pauses only', 13:39:14.858488 NULL SPEECH: stop, 'SPEECH: Speak []'`
- **Reproduced:** 2 of 2 runs
- **Verification:** confirmed. Reproduced: popover 2 of 2
- **Fix idea:** When the content has no focusable descendant, focus the surface Dialog and name it from the trigger's label by default. Give PopoverBody Role::GenericContainer so it is filtered out.

### catalog-a-12 {#catalog-a-12}

Content entering the tree with a selected item moves Orca's locus of focus off the real focus

- **Example:** widget-catalog
- **Scenario:** catalog-a-first-open-containers, catalog-a-first-open-chrome, catalog-a-scroll-reveal, catalog-a-inputs, launch of every tab
- **Act:** (a) Up from Chrome opens Containers (two embedded TabWidgets); (b) Down from Containers opens Chrome (Stepper); (c) Tab from the 'First' segment to the 'Segmented control width' slider, which scrolls the 'Document view' SegmentedControl into view, then Tab to it
- **The reader should get:** (a) 'Containers page tab' only; (b) 'Chrome page tab' only; (c) 'Segmented control width horizontal slider 720', then 'Overview … selected radio button'
- **The reader gets:** Each selection container that enters the tree with a selected child gets object:selection-changed, and Orca 46.1 moves its locus of focus to that child and speaks it. (a) 'Containers page tab.' is cut, then 'Overview page tab.' and 'Edit page tab.'. Orca ends on an embedded tab while focus is on Containers, and in some runs never re-reads Containers. (b) 'Account page tab.' is spoken, cut, and Orca once ended on it. (c) The slider's speech is cut by 'Document view panel. Document view. Overview. not selected radio button', and the next Tab onto Overview is silent because Orca already holds it as locus ('Setting locus of focus to existing locus of focus'). At launch the same happens: 'Palette page tab.' / 'Example choice. First. …' spoken, then cut by 'frame.'.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0 adapter.rs:78-80, 269-277
- **Evidence:**
  - `first-open-containers 13:16:57: 13:17:09.377865 object:state-changed:focused 1 [page tab] 'Containers'; 13:17:09.381006/.381374/.381430 object:selection-changed 0 [page tab list] ''; ORCA 13:17:09.496854 'Containers page tab.'; 13:17:09.566819 'Overview page tab.'; 13:17:09.602571 'Containers page tab.'; 13:17:09.644997 'Edit page tab.' (last)`
  - `chrome census 13:23:17 'Up: open Containers': ORCA 13:23:29.765944 'Containers page tab.'; 13:23:29.820557 'Overview page tab.'; 13:23:29.866806 'Edit page tab.' (last)`
  - `first-open-chrome 13:17:23: 13:17:35.526630 object:selection-changed 0 [page tab list] 'Steps'; ORCA 13:17:35.638106 'Account page tab.'`
  - `scroll-reveal 13:19:09: 13:19:23.123963 focused 1 [slider] 'Segmented control width'; 13:19:23.124155 object:selection-changed 0 [panel] 'Document view'; ORCA 13:19:23.229943 'Segmented control width horizontal slider 720.'; 13:19:23.324551 NULL SPEECH: stop; 13:19:23.324694 'Document view panel.' … 'Overview.' 'not selected radio button'`
  - `next act: 13:19:27.014397 focused 1 [radio button] 'Overview'; orca '13:19:27.053697 - FOCUS MANAGER: Setting locus of focus to existing locus of focus' (nothing spoken)`
  - `source: accesskit_atspi_common-0.20.0 adapter.rs:78-80 (add_node enqueues SelectionChanged for any added selected item) and 269-277 (emitted from a HashSet, so the order varies per run); orca/scripts/default.py:1579-1602 onSelectionChanged → set_locus_of_focus(event, child)`
  - `first-open-containers 13:48:20: 13:48:32.485994 focused 1 [page tab] 'Containers'; 13:48:32.488014/.488441/.488480 object:selection-changed [page tab list] ''; ORCA 13:48:32.560261 'Containers page tab.', 13:48:32.614124 stop, 'Overview page tab.', 13:48:32.659246 stop, 'Containers page tab.', 13:48:32.718169 stop, 13:48:32.718244 'Edit page tab.' (last)`
  - `scroll-reveal 13:48:53: 13:49:07.139581 focused 1 [slider] 'Segmented control width'; 13:49:07.139705 object:selection-changed [panel] 'Document view'; ORCA 13:49:07.271526 slider speech, 13:49:07.373769 NULL SPEECH: stop, 'Document view panel.' ... 'Overview.' 'not selected radio button'`
  - `scroll-reveal 13:40:10 orca-debug.out:2046 13:40:27.929147 FOCUS MANAGER: Setting locus of focus to existing locus of focus (the Tab onto Overview, nothing spoken)`
- **Reproduced:** stray embedded-tab speech 5 of 5 opens of Containers; Orca ending off 'Containers' 3 of 5. 'Account page tab' 5 of 5 opens of Chrome; ending on it 1 of 5. Scroll-reveal cut + locus moved 5 of 5 (scroll-reveal 3 of 3, inputs census 2 of 2), then Overview silent 3 of 3 scroll-reveal runs.
- **Verification:** confirmed. Reproduced: Containers opened for the first time: stray 'Overview/Edit page tab' speech 5 of 5 (first-open-containers 3 + chrome census 2), Orca ending on an embedded tab 4 of 5. Chrome first open: 'Account page tab' spoken 3 of 3, Orca ending on it 2 of 3. scroll-reveal: slider speech cut and locus moved 3 of 3, Overview silent next 3 of 3.
- **Fix idea:** Upstream: accesskit\_atspi\_common should not emit selection-changed for a container that is itself newly added. Teksilo cannot suppress it short of not publishing is\_selected until after the first update the container appears in.

### catalog-a-13 {#catalog-a-13}

A Banner announces only its title ('Warning'), never its message; the four banners on Chrome are cut on arrival

- **Example:** widget-catalog
- **Scenario:** catalog-a-first-open-chrome, catalog-a-chrome
- **Act:** Down from Containers opens the Chrome page (Info / Success / Warning / Error banners)
- **The reader should get:** If a banner announces itself it says its message, e.g. 'Warning: Disk is 90 % full.'
- **The reader gets:** Four announcements: 'Info', 'Error', 'Success', 'Warning' (titles only). They are all emitted just before the page tab's focus change and are cut. On a second opening they come from defunct nodes and are dropped (catalog-a-01). The messages ('Disk is 90 % full.', 'Network connection lost.') are never announced.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The announced string is the name on all three adapters (task platform facts), so Windows/macOS also get the title alone.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/banner.rs:256-265
- **Evidence:**
  - `first-open-chrome 13:17:23: 13:17:35.514314 object:announcement 1 [status bar] 'Error' text='Error'; 13:17:35.522939 … 'Info'; 13:17:35.523433 … 'Success'; 13:17:35.523918 … 'Warning'; 13:17:35.524820 object:state-changed:focused 1 [page tab] 'Chrome'`
  - `ORCA 13:17:35.539666 'Error' … 13:17:35.552739 'Warning'; 13:17:35.597562 NULL SPEECH: stop`
  - `FAIL each banner's announcement carries its message: 'Warning'+'Disk is 90 % full', 'Error'+'Network connection lost'`
  - `tree: [status bar] 'Warning' > [label] 'Disk is 90 % full.'`
  - `source: crates/teksilo-widgets/src/banner.rs:256-265 (Role::Status, Live::Polite, set_name(self.title), 'the description is read by descending into the body text widget', which no live-region announcement does); accesskit_atspi_common adapter.rs:72-77 announces the name`
  - `first-open-chrome 13:48:27: 13:48:39.211253..13:48:39.220825 object:announcement [status bar] 'Error','Warning','Success','Info'; 13:48:39.222090 focused 1 [page tab] 'Chrome'; ORCA 13:48:39.250221 'Error' ... 13:48:39.356469 NULL SPEECH: stop`
- **Reproduced:** titles only 5 of 5 page opens (first-open-chrome 3 of 3, chrome census 2 of 2); cut before focus 3 of 3 first opens; dropped as defunct on reopen 2 of 2
- **Verification:** confirmed. Reproduced: titles only 5 of 5 page opens (first-open-chrome 3, chrome census launch 2); announcements before the focus change and cut 3 of 3 first opens; dropped as defunct on reopen 2 of 2
- **Fix idea:** Name the live node 'title: description' (or announce title plus description through the announcer when a banner appears). Consider not announcing banners that are present when a page first mounts.

### catalog-a-14 {#catalog-a-14}

Collapsed Accordion content stays in the tree a reader walks

- **Example:** widget-catalog
- **Scenario:** catalog-a-accordion
- **Act:** Read the Containers page's two accordions: 'Show details' (collapsed) and 'Advanced' (expanded)
- **The reader should get:** The collapsed section's body is hidden from AT (as ToolBox does for its collapsed panels)
- **The reader gets:** Both sections expose identical subtrees (\[push button\] 'Show details' &gt; \[landmark\] 'Show details' &gt; \[label\] 'Body of the first accordion section.'). Toggling changes nothing on the bus, so a reader cannot tell what is open and reads collapsed content. Each body is also a named landmark (Role::Region).
- **Platform:** Linux AT-SPI (measured). The node is in the TreeUpdate for every platform (from source).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/accordion.rs:528-535
- **Evidence:**
  - `accordion 13:10:04 tree 'the Show details accordion header, focused': [push button] 'Show details' {focusable,focused} rel=['controller-for'] / [landmark] 'Show details' / [label] 'Body of the first accordion section.' / [push button] 'Advanced' {focusable} / [landmark] 'Advanced' / [label] 'Body of the second accordion section.'`
  - `'Space on Show details' 13:10:22.544473..13:10:25.231587: no events at all on the bus`
  - `ToolBox in the same tree: only [landmark] 'Editor' present after selecting Editor (collapsed panels hidden, tool_box.rs:1179-1181)`
  - `source: crates/teksilo-widgets/src/accordion.rs:528-535 (vertical orientation wraps the region in Collapse; only the horizontal orientation uses visible_when); animations/collapse.rs:197-203 (a11y-transparent, clips paint only); signals.rs example accordion_expanded=false for 'Show details'`
  - `accordion 13:36:59 tree-the-Show-details-accordion-header--focused.txt:76-81: [push button] 'Show details' {focusable,focused} > [landmark] 'Show details' > [label] 'Body of the first accordion section.' / [push button] 'Advanced' > [landmark] 'Advanced' > [label] 'Body of the second accordion section.'`
- **Reproduced:** 2 of 2 runs (deterministic)
- **Verification:** confirmed. Reproduced: accordion 2 of 2 (deterministic)
- **Fix idea:** Hide the AccordionRegion from AT (set\_hidden, or make it dormant once the collapse animation ends) while collapsed, as ToolBox does. Consider a Group role instead of Region so each body is not a landmark.

### catalog-a-15 {#catalog-a-15}

Slider value read as raw float noise: 'Vertical slider vertical slider 0.30000001192092896'

- **Example:** widget-catalog
- **Scenario:** catalog-a-segments, catalog-a-inputs
- **Act:** Focus the Inputs page's 'Vertical slider' (0..1, value 0.3)
- **The reader should get:** 'Vertical slider vertical slider 0.3' (or '30 %')
- **The reader gets:** 'Vertical slider vertical slider 0.30000001192092896.' The value is an f32 widened to f64 with no value text, and the increment is 0.009999999776482582.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows RangeValue carries the same f64 (from source).
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `5c8ff299` (state-publish).
- **Where:** crates/teksilo-widgets/src/slider.rs:791-799
- **Evidence:**
  - `segments 13:08:50: ORCA 13:08:59.002020 SPEECH OUTPUT: 'Vertical slider vertical slider 0.30000001192092896.'`
  - `value={'current': 0.30000001192092896, 'minimum': 0.0, 'maximum': 1.0, 'increment': 0.009999999776482582, 'text': None}`
  - `inputs census Tab 13: '+81.0 ms ORCA SAYS: 'Vertical slider vertical slider 0.30000001192092896.''`
  - `source: crates/teksilo-widgets/src/slider.rs:791 builder.set_numeric_value(self.value.get() as f64) (also 798-799 for step/jump)`
  - `segments 13:38:24: ORCA SAYS 'Vertical slider vertical slider 0.30000001192092896.'; value={'current': 0.30000001192092896, ..., 'increment': 0.009999999776482582, 'text': None}`
- **Reproduced:** 4 of 4 (segments 2 of 2, inputs census 2 of 2)
- **Verification:** confirmed. Reproduced: segments 2 of 2; inputs census; scroll-back 2 of 2; combo-reopen scenes
- **Fix idea:** Convert via the f32's shortest decimal form (e.g. `format!("{}", v).parse::<f64>()`) and publish a value text (a formatter, like SpinBox's NumberPresentation).

### catalog-a-16 {#catalog-a-16}

A Splitter divider's position is never spoken, on focus or when moved

- **Example:** widget-catalog
- **Scenario:** catalog-a-splitter, catalog-a-containers
- **Act:** Tab onto the Containers page's Splitter divider; press Right
- **The reader should get:** 'Splitter divider, 50 %' on focus; the new percentage on Right
- **The reader gets:** 'vertical splitter Splitter divider.' on focus. On Right the value event is sent at once (+38 ms), but Orca says only 'vertical splitter'. The name is the generic 'Splitter divider', which does not say which panes it separates.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0 node.rs:249; orca formatting.py:467-475
- **Evidence:**
  - `splitter 13:11:03: 13:11:03.992885 object:state-changed:focused 1 [separator] 'Splitter divider'; ORCA 13:11:04.709411 SPEECH OUTPUT: 'vertical splitter Splitter divider.'`
  - `'Right on the divider': 13:11:07.905568 object:property-change:accessible-value 0 [separator] 'Splitter divider'; ORCA 13:11:07.922080 SPEECH OUTPUT: 'vertical splitter'`
  - `source: accesskit_atspi_common node.rs:249 Role::Splitter => AtspiRole::Separator; orca formatting.py:467-470 SEPARATOR format has no value (SPLIT_PANE at 471-475 reads value+percentage); Teksilo sets value '50%' and numeric value (splitter/handle.rs:797-813) and name tr a11y_splitter_divider_name (handle.rs:798, en-US.ftl:34)`
  - `splitter 13:39:32: focused 1 [separator] 'Splitter divider'; ORCA 'vertical splitter Splitter divider.'; Right: +35.3 ms object:property-change:accessible-value; ORCA 'vertical splitter'`
- **Reproduced:** 2 of 2 runs
- **Verification:** corrected by the verifier. Reproduced: splitter 2 of 2 Real, but the Orca citation is wrong. formatting.py:467-470 does give SEPARATOR a value. 'focused' (used when the focused object changes, e.g. Right) is roleName + availability, with no value: that is why Right says only 'vertical splitter'. 'unfocused' (used on arrival) is roleName + availability + (labelOrName or displayedText or value): the value is dropped on arrival only because the divider has a name. The upstream mapping stays: Role::Splitter maps to Separator (node.rs:249), not SPLIT\_PANE, whose format reads value and percentage (formatting.py:471-475). Teksilo has no clean workaround: dropping the name would bring the value back on arrival only.
- **Fix idea:** Upstream: map a focusable Role::Splitter with a value to AT-SPI SPLIT\_PANE (as GTK's Paned does) or have Orca read a separator's value. Teksilo: name the divider from its panes (e.g. 'Resize leading / trailing').

### catalog-a-17 {#catalog-a-17}

Non-linear Stepper: every step is its own Tab stop and a step's status is not exposed

- **Example:** widget-catalog
- **Scenario:** catalog-a-chrome
- **Act:** Tab through the Chrome page's embedded Stepper
- **The reader should get:** One Tab stop for the step tab list (arrows between steps, as the TabWidget does); each step says whether it is complete, optional or current
- **The reader gets:** 'Account', 'Preferences' and 'Review' are three consecutive Tab stops, read 'Preferences page tab.' / 'Review page tab.' with nothing about optional/complete. The supporting text 'Complete this step to continue' is a child label of the tab that Orca does not read. The gated 'Next' is skipped by Tab, and since disabled buttons read as enabled on Linux (catalog-a-05) a reader exploring finds a 'Next push button' that does nothing.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/stepper/indicator.rs:170-176, 304-327
- **Evidence:**
  - `chrome census 12:59:24 Tab 8/9/10: focused 1 [page tab] 'Account' / 'Preferences' / 'Review'; '+117.0 ms ORCA SAYS: 'Preferences page tab.'' '+83.1 ms ORCA SAYS: 'Review page tab.''`
  - `tree: [page tab] 'Account' {focusable,selectable,selected} … > [label] 'Complete this step to continue'; [push button] 'Next' {focusable}`
  - `source: crates/teksilo-widgets/src/stepper/indicator.rs:170-176 (.focusable(true) on every clickable marker, no roving) and 304-327 (only selected, aria_current, name, position; StepStatus not exposed)`
  - `chrome census 13:38:23 Tab 8/9/10: focused 1 [page tab] 'Account' (Ignoring defunct), 'Preferences' ('Preferences page tab.'), 'Review' ('Review page tab.'); tree-launch.txt:83 [push button] 'Next' {focusable} while gated`
- **Reproduced:** 2 of 2 chrome census runs
- **Verification:** confirmed. Reproduced: chrome census 2 of 2
- **Fix idea:** Roving tab index over the markers (the TabBar pattern). Expose status in the description (e.g. 'completed', 'optional').

### catalog-a-18 {#catalog-a-18}

TabWidget publishes an unnamed ScrollView inside every tab list, and the tab lists are unnamed (confirmed)

- **Example:** widget-catalog (palette, layout, visuals, containers, chrome, buttons, styling, inputs)
- **Scenario:** all censuses
- **Act:** Read the launch tree
- **The reader should get:** The tab list named (e.g. 'Widget categories'), holding its tabs directly; no unnamed wrappers
- **The reader gets:** Each \[page tab list\] '' (the catalog's and both embedded ones on Containers) holds an unnamed \[panel\] '' (TabBar's header ScrollArea, Role::ScrollView → AT-SPI panel), which holds the tabs. The TabList's setsize '22' is reported on the panel and on the non-tab buttons in the list ('Scroll tabs down', 'Show all tabs', 'teksu! DSL'). Only 16 of the 22 page tabs are in the tree at a time, because the header ScrollArea clips the rest. Each page's own ScrollArea is also an unnamed \[panel\] '' under its \[scroll pane\] tab panel. Orca speaks none of this, so the reader-facing cost is low: the tab list is unnamed, and object navigation meets extra containers and a partial list.
- **Platform:** Linux AT-SPI (measured). ScrollView maps to UIA Pane on Windows (from source).
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/tab\_widget/bar.rs:1591-1612, 2199-2218
- **Evidence:**
  - `palette census tree-launch.txt: [page tab list] '' {vertical} / [panel] '' attrs={'setsize': '22'} / [page tab] 'Palette' … / [page tab] 'Menus' … (16 tabs) / [push button] 'Scroll tabs down' desc='Scroll tabs down' {focusable} attrs={'setsize': '22'}`
  - `containers census tree-launch.txt: [page tab list] '' {horizontal} / [panel] '' attrs={'setsize': '3'} / [page tab] 'Overview' …`
  - `[scroll pane] 'Palette' / [panel] '' (the example's TabContent ScrollArea, examples/widget_catalog/src/main.rs:730)`
  - `source: crates/teksilo-widgets/src/tab_widget/bar.rs:1591-1612 (ScrollArea around the header row), scroll_area.rs:1304 (Role::ScrollView, no name), accesskit_atspi_common node.rs:221 (ScrollView → Panel), tab_widget/bar.rs:2199-2218 (TabList: role, orientation, size_of_set, no name)`
  - `palette census tree-launch.txt: [page tab list] '' {vertical} / [panel] '' attrs={'setsize': '22'} / 16 [page tab] rows / [push button] 'Scroll tabs down' ... attrs={'setsize': '22'}`
- **Reproduced:** every census launch tree (16 of 16)
- **Verification:** confirmed. Reproduced: every census launch tree (8 of 8 tabs)
- **Fix idea:** Give the header ScrollArea Role::GenericContainer so it is filtered out (the clipped-tab reachability still comes from the roving keys). Add a TabWidget/TabBar label API for the TabList name.

### catalog-a-19 {#catalog-a-19}

No page has a heading: section titles and GroupHeader are plain labels

- **Example:** widget-catalog (palette, layout, visuals, containers, chrome, buttons, styling, inputs)
- **Scenario:** all censuses
- **Act:** Read any page's tree (e.g. Inputs: 'Checkbox', 'RadioButton (in a group)', 'Toggle' …)
- **The reader should get:** Section titles exposed as headings so a reader can jump between sections
- **The reader gets:** Every section title is a \[label\]. The GroupHeader widget, meant for section captions, emits Role::Label (and shows as \[label\] 'Section title' with a \[separator\] child). Not verified with Orca's own heading navigation, which the harness does not exercise.
- **Platform:** Linux AT-SPI (measured tree)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/group\_header.rs:170-175
- **Evidence:**
  - `inputs census tree-launch.txt: [label] 'Checkbox' / [check box] 'Two-state checkbox' … [label] 'RadioButton (in a group)' …`
  - `containers census tree: [label] 'GroupHeader' / [label] 'Section title' / [separator] ''`
  - `source: crates/teksilo-widgets/src/group_header.rs:170-175 (Role::Label); examples/widget_catalog/src/shared.rs:261-277 section() uses a TextWidget`
  - `inputs census tree-launch.txt: [label] 'Checkbox' / [check box] 'Two-state checkbox' ... [label] 'RadioButton (in a group)'`
- **Reproduced:** every census tree
- **Verification:** confirmed. Reproduced: every census tree
- **Fix idea:** Offer a heading role (with level) on GroupHeader and a TextWidget heading option. Use it in the catalog's section().

### catalog-a-20 {#catalog-a-20}

Example misuses: radio set not grouped, misleading IconButton names, unnamed breadcrumb, pointer-only TwistArrows

- **Example:** widget-catalog
- **Scenario:** catalog-a-inputs, catalog-a-buttons, catalog-a-chrome, catalog-a-visuals
- **Act:** Tab walk of Inputs and Buttons; read the Chrome and Visuals trees
- **The reader should get:** The 'RadioButton (in a group)' options in a named RadioGroup (one Tab stop, 'x of 3'); each icon button named for what it does; the breadcrumb landmark named
- **The reader gets:** Option A/B/C are three separate Tab stops with no group name ('Option A. selected radio button'). The X-glyph IconButton::clear() is named 'Find…', the same as the search button next to it. The five size demos are named 'Compact · 22dp' … 'Hero · 50dp' (lit!, untranslated) instead of 'Search'. The Breadcrumb is an unnamed \[landmark\] '' whose crumbs are non-focusable links, the current one 'widget-catalog' included. The three clickable TwistArrows on Visuals are hidden from AT and not focusable (pointer-only). The Wizard trigger is a focusable Button inside the OverlayTrigger's own \[push button\] 'Onboarding' (nested buttons).
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** crates/teksilo-widgets/src/stepper/wizard.rs:267-310
- **Evidence:**
  - `inputs census Tab 6/7/8: 'Option A.' 'selected radio button' / 'Option B.' 'not selected radio button' / 'Option C.' …`
  - `buttons census Tab 11/12: '[push button] 'Find…'' twice; Tab 14: 'Compact · 22dp push button.'`
  - `chrome tree: [landmark] '' / [link] 'Home' / [link] 'Documents' / [link] 'Teksilo' / [link] 'widget-catalog'; [push button] 'Onboarding' / [push button] 'Open wizard' {focusable}`
  - `source: examples/widget_catalog/src/tabs/inputs.rs:157-162 (no RadioGroup); tabs/buttons.rs:87 (IconButton::clear().tooltip(tr!(demo_find()))), 96-121; tabs/chrome.rs:117 (Breadcrumb::new() without .label()), 39 (.trigger(Button::new(...))); tabs/visuals.rs:100-104 (TwistArrow on_click; twist_arrow.rs:213-215 set_hidden)`
  - `wizard-trigger 13:49:43 run: 'AT-SPI click on the Onboarding trigger': RunError: {'name': 'Onboarding', 'role': 'push button'} offers no action on AT-SPI; chrome tree-launch.txt:68-69 [push button] 'Onboarding' > [push button] 'Open wizard' {focusable}`
- **Reproduced:** deterministic, 2 of 2 census runs each
- **Verification:** confirmed. Reproduced: buttons, inputs, chrome, visuals censuses 1-2 of 1-2 each; wizard-trigger 2 of 2
- **Fix idea:** Wrap the radios in RadioGroup::label(...); give the clear button its own tooltip; name the size demos 'Search (compact)'; label the Breadcrumb. Let the Wizard reuse a Button trigger instead of wrapping it.

### catalog-a-21 {#catalog-a-21}

Framework fallback names are generic and repeat the role ('Toolbar tool bar', 'Splitter divider', 'Tooltip', 'Step content')

- **Example:** widget-catalog
- **Scenario:** catalog-a-chrome, catalog-a-containers
- **Act:** Tab into the Chrome page's Toolbar; focus the Splitter divider; the Theme tooltip
- **The reader should get:** A container without an app-given name reads as its role alone, not a name that repeats it
- **The reader gets:** 'Toolbar tool bar' then 'New push button.' The divider reads 'vertical splitter Splitter divider.'; the tooltip reads 'Tooltip dialog …'; the Stepper root is \[panel\] 'Step content'.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toolbar.rs:1163; splitter/handle.rs:798; tooltip/composite.rs:437; stepper.rs:583
- **Evidence:**
  - `chrome census Tab 4: '+490.2 ms ORCA SAYS: 'Toolbar tool bar'' '+490.2 ms ORCA SAYS: 'New push button.''`
  - `splitter: 'vertical splitter Splitter divider.'`
  - `source: crates/teksilo-widgets/src/toolbar.rs:1160-1164 (tr_widget!(a11y_toolbar_name())), locales/en-US.ftl:32,34,39,50`
  - `chrome census Tab 4: ORCA SAYS 'Toolbar tool bar' 'New push button.'`
- **Reproduced:** 2 of 2 chrome census runs
- **Verification:** confirmed. Reproduced: chrome census 2 of 2; splitter 2 of 2; wizard-trigger 2 of 2
- **Fix idea:** Leave an unlabelled container unnamed (the role alone is spoken) or derive the name from the anchor/owner.

### catalog-a-M1 {#catalog-a-m1}

Controls scrolled out of a page and back are silent: Shift+Tab back up the Inputs page reads 7 controls in a row as nothing

- **Example:** widget-catalog
- **Scenario:** verify-catalog-a-scroll-back
- **Act:** Inputs page: Tab from the page tab down to the fruit combo box (every control spoken on the way; the page scrolls), then Shift+Tab 16 times back up
- **The reader should get:** Each control is read again on the way up ('With label toggle button pressed.', 'Enable feature ...', 'Option C ...', ..., 'Two-state checkbox check box not checked.')
- **The reader gets:** The 8 controls that never left the viewport are read. Seven in a row (With label, Enable feature, Option C, Option B, Option A, Tristate checkbox, Two-state checkbox) are silent. They scrolled out of the ScrollArea on the way down, and the consumer filter dropped them (filters.rs:64-86). The adapter sent them defunct, and they came back on the same paths, which Orca's libatspi still holds as defunct. This is catalog-a-01's mechanism with no Teksilo lifecycle event behind it, so a NodeId-generation fix in Teksilo cannot cover it. Any long scrolling page or pane is affected.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows/macOS: accesskit\_windows and accesskit\_macos have no equivalent defunct state (not measured).
- **Severity:** critical; **layer:** upstream
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: verify-catalog-a-scroll-back had 14 defunct drops in the sweep, none now.
- **Where:** upstream accesskit\_atspi\_common-0.20.0 adapter.rs:90-111 / accesskit\_unix-0.23.0 atspi/bus.rs:434-439; triggered by crates/teksilo-widgets/src/scroll\_area.rs (clips\_children)
- **Evidence:**
  - `verify-catalog-a-scroll-back 13:48:48: 13:48:57.920282 object:state-changed:focused 1 'Two-state checkbox' path /org/a11y/atspi/accessible/0/79228227907972078893904429056 (spoken going down)`
  - `13:49:04.919148 object:state-changed:defunct 1 'Two-state checkbox' (scrolled out while Tabbing down); 13:49:33.123032 children-changed:add -> 'Two-state checkbox' (scrolled back); 13:49:35.033967 focused 1 (same path); ORCA 13:49:35.042291 EVENT MANAGER: Ignoring defunct object: [check box: 'Two-state checkbox']`
  - `same run: 13:49:05.453907 defunct 'With label'; 13:49:23.475653 focused (same path); ORCA 13:49:23.507764 Ignoring defunct object: [toggle button: 'With label']`
  - `Shift+Tab 1-8 (Notebook .. Volume) spoken, Shift+Tab 9-15 nothing spoken, Shift+Tab 16 'teksu! DSL toggle button not pressed.' spoken: identical in both runs`
  - `source: accesskit_consumer-0.39.0 filters.rs:64-86 (a clipped child outside its parent's box is ExcludeSubtree); accesskit_atspi_common-0.20.0 adapter.rs:90-111 (remove_node emits Defunct); scroll_area.rs:1304 (Role::ScrollView clips)`
- **Reproduced:** 2 of 2 runs, identical stop by stop (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Upstream: when a node is only filtered out (clipped) and later re-included, emit state-changed:defunct=false or re-register it on a fresh path. Also make AddAccessible carry libatspi's '((so)(so)(so)iiassusau)' signature so the state cache is refreshed. Teksilo cannot fix this case with NodeId generations.

### catalog-a-M2 {#catalog-a-m2}

A ComboBox's list is silent from its second opening on: arrowing through the options says nothing

- **Example:** widget-catalog
- **Scenario:** verify-catalog-a-combo-reopen
- **Act:** Inputs page fruit combo: Space (open), Down, Enter (choose Banana), Space (open again), Down, Up
- **The reader should get:** On the reopen the reader hears the current fruit, then 'Cherry', then 'Banana' as they arrow
- **The reader gets:** The first opening works: Down says 'List with 3 items. Banana.'. Closing sends every row and the list box defunct. On the second opening the selection-changed that would announce Banana comes from the defunct list box and Orca ignores it. Down to Cherry and Up to Banana are ignored the same way. Nothing is spoken for any option, so a reader choosing a value a second time is blind.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** critical; **layer:** upstream
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: verify-catalog-a-combo-reopen had 8 failed checks in the sweep, none now.
- **Where:** same mechanism as catalog-a-01: the dropdown content keeps its WidgetIds across openings (crates/teksilo-widgets/src/combo\_box.rs dropdown\_content\_id, parked while closed) and so its AT-SPI paths; root cause accesskit\_atspi\_common-0.20.0 adapter.rs:90-111
- **Evidence:**
  - `verify-catalog-a-combo-reopen 13:42:36 'Down (1)': +29.4 ms state-changed:selected 1 [list item] 'Banana'; ORCA 'List with 3 items' 'Banana.'`
  - `'Enter: choose Banana': object:state-changed:defunct 1 [unknown] '', [list item] 'Cherry', [list box] '', [list item] 'Banana', [list item] 'Apple'`
  - `'Space: open the combo box again (2)': +28.3 ms object:selection-changed [list box] ''; ORCA 13:43:09.790773 EVENT MANAGER: Ignoring defunct object: [list box]; nothing spoken`
  - `'Down (2)': +20.0 ms state-changed:selected 1 [list item] 'Cherry'; ORCA 13:43:13.966938 Ignoring defunct object: [list item: 'Banana'], 13:43:13.967019 Ignoring defunct object: [list box]; nothing spoken`
  - `second run 13:44:42: 13:45:16.104039 / 13:45:20.194696 / 13:45:20.202805 / 13:45:24.076140 the same Ignoring defunct lines, nothing spoken`
- **Reproduced:** 2 of 2 runs (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** The upstream fix of catalog-a-01. In Teksilo, rebuild the popup rows (fresh WidgetIds, hence fresh NodeIds) on each opening, or fold an opening generation into the rows' NodeIds.

### catalog-a-M3 {#catalog-a-m3}

The Chrome page's Wizard opens as 'Dialog dialog', and its step text is never read

- **Example:** widget-catalog
- **Scenario:** verify-catalog-a-wizard-trigger
- **Act:** Tab to 'Open wizard' (Chrome page), press Space; also AT-SPI click on it
- **The reader should get:** 'Onboarding dialog', the current step ('Welcome' / its text 'Step 1 — welcome to Teksilo'), then the focused control
- **The reader gets:** 'Dialog dialog', 'Step content panel.', 'Cancel push button.'. The dialog node is \[dialog\] 'Dialog' (the framework fallback), and focus lands on Cancel, so the step's body is never spoken. Wizard::present\_wizard passes the title to ModalRequest::title, which only a native-window presentation reads (teksilo-app app.rs:722-742, the OS window title). The in-tree ModalContainer gets no .title(), so it falls back to a11y-dialog-name 'Dialog'.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The name is 'Dialog' in the TreeUpdate for every platform (from source).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/stepper/wizard.rs:85
- **Evidence:**
  - `wizard-trigger 13:52:15 'Space on Open wizard': +68.2 ms object:children-changed:add [frame] '' -> [dialog] 'Dialog'; +69.2 ms object:state-changed:focused 1 [push button] 'Cancel'; +158.2 ms ORCA SAYS: 'Dialog dialog' / 'Step content panel.' / 'Cancel push button.'; FAIL Orca says 'Onboarding', FAIL Orca says 'Welcome'`
  - `same run 'AT-SPI click on Open wizard': +95.1 ms add [dialog] 'Dialog'; ORCA 'Dialog dialog' 'Step content panel.' 'Cancel push button.'`
  - `run 13:53:04 and the 13:49:43 run: identical 'Dialog dialog' / 'Step content panel.' / 'Cancel push button.'`
  - `source: crates/teksilo-widgets/src/stepper/wizard.rs:85 tree.add(ModalContainer::boxed(Box::new(stepper))) with no .title(); wizard.rs:89 .title(title) on the request; crates/teksilo-app/src/app.rs:722-742 (request title used only for ResolvedModalPresentation::NativeWindow); crates/teksilo-widgets/src/dialog.rs:296-301 (fallback a11y_dialog_name)`
- **Reproduced:** 3 of 3 openings by keyboard over 3 runs, plus 2 of 2 by AT-SPI click (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Name the ModalContainer from the wizard's label (.title(spec.title)). Also honour ModalRequest::title for in-tree presentation. Point the dialog's described\_by (or initial focus) at the current step's content so it is read on opening.

### catalog-a-M4 {#catalog-a-m4}

Pressing Next in a Wizard/Stepper changes the step silently

- **Example:** widget-catalog
- **Scenario:** verify-catalog-a-wizard-trigger
- **Act:** In the open Onboarding wizard, Tab to Next and press Enter
- **The reader should get:** The reader hears that step 2 ('Configure') is now shown
- **The reader gets:** The step content is swapped (scroll pane 'Welcome' removed, 'Configure' added, a 'Back' button added). Focus stays on Next, so no focus event is sent, nothing is announced and Orca says nothing. nav.rs focus\_primary requests focus on the button that already has it, and the stepper announces nothing (no announce call in stepper.rs or stepper/).
- **Platform:** Linux AT-SPI / Orca 46.1 (measured); no event is produced for any platform (from source)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/stepper/nav.rs:156-167
- **Evidence:**
  - `wizard-trigger 13:52:15 'Enter on Next': +51.3 ms children-changed:add [panel] 'Step content' -> [scroll pane] 'Configure'; +52.0 ms children-changed:remove -> [scroll pane] 'Welcome'; +52.1 ms children-changed:add [panel] '' -> [push button] 'Back'; no focus event; FAIL Orca says 'Configure' (nothing spoken)`
  - `run 13:53:04: same three events at +55.7/+56.7/+57.1 ms, nothing spoken`
  - `run 13:49:43 (earlier version, same Enter on Next): add 'Configure', remove 'Welcome', add 'Back', no focus change`
  - `source: crates/teksilo-widgets/src/stepper/nav.rs:156-167 (focus_primary → request_focus on the already-focused Next)`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** On advance or back, announce 'Step N of M: &lt;title&gt;' through the announcer, or move focus to the new step's content or heading.

### catalog-a-M5 {#catalog-a-m5}

Escape does not close the Wizard dialog

- **Example:** widget-catalog
- **Scenario:** verify-catalog-a-wizard-trigger
- **Act:** Open the Onboarding wizard, press Escape
- **The reader should get:** The dialog closes and focus returns to 'Open wizard' (the dialog keyboard convention)
- **The reader gets:** Nothing happens: no event, and focus stays on Cancel inside the modal. Only Cancel closes it (Space on Cancel returns focus to 'Open wizard' correctly). Wizard::new defaults close\_behavior to ModalCloseBehavior::Manual, while the modal default is EscapeOrClickOutside.
- **Platform:** All platforms (framework behaviour); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/stepper/wizard.rs:131
- **Evidence:**
  - `wizard-trigger 13:52:15 'Escape': no events in the act; FAIL focus lands on [push button] 'Open wizard'`
  - `run 13:53:04 and both Escapes of the 13:49:43 run: no events; the following Shift+Tabs cycled Next/Cancel inside the dialog`
  - `'Space on Cancel': +44.9 ms children-changed:remove [frame] -> [dialog] 'Dialog'; +45.4 ms focused 1 [push button] 'Open wizard'; ORCA 'Open wizard push button.'`
  - `source: crates/teksilo-widgets/src/stepper/wizard.rs:131 close_behavior: ModalCloseBehavior::Manual; crates/teksilo-core/src/modal.rs:21-31 (default EscapeOrClickOutside)`
- **Reproduced:** 4 of 4 Escape presses over 3 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Default the Wizard to EscapeKey (routing Escape through the Cancel path, so an unsaved-changes confirmation can still veto it).

### catalog-a-M6 {#catalog-a-m6}

An Accordion is one push button whose subtree holds its whole content

- **Example:** widget-catalog
- **Scenario:** catalog-a-accordion
- **Act:** Read the Containers page's accordions in the tree a reader walks
- **The reader should get:** The header is a button, and the region it controls is its sibling (as ToolBox does): \[push button\] 'Show details', then \[landmark\] 'Show details' &gt; body
- **The reader gets:** The Accordion widget itself carries Role::Button with the title. Its children() are the whole VStack, header plus region, so the body sits inside the push button: \[push button\] 'Show details' &gt; \[landmark\] 'Show details' &gt; \[label\] 'Body ...'. Its controller-for relation points at its own descendant. Anything interactive in an accordion's content is published as a descendant of a button. That includes the header's trailing slot and every DockingLayout split pane (docking/panel.rs:850 builds each one as an Accordion). The ToolBox on the same page exposes header and panel as siblings.
- **Platform:** Linux AT-SPI (measured tree). The structure is in the TreeUpdate for every platform (from source); what NVDA or VoiceOver do with it was not measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/accordion.rs:668-682
- **Evidence:**
  - `accordion 13:36:59 tree-the-Show-details-accordion-header--focused.txt:76-81: [push button] 'Show details' {focusable,focused} rel=['controller-for'] > [landmark] 'Show details' > [label] 'Body of the first accordion section.'`
  - `same run, ToolBox scene: children-changed:add [panel] '' -> [push button] 'General' and children-changed:add [panel] '' -> [landmark] 'General' (siblings)`
  - `source: crates/teksilo-widgets/src/accordion.rs:668-678 (Role::Button on the Accordion itself), 680-682 (children = root VStack), 545-549 (VStack holds header_with_ring and the Collapse(region))`
- **Reproduced:** 2 of 2 runs (deterministic tree)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Put Role::Button (name, expanded, controls) on the header node and give the Accordion root GenericContainer (or Group), so the region is the header's sibling.

### catalog-a-M7 {#catalog-a-m7}

Every page tab reads a developer note ('See: cargo run -p …') as its description

- **Example:** widget-catalog (palette, layout, visuals, containers, chrome, buttons, styling, inputs)
- **Scenario:** every census
- **Act:** Focus any page tab (Up/Down or Tab)
- **The reader should get:** 'Chrome page tab.' (plus a short user-facing hint at most)
- **The reader gets:** 'Chrome page tab. App chrome: toolbar, status bar, breadcrumbs, wizards, banners. See: cargo run -p title\_bar\_demo.' on every arrival. The tab's tooltip is the page's 'refs' text, and it duplicates the subtitle label at the top of the page.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured)
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/widget\_catalog/src/main.rs:596
- **Evidence:**
  - `chrome census 13:38:23 'focus the Chrome tab': ORCA 'Chrome page tab.' 'App chrome: toolbar, status bar, breadcrumbs, wizards, banners. See: cargo run -p title_bar_demo.'`
  - `palette tree-launch.txt: [page tab] 'Palette' desc='All surface, text, and editor roles, with a rich-text + emoji pangram so theme switching reads visually. See: docs/reactive-…'`
  - `source: examples/widget_catalog/src/main.rs:596 .tooltip((entry.refs_fn)()); locales/en-US/widget_catalog.ftl:65`
- **Reproduced:** every page-tab arrival in 16 census runs (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Give the page tabs a short user-facing tooltip, or none, and keep the developer references in the page body.
