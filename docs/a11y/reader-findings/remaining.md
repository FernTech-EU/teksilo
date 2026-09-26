<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# The examples swept last

Examples: `simple-button`, `automation_bridge_smoke`, `telemetry-plausible`, `telemetry-teksilo`, `web-view-demo`.
10 findings: 5 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [rest-01](#rest-01) | simple-button | The window opens with nothing focused: the reader hears only the window's title and has to Tab to learn there is a button | low | all | open |
| [rest-02](#rest-02) | simple-button | The debug inspector (F12): closing it from inside drops focus onto the bare window, and several of its controls are unnamed | low | all | open |
| [rest-03](#rest-03) | automation\_bridge\_smoke | 'input-probe' is a push button a reader can neither focus nor activate | low | all | open (example) |
| [rest-04](#rest-04) | telemetry-plausible | Every consent change and every recorded event rebuilds the whole privacy panel, and focus inside it is thrown to the panel's first control | medium | all | open |
| [rest-05](#rest-05) | telemetry-plausible | The consent switch is not described by the line under its label, so moving onto it reads only 'Anonymous usage metrics toggle button' | low | all | open |
| [rest-06](#rest-06) | web-view-demo | A web view whose engine failed reads 'Loading… panel' forever: neither loading nor failure reaches the reader | medium | all | open |
| [rest-07](#rest-07) | web-view-demo | Nothing tells a reader the web view is web content or that Enter goes into the page: it reads as a plain 'panel', and its Enter hint is published where no adapter looks | medium | Linux | open |
| [rest-08](#rest-08) | web-view-demo | The Browser / Native UI tab switch is two plain buttons: nothing says which tab is shown, and switching is silent | medium | all | open (example) |
| [rest-09](#rest-09) | web-view-demo | Back, Forward and Reload are named by glyphs ('◀', '▶', '↻') | medium | all | open (example) |
| [rest-10](#rest-10) | web-view-demo | An empty label sits in the toolbar (the loading glyph's slot), and the loading state itself is a glyph | low | all | open (example) |

### rest-01 {#rest-01}

The window opens with nothing focused: the reader hears only the window's title and has to Tab to learn there is a button

- **Example:** simple-button
- **Scenario:** rest-simple-button
- **Act:** launch (every run); rest-simple-button 'the window as launched, nothing pressed'
- **The reader should get:** The window opens with focus on its one control, so Orca reads 'Teksilo — Simple Button frame', then 'Click Me push button' and its description; Space activates it at once.
- **The reader gets:** Focus is on the frame itself: Orca says 'Teksilo — Simple Button frame.' and nothing else. The button is heard only after a first Tab. The same launch reading happens in automation\_bridge\_smoke, telemetry-plausible and web-view-demo (focus on the frame, only the title spoken).
- **Platform:** all (framework policy, the same on every platform). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-app/src/window\_manager.rs:96-135
- **Evidence:**
  - `launch: '+280.9 ms object:state-changed:focused 1 [frame] 'Teksilo — Simple Button'' / ORCA SAYS 'Teksilo — Simple Button frame.' (rest-simple-button, 3 of 3 runs; tree and tabwalk runs the same)`
  - `rest-simple-button: FAIL 'the window opens with focus on a control, not on the bare frame' - 'after launch focus is on [frame] 'Teksilo — Simple Button'' (runs 2 and 3; run 1 predates the check and shows the same events)`
  - `crates/teksilo-app/src/window_manager.rs:96-135: initial_window_focus gives a plain (non-modal) window focus only through an explicit Widget::initial_focus_hint; 'There is no first_focusable_descendant fallback on purpose'`
  - `examples/simple_button/src/main.rs:20-31: the root is the bare Button, which reports no initial_focus_hint`
- **Reproduced:** yes: 3 of 3 rest-simple-button runs, plus the tree and tabwalk runs (deterministic)
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** Deliberate framework policy, so the cheapest fix is in the example: give the root an initial\_focus\_hint that names the button. If the policy is revisited, a plain window whose tree holds no text field could fall back to its first focusable descendant.

### rest-02 {#rest-02}

The debug inspector (F12): closing it from inside drops focus onto the bare window, and several of its controls are unnamed

- **Example:** simple-button
- **Scenario:** rest-simple-button-inspector
- **Act:** rest-simple-button-inspector: Tab to 'Click Me', F12, Tab (onto 'Pick'), F12
- **The reader should get:** F12 opens the inspector and says so; every control in it has a name; F12 again closes it and puts focus back where the reader was before entering it ('Click Me').
- **The reader gets:** Opening it is announced only by Orca reading a page tab that has no focus ('Tree page tab.', the known selection-moves-Orca's-locus pattern of chrome-24). Its toolbar holds a focusable unnamed \[panel\] (the Off/Sel/All bounds SegmentedControl), an unnamed \[slider\] (overlay opacity) and an \[entry\] with only a placeholder ('filter type names…'); the close button is named '×'; 'Overflow ✓' and 'Watch' carry their on/off state only as a check-mark glyph in the name. Closing it with F12 while focus is on 'Pick' drops focus onto the frame: Orca says 'Teksilo — Simple Button frame.'. Debug builds only.
- **Platform:** all (framework; debug builds only). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-inspector/src/shell.rs:359-470; crates/teksilo-inspector/src/tabs/tree.rs:63; crates/teksilo-inspector/src/state.rs:375-377
- **Evidence:**
  - `F12 opens: '+179.3 ms object:selection-changed [page tab list] ''' / ORCA SAYS 'Tree page tab.'; no focus change (3 of 3 runs)`
  - `F12 opens: FAIL 'every focusable control has a name' - 'unnamed focusable [panel]', 'unnamed focusable [slider]', 'unnamed focusable [entry] attrs={'placeholder-text': 'filter type names…'}' (runs 2 and 3)`
  - `tree-F12-opens-the-inspector.txt: "[push button] '×' {focusable}", "[push button] 'Overflow ✓' {focusable}"`
  - `F12 closes: '+17.3 ms object:state-changed:focused 1 [frame] 'Teksilo — Simple Button'' / ORCA SAYS 'Teksilo — Simple Button frame.'; FAIL 'focus lands on [push button] 'Click Me'' (3 of 3 runs)`
  - `crates/teksilo-inspector/src/shell.rs:378-412: SegmentedControl and Slider::new(state.overlay_opacity, 0.1, 1.0) built with no label; :416-452 Overflow/Watch state in the label text ('Overflow ✓', 'Watching ✓'); :456 Button::new(lit!("×"))`
  - `crates/teksilo-inspector/src/tabs/tree.rs:63: TextInput with a placeholder and no label`
  - ``crates/teksilo-inspector/src/state.rs:375-377: toggle() only flips `open`; the panel is parked by the Switcher on `open` (shell.rs:157-202), and nothing records or restores the focus the reader had before entering it``
- **Reproduced:** yes: 3 of 3 runs (the naming check in the 2 runs that carried it)
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** Name the bounds control, the opacity slider and the filter field; name the close button 'Close inspector'; make Overflow and Watch toggles (pressed state) instead of a glyph in the label; remember the focus holder when the panel opens and restore it (if still alive) when it closes with focus inside.

### rest-03 {#rest-03}

'input-probe' is a push button a reader can neither focus nor activate

- **Example:** automation\_bridge\_smoke
- **Scenario:** rest-bridge-smoke
- **Act:** rest-bridge-smoke 'the window as launched' (tree), tabwalk
- **The reader should get:** Every control a reader meets can be used, or is not presented as a control. The probe is a test fixture; a reader meeting it should hear it as status text, not a button.
- **The reader gets:** Flat review reaches '\[push button\] 'input-probe'' with no Action interface, no focusable state and an empty description; Tab never visits it. Its value (the last input it saw) is a string on a Role::Button, which accesskit\_atspi\_common exposes on no interface (Value is numeric only, Text only for text ranges), so a reader cannot read what it observed either.
- **Platform:** all (the tree is the same everywhere). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/automation\_bridge\_smoke/src/main.rs:135-142 (example)
- **Evidence:**
  - `tree: "{'name': 'input-probe', 'role': 'push button', 'states': ['enabled', 'sensitive', 'showing', 'visible'], 'interfaces': ['Accessible', 'Component']}"`
  - `rest-bridge-smoke: FAIL '[push button] 'input-probe' is focusable' - "states=['enabled', 'sensitive', 'showing', 'visible']", 'actions=[]' (runs 2 and 3; run 1 recorded the same states)`
  - `tabwalk: Tab 1 -> 'Save push button.', Tabs 2-5 -> only the key echo 'tab'`
  - `examples/automation_bridge_smoke/src/main.rs:135-142: Role::Button chosen so the bridge's find_node can find the probe by name; no handler makes it activatable`
  - `accesskit_atspi_common 0.20 src/node.rs:486-492: Text only when supports_text_ranges, Value only for a numeric current_value`
- **Reproduced:** yes: 3 of 3 runs plus the tree dump (deterministic)
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** Give the probe a non-interactive role that keeps its name findable (for example Role::Status, or a Role::Group with a name), or keep the button and mark it as a test fixture in its description.

### rest-04 {#rest-04}

Every consent change and every recorded event rebuilds the whole privacy panel, and focus inside it is thrown to the panel's first control

- **Example:** telemetry-plausible
- **Scenario:** rest-telemetry-accept-reject, rest-telemetry-withdraw, rest-telemetry-inspect
- **Act:** rest-telemetry-accept-reject 'Space on Accept all'; rest-telemetry-withdraw 'Enter confirms' (Withdraw consent, then OK); rest-telemetry-inspect 'AT-SPI click on Fire 'click' intent while focus is on the accordion'
- **The reader should get:** After Accept all focus stays on Accept all and the next Tab goes on to 'Inspect data sent'. After confirming Withdraw consent focus returns to Withdraw consent. An event recorded while the reader sits on the Inspect accordion leaves them there, with the accordion as they left it.
- **The reader gets:** Accept all: focus jumps up to the consent switch ('Anonymous usage metrics toggle button pressed.'), and the next Tab goes to 'Reject all'. Withdraw + OK: focus lands on 'Reject all', and because focus re-enters the panel from the dialog Orca first reads the panel's whole 977-character notice again, then 'Reject all push button.'; nothing says consent was withdrawn. An event recorded while focus is on the Inspect accordion moves focus to the switch ('Anonymous usage metrics toggle button pressed.') and the accordion comes back collapsed, for sighted users too. Reject all and the switch itself only seem to keep focus because they are the first focusable control of the rebuilt panel (the switch is disabled while consent is refused).
- **Platform:** all (framework; the rebuild and the focus rule are platform-independent). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/privacy\_settings.rs:208-219, 820-823; crates/teksilo-core/src/widget\_tree/layout\_impl.rs:419-434, 892-902
- **Evidence:**
  - `Space on Accept all: '+52.7 ms object:state-changed:focused 1 [toggle button] 'Anonymous usage metrics'', '+53.6 ms object:state-changed:focused 0 [push button] 'Accept all'', then defunct for every old node; ORCA SAYS 'Anonymous usage metrics toggle button pressed.'; FAIL 'focus ends on [push button] 'Accept all'' - 'focus moved to [toggle button] 'Anonymous usage metrics'' (3 of 3 runs)`
  - `Tab after Accept all: FAIL 'focus lands on [push button] 'Inspect data sent'' - '+19.4 ms object:state-changed:focused 1 [push button] 'Reject all'' (3 of 3 runs)`
  - `Enter confirms (Withdraw): '+61.0 ms object:state-changed:focused 1 [push button] 'Reject all'', '[push button] 'Withdraw consent'' defunct; ORCA SAYS 'Privacy & Telemetry settings panel.', a 977-character 'Data is processed by Plausible Insights OÜ; …' (Orca's unrelatedLabels for the panel), 'Reject all push button.' (3 of 3 runs)`
  - `AT-SPI click on Fire 'click' intent with focus on the accordion: '+58.1 ms object:state-changed:focused 1 [toggle button] 'Anonymous usage metrics'' / ORCA SAYS 'Anonymous usage metrics toggle button pressed.'; FAIL 'focus stays on [push button] 'Inspect data sent'' (3 of 3 runs)`
  - `crates/teksilo-widgets/src/privacy_settings.rs:208-219: the consent state signal and recent_log_revision are both bound at BindingLevel::Rebuild on the PrivacySettings node itself`
  - `crates/teksilo-widgets/src/privacy_settings.rs:820-823: the Inspect accordion is built with a fresh Signal::new(false) on every build, so every rebuild collapses it`
  - `crates/teksilo-core/src/widget_tree/layout_impl.rs:419-434 and 892-902: after a rebuild that destroyed the focused node, focus goes to first_focusable_descendant of the innermost surviving ancestor, here the PrivacySettings node, whose first focusable descendant is the switch (or Reject all while the switch is disabled)`
- **Reproduced:** yes: each of the three acts 3 of 3 runs (deterministic)
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** Stop rebuilding the panel for state it can bind: drive the switches' value and enabled state and the accordion title from signals derived from the consent state and the recent log (Prop bindings at Relayout/RepaintOnly), keep the accordion's expanded signal on the struct, and render the event list through a reactive list instead of a snapshot. If a rebuild stays, restore focus to the control with the same identity (for example a stable key per button) rather than the first focusable.

### rest-05 {#rest-05}

The consent switch is not described by the line under its label, so moving onto it reads only 'Anonymous usage metrics toggle button'

- **Example:** telemetry-plausible
- **Scenario:** rest-telemetry-switch
- **Act:** rest-telemetry-switch 'Shift+Tab back onto the switch' (from Reject all)
- **The reader should get:** Orca says 'Anonymous usage metrics toggle button pressed' and then what the switch shares ('Counts of which buttons / menu items / shortcuts are used, plus app version and OS.'), as a sighted user reads beside it.
- **The reader gets:** 'Anonymous usage metrics toggle button pressed.' and nothing more. The explanation is a separate label; a reader hears it only inside the 977-character block Orca reads as the panel's unrelated labels when focus first enters the panel from outside it.
- **Platform:** all (framework; description relations are the same tree everywhere). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/privacy\_settings.rs:436-439
- **Evidence:**
  - `Shift+Tab back onto the switch: '+32.3 ms object:state-changed:focused 1 [toggle button] 'Anonymous usage metrics'' / ORCA SAYS 'Anonymous usage metrics toggle button pressed.'; FAIL "Orca says 'Counts of which buttons'" (2 of 2 runs of the final scenario; the earlier grab_focus return in runs 1-3 read the same)`
  - `tree-launch.txt: "[label] 'Counts of which buttons / menu items / shortcuts are used, plus app version and OS.'" beside "[toggle button] 'Anonymous usage metrics' {focusable}" with no desc`
  - `crates/teksilo-widgets/src/privacy_settings.rs:436-439: label_id and description_id are built, the toggle gets access_labelled_by(label_id) only`
- **Reproduced:** yes: 2 of 2 runs of the final act, and the tree in every run
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** ctx.access\_described\_by(toggle\_id, description\_id) (crates/teksilo-core/src/build\_context.rs:1220) next to the labelled\_by.

### rest-06 {#rest-06}

A web view whose engine failed reads 'Loading… panel' forever: neither loading nor failure reaches the reader

- **Example:** web-view-demo
- **Scenario:** rest-web-view
- **Act:** rest-web-view 'the window as launched' and 'Tab to the web view'; tabwalk Tab 9
- **The reader should get:** While the page loads the web view is busy; when the engine or the page fails, the reader is told (a description or name saying it could not load), as a sighted user sees the error wash.
- **The reader gets:** The engine failed to open (wry cannot embed WebKitGTK in a Wayland parent, the session this harness gives it), the widget went to its Error state and paints the StatusError wash, but the node is '\[panel\] 'Loading…'' with no busy state and an empty description, so Orca says 'Loading… panel.' every time. 'Loading…' is the example's own seed for the title signal, which names the node and is never replaced when the engine fails. Enter on it says only the key echo 'return'. A sighted user gets only a colour change too, with no text.
- **Platform:** all (framework: the accessibility node is the same on every platform; the failure reproduced here is Linux/Wayland). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-webview/src/lib.rs:1116-1134; crates/teksilo-webview/src/styles/recipe\_web\_view\_style.rs:111-114
- **Evidence:**
  - `tree: "{'name': 'Loading…', 'role': 'panel', 'states': ['enabled', 'focusable', 'sensitive', 'showing', 'visible'], 'actions': [{'name': 'click'}]}"`
  - `rest-web-view: FAIL 'the web view tells a reader its page did not load' - "[panel] name='Loading…' description='' states=['enabled', 'focusable', 'focused', 'sensitive', 'showing', 'visible']" (runs 2, 3 and 4)`
  - `tabwalk Tab 9 and rest-web-view 'Tab to the web view': '+11.8 ms object:state-changed:focused 1 [panel] 'Loading…'' / ORCA SAYS 'Loading… panel.' (4 of 4 runs plus the tabwalk)`
  - `crates/teksilo-webview/src/wry_backend.rs:104-133 (fail) and :285-290: a failed build_as_child posts ConsoleMessage and NavigationFinished { success: false }; crates/teksilo-webview/src/lib.rs:699-712 sets WebViewVisualState::Error; nothing posts a title`
  - `crates/teksilo-webview/src/lib.rs:1116-1134: accessibility() sets role, the name from title_signal, Focus/Click actions and keyboard_shortcut, and nothing from state_signal; crates/teksilo-webview/src/styles/recipe_web_view_style.rs:111-114: the overlay that paints the wash is hidden from AT`
  - `crates/teksilo-core/src/styles/web_view_style.rs:42-47: Error maps to SurfaceRole::StatusError, the only signal of failure`
  - `examples/web_view_demo/src/main.rs:56: title = Signal::new("Loading…")`
  - `accesskit_atspi_common 0.20 src/node.rs:339-341: an AccessKit busy node is exported as State::Busy, so a busy flag would reach Orca`
- **Reproduced:** yes: 4 of 4 rest-web-view runs (the check in the last 3) plus the tree and tabwalk runs
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** Publish the visual state on the WebView node: set\_busy() while Loading, and on Error a description (or a name suffix) such as 'Page could not be loaded', re-walked when state\_signal changes; the default overlay could also paint a text message. The example should seed its title with a real name.

### rest-07 {#rest-07}

Nothing tells a reader the web view is web content or that Enter goes into the page: it reads as a plain 'panel', and its Enter hint is published where no adapter looks

- **Example:** web-view-demo
- **Scenario:** rest-web-view
- **Act:** rest-web-view 'Tab to the web view'
- **The reader should get:** Focus on the frame says it is a web page (a document or embedded role, or a role description) and that Enter moves into it.
- **The reader gets:** 'Loading… panel.' (with a working engine it would be '&lt;page title&gt; panel.'). The Enter hint is set only as keyboard\_shortcut, which no AccessKit adapter exports; the action's key binding on AT-SPI is empty. This is the console-06 pattern (a way in or out published only as keyboard\_shortcut), new here for the WebView, together with the role mapping.
- **Platform:** Linux: Role::WebView becomes AT-SPI 'panel' (accesskit\_atspi\_common 0.20 node.rs:288). From source only: accesskit\_windows 0.35 maps it to UIA Document (node.rs:210), accesskit\_macos 0.27 to NSAccessibilityUnknownRole (node.rs:177). None of the three adapters reads keyboard\_shortcut (no occurrence in their sources).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-webview/src/lib.rs:1116-1134
- **Evidence:**
  - `rest-web-view 'Tab to the web view': ORCA SAYS 'Loading… panel.'; FAIL "Orca says 'Enter'" (runs 2, 3 and 4; run 1 said the same without the check)`
  - `tree: "'actions': [{'name': 'click', 'description': '', 'key_binding': ''}]" on the web view node`
  - `crates/teksilo-webview/src/lib.rs:1120 set_role(Role::WebView); :1131-1133 set_keyboard_shortcut("Enter") as the only statement of the Enter step`
  - `accesskit_atspi_common 0.20 src/node.rs:288 'Role::WebView => AtspiRole::Panel'; grep for keyboard_shortcut in accesskit_atspi_common 0.20, accesskit_windows 0.35 and accesskit_macos 0.27 sources: no match`
  - `accesskit_atspi_common 0.20 src/node.rs:986-988: role_description is exported as the localized role name, which Teksilo could use`
- **Reproduced:** yes: 3 of 3 runs with the check (and the earlier run and tabwalk said the same)
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** Give the node a role description ('web page') and put the Enter step in its description until an adapter exports keyboard\_shortcut; upstream, map Role::WebView to an AT-SPI document or embedded role and export keyboard\_shortcut.

### rest-08 {#rest-08}

The Browser / Native UI tab switch is two plain buttons: nothing says which tab is shown, and switching is silent

- **Example:** web-view-demo
- **Scenario:** rest-web-view
- **Act:** rest-web-view 'Space on Native UI', 'Space on Browser'
- **The reader should get:** The two tabs expose which one is selected, and switching tells the reader what is now shown, as a sighted user sees the body change.
- **The reader gets:** Both are plain '\[push button\]'s with no selected, pressed or checked state. After Space on Native UI the web view leaves the tree, the native panel's two labels arrive and the status label's text changes ('Native UI tab — WebView subview hidden ✓'), but Orca says nothing at all; the same on the way back.
- **Platform:** all (the example's tree). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/web\_view\_demo/src/main.rs:88-96, 151-158 (example)
- **Evidence:**
  - `Space on Native UI: '+41.4 ms object:text-changed:delete [label] 'Browser tab — WebView subview VISIBLE'', '+41.5 ms object:text-changed:insert … text='Native UI tab — WebView subview hidden ✓'', no ORCA SAYS in the act; FAIL "Orca says 'Native UI tab'" (4 of 4 runs)`
  - `Space on Browser: the reverse text change, no speech; FAIL "Orca says 'Browser tab'" (4 of 4 runs)`
  - `tree: "[push button] 'Browser' {focusable}", "[push button] 'Native UI' {focusable}"`
  - `examples/web_view_demo/src/main.rs:88-96: Button::new(lit!("Browser")) / ("Native UI") set a Signal<usize>; :151-158 the status label is a plain TextWidget; :170-172 the Switcher`
- **Reproduced:** yes: 4 of 4 runs
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** Use a SegmentedControl or TabBar over the same selection signal (selected state and position come with it), or at least make the status line a polite live region.

### rest-09 {#rest-09}

Back, Forward and Reload are named by glyphs ('◀', '▶', '↻')

- **Example:** web-view-demo
- **Scenario:** rest-web-view
- **Act:** tabwalk Tabs 3-5; rest-web-view 'the window as launched'
- **The reader should get:** 'Back push button', 'Forward push button', 'Reload push button'.
- **The reader gets:** Orca sends the bare glyph to the synthesizer: '◀ push button.', '▶ push button.', '↻ push button.'. Whether it is voiced as its Unicode name ('black left-pointing triangle') or dropped depends on the synthesizer's symbol tables; either way the action is not named.
- **Platform:** all (the example's names). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/web\_view\_demo/src/main.rs:99-119 (example)
- **Evidence:**
  - `tabwalk: '+83.4 ms ORCA SAYS: '◀ push button.'', '+65.1 ms ORCA SAYS: '▶ push button.'', '+66.7 ms ORCA SAYS: '↻ push button.''`
  - `rest-web-view: FAIL 'every push button is named in words' - "[push button] '◀'", "'▶'", "'↻'" (runs 2, 3 and 4)`
  - `examples/web_view_demo/src/main.rs:99, 106, 113: Button::new(lit!("◀")), ("▶"), ("↻")`
- **Reproduced:** yes: tabwalk plus 3 of 3 runs with the check (deterministic)
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** IconButton with the glyph as its icon and names 'Back', 'Forward', 'Reload', or .access\_label(...) on the buttons.

### rest-10 {#rest-10}

An empty label sits in the toolbar (the loading glyph's slot), and the loading state itself is a glyph

- **Example:** web-view-demo
- **Scenario:** rest-web-view
- **Act:** rest-web-view 'the window as launched'
- **The reader should get:** No empty stop in flat review; loading, when shown, is said in words or as a busy state.
- **The reader gets:** '\[label\] ''' with 0 characters between the URL and the status line whenever nothing loads; while loading it would read '  ⏳'.
- **Platform:** all (the example's tree). Measured on Linux AT-SPI/Orca 46.1.
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/web\_view\_demo/src/main.rs:147-148 (example)
- **Evidence:**
  - `tree: "{'name': '', 'role': 'label', 'text': {'characters': 0, 'text': ''}}"`
  - `rest-web-view: FAIL "the tree holds no [label] ''" - "found [label] ''" (runs 2, 3 and 4)`
  - `examples/web_view_demo/src/main.rs:147-148: TextWidget::new(lit!("")).text(loading.map(|l| if *l { "  ⏳" } else { "" }))`
- **Reproduced:** yes: 3 of 3 runs (deterministic)
- **Verification:** reproduced by the sweep itself, on the build with the fixes; no second agent reran it.
- **Fix idea:** Show the label only while loading (visible\_when) with words ('Loading'), or drop it in favour of the web view's own busy state (rest-06).
