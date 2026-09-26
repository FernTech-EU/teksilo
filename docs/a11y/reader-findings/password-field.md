<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Password field

Examples: `password-field`.
19 findings: 8 high, 5 medium, 6 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [password-01](#password-01) | password-field | A masked password field says nothing on Linux: no echo while typing, no content on focus, no Text interface | high | Linux | fixed |
| [password-02](#password-02) | password-field | The Caps Lock warning is heard once per field: every later warning comes from a defunct node and Orca drops it | high | Linux | fixed |
| [password-03](#password-03) | password-field | Arriving in a password field with Caps Lock on: the warning comes before the focus event and Orca cuts it; the field's own focus speech never mentions it | high | Linux | open |
| [password-04](#password-04) | password-field | Caps Lock state is guessed by counting presses: when Caps Lock is on at launch the warning is inverted | high | Linux | open |
| [password-05](#password-05) | password-field | A validation message raised by leaving a field is cut by Orca every time | high | Linux | open |
| [password-06](#password-06) | password-field | An empty, untouched password field is flagged invalid as soon as focus leaves it, even for its own reveal toggle | medium | Linux | open |
| [password-07](#password-07) | password-field | The invalid state Teksilo sets on the field reaches no platform (no invalid-entry state) | low | Linux | upstream |
| [password-08](#password-08) | password-field | RevealMode::Hold exposes a 'button' no reader can operate: no action, not focusable, skipped by Tab | medium | Linux | open |
| [password-09](#password-09) | password-field | The disabled Sign in button reads as enabled and sensitive on AT-SPI, and a reader's focus request lands on it | medium | Linux | upstream |
| [password-10](#password-10) | password-field | The first character typed or pasted into an empty text field is never reported: an empty field has no text run and no Text interface | medium | Linux | fixed |
| [password-11](#password-11) | password-field | The Username field has no name: Orca reads its placeholder as if it were the field's content | high | Linux | open (example) |
| [password-12](#password-12) | password-field | The showcase captions duplicate the fields' names and are read as one run-on sentence on entering the panel | low | Linux | open (example) |
| [password-13](#password-13) | password-field | Every reveal toggle has the same name and no relation to its field | low | Linux | open |
| [password-v01](#password-v01) | password-field | On macOS the Caps Lock warning never appears: winit delivers no KeyboardInput for Caps Lock there, and that is Teksilo's only source | high | macOS | open |
| [password-v02](#password-v02) | password-field | Confirm password keeps reading 'Passwords don't match' after Password is changed to match, while Sign in has become enabled | medium | Linux | open |
| [password-v03](#password-v03) | password-field | libatspi discards every AccessKit cache signal (wrong D-Bus signature), which is why a re-added node stays defunct to Orca | high | Linux | upstream |
| [password-v04](#password-v04) | password-field | Every field carries an empty, unnamed status-bar node (its idle ValidationStrip), and Orca's 'read status bar' finds the first of them | low | Linux | open |
| [password-v05](#password-v05) | password-field | Orca never speaks a password field's placeholder, so the hint 'At least 8 characters' is never heard on Linux | low | Linux | upstream |
| [password-v06](#password-v06) | password-field | RevealWhileTyping shows the password in clear on screen, and nothing tells a screen-reader user | low | Linux | open |

### password-01 {#password-01}

A masked password field says nothing on Linux: no echo while typing, no content on focus, no Text interface

- **Example:** password-field
- **Scenario:** password-typing, password-echo-modes
- **Act:** password-typing: "type 'q7zx9wkp' into Password", "the Password field after typing", "Shift+Tab to Username and Tab back to Password", "Backspace in Password"; password-echo-modes: "type 'x' into Reveal while typing"
- **The reader should get:** Each keystroke and deletion is echoed as a mask character (GTK's password entry reports '●' inserted and Orca speaks it; Orca turns off its own key echo in password text and relies on text-inserted events). On arriving at a filled field the reader hears it holds N masked characters (Orca's password-text format reads currentLineText).
- **The reader gets:** Nothing at all. The typing and Backspace acts carry no AT-SPI event, and Orca says nothing. The node exposes only Accessible and Component, with no Text interface (confirmed by a fresh libatspi client), so the bullet string Teksilo sets as the node's value has nowhere to go on AT-SPI. Coming back to the filled field, Orca says only 'Password password text.', exactly as for an empty field. The reader cannot tell whether a keystroke landed, how long the password is, or whether the field is empty. RevealWhileTyping behaves the same. No plaintext leaks, which is correct.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows, from source only: accesskit\_windows node.rs:592-597 exposes the value ('••••••••') through the Value pattern and node.rs:727-728 sets IsPassword, so this is Linux-specific. macOS, from source only: AXSecureTextField subrole (accesskit\_macos node.rs:277).
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:1073-1085 (the protected branch sets role and value but emits no text run); crates/teksilo-widgets/src/primitives/text\_input\_field.rs:731-738 (a masked field keeps no retained measurement)
- **Evidence:**
  - `report (password-typing-20260925-140038-4154311), act "type 'q7zx9wkp' into Password": FAIL  a object:text-changed:insert event from [password text] '*' / no object:text-changed:insert event from [password text] '*'; FAIL  Orca says something / Orca said nothing (the act carries no event at all)`
  - `fresh libatspi client after typing: {'found': True, 'role': 'password text', 'name': 'Password', 'interfaces': ['Accessible', 'Component'], ... 'text_error': "Error: atspi_error: Unknown interface 'org.a11y.atspi.Text' (1)"}`
  - `act "Shift+Tab to Username and Tab back to Password": +1031.0 ms object:state-changed:focused 1 [password text] 'Password' / +1063.6 ms ORCA SAYS: 'Password password text.'`
  - `act "Backspace in Password": FAIL no object:text-changed:delete event from [password text] '*'; Orca said nothing`
  - `control act "type 'bob' into Username": +52.2 ms object:text-changed:insert [entry] '' text='o' / +88.4 ms object:text-changed:insert [entry] '' text='b' (a plain entry does report keystrokes)`
  - `crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:1073-1085: when protected, set_role(PasswordInput) and set_value(echo_char x count); no text runs are emitted`
  - `accesskit_atspi_common-0.20.0 node.rs:486-488: supports_text() is supports_text_ranges(), which needs a text run (accesskit_consumer text.rs:1402-1406); adapter.rs:122: text-changed is emitted only when both old and new nodes support ranges`
  - ``Orca scripts/default.py:2702: `if role == Atspi.Role.PASSWORD_TEXT and not event.isLockingKey(): return False` (no key echo in password text); script_utilities.py:3801-3802: an inserted-text event in password text is echoed when enableKeyEcho is on; formatting.py:405-406: PASSWORD_TEXT focused = 'labelOrName + readOnly + textRole + currentLineText + allTextSelection'``
  - `password-typing-20260925-142809-410320, act "type 'q7zx9wkp' into Password": FAIL a object:text-changed:insert event from [password text] / FAIL Orca says something (Orca said nothing); pass no event carries 'q7zx9wkp'`
  - `same run, act "the Password field after typing": fresh libatspi client {'role': 'password text', 'name': 'Password', 'interfaces': ['Accessible', 'Component'], ...}`
  - `password-typing-20260925-142109-279166, act "Shift+Tab to Username and Tab back to Password": +1030.5 ms object:state-changed:focused 1 [password text] 'Password' / +1060.9 ms ORCA SAYS: 'Password password text.'`
  - `password-echo-modes-20260925-142936-489494, act "type 'x' into Reveal while typing": FAIL a object:text-changed:insert event from [password text] '*'`
- **Reproduced:** Structural. 2 of 2 password-typing runs (13:49, 14:00) and 2 of 2 password-echo-modes runs (13:52, 14:09) with the current harness.
- **Verification:** confirmed. Reproduced: Structural. 2 of 2 password-typing runs (142109, 142809) and 1 of 1 password-echo-modes run (142936): typing into a masked field, including RevealWhileTyping, and Backspace put no text-changed event on the bus, and Orca said nothing. In both typing runs a fresh libatspi client reported interfaces \['Accessible','Component'\] and the Text error, and Orca read the filled field on return as just 'Password password text.'.
- **Fix idea:** While protected (Masked / RevealWhileTyping), emit the bullet string as a text run (echo\_char repeated, no word starts, caret kept at the end), as GTK does with its invisible char. That gives the node a Text interface, and text-changed insert/delete events that carry '•', never the secret. For NoEcho, emit one empty run so that nothing, not even the length, is exposed.

### password-02 {#password-02}

The Caps Lock warning is heard once per field: every later warning comes from a defunct node and Orca drops it

- **Example:** password-field
- **Scenario:** password-caps-lock
- **Act:** "Caps Lock on again" (after on, then off) and "Shift+Tab twice back to Password with Caps Lock on"
- **The reader should get:** Every time Caps Lock turns on in a password field, or focus comes back to a field while it is on, the reader hears 'Caps Lock is on'.
- **The reader gets:** Only the first warning in a field is spoken. The warning is a live Role::Status TextWidget shown with visible\_when. Hiding it removes the node, which AT-SPI announces as defunct, and showing it again re-adds the same id. libatspi keeps the object defunct, so Orca logs 'Ignoring defunct object' and drops every later announcement. This does not go through ctx.announce, so the K2 fix to the framework announcer does not cover it.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows and macOS, from source only: node\_added raises UIA LiveRegionChanged (accesskit\_windows adapter.rs:256-261) or posts an announcement (accesskit\_macos event.rs:237-239), with no defunct state, so the drop should be Linux-only.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: password-caps-lock: second Caps Lock warning heard 1/1.
- **Where:** crates/teksilo-widgets/src/password\_field.rs:491-500 (live TextWidget toggled by ctx.visible\_when)
- **Evidence:**
  - `password-caps-lock-20260925-135318-4038418, act "Caps Lock on in the Password field": +16.8 ms object:announcement [status bar] 'Caps Lock is on' text='Caps Lock is on'; Orca 13:53:27.309617 - SPEECH OUTPUT: 'Caps Lock is on'`
  - `act "Caps Lock off": +11.9 ms object:state-changed:defunct 1 [status bar] 'Caps Lock is on'`
  - `act "Caps Lock on again": +13.6 ms object:announcement [status bar] 'Caps Lock is on' text='Caps Lock is on' (path /org/a11y/atspi/accessible/0/79228167015269891578674544640, same as the defunct one); Orca 13:53:35.076576 EVENT MANAGER: Ignoring defunct object: [status bar: 'Caps Lock is on']; check FAIL Orca says 'Caps Lock is on' / Orca dropped: 'Caps Lock is on'`
  - `act "Shift+Tab twice back to Password with Caps Lock on": +32.2 ms object:announcement [status bar] 'Caps Lock is on' from the same path; Orca 13:53:46.758752 EVENT MANAGER: Ignoring defunct object: [status bar: 'Caps Lock is on']; Orca then says only 'Password password text.' / 'Use at least 8 characters.'`
  - `crates/teksilo-widgets/src/password_field.rs:491-500: TextWidget(CAPS_LOCK_GLYPH).access_role(Role::Status).access_live(Live::Polite).access_label(...), ctx.visible_when(warn_id, caps && focused)`
  - `accesskit_atspi_common-0.20.0 adapter.rs:91-105: remove_node emits StateChanged(Defunct, true); adapter.rs:72-77: announcement on add`
  - `password-caps-lock-20260925-142016-287207, act "Caps Lock off": +12.3 ms object:children-changed:remove [panel] 'Sign in' -> [status bar] 'Caps Lock is on' / +12.7 ms object:state-changed:defunct 1 [status bar] 'Caps Lock is on'`
  - `same run, act "Caps Lock on again": +8.3 ms object:announcement [status bar] 'Caps Lock is on' (path /org/a11y/atspi/accessible/0/79228167015269891578674544640); 14:20:32.413676 EVENT MANAGER: Ignoring defunct object: [status bar: 'Caps Lock is on']; FAIL Orca says 'Caps Lock is on' / Orca dropped`
  - `same run, act "Shift+Tab twice back to Password with Caps Lock on": +28.9 ms object:announcement from the same path; 14:20:44.099362 EVENT MANAGER: Ignoring defunct object; Orca then says only 'Password password text.' / 'Use at least 8 characters.'`
  - `accesskit_atspi_common-0.20.0 adapter.rs:72-77 (announcement on add), 98-102 (StateChanged(Defunct,true) on remove); nothing emits Defunct false on re-add`
- **Reproduced:** 3 of 3 runs (13:53:18, 13:54:48, 13:55:38), in both acts every time.
- **Verification:** confirmed. Reproduced: 3 of 3 password-caps-lock runs (141854, 142016, 142541), in both acts every time. Orca's log has 'EVENT MANAGER: Ignoring defunct object: \[status bar: 'Caps Lock is on'\]' twice per run (6 of 6).
- **Fix idea:** Keep the warning node permanently in the tree and switch its name between empty and 'Caps Lock is on': AT-SPI announces a live node's name change (atspi\_common node.rs:610-622), with no remove and re-add. Alternatively, route the warning through the fixed framework announcer and put the state into the field's description.

### password-03 {#password-03}

Arriving in a password field with Caps Lock on: the warning comes before the focus event and Orca cuts it; the field's own focus speech never mentions it

- **Example:** password-field
- **Scenario:** password-caps-lock
- **Act:** "Tab to Confirm password with Caps Lock on"
- **The reader should get:** Tabbing into a password field while Caps Lock is on, the reader is told Caps Lock is on (the warning's main use case).
- **The reader gets:** The new field's warning node is added, with its announcement, in the same update as the focus move. The consumer hands node changes to adapters before the focus event, so Orca starts 'Caps Lock is on' and stops it about 40 ms later to read 'Confirm password password text.'. The warning is not part of the field's name or description, so nothing in the focus speech carries it either. When the field's warning has been shown before, the drop in password-02 applies instead.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows/macOS not measured; the ordering is the consumer's (tree.rs:640-673), and NVDA is said to keep live-region text across a focus change.
- **Severity:** high; **layer:** framework
- **Status:** Open. In the announce-focus fix topic, not fixed there: Same class as password-05: the Caps Lock warning is a widget live-region node added as focus arrives (password\_field.rs:499-500), not an announcer message. The widget-level idea from the finding stands: describe the field by the warning.
- **Where:** crates/teksilo-widgets/src/password\_field.rs:499-500 (visible = caps.zip(&focused), where focused is the row's focus\_within, so the warning appears in the same update as the focus move)
- **Evidence:**
  - `password-caps-lock-20260925-135318-4038418: +12.9 ms object:announcement [status bar] 'Caps Lock is on' text='Caps Lock is on' / +14.6 ms object:state-changed:focused 1 [password text] 'Confirm password'`
  - `Orca: 13:53:42.862449 - SPEECH OUTPUT: 'Caps Lock is on' / 13:53:42.910134 - NULL SPEECH: stop / 13:53:42.910258 - SPEECH OUTPUT: 'Confirm password password text.'`
  - `observation: 'Caps Lock is on' reached the bus 1.8 ms before the act's focus change; Orca's 'Caps Lock is on' was cut by a stop 48 ms in (estimated)`
  - `runs 135448 and 135538: the same, 1.8 ms and 1.2 ms before focus, cut 37 ms and 30 ms in`
  - `crates/teksilo-widgets/src/password_field.rs:499-500: visibility = caps.zip(&focused), where focused is the row's focus_within, so the warning appears in the same update as the focus move`
  - `verify-password-caps-arrive-20260925-142402-405771, act "Tab to Password with Caps Lock on (the warning's first showing)": +12.9 ms object:announcement [status bar] 'Caps Lock is on' / +14.1 ms object:state-changed:focused 1 [password text] 'Password' / +23.1 ms ORCA SAYS (CUT): 'Caps Lock is on' / +60.1 ms ORCA SAYS: 'Password password text.'`
  - `orca-debug.out of that run: 14:24:14.672747 - SPEECH OUTPUT: 'Caps Lock is on' / 14:24:14.709617 - DEFAULT: Interrupting presentation / 14:24:14.709643 - NULL SPEECH: stop / 14:24:14.709740 - SPEECH OUTPUT: 'Password password text.'`
  - `password-caps-lock-20260925-142016-287207, act "Tab to Confirm password with Caps Lock on": +11.5 ms object:announcement / +12.9 ms object:state-changed:focused 1 [password text] 'Confirm password'; Orca's 'Caps Lock is on' was cut by a stop 31 ms in`
- **Reproduced:** 3 of 3 runs (13:53:18, 13:54:48, 13:55:38).
- **Verification:** confirmed. Reproduced: 6 of 6 arrivals. 3 of 3 password-caps-lock runs (arriving at Confirm password), cut 39, 31 and 39 ms in. 3 of 3 verify-password-caps-arrive runs (Caps Lock turned on in Username, then Tab into Password, the warning's first showing in the session), cut 37, 26 and 34 ms in.
- **Fix idea:** Describe the field by the warning (access\_described\_by, as for the validation strip), so 'Caps Lock is on' is part of what Orca reads on arrival. Keep the live announcement for Caps Lock toggled while focus stays in the field.

### password-04 {#password-04}

Caps Lock state is guessed by counting presses: when Caps Lock is on at launch the warning is inverted

- **Example:** password-field
- **Scenario:** password-caps-lock-leave-on then password-caps-lock-start-on (same invocation, same KWin session)
- **Act:** start-on: "Tab to Password with Caps Lock on" and "press Caps Lock (it is now really off)"
- **The reader should get:** With Caps Lock on, the field warns. When the user turns it off, nothing claims it is on.
- **The reader gets:** Teksilo starts from caps\_lock\_active = false and flips it on every CapsLock key-down, and never reads the OS lock state. If Caps Lock is already on when the app starts (or is toggled while another window has focus), the state is inverted. Arriving in Password with Caps Lock really on gives no warning. Pressing Caps Lock to turn it off makes the app announce 'Caps Lock is on', and Orca speaks it. A fresh AT-SPI client proves the real lock state: Username held 'Q' before the press and 'Qq' after it.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The tracking code in teksilo-app is platform-independent, so every platform is affected by source.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-app/src/app.rs:2605-2620 (the only writer of the Caps Lock state, via set\_caps\_lock\_from\_os at app.rs:2619); crates/teksilo-app/src/window\_manager.rs:1069
- **Evidence:**
  - `password-caps-lock-start-on-20260925-140335-4154311: fresh probe username-on: {... 'text': 'Q'} (Caps Lock is on at launch)`
  - `act "Tab to Password with Caps Lock on": +12.2 ms object:state-changed:focused 1 [password text] 'Password'; +61.0 ms ORCA SAYS: 'Password password text.'; FAIL the bus carries an announcement of 'Caps Lock is on' / no object:announcement in the act`
  - `act "press Caps Lock (it is now really off)": +16.2 ms object:announcement [status bar] 'Caps Lock is on' text='Caps Lock is on' / +29.5 ms ORCA SAYS: 'Caps Lock is on'`
  - `fresh probe username-off after typing 'q': {... 'text': 'Qq'} (the lock really is off)`
  - `crates/teksilo-app/src/app.rs:2605-2620 (comment: 'winit's ModifiersState carries no lock state'); app.rs:2616: managed.caps_lock_active = !managed.caps_lock_active; crates/teksilo-app/src/window_manager.rs:1069: caps_lock_active: false`
  - `password-caps-lock-start-on-20260925-142439-410320: fresh probe username-on text 'Q'; act "Tab to Password with Caps Lock on": +11.7 ms object:state-changed:focused 1 [password text] 'Password' / +42.2 ms ORCA SAYS: 'Password password text.' / FAIL the bus carries an announcement of 'Caps Lock is on' (no object:announcement in the act)`
  - `same run, act "press Caps Lock (it is now really off)": +17.3 ms object:announcement [status bar] 'Caps Lock is on' / +27.5 ms ORCA SAYS: 'Caps Lock is on'; next act: fresh client sees text 'Qq'`
  - `` winit-0.30.13/src/platform_impl/macos/view.rs:949-951: `let Some(event_modifier) = key_to_modifier(&logical_key) else { break 'send_event; };` ``
  - `winit-0.30.13/src/platform_impl/windows/keyboard.rs:657: is_repeat: (previous_state ^ transition_state) != 0`
- **Reproduced:** 2 of 2 pairs (13:56:28 and 13:56:50; 14:03:13 and 14:03:35); deterministic logic.
- **Verification:** corrected by the verifier. Reproduced: 2 of 2 leave-on/start-on pairs (141918/141941 and 142417/142439), plus the sweep's 2. Each time the fresh probe read Username 'Q' at launch. 'Tab to Password with Caps Lock on' put no announcement on the bus. 'press Caps Lock (it is now really off)' announced 'Caps Lock is on' and Orca spoke it. The probe then read 'Qq'. The Linux finding is right, deterministic, and the severity is right. The platform claim 'every platform is affected' needs correcting, because the effect differs. macOS (winit 0.30.13 source): Caps Lock arrives only as flagsChanged. update\_modifiers breaks out without sending any KeyboardInput for a key that key\_to\_modifier does not know (macos/view.rs:938-951), and that function knows only Alt, Control, Super and Shift (view.rs:81-89). So on macOS the counter never runs and the warning never shows at all (reported separately below). Windows (source only): WM\_KEYDOWN autorepeat is passed through as repeat=true (windows/keyboard.rs:657). app.rs:2609 tests only ElementState::Pressed, so holding Caps Lock past the repeat delay flips Teksilo's guess once per repeat. On X11/Wayland the xkb keymap does not repeat Caps\_Lock, so there only the start state and presses made in other windows desynchronise it. The state is also per window (caps\_lock\_active in each ManagedWindow), so a toggle in one window leaves the others wrong.
- **Fix idea:** Read the real lock state: on Wayland/X11, the locked modifiers are in the xkb state the compositor sends on keyboard enter; on Windows GetKeyState(VK\_CAPITAL) & 1; on macOS NSEvent.modifierFlags.capsLock. Resync on window focus and on every key event, instead of counting presses.

### password-05 {#password-05}

A validation message raised by leaving a field is cut by Orca every time

- **Example:** password-field
- **Scenario:** password-validation (also seen in password-caps-lock, password-blur-empty, password-caps-lock-start-on, tabwalk)
- **Act:** "Tab away from the short password" and "type 'abcdefgX' into Confirm, Tab away"
- **The reader should get:** Leaving a field with an invalid value, the reader hears the whole message ('Use at least 8 characters', 'Passwords don't match').
- **The reader gets:** The validator runs in the focus-loss handler. The ValidationStrip's name change and assertive announcement land in the same update as the focus move, ahead of the focus event, and Orca stops the message about 30-80 ms in to read the toggle. The reader hears it only by going back to the field, where the description gives 'Use at least 8 characters.'. A reader who moves on never learns of the error. Password-09 compounds this: Sign in reads as enabled, so a reader has no hint of why the form does not submit. None of these messages goes through ctx.announce, so the K2 fix does not cover them.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows/macOS not measured (the consumer orders node changes before focus on every adapter).
- **Severity:** high; **layer:** framework
- **Status:** Open. In the announce-focus fix topic, not fixed there: Same platform ordering, but a different producer. The message is the ValidationStrip's own live name change (validation\_strip.rs:148-160, Role::Status + Live::Assertive), raised in on\_blur (text\_input\_field/widget\_impl.rs:114-129). It is not ctx.announce, so this fix does not touch it. Two options: (a) a walker rule that holds any widget live region's new text for one update when the update moves focus, emitting the delivered name, or keeping a new live node hidden when it is off the focus path; this is broader and changes the timing of every widget live region; (b) have a blur-raised message go through ctx.announce, which now waits for the focus move, while the strip stays silent for that change.
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:115-129 (validator on blur); crates/teksilo-widgets/src/primitives/validation\_strip.rs:148-160
- **Evidence:**
  - `password-validation-20260925-140511-23050: +16.6 ms object:announcement [status bar] 'Use at least 8 characters' text='Use at least 8 characters' (0.3 ms before the focus change to the toggle)`
  - `Orca: 14:05:24.308697 - SPEECH OUTPUT: 'Use at least 8 characters' / 14:05:24.339173 - NULL SPEECH: stop / 14:05:24.339284 - SPEECH OUTPUT: 'Toggle password visibility toggle button not pressed.'`
  - `Orca: 14:05:42.526167 - SPEECH OUTPUT: 'Passwords don't match' / 14:05:42.572705 - NULL SPEECH: stop / 14:05:42.572821 - SPEECH OUTPUT: 'Toggle password visibility toggle button not pressed.'`
  - `on return: 14:05:28.214528 - SPEECH OUTPUT: 'Use at least 8 characters.' (description, works); 14:05:46.441366 - SPEECH OUTPUT: 'Passwords don't match.'`
  - `crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:114-128: on_blur runs run_validator_and_apply; crates/teksilo-widgets/src/primitives/validation_strip.rs:148-154: Role::Status, set_name(message), Live::Assertive`
  - `Orca default.py ~698-705 stops speech to present a new focus; accesskit_consumer tree.rs:640-673 hands node changes before the focus event`
  - `password-validation-20260925-142105-287207, act "Tab away from the short password": +12.3 ms object:property-change:accessible-name [status bar] 'Use at least 8 characters' / +12.5 ms object:announcement / +13.8 ms object:state-changed:focused 1 [toggle button] / +38.8 ms ORCA SAYS (CUT): 'Use at least 8 characters' / +86.7 ms ORCA SAYS: 'Toggle password visibility toggle button not pressed.'`
  - `orca-debug.out of that run: 14:21:18.240750 - SPEECH OUTPUT: 'Use at least 8 characters' ... 14:21:18.288419 - DEFAULT: Interrupting presentation / 14:21:18.288486 - NULL SPEECH: stop`
  - `password-validation-20260925-142631-405771, act "type 'abcdefgX' into Confirm, Tab away": Orca's "Passwords don't match" was cut by a stop 46 ms in`
- **Reproduced:** 'Use at least 8 characters' cut in 3 of 3 validation runs and in every other run where an invalid Password lost focus (caps-lock x3, blur-empty, start-on x2, tabwalk): 10 of 10. 'Passwords don't match' cut in 3 of 3.
- **Verification:** confirmed. Reproduced: 'Use at least 8 characters' was announced 12 times across validation (3), blur-empty (3), caps-lock (3), caps-lock-start-on (2) and tabwalk (1). It was cut 12 of 12 times, 25-67 ms in, and never spoken uncut. 'Passwords don't match' was cut 5 of 5 (3 validation runs and 2 stale-confirm runs), 34-46 ms in.
- **Fix idea:** Deliver a blur-triggered message after the focus event: set the strip's name in the next update, or announce it through the (fixed) announcer once focus has settled, so Orca queues it after the new focus instead of losing it. The on-return description can stay as it is.

### password-06 {#password-06}

An empty, untouched password field is flagged invalid as soon as focus leaves it, even for its own reveal toggle

- **Example:** password-field
- **Scenario:** password-blur-empty (also password-caps-lock, tabwalk)
- **Act:** "Tab on from the empty, untouched Password"
- **The reader should get:** Passing through an empty field, or moving to the field's own reveal toggle, does not raise an error the user has not earned yet.
- **The reader gets:** The validator runs on every focus loss, whatever the field's state. Tabbing from the empty Password to its own eye button sets 'Use at least 8 characters', which is announced (and cut, see password-05). From then on, every arrival in the field reads 'Password password text. Use at least 8 characters.', before the user has typed anything.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The behaviour is platform-independent.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:115-129 and text\_input\_field.rs:1149-1175 (run\_validator\_and\_apply has no pristine check)
- **Evidence:**
  - `password-blur-empty-20260925-135848-4105235: +18.3 ms object:announcement [status bar] 'Use at least 8 characters' text='Use at least 8 characters' / +18.4 ms object:state-changed:focused 1 [toggle button] 'Toggle password visibility'`
  - `act "Shift+Tab back to Password": +70.5 ms ORCA SAYS: 'Password password text.' / +70.5 ms ORCA SAYS: 'Use at least 8 characters.'`
  - `tabwalk-password-field-20260925-141235-179040, Tab 2 (empty Password to its toggle): +14.7 ms object:announcement [status bar] 'Use at least 8 characters'`
  - `crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:114-128 (validator wrapped into on_blur unconditionally); crates/teksilo-widgets/src/password_field.rs:505-510 (the toggle sits in the composite row, outside the TextInputField's focus)`
  - `password-blur-empty-20260925-142727-405771, act "Tab on from the empty, untouched Password": object:announcement 'Use at least 8 characters' 0.2 ms before focus on [toggle button]; Orca's 'Use at least 8 characters' was cut by a stop 32 ms in; next act: ORCA SAYS: 'Password password text.' / 'Use at least 8 characters.'`
  - `tabwalk-password-field-20260925-142628-460915, Tab 2: +13.3 ms object:announcement [status bar] 'Use at least 8 characters' / +13.9 ms object:state-changed:focused 1 [toggle button]`
- **Reproduced:** Deterministic; 5 of 5 runs where the empty Password lost focus (blur-empty, tabwalk, caps-lock x3).
- **Verification:** confirmed. Reproduced: Deterministic. 3 of 3 blur-empty runs (142040, 142202, 142727), 3 of 3 caps-lock runs (act "Tab to the reveal toggle"), and tabwalk Tab 2.
- **Fix idea:** Skip validate-on-blur while the field is still pristine (never edited), and in PasswordField do not treat a focus move to the field's own reveal toggle as a commit (run the validator when focus leaves the whole row).

### password-07 {#password-07}

The invalid state Teksilo sets on the field reaches no platform (no invalid-entry state)

- **Example:** password-field
- **Scenario:** password-validation
- **Act:** "Shift+Tab back to Password" (after 'abc' was rejected)
- **The reader should get:** An invalid field carries AT-SPI STATE\_INVALID\_ENTRY (Orca says 'invalid entry'), UIA IsDataValidForForm=false, or macOS AXInvalid.
- **The reader gets:** The field's states show no invalid-entry state. Only the description carries the message. TextInputField sets Invalid::True, but accesskit\_atspi\_common 0.20 never maps it, and the Windows and macOS adapters never read Node::invalid either. Teksilo's code comment claims this surfaces aria-invalid.
- **Platform:** Linux AT-SPI (measured). Windows/macOS, from source only: accesskit\_windows-0.35.0 and accesskit\_macos-0.27.0 never read invalid().
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** accesskit\_atspi\_common-0.20.0/src/node.rs:301-386 (state(): no Invalid mapping); Teksilo's side at crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:1180-1190
- **Evidence:**
  - `password-validation-20260925-140511-23050: FAIL  [password text] 'Password' has state invalid-entry / [password text] 'Password' interfaces=['Accessible', 'Component'] states=['editable', 'enabled', 'focusable', 'focused', 'selectable-text', 'sensitive', 'showing', 'single-line', 'visible'] text=None description='Use at least 8 characters'`
  - `crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:1186-1190: builder.inner_mut().set_invalid(Invalid::True)`
  - `accesskit_atspi_common-0.20.0 node.rs:300-386 (state()): no invalid mapping; grep -i invalid in accesskit_windows-0.35.0/src/node.rs and accesskit_macos-0.27.0/src finds no use of Node::invalid`
  - `password-validation-20260925-142631-405771, act "Shift+Tab back to Password": ORCA SAYS: 'Password password text.' / 'Use at least 8 characters.'; FAIL [password text] 'Password' has state invalid-entry`
  - `/usr/lib/python3/dist-packages/orca/formatting.py:405-406: PASSWORD_TEXT 'focused': 'labelOrName + readOnly + textRole + currentLineText + allTextSelection', 'unfocused': same + MNEMONIC (no invalid); formatting.py:258: ENTRY 'unfocused' includes '+ required + pause + invalid'`
- **Reproduced:** 3 of 3 validation runs; structural.
- **Verification:** corrected by the verifier. Reproduced: Structural, 3 of 3 validation runs: the invalid Password field's states never include invalid-entry. The AccessKit gap is real. atspi\_common state() never reads invalid(), and accesskit\_windows-0.35.0 and accesskit\_macos-0.27.0 never read Node::invalid either (grep finds no use). But the sweep's claim of what a reader would get is wrong for this widget. Orca 46.1's PASSWORD\_TEXT format (formatting.py:405-409) has no 'invalid' or 'required' generator, unlike ENTRY's 'unfocused' format (formatting.py:258). So even with STATE\_INVALID\_ENTRY mapped, Orca would say nothing more on arriving at an invalid password field. The reader already gets the error through the description ('Password password text. Use at least 8 characters.', passed). For PasswordField on Linux the loss is therefore nil in speech, which makes the severity low. It stays medium for a plain TextInput or other entry with a validator, where Orca would say 'invalid entry', and on Windows, where NVDA reads UIA IsDataValidForForm. The Teksilo comment says Invalid::True is aria-invalid in AccessKit terms, which is accurate to AccessKit. It is not misleading, just unfulfilled by the adapters.
- **Fix idea:** Upstream: map Invalid to State::InvalidEntry (AT-SPI), IsDataValidForForm (UIA) and AXInvalid (macOS). Until then, Teksilo could start the description with a localized 'Invalid:' and correct the comment.

### password-08 {#password-08}

RevealMode::Hold exposes a 'button' no reader can operate: no action, not focusable, skipped by Tab

- **Example:** password-field
- **Scenario:** password-hold, tabwalk
- **Act:** "the Hold to reveal button as the tree gives it", "Tab from the Hold to reveal field", "activate the Hold button through AT-SPI"
- **The reader should get:** A node announced as a push button can be reached and pressed: it takes focus, offers a click action, and a reader's activation reveals the field (or it is hidden from AT when it cannot be operated).
- **The reader gets:** The \[push button\] 'Toggle password visibility' has only the Accessible and Component interfaces, with no Action interface and no focusable state. Tab goes from 'Hold to reveal' straight to 'Always protected'. The reader's own activation fails with 'offers no action on AT-SPI'. The reveal is pointer-only, but AT is shown a control that does nothing.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). By source, the same node (Role::Button with no Click or Focus action) reaches every platform.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/password\_field.rs:513-538
- **Evidence:**
  - `password-hold-20260925-141029-98166: [push button] 'Toggle password visibility' interfaces=['Accessible', 'Component'] states=['enabled', 'sensitive', 'showing', 'visible'] text=None description=None actions=None`
  - `act "Tab from the Hold to reveal field": +13.2 ms object:state-changed:focused 1 [password text] 'Always protected' / ORCA SAYS: 'Always protected password text.'`
  - `act "activate the Hold button through AT-SPI": AT-SPI action failed: {'path': '/org/a11y/atspi/accessible/0/79228177788168430625052688384', 'name': 'Toggle password visibility', 'role': 'push button'} offers no action on AT-SPI`
  - `tabwalk-password-field-20260925-141235-179040: Tab 11 [password text] 'Hold to reveal', then Tab 12 [password text] 'Always protected'`
  - `crates/teksilo-widgets/src/password_field.rs:513-538: MinSize + on_pointer_event(PointerDown/Up) + access_role(Role::Button) + access_label, with no focusable, no Click action, no keyboard path`
  - `password-hold-20260925-142855-410320: FAIL [push button] 'Toggle password visibility' has interface Action / has state focusable; act "Tab from the Hold to reveal field": ORCA SAYS: 'Always protected password text.'; act "activate the Hold button through AT-SPI": FAIL the button offers an action a reader can invoke`
  - `crates/teksilo-widgets/src/password_field.rs:79-83: RevealMode::Hold doc: 'Press-and-hold to reveal, release to re-mask (WinUI "Peek"). Pointer-oriented; prefer Toggle for keyboard accessibility.'`
- **Reproduced:** 2 of 2 runs (13:57:50, 14:10:29) plus tabwalk; structural.
- **Verification:** corrected by the verifier. Reproduced: Structural, 2 of 2 password-hold runs (142231, 142855) and tabwalk (Tab 11 'Hold to reveal' then Tab 12 'Always protected'). The facts reproduce: \[push button\] 'Toggle password visibility' with interfaces \['Accessible','Component'\], no focusable state, and no action, and AT-SPI activation fails with 'offers no action on AT-SPI'. The severity is lowered to medium. RevealMode::Hold is documented as pointer-only ('Pointer-oriented; prefer Toggle for keyboard', password\_field.rs:81). The field itself stays fully usable while masked, so what a reader loses is the peek convenience, not access to content. That is not 'critical', and not 'essential information missing'. The real defect is a node announced as a push button that no reader can operate, which is misleading (medium). An AT click would not help either: the only handler is on\_pointer\_event for PointerDown/Up (password\_field.rs:523-535), not a Click action. Orca's synthesised mouse click would reveal for only an instant.
- **Fix idea:** Make the Hold affordance focusable, with Space/Enter held to reveal, and an AT Click (or Expand/Collapse) that toggles the reveal until focus leaves. Or give it Role::ToggleButton with the Toggle behaviour for keyboard and AT. At minimum, do not advertise Role::Button on a node with no action.

### password-09 {#password-09}

The disabled Sign in button reads as enabled and sensitive on AT-SPI, and a reader's focus request lands on it

- **Example:** password-field
- **Scenario:** password-sign-in
- **Act:** "the disabled Sign in button as the tree gives it", "a reader's own focus request on the disabled Sign in"
- **The reader should get:** While both passwords are empty, Sign in reads as unavailable (Orca 'grayed'). A reader's focus request is refused, and when it becomes available a state change tells the reader.
- **The reader gets:** States are \['enabled', 'focusable', 'sensitive', 'showing', 'visible'\] although enabled\_when(can\_submit) is false and Tab skips the button. accesskit\_atspi\_common adds Enabled\|Sensitive to every role outside its read-only list, whatever the node's disabled flag. Teksilo's Button advertises Action::Focus even while disabled, so AT-SPI grab\_focus focuses it, and Orca says 'Sign in push button.' with no hint it does nothing. Because the state set is the same either way, no event tells the reader when Sign in becomes enabled.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). Windows/macOS, from source only: UIA IsEnabled and AXEnabled follow is\_disabled, so the wrong state is Linux-only.
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** root cause accesskit\_atspi\_common-0.20.0/src/node.rs:376-380; Teksilo's share crates/teksilo-core/src/widget\_tree/pointer\_router.rs:1163-1201 (an AT Focus is serviced with no enabled check) and crates/teksilo-widgets/src/button.rs:1301-1302 (Focus advertised unconditionally)
- **Evidence:**
  - `password-sign-in-20260925-141101-98166: FAIL [push button] 'Sign in' has no state enabled / [push button] 'Sign in' interfaces=['Accessible', 'Action', 'Component'] states=['enabled', 'focusable', 'sensitive', 'showing', 'visible'] ... actions=[{'name': 'click', ...}]`
  - `act "a reader's own focus request on the disabled Sign in": +21.5 ms object:state-changed:focused 1 [push button] 'Sign in' / +43.6 ms ORCA SAYS: 'Sign in push button.'`
  - `tabwalk-password-field-20260925-141235-179040: Tab 4 [toggle button] (Confirm's), then Tab 5 [password text] 'Masked' (Tab skips the disabled Sign in)`
  - ``accesskit_atspi_common-0.20.0 node.rs:376-380: `if state.is_read_only_supported() && state.is_read_only_or_disabled() { ReadOnly } else { Enabled | Sensitive }`; accesskit_consumer-0.39.0 node.rs:861-879: Role::Button is not read-only-supported``
  - `crates/teksilo-widgets/src/button.rs:1301-1302: add_action(Click); add_action(Focus) unconditionally`
  - `password-sign-in-20260925-142927-410320: FAIL [push button] 'Sign in' has no state enabled / no state sensitive (states ['enabled','focusable','sensitive','showing','visible']); act "a reader's own focus request on the disabled Sign in": ORCA SAYS: 'Sign in push button.'`
  - `verify-password-stale-confirm-20260925-142628-410320, act "make Password 'abcdefgX' (now equal to Confirm)" (can_submit turns true): pass no event from 'Sign in'; two acts later Tab reaches it: ORCA SAYS: 'Sign in push button.'`
- **Reproduced:** 2 of 2 runs (13:58:23, 14:11:01); structural.
- **Verification:** confirmed. Reproduced: Structural: 2 of 2 password-sign-in runs (142303, 142927). The absence of any event on enabling: 2 of 2 verify-password-stale-confirm runs (142628, 142719).
- **Fix idea:** Upstream: insert Enabled\|Sensitive only when !is\_disabled() (ReadOnly remains a separate question). Teksilo: do not advertise Action::Focus (or honour an AT focus request) on a disabled node, matching Tab.

### password-10 {#password-10}

The first character typed or pasted into an empty text field is never reported: an empty field has no text run and no Text interface

- **Example:** password-field
- **Scenario:** password-typing (control act), password-copy, password-caps-lock-leave-on/start-on
- **Act:** "the empty Username entry, from a fresh AT-SPI client", "type 'bob' into Username", "Ctrl+V into Username after copying the revealed field"
- **The reader should get:** An empty entry already supports Text (0 characters), so its first insertion raises text-changed:insert, as the comment at widget\_impl.rs:1105-1108 promises ('Runs are emitted even for an empty field').
- **The reader gets:** The empty Username exposes no Text interface. Typing 'bob' reports only 'o' and 'b'; the first 'b' gives a caret move and no insertion. Pasting 'hunter2' into the empty field reports no insertion at all, although a fresh client then reads 'hunter2'. The retained measurement of '' comes from layout\_single\_line and has no line. from\_geometry adds a fallback line only when the lines cover less than a non-empty text, so no run is emitted. The empty-line run path in push\_text\_runs is never reached, supports\_text\_ranges() is false, and the adapter skips the change. This affects every TextInputField (TextInput, SpinBox, SearchField, a revealed PasswordField).
- **Platform:** Linux AT-SPI (measured). Windows/macOS not measured; they also gate text events on supports\_text\_ranges.
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-core/src/accessibility/text\_runs.rs:228-296 (TextRunSource::from\_geometry: no fallback line for empty text, 267-282)
- **Evidence:**
  - `password-typing-20260925-140038-4154311: fresh libatspi client: {'found': True, 'role': 'entry', 'name': '', 'interfaces': ['Accessible', 'Component'], ... 'text_error': "Error: atspi_error: Unknown interface 'org.a11y.atspi.Text' (1)"}`
  - `act "type 'bob' into Username": +19.2 ms object:text-caret-moved [entry] '' / +52.2 ms object:text-changed:insert [entry] '' text='o' / +88.4 ms object:text-changed:insert [entry] '' text='b'; FAIL [entry] reports 'bob' inserted / inserted text from [entry]: 'ob'`
  - `password-copy-20260925-140123-4154311, act "Ctrl+V into Username after copying the revealed field": only +30.1 ms object:text-caret-moved [entry] ''; fresh probe revealed: {... 'interfaces': ['Accessible', 'Component', 'EditableText', 'Text'], ... 'text': 'hunter2'}`
  - ``crates/teksilo-widgets/src/primitives/text_input_field.rs:739-745: retained = layout_single_line(&text) even for ''; crates/teksilo-core/src/accessibility/text_runs.rs:268-269: `let covered = ...; if covered < text.len()` (no fallback line for empty text); text_runs.rs:610-630: the empty-line run is emitted only for a line that exists``
  - `accesskit_consumer-0.39.0 text.rs:1402-1406: supports_text_ranges needs a run; accesskit_atspi_common-0.20.0 adapter.rs:122: no text change unless both old and new support ranges`
  - `verify-password-empty-edits-20260925-142514-410320, act "type 'a' into the empty Username": only +12.2 ms object:text-caret-moved [entry] ''; FAIL [entry] reports 'a' inserted`
  - `same run, act "BackSpace (Username 'ab' to 'a')": +10.9 ms object:text-changed:delete [entry] '' text='b' (pass)`
  - `same run, act "BackSpace (Username 'a' to empty)": no event at all; FAIL [entry] reports 'a' deleted; fresh client interfaces ['Accessible','Component'] with the Text error`
  - `same run, act "type 'c' into the emptied Username": only object:text-caret-moved; FAIL [entry] reports 'c' inserted`
  - ``text_runs.rs:267-268: `let covered = lines.last().map(|l| l.byte_range.end).unwrap_or(0); if covered < text.len() {` (false for '' so no line and no run)``
- **Reproduced:** Every run that typed or pasted into the empty Username: typing 2 of 2, copy 1 of 1, caps-lock-leave-on 2 of 2, caps-lock-start-on 2 of 2.
- **Verification:** corrected by the verifier. Reproduced: First insertion unreported: 2 of 2 typing runs, 2 of 2 empty-edits runs, 2 of 2 caps-lock-start-on runs, and the copy run (paste of 'hunter2'). Deletion that empties the field unreported: 2 of 2 verify-password-empty-edits runs. The finding is real, but narrower on platform and wider in scope than reported. Wider: the deletion that empties a field is also never reported. 'ab' to 'a' gives text-changed:delete 'b', but 'a' to '' gives no event at all, and the next first character is again unreported. A reader who clears a field hears no final deletion, and a paste into an empty field is silent. Narrower: 'Windows/macOS also gate text events on supports\_text\_ranges' is inaccurate. accesskit\_windows raises UIA Text\_TextChanged whenever the new node supports ranges (adapter.rs:54-72, no old-node test), and the value change fires too. accesskit\_macos posts ValueChanged on any value change (event.rs:270-288) and gates only SelectedTextChanged on both nodes (event.rs:291-293). So by source the lost first insertion and last deletion are Linux (AT-SPI) specific. On a desktop, Orca's own key echo still speaks the typed letter. What is lost is echo-by-character and braille from text events, the spoken deleted character on Backspace, and paste feedback. Medium stays.
- **Fix idea:** In from\_geometry, when the geometry has no lines, push one empty SourceLine (0..0 at the origin) so push\_text\_runs emits its empty run. Or have TextInputField fall back to TextRunSource::flat for empty text.

### password-11 {#password-11}

The Username field has no name: Orca reads its placeholder as if it were the field's content

- **Example:** password-field
- **Scenario:** every run's launch act (tree audit), tabwalk
- **Act:** launch; tabwalk Tab 15
- **The reader should get:** The field is announced as 'Username, entry' (TextInput has .label()).
- **The reader gets:** The entry has no name, which the launch audit reports as unnamed-control. Orca says 'entry you@example.com.', which can be heard as a field that already holds that address. The visible caption 'Username' is a separate label linked to nothing.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured); the missing name reaches every platform by source.
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/password\_field/src/main.rs:84-87
- **Evidence:**
  - `tree audit: unnamed-control: [entry] '': a focusable entry with no name`
  - `launch: +457.4 ms object:state-changed:focused 1 [entry] ''; ORCA SAYS: 'Sign in panel.' / ORCA SAYS: 'entry you@example.com.'`
  - `tree: [entry] '' {editable,focusable,focused,selectable-text,single-line} attrs={'placeholder-text': 'you@example.com'}`
  - `examples/password_field/src/main.rs:85-88: TextInput::new(self.username.clone()).placeholder(lit!("you@example.com")) with no .label(); main.rs:211-222 labeled() adds an unlinked caption`
  - `tabwalk-password-field-20260925-142628-460915 launch: tree audit unnamed-control: [entry] '': a focusable entry with no name; +573.1 ms ORCA SAYS: 'entry you@example.com.'; Tab 15: +50.2 ms ORCA SAYS: 'Sign in panel.' / 'entry you@example.com.'`
- **Reproduced:** Every run (structural).
- **Verification:** confirmed. Reproduced: Structural. Every launch audit in the verifier's 31 runs reports unnamed-control; tabwalk Tab 15.
- **Fix idea:** TextInput::new(...).label(lit!("Username")). Better still, have labeled() link the caption to its field (labelled\_by) rather than duplicating names.

### password-12 {#password-12}

The showcase captions duplicate the fields' names and are read as one run-on sentence on entering the panel

- **Example:** password-field
- **Scenario:** tabwalk, password-echo-modes, password-hold, password-protected, password-copy
- **Act:** tabwalk Tab 5 (first focus in the Echo modes panel)
- **The reader should get:** Entering the panel, the reader hears the panel name, then the focused field.
- **The reader gets:** Orca says 'Echo modes panel.', then 'Reveal while typing No echo (length hidden) Hold to reveal (press the eye) Always protected for AT', then the field. Every caption is a label related to no control; Orca reads unrelated labels of three or more words when focus enters a panel. The fields are already named by .label(), so the captions are redundant noise, and the list runs together.
- **Platform:** Linux / Orca 46.1 (measured).
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/password\_field/src/main.rs:198-207
- **Evidence:**
  - `tabwalk-password-field-20260925-141235-179040 Tab 5: +158.6 ms ORCA SAYS: 'Echo modes panel.' / +158.6 ms ORCA SAYS: 'Reveal while typing No echo (length hidden) Hold to reveal (press the eye) Always protected for AT' / +158.6 ms ORCA SAYS: 'Masked password text.'`
  - `examples/password_field/src/main.rs:211-222 (labeled(): caption TextWidget beside a field already named by .label())`
  - `Orca script_utilities.py:1771: def unrelatedLabels(self, root, onlyShowing=True, minimumWords=3)`
  - `tabwalk-password-field-20260925-142628-460915 Tab 5: +86.3 ms ORCA SAYS: 'Echo modes panel.' / 'Reveal while typing No echo (length hidden) Hold to reveal (press the eye) Always protected for AT' / +86.4 ms 'Masked password text.'`
- **Reproduced:** Every time focus enters the Echo modes panel (5 of 5 runs observed).
- **Verification:** confirmed. Reproduced: Every time focus entered the Echo modes panel in the verifier's runs: 6 of 6 (tabwalk Tab 5, hold, protected, echo-modes, and copy twice).
- **Fix idea:** Link each caption to its field (labelled\_by, or let the field name itself from the caption), or hide the caption from AT when the field carries the same name.

### password-13 {#password-13}

Every reveal toggle has the same name and no relation to its field

- **Example:** password-field
- **Scenario:** tree password-field, tabwalk
- **Act:** launch tree
- **The reader should get:** A reader listing buttons (Orca's structural navigation or button list, NVDA's elements list) can tell which field each eye button reveals, e.g. 'Show Confirm password', or via a controls relation.
- **The reader gets:** Five toggle buttons and the Hold push button are all named 'Toggle password visibility', with description 'Toggle visibility' and no relations. Only the Tab context tells them apart.
- **Platform:** Linux AT-SPI (measured); the fixed name reaches every platform by source.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/password\_field.rs:507-510; the description comes from IconButton::visibility\_toggle's tooltip at icon\_button.rs:560-565
- **Evidence:**
  - `tree-password-field-20260925-134617-3931473: [toggle button] 'Toggle password visibility' {'description': 'Toggle visibility', 'states': ['enabled', 'focusable', 'sensitive', 'showing', 'visible'], 'interfaces': ['Accessible', 'Action', 'Component'], ...} repeated after each of Password, Confirm password, Masked, Reveal while typing, No echo, Always protected; no 'relations' on any`
  - `crates/teksilo-widgets/src/password_field.rs:505-510: IconButton::visibility_toggle(...).access_label(tr_widget!(a11y_password_reveal())), a fixed string with no reference to the field`
  - `tabwalk-password-field-20260925-142628-460915 launch tree: toggle button 'Toggle password visibility' desc='Toggle visibility' rel=None after each of Password, Confirm password, Masked, Reveal while typing, No echo, Always protected; push button 'Toggle password visibility' desc=None after Hold to reveal`
- **Reproduced:** Structural, every run.
- **Verification:** confirmed. Reproduced: Structural, in the verifier's launch trees.
- **Fix idea:** Name the toggle from the field's label (e.g. 'Show password: Confirm password') when a label is set, and publish access\_controls(field\_id) on it.

### password-v01 {#password-v01}

On macOS the Caps Lock warning never appears: winit delivers no KeyboardInput for Caps Lock there, and that is Teksilo's only source

- **Example:** password-field
- **Scenario:** source reading
- **Act:** any: Caps Lock turned on in, or before reaching, a PasswordField
- **The reader should get:** The reader is told Caps Lock is on, as on the other platforms (the example's own doc promises 'a warning glyph appears and screen readers announce it').
- **The reader gets:** By source: nothing, ever. macOS reports Caps Lock only as flagsChanged. winit 0.30.13's update\_modifiers makes a KeyboardInput only for a key its key\_to\_modifier knows (Alt, Control, Super, Shift) and otherwise breaks out. Teksilo's only writer of the Caps Lock state is the CapsLock KeyboardInput arm in teksilo-app, so caps\_lock stays false and the warning node is never shown, for sighted users too.
- **Platform:** macOS, from winit and Teksilo source only (not measurable here). Linux and Windows get the counting behaviour of password-04 instead.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-app/src/app.rs:2605-2620
- **Evidence:**
  - `winit-0.30.13/src/platform_impl/macos/view.rs:521-525: flags_changed -> update_modifiers(event, true)`
  - ``winit-0.30.13/src/platform_impl/macos/view.rs:949-951: `let Some(event_modifier) = key_to_modifier(&logical_key) else { break 'send_event; };` (no KeyboardInput)``
  - `winit-0.30.13/src/platform_impl/macos/view.rs:81-89: key_to_modifier maps only Alt, Control, Super, Shift`
  - `crates/teksilo-app/src/app.rs:2605-2620: the only caller of set_caps_lock_from_os (grep: app.rs:2619 is the sole call site); crates/teksilo-widgets/src/password_field.rs:490 reads window.caps_lock()`
- **Reproduced:** Not reproducible here (no macOS). Source reading of winit 0.30.13, the version in Cargo.lock.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Read the lock state from the platform instead of counting keys: NSEvent.modifierFlags & NSEventModifierFlagCapsLock (or CGEventSourceFlagsState) on macOS, GetKeyState(VK\_CAPITAL) & 1 on Windows, xkb locked modifiers on Wayland/X11. Resync on window focus.

### password-v02 {#password-v02}

Confirm password keeps reading 'Passwords don't match' after Password is changed to match, while Sign in has become enabled

- **Example:** password-field
- **Scenario:** verify-password-stale-confirm
- **Act:** "make Password 'abcdefgX' (now equal to Confirm)" then "Tab twice to Confirm"
- **The reader should get:** Once the two passwords match, arriving at Confirm password does not report a mismatch, and the form's state is consistent (Sign in enabled, no error).
- **The reader gets:** Confirm's validator reads Password but runs only on Confirm's own blur, submit or edits, so its feedback and the field's description stay 'Passwords don't match'. Orca says 'Confirm password password text. Passwords don't match.' although the passwords match and Sign in is already enabled. The error clears only when focus leaves Confirm again. PasswordField offers no way to re-run a validator when a value it depends on changes.
- **Platform:** Linux AT-SPI / Orca 46.1 (measured). The stale feedback is platform-independent (the red strip stays on screen too).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:115-140
- **Evidence:**
  - `verify-password-stale-confirm-20260925-142628-410320, act "Tab twice to Confirm": +698.2 ms object:state-changed:focused 1 [password text] 'Confirm password' / +733.8 ms ORCA SAYS: 'Confirm password password text.' / +733.8 ms ORCA SAYS: "Passwords don't match."; FAIL [password text] 'Confirm password' has no description containing "don't match" (description="Passwords don't match")`
  - `same run, act "Tab twice to Sign in": +11.3 ms object:property-change:accessible-description [password text] 'Confirm password' text='' (cleared only on Confirm's own blur), then ORCA SAYS: 'Sign in push button.'`
  - `verify-password-stale-confirm-20260925-142719-410320: the same, ORCA SAYS: "Passwords don't match." on arrival`
  - `examples/password_field/src/main.rs:105-118 (the Confirm validator reads self.password); crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:115-140 (the validator is wrapped only into on_blur and on_submit); PasswordField's public API (extract_widget_api PasswordField) has no revalidate or dependency hook`
- **Reproduced:** 2 of 2 runs (142628, 142719). Deterministic.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Let a validator declare the signals it reads (validate\_on(signal)), or expose a revalidate() / external feedback signal so the example can re-run Confirm's validator when Password changes. Until then the example could clear Confirm's feedback when Password changes.

### password-v03 {#password-v03}

libatspi discards every AccessKit cache signal (wrong D-Bus signature), which is why a re-added node stays defunct to Orca

- **Example:** password-field
- **Scenario:** every run (visible in every batch log)
- **Act:** any act that adds or re-adds a node (the Caps Lock warning shown again, in password-02)
- **The reader should get:** When a node comes back, its AddAccessible cache signal gives libatspi clients (Orca) its current state set and interfaces, so a node that was defunct is live again.
- **The reader gets:** accesskit\_unix emits AddAccessible with the body signature (so)(so)(so)iiassusau and RemoveAccessible with 'so', while libatspi 2.52 expects one struct argument, ((so)(so)(so)iiassusau). Every libatspi client logs 'AddAccessible with unknown signature' and ignores the signal. Nothing else clears the DEFUNCT state libatspi cached from state-changed:defunct, so Orca keeps dropping the re-added node's announcements. This is the upstream mechanism under password-02 and K2. It is also why a long-lived libatspi client's interface list goes stale after a role swap, which is the sweep's harness issue 1. (That libatspi would replace the cached states from a well-formed AddAccessible is inferred from its cache design, not read in its source, which is not installed here.)
- **Platform:** Linux AT-SPI, every AccessKit application
- **Severity:** high; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Evidence:**
  - `logs/batchA.log: '(process:279818): dbind-WARNING **: 14:18:56.203: AT-SPI: AddAccessible with unknown signature (so)(so)(so)iiassusau', 152 times (batchB 264, batchC 229, batchD 334, batchE 148, tabwalk 37); 'AT-SPI: Unknown signature so for RemoveAccessible' x4`
  - `strings /usr/lib/x86_64-linux-gnu/libatspi.so.0: '((so)(so)(so)iiassusau)' (the expected signature)`
  - `accesskit_unix-0.23.0/src/atspi/bus.rs:434-439: emit_cache_add -> emit_cache_signal("AddAccessible", &item); 456-473: the item is passed as the whole signal body, so its fields are sent flattened`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:61 emit_cache_added on add, 98-102 StateChanged(Defunct, true) on remove, and no Defunct false on re-add`
- **Reproduced:** Every run. The warnings are logged in every batch.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Upstream: wrap the cache item in a one-field tuple (emit the body as (item,)) so the signature is ((so)(so)(so)iiassusau), and the same for RemoveAccessible ((so)). In Teksilo, never hide and re-show a live node with the same NodeId; keep it and change its name, as the K2 announcer fix does.

### password-v04 {#password-v04}

Every field carries an empty, unnamed status-bar node (its idle ValidationStrip), and Orca's 'read status bar' finds the first of them

- **Example:** password-field
- **Scenario:** launch tree (tabwalk-password-field)
- **Act:** launch
- **The reader should get:** A validation strip with no message stays out of the reader's tree, or at least is not a STATUS\_BAR that shadows a real status bar.
- **The reader gets:** The launch tree has eight \[status bar\] '' nodes, one after each field, the Username TextInput included, although it has no validator. Each is an object-navigation stop with nothing in it. Orca's 'present status bar' command reads the first STATUS\_BAR descendant of the frame, which here is the Username field's empty strip; in an app with a real StatusBar below any text field it would shadow that bar. Keeping the node permanently is deliberate (it avoids the defunct drop of a re-added live node), so the fix is a role or name choice, not removal.
- **Platform:** Linux AT-SPI (the tree was measured; the Orca command is from source, since the harness cannot press Orca's own keys). By source it reaches every platform as a Role::Status node.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/primitives/validation\_strip.rs:148-164
- **Evidence:**
  - `tabwalk-password-field-20260925-142628-460915 launch tree: 'entry '' ...' followed by 'status bar '' desc=None', and likewise after each of Password, Confirm password, Masked, Reveal while typing, No echo, Hold to reveal, Always protected`
  - `crates/teksilo-widgets/src/primitives/validation_strip.rs:148-164: Role::Status is set unconditionally; the name is set only for Invalid/Corrected ('Empty Status node — present in the AT tree')`
  - `/usr/lib/python3/dist-packages/orca/where_am_i_presenter.py:370-381 present_status_bar -> AXUtilities.get_status_bar(frame); ax_utilities.py:189-198 returns the first STATUS_BAR descendant`
- **Reproduced:** Structural: every launch tree in the verifier's runs.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** While the strip is empty, expose it as hidden, or give the idle state a role that is neither a landmark nor a status bar; switch to Role::Status only while it holds a message, with the name changing on the same node so it is never re-added.

### password-v05 {#password-v05}

Orca never speaks a password field's placeholder, so the hint 'At least 8 characters' is never heard on Linux

- **Example:** password-field
- **Scenario:** tabwalk, password-typing
- **Act:** Tab to the empty Password field
- **The reader should get:** Arriving at an empty field, the reader hears its hint (Orca reads placeholderText for an empty ENTRY).
- **The reader gets:** 'Password password text.' only, although the node carries placeholder-text 'At least 8 characters'. Orca 46.1's PASSWORD\_TEXT format has no placeholderText, unlike ENTRY. So on Linux the length rule is heard only after a failed attempt, and then cut (password-05).
- **Platform:** Linux / Orca 46.1 (measured). Upstream Orca, for any toolkit.
- **Severity:** low; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Evidence:**
  - `tabwalk-password-field-20260925-142628-460915 Tab 1: +10.8 ms object:state-changed:focused 1 [password text] 'Password' / +44.4 ms ORCA SAYS: 'Password password text.'; launch tree: password text 'Password' attrs={'placeholder-text': 'At least 8 characters'}`
  - `/usr/lib/python3/dist-packages/orca/formatting.py:405-406 (PASSWORD_TEXT: no placeholderText) vs 257-258 (ENTRY: '(currentLineText or placeholderText)')`
- **Reproduced:** Every arrival at the empty Password in the verifier's runs (tabwalk, typing x2, caps-arrive x3). Structural.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Teksilo could put a pristine field's hint into its description (it is empty until validation), so Orca's description reading carries it. Upstream: add placeholderText to Orca's PASSWORD\_TEXT format.

### password-v06 {#password-v06}

RevealWhileTyping shows the password in clear on screen, and nothing tells a screen-reader user

- **Example:** password-field
- **Scenario:** password-echo-modes
- **Act:** "focus Reveal while typing (a reader's own focus request)"
- **The reader should get:** A user who cannot see the screen learns that their password is displayed in clear while they type (a shoulder-surfing risk), as the Toggle mode tells them with 'pressed'.
- **The reader gets:** 'Reveal while typing password text.', which says what it does only because the demo named the field after its mode. The node is a plain PasswordInput with no state or description saying the text is visible. By design the AT protection ignores the focus reveal.
- **Platform:** Linux / Orca 46.1 (measured); by source every platform.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Evidence:**
  - `password-echo-modes-20260925-142936-489494, act "focus Reveal while typing": ORCA SAYS: 'Reveal while typing password text.'; pass no event or node carries 'hunter2'`
  - `crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:1058-1071 (AT protection tracks only the explicit reveal toggle, not the RevealWhileTyping focus reveal)`
- **Reproduced:** 1 of 1 verifier run, and the sweep's 2. Structural.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** A design question: while RevealWhileTyping is showing clear text, add a localized description such as 'Shown on screen while typing'.
