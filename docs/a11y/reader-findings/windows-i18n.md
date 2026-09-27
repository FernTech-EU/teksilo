<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Generated from the sweep of 25 and 26 September 2026 (on 261a218f) and its re-measure of 27 September 2026 (on c198e4d1). See ../reader-findings.md. -->

# Windows and languages

Examples: `multi-window`, `internationalization`.
22 findings: 2 critical, 2 high, 8 medium, 10 low.
Swept on `261a218f` on 25 and 26 September 2026, measured again on
`c198e4d1` on 27 September 2026.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [winintl-01](#winintl-01) | internationalization | A control that scrolls out of a ScrollArea and back is defunct to libatspi: Orca says nothing when it gets focus again | critical | Linux | fixed |
| [winintl-02](#winintl-02) | internationalization | A wrapped Arabic paragraph's accessible text is garbled: it starts mid-word with the paragraph's end, then repeats the whole paragraph | high | Linux | open |
| [winintl-03](#winintl-03) | internationalization | Combo boxes (Theme, Language) never tell Orca their current value, are never exposed as expanded, and are silent after a pick | medium | Linux | partly fixed |
| [winintl-04](#winintl-04) | internationalization | Arrowing through the LanguageSwitcher's open list switches the whole application's language at every step | medium | all | fixed |
| [winintl-05](#winintl-05) | internationalization | LanguageSwitcher's accessible name is a hardcoded English 'Language', while the UI around it is French or Arabic | medium | all | open |
| [winintl-06](#winintl-06) | multi-window | On Wayland, ctx.focus\_window does not raise an open window: F1 with Help already open does nothing and says nothing | medium | Linux | open |
| [winintl-07](#winintl-07) | multi-window | The multi-window text fields have no accessible name | high | all | open (example) |
| [winintl-08](#winintl-08) | internationalization | The price and count buttons ('− 100', '+ 100', '− 1', '+ 1') do not say what they change, and pressing them is silent | medium | all | open (example) |
| [winintl-09](#winintl-09) | internationalization | After a language switch the Signal-side Currency row keeps USD while the bundle row shows EUR or SAR, so the reader gets two currencies | medium | all | open (example) |
| [winintl-10](#winintl-10) | internationalization | The layout-direction note stays in English after switching to French, while tagged fr-FR | low | all | open (example) |
| [winintl-11](#winintl-11) | internationalization | Untranslated literals in the internationalization example are tagged with the active language, and the window title is never translated | low | all | open (example) |
| [winintl-12](#winintl-12) | internationalization | Switching to French with the Français button is silent | low | Linux | open (example) |
| [winintl-13](#winintl-13) | multi-window | Every TextInput carries an empty, unnamed status node | low | all | open |
| [winintl-14](#winintl-14) | internationalization | The default Toolbar name repeats its role: 'Toolbar tool bar' | low | Linux | open |
| [winintl-15](#winintl-15) | internationalization | Content outside the ScrollArea viewport is missing from the tree, and its text changes still produce events Orca drops | low | Linux | upstream |
| [winintl-16](#winintl-16) | internationalization | The declared language reaches Orca 46.1 only as a text attribute it does not use; the AT-SPI object Locale is always empty | low | Linux | upstream |
| [winintl-17](#winintl-17) | multi-window | winit 0.30.13 panics ('failed to get pointer data') when a window opens while the harness's fake-input device comes and goes | low | Linux | harness |
| [winintl-v-01](#winintl-v-01) | multi-window, internationalization | Every ComboBox (ThemeSwitcher, LanguageSwitcher) is silent from its second opening on: Orca drops its list box and rows as defunct | critical | Linux | fixed |
| [winintl-v-02](#winintl-v-02) | multi-window, internationalization | The 'Total (bundle)' row is frozen at the launch price: after + 100 the page states two different totals | medium | all | open (example) |
| [winintl-v-03](#winintl-v-03) | multi-window, internationalization | libatspi rejects every Cache.AddAccessible signal AccessKit sends (wrong D-Bus signature), so a re-added node is never refreshed in the reader's AT-SPI cache | medium | Linux | upstream |
| [winintl-v-04](#winintl-v-04) | multi-window, internationalization | Language names are declared in the UI language: 'العربية' and 'Français' inherit en-US, and so do the LanguageSwitcher's autonyms | low | Linux | open |
| [winintl-v-05](#winintl-v-05) | multi-window, internationalization | multi-window's two demo buttons mislead: 'Open help (F1) / Toggle fullscreen (F11)' only opens help, and 'This panel dims…' is a push button that does nothing | low | all | open (example) |

### winintl-01 {#winintl-01}

A control that scrolls out of a ScrollArea and back is defunct to libatspi: Orca says nothing when it gets focus again

- **Example:** internationalization
- **Scenario:** winintl-intl-scroll-focus-back
- **Act:** internationalization, window made 300 px tall (KWin script). grab\_focus on English, Tab down through Français, العربية, the Language combo, Leading, Trailing, − 100, + 100, − 1, + 1 (the ScrollArea scrolls to follow focus and the language row leaves the viewport), then Shift+Tab back up to Trailing, Leading, the combo box, العربية, Français, English.
- **The reader should get:** Each Shift+Tab stop is spoken, e.g. 'Trailing push button.', 'English push button.'.
- **The reader got (`261a218f`):** Focus lands on each control and the bus carries object:state-changed:focused 1, but Orca drops every event as coming from a defunct object and says nothing for all 6 stops. Orca's locus of focus stays on '− 100', so where-am-I would name the wrong control. Mechanism: AccessKit's clip filter drops ScrollView children that leave the viewport. accesskit\_atspi\_common then announces them defunct and never clears that when the same NodeId comes back. The K2 fix covers the announcer's reserved nodes, not this path. A child that leaves through a window resize gets no defunct event, but one that leaves by scrolling does (compare the 14:26:02 removals with the 14:26:19 ones).
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The defunct-and-same-id rule is AT-SPI-specific; UIA and macOS not assessed.
- **Severity:** critical; **layer:** upstream
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** Every Shift+Tab stop on a control that scrolled out and came back is spoken: 'Trailing push button.', 'Leading push button.', 'Language combo box.', 'العربية push button.', 'Français push button.', 'English push button.'. The controls are still announced defunct when they scroll out, but they come back under a new id, so Orca no longer drops their focus events.
- **Measured again:** winintl-intl-scroll-focus-back, 2 of 2 runs; winintl-intl-scroll-reenter, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-scroll-focus-back 'Tab to − 100': '+19.8 ms object:state-changed:defunct 1 [push button] 'Trailing'' ... '+28.3 ms object:state-changed:focused 1 [push button] '− 100''`
  - `pass1 winintl-intl-scroll-focus-back 'Shift+Tab to Trailing': '+40.0 ms object:children-changed:add [panel] '' -> [push button] 'Trailing'' / '+43.2 ms object:state-changed:focused 1 [push button] 'Trailing'' / '+142.7 ms ORCA SAYS: 'Trailing push button.''`
  - `pass1 winintl-intl-scroll-focus-back 'Shift+Tab to English': '+25.8 ms object:state-changed:focused 1 [push button] 'English'' / '+91.7 ms ORCA SAYS: 'English push button.''; 'every check passed, nothing observed'`
  - `pass2 winintl-intl-scroll-focus-back 'Shift+Tab to Trailing': '+315.1 ms ORCA SAYS: 'Trailing push button.''; 'Shift+Tab to English': '+134.5 ms ORCA SAYS: 'English push button.''; 'every check passed, nothing observed'`
  - `pass2 winintl-intl-scroll-reenter 'the window grows; the intro scrolls back in': 'pass  the heading that came back is not defunct to libatspi'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/scroll\_area.rs:1305 (set\_clips\_children on the ScrollView, unchanged); mechanism accesskit\_consumer-0.39.1/src/filters.rs:54-88, accesskit\_atspi\_common-0.21.0/src/adapter.rs:91-106 (remove\_node still emits Defunct; add\_node, 49-82, still never clears it); the workaround now: crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-283 (a returning node is handed a new id)
- **Evidence (`261a218f`):**
  - `events.jsonl (winintl-intl-scroll-focus-back-20260925-142554-450715): {"wall": "14:26:19.710965", "type": "object:state-changed:defunct", "detail1": 1, ... "source": {"path": "/org/a11y/atspi/accessible/0/79228163030773171657411395584", "name": "English", "role": "push button"}`
  - `events.jsonl: 14:26:48.706105 object:children-changed:add 2 panel '' -> push button 'English' (same path ...71657411395584 comes back)`
  - `report.txt: '+21.6 ms object:state-changed:focused 1 [push button] 'English'' / 'FAIL  Orca says 'English'' / 'Orca unheard: 'English''`
  - `orca-debug.out: '14:26:54.145137 - FOCUS MANAGER: Locus of focus is [push button: '− 100']'`
  - `orca-debug.out: '14:26:54.146188 - EVENT MANAGER: object:state-changed:focused for [push button: 'English'] in [application: 'internationalization'] (1, 0, 0) is not obsoleted'`
  - `orca-debug.out: '14:26:54.146205 - EVENT MANAGER: Ignoring defunct object: [push button: 'English']'`
  - `orca-debug.out: '14:26:41.198968 - EVENT MANAGER: Ignoring defunct object: [push button: 'Trailing']' (same for Leading, combo box 'Language', 'العربية', 'Français')`
  - `winintl-intl-scroll-reenter-20260925-142449-424170: after the intro scrolled out and back in, the heading is listed as '/org/a11y/atspi/accessible/0/79228162938539451288863637504 states=['defunct', 'enabled', 'sensitive', 'showing', 'visible']'`
  - `source: crates/teksilo-widgets/src/scroll_area.rs:1305 builder.inner_mut().set_clips_children(); accesskit_consumer-0.39.0/src/filters.rs:56-85 (a clipping parent excludes children outside its box beyond the first); accesskit_atspi_common-0.20.0/src/adapter.rs:91-106 remove_node emits ObjectEvent::StateChanged(State::Defunct, true), adapter.rs:49-83 add_node never clears it; Orca event_manager.py:797-798 ignores a defunct source, scripts/default.py:660 refuses a defunct locus of focus`
  - `winintl-intl-scroll-focus-back-20260925-143855-751209 'Tab to − 100': '+10.9 ms object:state-changed:defunct 1 [push button] 'Trailing'' ... '+38.4 ms object:state-changed:focused 1 [push button] '− 100''`
  - `same run 'Shift+Tab to Trailing': '+24.1 ms object:children-changed:add [panel] '' -> [push button] 'Trailing'' / '+26.6 ms object:state-changed:focused 1 [push button] 'Trailing'' / 'FAIL Orca says 'Trailing'' / '14:39:42.514255 EVENT MANAGER: Ignoring defunct object: [push button: 'Trailing']'`
  - `same run, window shrink act: 13 × object:children-changed:remove and no defunct event; 'Tab to Leading': '+74.3 ms ORCA SAYS: 'Leading push button.''`
  - `winintl-intl-scroll-focus-back-20260925-144424-819381: 'Shift+Tab to English' FAIL, '14:45:24.001597 EVENT MANAGER: Ignoring defunct object: [push button: 'English']'`
  - `winintl-intl-scroll-reenter-20260925-145220-970825: 'FAIL the heading that came back is not defunct to libatspi' / '/org/a11y/atspi/accessible/0/79228162938539451288863637504 states=['defunct', 'enabled', 'sensitive', 'showing', 'visible']'`
  - `batch1.log: 142 × 'dbind-WARNING: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau' from the listener processes; strings libatspi.so.0 → '((so)(so)(so)iiassusau)', '((so)(so)(so)a(so)assusau)', 'AT-SPI: AddAccessible with unknown signature %s'`
- **Reproduced:** 2 of 2 runs (142554, 143001), 6 of 6 re-entered focus stops silent in each run. Deterministic, not timing-dependent.
- **Verification:** confirmed. Reproduced: 2 of 2 of my runs (143855, 144424): 6 of 6 re-entered focus stops silent in each, Orca's locus stuck on '− 100'. Plus the sweep's 2 runs. Deterministic.
- **Fix idea:** Teksilo can stop marking the ScrollView as clipping children for AT, or keep offscreen children in the tree, so nothing leaves and comes back. Upstream, accesskit\_atspi\_common should clear the defunct state or re-register the object when a node re-enters the filtered tree. The announcer fix (K2) should be generalised to this case.

### winintl-02 {#winintl-02}

A wrapped Arabic paragraph's accessible text is garbled: it starts mid-word with the paragraph's end, then repeats the whole paragraph

- **Example:** internationalization
- **Scenario:** winintl-intl-rtl
- **Act:** internationalization: switch to ar-SA (Space on العربية, from en-US or from fr-FR), then read the intro paragraph label (body-paragraph, 218 characters, wraps to 2 lines).
- **The reader should get:** The label's name and text are the 218-character paragraph 'اختر لغة من القائمة أدناه. … بينهما.' split into 2 lines in logical order.
- **The reader got (`261a218f`):** The AT-SPI text is 339 characters. Line 0 (the top line, y=115) is 'لنهاية، ويعكس … بينهما': the last 121 characters without the final period, starting inside the word 'والنهاية'. Line 1 (y=131, only 495 px wide) holds the whole paragraph again. The accessible name, which is the concatenation of the runs, is the same garbled string, so Orca would read the second half first, starting mid-word, and then everything. Single-line Arabic labels are correct.
- **Platform:** Linux AT-SPI (measured). By source, Windows and macOS are affected too: the text runs are in the shared AccessKit tree, and a Label's name/value is their text (accesskit\_consumer node.rs:845-851).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** After the switch to Arabic, the intro paragraph's accessible text is still 339 characters: line 0 starts mid-word with the paragraph's end ('لنهاية، ويعكس … بينهما'), and line 1 holds the whole paragraph again. The label's name is the same garbled concatenation.
- **Measured again:** winintl-intl-switch, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-switch notes: "after العربية: line 0-121: '@x=18,y=115,w=681 لنهاية، ويعكس الصف السفلي أطفاله بشكل واضح. الإنجليزية والفرنسية من اليسار إلى اليمين، لذا يحتفظ الصف بنفس الترتيب بينهما'" / "line 121-339: '@x=17,y=131,w=495 اختر لغة من القائمة أدناه. …بينهما.'"`
  - `pass1 winintl-intl-switch 'Space on العربية (Arabe)': '+174.8 ms object:property-change:accessible-name [label] 'لنهاية، ويعكس الصف السفلي أطفاله بشكل واضح. … بينهمااختر لغة من القائمة أدناه. … بينهما.''`
  - `pass2 winintl-intl-switch notes: same line 0-121 and line 121-339 ranges`
  - `pass1 winintl-intl-switch notes: en-US lines 0-113, 113-223, 223-260 and fr-FR lines 0-105, 105-218, 218-298 are correct`
- **Where (`c198e4d1`):** text-typeset 1.12.0 (still the version in Cargo.lock) src/layout/paragraph.rs:543-590 and :684-708; trusted by crates/teksilo-core/src/accessibility/text\_runs.rs:228-303 (TextRunSource::from\_geometry)
- **Evidence (`261a218f`):**
  - `run note (winintl-intl-rtl-20260925-142744-482200): "en-US to ar-SA: line 0-121: '@x=18,y=115,w=681 لنهاية، ويعكس الصف السفلي أطفاله بشكل واضح. الإنجليزية والفرنسية من اليسار إلى اليمين، لذا يحتفظ الصف بنفس الترتيب بينهما'"`
  - `run note: "en-US to ar-SA: line 121-339: '@x=17,y=131,w=495 اختر لغة من القائمة أدناه. يؤدي التبديل إلى العربية إلى عكس اتجاه التخطيط — يتم تبديل البداية والنهاية، ويعكس الصف السفلي أطفاله بشكل واضح. الإنجليزية والفرنسية من اليسار إلى اليمين، لذا يحتفظ الصف بنفس الترتيب بينهما.'"`
  - `tree outline after the switch: "[label] 'لنهاية، ويعكس الصف السفلي أطفاله بشكل واضح. الإنجليزية والفرنسية من اليسار إلى اليمين، لذا يحتفظ الصف بنفس الترتيب بينهمااختر لغة من القائمة أ…"`
  - `The source paragraph (examples/internationalization/locales/ar-SA.ftl body-paragraph) is 218 chars / 399 bytes. Line 0 = chars 96..217 = bytes 175..398, line 1 = bytes 0..399.`
  - ``source: text-typeset-1.12.0/src/layout/paragraph.rs:543-559 flatten_runs keeps each RTL run's glyphs in visual order; :567-590 map_breaks_to_glyph_indices assumes ascending clusters; :684-708 line range from min/max cluster, with `byte_end = text.len()` whenever the line reaches the end of the visual array, so the logically first line claims the whole text; src/layout/geometry.rs:240-245 line byte_range from it; crates/teksilo-core/src/accessibility/text_runs.rs:228-300 (TextRunSource::from_geometry) trusts those ranges``
  - `winintl-intl-rtl-20260925-144537-819381 notes: "line 0-121: '@x=18,y=115,w=681 لنهاية، ويعكس …بينهما'" / "line 121-339: '@x=17,y=131,w=495 اختر لغة من القائمة أدناه. … بينهما.'"`
  - `winintl-intl-rtl-20260925-144008-751209 '+137.0 ms object:property-change:accessible-name [label] 'لنهاية، … بينهمااختر لغة … بينهما.''`
  - `winintl-intl-switch-20260925-144131-751209 note 'after العربية: line 0-121 … / line 121-339 …'; en-US lines 0-113, 113-223, 223-260 and fr-FR 0-105, 105-218, 218-298 correct`
- **Reproduced:** 4 of 4 runs that showed ar-SA (winintl-intl-switch 135858 and 142910, winintl-intl-rtl 140110 and 142744). Deterministic.
- **Verification:** confirmed. Reproduced: 4 of 4 of my runs that showed ar-SA (winintl-intl-rtl 144008, 144537; winintl-intl-switch 144131, 145045). Deterministic.
- **Fix idea:** Break lines over glyphs in logical order (or map each visual line back to its logical char range with the run's embedding level), and never snap byte\_end to text.len() for a line that is not logically last. As a guard, text\_runs could reject overlapping or out-of-order line ranges, falling back to one flat run. The line geometry puts the logical end on the top line, so the painted paragraph may be misordered too (not checked in pixels).

### winintl-03 {#winintl-03}

Combo boxes (Theme, Language) never tell Orca their current value, are never exposed as expanded, and are silent after a pick

- **Example:** internationalization
- **Scenario:** winintl-intl-switcher
- **Act:** Tab to the LanguageSwitcher combo box (also the ThemeSwitcher in both examples), Alt+Down, Down, Enter, then Shift+Tab/Tab back onto it.
- **The reader should get:** 'Language combo box English (en-US)'; an expanded state when the list opens; after Enter, the combo box and its new value.
- **The reader got (`261a218f`):** 'Language combo box.' with no value, and the same for 'Theme combo box.' / 'السمة combo box.'. There is no expanded or expandable state and no object:state-changed:expanded event when the list opens. After Enter the list is removed and Orca says nothing. The popup sits under an unnamed \[unknown\]-role node. ComboBox sets `value` (combo\_box.rs:1259-1264), but accesskit\_atspi\_common exposes a string value only through a Text interface (text ranges) and Value only for numbers, and it maps no expanded state.
- **Platform:** Linux AT-SPI / Orca (measured). By source, Windows exposes the value through ValuePattern (accesskit\_windows node.rs:592-597, has\_value) and macOS through AXValue.
- **Severity:** medium; **layer:** upstream
- **Severity in the sweep:** high. The list is now spoken on every opening (winintl-v-01 is gone) and names the current option ('English (en-US)'), so a Linux reader can learn the value by opening the list. The closed box still never says it.
- **Status:** Partly fixed by `f9ffa98c`, found by the re-measure: that commit did not report it. What remains is under **Now**.
- **Now (`c198e4d1`):** Focusing a combo box still says only 'Language combo box.' or 'Theme combo box.', with no value, and opening the list raises no expanded state or event. After Enter picks a language, focus now returns to the combo box and Orca says 'Language combo box.', still without the value. The popup is now a list box directly under the combo box, with no unnamed \[unknown\] node.
- **Measured again:** winintl-intl-switcher, 2 of 2 runs; winintl-intl-tabwalk-ar, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-switcher 'Tab to the language combo box': '+88.5 ms ORCA SAYS: 'Language combo box.'' / 'FAIL  Orca says 'English''`
  - `pass1 winintl-intl-switcher 'Alt+Down opens the list': '+37.9 ms object:children-changed:add [combo box] 'Language' -> [list box] ''' / 'FAIL  a object:state-changed:expanded event from [*] '*''`
  - `pass1 winintl-intl-switcher 'Enter picks it': '+76.6 ms object:state-changed:focused 1 [combo box] 'Language'' / '+489.6 ms ORCA SAYS: 'Teksilo — Internationalization Demo'' / '+489.6 ms ORCA SAYS: 'Language combo box.''`
  - `pass2 winintl-intl-switcher 'Tab to the language combo box': '+253.8 ms ORCA SAYS: 'Language combo box.'' / 'FAIL  Orca says 'English''; 'Enter picks it': '+1154.8 ms ORCA SAYS: 'Language combo box.''`
  - `pass2 winintl-intl-switcher 'Shift+Tab, Tab back onto the combo box': '+747.2 ms ORCA SAYS: 'Language combo box.'' / 'FAIL  Orca says 'Français''`
  - `pass2 winintl-intl-tabwalk-ar 'Tab 8': '+98.7 ms ORCA SAYS: 'السمة combo box.''`
  - `source: accesskit_atspi_common-0.21.0/src/node.rs:476-482 (Text only for text ranges, Value only for a numeric value); no 'expand' anywhere in accesskit_atspi_common-0.21.0/src`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box.rs:1327-1353 (sets value and expanded correctly); gap in accesskit\_atspi\_common-0.21.0/src/node.rs:476-482 and its state set at node.rs:301-389 (no Expanded/Expandable)
- **Evidence (`261a218f`):**
  - `report (winintl-intl-switcher-20260925-143125-582786) 'Tab to the language combo box': '+46.7 ms ORCA SAYS: 'Language combo box.'' / 'FAIL  Orca says 'English''`
  - `'Alt+Down opens the list': '+40.7 ms object:children-changed:add [combo box] 'Language' -> [unknown] ''' / 'FAIL  a object:state-changed:expanded event from [*] '*''`
  - `tree while open (winintl-intl-switcher-20260925-140335-4191370): "[combo box] 'Language' {focusable,focused} rel=['controller-for']", states ['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']`
  - `launch tree: combo box 'Language' child_count 0, interfaces ['Accessible', 'Action', 'Component', 'Selection']`
  - `'Enter picks it': '+8.6 ms object:children-changed:remove [combo box] 'Language' -> [unknown] ''' / 'FAIL  Orca says something' / 'Orca said nothing'`
  - `winintl-intl-tabwalk-ar-20260925-141917-286986 Tab 8: "ORCA SAYS: 'السمة combo box.'"`
  - `source: crates/teksilo-widgets/src/combo_box.rs:1248-1274; accesskit_atspi_common-0.20.0/src/node.rs:38-44 (name), 486-492 (supports_text = text ranges only, supports_value = numeric current_value), no Expanded mapping in the state set (node.rs ~300-360)`
  - `winintl-intl-switcher-20260925-144622-819381: '+98.7 ms ORCA SAYS: 'Language combo box.'' / 'FAIL Orca says 'English'' / 'FAIL a object:state-changed:expanded event from [*] '*''`
  - `winintl-intl-switcher-20260925-144053-751209 'Enter picks it': object:children-changed:remove [combo box] 'Language' -> [unknown] '' then 5 defunct events, 'FAIL Orca says something'`
- **Reproduced:** 2 of 2 switcher runs (140335, 143125), plus every Tab onto a combo box in the other runs. Deterministic.
- **Verification:** confirmed. Reproduced: 2 of 2 switcher runs (144053, 144622), plus every Tab onto a combo box in my switch, tabwalk-ar and mw-tabwalk runs ('Language combo box.', 'Theme combo box.', 'السمة combo box.'). Deterministic.
- **Fix idea:** Upstream: map a string value and the expanded state in accesskit\_atspi\_common. Meanwhile, Teksilo could keep a child that carries the selection when the combo box is closed (GTK exposes it as the selected child of the popup), or give the closed combo box a Text/label path to the value.

### winintl-04 {#winintl-04}

Arrowing through the LanguageSwitcher's open list switches the whole application's language at every step

- **Example:** internationalization
- **Scenario:** winintl-intl-switcher
- **Act:** Tab to the Language combo box, Alt+Down, Down (only moving to 'français (fr-FR)', not picking it).
- **The reader should get:** Moving through the options only moves the highlight. The language changes when the user picks one with Enter.
- **The reader got (`261a218f`):** Down already switches the app to French: every label is renamed ('Bonjour, Alice !', 'Barre d'outils', …) while the list is still open. A reader browsing to Arabic flips the whole UI to right-to-left mid-browse. This is a change of context on input (WCAG 3.2.2). ComboBox::on\_select fires on arrow keys by design, and LanguageSwitcher calls set\_locale from it.
- **Platform:** All (widget behaviour); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `f9ffa98c` (combobox).
- **Now (`c198e4d1`):** Arrowing through the LanguageSwitcher's list only moves the highlight: Orca says 'français (fr-FR)' and nothing is renamed. Escape keeps English. Enter switches the application to French. Down on the closed box opens the list without switching either.
- **Measured again:** winintl-intl-switcher, fix-combobox-language, verify-winintl-intl-switcher-closed and verify-winintl-intl-switcher-escape, 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-switcher 'Down to the next language': '+137.1 ms ORCA SAYS: 'français (fr-FR)'' / 'pass  no object:property-change:accessible-name event from [label] 'Bonjour''`
  - `pass2 winintl-intl-switcher 'Down to the next language': '+218.2 ms ORCA SAYS: 'français (fr-FR)'', no FAIL in the act`
  - `pass1 fix-combobox-language 'Escape keeps English': 'pass  the tree holds nodes named ['Hello, Alice!']'; 'Down, Enter switches to French': 'pass  the tree holds nodes named ['Vitrine i18n de Teksilo']'`
  - `pass2 verify-winintl-intl-switcher-closed 'Escape': 'FAIL  the tree holds nodes named ['Bonjour, Alice !']' / 'missing 'Bonjour, Alice !'' (the app stayed in English)`
  - `pass2 verify-winintl-intl-switcher-escape 'Down to français': 'FAIL  a object:property-change:accessible-name event from [label] 'Bonjour''; 'Escape closes the list': 'pass  the tree holds nodes named ['Hello, Alice!']'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box.rs:1096 (commit\_highlight fires on\_select on commit only); crates/teksilo-widgets/src/language\_switcher.rs:220
- **Evidence (`261a218f`):**
  - `report (winintl-intl-switcher-20260925-143125-582786) 'Down to the next language': '+61.7 ms object:property-change:accessible-name [label] 'Bonjour, Alice !' text='Bonjour, Alice !'' / '+346.8 ms ORCA SAYS: 'français (fr-FR)'' / 'FAIL  no object:property-change:accessible-name event from [label] 'Bonjour''`
  - `same act, first run (140335): '+44.3 ms object:property-change:accessible-name [tool bar] "Barre d'outils"' and 8 × 'Orca ignored an event whose source was defunct'`
  - `source: crates/teksilo-widgets/src/combo_box.rs:360-370 (on_select fires on 'arrows / type-ahead / Home / End'); crates/teksilo-widgets/src/language_switcher.rs:220 .on_select(|c: &LocaleChoice, ctx| ctx.set_locale(c.tag.clone()))`
  - `verify-winintl-intl-switcher-escape-20260925-144811-970825 'Down to français': '+87.9 ms object:property-change:accessible-name [label] 'Bonjour, Alice !''; 'Escape closes the list': 'FAIL the tree holds nodes named ['Hello, Alice!']' / 'missing 'Hello, Alice!''`
  - `verify-winintl-intl-switcher-closed-20260925-144841-970825 'Down on the closed combo box': '+90.9 ms object:property-change:accessible-name [label] 'Bonjour, Alice !'' / '+775.7 ms ORCA SAYS: 'List with 3 items'' / 'français (fr-FR)'; 'Escape': 'pass the tree holds nodes named ['Bonjour, Alice !']'`
- **Reproduced:** 2 of 2 runs
- **Verification:** corrected by the verifier. Reproduced: 4 of 4: switcher 144053 and 144622 (Down in the open list), verify-winintl-intl-switcher-escape 144811, verify-winintl-intl-switcher-closed 144841 Confirmed, and wider than reported. Down with the list open renames the page to French at +51.9 ms, before Orca even says 'français (fr-FR)'. Down on the closed combo box does the same: it opens the list and switches in one keystroke. Escape does not take the switch back: after Escape the tree still holds 'Bonjour, Alice !' and not 'Hello, Alice!'. A reader who only arrows to hear the options therefore changes the application language, possibly to RTL Arabic, with no cancel. The ComboBox doc (combo\_box.rs:360-373) calls arrow selection a commit by design, so the defect is LanguageSwitcher wiring a context change (set\_locale) to on\_select. Severity medium stands.
- **Fix idea:** Switch the locale only on commit (Enter / click / closing the list), for example with a commit-only callback on ComboBox, not on selection-follows-focus.

### winintl-05 {#winintl-05}

LanguageSwitcher's accessible name is a hardcoded English 'Language', while the UI around it is French or Arabic

- **Example:** internationalization
- **Scenario:** winintl-intl-switch
- **Act:** Switch the app to fr-FR or ar-SA, then Tab to the language combo box.
- **The reader should get:** The combo box is named in the active language ('Langue', 'اللغة'), as ThemeSwitcher ('Thème', 'السمة') and Toolbar ('Barre d'outils', 'شريط الأدوات') are.
- **The reader got (`261a218f`):** 'Language combo box.' in French and in Arabic. The default label and placeholder are lit!("Language"), not a framework translation. The language picker is where a user who does not read the current UI language most needs a name they understand.
- **Platform:** All (tree content); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** In French and in Arabic the language combo box is still named 'Language' and Orca says 'Language combo box.', while the Theme combo box and the toolbar are renamed ('Thème', 'السمة', 'Barre d'outils', 'شريط الأدوات').
- **Measured again:** winintl-intl-switch, winintl-intl-switcher and winintl-intl-tabwalk-ar, 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-switch 'Space on Français': 'FAIL  the tree holds [combo box] 'Langue'' while '+100.2 ms object:property-change:accessible-name [combo box] 'Thème' text='Thème''`
  - `pass1 winintl-intl-switch 'Space on العربية (Arabe)': 'FAIL  the tree holds [combo box] 'اللغة'' while '+177.8 ms object:property-change:accessible-name [combo box] 'السمة' text='السمة''`
  - `pass2 winintl-intl-switch tree after Français: "[label] 'Langue :'" then "[combo box] 'Language' {focusable}"`
  - `pass2 winintl-intl-switcher 'Shift+Tab, Tab back onto the combo box' (app in French): '+747.2 ms ORCA SAYS: 'Language combo box.''`
  - `pass1 winintl-intl-tabwalk-ar 'Tab 12' (app in Arabic): '+82.3 ms ORCA SAYS: 'Language combo box.''`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/language\_switcher.rs:206 and :215
- **Evidence (`261a218f`):**
  - `report (winintl-intl-switch-20260925-142910-514835) 'Space on Français': 'FAIL  the tree holds [combo box] 'Langue'' while '+65.2 ms object:property-change:accessible-name [combo box] 'Thème' text='Thème''`
  - `'Space on العربية (Arabe)': 'FAIL  the tree holds [combo box] 'اللغة'' while '+198.3 ms object:property-change:accessible-name [combo box] 'السمة' text='السمة''`
  - `tree after ar-SA: "[combo box] 'Language' {focusable}" beside "[label] 'اللغة:'"`
  - `winintl-intl-switcher-20260925-143125-582786 after the switch to French: '+719.5 ms ORCA SAYS: 'Language combo box.''`
  - `source: crates/teksilo-widgets/src/language_switcher.rs:206 unwrap_or_else(|| lit!("Language")), :215 .placeholder(lit!("Language"))`
  - `winintl-intl-switch-20260925-145045-970825: 'FAIL the tree holds [combo box] 'Langue'' and 'FAIL the tree holds [combo box] 'اللغة''`
  - `winintl-intl-switcher-20260925-144622-819381 after Enter on français: '+793.5 ms ORCA SAYS: 'Language combo box.''`
- **Reproduced:** 2 of 2 switch runs, 2 of 2 switcher runs; deterministic
- **Verification:** confirmed. Reproduced: 4 of 4 (switch 144131, 145045; switcher 144053, 144622). Deterministic.
- **Fix idea:** Ship a framework\_locales translation for the default label and placeholder, as ThemeSwitcher does. The example could also label it from its visible 'Language:' text (labelled\_by).

### winintl-06 {#winintl-06}

On Wayland, ctx.focus\_window does not raise an open window: F1 with Help already open does nothing and says nothing

- **Example:** multi-window
- **Scenario:** winintl-mw-switch
- **Act:** multi-window: F1 opens Help; KWin activates the main window; F1 again (the example finds the open Help window and calls ctx.focus\_window).
- **The reader should get:** Help comes to the front and is active; the reader hears its focused field.
- **The reader got (`261a218f`):** No window:activate, no focus event and no speech, so the command is silently ignored. Without a token, Teksilo's Wayland raise degrades to request\_user\_attention. The main window that received F1 is focused, so the app could mint an xdg-activation token itself; WindowOps::request\_activation\_token exists for that.
- **Platform:** Linux Wayland only (measured under KWin). X11, Windows and macOS use winit focus\_window.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** F1 with Help already open and the main window active still does nothing a reader can perceive: Orca echoes 'F1' and nothing else. No window:activate, no focus event, and Help does not come to the front.
- **Measured again:** winintl-mw-switch, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-mw-switch 'F1 while Help is open': '+5.7 ms ORCA SAYS: 'F1'' / 'FAIL  a window:activate event reaches the bus' / 'no window:activate in the act' / 'FAIL  focus lands on the entry holding 'editable — select me'' / 'FAIL  Orca says 'editable — select me''`
  - `pass2 winintl-mw-switch 'F1 while Help is open': '+6.7 ms ORCA SAYS: 'F1'' / 'FAIL  a window:activate event reaches the bus' / 'FAIL  Orca says 'editable — select me''`
- **Where (`c198e4d1`):** crates/teksilo-app/src/window\_manager.rs:2207-2213; crates/teksilo-platform/src/window\_activation.rs:32-43; examples/multi\_window/src/main.rs:94-98
- **Evidence (`261a218f`):**
  - `report (winintl-mw-switch-20260925-141756-266840) 'F1 while Help is open': 'FAIL  a window:activate event reaches the bus' / 'no window:activate in the act' / 'FAIL  focus lands on the entry holding 'editable — select me'' / 'FAIL  Orca says 'editable — select me''`
  - `same result in winintl-mw-switch-20260925-141833-275455 and -140736-79225`
  - `source: examples/multi_window/src/main.rs:94-98; crates/teksilo-app/src/window_manager.rs:2207-2213 raise(managed.platform_window.window(), None); crates/teksilo-platform/src/window_activation.rs:32-43 (Wayland with token None -> request_attention); winit-0.30.13/src/platform_impl/linux/wayland/window/mod.rs:629 'pub fn focus_window(&self) {}'`
  - `winintl-mw-switch-20260925-145443-1100821 'F1 while Help is open': 'FAIL a window:activate event reaches the bus' / 'FAIL focus lands on the entry holding 'editable — select me'' / 'FAIL Orca says 'editable — select me''`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 2 of 2 of my mw-switch runs (144237, 145443), plus the sweep's 3. Deterministic.
- **Fix idea:** When the requesting window of the same app is focused, request an xdg\_activation token from it and hand that token to raise() for the target window.

### winintl-07 {#winintl-07}

The multi-window text fields have no accessible name

- **Example:** multi-window
- **Scenario:** winintl-mw-open-close
- **Act:** Launch multi-window (focus lands in the main field); F1 (focus lands in Help's field).
- **The reader should get:** Each entry has a name saying what it is for.
- **The reader got (`261a218f`):** Orca reads 'entry' followed by the field's content: 'entry Select some of this text, then click the Help window → selected.' / 'entry editable — select me, then switch windows selected.'. The launch audit reports the entry as an unnamed control.
- **Platform:** All (tree content); measured on Linux
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Both text fields are still unnamed. Orca reads 'entry' followed by the content: 'entry Select some of this text, then click the Help window → selected.' and 'entry editable — select me, then switch windows selected.'.
- **Measured again:** winintl-mw-open-close, 2 of 2 runs (and every multi-window run)
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-mw-open-close 'launch': 'unnamed-control: [entry] '': a focusable entry with no name (its text is 'Select some of this text, then click the Help window →')' / '+356.6 ms ORCA SAYS: 'entry Select some of this text, then click the Help window → selected.''`
  - `pass1 winintl-mw-open-close 'F1 opens the Help window': '+230.3 ms ORCA SAYS: 'entry editable — select me, then switch windows selected.''`
  - `pass2 winintl-mw-open-close 'launch': same tree audit line; 'F1 opens the Help window': '+279.1 ms ORCA SAYS: 'entry editable — select me, then switch windows selected.''`
- **Where (`c198e4d1`):** examples/multi\_window/src/main.rs:111-113, :155
- **Evidence (`261a218f`):**
  - `launch audit (winintl-mw-open-close-20260925-140600-37697): 'unnamed-control: [entry] '': a focusable entry with no name (its text is 'Select some of this text, then click the Help window →')'`
  - `'+357.6 ms ORCA SAYS: 'entry Select some of this text, then click the Help window → selected.''`
  - `'+259.2 ms ORCA SAYS: 'entry editable — select me, then switch windows selected.''`
  - `source: examples/multi_window/src/main.rs:111-113 and :155 TextInput::new(...) with no label`
  - `winintl-mw-open-close-20260925-144313-819381 audit: 'unnamed-control: [entry] '': a focusable entry with no name'`
- **Reproduced:** every launch and every Help open (3 of 3 open-close runs)
- **Verification:** confirmed. Reproduced: Every multi-window launch of mine (mw-open-close 144313, mw-switch ×2, mw-tabwalk, crash ×2) and every Help open
- **Fix idea:** Give both TextInputs a label (or .access\_label(tr!(...))).

### winintl-08 {#winintl-08}

The price and count buttons ('− 100', '+ 100', '− 1', '+ 1') do not say what they change, and pressing them is silent

- **Example:** internationalization
- **Scenario:** winintl-intl-values
- **Act:** internationalization: from Trailing, Tab to − 100 and + 100, then Space on + 100 (in en-US, and again after switching to French).
- **The reader should get:** The buttons name the quantity they change (for example 'Decrease price by 100'), and the reader learns the new price.
- **The reader got (`261a218f`):** '− 100 push button.' / '+ 100 push button.' / '− 1 push button.' with no mention of price or count. The visible 'Price:' and 'Count:' labels are not related to the buttons. After Space, four labels are renamed ('$1,334.56', '66.7%' …) but nothing is live and Orca says nothing.
- **Platform:** All (tree content); measured on Linux
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** The buttons still say only '− 100 push button.', '+ 100 push button.', '− 1 push button.'. Space on + 100 renames four labels and Orca says nothing, in English and in French.
- **Measured again:** winintl-intl-values, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-values 'Tab to − 100': '+141.8 ms ORCA SAYS: '− 100 push button.'' / 'FAIL  Orca says 'Price''`
  - `pass1 winintl-intl-values 'Space on + 100': '+34.2 ms object:property-change:accessible-name [label] '$1,334.56' text='$1,334.56'' / 'FAIL  Orca says something' / 'Orca said nothing'`
  - `pass2 winintl-intl-values 'Space on + 100 in French': '+38.5 ms object:property-change:accessible-name [label] '1 434,56\xa0$US' text='1 434,56\xa0$US'' / 'FAIL  Orca says something' / 'Orca said nothing'`
- **Where (`c198e4d1`):** examples/internationalization/src/main.rs:274-330
- **Evidence (`261a218f`):**
  - `report (winintl-intl-values-20260925-142114-347797) 'Tab to − 100': '+86.4 ms ORCA SAYS: '− 100 push button.'' / 'FAIL  Orca says 'Price'' / 'Orca unheard: 'Price''`
  - `'Space on + 100': '+31.6 ms object:property-change:accessible-name [label] '$1,334.56' text='$1,334.56'' / 'FAIL  Orca says something' / 'Orca said nothing'`
  - `'Space on + 100 in French': '+32.3 ms object:property-change:accessible-name [label] '1 434,56\xa0$US' text='1 434,56\xa0$US'' / 'FAIL  Orca says something'`
  - `source: examples/internationalization/src/main.rs:274-330 (lit!("− 100") … beside a separate price_label/count_label TextWidget, no labelled_by/described_by, no announce)`
  - `winintl-intl-values-20260925-144343-819381 'Space on + 100': 'FAIL Orca says something' / 'Orca said nothing'; 'Space on + 100 in French': same`
- **Reproduced:** 1 run, 2 presses (en-US, fr-FR); deterministic
- **Verification:** confirmed. Reproduced: 1 of 1 of mine (values 144343), 2 presses (en-US, fr-FR); deterministic
- **Fix idea:** Give the buttons names that include the quantity (tr!), relate them to their label, and ctx.announce the new value, or make the cart summary a polite live region.

### winintl-09 {#winintl-09}

After a language switch the Signal-side Currency row keeps USD while the bundle row shows EUR or SAR, so the reader gets two currencies

- **Example:** internationalization
- **Scenario:** winintl-intl-switch
- **Act:** Switch to fr-FR, then ar-SA.
- **The reader should get:** The Currency row shows the same currency as 'Total (bundle)' (EUR in fr-FR, SAR in ar-SA), as the example's own comment says it does.
- **The reader got (`261a218f`):** fr-FR: 'Total (bundle) : 1 234,56 €' next to Currency '1 234,56 $US'. ar-SA: 'الإجمالي: ١٬٢٣٤٫٥٦ ر.س.' next to '١٬٢٣٤٫٥٦ US$'. The currency code is chosen once in build(), and set\_locale no longer rebuilds, contrary to the example's stale comments.
- **Platform:** All (content); measured on Linux
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** After the switch to French the page shows 'Total (bundle) : 1 234,56 €' beside Currency '1 234,56 $US'; in Arabic 'ر.س.' beside 'US$'. The reader still gets two currencies for one amount.
- **Measured again:** winintl-intl-switch, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-switch 'Space on Français': '+106.8 ms object:property-change:accessible-name [label] 'Total (bundle) : 1 234,56\xa0€'' and '+107.1 ms object:property-change:accessible-name [label] '1 234,56\xa0$US''`
  - `pass1 winintl-intl-switch 'Space on العربية (Arabe)': '+177.2 ms object:property-change:accessible-name [label] '‏١٬٢٣٤٫٥٦\xa0US$'' and '+177.4 ms object:property-change:accessible-name [label] 'الإجمالي: ‏١٬٢٣٤٫٥٦\xa0ر.س.‏''`
  - `pass2 winintl-intl-switch notes: "after العربية: [label] '‏١٬٢٣٤٫٥٦\xa0US$' language='ar-SA'"`
- **Where (`c198e4d1`):** examples/internationalization/src/main.rs:242-245, :416-427; crates/teksilo-core/src/widget\_tree.rs:1456-1464 (set\_locale marks dirty, no rebuild)
- **Evidence (`261a218f`):**
  - `report (winintl-intl-switch-20260925-142910-514835): '+59.8 ms object:property-change:accessible-name [label] 'Total (bundle) : 1 234,56\xa0€'' and '+62.5 ms object:property-change:accessible-name [label] '1 234,56\xa0$US''`
  - `ar-SA: '+192.2 ms object:property-change:accessible-name [label] '‏١٬٢٣٤٫٥٦\xa0US$''`
  - `source: examples/internationalization/src/main.rs:242-245 .currency(per_locale_currency(ctx)) evaluated in build(); :411-427 (comment: 'Matches the bundle-currency-row choice'); crates/teksilo-core/src/widget_tree.rs:1413-1428 set_locale marks dirty and does not rebuild; stale comments at main.rs:26-31 and :202-206`
  - `winintl-intl-switch-20260925-145045-970825 notes: "after Français: [label] '1 234,56\xa0$US'", "after العربية: [label] '‏١٬٢٣٤٫٥٦\xa0US$'"`
- **Reproduced:** 3 of 3 runs that switched language (135858, 142910, values 142114); deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 of my runs that switched to French (switch 144131, 145045; values 144343); both switch runs for Arabic
- **Fix idea:** Derive the currency code from the locale signal (for example a Signal-bound formatter rebuilt in an effect) instead of reading it once in build().

### winintl-10 {#winintl-10}

The layout-direction note stays in English after switching to French, while tagged fr-FR

- **Example:** internationalization
- **Scenario:** winintl-intl-switch
- **Act:** Space on Français (en-US → fr-FR, same direction).
- **The reader should get:** 'Direction de la mise en page : de gauche à droite'.
- **The reader got (`261a218f`):** 'Layout direction: Left to Right' stays, and its language attribute says fr-FR. The note is a frozen tr!(...).resolve\_now() string that is re-set only when the direction changes, so it is translated only on a switch to or from Arabic.
- **Platform:** All (content); measured on Linux
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** After the switch to French the note still reads 'Layout direction: Left to Right', tagged fr-FR. It changes only on a switch to or from Arabic.
- **Measured again:** winintl-intl-switch, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-switch notes: "after Français: [label] 'Layout direction: Left to Right' language='fr-FR'"`
  - `pass1 winintl-intl-switch 'Space on Français': 'missing 'Direction de la mise en page : de gauche à droite''`
  - `pass2 winintl-intl-switch 'Space on Français': 'missing 'Direction de la mise en page : de gauche à droite''`
- **Where (`c198e4d1`):** examples/internationalization/src/main.rs:99-106, :433-438
- **Evidence (`261a218f`):**
  - `report (winintl-intl-switch-20260925-142910-514835) 'Space on Français': 'missing 'Direction de la mise en page : de gauche à droite''`
  - `run note: "after Français: [label] 'Layout direction: Left to Right' language='fr-FR'"`
  - `source: examples/internationalization/src/main.rs:99-106 (ctx.effect on the direction signal only), :433-438 (tr!(direction_note_ltr()).resolve_now())`
- **Reproduced:** 2 of 2 runs
- **Verification:** confirmed. Reproduced: 2 of 2 switch runs
- **Fix idea:** Bind the note to a LocalizedString signal (tr\_signal! or a map over the direction plus the i18n version) instead of a resolved String.

### winintl-11 {#winintl-11}

Untranslated literals in the internationalization example are tagged with the active language, and the window title is never translated

- **Example:** internationalization
- **Scenario:** winintl-intl-switch
- **Act:** Switch to fr-FR and ar-SA; read the formatting rows.
- **The reader should get:** 'Decimal:', 'Currency:', 'Percent:' and 'Date:' are translated (or marked as English), and the window title follows the locale (each .ftl defines window-title).
- **The reader got (`261a218f`):** The four row labels stay in English and carry language fr-FR / ar-SA, so a voice that switched by language would read English in a French or Arabic voice. The window title is hardcoded 'Teksilo — Internationalization Demo' and the window-title keys are unused, which matters once K1 names windows.
- **Platform:** All (content); measured on Linux
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** 'Decimal:', 'Currency:', 'Percent:' and 'Date:' stay in English and carry language fr-FR or ar-SA. The window is still named 'Teksilo — Internationalization Demo' in every language, and since windows are now named by their title, that English name reaches the reader.
- **Measured again:** winintl-intl-switch, 2 of 2 runs; winintl-intl-switcher, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-switch notes: "after Français: [label] 'Decimal:' language='fr-FR'", "after Français: [label] 'Currency:' language='fr-FR'", "after العربية: [label] 'Decimal:' language='ar-SA'"`
  - `pass2 winintl-intl-switch notes: "after العربية: [label] 'Currency:' language='ar-SA'"`
  - `pass2 winintl-intl-switcher 'Enter picks it' (the app now in French): '+1154.7 ms ORCA SAYS: 'Teksilo — Internationalization Demo''`
- **Where (`c198e4d1`):** examples/internationalization/src/main.rs:258-261, :393, :535
- **Evidence (`261a218f`):**
  - `run notes (winintl-intl-switch-20260925-142910-514835): "after العربية: [label] 'Decimal:' language='ar-SA'", "after Français: [label] 'Currency:' language='fr-FR'"`
  - `source: examples/internationalization/src/main.rs:258-261 formatting_row(ctx, "Decimal:", …) → lit!(label); :535 .title("Teksilo — Internationalization Demo"); locales/*.ftl window-title unused`
- **Reproduced:** 2 of 2 runs; deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 switch runs; deterministic

### winintl-12 {#winintl-12}

Switching to French with the Français button is silent

- **Example:** internationalization
- **Scenario:** winintl-intl-switch
- **Act:** Space on the focused 'Français' button.
- **The reader should get:** The reader learns that the interface language changed.
- **The reader got (`261a218f`):** Nothing is spoken. The focused button is still named 'Français', no focus event occurs, and none of the renamed labels is live. The switches to Arabic and English are 'confirmed' only because the focused button's own name changes ('العربية', 'English'). No message in either example goes through ctx.announce.
- **Platform:** Linux / Orca (measured); by construction on all platforms
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Space on the focused 'Français' button renames the page and Orca says nothing. The switches to Arabic and English are heard only because the focused button's own name changes ('العربية', 'English').
- **Measured again:** winintl-intl-switch, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-switch 'Space on Français': 'FAIL  Orca says something' / 'Orca said nothing'`
  - `pass2 winintl-intl-switch 'Space on Français': 'FAIL  Orca says something', no ORCA SAYS line in the act`
  - `pass1 winintl-intl-switch 'Space on العربية (Arabe)': '+283.0 ms ORCA SAYS: 'العربية''; 'Space on English (الإنجليزية)': '+107.0 ms ORCA SAYS: 'English''`
- **Where (`c198e4d1`):** examples/internationalization/src/main.rs:144-148
- **Evidence (`261a218f`):**
  - `report (winintl-intl-switch-20260925-142910-514835) 'Space on Français': 'FAIL  Orca says something' (no ORCA SAYS line in the act; the 42 name/text changes are on the bus)`
  - `'Space on العربية (Arabe)': '+361.1 ms ORCA SAYS: 'العربية''; 'Space on English': '+82.1 ms ORCA SAYS: 'English''`
- **Reproduced:** 2 of 2 runs (135858, 142910)
- **Verification:** confirmed. Reproduced: 2 of 2 of my switch runs (plus the sweep's 2)
- **Fix idea:** ctx.announce the new language (in that language) after set\_locale. This path is covered by the K2 announcer fix.

### winintl-13 {#winintl-13}

Every TextInput carries an empty, unnamed status node

- **Example:** multi-window
- **Scenario:** winintl-mw-open-close
- **Act:** Launch multi-window; F1 opens Help.
- **The reader should get:** No empty node while there is no validation message.
- **The reader got (`261a218f`):** An empty '\[status bar\] ''' follows each entry in both windows, a stop that says nothing to anyone walking the tree (flat review, object navigation).
- **Platform:** All (tree content); measured on Linux
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** An empty, unnamed '\[status bar\] ''' still follows the entry in both windows.
- **Measured again:** winintl-mw-open-close, 2 of 2 runs (and every multi-window run)
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-mw-open-close 'launch': '+248.4 ms object:children-changed:add [frame] 'Teksilo — Multi-Window Demo' -> [status bar] '''`
  - `pass1 winintl-mw-open-close 'F1 opens the Help window': '+99.1 ms object:children-changed:add [frame] 'Teksilo — Help' -> [status bar] '''`
  - `pass2 winintl-mw-open-close tree after F1: "[entry] '' {editable,focusable,focused,selectable-text,single-line} text='editable — select me, then switch windows'" / "[status bar] ''"`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/primitives/validation\_strip.rs:148-165
- **Evidence (`261a218f`):**
  - `report (winintl-mw-open-close-20260925-140600-37697): '+250.6 ms object:children-changed:add [frame] '' -> [status bar] ''' and '+113.3 ms object:children-changed:add [frame] '' -> [status bar] '''`
  - `tree after F1: "[entry] '' {editable,focusable,focused,selectable-text,single-line} text='editable — select me, then switch windows'" / "[status bar] ''"`
  - `source: crates/teksilo-widgets/src/primitives/validation_strip.rs:148-165 (Role::Status always emitted; empty with Live::Off when Pristine/Valid)`
- **Reproduced:** every run
- **Verification:** confirmed. Reproduced: Every multi-window run
- **Fix idea:** Hide the strip from AT while it has no message (access\_hidden), keeping the live-region node alive in a way that still announces when a message appears.

### winintl-14 {#winintl-14}

The default Toolbar name repeats its role: 'Toolbar tool bar'

- **Example:** internationalization
- **Scenario:** winintl-intl-tabwalk-ar
- **Act:** Tab into the toolbar's Theme combo box (both examples).
- **The reader should get:** A toolbar with no meaningful name is announced once as a tool bar, or has a name that adds information.
- **The reader got (`261a218f`):** Orca says 'Toolbar tool bar' before the combo box (in Arabic, 'شريط الأدوات tool bar').
- **Platform:** Linux / Orca (measured)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Tabbing into the toolbar's Theme combo box still says 'Toolbar tool bar' first ('شريط الأدوات tool bar' in Arabic). The same double role is heard each time focus returns to the Theme combo box from its list.
- **Measured again:** verify-winintl-mw-tabwalk, winintl-intl-tabwalk-ar and verify-winintl-intl-combo-reopen, 2 of 2 runs each
- **Evidence (`c198e4d1`):**
  - `pass1 verify-winintl-mw-tabwalk 'Tab 3': '+679.0 ms ORCA SAYS: 'Toolbar tool bar'' / '+679.1 ms ORCA SAYS: 'Theme combo box.''`
  - `pass2 verify-winintl-mw-tabwalk 'Tab 3': '+217.2 ms ORCA SAYS: 'Toolbar tool bar''`
  - `pass2 winintl-intl-tabwalk-ar 'Tab 8': '+98.7 ms ORCA SAYS: 'شريط الأدوات tool bar'' / '+98.7 ms ORCA SAYS: 'السمة combo box.''`
  - `pass2 verify-winintl-intl-combo-reopen 'Escape closes the list (1)': '+133.2 ms ORCA SAYS: 'Toolbar tool bar''`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/toolbar.rs:702 (the label override's doc), default name at crates/teksilo-widgets/src/toolbar.rs:1163 (tr\_widget!(a11y\_toolbar\_name))
- **Evidence (`261a218f`):**
  - `winintl-intl-tabwalk-ar-20260925-141917-286986 Tab 8: "ORCA SAYS: 'شريط الأدوات tool bar'" then "ORCA SAYS: 'السمة combo box.'"`
  - `tabwalk-multi-window (previous sweep): 'ORCA SAYS: 'Toolbar tool bar''`
  - `source: crates/teksilo-widgets/src/toolbar.rs:702 (default accessible name: the localized "Toolbar")`
- **Reproduced:** every Tab into the toolbar
- **Verification:** confirmed. Reproduced: verify-winintl-mw-tabwalk 144934 Tab 3, winintl-intl-tabwalk-ar 145010 Tab 8
- **Fix idea:** Leave the toolbar unnamed by default (the role says it), or have examples name it by purpose.

### winintl-15 {#winintl-15}

Content outside the ScrollArea viewport is missing from the tree, and its text changes still produce events Orca drops

- **Example:** internationalization
- **Scenario:** winintl-intl-switch
- **Act:** Launch internationalization (520 px), read the tree; switch language.
- **The reader should get:** A reader can reach every line of the page (the date value, cart summary, price and count rows), or at least those events are not sent for nodes that are not exported.
- **The reader got (`261a218f`):** At launch the scroll pane lists 22 of its 30 children: everything after 'Date:' is missing. Once Tab has scrolled down, the heading, greeting and intro leave the tree and stay out. Labels are not focusable, so Tab cannot bring them back, and Orca cannot scroll. On each language switch the adapter sends text-changed events for the unexported labels, and Orca logs 'Unknown object' then drops 8 events as '\[DEAD\]'.
- **Platform:** Linux AT-SPI (measured); the filter is in accesskit\_consumer, shared by all adapters
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** At launch the scroll pane still lists 22 children and ends at 'Date:'; the date value, the cart summary and the price and count rows are missing until Tab scrolls to them, and labels that scroll out stay out. On every language switch Orca still drops 8 events as coming from '\[DEAD\]'.
- **Measured again:** winintl-intl-switch, 2 of 2 runs; winintl-intl-tabwalk-ar, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass2 winintl-intl-switch tree-launch.txt: the scroll pane's last child is "[label] 'Date:'"`
  - `pass1 winintl-intl-switch 'Space on Français': 'observed  Orca ignored an event whose source was defunct' / '16:54:43.457005 EVENT MANAGER: Ignoring defunct object: [DEAD]' (8 times)`
  - `pass1 winintl-intl-switch 'Space on العربية (Arabe)': '16:54:55.149910 EVENT MANAGER: Ignoring defunct object: [DEAD]' (8 times)`
  - `pass2 winintl-intl-tabwalk-ar 'Space on العربية': 8 × 'observed  Orca ignored an event whose source was defunct'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/scroll\_area.rs:1305; accesskit\_consumer-0.39.1/src/filters.rs:54-88; accesskit\_atspi\_common-0.21.0/src/adapter.rs:286-290 and :184-194 (unchanged from 0.20.0)
- **Evidence (`261a218f`):**
  - `tree (tree-internationalization-20260925-135413-4057835): panel '' child_count 22, last child "label 'Date:'" (the date value, '3 items at …', 'Price:', '− 100', '+ 100', 'Count:', '− 1', '+ 1' absent)`
  - `report (winintl-intl-switch-20260925-142910-514835): '+60.5 ms object:text-changed:delete [<Error>] '' text='May 4,''`
  - `orca-debug.out: "14:29:23.101313 - AXObject: Exception in get_role_name: atspi_error: Unknown object '/org/a11y/atspi/accessible/0/79228164137577816079984492544' (1)" and '14:29:23.182838 - EVENT MANAGER: Ignoring defunct object: [DEAD]'`
  - `source: accesskit_consumer-0.39.0/src/filters.rs:56-85; accesskit_atspi_common-0.20.0/src/adapter.rs:287-290 (node_updated emits text changes before checking the filter); crates/teksilo-widgets/src/scroll_area.rs:1305`
  - `verify-winintl-intl-heading-defunct-20260925-144907-970825 'AT-SPI click on Français': '14:49:31.639973 EVENT MANAGER: Ignoring defunct object: [label]', '14:49:31.614538 … [DEAD]'`
- **Reproduced:** every run
- **Verification:** confirmed. Reproduced: Every internationalization run
- **Fix idea:** Same lever as winintl-01: not setting clips\_children keeps the whole page in the tree.

### winintl-16 {#winintl-16}

The declared language reaches Orca 46.1 only as a text attribute it does not use; the AT-SPI object Locale is always empty

- **Example:** internationalization
- **Scenario:** winintl-intl-switch
- **Act:** Launch, then switch to fr-FR and ar-SA; read each node's locale and its `language` text attribute.
- **The reader should get:** A reader's speech follows the UI language: Arabic names in an Arabic voice.
- **The reader got (`261a218f`):** The `language` text attribute is right on every Text node (en-US, then fr-FR, then ar-SA, updated live). The AT-SPI object Locale is '' on every node, because accesskit\_unix hardcodes it. Orca 46.1 asks for every name's voice with language='None' and has voice switching disabled in code, so Arabic names and digits are sent to the default voice.
- **Platform:** Linux / Orca 46.1 (measured). By source, Windows exposes UIA Culture per node (accesskit\_windows node.rs:424, 1312) and macOS exposes NSAccessibilityLanguageTextAttribute on text (accesskit\_macos node.rs:929-932); NVDA and VoiceOver use of these was not verified.
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** The language text attribute is right and follows each switch (en-US, fr-FR, ar-SA), but the AT-SPI object Locale is still '' on every node, and Orca 46.1 asks for every voice with language 'None', so Arabic names go to the default voice.
- **Measured again:** winintl-intl-switch, 2 of 2 runs; winintl-intl-tabwalk-ar, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - ``pass1 winintl-intl-switch notes: "at launch: `language` text attributes on Text nodes: ['en-US']; object locales: ['']", "after Français: `language` text attributes on Text nodes: ['fr-FR']; object locales: ['']", "after العربية: `language` text attributes on Text nodes: ['ar-SA']; object locales: ['']"``
  - `pass2 winintl-intl-tabwalk-ar orca-debug.out: 46 × "voice requested with language='None'" and no other language`
  - ``source: accesskit_unix-0.24.0/src/atspi/interfaces/accessible.rs:65-67 `fn locale(&self) -> &str { "" }`; Orca speech_generator.py:2786-2788 checkVoicesForLanguage = False``
- **Where (`c198e4d1`):** accesskit\_unix-0.24.0/src/atspi/interfaces/accessible.rs:65-67; Orca speech\_generator.py:2777-2789; crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:75-77 (root.set\_language)
- **Evidence (`261a218f`):**
  - ``run notes: "at launch: `language` text attributes on Text nodes: ['en-US']; object locales: ['']", "after Français: … ['fr-FR']; object locales: ['']", "after العربية: … ['ar-SA']; object locales: ['']"``
  - `orca-debug.out (all internationalization runs): only "voice requested with language='None', dialect=''" (e.g. 32 in winintl-intl-tabwalk-ar-20260925-141917-286986)`
  - ``source: accesskit_unix-0.23.0/src/atspi/interfaces/accessible.rs:64-67 `fn locale(&self) -> &str { "" }`; accesskit_atspi_common-0.20.0/src/text_attributes.rs:52-54,77; crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:46-54 (root.set_language); Orca speech_generator.py:2777-2789 (checkVoicesForLanguage = False: 'The code needed to actually switch voices does not yet exist')``
- **Reproduced:** every internationalization run
- **Verification:** confirmed. Reproduced: Every internationalization run

### winintl-17 {#winintl-17}

winit 0.30.13 panics ('failed to get pointer data') when a window opens while the harness's fake-input device comes and goes

- **Example:** multi-window
- **Scenario:** winintl-mw-crash
- **Act:** multi-window: open and close Help several times with real keys (F1, Space/Enter on buttons) through fake\_key.
- **The reader should get:** The application keeps running.
- **The reader got (`261a218f`):** The application panics in winit's Wayland pointer code and exits with status 101, taking both windows away. Each fake\_key run adds and removes a KWin fake-input device, and in the private session the seat's pointer capability flaps with it. With one fake-input client held open for the whole run, the same sequence never crashed. On a normal desktop the pointer capability rarely flaps, so reader impact outside the harness is unproven.
- **Platform:** Linux Wayland (KWin --virtual)
- **Severity:** low; **layer:** harness
- **Status:** A harness defect, fixed in the harness.
- **Now (`c198e4d1`):** The application kept running through the whole open and close sequence in every run, with the seat left to flap and with it held steady. No panic was logged.
- **Measured again:** winintl-mw-crash, 0 of 3 runs crashed; winintl-mw-crash-steady, 0 of 2 runs crashed
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-mw-crash: app.log has no 'panicked'; 'Enter on Open help' / 'the application is still there' ran to the end`
  - `pass2 winintl-mw-crash: app.log has no 'panicked'; the run reached 'the application is still there'`
  - `extra run winintl-mw-crash-20260927-180333-2119203: no 'panicked', 'the application is still there' reached`
  - `pass1 and pass2 winintl-mw-crash-steady: no 'panicked'`
- **Where (`c198e4d1`):** winit-0.30.13/src/platform\_impl/linux/wayland/seat/pointer/mod.rs:46, :407-409
- **Evidence (`261a218f`):**
  - `app.log (winintl-mw-crash-20260925-141123-157306): "thread 'main' (157545) panicked at ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-0.30.13/src/platform_impl/linux/wayland/seat/pointer/mod.rs:409:41:" / "failed to get pointer data."`
  - `report: '== F1 again  (Help opens)' / 'COULD NOT RUN:  the application exited (101)'`
  - `also crashed: winintl-mw-open-close-20260925-140458-21440 (first F1), winintl-mw-focus-return-20260925-140833-98799 (Space on Open help), previous sweep winintl-mw-reopen (Enter on Open help)`
  - `winintl-mw-crash-steady ×3 (141501, 141557, 141653): 'no panic'`
  - `source: winit-0.30.13 …/wayland/seat/pointer/mod.rs:46 pointer.winit_data(), :407-409 .expect("failed to get pointer data.")`
- **Reproduced:** Seat left to flap: 2 of 3 runs of the sequence crashed, plus 2 crashes in 6 other window opens. Seat held steady: 0 of 3.
- **Verification:** corrected by the verifier. Reproduced: My runs: 0 of 2 crashed with the seat left to flap (winintl-mw-crash 145348, 145612; 4 window opens each, all passed). Sweep: 2 of 3. Combined: 2 of 5 flapping-seat runs, 0 of 3 steady (sweep only). The crash is real. I read both sweep app.logs: "panicked at …/wayland/seat/pointer/mod.rs:409:41: failed to get pointer data.", exit 101. The source is an .expect() on WlPointer user data. My two reruns of the identical flapping sequence did not crash, so the rate is lower than reported (2 of 5 overall). The link to the fake-input seat flapping is plausible, but 2/5 against 0/3 does not prove it. Harness-triggered, upstream winit bug; reader impact outside the harness is unproven.
- **Fix idea:** Harness: keep one fake-input client connected for the whole run (windows\_i18n.py SteadySeat does this for its multi-window scenarios). Upstream winit should not expect() pointer user data.

### winintl-v-01 {#winintl-v-01}

Every ComboBox (ThemeSwitcher, LanguageSwitcher) is silent from its second opening on: Orca drops its list box and rows as defunct

- **Example:** multi-window, internationalization
- **Act:** internationalization: AT focus on the Theme combo box; Alt+Down, Down, Escape; repeated three times (verify-winintl-intl-combo-reopen).
- **The reader should get:** Each opening says 'List with 3 items' and the current row, and each Down says the new row, as the first opening does.
- **The reader got (`261a218f`):** First opening: 'List with 3 items', 'Light.', then 'Dark.'. Escape sends object:state-changed:defunct 1 for the list box, every row and the \[unknown\] wrapper. The second and third openings re-add the same paths (children-changed:add) and Orca ignores the list box and rows as defunct, so nothing is spoken, open or arrowing. The dropdown is built once, kept dormant and shown with visible\_when(is\_open), so every opening reuses the node ids the adapter has already declared defunct. The K2 fix (fresh ids for announcer nodes) does not reach it. Together with winintl-03 (a closed combo's value is not exposed on AT-SPI), a Linux reader cannot learn the options or the value of any combo box after its first use.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The defunct state is AT-SPI-specific; UIA and macOS not measured.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** Every opening of the Theme combo box is spoken: 'List with 3 items', 'Light.', then 'Dark.' on Down, on the first, second and third opening alike. The list still goes defunct on Escape, but each opening now comes back under new ids.
- **Measured again:** verify-winintl-intl-combo-reopen, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass2 verify-winintl-intl-combo-reopen 'Alt+Down opens the list (2)': '+90.9 ms ORCA SAYS: 'List with 3 items'' / '+90.9 ms ORCA SAYS: 'Light.''; 'Down to the next row (2)': '+148.6 ms ORCA SAYS: 'Dark.''`
  - `pass2 verify-winintl-intl-combo-reopen 'Alt+Down opens the list (3)': '+94.2 ms ORCA SAYS: 'List with 3 items'' / 'Light.'; 'Down to the next row (3)': '+119.4 ms ORCA SAYS: 'Dark.''`
  - `pass1 verify-winintl-intl-combo-reopen 'Down to the next row (2)': '+2686.2 ms ORCA SAYS: 'Dark.''; opening (3): '+646.9 ms ORCA SAYS: 'List with 3 items''`
  - `pass1 verify-winintl-intl-combo-reopen opening (2): no ORCA SAYS in the act, but orca-debug.out '16:53:12.797106 - SPEECH OUTPUT: 'List with 3 items'' and '16:53:12.802750 - SPEECH OUTPUT: 'Light.'', with no 'Ignoring defunct' for the list`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/combo\_box.rs:852-861 (the panel is still deferred, dormant and shown with visible\_when); the cure: crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-283
- **Evidence (`261a218f`):**
  - `verify-winintl-intl-combo-reopen-20260925-145254-1100821 'Alt+Down opens the list (1)': '+132.0 ms ORCA SAYS: 'List with 3 items'' / 'Light.'; 'Down to the next row (1)': '+201.0 ms ORCA SAYS: 'Dark.''`
  - `same run 'Escape closes the list (1)': '+13.0 ms object:state-changed:defunct 1 [list item] 'Dark'' … '+15.5 ms object:state-changed:defunct 1 [list box] '''`
  - `same run 'Alt+Down opens the list (2)': '+29.1 ms object:children-changed:add [combo box] 'Theme' -> [unknown] ''' / '+29.4 ms object:selection-changed [list box] ''' / no ORCA SAYS / '14:53:15.051691 EVENT MANAGER: Ignoring defunct object: [list box]'`
  - `same run 'Down to the next row (2)': 'object:state-changed:selected 1 [list item] 'System'' / no ORCA SAYS / '14:53:19.426419 EVENT MANAGER: Ignoring defunct object: [list item: 'Dark']'`
  - `same run '(3)': '14:53:27.244053 EVENT MANAGER: Ignoring defunct object: [list box]', no speech`
  - `source: crates/teksilo-widgets/src/combo_box.rs:15-16 (panel created in build and kept dormant), :791-801 (set_dormant + visible_when(dropdown_id, is_open)); accesskit_atspi_common-0.20.0/src/adapter.rs:91-106; Orca event_manager.py:796-798`
- **Reproduced:** 2 of 2 runs (145254, 145519), openings 2 and 3 silent in both; deterministic
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Give the popup subtree fresh AccessKit ids per opening (as the K2 fix did for the announcer), or keep it exported while closed. The generic fix is in the AT-SPI path: see winintl-v-03. Every popup kept dormant and re-shown (menus, popovers) is likely affected the same way.

### winintl-v-02 {#winintl-v-02}

The 'Total (bundle)' row is frozen at the launch price: after + 100 the page states two different totals

- **Example:** multi-window, internationalization
- **Act:** internationalization: Space on + 100 (twice), switching to French in between (winintl-intl-values).
- **The reader should get:** 'Total (bundle)' follows the price, or at least agrees with the other rows after a language switch, as the example's comment says ('update on locale flips (composite rebuild re-evaluates tr!)').
- **The reader got (`261a218f`):** After the presses the tree holds 'Total (bundle) : 1 234,56 €' beside '1 434,56', '1 434,56 $US' and '3 articles à 1 434,56 pièce'. tr!(bundle\_currency\_row(price = self.price.get())) captures the launch value, and set\_locale never rebuilds Root, so the row can never change its number. Same root cause as winintl-09.
- **Platform:** All (content); measured on Linux
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** After + 100 the page states two totals: 'Total (bundle) : 1 234,56 €' beside '1 334,56', '1 334,56 $US' and '3 articles à 1 334,56 pièce', and after the second press the other rows reach 1 434,56 while the bundle row stays at the launch price.
- **Measured again:** winintl-intl-values, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-values 'AT-SPI click on Français (focus stays on + 100)': '+86.2 ms object:property-change:accessible-name [label] '3 articles à 1 334,56 pièce'' and '+90.8 ms object:property-change:accessible-name [label] 'Total (bundle) : 1 234,56\xa0€''`
  - `pass1 winintl-intl-values 'Space on + 100 in French': '+37.5 ms object:property-change:accessible-name [label] '1 434,56\xa0$US'', no change to the bundle row`
  - `pass2 winintl-intl-values 'AT-SPI click on Français (focus stays on + 100)': '+97.1 ms object:property-change:accessible-name [label] 'Total (bundle) : 1 234,56\xa0€'' and '+98.2 ms object:property-change:accessible-name [label] '3 articles à 1 334,56 pièce''`
- **Where (`c198e4d1`):** examples/internationalization/src/main.rs:227-231; crates/teksilo-core/src/widget\_tree.rs:1456-1464
- **Evidence (`261a218f`):**
  - `winintl-intl-values-20260925-144343-819381 'Space on + 100 in French', recorded labels: "'Total (bundle) : 1 234,56\xa0€'", "'1 434,56'", "'1 434,56\xa0$US'", "'3 articles à 1 434,56 pièce'"`
  - `source: examples/internationalization/src/main.rs:227-231 (tr! with price.get()), :201-206 (stale comment), crates/teksilo-core/src/widget_tree.rs:1419-1427`
- **Reproduced:** 1 of 1 run; deterministic
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Use tr\_signal! with the price signal for the bundle row, or drop the claim that it follows locale flips.

### winintl-v-03 {#winintl-v-03}

libatspi rejects every Cache.AddAccessible signal AccessKit sends (wrong D-Bus signature), so a re-added node is never refreshed in the reader's AT-SPI cache

- **Example:** multi-window, internationalization
- **Act:** Any run: the listener (libatspi 2.52, as in Orca) warns on every node AccessKit adds.
- **The reader should get:** libatspi processes AddAccessible and updates the cached object: role, name, states (clearing a stale defunct).
- **The reader got (`261a218f`):** 'AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau' is logged 142 times in 4 runs. accesskit\_unix emits the CacheItem struct's fields as top-level arguments. libatspi accepts only a single struct argument ('((so)(so)(so)iiassusau)' or '((so)(so)(so)a(so)assusau)', both in libatspi.so.0) and returns. Inferred consequence, consistent with every defunct observation here: a node re-added under a path the client holds keeps its cached defunct state (the heading's states list 'defunct' after it re-enters; K2, winintl-01, winintl-v-01). The libatspi-internal step is inferred, not read in source (at-spi2-core source is not on this machine).
- **Platform:** Linux AT-SPI (measured warnings); every Teksilo app
- **Severity:** medium; **layer:** upstream
- **Severity in the sweep:** high. The one consequence measured, a stale defunct state on a node that came back, is now avoided by Teksilo's new-id workaround (85624a1a). The refused signals remain, so any other cached state they would carry is still lost.
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Now (`c198e4d1`):** libatspi still refuses every AddAccessible and RemoveAccessible cache signal the application sends, with the same signature warning. Teksilo now hands a node that comes back a new id, so the stale defunct state this used to leave in the reader's cache no longer appeared in any run here.
- **Measured again:** every run in the group, 2 of 2 passes
- **Evidence (`c198e4d1`):**
  - `pass1 winintl-intl-scroll-focus-back report: 'libatspi refused the application's cache signals: AddAccessible refused 59x, RemoveAccessible refused 29x (accesskit_unix sends them flattened; see listener.log)'`
  - `pass1 winintl-intl-scroll-focus-back listener.log: '(process:2495954): dbind-WARNING **: 16:54:06.452: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau' (59 lines)`
  - `pass2 winintl-intl-switch listener.log: 26 'unknown signature' warnings`
  - `source: accesskit_unix-0.24.0/src/atspi/bus.rs:441-446 (emit_cache_add), :463-481 (emit_signal with the struct as the body)`
- **Where (`c198e4d1`):** accesskit\_unix-0.24.0/src/atspi/bus.rs:441-481
- **Evidence (`261a218f`):**
  - `batch1.log: 142 lines 'dbind-WARNING **: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau' from the listener processes (Orca's stderr goes to /dev/null)`
  - `strings /usr/lib/x86_64-linux-gnu/libatspi.so.0: '((so)(so)(so)a(so)assusau)', '((so)(so)(so)iiassusau)', 'AT-SPI: AddAccessible with unknown signature %s'`
  - `source: accesskit_unix-0.23.0/src/atspi/bus.rs:434-439 (emit_cache_add → emit_cache_signal("AddAccessible", &item)), :456-473 (zbus emit_signal with the struct as body)`
  - `winintl-intl-scroll-reenter-20260925-145220-970825: re-entered heading 'states=['defunct', 'enabled', 'sensitive', 'showing', 'visible']'`
- **Reproduced:** every run (warnings); the defunct consequence 5 of 5 runs where a node left and came back
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Upstream: wrap the AddAccessible body in a one-field tuple so the signature is '((so)(so)(so)iiassusau)'. Until then Teksilo can avoid re-adding a node under an id it has let leave (fresh ids, or keep the node exported).

### winintl-v-04 {#winintl-v-04}

Language names are declared in the UI language: 'العربية' and 'Français' inherit en-US, and so do the LanguageSwitcher's autonyms

- **Example:** multi-window, internationalization
- **Act:** internationalization at launch (en-US); the LanguageSwitcher list open.
- **The reader should get:** An autonym ('Français', 'العربية', 'français (fr-FR)', 'العربية (ar-SA)') declares its own language, so a reader that switches voice by language reads it in that voice.
- **The reader got (`261a218f`):** Teksilo sets the language only on the root (accessibility\_emit\_impl.rs:53) and AccessKit inherits it (consumer text.rs:1203 via fetch\_inherited\_property), so every autonym is en-US. On AT-SPI these buttons and list items have no Text interface, so no language reaches Orca at all (measured). On Windows each gets UIA Culture en-US (accesskit\_windows node.rs:423-425, by source). No reader was observed switching voice on it (Orca 46.1 cannot switch voices), so impact today is theoretical.
- **Platform:** Linux (measured: no language exposed on controls); Windows by source
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The autonym buttons and list items ('Français', 'العربية', 'français (fr-FR)', 'العربية (ar-SA)') still carry no language of their own and have no Text interface on AT-SPI, so no language reaches Orca for them.
- **Measured again:** winintl-intl-switcher, 2 of 2 runs; winintl-intl-switch, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass2 winintl-intl-switcher run.json: push button 'العربية' interfaces ['Accessible', 'Action', 'Component']; list item 'العربية (ar-SA)' interfaces ['Accessible', 'Component']; list item 'français (fr-FR)' interfaces ['Accessible', 'Component']`
  - ``pass1 winintl-intl-switch note: "at launch: `language` text attributes on Text nodes: ['en-US']" (the one language in the tree)``
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/language\_switcher.rs:208-212
- **Evidence (`261a218f`):**
  - `winintl-intl-switch-20260925-144131-751209 run.json tree: push button 'العربية' interfaces ['Accessible', 'Action', 'Component']; list items 'العربية (ar-SA)', 'français (fr-FR)' interfaces ['Accessible', 'Component'] (switcher 144622)`
  - `source: crates/teksilo-widgets/src/language_switcher.rs:208-212 (items built as literal strings, no language), examples/internationalization/locales/en-US.ftl:16-17`
- **Reproduced:** deterministic (tree content)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Set each LocaleChoice row's language to its tag (and let apps do the same on a button through access\_customize).

### winintl-v-05 {#winintl-v-05}

multi-window's two demo buttons mislead: 'Open help (F1) / Toggle fullscreen (F11)' only opens help, and 'This panel dims…' is a push button that does nothing

- **Example:** multi-window, internationalization
- **Act:** multi-window: Tab to each button; Space on the dimming button.
- **The reader should get:** A push button's name says what activating it does, and a push button does something.
- **The reader got (`261a218f`):** The Open help button only sends ShowHelp. The dimming 'panel' is a focusable push button with no handler: Space produces no event and no speech. A reader meets a tab stop that does nothing.
- **Platform:** All (tree content); measured on Linux
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Orca still reads 'This panel dims when the window is inactive push button.' and 'Open help (F1) / Toggle fullscreen (F11) push button.'. Space on the dimming button does nothing; Orca only echoes the key, 'space'.
- **Measured again:** verify-winintl-mw-tabwalk, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass2 verify-winintl-mw-tabwalk 'Tab 1': '+131.7 ms ORCA SAYS: 'This panel dims when the window is inactive push button.''; 'Tab 2': '+148.1 ms ORCA SAYS: 'Open help (F1) / Toggle fullscreen (F11) push button.''`
  - `pass2 verify-winintl-mw-tabwalk 'Space on the dimming button': '+13.4 ms ORCA SAYS: 'space'', no event on the bus`
  - `pass1 verify-winintl-mw-tabwalk 'Space on the dimming button': '+187.3 ms ORCA SAYS: 'space''`
- **Where (`c198e4d1`):** examples/multi\_window/src/main.rs:158-163, :164-168
- **Evidence (`261a218f`):**
  - `verify-winintl-mw-tabwalk-20260925-144934-970825 Tab 1: 'ORCA SAYS: 'This panel dims when the window is inactive push button.''; 'Space on the dimming button': 'Orca said nothing in this act', 'no focus change', 0 events`
  - `source: examples/multi_window/src/main.rs:158-163 (Button with no on_activate), :164-168 (on_activate sends ShowHelp only)`
- **Reproduced:** 1 of 1; deterministic
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Name the button 'Open help (F1)' and make the dimming demo a non-focusable panel (or give the button an action).
