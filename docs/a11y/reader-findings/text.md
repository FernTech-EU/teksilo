<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Rich text and input methods

Examples: `rich-text-editor`, `ime-playground`, `rich-text-viewer`.
22 findings: 2 critical, 12 high, 4 medium, 4 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [text-01](#text-01) | rich-text-editor | Keyboard focus into a RichTextEditor (editor or read-only viewer) lands on an unnamed 'section', not on the text: no caret or selection event ever follows | critical | Linux | fixed |
| [text-02](#text-02) | rich-text-editor | Coming back to an editor or viewer is silent: the wrapper node is removed when focus leaves and returns under the same id, which libatspi holds as defunct | high | Linux | fixed |
| [text-03](#text-03) | rich-text-editor | No character separates paragraphs in the text a reader gets: the end of one paragraph and the start of the next are one offset | high | Linux | open |
| [text-04](#text-04) | rich-text-editor | Lists reach the reader as plain paragraphs: no bullet, no number, no depth, no list node | high | Linux | open |
| [text-05](#text-05) | rich-text-editor | Links in rich text are not exposed: no link node, no Hypertext, read as plain text | high | Linux | open |
| [text-06](#text-06) | rich-text-editor | Character formatting (bold, italic, underline, spelling marks) is not exposed as text attributes | high | Linux | open |
| [text-07](#text-07) | rich-text-editor | Both rich text panes are unnamed, and RichTextEditor offers no way to name its text node | high | Linux | fixed |
| [text-08](#text-08) | ime-playground | Unnamed text fields in the examples: the rich-text-editor search field (focused at launch) and the ime-playground fields read only their placeholder or nothing | high | Linux | open (example) |
| [text-09](#text-09) | ime-playground | TextInput caret moves never reach the reader: arrows and Home are silent and the reported caret stays at the last edit | high | Linux | fixed |
| [text-10](#text-10) | ime-playground | An empty TextInput exposes no Text interface, so the first text entered (a typed letter, an IME commit) produces no text-changed event | medium | Linux | fixed |
| [text-11](#text-11) | rich-text-editor | The formatting toolbar cannot be reached by keyboard, yet AT-SPI reports every button as focusable | high | Linux | open (example) |
| [text-12](#text-12) | rich-text-editor | Ctrl+B / Italic change formatting with no feedback a reader can get | low | Linux | open |
| [text-13](#text-13) | rich-text-editor | Headings in rich text are unnamed and carry no level on AT-SPI | medium | Linux | open |
| [text-14](#text-14) | rich-text-editor | The Heading level, Font family and Theme combo boxes never tell the reader their current value | medium | Linux | upstream |
| [text-15](#text-15) | rich-text-editor | Orca in a Wayland session never learns of a key typed into a Teksilo window, so it says nothing about any caret move | high | Linux | fixed |
| [text-16](#text-16) | rich-text-editor | After a screen reader moves focus into the editor, Ctrl+Tab jumps to the window's first control instead of the next one | low | Linux | open |
| [text-17](#text-17) | rich-text-editor, ime-playground, rich-text-viewer | Every TextInput/SearchField carries an always-present, empty, unnamed status bar node | low | Linux | open |
| [text-v01](#text-v01) | rich-text-editor | Shift+F10 moves the caret to the middle of the editor or field and drops the selection, so the menu's Paste writes somewhere else | critical | Linux | fixed |
| [text-v02](#text-v02) | ime-playground | Closing a TextInput's context menu with Escape selects the whole field, so the next key replaces its content | high | Linux | fixed |
| [text-v03](#text-v03) | rich-text-editor | The editor's context menu cannot be operated by a reader: focus lands on an unnamed menu, Down is silent, and the disabled Cut and Copy report enabled | high | Linux | partly fixed |
| [text-v04](#text-v04) | rich-text-editor | The Highlighter search tells a reader nothing: no match count, and the matches are not in the text attributes | medium | Linux | open (example) |
| [text-v05](#text-v05) | rich-text-editor, ime-playground, rich-text-viewer | SearchField publishes a second, non-focusable entry holding the same text around the focused one | low | Linux | open |

### text-01 {#text-01}

Keyboard focus into a RichTextEditor (editor or read-only viewer) lands on an unnamed 'section', not on the text: no caret or selection event ever follows

- **Example:** rich-text-editor
- **Scenario:** text-editor-tab-in, text-viewer, text-ime, text-editor-shift-tab
- **Act:** text-editor-tab-in 'Tab from the search field into the editor' then 'Down in the editor (Orca told of the key)'; text-viewer 'Tab into the viewer' then 'Down in the viewer'; text-ime 'Tab to the rich editor'; text-editor-shift-tab stops 6 and 8
- **The reader should get:** Focus lands on the multi-line entry (editor) or document frame (viewer) that carries the Text interface; the reader hears a name and 'entry'/'document'; every arrow key then moves a caret the reader hears.
- **The reader gets:** AT-SPI focus goes to the wrapper RichTextEditor node, a Role::GenericContainer that AT-SPI maps to 'section'. Orca says only 'section.'. The entry is not the focused node, so accesskit\_atspi\_common emits no text-caret-moved and no text-selection-changed for it: Down, Right, End and all other caret keys are silent. Typing still emits text-changed on the unfocused entry, and Orca only moves its locus there as a heuristic. The only route that puts focus on the text node is an AT-SPI grab\_focus on the entry itself, which a user does not do. Every keyboard or pointer entry into every RichTextEditor in the three examples is affected.
- **Platform:** Linux AT-SPI/Orca 46.1 measured. Windows/macOS by adapter source only: the same TreeUpdate.focus is the GenericContainer, which is a UIA Group (accesskit\_windows node.rs:76) or NSAccessibilityUnknownRole (accesskit\_macos node.rs:55). Those adapters do raise text-selection events for the unfocused entry (windows node.rs:761-768, macos event.rs:291-297), but a screen reader tracks its focus object, which has no text pattern. NVDA and VoiceOver were not verified.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `731cc2e1` (editor-focus).
- **Where:** crates/teksilo-widgets/src/rich\_text.rs:3853, :4281-4291; crates/teksilo-widgets/src/rich\_text/body.rs:760-772; crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:126-139
- **Evidence:**
  - `text-editor-tab-in-20260925-142522-443443 act 'Tab from the search field into the editor': 14:25:31.543160 object:state-changed:focused 1 [section] ''`
  - `same act: 14:25:31.544409 object:state-changed:focused 0 [entry] ''`
  - `orca-debug.out: 14:25:31.586022 - SPEECH OUTPUT: 'section.'`
  - `act 'Down in the editor (Orca told of the key)': no event at all on the bus; checks: FAIL 'a object:text-caret-moved event from [entry]', FAIL 'Orca reads the next line' (Orca said nothing); the key reached Orca (KEYBOARD_EVENT logged)`
  - `text-viewer-20260925-142451-427749 'Tab into the viewer': 14:25:05.337266 object:state-changed:focused 1 [section] '' / 14:25:05.389799 - SPEECH OUTPUT: 'section.'; 'Down in the viewer': no text-caret-moved, Orca silent`
  - `text-ime-20260925-142728-476941 'Tab to the rich editor': object:state-changed:focused 1 [section] '' ; 14:28:28.486562 - SPEECH OUTPUT: 'section.'`
  - `Contrast, same editor after AT-SPI grab_focus on the entry: 'object:state-changed:focused 1 [entry]' then every Down/Right emits object:text-caret-moved and Orca reads it (text-editor-caret, 3 runs)`
  - `source: crates/teksilo-widgets/src/rich_text.rs:3853 '.focusable(true)' is on the wrapper; rich_text.rs:4281-4291 the wrapper's accessibility() is Role::GenericContainer; rich_text/body.rs:760-772 the text role lives on the non-focusable RichTextEditorBody`
  - `source: crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:126-139 TreeUpdate.focus = the focused widget's own node; accessibility_emit_impl.rs:229-235 the focused node is exempt from presentational pruning; accesskit_consumer filters.rs:18-20 a focused node is always included; accesskit_atspi_common node.rs:173 GenericContainer -> AtspiRole::Section`
  - `source: accesskit_atspi_common-0.20.0 adapter.rs:216-219 'if !old_node.is_focused() || ... { return; }' so caret and selection events are only emitted for the focused node`
  - `text-editor-tab-in-20260925-143636-700740: 14:36:46.114282 object:state-changed:focused 1 [section] '' (path …276401635328); orca 14:36:46.142283 - SPEECH OUTPUT: 'section.'; 'Down in the editor': no event on the bus, yet the probe afterwards shows caret 28, line 'Showcase', so the caret did move`
  - `text-editor-tab-in-20260925-143831-746774: 14:38:40.465257 focused 1 [section]; 14:38:40.507305 - SPEECH OUTPUT: 'section.'`
  - `text-editor-tab-in-20260925-144645-931187: 14:46:55.251961 focused 1 [section]; 14:46:55.285996 - SPEECH OUTPUT: 'section.'`
  - `text-viewer-20260925-143723-721398: 14:37:37.286534 focused 1 [section]; 14:37:37.322972 - SPEECH OUTPUT: 'section.'; text-viewer-20260925-144733-954413: 14:47:46.661716 / 14:47:46.715471 'section.'`
  - `text-ime-20260925-143803-736718 14:39:03.068953, -144619-914950 14:47:20.153001, -144833-983323 14:49:34.505928: SPEECH OUTPUT: 'section.' on Tab to the rich editor; the IME commits that follow reach the bus as text-changed:insert on the unfocused [entry] and Orca says nothing`
  - `crates/teksilo-core/src/widget.rs:496-524 accessibility_proxy moves overrides only; accessibility_emit_impl.rs:126-139 resolves focus without it`
- **Reproduced:** text-editor-tab-in 3 of 3 runs; text-viewer 2 of 2; text-ime (rich editor) 3 of 3; text-editor-shift-tab 1 of 1 (both the editor and the preview wrappers). Deterministic.
- **Verification:** confirmed. Reproduced: Tab into the editor: 3 of 3 runs of text-editor-tab-in, plus 2 of 2 of verify-text-ctxmenu. Tab into the viewer: 2 of 2 of text-viewer. Tab into the ime-playground rich editor: 3 of 3 of text-ime. The first Down after arrival gave no caret event in all 8. Deterministic.
- **Fix idea:** Make the node that carries Role::MultilineTextInput/Document the AT focus whenever the RichTextEditor widget holds focus. One way is a Widget hook (for example an accessibility focus target) that accessibility\_emit\_impl.rs:126 consults, returning the body's node. Another is to move the role, the runs and the text selection onto the wrapper and drop the separate body node. Add a regression test: Tab into an editor, then assert that TreeUpdate.focus is the node with Role::MultilineTextInput and that a caret move changes its text selection.

### text-02 {#text-02}

Coming back to an editor or viewer is silent: the wrapper node is removed when focus leaves and returns under the same id, which libatspi holds as defunct

- **Example:** rich-text-editor
- **Scenario:** text-editor-tab-in, text-viewer
- **Act:** text-editor-tab-in 'Ctrl+Tab out of the editor' then 'Ctrl+Shift+Tab back into the editor'; text-viewer 'Tab out to the Theme combo box' then 'Tab into the viewer a second time'
- **The reader should get:** Returning focus to the editor is announced like the first arrival.
- **The reader gets:** The GenericContainer exists in the AT tree only while it is focused, because it is otherwise pruned as a content-free container. When focus leaves, the adapter removes it and marks it defunct. When focus comes back, the node reappears with the same NodeId (path), which libatspi still holds as defunct, and Orca logs 'Ignoring defunct object: \[section\]' and says nothing. This is the mechanism of K2 (a reused id after removal), here on the editor's wrapper rather than the announcer's nodes, so the K2 fix does not cover it.
- **Platform:** Linux AT-SPI/Orca 46.1 measured. The defunct-on-reuse behaviour is AT-SPI-specific (accesskit\_atspi\_common remove\_node). On Windows/macOS the node still appears and disappears with focus, not verified.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `85624a1a` (node-ids). Fixed part: text-editor-tab-in: returning to the editor is heard as 'section.' 1/1; the separate text-01 defect of focusing a GenericContainer remains.
- **Where:** crates/teksilo-widgets/src/rich\_text.rs:3853, :4290 (a focusable GenericContainer); crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:229-235 contributes but is not sufficient
- **Evidence:**
  - `text-editor-tab-in-20260925-142522-443443 'Ctrl+Tab out of the editor': 14:25:47.250823 object:state-changed:defunct 1 [section] '' ; 14:25:47.251042 object:state-changed:focused 1 [separator] 'Splitter divider' ; 14:25:47.297698 - SPEECH OUTPUT: 'vertical splitter Splitter divider.'`
  - `'Ctrl+Shift+Tab back into the editor': 14:25:51.193556 object:state-changed:focused 1 [section] '' ; orca-debug.out 14:25:51.195193 - EVENT MANAGER: Ignoring defunct object: [section] ; no SPEECH OUTPUT but the key echo 'tab' (14:25:51.136250)`
  - `text-viewer-20260925-142451-427749 'Tab out to the Theme combo box': 14:25:13.133749 object:state-changed:defunct 1 [section] '' ; 'Tab into the viewer a second time': 14:25:17.033059 object:state-changed:focused 1 [section] '' ; 14:25:17.034178 - EVENT MANAGER: Ignoring defunct object: [section] ; Orca said nothing`
  - `source: crates/teksilo-core/src/widget_tree/accessibility_emit_impl.rs:229-235 the prunable set excludes only the current focus, so a GenericContainer wrapper exists exactly while focused; accesskit_atspi_common adapter.rs remove_node marks it defunct; rich_text.rs:4290 wrapper role`
  - `text-editor-tab-in-20260925-143636-700740: 14:37:01.804354 object:state-changed:defunct 1 [section] ''; 14:37:05.746448 object:state-changed:focused 1 [section] '' (same path …276401635328); orca 14:37:05.748091 - EVENT MANAGER: Ignoring defunct object: [section]; only key echo 'tab' spoken`
  - `text-editor-tab-in-20260925-144645-931187: 14:47:10.958065 defunct 1 [section]; 14:47:14.897145 focused 1 [section]; 14:47:14.898981 Ignoring defunct object: [section]`
  - `text-viewer-20260925-144733-954413: 14:47:54.458429 defunct 1 [section]; 14:47:58.355451 focused 1 [section]; 14:47:58.357113 Ignoring defunct object: [section]; Orca silent`
  - `verify-text-ctxmenu-20260925-144245-823122 'Escape closes the menu': focused 1 [section] after defunct at Shift+F10; 14:43:06.303329 Ignoring defunct object: [section]; only 'escape' key echo (14:43:06.261771)`
  - `~/.cargo/registry/.../accesskit_atspi_common-0.20.0/src/adapter.rs:287-304 (Include -> ExcludeNode => remove_node => StateChanged(Defunct))`
- **Reproduced:** 5 of 5 returns: text-editor-tab-in 3 of 3 runs, text-viewer 2 of 2 runs
- **Verification:** corrected by the verifier. Reproduced: Return to the editor by Ctrl+Shift+Tab: 3 of 3 runs of text-editor-tab-in. Return to the viewer by Tab: 2 of 2 runs of text-viewer. Also seen on return from the editor's context menu by Escape after a Tab arrival: 2 of 2 runs of verify-text-ctxmenu. Silent every time. Deterministic. The defect is real and reproduces. The correction is to its mechanism and its fix. Teksilo's pruning (accessibility\_emit\_impl.rs:229-235) is not the only thing that drops the wrapper. accesskit\_consumer's common\_filter includes a GenericContainer only while it is focused (filters.rs:18-20, then filters.rs:32-34 ExcludeNode). accesskit\_atspi\_common's node\_updated turns a change from Include to ExcludeNode into remove\_node, which marks the node defunct (adapter.rs:287-304, :90-110). So a focused GenericContainer leaves the AT-SPI tree and returns with the same path each time focus leaves and comes back, whatever Teksilo prunes. The fix idea 'presence should not depend on focus' cannot be met at the pruning step. The rule has to be that TreeUpdate.focus is never a GenericContainer, which means fixing text-01. The finding's framework\_location should point at the focused GenericContainer (rich\_text.rs:3853/4290), not only at the pruning. The K2 fix (b9586ea2b, announcer ids drawn from 1..1&lt;&lt;32) is specific to the announcer and does not cover this. The same path, …276401635328, came back in all runs.
- **Fix idea:** Fixing text-01 removes this instance. In general, the presence of a node in the AT tree should not depend on whether it holds focus, and a NodeId should never be reused after the adapter has removed it. The same rule the K2 fix applies to the announcer should apply to the focus-exempt pruning.

### text-03 {#text-03}

No character separates paragraphs in the text a reader gets: the end of one paragraph and the start of the next are one offset

- **Example:** rich-text-editor
- **Scenario:** text-editor-caret, text-editor-typing
- **Act:** text-editor-caret: 'Down onto the heading's short last line', 'End: end of the first paragraph', 'Right across the paragraph break', 'the word at the paragraph break'; text-editor-typing 'Enter: a new paragraph'
- **The reader should get:** The Text interface holds a line break at the end of each paragraph, as AccessKit expects: a run whose value ends in a newline marks a paragraph end. The caret at the end of 'Showcase' is on the heading's line, End there says nothing of the next paragraph, Right moves to a new offset and speaks 'T', Enter inserts a line break, and paragraph granularity stops at the heading.
- **The reader gets:** get\_text(28,40) is 'ShowcaseThis'. The caret at the end of the H1 (document position 36) and at the start of 'This window…' (document position 37) both report offset 36. (1) Down from offset 14 puts the caret on the heading's second line, at the end of 'Showcase'. Orca reads the next paragraph's line, 'This window hosts two RichTextEditor widgets bound to the same TextDocument. The ', and Up then returns to column 14 of the heading, which proves the caret was on 'Showcase'. (2) End at the end of the heading: Orca says 'T'. (3) Right across the break: the caret offset stays 36 (text-caret-moved 36 again), Orca logs 'Event is for last saved cursor position' and says nothing. (4) Enter at the end of the document: the character count stays 7596, no text-changed event, silence. (5) PARAGRAPH granularity at offset 35 returns 0..2981, twenty or so paragraphs up to the first code block, whose text holds a newline. The reader is told the caret is somewhere it is not, and text typed there lands in the other paragraph.
- **Platform:** Linux AT-SPI/Orca 46.1 measured. The text model and paragraph logic are in accesskit\_consumer (text.rs:71-72), which all three adapters share, so by source UIA text ranges (NVDA paragraph navigation) and macOS text ranges see the same run-together text. Not verified on Windows or macOS.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/rich\_text/body/flow\_walk.rs:348-364, :436-446
- **Evidence:**
  - `text-editor-caret-20260925-142559-454805 'Down onto the heading's short last line': 14:26:24.439739 object:text-caret-moved 36 [entry] '' ; orca 14:26:24.448607 - SPEECH OUTPUT: 'This window hosts two RichTextEditor widgets bound to the same TextDocument. The ' ; next act 'Up': 14:26:28.428806 object:text-caret-moved 14 [entry] ''`
  - `'End: end of the first paragraph': 14:26:37.598712 object:text-caret-moved 36 [entry] '' ; 14:26:37.604748 - DEFAULT: Presenting result of line boundary nav ; 14:26:37.610556 - SPEECH OUTPUT: 'T' ; probe: char at caret {'text': 'T', 'start': 36, 'end': 37}, text 28..44 'ShowcaseThis win'`
  - `'Right across the paragraph break': 14:26:41.549424 object:text-caret-moved 36 [entry] '' ; 14:26:41.554005 - DEFAULT: Event is for last saved cursor position ; FAIL 'the caret offset moves on': caret before 36, after 36`
  - `'the word at the paragraph break': FAIL 'the text holds a separator': text 28..40: 'ShowcaseThis' ; FAIL 'the paragraph at the heading is the heading alone': paragraph start 0 end 2981`
  - `text-editor-typing-20260925-141942-298240 'Enter: a new paragraph': 14:19:53.671020 object:text-caret-moved 7596 [entry] '' ; 14:19:53.676658 - DEFAULT: Event is for last saved cursor position ; FAIL 'the text grows by a line break': characters before 7596, after 7596 ; FAIL no object:text-changed:insert`
  - `source: crates/teksilo-widgets/src/rich_text/body/flow_walk.rs:348-364,397 each block's runs come from block.text, which carries no trailing break; crates/teksilo-core/src/accessibility/text_runs.rs:22-25 the emitter's own contract is 'a hard break is one character at the end of its line's last run'; accesskit_consumer-0.39.0 text.rs:71-72 is_paragraph_end = line end whose run value ends_with('\n')`
  - `text-editor-caret-20260925-143658-708310: 14:37:18.095782 object:text-caret-moved 36 [entry]; 14:37:18.106593 - SPEECH OUTPUT: 'This window hosts two RichTextEditor widgets bound to the same TextDocument. The '; Up then 14:37:22.100610 caret 14 (the caret was on 'Showcase')`
  - `same run: End 14:37:31.227074 caret-moved 36; 14:37:31.235624 DEFAULT: Presenting result of line boundary nav; 14:37:31.239826 SPEECH OUTPUT: 'T'; Right 14:37:35.217671 caret-moved 36; 14:37:35.221555 DEFAULT: Event is for last saved cursor position`
  - `runs -143929-762087 and -145121-1065362: identical failures; the probe gives paragraph {'start': 0, 'end': 2981} and text 28..40 'ShowcaseThis'`
  - `text-editor-typing-20260925-144000-772939 and -144225-814025: Enter gives only object:text-caret-moved 7596, count 7596 before and after, line at caret ''`
  - `text-viewer-20260925-143723-721398 orca 14:38:06.373622 and -144733-954413 14:48:15.828825 - SPEECH OUTPUT: 'string. It is the live target of Milestone 8a of §27.10 of the Teksilo architecture.What works todayCrisp glyph rendering at any display DPI …'`
- **Reproduced:** text-editor-caret 3 of 3 runs (the Down misreading, End 'T' and silent Right in each); Enter with no text change 2 of 2 runs (text-editor-typing). Deterministic.
- **Verification:** confirmed. Reproduced: text-editor-caret, 3 of 3 runs: Down read the next paragraph's line, End said 'T', Right across the break left the caret at 36 and Orca said nothing, get\_text(28,40) was 'ShowcaseThis', and the paragraph at 35 ran from 0 to 2981. text-editor-typing, 2 of 2 runs: Enter left the count at 7596 with no text-changed. text-viewer, 2 of 2 runs: Orca spoke the paragraphs run together. Deterministic.
- **Fix idea:** In FlowWalk::block, give every block but the last a hard break of one character (for example '\\n') at the end of its last line's run, through TextRunSource, which already knows how to carry a break per the text\_runs contract. An empty block then gets the one run its break rides. Map the extra character in syn\_map and in SetTextSelection resolution so AT offsets and document positions stay aligned (document positions already count a separator). Add a test: at the end of a paragraph the caret's character is the break, and Enter produces a text-changed insert.

### text-04 {#text-04}

Lists reach the reader as plain paragraphs: no bullet, no number, no depth, no list node

- **Example:** rich-text-editor
- **Scenario:** text-editor-structure, text-viewer
- **Act:** text-editor-structure 'Down onto the first bullet item', 'Down onto the first numbered item', 'Down onto a nested numbered item', 'the document's structure in the tree'; text-viewer 'Down onto a bullet item'
- **The reader should get:** A reader hears that a line is a list item, with its marker ('•', '1.') and its nesting, the way the bullets and numbers are drawn. The marker can be in the text, as browsers expose list markers, or come from list and list item nodes.
- **The reader gets:** Orca reads 'First item at indent 0', 'First numbered item' and 'Nested decimal at indent 1' with no marker. The tree holds no list or list item node, and the text has no marker. The nested numbered list's own '1.' and its depth are lost, so a numbered procedure cannot be followed by number.
- **Platform:** Linux AT-SPI/Orca 46.1 measured. The missing tree content applies to every platform (the AccessKit tree has no list nodes and no marker text).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/rich\_text/body/flow\_walk.rs:371-395
- **Evidence:**
  - `text-editor-structure-20260925-142333-394383 'Down onto the first bullet item': 14:23:48.811838 object:text-caret-moved 1855 [entry] '' ; 14:23:48.828225 - SPEECH OUTPUT: 'First item at indent 0' ; FAIL Orca says '•' or 'bullet' or 'list item'`
  - `'Down onto the first numbered item': 14:23:55.343473 object:text-caret-moved 2092 [entry] '' ; 14:23:55.358705 - SPEECH OUTPUT: 'First numbered item' ; FAIL`
  - `'Down onto a nested numbered item': SPEECH OUTPUT 'Nested decimal at indent 1' ; FAIL Orca says '1.' or 'nesting level' or 'level 2'`
  - `'the document's structure in the tree': FAIL 'the lists are lists (a list or list item node)': no such node`
  - `text-viewer-20260925-142451-427749 'Down onto a bullet item': 14:25:37.736876 - SPEECH OUTPUT: 'Mouse wheel scrolling.' ; FAIL Orca says the bullet`
  - `source: crates/teksilo-widgets/src/rich_text/body/flow_walk.rs:371-395 only a cell, a heading or a blockquote gets a node between the editor and its runs; the list marker is painted only, never emitted as text`
  - `text-editor-structure-20260925-144117-792254 and -145023-1040636: 'Down onto the first bullet item' SAYS 'First item at indent 0'; 'Down onto a nested numbered item' SAYS 'Nested decimal at indent 1'; tree check: no list or list item node`
  - `text-viewer-20260925-144733-954413: 14:48:19.153794 Orca said 'Mouse wheel scrolling.' (FAIL: Orca says the bullet)`
  - `verify-text-list-indent-20260925-144433-867525: 'Tab on a list item': no event; the line before and after is 'Second item, mixing bold and italic in the same line', count 7596 -> 7596`
- **Reproduced:** text-editor-structure 2 of 2 runs; text-viewer 2 of 2 runs
- **Verification:** confirmed. Reproduced: text-editor-structure: 2 of 2 runs (no list node, no marker spoken). text-viewer: 2 of 2 runs ('Mouse wheel scrolling.' with no bullet). verify-text-list-indent: 1 of 1 run, where Tab on a list item changed its depth and put no event on the bus.
- **Fix idea:** Emit each list item's marker text ('• ', '1. ') as part of its first run, as a marker the caret cannot enter, and add Role::List/ListItem nodes with level, position and set size. Each such node needs a text-range-capable container below it, by the flow\_walk rule.

### text-05 {#text-05}

Links in rich text are not exposed: no link node, no Hypertext, read as plain text

- **Example:** rich-text-editor
- **Scenario:** text-editor-format, text-editor-structure
- **Act:** text-editor-format 'the link 'the text-document repo', as the Text interface answers'; text-editor-structure 'Down onto a line with a link' and the tree check
- **The reader should get:** A reader can hear that 'the text-document repo' is a link, find it, and follow it (a Role::Link node or Hypertext).
- **The reader gets:** The editor exposes no Hypertext interface (hypertext links: None), the tree holds no link node, and the attribute run at the link is empty. Orca reads the line 'switches to the monospace family. Links like the text-document repo carry an ' with nothing to mark a link. The editor supports link activation by pointer (.on\_link\_activated), but no AT user can find the link or activate it.
- **Platform:** Linux AT-SPI/Orca measured. The AccessKit tree content applies to all platforms.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/rich\_text/body/flow\_walk.rs:348-397 (add a push\_link\_child per anchor\_href fragment); crates/teksilo-widgets/src/rich\_text/mouse.rs:459-490 (pointer-only activation)
- **Evidence:**
  - `text-editor-format-20260925-142932-527100: FAIL 'the editor offers its links (Hypertext)': hypertext links: None, attrs at the link: {'attrs': {}, 'start': 0, 'end': 7596}`
  - `text-editor-structure-20260925-142333-394383 'Down onto a line with a link': 14:24:08.411046 object:text-caret-moved 1056 [entry] '' ; 14:24:08.423619 - SPEECH OUTPUT: 'switches to the monospace family. Links like the text-document repo carry an '`
  - `text-editor-structure-20260925-141819-270614 tree check FAIL 'the links are links': no such node`
  - `source: crates/teksilo-widgets/src/rich_text/body/flow_walk.rs:352-357 the TextGeometry handed to the run emitter has 'links: Vec::new()'; no Role::Link node is pushed for fragments carrying anchor_href`
  - `text-editor-format-20260925-144049-785272 and -144314-834725: 'the editor offers its links (Hypertext)' FAIL: hypertext links: None, attrs at the link: {'attrs': {}, 'start': 0, 'end': 7596}`
  - `text-editor-structure-20260925-145023-1040636: 'Down onto a line with a link' SAYS 'switches to the monospace family. Links like the text-document repo carry an '`
  - `crates/teksilo-widgets/src/primitives/text_widget.rs:1036-1046 (push_link_child from geometry.links: the working precedent)`
- **Reproduced:** text-editor-format 2 of 2 runs; text-editor-structure 2 of 2 runs (tree), 1 of 1 (line read)
- **Verification:** corrected by the verifier. Reproduced: text-editor-format: 2 of 2 runs (hypertext links: None, empty attributes). text-editor-structure: 2 of 2 runs (no link node; the line is read as plain text). The defect reproduces. The fix idea needs correcting. 'Fill TextGeometry.links so the emitter can link runs' would do nothing: TextRunSource::from\_geometry and push\_text\_runs never read geometry.links (text\_runs.rs has no consumer of it). The only consumer is TextWidget, which pushes a Role::Link child for each markup link with builder.push\_link\_child (text\_widget.rs:1036-1046), and that is the precedent to follow. The claim that the editor supports pointer activation is also narrower than stated. mouse.rs:459-490 follows a link only on Ctrl+click in an editor or a plain click in a viewer, there is no keyboard route, and this example installs no on\_link\_activated, so its links do nothing even by pointer. The rich-text-viewer sample contains no links, so the viewer's links could not be tested.
- **Fix idea:** Emit a Role::Link node (with URL and a Click action routed to the link-activation callback) around the runs of an anchor\_href fragment, with a text container below it per the flow\_walk rule, or fill TextGeometry.links so the emitter can link runs.

### text-06 {#text-06}

Character formatting (bold, italic, underline, spelling marks) is not exposed as text attributes

- **Example:** rich-text-editor
- **Scenario:** text-editor-format
- **Act:** text-editor-format 'the attributes of the bold word 'two'', 'Ctrl+B on the selected word', 'activate Italic through AT-SPI'
- **The reader should get:** The Text interface's attribute run at a bold or italic word reports its weight or style (WCAG 1.3.1). Orca's read-attributes command and its misspelling indicator depend on these attributes.
- **The reader gets:** get\_attribute\_run anywhere in the document returns {} over 0..7596, a single attribute-less run covering the whole document. This holds for the markdown bold 'two', and for 'window' after Ctrl+B and after Italic. The Spell highlighter's misspellings are painted only, never exposed.
- **Platform:** Linux AT-SPI measured. The runs carry default TextRunAttributes on every platform (by source).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/rich\_text/body/flow\_walk.rs:348-364; crates/teksilo-core/src/accessibility/text\_runs.rs:228-296
- **Evidence:**
  - `text-editor-format-20260925-142932-527100: FAIL 'the attribute run at 'two' says it is bold': {'attrs': {}, 'start': 0, 'end': 7596}`
  - `'Ctrl+B on the selected word': 14:29:48.402520 object:state-changed:pressed 1 [toggle button] 'Bold (Ctrl+B)' ; FAIL 'the Text interface now says the word is bold': {'attrs': {}, 'start': 0, 'end': 7596}`
  - `run note: attributes at 'window' after Italic: {'attrs': {}, 'start': 0, 'end': 7596}`
  - `source: crates/teksilo-widgets/src/rich_text/body/flow_walk.rs:364 TextRunSource::from_geometry(...) is never given attributes; crates/teksilo-core/src/accessibility/text_runs.rs:292 from_geometry sets attrs: TextRunAttributes::default(); only inline objects get TextRunAttributes (flow_walk.rs, the image/footnote branch); rich_text/body.rs:786-796 the AT walk deliberately skips highlight overlays, so spelling is never exposed`
  - `text-editor-format-20260925-144314-834725: 'the attribute run at 'two' says it is bold' FAIL {'attrs': {}, 'start': 0, 'end': 7596}; 14:43:31.073533 object:state-changed:pressed 1 [toggle button] 'Bold (Ctrl+B)', then attributes at 'window' still {'attrs': {}, 'start': 0, 'end': 7596}`
  - `accesskit_atspi_common-0.20.0/src/text_attributes.rs ATTRIBUTE_GETTERS: family-name, size, weight, style, strikethrough, underline, bg-color, fg-color, language, justification (no spelling key)`
- **Reproduced:** 2 of 2 runs (text-editor-format)
- **Verification:** confirmed. Reproduced: text-editor-format: 2 of 2 runs. There is one attribute-less run over 0..7596 at 'two', after Ctrl+B, and after Italic.
- **Fix idea:** Split each block's runs at fragment format boundaries and pass the fragment's TextRunAttributes (weight, italic, underline, strikethrough) through TextRunSource. Consider exposing spelling ranges as an invalid-spelling attribute on the editable pane.

### text-07 {#text-07}

Both rich text panes are unnamed, and RichTextEditor offers no way to name its text node

- **Example:** rich-text-editor
- **Scenario:** text-editor-structure, text-viewer, tree
- **Act:** launch tree audit (rich-text-editor, rich-text-viewer); text-editor-structure tree check; grab\_focus scenes
- **The reader should get:** The editor and the preview (or the viewer's document) carry a name ('Editor', 'Preview'), spoken on focus.
- **The reader gets:** The audit reports an unnamed focusable entry. Orca says 'entry This window hosts two RichTextEditor…' and 'document frame This window holds a single…', with no name. RichTextEditor has no label builder (tools/extract\_widget\_api.py RichTextEditor lists none). An .access\_label on it would land on the wrapper, a GenericContainer, which Teksilo keeps once it has a property but which accesskit\_consumer's common\_filter still excludes (filters.rs:30-33), so the name reaches no reader. The examples cannot name the panes.
- **Platform:** Linux AT-SPI/Orca measured. The AccessKit tree has no name on every platform.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `731cc2e1` (editor-focus).
- **Where:** crates/teksilo-widgets/src/rich\_text.rs:4281-4291 (no accessibility\_proxy, no label); crates/teksilo-widgets/src/rich\_text/body.rs:760-846
- **Evidence:**
  - `tree-rich-text-editor-20260925-135811-4106847 audit: "unnamed-control: [entry] '': a focusable entry with no name (its text is 'RichTextEditor — Capability ShowcaseThis window hosts…')"`
  - `text-editor-structure-20260925-141819-270614: FAIL 'the editor and the preview are named': 2 of 2 fail, e.g. [entry] '' … [document frame] ''`
  - `text-editor-caret-20260925-142559-454805 grab_focus scene: SPEECH OUTPUT 'entry RichTextEditor — Capability '`
  - `text-viewer-20260925-142451-427749 grab_focus scene: SPEECH OUTPUT 'document frame This window holds a single RichTextEditor::read_only bound to a TextDocument loaded from an embedded '`
  - `source: crates/teksilo-widgets/src/rich_text/body.rs:760-846 the body never sets a name; rich_text.rs:4281-4291 the wrapper is a GenericContainer; accesskit_consumer filters.rs:30-33 excludes any GenericContainer`
  - `text-editor-structure-20260925-144117-792254: 'the editor and the preview are named' FAIL: 2 of 2 fail, [entry] '' / [document frame] ''`
  - `text-viewer-20260925-144733-954413 tree-launch.txt: [document frame] '' {focusable}`
  - `crates/teksilo-core/src/widget.rs:496-524 (accessibility_proxy); crates/teksilo-widgets/src/spin_box.rs:1832`
- **Reproduced:** every run (tree audit at launch in all rich-text-editor runs; viewer 2 of 2)
- **Verification:** corrected by the verifier. Reproduced: Every run: tree audit at launch in all 25 rich-text-editor runs, the text-editor-structure tree check 2 of 2, and the viewer 2 of 2. Both panes and the viewer's document are unnamed, and RichTextEditor has no label builder (extract\_widget\_api lists none). One statement is wrong: 'an .access\_label on the wrapper … reaches no reader'. The wrapper holds keyboard focus whenever the editor does, and common\_filter always includes a focused node (filters.rs:18-20). A label on the wrapper would therefore be spoken on Tab arrival as '&lt;label&gt; section'. It would sit on the wrong node, never on the text node a screen-reader focus request lands on, and it would be lost on every return (text-02). The fix can use the existing hook: implement Widget::accessibility\_proxy (widget.rs:496-524) on RichTextEditor so that builder overrides and labelled\_by land on the body. That is the SpinBox pattern (spin\_box.rs:1832). A dedicated .label() could then forward to it.
- **Fix idea:** Add RichTextEditor::label(impl Into&lt;LocalizedString&gt;) (and forward access\_label/labelled\_by) to the body node that carries the role, then name the two panes in rich-text-editor and the viewer's document.

### text-08 {#text-08}

Unnamed text fields in the examples: the rich-text-editor search field (focused at launch) and the ime-playground fields read only their placeholder or nothing

- **Example:** ime-playground
- **Scenario:** launch of every rich-text-editor and ime-playground scenario; text-ime
- **Act:** launch of rich-text-editor and ime-playground; text-ime 'the fields in the tree'
- **The reader should get:** The search field is named (for example 'Find in document'), and each ime-playground field carries the caption drawn above it ('Single-line TextInput', 'Multi-line RichTextEditor').
- **The reader gets:** rich-text-editor: SearchField::new(query).placeholder("Find in document…") without .label(). It holds focus at launch and Orca says 'entry heading selected.' ('heading' is the pre-filled query). ime-playground: labeled() stacks a TextWidget caption above each control without any association, so the TextInput reads 'entry compose here.' (its placeholder) and the rich editor has no name. The PasswordField is named only because it has its own .label("Password"). The SearchField is also exposed as an unnamed \[entry\] wrapping a second \[entry\] plus an empty \[status bar\].
- **Platform:** Linux AT-SPI/Orca measured. The missing names apply to every platform.
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/rich\_text\_editor/src/highlight\_controls.rs:216-217; examples/ime\_playground/src/main.rs:140-176
- **Evidence:**
  - `tabwalk-rich-text-editor-20260925-140442-18361 launch: +1167.2 ms object:state-changed:focused 1 [entry] '' ; ORCA SAYS: 'entry heading selected.' ; audit: "unnamed-control: [entry] '': a focusable entry with no name (its text is 'heading')"`
  - `tree-launch.txt: [entry] '' {editable,selectable-text,single-line} text='heading' / [entry] '' {editable,focusable,focused,selectable-text,single-line} text='heading' / [status bar] ''`
  - `text-ime-20260925-142728-476941 orca-debug.out 14:27:30.484878 - SPEECH OUTPUT: 'entry compose here.' ; FAIL 'every text field is named': 2 of 3 fail, e.g. [entry] '' attrs={'placeholder-text': 'compose here'}`
  - `source: examples/rich_text_editor/src/highlight_controls.rs:217 SearchField::new(query.clone()).placeholder(lit!("Find in document…")) with no .label (SearchField::label exists); examples/ime_playground/src/main.rs:171-176 labeled() adds an unassociated TextWidget; main.rs:147 TextInput::new(...).placeholder("compose here")`
  - `text-editor-tab-in-20260925-143636-700740 launch: +1292.0 ms object:state-changed:focused 1 [entry] ''; ORCA SAYS: 'entry heading selected.'; audit unnamed-control [entry] '' (its text is 'heading')`
  - `text-ime-20260925-144833-983323 orca 14:48:36.109917 - SPEECH OUTPUT: 'entry compose here.'; 'every text field is named' FAIL 2 of 3`
- **Reproduced:** every launch (rich-text-editor 10+ runs, ime-playground 4 runs)
- **Verification:** confirmed. Reproduced: Every launch: 25 of 25 rich-text-editor runs said 'entry heading selected.', and 5 of 5 ime-playground runs said 'entry compose here.'.
- **Fix idea:** Give the SearchField .label(lit!("Find in document")). In ime-playground, name each control from its caption with .label()/.access\_label or labelled\_by the caption. The rich editor needs text-07 first.

### text-09 {#text-09}

TextInput caret moves never reach the reader: arrows and Home are silent and the reported caret stays at the last edit

- **Example:** ime-playground
- **Scenario:** text-ime
- **Act:** text-ime 'Left in the TextInput', 'Home in the TextInput', 'type c at the start'
- **The reader should get:** Left over 'ab' reports the caret at 1 and Orca says 'b'; Home reports 0 and Orca says 'a'.
- **The reader gets:** No text-caret-moved event for Left or Home. The Text interface still reports caret 2 after both. Orca knows the keys but says nothing. The next typed 'c' goes in at offset 0 ('cab', text-changed:insert at 0), which proves the real caret was at 0 while AT-SPI said 2. A reader reviewing a single-line field (ime-playground's TextInput, and every TextInput/SearchField built on TextInputField) hears nothing and is told a wrong position.
- **Platform:** Linux AT-SPI/Orca measured. The missing AT update means no platform receives the move (by source).
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:193-256, 452-481
- **Evidence:**
  - `text-ime-20260925-142728-476941 'Left in the TextInput (Orca told of the key)': no event on the bus; FAIL 'a object:text-caret-moved event from [entry]'; FAIL "Orca says exactly 'b'"; note 'the TextInput after Left: caret 2'`
  - `'Home in the TextInput': no event; FAIL "the Text interface's caret is at 0": caret 2`
  - `'type c at the start': 14:28:07.447250 object:text-changed:insert 0 [entry] '' text='c' ; 14:28:07.447626 object:text-caret-moved 1 [entry] '' ; probe text 'cab', caret 1`
  - `text-ime-20260925-142333-394397 'Left in the TextInput': no object:text-caret-moved, Orca said nothing`
  - `source: crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:452-481 binds text_signal at AccessibilityOnly, but cursor_position is only mirrored by an effect (widget_impl.rs:193-205) and never bound, so a caret-only move never marks the AT tree dirty; rich_text/body.rs:190-214 documents and fixes exactly this for the rich editor ('a screen reader hears the caret frozen at the last edit')`
  - `text-ime-20260925-143803-736718 events: 14:38:30.161657 text-changed:insert 1 'b'; no event for Left or Home; 14:38:41.976983 object:text-changed:insert 0 1 'c' (the real caret was 0 while AT-SPI reported 2)`
  - `text-ime-20260925-144619-914950 and -144833-983323: 'the Text interface's caret is at 0' FAIL caret 2`
  - `verify-text-search-20260925-144407-857015: after Ctrl+A,t,e,h the first event is at +287.1 ms: text-changed:delete 'heading', and there is no selection event for Ctrl+A itself`
  - `verify-text-ctxmenu-field-20260925-145430-1150948: 14:54:39.709337 caret-moved 60 (after typing), Home unreported, 14:54:43.085834 caret-moved 51 when the menu opened`
- **Reproduced:** 2 of 2 runs (Left silent in both; Home and the stale caret measured in the second)
- **Verification:** corrected by the verifier. Reproduced: text-ime: 3 of 3 runs. Left and Home emitted nothing, the AT caret stayed 2, and 'c' went in at offset 0. Ctrl+A in a non-empty TextInput emitted no text-selection-changed in 3 of 3 runs, and in the SearchField in 1 of 1 run of verify-text-search. Deterministic. Reproduced, and wider than stated. TextInputField binds only the text, feedback, active-descendant, controls and reveal signals at AccessibilityOnly (widget\_impl.rs:209-256, 452-481). The cursor position and anchor are not bound (the effect at :193-205 only mirrors them), so no caret or selection change reaches AccessKit until something else re-walks the tree. Selection is affected as well as the caret. In the 'Ctrl+A BackSpace' scene of all three text-ime runs the bus carries no selection event at all, and in verify-text-search Ctrl+A in the SearchField shows nothing until the next letter. A stale caret also surfaces when an unrelated tree update forces a re-walk: in verify-text-ctxmenu-field, opening the context menu produced object:text-caret-moved 51 while the reader had last been told 60. This affects every TextInputField-based field (TextInput, SearchField, PasswordField and the field inside SpinBox), on every platform, because the TreeUpdate is never produced.
- **Fix idea:** Bind the field's cursor position and anchor signals at BindingLevel::AccessibilityOnly in TextInputField::build, as RichTextEditorBody does, with a test that a caret-only move changes the emitted text selection.

### text-10 {#text-10}

An empty TextInput exposes no Text interface, so the first text entered (a typed letter, an IME commit) produces no text-changed event

- **Example:** ime-playground
- **Scenario:** text-ime
- **Act:** text-ime 'F1: Pinyin … commit 你好 in the entry' (empty field); 'type a into the empty TextInput'
- **The reader should get:** Committing 你好 into the empty field, or typing 'a' into it, emits object:text-changed:insert like every later insertion.
- **The reader gets:** The empty field's interfaces are \['Accessible', 'Component'\], with no Text and no EditableText. The first insertion emits only text-caret-moved and no text-changed:insert. After the first character, Text and EditableText appear and 'b' is reported normally. Orca's key echo hides this for typed letters, but an IME commit into an empty field is never presented, and a client that checked the interfaces while the field was empty sees no Text.
- **Platform:** Linux AT-SPI measured. By source, accesskit\_consumer's supports\_text\_ranges (text.rs:1402-1406) also gates text events on the other adapters.
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-core/src/accessibility/text\_runs.rs:228-296 (from\_geometry with an empty geometry)
- **Evidence:**
  - `text-ime-20260925-142728-476941 note: the empty TextInput at launch: interfaces ['Accessible', 'Component']`
  - `'F1: Pinyin ni → nihao → commit 你好 in the entry': 14:27:38.246915 object:text-caret-moved 2 [entry] '' only; FAIL 'a object:text-changed:insert event from [entry]'; probe after: '你好'`
  - `'type a into the empty TextInput': 14:27:51.694520 object:text-caret-moved 1 [entry] '' only; FAIL text-changed:insert ; 'type b after it': 14:27:55.590138 object:text-changed:insert 1 [entry] '' text='b'`
  - `source: crates/teksilo-core/src/accessibility/text_runs.rs:262-282 from_geometry adds no line when the measured text is empty and the geometry has no lines, so no run is emitted, contrary to the intent stated at text_input_field/widget_impl.rs:1103-1108 and tested only for a flat source (text_runs.rs:950-966); accesskit_atspi_common adapter.rs:122-124 drops the text change when the old node has no text ranges`
  - `text-ime-20260925-143803-736718 note: the empty TextInput at launch: interfaces ['Accessible', 'Component']; 'type a into the empty TextInput': only object:text-caret-moved; 14:38:30.161657 text-changed:insert 1 'b' for the second letter`
  - `same run 'scene: Ctrl+A BackSpace': no object:text-changed:delete and no selection event; Orca says only 'A' and 'backspace' (key echo)`
  - `orca-debug.out 14:38:16.740751 text-changed:insert for [entry] … interfaces='Component' … 14:38:16.751950 DEFAULT: Not speaking inserted string due to lack of cause (the IME commit is not spoken even into a non-empty field, for lack of a key: see text-15)`
- **Reproduced:** F1 into the empty field 3 of 3 runs; typed 'a' 2 of 2 runs
- **Verification:** corrected by the verifier. Reproduced: text-ime: 3 of 3 runs. The empty field exposed interfaces \['Accessible','Component'\]. F1 into it gave only caret-moved, and typing 'a' into it gave only caret-moved. Emptying the field (Ctrl+A, BackSpace on '你好ê') emitted no text-changed:delete, 3 of 3. Reproduced, and it is symmetric: the edit that empties a field is lost as well as the one that fills it. The old node supports text ranges and the new one does not, and emit\_text\_change\_if\_needed\_parent requires both (atspi adapter.rs:122-124). The source analysis holds. from\_geometry adds no line when the measured text is empty and the geometry has none, so no run is emitted (text\_runs.rs:262-282), against the stated intent at text\_input\_field/widget\_impl.rs:1103-1108. The test at text\_runs.rs:950-966 covers only the flat source. The rich editor is not affected: its empty document still reported '你好' as text-changed:insert 0 in text-ime. By macOS source, and not verified: accesskit\_macos posts AXSelectedTextChanged only when both old and new nodes support text ranges (event.rs:291-297), so VoiceOver would not echo the first character typed into an empty field either.
- **Fix idea:** In TextRunSource::from\_geometry, when the text is empty and the geometry has no lines, push one empty SourceLine at the origin (as the flat source does), so an empty measured field still emits its one run. Test with a laid-out empty field.

### text-11 {#text-11}

The formatting toolbar cannot be reached by keyboard, yet AT-SPI reports every button as focusable

- **Example:** rich-text-editor
- **Scenario:** text-editor-shift-tab, text-editor-format
- **Act:** text-editor-shift-tab (Shift+Tab walk from the search field); rich-text-editor tabwalk; text-editor-format 'Tab from the editor to the toolbar'
- **The reader should get:** Keyboard and screen reader users can reach and operate alignment, lists, indent, blockquote, insert table, the table operations, undo and redo, either through the Tab order or a roving toolbar.
- **The reader gets:** Every IconButton is .focusable(false). Shift+Tab from the search field stops at Spell, Syntax, Font family, Heading level, Theme, then the preview's and the editor's sections, never on a formatting button. Only B/I/U, undo/redo and indent have keyboard shortcuts. Alignment, bullets, numbering, blockquote, insert table and all seven table operations have no keyboard route; a screen reader user can reach them only through an AT-SPI action from flat review. Each button still advertises Action::Focus, so AT-SPI marks them 'focusable', which misleads object navigation.
- **Platform:** Linux AT-SPI/Orca measured. The Tab order is platform-independent.
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/rich\_text\_editor/src/format\_toolbar.rs:586-609; crates/teksilo-widgets/src/icon\_button.rs:895
- **Evidence:**
  - `text-editor-shift-tab-20260925-142442-423219 focus sequence: 14:24:50.454661 focused 1 [check box] 'Spell'; 14:24:52.369099 [check box] 'Syntax'; 14:24:54.285472 [combo box] 'Font family'; 14:24:56.234522 [combo box] 'Heading level'; 14:24:58.137966 [combo box] 'Theme'; 14:25:00.053856 [section] ''; 14:25:01.965348 [separator] 'Splitter divider'; 14:25:03.882786 [section] ''`
  - `FAIL 'a formatting toggle (Bold) is among the Shift+Tab stops'`
  - `tree-rich-text-editor-20260925-135811-4106847 tree-launch.txt: [toggle button] 'Bold (Ctrl+B)' desc='Bold (Ctrl+B)' {focusable}`
  - `source: examples/rich_text_editor/src/format_toolbar.rs:593 and :606 '.focusable(false)' on every toggle_btn/plain_btn; crates/teksilo-widgets/src/icon_button.rs:895 adds Action::Focus unconditionally, and accesskit_consumer node.rs:111-113 derives focusable from it`
  - `text-editor-shift-tab-20260925-144539-894036 focus sequence: 14:45:48.255973 Spell, 14:45:50.174052 Syntax, 14:45:52.101514 Font family, 14:45:54.020026 Heading level, 14:45:55.935639 Theme, 14:45:57.855839 section (preview), 14:45:59.772444 Splitter divider, 14:46:01.704094 section (editor); Shift+Tab 9 and 10 left focus in the editor`
- **Reproduced:** 1 of 1 Shift+Tab walk and 1 of 1 Tab walk; the Tab order is deterministic (source)
- **Verification:** confirmed. Reproduced: Shift+Tab walk: 1 of 1 run (the order is deterministic). Ctrl+Tab from the AT-focused editor never reached a toolbar button: 2 of 2 runs of text-editor-format.
- **Fix idea:** Example: make the toolbar keyboard-reachable (Toolbar's roving tab stop, with the buttons focusable) and return focus to the editor after activation, or add shortcuts for every command. Framework: IconButton should advertise Action::Focus only when it is focusable.

### text-12 {#text-12}

Ctrl+B / Italic change formatting with no feedback a reader can get

- **Example:** rich-text-editor
- **Scenario:** text-editor-format
- **Act:** text-editor-format 'Ctrl+B on the selected word', 'activate Italic through AT-SPI'
- **The reader should get:** After Ctrl+B the reader hears that bold is now on (or can query it, see text-06).
- **The reader gets:** The Bold toggle's pressed state flips (state-changed:pressed 1 on an unfocused button, which Orca ignores). Orca says only its key echo 'B', and the text attributes stay empty. With text-06, a reader has no way to learn the word is now bold, or to tell a toggle-on from a toggle-off.
- **Platform:** Linux AT-SPI/Orca measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/rich\_text/keyboard.rs:502-514
- **Evidence:**
  - `text-editor-format-20260925-142932-527100 'Ctrl+B on the selected word': 14:29:48.402520 object:state-changed:pressed 1 [toggle button] 'Bold (Ctrl+B)' ; orca 14:29:48.071943 - SPEECH OUTPUT: 'B' (key echo) ; FAIL 'the reader hears that bold is now on': Orca said nothing`
  - `'activate Italic through AT-SPI': object:state-changed:pressed 1 [toggle button] 'Italic (Ctrl+I)' (pass), nothing spoken`
  - `source: crates/teksilo-widgets/src/rich_text/keyboard.rs handles Ctrl+B/I/U with no announcement; no ctx.announce anywhere in rich_text`
  - `text-editor-format-20260925-144314-834725: 14:43:31.073533 object:state-changed:pressed 1 [toggle button] 'Bold (Ctrl+B)'; 'the reader hears that bold is now on' FAIL: Orca said nothing in this act (key echo excluded)`
- **Reproduced:** 2 of 2 runs
- **Verification:** confirmed. Reproduced: 2 of 2 runs of text-editor-format
- **Fix idea:** Fix text-06 first. Optionally have the editor announce ('Bold on' / 'Bold off') through the tree's announcer when its own formatting shortcut toggles a format.

### text-13 {#text-13}

Headings in rich text are unnamed and carry no level on AT-SPI

- **Example:** rich-text-editor
- **Scenario:** text-editor-structure
- **Act:** text-editor-structure 'the document's structure in the tree'; viewer tree
- **The reader should get:** Each heading node is named by its text and carries its level (Orca and object navigation say 'heading level 2, Heading scale').
- **The reader gets:** All 38 heading nodes (19 per pane) are '\[heading\] ''', each holding a \[label\] with the text. There is no 'level' attribute. Object navigation or flat review meets a nameless 'heading'. (Orca's default script never announces headings on caret moves in any toolkit, so line reading itself is unaffected.)
- **Platform:** Linux AT-SPI measured. By source, Windows exports the level (accesskit\_windows node.rs:500, 690-699); the empty name applies to every platform.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/rich\_text/body/flow\_walk.rs:383-389
- **Evidence:**
  - `text-editor-structure-20260925-142333-394383: FAIL 'every heading has a name': 38 of 38 fail, e.g. [heading] '' attrs=None children=[('label', 'Heading scale')]`
  - `FAIL 'every heading has a level': 38 of 38 fail`
  - `source: crates/teksilo-widgets/src/rich_text/body/flow_walk.rs:384-385 push_paragraph_child + set_paragraph_as_heading, no label or labelled_by; accesskit_consumer node.rs:720-733 name-from-descendants only for Button/CheckBox/Link/MenuItem…/RadioButton, not Heading; accesskit_atspi_common node.rs:415-437 attributes() exports no 'level' (upstream)`
  - `text-editor-structure-20260925-144117-792254: 'every heading has a name' FAIL 38 of 38, e.g. [heading] '' children=[('label', 'RichTextEditor — Capability Showcase')]; 'every heading has a level' FAIL 38 of 38`
- **Reproduced:** 2 of 2 runs (tree)
- **Verification:** confirmed. Reproduced: 2 of 2 runs of text-editor-structure (38 of 38 headings have no name and no level). The viewer tree shows the same.
- **Fix idea:** Set labelled\_by from the heading node to its Label container, which names it without copying the string. Report the missing AT-SPI 'level' attribute upstream to accesskit\_atspi\_common.

### text-14 {#text-14}

The Heading level, Font family and Theme combo boxes never tell the reader their current value

- **Example:** rich-text-editor
- **Scenario:** text-editor-shift-tab
- **Act:** text-editor-shift-tab stops 3-5
- **The reader should get:** 'Heading level combo box, Normal', 'Font family combo box, &lt;family&gt;', 'Theme combo box, Light'.
- **The reader gets:** Orca says 'Font family combo box.', 'Heading level combo box.' and 'Theme combo box.', with no value. The ComboBox sets its selected label as the AccessKit value, but accesskit\_atspi\_common exports a string value only as the name of a Role::Label, and a closed combo has no selected child (Selection reports 0). A reader cannot tell which heading level the caret's paragraph has, although the picker tracks it.
- **Platform:** Linux AT-SPI/Orca measured. On Windows the ValuePattern likely carries it (not verified).
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/combo\_box.rs:1247-1265 (a Teksilo-side workaround is possible)
- **Evidence:**
  - `text-editor-shift-tab-20260925-142442-423219 orca-debug.out: 14:24:54.342404 - SPEECH OUTPUT: 'Font family combo box.' ; 14:24:56.273016 - SPEECH OUTPUT: 'Heading level combo box.' ; 14:24:58.214976 - SPEECH OUTPUT: 'Theme combo box.'`
  - `tree-launch.txt: [combo box] 'Heading level' {focusable} (no value, no text; selected_children 0)`
  - `source: crates/teksilo-widgets/src/combo_box.rs:1249-1263 set_value(label); accesskit_atspi_common-0.20.0 node.rs:38-44 name from value only when label_comes_from_value (Role::Label); no string-value export for other roles`
  - `text-editor-shift-tab-20260925-144539-894036 orca: 14:45:52.181230 SPEECH OUTPUT: 'Font family combo box.'; 14:45:54.071655 'Heading level combo box.'; 14:45:56.006126 'Theme combo box.'`
- **Reproduced:** 1 of 1 walk; deterministic
- **Verification:** confirmed. Reproduced: 1 of 1 Shift+Tab walk. The Theme combo read 'Theme combo box.' with no value in every text-viewer (2) and text-editor-format (2) run. Deterministic.
- **Fix idea:** Upstream: have accesskit\_atspi\_common expose a combo box's value (for example through a Text interface or a selected child). In Teksilo meanwhile, keep the selected option present as a selected child of the closed combo, or give the combo a text run holding its value, so Orca's combo box generator can read it.

### text-15 {#text-15}

Orca in a Wayland session never learns of a key typed into a Teksilo window, so it says nothing about any caret move

- **Example:** rich-text-editor
- **Scenario:** text-editor-keys-unreported, text-editor-caret
- **Act:** text-editor-keys-unreported (Down, Right, Ctrl+Right, End with focus on the entry, no key reported)
- **The reader should get:** Down reads the new line, Right the character, and Ctrl+Right the word, as they are once Orca knows the key (text-editor-caret).
- **The reader gets:** text-caret-moved reaches Orca, which logs 'DEFAULT: Presenting text at new caret position' and then says nothing: \_presentTextAtNewCaretPosition chooses line, word or character from the last key it saw, and it saw none. Orca's log has 0 KEYBOARD\_EVENT lines for the whole run. Under WAYLAND\_DISPLAY, libatspi gives Orca its legacy device, which gets keys only from applications that report them to the registry's DeviceEventController, as the GTK and Qt bridges do. accesskit\_unix 0.23 never does. A run that reports each key to the registry the way those bridges do (my module's tell\_orca) gets full caret speech (text-editor-caret), which confirms the path. By source, Orca's key echo and Orca 46's own command keys depend on the same reports; this was not tested.
- **Platform:** Linux, Wayland session with Orca 46 (legacy device) measured. X11 sessions (Orca reads the X keyboard) and newer Orcas using a compositor keyboard monitor are not affected. Windows and macOS are not affected.
- **Severity:** high; **layer:** upstream
- **Status:** Fixed by `276ff85b` (key-report).
- **Where:** accesskit\_unix (no DeviceEventController reports); a Teksilo-side workaround is possible in teksilo-platform's key path
- **Evidence:**
  - `text-editor-keys-unreported-20260925-142817-494623 'Down, no key reported to Orca': 14:28:28.927401 object:text-caret-moved 28 [entry] '' ; orca 14:28:28.932693 - DEFAULT: Presenting text at new caret position ; no SPEECH OUTPUT`
  - `'Right, no key reported to Orca': 14:28:32.811535 object:text-caret-moved 29 [entry] '' ; 14:28:32.818518 - DEFAULT: Presenting text at new caret position ; no SPEECH OUTPUT`
  - `orca-debug.out: 0 'KEYBOARD_EVENT:' lines (a run with keys reported: 24)`
  - `/usr/lib/python3/dist-packages/orca/scripts/default.py:1967-2018 _presentTextAtNewCaretPosition speaks only after line, word, char, page or boundary navigation keys; libatspi.so strings WAYLAND_DISPLAY / ATSPI_USE_LEGACY_DEVICE / atspi_device_legacy_new; accesskit_unix-0.23.0/src has no DeviceEventController or NotifyListeners call`
  - `text-editor-keys-unreported-20260925-144501-877596: caret-moved on Down, Right and Ctrl+Right; orca 14:45:13.568713, 14:45:17.458210, 14:45:21.366989 DEFAULT: Presenting text at new caret position, with no SPEECH OUTPUT after; grep -c 'KEYBOARD_EVENT:' = 0`
  - `text-editor-keys-unreported-20260925-145001-1030706: same; KEYBOARD_EVENT count 0`
- **Reproduced:** 2 of 2 runs (plus the first exploratory run); deterministic
- **Verification:** confirmed. Reproduced: 2 of 2 runs of text-editor-keys-unreported, with 0 KEYBOARD\_EVENT lines in Orca's log each time. Deterministic.
- **Fix idea:** Report each key to org.a11y.atspi.DeviceEventController.NotifyListenersSync on the a11y bus (Qt's QSpiDeviceEvent signature (uiiiisb)) before the application handles it, in accesskit\_unix upstream or in teksilo-platform's winit key path, and honour the 'consumed' reply so Orca's commands work.

### text-16 {#text-16}

After a screen reader moves focus into the editor, Ctrl+Tab jumps to the window's first control instead of the next one

- **Example:** rich-text-editor
- **Scenario:** text-editor-format
- **Act:** text-editor-format 'Tab from the editor to the toolbar' (focus previously put on the entry by AT-SPI grab\_focus)
- **The reader should get:** Ctrl+Tab from the editor moves to the next control in order (the Splitter divider), as it does after keyboard entry (text-editor-tab-in).
- **The reader gets:** Focus goes to the Theme combo box, the first control of the window. The AT Focus action parks the widget tree's focus on the non-focusable body widget, so traversal restarts from the top.
- **Platform:** Linux measured (keyboard behaviour, platform-independent)
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/rich\_text/body.rs:839 (the body advertises Focus); crates/teksilo-core AT Focus dispatch (walks down only)
- **Evidence:**
  - `text-editor-format-20260925-142932-527100: 14:29:55.985205 object:state-changed:focused 1 [combo box] 'Theme' ; 14:29:55.985587 object:state-changed:focused 0 [entry] '' ; 14:29:56.049042 - SPEECH OUTPUT: 'Theme combo box.'`
  - `contrast text-editor-tab-in-20260925-142522-443443 'Ctrl+Tab out of the editor': 14:25:47.251042 object:state-changed:focused 1 [separator] 'Splitter divider'`
  - `source: rich_text.rs:3511 the body is non-focusable; the AT Focus action targets the body node (body.rs:840)`
  - `verify-text-traversal-20260925-144329-840480: 'Ctrl+Tab after a focus request': focused 1 [combo box] 'Theme'; 'Ctrl+Shift+Tab after a focus request': focused 1 [section] '' path …/79228171110447075942195003392 (the preview wrapper; see text-editor-shift-tab 14:45:57.855839)`
  - `verify-text-traversal-20260925-145042-1051535: same two results`
  - `text-editor-format-20260925-144314-834725: 14:43:38.629002 focused 1 [combo box] 'Theme' on Ctrl+Tab`
- **Reproduced:** 2 of 2 runs (text-editor-format)
- **Verification:** corrected by the verifier. Reproduced: Forward (Ctrl+Tab lands on the Theme combo): 4 of 4 runs (text-editor-format 2, verify-text-traversal 2). Backward (Ctrl+Shift+Tab lands on the preview's wrapper, the window's last stop): 2 of 2 runs of verify-text-traversal. Real, and it happens in both directions. After an AT Focus on the text node, Ctrl+Tab goes to the window's first control and Ctrl+Shift+Tab goes to its last, the preview's section (path …75942195003392, the stop Shift+Tab reaches right after Theme in the walk). So traversal restarts from the ends because tree focus is parked on the non-focusable body. The core walks an AT Focus on a non-focusable composite down to a focusable descendant (accessibility\_impl.rs:1015-1040), never up to a focusable ancestor. The body has no focusable descendant, so focus stays on it. The last two acts of verify-text-traversal are void: my scenario assumed the Ctrl+Shift+Tab landing point, so its Tab went to Theme and its Down changed the theme.
- **Fix idea:** Route the body's Action::Focus to the focusable wrapper widget (the same resolution text-01 needs, in the other direction).

### text-17 {#text-17}

Every TextInput/SearchField carries an always-present, empty, unnamed status bar node

- **Example:** rich-text-editor, ime-playground, rich-text-viewer
- **Scenario:** launch
- **Act:** launch trees of rich-text-editor and ime-playground
- **The reader should get:** A validation status node exists only while it has a message, or at least is not a 'status bar' in the window when empty.
- **The reader gets:** An empty \[status bar\] '' follows the ime-playground TextInput and PasswordField, and sits inside the search field. Orca's object navigation and its 'read status bar' command can land on an empty status bar in these windows.
- **Platform:** Linux AT-SPI measured
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/primitives/validation\_strip.rs:148-165
- **Evidence:**
  - `tabwalk-ime-playground-20260925-141044-142257 launch: +408.5 ms object:children-changed:add [frame] '' -> [status bar] '' ; +411.3 ms object:children-changed:add [frame] '' -> [status bar] ''`
  - `tree-rich-text-editor-20260925-135811-4106847 tree-launch.txt: [entry] '' … / [entry] '' {…focused…} / [status bar] ''`
  - `source: crates/teksilo-widgets/src/primitives/validation_strip.rs:149 builder.set_role(Role::Status) whether or not a message is shown`
  - `text-ime-20260925-143803-736718 launch: +430.6 ms and +435.4 ms object:children-changed:add [frame] '' -> [status bar] ''`
  - `text-editor-tab-in-20260925-143636-700740 tree-launch.txt: [entry] '' … / [status bar] '' inside the search field`
- **Reproduced:** every launch
- **Verification:** confirmed. Reproduced: Every launch of ime-playground (5 of 5) and rich-text-editor (tree-launch).
- **Fix idea:** Hide the ValidationStrip from AT while it is pristine or empty (it can stay the described\_by target once it has text).

### text-v01 {#text-v01}

Shift+F10 moves the caret to the middle of the editor or field and drops the selection, so the menu's Paste writes somewhere else

- **Example:** rich-text-editor
- **Scenario:** verify-text-ctxmenu-caret, verify-text-ctxmenu-field
- **Act:** verify-text-ctxmenu-caret: Ctrl+Home, Shift+F10, Escape; Ctrl+Home, Shift+F10, 'p' + Return (Paste); select the first word, Shift+F10. verify-text-ctxmenu-field (ime-playground TextInput): Home, Shift+F10.
- **The reader should get:** A menu opened from the keyboard acts where the caret and selection are. Escape leaves the caret at 0, Paste inserts at 0, and Copy or Cut act on the selected word.
- **The reader gets:** Opening the menu moves the caret from 0 to 645, the character under the centre of the editor pane. Paste inserts 'RichTextEditor' at 645, in the middle of a word ('recievRichTextEditore'). With a word selected, the selection collapses: Orca says 'Text unselected.' and then 'menu.'. After Escape the selection is gone and the caret is at 645. In a TextInput the caret jumps from 0 to 51. A reader who arrived by Tab (text-01) gets no caret event at all, so the paste lands where they never were, without their knowing.
- **Platform:** Linux measured. The behaviour itself is platform-independent keyboard handling.
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `27022d39` (context-menu-caret). Fixed part: Shift+F10 / Menu key in RichTextEditor, CodeEditor or a TextInputField-based field no longer moves the caret or drops the selection. The menu's Paste inserts at the caret, and Cut/Copy act on the selection. A right-click still moves the caret to the click..
- **Where:** crates/teksilo-core/src/widget\_tree/pointer\_router.rs:1264-1284; crates/teksilo-widgets/src/rich\_text/context\_menu.rs:105-123; crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:806-817
- **Evidence:**
  - `verify-text-ctxmenu-caret-20260925-144603-905511: 14:46:14.264312 caret-moved 0; Shift+F10 14:46:16.639943 object:text-caret-moved 645; 14:46:31.177793 object:text-changed:insert 645 'RichTextEditor'; orca 14:46:31.222699 AXText: Text of [entry] (598-684): 'squiggle is visible: you will definately recievRichTextEditore teh sepe…'`
  - `verify-text-ctxmenu-caret-20260925-144841-989699: selection [[0, 14]] before; Shift+F10 14:49:16.204173 object:text-selection-changed + caret-moved 645; orca 14:49:16.221092 SPEECH OUTPUT: 'Text unselected.'; after Escape selections [] caret 645`
  - `verify-text-ctxmenu-caret-20260925-145227-1091797: the same (645, paste at 645, 14:52:59.754902 'Text unselected.')`
  - `verify-text-ctxmenu-field-20260925-145430-1150948 and -145502-1162551: TextInput after Home, Shift+F10: object:text-caret-moved 51`
  - `source: crates/teksilo-core/src/widget_tree/pointer_router.rs:1264-1284 (the keyboard menu is anchored at the centre of the target's bounds); crates/teksilo-widgets/src/rich_text/context_menu.rs:105-123 (default_factory calls reposition_caret_for_context_menu for every invocation); rich_text/mouse.rs:1013-1038 (a hit outside the selection collapses it); primitives/text_input_field/widget_impl.rs:806-817 + mouse.rs:295 (the same in the field)`
- **Reproduced:** RichTextEditor caret jump and paste 3 of 3 runs; selection lost 2 of 2 runs; TextInput caret jump 2 of 2 runs. Deterministic.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Anchor a keyboard-opened menu at the target's focus\_reveal\_rect (RichTextEditor already returns the caret rect there, rich\_text.rs:4268), and pass the factory whether it was opened by keyboard or by pointer. Reposition the caret only for a pointer. Test: Ctrl+Home, Shift+F10, Paste inserts at 0, and a selection survives Shift+F10.

### text-v02 {#text-v02}

Closing a TextInput's context menu with Escape selects the whole field, so the next key replaces its content

- **Example:** ime-playground
- **Scenario:** verify-text-ctxmenu-field
- **Act:** verify-text-ctxmenu-field (ime-playground): type 60 letters, Home, Shift+F10, Escape, type x
- **The reader should get:** Escape returns focus to the field with its caret or selection as before the menu, and 'x' goes in as one character.
- **The reader gets:** When focus comes back from the menu the whole field is selected. Orca says 'entry abcdefghij… selected.'. The next 'x' deletes all 60 characters and leaves 'x'.
- **Platform:** Linux measured. The behaviour itself is platform-independent.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `27022d39` (context-menu-caret). Fixed part: closing a TextInputField's own context menu (Escape, a menu command such as Paste, or a click outside) no longer selects the whole field. Focus comes back to the caret and selection the menu found..
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:700-709
- **Evidence:**
  - `verify-text-ctxmenu-field-20260925-145430-1150948 'scene: Escape': object:text-selection-changed + caret-moved 60 on [entry]; ORCA SAYS 'entry abcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghij selected.'; 'type x': 14:54:48.304558 object:text-changed:delete 0 60 'abcdefghij…', 14:54:48.304815 insert 0 'x'; Orca 'Selection deleted.'; field text after: 'x'`
  - `verify-text-ctxmenu-field-20260925-145502-1162551: identical (14:55:20.202479 delete 0 60, insert 'x')`
  - `source: crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:700-709: every focus gain without a hovering pointer counts as a keyboard arrival and calls cursor.select(SelectionType::Document), including focus coming back from the field's own menu. The focus-loss branch at :711-722 preserves the selection precisely for that menu.`
- **Reproduced:** 2 of 2 runs
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Select all only when focus arrives by Tab traversal. Restore the previous caret and selection when focus returns from the field's own overlay.

### text-v03 {#text-v03}

The editor's context menu cannot be operated by a reader: focus lands on an unnamed menu, Down is silent, and the disabled Cut and Copy report enabled

- **Example:** rich-text-editor
- **Scenario:** verify-text-ctxmenu, verify-text-ctxmenu-caret
- **Act:** verify-text-ctxmenu: Shift+F10 in the editor, then Down, then Escape. verify-text-ctxmenu-caret: the menu opened over a collapsed selection.
- **The reader should get:** 'Cut, dimmed' (or the first item) is read on opening, each arrow reads the next item, and disabled items are exposed as disabled.
- **The reader gets:** Orca says 'menu.' (\[menu\] '' takes focus). Down puts no event on the bus and Orca says nothing. Cut and Copy were built .enabled(has\_selection) with has\_selection false, yet report 'enabled','sensitive'. This is the MenuList/MenuItem defect the menus package reports (the menu\_list.rs arrow handler moves a private focused\_index; a disabled item is not exposed), reached here through RichTextEditor's own menu. It is listed so this report can cite it, not as a new root cause.
- **Platform:** Linux measured
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `9636094c` (menus). Fixed part: the arrow part only.
- **Where:** crates/teksilo-widgets/src/menu\_list.rs (arrow handling); crates/teksilo-widgets/src/menu\_item/widget\_impl.rs (disabled exposure)
- **Evidence:**
  - `verify-text-ctxmenu-20260925-144245-823122: 14:42:58.514007 object:state-changed:focused 1 [menu] ''; orca 14:42:58.605091 SPEECH OUTPUT: 'menu.'; act 'Down in the context menu': no event on the bus, only the key echo`
  - `verify-text-ctxmenu-20260925-144936-1017457: the same (14:49:51.754848 'menu.', Down silent)`
  - `verify-text-ctxmenu-caret-20260925-144841-989699 tree-Shift-F10-with-the-first-word-selected.txt: [menu item] 'Cut' and 'Copy' states ['enabled','sensitive','showing','visible'], although the selection had just collapsed ('Text unselected.' 14:49:16.221092)`
  - `source: crates/teksilo-widgets/src/rich_text/context_menu.rs:127-160 (Cut/Copy .enabled(has_selection))`
- **Reproduced:** 2 of 2 runs (menu focus and silent Down); 2 of 2 (disabled rows exposed as enabled)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Fixed with the menus package's MenuList/MenuItem findings. Nothing specific to the editor.

### text-v04 {#text-v04}

The Highlighter search tells a reader nothing: no match count, and the matches are not in the text attributes

- **Example:** rich-text-editor
- **Scenario:** verify-text-search
- **Act:** verify-text-search: select the query in the search field and type 'teh'
- **The reader should get:** The reader learns how many matches there are (a status or an announcement) and can find them, for example as a highlight attribute on the text.
- **The reader gets:** Only the field's own text changes reach the bus. There is no announcement, no status and no name change. The attribute run at the 'teh' match is {} over 0..7596. The matches are painted in yellow and orange only.
- **Platform:** Linux measured. The missing count is platform-independent.
- **Severity:** medium; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/rich\_text\_editor/src/highlight\_controls.rs:180-217
- **Evidence:**
  - `verify-text-search-20260925-144407-857015: 'an announcement or a status tells how many matches' FAIL: no announcement and no name change; the only events are text-changed:delete 'heading', insert 't','e','h' on the search [entry]`
  - `same run: attributes at the 'teh' match: {'attrs': {}, 'start': 0, 'end': 7596}`
  - `source: examples/rich_text_editor/src/highlight_controls.rs:180-212 builds a FindSession (which has match_count()) and exposes nothing; crates/teksilo-widgets/src/rich_text/body.rs:786-796 leaves highlight overlays out of the AT walk by design`
- **Reproduced:** 1 of 1 run (deterministic: nothing in the source produces a count)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Example: show the match count in a live Status label (or ctx.announce it). Framework: optionally expose find highlights as a bg-color text attribute on the editable pane.

### text-v05 {#text-v05}

SearchField publishes a second, non-focusable entry holding the same text around the focused one

- **Example:** rich-text-editor, ime-playground, rich-text-viewer
- **Scenario:** launch
- **Act:** launch tree of rich-text-editor
- **The reader should get:** One search entry: the focused field, carrying the search role and the popup semantics.
- **The reader gets:** '\[entry\] '' {editable,selectable-text,single-line} text='heading'' wraps the focused '\[entry\] '' … text='heading''. Object navigation and flat review meet two entries for one field. The outer node, Role::SearchInput, also holds has\_popup, expanded and controls, on a node that never takes focus.
- **Platform:** Linux measured (tree). The duplicate node is in the AccessKit tree on every platform.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/search\_field.rs:772-803
- **Evidence:**
  - `text-editor-tab-in-20260925-143636-700740 tree-launch.txt: [entry] '' {editable,selectable-text,single-line} text='heading' / [entry] '' {editable,focusable,focused,selectable-text,single-line} text='heading' / [status bar] ''`
  - `source: crates/teksilo-widgets/src/search_field.rs:772-803 (Role::SearchInput on the non-focusable outer node; its text comes from the inner field's runs, found at any depth)`
- **Reproduced:** every rich-text-editor launch (25 of 25)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Make the outer node a GenericContainer and move the search role and popup semantics onto the focused field through Widget::accessibility\_proxy, as SpinBox does.
