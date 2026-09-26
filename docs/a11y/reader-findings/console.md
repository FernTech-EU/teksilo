<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Terminal, log view and code editor

Examples: `terminal-demo`, `log_view`, `code_editor`.
21 findings: 2 critical, 8 high, 5 medium, 6 low.
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
- **The reader gets:** Focus lands on the widget's outer wrapper, which emits no accessibility of its own and so reaches AT-SPI as role 'unknown' with no name and no Text interface. Orca generates pauses only and says nothing. The \[entry\] / \[document frame\] child is never focused, so accesskit\_atspi\_common never emits text-caret-moved or text-selection-changed for it: Down and typing produce no caret event at all (only the status label changes). Orca also treats the editor's insertions as having no cause and does not speak them (typing, Tab, completion accept are all silent). The same mis-aimed focus is why the completion list's active\_descendant (set on the entry) can never become the platform focus (consumer node.rs:90-102 follows active\_descendant only on the focused node).
- **Platform:** Linux AT-SPI / Orca 46.1, measured. Windows and macOS were not measured: focus is chosen in teksilo-core, not in the adapter, so there too the focused element is the Role::Unknown wrapper with no text pattern (from source). What NVDA or VoiceOver says for it was not read.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `731cc2e1` (editor-focus).
- **Where:** crates/teksilo-widgets/src/code\_editor/widget.rs:568 (focusable wrapper), :989-993; crates/teksilo-widgets/src/code\_editor/log\_view.rs:347-348; crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:125-139
- **Evidence:**
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
- **The reader gets:** Focus never leaves. Ctrl+Tab inserts four spaces into the code, Escape+Tab inserts four more, and Ctrl+Shift+Tab deletes four. Orca says nothing for any of it (console-01), so the reader edits their source file without knowing.
- **Platform:** Linux, measured. On every platform the key handling is Teksilo's own (keyboard.rs), so the trap is the same there.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `c515e521` (code-editor-trap). Fixed part: CodeEditor keyboard trap: Ctrl+Tab indents, Ctrl+Shift+Tab dedents, focus never leaves, nothing tells the reader), Ctrl+Tab / Ctrl+Shift+Tab now leave and write nothing; the way out is in the text node's description.
- **Where:** crates/teksilo-widgets/src/code\_editor/keyboard.rs:163-170
- **Evidence:**
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
- **The reader gets:** Each row is its own text-run source ending in LineEnd::EndOfText, so the terminal's text is the rows concatenated with nothing between them. AT-SPI's text-changed:insert carries 'hellouser@host:~$' and 'onetwouser@host:~$', and Orca's terminal script speaks the insertion as one word. Line navigation itself is right: get\_string\_at\_offset(LINE) returns one line per row.
- **Platform:** Linux, measured. Windows and macOS (from source): accesskit\_windows text.rs:520-529 GetText writes Range::write\_text, and macOS node.rs:772/845 uses range.text(), the same concatenation, so any client reading or diffing the whole text there gets the same run-together words
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-terminal/src/a11y.rs:91-100
- **Evidence:**
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
- **The reader gets:** The accessible text holds only the visible screen, so a one-line scroll changes every row. The AT-SPI adapter's prefix/suffix diff then sends a delete and an insert of nearly the whole screen, and Orca speaks the whole insertion: '313233...5960user@host:~$ echo hellohellouser@host:~$'. The paced loop gets six such utterances, 885 characters in all, for six five-letter lines, and 'line1' is never heard as a word. Because the cursor row no longer changes, no live announcement is sent at all.
- **Platform:** Linux, measured. Windows gets UIA TextChanged with no content (adapter.rs:55-75), so what NVDA reads there depends on its own diffing and was not measured
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-terminal/src/terminal.rs:609-636; crates/teksilo-terminal/src/a11y.rs (visible screen only)
- **Evidence:**
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
- **The reader gets:** It announces the row the cursor left, read from the new snapshot: 'user@host:~$ echo hello', which is the command the user just typed. Output that arrives in the same drain as the command echo (the usual case) is never announced. The second identical command announces nothing, because the node's name does not change. After 'seq 1 60' it announced '30', a row from the middle of the output. Only output arriving in a later drain ('sleep 1; echo late') is announced right.
- **Platform:** Linux, measured. The name-must-change rule is the same on Windows (accesskit\_windows adapter.rs:313-320) and macOS (event.rs:301-310) (from source)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-terminal/src/terminal.rs:609-636
- **Evidence:**
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
- **The reader gets:** The terminal node has no description, no attribute and no Action interface; on focus Orca says only the current line 'user@host:~$' (its terminal formatting). 'Ctrl+Tab' is nowhere on the bus. The same field carries every .access\_shortcut\_id / .access\_shortcut\_literal in the framework, so those chords are never announced either.
- **Platform:** Linux, measured. Windows and macOS from source: accesskit\_windows-0.35.0 and accesskit\_macos-0.27.0 never read keyboard\_shortcut (and neither do accesskit\_atspi\_common-0.20.0 or accesskit\_consumer-0.39.0)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-terminal/src/a11y.rs:62-66
- **Evidence:**
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
- **The reader gets:** First opening: Orca says 'List with 2 items', 'k while keyword.', and Down 'k where keyword.'. It gets there through the list box's selection-changed, not through the editor. Accepting with Enter is silent. At the second opening (Ctrl+Space) the list box node is reused with the same id the adapter already declared defunct when the popup closed, so Orca drops its selection-changed and says nothing. On every opening the editor reports no expanded change and no active-descendant change.
- **Platform:** Linux, measured. Expanded: accesskit\_atspi\_common-0.20.0 maps no Expanded/Expandable state at all (node.rs:301-376, grep 'expand' finds nothing), an upstream gap. Active descendant: blocked by console-01.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: measured on the combined build, 1 run: console-editor-completion, Orca now says the accepted word and drops nothing as defunct; the list's expanded state and active descendant are other findings.
- **Where:** crates/teksilo-widgets/src/code\_editor/widget.rs:794-797; crates/teksilo-widgets/src/code\_editor/completion.rs:585-640
- **Evidence:**
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
- **The reader gets:** The opt-in sets Live::Polite on the log's Document node, which has no name, and changes only its text runs. All three adapters announce a live node only when it has a name and that name changes (or it appears with one), so the opt-in is a no-op everywhere.
- **Platform:** all three, from source (not measured)
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/code\_editor/log\_view.rs:196-203, 836-840
- **Evidence:**
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
- **The reader gets:** Audit: an unnamed focusable entry. CodeEditor and LogView expose no label builder, and the WidgetBuilder .access\_label an app could use lands on the wrapper (console-01), not on the entry/document that holds the text
- **Platform:** Linux measured; Windows/macOS the same node from source
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `731cc2e1` (editor-focus).
- **Where:** crates/teksilo-widgets/src/code\_editor/widget.rs (builder), log\_view.rs (builder); code\_editor/a11y.rs:51-60 set\_role (no name)
- **Evidence:**
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
- **The reader gets:** Each late line is spoken by Orca's terminal script from the text insertion and again from the Status announcement ('line2', 'line2', 'line3', 'line3' ...; 'lateuser@host:~$' then 'late'). Every Enter also reads back the command line just typed (console-05).
- **Platform:** Linux, measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-terminal/src/a11y.rs LiveAnnouncer + terminal.rs:609-636
- **Evidence:**
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
- **The reader gets:** Only the demo's status label changes ('char 0 · 2 carets', not live). The edit arrives as one text-changed:delete of the whole first line and one insert of 'X// A little Teksilo widget. Edit me!\\nX'. Orca said nothing here (console-01). A client that reads event content hears the whole span as deleted and inserted.
- **Platform:** Linux, measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/code\_editor/a11y.rs:62-70 (framework); accesskit\_atspi\_common-0.20.0/src/adapter.rs:131-181 (span diff, upstream)
- **Evidence:**
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
- **The reader gets:** Orca reads bash's own 'logout' and the command line again (console-05). The demo's status label changes to '○ exited' without being announced, the terminal node keeps the same states (focused, no read-only or other change), and typing 'ls' produces no event at all
- **Platform:** Linux, measured
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-terminal/src/terminal.rs:640-656; a11y.rs build\_terminal\_a11y
- **Evidence:**
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
- **The reader gets:** Nothing is spoken. The only change is the status label's text, which is not a live region and which rewrites itself 12 times a second while streaming
- **Platform:** Linux, measured
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/log\_view/src/main.rs:143-167, 211-215
- **Evidence:**
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
- **The reader gets:** No text or caret event for the space. The next letter arrives as ' h' and the caret jumps two cells (23 -&gt; 25). Orca reads ' e' / ' h'. A braille display or a reader asking for the character at the caret after a space gets the wrong position
- **Platform:** Linux, measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-terminal/src/a11y.rs:176-190
- **Evidence:**
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
- **The reader gets:** Orca builds the option's text from its three labels: badge 'k', label, detail. The badges for other kinds are '•', '☐', '▢', 'ƒ', which Orca would read as symbol names
- **Platform:** Linux, measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/code\_editor/completion.rs:135-148, 811-826
- **Evidence:**
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
- **The reader gets:** The main thread burns about 46-53 s of CPU and answers no key and no AT-SPI action meanwhile. The status label reads '10030 generated' only 50-55 s later. No busy state appears on the log
- **Platform:** Linux, debug build only (release not available here)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/code\_editor/log\_stream.rs:282-302 apply\_appends (cause not isolated; possibly text-document)
- **Evidence:**
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
- **The reader gets:** Only '+50.3 ms object:text-changed:insert \[entry\] '' text='()'', nothing about the match
- **Platform:** Linux, measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/code\_editor (bracket matching, paint only)
- **Evidence:**
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
- **The reader gets:** The doc says only the visible lines are emitted 'as paragraphs (numbered by global line, "line 41 002 of 128 449")'. code\_editor/a11y.rs says the paragraphs and that ordinal were removed. It also presents announce\_appends as a working opt-in (see console-08)
- **Platform:** documentation
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** docs/log-view.md:161-175
- **Evidence:**
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
- **The reader gets:** Shift+PageUp and Shift+PageDown emit no event at all, and a fresh AT-SPI client still reads the bottom page ('7071...100user@host:~$ sleep 8; echo LATE'). The view has scrolled back 33 rows: when LATE arrives, the drain re-walks the tree and the text jumps to '3738...69', sent as a delete of the whole bottom page and an insert of the history page. The live region announces '69', the history row now under the old cursor row, and Orca says '69'. LATE itself is never heard. The AT action path does it right: access\_scroll calls ctx.request\_accessibility\_update() (terminal.rs:1852-1854). The key path (terminal.rs:1224-1232 → scroll\_view :1735-1743) refreshes the snapshot but neither bumps document\_version, the only trigger of the terminal's re-walk (bound at :681-685, bumped only in the drain at :600-608), nor requests an update. So scrollback history cannot be reviewed by keyboard.
- **Platform:** Linux AT-SPI / Orca 46.1, measured. The missing re-walk is in teksilo-terminal, so the stale text is the same on Windows and macOS (from source; not measured there).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-terminal/src/terminal.rs:1224-1232, 1735-1743
- **Evidence:**
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
- **The reader gets:** Every option reaches AT-SPI as \[list item\] ''. The emit pass that fills a nameless Role::ListBoxOption/TreeItem from its first named descendant reads `descendant.label()` (accessibility\_emit\_impl.rs:188). But AccessNodeBuilder::build moves a Role::Label's name into `value` and clears `label` (accessibility.rs:1014-1020), so a TextWidget's text is never found. Orca makes up for it by speaking the children ('k while keyword.', badge included, console-15). On Windows the UIA Name comes from the label (read from source), so the option would be nameless there. What NVDA then reads was not measured. The same pass serves every virtualized row whose name comes from content; only the completion rows were measured here.
- **Platform:** Linux, measured (bus name ''); Windows/macOS from source only
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:188
- **Evidence:**
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
- **The reader gets:** The entry's controller-for target is an \[unknown\] '' node, and the \[list box\] is that node's child. code\_editor.rs:395 pushes widget\_id\_to\_node\_id(panel\_id), where panel\_id is the add\_deferred wrapper (widget.rs:794), not the CompletionPanel (the Role::ListBox). Being a relation target also exempts the empty wrapper from presentational pruning (accessibility\_emit\_impl.rs:220-235), so it shows up in the tree, and it is defunct from the second opening on (console-07).
- **Platform:** Linux, measured; the relation is emitted the same for every adapter (source)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/code\_editor.rs:395
- **Evidence:**
  - `console-editor-completion-20260925-144543-813521 tree after 'wh': entry relations {'controller-for': ['/org/a11y/atspi/accessible/0/79228164801660602733528350720']} ; that path is [unknown] ; [list box] is /org/a11y/atspi/accessible/0/79228166258953384556582928384`
  - `tree-type--wh-.txt: "[unknown] '' {focusable,focused}" / "  [entry] '' ... rel=['controller-for']" / "  [unknown] ''" / "    [list box] '' {vertical}"`
  - `crates/teksilo-widgets/src/code_editor.rs:393-395; crates/teksilo-widgets/src/code_editor/widget.rs:794`
- **Reproduced:** 3 of 3 completion runs (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Point controls at the CompletionPanel's own id (the ListBox node), which the deferred subtree can report once materialized.
