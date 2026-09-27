<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Generated from the sweep of 25 and 26 September 2026 (on 261a218f) and its re-measure of 27 September 2026 (on c198e4d1). See ../reader-findings.md. -->

# Terminal, log view and code editor

Examples: `terminal-demo`, `log_view`, `code_editor`.
21 findings: 2 critical, 8 high, 5 medium, 6 low.
Swept on `261a218f` on 25 and 26 September 2026, measured again on
`c198e4d1` on 27 September 2026.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [console-01](#console-01) | code\_editor | CodeEditor and LogView put focus on an unnamed \[unknown\] wrapper; the text lives in a child, so Orca says nothing and no caret event is ever sent | critical | Linux | fixed |
| [console-02](#console-02) | code\_editor | CodeEditor is a keyboard trap: Tab, Ctrl+Tab and Escape+Tab all indent, Ctrl+Shift+Tab dedents, and nothing tells the reader the code changed | critical | Linux | fixed |
| [console-03](#console-03) | terminal-demo | Terminal rows reach the reader glued together with no separator: new output is heard as 'hellouser@host:~$' | high | Linux | open |
| [console-04](#console-04) | terminal-demo | On a full terminal screen every new line re-reads the whole screen, run together, and the live region falls silent | high | Linux | open |
| [console-05](#console-05) | terminal-demo | The terminal's live region announces the line the cursor left (usually the command the user typed), not the output; repeated output is never re-announced | high | Linux | open |
| [console-06](#console-06) | terminal-demo | The terminal's way out (Ctrl+Tab) is published only as keyboard\_shortcut, which no AccessKit adapter exports | high | Linux | open |
| [console-07](#console-07) | code\_editor | The code-completion list is silent from its second opening on: its list box comes back under a defunct id; 'expanded' and the active descendant never reach AT-SPI | high | Linux | fixed |
| [console-08](#console-08) | terminal-demo, log\_view, code\_editor | LogView::announce\_appends(true) cannot announce anything on any adapter | high | all | open |
| [console-09](#console-09) | code\_editor | The code editor and the log have no accessible name, and no API can give their text node one | high | Linux | fixed |
| [console-10](#console-10) | terminal-demo | Terminal output is heard twice (text insertion + live announcement), and the typed command is read back after Enter | medium | Linux | open |
| [console-11](#console-11) | code\_editor | A second caret is invisible to a reader, and typing at two carets is published as a delete and re-insert of everything between them | medium | Linux | open |
| [console-12](#console-12) | terminal-demo | When the shell exits, the terminal tells a reader nothing and silently swallows keys | medium | Linux | open |
| [console-13](#console-13) | log\_view | log\_view: Start / Stop gives the reader no state: no pressed state, a static name, a status line that is not live | medium | Linux | open (example) |
| [console-14](#console-14) | terminal-demo | Terminal: a typed space is invisible until the next non-blank character, because each row's trailing blanks are trimmed | low | Linux | open |
| [console-15](#console-15) | code\_editor | Completion options are read with their kind badge as a letter: 'k while keyword.' | low | Linux | open |
| [console-16](#console-16) | log\_view | log\_view 'Burst 10k' freezes the debug-build app for about 50 s, with nothing to tell a reader it is busy | low | Linux | open |
| [console-17](#console-17) | code\_editor | Bracket matching and the current-line band are visual only | low | Linux | open |
| [console-18](#console-18) | terminal-demo, log\_view, code\_editor | docs/log-view.md describes the log's accessibility as it no longer is | low | see entry | open |
| [console-V1](#console-v1) | terminal-demo | Terminal scrollback never reaches the reader: Shift+PageUp/PageDown leave the accessible text on the old page, and the next output swaps in the scrolled page while the live region announces an old history row | high | Linux | open |
| [console-V2](#console-v2) | code\_editor | Completion options have no accessible name: the row name-from-content pass cannot see TextWidget text | medium | Linux | open |
| [console-V3](#console-v3) | code\_editor | The editor's `controls` relation points at the completion panel's deferred wrapper (\[unknown\]), not at the list box, and keeps that empty node in the tree | low | Linux | open |

### console-01 {#console-01}

CodeEditor and LogView put focus on an unnamed \[unknown\] wrapper; the text lives in a child, so Orca says nothing and no caret event is ever sent

- **Example:** code\_editor
- **Scenario:** console-editor-focus, console-log-focus
- **Act:** Tab from the Theme combo box to the editor (code\_editor) or to the log (log\_view), then Down, then type a character
- **The reader should get:** Focus lands on the node that holds the text (\[entry\] multi-line for the editor, \[document frame\] for the log). The reader hears a name, the role and the line at the caret, and every caret move and edit arrives as text-caret-moved / text-changed on the focused node.
- **The reader got (`261a218f`):** Focus lands on the widget's outer wrapper, which emits no accessibility of its own and so reaches AT-SPI as role 'unknown' with no name and no Text interface. Orca generates pauses only and says nothing. The \[entry\] / \[document frame\] child is never focused, so accesskit\_atspi\_common never emits text-caret-moved or text-selection-changed for it: Down and typing produce no caret event at all (only the status label changes). Orca also treats the editor's insertions as having no cause and does not speak them (typing, Tab, completion accept are all silent). The same mis-aimed focus is why the completion list's active\_descendant (set on the entry) can never become the platform focus (consumer node.rs:90-102 follows active\_descendant only on the focused node).
- **Platform:** Linux AT-SPI / Orca 46.1, measured. Windows and macOS were not measured: focus is chosen in teksilo-core, not in the adapter, so there too the focused element is the Role::Unknown wrapper with no text pattern (from source). What NVDA or VoiceOver says for it was not read.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `731cc2e1` (editor-focus).
- **Now (`c198e4d1`):** Tab puts focus on the text node itself: the editor is \[entry\] 'Code' and Orca says 'Code entry // A little Teksilo widget. Edit me!' followed by the way out; the log is \[document frame\] 'Log' and Orca says 'Log document frame \[00:00:00.000\] WARN net: event 0 handled in 0ms.'. In the editor, Down and typing now send text-caret-moved and text-changed from the focused \[entry\], and Orca reads the new line on Down.
- **Measured again:** console-editor-focus, 2 of 2 runs; console-log-focus, 2 of 2 runs; fix-editor-focus-code and fix-editor-focus-log, 2 of 2 runs; tabwalk-code\_editor and tabwalk-log\_view, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-editor-focus 'Tab to the editor': +12.3 ms object:state-changed:focused 1 [entry] 'Code' / +12.6 ms object:text-caret-moved [entry] 'Code' / +61.1 ms ORCA SAYS: 'Code entry // A little Teksilo widget. Edit me!'`
  - `pass1 console-editor-focus 'Down in the editor': +17.9 ms object:text-caret-moved [entry] 'Code' / +28.8 ms ORCA SAYS: '// Ctrl+/ comments a line, Tab indents, Ctrl+D adds a caret.'`
  - `pass1 console-editor-focus 'type 'Z'': +43.7 ms object:text-changed:insert [entry] 'Code' text='Z' / +44.0 ms object:text-caret-moved [entry] 'Code'`
  - `pass2 console-editor-focus 'Tab to the editor': +11.2 ms object:state-changed:focused 1 [entry] 'Code' / +52.4 ms ORCA SAYS: 'Code entry // A little Teksilo widget. Edit me!'`
  - `pass1 console-log-focus 'Tab to the log': +11.3 ms object:state-changed:focused 1 [document frame] 'Log' / +56.4 ms ORCA SAYS: 'Log document frame [00:00:00.000] WARN net: event 0 handled in 0ms.'`
  - `pass2 console-log-focus 'Tab to the log': +14.9 ms object:state-changed:focused 1 [document frame] 'Log' / +77.5 ms ORCA SAYS: 'Log document frame [00:00:00.000] WARN net: event 0 handled in 0ms.'`
  - `pass1 fix-editor-focus-log 'Tab into the log': +17.0 ms object:state-changed:focused 1 [document frame] 'Log' / +70.4 ms ORCA SAYS: 'Log document frame [00:00:00.000] WARN net: event 0 handled in 0ms.'`
  - `pass1 and pass2 console-log-focus 'Down in the log': FAIL a object:text-caret-moved event (no event of any kind)`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor/widget.rs:589 (focusable wrapper), :1015-1027 (the wrapper is a GenericContainer and hands its node to the body through accessibility\_proxy); crates/teksilo-widgets/src/code\_editor/log\_view.rs:366-367, :677-689; crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:128-167 (focus handed to the proxy)
- **Evidence (`261a218f`):**
  - `console-editor-focus report.txt: '+8.4 ms object:state-changed:focused 1 [unknown] '''`
  - `events.jsonl: {"wall": "14:16:07.287065", "type": "object:state-changed:focused", "detail1": 1, ... "source": {"path": "/org/a11y/atspi/accessible/0/79228162938539451288863637504", "name": "", "role": "unknown"}}`
  - `orca-debug.out: 14:16:07.288634 - EVENT MANAGER: object:state-changed:focused for [unknown] in [application: 'code_editor'] (1, 0, 0)`
  - `orca-debug.out: 14:16:07.313100 - SPEECH GENERATOR: Starting unfocused generation for [unknown] (using role: unknown)`
  - `orca-debug.out: 14:16:07.320714 - SPEECH GENERATOR: Results for [unknown] are pauses only`
  - `orca-debug.out: 14:16:07.321275 - NULL SPEECH: stop`
  - `console-editor-focus, act 'Down in the editor': FAIL a object:text-caret-moved event from [entry] '*' (only '+11.6 ms object:property-change:accessible-name [label] 'char 37 · 1 caret'')`
  - `console-editor-focus, act 'type Z': '+48.9 ms object:text-changed:insert [entry] '' text='Z'' but FAIL a object:text-caret-moved event from [entry]`
  - `tree after launch: "[unknown] '' {focusable}" / "  [entry] '' {editable,focusable,multi-line,selectable-text} text='// A little Teksilo widget. Edit me!...'"`
  - `console-log-focus: '+9.3 ms object:state-changed:focused 1 [unknown] ''' ; orca-debug.out 14:15:37.061131 - SPEECH GENERATOR: Results for [unknown] are pauses only ; 14:15:37.062462 - NULL SPEECH: stop`
  - `log tree: "[unknown] '' {focusable}" / "  [document frame] '' {focusable} text='[00:00:00.000] WARN net: event 0 handled in 0ms\n...'"`
  - `launch audit (both examples): 'unknown-role: [unknown] '': a node whose role the adapter could not map'`
  - ``crates/teksilo-widgets/src/code_editor/widget.rs:568 `.focusable(true)` on the wrapper; widget.rs:989-993 `fn accessibility(&self, _builder)` is empty ('The wrapper stays a plain focusable container'), so the node keeps AccessNodeBuilder's default Role::Unknown (crates/teksilo-core/src/accessibility.rs:475-478)``
  - ``crates/teksilo-widgets/src/code_editor/log_view.rs:347-348 `.focusable(true)` on the LogView wrapper, which has no accessibility() either (the only one is LogViewBody's, log_view.rs:826)``
  - `crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:126-139: the TreeUpdate focus is the focused widget's own node`
  - ``accesskit_atspi_common-0.20.0/src/adapter.rs:215-218: `if !old_node.is_focused() || ... { return; }` before any CaretMoved / TextSelectionChanged``
  - `verify-console-editor-atfocus-20260925-144840-987106: '+16.5 ms object:state-changed:focused 1 [entry] ''' / '+16.7 ms object:text-caret-moved [entry] ''' / '+191.4 ms ORCA SAYS: 'entry // A little Teksilo widget. Edit me!'' ; Down: '+13.4 ms object:text-caret-moved [entry] '''`
  - `verify-console-log-atfocus-20260925-144905-987106: '+21.9 ms object:state-changed:focused 1 [document frame] ''' / '+125.1 ms ORCA SAYS: 'document frame [00:00:00.000] WARN net: event 0 handled in 0ms.''`
  - `console-editor-focus-20260925-144225-813521 orca-debug.out: '14:42:37.705804 - EVENT MANAGER: object:state-changed:focused for [unknown]' / '14:42:37.737674 - SPEECH GENERATOR: Results for [unknown] are pauses only' / '14:42:37.738561 - NULL SPEECH: stop' ; type Z: '14:42:45.564006 - FOCUS MANAGER: Changing locus of focus from [unknown] to [entry]' then '14:42:45.566720 - DEFAULT: Not speaking inserted string due to lack of cause'`
  - `run dirs: target/reader-verify/console/console-editor-focus-20260925-144225-813521, -144258-813521, console-log-focus-20260925-144332-813521, -144403-813521, verify-console-editor-atfocus-20260925-144840-987106, -145042-987106, verify-console-log-atfocus-20260925-144905-987106, -145107-987106`
- **Reproduced:** deterministic; 1 of 1 judged run each (console-editor-focus, console-log-focus), and the same focus event on \[unknown\] in every other editor/log run (tabwalks, trap, completion, multicaret, brackets, stream, burst: 9 runs)
- **Verification:** corrected by the verifier. Reproduced: editor-focus 2/2, log-focus 2/2; the same \[unknown\] focus in trap 2/2, completion 3/3, multicaret 2/2, brackets 1/1, log-stream 1/1, burst 1/1. AT-SPI grab\_focus lands on the text node: 2/2 editor, 2/2 log Reproduces exactly, and the source agrees. Tab focus lands on the CodeEditor/LogView wrapper. The wrapper emits no accessibility of its own (widget.rs:989-993 is empty; LogView, log\_view.rs:265-660, has no accessibility()), so it keeps AccessNodeBuilder's Role::Unknown (accessibility.rs:473-478). The emit pass makes the focused widget's own node the TreeUpdate focus (accessibility\_emit\_impl.rs:125-139) and exempts the focused node from presentational pruning (:228-235). atspi\_common emits CaretMoved only when the old node was focused (adapter.rs:215-218), and the consumer lets active\_descendant stand for focus only on the focus node (consumer node.rs:90-102). Two corrections. (1) Location: my verify runs show the defect is only in where KEYBOARD focus lands. An AT-SPI grab\_focus on the \[entry\] or the \[document frame\] focuses the text node, and everything then works: Orca says 'entry // A little Teksilo widget. Edit me!' and 'document frame \[00:00:00.000\] WARN net: ...', and Down gives text-caret-moved from \[entry\] (2 of 2 runs each). So the fix is to aim Tab/click focus at the body, or have the wrapper delegate its focus to it. (2) One side claim is partly a harness artifact. That the typed 'Z' is silent holds in any editor under this harness: Orca never sees the key (no key echo), and enableEchoByCharacter is off by default (script\_utilities.py:3789-3807). The Tab-indent / completion-accept / auto-close insertions are a different case: they go through isAutoTextEvent, which requires the source to be focused (script\_utilities.py:2432-2435), so that part is caused by this defect. Severity stays critical for the editor: no caret feedback while editing, plus console-02. For the read-only log alone it would be high.
- **Fix idea:** Make the AT focus the body node: either make the body the focusable widget, or give the wrapper no node of its own and let focus resolve to the body (a focus-delegate hook in the emit pass). Setting GenericContainer on the wrapper, as RichTextEditor does, is not enough on its own: a focused GenericContainer is still kept by common\_filter and still has no text.

### console-02 {#console-02}

CodeEditor is a keyboard trap: Tab, Ctrl+Tab and Escape+Tab all indent, Ctrl+Shift+Tab dedents, and nothing tells the reader the code changed

- **Example:** code\_editor
- **Scenario:** console-editor-trap
- **Act:** Tab into the editor, then press Ctrl+Tab; then Escape followed by Tab; then Ctrl+Shift+Tab
- **The reader should get:** Some keyboard route leaves the editor (Ctrl+Tab is the framework's own escape for the terminal and for rich-text tables), and a key pressed to leave writes nothing into the document
- **The reader got (`261a218f`):** Focus never leaves. Ctrl+Tab inserts four spaces into the code, Escape+Tab inserts four more, and Ctrl+Shift+Tab deletes four. Orca says nothing for any of it (console-01), so the reader edits their source file without knowing.
- **Platform:** Linux, measured. On every platform the key handling is Teksilo's own (keyboard.rs), so the trap is the same there.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `c515e521` (code-editor-trap).
- **Now (`c198e4d1`):** Ctrl+Tab leaves the editor for the Theme combo box and Ctrl+Shift+Tab leaves it backwards; neither writes anything into the document, and Orca says 'Toolbar tool bar', 'Theme combo box.'. Tab and Shift+Tab still indent and dedent. On arrival Orca reads the way out from the entry's description: 'Tab indents. Ctrl+Tab moves to the next control, Ctrl+Shift+Tab to the previous one.'
- **Measured again:** console-editor-trap, 2 of 2 runs; fix-code-editor-trap-keys, 2 of 2 runs; fix-code-editor-trap-hint, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-editor-trap 'Ctrl+Tab in the editor': +23.0 ms object:state-changed:focused 1 [combo box] 'Theme' / +52.8 ms ORCA SAYS: 'Theme combo box.' / pass no object:text-changed event`
  - `pass1 console-editor-trap 'Ctrl+Shift+Tab': +43.5 ms object:state-changed:focused 1 [combo box] 'Theme' / pass no object:text-changed event`
  - `pass2 console-editor-trap 'Ctrl+Tab in the editor': +93.4 ms object:state-changed:focused 1 [combo box] 'Theme' / pass no object:text-changed event`
  - `pass1 fix-code-editor-trap-keys 'Tab in the editor': +32.5 ms object:text-changed:insert [entry] 'Code' text='    ' / pass focus stays in the editor`
  - `pass1 console-editor-focus 'Tab to the editor': +61.1 ms ORCA SAYS: 'Tab indents. Ctrl+Tab moves to the next control, Ctrl+Shift+Tab to the previous one.'`
  - `pass2 fix-code-editor-trap-keys 'Ctrl+Shift+Tab in the editor': pass focus lands on [combo box] 'Theme' / pass no object:text-changed event`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor/keyboard.rs:97-106 (Ctrl+Tab and Ctrl+Shift+Tab left unhandled), :177-184 (Tab and Shift+Tab indent and dedent), :421-429 (tab\_escape\_hint); crates/teksilo-widgets/src/code\_editor/a11y.rs:106-110 (the hint in the description)
- **Evidence (`261a218f`):**
  - `console-editor-trap act 'Ctrl+Tab in the editor': '+47.6 ms object:text-changed:insert [entry] '' text='    '' ; FAIL focus leaves the editor ; FAIL no object:text-changed event`
  - `act 'Escape then Tab': 'object:text-changed:insert entry '    '' ; FAIL focus leaves the editor`
  - `act 'Ctrl+Shift+Tab': '+47.7 ms object:text-changed:delete [entry] '' text='    '' ; FAIL focus leaves the editor`
  - `tabwalk-code_editor 'Tab 3': '+27.7 ms object:text-changed:insert [entry] '' text='    '' ; Tab 4 and Tab 5 the same; no focus event after Tab 2`
  - `no ORCA SAYS line in any of the three acts`
  - ``crates/teksilo-widgets/src/code_editor/keyboard.rs:163 `Key::Tab if !shift && filter.accepts(CodeCommand::IndentLines)` does not check ctrl; keyboard.rs:167 the same for Shift+Tab``
  - ``the wrapper is `.focusable(true)` but not `.keyboard_capture(true)` (widget.rs:568), so the dispatcher's reserved Ctrl+Tab escape (crates/teksilo-core/src/widget_tree/pointer_router.rs:1103-1125) does not apply, and Tab goes to the widget first``
  - ``contrast crates/teksilo-widgets/src/rich_text/keyboard.rs:144-149 `let tab_escape = ctrl || modifiers.ctrl();`, which CodeEditor has no counterpart of``
  - `console-editor-trap-20260925-144435-813521 'Ctrl+Tab in the editor': '+47.3 ms object:text-changed:insert [entry] '' text='    '' ; FAIL focus leaves the editor`
  - `same run 'Ctrl+Shift+Tab': '+46.8 ms object:text-changed:delete [entry] '' text='    ''`
  - `console-editor-trap-20260925-144509-813521: identical ('+33.8 ms object:text-changed:insert [entry] '' text='    '')`
  - `keyboard.rs:121-124 comment: 'a ⌃-modified Tab belongs to focus navigation on every platform ... so the popup must not swallow it'. The main Tab arm at :163 does not apply the same rule`
- **Reproduced:** deterministic; 1 of 1 trap run, and Tab indenting in the code\_editor tabwalk
- **Verification:** confirmed. Reproduced: 2 of 2 trap runs (deterministic)
- **Fix idea:** Leave Ctrl+Tab / Ctrl+Shift+Tab unhandled in the editor (as rich\_text's tab\_escape does), so the framework moves focus. Also expose that chord through the description (see console-06).

### console-03 {#console-03}

Terminal rows reach the reader glued together with no separator: new output is heard as 'hellouser@host:~$'

- **Example:** terminal-demo
- **Scenario:** console-terminal-echo
- **Act:** In terminal-demo, Tab to the terminal, type 'echo hello' with real keys, Enter; then 'echo one; echo two', Enter
- **The reader should get:** The output 'hello' is heard as its own line, separate from the next prompt; 'one' and 'two' as two lines
- **The reader got (`261a218f`):** Each row is its own text-run source ending in LineEnd::EndOfText, so the terminal's text is the rows concatenated with nothing between them. AT-SPI's text-changed:insert carries 'hellouser@host:~$' and 'onetwouser@host:~$', and Orca's terminal script speaks the insertion as one word. Line navigation itself is right: get\_string\_at\_offset(LINE) returns one line per row.
- **Platform:** Linux, measured. Windows and macOS (from source): accesskit\_windows text.rs:520-529 GetText writes Range::write\_text, and macOS node.rs:772/845 uses range.text(), the same concatenation, so any client reading or diffing the whole text there gets the same run-together words
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The terminal's text still joins its rows with nothing between them. After 'echo hello' the insertion is 'hellouser@host:~$' and Orca says it as one word; 'echo one; echo two' gives 'onetwouser@host:~$', and a three-line printf gives 'alphabetagammauser@host:~$'. Line navigation is still right: one line per row.
- **Measured again:** console-terminal-echo, 2 of 2 runs; verify-console-terminal-ls, 2 of 2 runs; the same fused text in every terminal run
- **Evidence (`c198e4d1`):**
  - `pass1 console-terminal-echo 'Enter': +21.1 ms object:text-changed:insert [terminal] 'Demo shell' text='hellouser@host:~$' / +34.2 ms ORCA SAYS: 'hellouser@host:~$'`
  - `pass1 console-terminal-echo 'type 'echo one; echo two' and Enter': +664.8 ms object:text-changed:insert [terminal] 'Demo shell' text='onetwouser@host:~$' / +676.9 ms ORCA SAYS: 'onetwouser@host:~$'`
  - `pass1 console-terminal-echo 'read the terminal's text and lines (fresh AT-SPI client)': text 'user@host:~$ echo hellohellouser@host:~$ echo hellohellouser@host:~$' / line 29-34 'hello'`
  - `pass2 console-terminal-echo 'Enter': object:text-changed:insert [terminal] 'Demo shell' text='hellocyril@...' / +184.7 ms ORCA SAYS: 'hellouser@host:~$'`
  - `pass2 verify-console-terminal-ls: +1191.0 ms object:text-changed:insert [terminal] 'Demo shell' text='alphabetagammauser@host:~$' / +2842.7 ms ORCA SAYS: 'alphabetagammauser@host:~$'`
- **Where (`c198e4d1`):** crates/teksilo-terminal/src/a11y.rs:91-100
- **Evidence (`261a218f`):**
  - `console-terminal-echo report.txt act 'Enter': '+16.6 ms object:text-changed:insert [terminal] 'Demo shell' text='hellouser@host:~$'' ; '+33.1 ms ORCA SAYS: 'hellouser@host:~$''`
  - `orca-debug.out: 14:08:16.607254 - SPEECH OUTPUT: 'hellouser@host:~$'`
  - `act 'echo one; echo two': '+671.4 ms object:text-changed:insert [terminal] 'Demo shell' text='onetwouser@host:~$'' ; orca-debug.out 14:08:26.416309 - SPEECH OUTPUT: 'onetwouser@host:~$'`
  - `fresh AT-SPI probe: text 'user@host:~$ echo hellohellouser@host:~$ echo hellohellouser@host:~$' ; line 0-29 'user@host:~$ echo hello' ; line 29-34 'hello' ; line 34-63 ...`
  - `orca-debug.out: 'TERMINAL: Insertion is believed to be due to terminal command' (orca/scripts/terminal/script.py:89-99 speaks the inserted string whole)`
  - `` crates/teksilo-terminal/src/a11y.rs:95-100: 'Emitting a break here would put characters in the accessible text that the grid does not hold.' / `end: LineEnd::EndOfText,` ``
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:128-129 old/new text = node.document_range().text()`
  - `console-terminal-echo-20260925-144223-811667 'Enter': '+18.6 ms object:text-changed:insert [terminal] 'Demo shell' text='hellouser@host:~$'' ; '+39.6 ms ORCA SAYS: 'hellouser@host:~$''`
  - `same run line probe: text 'user@host:~$ echo hellohellouser@host:~$ echo hellohellouser@host:~$' ; line 0-29 'user@host:~$ echo hello' ; line 29-34 'hello'`
  - `'echo one; echo two': object:text-changed:insert 'onetwouser@host:~$' and Orca says it (3 of 3)`
  - `verify-console-terminal-ls-20260925-145224-987106: '+1065.5 ms object:text-changed:insert [terminal] 'Demo shell' text='alphabetagammauser@host:~$'' ; '+1076.8 ms ORCA SAYS: 'alphabetagammauser@host:~$''`
- **Reproduced:** 4 of 4 echo runs (and in every late/scroll/flood run)
- **Verification:** confirmed. Reproduced: 3 of 3 echo runs, 2 of 2 printf runs (verify-console-terminal-ls), and in every scroll/late/flood run
- **Fix idea:** End every row except the last with a one-character hard break (LineEnd::HardBreak), as the code editor's walk does (code\_editor/a11y.rs emit\_block). A reader's text then has a line separator where the grid has a row boundary. The 'the grid does not hold it' objection costs less than run-together speech.

### console-04 {#console-04}

On a full terminal screen every new line re-reads the whole screen, run together, and the live region falls silent

- **Example:** terminal-demo
- **Scenario:** console-terminal-scroll, console-terminal-flood
- **Act:** Fill the screen ('seq 1 60'), then run 'echo hello', 'sleep 1; echo late' and 'for i in 1 2 3 4 5 6; do echo line$i; sleep 0.4; done'
- **The reader should get:** Each new output line is heard once, as on an empty screen
- **The reader got (`261a218f`):** The accessible text holds only the visible screen, so a one-line scroll changes every row. The AT-SPI adapter's prefix/suffix diff then sends a delete and an insert of nearly the whole screen, and Orca speaks the whole insertion: '313233...5960user@host:~$ echo hellohellouser@host:~$'. The paced loop gets six such utterances, 885 characters in all, for six five-letter lines, and 'line1' is never heard as a word. Because the cursor row no longer changes, no live announcement is sent at all.
- **Platform:** Linux, measured. Windows gets UIA TextChanged with no content (adapter.rs:55-75), so what NVDA reads there depends on its own diffing and was not measured
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** On a full screen each new line still re-reads nearly the whole screen, run together. 'echo hello' on a full screen is heard as '313233...5960user@host:~$ echo hellohellouser@host:~$', and the paced six-line loop gives six such utterances, 885 to 891 characters in all. No live announcement is sent while the cursor stays on the last row.
- **Measured again:** console-terminal-scroll, 2 of 2 runs; console-terminal-flood, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-terminal-scroll 'type 'echo hello' and Enter on a full screen': +380.6 ms object:text-changed:insert [terminal] 'Demo shell' text='313233343536373839404142434445464748495051525354555657585960user@host:~$ echo hellohellouser@host:~$' / +389.1 ms ORCA SAYS: '313233343536373839404142434445464748495051525354555657585960user@host:~$ echo hellohellouser@host:~$' / no object:announcement in the act`
  - `pass2 console-terminal-scroll 'type 'echo hello' and Enter on a full screen': +396.7 ms ORCA SAYS: '313233343536373839404142434445464748495051525354555657585960user@host:~$ echo hellohellouser@host:~$' / no object:announcement in the act`
  - `pass1 console-terminal-flood 'paced output on a full screen': FAIL each line heard, output speech at most 400 characters / after the command: 8 utterances, 891 characters / announcements: []`
  - `pass2 console-terminal-flood 'paced output on a full screen': after the command: 7 utterances, 885 characters / announcements: []`
  - `pass1 console-terminal-scroll 'type 'sleep 1; echo late' and Enter on a full screen': +1695.9 ms ORCA SAYS: '33343536373839404142434445464748495051525354555657585960user@host:~$ echo hellohellouser@host:~$ sleep 1; echo latelateuser@host:~$' / no object:announcement in the act`
- **Where (`c198e4d1`):** crates/teksilo-terminal/src/terminal.rs:609-636; crates/teksilo-terminal/src/a11y.rs (visible screen only)
- **Evidence (`261a218f`):**
  - `console-terminal-scroll act 'echo hello on a full screen': '+378.8 ms object:text-changed:insert [terminal] 'Demo shell' text='313233343536373839404142434445464748495051525354555657585960user@host:~$ echo hellohellouser@host:~$'' ; 'no object:announcement in the act'`
  - `orca-debug.out: 14:11:25.964986 - SPEECH OUTPUT: '313233343536373839404142434445464748495051525354555657585960user@host:~$ echo hellohellouser@host:~$'`
  - `console-terminal-flood 'paced output on a full screen': FAIL each line heard ... 'after the command: 7 utterances, 885 characters' ; 'announcements: []'`
  - `orca-debug.out (flood): 14:34:36.894325 - SPEECH OUTPUT: '313233343536373839404142434445464748495051525354555657585960user@host:~$ fo...' ; 14:34:37.299094 - SPEECH OUTPUT: '3233343536...' ; 14:34:37.715503 ; 14:34:38.123636 ; 14:34:38.524387 ; 14:34:38.921963 (one whole screen each 0.4 s)`
  - ``crates/teksilo-terminal/src/terminal.rs:622 `let announcement = if cursor_line != st.prev_cursor_line {`: false for as long as the cursor sits on the last row``
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:131-181 common-prefix / common-suffix diff of the whole document text`
  - `console-terminal-scroll-20260925-144608-811667 'echo hello on a full screen': insert '313233343536373839404142434445464748495051525354555657585960user@host:~$ echo hellohellouser@host:~$' and Orca says it whole; no object:announcement`
  - `console-terminal-flood-20260925-144906-1000105 / -144940 / -145013 'paced output on a full screen': 'after the command: 7 utterances, 885 characters' ; ANN []`
- **Reproduced:** 3 of 3 scroll runs, 3 of 3 paced full-screen runs
- **Verification:** confirmed. Reproduced: 3 of 3 scroll runs, 3 of 3 paced full-screen runs
- **Fix idea:** Keep the accessible text anchored to the buffer (scrollback lines leaving the top as deletions from the front, new lines appended at the end, e.g. by a stable run id per buffer line) so the adapter's diff is an append. Drive the live region from the engine's newly completed lines rather than from a change of the cursor row.

### console-05 {#console-05}

The terminal's live region announces the line the cursor left (usually the command the user typed), not the output; repeated output is never re-announced

- **Example:** terminal-demo
- **Scenario:** console-terminal-echo, console-terminal-scroll, console-terminal-late
- **Act:** Type 'echo hello' + Enter, again 'echo hello' + Enter, 'echo one; echo two' + Enter, and 'seq 1 60' + Enter
- **The reader should get:** The Status live region ('each newly-completed output line') announces 'hello', 'hello' again, 'one' and 'two'
- **The reader got (`261a218f`):** It announces the row the cursor left, read from the new snapshot: 'user@host:~$ echo hello', which is the command the user just typed. Output that arrives in the same drain as the command echo (the usual case) is never announced. The second identical command announces nothing, because the node's name does not change. After 'seq 1 60' it announced '30', a row from the middle of the output. Only output arriving in a later drain ('sleep 1; echo late') is announced right.
- **Platform:** Linux, measured. The name-must-change rule is the same on Windows (accesskit\_windows adapter.rs:313-320) and macOS (event.rs:301-310) (from source)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The Status live region still announces the row the cursor left. Enter after 'echo hello' announces 'user@host:~$ echo hello', not 'hello'. The same command again announces nothing. 'echo one; echo two' announces the command line, and 'seq 1 60' announces '30'. Only output that arrives in a later drain ('sleep 1; echo late' on an empty screen) is announced right.
- **Measured again:** console-terminal-echo, 2 of 2 runs; console-terminal-scroll, 2 of 2 runs; console-terminal-late, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-terminal-echo 'Enter': +21.7 ms object:announcement [status bar] 'user@host:~$ echo hello' text='user@host:~$ echo hello' / +59.0 ms ORCA SAYS: 'user@host:~$ echo hello'`
  - `pass1 console-terminal-echo 'type 'echo hello' again and Enter': FAIL the bus announces the line 'hello' on its own / no object:announcement in the act`
  - `pass1 console-terminal-echo 'type 'echo one; echo two' and Enter': +665.2 ms object:announcement [status bar] 'user@host:~$ echo one; echo two' text='user@host:~$ echo one; echo two'`
  - `pass2 console-terminal-echo 'Enter': +60.3 ms object:announcement [status bar] 'user@host:~$ echo hello' / 'type 'echo hello' again and Enter': no object:announcement in the act`
  - `pass1 console-terminal-scroll 'scene: fill the screen with 'seq 1 60'': +335.0 ms object:announcement [status bar] '30' text='30' / +354.6 ms ORCA SAYS: '30'`
  - `pass2 console-terminal-scroll 'scene: fill the screen with 'seq 1 60'': +330.5 ms object:announcement [status bar] '30' text='30'`
  - `pass1 console-terminal-late 'type 'sleep 1; echo late' and Enter': +1677.5 ms object:announcement [status bar] 'late' text='late' / pass the bus announces the line 'late' on its own`
- **Where (`c198e4d1`):** crates/teksilo-terminal/src/terminal.rs:609-636
- **Evidence (`261a218f`):**
  - `console-terminal-echo act 'Enter': '+17.2 ms object:announcement [status bar] 'user@host:~$ echo hello' text='user@host:~$ echo hello'' ; '+58.2 ms ORCA SAYS: 'user@host:~$ echo hello''`
  - `act 'type echo hello again and Enter': FAIL the bus announces the line 'hello' on its own / 'no object:announcement in the act'`
  - `act 'echo one; echo two': '+671.9 ms object:announcement [status bar] 'user@host:~$ echo one; echo two'' (no 'one', no 'two')`
  - `console-terminal-scroll 'fill the screen': '+339.2 ms object:announcement [status bar] '30' text='30'' ; orca-debug.out 14:11:20.266073 - SPEECH OUTPUT: '30'`
  - `console-terminal-late: '+1677.8 ms object:announcement [status bar] 'late' text='late'' (the one shape that works, 3 of 3)`
  - `crates/teksilo-terminal/src/terminal.rs:622-627 announce row_text(&st.snapshot, st.prev_cursor_line) when the cursor row changed; :634 last_output_line.set(line)`
  - `` accesskit_atspi_common-0.20.0/src/node.rs:610-622 announcement only `if name != old.name()` ``
  - `console-terminal-echo-20260925-144223-811667 'Enter': '+18.5 ms object:announcement [status bar] 'user@host:~$ echo hello'' ; '+33.2 ms ORCA SAYS: 'user@host:~$ echo hello''`
  - `same run 'type echo hello again and Enter': no object:announcement`
  - `console-terminal-scroll-20260925-144608-811667 'fill the screen': ANN 'user@host:~$ seq 1 60' then ANN '30'`
  - `console-terminal-late-20260925-144439-811667: '+1694.7 ms object:announcement [status bar] 'late''`
- **Reproduced:** wrong line 4 of 4 echo runs; repeat not announced 4 of 4; '30' 3 of 3 scroll runs
- **Verification:** confirmed. Reproduced: wrong line 3 of 3 echo runs; repeat not announced 3 of 3; '30' 3 of 3 scroll runs; '70' 4 of 4 seq 1 100 runs; 'late' right 3 of 3
- **Fix idea:** Announce the lines completed since the last drain (all of them, joined, and skipping the line holding the user's own input), taken from the engine's buffer, not from a cursor-row delta. Make a repeat distinguishable, for example by announcing through ctx.announce (after the K2 fix) rather than a name that must change.

### console-06 {#console-06}

The terminal's way out (Ctrl+Tab) is published only as keyboard\_shortcut, which no AccessKit adapter exports

- **Example:** terminal-demo
- **Scenario:** console-terminal-hint
- **Act:** Launch terminal-demo and read the terminal node; Tab to the terminal
- **The reader should get:** A reader can find out that Tab goes to the shell and Ctrl+Tab leaves (WCAG 2.1.2 asks for the non-standard exit to be made known)
- **The reader got (`261a218f`):** The terminal node has no description, no attribute and no Action interface; on focus Orca says only the current line 'user@host:~$' (its terminal formatting). 'Ctrl+Tab' is nowhere on the bus. The same field carries every .access\_shortcut\_id / .access\_shortcut\_literal in the framework, so those chords are never announced either.
- **Platform:** Linux, measured. Windows and macOS from source: accesskit\_windows-0.35.0 and accesskit\_macos-0.27.0 never read keyboard\_shortcut (and neither do accesskit\_atspi\_common-0.20.0 or accesskit\_consumer-0.39.0)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The terminal's way out is still published only as a keyboard shortcut that no adapter exports. The terminal node reaches AT-SPI with no description, no attributes and no Action interface. On focus Orca says only 'user@host:~$'. Ctrl+Tab itself works: it leaves to 'Clear' and Ctrl+Shift+Tab to 'Scroll to bottom'.
- **Measured again:** console-terminal-hint, 2 of 2 runs; console-terminal-escape, 2 of 2 runs; tabwalk-terminal-demo, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-terminal-hint 'read the terminal node': FAIL the terminal node carries 'Ctrl+Tab' somewhere a reader can read it / terminal node: description=None attributes=None actions=None interfaces=['Accessible', 'Component', 'Text']`
  - `pass2 console-terminal-hint 'read the terminal node': terminal node: description=None attributes=None actions=None interfaces=['Accessible', 'Component', 'Text']`
  - `pass1 tabwalk-terminal-demo 'Tab 4': +11.6 ms object:state-changed:focused 1 [terminal] 'Demo shell' / +53.0 ms ORCA SAYS: 'user@host:~$'`
  - `pass1 console-terminal-escape 'Ctrl+Tab': +28.1 ms object:state-changed:focused 1 [push button] 'Clear' / +74.8 ms ORCA SAYS: 'Clear push button.'`
- **Where (`c198e4d1`):** crates/teksilo-terminal/src/a11y.rs:62-66
- **Evidence (`261a218f`):**
  - `console-terminal-hint: FAIL the terminal node carries 'Ctrl+Tab' somewhere a reader can read it / 'terminal node: description=None attributes=None actions=None interfaces=['Accessible', 'Component', 'Text']'`
  - `tabwalk-terminal-demo 'Tab 4': '+6.3 ms object:state-changed:focused 1 [terminal] 'Demo shell'' ; '+45.3 ms ORCA SAYS: 'user@host:~$''`
  - `crates/teksilo-terminal/src/a11y.rs:62-66 'Announce it, because a screen-reader user has no other way to discover ...' / builder.set_keyboard_shortcut("Ctrl+Tab")`
  - `grep -rn keyboard_shortcut over accesskit_windows-0.35.0/src, accesskit_macos-0.27.0/src, accesskit_atspi_common-0.20.0/src, accesskit_consumer-0.39.0/src: no match`
  - `crates/teksilo-core/src/widget_builder.rs:277 and widget_tree/accessibility_emit_impl.rs:932 route access_shortcut_* into the same set_keyboard_shortcut`
  - `console-terminal-hint-20260925-145047-1000105 and -145108-1000105: "terminal node: description=None attributes=None actions=None interfaces=['Accessible', 'Component', 'Text']"`
  - `console-terminal-escape (reader-verify): '+38.9 ms object:state-changed:focused 1 [push button] 'Clear'' / '+140.0 ms ORCA SAYS: 'Clear push button.''`
- **Reproduced:** deterministic, 1 of 1 hint run and 3 terminal focus acts
- **Verification:** confirmed. Reproduced: 2 of 2 hint runs (deterministic)
- **Fix idea:** Put the hint in the description (exported by all three adapters), e.g. 'Tab goes to the program; Ctrl+Tab leaves the terminal', and consider the same fallback for access\_shortcut\_\* until AccessKit exports keyboard\_shortcut. Also report the gap upstream.

### console-07 {#console-07}

The code-completion list is silent from its second opening on: its list box comes back under a defunct id; 'expanded' and the active descendant never reach AT-SPI

- **Example:** code\_editor
- **Scenario:** console-editor-completion
- **Act:** In code\_editor, at a fresh line type 'wh' (list opens), Down, Enter; then on a new line Ctrl+Space
- **The reader should get:** Every time the list opens the reader hears it and its highlighted entry, and arrowing speaks each entry
- **The reader got (`261a218f`):** First opening: Orca says 'List with 2 items', 'k while keyword.', and Down 'k where keyword.'. It gets there through the list box's selection-changed, not through the editor. Accepting with Enter is silent. At the second opening (Ctrl+Space) the list box node is reused with the same id the adapter already declared defunct when the popup closed, so Orca drops its selection-changed and says nothing. On every opening the editor reports no expanded change and no active-descendant change.
- **Platform:** Linux, measured. Expanded: accesskit\_atspi\_common-0.20.0 maps no Expanded/Expandable state at all (node.rs:301-376, grep 'expand' finds nothing), an upstream gap. Active descendant: blocked by console-01.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids).
- **Now (`c198e4d1`):** Every opening of the completion list is heard. 'wh' gives 'List with 2 items', 'k while keyword.'; Down gives 'k where keyword.'; Ctrl+Space on a fresh line gives 'List with 10 items', 'k fn keyword.'. Each option takes the platform focus, and Enter reads the accepted word ('Code entry where.'). Two things remain: the editor's expanded state never reaches AT-SPI (the adapter maps none), and because every arrow press rebuilds the rows, Orca finds no common ancestor with the old option and re-reads the window title and the list before each option ('Teksilo — CodeEditor', 'List with 2 items', 'k where keyword.').
- **Measured again:** console-editor-completion, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-editor-completion 'type 'wh'': +54.3 ms object:state-changed:focused 1 [list item] '' / +260.4 ms ORCA SAYS: 'List with 2 items' / +260.5 ms ORCA SAYS: 'k while keyword.'`
  - `pass1 console-editor-completion 'Down': +155.7 ms ORCA SAYS: 'Teksilo — CodeEditor' / +155.8 ms ORCA SAYS: 'List with 2 items' / +155.8 ms ORCA SAYS: 'k where keyword.'`
  - `pass1 console-editor-completion 'Enter accepts': +35.4 ms object:state-changed:focused 1 [entry] 'Code' / +96.0 ms ORCA SAYS: 'Code entry where.'`
  - `pass1 console-editor-completion 'Ctrl+Space on an empty line': +50.9 ms object:state-changed:focused 1 [list item] '' / +135.4 ms ORCA SAYS: 'List with 10 items' / +135.4 ms ORCA SAYS: 'k fn keyword.' / FAIL the editor reports expanded=1`
  - `pass2 console-editor-completion 'Ctrl+Space on an empty line': +509.0 ms ORCA SAYS: 'List with 10 items' / +509.1 ms ORCA SAYS: 'k fn keyword.'`
  - `pass1 console-editor-completion orca-debug.out: '16:04:32.319595 - SCRIPT UTILITIES: Common ancestor of [list item] and [list item] is None' before '16:04:32.373060 - SPEECH OUTPUT: 'Teksilo — CodeEditor''`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor/widget.rs:820-823 (one panel id kept across openings); crates/teksilo-widgets/src/code\_editor/completion.rs:585-640; crates/teksilo-core/src/accessibility/adapter\_ids.rs:128-212 (a node that comes back gets a new platform id); crates/teksilo-widgets/src/code\_editor/completion.rs:761 (rows rebuilt on each move)
- **Evidence (`261a218f`):**
  - `act 'Ctrl+Space on an empty line': '+45.0 ms object:selection-changed [list box] ''' ; observed 'Orca ignored an event whose source was defunct' / '14:35:16.107266 EVENT MANAGER: Ignoring defunct object: [list box]'`
  - `orca-debug.out: 14:25:39.394482 - EVENT MANAGER: object:selection-changed for [list box] in [application: 'code_editor'] (0, 0, 0) is not obsoleted / 14:25:39.394527 - EVENT MANAGER: Ignoring defunct object: [list box]`
  - `tree after Ctrl+Space: {"name": "", "role": "list box", "states": ["defunct", "enabled", "sensitive", "showing", "vertical", "visible"]} with 30 list items setsize 30`
  - `first opening: orca-debug.out 14:35:00.071996 - SPEECH OUTPUT: 'List with 2 items' ; 14:35:00.072020 - SPEECH OUTPUT: 'k while keyword.'`
  - `every opening: FAIL the editor reports expanded=1 / 'no expanded change' ; FAIL a object:active-descendant-changed event`
  - `each arrow press rebuilds every row: 'object:children-changed:remove' x2, 'object:state-changed:defunct 1 [label] 'where'' ... (8 defunct events per Down)`
  - `crates/teksilo-widgets/src/code_editor/widget.rs:794-797 one persistent panel_id made once (add_deferred + set_dormant + visible_when(open)); completion.rs:594 materialize_now / :635 dismiss_overlay_by_content reuse it`
  - `crates/teksilo-widgets/src/code_editor/completion.rs:761 selection bound at BindingLevel::Rebuild (new row ids on every move)`
  - `console-editor-completion-20260925-144543-813521 orca-debug.out: '14:46:17.026460 - EVENT MANAGER: object:selection-changed for [list box] ... is not obsoleted' / '14:46:17.026492 - EVENT MANAGER: Ignoring defunct object: [list box]'`
  - `tree-Ctrl-Space-on-an-empty-line.txt: "[unknown] '' {defunct}" / "[list box] '' {defunct,vertical}" with list items setsize 30`
  - `same Orca drop in -144634-813521 (14:47:07.342109) and -144724-813521 (14:47:57.735433)`
  - `first opening: '14:46:01.218787 - SPEECH OUTPUT: 'List with 2 items'' / '14:46:01.218821 - SPEECH OUTPUT: 'k while keyword.''`
- **Reproduced:** 4 of 4 completion runs (second opening dropped as defunct each time)
- **Verification:** corrected by the verifier. Reproduced: 3 of 3 completion runs (second opening dropped as defunct each time) The core claim reproduces 3 of 3. The first opening is heard through the list box's selection-changed ('List with 2 items', 'k while keyword.'; Down gives 'k where keyword.'). At the second opening (Ctrl+Space) the list box is on the bus with state 'defunct', and Orca logs 'Ignoring defunct object: \[list box\]', so nothing is heard. The cause is a persistent panel id reused across openings (widget.rs:794-797, completion.rs:594/635). A second node of the same deferred wrapper, \[unknown\] '', is also defunct. Corrections: the 'expanded' half is upstream on Linux. atspi\_common 0.20.0 maps no expanded state at all (node.rs:301-376 has no expand), although the editor body does set it (code\_editor.rs:388-390). accesskit\_windows does map it (node.rs:714-720). The active-descendant half is a consequence of console-01, not a defect of its own. So the framework defect is the defunct-id reuse. I found two more problems in the same popup and list them as missed (console-V2, console-V3).
- **Fix idea:** Give the panel's AT subtree fresh node ids per opening (the same cure as K2's reserved nodes), or keep it in the tree hidden-but-not-removed. Once console-01 is fixed, the active descendant on the focused entry will make each option the platform focus.

### console-08 {#console-08}

LogView::announce\_appends(true) cannot announce anything on any adapter

- **Example:** terminal-demo, log\_view, code\_editor
- **Act:** Source reading only: log\_view does not opt in, and a rebuild is forbidden here
- **The reader should get:** An app that opts in hears new lines
- **The reader got (`261a218f`):** The opt-in sets Live::Polite on the log's Document node, which has no name, and changes only its text runs. All three adapters announce a live node only when it has a name and that name changes (or it appears with one), so the opt-in is a no-op everywhere.
- **Platform:** all three, from source (not measured)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** announce\_appends(true) still marks the log's Document node live and changes only its text runs. The node can now carry a name (LogView::label), but that name never changes with an append, and every adapter announces a live node only when its name appears or changes. So the opt-in still announces nothing.
- **Measured again:** read from source
- **Evidence (`c198e4d1`):**
  - ``crates/teksilo-widgets/src/code_editor/log_view.rs:876-878: `if st.announce_appends { builder.inner_mut().set_live(Live::Polite); }` on the body's Document node``
  - `accesskit_atspi_common-0.21.0/src/node.rs:624-633 (Announcement only when the name changes) and adapter.rs:74-76 (on add, only with a name)`
  - `accesskit_windows-0.35.1/src/adapter.rs:314-322 (LiveRegionChanged needs a changed name)`
  - `accesskit_macos-0.27.1/src/event.rs:237-239 and :301 (announcement from a label that appears or changes)`
  - `crates/teksilo-widgets/src/code_editor/tests.rs:642 still only asserts the default is off`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor/log\_view.rs:212-222, 876-878
- **Evidence (`261a218f`):**
  - ``crates/teksilo-widgets/src/code_editor/log_view.rs:836-840: `if st.announce_appends { builder.inner_mut().set_live(Live::Polite); }` on the nameless Document``
  - ``accesskit_atspi_common-0.20.0/src/node.rs:610-622 (Announcement only when name != old.name()) and adapter.rs:72-77 (on add, only `if let Some(name)`)``
  - ``accesskit_windows-0.35.0/src/adapter.rs:256 and :313-320 (LiveRegionChanged needs `new_name.is_some()` and a changed name)``
  - `accesskit_macos-0.27.0/src/event.rs:236-240 and :301-310 (announcement only from a label that changed)`
  - `the only test touching it, crates/teksilo-widgets/src/code_editor/tests.rs:642, asserts the default is off`
- **Reproduced:** source only; not run (the demo does not call announce\_appends and building is not allowed in this worktree)
- **Verification:** confirmed. Reproduced: source only (all three adapters read); not run
- **Fix idea:** Announce appended lines through a separate named Status node (or ctx.announce once K2 is fixed), throttled or batched, rather than marking the document itself live.

### console-09 {#console-09}

The code editor and the log have no accessible name, and no API can give their text node one

- **Example:** code\_editor
- **Scenario:** console-editor-focus, console-log-focus
- **Act:** Launch code\_editor / log\_view (launch audit)
- **The reader should get:** The editor and the log carry names ('Code', 'Log'), so a reader knows which text they are in
- **The reader got (`261a218f`):** Audit: an unnamed focusable entry. CodeEditor and LogView expose no label builder, and the WidgetBuilder .access\_label an app could use lands on the wrapper (console-01), not on the entry/document that holds the text
- **Platform:** Linux measured; Windows/macOS the same node from source
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `731cc2e1` (editor-focus).
- **Now (`c198e4d1`):** Both text nodes are named. The editor reaches AT-SPI as \[entry\] 'Code' and the log as \[document frame\] 'Log'; Orca says 'Code entry ...' and 'Log document frame ...'. CodeEditor, PlainTextEditor and LogView now have a .label(..) builder, and an .access\_label on the widget reaches the same node.
- **Measured again:** tabwalk-code\_editor and tabwalk-log\_view, 2 of 2 runs; console-editor-focus and console-log-focus, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 tabwalk-code_editor tree-launch: "[entry] 'Code' desc='Tab indents. Ctrl+Tab moves to the next control, Ctrl+Shift+Tab to the previous one.' {editable,focusable,multi-line,selectable-text}"`
  - `pass1 tabwalk-log_view tree-launch: "[document frame] 'Log' {focusable} text='[00:00:00.000] WARN net: event 0 handled in 0ms\n[00:00:00.00'"`
  - `pass1 console-editor-focus 'Tab to the editor': +61.1 ms ORCA SAYS: 'Code entry // A little Teksilo widget. Edit me!'`
  - `pass1 console-log-focus 'Tab to the log': +56.4 ms ORCA SAYS: 'Log document frame [00:00:00.000] WARN net: event 0 handled in 0ms.'`
  - `pass1 and pass2 tabwalk-code_editor, tabwalk-log_view: no unnamed-control in the launch audit`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor/widget.rs:170-184 (CodeEditor::label), :743-744 (name put on the body), :1155 (PlainTextEditor::label); crates/teksilo-widgets/src/code\_editor/log\_view.rs:140-153, :490-495; examples/code\_editor/src/main.rs:177; examples/log\_view/src/main.rs:119
- **Evidence (`261a218f`):**
  - `tabwalk-code_editor launch audit: 'unnamed-control: [entry] '': a focusable entry with no name (its text is '// A little Teksilo widget. Edit me!...'`
  - `log tree: "[document frame] '' {focusable}"`
  - `python3 tools/extract_widget_api.py CodeEditor / LogView: no label/name builder`
  - `examples/code_editor/src/main.rs:165-171 and examples/log_view/src/main.rs:117-120 give none`
- **Reproduced:** deterministic, every launch (3 code\_editor + 8 log\_view runs)
- **Verification:** confirmed. Reproduced: deterministic, every launch (4 code\_editor, 6 log\_view runs here)
- **Fix idea:** Add .label(..) to CodeEditor/PlainTextEditor/LogView that sets the body's name (or route .access\_label from the wrapper to the body), and name the demos.

### console-10 {#console-10}

Terminal output is heard twice (text insertion + live announcement), and the typed command is read back after Enter

- **Example:** terminal-demo
- **Scenario:** console-terminal-late, console-terminal-flood
- **Act:** 'sleep 1; echo late' + Enter; the paced 'for ... echo line$i; sleep 0.4' loop on an empty screen
- **The reader should get:** Each output line is heard once
- **The reader got (`261a218f`):** Each late line is spoken by Orca's terminal script from the text insertion and again from the Status announcement ('line2', 'line2', 'line3', 'line3' ...; 'lateuser@host:~$' then 'late'). Every Enter also reads back the command line just typed (console-05).
- **Platform:** Linux, measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Each late line is still heard twice: once from the text insertion and once from the Status announcement. In the paced loop on an empty screen, line2 to line6 are each spoken twice. 'sleep 1; echo late' gives 'lateuser@host:~$' and 'late' (in either order). Every Enter also reads back the typed command.
- **Measured again:** console-terminal-flood, 2 of 2 runs; console-terminal-late, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-terminal-flood 'paced output on an empty screen': +2353.9 ms object:text-changed:insert [terminal] 'Demo shell' text='line2' / +2354.4 ms object:announcement [status bar] 'line2' / +2363.5 ms ORCA SAYS: 'line2' / +2380.3 ms ORCA SAYS: 'line2'`
  - `pass2 console-terminal-flood 'paced output on an empty screen': +2357.4 ms ORCA SAYS: 'line2' / +2362.3 ms ORCA SAYS: 'line2' (and the same for line3 to line6)`
  - `pass1 console-terminal-late 'type 'sleep 1; echo late' and Enter': +670.5 ms ORCA SAYS: 'user@host:~$ sleep 1; echo late' / +1684.5 ms ORCA SAYS: 'lateuser@host:~$' / +1694.3 ms ORCA SAYS: 'late'`
  - `pass2 console-terminal-late 'type 'sleep 1; echo late' and Enter': +1692.2 ms ORCA SAYS: 'late' / +1698.6 ms ORCA SAYS: 'lateuser@host:~$'`
- **Where (`c198e4d1`):** crates/teksilo-terminal/src/a11y.rs LiveAnnouncer + crates/teksilo-terminal/src/terminal.rs:609-636
- **Evidence (`261a218f`):**
  - `console-terminal-flood 'paced output on an empty screen': '+2349.8 ms object:announcement [status bar] 'line2' text='line2'' ; '+2350.0 ms object:text-changed:insert [terminal] 'Demo shell' text='line2'' ; '+2361.3 ms ORCA SAYS: 'line2'' ; '+2369.9 ms ORCA SAYS: 'line2''`
  - `orca-debug.out: 14:34:25.317600 - SPEECH OUTPUT: 'line2' / 14:34:25.326187 - SPEECH OUTPUT: 'line2'`
  - `console-terminal-late: '+1684.5 ms object:announcement [status bar] 'late'' ; '+1698.1 ms ORCA SAYS: 'lateuser@host:~$'' ; '+1714.0 ms ORCA SAYS: 'late''`
  - `utterances (flood run): ['line1', 'user@host:~$ for i in 1 2 3 4 5 6;...', 'line2', 'line2', 'line3', 'line3', 'line4', 'line4', 'line5', 'line5', 'line6', 'line6', 'user@host:~$']`
  - `console-terminal-flood-20260925-144940-1000105 says: ['line1', 'user@host:~$ for i in ...', 'line2', 'line2', 'line3', 'line3', 'line4', 'line4', 'line5', 'line5', 'line6', 'line6', 'user@host:~$']`
  - `console-terminal-late-20260925-144439-811667: '+1706.9 ms ORCA SAYS: 'late'' / '+1712.8 ms ORCA SAYS: 'lateuser@host:~$''`
- **Reproduced:** 3 of 3 paced empty-screen runs; 3 of 3 late runs
- **Verification:** confirmed. Reproduced: 3 of 3 late runs, 3 of 3 paced empty-screen runs
- **Fix idea:** Orca already reads a Role::Terminal's insertions (its terminal script), so the live region duplicates it there. Either announce only while the terminal does not have focus, or drop the live region for AT-SPI and keep it where the platform reader does not track terminal text.

### console-11 {#console-11}

A second caret is invisible to a reader, and typing at two carets is published as a delete and re-insert of everything between them

- **Example:** code\_editor
- **Scenario:** console-editor-multicaret
- **Act:** Ctrl+Home, Ctrl+Alt+Down (add caret), type 'X'
- **The reader should get:** The reader is told there are now two carets, and the edit arrives as two one-character insertions
- **The reader got (`261a218f`):** Only the demo's status label changes ('char 0 · 2 carets', not live). The edit arrives as one text-changed:delete of the whole first line and one insert of 'X// A little Teksilo widget. Edit me!\\nX'. Orca said nothing here (console-01). A client that reads event content hears the whole span as deleted and inserted.
- **Platform:** Linux, measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** A second caret is still told to a reader only through the demo's status label ('char 0 · 2 carets'), which is not live. Typing X at two carets still arrives as a delete of the whole first line and an insert of 'X// A little Teksilo widget. Edit me!\\nX'. Orca says nothing for it.
- **Measured again:** console-editor-multicaret, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-editor-multicaret 'Ctrl+Alt+Down adds a caret below': only +47.8 ms object:property-change:accessible-name [label] 'char 0 · 2 carets'`
  - `pass1 console-editor-multicaret 'type 'X' at both carets': +47.3 ms object:text-changed:delete [entry] 'Code' text='// A little Teksilo widget. Edit me!\n' / +47.4 ms object:text-changed:insert [entry] 'Code' text='X// A little Teksilo widget. Edit me!\nX'`
  - `pass2 console-editor-multicaret 'type 'X' at both carets': +105.8 ms object:text-changed:delete [entry] 'Code' text='// A little Teksilo widget. Edit me!\n' / +106.0 ms object:text-changed:insert [entry] 'Code' text='X// A little Teksilo widget. Edit me!\nX'`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor/a11y.rs:63-71 (framework); accesskit\_atspi\_common-0.21.0/src/adapter.rs:121-181 (span diff, upstream)
- **Evidence (`261a218f`):**
  - `act 'Ctrl+Alt+Down adds a caret below': only '+40.8 ms object:property-change:accessible-name [label] 'char 0 · 2 carets''`
  - `act 'type X at both carets': '+33.6 ms object:text-changed:delete [entry] '' text='// A little Teksilo widget. Edit me!\n'' / '+34.0 ms object:text-changed:insert [entry] '' text='X// A little Teksilo widget. Edit me!\nX''`
  - `crates/teksilo-widgets/src/code_editor/a11y.rs:62-70 'secondary carets are editing-only and the AT tree reports just the primary'`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:131-181 single common-prefix/suffix diff per update (upstream: two edits in one update become one span)`
- **Reproduced:** 1 of 1 (deterministic, no timing)
- **Verification:** confirmed. Reproduced: 2 of 2 multicaret runs
- **Fix idea:** Announce caret-count changes (e.g. '2 carets') through the announcer, and consider a description on the editor while several carets are live. The span diff is upstream behaviour to report to AccessKit.

### console-12 {#console-12}

When the shell exits, the terminal tells a reader nothing and silently swallows keys

- **Example:** terminal-demo
- **Scenario:** console-terminal-exit
- **Act:** Type 'exit' + Enter; then type 'ls'
- **The reader should get:** The reader hears that the program ended, and the terminal exposes that it no longer runs anything
- **The reader got (`261a218f`):** Orca reads bash's own 'logout' and the command line again (console-05). The demo's status label changes to '○ exited' without being announced, the terminal node keeps the same states (focused, no read-only or other change), and typing 'ls' produces no event at all
- **Platform:** Linux, measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** After 'exit', Orca reads the command line again ('user@host:~$ exit') and bash's own 'logout'. The '○ exited' status label changes without being announced, the terminal node keeps the same states, and typing 'ls' afterwards produces no event and no speech.
- **Measured again:** console-terminal-exit, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-terminal-exit 'type 'exit' and Enter': +168.7 ms object:announcement [status bar] 'user@host:~$ exit' / +188.4 ms object:property-change:accessible-name [label] '○ exited   ·   user@host: ~' / +190.5 ms ORCA SAYS: 'logout' / FAIL Orca says 'exited'`
  - `pass2 console-terminal-exit 'type 'exit' and Enter': +497.8 ms object:property-change:accessible-name [label] '○ exited   ·   user@host: ~' / +715.3 ms ORCA SAYS: 'logout' / FAIL Orca says 'exited'`
  - `pass1 and pass2 console-terminal-exit 'type 'ls' into the dead terminal': no event, no speech`
  - `pass1 console-terminal-exit tree after: "[terminal] 'Demo shell' {focusable,focused} text='user@host:~$ exitlogout'"`
- **Where (`c198e4d1`):** crates/teksilo-terminal/src/terminal.rs:640-656; crates/teksilo-terminal/src/a11y.rs build\_terminal\_a11y
- **Evidence (`261a218f`):**
  - `'+197.5 ms object:property-change:accessible-name [label] '○ exited   ·   user@host: ~'' (not live, not spoken) ; FAIL Orca says 'exited'`
  - `'+198.5 ms ORCA SAYS: 'logout''`
  - `tree after: "[terminal] 'Demo shell' {focusable,focused} text='user@host:~$ exitlogout'"`
  - `act 'type ls into the dead terminal': events {} (no event)`
  - `console-terminal-exit-20260925-145129-1000105: '+171.0 ms ORCA SAYS: 'logout'' ; '+186.2 ms object:property-change:accessible-name [label] '○ exited   ·   user@host: ~'' ; FAIL Orca says 'exited' ; 'type ls': 0 events`
  - `tree after: "[terminal] 'Demo shell' {focusable,focused} text='user@host:~$ exitlogout'"`
- **Reproduced:** 1 of 1 (deterministic)
- **Verification:** confirmed. Reproduced: 2 of 2 exit runs
- **Fix idea:** On child exit, announce it (e.g. 'Process exited, code 0') and mark the node read-only (or set a description), so the reader knows why keys do nothing. The demo's status label could also be a polite live region.

### console-13 {#console-13}

log\_view: Start / Stop gives the reader no state: no pressed state, a static name, a status line that is not live

- **Example:** log\_view
- **Scenario:** console-log-buttons, console-log-stream
- **Act:** Tab to Start / Stop and press Space; press it again (also via AT-SPI click with focus on the log)
- **The reader should get:** The reader hears that streaming started or stopped (a toggle's pressed state, or the status 'streaming' / 'paused' announced)
- **The reader got (`261a218f`):** Nothing is spoken. The only change is the status label's text, which is not a live region and which rewrites itself 12 times a second while streaming
- **Platform:** Linux, measured
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Now (`c198e4d1`):** Pressing Start / Stop still gives the reader nothing about the new state. With Space, Orca says only 'space' on the second press and nothing on the first. The button is a plain push button with no pressed state, and the status label, which is not a live region, rewrites itself about ten times a second while streaming and is never spoken.
- **Measured again:** console-log-buttons, 2 of 2 runs; console-log-stream, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-log-buttons 'Space on Start / Stop': '+200.6 ms object:text-changed:insert [label] '30 generated · 30 retained · paused' text='110 generated · 110 retained · streaming'' ; FAIL Orca says 'streaming', Orca unheard: 'streaming'`
  - `pass1 console-log-buttons 'Space on Start / Stop again': '+183.9 ms ORCA SAYS: 'space'' then '+406.9 ms object:property-change:accessible-name [label] '2870 generated · 2870 retained · paused'' ; FAIL Orca says 'paused'`
  - `pass2 console-log-buttons: the same two FAILs, Orca said: 'space' only`
  - `pass1 console-log-stream 'Start / Stop through AT-SPI, focus left on the log': 37 'object:property-change:accessible-name [label] ...' in 3.9 s, no speech; FAIL Orca says 'streaming'`
  - `pass2 console-log-stream, same act: 41 label name changes in 3.9 s, no speech`
  - `pass1 console-log-stream tree after the click: '[push button] 'Start / Stop' {focusable}' (no pressed or checked state), '[label] '2070 generated · 2070 retained · streaming''`
- **Where (`c198e4d1`):** examples/log\_view/src/main.rs:144-168, 212-216
- **Evidence (`261a218f`):**
  - `console-log-buttons 'Space on Start / Stop': '+239.5 ms object:text-changed:insert [label] '30 generated · 30 retained · paused' text='110 generated · 110 retained · streaming'' ; FAIL Orca says 'streaming'`
  - `'Space on Start / Stop again': FAIL Orca says 'paused'`
  - `console-log-stream: 40 'object:property-change:accessible-name [label] ...' in 4 s and no speech`
  - `examples/log_view/src/main.rs:147-158 (Button, no toggle state), :141-146 + :212-217 (status TextWidget, no live)`
- **Reproduced:** 2 of 2 log-buttons runs, 1 of 1 log-stream run
- **Verification:** confirmed. Reproduced: 2 of 2 log-buttons runs, 1 of 1 log-stream run
- **Fix idea:** Make Start/Stop a toggle (pressed state), or announce 'Streaming' / 'Paused' on activation. Keep the fast-changing counter out of any live region.

### console-14 {#console-14}

Terminal: a typed space is invisible until the next non-blank character, because each row's trailing blanks are trimmed

- **Example:** terminal-demo
- **Scenario:** console-terminal-echo
- **Act:** Type 'echo hello' in the terminal
- **The reader should get:** The space is inserted and the caret moves one cell
- **The reader got (`261a218f`):** No text or caret event for the space. The next letter arrives as ' h' and the caret jumps two cells (23 -&gt; 25). Orca reads ' e' / ' h'. A braille display or a reader asking for the character at the caret after a space gets the wrong position
- **Platform:** Linux, measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** A typed space still sends no text or caret event. The next letter arrives as ' h' and the caret jumps two cells (23 to 25). Orca now also echoes the key, so the reader hears 'space' and then the next letter twice (' h' and 'h'), but the character at the caret after a space is still wrong.
- **Measured again:** console-terminal-echo, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-terminal-echo events.jsonl text-caret-moved offsets: 18, 20, 21, 22, 23, 25, 26, 27, 28, 29 (no 24)`
  - `pass1 console-terminal-echo 'type 'echo hello'': +176.7 ms ORCA SAYS (CUT): 'space' / +197.9 ms object:text-changed:insert [terminal] 'Demo shell' text=' h' / +208.4 ms ORCA SAYS (CUT): ' h' / +220.5 ms ORCA SAYS (CUT): 'h'`
  - `pass2 console-terminal-echo events.jsonl text-caret-moved offsets: 18, 20, 21, 22, 23, 29 (no 24)`
- **Where (`c198e4d1`):** crates/teksilo-terminal/src/a11y.rs:176-190
- **Evidence (`261a218f`):**
  - `events.jsonl text-caret-moved detail1 sequence: 18, 20, 21, 22, 23, 25, 26, 27, 28, 29 (no 24)`
  - `'+212.7 ms object:text-changed:insert [terminal] 'Demo shell' text=' h'' ; 'ORCA SAYS: ' h''`
  - ``crates/teksilo-terminal/src/a11y.rs:180-182 `let trimmed = text.trim_end().len(); text.truncate(trimmed);` and :186-188 caret clamped to the trimmed length``
- **Reproduced:** every echo run (4 of 4)
- **Verification:** confirmed. Reproduced: 3 of 3 echo runs
- **Fix idea:** Trim trailing blanks only past the cursor on the cursor's row (keep blanks up to the cursor column).

### console-15 {#console-15}

Completion options are read with their kind badge as a letter: 'k while keyword.'

- **Example:** code\_editor
- **Scenario:** console-editor-completion
- **Act:** Type 'wh' in the editor (completion opens)
- **The reader should get:** 'while, keyword' (or 'while'), without the badge glyph
- **The reader got (`261a218f`):** Orca builds the option's text from its three labels: badge 'k', label, detail. The badges for other kinds are '•', '☐', '▢', 'ƒ', which Orca would read as symbol names
- **Platform:** Linux, measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Completion options are still read with the kind badge first: 'k while keyword.', 'k where keyword.', 'k fn keyword.'.
- **Measured again:** console-editor-completion, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-editor-completion 'type 'wh'': +260.5 ms ORCA SAYS: 'k while keyword.'`
  - `pass1 console-editor-completion 'Down': +155.8 ms ORCA SAYS: 'k where keyword.'`
  - `pass2 console-editor-completion 'type 'wh'': +274.1 ms ORCA SAYS: 'k while keyword.'`
  - `pass1 console-editor-completion tree-type--wh-.txt: "[list item] '' {focused,selectable,selected}" / "[label] 'k'" / "[label] 'while'" / "[label] 'keyword'"`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor/completion.rs:135-148, 811-826
- **Evidence (`261a218f`):**
  - `orca-debug.out: 14:35:00.072020 - SPEECH OUTPUT: 'k while keyword.' ; 'k where keyword.' on Down`
  - `crates/teksilo-widgets/src/code_editor/completion.rs:135-148 badge(), :811-826 the badge TextWidget is a visible, unhidden label in the row`
- **Reproduced:** 4 of 4 completion runs
- **Verification:** confirmed. Reproduced: 3 of 3 completion runs
- **Fix idea:** a11y\_hidden() the badge and name the option '&lt;label&gt;, &lt;kind&gt;' explicitly.

### console-16 {#console-16}

log\_view 'Burst 10k' freezes the debug-build app for about 50 s, with nothing to tell a reader it is busy

- **Example:** log\_view
- **Scenario:** console-log-burst
- **Act:** AT-SPI click on Burst 10k with focus on the log, then keep asking the bus for the status label
- **The reader should get:** The burst lands promptly (the doc promises a windowed append), or the app reports busy
- **The reader got (`261a218f`):** The main thread burns about 46-53 s of CPU and answers no key and no AT-SPI action meanwhile. The status label reads '10030 generated' only 50-55 s later. No busy state appears on the log
- **Platform:** Linux, debug build only (release not available here)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Burst 10k still freezes the debug build: the status label read '10030 generated' 55.1 s after the click in one run and 95.7 s in the other, with the main thread busy throughout. Nothing reaches the bus meanwhile and the log shows no busy state. Keys work again once it lands.
- **Measured again:** console-log-burst, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-log-burst note: burst: the status label read '10030 generated · 10030 retained · paused' 55.1s after the click (app CPU 55.0s)`
  - `pass1 console-log-burst 'Burst 10k through AT-SPI, focus left on the log': first event +53787.2 ms object:text-changed:delete [label] '30 generated · 30 retained · paused'`
  - `pass2 console-log-burst note: burst: the status label read '10030 generated · 10030 retained · paused' 95.7s after the click (app CPU 90.7s)`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor/log\_stream.rs:282-302 apply\_appends (cause not isolated; possibly text-document)
- **Evidence (`261a218f`):**
  - `note: burst +50s: label '30 generated · 30 retained · paused', app CPU 45.8s; threads log_view[384575]=46.2s, log_view[384674]=0.3s, ...`
  - `note: burst: the status label read '10030 generated · 10030 retained · paused' 50.1s after the click (app CPU 46.6s)`
  - `earlier run: 'burst: the status label read ... 55.1s after the click (app CPU 53.1s)'; a 20 s run: 'scene: Tab round to Burst 10k ... RunError: 10 x Tab never focused'`
  - `the same freeze without Orca (last run), so reader traffic is not the cause`
- **Reproduced:** 4 of 4 burst runs (2 measured at 55.1 s and 50.1 s, 2 that gave up after 4 s and 20 s)
- **Verification:** confirmed. Reproduced: 1 of 1 here (55.1 s); the sweep measured 55.1 s and 50.1 s
- **Fix idea:** Profile a debug and a release burst; if the cost is O(n^2) in append\_lines, batch it. Consider set\_busy on the document while a large append is pending.

### console-17 {#console-17}

Bracket matching and the current-line band are visual only

- **Example:** code\_editor
- **Scenario:** console-editor-brackets
- **Act:** Type '(' in the editor
- **The reader should get:** Some non-visual cue where the matching bracket is (it is the feature's point)
- **The reader got (`261a218f`):** Only '+50.3 ms object:text-changed:insert \[entry\] '' text='()'', nothing about the match
- **Platform:** Linux, measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Typing '(' still gives only the insertion of '()'. Nothing tells a reader where the matching bracket is; Orca says nothing for the insertion.
- **Measured again:** console-editor-brackets, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-editor-brackets 'type '('': +46.0 ms object:text-changed:insert [entry] 'Code' text='()' / FAIL Orca says '('`
  - `pass2 console-editor-brackets 'type '('': +55.9 ms object:text-changed:insert [entry] 'Code' text='()' / FAIL Orca says '('`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor (bracket matching, paint only)
- **Evidence (`261a218f`):**
  - `console-editor-brackets act 'type (': '+50.3 ms object:text-changed:insert [entry] '' text='()''`
  - `examples/code_editor/src/main.rs:17-18 'the caret's bracket and its partner get a faint wash'`
- **Reproduced:** 1 of 1 (deterministic)
- **Verification:** confirmed. Reproduced: 1 of 1 brackets run (deterministic)
- **Fix idea:** Optional: a command that announces or jumps to the matching bracket (as VS Code's accessibility signals do).

### console-18 {#console-18}

docs/log-view.md describes the log's accessibility as it no longer is

- **Example:** terminal-demo, log\_view, code\_editor
- **Act:** Read the doc against the walk
- **The reader should get:** The doc matches the code
- **The reader got (`261a218f`):** The doc says only the visible lines are emitted 'as paragraphs (numbered by global line, "line 41 002 of 128 449")'. code\_editor/a11y.rs says the paragraphs and that ordinal were removed. It also presents announce\_appends as a working opt-in (see console-08)
- **Platform:** documentation
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** docs/log-view.md still says the visible lines are emitted as paragraphs numbered by global line ('line 41 002 of 128 449'), which the code removed, and still presents announce\_appends as a working opt-in. It also does not mention the new label builder.
- **Measured again:** read from source
- **Evidence (`c198e4d1`):**
  - `docs/log-view.md:163-173 unchanged since 261a218f`
  - `crates/teksilo-widgets/src/code_editor/a11y.rs:30 '**Removed with the paragraphs: the "line 42 of 200" ordinal.**'`
- **Where (`c198e4d1`):** docs/log-view.md:161-175
- **Evidence (`261a218f`):**
  - `docs/log-view.md:163-173`
  - `crates/teksilo-widgets/src/code_editor/a11y.rs:29-32 '**Removed with the paragraphs: the "line 42 of 200" ordinal.**'`
- **Reproduced:** source reading
- **Verification:** confirmed. Reproduced: source reading
- **Fix idea:** Rewrite the Accessibility section: runs straight off the document, no ordinal, and what announce\_appends really does.

### console-V1 {#console-v1}

Terminal scrollback never reaches the reader: Shift+PageUp/PageDown leave the accessible text on the old page, and the next output swaps in the scrolled page while the live region announces an old history row

- **Example:** terminal-demo
- **Scenario:** verify-console-terminal-scrolled-output, verify-console-terminal-scrollback
- **Act:** terminal-demo: Tab to the terminal, 'seq 1 100' Enter; then 'sleep 8; echo LATE' Enter, Shift+PageUp at once, and wait for LATE
- **The reader should get:** After Shift+PageUp the terminal's text is the page the view shows, so the reader can review the history they scrolled to. When output arrives while scrolled back, the reader hears that output (or nothing), not a history row.
- **The reader got (`261a218f`):** Shift+PageUp and Shift+PageDown emit no event at all, and a fresh AT-SPI client still reads the bottom page ('7071...100user@host:~$ sleep 8; echo LATE'). The view has scrolled back 33 rows: when LATE arrives, the drain re-walks the tree and the text jumps to '3738...69', sent as a delete of the whole bottom page and an insert of the history page. The live region announces '69', the history row now under the old cursor row, and Orca says '69'. LATE itself is never heard. The AT action path does it right: access\_scroll calls ctx.request\_accessibility\_update() (terminal.rs:1852-1854). The key path (terminal.rs:1224-1232 → scroll\_view :1735-1743) refreshes the snapshot but neither bumps document\_version, the only trigger of the terminal's re-walk (bound at :681-685, bumped only in the drain at :600-608), nor requests an update. So scrollback history cannot be reviewed by keyboard.
- **Platform:** Linux AT-SPI / Orca 46.1, measured. The missing re-walk is in teksilo-terminal, so the stale text is the same on Windows and macOS (from source; not measured there).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Shift+PageUp and Shift+PageDown still leave the accessible text on the bottom page; Orca only echoes 'page up' / 'page down'. When LATE arrives while scrolled back, the text jumps to the history page ('3738...69'), sent as a delete of the bottom page and an insert of the history page. LATE is never heard. In 2 of 3 runs the live region announced '69' and Orca said '69'; in the third it stayed silent because its previous announcement was already '69'.
- **Measured again:** verify-console-terminal-scrollback, 2 of 2 runs; verify-console-terminal-scrolled-output, 3 of 3 runs (the '69' announcement 2 of 3)
- **Evidence (`c198e4d1`):**
  - `pass1 verify-console-terminal-scrolled-output note: after Shift+PageUp: '707172737475767778798081828384858687888990919293949596979899100user@host:~$ sleep 8; echo LATE' ; after LATE: '373839404142434445464748495051525354555657585960616263646566676869'`
  - `pass1 verify-console-terminal-scrolled-output 'LATE arrives while scrolled back': +3813.6 ms object:text-changed:insert [terminal] 'Demo shell' text='373839404142434445464748495051525354555657585960616263646566676869' / +3814.1 ms object:announcement [status bar] '69' text='69' / +3856.3 ms ORCA SAYS: '69'`
  - `pass2 verify-console-terminal-scrolled-output 'LATE arrives while scrolled back': +3912.4 ms object:text-changed:insert [terminal] 'Demo shell' text='373839404142434445464748495051525354555657585960616263646566676869' (no announcement; the fill scene had already announced '69')`
  - `judge run 18:04 verify-console-terminal-scrolled-output 'LATE arrives while scrolled back': +3841.2 ms object:text-changed:insert [terminal] 'Demo shell' text='3738...69' / +3841.4 ms object:announcement [status bar] '69' text='69' / +3884.1 ms ORCA SAYS: '69'`
  - `pass1 verify-console-terminal-scrollback 'Shift+PageUp (scroll back one page)': no event / +20.5 ms ORCA SAYS: 'page up'`
- **Where (`c198e4d1`):** crates/teksilo-terminal/src/terminal.rs:1224-1232, 1735-1743
- **Evidence (`261a218f`):**
  - `verify-console-terminal-scrollback-20260925-144926-987106 and -145128-987106: 'Shift+PageUp (scroll back one page)' 0 events, Orca silent; 'Shift+PageDown' 0 events`
  - `verify-console-terminal-scrolled-output-20260925-145159-1081658 note: after Shift+PageUp: '707172737475767778798081828384858687888990919293949596979899100user@host:~$ sleep 8; echo LATE' ; after LATE: '373839404142434445464748495051525354555657585960616263646566676869'`
  - `same run 'LATE arrives while scrolled back': object:text-changed:delete [terminal] '7071...100user@host:~$ sleep 8; echo LATE' / insert '3738...69' / object:announcement [status bar] '69' ; orca-debug.out '14:52:28.672414 - SPEECH OUTPUT: '69''`
  - `verify-console-terminal-scrolled-output-20260925-145245-1081658: '+3783.3 ms object:text-changed:insert [terminal] 'Demo shell' text='373839...69'' / '+3783.9 ms object:announcement [status bar] '69'' / '+3810.6 ms ORCA SAYS: '69'' ; orca-debug.out '14:53:14.192801 - SPEECH OUTPUT: '69''`
  - `crates/teksilo-terminal/src/terminal.rs:1224-1232 (Shift+PageUp/Down → scroll_view), :1735-1743 (scroll_view: refresh_snapshot + request_frame only), :1846-1855 (access_scroll adds request_accessibility_update)`
- **Reproduced:** 2 of 2 scrolled-output runs; Shift+PageUp/Down silent 2 of 2 scrollback runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Bump document\_version (or call ctx.request\_accessibility\_update()) in scroll\_view, as access\_scroll already does. Also compute the live-region row from the live grid, not the scrolled snapshot, so a scrolled-back view does not announce history.

### console-V2 {#console-v2}

Completion options have no accessible name: the row name-from-content pass cannot see TextWidget text

- **Example:** code\_editor
- **Scenario:** console-editor-completion
- **Act:** code\_editor: type 'wh' at a fresh line (completion opens)
- **The reader should get:** Each \[list item\] is named from its content ('while', or 'while, keyword')
- **The reader got (`261a218f`):** Every option reaches AT-SPI as \[list item\] ''. The emit pass that fills a nameless Role::ListBoxOption/TreeItem from its first named descendant reads `descendant.label()` (accessibility\_emit\_impl.rs:188). But AccessNodeBuilder::build moves a Role::Label's name into `value` and clears `label` (accessibility.rs:1014-1020), so a TextWidget's text is never found. Orca makes up for it by speaking the children ('k while keyword.', badge included, console-15). On Windows the UIA Name comes from the label (read from source), so the option would be nameless there. What NVDA then reads was not measured. The same pass serves every virtualized row whose name comes from content; only the completion rows were measured here.
- **Platform:** Linux, measured (bus name ''); Windows/macOS from source only
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** Every completion option still reaches AT-SPI as \[list item\] '' with no name. Orca makes up for it by reading the row's labels, badge included ('k while keyword.').
- **Measured again:** console-editor-completion, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-editor-completion tree-type--wh-.txt: "[list item] '' {focused,selectable,selected} attrs={'posinset': '1', 'setsize': '2'}" with children "[label] 'k'", "[label] 'while'", "[label] 'keyword'"`
  - `pass1 console-editor-completion tree-Ctrl-Space-on-an-empty-line.txt: "[list item] '' {focused,selectable,selected} attrs={'posinset': '1', 'setsize': '30'}"`
  - `pass1 console-editor-completion 'type 'wh'': +54.3 ms object:state-changed:focused 1 [list item] ''`
- **Where (`c198e4d1`):** crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:217 (the hoist reads descendant.label()); crates/teksilo-core/src/accessibility.rs:1015-1021 (a Role::Label's name moved to value)
- **Evidence (`261a218f`):**
  - `console-editor-completion-20260925-144543-813521 tree-type--wh-.txt: "[list item] '' {selectable,selected} attrs={'setsize': '2', 'posinset': '1'}" with children "[label] 'k'", "[label] 'while'", "[label] 'keyword'"`
  - `crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:176-196 (hoist reads descendant.label())`
  - `crates/teksilo-core/src/accessibility.rs:1014-1020 (Role::Label: label moved to value, clear_label)`
  - `crates/teksilo-widgets/src/primitives/text_widget.rs:958, 998 (Role::Label + set_name)`
- **Reproduced:** 3 of 3 completion runs (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** In the hoist, read `label().or(value())` for a Role::Label descendant (or hoist before build re-serializes). Also name the completion row explicitly ('while, keyword') and a11y\_hide the badge.

### console-V3 {#console-v3}

The editor's `controls` relation points at the completion panel's deferred wrapper (\[unknown\]), not at the list box, and keeps that empty node in the tree

- **Example:** code\_editor
- **Scenario:** console-editor-completion
- **Act:** code\_editor: type 'wh' (completion opens), read the entry's relations
- **The reader should get:** controller-for names the \[list box\]; no nameless \[unknown\] node sits between the editor and the list
- **The reader got (`261a218f`):** The entry's controller-for target is an \[unknown\] '' node, and the \[list box\] is that node's child. code\_editor.rs:395 pushes widget\_id\_to\_node\_id(panel\_id), where panel\_id is the add\_deferred wrapper (widget.rs:794), not the CompletionPanel (the Role::ListBox). Being a relation target also exempts the empty wrapper from presentational pruning (accessibility\_emit\_impl.rs:220-235), so it shows up in the tree, and it is defunct from the second opening on (console-07).
- **Platform:** Linux, measured; the relation is emitted the same for every adapter (source)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Now (`c198e4d1`):** The entry's controller-for still points at an \[unknown\] '' node, with the \[list box\] as its child, and that empty node still sits in the tree. It is no longer defunct at the second opening: it now gets a fresh id each time.
- **Measured again:** console-editor-completion, 2 of 2 runs
- **Evidence (`c198e4d1`):**
  - `pass1 console-editor-completion 'type 'wh'' tree: entry controller-for ['/org/a11y/atspi/accessible/0/79228164801660602733528350720'], which is the [unknown] node; [list box] is /org/a11y/atspi/accessible/0/79228166258953384556582928384`
  - `pass1 console-editor-completion 'Ctrl+Space on an empty line' tree: entry controller-for ['/org/a11y/atspi/accessible/0/158456325028528675187087900672'], again the [unknown] node, with the [list box] below it`
  - `pass1 console-editor-completion tree-type--wh-.txt: "[unknown] ''" / "  [list box] '' {vertical}"`
- **Where (`c198e4d1`):** crates/teksilo-widgets/src/code\_editor.rs:393-395; crates/teksilo-widgets/src/code\_editor/widget.rs:820
- **Evidence (`261a218f`):**
  - `console-editor-completion-20260925-144543-813521 tree after 'wh': entry relations {'controller-for': ['/org/a11y/atspi/accessible/0/79228164801660602733528350720']} ; that path is [unknown] ; [list box] is /org/a11y/atspi/accessible/0/79228166258953384556582928384`
  - `tree-type--wh-.txt: "[unknown] '' {focusable,focused}" / "  [entry] '' ... rel=['controller-for']" / "  [unknown] ''" / "    [list box] '' {vertical}"`
  - `crates/teksilo-widgets/src/code_editor.rs:393-395; crates/teksilo-widgets/src/code_editor/widget.rs:794`
- **Reproduced:** 3 of 3 completion runs (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Point controls at the CompletionPanel's own id (the ListBox node), which the deferred subtree can report once materialized.
