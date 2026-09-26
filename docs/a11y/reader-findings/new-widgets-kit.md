<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# New widgets kit

Examples: `new-widgets-kit`.
14 findings: 5 high, 4 medium, 5 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [newkit-01](#newkit-01) | new-widgets-kit | A banner dismissed through its Collapse stays in the reader's tree and Tab order: focus is left on an invisible button, and restoring it is never announced | high | Linux | open |
| [newkit-02](#newkit-02) | new-widgets-kit | SearchField suggestions are silent: the popup's combobox semantics sit on a non-focused outer node, and a highlight change never re-publishes accessibility | high | Linux | open |
| [newkit-03](#newkit-03) | new-widgets-kit | InputDialog's field has no name: the prompt is a separate label, so returning to the field says only 'entry untitled.txt' | high | Linux | open |
| [newkit-04](#newkit-04) | new-widgets-kit | At launch all three banners are announced at once, in random order, each cutting the last, and all are then cut by the window activation | medium | Linux | open |
| [newkit-05](#newkit-05) | new-widgets-kit | A banner's dismiss button is announced as 'Clear' (IconButton::clear), not as dismissing the banner | medium | Linux | open |
| [newkit-06](#newkit-06) | new-widgets-kit | An empty text field publishes no Text or EditableText interface: typing its first character and deleting its last emit no text-changed event | medium | Linux | fixed |
| [newkit-07](#newkit-07) | new-widgets-kit | The SearchField and the FilePickerField entry have no accessible name: once filled, the reader hears only 'entry Apricot' | high | Linux | open (example) |
| [newkit-08](#newkit-08) | new-widgets-kit | The text field's visible clear (X) button has no accessibility node and cannot be focused | low | Linux | open |
| [newkit-09](#newkit-09) | new-widgets-kit | Every TextInput carries an empty, unnamed 'status bar' (its ValidationStrip), including inside the dialog | low | Linux | open |
| [newkit-10](#newkit-10) | new-widgets-kit | The example's status lines ('Submitted 1 time(s).', 'Last result: …', 'Filtering: …') never reach the reader; empty readout labels are exposed | low | Linux | open (example) |
| [newkit-11](#newkit-11) | new-widgets-kit | A banner's body text is never spoken when a reader tabs into the banner's controls | low | Linux | open |
| [newkit-12](#newkit-12) | new-widgets-kit | CommandLinkButton folds its description into its name, frozen at build | low | Linux | open |
| [newkit-M1](#newkit-m1) | new-widgets-kit | Caret moves and selection changes in any TextInputField are never published: arrow keys, Home/End, Shift+Home/End and Ctrl+A put nothing on the bus, and the stale selection is spoken later when focus leaves | high | Linux | fixed |
| [newkit-M2](#newkit-m2) | new-widgets-kit | The SearchField's outer Role::SearchInput node is spoken as an extra unnamed 'entry' every time focus enters the field | medium | Linux | open |

### newkit-01 {#newkit-01}

A banner dismissed through its Collapse stays in the reader's tree and Tab order: focus is left on an invisible button, and restoring it is never announced

- **Example:** new-widgets-kit
- **Scenario:** newkit-banner-dismiss-keys, newkit-banner-dismiss-at
- **Act:** Shift+Tab x3 to the 'Unsaved changes' dismiss button, Space; then Shift+Tab; then AT-SPI click on 'Restore banners'. Separately: AT-SPI click on the 'Disk almost full' dismiss button, then Shift+Tab twice from the search field.
- **The reader should get:** The dismissed banner leaves the tree (as it leaves the screen); focus moves to a control that is still visible and the reader hears it; Shift+Tab never lands inside the dismissed banner; when 'Restore banners' brings a banner back, the reader hears it (it is a polite live region).
- **The reader gets:** Space on the dismiss button puts nothing on the bus except the height tween's object:bounds-changed events (100 of them): no children-changed, no focus change, and Orca says nothing. The \[status bar\] 'Unsaved changes' stays in the tree at zero width, and focus stays on its invisible 'Clear'. Shift+Tab lands on the invisible 'Save now' and Orca says 'Save now push button.'. After the 'Disk almost full' banner is dismissed, Shift+Tab from 'Restore banners' lands on its invisible Clear, and Orca says 'Disk almost full statusbar' 'Clear push button.'. 'Restore banners' emits no announcement, because the banner never left the tree. A sighted keyboard user also tabs onto these invisible buttons.
- **Platform:** Linux AT-SPI/Orca 46.1 (measured). Windows and macOS read the same tree: the nodes are never hidden, and common\_filter drops only hidden nodes (filters.rs:17-34), so the dismissed banner stays on all three platforms (from the source).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/animations/collapse.rs:100-133 (build: nothing makes the child dormant/hidden or moves focus out), 135-166 (layout only), 197-203 (accessibility empty); same wrapper used by the vertical Accordion, accordion.rs:526-536
- **Evidence:**
  - `Space act (banner-dismiss-keys, round 2): 'events: 100 Counter({object:bounds-changed: 100})'. The report shows no other event, then 'FAIL  Orca says something / Orca said nothing'`
  - `FAIL  the tree holds no [status bar] 'Unsaved changes' / found [status bar] 'Unsaved changes'`
  - `focus path: [application] 'new-widgets-kit' ext=None > [frame] '' ext=[0, 0, 720, 720] > [panel] '' ext=[0, 28, 720, 692] > [status bar] 'Unsaved changes' ext=[24, 137, 0, 53] > [push button] 'Clear' ext=[144, 151, 24, 24]`
  - `tree after dismiss: [status bar] 'Unsaved changes' / [label] 'Closing the document now will discard your edits.' / [push button] 'Save now' {focusable} / [push button] 'Clear' desc='Clear' {focusable,focused}`
  - `Shift+Tab after the dismiss: '+33.3 ms object:state-changed:focused 1 [push button] 'Save now''. Orca log: '13:23:13.338102 - SPEECH OUTPUT: 'Save now push button.'' (run 132256-2943801)`
  - `banner-dismiss-at, Shift+Tab again: focus path ... > [status bar] 'Disk almost full' ext=[24, 198, 0, 53] > [push button] 'Clear' ext=[47, 213, 24, 24]. Orca: '13:23:47.330183 - SPEECH OUTPUT: 'Disk almost full statusbar'' and '13:23:47.330199 - SPEECH OUTPUT: 'Clear push button.''`
  - `Activate Restore banners through AT-SPI: 'FAIL  the bus carries an announcement of 'Unsaved changes' / no object:announcement in the act'`
  - `crates/teksilo-widgets/src/animations/collapse.rs:135-166: layout_response only scales the height (width snaps to 0 below progress 0.005). collapse.rs:197-203: accessibility() is empty on purpose. Nothing in build() (100-133) marks the child dormant or hidden, or moves focus out of it`
  - `crates/teksilo-widgets/src/banner.rs:106-108 documents on_dismiss as expecting the host to remove the banner 'typically by toggling a Signal<bool> driving a Switcher'. The example's Collapse (main.rs:86-92) is Collapse's own advertised use: 'animates between hidden and natural size'`
  - `verify-newkit-banner-invisible run 134030-3750307: after Space dismissed 'Unsaved changes', 'Shift+Tab twice, back into the dismissed banner': '+240.3 ms object:state-changed:focused 1 [push button] 'Save now'', Orca '+274.7 ms ORCA SAYS: 'Save now push button.''; then Space on it: app.log line 'Save now clicked'`
  - `run 133231-3321571 'Space on the dismiss button': events Counter({'object:bounds-changed': 268}); tree: status bar 'Unsaved changes' ['enabled','sensitive','showing','visible'] [24,137,0,53]; push button 'Clear' ['enabled','focusable','focused',...] [144,151,24,24]; the next banner 'Disk almost full' moved to [24,145,672,53], overlapping the invisible buttons`
  - `collapse.rs:160-165: width snaps to 0 below progress 0.005 and height = natural*progress; place_children (181-185) still lays the child at full natural size; nothing else reacts to expanded`
  - `accordion.rs:526-536: vertical accordion content goes through Collapse::new(self.expanded).child(region_id), horizontal goes through ctx.visible_when (dormancy). Only the horizontal path hides from AT`
- **Reproduced:** 3 of 3 runs (keys) and 3 of 3 runs (AT-SPI click); deterministic, since no event is emitted at all
- **Verification:** confirmed. Reproduced: 3 of 3 runs (keys), 3 of 3 runs (AT-SPI click); plus 1 extra run showing the invisible 'Save now' still acts. Deterministic: no event but bounds-changed is ever emitted.
- **Fix idea:** When the collapse tween reaches 0, make the child dormant (the visible\_when / set\_dormant path), so it leaves the AT tree, focus traversal and hit-testing. Activate it again before the expand tween starts. If focus is inside the child when it collapses, hand focus to the next focusable node so the reader hears where they are. Caveat: a subtree re-added under the same NodeIds is held defunct by libatspi (the K2 mechanism), so the restored banner's announcement would still be dropped unless the re-shown nodes get fresh ids or K2's fix covers this path.

### newkit-02 {#newkit-02}

SearchField suggestions are silent: the popup's combobox semantics sit on a non-focused outer node, and a highlight change never re-publishes accessibility

- **Example:** new-widgets-kit
- **Scenario:** newkit-search-suggestions, newkit-search-escape
- **Act:** With focus in the search field, type 'ap' (the list opens with Apple, Apricot), then press Down, Down (or type 'b' and press Up).
- **The reader should get:** The focused field says it expanded and controls a list, and each arrow moves the active descendant, so the reader hears 'Apple', then 'Apricot' (or 'Blueberry').
- **The reader gets:** Opening the list is silent: the focused entry has no expanded state, no controller-for relation and no active descendant. Every Down or Up puts zero events on the bus, and Orca says nothing. The field has two nested nodes: an outer, unfocusable, unnamed \[entry\] (Role::SearchInput) that holds controller-for and contains the list box, and the focused inner \[entry\]. Both emit text-changed for every character (duplicate events).
- **Platform:** Linux AT-SPI/Orca (measured). The source says Windows and macOS are the same: every adapter follows accesskit\_consumer's focus, which takes the active descendant of the focused node only (tree.rs:538-542). Upstream on Linux: accesskit\_atspi\_common 0.20 maps no expanded/expandable state at all (no Expanded state anywhere in the crate), so even after a fix Orca will not hear 'expanded'. Windows has an ExpandCollapse pattern (accesskit\_windows node.rs).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/search\_field.rs:772-805 (combobox semantics on the unfocused outer Role::SearchInput node), 842-874 (highlight drives only a repaint-level RectWidget background; the only bind\_to in the file is suggestions at Rebuild, line 848), 1112-1117 (row set\_selected read only when a walk happens)
- **Evidence:**
  - `type 'ap': 'FAIL  the focused node has state 'expanded' / [entry] '' ext=[54, 336, 618, 18] states=['editable', 'enabled', 'focusable', 'focused', 'selectable-text', 'sensitive', 'showing', 'single-line', 'visible'] attrs=None rel=None'`
  - `probe: outer node '[entry] '' ext=[24, 331, 672, 32] states=['editable', 'enabled', 'selectable-text', 'sensitive', 'showing', 'single-line', 'visible'] ... rel={'controller-for': ['/org/a11y/atspi/accessible/0/396140821481099075569433182208']}'. Under it: the focused inner [entry], [status bar] '', [list box] '', [list item] 'Apple' {posinset 1, setsize 2}, [list item] 'Apricot'`
  - `'+0.9 object:children-changed:add [entry] -> [list box]' comes from the outer entry. '+49.5 object:text-changed:insert [entry] 'p'' is followed by '+50.4 object:text-changed:insert [entry] 'p'' from the outer entry (the same character twice)`
  - `Down into the suggestions: 'events: []'. 'FAIL  a object:active-descendant-changed event from [*] '*' / no object:active-descendant-changed event'. 'FAIL  Orca says 'Apple' / Orca unheard: 'Apple''. 'FAIL  the tree holds [list item] 'Apple'' with state selected (run 132745-3071877)`
  - `Up wraps to the last suggestion: 'FAIL  Orca says 'Blueberry' / Orca unheard: 'Blueberry''`
  - `crates/teksilo-widgets/src/search_field.rs:772-805: has_popup, auto_complete, expanded, push_controlled and set_active_descendant are all written on the SearchField's own node, but focus is on the inner TextInputField. The file's own drives_listbox doc (search_field.rs:212-219) says that inner field is 'the only one whose active_descendant assistive technology follows', and TextInputField already has .active_descendant()/.controls() (text_input_field.rs:573,581)`
  - ``search_field.rs:871-874: the highlight only drives a RectWidget background (a repaint). No binding in search_field.rs invalidates accessibility on `highlighted`, so the rows' set_selected (search_field.rs:1112-1117) and the active descendant are never re-published: no event of any kind on Down/Up``
  - `~/.cargo/registry/.../accesskit_consumer-0.39.0/src/tree.rs:541 'focused.active_descendant().unwrap_or(focused)'`
  - `run 133336-3321571 probe: outer '[entry] '' ext=[24,331,672,32] states=[editable,enabled,selectable-text,sensitive,showing,single-line,visible] rel={'controller-for': [...]}'; inner '[entry] '' ext=[54,336,618,18] states=[...,focused,...] rel=None'`
  - `acts 'Down into the suggestions' / 'Down again' / 'Up wraps to the last suggestion': Counter() (no event at all) in runs 133336, 134242, 134707, 133419, 134325, 134751 and verify 134108`
  - `'Enter picks' (run 133336): 'object:text-changed:delete ... 'ap'' then 'insert 'Apricot'' from path ...2944 and again from ...9712 (outer SearchInput)`
- **Reproduced:** 4 of 4 runs of newkit-search-suggestions (Down x2) and 3 of 3 of newkit-search-escape (Up)
- **Verification:** confirmed. Reproduced: 3 of 3 runs of newkit-search-suggestions (Down x2: zero events each), 3 of 3 of newkit-search-escape (Up: zero events), 1 of 1 of verify-newkit-search-space (Down: zero events)
- **Fix idea:** Route the built-in popup through the same path as drives\_listbox: give the inner TextInput the listbox signal (controls) and a Signal&lt;Option&lt;WidgetId&gt;&gt; for the highlighted row (active\_descendant), plus expanded and has\_popup on the inner field. Bind `highlighted` at BindingLevel::AccessibilityOnly (field and rows), so a highlight change reaches the adapter. The outer node could become a GenericContainer so the tree stops showing two nested entries.

### newkit-03 {#newkit-03}

InputDialog's field has no name: the prompt is a separate label, so returning to the field says only 'entry untitled.txt'

- **Example:** new-widgets-kit
- **Scenario:** newkit-dialog-accept, newkit-dialog-escape
- **Act:** Tab x3 to 'Rename…', Space (or AT-SPI click) opens the InputDialog; then Tab, Tab, Tab (Cancel, OK, back to the field).
- **The reader should get:** The field is named by the prompt 'Enter the new file name:', so the reader hears it both when the dialog opens and whenever focus comes back to the field.
- **The reader gets:** On open, Orca reads the prompt only as dialog text: 'Rename document dialog Enter the new file name:', then 'entry untitled.txt selected.'. The field itself is \[entry\] '' with no name, no description and no relation. When Tab wraps back to it, the reader hears only 'entry untitled.txt selected.'. The launch audit already flags unnamed entries.
- **Platform:** Linux AT-SPI/Orca (measured). The name is empty in the AccessKit tree itself, so from the source the UIA and macOS adapters also publish an unnamed edit.
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/input\_dialog.rs:340-342 (prompt is a plain TextWidget), 383-396 (TextInput built with no .label()/labelled\_by)
- **Evidence:**
  - `tree while open: [dialog] 'Rename document' {active,modal} / [label] 'Rename document' / [label] 'Enter the new file name:' / [entry] '' {editable,focusable,focused,selectable-text,single-line} text='untitled.txt' / [status bar] '' / [push button] 'Cancel' / [push button] 'OK'`
  - `FAIL  the focused [entry] has a name / [entry] '' ext=[248, 357, 224, 18] desc=None attrs={} rel=None text={'characters': 12, 'text': 'untitled.txt', 'caret': 12}`
  - `Orca (run 132525-2943801): '13:25:37.617959 - SPEECH OUTPUT: 'Rename document dialog Enter the new file name:'' then '13:25:37.617975 - SPEECH OUTPUT: 'entry untitled.txt selected.''`
  - `Tab wraps to the field: '13:25:50.265227 - SPEECH OUTPUT: 'entry untitled.txt selected.''. 'FAIL  Orca says 'Enter the new file name' / Orca unheard'`
  - `crates/teksilo-widgets/src/input_dialog.rs:340-342 adds the prompt as a plain TextWidget. input_dialog.rs:383-396 builds TextInput::new(..).on_submit_fn(..) with no .label(). TextInput::label puts access_label on the inner field (text_input/widget_impl.rs:111-114)`
  - `run 134832-3927144 'Tab wraps to the field': 'FAIL the focused [entry] has a name / [entry] '' ext=[248,357,224,18] desc=None attrs={} rel=None'; 'FAIL Orca says 'Enter the new file name''`
- **Reproduced:** 3 of 3 runs of each scenario (6 opens, plus 3 AT-SPI opens); deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 runs of newkit-dialog-accept and 3 of 3 of newkit-dialog-escape (9 opens, including 3 through an AT-SPI click); deterministic
- **Fix idea:** When a prompt is set, name the field by it: `input = input.label(prompt.clone())`, or point the field at the prompt's label with labelled\_by, which keeps one string. Otherwise fall back to the dialog title.

### newkit-04 {#newkit-04}

At launch all three banners are announced at once, in random order, each cutting the last, and all are then cut by the window activation

- **Example:** new-widgets-kit
- **Scenario:** every newkit scenario's launch act, and the tabwalk
- **Act:** Launch the example; initial focus goes to the search field.
- **The reader should get:** Live regions that are present when the window opens are not announced as new content (the web/ARIA convention), or they are heard whole after the focus reading. The reader hears the window and the focused field cleanly.
- **The reader gets:** The bus carries three object:announcement events (the banner titles, in a different order every run) about 5-110 ms before the window's activation and focus. Orca speaks each with interrupt=True, so each cuts the one before, and then stops for 'frame.' (K1) and the focused entry. No banner title is ever heard whole. The info banner 'Welcome to Teksilo' has no focusable content, so for a Tab user this launch announcement is the only time it would ever be spoken.
- **Platform:** Linux AT-SPI/Orca (measured). By source, Windows (accesskit\_windows adapter.rs:256-263, UIA LiveRegionChanged) and macOS (accesskit\_macos event.rs:236-241) also announce every named live node that is added. Whether their first client request also meets the placeholder tree was not measured.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-platform/src/window.rs:157-165 (on\_activate answers the placeholder), 738 (window shown before the first frame), 1119-1128 (empty\_initial\_tree)
- **Evidence:**
  - `tabwalk launch: '+448.7 ms object:announcement [status bar] 'Unsaved changes'', '+454.2 ms object:announcement [status bar] 'Disk almost full'', '+455.7 ms object:announcement [status bar] 'Welcome to Teksilo'', then '+468.5 ms window:activate [frame] ''' and '+469.1 ms object:state-changed:focused 1 [entry] '''`
  - `the first update only adds the empty frame: '+282.4 ms object:children-changed:add [application] 'new-widgets-kit' -> [frame] '''. The content arrives later: '+457.1 ms object:children-changed:add [frame] '' -> [tool bar] 'Toolbar'', '+458.9 ms ... -> [panel] '''`
  - `Orca: '12:57:06.702668 - NULL SPEECH: speak 'Unsaved changes' interrupt=True', '12:57:06.716883 - NULL SPEECH: speak 'Disk almost full' interrupt=True', '12:57:06.726664 - NULL SPEECH: speak 'Welcome to Teksilo' interrupt=True', '12:57:06.784490 - NULL SPEECH: stop', '12:57:06.784620 - NULL SPEECH: speak 'frame.' interrupt=False'`
  - `order differs across runs, e.g. ['Disk almost full', 'Unsaved changes', 'Welcome to Teksilo'], ['Welcome to Teksilo', 'Unsaved changes', 'Disk almost full'], ['Unsaved changes', 'Disk almost full', 'Welcome to Teksilo']`
  - `crates/teksilo-platform/src/window.rs:738 shows the window before its first frame. When there is no snapshot yet, on_activate (window.rs:157-165) answers empty_initial_tree() (window.rs:1119-1128), a bare Role::Window. The real tree then comes as an update that adds every node, and accesskit_atspi_common adapter.rs:72-77 announces each added node that has a name and a live politeness`
  - `none of these messages goes through ctx.announce, so the K2 fix does not cover them. The 'frame.' is K1`
  - `run 133231-3321571 launch: '+373.6 ms object:announcement [status bar] 'Disk almost full'', '+379.2 'Welcome to Teksilo'', '+380.0 'Unsaved changes'', '+381.1 children-changed:add [frame] -> [tool bar]', '+395.7 state-changed:focused 1 [entry]'; Orca: '13:32:33.145001 NULL SPEECH: speak 'Disk almost full' interrupt=True', '13:32:33.158925 speak 'Welcome to Teksilo' interrupt=True', '13:32:33.168769 speak 'Unsaved changes' interrupt=True', '13:32:33.224589 NULL SPEECH: stop'`
- **Reproduced:** 19 of 19 launches (every newkit run plus the tabwalk): every banner title Orca began was cut
- **Verification:** confirmed. Reproduced: 27 of 27 launches across my runs (every scenario and the tabwalk): all three banner titles announced and every one cut; the order took all 6 permutations
- **Fix idea:** Answer request\_initial\_tree with the first real tree: lay out and sync once before set\_visible, or hold activation until the first frame. Or leave live politeness off on nodes carried by the first content update after activation, so content present at open is not announced as a change.

### newkit-05 {#newkit-05}

A banner's dismiss button is announced as 'Clear' (IconButton::clear), not as dismissing the banner

- **Example:** new-widgets-kit
- **Scenario:** newkit-banner-dismiss-keys, tabwalk
- **Act:** Tab or Shift+Tab onto the X button of 'Unsaved changes' or 'Disk almost full'.
- **The reader should get:** A name that says what it does: 'Dismiss' or 'Close' (ideally 'Dismiss Unsaved changes').
- **The reader gets:** \[push button\] 'Clear' desc='Clear'. Orca says 'Unsaved changes statusbar' 'Clear push button.'. 'Clear' reads as clearing a field or a value, the window now has two buttons both named 'Clear', and the description repeats the name.
- **Platform:** Linux measured. The name comes from the widget, so the same text reaches every platform (from the source).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/banner.rs:172-182
- **Evidence:**
  - `FAIL  the focused control's name says one of ['dismiss', 'close'] / [push button] 'Clear' ext=[660, 151, 24, 24] desc='Clear'`
  - `Orca (run 132256-2943801): '13:23:04.827559 - SPEECH OUTPUT: 'Unsaved changes statusbar'' and '13:23:04.827591 - SPEECH OUTPUT: 'Clear push button.''`
  - `crates/teksilo-widgets/src/banner.rs:173-181 uses IconButton::clear().embedded(), explaining it as 'adequate for a banner dismiss button without inventing a new i18n key'. icon_button.rs:505-508: clear() gets its tooltip from tr_widget!(a11y_builtin_clear())`
  - `tabwalk 134545-3921245 Tab 8: 'ORCA SAYS: 'Clear push button.'', Tab 9: 'Disk almost full statusbar' 'Clear push button.'`
- **Reproduced:** 3 of 3 runs, plus the tabwalk; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 runs of newkit-banner-dismiss-keys plus my tabwalk; deterministic
- **Fix idea:** Give the dismiss button its own translated label (a 'Dismiss' / 'Close' built-in string), preferably with the banner title in it ('Dismiss Unsaved changes'), and do not also publish the tooltip as a description.

### newkit-06 {#newkit-06}

An empty text field publishes no Text or EditableText interface: typing its first character and deleting its last emit no text-changed event

- **Example:** new-widgets-kit
- **Scenario:** newkit-search-escape (and every TextInputField: search, file entry, dialog field)
- **Act:** In the search field: Ctrl+A, BackSpace (the field goes from 'b' to empty), then type 'c', then 'h'.
- **The reader should get:** object:text-changed:delete for the 'b' and object:text-changed:insert for the 'c', as for any other edit, and an entry that always offers Text/EditableText.
- **The reader gets:** Emptying the field gives only text-selection-changed. The first character gives only text-caret-moved. Only the second character ('h') produces text-changed:insert. Orca's own log shows the empty focused entry with interfaces='Component', and the same entry with 'Component, EditableText, Text' once it holds text. Orca speaks deletion (BackSpace) and pasted or inserted text from these events, and it cannot read the text or caret of an interface-less entry. The harness cannot show Orca's key-driven echo (it never sees the keys), so the audible effect is inferred from the missing events.
- **Platform:** Linux AT-SPI (measured). accesskit\_consumer gates text ranges on having a TextRun child on every platform, but I did not verify the Windows/macOS text-change paths.
- **Severity:** medium; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-core/src/accessibility/text\_runs.rs:228-290 (TextRunSource::from\_geometry builds zero lines from a geometry with no lines, and adds the residual line only when covered &lt; text.len()), with the empty-text geometry coming from text-typeset 1.12.0 document\_flow.rs:1260-1262 and teksilo-text typesetter\_bridge.rs:697; the field retains it at crates/teksilo-widgets/src/primitives/text\_input\_field.rs:740-746 and uses it at primitives/text\_input\_field/widget\_impl.rs:1112-1136
- **Evidence:**
  - `Ctrl+A, BackSpace: '+30.6 ms object:text-selection-changed [entry] ''', then only label changes and list box churn. 'FAIL  a object:text-changed:delete event from [entry] '*''`
  - `type 'c' into the empty field: '+23.0 ms object:text-caret-moved [entry] '''. 'FAIL  a object:text-changed:insert event from [entry] '*''`
  - `type 'h' after it: '+19.3 ms object:text-changed:insert [entry] '' text='h''. The check passes`
  - `Orca (run 132443-2943801): '13:24:45.728513 - OBJECT EVENT: object:state-changed:focused for [entry] ...' with 'interfaces='Component'' and 'attributes='placeholder-text:Type a fruit — Apple, Banana, …''. Later, line 2066: 'interfaces='Component, EditableText, Text''`
  - `accesskit_consumer-0.39.0/src/text.rs:1402-1406: supports_text_ranges needs a TextRun child. accesskit_atspi_common-0.20.0/src/adapter.rs:121-124: no text-change event unless both the old and new node support text ranges`
  - `crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs:1099-1107 says runs are 'emitted even for an empty field so supports_text_ranges() is already true before the first keystroke', and teksilo-core's text_runs.rs:610-630 does emit an EmptyRun for an empty line. The measured tree contradicts this for the empty field. I could not verify which source branch (the retained-geometry path at widget_impl.rs:1115-1124 is the likely one) drops it`
  - `verify-newkit-empty-edits 133929 & 135151: file entry 'type 'a' into the empty file entry' → only 'object:text-caret-moved [entry]'; 'BackSpace deletes 'a' and empties the field' → no entry event (only the example's label changes); control acts 'type b' / 'BackSpace deletes b' → text-changed insert/delete present`
  - `same runs, dialog: 'Ctrl+A, BackSpace empties the dialog field' → no event at all; 'FAIL Orca says 'Selection deleted''; 'type 'x' into the emptied dialog field' → '+12.9 ms object:text-caret-moved [entry]', '+24.0 ms ORCA SAYS: 'Text unselected.''; Orca log: 'SCRIPT UTILITIES: Cached selection for [entry] is 'untitled.txt' (0, 12)' then 'New selection for [entry] is '' (0, 0)'`
  - `run 133419 Orca log: at focus (cache cleared 13:34:21.515) the empty entry has interfaces='Component'; after its cache is cleared at 13:34:47.81 with text present, interfaces='Component, EditableText, Text'`
- **Reproduced:** 3 of 3 runs; deterministic
- **Verification:** corrected by the verifier. Reproduced: search field 3 of 3 (newkit-search-escape); FilePickerField entry 2 of 2 and InputDialog field 2 of 2 (verify-newkit-empty-edits); deterministic Real, and it affects every TextInputField, not just the search field. I pin the source branch the sweep could not: for "" text-typeset returns LayoutGeometry::default() with no lines (document\_flow.rs:1260-1262, 'if text.is\_empty() { return (empty, no\_geometry) }'). teksilo-text wraps that as Some(...) (typesetter\_bridge.rs:697). The field retains it with placed.text == "", so widget\_impl.rs:1115-1124 takes from\_geometry, which yields zero SourceLines. push\_text\_runs emits its EmptyRun per line (text\_runs.rs:610-630), so an empty field gets no run at all. supports\_text\_ranges is then false, and atspi\_common emits no text change (adapter.rs:121-124) and no selection or caret event (adapter.rs:197-199). What the reader loses is now measured, not inferred. Ctrl+A, BackSpace on the dialog field put zero events on the bus and Orca said nothing, where the same field with a non-empty result makes Orca say 'Selection deleted.' (newkit-dialog-accept, 3/3). Typing 'x' into the emptied field then made Orca say the wrong 'Text unselected.' (2/2): its cached selection was still 'untitled.txt' (0,12), because no event had told it otherwise. Deleting the last character with BackSpace emits no delete either, and Orca speaks BackSpace deletions from that event (default.py:1678-1722). Windows gates UIA TextChanged the same way (accesskit\_windows adapter.rs:54-56), from the source. Medium stands.
- **Fix idea:** Make sure an empty field always publishes one empty Role::TextRun, including when the retained measurement of "" has no lines (fall back to TextRunSource::flat with a caret-sized fallback rect). Add a test through accesskit\_consumer asserting supports\_text\_ranges() on an empty TextInput.

### newkit-07 {#newkit-07}

The SearchField and the FilePickerField entry have no accessible name: once filled, the reader hears only 'entry Apricot'

- **Example:** new-widgets-kit
- **Scenario:** newkit-search-suggestions, tabwalk, launch audit
- **Act:** Tab onto the search field and the file entry, empty and after the search field holds 'Apricot'.
- **The reader should get:** Each field is named ('Search fruits', 'File').
- **The reader gets:** Both entries are unnamed; the launch audit reports 'unnamed-control' twice. While empty, Orca reads the placeholder ('entry Type a fruit — Apple, Banana, …', 'entry No file selected.'). Once filled, only 'entry Apricot.' is heard. The only visible text nearby is the group header 'SearchField & FilePickerField', which is not associated with either field.
- **Platform:** Linux measured. The name is empty in the tree, so it is empty on every platform (from the source).
- **Severity:** high; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/new\_widgets\_kit/src/main.rs:190-208 and 221-226 (no .label()); APIs exist at crates/teksilo-widgets/src/search\_field.rs:206-210 and file\_picker\_field.rs:172 (forwarded at 322-324)
- **Evidence:**
  - `tree audit: 'unnamed-control: [entry] '': a focusable entry with no name' (x2)`
  - `FAIL  the focused [entry] has a name / [entry] '' ext=[54, 336, 618, 18] desc=None attrs={} rel=None`
  - `Orca (run 132400-2943801): '13:24:30.405242 - SPEECH OUTPUT: 'entry Apricot.''. Tabwalk: 'ORCA SAYS: 'entry No file selected.''`
  - `examples/new_widgets_kit/src/main.rs:190-208 (SearchField) and 221-226 (FilePickerField) call neither widget's .label(). Both APIs exist: search_field.rs:206-210 and FilePickerField::label (file_picker_field.rs:172)`
  - `run 134707-3927144 'Shift+Tab away and Tab back to the field': 'FAIL the focused [entry] has a name / [entry] '' ... desc=None attrs={} rel=None'; Orca 'entry' 'entry Apricot.'`
- **Reproduced:** 3 of 3 runs, plus the tabwalk; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 runs plus my tabwalk and all 27 launch audits (unnamed-control x2); deterministic
- **Fix idea:** Example: add .label(lit!("Search fruits")) and .label(lit!("File")). Framework, optionally: when no label is set, SearchField could name its field with the built-in 'Search' string it already uses as a fallback placeholder (search\_field.rs:350-353).

### newkit-08 {#newkit-08}

The text field's visible clear (X) button has no accessibility node and cannot be focused

- **Example:** new-widgets-kit
- **Scenario:** newkit-search-escape
- **Act:** Type 'b' in the search field (the X appears), Escape to close the list, then look for the clear control.
- **The reader should get:** The X is a named button a reader can find and activate (or it is advertised as an action on the field).
- **The reader gets:** The field's subtree holds only the two entries and an empty status bar, with no button. The X is pointer-only. Clearing still works from the keyboard (Ctrl+A, BackSpace), and Escape does not clear.
- **Platform:** Linux measured; the node is absent from the tree on every platform (from the source).
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/text\_input/widget\_impl.rs:205-243 (HitTarget clear affordance); crates/teksilo-widgets/src/button.rs HitTarget has no accessibility() and no focusable
- **Evidence:**
  - `FAIL  the SearchField's clear button is in the tree, inside the field / [entry] '' ext=[24, 331, 672, 32] / [entry] '' ext=[54, 336, 618, 18] / [status bar] '' ext=[24, 363, 0, 0]`
  - `crates/teksilo-widgets/src/text_input/widget_impl.rs:205-243: the clear affordance is a HitTarget with only .on_tap(...). HitTarget (crates/teksilo-widgets/src/button.rs:128-230) implements no accessibility() and is not focusable`
- **Reproduced:** 3 of 3 runs; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 runs; deterministic
- **Fix idea:** Publish the clear affordance as a named Role::Button (the 'Clear' built-in string) that is hidden while the field is empty, or add a named custom action on the field, and consider Escape-to-clear on an empty popup for SearchField.

### newkit-09 {#newkit-09}

Every TextInput carries an empty, unnamed 'status bar' (its ValidationStrip), including inside the dialog

- **Example:** new-widgets-kit
- **Scenario:** tree at launch, newkit-dialog-accept
- **Act:** Walk the tree (Orca flat review / object navigation).
- **The reader should get:** No node while there is no validation message (or a node without a status role).
- **The reader gets:** The tree has an unnamed \[status bar\] '' under the search field, after the file entry, and inside the Rename dialog between the entry and Cancel. A reader who explores meets empty status bars.
- **Platform:** Linux measured.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/primitives/validation\_strip.rs:147-163
- **Evidence:**
  - `tree: '[entry] '' ... / [status bar] ''' (search), '[push button] 'Browse' ... / [status bar] ''' (file picker), '[entry] '' ... text='untitled.txt' / [status bar] '' / [push button] 'Cancel'' (dialog)`
  - `crates/teksilo-widgets/src/primitives/validation_strip.rs:148-163 always sets Role::Status, even when the feedback is Pristine`
- **Reproduced:** every run; deterministic
- **Verification:** confirmed. Reproduced: every run (search field, file picker, dialog field); deterministic
- **Fix idea:** While pristine, publish the strip as GenericContainer (pruned) or hidden, and keep Role::Status plus live politeness for when a message exists. It must keep a stable identity if it is to announce later.

### newkit-10 {#newkit-10}

The example's status lines ('Submitted 1 time(s).', 'Last result: …', 'Filtering: …') never reach the reader; empty readout labels are exposed

- **Example:** new-widgets-kit
- **Scenario:** newkit-search-suggestions, newkit-dialog-accept/escape
- **Act:** Press Enter in the search field to submit, or accept or cancel the InputDialog.
- **The reader should get:** A status message is announced (WCAG 4.1.3). The submit confirmation and the dialog result should be heard.
- **The reader gets:** The labels change (object:property-change:accessible-name) but they are not live regions, and Orca says nothing. Before anything happens, two empty \[label\] '' nodes sit in the tree.
- **Platform:** Linux measured.
- **Severity:** low; **layer:** example
- **Status:** Open, in the example's own code.
- **Where:** examples/new\_widgets\_kit/src/main.rs:136-145 and 250-256
- **Evidence:**
  - `Enter again submits the query: '+12.6 ms object:property-change:accessible-name [label] 'Submitted 1 time(s).' text='Submitted 1 time(s).''. 'FAIL  Orca says 'Submitted' / Orca unheard: 'Submitted''`
  - `Escape cancels: '+13.4 ms object:property-change:accessible-name [label] 'Last result: cancelled — name unchanged.'' is followed only by 'ORCA SAYS: 'Rename… push button.''`
  - `tree at launch: '[label] ''' after the search readout and after 'Current name: untitled.txt'`
  - `examples/new_widgets_kit/src/main.rs:136-145 (submit_readout) and 250-256 (action_readout): plain TextWidgets`
- **Reproduced:** 3 of 3 runs; deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 runs for 'Submitted', 3 of 3 for the dialog result labels; deterministic
- **Fix idea:** Mark submit\_readout and action\_readout .access\_live(Live::Polite), or call ctx.announce from the handlers (which the K2 fix would then cover). Hide the labels while they are empty.

### newkit-11 {#newkit-11}

A banner's body text is never spoken when a reader tabs into the banner's controls

- **Example:** new-widgets-kit
- **Scenario:** tabwalk
- **Act:** Tab onto 'Save now' (Unsaved changes) and onto the 'Disk almost full' X.
- **The reader should get:** The reader learns what the banner says: 'Closing the document now will discard your edits.' and 'Less than 200 MB remaining on /Users/you.'
- **The reader gets:** Orca gives only the banner's name as context: 'Unsaved changes statusbar' 'Save now push button.' and 'Disk almost full statusbar' 'Clear push button.'. The launch announcement carries only the title, and it is cut (newkit-04). The description is reachable only by exploring (flat review).
- **Platform:** Linux measured.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/banner.rs:256-265
- **Evidence:**
  - `tabwalk Tab 7: 'ORCA SAYS: 'Unsaved changes statusbar'', 'ORCA SAYS: 'Save now push button.''. Tab 9: 'ORCA SAYS: 'Disk almost full statusbar'', 'ORCA SAYS: 'Clear push button.''`
  - `crates/teksilo-widgets/src/banner.rs:262-264: 'Use the title alone as the AT name; the description is read by descending into the body text widget'`
- **Reproduced:** 1 tabwalk plus 3 runs of newkit-banner-dismiss-keys (same context speech); deterministic
- **Verification:** confirmed. Reproduced: my tabwalk and 3 of 3 banner-dismiss-keys runs; deterministic
- **Fix idea:** Tie the controls to the body text, e.g. give the action and dismiss buttons described\_by pointing at the description label, which Teksilo already writes into the description (heard on focus). Or set the banner node's description to the body text.

### newkit-12 {#newkit-12}

CommandLinkButton folds its description into its name, frozen at build

- **Example:** new-widgets-kit
- **Scenario:** tabwalk
- **Act:** Tab onto the two command links.
- **The reader should get:** Name 'Create new project', description 'Start with a blank workspace.' (locale-reactive).
- **The reader gets:** The name is 'Create new project — Start with a blank workspace.' and Orca reads the whole string as the name. The string is built with resolve\_now(), so it does not follow a locale change, and a voice-control user must say the whole sentence.
- **Platform:** Linux measured; the name is the same on every platform (from the source).
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/command\_link\_button.rs:427-436
- **Evidence:**
  - `tabwalk Tab 4: 'ORCA SAYS: 'Create new project — Start with a blank workspace. push button.''`
  - `crates/teksilo-widgets/src/command_link_button.rs:427-436: format!("{} — {}", self.title.resolve_now(), desc.resolve_now()) in set_name`
  - `tabwalk 134545-3921245 Tab 4: 'ORCA SAYS: 'Create new project — Start with a blank workspace. push button.''`
- **Reproduced:** tabwalk; deterministic
- **Verification:** corrected by the verifier. Reproduced: my tabwalk (Tab 4 and 5); deterministic The name folding is real: 'Create new project — Start with a blank workspace. push button.' The 'frozen at build, does not follow a locale change' part is wrong. The format!/resolve\_now() sits in fn accessibility (command\_link\_button.rs:427), not in build(), and runs on every AT walk, and sync\_accessibility forces a re-walk on every locale switch (widget\_tree/accessibility\_impl.rs:59-63). The name therefore re-resolves in the new locale, as Banner's and AccordionRegion's do. What remains is low: the description is folded into the name instead of set as a description, and voice control gets a long name.
- **Fix idea:** set\_name(title) and set\_description(description), both as LocalizedString or Prop, so they stay locale-reactive.

### newkit-M1 {#newkit-m1}

Caret moves and selection changes in any TextInputField are never published: arrow keys, Home/End, Shift+Home/End and Ctrl+A put nothing on the bus, and the stale selection is spoken later when focus leaves

- **Example:** new-widgets-kit
- **Act:** verify-newkit-caret: Tab to the file entry, type 'abc', then Left, Left, Home, Shift+End, Right, Ctrl+A. verify-newkit-caret-dialog: in the search field type 'xy', Left, Shift+Home, then Tab away; open the InputDialog, End, Left, Shift+Home.
- **The reader should get:** Each caret move emits object:text-caret-moved and each selection change emits object:text-selection-changed, so Orca echoes the character, word or selection, and its caret-offset queries are current.
- **The reader gets:** No event of any kind for any of these acts, in all three fields (FilePickerField entry, SearchField, InputDialog field). The AT tree keeps the old caret and selection, so Orca speaks nothing ('selected', 'Text unselected' and character echo are all missing), and its own caret-offset reads are stale. The pending selection is only published when something else dirties the tree. When Tab left the search field, the bus carried text-selection-changed plus caret-moved for the field being left, and Orca said 'x' 'selected' at that moment, cut at once by the new focus.
- **Platform:** Linux AT-SPI/Orca measured. The AccessKit tree itself is not updated, so no adapter has anything to report: no UIA TextSelectionChanged on Windows and no AXSelectedTextChanged on macOS (from the source; not measured).
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `98211359` (text-caret).
- **Where:** crates/teksilo-widgets/src/primitives/text\_input\_field/widget\_impl.rs:209-255 and 452-481: the only AccessibilityOnly bindings are text, feedback, active\_descendant/controls and revealed. The cursor\_position signal (primitives/text\_input\_field/state.rs:67, set at state.rs:606-607) is not bound. crates/teksilo-core/src/widget\_tree/accessibility\_impl.rs:20-30 and 74: sync\_accessibility re-walks only on focus, overlay, rebuild, activation, an AccessibilityOnly flip, a shortcut rebind, a locale switch, an announcement or an explicit request, so a caret-only change is never sent. keyboard.rs:130-132 (Ctrl+A) only changes the cursor.
- **Evidence:**
  - `verify-newkit-caret 134332-3872224 and 135009-3927144: 'Left', 'Left again', 'Home' → 'FAIL a object:text-caret-moved event from [entry]'; 'Shift+End', 'Right', 'Ctrl+A' → 'FAIL a object:text-selection-changed event from [entry]' (no event of any type in these acts)`
  - `verify-newkit-caret-dialog 134448-3905984 and 135057-3927144: search field 'Left' and 'Shift+Home' → no event; dialog 'End' (collapse the all-selected text), 'Left', 'Shift+Home' → no event, 'FAIL Orca says 'selected''`
  - `same runs, 'Tab three times to Rename…': '+7.4 ms object:text-selection-changed [entry] ''', '+7.6 ms object:text-caret-moved [entry] ''', '+7.6 ms object:state-changed:focused 1 [entry] ''' (the file entry) then Orca '13:45:08.372163 SPEECH OUTPUT: 'x'', '13:45:08.372242 SPEECH OUTPUT: 'selected'' both marked CUT: the Shift+Home selection from the previous act, published only on the focus change`
  - `the spinbox sweep (target/reader-sweep/spinbox/spinbox-caret-*) measured the same on a SpinBox built on the same TextInputField: Home/Right/End/Left 'no object:text-caret-moved event from [spin button]'`
- **Reproduced:** file entry 2 of 2 runs, search field 2 of 2, dialog field 2 of 2; deterministic (no event at all). The late publication on focus change was seen 2 of 2.
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Bind the field's cursor\_position and selection anchor, or a caret/selection version signal, at BindingLevel::AccessibilityOnly in TextInputField::build, as text\_signal is, so that every cursor or selection change re-walks the field's node. Add a headless test that a Left arrow changes the TreeUpdate's text\_selection.

### newkit-M2 {#newkit-m2}

The SearchField's outer Role::SearchInput node is spoken as an extra unnamed 'entry' every time focus enters the field

- **Example:** new-widgets-kit
- **Act:** Launch (initial focus on the search field), or Tab/Shift+Tab back onto the search field.
- **The reader should get:** One entry is read: 'Search fruits entry …' (or at least a single 'entry …').
- **The reader gets:** Orca says 'entry' and then 'entry Type a fruit — Apple, Banana, …' (or 'entry' 'entry Apricot.'). Orca presents the outer, unfocused, unnamed \[entry\] as a new ancestor of the focused inner \[entry\]. The reader hears two text fields where there is one.
- **Platform:** Linux AT-SPI/Orca measured. On Windows the outer node is an Edit control in the UIA tree too (from the source; not measured whether NVDA voices it).
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/search\_field.rs:774 (builder.set\_role(Role::SearchInput) on the composite's own node, while the focused node is the inner TextInputField; the TextInput composite uses GenericContainer for exactly this reason, text\_input/widget\_impl.rs:103-110)
- **Evidence:**
  - `run 133336-3321571 'Shift+Tab away and Tab back to the field': Orca log 'GENERATION TIME: 0.0123 ----> newAncestors=[entry]', then '13:34:06.404205 - SPEECH OUTPUT: 'entry'' and '13:34:06.404229 - SPEECH OUTPUT: 'entry Apricot.''`
  - `every launch act: '+610.6 ms ORCA SAYS: 'entry'', '+610.6 ms ORCA SAYS: 'entry Type a fruit — Apple, Banana, …'' (run 133231-3321571); tabwalk 134545-3921245 Tab 11: 'entry' then 'entry Type a fruit — Apple, Banana, …'`
  - `tree: '[entry] '' {editable,selectable-text,single-line}' (outer, not focusable) > '[entry] '' {editable,focusable,…}' (inner) > '[status bar] ''', plus the list box when open`
- **Reproduced:** 27 of 27 launches, plus 3 of 3 returns to the field in newkit-search-suggestions and the tabwalk; deterministic
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Make the outer node a GenericContainer (pruned) and move has\_popup, expanded, controls and active\_descendant onto the inner field. This is the same change newkit-02 needs.
